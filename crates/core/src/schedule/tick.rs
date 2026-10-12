//! The schedule's clock tick: which departures are due, and the buses on their way.

use super::*;

impl Schedule {
    /// The destinations for timetable calls due within `ahead` seconds.
    ///
    /// Passenger demand is keyed by a destination and the compatible terminus, not a
    /// specific departure. This lets either bus on the same service carry the passengers.
    pub fn stop_targets_within(
        &self,
        now: f64,
        ahead: f64,
    ) -> HashMap<i64, Vec<PassengerTarget>> {
        let names = self.stop_names();
        let name_of = |id: i64| names.get(&id).cloned().unwrap_or_else(|| id.to_string());
        let calls: Vec<(usize, i64, f64)> = self.departures.iter().enumerate()
            .filter(|(departure, _)| self.runs(*departure))
            .flat_map(|(departure, entry)| {
                let times = self.times_of(departure);
                trip_stations(&self.data.trips[entry.trip]).into_iter().enumerate().filter_map(move |(index, stop)| {
                    times.stops.get(index).copied().unwrap_or(true)
                        .then(|| times.stations.get(index).map(|station| (departure, stop, self.dep_time(departure) + station.1)))
                        .flatten()
                })
            })
            .collect();
        let mut targets: HashMap<i64, Vec<PassengerTarget>> = HashMap::new();
        for (departure, entry) in self.departures.iter().enumerate() {
            if !self.runs(departure) {
                continue;
            }
            let times = self.times_of(departure);
            let stations = trip_stations(&self.data.trips[entry.trip]);
            let terminus = self.data.trips[entry.trip].terminus.trim().to_string();
            for from in 0..stations.len() {
                let Some((_, offset)) = times.stations.get(from) else { continue };
                let call = self.dep_time(departure) + offset;
                if !times.stops.get(from).copied().unwrap_or(true)
                    || !(now..=now + ahead).contains(&call)
                {
                    continue;
                }
                let here = name_of(stations[from]);
                let list = targets.entry(stations[from]).or_default();
                let deadline = calls.iter()
                    .filter(|(candidate, stop, candidate_time)| {
                        *stop == stations[from]
                            && *candidate_time > call
                            && if entry.line.trim().is_empty() {
                                self.departures[*candidate].trip == entry.trip
                            } else {
                                self.departures[*candidate].line == entry.line
                            }
                    })
                    .map(|(_, _, candidate_time)| *candidate_time)
                    .min_by(|a, b| a.total_cmp(b))
                    .map(|next| call + (next - call) * 1.25)
                    .unwrap_or(call + 60.0 * 60.0);
                for (to, stop) in stations.iter().enumerate().skip(from + 1) {
                    if !times.stops.get(to).copied().unwrap_or(true) {
                        continue;
                    }
                    let destination = name_of(*stop);
                    if destination == here {
                        continue;
                    }
                    match list.iter_mut().find(|target| target.destination == destination) {
                        Some(target) => {
                            target.termini.insert(terminus.clone());
                            target.deadline = target.deadline.max(deadline);
                        }
                        None => list.push(PassengerTarget {
                            destination,
                            termini: HashSet::from_iter([terminus.clone()]),
                            deadline,
                        }),
                    }
                }
            }
        }
        targets
    }

    /// How many due departures are still waiting to be put on the road.
    /// Omsi.exe's station targets (0x61cb18): per bus stop, the stops the timetable's trips
    /// go on to from there, each with the termini of the trips that do. A passenger waiting
    /// at the stop wants one of these targets and boards a bus whose terminus is among its
    /// termini (0x61c33c); the names compare exactly.
    pub fn stop_targets(&self) -> HashMap<i64, Vec<(String, HashSet<String>)>> {
        let names = self.stop_names();
        let name_of = |id: i64| names.get(&id).cloned().unwrap_or_else(|| id.to_string());
        station_targets(
            self.data
                .trips
                .iter()
                .map(|t| (trip_stations(t), t.terminus.trim().to_string())),
            name_of,
        )
    }

    /// The name each bus stop object has in the timetable (`Busstops.cfg`, the first entry
    /// of an object id): what [`Schedule::stop_targets`] calls it. The map object's own
    /// label can read otherwise (renamed in the editor, another code page than the tiles').
    pub fn stop_names(&self) -> HashMap<i64, String> {
        let mut names = HashMap::new();
        for b in &self.data.bus_stops {
            names
                .entry(b.object_id)
                .or_insert_with(|| b.name.trim().to_string());
        }
        names
    }

    pub fn pending(&self) -> usize {
        self.pending.len()
    }

