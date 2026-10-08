//! Population/spawn adapter: demand, admission, dormant lifecycle, parking and
//! the AI density/type selection, driving the `traffic::population` domain owner.

use super::*;

impl Traffic {

    /// Number of dormant (out-of-range) AI cars.
    pub fn dormant_count(&self) -> usize {
        self.dormant.len()
    }


    /// Ask for a loaded tile ahead of a route frontier, so a bus reaches loaded ground where
    /// feasible instead of stopping at the edge. Bounded and de-duplicated by the owner.
    pub fn request_topology_tile(&mut self, tile: (i32, i32)) {
        self.population.request_topology(tile);
    }


    /// World positions of the wanted route-frontier tiles, for the streamer's centres.
    pub fn topology_centers(&self) -> Vec<DVec3> {
        let ts = ::map::tile_size();
        self.population
            .topology_demand()
            .iter()
            .map(|&(tx, ty)| DVec3::new((tx as f64 + 0.5) * ts, (ty as f64 + 0.5) * ts, 0.0))
            .collect()
    }


    /// How much traffic group `g` makes now: its `unsched_trafficdens.txt` factor times its
    /// curve for this day of the week (+1 Monday to Friday, +2 Saturday, +4 Sunday, 0 every
    /// day).
    pub(crate) fn group_density(&self, g: usize) -> f32 {
        let Some(gr) = self.groups.get(g) else {
            return 0.0;
        };
        if !self.group_curves {
            return 1.0;
        }
        let bit = match self.weekday {
            0..=4 => 1,
            5 => 2,
            _ => 4,
        };
        let hour = (self.day_time.rem_euclid(86400.0) / 3600.0) as f32;
        match gr
            .densities
            .iter()
            .find(|(mask, _)| *mask == 0 || mask & bit != 0)
        {
            Some((_, curve)) => gr.factor * ::map::global::curve_at(curve, hour).max(0.0),
            None => 0.0,
        }
    }


    /// The street traffic density now, 1 = the map's normal level, times the options' share
    /// of random traffic.
    pub(crate) fn street_density(&self) -> f32 {
        self.street_density_map() * self.unsched_factor
    }


    pub(crate) fn street_density_map(&self) -> f32 {
        if !self.group_curves {
            return ::map::global::curve_at(
                &self.density_curve,
                (self.day_time.rem_euclid(86400.0) / 3600.0) as f32,
            )
                .clamp(0.0, 2.0);
        }
        let groups: Vec<usize> = (0..self.groups.len())
            .filter(|&g| {
                self.types
                    .iter()
                    .any(|t| t.3 == g && t.2 == LaneKind::Street)
            })
            .collect();
        let factors: f32 = groups.iter().map(|&g| self.groups[g].factor).sum();
        if factors <= 0.0 {
            return 0.0;
        }
        (groups.iter().map(|&g| self.group_density(g)).sum::<f32>() / factors).clamp(0.0, 2.0)
    }


    /// How much of group `g`'s traffic `lane` carries: the path's `[rule] trafficdensity`
    /// for the group, else the group's default (see `uvg_defaults`).
    pub(crate) fn lane_group_density(&self, lane: &::traffic::Lane, g: usize) -> f32 {
        match self.group_uvg.get(g).copied().flatten() {
            Some(u) => lane.pool_density(&self.uvg_defaults, u),
            None => lane.density,
        }
    }


    /// A random vehicle type for a lane of `kind`: on a street `lane`, of the groups that
    /// lane carries, as much as it carries of each.
    pub(crate) fn pick_type(&mut self, kind: LaneKind, lane: Option<usize>) -> Option<Arc<VehicleType>> {
        // a vehicle's share: its weight within its group times what the group makes now
        let group_weight: Vec<f32> = (0..self.groups.len())
            .map(|g| {
                self.types
                    .iter()
                    .filter(|t| t.3 == g && t.2 == kind)
                    .map(|t| t.1)
                    .sum::<f32>()
            })
            .collect();
        let dens: Vec<f32> = (0..self.groups.len())
            .map(|g| {
                if kind == LaneKind::Street {
                    let here = lane
                        .and_then(|i| self.net.lanes.get(i))
                        .map(|l| self.lane_group_density(l, g))
                        .unwrap_or(1.0);
                    self.group_density(g) * here
                } else {
                    1.0
                }
            })
            .collect();
        // (and only the vehicles the lane is open to: Grundorf's trucks where it says
        // `trucks`, Omsi.exe 0x71d714)
        let barred: Vec<*const VehicleType> = match lane
            .filter(|_| kind == LaneKind::Street)
            .and_then(|i| self.net.lanes.get(i))
        {
            Some(l) => self
                .types
                .iter()
                .filter(|t| !l.allows(t.0.def.ai_veh_type))
                .map(|t| Arc::as_ptr(&t.0))
                .collect(),
            None => Vec::new(),
        };
        let weight = |t: &(Arc<VehicleType>, f32, LaneKind, usize)| -> f32 {
            let gw = group_weight.get(t.3).copied().unwrap_or(0.0);
            if gw <= 0.0 || barred.contains(&Arc::as_ptr(&t.0)) {
                0.0
            } else {
                t.1 / gw * dens.get(t.3).copied().unwrap_or(0.0)
            }
        };
        let total: f32 = self.types.iter().filter(|t| t.2 == kind).map(weight).sum();
        if total <= 0.0 {
            return None;
        }
        let mut x = self.rand_f() as f32 * total;
        for t in self.types.iter().filter(|t| t.2 == kind) {
            let w = weight(t);
            if x < w {
                return Some(t.0.clone());
            }
            x -= w;
        }
        self.types
            .iter()
            .filter(|t| t.2 == kind)
            .last()
            .map(|t| t.0.clone())
    }


