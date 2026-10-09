//! Vehicle construction and content loading: building `Traffic`, attaching cars,
//! trailers/consists, creating a car and precaching AI types.

use super::*;

impl Traffic {

    /// Build the network from the lanes collected by `World::build_scene` and load the AI
    /// car types of the map's `ailists.cfg` (the `[aigroup_2]` groups that are not depots).
    pub fn new(root: &Path, world: &World, target: usize) -> Result<Traffic> {
        let (lanes, parked_cars, lane_tiles) = take_from_tiles(world);
        let mut net = Network {
            lanes,
            // (`[lht]`: priority to the left, passing on the right, keeping left)
            left_hand: world.global.left_hand_traffic,
            ..Default::default()
        };
        if net.left_hand {
            log::info!("traffic: the map drives on the left");
        }
        net.link(1.5);
        report_network_defects(&net);
        let mut types = Vec::new();
        let mut groups: Vec<::map::ailists::UnschedGroup> = Vec::new();
        let mut group_uvg: Vec<Option<usize>> = Vec::new();
        let mut uvg_defaults: Vec<i32> = Vec::new();
        // `unsched_trafficdens.txt`: per random group a factor and its density over the day
        // (by day of the week); the global.cfg curve is the fallback of maps without it
        let dens: Vec<::map::ailists::UnschedGroup> =
            ::legacy_config::CfgFile::read(&world.map_dir.join("unsched_trafficdens.txt"))
                .ok()
                .map(|f| ::map::ailists::parse_unsched_trafficdens(&f))
                .unwrap_or_default();
        let group_curves = !dens.is_empty();
        {
            // `unsched_vehgroups.txt` names the groups the random traffic is made of. The
            // other `[aigroup_2]`s exist only for the timetable: on Berlin-Spandau "Pan Am"
            // and "Mi-8 Soviet AF" fly TXL.ttl and Relais.ttl, and taking them into the
            // random pool put airliners on the flight paths at any hour of the day. Its
            // number is the group's default density on the paths without a `[rule]
            // trafficdensity` for it (see `uvg_density`): 0 means only where the paths ask
            // for the group. Taken as "off", Spandau had no trucks and no Trabant at all,
            // though 865 paths ask for the one and 462 around Falkensee for the other.
            // `OMSI_TRAFFIC_ALL_GROUPS=1` lets such groups drive everywhere.
            let all_groups = ::legacy_config::env::var_os("OMSI_TRAFFIC_ALL_GROUPS").is_some();
            let unscheduled: Option<Vec<(String, i32)>> =
                ::legacy_config::CfgFile::read(&world.map_dir.join("unsched_vehgroups.txt"))
                    .ok()
                    .map(|f| {
                        ::map::ailists::parse_unsched_vehgroups(&f)
                            .into_iter()
                            .map(|(n, c)| (n.trim().to_ascii_lowercase(), c))
                            .collect()
                    });
            if let Some(names) = &unscheduled {
                log::info!("random traffic groups (unsched_vehgroups.txt): {names:?}");
                uvg_defaults = names
                    .iter()
                    .map(|n| if all_groups && n.1 <= 0 { 1 } else { n.1 })
                    .collect();
            }
            let lists = &world.ailists;
            for g in lists.groups.iter().filter(|g| {
                !g.is_depot
                    && g.hof.is_none()
                    && !g
                    .vehicles
                    .iter()
                    .any(|v| v.file.to_ascii_lowercase().ends_with(".zug"))
            }) {
                let lname = g.name.trim().to_ascii_lowercase();
                let uvg = match &unscheduled {
                    Some(names) => match names.iter().position(|n| n.0 == lname) {
                        None => continue,
                        Some(u) => {
                            if uvg_defaults.get(u).copied().unwrap_or(0) <= 0 {
                                log::info!(
                                    "random traffic group {} drives only where its paths ask for it (unsched_vehgroups.txt)",
                                    g.name
                                );
                            }
                            Some(u)
                        }
                    },
                    None => None,
                };
                let gi = groups.len();
                group_uvg.push(uvg);
                groups.push(
                    dens.iter()
                        .find(|d| d.name.trim().eq_ignore_ascii_case(g.name.trim()))
                        .cloned()
                        .unwrap_or(::map::ailists::UnschedGroup {
                            name: g.name.clone(),
                            factor: if group_curves { 0.0 } else { 1.0 },
                            densities: Vec::new(),
                        }),
                );
                for v in &g.vehicles {
                    let lower = v.file.to_ascii_lowercase();
                    if lower.ends_with(".zug")
                        || lower.contains("trains\\")
                        || lower.contains("trains/")
                    {
                        continue;
                    }
                    let path = ::legacy_config::resolve_path(root, &v.file);
                    match VehicleType::load_ai(root, &path) {
                        Ok(t) => {
                            // `[type]` 2 = rail (only as scheduled trains), 3 = aircraft on flight paths
                            let rail =
                                matches!(t.def.kind, ::legacy_vehicle::vehicle::VehicleKind::Other(2))
                                    || t.def.rail_body_osc.is_some()
                                    || !t.def.contact_shoes.is_empty();
                            let air =
                                matches!(t.def.kind, ::legacy_vehicle::vehicle::VehicleKind::Other(3));
                            if rail {
                                log::debug!(
                                    "AI vehicle {} is rail-bound, not street traffic",
                                    v.file
                                );
                            } else {
                                types.push((
                                    Arc::new(t),
                                    v.weight.max(0.0),
                                    if air { LaneKind::Air } else { LaneKind::Street },
                                    gi,
                                ));
                            }
                        }
                        Err(e) => log::warn!("AI vehicle {}: {e}", v.file),
                    }
                }
            }
        }
        // No buses in the random road traffic: the depot groups of `ailists.cfg` are the
        // fleet the *timetable* drives, and OMSI puts a bus on a street only because a trip
        // of the map's TTData runs there. Mixing the depot fleet into the random pool put
        // the map's one bus type on every road of the map - on Grundorf that is a single
        // articulated GN92, which is why it seemed to be a type of our own choosing.
        if let Ok(list) = ::legacy_config::env::var("OMSI_DEBUG_LANES") {
            // lane indices, or `at:x,y,r` for the street lanes passing within r m of a point
            // (the indices change from run to run on a map whose tiles load in parallel)
            let chosen: Vec<usize> = match list.strip_prefix("at:") {
                Some(rest) => {
                    let v: Vec<f64> = rest
                        .split(',')
                        .filter_map(|x| x.trim().parse().ok())
                        .collect();
                    let (p, r) = (
                        DVec3::new(
                            v.first().copied().unwrap_or(0.0),
                            v.get(1).copied().unwrap_or(0.0),
                            0.0,
                        ),
                        v.get(2).copied().unwrap_or(10.0),
                    );
                    (0..net.lanes.len())
                        .filter(|&i| {
                            net.lanes[i].kind == LaneKind::Street
                                && net.lanes[i]
                                .points
                                .iter()
                                .any(|q| (q.truncate() - p.truncate()).length() < r)
                        })
                        .collect()
                }
                None => list
                    .split(',')
                    .filter_map(|v| v.trim().parse::<usize>().ok())
                    .filter(|&i| i < net.lanes.len())
                    .collect(),
            };
            for i in chosen {
                let l = &net.lanes[i];
                let samples: Vec<String> = (0..l.points.len())
                    .step_by((l.points.len() / 8).max(1))
                    .map(|k| {
                        format!(
                            "[{:.1} m h {:.1} k {:.3}]",
                            l.dist[k],
                            l.headings[k],
                            l.curvature.get(k).copied().unwrap_or(0.0)
                        )
                    })
                    .collect();
                log::info!(
                    "lane {i}: {} {:?} rev {} turn {} prio {} len {:.1} start ({:.1}, {:.1}) end ({:.1}, {:.1}) next {:?} light {:?} crossings {:?} {}",
                    l.name,
                    l.key,
                    l.reversed,
                    l.turn,
                    l.priority,
                    l.length(),
                    l.start().x,
                    l.start().y,
                    l.end().x,
                    l.end().y,
                    l.next,
                    l.traffic_light,
                    net.crossings.get(i),
                    samples.join(" ")
                );
            }
        }
        if ::legacy_config::env::var_os("OMSI_DEBUG_WHEELS").is_some() {
            for (t, ..) in &types {
                let v = VehicleInstance::new(
                    t.clone(),
                    ::simulation::VehicleHost::new(::simulation::SimClock::default()),
                );
                for line in v.wheel_pivot_report() {
                    log::info!("wheel pivot: {line}");
                }
            }
        }
        let lights = world.traffic_lights.lock().clone();
        let controller_of_object = world.controller_of_object.lock().clone();
        let parked: HashMap<usize, Vec<(f32, f32)>> = HashMap::new();
        let turning = net.lanes.iter().filter(|l| l.turn != 0).count();
        let with_side = net
            .lanes
            .iter()
            .filter(|l| l.left.is_some() || l.right.is_some())
            .count();
        let turn_lanes = net
            .lanes
            .iter()
            .filter(|l| {
                (l.left.is_some() || l.right.is_some())
                    && l.next.iter().any(|&n| net.lanes[n].turn != 0)
            })
            .count();
        let closed = net
            .lanes
            .iter()
            .filter(|l| l.no_cars || l.density <= 0.0)
            .count();
        let closed_junctions = net
            .lanes
            .iter()
            .filter(|l| (l.no_cars || l.density <= 0.0) && l.source == 2)
            .count();
        let quiet = net
            .lanes
            .iter()
            .filter(|l| l.density < 1.0 && l.density > 0.0)
            .count();
        let prio = net
            .lanes
            .iter()
            .filter(|l| (l.priority - ::traffic::DEFAULT_PRIORITY).abs() > 0.5)
            .count();
        log::info!(
            "traffic: {} lanes ({turning} turning, {with_side} with a neighbour, {turn_lanes} where a turn lane applies, {closed} closed to cars of which {closed_junctions} are junctions, {quiet} with less traffic by [rule], {prio} with a [rule] priority), {} AI vehicle types in {} groups, {} light programs, {} lamps",
            net.lanes.len(),
            types.len(),
            groups.len(),
            lights.len(),
            world.light_objects.lock().len()
        );
        // lights on paths a car reaches from a lit path of the same crossing (they hold
        // only a car that comes into the crossing there, see `light_at_entry`)
        let inner_lights = (0..net.lanes.len())
            .filter(|&l| {
                let lane = &net.lanes[l];
                lane.traffic_light.is_some()
                    && lane.source == 2
                    && net.prev.get(l).is_some_and(|ps| {
                    ps.iter().any(|&p| {
                        let q = &net.lanes[p];
                        q.source == 2
                            && q.traffic_light.is_some()
                            && q.key.map(|k| (k.tile, k.id)) == lane.key.map(|k| (k.tile, k.id))
                    })
                })
            })
            .count();
        log::info!(
            "traffic: {inner_lights} lit paths inside crossings (a car already in the crossing is not held there again)"
        );
        let light_log = ::legacy_config::env::var("OMSI_DEBUG_LIGHTS").ok();
        let light_prev = lights.iter().map(|c| vec![-100; c.lights.len()]).collect();
        let lanes = 0..net.lanes.len();
        let street_weight = net.lanes.iter().filter_map(street_lane_weight).sum();
        let mut t = Traffic {
            net,
            street_weight,
            parked,
            parked_shapes: Arc::new(Vec::new()),
            parked_collision: Default::default(),
            parked_waiting: Vec::new(),
            lane_tiles: lane_tiles.into_iter().collect(),
            lanes_generation: 0,
            types,
            groups,
            group_curves,
            group_uvg,
            uvg_defaults: Arc::new(uvg_defaults),
            cars: Vec::new(),
            dormant: Vec::new(),
            dormant_time: 0.0,
            rng: 0x9E37_79B9_7F4A_7C15,
            target,
            lights_only: false,
            spawn_radius: 400.0,
            time: 0.0,
            released: Vec::new(),
            camera: None,
            orphan_sounds: Vec::new(),
            lights,
            controller_of_object,
            trailer_types: HashMap::new(),
            sound_cfgs: HashMap::new(),
            root: root.to_path_buf(),
            held_at_red: 0,
            stop_wishes: None,
            player_still: 0.0,
            day_time: 0.0,
            time_scale: 1.0,
            weekday: 0,
            night: false,
            daylight: None,
            next_id: 1,
            last_overtaker: None,
            first_turner: None,
            first_red: None,
            first_yield: None,
            first_passer: None,
            density_curve: world.global.traffic_density_road.clone(),
            unsched_factor: ::config::get_float("ai", "unsched_factor").unwrap_or(1.0) as f32,
            max_scheduled: ::config::get_int("ai", "max_scheduled")
                .and_then(|v| u32::try_from(v).ok())
                .unwrap_or(0),
            viewer: None,
            occluders: None,
            road_collision: world.collision.lock().clone(),
            walkers: Vec::new(),
            people: Vec::new(),
            initial: true,
            last_dt: 0.0,
            lamp_dt: 0.0,
            trace: open_trace(),
            logged_hard: Default::default(),
            light_log,
            light_prev,
            debug_population: ::legacy_config::env::var_os("OMSI_DEBUG_POPULATION").is_some(),
            framed_spawns: Vec::new(),
            player: None,
            player_priority: false,
            player_emergency: false,
            others: Vec::new(),
            drivers: HashMap::new(),
            driver_pool: Vec::new(),
            tick_split: [0.0; 3],
            others_still: HashMap::new(),
            geo_prev: HashMap::new(),
            index_of: HashMap::new(),
            junctions: JunctionCoordinator::new(),
            services: ServiceCoordinator::new(),
            maneuvers: ManeuverCoordinator::new(),
            population: PopulationCoordinator::new(),
            pull_out_rooms: HashMap::new(),
            removed_scheduled: Vec::new(),
            twinned: Default::default(),
            keep_clear: Vec::new(),
            mirror: false,
            count_near: None,
            lan_centers: Vec::new(),
            capture: None,
            capture_written: false,
        };
        t.sort_parked(parked_cars, lanes);
        t.refresh_parked_geometry(world);
        Ok(t)
    }


