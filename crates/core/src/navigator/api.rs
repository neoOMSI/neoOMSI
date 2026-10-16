use super::*;

fn build_pool() -> Option<std::sync::Arc<rayon::ThreadPool>> {
    let cpus = std::thread::available_parallelism().map_or(4, |n| n.get());
    rayon::ThreadPoolBuilder::new()
        .num_threads((cpus / 8).clamp(1, 2))
        .thread_name(|i| format!("navigator map {i}"))
        // (minutes of work on a big map that the frame need not wait for, see `threads`)
        .start_handler(|_| crate::threads::lower_thread_priority())
        .build()
        .ok()
        .map(std::sync::Arc::new)
}

fn on_pool<T: Send>(
    pool: &Option<std::sync::Arc<rayon::ThreadPool>>,
    f: impl FnOnce() -> T + Send,
) -> T {
    match pool {
        Some(p) => p.install(f),
        None => f(),
    }
}

impl Navigator {
    pub fn new(enabled: bool, opacity: f32, corner: &str) -> Navigator {
        Navigator {
            panel_overlay: None,
            cockpit_display: false,
            drawn_at: f32::MIN,
            enabled,
            schedule: false,
            speed_avg: 8.0,
            dim_ahead: 0.0,
            glass: None,
            dim_at: 0.0,
            opacity: opacity.clamp(0.2, 1.0),
            corner: corner.to_string(),
            city: CityMap::default(),
            panel_rect: [0.0; 4],
            gpu: None,
            fonts: Fonts::new(),
            atlas: Atlas::new(1024),
            target: None,
            bottom_t: 0.0,
            bottom_e: 0.0,
            turn_t: 0.0,
            turn_e: 0.0,
            turn_shown: None,
            show_topbar: true,
            show_turn: true,
            show_stoplist: true,
            sched_t: 0.0,
            sched_e: 0.0,
            sched_rows: 1.0,
            own_net: None,
            global: None,
            stop_pos: Default::default(),
            streets: None,
            graph: None,
            building: None,
            build_failed: false,
            surfaces: None,
            global_version: 0,
            roads: None,
            surfaces_ready: false,
            chunks_waiting: Vec::new(),
            chunks_near: false,
            chunks_added: 0.0,
            roads_building: None,
            route: Route::default(),
            route_mesh: RouteMesh::default(),
            route_cum: (0, Vec::new()),
            congestion: HashMap::new(),
            route_jam: HashMap::new(),
            jam_version: 0,
            congestion_t: 0.0,
            zoom: 120.0,
            cam_heading: 0.0,
            time: 0.0,
            next_dist: None,
            arrows: false,
            show_ai: true,
            shown: if enabled { 1.0 } else { 0.0 },
            stop_spots: Vec::new(),
            bus_at: DVec3::ZERO,
            next_turn: None,
            street_here: None,
            dist_t: 0.0,
            jam_cost: 0.0,
            first: true,
        }
    }

    pub fn start_map(&mut self, world: std::sync::Arc<crate::scene::World>, centre: DVec2) {
        if self.building.is_some() || self.global.is_some() || self.build_failed {
            return;
        }
        let (tx, rx) = std::sync::mpsc::channel();
        let spawned = std::thread::Builder::new()
            .name("navigator map".into())
            .spawn(move || {
                crate::threads::lower_thread_priority();
                let t0 = std::time::Instant::now();
                let pool = build_pool();
                // the ground: as the last session left it if the map is unchanged, else built
                // from the map's paths (sent once read) and kept for the next one
                let (tx_lanes, rx_lanes) = std::sync::mpsc::channel::<Vec<::traffic::Lane>>();
                let tx_ground = tx.clone();
                let world_ground = world.clone();
                let pool_ground = pool.clone();
                let ground = std::thread::Builder::new()
                    .name("navigator ground".into())
                    .spawn(move || {
                        crate::threads::lower_thread_priority();
                        if let Some((chunks, meta)) = crate::navmap::load_cached(&world_ground) {
                            let _ = tx_ground.send(MapPart::Chunks(chunks));
                            let _ = tx_ground.send(MapPart::GroundDone(Box::new(meta)));
                            return;
                        }
                        let Ok(lanes) = rx_lanes.recv() else {
                            return;
                        };
                        on_pool(&pool_ground, || {
                            let mut all = Vec::new();
                            let mut emit = |part: Vec<_>| {
                                all.extend(part.iter().cloned());
                                let _ = tx_ground.send(MapPart::Chunks(part));
                            };
                            let mut meta = crate::navmap::build_surface_stream(
                                &world_ground,
                                &lanes,
                                centre,
                                &mut emit,
                            );
                            let read = std::mem::take(&mut meta.read);
                            let _ = tx_ground.send(MapPart::GroundDone(Box::new(meta.clone())));
                            crate::navmap::save_cached(&world_ground, &all, &meta, read);
                        });
                    })
                    .ok();
                let m = on_pool(&pool, || world.navigation_map());
                let _ = tx_lanes.send(m.lanes.clone());
                let mut net = Network {
                    lanes: m.lanes,
                    ..Default::default()
                };
                net.link(1.5);
                confirm_road_surfaces(&mut net, &m.road_surfaces);
                probe_lanes(&net);
                let streets = build_streets(&net, &m.signs);
                let graph = RoadGraph::build(&net, &m.carriageways);
                log::info!(
                    "navigator: network built in {:.0} ms",
                    t0.elapsed().as_secs_f64() * 1e3
                );
                let _ = tx.send(MapPart::Network(Box::new((net, m.places, streets, graph))));
                if let Some(g) = ground {
                    let _ = g.join();
                }
            })
            .is_ok();
        if !spawned {
            self.build_failed = true;
            return;
        }
        self.building = Some(rx);
    }

