//! The per-tick pipeline: clock, frozen snapshot, per-owner planning, realization and
//! commit, split into named phases.

use super::*;

impl Traffic {

    pub fn tick(&mut self, dt: f32, player: Option<PlayerBox>) {
        self.lamp_dt += dt;
        if self.mirror {
            self.mirror_tick(dt);
            return;
        }
        let t_start = std::time::Instant::now();
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
        let debug = ::legacy_config::env::var_os("OMSI_DEBUG_TRAFFIC").is_some();
        // One immutable occupancy snapshot for the tick: realized bodies plus their lane
        // placements, built once. Geometry is the truth; `by_lane` is a flat id-keyed view
        // for the checks not yet migrated (leader scans, lane changes, passing).
        let occupancy = Occupancy::build(
            self.net.version(),
            (self.time * 1000.0).max(0.0) as u64,
            self.body_feet(player, &self.others),
        );
        let by_lane = occupancy.lane_view(&self.index_of);
        // The external road users' synthetic ids (see `body_feet`): bodies to keep clear of,
        // not AI blockers to sort out by `geo_block`.
        let mut external_ids: Vec<VehicleId> = vec![VehicleId(u64::MAX)];
        external_ids.extend(
            self.others
                .iter()
                .map(|(id, _)| VehicleId(u64::MAX - 1 - *id as u64)),
        );
        // cars coming to a junction lane: (car, distance from its origin to the lane start)
        let mut coming: HashMap<usize, Vec<(usize, f32)>> = HashMap::new();
        for (i, c) in self.cars.iter().enumerate() {
            for (l, d) in self
                .way_lanes(&c.state, LOOK_AHEAD + 30.0)
                .into_iter()
                .skip(1)
            {
                if !self.net.crossings[l].is_empty() {
                    coming.entry(l).or_default().push((i, d));
                }
            }
        }
        // The junction coordinator owns its own claim/store state (see `begin_tick` below);
        // exit storage is recomputed fresh this tick against the realized occupancy.
        // the light programs: requests of whoever is coming, then the cycle clocks
        for c in self.lights.iter_mut() {
            c.request.iter_mut().for_each(|r| *r = false);
        }
        for c in &self.cars {
            for (l, d) in self.way_lanes(&c.state, 160.0) {
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
            for l in 0..self.net.lanes.len() {
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
        let t_plan = std::time::Instant::now();
        let mut remove = Vec::new();
        let mut frames: Vec<Option<AiFrame>> = vec![None; self.cars.len()];
        self.break_lead_pairs();
        // The junction actors and the signal aspects are frozen once for the tick; all
        // junction decisions read this snapshot, and the coordinator owns the claims.
        let junction_actors: Vec<JunctionActor> = self
            .cars
            .iter()
            .map(|c| JunctionActor {
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
                    .map(move |li| ((ci, li), TrafficLightController::aspect(ctl.state(li))))
            })
            .collect();
        self.junctions.begin_tick((self.time * 1000.0).max(0.0) as u64);
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
                    rear: st.rear,
                    length: st.length,
                    half_width: c.half_width,
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
        let maneuver_intents: Vec<ManeuverIntent> = maneuver_actors
            .iter()
            .map(|a| ManeuverIntent {
                vehicle: a.id,
                target: a
                    .change
                    .map(|c| LaneId(c.to))
                    .or_else(|| required_target(&self.net, a).map(LaneId)),
                required: true,
            })
            .collect();
        self.maneuvers
            .begin_tick(&maneuver_intents, (self.time * 1000.0).max(0.0) as u64);
        for i in 0..self.cars.len() {
            let ahead = self.obstacle_ahead(i, look_ahead(self.cars[i].state.speed), &by_lane);
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
            for (id, o) in &others {
                if let Some(l) = self.player_in_way(i, o) {
                    if lead.map(|x| l.gap < x.0.gap).unwrap_or(true) {
                        lead = Some((l, Some(usize::MAX)));
                        player = Some(*o);
                        player_standing = self.others_still.get(id).copied().unwrap_or(0.0);
                    }
                }
            }
            // other vehicles' bodies in the way off the lanes
            if let Some((l, j)) = self.body_in_way(i, &occupancy, &external_ids) {
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
            {
                let car = &self.cars[i];
                let st = &car.state;
                let near_way = self.way_lanes(st, 100.0);
                let passing = car.maneuver.passing.map(|p| !p.aborted).unwrap_or(false);
                let mut swerve: Option<f32> = None;
                let mut stand: Option<(f32, usize, f32, f32)> = None;
                let mut check = |along: f32, lat: f32, lane: usize, at: f32| {
                    if !(-6.0..=100.0).contains(&along) {
                        return;
                    }
                    let a = lat.abs();
                    // in the way at the side the car is on now (a car pulled out onto the
                    // other half passes it)
                    let blocks = if passing {
                        (lat - st.lateral_ahead(along)).abs() < car.half_width + 0.9 + 0.2
                    } else {
                        a < 0.9
                    };
                    if blocks {
                        if along > 0.0 {
                            let gap = along - 2.3 - st.front;
                            if stand.map(|o| gap < o.0).unwrap_or(true) {
                                stand = Some((gap, lane, at, lat));
                            }
                        }
                    } else if !passing && a < car.half_width + 0.9 + 0.15 && along < 30.0 {
                        // (only as far as the two bodies would touch: OMSI's cars keep to
                        // their paths, and moved out by a margin of our own round every car
                        // at the kerb - 2.7 m from the lane's middle - the traffic of a
                        // narrow British street lined with parked cars wove to and fro
                        // across the road instead of keeping to its lane)
                        let need = (car.half_width + 0.9 + 0.15 - a) * -lat.signum();
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
                for &(l, d) in &near_way {
                    if Some(l) == leaving {
                        continue;
                    }
                    for &(s, lat) in self.parked.get(&l).map(|v| v.as_slice()).unwrap_or(&[]) {
                        check(d + s, lat, l, s);
                    }
                }
                // a bus standing half in its bay: squeeze past on
                // the other side when a metre is enough, instead of queueing behind it -
                // and stay out until past its front (moving back in while still beside it
                // steered the car into the bus)
                if !passing {
                    let swerving = st.lateral_target.abs() > 0.1;
                    for &(l, d) in &near_way {
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
                    l.speed.abs() < 0.3
                        && match who {
                        Some(usize::MAX) => player.map(|p| p.4.abs() < 0.3).unwrap_or(false),
                        Some(j) if j < self.cars.len() => self.cars[j].at_stop(),
                        _ => false,
                    }
                })
                .unwrap_or(false);
            // It stops `pass_room` short of it (the room its own steering needs to get out
            // round it), or as far back as it can without braking hard. The two metres taken
            // off the gap it keeps to such a thing were not enough: the car still crept up to
            // under three metres behind the player's bus and never got round it.
            let mut keep_back: Option<f32> = None;
            if standing || may_stand {
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
            let way = self.way_lanes(&self.cars[i].state, 200.0);
            let merge_wait: Option<f32>;
            let mut maneuver_why: Option<(Reason, f32)> = None;
            {
                let mut inputs = ManeuverInputs::new(i);
                inputs.kerb_swerve = kerb_swerve;
                inputs.lead_gap = lead.map(|l| l.0.gap);
                inputs.lead_standing = standing;
                inputs.obstacle_len = obstacle_len;
                inputs.parked = parked_ahead || at_stop;
                let decision = {
                    let scene = ManeuverScene {
                        net: &self.net,
                        occupancy: &occupancy,
                        actors: &maneuver_actors,
                        people: &[],
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
            }
            // The coordinator decides the signal hold and the right of way from the frozen
            // view; it is the only writer of junction claims.
            let decision = self.junction_plan(
                i,
                &way,
                lead.map(|l| l.0),
                &by_lane,
                &coming,
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
                junction_ahead(&self.net, &way).filter(|jn| {
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
                self.people_stop(i, &way)
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
            let mut stop_at = [light, yield_at, merge_wait, keep_back, people]
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
                        kerb_swerve,
                        junction_first,
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
                    if let Some(binding) = decision.binding {
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
                    let in_service = service
                        .as_ref()
                        .map(|b| b.route_open || (b.stops.is_empty() && !b.at_stop()))
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
            if (car.stopped > 60.0 && !car.yielding || car.stopped > 150.0)
                && !car.is_bus()
                && !car.light_hold
                && !car.gone
            {
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
            if ::legacy_config::env::var("OMSI_DEBUG_CAR")
                .ok()
                .and_then(|v| v.parse::<u64>().ok())
                == Some(car.id.get())
            {
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
            if !car.state.drive(&self.net, dt, lead_now, stop_at) {
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
            if ::legacy_config::env::var_os("OMSI_DEBUG_DOORS").is_some()
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
        let t_par = std::time::Instant::now();
        // The bodies and the scripts of the AI vehicles run in parallel: each car follows
        // its own way and its OMSI script is its own little machine reading only its own
        // state; with thirty cars and a dozen timetable buses they were the largest single
        // cost of a frame.
        {
            use rayon::prelude::*;
            let net = &self.net;
            type Work<'a> = (
                &'a AiState,
                &'a mut AiBody,
                &'a mut VehicleInstance,
                &'a mut AiFrame,
                &'a mut std::collections::VecDeque<(f64, DVec3)>,
            );
            let mut work: Vec<Work> = self
                .cars
                .iter_mut()
                .zip(frames.iter_mut())
                .filter_map(|(c, f)| {
                    let f = f.as_mut()?;
                    Some((&c.state, &mut c.body, &mut c.vehicle, f, &mut c.rail_trail))
                })
                .collect();
            let profile = ::legacy_config::env::var_os("OMSI_PROFILE").is_some();
            // (a few cars per job: every job handed out wakes a worker, and the waking cost
            // the main thread more than a car's work)
            work.par_iter_mut()
                .with_min_len(4)
                .for_each(|(state, body, vehicle, frame, trail)| {
                    let t0 = std::time::Instant::now();
                    let ground = vehicle.ground.clone();
                    let contact = vehicle.contact.clone();
                    let rail = body.kind == MotionKind::Rail;
                    if rail {
                        record_rail_trail(trail, state.odometer as f64, state.way_point(net, 0.0));
                    }
                    let trail = &**trail;
                    let behind = |d: f64| rail_behind(trail, state, net, d);
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
                    body.apply(vehicle);
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
        // Motion feedback (Stage 4, A7): the body is the single pose owner. Each road
        // vehicle's realized pose and speed are read back into its planner, so route
        // progress - and every stop distance derived from it - is committed from realized
        // movement, never from a planner coordinate alone.
        for c in &mut self.cars {
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
        }
        self.tick_split = [
            (t_plan - t_start).as_secs_f64(),
            (t_par - t_plan).as_secs_f64(),
            t_par.elapsed().as_secs_f64(),
        ];
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
            for (c, fr) in self.cars.iter().zip(&frames) {
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
        // speculative claims so a valid safe manoeuvre can be retried. It never forces a
        // vehicle across a conflicting body or a red signal.
        {
            let wait_scene = JunctionScene {
                net: &self.net,
                actors: &junction_actors,
                index_of: &self.index_of,
                on_lane: &by_lane,
                coming: &coming,
                walkers: &walkers,
                geo_prev: &self.geo_prev,
                aspects: &aspects,
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
            for (c, fr) in self.cars.iter().zip(&frames) {
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
            self.check_overlaps(player, &others);
        }
        for i in remove.into_iter().rev() {
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
