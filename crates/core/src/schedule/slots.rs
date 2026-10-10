//! The lanes of a trip: slots, special stops, and which vehicle runs which departure.

use super::*;

impl Schedule {
    /// The steps as the loaded network has them, each lane's direction chosen so that it
    /// follows the lane before it (`prev` for the first) and leads into the one after it.
    /// Taking whichever direction came first - as the first step used to, with nothing
    /// before it - sent the route (and the navigator) the wrong way along a two-way street.
    pub(super) fn slots(
        &self,
        world: &World,
        traffic: &Traffic,
        steps: &[Step],
        prev: Option<usize>,
    ) -> Vec<Slot> {
        let net = traffic.net();
        let keys: Vec<Option<LaneKey>> = steps.iter().map(|st| st.key).collect();
        let compiled = compile_route(
            net,
            &keys,
            |tile| {
                if traffic.has_lane_tile(tile) {
                    TileState::Loaded
                } else if world.has_tile(tile) {
                    TileState::InMap
                } else {
                    TileState::Unknown
                }
            },
            prev.map(LaneId),
        );
        let out: Vec<Slot> = compiled
            .steps
            .iter()
            .map(|s| match s {
                RouteStepState::Lane(l) => Slot::Lane(l.index()),
                RouteStepState::PendingTiles => Slot::Waiting,
                RouteStepState::Missing => Slot::Absent,
            })
            .collect();
        if ::legacy_config::env::var_os("OMSI_DEBUG_ROUTES").is_some() {
            // where consecutive lanes of the route do not join (a gap, or a change within the
            // same spline, which is a lane change)
            let lanes: Vec<usize> = out
                .iter()
                .filter_map(|s| {
                    if let Slot::Lane(l) = s {
                        Some(*l)
                    } else {
                        None
                    }
                })
                .collect();
            for (k, w) in lanes.windows(2).enumerate() {
                let (a, b) = (&net.lanes[w[0]], &net.lanes[w[1]]);
                let gap = (b.start() - a.end()).truncate().length();
                if gap > 2.0
                    || a.key.map(|k| (k.tile, k.id, k.path))
                    == b.key.map(|k| (k.tile, k.id, k.path))
                {
                    log::info!(
                        "route: step {k}: lane {} {:?} rev {} -> lane {} {:?} rev {}: gap {gap:.1} m, linked {}, lane change {}",
                        w[0],
                        a.key,
                        a.reversed,
                        w[1],
                        b.key,
                        b.reversed,
                        a.next.contains(&w[1]),
                        net.parallel(w[0], w[1])
                    );
                }
            }
        }
        let waiting = out.iter().filter(|s| **s == Slot::Waiting).count();
        let absent = out.iter().filter(|s| **s == Slot::Absent).count();
        if absent > 0 || ::legacy_config::env::var_os("OMSI_DEBUG_TRAFFIC").is_some() {
            log::debug!(
                "route: {} of {} steps on loaded lanes, {waiting} on tiles still to come, {absent} not in the map",
                out.len() - waiting - absent,
                out.len()
            );
        }
        out
    }