    /// Attach explicitly listed cars (a `.zug` train): (type, reversed).
    pub fn attach_cars(
        &mut self,
        world: &World,
        renderer: &Renderer,
        scene: &mut Scene,
        car: usize,
        cars: &[(Arc<VehicleType>, bool)],
    ) {
        let c = &mut self.cars[car];
        for (t, rev) in cars {
            c.trailer_renders.push(world.add_vehicle_shared(
                renderer,
                scene,
                t,
                None,
                Some(&c.render),
            ));
            c.vehicle.attach_trailer_ex(t.clone(), *rev);
        }
    }


    /// The cars of car `ci`'s train, front to back, each with whether it is turned round
    /// (the first, the one that drives, is not).
    pub(crate) fn consist(&self, ci: usize) -> Vec<(Arc<VehicleType>, bool)> {
        let v = &self.cars[ci].vehicle;
        std::iter::once((v.ty.clone(), false))
            .chain(v.trailers.iter().map(|t| (t.ty.clone(), t.reversed)))
            .collect()
    }


    /// Couple `cars` behind car `ci` instead of the ones it has (a train made up anew).
    pub(crate) fn set_trailers(
        &mut self,
        world: &World,
        renderer: &Renderer,
        scene: &mut Scene,
        ci: usize,
        cars: &[(Arc<VehicleType>, bool)],
    ) {
        let c = &mut self.cars[ci];
        for r in c.trailer_renders.drain(..) {
            world.release_vehicle(renderer, scene, r);
        }
        c.vehicle.trailers.clear();
        self.attach_cars(world, renderer, scene, ci, cars);
    }