    /// Put parked cars onto the lanes they stand in or beside: (distance along the lane,
    /// signed lateral offset, + = right). `cars` are the ones the tiles placed since the last
    /// call; the cars no lane was found for before are tried again where the lanes `added`
    /// just came in.
    pub(crate) fn sort_parked(&mut self, cars: Vec<(DVec3, f64)>, added: std::ops::Range<usize>) {
        let mut todo: Vec<DVec3> = Vec::new();
        if !added.is_empty() && !self.parked_waiting.is_empty() {
            let cells: hashbrown::HashSet<(i32, i32)> = added
                .clone()
                .flat_map(|i| Network::lane_cells(&self.net.lanes[i]))
                .collect();
            let near = |p: &DVec3| {
                let (cx, cy) = Network::grid_cell(*p);
                (-1..=1).any(|dx| (-1..=1).any(|dy| cells.contains(&(cx + dx, cy + dy))))
            };
            let (retry, keep): (Vec<DVec3>, Vec<DVec3>) = std::mem::take(&mut self.parked_waiting)
                .into_iter()
                .partition(|p| near(p));
            self.parked_waiting = keep;
            todo = retry;
        }
        let new_cars = cars.len();
        todo.extend(cars.into_iter().map(|(p, _heading)| p));
        if todo.is_empty() {
            return;
        }
        let mut on_lanes = 0usize;
        for p in todo {
            // a car beside a lane is within a few metres of it: the lanes of the cells
            // around it are enough, and a car in a car park finds none
            let beside = self
                .net
                .nearest_lane_near(p, LaneKind::Street)
                .filter(|(l, _, d)| *d <= (self.net.lanes[*l].width * 0.5).max(1.5) as f64 + 1.6);
            let Some((l, s, _)) = beside else {
                self.parked_waiting.push(p);
                continue;
            };
            let (q, h) = self.net.lanes[l].at(s);
            let hr = (h as f64).to_radians();
            let right = DVec3::new(hr.cos(), -hr.sin(), 0.0);
            let lat = (p - q).dot(right) as f32;
            if lat.abs() < 0.9 && ::legacy_config::env::var_os("OMSI_DEBUG_TRAFFIC").is_some() {
                let lane = &self.net.lanes[l];
                log::info!(
                    "parked car at ({:.1}, {:.1}) stands in lane {l} ({} {:?}, width {:.1}, heading {:.0} there, s {s:.1} of {:.1}, {lat:+.2} m to the side)",
                    p.x,
                    p.y,
                    lane.name,
                    lane.key,
                    lane.width,
                    h,
                    lane.length()
                );
            }
            self.parked.entry(l).or_default().push((s, lat));
            on_lanes += 1;
        }
        if ::legacy_config::env::var_os("OMSI_DEBUG_TRAFFIC").is_some() {
            log::info!(
                "traffic: {new_cars} parked cars placed, {on_lanes} more stand in or beside a lane ({} in all, {} not beside one)",
                self.parked.values().map(|v| v.len()).sum::<usize>(),
                self.parked_waiting.len()
            );
        }
    }


    /// The world is still being built (the first populate, the first seconds): vehicles
    /// may be put anywhere.
    pub fn loading_phase(&self) -> bool {
        self.initial || self.time < 2.0
    }


    /// Spawn cars until `target` are within `spawn_radius` of `center`; despawn far ones.
    pub fn populate(
        &mut self,
        world: &World,
        renderer: &Renderer,
        scene: &mut Scene,
        center: DVec3,
    ) {
        self.populate_seen(world, renderer, scene, center, None);
    }


