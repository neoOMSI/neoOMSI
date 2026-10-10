//! The per-tick pipeline: clock, frozen snapshot, per-owner planning, realization and
//! commit, split into named phases.

use super::*;

impl Traffic {

    /// The fixed simulation tick: the ordered pipeline of named phases below. Every phase
    /// reads/writes this `Traffic` and the frozen per-tick data phase 4 hands on; a live
    /// vehicle is never switched between state models mid-tick.
    pub fn tick(&mut self, dt: f32, player: Option<PlayerBox>) {
        self.lamp_dt += dt;
        if self.mirror {
            self.mirror_tick(dt);
            return;
        }
        let t_start = std::time::Instant::now();
        // 1/2. Clock/index, then the light requests and pedestrian pushes.
        self.tick_clock_and_index(dt, player);
        let walkers = self.tick_light_requests(dt, player);
        let (player_standing, others) = self.tick_presence(dt, player);
        let debug = ::legacy_config::env::var_os("OMSI_DEBUG_TRAFFIC").is_some();
        // 3. Freeze the per-tick snapshot and run every owner's decisions.
        let t_plan = std::time::Instant::now();
        let mut frame =
            self.tick_plan(dt, player, &others, player_standing, walkers, debug);
        // 4/5. Realize bodies/scripts, then commit realized feedback.
        let t_par = std::time::Instant::now();
        self.tick_realize(dt, &mut frame.frames, &frame.previous_odometer);
        self.tick_commit_feedback(dt, &frame.previous_odometer);
        self.tick_split = [
            (t_plan - t_start).as_secs_f64(),
            (t_par - t_plan).as_secs_f64(),
            t_par.elapsed().as_secs_f64(),
        ];
        // 6. Diagnose, trace, remove and capture.
        self.tick_finish(player, &frame, &others, debug);
    }

    /// 1. Advance the clock and rebuild the per-tick indexes (`geo_prev`, `index_of`).
    fn tick_clock_and_index(&mut self, dt: f32, player: Option<PlayerBox>) {
        self.time += dt;
        self.day_time += dt as f64 * self.time_scale;
        self.last_dt = dt;
        self.held_at_red = 0;
        self.player = player;
        self.geo_prev = self
            .cars
            .iter_mut()
            .map(|c| (c.id, c.geo_block.take()))
            .collect();
        self.index_of = self
            .cars
            .iter()
            .enumerate()
            .map(|(i, c)| (c.id, i))
            .collect();
    }

    /// 2. The light programs: reset and collect the requests of every approaching car, the
    /// player and the other players (depot gates), the pedestrian push buttons, then advance
    /// the cycle clocks. Returns the pedestrian positions for the junction scene.
    fn tick_light_requests(
        &mut self,
        dt: f32,
        player: Option<PlayerBox>,
    ) -> HashMap<usize, Vec<f32>> {
        // the light programs: requests of whoever is coming, then the cycle clocks
        for c in self.lights.iter_mut() {
            c.request.iter_mut().for_each(|r| *r = false);
        }
        let mut way_scratch: Vec<(usize, f32)> = Vec::new();
        for c in &self.cars {
            self.way_lanes_into(&c.state, 160.0, &mut way_scratch);
            for &(l, d) in &way_scratch {
                if let Some((ci, li)) = self.net.lanes[l].traffic_light {
                    if let Some(ctl) = self.lights.get_mut(ci) {
                        let gap = d - c.state.front;
                        if gap <= ctl.approach_dist(li) && d > -self.net.lanes[l].length() {
                            if let Some(r) = ctl.request.get_mut(li) {
                                *r = true;
                            }
                        }
                    }
                }
            }
        }
        // the player's bus and the other players' vehicles ask too: a depot gate (Spandau's
        // `Omnibushof_S_1`, the exit arm on light 1) opens only for whoever asks, and the
        // player driving out of the depot at the start of a duty found it shut
        let askers: Vec<(DVec3, f64)> = player
            .iter()
            .map(|p| (p.0, p.1))
            .chain(self.others.iter().map(|(_, b)| (b.0, b.1)))
            .collect();
        for &(pos, heading) in &askers {
            // (off the lanes - a depot yard, a car park - a gate's lane that starts just
            // ahead, the way the bus is facing, is asked all the same: standing a few metres
            // beside every lane there, the bus never opened the barrier in front of it)
            let h = heading.to_radians();
            let fwd = glam::DVec2::new(h.sin(), h.cos());
            for l in self.net.lanes_starting_near(pos, 35.0) {
                let lane = &self.net.lanes[l];
                let Some((ci, li)) = lane.traffic_light else {
                    continue;
                };
                let (p0, h0) = lane.at(0.0);
                let d = (p0 - pos).truncate();
                let (along, across) = (d.dot(fwd), d.perp_dot(fwd).abs());
                let turn = ((h0 as f64 - heading + 540.0).rem_euclid(360.0) - 180.0).abs();
                if (-2.0..25.0).contains(&along)
                    && across < 6.0
                    && turn < 60.0
                    && (p0.z - pos.z).abs() < 4.0
                {
                    if let Some(r) = self.lights.get_mut(ci).and_then(|c| c.request.get_mut(li)) {
                        *r = true;
                    }
                }
            }
        }
        for (pos, heading) in askers {
            for (l, d) in self.lanes_ahead_of(pos, heading, 160.0) {
                if let Some((ci, li)) = self.net.lanes[l].traffic_light {
                    if let Some(ctl) = self.lights.get_mut(ci) {
                        if d <= ctl.approach_dist(li) {
                            if let Some(r) = ctl.request.get_mut(li) {
                                *r = true;
                            }
                        }
                    }
                }
            }
        }
        let mut walkers: HashMap<usize, Vec<f32>> = HashMap::new();
        for &(l, s) in &self.walkers {
            walkers.entry(l).or_default().push(s);
            let Some(lane) = self.net.lanes.get(l) else {
                continue;
            };
            // the push button of a pedestrian light on the way
            for (ahead, dist) in lane
                .next
                .iter()
                .map(|&n| (n, lane.length() - s))
                .chain(self.net.prev.get(l).into_iter().flatten().map(|&p| (p, s)))
                .chain(std::iter::once((l, 0.0)))
            {
                if let Some((ci, li)) = self.net.lanes.get(ahead).and_then(|x| x.traffic_light) {
                    if let Some(ctl) = self.lights.get_mut(ci) {
                        if dist <= ctl.approach_dist(li).min(10.0) {
                            if let Some(r) = ctl.request.get_mut(li) {
                                *r = true;
                            }
                        }
                    }
                }
            }
        }
        let day_time = self.day_time;
        for c in self.lights.iter_mut() {
            c.start(day_time);
            c.advance(dt);
        }
        self.log_lights();
        walkers
    }

    /// 3a. Player/other-player standing times for the tick. Takes the external actors out of
    /// `self` for the tick; they are put back by the adapter after the tick.
    fn tick_presence(
        &mut self,
        dt: f32,
        player: Option<PlayerBox>,
    ) -> (f32, Vec<(u32, PlayerBox)>) {
        self.player_still = match player {
            Some(p) if p.4.abs() < 0.3 => self.player_still + dt,
            _ => 0.0,
        };
        let player_standing = self.player_still;
        let others = std::mem::take(&mut self.others);
        let mut others_still: HashMap<u32, f32> = HashMap::new();
        for (id, b) in &others {
            let before = self.others_still.get(id).copied().unwrap_or(0.0);
            others_still.insert(*id, if b.4.abs() < 0.3 { before + dt } else { 0.0 });
        }
        self.others_still = others_still;
        (player_standing, others)
    }