    /// Spawn buses whose departure time has come (or passed within `window` seconds), and
    /// the layover buses of the next quarter of an hour.
    pub fn tick(
        &mut self,
        world: &World,
        traffic: &mut Traffic,
        renderer: &Renderer,
        scene: &mut Scene,
        day_time: f64,
        window: f64,
    ) {
        // (a LAN client draws the host's timetable buses)
        if traffic.is_mirror() {
            return;
        }
        self.roll_day(day_time);
        // the time of day of today's timetable
        let tod = day_time - self.day_base;
        self.last_tod = tod;
        if std::mem::take(&mut self.purge_player_tour) {
            let gone: Vec<u64> = self
                .car_departure
                .iter()
                .filter(|(_, i)| self.is_player_tour(**i))
                .map(|(id, _)| *id)
                .collect();
            for id in gone {
                self.car_departure.remove(&id);
                self.running.retain(|r| r.car != id);
                if traffic.remove_car(world, renderer, scene, id) {
                    log::info!("timetable: bus {id} of the player's tour taken off the road");
                }
            }
        }
        let loading = window > 60.0;
        let due: Vec<usize> = self
            .departures
            .iter()
            .enumerate()
            .filter(|(i, d)| !d.spawned && d.time <= tod && d.time > tod - window && self.runs(*i))
            .map(|(i, _)| i)
            .collect();
        for i in due {
            self.departures[i].spawned = true;
            // the tour's bus is still on its way here: it takes the trip on when it arrives
            if self.tour_prev[i]
                .and_then(|k| self.tour_bus(k, traffic))
                .is_some()
            {
                self.awaiting.insert(i);
                continue;
            }
            self.pending.push_back(i);
            if loading {
                self.startup.insert(i);
            }
        }
        // Buses on their layover: a trip that leaves within the next quarter of an hour,
        // whose tour's previous trip is already over, stands at its first stop with the
        // doors shut until its departure. Without this a map with one bus per line
        // showed no bus at all for most of the hour - it only existed while driving.
        let mut early: Vec<usize> = Vec::new();
        // departures are sorted by time: only the ones in the next quarter of an hour
        let start = self.departures.partition_point(|d| d.time <= tod);
        for i in start..self.departures.len() {
            let d = &self.departures[i];
            if d.time > tod + LAYOVER {
                break;
            }
            if d.spawned || self.later_layover.contains(&i) || !self.runs(i) {
                continue;
            }
            // only a trip with stops has a first stop to wait at: a flight (TXL.ttl, every
            // ten minutes) would take off a quarter of an hour early
            let Some(first) = trip_stations(&self.data.trips[d.trip]).first().copied() else {
                continue;
            };
            if d.time > tod + LAYOVER_SHARED {
                let shared = match self.shared_stand.get(&first) {
                    Some(&v) => v,
                    None => {
                        let positions = world.object_positions.lock();
                        let Some(&(here, _)) = positions.get(&first) else {
                            continue;
                        };
                        let v = self.served.contains(&first)
                            || self.served.iter().any(|sid| {
                            positions
                                .get(sid)
                                .map(|p| (p.0 - here).length() < 30.0)
                                .unwrap_or(false)
                        });
                        if ::legacy_config::env::var_os("OMSI_DEBUG_TRAFFIC").is_some() {
                            let nearest = self
                                .served
                                .iter()
                                .filter_map(|sid| {
                                    positions.get(sid).map(|p| ((p.0 - here).length(), *sid))
                                })
                                .fold((f64::MAX, 0), |a, b| if b.0 < a.0 { b } else { a });
                            log::info!(
                                "layover stand {first} at ({:.0}, {:.0}): shared {v}, nearest served station {} at {:.0} m",
                                here.x,
                                here.y,
                                nearest.1,
                                nearest.0
                            );
                        }
                        drop(positions);
                        self.shared_stand.insert(first, v);
                        v
                    }
                };
                if shared {
                    continue;
                }
            }
            let prev_running = self.tour_prev[i]
                .map(|k| {
                    self.dep_time(k) + self.times_of(k).duration >= day_time
                        || self.tour_bus(k, traffic).is_some()
                })
                .unwrap_or(false);
            if !prev_running {
                early.push(i);
            }
        }
        for i in early {
            self.departures[i].spawned = true;
            self.pending.push_back(i);
            if loading {
                self.startup.insert(i);
            }
        }
        // buses whose ground was unloaded under them wait for it to come back
        for id in std::mem::take(&mut traffic.removed_scheduled) {
            if let Some(i) = self.car_departure.remove(&id) {
                log::debug!("departure {i}: its bus left the loaded tiles, waiting for them");
                self.waiting.push(i);
            }
        }
        if self.car_departure.len() > 64 + traffic.cars.len() * 2 {
            let alive: std::collections::HashSet<u64> = traffic.cars.iter().map(|c| c.id).collect();
            self.car_departure.retain(|id, _| alive.contains(id));
        }
        // tiles brought lanes, or half a minute went by: the waiting departures may be on
        // loaded ground now, and the routes that stopped short may go on
        let grew = traffic.lanes_generation != self.seen_generation;
        if grew || day_time - self.last_retry >= 30.0 || day_time < self.last_retry {
            self.last_retry = day_time;
            for i in std::mem::take(&mut self.waiting) {
                if !self.pending.contains(&i) {
                    self.pending.push_back(i);
                }
            }
            self.retry_at.clear();
        } else if !self.retry_at.is_empty() {
            // the ones whose vehicle has just reached the loaded part of its way
            let due: Vec<usize> = self
                .waiting
                .iter()
                .copied()
                .filter(|i| {
                    self.retry_at
                        .get(i)
                        .map(|t| *t <= day_time)
                        .unwrap_or(false)
                })
                .collect();
            if !due.is_empty() {
                self.waiting.retain(|i| !due.contains(i));
                for i in due {
                    self.retry_at.remove(&i);
                    if !self.pending.contains(&i) {
                        // ahead of the others: it is due now
                        self.pending.push_front(i);
                    }
                }
            }
        }
        if grew {
            self.seen_generation = traffic.lanes_generation;
            self.carry_on(world, traffic);
        }
        self.fleet(world, traffic, renderer, scene, day_time);
        self.tour_handover(world, traffic, renderer, scene, day_time);
        // a handful per call: spawning a bus builds its meshes, and a whole rush hour at
        // once is a frame that lasts seconds (a departure that has to wait costs little)
        let (mut spawned, mut tried) = (0, 0);
        while spawned < if loading { 3 } else { 1 } && tried < 24 {
            let Some(i) = self.pending.pop_front() else {
                break;
            };
            tried += 1;
            match self.spawn_departure(i, world, traffic, renderer, scene, day_time, None) {
                Placed::Spawned => spawned += 1,
                Placed::Wait => self.waiting.push(i),
                Placed::Busy => {
                    // a vehicle stands where the bus would appear (the player's bus may stand
                    // there for its whole layover): again in a few seconds, without keeping
                    // the traffic on its quick spawning pace meanwhile
                    self.retry_at.insert(i, day_time + 3.0);
                    self.waiting.push(i);
                }
                Placed::Drop => {}
            }
        }
    }