    /// Turn train `ci` round as Omsi.exe does for a trip whose `[trainreverse]` differs from
    /// how the train stands (0x613a98): the whole consist the other way, its last car
    /// leading - here the vehicle that drives is made anew as that car, standing where it
    /// stands (at `s` on `lane`, which runs the new way), and the others coupled behind it
    /// in the opposite order, each turned round. `behind`: the lanes the train has behind
    /// it now, nearest last, for the track its cars stand on. The car keeps its id and its
    /// service.
    pub(crate) fn turn_train(
        &mut self,
        world: &World,
        renderer: &Renderer,
        scene: &mut Scene,
        ci: usize,
        lane: usize,
        s: f32,
        behind: &[usize],
        reversed: bool,
    ) {
        let cars: Vec<(Arc<VehicleType>, bool)> = self
            .consist(ci)
            .into_iter()
            .rev()
            .map(|(t, r)| (t, !r))
            .collect();
        let Some((lead, lead_turned)) = cars.first().cloned() else {
            return;
        };
        if lead_turned {
            // (a lead car turned round is drawn facing the way: none of the stock trains has one)
            log::debug!(
                "train {}: its last car leads turned round",
                self.cars[ci].id
            );
        }
        let (id, seed, scheme) = (self.cars[ci].id, self.cars[ci].seed, self.cars[ci].scheme);
        // (where its cars stood, front to back: they stand there still, the other way round)
        let before: Vec<DVec3> = std::iter::once(self.cars[ci].vehicle.position)
            .chain(self.cars[ci].vehicle.trailers.iter().map(|t| t.position))
            .collect();
        let center = self.viewer.map(|v| v.pos).unwrap_or_default();
        let kind = self.net.lanes[lane].kind;
        self.create_car(
            world,
            renderer,
            scene,
            center,
            kind,
            lane,
            s,
            lead,
            seed,
            Some(scheme),
            Some(id),
            Some(0.0),
            None,
        );
        let Some(mut new) = self.cars.pop() else {
            return;
        };
        let old = &mut self.cars[ci];
        // its service goes with it (its line and destination are set for the trip it takes
        // on); the way it drives, from where it stands
        new.vehicle.host.hof = old.vehicle.host.hof.clone();
        new.bus = old.bus.take();
        new.state.max_speed_kmh = old.state.max_speed_kmh;
        new.state.length = old.state.length;
        new.state.accel = old.state.accel;
        new.state.decel = old.state.decel;
        new.state.lat_accel = old.state.lat_accel;
        new.state.min_gap = old.state.min_gap;
        new.state.speed = 0.0;
        new.consist_reversed = reversed;
        let old = std::mem::replace(&mut self.cars[ci], new);
        self.orphan_sounds.extend(old.sounds);
        for r in std::iter::once(old.render).chain(old.trailer_renders) {
            world.release_vehicle(renderer, scene, r);
        }
        self.set_trailers(world, renderer, scene, ci, &cars[1..]);
        self.seed_rail_trail(ci, behind);
        let c = &mut self.cars[ci];
        let trail = &c.rail_trail;
        let (state, net) = (&c.state, &self.net);
        c.vehicle
            .retrail(0.0, &|d| Some(rail_behind(trail, state, net, d)));
        let after: Vec<DVec3> = std::iter::once(c.vehicle.position)
            .chain(c.vehicle.trailers.iter().map(|t| t.position))
            .collect();
        let moved = before
            .iter()
            .rev()
            .zip(&after)
            .map(|(a, b)| (*a - *b).truncate().length())
            .fold(0.0f64, f64::max);
        log::info!(
            "train {id} turned round: {} (its cars {:.1} m at most from where they stood)",
            std::iter::once(&c.vehicle.ty)
                .chain(c.vehicle.trailers.iter().map(|t| &t.ty))
                .map(|t| t
                    .def
                    .path
                    .file_stem()
                    .unwrap_or_default()
                    .to_string_lossy()
                    .to_string())
                .collect::<Vec<_>>()
                .join(" + "),
            moved
        );
    }