    /// 3b. Build the frozen `Occupancy`/lane view/junction, service and maneuver actors once,
    /// then run the per-vehicle decision loop. Returns the frozen data `tick_realize` and
    /// `tick_finish` still need.
    fn tick_plan(
        &mut self,
        dt: f32,
        player: Option<PlayerBox>,
        others: &[(u32, PlayerBox)],
        player_standing: f32,
        walkers: HashMap<usize, Vec<f32>>,
        debug: bool,
    ) -> TickFrame {
        let previous_odometer = self.cars.iter().map(|c| c.state.odometer).collect();
        // One immutable occupancy snapshot for the tick: realized bodies plus their lane
        // placements, built once. Geometry is the truth; `by_lane` is a flat id-keyed view
        // for the checks not yet migrated (leader scans, lane changes, passing).
        let occupancy = Occupancy::build(
            self.net.version(),
            (self.time * 1000.0).max(0.0) as u64,
            self.body_feet(player, others),
        );
        let by_lane = occupancy.lane_view(&self.index_of);
        let maneuver_people: Vec<_> = self.people.iter().map(|&(p, _, _)| p).collect();
        let road_collision = self.road_collision.clone();
        let maneuver_geometry: HashMap<_, _> = self.cars.iter().map(|c| {
            let parts: Vec<_> = c.vehicle.trailers.iter().filter_map(|t| {
                let (back, front) = t.couplings();
                Some(safety::SweepTrailer { position: t.position, heading: t.heading,
                    back: back.truncate().as_dvec2(), front: front.truncate().as_dvec2(),
                    length: t.pivot_length() as f64, bbox: t.ty.def.bounding_box?,
                    lift: t.position.z - c.vehicle.contact.as_deref()
                        .and_then(|g| g.road_height(t.position.x, t.position.y, t.position.z, 1.5))
                        .unwrap_or(c.body.position.z),
                    max_angle: t.ty.def.coupling_front_character
                        .filter(|c| c[3] != 0.0 && c[0] > 0.0).map(|c| c[0] as f64) })
            }).collect();
            (c.id, (c.vehicle.contact.clone(), c.caps.rear, parts, c.body.clone()))
        }).collect();
        let scenery_clear = |samples: &[::traffic::perception::SweepSample], actor: &ManeuverActor| {
            let Some((contact, rear, parts, body)) = maneuver_geometry.get(&actor.id) else { return false; };
            safety::articulated_clear(&road_collision, &occupancy, samples, actor, contact.as_deref(), *rear, parts, body)
        };
        // The external road users' synthetic ids (see `body_feet`): bodies to keep clear of,
        // not AI blockers to sort out by `geo_block`.
        let mut external_ids: Vec<VehicleId> = vec![VehicleId(u64::MAX)];
        external_ids.extend(
            others
                .iter()
                .map(|(id, _)| VehicleId(u64::MAX - 1 - *id as u64)),
        );
        // cars coming to a junction lane: (car, distance from its origin to the lane start)
        let mut coming: HashMap<usize, Vec<(usize, f32)>> = HashMap::new();
        let mut way_scratch: Vec<(usize, f32)> = Vec::new();
        for (i, c) in self.cars.iter().enumerate() {
            self.way_lanes_into(&c.state, LOOK_AHEAD + 30.0, &mut way_scratch);
            for &(l, d) in way_scratch.iter().skip(1) {
                if !self.net.crossings[l].is_empty() {
                    coming.entry(l).or_default().push((i, d));
                }
            }
        }
        // The junction coordinator owns its own claim/store state (see `begin_tick` below);
        // exit storage is recomputed fresh this tick against the realized occupancy.
        let mut remove = Vec::new();
        let mut frames: Vec<Option<AiFrame>> = vec![None; self.cars.len()];
        self.break_lead_pairs();
        // The junction actors and the signal aspects are frozen once for the tick; all
        // junction decisions read this snapshot, and the coordinator owns the claims.
        let mut junction_actors: Vec<JunctionActor> = self
            .cars
            .iter()
            .map(|c| JunctionActor {
                // (an emergency vehicle that has stood for a while gives up its rights until
                // it moves again: its reservation and everybody making way for it otherwise
                // held the whole junction, and nothing could resolve what held it)
                emergency: emergency_drive(&c.vehicle, c.is_bus()) && c.stopped < EMERGENCY_STUCK_AFTER,
                id: c.id,
                lane: c.state.lane,
                s: c.state.s,
                front: c.state.front,
                rear: c.state.rear,
                length: c.state.length,
                min_gap: c.state.min_gap,
                speed: c.state.speed,
                accel: c.state.accel,
                decel: c.state.decel,
                reaction: c.state.reaction,
                accept_gap: c.state.accept_gap,
                yield_time: c.state.yield_time,
                stopped: c.stopped,
                crawl: c.crawl,
                lead_info: c.lead_info,
                light_hold: c.light_hold,
                yielding: c.yielding,
                wait_at: c.wait_at,
                priority: c.vehicle.var("TrafficPriority").is_some_and(|v| v > 0.5),
            })
            .collect();
        let aspects: HashMap<(usize, usize), Aspect> = self
            .lights
            .iter()
            .enumerate()
            .flat_map(|(ci, ctl)| {
                (0..ctl.lights.len())
                    .map(move |li| ((ci, li), ctl.vehicle_aspect(li)))
            })
            .collect();
        self.junctions.begin_tick((self.time * 1000.0).max(0.0) as u64);
        // LAN players on an emergency drive (their main body; trailers share the id)
        let mut remote_emergencies: Vec<(u32, PlayerBox)> = Vec::new();
        for &(id, b) in others {
            if self.external_emergencies.contains(&id) && !remote_emergencies.iter().any(|r| r.0 == id) {
                remote_emergencies.push((id, b));
            }
        }
        let prepare_emergency = self.player_emergency || !remote_emergencies.is_empty()
            || self.junctions.has_emergency_reservations()
            || junction_actors.iter().any(|a| a.emergency);
        let mut emergency_ways: Vec<_> = if prepare_emergency {
            self.cars.iter().map(|c| self.way_lanes(&c.state, 160.0)).collect()
        } else { Vec::new() };
        let mut junction_on_lane = by_lane.clone();
        let mut junction_coming = coming.clone();
        let external = player
            .and_then(|p| safety::external_actor(&self.net, VehicleId(u64::MAX), p, self.player_emergency))
            .into_iter()
            .chain(remote_emergencies.iter().filter_map(|&(id, b)| {
                safety::external_actor(&self.net, VehicleId(u64::MAX - 1 - id as u64), b, true)
            }));
        for (actor, way) in external {
            let index = junction_actors.len();
            junction_on_lane.entry(actor.lane).or_default().push((index, actor.s, 0.0, false));
            for &(l, d) in way.iter().skip(1) {
                junction_coming.entry(l).or_default().push((index, d));
            }
            junction_actors.push(actor);
            if prepare_emergency { emergency_ways.push(way); }
        }
        if prepare_emergency {
            self.junctions.prepare_emergencies(&self.net, &junction_actors, &emergency_ways, &junction_on_lane);
        }
        let emergency_drives: Vec<_> = junction_actors.iter().zip(&emergency_ways)
            .filter(|(a, _)| a.emergency).map(|(a, way)| ::traffic::EmergencyDrive {
                vehicle: a.id, way: way.clone(), front: a.front, speed: a.speed,
            }).collect();
        // The service actors are frozen once too; the coordinator owns berth capacity and
        // decides the service phases. Arrival order is recorded when a bus first comes
        // within STOP_REACH of a stop, so a queue is assigned by stable arrival, not by
        // container position.
        let service_actors: Vec<ServiceActor> = self
            .cars
            .iter()
            .map(|c| ServiceActor {
                id: c.id,
                lane: c.state.lane,
                s: c.state.s,
                front: c.state.front,
                rear: c.state.rear,
                length: c.state.length,
                speed: c.state.speed,
                lateral: c.state.lateral,
                min_gap: c.state.min_gap,
            })
            .collect();
        let service_intents: Vec<BerthIntent> = self
            .cars
            .iter()
            .filter_map(|c| {
                let b = c.bus.as_ref()?;
                if let Some(held) = b.state.berth {
                    return Some(BerthIntent {
                        vehicle: c.id,
                        stop: held.stop,
                        occurrence: held.occurrence,
                        holds: true,
                    });
                }
                let t = b.stops.front()?;
                c.state.route.get(t.route_index)?;
                let d = c.state.route_distance(&self.net, t.route_index, t.s);
                (d <= STOP_REACH).then_some(BerthIntent {
                    vehicle: c.id,
                    stop: t.stop,
                    occurrence: t.occurrence,
                    holds: false,
                })
            })
            .collect();
        self.services
            .begin_tick(&service_intents, (self.time * 1000.0).max(0.0) as u64);
        // The maneuver actors are frozen once too; the maneuver coordinator owns every lateral
        // decision and orders simultaneous lane changes by stable id.
        let maneuver_actors: Vec<ManeuverActor> = self
            .cars
            .iter()
            .map(|c| {
                let st = &c.state;
                let h = c.vehicle.heading.to_radians();
                let fwd = glam::DVec2::new(h.sin(), h.cos());
                let mut rear = st.rear;
                for tr in &c.vehicle.trailers {
                    if let Some(bb) = tr.ty.def.bounding_box {
                        let o = ::simulation::collision::Obb::from_box(bb, tr.position, tr.heading);
                        let behind = -(o.center - c.vehicle.position.truncate()).dot(fwd) + o.half.y;
                        rear = rear.max(behind as f32);
                    }
                }
                ManeuverActor {
                    id: c.id,
                    lane: st.lane,
                    s: st.s,
                    lateral: st.lateral,
                    speed: st.speed,
                    accel: st.accel,
                    decel: st.decel,
                    reaction: st.reaction,
                    desire: st.desire,
                    max_speed_kmh: st.max_speed_kmh,
                    front: st.front,
                    rear,
                    length: st.length,
                    half_width: c.half_width,
                    height: safety::vehicle_height(&c.vehicle),
                    odometer: st.odometer,
                    min_gap: st.min_gap,
                    veh_type: st.veh_type,
                    lane_kind: self.net.lanes[st.lane].kind,
                    planned_next: st.planned_next,
                    route_next: st
                        .route
                        .get(st.route_index + 1)
                        .copied()
                        .filter(|&b| self.net.parallel(st.lane, b)),
                    turn_wish: st.turn_wish,
                    change: st.change.map(|ch| ChangeInfo {
                        to: ch.to,
                        dir: ch.dir,
                        t: ch.t,
                        length: ch.length,
                        s_to: ch.s_to,
                        wait: ch.wait,
                        bypass: ch.bypass,
                    }),
                    stopped: c.stopped,
                    light_hold: c.light_hold,
                    yielding: c.yielding,
                    at_stop: c.at_stop(),
                    pass_room: c.pass_room,
                    lat_accel: st.lat_accel,
                }
            })
            .collect();
        let intent_scene = ManeuverScene { static_clearance: None, net: &self.net, occupancy: &occupancy,
            actors: &maneuver_actors, people: &[], time: self.time, dt, tick: (self.time * 1000.0).max(0.0) as u64 };
        let maneuver_intents: Vec<ManeuverIntent> = maneuver_actors.iter().zip(&self.cars)
            .map(|(a, c)| self.maneuvers.intent(&intent_scene, a, &c.maneuver)).collect();
        self.maneuvers
            .begin_tick(&maneuver_intents, (self.time * 1000.0).max(0.0) as u64);
        // (the debug switches are asked once a tick, not once a car)
        let debug_car = ::legacy_config::env::var("OMSI_DEBUG_CAR")
            .ok()
            .and_then(|v| v.parse::<u64>().ok());
        let debug_doors = ::legacy_config::env::var_os("OMSI_DEBUG_DOORS").is_some();
        for i in 0..self.cars.len() {
            self.cars[i].state.emergency_drive = junction_actors[i].emergency;
            let ahead = self.obstacle_ahead(i, look_ahead(self.cars[i].state.speed), &by_lane);
            let body_ahead = self.body_in_way(i, &occupancy, &external_ids);
            if debug && self.cars[i].stopped >= 0.5 && self.cars[i].stopped < 0.5 + dt {
                let id = self.cars[i].id;
                let indexed = ahead.map(|(l, j)| (self.cars[j].id, l.gap));
                let physical = body_ahead.map(|(l, j)| (self.cars[j].id, l.gap));
                let feet: Vec<_> = occupancy.feet().iter()
                    .filter(|f| Some(f.owner) == indexed.map(|x| x.0) || Some(f.owner) == physical.map(|x| x.0))
                    .map(|f| (f.owner, f.part, f.center, f.fwd, f.half_len, f.half_w)).collect();
                log::info!("AI perception t={:.2} car {id}: indexed {indexed:?}, body {physical:?}, lane {} upcoming {:?}, feet {feet:?}",
                    self.time, self.cars[i].state.lane, self.cars[i].state.upcoming().take(3).collect::<Vec<_>>());
            }
            // remember whom it lets in at a merge (a car on another lane)
            let merging = ahead
                .filter(|(_, j)| {
                    self.cars[*j].state.lane != self.cars[i].state.lane
                        && !self.cars[i]
                        .state
                        .upcoming()
                        .any(|u| u == self.cars[*j].state.lane)
                })
                .map(|(_, j)| self.cars[j].id);
            self.cars[i].merge_after = merging;
            let mut lead = ahead.map(|(l, j)| (l, Some(j)));
            // the player's bus, wherever it overlaps this car's way, or a LAN player's (the
            // nearest in the way stands for "the player's bus" in what follows)
            let (mut player, mut player_standing) = (player, player_standing);
            if let Some(p) = player.as_ref() {
                if let Some(l) = self.player_in_way(i, p) {
                    if lead.map(|x| l.gap < x.0.gap).unwrap_or(true) {
                        lead = Some((l, Some(usize::MAX)));
                    }
                }
            }
            for (id, o) in others {
                if let Some(l) = self.player_in_way(i, o) {
                    if lead.map(|x| l.gap < x.0.gap).unwrap_or(true) {
                        lead = Some((l, Some(usize::MAX)));
                        player = Some(*o);
                        player_standing = self.others_still.get(id).copied().unwrap_or(0.0);
                    }
                }
            }
            // other vehicles' bodies in the way off the lanes
            if let Some((l, j)) = body_ahead {
                self.cars[i].geo_block = Some(self.cars[j].id);
                if lead.map(|x| l.gap < x.0.gap - 0.5).unwrap_or(true) {
                    if debug
                        && l.gap < 3.0
                        && l.speed < 0.5
                        && self.cars[i].stopped == 0.0
                        && self.cars[i].state.speed > 0.5
                    {
                        log::info!(
                            "t={:.1}: car {} stops for the body of car {} in its way ({:.1} m) off the lanes",
                            self.time,
                            self.cars[i].id,
                            self.cars[j].id,
                            l.gap
                        );
                    }
                    lead = Some((l, Some(j)));
                }
            }
            if let Some((_, Some(j))) = lead {
                if j < self.cars.len()
                    && self.cars[i].ignore_lead.is_some_and(|(id, until)| {
                    id == self.cars[j].id && (self.time as f64) < until
                })
                {
                    lead = None;
                }
            }
            // parked cars: stop behind one in the middle of the lane, swerve round one at
            // the kerb (a parked car eats the right half of the lane; the passing car
            // moves left by what is missing, and back once it is past)
            let mut parked_ahead = false;
            let kerb_swerve: Option<f32>;
            let mut squeeze: Option<VehicleId> = None;
            // (the car's state does not change before the maneuver block below: one lookahead
            // serves both the parked-car check and the plans)
            self.way_lanes_into(&self.cars[i].state, 200.0, &mut way_scratch);
            {
                let car = &self.cars[i];
                let st = &car.state;
                let near_way = Self::way_prefix(&way_scratch, 100.0);
                let passing = car.maneuver.passing.map(|p| !p.aborted).unwrap_or(false);
                let mut swerve: Option<f32> = None;
                let mut stand: Option<(f32, usize, f32, f32)> = None;
                let mut check = |along: f32, lat: f32, lane: usize, at: f32, width: f32, length: f32| {
                    if !(-6.0..=100.0).contains(&along) {
                        return;
                    }
                    let a = lat.abs();
                    let side_extent = if along < st.front + 3.3 {
                        let lane_heading = self.net.lanes[st.lane].at(st.s).1;
                        safety::side_extent(st.front, st.rear, car.half_width,
                            car.vehicle.heading as f32 - lane_heading, lat.signum())
                    } else { car.half_width };
                    // in the way at the side the car is on now (a car pulled out onto the
                    // other half passes it)
                    let blocks = if passing {
                        (lat - st.lateral_ahead(along)).abs() < car.half_width + width + 0.2
                    } else {
                        a < 0.9
                    };
                    if blocks {
                        if along > 0.0 {
                            let gap = along - length - st.front;
                            if stand.map(|o| gap < o.0).unwrap_or(true) {
                                stand = Some((gap, lane, at, lat));
                            }
                        }
                    } else if !passing && a < side_extent + width + 0.15 && along < 30.0 {
                        // (only as far as the two bodies would touch: OMSI's cars keep to
                        // their paths, and moved out by a margin of our own round every car
                        // at the kerb - 2.7 m from the lane's middle - the traffic of a
                        // narrow British street lined with parked cars wove to and fro
                        // across the road instead of keeping to its lane)
                        let need = (side_extent + width + 0.15 - a) * -lat.signum();
                        swerve = Some(
                            swerve
                                .map(|w| if w.abs() > need.abs() { w } else { need })
                                .unwrap_or(need),
                        );
                    }
                };
                // (once committed to a lane change - well over, or pulling out round what
                // stands in the way - the parked cars of the lane it leaves hold it no more,
                // as the cars standing there do not, `obstacle_ahead`: counted still, the car
                // that had begun to pull out round a row of them stopped with its nose on the
                // first, and a lane change that moves on with the car never got anywhere -
                // six cars queued for good behind the parked row on the Heerstraße)
                let leaving = st
                    .change
                    .filter(|c| c.t > 0.4 || (c.bypass && c.wait <= 0.0))
                    .map(|_| st.lane);
                for &(l, d) in near_way {
                    if Some(l) == leaving {
                        continue;
                    }
                    for &(s, lat) in self.parked.get(&l).map(|v| v.as_slice()).unwrap_or(&[]) {
                        check(d + s, lat, l, s, 0.9, 2.3);
                    }
                }
                if !passing && leaving.is_none() {
                    // Lane assignment is a population hint, not physical clearance.
                    // Project nearby actual parked boxes onto this route as well, so
                    // a car on the next/adjacent segment cannot hide a corner from a bus.
                    let probe = Obb::vehicle(car.vehicle.position.truncate(), car.vehicle.heading,
                        35.0, st.rear as f64 + 3.0, 6.0);
                    let route: Vec<_> = near_way.iter().map(|w| w.0).collect();
                    for j in self.parked_collision.near(&probe) {
                        let parked = &self.parked_collision.boxes[j];
                        let p = parked.center.extend(parked.z0);
                        let Some((ri, at, lat)) = self.net.project_on_route_lateral(&route, p) else { continue };
                        let (lane, d) = near_way[ri];
                        let (q, heading) = self.net.lanes[lane].at(at);
                        if (p.z - q.z).abs() > 2.0 || (p - q).truncate().length() > 6.0 { continue; }
                        let h = (heading as f64).to_radians();
                        let right = DVec2::new(h.cos(), -h.sin());
                        let forward = DVec2::new(h.sin(), h.cos());
                        let [r, f] = parked.axes();
                        let width = (parked.half.x * r.dot(right).abs() + parked.half.y * f.dot(right).abs()) as f32;
                        let length = (parked.half.x * r.dot(forward).abs() + parked.half.y * f.dot(forward).abs()) as f32;
                        check(d + at, lat, lane, at, width, length);
                    }
                }
                // a bus standing half in its bay: squeeze past on
                // the other side when a metre is enough, instead of queueing behind it -
                // and stay out until past its front (moving back in while still beside it
                // steered the car into the bus)
                if !passing {
                    let swerving = st.lateral_target.abs() > 0.1;
                    for &(l, d) in near_way {
                        for &(j, os, lat, foreign) in
                            by_lane.get(&l).map(|v| v.as_slice()).unwrap_or(&[])
                        {
                            let o = &self.cars[j];
                            let along = d + os;
                            if foreign
                                || j == i
                                || lat.abs() < 0.5
                                || !(-(o.state.front + st.rear + 1.0)..=40.0).contains(&along)
                            {
                                continue;
                            }
                            // a bus that is about to pull away (its last seconds at the stop, the
                            // indicator on) is not started round; one the car is already going
                            // round is passed, unless the car can still stop behind it gently
                            // (only a bus at its stop stands out of the lane on purpose: a car
                            // off the middle is squeezing past something itself)
                            let standing =
                                o.state.speed < 0.3 && o.standing_for(self.day_time) > 3.0;
                            let keep = swerving
                                && car.squeeze == Some(o.id)
                                && (o.state.speed < 2.0
                                || along
                                < o.state.front
                                + st.front
                                + st.speed * st.speed / (2.0 * st.decel.max(1.0)));
                            if !standing && !keep {
                                continue;
                            }
                            let need = car.half_width + o.half_width + 0.35 - lat.abs();
                            if need > 0.0 && need <= 1.1 {
                                let w = need * -lat.signum();
                                if swerve.map(|v: f32| w.abs() > v.abs()).unwrap_or(true) {
                                    swerve = Some(w);
                                    squeeze = Some(o.id);
                                }
                            }
                        }
                    }
                }
                if let Some((gap, _pl, _ps, _lat)) = stand {
                    // stop a little further back than behind a car that will move on
                    let l = Lead {
                        gap: (gap - 2.0).max(0.0),
                        speed: 0.0,
                        acc: 0.0,
                    };
                    if lead.map(|x| l.gap < x.0.gap).unwrap_or(true) {
                        lead = Some((l, None));
                        parked_ahead = true;
                    }
                }
                if swerve.is_none() {
                    if let Some(ramp) = car.maneuver.kerb_ramp.filter(|r|
                        st.odometer < r.2 + r.3 + st.rear + 3.0)
                    { swerve = Some(ramp.1); }
                }
                if swerve.is_none()
                    && (car.motion_fault.is_some() || car.scenery_streak > 0.0 || car.scenery_ahead.is_some())
                {
                    swerve = safety::corner_swerve(&self.road_collision, &occupancy, car.id, &self.net, st,
                        &car.body, &car.vehicle, &car.caps, car.scenery_ahead);
                }
                kerb_swerve = swerve;
            }
            if squeeze.is_some()
                && self.cars[i].squeeze.is_none()
                && self.first_passer.is_none()
                && !self.cars[i].is_bus()
            {
                self.first_passer = Some((self.cars[i].id, self.time));
            }
            self.cars[i].squeeze = squeeze;
            let standing = self.standing_obstacle(i, lead, parked_ahead, player_standing);
            // (a queue at a stop is passed as a whole)
            let (obstacle_len, at_stop) = match lead.and_then(|l| l.1) {
                Some(usize::MAX) => (
                    player.map(|p| p.2 * 2.0).unwrap_or(12.0),
                    player_standing > 10.0,
                ),
                Some(j) if j < self.cars.len() => self.standing_queue(j),
                _ => (4.8, false),
            };
            self.cars[i].lead_info = lead.and_then(|(l, who)| {
                who.filter(|&j| j < self.cars.len())
                    .map(|j| (self.cars[j].id, l.gap))
            });
            // Something that may stand for a while (a bus at its stop, the player's bus that
            // has stopped) is waited behind with room to pull out round it later: a car that
            // had stopped a metre behind the player's bus scraped its corner when it went
            // round, and no car can steer out of that.
            let may_stand = lead
                .map(|(l, who)| {
                    match who {
                        Some(usize::MAX) => l.speed.abs() < 0.3
                            && player.map(|p| p.4.abs() < 0.3).unwrap_or(false),
                        Some(j) if j < self.cars.len() => {
                            let leader = &self.cars[j];
                            (l.speed.abs() < 0.3 && leader.at_stop())
                                || leader.bus.as_ref().is_some_and(|b| b.state.phase == ServicePhase::Docking)
                        },
                        _ => false,
                    }
                })
                .unwrap_or(false);
            // It stops `pass_room` short of it (the room its own steering needs to get out
            // round it), or as far back as it can without braking hard. The two metres taken
            // off the gap it keeps to such a thing were not enough: the car still crept up to
            // under three metres behind the player's bus and never got round it.
            let mut keep_back: Option<f32> = None;
            let mut reserve_pull_out = false;
            // (an emergency vehicle keeps the room to pull out behind whatever slows in front
            // of it: closed up to two metres behind a car making way, an ambulance could not
            // get round it and stood there, blocking the road)
            let emergency_behind_slow = junction_actors[i].emergency
                && lead.is_some_and(|l| l.0.speed < 3.0);
            if standing || may_stand || emergency_behind_slow {
                if let Some((l, who)) = lead.filter(|_| !parked_ahead) {
                    let car = &self.cars[i];
                    let st = &car.state;
                    let real = l.gap
                        + if who == Some(usize::MAX) {
                        PLAYER_BOX_MARGIN
                    } else {
                        0.0
                    };
                    // (a timetable bus queueing for its own stop is not going round it)
                    let queues = car
                        .next_stop()
                        .map(|(ri, ss)| {
                            ri >= st.route_index
                                && st.route_distance(&self.net, ri, ss)
                                < real + obstacle_len + st.front + 15.0
                        })
                        .unwrap_or(false);
                    let want = if queues {
                        st.min_gap + 2.0
                    } else {
                        car.pass_room.max(st.min_gap)
                    };
                    reserve_pull_out = !queues;
                    let comfortable = st.speed * st.speed / (2.0 * st.decel.max(1.0));
                    let stop_gap = if real - want >= comfortable {
                        want
                    } else {
                        // (a gap wanted under half a metre is the floor itself: clamp
                        // panicked with its bounds the wrong way round, #138)
                        (real - comfortable).clamp(real.min(0.5).min(want), want)
                    };
                    keep_back = Some(st.front + (real - stop_gap).max(0.0) + 0.6);
                }
            }
            // The maneuver owner decides every lateral intent from the frozen scene: passing,
            // lane changes, bypass, route changes and the kerb swerve round a parked car. No
            // other function writes `lateral_target`.
            let way = &way_scratch[..];
            let merge_wait: Option<f32>;
            let mut maneuver_why: Option<(Reason, f32)> = None;
            {
                let mut inputs = ManeuverInputs::new(i);
                inputs.kerb_swerve = kerb_swerve;
                inputs.lead_gap = lead.map(|l| l.0.gap);
                inputs.lead_standing = standing;
                inputs.obstacle_len = obstacle_len;
                inputs.parked = parked_ahead || at_stop;
                inputs.priority_pass = junction_actors[i].emergency;
                inputs.queue_for_stop = self.cars[i].next_stop().is_some_and(|(ri, ss)| {
                    let st = &self.cars[i].state;
                    ri >= st.route_index && st.route_distance(&self.net, ri, ss)
                        < inputs.lead_gap.unwrap_or(0.0) + obstacle_len + st.front + 15.0
                });
                // (an emergency vehicle also passes a car still slowing down to make way, out
                // on a free oncoming lane, as a real one does)
                inputs.lead_speed = lead.map(|l| l.0.speed).unwrap_or(0.0);
                inputs.lead_standing |= inputs.priority_pass
                    && lead.is_some_and(|l| l.0.speed < EMERGENCY_PASS_LEAD_SPEED);
                if !inputs.priority_pass {
                    let car = &self.cars[i];
                    inputs.emergency = ::traffic::approaching_emergency(
                        &self.net, car.id, &car.state, car.state.rear, &emergency_drives,
                    );
                }
                let decision = {
                    let scene = ManeuverScene {
                        static_clearance: Some(&scenery_clear),
                        net: &self.net,
                        occupancy: &occupancy,
                        actors: &maneuver_actors,
                        people: &maneuver_people,
                        time: self.time,
                        dt,
                        tick: (self.time * 1000.0).max(0.0) as u64,
                    };
                    self.maneuvers
                        .plan(&scene, &mut self.cars[i].maneuver, &inputs)
                };
                merge_wait = decision.stop_at;
                if let Some(binding) = decision.binding {
                    maneuver_why = Some((binding, decision.stop_at.unwrap_or(0.0)));
                }
                if let Some(cmd) = decision.change {
                    let net = &self.net;
                    let car = &mut self.cars[i];
                    match cmd.kind {
                        ChangeKind::RouteChange => car.state.start_route_change(net, cmd.to, cmd.dir),
                        ChangeKind::Bypass => car.state.start_bypass(net, cmd.to, cmd.dir),
                        ChangeKind::Change => car.state.start_change(net, cmd.to, cmd.dir),
                    }
                }
                let car = &mut self.cars[i];
                if let Some(t) = decision.lateral_target {
                    car.state.lateral_target = t;
                }
                if let Some(r) = decision.lateral_ramp {
                    car.state.lateral_ramp = r;
                }
                if let Some((blinker, dur)) = decision.signal {
                    car.state.signal = blinker;
                    car.state.signal_time = car.state.signal_time.max(dur);
                }
                if let Some(cap) = decision.accel_cap {
                    car.state.accel_cap = Some(cap);
                } else {
                    car.state.accel_cap = None;
                }
                if let Some(v) = self.junctions.emergency_speed_cap(car.id) {
                    let cap = ((v - car.state.speed) / 0.5).clamp(-car.state.decel, car.state.accel);
                    car.state.accel_cap = Some(car.state.accel_cap.map_or(cap, |a| a.min(cap)));
                }
            }
            // The coordinator decides the signal hold and the right of way from the frozen
            // view; it is the only writer of junction claims.
            let decision = self.junction_plan(
                i,
                way,
                lead.map(|l| l.0),
                &junction_on_lane,
                &junction_coming,
                &walkers,
                &junction_actors,
                &aspects,
            );
            let light = decision.light;
            let yield_at = decision.yield_at;
            self.cars[i].light_hold = light.is_some();
            self.cars[i].light_at = light;
            self.cars[i].junction_state = decision.state;
            if light.is_some() && self.cars[i].state.speed < 0.5 {
                self.held_at_red += 1;
                if self.first_red.is_none()
                    && self.cars[i].state.speed < 0.2
                    && !self.cars[i].is_bus()
                {
                    self.first_red = Some((self.cars[i].id, self.time));
                }
            }
            // right of way: at every junction before the red light's line (and in the one the
            // car is in already) - skipping them all whenever some light ahead was red let a
            // car cross another's path unchecked on its way to a light further on
            let junction = if self.net.lanes[self.cars[i].state.lane].kind == LaneKind::Air {
                None
            } else {
                junction_ahead(&self.net, way).filter(|jn| {
                    light
                        .map(|l| jn.inside || jn.lanes[0].1 < l - 0.5)
                        .unwrap_or(true)
                })
            };
            // what it has claimed and is through no longer counts
            {
                let on_way: Vec<usize> = way.iter().map(|w| w.0).collect();
                self.junctions.retain_on_way(self.cars[i].id, &on_way);
                self.maneuvers.retain_on_way(self.cars[i].id, &on_way);
                let car = &mut self.cars[i];
                car.yielding = yield_at.is_some();
                car.wait_at = yield_at;
                let st = &mut car.state;
                if yield_at.is_some() && st.speed < 0.3 {
                    st.yield_time += dt;
                } else if yield_at.is_none() {
                    st.yield_time = 0.0;
                }
                // (`--follow yield`: a car that has stood for a couple of seconds giving way at
                // a junction without lights)
                if car.yielding
                    && st.yield_time >= 2.0
                    && st.yield_time - dt < 2.0
                    && self.first_yield.is_none()
                    && !car.is_bus()
                    && junction
                    .as_ref()
                    .map(|j| {
                        j.lanes.iter().all(|l| {
                            self.net.lanes[l.0].traffic_light.is_none()
                                && self.net.prev[l.0]
                                .iter()
                                .all(|&p| self.net.lanes[p].traffic_light.is_none())
                        })
                    })
                    .unwrap_or(false)
                {
                    self.first_yield = Some((car.id, self.time));
                }
            }
            let for_people = if self.net.lanes[self.cars[i].state.lane].kind == LaneKind::Air {
                None
            } else {
                self.people_stop(i, way)
            };
            if let Some((at, who)) = for_people {
                let car = &self.cars[i];
                if debug && car.state.speed > 0.5 {
                    log::info!(
                        "t={:.2}: car {} stops for somebody on foot {:.1} m ahead",
                        self.time,
                        car.id,
                        at - car.state.front
                    );
                }
                // somebody who never moves out of the way (standing in the carriageway)
                if car.stopped >= 20.0 && car.stopped - dt < 20.0 {
                    log::info!(
                        "car {} has stood 20 s for somebody on foot at ({:.1}, {:.1})",
                        car.id,
                        who.x,
                        who.y
                    );
                }
            }
            let people = for_people.map(|x| x.0);
            let ground_hold = (!self.cars[i].body.ground_supported)
                .then_some(self.cars[i].state.front + 0.1);
            let motion_hold = self.cars[i].motion_fault.map(|_| self.cars[i].state.front + 0.1);
            // scenery the body would touch ahead on its own way: stop short of it, as for
            // anything standing there
            let scenery_hold = self.cars[i]
                .scenery_ahead
                .map(|d| self.cars[i].state.front + (d - SCENERY_STOP_MARGIN).max(0.0));
            let mut stop_at = [light, yield_at, merge_wait, keep_back, people, ground_hold, motion_hold, scenery_hold]
                .into_iter()
                .flatten()
                .reduce(f32::min);
            let mut why: (Reason, f32) = (Reason::NONE, f32::MAX);
            for (reason, v) in [
                (Reason::RedSignal, light),
                (Reason::Yield, yield_at),
                (Reason::Yield, merge_wait),
                (Reason::Leader, keep_back),
                (Reason::Pedestrian, people),
                (Reason::GroundUnavailable, ground_hold),
                (Reason::SceneryBlocked, motion_hold),
                (Reason::SceneryBlocked, scenery_hold),
                (
                    maneuver_why.map(|x| x.0).unwrap_or(Reason::NONE),
                    maneuver_why.map(|x| x.1),
                ),
            ] {
                if let Some(v) = v {
                    if v < why.1 {
                        why = (reason, v);
                    }
                }
            }
            // held standing by scenery with no way round: give up on it for a while
            {
                let car = &mut self.cars[i];
                let held = why.0 == Reason::SceneryBlocked && car.state.speed.abs() < 0.3;
                car.scenery_wait = if held { car.scenery_wait + dt } else { 0.0 };
                if car.scenery_wait >= SCENERY_GHOST_AFTER {
                    car.scenery_wait = 0.0;
                    car.scenery_ghost = car.state.odometer + car.state.length + SCENERY_GHOST_MARGIN;
                    car.scenery_ahead = None;
                    car.motion_fault = None;
                    car.scenery_streak = 0.0;
                    log::info!(
                        "t={:.1}: car {} held by scenery at lane {} s {:.1}: ignoring scenery for {:.0} m",
                        self.time,
                        car.id,
                        car.state.lane,
                        car.state.s,
                        car.state.length + SCENERY_GHOST_MARGIN
                    );
                    // (this tick's hold is lifted too, the next one would not have it)
                    stop_at = [light, yield_at, merge_wait, keep_back, people, ground_hold]
                        .into_iter()
                        .flatten()
                        .reduce(f32::min);
                }
            }
            // An AI driver sounds its horn (`ev_AI_Horn`) when held standing at low speed
            // behind a non-moving obstruction. This is a documented provisional neoOMSI
            // trigger (the reference proves only that the event exists), it is presentation
            // feedback only, and it is never a way to resolve a blocked maneuver: it does not
            // touch stop_at, claims or admission. The script ignores an event it has none of.
            let mut horn_reason: Option<Reason> = None;
            {
                let car = &mut self.cars[i];
                car.horn_cooldown = (car.horn_cooldown - dt).max(0.0);
                if car.horn_cooldown <= 0.0
                    && !car.is_bus()
                    && !car.is_rail()
                    && car.stopped >= HORN_HOLD
                    && car.state.speed < HORN_SPEED
                    && matches!(
                        why.0,
                        Reason::Leader
                            | Reason::Pedestrian
                            | Reason::Yield
                            | Reason::OccupiedExit
                            | Reason::JunctionClaim
                            | Reason::Passing
                    )
                {
                    car.horn_cooldown = HORN_COOLDOWN;
                    let _ = car.vehicle.trigger("ev_AI_Horn");
                    horn_reason = Some(why.0);
                }
            }
            if let Some(reason) = horn_reason {
                let vehicle = self.cars[i].id;
                self.emit_trace(TraceEvent::Horn { vehicle, reason });
            }
            self.cars[i].held = stop_at.is_some() || lead.map(|l| l.0.gap < 12.0).unwrap_or(false);
            // a timetable bus: its stops (see `bus_service`); any other car keeps to the middle
            // of its lane, or swerves round a car parked at the kerb
            let berth_held_long = {
                let c = &self.cars[i];
                c.bus
                    .as_ref()
                    .and_then(|s| s.front_berth(&c.state.route))
                    .and_then(|b| {
                        let (stop, occurrence) = b.key();
                        self.services.berth_owner(stop, occurrence)
                    })
                    .filter(|&o| o != c.id)
                    .and_then(|o| self.index_of.get(&o).copied())
                    .is_some_and(|j| {
                        j < self.cars.len() && self.cars[j].standing_for(self.day_time) > BERTH_HELD_LONG
                    })
            };
            {
                let car = &mut self.cars[i];
                if let Some(service) = car.bus.as_mut() {
                    // a stop already behind the vehicle (the route was cut short) is dropped
                    while service
                        .stops
                        .front()
                        .is_some_and(|t| t.route_index < car.state.route_index)
                    {
                        service.stops.pop_front();
                        crate::traffic::ibis_to_next_stop(&mut car.vehicle, service.stops.len());
                    }
                    let berth = service.front_berth(&car.state.route);
                    let distance = berth
                        .map(|b| {
                            car.state
                                .route_distance(&self.net, b.route_index, b.s)
                        })
                        .unwrap_or(f32::MAX);
                    let wanted = self.stop_wishes.as_ref().map(|(alighting, waiting)| {
                        alighting.contains(&car.id)
                            || service
                                .stops
                                .front()
                                .is_some_and(|s| waiting.contains(&s.stop.get()))
                    });
                    let rail = self
                        .net
                        .lanes
                        .get(car.state.lane)
                        .is_some_and(|l| l.kind == LaneKind::Rail);
                    let feedback = crate::bus_service::script_feedback(&car.vehicle, service.state.phase);
                    let junction_first = berth
                        .map(|b| {
                            let ramp = ((b.bay - car.state.lateral).abs() * 8.0).clamp(8.0, 30.0);
                            let junction_end = way
                                .iter()
                                .filter(|&&(l, dl)| dl < distance && !self.net.crossings[l].is_empty())
                                .map(|&(l, dl)| dl + self.net.lanes[l].length())
                                .reduce(f32::max);
                            junction_end.is_some_and(|e| distance - e >= ramp)
                        })
                        .unwrap_or(false);
                    let inputs = ServiceInputs {
                        actor: i,
                        berth,
                        distance,
                        policy: service.policy(),
                        demand: StopDemand { wanted, rail },
                        feedback,
                        passing: car.maneuver.passing.is_some(),
                        kerb_swerve: car.maneuver.kerb_ramp.map(|r| r.1).or(kerb_swerve),
                        junction_first,
                        berth_held_long,
                    };
                    let scene = ServiceScene {
                        net: &self.net,
                        occupancy: &occupancy,
                        actors: &service_actors,
                        day_time: self.day_time,
                        dt,
                        tick: (self.time * 1000.0).max(0.0) as u64,
                    };
                    let decision = self.services.plan(&scene, &mut service.state, &inputs);
                    if let Some(at) = decision.stop_at {
                        stop_at = Some(stop_at.map(|x| x.min(at)).unwrap_or(at));
                    }
                    if let Some(binding) = decision.binding.or(decision.stop_at.map(|_| Reason::StopTarget)) {
                        let at = decision.stop_at.unwrap_or(0.0);
                        if at < why.1 {
                            why = (binding, at);
                        }
                    }
                    if let Some(t) = decision.lateral_target {
                        let mv =
                            self.maneuvers
                                .service_lateral(t, service_maneuver_phase(decision.phase));
                        if let Some(t2) = mv.lateral_target {
                            car.state.lateral_target = t2;
                        }
                    }
                    if let Some((blinker, dur)) = decision.signal {
                        car.state.signal = blinker;
                        car.state.signal_time = car.state.signal_time.max(dur);
                    }
                    if decision.consume_stop {
                        if debug && decision.phase == ServicePhase::EnRoute && !decision.release_berth {
                            log::info!(
                                "t={:.1}: bus {} passes stop {:?}: nobody to board or alight (wanted {:?})",
                                self.time,
                                car.id,
                                berth.map(|b| b.stop),
                                wanted
                            );
                        } else if debug
                            && decision.events.iter().any(|e| {
                                matches!(e, TraceEvent::Fault { reason: Reason::MissedStop, .. })
                            })
                        {
                            log::info!(
                                "t={:.1}: bus {} missed stop {:?} at lateral {:.2}",
                                self.time,
                                car.id,
                                berth.map(|b| b.stop),
                                car.state.lateral
                            );
                        }
                        service.stops.pop_front();
                        crate::traffic::ibis_to_next_stop(&mut car.vehicle, service.stops.len());
                    }
                    if !decision.events.is_empty() {
                        if let Some((_, cap)) = self.capture.as_mut() {
                            for ev in &decision.events {
                                cap.emit(ev.clone());
                            }
                        }
                    }
                }
            }
            // on its layover at the stand (where its timetable track begins, short of the
            // first stop): it stands there until it is time to drive to the stop
            {
                let car = &mut self.cars[i];
                if let Some(b) = car.bus.as_deref_mut() {
                    if let Some(t) = b.hold_until {
                        if self.day_time >= t {
                            b.hold_until = None;
                        } else {
                            let at = car.state.front + 0.1;
                            stop_at = Some(stop_at.map(|x| x.min(at)).unwrap_or(at));
                            if at < why.1 {
                                why = (Reason::StopTarget, at);
                            }
                        }
                    }
                }
            }
            // the end of the way: a timetable bus at the end of its trip drives on as
            // ordinary traffic until it is out of sight; a dead end is a place to stop
            {
                let car = &mut self.cars[i];
                let st = &mut car.state;
                let (last, end) = way
                    .last()
                    .map(|&(l, d)| (l, d + self.net.lanes[l].length()))
                    .unwrap_or((st.lane, 0.0));
                let exhausted = if st.route.is_empty() {
                    self.net.lanes[last].next.is_empty()
                } else {
                    st.route.last() == Some(&last)
                };
                let air = self.net.lanes[st.lane].kind == LaneKind::Air;
                if exhausted && st.change.is_none() && end < 150.0 {
                    let service = car.bus.as_deref_mut();
                    // (leaving its last stop where the route ends - a terminus stop at the
                    // stand, the end of the timetable track: the berth is only cleared a body
                    // length on, which the route does not reach, and the bus ran off its way
                    // and was put back on it for ever. The end of the way ends the trip; the
                    // next trip's track begins there.)
                    let in_service = service
                        .as_ref()
                        .map(|b| {
                            b.route_open
                                || (b.stops.is_empty() && !b.at_stop())
                                || (b.stops.len() <= 1 && b.state.phase == ServicePhase::Departing)
                        })
                        .unwrap_or(false);
                    let stops_left = service
                        .as_ref()
                        .map(|b| !b.stops.is_empty() || b.at_stop())
                        .unwrap_or(false);
                    if !st.route.is_empty() && in_service && !air && !car.gone {
                        // the end of the route it has: where the loaded tiles end, it waits
                        // for more route (or to be taken off out of sight); at the end of its
                        // trip, it stops there and waits for the timetable. An open route is
                        // not turned into random traffic after a timeout: missing capacity is
                        // a diagnosed content/service limitation (`RoutePending`), and its
                        // remaining stops are kept.
                        let at = end - 0.5;
                        stop_at = Some(stop_at.map(|x| x.min(at)).unwrap_or(at));
                        let reason = if service.as_ref().map(|b| b.route_open).unwrap_or(false) {
                            Reason::RoutePending
                        } else {
                            Reason::InvalidRoute
                        };
                        if at < why.1 {
                            why = (reason, at);
                        }
                        let b = service.unwrap();
                        if b.route_open {
                            if b.state.phase != ServicePhase::RoutePending {
                                b.state.phase = ServicePhase::RoutePending;
                                b.state.phase_t = 0.0;
                            }
                        } else if st.speed < 0.3 && !b.trip_done() {
                            if b.state.phase == ServicePhase::Departing {
                                // (its berth was never cleared by driving on)
                                self.services.release(car.id);
                            }
                            b.state.phase = ServicePhase::NextTrip;
                            b.state.phase_t = 0.0;
                            if debug {
                                log::info!(
                                    "t={:.1}: timetable bus {} at the end of its trip",
                                    self.time,
                                    car.id
                                );
                            }
                        }
                    } else if !st.route.is_empty() && !stops_left {
                        st.route.clear();
                        st.route_index = 0;
                        st.planned_next = None;
                        st.ahead.clear();
                        st.plan_next(&self.net);
                        car.gone = true;
                        if debug {
                            log::info!(
                                "t={:.1}: car {} finished its trip, drives on until out of sight",
                                self.time,
                                car.id
                            );
                        }
                    } else if st.route.is_empty() {
                        // a dead end (the map's edge, the end of a street spline): Omsi.exe
                        // drives on at speed and deletes the car the frame it runs out of
                        // road (0x71dc9c finds no next segment, 0x6fe3fc deletes it), and
                        // `drive` takes it off there. Braking for the end, the cars stopped
                        // there one by one and those behind queued into a stop-and-go (an
                        // aircraft flies on in any case)
                        car.gone = true;
                    }
                }
            }
            let lead_id = lead
                .and_then(|l| l.1)
                .filter(|&j| j < self.cars.len())
                .map(|j| self.cars[j].id);
            let car = &mut self.cars[i];
            car.lead_car = lead_id;
            if car.state.speed.abs() < 0.1 && !car.at_stop() {
                car.stopped += dt;
            } else {
                car.stopped = 0.0;
            }
            if car.state.speed.abs() < 1.0 && !car.at_stop() {
                car.crawl += dt;
            } else {
                car.crawl = 0.0;
            }
            // a random car that has stood for a minute without a light or a junction
            // holding it has given up: it leaves as soon as nobody sees it
            // (one yielding for minutes is in a gridlock nobody else will end)
            let stood = (car.stopped > 60.0 && !car.yielding || car.stopped > 150.0)
                && !car.is_bus()
                && !car.light_hold;
            // (a vehicle - a bus too - that cannot move without touching scenery gives up
            // sooner: nothing will change for it)
            let pinned = car.scenery_streak > SCENERY_PINNED_GONE;
            if (stood || pinned) && !car.gone {
                car.gone = true;
                if debug {
                    log::info!(
                        "t={:.1}: car {} stood for {:.0} s: taken off once out of sight",
                        self.time,
                        car.id,
                        car.stopped
                    );
                }
            }
            let lane_before = car.state.lane;
            let lead_now = lead.map(|l| l.0);
            car.why = match (why.1 < f32::MAX, lead_now) {
                (_, Some(l)) if l.gap + car.state.front < why.1 => {
                    let reason = match lead.and_then(|l| l.1) {
                        Some(usize::MAX) => Reason::Leader,
                        Some(_) => Reason::Leader,
                        None => Reason::Parking,
                    };
                    (reason, l.gap)
                }
                (true, _) => (why.0, why.1 - car.state.front),
                _ => (Reason::NONE, 0.0),
            };
            if car.fresh > 0.0 {
                car.fresh -= dt;
                // placed moving before a queue or a red light: arrive slower rather than
                // start with an emergency stop
                for _ in 0..16 {
                    if car.state.speed < 0.5
                        || car.state.desired_accel(&self.net, lead_now, stop_at) >= -car.state.decel
                    {
                        break;
                    }
                    car.state.speed *= 0.8;
                }
                if car.state.speed < 0.5 {
                    car.state.speed = 0.0;
                }
            }
            let (speed_before, lane_now, s_now) = (car.state.speed, car.state.lane, car.state.s);
            if debug && car.stopped > 30.0 {
                car.holding = Some(format!(
                    "lead {:?} (car {:?}), stop {:?} (light {:?}, junction {:?}, merge {:?}), bus {:?}, lane {} s {:.1} of {:.1}, next {:?}, lateral {:.2}, stops {:?}",
                    lead_now,
                    lead_id,
                    stop_at.map(|x| x - car.state.front),
                    light,
                    yield_at,
                    merge_wait,
                    car.bus.as_ref().map(|b| (b.state.phase, b.state.phase_t as i32)),
                    car.state.lane,
                    car.state.s,
                    self.net.lanes[car.state.lane].length(),
                    car.state.planned_next,
                    car.state.lateral,
                    car.next_stop()
                ));
                if car.stopped - dt <= 30.0 {
                    log::info!(
                        "t={:.1}: car {} ({}) has stood for 30 s at ({:.0}, {:.0}): {}",
                        self.time,
                        car.id,
                        car.vehicle.ty.def.type_name,
                        car.vehicle.position.x,
                        car.vehicle.position.y,
                        car.holding.as_deref().unwrap_or("-")
                    );
                }
            }
            if debug_car == Some(car.id.get()) {
                let up: Vec<usize> = car.state.upcoming().take(4).collect();
                log::info!(
                    "t={:.2} car {}: v {:.2} lane {} s {:.1}/{:.1} upcoming {:?} bend {:.2} desired {:.2} lead {:?} stop {:?} why {:?}",
                    self.time,
                    car.id,
                    car.state.speed,
                    car.state.lane,
                    car.state.s,
                    self.net.lanes[car.state.lane].length(),
                    up,
                    car.state.curve_speed(&self.net),
                    car.state.desired_accel(&self.net, lead_now, stop_at),
                    lead_now.map(|l| l.gap),
                    stop_at.map(|x| x - car.state.front),
                    car.why
                );
            }
            // Keep steering room through the approach as well as the final hold. A
            // per-tick stop target alone let the ordinary following model creep back
            // to min_gap while the bus was still docking, too close to pull out later.
            let following_lead = lead_now.map(|l| if reserve_pull_out {
                pull_out_following_lead(l, car.state.min_gap, car.pass_room)
            } else { l });
            if !car.state.drive(&self.net, dt, following_lead, stop_at) {
                if debug {
                    log::info!(
                        "t={:.1}: car {} ran out of road at {:.1} m/s: taken off",
                        self.time,
                        car.id,
                        car.state.speed
                    );
                }
                remove.push(i);
                continue;
            }
            if debug && car.state.acc < -4.5 {
                // hard braking is for emergencies: say what asked for it
                let who = match lead.and_then(|l| l.1) {
                    Some(usize::MAX) => "the player".to_string(),
                    Some(_) => format!("car {}", lead_id.map(|v| v.get()).unwrap_or(0)),
                    None if lead.is_some() => "a parked car".to_string(),
                    None => "-".to_string(),
                };
                log::info!(
                    "t={:.2}: car {} brakes {:.1} m/s² at {:.1} m/s on lane {lane_now} s {s_now:.2} (len {:.1}): lead {:?} ({who}), stop {:?} (light {:?}, junction {:?}, merge {:?})",
                    self.time,
                    car.id,
                    car.state.acc,
                    speed_before,
                    self.net.lanes[lane_now].length(),
                    lead_now,
                    stop_at.map(|x| x - car.state.front),
                    light.map(|x| x - car.state.front),
                    yield_at.map(|x| x - car.state.front),
                    merge_wait
                );
            }
            if car.state.lane != lane_before
                && self.first_turner.is_none()
                && !car.is_bus()
                && self.net.lanes[car.state.lane].turn != 0
            {
                self.first_turner = Some((car.id, self.time));
            }
            car.state.update_blinker(&self.net);
            if matches!(
                car.bus.as_ref().map(|b| b.state.phase),
                Some(ServicePhase::Boarding | ServicePhase::Layover)
            ) {
                // waiting at a stop: dark until it is about to pull away
                car.state.blinker = 0;
            }
            if debug_doors
                && car.is_bus()
                && (self.time * 2.0).floor() != ((self.time - dt) * 2.0).floor()
            {
                let v = &car.vehicle;
                let g = |n: &str| v.var(n).map(|x| format!("{x:.2}")).unwrap_or("-".into());
                let st = g("AI_Scheduled_AtStation");
                if st != "0.00" || car.bus.as_ref().is_some_and(|b| b.at_stop()) {
                    log::info!(
                        "doors t={:.1} car {} {} phase {:?} speed {:.1}: AtStation {st} door {} {} {} {} target {} {} {} halte {} timer {}",
                        self.time,
                        car.id,
                        v.ty.def.type_name,
                        car.bus.as_ref().map(|b| b.state.phase),
                        car.state.speed,
                        g("door_0"),
                        g("door_1"),
                        g("door_2"),
                        g("door_3"),
                        g("doorTarget_0"),
                        g("doorTarget_1"),
                        g("doorTarget_2"),
                        g("bremse_halte_sw"),
                        g("door_AI_timer")
                    );
                }
            }
            // An emergency vehicle (its script sets `TrafficPriority`, the stock ambulance)
            // is told `TrafficPriorityWarningNeeded` while something holds it up close ahead:
            // a car or the player's bus it catches up with or has to follow, a red light, a
            // junction it has to wait at. Its script sounds the siren on it; without the
            // variable it drove silent all day. (Behind a car at the same speed the siren
            // flickered on a strict "slower".)
            let priority_warning = car.vehicle.var("TrafficPriority").is_some_and(|v| v > 0.5)
                && (lead_now
                .is_some_and(|l| l.gap < PRIORITY_WARN_GAP && l.speed < car.state.speed + 0.5)
                || stop_at.is_some_and(|x| x - car.state.front < PRIORITY_WARN_GAP));
            frames[i] = Some(AiFrame {
                speed: car.state.speed,
                odometer: car.state.odometer,
                steer_deg: 0.0,
                blinker: car.state.blinker,
                brake: car.state.braking,
                lights: self.night,
                at_station: car.at_station() as i32,
                at_station_side: car.at_station_side(),
                priority_warning,
            });
        }
        // Who can be seen: a car out of the view (and farther than the mirrors and the
        // shadows reach) leaves its animations as they are and is not drawn at all.
        if let Some(v) = self.viewer {
            for c in &mut self.cars {
                let p = c.vehicle.position;
                let r = (c.state.front + c.state.rear).abs().max(4.0) as f64 + 2.0;
                c.vehicle.ai_visuals = (p - v.pos).length() < UNSEEN_NEAR || v.frames(p, r);
            }
        }
        TickFrame { by_lane, coming, walkers, aspects, junction_actors, frames, remove, previous_odometer }
    }

