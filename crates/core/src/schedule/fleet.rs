//! The fleet ahead of the clock, the routes of its trips and their checks.

use super::*;

impl Schedule {
    /// Keep the GPU's fleet to the vehicles of the next minutes: read the sets of the
    /// coming departures on the workers and upload them (one a frame) well before they are
    /// due, and let go of the sets nobody uses any more.
    pub(super) fn fleet(
        &mut self,
        world: &World,
        traffic: &mut Traffic,
        renderer: &Renderer,
        scene: &mut Scene,
        day_time: f64,
    ) {
        let ready = self.fleet_ready.lock().pop();
        if let Some(key) = ready {
            if let Some(ty) = self.fleet_reading.remove(&key) {
                let t = std::time::Instant::now();
                world.precache_vehicle(renderer, scene, &ty, key.1);
                if ::legacy_config::env::var_os("OMSI_PROFILE").is_some() {
                    log::info!(
                        "timetable fleet: {} (scheme {:?}) uploaded ahead in {:.1} ms",
                        ty.def
                            .path
                            .file_name()
                            .unwrap_or_default()
                            .to_string_lossy(),
                        key.1,
                        t.elapsed().as_secs_f64() * 1000.0
                    );
                }
            }
        }
        if (day_time - self.fleet_check).abs() < 5.0 {
            return;
        }
        self.fleet_check = day_time;
        let sets = self.upcoming_sets(world, traffic, day_time);
        let keep: HashSet<crate::scene::VehicleKey> = sets
            .iter()
            .map(|(t, s)| (t.def.path.clone(), *s))
            .chain(self.fleet_reading.keys().cloned())
            .chain(
                traffic
                    .random_sets()
                    .into_iter()
                    .map(|(t, s)| (t.def.path.clone(), s)),
            )
            .collect();
        // (OMSI_FLEET_IDLE=<s> shortens the wait, for tests)
        let idle = ::legacy_config::env::var("OMSI_FLEET_IDLE")
            .ok()
            .and_then(|v| v.parse::<f32>().ok())
            .map(std::time::Duration::from_secs_f32)
            .unwrap_or(FLEET_IDLE);
        if world.trim_vehicle_sets(renderer, scene, &keep, idle) > 0 {
            crate::release_free_memory();
        }
        let prefetch = world.vehicle_prefetch(renderer);
        for (ty, scheme) in sets {
            let key = (ty.def.path.clone(), scheme);
            if self.fleet_reading.contains_key(&key) || world.has_vehicle_set(&key) {
                continue;
            }
            // two at a time: a set's repaints are compressed from pictures of tens of
            // megabytes (the rest follows at the next look, five seconds on)
            if self.fleet_reading.len() >= 2 {
                break;
            }
            self.fleet_reading.insert(key.clone(), ty.clone());
            let (p, ready) = (prefetch.clone(), self.fleet_ready.clone());
            // (off the frame's pool: see `threads`)
            crate::threads::background_pool().spawn(move || {
                p.prefetch(&ty, scheme);
                ready.lock().push(key);
            });
        }
    }

    /// The lanes a trip runs on (for the navigator): its track, else the station links
    /// between its stops, as far as the tiles have brought them - and whether that is all.
    pub fn trip_route(
        &self,
        world: &World,
        traffic: &Traffic,
        trip_name: &str,
    ) -> (Vec<usize>, RouteStatus) {
        let Some(trip) = self
            .data
            .trips
            .iter()
            .find(|x| x.name.eq_ignore_ascii_case(trip_name))
        else {
            return (Vec::new(), RouteStatus::Invalid);
        };
        let slots = self.slots(
            world,
            traffic,
            &self.steps_of(trip_name, &trip_stations(trip)).0,
            None,
        );
        let status = if slots.contains(&Slot::Waiting) {
            RouteStatus::PendingTiles
        } else if slots.iter().any(|s| matches!(s, Slot::Lane(_))) {
            RouteStatus::Complete
        } else {
            RouteStatus::Invalid
        };
        (
            slots
                .into_iter()
                .filter_map(|s| if let Slot::Lane(l) = s { Some(l) } else { None })
                .collect(),
            status,
        )
    }

