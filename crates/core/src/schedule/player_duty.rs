//! The player's duty on the schedule: its stops, departures and boards.

use super::*;

impl Schedule {
    /// The stops of a tour in the order it drives them, over all its trips: (trip number in
    /// the duty, stop number in the trip, name, departure there in seconds). Passing stations
    /// are left out.
    pub fn tour_stops(&self, line: &str, tour: &str) -> Vec<(usize, usize, String, f64)> {
        let Some(l) = self
            .data
            .lines
            .iter()
            .find(|l| l.name.eq_ignore_ascii_case(line))
        else {
            return Vec::new();
        };
        let Some(t) = l.tours.iter().find(|t| t.number.eq_ignore_ascii_case(tour)) else {
            return Vec::new();
        };
        let mut out = Vec::new();
        let mut k = 0;
        for tt in &t.trips {
            let Some(ti) = self
                .data
                .trips
                .iter()
                .position(|x| x.name.eq_ignore_ascii_case(&tt.trip))
            else {
                continue;
            };
            let trip = &self.data.trips[ti];
            let departure = tt.departure as f64 * 60.0;
            let times = &self.times[ti][usize::try_from(tt.profile)
                .unwrap_or(0)
                .min(self.times[ti].len() - 1)];
            for (i, id) in trip_stations(trip).iter().enumerate() {
                if !times.stops.get(i).copied().unwrap_or(true) {
                    continue;
                }
                let name = self
                    .data
                    .bus_stops
                    .iter()
                    .find(|b| b.object_id == *id)
                    .map(|b| b.name.clone())
                    .filter(|n| !n.trim().is_empty())
                    .or_else(|| {
                        trip.stations
                            .is_empty()
                            .then(|| {
                                trip.stations_legacy
                                    .get(i)
                                    .and_then(|r| r.get(2))
                                    .map(|n| n.trim().to_string())
                            })
                            .flatten()
                    })
                    .unwrap_or_else(|| format!("{}", i + 1));
                out.push((k, i, name, departure + times.stations[i].1));
            }
            k += 1;
        }
        out
    }

    /// How many trips a tour has that the timetable knows (the trips `tour_stops` numbers).
    pub fn tour_trip_count(&self, line: &str, tour: &str) -> usize {
        let mut n = 0;
        let mut last = None;
        for s in self.tour_stops(line, tour) {
            if last != Some(s.0) {
                n += 1;
                last = Some(s.0);
            }
        }
        n
    }

    /// The position (in order of departure, as `tour_trip_stops` counts) of the trip of a
    /// tour that is under way or next to leave at `now` (seconds of the day).
    pub fn tour_trip_now(&self, line: &str, tour: &str, now: f64) -> usize {
        let all = self.tour_stops(line, tour);
        let mut order: Vec<(usize, f64, f64)> = Vec::new();
        for s in &all {
            match order.iter_mut().find(|o| o.0 == s.0) {
                Some(o) => o.2 = o.2.max(s.3),
                None => order.push((s.0, s.3, s.3)),
            }
        }
        order.sort_by(|a, b| {
            a.1.partial_cmp(&b.1)
                .unwrap_or(std::cmp::Ordering::Equal)
                .then(a.0.cmp(&b.0))
        });
        order.iter().position(|o| o.2 >= now - 60.0).unwrap_or(0)
    }

    /// All the stops of the trip in position `pos` of the tour's trips in order of the time
    /// they leave. The trip number in the entries is the one `tour_stops` gives it.
    pub fn tour_trip_stops(
        &self,
        line: &str,
        tour: &str,
        pos: usize,
    ) -> Vec<(usize, usize, String, f64)> {
        let all = self.tour_stops(line, tour);
        let mut order: Vec<(usize, f64)> = Vec::new();
        for s in &all {
            if !order.iter().any(|o| o.0 == s.0) {
                order.push((s.0, s.3));
            }
        }
        order.sort_by(|a, b| {
            a.1.partial_cmp(&b.1)
                .unwrap_or(std::cmp::Ordering::Equal)
                .then(a.0.cmp(&b.0))
        });
        let Some(&(k, _)) = order.get(pos) else {
            return Vec::new();
        };
        all.into_iter().filter(|s| s.0 == k).collect()
    }