    /// 4. Realize every body and run its script, in parallel. The domain never owns pose;
    /// this is where `simulation::ai_motion` writes it.
    fn tick_realize(&mut self, dt: f32, frames: &mut [Option<AiFrame>], previous_odometer: &[f32]) {
        // The bodies and the scripts of the AI vehicles run in parallel: each car follows
        // its own way and its OMSI script is its own little machine reading only its own
        // state; with thirty cars and a dozen timetable buses they were the largest single
        // cost of a frame.
        {
            use rayon::prelude::*;
            let net = &self.net;
            let road_collision = &self.road_collision;
            type Work<'a> = (
                &'a AiState,
                &'a mut AiBody,
                &'a mut VehicleInstance,
                &'a mut AiFrame,
                &'a mut std::collections::VecDeque<(f64, DVec3)>,
                &'a mut Option<Reason>,
                &'a mut f32,
                f32,
                &'a VehicleCapabilities,
                &'a mut Option<f32>,
                bool,
            );
            let mut work: Vec<Work> = self
                .cars
                .iter_mut()
                .zip(frames.iter_mut())
                .enumerate()
                .filter_map(|(i, (c, f))| {
                    let f = f.as_mut()?;
                    Some((&c.state, &mut c.body, &mut c.vehicle, f, &mut c.rail_trail, &mut c.motion_fault, &mut c.scenery_streak, previous_odometer[i], &c.caps, &mut c.scenery_ahead, c.state.odometer < c.scenery_ghost))
                })
                .collect();
            let profile = ::legacy_config::env::var_os("OMSI_PROFILE").is_some();
            // (a few cars per job: every job handed out wakes a worker, and the waking cost
            // the main thread more than a car's work)
            work.par_iter_mut()
                .with_min_len(4)
                .for_each(|(state, body, vehicle, frame, trail, fault, streak, previous, caps, ahead, ghost)| {
                    let t0 = std::time::Instant::now();
                    let ground = vehicle.ground.clone();
                    let contact = vehicle.contact.clone();
                    let rail = body.kind == MotionKind::Rail;
                    if rail {
                        record_rail_trail(trail, state.odometer as f64, state.way_point(net, 0.0));
                    }
                    let trail = &**trail;
                    let behind = |d: f64| rail_behind(trail, state, net, d);
                    // (given up on scenery: it follows its way through it, see SCENERY_GHOST_AFTER)
                    if *ghost {
                        **ahead = None;
                        **fault = None;
                        **streak = 0.0;
                    }
                    let before = (body.kind == MotionKind::Road && !*ghost).then(|| (**body).clone());
                    // Where would this body touch scenery if it kept to its way? The planner
                    // brakes for it next tick (and looks for a way round); a refused move is
                    // only the last resort, with the bumper already against the thing.
                    if before.is_some() && body.ground_supported {
                        if state.speed > 0.05 || ahead.is_some() || **streak > 0.0 || fault.is_some() {
                            let reach = (state.speed * state.speed / 6.0 + 4.0).clamp(4.0, 16.0);
                            let veh: &VehicleInstance = vehicle;
                            let cp: &VehicleCapabilities = caps;
                            let touches = |b: &AiBody| road_collision.hit(&safety::road_body_box(veh, b, cp)).is_some();
                            **ahead = body.distance_to_contact(&|d| state.way_point(net, d), state.speed, reach, &touches);
                        } else {
                            **ahead = None;
                        }
                    }
                    body.step(
                        dt,
                        state.speed,
                        &|d| {
                            if rail && d < 0.0 {
                                behind(-d as f64)
                            } else {
                                state.way_point(net, d)
                            }
                        },
                        ground
                            .as_ref()
                            .map(|g| g.as_ref() as &dyn Fn(f64, f64) -> Option<f64>),
                        contact.as_deref(),
                    );
                    if let Some(before) = before {
                        let moved = body.position.truncate() != before.position.truncate() || body.heading != before.heading;
                        let obstruction = moved.then(|| road_collision.hit(&safety::road_body_box(vehicle, body, caps))).flatten();
                        let blocked = obstruction.is_some()
                            && road_collision.hit(&safety::road_body_box(vehicle, &before, caps)).is_none();
                        if blocked {
                            let first = **streak == 0.0;
                            **streak += dt;
                            body.reject_motion(&before);
                            // (every refused move: with the planner holding the vehicle
                            // between attempts these come about once a second, and choosing
                            // the way out only every half second *of them* left a bus
                            // pinned for a quarter of a minute with its wheels as they were)
                            body.recover_corner(&|d| state.way_point(net, d),
                                (caps.front, caps.rear, caps.half_width), &obstruction.unwrap());
                            frame.speed = 0.0;
                            frame.brake = true;
                            if first {
                                log::warn!("AI scenery sweep blocked {} at {:?}, lane {}, heading {:.1}, steer {:.1}, lateral {:.2}, target {:.2}; obstacle {:?}",
                                    vehicle.ty.def.path.display(), before.position, state.lane,
                                    before.heading, body.steer, state.lateral, state.lateral_target, obstruction);
                            }
                            **fault = Some(Reason::SceneryBlocked);
                        } else {
                            // (still pinned while it has not moved: the hold between two
                            // refused attempts is part of being stuck)
                            if moved {
                                **streak = 0.0;
                            } else if **streak > 0.0 {
                                **streak += dt;
                            }
                            **fault = None;
                        }
                    }
                    body.apply(vehicle);
                    if body.kind == MotionKind::Road {
                        frame.odometer = *previous + body.realized_speed(dt) * dt;
                    }
                    if rail && !vehicle.trailers.is_empty() {
                        // the coupled cars (a train's, a tram's sections) on the track it
                        // came along, not dragged round the bends like a road trailer
                        vehicle.retrail(0.0, &|d| Some(behind(d)));
                    }
                    frame.steer_deg = body.steer;
                    let t1 = std::time::Instant::now();
                    vehicle.update_ai(dt, frame);
                    if profile && t0.elapsed().as_secs_f64() > 0.01 {
                        log::info!(
                            "  slow AI frame: {} body {:.1} ms, scripts {:.1} ms",
                            vehicle.ty.def.path.display(),
                            (t1 - t0).as_secs_f64() * 1000.0,
                            t1.elapsed().as_secs_f64() * 1000.0
                        );
                    }
                });
        }
    }

    /// 5. Read the realized pose/speed back into each planner (single pose owner, Stage 4).
    fn tick_commit_feedback(&mut self, dt: f32, previous_odometer: &[f32]) {
        // Motion feedback (Stage 4, A7): the body is the single pose owner. Each road
        // vehicle's realized pose and speed are read back into its planner, so route
        // progress - and every stop distance derived from it - is committed from realized
        // movement, never from a planner coordinate alone.
        for (i, c) in self.cars.iter_mut().enumerate() {
            if self.net.lanes.get(c.state.lane).map(|l| l.kind) != Some(LaneKind::Street) {
                continue;
            }
            let realized = RealizedMotion {
                pose: c.vehicle.position,
                heading_deg: c.vehicle.heading as f32,
                speed: c.body.realized_speed(dt),
                half_width: c.half_width as f64,
            };
            c.state.commit_feedback(&self.net, realized);
            c.state.odometer = previous_odometer[i] + realized.speed * dt;
        }
    }

    /// 6. The tick tail: optional debug dumps, the wait-for graph, the `OMSI_TRACE_AI` dump,
    /// the overlap check, removal (releasing every owner exactly once) and the failure
    /// capture sample.
    fn tick_finish(
        &mut self,
        player: Option<PlayerBox>,
        frame: &TickFrame,
        others: &[(u32, PlayerBox)],
        debug: bool,
    ) {
        if ::legacy_config::env::var_os("OMSI_DEBUG_TRAILERS").is_some() {
            // coupled parts off the level of what pulls them (#140: trains' and articulated
            // buses' rear parts under bridges)
            for c in &self.cars {
                let mut lead_z = c.vehicle.position.z;
                for (k, t) in c.vehicle.trailers.iter().enumerate() {
                    let (pitch, axle, track) = t.debug_pose();
                    if pitch.abs() > 4.0 || (t.position.z - lead_z).abs() > 1.2 {
                        log::info!(
                            "trailer: car {} {} part {k} at ({:.1}, {:.1}, {:.2}) lead z {:.2} pitch {pitch:.1} axle {:?} track {:?} lane {} kind {:?}",
                            c.id,
                            c.vehicle.ty.def.type_name,
                            t.position.x,
                            t.position.y,
                            t.position.z,
                            lead_z,
                            axle,
                            track.map(|p| p.z),
                            c.state.lane,
                            self.net.lanes[c.state.lane].kind
                        );
                    }
                    lead_z = t.position.z;
                }
            }
        }
        if debug {
            // a car pulled round harder than a driver would: what way was it given?
            for (c, fr) in self.cars.iter().zip(&frame.frames) {
                if fr.is_none() || c.body.a_lat.abs() < 4.0 || !self.logged_hard.insert(c.id) {
                    continue;
                }
                let st = &c.state;
                let lanes: Vec<String> = std::iter::once(st.lane)
                    .chain(st.upcoming())
                    .take(5)
                    .map(|l| {
                        let l_ = &self.net.lanes[l];
                        format!(
                            "{l} ({} turn {} len {:.1} h {:.0}->{:.0} k {:.3}->{:.3})",
                            l_.name,
                            l_.turn,
                            l_.length(),
                            l_.start_heading(),
                            l_.end_heading(),
                            l_.curvature.first().copied().unwrap_or(0.0),
                            l_.curvature.last().copied().unwrap_or(0.0)
                        )
                    })
                    .collect();
                log::info!(
                    "t={:.1}: car {} {} at {:.1} m/s pulled {:.1} m/s² sideways (steering {:.1}°), bend speed {:.1}, s {:.1}, way {}",
                    self.time,
                    c.id,
                    c.vehicle
                        .ty
                        .def
                        .path
                        .file_stem()
                        .unwrap_or_default()
                        .to_string_lossy(),
                    st.speed,
                    c.body.a_lat,
                    c.body.steer,
                    st.curve_speed(&self.net),
                    st.s,
                    lanes.join(" / ")
                );
            }
        }
        // Persistent unexplained holds: classify a cyclic stale-claim deadlock separately
        // from legal congestion or a physically full road; the coordinator cancels stale
        // speculative claims so a valid safe manoeuvre can be retried, and lets one vehicle
        // of a lasting cycle that only gives way by the rules go first. It never forces a
        // vehicle across a conflicting body or a red signal.
        {
            let wait_scene = JunctionScene {
                net: &self.net,
                actors: &frame.junction_actors,
                index_of: &self.index_of,
                on_lane: &frame.by_lane,
                coming: &frame.coming,
                walkers: &frame.walkers,
                geo_prev: &self.geo_prev,
                aspects: &frame.aspects,
                time: self.time,
                tick: (self.time * 1000.0).max(0.0) as u64,
            };
            let _ = self.junctions.classify_waits(&wait_scene);
        }
        if let Some(f) = self.trace.as_mut() {
            use std::io::Write;
            // the player's vehicle as id 0 (its box centre, half length both ways)
            if let Some((c, h, hl, hw, v)) = player {
                let _ = writeln!(
                    f,
                    "{:.3},0,player,{:.3},{:.3},{:.3},{:.3},0,0,0,{:.3},-1,0,0,0,0,0,0,0,0,0,0,{hl:.2},{hl:.2},{hw:.2},0,,0,None",
                    self.time, c.x, c.y, c.z, h, v
                );
            }
            // (`OMSI_TRACE_AI_BUSES=1`: the timetable buses only)
            let buses_only = ::legacy_config::env::var_os("OMSI_TRACE_AI_BUSES").is_some();
            for (c, fr) in self.cars.iter().zip(&frame.frames) {
                let Some(fr) = fr else { continue };
                if buses_only && !c.is_bus() {
                    continue;
                }
                let v = &c.vehicle;
                let lane_heading = self.net.lanes[c.state.lane]
                    .at(c.state.s)
                    .1
                    .rem_euclid(360.0);
                let _ = writeln!(
                    f,
                    "{:.3},{},{},{:.3},{:.3},{:.3},{:.3},{:.3},{:.3},{:.3},{:.3},{},{:.2},{},{},{:.2},{:.2},{},{:.2},{},{},{},{:.2},{:.2},{:.2},{},{},{:.1},{:?},{:.3}",
                    self.time,
                    c.id,
                    v.ty.def
                        .path
                        .file_stem()
                        .unwrap_or_default()
                        .to_string_lossy(),
                    v.position.x,
                    v.position.y,
                    v.position.z,
                    v.heading,
                    v.pitch,
                    v.bank,
                    fr.steer_deg,
                    c.state.speed,
                    c.state.lane,
                    c.state.s,
                    fr.blinker,
                    self.net.lanes[c.state.lane].turn,
                    lane_heading,
                    c.state.lateral,
                    c.at_station() as i32,
                    c.state.acc,
                    c.yielding as i32,
                    c.light_hold as i32,
                    c.maneuver.passing.is_some() as i32,
                    c.state.front,
                    c.state.rear,
                    c.half_width,
                    c.is_bus() as i32,
                    c.why.0.trace_label(),
                    c.why.1.min(999.0),
                    c.bus.as_ref().map(|b| b.state.phase),
                    self.net.lanes[c.state.lane].at(c.state.s).0.z
                );
            }
        }
        if ::legacy_config::env::var_os("OMSI_CHECK_OVERLAP").is_some() {
            self.check_overlaps(player, others);
        }
        for i in frame.remove.iter().copied().rev() {
            let c = self.cars.swap_remove(i);
            self.junctions.release(c.id, Reason::Removed);
            self.services.release(c.id);
            self.maneuvers.release(c.id);
            self.population.release(c.id, RemovalCause::Finished);
            self.emit_trace(TraceEvent::Removal {
                vehicle: c.id,
                reason: Reason::Removed,
            });
            self.orphan_sounds.extend(c.sounds);
            // the renders go back to the world at the next sync
            self.released.push(c.render);
            self.released.extend(c.trailer_renders);
        }
        if self.capture.is_some() {
            self.sample_capture();
        }
    }
}

