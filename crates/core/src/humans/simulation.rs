use super::*;

impl Humans {
    /// Advance everybody. `bus`: the player's vehicle; `traffic`: the timetable buses,
    /// the traffic lights and the cars pedestrians wait for. Returns true when a passenger
    /// took the printed ticket (the caller resets `GivenTicket`).
    pub fn tick(
        &mut self,
        dt: f32,
        world: &World,
        bus: Option<&VehicleInstance>,
        traffic: Option<&Traffic>,
        renderer: &Renderer,
        scene: &mut Scene,
    ) -> bool {
        let started = std::time::Instant::now();
        self.tick_stages.clear();
        let took = self.tick_inner(dt, world, bus, traffic, renderer, scene);
        let ms = started.elapsed().as_secs_f64() * 1000.0;
        if (debug_pax() || ::legacy_config::env::var_os("OMSI_PROFILE").is_some()) && ms > 30.0 {
            log::info!(
                "t={:.1} slow people tick: {ms:.1} ms ({} people): {}",
                self.time,
                self.people.len(),
                self.tick_stages
                    .iter()
                    .filter(|s| s.1 >= 1.0)
                    .map(|(n, t)| format!("{n} {t:.1}"))
                    .collect::<Vec<_>>()
                    .join(", ")
            );
        }
        if ::legacy_config::env::var_os("OMSI_CHECK_WALLS").is_some() {
            self.check_walls();
        }
        // OMSI_CHECK_GROUND=1: people on foot with a walkable surface over their heads'
        // reach above them, every two seconds (people "in the ground")
        if ::legacy_config::env::var_os("OMSI_CHECK_GROUND").is_some()
            && (self.time / 2.0).floor() != ((self.time - dt as f64) / 2.0).floor()
        {
            for p in &self.people {
                if !matches!(p.place, Place::Ground) || p.puppet.is_some() {
                    continue;
                }
                // the floor under the feet: the highest face up to a step (0.5 m) over them
                let floor = world.walk_height_near(p.position.x, p.position.y, p.position.z);
                if let Some(f) = floor {
                    if f - p.position.z > 0.05 {
                        let detail = match &p.state {
                            State::Pax(x) => format!(
                                " st {} pax_state {} task {:?} pos.z {:.2}",
                                x.movement, x.posture, x.task, x.pos.z
                            ),
                            _ => String::new(),
                        };
                        log::warn!(
                            "t={:.1} person {} ({}) {:.2} m under the floor at ({:.1}, {:.1}, {:.2}), top surface {:?}{detail}",
                            self.time,
                            p.id,
                            p.state.name(),
                            f - p.position.z,
                            p.position.x,
                            p.position.y,
                            p.position.z,
                            world.walk_height(p.position.x, p.position.y)
                        );
                    }
                }
            }
        }
        self.tick_stats.0 += 1;
        self.tick_stats.1 += ms;
        self.tick_stats.2 = self.tick_stats.2.max(ms);
        took
    }