    /// Keep the population around the player: cars are taken off only where nobody sees
    /// it (far away and out of view, or behind a building), and new ones appear only there.
    /// `view` (a unit vector) stands in for the viewer when none has been set.
    pub fn populate_seen(
        &mut self,
        world: &World,
        renderer: &Renderer,
        scene: &mut Scene,
        center: DVec3,
        view: Option<DVec3>,
    ) {
        if self.mirror {
            return;
        }
        if self.viewer.is_none() {
            if let Some(f) = view {
                self.viewer = Some(Viewer {
                    pos: center,
                    forward: f,
                    tan_x: 1.2,
                    tan_y: 0.6,
                    range: VISIBLE_RANGE,
                    min_size: 0.0,
                    max_dist: 0.0,
                    fov: 1.0,
                });
            }
        }
        let far = self.spawn_radius * DESPAWN_FACTOR;
        self.advance_dormant();
        // the cars somebody stands behind
        let queued: std::collections::HashSet<VehicleId> = self
            .cars
            .iter()
            .filter(|c| c.stopped > 5.0)
            .filter_map(|c| c.lead_car)
            .collect();
        let mut i = 0;
        let mut off_ground = 0usize;
        let mut asleep = 0usize;
        while i < self.cars.len() {
            let c = &self.cars[i];
            let p = c.vehicle.position;
            // (the nearest player: a LAN host keeps the traffic around the others too)
            let dist = self
                .lan_centers
                .iter()
                .fold((p - center).length(), |d, o| d.min((p - *o).length()));
            // every car on the road or the rails whose ground has been unloaded goes: the
            // lanes stay in the network when their tile goes, but nothing may drive over
            // ground that is not there (tiles only go well beyond the view, timetable buses
            // included; their trips come back with the tiles)
            let flying = self
                .net
                .lanes
                .get(c.state.lane)
                .map(|l| l.kind == LaneKind::Air)
                .unwrap_or(false);
            let unloaded = !flying && !world.has_ground(p.x, p.y);
            off_ground += unloaded as usize;
            let random = !c.is_bus() || c.gone;
            let r = (c.state.length as f64 * 0.5).max(2.0);
            // standing at the end of the network (the map's edge): Omsi.exe never lets a
            // random car stand there - once 0x71dc9c finds no next segment (0x612e10) its
            // segment stays -1 and 0x6fe3fc deletes it the same frame (0x703bb0); a bus
            // that gave up goes once out of sight, or too far off for the renderer to draw it
            let at_end = c.gone
                && c.stopped > if c.is_bus() { 20.0 } else { 0.5 }
                && c.state.route.is_empty()
                && self.net.lanes[c.state.lane].next.is_empty();
            let from_eye = self.viewer.map(|v| (p - v.pos).length()).unwrap_or(dist);
            // a timetable bus waiting where the loaded part of its route ends
            let at_edge = c.route_open()
                && c.state.speed < 0.1
                && c.state.route.last() == Some(&c.state.lane)
                && c.state.s > self.net.lanes[c.state.lane].length() - 25.0;
            // a random car still on its way that goes out of range sleeps instead (see
            // `DormantCar`); only one whose trip is over leaves the map
            let sleeps_instead = random
                && !c.gone
                && !c.is_bus()
                && self
                .net
                .lanes
                .get(c.state.lane)
                .map(|l| l.kind == LaneKind::Street)
                .unwrap_or(false);
            let remove = if unloaded {
                true
            } else if at_edge {
                // (its trip comes back with the tiles; kept until nobody saw it, a bus stood
                // with its passengers at the far end of a straight road for good)
                self.hidden(world, p, r)
                    || (c.stopped > 8.0 && from_eye > 180.0)
                    || c.stopped > 150.0
            } else if !random {
                false
            } else if at_end
                && (!c.is_bus()
                || self.hidden(world, p, r)
                || (c.stopped > 8.0 && from_eye > 180.0)
                || c.stopped > 150.0
                || (c.stopped > 25.0 && queued.contains(&c.id) && from_eye > 25.0))
            {
                // (and in view too once others wait behind it: a fire engine at the end of a
                // dead-end street held a queue of fourteen cars for two and a half minutes)
                // (taken at once it vanished in plain view 300 m ahead; but a car kept
                // until nobody could see it stood for good at the end of a long straight
                // road in view, and the queue behind it - timetable buses with their
                // passengers among them - never moved again)
                true
            } else if c.gone || dist > far {
                // in plain view a car stays until the renderer leaves it out anyway, and
                // close by (the mirrors, a turn of the head) it stays in any case
                // (one that gave up in a gridlock goes after four minutes even in view,
                // unless right beside the viewer: kept until nobody saw it, a jam at a
                // junction the player watched never cleared)
                (dist > VISIBLE_RANGE * 1.3 && from_eye > NEAR_HIDE)
                    || self.hidden(world, p, r)
                    || (c.gone && c.stopped > 240.0 && from_eye > 40.0)
            } else {
                false
            };
            if remove && sleeps_instead && !at_end {
                let c = self.cars.swap_remove(i);
                asleep += 1;
                self.population.enter_dormant(
                    c.id,
                    SpawnClass::Unscheduled,
                    c.is_bus(),
                    c.state.lane,
                    c.state.s,
                );
                self.emit_trace(TraceEvent::DormantEntered { vehicle: c.id });
                self.dormant.push(DormantCar {
                    id: c.id,
                    ty: c.vehicle.ty.clone(),
                    kind: LaneKind::Street,
                    lane: c.state.lane,
                    s: c.state.s,
                    speed: c.state.speed.max(2.0),
                    seed: c.seed,
                    scheme: c.scheme,
                    walk: c.seed ^ c.id.get().wrapping_mul(0x9E37_79B9_7F4A_7C15) | 1,
                });
                self.orphan_sounds.extend(c.sounds);
                for r in std::iter::once(c.render).chain(c.trailer_renders) {
                    world.release_vehicle(renderer, scene, r);
                }
            } else if remove {
                let c = self.cars.swap_remove(i);
                self.population.release(
                    c.id,
                    if unloaded || at_edge {
                        RemovalCause::Unloaded
                    } else {
                        RemovalCause::Finished
                    },
                );
                if c.is_bus() && (unloaded || at_edge) {
                    self.removed_scheduled.push(c.id);
                }
                if self.debug_population {
                    let v = self.viewer;
                    log::info!(
                        "population t={:.1}: car {} removed at ({:.0}, {:.0}), {:.0} m from the player, in frame {}, behind a building {}, {}",
                        self.time,
                        c.id,
                        p.x,
                        p.y,
                        dist,
                        v.map(|v| v.frames(p, r)).unwrap_or(false),
                        v.map(|v| self.occluded(world, &v, p, r)).unwrap_or(false),
                        if c.gone { "finished" } else { "far away" }
                    );
                }
                self.orphan_sounds.extend(c.sounds);
                for r in std::iter::once(c.render).chain(c.trailer_renders) {
                    world.release_vehicle(renderer, scene, r);
                }
            } else {
                i += 1;
            }
        }
        if off_ground > 0 && ::legacy_config::env::var_os("OMSI_DEBUG_TRAFFIC").is_some() {
            log::info!("traffic: {off_ground} vehicles taken away with the tiles under them");
        }
        if (asleep > 0 || !self.dormant.is_empty()) && self.debug_population {
            log::info!(
                "population t={:.1}: {asleep} cars went out of range and drive on unseen; {} on the map out of range, {} in range",
                self.time,
                self.dormant.len(),
                self.cars.len()
            );
        }
        if self.types.is_empty() || self.net.lanes.is_empty() {
            self.initial = false;
            return;
        }
        // made only for the lights: nothing new while the target is 0, but the cars of a
        // target raised and lowered again go as they do anywhere (returning before the loop
        // above, they stood at the map's edge and drove over unloaded tiles for good)
        if self.lights_only && self.target == 0 {
            return;
        }
        // aircraft: a few on the flight paths, independent of the street target
        let has_air = self.types.iter().any(|t| t.2 == LaneKind::Air);
        // the map's traffic density by hour (and group) scales the street traffic ...
        let density = self.street_density();
        // ... and so does how much road there is around: the same number of cars looks
        // empty on a six-lane Berlin junction and crowded on a village lane, so the count
        // asked for is per a neighbourhood of about 250 lanes; and as Omsi spawns on each
        // path at a rate of its [rule] trafficdensity, paths of low density bring fewer cars
        // and those of density 0 (or kept clear of cars) none
        let near_density: Vec<f32> = self
            .net
            .lanes_starting_near(center, self.spawn_radius)
            .into_iter()
            .map(|i| &self.net.lanes[i])
            .filter(|l| {
                l.kind == LaneKind::Street
                    && l.points
                    .first()
                    .map(|p| (*p - center).length() < self.spawn_radius)
                    .unwrap_or(false)
            })
            .map(|l| {
                if l.no_cars {
                    0.0
                } else {
                    l.density.clamp(0.0, 4.0)
                }
            })
            .collect();
        let street_target =
            (self.target as f32 * density * road_scale(&near_density)).round() as usize;
        // One frozen occupancy snapshot for this population pass: the domain admission owner
        // checks physical gaps against the same realized bodies the rest of the tick uses.
        let pass_tick = (self.time * 1000.0).max(0.0) as u64;
        let occupancy = Occupancy::build(self.net.version(), pass_tick, self.body_feet(None, &[]));
        self.population.begin_tick(pass_tick, self.time);
        // the cars that come into range again, where they have got to
        self.wake_dormant(world, renderer, scene, center, street_target, &occupancy);
        // the whole map's population: as dense as around the player, on every street the
        // map has shown so far (sleeping where the player is not)
        self.fill_map(center, street_target);
        for (kind, target) in [
            (LaneKind::Street, street_target),
            (LaneKind::Air, if has_air { 3 } else { 0 }),
        ] {
            // (a LAN host counts the cars round itself only: counted over the whole map,
            // the traffic it keeps round the other players met its own target and the
            // host drove through empty streets, #342)
            if kind == LaneKind::Street && !self.lan_centers.is_empty() {
                self.count_near = Some((center, self.spawn_radius));
            }
            self.populate_kind(world, renderer, scene, center, kind, target, &occupancy);
        }
        self.populate_lan_centers(world, renderer, scene, center, street_target, &occupancy);
        if !self.initial {
            self.pull_out_parked(world, renderer, scene, center);
            self.park_in(world, center);
        }
        self.initial = false;
    }


