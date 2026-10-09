//! Putting a scheduled bus on its track.

use super::*;

impl Schedule {
    /// Put departure `i` on the road where its bus is at `day_time`: on the part of the route
    /// the loaded tiles have, which is carried on as more tiles come.
    ///
    /// With `onto`, the timetable bus of that index (the tour's bus, at the end of its
    /// previous trip) takes the trip on instead of a new one.
    #[allow(clippy::too_many_arguments)]
    pub(super) fn spawn_departure(
        &mut self,
        i: usize,
        world: &World,
        traffic: &mut Traffic,
        renderer: &Renderer,
        scene: &mut Scene,
        day_time: f64,
        onto: Option<usize>,
    ) -> Placed {
        let profile = ::legacy_config::env::var_os("OMSI_PROFILE").is_some();
        let t_spawn = std::time::Instant::now();
        let trip = &self.data.trips[self.departures[i].trip];
        let trip_name = trip.name.clone();
        // (the [station] records of a type-1 trip as well: Novi Sad's buses have no others,
        // and without them they drove past every stop)
        let stations = trip_stations(trip);
        // the timetable's times at the stations (see `TripTimes`)
        let departure = self.dep_time(i);
        let tt = self.times_of(i).clone();
        let duration = tt.duration;
        if day_time - departure > duration && onto.is_none() {
            return Placed::Drop; // already arrived
        }
        let arrive: Vec<f64> = tt.stations.iter().map(|s| departure + s.0).collect();
        let leave: Vec<f64> = tt.stations.iter().map(|s| departure + s.1).collect();
        let (steps, track) = self.steps_of(&trip_name, &stations);
        Self::add_twins(traffic, &steps);
        let slots = self.slots(world, traffic, &steps, None);
        if !slots.iter().any(|s| matches!(s, Slot::Lane(_))) && !slots.contains(&Slot::Waiting) {
            log::debug!("trip {trip_name}: no route");
            return Placed::Drop;
        }
        // the leg the bus is on now, and how far along it (a layover bus stands at the start)
        let legs = if track {
            1
        } else {
            stations.len().saturating_sub(1).max(1)
        };
        let leg_time = |k: usize| {
            if track {
                (departure, departure + duration)
            } else {
                (
                    leave[k],
                    arrive.get(k + 1).copied().unwrap_or(departure + duration),
                )
            }
        };
        // (the tour's own bus taking the trip on starts it at its first stop, however late)
        let leg = (0..legs)
            .rev()
            .find(|&k| leg_time(k).0 <= day_time)
            .filter(|_| onto.is_none())
            .unwrap_or(0);
        let (t0, t1) = leg_time(leg);
        let frac = if day_time <= departure || onto.is_some() {
            0.0
        } else {
            ((day_time - t0) / (t1 - t0).max(1e-3)).clamp(0.0, 1.0)
        };
        // lengths of the steps: a step still to come counts as long as an average one
        let net = &traffic.net;
        let known: Vec<f64> = slots
            .iter()
            .filter_map(|s| {
                if let Slot::Lane(l) = s {
                    Some(net.lanes[*l].length() as f64)
                } else {
                    None
                }
            })
            .collect();
        let average = if known.is_empty() {
            40.0
        } else {
            known.iter().sum::<f64>() / known.len() as f64
        };
        let est: Vec<f64> = slots
            .iter()
            .map(|slot| match slot {
                Slot::Lane(l) => net.lanes[*l].length() as f64,
                Slot::Waiting => average,
                Slot::Absent => 0.0,
            })
            .collect();
        let Some((at, offset)) = step_at(&steps, &slots, &est, leg, frac) else {
            return Placed::Drop;
        };
        if slots[at] == Slot::Waiting {
            // when it reaches the next loaded step, at the pace of its leg
            let in_leg = |k: usize| track || steps[k].leg == leg;
            let leg_len: f64 = (0..steps.len())
                .filter(|&k| in_leg(k))
                .map(|k| est[k])
                .sum();
            let rate = (t1 - t0).max(0.0) / leg_len.max(1e-3);
            let base = day_time.max(departure);
            let retry = match (at + 1..slots.len()).find(|&k| matches!(slots[k], Slot::Lane(_))) {
                Some(k) if in_leg(k) => {
                    let ahead =
                        (est[at] - offset).max(0.0) + (at + 1..k).map(|j| est[j]).sum::<f64>();
                    base + ahead * rate
                }
                // on a later leg (or nowhere): look again when this leg is over
                _ => t1.max(base),
            };
            log::debug!(
                "trip {trip_name}: the bus is on a tile that is not loaded (again at {:.2} min)",
                retry / 60.0
            );
            self.retry_at.insert(i, retry);
            return Placed::Wait;
        }
        // the part of the route around the bus that the network has
        let (start, end) = section_around(&slots, at);
        let whole = start == 0 && end == slots.len();
        let lane_of = |s: &Slot| {
            if let Slot::Lane(l) = s {
                Some(*l)
            } else {
                None
            }
        };
        let section: Vec<usize> = slots[start..end].iter().filter_map(lane_of).collect();
        let start_index = slots[start..at].iter().filter_map(lane_of).count();
        Self::add_connectors(traffic, &section);
        let net = &traffic.net;
        let (section, index) = bridge_gaps(net, &section);
        let start_index = index[start_index.min(index.len() - 1)];
        let mut s = offset.min(net.lanes[section[start_index]].length() as f64) as f32;
        // stations → stop points on that part of the route
        let reach = if whole { None } else { Some(STOP_REACH) };
        let mut served = vec![false; stations.len()];
        let mut stops = Vec::new();
        let mut from = 0;
        for (si, sid) in stations.iter().enumerate() {
            // a station the trip runs through is none of its stops
            if !tt.stops[si] {
                served[si] = true;
                continue;
            }
            // the stations of the legs behind the bus are passed
            if !track && (si < leg || (si == leg && frac > 0.0)) {
                served[si] = true;
            }
            let found = world.object_positions.lock().get(sid).copied();
            match found {
                Some((pos, _)) => match project_stop(net, &section, pos, reach, from) {
                    Some((ri, ss, lat)) => {
                        from = ri;
                        served[si] = true;
                        stops.push((
                            ri,
                            ss,
                            bay_offset(lat),
                            leave[si],
                            *sid,
                            world.stop_side(*sid),
                        ));
                    }
                    None => log::debug!("station {sid}: not near the route"),
                },
                None => log::debug!("station {sid}: object not in the map"),
            }
        }
        stops.sort_by(|a, b| a.0.cmp(&b.0).then(a.1.total_cmp(&b.1)));
        if ::legacy_config::env::var_os("OMSI_DEBUG_TRAFFIC").is_some() {
            let len: f32 = section.iter().map(|&l| net.lanes[l].length()).sum();
            log::info!(
                "trip {trip_name}: {} stations {:?}, route {} of {} steps ({} lanes, {len:.0} m) from step {start}, bus on step {at} (leg {leg}, {:.0} %), stops {:?}",
                stations.len(),
                stations,
                end - start,
                steps.len(),
                section.len(),
                frac * 100.0,
                stops
            );
        }
        let t_route = t_spawn.elapsed();
        if let Some(ci) = onto {
            // a train whose next trip runs the other way (its `[trainreverse]` is not how the
            // train stands) is turned round where it stands, as Omsi.exe turns it when the
            // trip begins (0x613a98): its last car leads, on the way back. (It drove off
            // along the siding instead - past the end of the track - and another train
            // appeared for the trip.)
            let reverse = self.data.trips[self.departures[i].trip].train_reverse;
            if traffic.cars[ci].is_rail() && traffic.cars[ci].consist_reversed != reverse {
                let c = &traffic.cars[ci];
                let tail = c
                    .vehicle
                    .trailers
                    .last()
                    .map(|t| t.position)
                    .unwrap_or(c.vehicle.position);
                let net = &traffic.net;
                let found = section
                    .iter()
                    .enumerate()
                    .take(24)
                    .filter_map(|(k, &l)| {
                        net.lanes[l].nearest_point(tail).map(|(s, d)| (k, l, s, d))
                    })
                    .min_by(|a, b| a.3.total_cmp(&b.3));
                match found {
                    Some((k, l, s, d)) if d < 2.5 => {
                        traffic.turn_train(
                            world,
                            renderer,
                            scene,
                            ci,
                            l,
                            s,
                            &section[..k],
                            reverse,
                        );
                    }
                    _ => {
                        if ::legacy_config::env::var_os("OMSI_DEBUG_TRAFFIC").is_some() {
                            log::info!(
                                "trip {trip_name}: train {} would turn round, but its last car at ({:.1}, {:.1}) is not on the trip's way (nearest {:?}); its front at ({:.1}, {:.1}) on lane {}",
                                c.id,
                                tail.x,
                                tail.y,
                                found.map(|f| (f.0, f.1, f.2, f.3)),
                                c.vehicle.position.x,
                                c.vehicle.position.y,
                                c.state.lane
                            );
                            for &l in section.iter().take(4).chain(std::iter::once(&c.state.lane)) {
                                let ln = &net.lanes[l];
                                log::info!(
                                    "  lane {l} {:?} len {:.1} ({:.1}, {:.1}) -> ({:.1}, {:.1}) next {:?} rev {}",
                                    ln.key,
                                    ln.length(),
                                    ln.start().x,
                                    ln.start().y,
                                    ln.end().x,
                                    ln.end().y,
                                    ln.next,
                                    ln.reversed
                                );
                            }
                        }
                    }
                }
            }
            let (ty, rail) = (
                traffic.cars[ci].vehicle.ty.clone(),
                traffic.cars[ci].is_rail(),
            );
            place_stops(&traffic.net, &section, 0, &mut stops, &ty, rail);
            // the tour's bus that has just finished its trip takes this one on from where
            // it stands: the section itself when it stands on it, else the shortest way
            // from its lane onto one of the section's first lanes (round a terminal loop)
            let (lane0, s0) = (traffic.cars[ci].state.lane, traffic.cars[ci].state.s);
            let net = &traffic.net;
            let (prefix, from) = match section.iter().position(|&l| l == lane0) {
                Some(r) => (Vec::new(), r),
                None => {
                    let mut best: Option<(f32, Vec<usize>, usize)> = None;
                    for t in 0..section.len().min(4) {
                        if let Some(p) = net.shortest_path(lane0, section[t]) {
                            let len = p[..p.len() - 1]
                                .iter()
                                .map(|&l| net.lanes[l].length())
                                .sum::<f32>()
                                - s0;
                            if len < 400.0 && best.as_ref().map(|b| len < b.0).unwrap_or(true) {
                                best = Some((len, p, t));
                            }
                        }
                    }
                    match best {
                        Some((_, p, t)) => (p[..p.len() - 1].to_vec(), t),
                        None => {
                            log::debug!(
                                "trip {trip_name}: the tour's bus has no way from where it stands"
                            );
                            if ::legacy_config::env::var_os("OMSI_DEBUG_TRAFFIC").is_some() {
                                let ln = &net.lanes[lane0];
                                log::info!(
                                    "trip {trip_name}: tour bus on lane {lane0} {:?} at s {s0:.1} of {:.1}, ({:.1}, {:.1}) -> ({:.1}, {:.1}), next {:?}",
                                    ln.key,
                                    ln.length(),
                                    ln.start().x,
                                    ln.start().y,
                                    ln.end().x,
                                    ln.end().y,
                                    ln.next
                                );
                                for &l in section.iter().take(4) {
                                    let ln = &net.lanes[l];
                                    log::info!(
                                        "  trip lane {l} {:?} len {:.1} ({:.1}, {:.1}) -> ({:.1}, {:.1}) prev? next {:?}",
                                        ln.key,
                                        ln.length(),
                                        ln.start().x,
                                        ln.start().y,
                                        ln.end().x,
                                        ln.end().y,
                                        ln.next
                                    );
                                }
                            }
                            return Placed::Drop;
                        }
                    }
                }
            };
            let route: Vec<usize> = prefix
                .iter()
                .copied()
                .chain(section[from..].iter().copied())
                .collect();
            let shift = prefix.len() as isize - from as isize;
            // the stops from the bus on; one just behind it on its lane is where it stands
            let stops: Vec<(usize, f32, f32, f64, i64, f32)> = stops
                .into_iter()
                .filter(|st| st.0 >= from)
                .filter_map(|(ri, ss, lat, t, id, side)| {
                    let nri = (ri as isize + shift) as usize;
                    if nri == 0 && ss <= s0 + 0.3 {
                        (s0 - ss < 25.0).then_some((0, s0 + 0.3, 0.0, t, id, side))
                    } else {
                        Some((nri, ss, lat, t, id, side))
                    }
                })
                .collect();
            let layover = departure > day_time;
            let n_stops = stops.len();
            traffic.reroute(ci, route, s0, stops, layover);
            let line = self.display_line(i);
            let terminus = self.data.trips[self.departures[i].trip].terminus.clone();
            let names = self.trip_stop_names(self.departures[i].trip);
            let last_stop = trip_stations(&self.data.trips[self.departures[i].trip])
                .last()
                .copied();
            let (always, early) = self.special_stops(i);
            let car = &mut traffic.cars[ci];
            if let Some(k) = car.vehicle.ty.program.str_var("Linie") {
                car.vehicle.state.str_vars[k as usize] = line.clone();
            }
            let hof = car.vehicle.host.hof.clone();
            let names: Vec<&str> = names.iter().map(String::as_str).collect();
            set_ai_destination(&mut car.vehicle, hof.as_deref(), &line, &terminus, &names);
            if let Some(b) = car.bus.as_mut() {
                b.route_open = end < slots.len();
                b.terminus = terminus.clone();
                b.last_stop = last_stop;
                b.always = always;
                b.serve_early = early;
            }
            let id = car.id;
            self.car_departure.insert(id, i);
            self.running.retain(|r| r.car != id);
            if end < slots.len() {
                self.running.push(RunningTrip {
                    car: id,
                    steps,
                    next: end,
                    stations: stations
                        .iter()
                        .copied()
                        .zip(leave.iter().copied())
                        .collect(),
                    served,
                });
            }
            log::info!(
                "scheduled bus {id}: line {line} tour {} goes on with trip {trip_name} to {} at {:.1} min (leaves {:.1} min), {n_stops} stops{}",
                self.departures[i].tour,
                terminus.trim(),
                day_time / 60.0,
                departure / 60.0,
                if prefix.is_empty() {
                    String::new()
                } else {
                    format!(", {} lanes to its first stop", prefix.len())
                }
            );
            return Placed::Spawned;
        }
        let Some(Choice {
                     ty,
                     number,
                     hof,
                     scheme,
                     train,
                 }) = self.choose(i, world)
        else {
            log::warn!(
                "trip {trip_name}: no vehicles for AI group '{}'",
                self.departures[i].ai_group
            );
            return Placed::Drop;
        };
        self.next_number += 1;
        let rail =
            traffic.net.lanes[section[start_index]].kind == ::simulation::traffic::LaneKind::Rail;
        // every further car of the train with the cars of its unit, as Omsi.exe creates
        // each car of a `.zug` (the first has its own with `create_car`): the ones before it
        // (towards the front of the train), the car, the ones behind it
        let rest: Option<Vec<(Arc<VehicleType>, bool)>> = train.as_ref().map(|cars| {
            let mut rest = Vec::new();
            for (t, rev) in &cars[1..] {
                let mut front = traffic.coupled_chain(t, *rev, false);
                front.reverse();
                rest.extend(front);
                rest.push((t.clone(), *rev));
                rest.extend(traffic.coupled_chain(t, *rev, true));
            }
            rest
        });
        // a trip that runs the train turned round (`[trainreverse]`): its last car leads
        let turned: Option<Vec<(Arc<VehicleType>, bool)>> =
            (self.data.trips[self.departures[i].trip].train_reverse && rail).then(|| {
                let mut all = vec![(ty.clone(), false)];
                all.extend(traffic.trailer_chain(&ty));
                all.extend(rest.clone().unwrap_or_default());
                all.into_iter().rev().map(|(t, r)| (t, !r)).collect()
            });
        // where the one that leads comes to rest at a station
        let lead_ty = turned
            .as_ref()
            .map(|t| t[0].0.clone())
            .unwrap_or_else(|| ty.clone());
        place_stops(&traffic.net, &section, 0, &mut stops, &lead_ty, rail);
        log::debug!(
            "spawn trip {trip_name}: departure {:.2} min, now {:.2} min, leg {leg} at {:.0} %, step {at} of {}, start {s:.0} m into its lane",
            departure / 60.0,
            day_time / 60.0,
            frac * 100.0,
            steps.len()
        );
        // the bus starts on its step's lane; the stops behind it are dropped
        let mut start_index = start_index;
        while start_index + 1 < section.len()
            && s > traffic.net.lanes[section[start_index]].length()
        {
            s -= traffic.net.lanes[section[start_index]].length();
            start_index += 1;
        }
        // a bus that would start a few metres short of its next stop stands at it (half a
        // metre short, so that it is served): starting before it, it had to pull over into
        // the stop - often a lane over - in less than its own length
        if let Some(&(ri, ss, _, _, _, _)) = stops
            .iter()
            .find(|st| st.0 > start_index || (st.0 == start_index && st.1 > s))
        {
            let mut d = ss - s;
            for k in start_index..ri {
                if !traffic.net.parallel(section[k], section[k + 1]) {
                    d += traffic.net.lanes[section[k]].length();
                }
            }
            if d < 25.0 {
                start_index = ri;
                s = (ss - 0.5).max(0.0);
            }
        }
        let at_pos = traffic.net.lanes[section[start_index]].at(s).0;
        // lanes stay in the network when their tile is unloaded: nothing is put on ground
        // that is not there (the departure waits for its tile)
        if !track_is_air(traffic, section[start_index]) && !world.has_ground(at_pos.x, at_pos.y) {
            log::debug!("trip {trip_name}: the bus would stand on an unloaded tile");
            return Placed::Wait;
        }
        // not into a vehicle that happens to be there (the player's bus at its stop, a car),
        // nor just in front of one driving up to that place: try again in a moment
        let at_heading = traffic.net.lanes[section[start_index]].at(s).1 as f64;
        if traffic.blocked(&ty, at_pos, at_heading) || !traffic.spawn_clear(&ty, at_pos, at_heading)
        {
            log::debug!("trip {trip_name}: a vehicle stands where the bus would appear");
            return Placed::Busy;
        }
        // A bus on its layover waits at the stand only when no other bus stands there or
        // pulls in; otherwise it comes at its departure time. (A stand shared by four lines
        // had five buses queueing in the road for a quarter of an hour, and every bus
        // serving the stop and every car behind them waiting as well.)
        if departure > day_time + 30.0
            && traffic
            .cars
            .iter()
            .any(|c| c.is_bus() && !c.gone && (c.vehicle.position - at_pos).length() < 50.0)
        {
            log::debug!(
                "trip {trip_name}: its stand is taken, the bus comes at its departure time"
            );
            self.departures[i].spawned = false;
            self.startup.remove(&i);
            self.later_layover.insert(i);
            return Placed::Drop;
        }
        // nobody may see a bus appear (except while the map loads)
        // (the map-loading exemption holds only while the world is being built: a departure
        // of that moment that had to wait popped up in plain view minutes later)
        if !(self.startup.contains(&i) && traffic.loading_phase())
            && !traffic.may_appear(world, at_pos)
        {
            log::debug!("trip {trip_name}: the bus would appear in view");
            return Placed::Busy;
        }
        self.startup.remove(&i);
        let stops: Vec<(usize, f32, f32, f64, i64, f32)> = stops
            .into_iter()
            .filter(|(ri, ss, _, _, _, _)| *ri > start_index || (*ri == start_index && *ss > s))
            .map(|(ri, ss, lat, t, id, side)| (ri - start_index, ss, lat, t, id, side))
            .collect();
        let route: Vec<usize> = section[start_index..].to_vec();
        // the trip's own line (" 5"), which is what the displays show; the timetable line's
        // name ("5 & 5N") only groups the tours
        let trip_line = self.data.trips[self.departures[i].trip]
            .line
            .trim()
            .to_string();
        let line = if trip_line.is_empty() {
            self.departures[i].line.clone()
        } else {
            trip_line
        };
        let tour = self.departures[i].tour.clone();
        let terminus = self.data.trips[self.departures[i].trip].terminus.clone();
        let Some(ci) = traffic.spawn_bus(
            world,
            renderer,
            scene,
            lead_ty.clone(),
            route,
            s,
            stops,
            number.clone(),
            hof.clone(),
            Some(scheme),
        ) else {
            return Placed::Drop;
        };
        self.car_departure.insert(traffic.cars[ci].id, i);
        if let Some(t) = &turned {
            traffic.set_trailers(world, renderer, scene, ci, &t[1..]);
            traffic.cars[ci].consist_reversed = true;
        } else if let Some(rest) = &rest {
            traffic.attach_cars(world, renderer, scene, ci, rest);
        }
        if train.is_some() {
            log::info!(
                "train: {}",
                std::iter::once(
                    traffic.cars[ci]
                        .vehicle
                        .ty
                        .def
                        .path
                        .file_stem()
                        .unwrap_or_default()
                        .to_string_lossy()
                        .to_string()
                )
                .chain(traffic.cars[ci].vehicle.trailers.iter().map(|t| {
                    format!(
                        "{}{}",
                        t.ty.def
                            .path
                            .file_stem()
                            .unwrap_or_default()
                            .to_string_lossy(),
                        if t.reversed { " (turned)" } else { "" }
                    )
                }))
                .collect::<Vec<_>>()
                .join(" + ")
            );
            traffic.cars[ci].state.max_speed_kmh = 90.0;
            traffic.cars[ci].state.length =
                20.0 * (1 + traffic.cars[ci].vehicle.trailers.len()) as f32;
        }
        let names = self.trip_stop_names(self.departures[i].trip);
        let names: Vec<&str> = names.iter().map(String::as_str).collect();
        let last_stop = trip_stations(&self.data.trips[self.departures[i].trip])
            .last()
            .copied();
        let (always, early) = self.special_stops(i);
        let car = &mut traffic.cars[ci];
        // on its layover only when it stands at its first stop now (the trip's first station
        // may lie on a part of the track that is not loaded): it waits there for its departure
        if let Some(b) = car.bus.as_mut() {
            b.layover = departure > day_time
                && b.stops
                .front()
                .map(|st| st.ri == 0 && (st.s - s).abs() < 2.0)
                .unwrap_or(false);
            b.route_open = end < slots.len();
            b.terminus = terminus.clone();
            b.last_stop = last_stop;
            b.always = always;
            b.serve_early = early;
        }
        // the bus scripts read the line/terminus for their displays
        if let Some(i) = ty.program.str_var("Linie") {
            car.vehicle.state.str_vars[i as usize] = line.clone();
        }
        set_ai_destination(&mut car.vehicle, hof.as_deref(), &line, &terminus, &names);
        log::info!(
            "scheduled bus: line {line} tour {tour} trip {trip_name} {} #{:?} at {:.1} min, {} stops, at ({:.1}, {:.1}) heading {:.0}{}",
            ty.def.type_name,
            number.as_ref().map(|n| format!(
                "{} plate {:?} paint {:?}",
                n.0,
                n.1,
                scheme
                    .and_then(|i| ty.paint_schemes.get(i))
                    .map(|p| p.name.as_str())
            )),
            day_time / 60.0,
            car.bus.as_ref().map(|b| b.stops.len()).unwrap_or(0),
            car.vehicle.position.x,
            car.vehicle.position.y,
            car.vehicle.heading,
            if end < slots.len() {
                format!(", route {} of {} steps so far", end - start, steps.len())
            } else {
                String::new()
            }
        );
        if end < slots.len() {
            self.running.push(RunningTrip {
                car: car.id,
                steps,
                next: end,
                stations: stations
                    .iter()
                    .copied()
                    .zip(leave.iter().copied())
                    .collect(),
                served,
            });
        }
        if profile {
            log::info!(
                "  spawn took {:.1} ms (route {:.1} ms), {} pending, {} waiting",
                t_spawn.elapsed().as_secs_f64() * 1000.0,
                t_route.as_secs_f64() * 1000.0,
                self.pending.len(),
                self.waiting.len()
            );
        }
        Placed::Spawned
    }
}