    #[allow(unused_assignments)]
    pub(super) fn tick_inner(
        &mut self,
        dt: f32,
        world: &World,
        bus: Option<&VehicleInstance>,
        traffic: Option<&Traffic>,
        renderer: &Renderer,
        scene: &mut Scene,
    ) -> bool {
        let mut mark = std::time::Instant::now();
        macro_rules! stage {
            ($name:expr) => {{
                let now = std::time::Instant::now();
                self.tick_stages
                    .push(($name, (now - mark).as_secs_f64() * 1000.0));
                mark = now;
            }};
        }
        self.use_map_humans(world);
        self.time += dt as f64;
        let net = traffic.map(|t| &t.net);
        if let Some(b) = bus {
            self.center = b.position;
        } else if let Some(e) = self.eye {
            self.center = e.pos;
        }
        let generation = world
            .tiles_generation
            .load(std::sync::atomic::Ordering::Relaxed);
        if generation != self.tiles_seen {
            self.tiles_seen = generation;
            self.tiles_changed(world);
        }
        stage!("tiles");
        // tiles brought lanes: their pavements join the network, and stops without one look again
        if let (Some(pn), Some(n)) = (self.walking.ped.as_mut(), net) {
            if pn.built < n.lanes.len() {
                let added = pn.extend(n);
                if added > 0 {
                    let ids: Vec<(i64, DVec3)> = self
                        .stops
                        .iter()
                        .filter(|(_, s)| s.lane.is_none())
                        .map(|(k, s)| (*k, s.pos))
                        .collect();
                    for (id, pos) in ids {
                        let lane = self
                            .walking
                            .ped
                            .as_ref()
                            .and_then(|pn| pn.nearest(n, pos, 12.0))
                            .map(|(l, s, _)| (l, s));
                        self.stops.get_mut(&id).unwrap().lane = lane;
                    }
                }
            }
        }
        if self.walking.ped.is_none() {
            if let Some(n) = net {
                self.walking.ped = Some(PedNet::build(n));
                let ids: Vec<(i64, DVec3)> = self.stops.iter().map(|(k, s)| (*k, s.pos)).collect();
                for (id, pos) in ids {
                    let lane = self
                        .walking
                        .ped
                        .as_ref()
                        .and_then(|pn| pn.nearest(n, pos, 12.0))
                        .map(|(l, s, _)| (l, s));
                    self.stops.get_mut(&id).unwrap().lane = lane;
                }
            }
        }
        stage!("pedestrian network");
        if let Some(n) = net {
            self.walking.stroll_timer -= dt;
            if self.walking.stroll_timer <= 0.0 {
                self.walking.stroll_timer = 1.0;
                let c = self.center;
                self.populate_with(world, Some(n), renderer, scene, c);
                if !self.network.mirror {
                    self.populate_on_foot(world, n, renderer, scene, 1.0);
                    self.populate_lan_centers(world, n, renderer, scene);
                }
            }
        }
        stage!("populate");
        let mut buses = self.gather_buses(world, bus, traffic);
        for b in &buses {
            if b.entry_open.iter().chain(b.exit_open.iter()).any(|o| *o) {
                self.buses.last_door_open.insert(b.id, self.time);
            }
        }
        // how the floor of each bus accelerates (for the drawing of its riders)
        if dt > 1e-4 {
            let mut motion = HashMap::new();
            for bn in buses.iter_mut() {
                let accel = match self.buses.bus_motion.get(&bn.id) {
                    Some(&(v0, h0, a0)) => {
                        let yaw_rate = crowd::angle_diff(h0, bn.heading).to_radians() / dt as f64;
                        let raw = DVec2::new(bn.speed * yaw_rate, (bn.speed - v0) / dt as f64)
                            .clamp(DVec2::splat(-6.0), DVec2::splat(6.0));
                        a0 + (raw - a0) * (1.0 - (-(dt as f64) / 0.2).exp())
                    }
                    None => DVec2::ZERO,
                };
                bn.accel = accel;
                motion.insert(bn.id, (bn.speed, bn.heading, accel));
            }
            self.buses.bus_motion = motion;
        }
        let buses = buses;
        self.buses.last_buses = buses.clone();
        let bus_ix: HashMap<BusId, usize> =
            buses.iter().enumerate().map(|(i, b)| (b.id, i)).collect();
        // the stops: which buses stand at them (sub_61f93c), who waits there (sub_61bf94)
        let at_stops = self.register_buses(&buses, dt);
        self.claim_waiting();
        self.ride_comfort(dt, bus, &buses, &bus_ix, world);
        if !self.avatar_only {
            self.stops_tick(dt, world, renderer, scene);
        }
        stage!("stops");
        // the passengers (sub_6ffc7c)
        let mut taken_ticket = false;
        let mut remove: Vec<usize> = Vec::new();
        let passenger_origins: Vec<_> = self.people.iter().map(|p| p.position).collect();
        let pay_parent = self.buses.player_cabin.as_ref().and_then(|c| c.money_parent);
        self.pax_frame(
            dt,
            world,
            net,
            &buses,
            &bus_ix,
            &at_stops,
            bus.and_then(|b| b.var("GivenTicket")),
            &mut |money, coins, pos, var| {
                money.place(world, renderer, scene, coins, pos, var, pay_parent, false)
            },
            &mut taken_ticket,
            &mut remove,
        );
        stage!("passengers");
        // the pedestrians: a crowd on the pavements
        let mut cars: Vec<(DVec2, DVec2, f64)> = Vec::new();
        let mut blocks: Vec<Block> = Vec::new();
        if let Some(t) = traffic {
            for c in &t.cars {
                if (c.vehicle.position - self.center).length() > 320.0 {
                    continue;
                }
                let h = c.vehicle.heading.to_radians();
                let fwd = DVec2::new(h.sin(), h.cos());
                let bb = c
                    .vehicle
                    .ty
                    .def
                    .bounding_box
                    .unwrap_or([2.0, 4.5, 1.6, 0.0, 0.0, 0.8]);
                cars.push((
                    c.vehicle.position.truncate(),
                    fwd * c.state.speed as f64,
                    bb[1] as f64 * 0.5,
                ));
                if !matches!(c.vehicle.ty.def.kind, ::legacy_vehicle::VehicleKind::Other(3)) {
                    let o = ::simulation::collision::Obb::from_box(
                        bb,
                        c.vehicle.position,
                        c.vehicle.heading,
                    );
                    blocks.push(Block {
                        center: o.center,
                        half: o.half,
                        heading: o.heading,
                        vel: fwd * c.state.speed as f64,
                    });
                    for t in &c.vehicle.trailers {
                        let tb =
                            t.ty.def
                                .bounding_box
                                .unwrap_or([2.5, 7.0, 3.0, 0.0, 0.0, 1.5]);
                        let o = ::simulation::collision::Obb::from_box(tb, t.position, t.heading);
                        let th = t.heading.to_radians();
                        blocks.push(Block {
                            center: o.center,
                            half: o.half,
                            heading: o.heading,
                            vel: DVec2::new(th.sin(), th.cos()) * c.state.speed as f64,
                        });
                    }
                }
            }
        }
        for o in world.parked_boxes.lock().iter() {
            if (o.center - self.center.truncate()).length() < 320.0 {
                blocks.push(Block {
                    center: o.center,
                    half: o.half,
                    heading: o.heading,
                    vel: DVec2::ZERO,
                });
            }
        }
        if let Some(pb) = bus_ix.get(&BusId::Player).map(|&i| &buses[i]) {
            cars.push((pb.pos.truncate(), pb.fwd() * pb.speed, pb.half.y));
            for t in &pb.trailers {
                let h = t.heading.to_radians();
                cars.push((
                    t.pos.truncate(),
                    DVec2::new(h.sin(), h.cos()) * pb.speed,
                    t.half.y,
                ));
            }
            blocks.extend(pb.blocks());
        }
        let mut wants: Vec<Want> = Vec::with_capacity(self.people.len());
        for i in 0..self.people.len() {
            self.people[i].t_state += dt;
            let w = if self.people[i].puppet.is_some()
                || matches!(self.people[i].state, State::Pax(_))
            {
                Want::stand(None, Activity::Stand)
            } else if self.people[i].remote {
                self.mirror_want(i, &buses)
            } else {
                self.decide(i, dt, world, net, traffic, &cars, &mut remove)
            };
            wants.push(w);
        }
        // a standing vehicle in the way: wait, then go round it
        for i in 0..self.people.len() {
            let p = &self.people[i];
            if remove.contains(&i)
                || p.puppet.is_some()
                || p.remote
                || !matches!(p.state, State::Strolling(_))
            {
                continue;
            }
            let want = wants[i].vel;
            let speed = want.length();
            if speed < 0.2 {
                self.people[i].car_wait = 0.0;
                continue;
            }
            let ahead = p.position.truncate() + want / speed * 0.9;
            let in_way = blocks.iter().any(|b| {
                b.vel.length() < 0.5 && b.near(ahead, BODY_OUTSIDE + 0.15) && {
                    let (q, inside) = b.closest(ahead);
                    inside || (ahead - q).length() < BODY_OUTSIDE + 0.15
                }
            });
            if !in_way {
                self.people[i].car_wait = 0.0;
                continue;
            }
            self.people[i].car_wait += dt;
            if self.people[i].car_wait > 8.0 {
                self.people[i].detour = self.people[i].detour.max(4.0);
                self.people[i].car_wait = 0.0;
            } else if self.people[i].detour <= 0.0 {
                wants[i].vel = DVec2::ZERO;
            }
        }
        // the crowd of the pavements (passengers outside stand in it as they are)
        let mut walkers: Vec<Walker> = Vec::with_capacity(self.people.len());
        let mut who: Vec<usize> = Vec::with_capacity(self.people.len());
        for (i, p) in self.people.iter().enumerate() {
            let crossing = matches!(&p.state, State::Pax(passenger) if passenger.doorway.is_some());
            if remove.contains(&i)
                || p.puppet.is_some()
                || p.remote
                || (p.place != Place::Ground && !crossing)
            {
                continue;
            }
            let fixed = matches!(p.state, State::Pax(_));
            let w = &wants[i];
            walkers.push(Walker {
                pos: p.position.truncate(),
                // A crossing remains cabin-owned, but its world projection already
                // occupies space outside. Let pedestrians make room before handoff.
                vel: if crossing { DVec2::ZERO } else { p.vel },
                radius: BODY_OUTSIDE,
                want: w.vel,
                give: w.give,
                space: 0,
                fixed,
                ghost: p.ghost > 0.0,
                corridor: if p.detour > 0.0 { None } else { w.corridor },
            });
            who.push(i);
        }
        let near_blocks: Vec<Block> = blocks
            .into_iter()
            .filter(|b| walkers.iter().any(|w| b.near(w.pos, 25.0)))
            .collect();
        let mut g = walkers.clone();
        crowd::step(&mut g, &near_blocks, &CrowdParams::default(), dt as f64);
        let mut ground: Vec<(usize, Walker)> = g.into_iter().enumerate().collect();
        self.keep_out_of_walls(world, &who, &mut ground);
        let mut moved = vec![false; self.people.len()];
        for (k, w) in ground {
            let i = who[k];
            if matches!(self.people[i].state, State::Pax(_)) {
                continue;
            }
            moved[i] = true;
            self.apply(i, &w, &wants[i], dt, world, net, &buses, &bus_ix);
        }
        for i in 0..self.people.len() {
            if !moved[i] && !matches!(self.people[i].state, State::Pax(_)) {
                self.carry(i, dt, &buses, &bus_ix, wants[i].face);
            }
        }
        self.pax_room(dt, &passenger_origins, &buses, &bus_ix);
        self.animate(dt, world, &buses, &bus_ix);
        remove.sort_unstable();
        remove.dedup();
        for i in remove.into_iter().rev() {
            self.release(i);
            let p = self.people.swap_remove(i);
            if debug_pax() {
                log::info!(
                    "t={:.1} pax {} taken away ({}){}",
                    self.time,
                    p.label(),
                    p.state.name(),
                    if self.seen(p.position) {
                        " IN SIGHT"
                    } else {
                        ""
                    }
                );
            }
            self.retire(&p);
        }
        self.give_ticket = false;
        stage!("pedestrians");
        taken_ticket
    }
}
