//! Building the schedule: the tours, their vehicles and the cars they use.

use super::*;

impl Schedule {
    /// `clock` gives the date: tours carry a validity mask (bits 0-6 Monday…Sunday, 7 public
    /// holiday, 8 school holidays, 9 school days) that selects which run today.
    pub fn new(root: &Path, world: &World, clock: &::simulation::SimClock) -> Schedule {
        let chrono_dirs = world.chrono_dirs.read().clone();
        let deactivated = ::map::chrono_deactivated_lines(&chrono_dirs);
        let data = TimetableData::load_with_chrono(&world.map_dir, &chrono_dirs, &deactivated);
        for e in &data.errors {
            log::warn!("timetable: {e}");
        }
        let calendar =
            ::map::Calendar::load(&world.map_dir.join("Holidays.txt")).unwrap_or_default();
        let car_use = ::timetable::CarUse::load_dir(&world.map_dir);
        // station link lengths, the first link of a pair counts (as the routes take it)
        let mut links: HashMap<(i64, i64), f64> = HashMap::new();
        for l in &data.stn_links {
            links.entry((l.from_id, l.to_id)).or_insert(l.length);
        }
        let link_length = |a: i64, b: i64| links.get(&(a, b)).copied();
        let times: Vec<Vec<TripTimes>> = data
            .trips
            .iter()
            .map(|t| {
                let stations = trip_stations(t);
                if t.profiles.is_empty() {
                    vec![TripTimes::new(&stations, None, &link_length)]
                } else {
                    t.profiles
                        .iter()
                        .map(|p| TripTimes::new(&stations, Some(p), &link_length))
                        .collect()
                }
            })
            .collect();
        let mut visits: HashMap<i64, Vec<(usize, usize)>> = HashMap::new();
        for (ti, t) in data.trips.iter().enumerate() {
            for (k, sid) in trip_stations(t).iter().enumerate() {
                visits.entry(*sid).or_default().push((ti, k));
            }
        }
        let date = clock.date_code();
        let weekday = clock.weekday();
        let holiday = calendar.is_holiday(date);
        let school_holiday = calendar.in_holiday_range(date);
        let (day_bit, school_bit) = day_bits(&calendar, clock);
        let mut departures = Vec::new();
        let mut skipped_tours = 0;
        for line in &data.lines {
            for tour in &line.tours {
                let mask = tour.extra.trim().parse::<i32>().unwrap_or(1023);
                if mask & day_bit == 0 || mask & school_bit == 0 {
                    skipped_tours += 1;
                }
                for t in &tour.trips {
                    if let Some(ti) = data
                        .trips
                        .iter()
                        .position(|x| x.name.eq_ignore_ascii_case(&t.trip))
                    {
                        let profile = usize::try_from(t.profile)
                            .unwrap_or(0)
                            .min(times[ti].len() - 1);
                        departures.push(Departure {
                            time: t.departure as f64 * 60.0,
                            trip: ti,
                            profile,
                            line: line.name.clone(),
                            ai_group: tour.ai_group.clone(),
                            tour: tour.number.clone(),
                            mask,
                            spawned: false,
                        });
                    }
                }
            }
        }
        departures.sort_by(|a, b| a.time.total_cmp(&b.time));
        let mut trip_departures = vec![Vec::new(); data.trips.len()];
        for (i, d) in departures.iter().enumerate() {
            trip_departures[d.trip].push(i);
        }
        // the previous departure of each tour, looked up once: the layover check used to
        // walk all 3600 Spandau departures for each of them, every two seconds - 1.5 fps
        let mut tour_prev = vec![None; departures.len()];
        let mut last_of: HashMap<(String, String), usize> = HashMap::new();
        for (i, d) in departures.iter().enumerate() {
            let key = (d.line.clone(), d.tour.clone());
            tour_prev[i] = last_of.get(&key).copied();
            last_of.insert(key, i);
        }
        let mut tour_next = vec![None; departures.len()];
        for (i, p) in tour_prev.iter().enumerate() {
            if let Some(p) = p {
                tour_next[*p] = Some(i);
            }
        }
        let mut depots = HashMap::new();
        let mut warned: HashSet<String> = HashSet::new();
        let mut hof_cache: HashMap<(std::path::PathBuf, String), Option<Arc<::legacy_vehicle::Hof>>> =
            HashMap::new();
        // a bus file several depots (or typgroups) list is one type: loaded once
        let mut loaded: HashMap<std::path::PathBuf, Option<Arc<VehicleType>>> = HashMap::new();
        let mut load = |path: &std::path::Path| -> Result<Arc<VehicleType>, String> {
            loaded
                .entry(path.to_path_buf())
                .or_insert_with(|| {
                    VehicleType::load_ai(root, path)
                        .map(Arc::new)
                        .map_err(|e| log::debug!("{}: {e}", path.display()))
                        .ok()
                })
                .clone()
                .ok_or_else(|| "could not be loaded".to_string())
        };
        {
            let lists = &world.ailists;
            for g in lists.groups.iter().filter(|g| g.is_depot) {
                let mut vehicles = Vec::new();
                for tg in &g.typgroups {
                    let path = ::legacy_config::resolve_path(root, &tg.file);
                    match load(&path) {
                        Ok(t) => {
                            let hof = g
                                .hof
                                .as_ref()
                                .and_then(|h| depot_file(&mut hof_cache, t.def.dir(), h));
                            vehicles.push((t, tg.entries.clone(), hof));
                        }
                        Err(e) => {
                            if !warned.contains(&tg.file) {
                                log::warn!("depot vehicle {}: {e}", tg.file);
                                warned.insert(tg.file.clone());
                            }
                        }
                    }
                }
                depots.insert(g.name.to_ascii_lowercase(), vehicles);
            }
        }
        // train groups: [aigroup_2] entries pointing at .zug files
        let mut trains: HashMap<String, Vec<Vec<(Arc<VehicleType>, bool)>>> = HashMap::new();
        {
            let lists = &world.ailists;
            for g in lists.groups.iter().filter(|g| !g.is_depot) {
                for v in &g.vehicles {
                    if !v.file.to_ascii_lowercase().ends_with(".zug") {
                        continue;
                    }
                    let path = ::legacy_config::resolve_path(root, &v.file);
                    let Ok(train) = ::legacy_vehicle::vehicle::Train::load(&path) else {
                        continue;
                    };
                    let mut cars = Vec::new();
                    let mut pick = 0usize;
                    for (file, rev) in &train.cars {
                        // an entry is a vehicle file, or the name of a vehicle pool group
                        let p = ::legacy_config::resolve_path(root, file);
                        let path = if ::legacy_config::vfs::is_file(&p) {
                            Some(p)
                        } else {
                            lists
                                .groups
                                .iter()
                                .find(|g| {
                                    g.name.eq_ignore_ascii_case(file.trim())
                                        && !g.vehicles.is_empty()
                                })
                                .map(|g| {
                                    pick += 1;
                                    ::legacy_config::resolve_path(
                                        root,
                                        &g.vehicles[pick % g.vehicles.len()].file,
                                    )
                                })
                        };
                        let Some(path) = path else {
                            log::warn!("train car {file}: no such file or group");
                            continue;
                        };
                        match load(&path) {
                            Ok(t) => cars.push((t, *rev)),
                            Err(e) => log::warn!("train car {}: {e}", file),
                        }
                    }
                    if !cars.is_empty() {
                        trains
                            .entry(g.name.to_ascii_lowercase())
                            .or_default()
                            .push(cars);
                    }
                }
            }
        }
        let tile_coords = world.global.raw_tiles.clone();
        if ::legacy_config::env::var_os("OMSI_PROFILE").is_some() {
            let mut seen: HashSet<*const VehicleType> = HashSet::new();
            let mut bytes = 0usize;
            for t in depots
                .values()
                .flatten()
                .map(|v| &v.0)
                .chain(trains.values().flatten().flatten().map(|c| &c.0))
            {
                if seen.insert(Arc::as_ptr(t)) {
                    bytes += t.mesh_bytes();
                }
            }
            log::info!(
                "timetable: {} vehicle types loaded, {:.0} MB of meshes on the CPU",
                seen.len(),
                bytes as f64 / 1e6
            );
        }
        let today = departures
            .iter()
            .filter(|d| d.mask & day_bit != 0 && d.mask & school_bit != 0)
            .count();
        log::info!(
            "timetable: {} lines, {} trips, {} tracks, {} departures today (weekday {weekday}, holiday {holiday}, school holidays {school_holiday}; {skipped_tours} tours not today), {} depot groups",
            data.lines.len(),
            data.trips.len(),
            data.tracks.len(),
            today,
            depots.len()
        );
        let served = data
            .trips
            .iter()
            .flat_map(|t| trip_stations(t).into_iter().skip(1))
            .collect();
        let mut s = Schedule {
            data,
            departures,
            depots,
            tile_coords,
            next_number: 0,
            trains,
            pools: HashMap::new(),
            pending: Default::default(),
            tour_prev,
            waiting: Vec::new(),
            running: Vec::new(),
            seen_generation: 0,
            last_retry: f64::NEG_INFINITY,
            car_departure: HashMap::new(),
            retry_at: HashMap::new(),
            fleet_reading: HashMap::new(),
            fleet_ready: Default::default(),
            fleet_check: f64::NEG_INFINITY,
            times,
            visits,
            trip_departures,
            boards_made: f64::NEG_INFINITY,
            player_tour: None,
            player_departure: None,
            purge_player_tour: false,
            lan_tours: HashSet::new(),
            last_tod: 0.0,
            deactivated,
            served,
            shared_stand: HashMap::new(),
            startup: Default::default(),
            later_layover: Default::default(),
            tour_next,
            awaiting: Default::default(),
            calendar,
            day: date,
            day_bits: (day_bit, school_bit),
            next_day_bit: 1 << ((clock.weekday() + 1) % 7),
            day_base: 0.0,
            date_clock: clock.clone(),
            car_use,
            tour_vehicle: HashMap::new(),
            used_numbers: HashSet::new(),
        };
        s.assign_car_use();
        s
    }