/// The frozen per-tick data the planning phase hands to realization and finish. Every buffer
/// is owned (no borrow of `Traffic`), so the phases can be separate methods.
struct TickFrame {
    previous_odometer: Vec<f32>,
    by_lane: HashMap<usize, Vec<(usize, f32, f32, bool)>>,
    coming: HashMap<usize, Vec<(usize, f32)>>,
    walkers: HashMap<usize, Vec<f32>>,
    aspects: HashMap<(usize, usize), Aspect>,
    junction_actors: Vec<JunctionActor>,
    frames: Vec<Option<AiFrame>>,
    remove: Vec<usize>,
}

fn pull_out_following_lead(mut lead: Lead, min_gap: f32, room: f32) -> Lead {
    lead.gap = (lead.gap - (room - min_gap).max(0.0)).max(0.05);
    lead
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn docking_following_retains_room_to_steer_around_the_articulated_rear() {
        let mut net = Network::default();
        net.lanes.push(::traffic::LaneBuilder::polyline(
            vec![DVec3::ZERO, DVec3::new(0.0, 400.0, 0.0)], LaneKind::Street, 3.0));
        net.link(1.5);
        let mut car = AiState::new(0, 30.0, 167);
        car.front = 2.13;
        car.min_gap = 2.45;
        car.speed = 8.0;
        car.plan_next(&net);
        let (mut rear, mut speed) = (50.0, 6.0f32);
        let room = 5.25;
        let mut closest = f32::MAX;
        for _ in 0..1500 {
            let gap = rear - car.s - car.front;
            closest = closest.min(gap);
            let leader = Lead { gap, speed, acc: if speed > 0.0 { -1.0 } else { 0.0 } };
            car.drive(&net, 0.02, Some(pull_out_following_lead(leader, car.min_gap, room)), None);
            let next_speed = (speed - 0.02).max(0.0);
            rear += (speed + next_speed) * 0.01;
            speed = next_speed;
        }
        assert!(closest >= room - 0.3, "lost steering room: {closest}");
        assert!(car.speed < 0.1);
        assert!((rear - car.s - car.front - room).abs() < 0.3);
    }
}