    /// The track behind rail car `ci` as it has just been put on its lane: back along its
    /// lane and then `behind` (the lanes before it, nearest last), for its coupled cars.
    pub(crate) fn seed_rail_trail(&mut self, ci: usize, behind: &[usize]) {
        let c = &mut self.cars[ci];
        let net = &self.net;
        let odo = c.state.odometer as f64;
        let mut pts: Vec<(f64, DVec3)> = Vec::new();
        let (mut lane, mut s) = (c.state.lane, c.state.s);
        let mut back = behind.iter().rev();
        let mut d = 0.0f64;
        while d <= RAIL_TRAIL {
            pts.push((odo - d, net.lanes[lane].at(s.max(0.0)).0));
            s -= 1.0;
            if s < 0.0 {
                match back.next() {
                    Some(&l) => {
                        s += net.lanes[l].length();
                        lane = l;
                    }
                    None => break,
                }
            }
            d += 1.0;
        }
        c.rail_trail = pts.into_iter().rev().collect();
    }


    /// The vehicles coupled behind `ty` (its rear sections, trailers, the cars of a unit),
    /// each with whether it is turned round, loaded (once per file). As Omsi.exe builds a
    /// consist (0x70a174): towards the back of the train a vehicle goes on with its
    /// `[couple_back]`, or with its `[couple_front]` when it is itself turned round; the
    /// coupled one is turned round when the coupling's flag says so, against the one it
    /// hangs on; and a coupling back to the file it came from that turns nothing round is
    /// not followed. (Following `[couple_back]` whatever the way, the Berlin A3's unit -
    /// the S car and its K car turned round behind it, whose own `[couple_back]` names the
    /// S car again - went on S, K, S, K, S, none of them turned.)
    pub(crate) fn trailer_chain(&mut self, ty: &Arc<VehicleType>) -> Vec<(Arc<VehicleType>, bool)> {
        self.coupled_chain(ty, false, true)
    }