    /// Now and then a car near the player parks: a space at the kerb that a parked car has
    /// left is taken by a car of the same kind driving up the lane beside it - it indicates,
    /// slows down, stops beside the space, moves over into it and stands there as the
    /// parked car it was. Called with every population pass (about every two seconds).
    pub fn park_in(&mut self, world: &World, center: DVec3) {
        let forced = ::legacy_config::env::var("OMSI_PARK_IN")
            .ok()
            .and_then(|v| v.parse::<f64>().ok());
        if self.rand_f() >= forced.unwrap_or(0.04) {
            return;
        }
        let debug = ::legacy_config::env::var_os("OMSI_DEBUG_TRAFFIC").is_some() || forced.is_some();
        let taken: hashbrown::HashSet<i64> = self
            .cars
            .iter()
            .filter_map(|c| c.maneuver.park.map(|p| p.key))
            .collect();
        let mut spots = world.free_parking();
        spots.retain(|(k, p)| {
            !taken.contains(k) && (30.0..260.0).contains(&(p.pos - center).truncate().length())
        });
        spots.sort_by_key(|s| s.0);
        let mut why: Vec<String> = Vec::new();
        while !spots.is_empty() {
            let (key, p) = spots.swap_remove(self.rand() as usize % spots.len());
            let Some((l, s, _)) = self.net.nearest_lane_near(p.pos, LaneKind::Street) else {
                continue;
            };
            let lane = &self.net.lanes[l];
            if lane.no_cars || s < 8.0 || s > lane.length() - 4.0 {
                continue;
            }
            let (q, h) = lane.at(s);
            let hr = (h as f64).to_radians();
            let lat = (p.pos - q).dot(DVec3::new(hr.cos(), -hr.sin(), 0.0)) as f32;
            let mut dh = (p.heading - h as f64).rem_euclid(360.0);
            if dh > 180.0 {
                dh -= 360.0;
            }
            // beside the lane on the right and in line with it (a space across the kerb or in
            // a row is not driven into)
            if !(1.2..4.2).contains(&lat) || dh.abs() > 12.0 {
                why.push(format!(
                    "space {key}: {lat:+.1} m beside lane {l}, {dh:+.0} deg"
                ));
                continue;
            }
            let folder = p.sco.parent().map(|d| d.to_string_lossy().to_lowercase());
            // a car of that kind coming up the lane, far enough off to slow down gently
            let mut best: Option<(usize, f32)> = None;
            for (i, c) in self.cars.iter().enumerate() {
                if c.is_bus()
                    || c.gone
                    || c.maneuver.park.is_some()
                    || c.maneuver.passing.is_some()
                    || c.state.change.is_some()
                    || c.maneuver.pull_out > 0.0
                {
                    continue;
                }
                if c.vehicle
                    .ty
                    .def
                    .path
                    .parent()
                    .map(|d| d.to_string_lossy().to_lowercase())
                    != folder
                {
                    continue;
                }
                if c.state.speed > 15.0 || c.state.lateral.abs() > 0.2 {
                    continue;
                }
                let Some(&(_, dl)) = self.way_lanes(&c.state, 160.0).iter().find(|w| w.0 == l)
                else {
                    continue;
                };
                let d = dl + s;
                let need = c.state.speed * c.state.speed / 2.0 + 20.0;
                if d > need && d < 150.0 && best.map(|b| d < b.1).unwrap_or(true) {
                    best = Some((i, d));
                }
            }
            let Some((i, d)) = best else {
                why.push(format!(
                    "space {key}: no car of its kind coming up lane {l}"
                ));
                continue;
            };
            let car = &mut self.cars[i];
            car.maneuver.park = Some(ParkPlan {
                key,
                lane: l,
                s,
                lat,
                ramped: false,
                done: false,
            });
            // (it parks: no more route to plan than to here)
            car.gone = false;
            if debug {
                log::info!(
                    "car {} parks in the space of parked car {key} ({:.0} m ahead, {lat:.1} m right of lane {l})",
                    car.id,
                    d
                );
            }
            return;
        }
        if debug && !why.is_empty() {
            log::info!("park-in: none this time ({})", why.join("; "));
        }
    }


    /// Now and then a car parked at the kerb near the player drives off: the parked object
    /// goes (its space stays empty) and the AI car of the same folder takes its place, in
    /// the parking position, indicating, and pulls out into its lane once the road behind
    /// it is clear. Called with every population pass (about every two seconds).
    pub fn pull_out_parked(
        &mut self,
        world: &World,
        renderer: &Renderer,
        scene: &mut Scene,
        center: DVec3,
    ) {
        let forced = ::legacy_config::env::var("OMSI_PARKED_PULL_OUT")
            .ok()
            .and_then(|v| v.parse::<f64>().ok());
        // about one car a minute
        if self.rand_f() >= forced.unwrap_or(0.035) {
            return;
        }
        let debug = ::legacy_config::env::var_os("OMSI_DEBUG_TRAFFIC").is_some() || forced.is_some();
        let mut candidates: Vec<(i64, crate::scene::ParkedObject)> = world
            .parked_objects
            .lock()
            .iter()
            .filter(|(_, p)| {
                let d = (p.pos - center).truncate().length();
                (25.0..180.0).contains(&d)
            })
            .map(|(k, p)| (*k, p.clone()))
            .collect();
        candidates.sort_by_key(|c| c.0);
        if debug {
            log::info!(
                "parked pull-out: {} parked cars in range ({} loaded)",
                candidates.len(),
                world.parked_objects.lock().len()
            );
        }
        let mut why: Vec<String> = Vec::new();
        let mut tried = 0;
        while !candidates.is_empty() && tried < 8 {
            tried += 1;
            let (key, p) = candidates.swap_remove(self.rand() as usize % candidates.len());
            // the AI car of the same folder (a parked Golf is `parked_vw_golf_2.sco` next to
            // `ai_vw_golf_2.bus`)
            let folder = p.sco.parent().map(|d| d.to_string_lossy().to_lowercase());
            let Some(ty) = self
                .types
                .iter()
                .filter(|t| t.2 == LaneKind::Street)
                .find(|t| {
                    t.0.def
                        .path
                        .parent()
                        .map(|d| d.to_string_lossy().to_lowercase())
                        == folder
                })
                .map(|t| t.0.clone())
            else {
                why.push(format!("no AI car in {folder:?}"));
                continue;
            };
            let Some((l, s, _)) = self.net.nearest_lane_near(p.pos, LaneKind::Street) else {
                why.push("no lane".into());
                continue;
            };
            let lane = &self.net.lanes[l];
            if lane.no_cars || s < 4.0 || s > lane.length() - 4.0 {
                continue;
            }
            let (q, h) = lane.at(s);
            let hr = (h as f64).to_radians();
            let right = DVec3::new(hr.cos(), -hr.sin(), 0.0);
            let lat = (p.pos - q).dot(right) as f32;
            // beside the lane on the right, facing its way (one parked in the lane itself stands
            // bumper to bumper in a row it cannot steer out of)
            let mut dh = (p.heading - h as f64).rem_euclid(360.0);
            if dh > 180.0 {
                dh -= 360.0;
            }
            if !(0.8..4.5).contains(&lat) || dh.abs() > 30.0 {
                why.push(format!("{lat:+.1} m beside lane {l}, {dh:+.0} deg to it"));
                continue;
            }
            // nobody close by on the road
            if self
                .cars
                .iter()
                .any(|c| (c.vehicle.position - p.pos).length() < 30.0)
                || !self.spawn_clear(&ty, q, h as f64)
            {
                why.push("road not clear".into());
                continue;
            }
            if world.depart_parked(renderer, scene, key).is_none() {
                continue;
            }
            if let Some(list) = self.parked.get_mut(&l) {
                if let Some(j) = (0..list.len())
                    .min_by(|&a, &b| (list[a].0 - s).abs().total_cmp(&(list[b].0 - s).abs()))
                {
                    if (list[j].0 - s).abs() < 3.0 {
                        list.swap_remove(j);
                    }
                }
            }
            let seed = self.rand();
            let id = self.create_car(
                world,
                renderer,
                scene,
                center,
                LaneKind::Street,
                l,
                s,
                ty.clone(),
                seed,
                None,
                None,
                Some(0.0),
                None,
            );
            let net = &self.net;
            if let Some(car) = self.cars.iter_mut().find(|c| c.id == id) {
                car.state.lateral = lat;
                car.state.lateral_target = 0.0;
                car.state.lateral_ramp =
                    (lat, 0.0, car.state.odometer, (lat * 6.0).clamp(8.0, 16.0));
                car.body = place_body(net, &car.state, &mut car.vehicle, MotionKind::Road);
                car.maneuver.pull_out = 2.0 + (seed % 1000) as f32 / 400.0;
                car.state.blinker = 1;
            }
            if debug {
                log::info!(
                    "parked car {key} ({}) at ({:.1}, {:.1}) pulls out as car {id}, {lat:.1} m right of lane {l}",
                    ty.def.path.display(),
                    p.pos.x,
                    p.pos.y
                );
            }
            return;
        }
        if debug && !why.is_empty() {
            log::info!("parked pull-out: none this time ({})", why.join("; "));
        }
    }