    /// Carry the routes of the running trips on over the lanes the network gained.
    pub(super) fn carry_on(&mut self, world: &World, traffic: &mut Traffic) {
        let mut keep = Vec::new();
        for mut run in std::mem::take(&mut self.running) {
            let Some(ci) = traffic.cars.iter().position(|c| c.id == run.car) else {
                continue;
            };
            let last = traffic.cars[ci].state.route.last().copied();
            Self::add_twins(traffic, &run.steps[run.next.saturating_sub(1)..]);
            let slots = self.slots(world, traffic, &run.steps[run.next..], last);
            let n = slots
                .iter()
                .position(|s| *s == Slot::Waiting)
                .unwrap_or(slots.len());
            let lanes: Vec<usize> = slots[..n]
                .iter()
                .filter_map(|s| {
                    if let Slot::Lane(l) = s {
                        Some(*l)
                    } else {
                        None
                    }
                })
                .collect();
            // (bridged from the end of what the bus has)
            let lanes = match last {
                Some(l) if !lanes.is_empty() => {
                    let with: Vec<usize> =
                        std::iter::once(l).chain(lanes.iter().copied()).collect();
                    Self::add_connectors(traffic, &with);
                    bridge_gaps(&traffic.net, &with).0[1..].to_vec()
                }
                _ => {
                    Self::add_connectors(traffic, &lanes);
                    bridge_gaps(&traffic.net, &lanes).0
                }
            };
            if !lanes.is_empty() {
                let base = traffic.cars[ci].state.route.len();
                let mut stops = Vec::new();
                let mut from = 0;
                for (si, (sid, t_dep)) in run.stations.iter().enumerate() {
                    if run.served[si] {
                        continue;
                    }
                    let Some((pos, _)) = world.object_positions.lock().get(sid).copied() else {
                        continue;
                    };
                    if let Some((ri, ss, lat)) =
                        project_stop(&traffic.net, &lanes, pos, Some(STOP_REACH), from)
                    {
                        from = ri;
                        stops.push((
                            base + ri,
                            ss,
                            bay_offset(lat),
                            *t_dep,
                            *sid,
                            world.stop_side(*sid),
                        ));
                        run.served[si] = true;
                    }
                }
                let (ty, rail) = (
                    traffic.cars[ci].vehicle.ty.clone(),
                    traffic.cars[ci].is_rail(),
                );
                place_stops(&traffic.net, &lanes, base, &mut stops, &ty, rail);
                stops.sort_by(|a, b| a.0.cmp(&b.0).then(a.1.total_cmp(&b.1)));
                log::debug!(
                    "scheduled bus {}: route carried on by {} lanes, {} more stops",
                    run.car,
                    lanes.len(),
                    stops.len()
                );
                let car = &mut traffic.cars[ci];
                car.state.route.extend(lanes);
                if let Some(b) = car.bus.as_mut() {
                    b.stops
                        .extend(stops.into_iter().map(crate::bus_service::Stop::from_tuple));
                }
                // (it may have stood waiting at the end of what it had)
                car.state.planned_next = None;
                car.state.plan_next(&traffic.net);
            }
            run.next += n;
            if run.next < run.steps.len() {
                keep.push(run);
            } else {
                if let Some(b) = traffic.cars[ci].bus.as_mut() {
                    b.route_open = false;
                }
            }
        }
        self.running = keep;
    }
}