    /// See [`Traffic::trailer_chain`]: from `ty` (turned round: `rev`) towards the back of
    /// the train, or towards its front, the nearest first.
    pub(crate) fn coupled_chain(
        &mut self,
        ty: &Arc<VehicleType>,
        rev: bool,
        toward_back: bool,
    ) -> Vec<(Arc<VehicleType>, bool)> {
        let mut out = Vec::new();
        let (mut lead, mut lead_rev) = (ty.clone(), rev);
        for _ in 0..8 {
            let Some((path, r)) = crate::spawn::next_coupled(&lead.def, lead_rev, toward_back)
            else {
                break;
            };
            let root = self.root.clone();
            let t =
                self.trailer_types.entry(path.clone()).or_insert_with(
                    || match VehicleType::load_ai(&root, &path) {
                        Ok(t) => Some(Arc::new(t)),
                        Err(e) => {
                            log::warn!("trailer {}: {e}", path.display());
                            None
                        }
                    },
                );
            let Some(t) = t.clone() else { break };
            out.push((t.clone(), r));
            lead = t;
            lead_rev = r;
        }
        out
    }


    /// Load and attach the `[couple_back]` chain of `vehicle`; returns the renders.
    pub(crate) fn attach_trailers(
        &mut self,
        world: &World,
        renderer: &Renderer,
        scene: &mut Scene,
        vehicle: &mut VehicleInstance,
        scheme: Option<usize>,
        lead: &VehicleRender,
    ) -> Vec<VehicleRender> {
        let mut renders = Vec::new();
        let ty = vehicle.ty.clone();
        for (t, rev) in self.trailer_chain(&ty) {
            renders.push(world.add_vehicle_shared(
                renderer,
                scene,
                &t,
                scheme.filter(|i| *i < t.paint_schemes.len()),
                Some(lead),
            ));
            vehicle.attach_trailer_ex(t, rev);
        }
        renders
    }


    pub fn prime_pull_out_room(&mut self, ty: &VehicleType, bus: bool) {
        let (front, rear, half_width) = extents(ty, if bus { 12.0 } else { 4.5 });
        self.pull_out_room(ty, front, rear, half_width);
    }