    /// The stops of the trip of a tour that is under way or next to start at `now` (seconds
    /// of the day): the first trip with a stop still to come (a minute of grace), and only
    /// that trip's stops from there on. A tour with nothing left today lists its first trip
    /// whole. Same entries and numbering as `tour_stops`.
    pub fn tour_stops_from(
        &self,
        line: &str,
        tour: &str,
        now: f64,
    ) -> Vec<(usize, usize, String, f64)> {
        let all = self.tour_stops(line, tour);
        let limit = now - 60.0;
        let Some(k) = all
            .iter()
            .find(|s| s.3 >= limit)
            .map(|s| s.0)
            .or_else(|| all.first().map(|s| s.0))
        else {
            return Vec::new();
        };
        let trip: Vec<_> = all.into_iter().filter(|s| s.0 == k).collect();
        let rest: Vec<_> = trip.iter().filter(|s| s.3 >= limit).cloned().collect();
        if rest.is_empty() { trip } else { rest }
    }

    /// The player drives this tour (line and tour as `player_duty` names them): OMSI leaves
    /// it to the player, so no AI bus runs it as well - one of line 76 tour 1 appeared
    /// 5 m beside the player's own bus at the Bauernhof, where the passengers queued.
    /// Returns how many of today's departures that takes from the AI.
    pub fn reserve_tour(&mut self, line: &str, tour: &str) -> usize {
        self.player_tour = Some((line.to_string(), tour.to_string()));
        let mine: Vec<bool> = (0..self.departures.len())
            .map(|i| self.is_player_tour(i))
            .collect();
        let mut n = 0;
        for (d, m) in self.departures.iter_mut().zip(&mine) {
            if *m {
                n += 1;
                d.spawned = true;
            }
        }
        self.pending.retain(|i| !mine[*i]);
        self.waiting.retain(|i| !mine[*i]);
        self.retry_at.retain(|i, _| !mine[*i]);
        self.purge_player_tour = true;
        log::info!("timetable: {n} departures of line {line} tour {tour} left to the player");
        n
    }

    /// LAN play (host): the tours the other players drive now. A tour taken leaves the
    /// timetable like the player's own (its bus on the road goes); one given up (the player
    /// left, or took another duty) runs again from its next departure.
    pub fn set_lan_tours(&mut self, tours: HashSet<(String, String)>) {
        if tours == self.lan_tours {
            return;
        }
        let added: Vec<(String, String)> = tours.difference(&self.lan_tours).cloned().collect();
        let removed: Vec<(String, String)> = self.lan_tours.difference(&tours).cloned().collect();
        self.lan_tours = tours;
        let of = |d: &Departure, t: &(String, String)| {
            d.line.eq_ignore_ascii_case(&t.0) && d.tour.eq_ignore_ascii_case(&t.1)
        };
        for t in &removed {
            let later: Vec<usize> = (0..self.departures.len())
                .filter(|&i| {
                    of(&self.departures[i], t)
                        && self.departures[i].time > self.last_tod
                        && !self.is_player_tour(i)
                })
                .collect();
            for &i in &later {
                self.departures[i].spawned = false;
            }
            log::info!(
                "timetable: line {} tour {} is no LAN player's any more: its {} later departures run again",
                t.0,
                t.1,
                later.len()
            );
        }
        if !added.is_empty() {
            let mine: Vec<bool> = (0..self.departures.len())
                .map(|i| self.is_player_tour(i))
                .collect();
            for (d, m) in self.departures.iter_mut().zip(&mine) {
                if *m {
                    d.spawned = true;
                }
            }
            self.pending.retain(|i| !mine[*i]);
            self.waiting.retain(|i| !mine[*i]);
            self.retry_at.retain(|i, _| !mine[*i]);
            self.purge_player_tour = true;
            for t in &added {
                log::info!("timetable: line {} tour {} left to a LAN player", t.0, t.1);
            }
        }
    }