    pub(crate) fn populate_kind(
        &mut self,
        world: &World,
        renderer: &Renderer,
        scene: &mut Scene,
        center: DVec3,
        kind: LaneKind,
        target: usize,
        occupancy: &Occupancy,
    ) {
        let radius = if kind == LaneKind::Air {
            self.spawn_radius * 6.0
        } else {
            self.spawn_radius
        };
        let nearby = self.net.lanes_starting_near(center, radius);
        // candidate lanes of this kind near the centre
        let pick = |through: bool| -> Vec<(usize, f32)> {
            nearby
                .iter()
                .copied()
                .map(|i| (i, &self.net.lanes[i]))
                .filter(|(_, l)| {
                    l.kind == kind
                        && l.length() > 8.0
                        && (l.start() - center).truncate().length() < radius
                })
                // lanes the map keeps clear of cars, and those whose [rule] trafficdensity is
                // zero, are not spawned on at all; a lower density makes a lane that much less
                // likely to be picked
                .filter(|(_, l)| !l.no_cars && l.density > 0.0)
                // nor, where there are others, lanes that end the network just ahead (the car
                // would only drive into the end and wait there to be taken away)
                .filter(|(i, _)| {
                    !through
                        || kind != LaneKind::Street
                        || self
                        .net
                        .reach
                        .get(*i)
                        .map(|r| *r >= ::traffic::DEAD_END)
                        .unwrap_or(true)
                })
                // as many cars on a lane as metres of it (times its density): counted per
                // lane, the many short lanes of a junction drew the cars into the town's
                // tangles and left the long roads between them empty
                .map(|(i, l)| (i, l.length() * l.density.clamp(0.05, 4.0)))
                .collect::<Vec<(usize, f32)>>()
        };
        let mut candidates = pick(true);
        if candidates.is_empty() {
            candidates = pick(false);
        }
        if candidates.is_empty() {
            return;
        }
        let mut acc = 0.0f32;
        let cumulative: Vec<f32> = candidates
            .iter()
            .map(|c| {
                acc += c.1;
                acc
            })
            .collect();
        let total_w = acc.max(1e-3);
        let counted_near = self.count_near.take();
        let unscheduled = self
            .cars
            .iter()
            .filter(|c| {
                !c.is_bus()
                    && !c.gone
                    && counted_near
                    .map(|(p, r)| (c.vehicle.position - p).length() < r)
                    .unwrap_or(true)
                    && self
                    .net
                    .lanes
                    .get(c.state.lane)
                    .map(|l| l.kind == kind)
                    .unwrap_or(false)
            })
            .count();
        let mut count = unscheduled;
        // Submit the deficit as demand to the population owner. The cheap, non-type checks
        // stay here; the admission decision (budget, path, ground, gap, visibility) belongs
        // to `traffic::population`, which bounds the queue and the per-pass attempts.
        let deficit = target.saturating_sub(count);
        let mut attempts = 0usize;
        while count < target && deficit > 0 && attempts < target.saturating_mul(12).max(1) {
            attempts += 1;
            let x = self.rand_f() as f32 * total_w;
            let lane = candidates[cumulative
                .partition_point(|&c| c < x)
                .min(candidates.len() - 1)]
                .0;
            let s = (self.rand_f() * (self.net.lanes[lane].length() as f64 - 6.0)) as f32 + 3.0;
            let (p, _) = self.net.lanes[lane].at(s);
            let rel = p - center;
            if rel.length() < 40.0 {
                continue; // not right next to the player
            }
            // nobody may see it appear (the first population is the world as it loads)
            if kind == LaneKind::Street && !self.initial && !self.may_appear(world, p) {
                continue;
            }
            if self
                .cars
                .iter()
                .any(|c| (c.vehicle.position - p).length() < 14.0)
            {
                continue;
            }
            // nor just in front of one driving up to that place (it would have to stop hard)
            let in_front_of_someone = self.cars.iter().any(|c| {
                let rel = p - c.vehicle.position;
                let h = c.vehicle.heading.to_radians();
                let (along, across) = (
                    rel.x * h.sin() + rel.y * h.cos(),
                    (rel.x * h.cos() - rel.y * h.sin()).abs(),
                );
                along > 0.0
                    && along
                    < 20.0
                    + (c.state.speed * c.state.speed / (2.0 * c.state.decel.max(1.0)))
                    as f64
                    * 1.5
                    && across < 3.0
            });
            if in_front_of_someone {
                continue;
            }
            if self
                .parked
                .get(&lane)
                .map(|l| {
                    l.iter()
                        .any(|&(ps, lat)| (ps - s).abs() < 8.0 && lat.abs() < 1.5)
                })
                .unwrap_or(false)
            {
                continue; // not into a car parked in the lane
            }
            if !self.population.has_room() {
                break;
            }
            self.population
                .request(SpawnClass::Unscheduled, lane, s);
            count += 1;
        }
        // Annotate every outstanding request of this kind with its runtime facts, then let
        // the owner decide. Requests of another kind are left untouched for their own pass.
        let reqs: Vec<SpawnRequest> = self.population.requests().copied().collect();
        let mut facts: HashMap<SpawnRequestId, SpawnFacts> = HashMap::new();
        for req in &reqs {
            let Some(l) = self.net.lanes.get(req.lane) else {
                continue;
            };
            if l.kind != kind {
                continue;
            }
            let s = req.s.clamp(0.0, (l.length() - 0.1).max(0.0));
            let (p, _) = l.at(s);
            let ground = kind == LaneKind::Air || world.has_ground(p.x, p.y);
            let path_valid = kind == LaneKind::Air
                || !l.next.is_empty()
                || self
                    .net
                    .reach
                    .get(req.lane)
                    .map(|r| *r >= ::traffic::DEAD_END)
                    .unwrap_or(true);
            let continuation = self
                .net
                .reach
                .get(req.lane)
                .map(|r| *r >= ::traffic::DEAD_END)
                .unwrap_or(true);
            let visible = kind == LaneKind::Air || self.initial || self.may_appear(world, p);
            facts.insert(
                req.id,
                SpawnFacts {
                    path_valid,
                    continuation,
                    ground,
                    visible,
                },
            );
        }
        let demand = PopulationDemand {
            street_target: if kind == LaneKind::Street { target } else { 0 },
            air_target: if kind == LaneKind::Air { target } else { 0 },
            unscheduled_count: unscheduled,
            ..Default::default()
        };
        let decisions = {
            let scene = PopulationScene {
                net: &self.net,
                occupancy,
                demand,
                initial: self.initial,
                tick: self.population.tick(),
            };
            self.population.plan(&scene, &facts)
        };
        for d in decisions {
            match d.outcome {
                SpawnOutcome::Admit => {
                    let lane = d.request.lane;
                    let s = d.request.s;
                    // A lane may carry none of the groups that drive now: try another.
                    let Some(ty) = self.pick_type(kind, Some(lane)) else {
                        self.emit_trace(TraceEvent::SpawnDenied {
                            reason: Reason::NoPath,
                        });
                        continue;
                    };
                    let (p, heading) = {
                        let l = &self.net.lanes[lane];
                        let (p, h) = l.at(s.clamp(0.0, (l.length() - 0.1).max(0.0)));
                        (p, h as f64)
                    };
                    // Nor onto the rear section of an articulated bus, nor the player's bus.
                    if kind != LaneKind::Air && !self.spawn_clear(&ty, p, heading) {
                        self.emit_trace(TraceEvent::SpawnDenied {
                            reason: Reason::EntranceBusy,
                        });
                        continue;
                    }
                    let seed = self.rand();
                    let id = self.create_car(
                        world, renderer, scene, center, kind, lane, s, ty, seed, None, None, None,
                        None,
                    );
                    self.emit_trace(TraceEvent::SpawnAdmitted { vehicle: id });
                }
                SpawnOutcome::Deny(reason) => {
                    self.emit_trace(TraceEvent::SpawnDenied { reason });
                }
                SpawnOutcome::Retry { .. } => {
                    self.emit_trace(TraceEvent::SpawnRetried {
                        request: d.request.id.0,
                    });
                }
            }
        }
    }