    /// Upload the vehicles of the buses on the road at `day_time` and of the departures of the
    /// next minutes before the first frame, so that they spawn without a hitch; the rest of
    /// the fleet follows as its departures come near (see [`Schedule::tick`]). Uploading the
    /// whole fleet in every paint scheme up front took 109 sets and 640 MB on Ahlheim.
    ///
    /// Every type of the fleet is started once, which reads the files its scripts and
    /// displays need: done on the first bus of a type that comes along, those were frames of
    /// 60 to 170 ms in the middle of a drive.
    pub fn precache(
        &mut self,
        world: &World,
        renderer: &Renderer,
        scene: &mut Scene,
        traffic: Option<&mut crate::traffic::Traffic>,
        day_time: f64,
    ) {
        let t0 = std::time::Instant::now();
        let Some(t) = traffic else { return };
        let sets = self.upcoming_sets(world, t, day_time);
        // a few sets at a time: read on the workers, uploaded, and the copies let go
        let mut most_held = 0usize;
        for chunk in sets.chunks(3) {
            most_held = most_held.max(world.prefetch_vehicle_sets(renderer, chunk));
            for (ty, scheme) in chunk {
                world.precache_vehicle(renderer, scene, ty, *scheme);
            }
        }
        let t1 = std::time::Instant::now();
        let mut seen = std::collections::HashSet::new();
        for (ty, _, hof) in self.depots.values().flatten() {
            if !seen.insert(ty.def.path.clone()) {
                continue;
            }
            crate::traffic::warm_up(world, ty, hof.clone());
            t.prime_pull_out_room(ty, true);
            for (tr, _) in t.trailer_chain(ty) {
                if seen.insert(tr.def.path.clone()) {
                    crate::traffic::warm_up(world, &tr, None);
                }
            }
        }
        let pooled: Vec<Arc<VehicleType>> = self.pools.values().flatten().cloned().collect();
        for ty in &pooled {
            if seen.insert(ty.def.path.clone()) {
                crate::traffic::warm_up(world, ty, None);
            }
        }
        // what was read ahead and not used (textures of variants the AI never shows)
        world.forget_prefetched();
        crate::release_free_memory();
        self.fleet_check = day_time;
        log::info!(
            "timetable fleet: {} vehicle/paint sets of the first {:.0} minutes read and uploaded in {:.1} s (at most {:.0} MB read ahead at once), a first start of every type in {:.1} s",
            sets.len(),
            fleet_ahead() / 60.0,
            (t1 - t0).as_secs_f32(),
            most_held as f64 / 1e6,
            t1.elapsed().as_secs_f32()
        );
    }

    /// When departure `i`'s bus is at its trip's stations.
    /// The stations departure `i` serves whoever wants them or not (`[profile_otherstopping]`
    /// 1 or 4), and those it serves when it would be early (3), by object id.
    pub(super) fn special_stops(&self, i: usize) -> (Vec<i64>, Vec<i64>) {
        let stations = trip_stations(&self.data.trips[self.departures[i].trip]);
        let kinds = &self.times_of(i).kinds;
        let of = |want: &[u8]| -> Vec<i64> {
            stations
                .iter()
                .zip(kinds)
                .filter(|(_, k)| want.contains(k))
                .map(|(id, _)| *id)
                .collect()
        };
        (of(&[1, 4]), of(&[3]))
    }

    pub(super) fn times_of(&self, i: usize) -> &TripTimes {
        let d = &self.departures[i];
        &self.times[d.trip][d.profile]
    }

    /// A 64-bit hash of a departure's tour (its group, line and tour number).
    pub(super) fn tour_key(&self, i: usize) -> u64 {
        let d = &self.departures[i];
        tour_key_of(&d.ai_group.to_ascii_lowercase(), &d.line, &d.tour)
    }