    /// Does a player drive departure `i` (we, or another LAN player)?
    pub(super) fn is_player_tour(&self, i: usize) -> bool {
        if let Some(d) = self.departures.get(i) {
            if self
                .lan_tours
                .iter()
                .any(|t| d.line.eq_ignore_ascii_case(&t.0) && d.tour.eq_ignore_ascii_case(&t.1))
            {
                return true;
            }
        }
        match (&self.player_tour, self.departures.get(i)) {
            (Some((line, tour)), Some(d)) => {
                d.line.eq_ignore_ascii_case(line)
                    && d.tour.eq_ignore_ascii_case(tour)
                    && self
                    .player_departure
                    .map(|t| (d.time - t).abs() < 30.0)
                    .unwrap_or(true)
            }
            _ => false,
        }
    }

    /// Assign the player a tour of a line (names as in the .ttl / `[newtour]`), starting
    /// with the trip that fits the time of day `now`: the one under way, else the next to
    /// leave - as OMSI does when a time is picked for a tour. (The duty used to start with
    /// the tour's first trip whatever the time: Spandau's "Mo-Fr 3" of line 5 at 15:05 began
    /// with the 14:44 depot run, and the IBIS was typed for it.) The AI no longer drives the
    /// tour: its trips are the player's ([`Schedule::reserve_tour`]).
    /// When the line or its tour cannot be driven, the error says why in words a driver can
    /// act on (a line the date's chrono takes off names the chrono and the day it starts);
    /// the callers used to drop that silently and the game started without a duty.
    ///
    /// `trip` is the trip to start with as the player picked it (OMSI's timetable dialog
    /// has line, tour and trip): its departure time "HH:MM" - the first trip leaving then or
    /// later - or its number in the tour (1 = first). The duty goes on with the tour's next
    /// trips from there, as OMSI's does.
    pub fn player_duty(
        &mut self,
        world: &World,
        line: &str,
        tour: &str,
        now: f64,
        trip: Option<&str>,
        whole_tour: bool,
    ) -> Result<PlayerDuty, String> {
        let date = |code: i32| {
            format!(
                "{:04}-{:02}-{:02}",
                code / 10000,
                code / 100 % 100,
                code % 100
            )
        };
        let Some(l) = self
            .data
            .lines
            .iter()
            .find(|l| l.name.eq_ignore_ascii_case(line))
        else {
            let running = self.data.lines.len();
            if let Some((name, dir)) = self
                .deactivated
                .iter()
                .find(|(l, _)| l.trim().eq_ignore_ascii_case(line.trim()))
            {
                let from = Some(::legacy_config::resolve_path(dir, "Chrono.cfg"))
                    .and_then(|p| ::legacy_config::CfgFile::read(&p).ok())
                    .map(|f| ::map::ailists::parse_chrono_cfg(&f).start_date)
                    .filter(|d| *d > 0);
                return Err(format!(
                    "line {name} does not run on {}: the timetable change '{}'{} takes it off. {running} other lines run that day: pick one of them, or an earlier date",
                    date(world.date),
                    dir.file_name().unwrap_or_default().to_string_lossy(),
                    from.map(|d| format!(" of {}", date(d))).unwrap_or_default()
                ));
            }
            return Err(format!(
                "line {line} is not in the timetable of this map on {}: {running} lines run that day",
                date(world.date)
            ));
        };
        let t = match l.tours.iter().find(|t| t.number.eq_ignore_ascii_case(tour)) {
            Some(t) => t,
            None => {
                let Some(first) = l.tours.first() else {
                    return Err(format!("line {} has no tours", l.name));
                };
                if !tour.is_empty() {
                    log::warn!(
                        "line {} has no tour '{tour}': driving its tour {}",
                        l.name,
                        first.number
                    );
                }
                first
            }
        };
        let mut trips = Vec::new();
        for tt in &t.trips {
            let Some(ti) = self
                .data
                .trips
                .iter()
                .position(|x| x.name.eq_ignore_ascii_case(&tt.trip))
            else {
                continue;
            };
            let trip = &self.data.trips[ti];
            let departure = tt.departure as f64 * 60.0;
            let times = &self.times[ti][usize::try_from(tt.profile)
                .unwrap_or(0)
                .min(self.times[ti].len() - 1)];
            let stops = trip_stations(trip)
                .iter()
                .enumerate()
                .map(|(i, id)| {
                    let name = self.station_name(trip, i, *id);
                    let position = world.object_positions.lock().get(id).map(|p| p.0);
                    let (arr, dep) = times.stations[i];
                    PlannedStop {
                        object_id: *id,
                        name,
                        arr: departure + arr,
                        dep: departure + dep,
                        position,
                        dir: StopDir::default(),
                        stops: times.stops[i],
                    }
                })
                .collect();
            let mut planned = PlannedTrip {
                name: trip.name.clone(),
                line: trip.line.clone(),
                terminus: trip.terminus.clone(),
                departure,
                end: departure + times.duration,
                stops,
            };
            planned.set_dirs();
            trips.push(planned);
        }
        if trips.is_empty() {
            return Err(format!(
                "line {} tour {} has no trip the timetable knows ({} listed)",
                l.name,
                t.number,
                t.trips.len()
            ));
        }
        let (line_name, tour_name) = (l.name.clone(), t.number.clone());
        let trip_index = match trip.map(str::trim).filter(|t| !t.is_empty()) {
            Some(pick) => chosen_trip(&trips, pick).ok_or_else(|| {
                format!(
                    "line {line_name} tour {tour_name} has no trip '{pick}': its {} trips leave {}",
                    trips.len(),
                    trips
                        .iter()
                        .map(|t| hhmm(t.departure))
                        .collect::<Vec<_>>()
                        .join(", ")
                )
            })?,
            None => starting_trip(&trips, now),
        };
        let hm = |t: f64| {
            format!(
                "{:02}:{:02}",
                (t / 3600.0) as i32,
                ((t % 3600.0) / 60.0) as i32
            )
        };
        // A picked trip is the duty, one way to its terminus, as a trip chosen in OMSI is;
        // the rest of the tour only with `--whole-tour`.
        let (trips, trip_index, first_trip) = if trip.is_some() && !whole_tour {
            (vec![trips[trip_index].clone()], 0, trip_index)
        } else {
            (trips, trip_index, 0)
        };
        // the AI leaves the player what the player drives: the tour, or just the one trip
        self.player_departure =
            (trips.len() == 1 && trip.is_some() && !whole_tour).then(|| trips[0].departure);
        let reserved = self.reserve_tour(&line_name, &tour_name);
        let current = &trips[trip_index];
        log::info!(
            "player duty: line {line_name} tour {tour_name}: {} trips from {} ({} of them not driven by the AI); at {} trip {} {} {} (to {}), then {}",
            trips.len(),
            hm(trips[0].departure),
            reserved,
            hm(now),
            trip_index + 1,
            current.name,
            // (a trip after midnight picked in the evening leaves tonight)
            if current.departure < now && now - current.departure < DAY / 2.0 {
                format!("under way since {}", hm(current.departure))
            } else {
                format!("leaves at {}", hm(current.departure))
            },
            current.terminus,
            trips
                .get(trip_index + 1)
                .map(|t| format!("{} at {}", t.name, hm(t.departure)))
                .unwrap_or_else(|| "the end of the duty".into())
        );
        Ok(PlayerDuty {
            line: line_name,
            tour: tour_name,
            trips,
            trip_index,
            first_trip,
            statistics: Default::default(),
            completed_report: None,
            next_stop: 0,
            at_stop: false,
            arrived_late: None,
            done: false,
            left_late: None,
            held_back: false,
            odo: 0.0,
            last_pos: None,
            arrival_odo: 0.0,
            left_odo: 0.0,
            placed: false,
            trip_changed: false,
            picked: trip.map(|t| !t.trim().is_empty()).unwrap_or(false),
            first_update: None,
            heading: 0.0,
        })
    }