    /// The cars out of range drive on: along their lanes at about the lanes' speed, taking
    /// a random way at every fork; one that reaches the end of the network has left the map.
    pub(crate) fn advance_dormant(&mut self) {
        let dt = (self.time - self.dormant_time).clamp(0.0, 10.0);
        self.dormant_time = self.time;
        if dt <= 0.0 || self.dormant.is_empty() {
            return;
        }
        let mut i = 0;
        while i < self.dormant.len() {
            let mut gone = false;
            {
                let d = &mut self.dormant[i];
                let lane = &self.net.lanes[d.lane];
                // (waits at lights and junctions taken as a quarter off the speed limit)
                d.speed = (lane.speed_limit_kmh.min(60.0) / 3.6 * 0.75).max(2.0);
                d.s += d.speed * dt;
                let mut guard = 0;
                while d.s > self.net.lanes[d.lane].length() && guard < 32 {
                    guard += 1;
                    let l = &self.net.lanes[d.lane];
                    // the ways a car on the road would take (`AiState::choose_after`): those
                    // of its group, else those open to cars, else any - only where the
                    // network ends does it leave the map (filtering by its group alone, the
                    // trucks of Spandau were gone at the first junction whose turn has no
                    // `trucks` rule, and hardly one of them ever came into range)
                    let pool = self
                        .types
                        .iter()
                        .find(|t| Arc::ptr_eq(&t.0, &d.ty))
                        .and_then(|t| self.group_uvg[t.3]);
                    let same_kind = |n: &usize| self.net.lanes[*n].kind == d.kind;
                    let open = |pooled: bool| -> Vec<usize> {
                        l.next
                            .iter()
                            .copied()
                            .filter(same_kind)
                            .filter(|&n| {
                                let nl = &self.net.lanes[n];
                                let density = match pool.filter(|_| pooled) {
                                    Some(p) => nl.pool_density(&self.uvg_defaults, p),
                                    None => nl.density,
                                };
                                nl.allows(d.ty.def.ai_veh_type) && density > 0.0
                            })
                            .collect()
                    };
                    let mut options = if pool.is_some() {
                        open(true)
                    } else {
                        Vec::new()
                    };
                    if options.is_empty() {
                        options = open(false);
                    }
                    if options.is_empty() {
                        options = l.next.iter().copied().filter(same_kind).collect();
                    }
                    if options.is_empty() {
                        gone = true;
                        break;
                    }
                    d.s -= l.length();
                    d.walk = d
                        .walk
                        .wrapping_mul(6364136223846793005)
                        .wrapping_add(1442695040888963407);
                    d.lane = options[(d.walk >> 33) as usize % options.len()];
                }
            }
            if gone {
                self.dormant.swap_remove(i);
            } else {
                i += 1;
            }
        }
    }


