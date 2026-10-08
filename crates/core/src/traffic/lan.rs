//! LAN mirror adapter: host snapshots and client presentation.

use super::*;

impl Traffic {

    /// The centres the LAN host last sent for replication.
    pub fn set_lan_centers(&mut self, centers: Vec<DVec3>) {
        self.lan_centers = centers;
    }


    /// Apply a host-replicated car state (LAN client mirror): pose, speed, blinker, brake,
    /// station phase, and steering. Running the scripts is the motion adapter's job.
    #[allow(clippy::too_many_arguments)]
    pub fn apply_host_car(
        &mut self,
        ci: usize,
        pose: DVec3,
        heading: f64,
        pitch: f32,
        bank: f32,
        speed: f32,
        blinker: i32,
        brake: bool,
        at_station: bool,
        steer: f32,
    ) {
        let car = &mut self.cars[ci];
        car.vehicle.position = pose;
        car.vehicle.heading = heading;
        car.vehicle.pitch = pitch;
        car.vehicle.bank = bank;
        car.state.speed = speed;
        car.state.blinker = blinker;
        car.state.braking = brake;
        if let Some(b) = car.bus.as_mut() {
            b.state.phase = if at_station {
                ServicePhase::Boarding
            } else {
                ServicePhase::EnRoute
            };
        }
        car.body.steer = steer;
    }


    /// Make the population deterministic for a LAN room.  The room's session id is
    /// shared by the host and every client, so the same map/time produces the same
    /// initial cars instead of each process inventing a different world.
    pub fn set_lan_seed(&mut self, seed: u64) {
        self.rng = seed | 1;
    }

    /// Street traffic around the other players of a LAN session too (host): each of them
    /// gets its own share of cars where no other player's share lies already.
    pub(crate) fn populate_lan_centers(
        &mut self,
        world: &World,
        renderer: &Renderer,
        scene: &mut Scene,
        center: DVec3,
        target: usize,
        occupancy: &Occupancy,
    ) {
        let centers = self.lan_centers.clone();
        let mut done = vec![center];
        for c in centers {
            if done
                .iter()
                .any(|d| (*d - c).truncate().length() < self.spawn_radius)
            {
                continue;
            }
            self.count_near = Some((c, self.spawn_radius));
            self.populate_kind(world, renderer, scene, c, LaneKind::Street, target, occupancy);
            self.count_near = None;
            done.push(c);
        }
    }


    pub fn is_mirror(&self) -> bool {
        self.mirror
    }


    /// Draw the host's traffic from now on (`on`), or simulate our own again: either way
    /// every car there is now goes (ours make room for the host's, the host's copies
    /// cannot drive on by themselves).
    pub fn set_mirror(&mut self, world: &World, renderer: &Renderer, scene: &mut Scene, on: bool) {
        if self.mirror == on {
            return;
        }
        self.mirror = on;
        let ids: Vec<VehicleId> = self.cars.iter().map(|c| c.id).collect();
        for id in ids {
            self.remove_car(world, renderer, scene, id);
        }
        self.initial = !on;
        log::info!(
            "traffic: {}",
            if on {
                "the LAN host's traffic is drawn instead of our own"
            } else {
                "simulating our own traffic again"
            }
        );
    }


    /// A client's frame: the clock and the light programs run on (the host corrects them
    /// every second, `set_light_state`); the cars are moved by `lan_world`.
    pub(crate) fn mirror_tick(&mut self, dt: f32) {
        self.time += dt;
        self.day_time += dt as f64 * self.time_scale;
        self.last_dt = dt;
        let day_time = self.day_time;
        for c in self.lights.iter_mut() {
            c.request.iter_mut().for_each(|r| *r = false);
            c.start(day_time);
            c.advance(dt);
        }
        self.log_lights();
    }


    /// A car of the host's traffic, standing at `pos` (client). Its id is the host's.
    #[allow(clippy::too_many_arguments)]
    pub fn add_mirror_car(
        &mut self,
        world: &World,
        renderer: &Renderer,
        scene: &mut Scene,
        id: VehicleId,
        ty: Arc<VehicleType>,
        scheme: Option<usize>,
        scheduled: bool,
        pos: DVec3,
        heading: f64,
    ) -> usize {
        let mut host = ::simulation::VehicleHost::new(::simulation::SimClock::default());
        host.font_lib = Some(world.fonts.clone());
        let scheme = scheme.filter(|i| *i < ty.paint_schemes.len());
        host.paint_scheme = Some(scheme);
        let mut vehicle = VehicleInstance::new(ty.clone(), host);
        // (the host's poses say where it stands; nothing here pulls it onto the ground)
        vehicle.ground = None;
        vehicle.apply_paint_vars(scheme);
        let render = world.add_vehicle_shared(renderer, scene, &ty, scheme, None);
        let trailer_renders =
            self.attach_trailers(world, renderer, scene, &mut vehicle, scheme, &render);
        if !ty.model.text_textures.is_empty() {
            vehicle.init_text_textures(&mut world.fonts.lock(), &|p| {
                ::texture::decode_file(p)
                    .ok()
                    .map(|i| (i.width, i.height, i.rgba))
            });
        }
        vehicle.position = pos;
        vehicle.heading = heading;
        let caps = crate::traffic_runtime::content::capabilities(&ty, 0, 50.0, 4.5);
        let (front, rear, half_width) = (caps.front, caps.rear, caps.half_width);
        let mut state = AiState::new(0, 0.0, id.get());
        state.front = front;
        state.rear = rear;
        state.length = front + rear;
        let body = AiBody::new(&ty.def, MotionKind::Road);
        self.cars.push(AiCar {
            id,
            caps,
            motion_fault: None,
            state,
            vehicle,
            render,
            trailer_renders,
            body,
            stopped: 0.0,
            lead_car: None,
            ignore_lead: None,
            crawl: 0.0,
            bus: scheduled.then(|| Box::new(BusService::new(Vec::new()))),
            sounds: None,
            half_width,
            yielding: false,
            light_hold: false,
            junction_state: JunctionState::Approaching,
            maneuver: ManeuverState::default(),
            gone: false,
            fresh: 0.0,
            merge_after: None,
            holding: None,
            why: (Reason::NONE, 0.0),
            held: false,
            geo_block: None,
            lead_info: None,
            junction_why: String::new(),
            wait_at: None,
            seed: 0,
            scheme: None,
            squeeze: None,
            pass_room: 0.0,
            horn_cooldown: 0.0,
            light_at: None,
            rail_trail: Default::default(),
            consist_reversed: false,
        });
        self.cars.len() - 1
    }

}