    /// The buses due at one bus stop (map object id) within the next two hours, unsorted, as
    /// (expected arrival, line, terminus, time it stands at the stop), all in seconds of the day:
    /// the timetable buses with the delay they run with, and the player's. The terminus is the
    /// trip's own, as OMSI hands it to the displays.
    pub(super) fn stop_list(
        &self,
        stop: i64,
        now: f64,
        on_road: &HashMap<usize, OnRoad>,
        duty: Option<&PlayerDuty>,
    ) -> Vec<(f64, String, String, f64)> {
        let mut list: Vec<(f64, String, String, f64)> = Vec::new();
        for &(trip, k) in self.visits.get(&stop).map(|v| v.as_slice()).unwrap_or(&[]) {
            for &i in &self.trip_departures[trip] {
                if !self.runs(i) {
                    continue;
                }
                let d = &self.departures[i];
                let tt = &self.times[d.trip][d.profile];
                if !tt.stops[k] {
                    continue;
                }
                let (arrive, leave) = (d.time + tt.stations[k].0, d.time + tt.stations[k].1);
                if arrive > now + BOARD_AHEAD || leave < now - BOARD_AHEAD {
                    continue;
                }
                if self.is_player_tour(i) {
                    continue;
                }
                let expected = match on_road.get(&i) {
                    Some(r) => match r.next {
                        // the stations before the bus's next stop are behind it
                        Some(next) if leave < next - 0.5 => continue,
                        None => continue,
                        // standing at this stop
                        Some(next) if r.dwelling && (leave - next).abs() < 0.5 => now,
                        Some(next) => {
                            // on its way: at least as late as it left its last stop, and
                            // later still once its next stop is overdue
                            let next_arrive = tt
                                .stations
                                .iter()
                                .find(|s| (d.time + s.1 - next).abs() < 0.5)
                                .map(|s| d.time + s.0)
                                .unwrap_or(next);
                            let late = if r.dwelling {
                                r.late
                            } else {
                                r.late.max(now - next_arrive)
                            };
                            (arrive + late).max(now)
                        }
                    },
                    // not on the road (still to come, or where no tiles are loaded): on time
                    None if leave < now => continue,
                    None => arrive.max(now),
                };
                list.push((
                    expected,
                    self.display_line(i),
                    self.data.trips[d.trip].terminus.clone(),
                    (leave - arrive).max(0.0),
                ));
            }
        }
        // the player's bus
        if let Some(duty) = duty {
            // a trip not begun leaves on time at the earliest (the bus waits at its
            // first stop), one under way arrives as early or late as it runs
            let delay = duty.delay(now);
            let lateness = if duty.left_late.is_some() || duty.at_stop {
                delay
            } else {
                delay.max(0.0)
            };
            for (ti, trip) in duty.trips.iter().enumerate().skip(duty.trip_index) {
                if trip.departure > now + BOARD_AHEAD {
                    break;
                }
                for (k, s) in trip
                    .stops
                    .iter()
                    .enumerate()
                    .filter(|(_, s)| s.object_id == stop && s.stops)
                {
                    let current = ti == duty.trip_index;
                    let expected = if current
                        && (k < duty.next_stop || duty.done && k + 1 < trip.stops.len())
                    {
                        continue;
                    } else if current && k == duty.next_stop && duty.at_stop {
                        now
                    } else if current {
                        (s.arr + lateness).max(now)
                    } else {
                        (s.arr + delay.max(0.0)).max(now)
                    };
                    list.push((
                        expected,
                        trip.line.trim().to_string(),
                        trip.terminus.clone(),
                        (s.dep - s.arr).max(0.0),
                    ));
                }
            }
        }
        list
    }