    /// The cars out of range that have come near again take their bodies back - where
    /// nobody sees it happen, up to a little over the number asked for around the player.
    pub(crate) fn wake_dormant(
        &mut self,
        world: &World,
        renderer: &Renderer,
        scene: &mut Scene,
        center: DVec3,
        target: usize,
        occupancy: &Occupancy,
    ) {
        if self.dormant.is_empty() {
            return;
        }
        let active = self.cars.iter().filter(|c| !c.is_bus() && !c.gone).count();
        let centers: Vec<DVec3> = std::iter::once(center)
            .chain(self.lan_centers.iter().copied())
            .collect();
        // Only the actors that have come near are candidates; the coordinator validates the
        // rest (ground, visibility, gap) and never places one that would overlap.
        let views: Vec<DormantView> = self
            .dormant
            .iter()
            .filter_map(|d| {
                let l = self.net.lanes.get(d.lane)?;
                let (p, _) = l.at(d.s.clamp(0.0, (l.length() - 0.1).max(0.0)));
                let near = centers
                    .iter()
                    .any(|c| (p - *c).truncate().length() < self.spawn_radius);
                if !near {
                    return None;
                }
                Some(DormantView {
                    id: d.id,
                    class: SpawnClass::Unscheduled,
                    duty: false,
                    lane: d.lane,
                    s: d.s,
                    ground: world.has_ground(p.x, p.y),
                    visible: self.initial || self.may_appear(world, p),
                })
            })
            .collect();
        if views.is_empty() {
            return;
        }
        let demand = PopulationDemand {
            street_target: target,
            unscheduled_count: active,
            dormant_capacity: (target as f32 * DORMANT_CAP_FACTOR) as usize,
            ..Default::default()
        };
        let decisions = {
            let scene = PopulationScene {
                net: &self.net,
                occupancy,
                demand,
                initial: self.initial,
                tick: self.population.tick(),
            };
            self.population.plan_dormant(&scene, &views)
        };
        for dec in decisions {
            if dec.outcome != SpawnOutcome::Admit {
                continue;
            }
            let Some(pos) = self.dormant.iter().position(|d| d.id == dec.id) else {
                continue;
            };
            let d = self.dormant.swap_remove(pos);
            let (p, h) = {
                let l = &self.net.lanes[d.lane];
                let (p, h) = l.at(d.s.clamp(0.0, (l.length() - 0.1).max(0.0)));
                (p, h as f64)
            };
            // The domain checked the generic gap; the asset-specific clearance is still the
            // adapter's, so a trailer or the player's bus cannot be woken onto.
            if !self.spawn_clear(&d.ty, p, h) {
                self.dormant.push(d);
                continue;
            }
            let (kind, lane, s, seed, scheme, speed, id) =
                (d.kind, d.lane, d.s, d.seed, d.scheme, d.speed, d.id);
            self.create_car(
                world, renderer, scene, center, kind, lane, s, d.ty, seed, Some(scheme),
                Some(id), Some(speed), None,
            );
            self.population.note_reactivated(id);
            self.emit_trace(TraceEvent::DormantReactivated { vehicle: id });
        }
    }


    /// Fill the map: the streets the map has shown so far carry as many cars per metre as
    /// the ones around the player, the ones out of range as dormant cars (see `DormantCar`).
    pub(crate) fn fill_map(&mut self, center: DVec3, street_target: usize) {
        if street_target == 0 {
            return;
        }
        let far = self.spawn_radius * DESPAWN_FACTOR;
        let centers: Vec<DVec3> = std::iter::once(center)
            .chain(self.lan_centers.iter().copied())
            .collect();
        let mut nearby: Vec<usize> = centers
            .iter()
            .flat_map(|&c| self.net.lanes_starting_near(c, self.spawn_radius))
            .collect();
        nearby.sort_unstable();
        nearby.dedup();
        let mut near = 0f64;
        for i in nearby {
            let l = &self.net.lanes[i];
            let Some(w) = street_lane_weight(l) else {
                continue;
            };
            let d = centers
                .iter()
                .map(|c| (l.start() - *c).truncate().length())
                .fold(f64::MAX, f64::min);
            if d < self.spawn_radius {
                near += w;
            }
        }
        if near < 50.0 {
            return;
        }
        let map_target = ((street_target as f64 * self.street_weight / near)
            .min(street_target as f64 * MAP_POPULATION_FACTOR as f64))
            as usize;
        let present =
            self.cars.iter().filter(|c| !c.is_bus() && !c.gone).count() + self.dormant.len();
        if present >= map_target {
            return;
        }
        // The full outside list is only needed while replenishing the map population.
        let outside: Vec<(usize, f32)> = self
            .net
            .lanes
            .iter()
            .enumerate()
            .filter_map(|(i, l)| {
                let w = street_lane_weight(l)?;
                let d = centers
                    .iter()
                    .map(|c| (l.start() - *c).truncate().length())
                    .fold(f64::MAX, f64::min);
                (d > far).then_some((i, w as f32))
            })
            .collect();
        if outside.is_empty() {
            return;
        }
        let mut acc = 0.0f32;
        let cumulative: Vec<f32> = outside
            .iter()
            .map(|c| {
                acc += c.1;
                acc
            })
            .collect();
        let dormant_capacity = (street_target as f32 * DORMANT_CAP_FACTOR) as usize;
        for _ in 0..(map_target - present).min(64) {
            if !self.population.dormant_has_room(dormant_capacity) {
                break;
            }
            let x = self.rand_f() as f32 * acc;
            let lane = outside[cumulative
                .partition_point(|&c| c < x)
                .min(outside.len() - 1)]
                .0;
            let s = (self.rand_f() * (self.net.lanes[lane].length() as f64 - 4.0)) as f32 + 2.0;
            let Some(ty) = self.pick_type(LaneKind::Street, Some(lane)) else {
                continue;
            };
            let seed = self.rand();
            let scheme = if ty.paint_schemes.is_empty() {
                None
            } else {
                Some((seed >> 8) as usize % ty.paint_schemes.len().min(AI_SCHEMES))
            };
            let id = VehicleId(self.next_id);
            self.next_id += 1;
            self.population.enter_dormant(
                id,
                SpawnClass::Unscheduled,
                false,
                lane,
                s,
            );
            self.dormant.push(DormantCar {
                id,
                ty,
                kind: LaneKind::Street,
                lane,
                s,
                speed: 8.0,
                seed,
                scheme,
                walk: seed | 1,
            });
        }
    }