    /// The day's vehicles of the tours the map's `car_use` names (the original, run
    /// at load and when the date changes): for every record in force whose line runs,
    /// first its `[number_tour]` pairs (a fleet number for a tour), then for each other
    /// tour of the line, with the probability `[types_prefered]` gives (1 for
    /// `[onlytypes]`), a fleet number of one of the listed types from the tour's depot
    /// group that no tour has yet. Other tours draw theirs when they are first due.
    pub(super) fn assign_car_use(&mut self) {
        self.tour_vehicle.clear();
        self.used_numbers.clear();
        let date = self.day;
        let mut n_fixed = 0;
        let mut n_typed = 0;
        for cu in self.car_use.iter().filter(|c| c.valid_on(date)) {
            let Some(line) = self
                .data
                .lines
                .iter()
                .find(|l| l.name.trim().eq_ignore_ascii_case(cu.line.trim()))
            else {
                continue;
            };
            // [number_tour]: this tour runs with this vehicle
            for (number, tour) in &cu.number_tour {
                let Some(t) = line
                    .tours
                    .iter()
                    .find(|t| t.number.trim().eq_ignore_ascii_case(tour.trim()))
                else {
                    continue;
                };
                let group = t.ai_group.to_ascii_lowercase();
                let key = tour_key_of(&group, &line.name, &t.number);
                if self.tour_vehicle.contains_key(&key)
                    || self
                    .used_numbers
                    .contains(&(group.clone(), number.trim().to_string()))
                {
                    continue;
                }
                let found = self.depots.get(&group).and_then(|v| {
                    v.iter().enumerate().find_map(|(k, (_, nums, _))| {
                        nums.iter()
                            .position(|e| e.number.trim() == number.trim())
                            .map(|j| (k, j))
                    })
                });
                if let Some(kj) = found {
                    self.tour_vehicle.insert(key, kj);
                    self.used_numbers.insert((group, number.trim().to_string()));
                    n_fixed += 1;
                }
            }
            // [onlytypes] / [types_prefered]: the other tours from those types
            let Some((factor, types)) = cu.types() else {
                continue;
            };
            let types: Vec<String> = types.iter().map(|t| norm_vehicle_path(t)).collect();
            for t in &line.tours {
                let group = t.ai_group.to_ascii_lowercase();
                let key = tour_key_of(&group, &line.name, &t.number);
                if self.tour_vehicle.contains_key(&key) {
                    continue;
                }
                let h = mix(key ^ date as u64);
                if (h >> 11) as f64 / (1u64 << 53) as f64 > factor as f64 {
                    continue;
                }
                let Some(vehicles) = self.depots.get(&group) else {
                    continue;
                };
                let candidates: Vec<(usize, usize)> = vehicles
                    .iter()
                    .enumerate()
                    .filter(|(_, (ty, _, _))| {
                        let p = norm_vehicle_path(&ty.def.path.to_string_lossy());
                        types.iter().any(|w| p.ends_with(w.as_str()))
                    })
                    .flat_map(|(k, (_, nums, _))| (0..nums.len()).map(move |j| (k, j)))
                    .filter(|(k, j)| {
                        !self.used_numbers.contains(&(
                            group.clone(),
                            vehicles[*k].1[*j].number.trim().to_string(),
                        ))
                    })
                    .collect();
                if candidates.is_empty() {
                    continue;
                }
                let (k, j) = candidates[(mix(h) % candidates.len() as u64) as usize];
                if ::legacy_config::env::var_os("OMSI_DEBUG_TRAFFIC").is_some() {
                    log::info!(
                        "car_use: line {} tour {} -> {} #{}",
                        line.name,
                        t.number,
                        vehicles[k].0.def.path.display(),
                        vehicles[k].1[j].number
                    );
                }
                self.used_numbers
                    .insert((group.clone(), vehicles[k].1[j].number.trim().to_string()));
                self.tour_vehicle.insert(key, (k, j));
                n_typed += 1;
            }
        }
        if n_fixed + n_typed > 0 {
            log::info!(
                "timetable: car_use gives {n_fixed} tours their fleet number and {n_typed} a vehicle of their line's types"
            );
        }
    }
}