    /// Make the departure boards of the stops whose displays are near
    /// (`World::timetable_boards`), and hand the scenery the time of day. The boards are
    /// made at most once a second: the buses due at each stop in the next two hours,
    /// soonest first - the timetable buses with the delay they run with, and the player's.
    pub fn update_boards(
        &mut self,
        world: &World,
        traffic: Option<&Traffic>,
        duty: Option<&PlayerDuty>,
        clock: &::simulation::SimClock,
    ) {
        let now = clock.time;
        let mut boards = world.timetable_boards.lock();
        boards.clock = Some(clock.clone());
        if (now - self.boards_made).abs() < 1.0
            || (boards.wanted.is_empty() && boards.wanted_names.is_empty())
        {
            return;
        }
        self.boards_made = now;
        // the timetable buses on the road: departure -> where they are in their trip (a
        // train runs its track without stops of its own and is taken as on time)
        let mut on_road: HashMap<usize, OnRoad> = HashMap::new();
        if let Some(t) = traffic {
            for car in t.cars() {
                let Some(&i) = self.car_departure.get(&car.id.get()) else {
                    continue;
                };
                if trip_stations(&self.data.trips[self.departures[i].trip]).is_empty() {
                    continue;
                }
                let next = car
                    .bus
                    .as_ref()
                    .and_then(|b| b.stops.front())
                    .map(|s| s.depart)
                    .or_else(|| {
                        self.running.iter().find(|r| r.car == car.id).and_then(|r| {
                            r.stations
                                .iter()
                                .zip(&r.served)
                                .find(|(_, s)| !**s)
                                .map(|(st, _)| st.1)
                        })
                    });
                on_road.insert(
                    i,
                    OnRoad {
                        next,
                        dwelling: car.at_stop(),
                        late: car.bus.as_ref().map(|b| b.state.delay).unwrap_or(0.0).max(0.0),
                    },
                );
            }
        }
        let wanted = boards.wanted.clone();
        let mut made = HashMap::new();
        for stop in wanted {
            let mut list = self.stop_list(stop, now, &on_road, duty);
            list.sort_by(|a, b| a.0.total_cmp(&b.0));
            list.truncate(8);
            if ::legacy_config::env::var_os("OMSI_DEBUG_BOARDS").is_some() {
                log::info!(
                    "board of stop {stop} at {:.0} s: {:?}",
                    now,
                    list.iter()
                        .map(|(t, l, d, _)| format!("{l} {d} in {:.1} min", (t - now) / 60.0))
                        .collect::<Vec<_>>()
                );
            }
            made.insert(
                stop,
                list.into_iter().map(|(t, l, d, _)| (l, d, t)).collect(),
            );
        }
        boards.by_stop = made;
        // the departures the pages asked for by stop name (`omsi.getDepartures`): the next two
        // hours, at most 20, as (line, destination, timestamp)
        let mut departures = std::collections::HashMap::new();
        for key in boards.wanted_names.clone() {
            let mut ids: Vec<i64> = self
                .data
                .bus_stops
                .iter()
                .filter(|b| b.name.trim().eq_ignore_ascii_case(&key))
                .map(|b| b.object_id)
                .collect();
            ids.sort_unstable();
            ids.dedup();
            let mut list: Vec<(f64, String, String)> = Vec::new();
            for id in ids {
                for (expected, line, terminus, dwell) in self.stop_list(id, now, &on_road, duty) {
                    let leaves = expected + dwell;
                    if leaves <= now + BOARD_AHEAD {
                        list.push((leaves, line, terminus));
                    }
                }
            }
            list.sort_by(|a, b| a.0.total_cmp(&b.0));
            list.truncate(MAX_PAGE_DEPARTURES);
            departures.insert(
                key,
                list.into_iter()
                    .map(|(t, l, d)| (l, d, ::simulation::vehicle_api::timestamp(clock, t)))
                    .collect(),
            );
        }
        boards.departures = departures;
        boards.departures_gen = boards.departures_gen.wrapping_add(1);
    }