    /// The vehicle departure `i` is driven with (see [`Choice`]).
    pub(super) fn choose(&mut self, i: usize, world: &World) -> Option<Choice> {
        let group = self.departures[i].ai_group.to_ascii_lowercase();
        let h = self.tour_key(i);
        // trains: the group lists .zug files instead of depot vehicles
        let train = self
            .trains
            .get(&group)
            .and_then(|t| t.get((h % t.len().max(1) as u64) as usize).cloned());
        let (ty, numbers, hof): (
            Arc<VehicleType>,
            Vec<::map::DepotEntry>,
            Option<Arc<::legacy_vehicle::Hof>>,
        ) = match (&train, self.depots.get(&group).filter(|v| !v.is_empty())) {
            (Some(cars), _) => (cars[0].0.clone(), Vec::new(), None),
            (None, Some(vehicles)) => {
                // The depot's types come out in proportion to their fleets: a typgroup
                // listing 40 fleet numbers appears eight times as often as one with
                // 5, as in OMSI - a plain round robin gave the single MB O305 of a
                // depot the same share as the whole SD200 fleet.
                // A tour's vehicle for the day: the one `car_use` gives it, else one drawn now
                // and kept (a fleet number no other tour has, while there are any left).
                let (k, j) = match self.tour_vehicle.get(&h) {
                    Some(&kj) => kj,
                    None => {
                        let weights: Vec<usize> =
                            vehicles.iter().map(|(_, n, _)| n.len().max(1)).collect();
                        let total: usize = weights.iter().sum();
                        let mut x = (h % total.max(1) as u64) as usize;
                        let mut k = 0usize;
                        for (i, w) in weights.iter().enumerate() {
                            if x < *w {
                                k = i;
                                break;
                            }
                            x -= w;
                        }
                        let nums = &vehicles[k].1;
                        let start = ((h >> 21) % nums.len().max(1) as u64) as usize;
                        let free = (0..nums.len())
                            .map(|o| (start + o) % nums.len())
                            .find(|&j| {
                                !self
                                    .used_numbers
                                    .contains(&(group.clone(), nums[j].number.trim().to_string()))
                            });
                        let j = free.unwrap_or(start);
                        if let Some(n) = nums.get(j) {
                            self.used_numbers
                                .insert((group.clone(), n.number.trim().to_string()));
                        }
                        self.tour_vehicle.insert(h, (k, j));
                        (k, j)
                    }
                };
                let (ty, numbers, hof) = &vehicles[k.min(vehicles.len() - 1)];
                let numbers = numbers.get(j).cloned().into_iter().collect::<Vec<_>>();
                (ty.clone(), numbers, hof.clone())
            }
            _ => {
                // a plain [aigroup_2] flies/drives its own vehicles (the Tegel approach)
                let root = world.root.clone();
                let pool = self.pool(&root, world, &group);
                (
                    pool.get((h % pool.len().max(1) as u64) as usize).cloned()?,
                    Vec::new(),
                    None,
                )
            }
        };
        // A depot bus as Omsi.exe makes it (0x70a174): the fleet number of its ailists line;
        // the plate of that line, else - unless the bus's plates are free - the plate the bus
        // gives the number ([registration_list] / [registration_automatic]); and the repaint
        // that line names, else the model's own paint (the first repaint when the default
        // paint is "<nouse>"). Another tour's bus draws a repaint at random, as random
        // traffic does.
        let entry = numbers.first().cloned();
        let number = entry.as_ref().map(|e| {
            let plate = if !e.registration.trim().is_empty() {
                e.registration.clone()
            } else if ty.def.registration_mode != 1 {
                ty.def.plate_of_number(&e.number)
            } else {
                String::new()
            };
            (e.number.clone(), plate)
        });
        let scheme = if ty.paint_schemes.is_empty() {
            None
        } else if let Some(e) = &entry {
            ty.paint_schemes
                .iter()
                .position(|s| s.name.trim_end() == e.paint.trim_end())
                .or_else(|| (ty.def.default_paint.trim() == "<nouse>").then_some(0))
        } else {
            Some(
                ((h >> 42) % ty.paint_schemes.len().min(crate::traffic::AI_SCHEMES) as u64)
                    as usize,
            )
        };
        Some(Choice {
            ty,
            number,
            hof,
            scheme,
            train,
        })
    }

    /// The vehicle sets a choice is drawn with: the vehicle, its rear sections, a train's
    /// further cars.
    pub(super) fn choice_sets(c: &Choice, traffic: &mut Traffic) -> Vec<(Arc<VehicleType>, Option<usize>)> {
        let mut out = vec![(c.ty.clone(), c.scheme)];
        for (t, _) in traffic.trailer_chain(&c.ty) {
            let s = c.scheme.filter(|i| *i < t.paint_schemes.len());
            out.push((t, s));
        }
        if let Some(cars) = &c.train {
            out.extend(cars.iter().skip(1).map(|(t, _)| (t.clone(), None)));
        }
        out
    }

    /// The vehicle sets of the trips on the road at `day_time` and of the departures of the
    /// next [`FLEET_AHEAD`] seconds.
    pub(super) fn upcoming_sets(
        &mut self,
        world: &World,
        traffic: &mut Traffic,
        day_time: f64,
    ) -> Vec<(Arc<VehicleType>, Option<usize>)> {
        let mut out = Vec::new();
        let mut seen: HashSet<crate::scene::VehicleKey> = HashSet::new();
        let end = self
            .departures
            .partition_point(|d| d.time <= day_time - self.day_base + fleet_ahead());
        for i in 0..end {
            if self.is_player_tour(i) || !self.runs(i) {
                continue;
            }
            if self.dep_time(i) + self.times_of(i).duration < day_time {
                continue;
            }
            let Some(c) = self.choose(i, world) else {
                continue;
            };
            for (ty, scheme) in Self::choice_sets(&c, traffic) {
                if seen.insert((ty.def.path.clone(), scheme)) {
                    out.push((ty, scheme));
                }
            }
        }
        out
    }
}