    /// Put a timetable bus on the road: an AI car like any other (`create_car`), on its
    /// trip's route at `s` metres into the first lane, with its service (stops).
    /// Returns the car index. `scheme`: the paint scheme to use (Some), or a random one.
    #[allow(clippy::too_many_arguments)]
    pub fn spawn_bus(
        &mut self,
        world: &World,
        renderer: &Renderer,
        scene: &mut Scene,
        ty: Arc<VehicleType>,
        route: Vec<usize>,
        s: f32,
        stops: Vec<(usize, f32, f32, f64, i64, f32)>,
        number: Option<(String, String)>,
        hof: Option<Arc<::legacy_vehicle::Hof>>,
        scheme: Option<Option<usize>>,
    ) -> Result<usize, Reason> {
        let &lane = route.first().ok_or(Reason::NoPath)?;
        let kind = self.net.lanes.get(lane).ok_or(Reason::NoPath)?.kind;
        // the options' [AIMaxCountScheduled]: no more timetable vehicles than that at once.
        // Scheduled capacity is a distinct budget from the random population.
        let scheduled_count = self
            .cars
            .iter()
            .filter(|c| c.is_bus() || !c.state.route.is_empty())
            .count();
        let demand = PopulationDemand {
            scheduled_cap: self.max_scheduled,
            scheduled_count,
            ..Default::default()
        };
        if let SpawnOutcome::Deny(reason) = self.population.scheduled_admission(&demand) {
            self.emit_trace(TraceEvent::SpawnDenied { reason });
            return Err(reason);
        }
        let seed = self.rand();
        let setup = BusSetup {
            route,
            stops: stops
                .into_iter()
                .map(StopTarget::from_tuple)
                .collect(),
            number,
            hof,
        };
        let center = self.viewer.map(|v| v.pos).unwrap_or_default();
        let id = self.create_car(
            world,
            renderer,
            scene,
            center,
            kind,
            lane,
            s,
            ty.clone(),
            seed,
            scheme,
            None,
            None,
            Some(setup),
        );
        let ci = self
            .cars
            .iter()
            .rposition(|c| c.id == id)
            .ok_or(Reason::NoPath)?;
        self.emit_trace(TraceEvent::SpawnAdmitted { vehicle: id });
        if kind == LaneKind::Air {
            let p = self.cars[ci].vehicle.position;
            let ground = world
                .ground_height(p.x, p.y)
                .map(|g| format!("{:.0} m above the ground", p.z - g))
                .unwrap_or_else(|| "over unloaded ground".into());
            log::info!(
                "aircraft {} on its flight path at ({:.0}, {:.0}), height {:.0} m, {ground}, {:.0} km/h",
                ty.def
                    .path
                    .file_name()
                    .unwrap_or_default()
                    .to_string_lossy(),
                p.x,
                p.y,
                p.z,
                self.cars[ci].state.speed * 3.6
            );
        }
        Ok(ci)
    }


    /// May a vehicle of `ty` be put on the road at `pos` facing `heading` (deg)? Not onto
    /// (or right up against) another vehicle or the player's: a timetable bus used to be
    /// checked only for a vehicle origin within 9 m, and a layover bus appeared inside the
    /// articulated bus waiting at the same stand.
    pub fn spawn_clear(&self, ty: &VehicleType, pos: DVec3, heading: f64) -> bool {
        let (front, rear, half_w) = extents(ty, 12.0);
        let h = heading.to_radians();
        let (fwd, right) = (DVec2::new(h.sin(), h.cos()), DVec2::new(h.cos(), -h.sin()));
        let me = Footprint {
            car: usize::MAX,
            center: pos.truncate() + fwd * ((front - rear) * 0.5) as f64,
            fwd,
            right,
            half_len: ((front + rear) * 0.5) as f64,
            half_w: half_w as f64,
            speed: 0.0,
            z: pos.z,
        };
        if self.footprints().iter().any(|f| {
            (f.center - me.center).length() < f.half_len + me.half_len + 5.0
                && (f.z - me.z).abs() < 4.0
                && f.overlaps(&me, 1.0)
        }) {
            return false;
        }
        // nor just in front of a car driving up to that place (it would have to stop hard)
        let in_front = self.cars.iter().any(|c| {
            let rel = pos - c.vehicle.position;
            let h = c.vehicle.heading.to_radians();
            let (along, across) = (
                rel.x * h.sin() + rel.y * h.cos(),
                (rel.x * h.cos() - rel.y * h.sin()).abs(),
            );
            let v = c.state.speed;
            along > 0.0
                && along
                < (c.state.front + rear + 15.0 + v * v / (2.0 * c.state.decel.max(1.0)) * 1.5)
                as f64
                && across < 3.0
                && (rel.z).abs() < 4.0
        });
        if in_front {
            return false;
        }
        match self.player {
            Some((c, ph, hl, hw, _)) => {
                let h = ph.to_radians();
                let p = Footprint {
                    car: usize::MAX,
                    center: c.truncate(),
                    fwd: DVec2::new(h.sin(), h.cos()),
                    right: DVec2::new(h.cos(), -h.sin()),
                    half_len: hl as f64,
                    half_w: hw as f64,
                    speed: 0.0,
                    z: c.z,
                };
                (p.z - me.z).abs() > 4.0 || !p.overlaps(&me, 1.5)
            }
            None => true,
        }
    }


    /// Would a vehicle of type `ty` with its origin at `pos`, heading `heading` (and its
    /// coupled parts, straight behind it) touch one that is already there - an AI vehicle,
    /// or one of `keep_clear`? Bodies are compared with half a metre to spare, not centres:
    /// an articulated bus reaches 6 m ahead of its origin and 12 m behind it, and one put
    /// down 9.3 m from the player's bus stood 1.8 m inside it.
    pub fn blocked(&mut self, ty: &Arc<VehicleType>, pos: DVec3, heading: f64) -> bool {
        let grown = |mut b: ::simulation::collision::Obb| {
            b.half += glam::DVec2::splat(0.5);
            b
        };
        let mut bodies = vec![grown(::simulation::collision::Obb::from_box(
            ty.def.bounding_box.unwrap_or(DEFAULT_BOX),
            pos,
            heading,
        ))];
        let h = heading.to_radians();
        let fwd = DVec3::new(h.sin(), h.cos(), 0.0);
        let (mut origin, mut lead) = (pos, ty.clone());
        for (t, _) in self.trailer_chain(ty) {
            let back = lead
                .def
                .coupling_back
                .as_ref()
                .map(|c| c.pos[1])
                .unwrap_or(-4.0);
            let front = t
                .def
                .coupling_front
                .as_ref()
                .map(|c| c.pos[1])
                .unwrap_or(4.0);
            origin += fwd * (back - front) as f64;
            bodies.push(grown(::simulation::collision::Obb::from_box(
                t.def.bounding_box.unwrap_or(DEFAULT_BOX),
                origin,
                heading,
            )));
            lead = t;
        }
        let reach = bodies
            .iter()
            .map(|b| (b.center - pos.truncate()).length() + b.half.length())
            .fold(0.0, f64::max)
            + 40.0;
        let touches = |o: &::simulation::collision::Obb| bodies.iter().any(|b| b.overlaps(o));
        self.keep_clear.iter().any(|o| touches(o))
            || self
            .cars
            .iter()
            .filter(|c| (c.vehicle.position - pos).length() < reach)
            .any(|c| vehicle_bodies(&c.vehicle).iter().any(|o| touches(o)))
    }

}