    /// The line a departure's displays show: its trip's own (" 5"), else the timetable
    /// line's name.
    /// The name of station `i` (object `id`) of `trip`, as the map's stop calls it.
    pub(super) fn station_name(&self, trip: &::timetable::Trip, i: usize, id: i64) -> String {
        self.data
            .bus_stops
            .iter()
            .find(|b| b.object_id == id)
            .map(|b| b.name.clone())
            .filter(|n| !n.trim().is_empty())
            // (the trip file names its stations too: a stop whose object is not
            // among the map's known stops - Novi Sad's, on tiles not loaded yet -
            // had no name on the navigator, the HUD and in the log)
            .or_else(|| {
                trip.stations
                    .is_empty()
                    .then(|| {
                        trip.stations_legacy
                            .get(i)
                            .and_then(|r| r.get(2))
                            .map(|n| n.trim().to_string())
                    })
                    .flatten()
            })
            .unwrap_or_default()
    }

    /// The station names of trip `ti`, for picking its route in the depot file.
    pub(super) fn trip_stop_names(&self, ti: usize) -> Vec<String> {
        let trip = &self.data.trips[ti];
        trip_stations(trip)
            .iter()
            .enumerate()
            .map(|(i, id)| self.station_name(trip, i, *id))
            .collect()
    }

    pub(super) fn display_line(&self, i: usize) -> String {
        let d = &self.departures[i];
        let own = self.data.trips[d.trip].line.trim();
        if own.is_empty() {
            d.line.trim().to_string()
        } else {
            own.to_string()
        }
    }
}