    /// How far behind something standing a vehicle of `ty` stops so that it can pull out
    /// round it later (`::simulation::ai_motion::pull_out_room` against a standing bus with the
    /// oncoming lane 3.3 m over; by vehicle file).
    pub(crate) fn pull_out_room(&mut self, ty: &VehicleType, front: f32, rear: f32, half_width: f32) -> f32 {
        if let Some(&r) = self.pull_out_rooms.get(&ty.def.path) {
            return r;
        }
        // (a bus 2.5 m wide that stands up to 0.35 m further over than the car: bus and car
        // are rarely both in the middle of the lane)
        let r = ::simulation::ai_motion::pull_out_room(&ty.def, (front, rear, half_width), 1.6, 3.3);
        if ::legacy_config::env::var_os("OMSI_DEBUG_TRAFFIC").is_some() {
            log::info!(
                "pull-out room of {}: {r:.2} m (front {front:.2}, half width {half_width:.2})",
                ty.def
                    .path
                    .file_name()
                    .unwrap_or_default()
                    .to_string_lossy()
            );
        }
        self.pull_out_rooms.insert(ty.def.path.clone(), r);
        r
    }


    /// Put a random car of type `ty` on `lane` at `s` metres into it and return its id:
    /// `scheme` Some = that paint scheme (a car coming back from out of range keeps its
    /// looks), `id` Some = that id, `speed` Some = at about that speed.
    #[allow(clippy::too_many_arguments)]
    pub(crate) fn create_car(
        &mut self,
        world: &World,
        renderer: &Renderer,
        scene: &mut Scene,
        center: DVec3,
        kind: LaneKind,
        lane: usize,
        s: f32,
        ty: Arc<VehicleType>,
        seed: u64,
        scheme: Option<Option<usize>>,
        id: Option<VehicleId>,
        speed: Option<f32>,
        bus: Option<BusSetup>,
    ) -> VehicleId {
        let mut host = ::simulation::VehicleHost::new(::simulation::SimClock::default());
        host.font_lib = Some(world.fonts.clone());
        if let Some(b) = &bus {
            host.hof = b.hof.clone();
        }
        // random paint scheme / advert (its variables there for the scripts' {init})
        let scheme = match scheme {
            Some(s) => s,
            None if ty.paint_schemes.is_empty() => None,
            None => Some((seed >> 8) as usize % ty.paint_schemes.len().min(AI_SCHEMES)),
        };
        host.paint_scheme = Some(scheme);
        let mut vehicle = VehicleInstance::new(ty.clone(), host);
        if let Some((num, reg)) = bus.as_ref().and_then(|b| b.number.clone()) {
            if let Some(i) = ty.program.str_var("number") {
                vehicle.state.str_vars[i as usize] = num;
            }
            if let Some(i) = ty.program.str_var("ident") {
                vehicle.state.str_vars[i as usize] = reg;
            }
        } else {
            // A vehicle of the random traffic with a `[number]` list takes a number of it at
            // random and the plate beside it, else the plate its mode makes of the number
            // (TRoadVehicleInst.virtual_11 at 0x7e7b51); a free plate is one of the map's
            // registrations.txt.
            let numbers = ty.def.numbers_with_plates();
            if !numbers.is_empty() {
                let (n, plate) = &numbers[(seed.rotate_left(29) % numbers.len() as u64) as usize];
                if let Some(i) = ty.program.str_var("number") {
                    vehicle.state.str_vars[i as usize] = n.clone();
                }
                if ty.def.registration_mode != 1 {
                    if let Some(i) = ty.program.str_var("ident") {
                        vehicle.state.str_vars[i as usize] = if plate.is_empty() {
                            ty.def.plate_of_number(n)
                        } else {
                            plate.clone()
                        };
                    }
                }
            }
            if ty.def.registration_mode == 1 {
                if let (Some(i), Some(reg)) = (
                    ty.program.str_var("ident"),
                    world.free_registration(seed.rotate_left(17)),
                ) {
                    vehicle.state.str_vars[i as usize] = reg;
                }
            }
        }
        // aircraft keep the height of their flight path: a ground sampler would pull
        // them down onto the streets
        vehicle.ground = if kind == LaneKind::Air {
            None
        } else {
            Some(ai_ground(world))
        };
        // and what its wheels stand on, asked as the player's are (see `AiBody::settle`); a
        // coupled part (an articulated bus's rear, a lorry's trailer) asks it too, with the
        // height it is at - the plain sampler gave it the deck of a bridge over its road
        // (`OMSI_AI_WAY_ONLY=1`: on the way and the plain sampler, as before - A/B runs)
        vehicle.contact = (kind == LaneKind::Street
            && ::legacy_config::env::var_os("OMSI_AI_WAY_ONLY").is_none())
            .then(|| {
                std::sync::Arc::new(crate::scene::DriveGround {
                    terrains: world.terrains.clone(),
                    surfaces: world.surfaces.clone(),
                }) as std::sync::Arc<dyn ::simulation::rigid::Ground>
            });
        vehicle.apply_paint_vars(scheme);
        let render = world.add_vehicle_shared(renderer, scene, &ty, scheme, None);
        let trailer_renders =
            self.attach_trailers(world, renderer, scene, &mut vehicle, scheme, &render);
        if !ty.model.text_textures.is_empty() || bus.is_some() {
            vehicle.init_text_textures(&mut world.fonts.lock(), &|p| {
                ::texture::decode_file(p)
                    .ok()
                    .map(|i| (i.width, i.height, i.rgba))
            });
        }
        // a rear section's plates and numbers are `[texttexture]`s of its own reading the
        // leading vehicle's strings (`TrailerPart::update_text_textures`), so they need the
        // same fonts the front's do
        for t in vehicle.trailers.iter_mut() {
            t.init_text_textures(&mut world.fonts.lock(), &|p| {
                ::texture::decode_file(p)
                    .ok()
                    .map(|i| (i.width, i.height, i.rgba))
            });
        }
        let mut state = AiState::new(lane, s, seed);
        state.veh_type = if bus.is_some() {
            -1
        } else {
            ty.def.ai_veh_type
        };
        if bus.is_none() {
            state.traffic_pool = self
                .types
                .iter()
                .find(|t| Arc::ptr_eq(&t.0, &ty))
                .and_then(|t| self.group_uvg[t.3])
                .map(|pool| (pool, self.uvg_defaults.clone()));
        }
        state.plan_next(&self.net);
        // heavy vehicles (trucks, vans) cruise slower, which is what gets them overtaken
        let heavy = ty.def.mass > 6.0 || bus.is_some();
        personality(&mut state, seed, heavy);
        state.max_speed_kmh = if kind == LaneKind::Air {
            AIRCRAFT_KMH
        } else if bus.is_some() {
            // a bus driver keeps to the limit (the town's 50) like the cars round him,
            // with a little more on the arterial roads
            56.0 + (seed % 7) as f32
        } else if heavy {
            // (a truck keeps to the limit like the cars, up to a truck's own 80-90 km/h: it
            // took 38-47 on every road, crawling along 80 km/h roads, #327)
            80.0 + (seed % 10) as f32
        } else if kind == LaneKind::Street && ty.def.mass > 0.0 && ty.def.mass <= 0.3 {
            // a bicycle (stock ones weigh exactly 0.3 t): 15-21 km/h, the `vmax` range their
            // script cuts the drive at (#327)
            15.0 + (seed % 7) as f32
        } else {
            100.0
        };
        // lorries and vans take bends more gently than cars
        state.lat_accel = if kind == LaneKind::Air {
            50.0
        } else if heavy {
            1.6
        } else {
            2.4 + (seed % 7) as f32 * 0.1
        };
        if bus.is_some() {
            // it brakes for its stops the way the town's drivers brake for a light: with
            // 1.5 m/s² of "comfortable" braking the planner braked at half that and crept
            // up to every stop for a hundred metres
            state.decel = 2.1;
            state.accel = state.accel.max(1.0);
            state.min_gap = state.min_gap.max(2.2);
        }
        let mut caps = crate::traffic_runtime::content::capabilities(
            &ty,
            state.veh_type,
            state.max_speed_kmh,
            if bus.is_some() { 12.0 } else { 4.5 },
        );
        // `veh_type` is -1 for a timetable bus (so lane permissions read correctly); the
        // physical class therefore has to be set explicitly for the braking fallback.
        if bus.is_some() {
            caps.class = VehicleClass::Bus;
        }
        // The vehicle's braking envelope: the verified stop correction plus the explicit
        // provisional class strength. The driver's comfortable `decel` stays a personality
        // trait but is bounded by what the vehicle can actually do.
        state.brakes = caps.braking();
        state.decel = state.decel.min(state.brakes.max_decel.max(0.5));
        let (front, rear, half_width) = (caps.front, caps.rear, caps.half_width);
        state.front = front;
        state.rear = rear;
        state.length = front + rear;
        if let Some(b) = &bus {
            state.set_route(&self.net, b.route.clone(), s);
        }
        state.speed = (self.net.lanes[lane]
            .speed_limit_kmh
            .min(state.max_speed_kmh)
            / 3.6
            * 0.7)
            .min(state.curve_speed(&self.net));
        if kind == LaneKind::Air {
            state.speed = self.net.lanes[lane]
                .speed_limit_kmh
                .min(state.max_speed_kmh)
                / 3.6;
            state.accel = 0.5;
            state.decel = 0.5;
        }
        // a bus put out at a stop stands there (in the bay, if it has one)
        let at_stop = bus
            .as_ref()
            .and_then(|b| b.stops.first())
            .filter(|st| st.route_index == 0 && (st.s - s).abs() < 1.5);
        if let Some(st) = at_stop {
            state.speed = 0.0;
            if st.bay.abs() > 0.01 {
                state.lateral = st.bay;
                state.lateral_target = st.bay;
                state.lateral_ramp = (st.bay, st.bay, 0.0, 1.0);
            }
        }
        let body = place_body(&self.net, &state, &mut vehicle, motion_kind(kind));
        if ::legacy_config::env::var_os("OMSI_DEBUG_TRAFFIC").is_some() {
            let pos = vehicle.position;
            log::info!(
                "spawn {} on lane {lane} s={s:.1} at ({:.1}, {:.1}, {:.1}) heading {:.0}",
                ty.def.path.display(),
                pos.x,
                pos.y,
                pos.z,
                vehicle.heading
            );
        }
        let pass_room = if kind == LaneKind::Street {
            self.pull_out_room(&ty, front, rear, half_width)
        } else {
            0.0
        };
        let id = id.unwrap_or_else(|| {
            let id = VehicleId(self.next_id);
            self.next_id += 1;
            id
        });
        if let Some(v) = speed {
            state.speed = v.min(state.speed.max(v * 0.5));
        }
        if self.debug_population && kind == LaneKind::Street {
            let v = self.viewer;
            let pos = vehicle.position;
            if !self.initial && v.map(|v| v.frames(pos, 2.5)).unwrap_or(false) {
                self.framed_spawns.push((id, pos));
            }
            log::info!(
                "population t={:.1}: car {id} appears at ({:.0}, {:.0}), {:.0} m from the centre, {:.0} m from the camera, in frame {}, behind a building {}{}",
                self.time,
                pos.x,
                pos.y,
                (pos - center).length(),
                v.map(|v| (pos - v.pos).length()).unwrap_or(0.0),
                v.map(|v| v.frames(pos, 2.5)).unwrap_or(false),
                v.map(|v| self.occluded(world, &v, pos, 2.5))
                    .unwrap_or(false),
                if self.initial { " (initial)" } else { "" }
            );
        }
        self.cars.push(AiCar {
            id,
            caps,
            motion_fault: None,
            scenery_streak: 0.0,
            scenery_ahead: None,
            state,
            vehicle,
            render,
            trailer_renders,
            body,
            stopped: 0.0,
            lead_car: None,
            ignore_lead: None,
            crawl: 0.0,
            bus: bus.map(|b| Box::new(BusService::new(b.stops))),
            sounds: None,
            half_width,
            yielding: false,
            light_hold: false,
            junction_state: JunctionState::Approaching,
            maneuver: ManeuverState::default(),
            gone: false,
            fresh: 1.5,
            merge_after: None,
            holding: None,
            why: (Reason::NONE, 0.0),
            held: false,
            geo_block: None,
            lead_info: None,
            junction_why: String::new(),
            wait_at: None,
            squeeze: None,
            pass_room,
            horn_cooldown: 0.0,
            light_at: None,
            rail_trail: Default::default(),
            consist_reversed: false,
            seed,
            scheme,
        });
        id
    }