    pub fn set_map(&mut self, map: crate::scene::NavigationMap) {
        let mut net = Network {
            lanes: map.lanes,
            ..Default::default()
        };
        net.link(1.5);
        confirm_road_surfaces(&mut net, &map.road_surfaces);
        probe_lanes(&net);
        self.streets = Some(std::sync::Arc::new(build_streets(&net, &map.signs)));
        self.graph = Some(std::sync::Arc::new(RoadGraph::build(&net, &map.carriageways)));
        self.roads = None;
        let global = std::sync::Arc::new(net);
        self.global = Some(global);
        self.stop_pos = std::sync::Arc::new(map.places);
        self.global_version += 1;
    }

    /// Navigator 2.0's ground for the map (see [`crate::navmap`]).
    pub fn set_surfaces(&mut self, surfaces: crate::navmap::SurfaceMap) {
        self.surfaces = Some(std::sync::Arc::new(surfaces));
        self.surfaces_ready = true;
        self.roads = None;
        self.city.roads = None;
    }

    pub fn places(&self) -> Option<&HashMap<i64, DVec3>> {
        self.global.as_ref().map(|_| &*self.stop_pos)
    }

    pub fn map_net(&self) -> Option<&Network> {
        self.global.as_deref()
    }

    pub fn map_net_arc(&self) -> Option<std::sync::Arc<Network>> {
        self.global.clone()
    }

    pub fn add_lanes(&mut self, lanes: Vec<::traffic::Lane>) {
        if lanes.is_empty() {
            return;
        }
        match self.own_net.as_mut() {
            Some(n) => {
                if let Some(n) = std::sync::Arc::get_mut(n) {
                    n.extend(lanes, 1.5);
                    n.build_grid();
                }
            }
            None => {
                let mut n = Network {
                    lanes,
                    ..Default::default()
                };
                n.link(1.5);
                n.build_grid();
                self.own_net = Some(std::sync::Arc::new(n));
            }
        }
    }

    pub fn wants_route(&self, key: &str, generation: u64) -> bool {
        if self.global.is_some() {
            return self.route.key != key
                || self.route.generation != self.global_version + (1 << 40);
        }
        self.route.key != key || (!self.route.complete && self.route.generation != generation)
    }

    pub fn set_route(&mut self, key: &str, lanes: Vec<usize>, complete: bool, generation: u64) {
        let same_trip = self.route.key == key;
        self.route.key = key.to_string();
        self.route.complete = complete;
        self.route.generation = generation;
        if same_trip
            && !self.route.lanes.is_empty()
            && !self.route.on_route
            && !self.route.provisional
        {
            return;
        }
        if self.route.provisional && lanes.is_empty() {
            return;
        }
        self.route.provisional = false;
        self.route.lanes = lanes;
        self.route.lead = 0;
        if !same_trip {
            self.route.progress = 0;
            self.route.on_route = false;
            self.route.off_for = 0.0;
        }
        self.route.version += 1;
    }

    pub fn clear_route(&mut self) {
        if !self.route.key.is_empty() || !self.route.lanes.is_empty() {
            self.route = Route {
                version: self.route.version + 1,
                ..Route::default()
            };
        }
    }
}