    /// The lanes a trip runs on in `net` - the navigator's network of the whole map, which
    /// has every tile's lanes whether loaded or not - chosen as `slots` chooses them (of a
    /// two-way path the direction that joins the lanes before and after).
    pub fn trip_route_in(&self, net: &::traffic::Network, trip_name: &str) -> Vec<usize> {
        let Some(trip) = self
            .data
            .trips
            .iter()
            .find(|x| x.name.eq_ignore_ascii_case(trip_name))
        else {
            return Vec::new();
        };
        let (steps, _) = self.steps_of(trip_name, &trip_stations(trip));
        let keys: Vec<Option<LaneKey>> = steps.iter().map(|st| st.key).collect();
        // The navigator's whole-map network has every tile's lanes whether loaded or not:
        // a key with no lane is missing here, never merely pending.
        compile_route(net, &keys, |_| TileState::Unknown, None)
            .lanes()
            .into_iter()
            .map(|l| l.index())
            .collect()
    }

    /// `OMSI_CHECK_TRIPS=1`: build the route of every trip on the loaded lanes and say where
    /// consecutive lanes do not join - a gap the bus would jump, or a lane taken the wrong
    /// way round (its end, not its start, lies where the lane before ends), which sends a
    /// bus into the oncoming traffic. Only trips whose route is wholly loaded are judged.
    pub fn check_routes(&self, world: &World, traffic: &mut Traffic) {
        for trip in &self.data.trips {
            let (steps, _) = self.steps_of(&trip.name, &trip_stations(trip));
            Self::add_twins(traffic, &steps);
            let lanes: Vec<usize> = self
                .slots(world, traffic, &steps, None)
                .iter()
                .filter_map(|s| {
                    if let Slot::Lane(l) = s {
                        Some(*l)
                    } else {
                        None
                    }
                })
                .collect();
            Self::add_connectors(traffic, &lanes);
        }
        let traffic = &*traffic;
        let net = traffic.net();
        let (mut trips, mut joints, mut linked, mut changes, mut gaps, mut wrong, mut partial) =
            (0, 0, 0, 0, 0, 0, 0);
        let mut bad_length = 0;
        for trip in &self.data.trips {
            let stations = trip_stations(trip);
            let (steps, _) = self.steps_of(&trip.name, &stations);
            let slots = self.slots(world, traffic, &steps, None);
            if slots.contains(&Slot::Waiting) || steps.is_empty() {
                if partial < 5 {
                    let k = slots.iter().position(|s| *s == Slot::Waiting).unwrap_or(0);
                    log::info!(
                        "check trips: {}: {} steps, {} on tiles not loaded, first {:?} (tile loaded {}, in the map {})",
                        trip.name,
                        steps.len(),
                        slots.iter().filter(|s| **s == Slot::Waiting).count(),
                        steps.get(k).and_then(|s| s.key),
                        steps
                            .get(k)
                            .and_then(|s| s.key)
                            .map(|key| traffic.has_lane_tile(key.tile))
                            .unwrap_or(false),
                        steps
                            .get(k)
                            .and_then(|s| s.key)
                            .map(|key| world.has_tile(key.tile))
                            .unwrap_or(false),
                    );
                }
                partial += 1;
                continue;
            }
            trips += 1;
            // `OMSI_CHECK_TRIPS=<trip name>`: every step of that trip
            if ::legacy_config::env::var("OMSI_CHECK_TRIPS")
                .map(|v| v.eq_ignore_ascii_case(&trip.name))
                .unwrap_or(false)
            {
                for (k, (st, sl)) in steps.iter().zip(&slots).enumerate() {
                    let cands: Vec<String> = st
                        .key
                        .and_then(|key| net.by_key.get(&key))
                        .map(|c| {
                            c.iter()
                                .map(|&l| {
                                    let x = &net.lanes[l];
                                    format!(
                                        "{l}{} ({:.1},{:.1})->({:.1},{:.1}) {:.1} m",
                                        if x.reversed { "r" } else { "" },
                                        x.start().x,
                                        x.start().y,
                                        x.end().x,
                                        x.end().y,
                                        x.length()
                                    )
                                })
                                .collect()
                        })
                        .unwrap_or_default();
                    log::info!(
                        "check trips: {} step {k} leg {} {:?} ({:.1} m): {:?} of {:?}",
                        trip.name,
                        st.leg,
                        st.key,
                        st.length,
                        sl,
                        cands
                    );
                }
            }
            // the lane a step names should be as long as the file says its path is
            for (st, sl) in steps.iter().zip(&slots) {
                if let Slot::Lane(l) = sl {
                    let len = net.lanes[*l].length() as f64;
                    if st.length > 0.5 && (len - st.length).abs() > 1.0 + 0.05 * st.length {
                        if bad_length < 12 {
                            log::info!(
                                "check trips: {}: lane {} {:?} is {len:.1} m long, the file says {:.1} m",
                                trip.name,
                                l,
                                net.lanes[*l].key,
                                st.length
                            );
                        }
                        bad_length += 1;
                    }
                }
            }
            let lanes: Vec<usize> = slots
                .iter()
                .filter_map(|s| {
                    if let Slot::Lane(l) = s {
                        Some(*l)
                    } else {
                        None
                    }
                })
                .collect();
            let lanes = bridge_gaps(net, &lanes).0;
            let mut shown = 0;
            for (k, w) in lanes.windows(2).enumerate() {
                let (a, b) = (&net.lanes[w[0]], &net.lanes[w[1]]);
                joints += 1;
                if a.next.contains(&w[1]) {
                    linked += 1;
                    continue;
                }
                if net.parallel(w[0], w[1]) {
                    changes += 1;
                    continue;
                }
                let gap = (b.start() - a.end()).truncate().length();
                let backwards = (b.end() - a.end()).truncate().length();
                let is_wrong = backwards + 1.0 < gap && backwards < 3.0;
                if is_wrong {
                    wrong += 1;
                } else if gap > 2.0 {
                    gaps += 1;
                } else {
                    linked += 1;
                    continue;
                }
                if shown < 6 {
                    shown += 1;
                    log::info!(
                        "check trips: {}: step {k}: lane {} {:?} rev {} -> {} {:?} rev {}: {} (gap {gap:.1} m, to its end {backwards:.1} m) at ({:.0}, {:.0})",
                        trip.name,
                        w[0],
                        a.key,
                        a.reversed,
                        w[1],
                        b.key,
                        b.reversed,
                        if is_wrong { "WRONG WAY" } else { "gap" },
                        a.end().x,
                        a.end().y
                    );
                }
            }
        }
        log::info!(
            "check trips: {trips} trips on loaded lanes ({partial} not wholly loaded): {joints} joints, {linked} joined, {changes} lane changes, {gaps} gaps, {wrong} taken the wrong way round; {bad_length} lanes not as long as the file says"
        );
    }

    /// Vehicles of a plain `[aigroup_2]`, loaded on first use and kept.
    pub(super) fn pool(&mut self, root: &Path, world: &World, group: &str) -> &[Arc<VehicleType>] {
        if !self.pools.contains_key(group) {
            let mut out = Vec::new();
            for g in world
                .ailists
                .groups
                .iter()
                .filter(|g| !g.is_depot && g.name.to_ascii_lowercase() == group)
            {
                for v in &g.vehicles {
                    if v.file.to_ascii_lowercase().ends_with(".zug") {
                        continue;
                    }
                    let path = ::legacy_config::resolve_path(root, &v.file);
                    match VehicleType::load_ai(root, &path) {
                        Ok(t) => out.push(Arc::new(t)),
                        Err(e) => log::warn!("AI group '{}' vehicle {}: {e}", g.name, v.file),
                    }
                }
            }
            if !out.is_empty() {
                log::info!(
                    "AI group '{group}': {} vehicles loaded for its timetable trips",
                    out.len()
                );
            }
            self.pools.insert(group.to_string(), out);
        }
        self.pools.get(group).map(|v| v.as_slice()).unwrap_or(&[])
    }
}