    /// The vehicle/paint sets the random traffic draws from.
    pub fn random_sets(&self) -> Vec<(Arc<VehicleType>, Option<usize>)> {
        let mut sets: Vec<(Arc<VehicleType>, Option<usize>)> = Vec::new();
        for (ty, ..) in &self.types {
            let n = ty.paint_schemes.len().min(AI_SCHEMES);
            let schemes: Vec<Option<usize>> = if n == 0 {
                vec![None]
            } else {
                (0..n).map(Some).collect()
            };
            for scheme in schemes {
                if !sets
                    .iter()
                    .any(|(t, s)| t.def.path == ty.def.path && *s == scheme)
                {
                    sets.push((ty.clone(), scheme));
                }
            }
        }
        sets
    }


    pub fn precache_random(&mut self, world: &World, renderer: &Renderer, scene: &mut Scene) {
        let t0 = std::time::Instant::now();
        let sets = self.random_sets();
        for chunk in sets.chunks(3) {
            world.prefetch_vehicle_sets(renderer, chunk);
            for (ty, scheme) in chunk {
                world.precache_vehicle(renderer, scene, ty, *scheme);
            }
        }
        world.forget_prefetched();
        for (ty, _) in &sets {
            self.prime_pull_out_room(ty, false);
        }
        log::info!(
            "traffic: {} vehicle/paint sets of the random traffic read and uploaded in {:.1} s",
            sets.len(),
            t0.elapsed().as_secs_f32()
        );
    }


    /// A vehicle type the traffic has loaded already (the random traffic's types and the
    /// coupled parts), by file.
    pub fn loaded_type(&self, path: &Path) -> Option<Arc<VehicleType>> {
        self.types
            .iter()
            .map(|t| &t.0)
            .chain(self.trailer_types.values().flatten())
            .find(|t| t.def.path == path)
            .cloned()
    }

}
