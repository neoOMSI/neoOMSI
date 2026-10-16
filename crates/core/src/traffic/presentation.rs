//! Presentation, lighting, audio and viewer sync adapter.

use super::*;

impl Traffic {

    /// Whether AI vehicles switch their lights on.
    pub fn set_night(&mut self, night: bool) {
        self.night = night;
    }


    /// Whether AI vehicles currently switch their lights on.
    pub fn night(&self) -> bool {
        self.night
    }


    /// How many vehicles are currently held at a red light.
    pub fn held_at_red(&self) -> usize {
        self.held_at_red
    }


    /// The current daylight values.
    pub fn set_daylight(&mut self, daylight: ::simulation::Daylight) {
        self.daylight = Some(daylight);
    }


    /// Whether only the light programs run (no vehicles yet).
    pub fn set_lights_only(&mut self, lights_only: bool) {
        self.lights_only = lights_only;
    }


    /// Advance all cars.
    /// `player`: (centre, heading in degrees, half length, half width, speed) of the
    /// player's vehicle.
    /// The railway signals' aspects from where the trains are: a signal shows go (1, or 2
    /// with a `[speedlimit]`) while a train's route is about to enter the track its signal
    /// route covers and no other train is on it; otherwise it shows stop (and falls back to
    /// stop behind the train that passed it).
    pub fn signal_aspects(
        &self,
        routes: &[::map::ailists::SignalRoute],
        player_rail: Option<(usize, bool)>,
    ) -> hashbrown::HashMap<i64, f32> {
        let mut out: hashbrown::HashMap<i64, f32> = hashbrown::HashMap::new();
        if routes.is_empty() {
            return out;
        }
        // per train: the map ids it stands on and those of its next lanes
        let mut trains: Vec<(i64, Vec<i64>)> = self
            .cars
            .iter()
            .filter(|c| {
                !c.state.route.is_empty()
                    && self
                    .net
                    .lanes
                    .get(c.state.lane)
                    .map(|l| l.kind == ::traffic::LaneKind::Rail)
                    .unwrap_or(false)
            })
            .map(|c| {
                let here = self.net.lanes[c.state.lane].key.map(|k| k.id).unwrap_or(-1);
                let ahead = c
                    .state
                    .route
                    .iter()
                    .skip(c.state.route_index + 1)
                    .take(40)
                    .filter_map(|&l| self.net.lanes.get(l).and_then(|l| l.key).map(|k| k.id))
                    .collect();
                (here, ahead)
            })
            .collect();
        // the player's own train (driven on the rails): the lanes ahead of it the way it goes,
        // every branch at a fork (which it takes is not known yet) - its signals stayed at
        // stop, only an AI train ever cleared them
        if let Some((lane, along)) = player_rail.filter(|(l, _)| *l < self.net.lanes.len()) {
            let here = self.net.lanes[lane].key.map(|k| k.id).unwrap_or(-1);
            let mut ahead: Vec<i64> = Vec::new();
            let mut frontier = vec![lane];
            for _ in 0..6 {
                let mut next = Vec::new();
                for l in frontier {
                    let nb: Vec<usize> = if along {
                        self.net.lanes[l].next.clone()
                    } else {
                        self.net.prev.get(l).cloned().unwrap_or_default()
                    };
                    for n in nb {
                        if let Some(k) = self.net.lanes.get(n).and_then(|x| x.key) {
                            if !ahead.contains(&k.id) {
                                ahead.push(k.id);
                            }
                        }
                        next.push(n);
                    }
                }
                frontier = next;
                if frontier.len() > 32 {
                    break;
                }
            }
            trains.push((here, ahead));
        }
        for r in routes {
            let pieces: hashbrown::HashSet<i64> = r.entries.iter().map(|e| e[0]).collect();
            let occupied = trains.iter().any(|(here, _)| pieces.contains(here));
            let wanted = trains.iter().any(|(here, ahead)| {
                !pieces.contains(here) && ahead.iter().any(|id| pieces.contains(id))
            });
            let aspect = if wanted && !occupied {
                if r.speed_limit.is_some() { 2.0 } else { 1.0 }
            } else {
                0.0
            };
            let e = out.entry(r.signal.0).or_insert(0.0);
            *e = e.max(aspect);
        }
        out
    }


    /// The switches the trains need set: (map object, `[path]` index) of the next lanes of
    /// every train's timetable route. A train throws the points ahead of it to the
    /// `[switchdir]` of the path its route takes (see `World::set_switches`).
    pub fn switch_requests(&self) -> Vec<(i64, u16)> {
        let mut out = Vec::new();
        for c in &self.cars {
            let st = &c.state;
            if st.route.is_empty()
                || self
                .net
                .lanes
                .get(st.lane)
                .map(|l| l.kind != ::traffic::LaneKind::Rail)
                .unwrap_or(true)
            {
                continue;
            }
            for &l in st.route.iter().skip(st.route_index).take(5) {
                if let Some(k) = self.net.lanes.get(l).and_then(|l| l.key) {
                    out.push((k.id, k.path));
                }
            }
        }
        out
    }


    /// `OMSI_DEBUG_LIGHTS`: every change of the lights of the chosen programs, with the
    /// game time and the program's cycle position.
    pub(crate) fn log_lights(&mut self) {
        let Some(sel) = self.light_log.as_deref() else {
            return;
        };
        let near = sel.eq_ignore_ascii_case("near");
        let chosen: Vec<usize> = sel
            .split(',')
            .filter_map(|v| v.trim().parse().ok())
            .collect();
        let viewer = self.viewer.map(|v| v.pos);
        for (ci, c) in self.lights.iter().enumerate() {
            let pick = sel.eq_ignore_ascii_case("all")
                || chosen.contains(&ci)
                || (near && viewer.is_some());
            if !pick {
                continue;
            }
            if near {
                // the programs within 150 m of the camera (by the lanes they control)
                let vp = viewer.unwrap();
                let close = self.net.lanes.iter().any(|l| {
                    l.traffic_light.map(|t| t.0) == Some(ci) && (l.start() - vp).length() < 150.0
                });
                if !close {
                    continue;
                }
            }
            for li in 0..c.lights.len() {
                let s = c.state(li);
                let prev = self.light_prev[ci][li];
                if s != prev {
                    let h = self.day_time.rem_euclid(86400.0);
                    log::info!(
                        "light {ci}.{li}: {:?} ({s}) at {:02}:{:02}:{:05.2} (cycle {:.2} of {:.0} s{}{})",
                        TrafficLightController::aspect(s),
                        (h / 3600.0) as u32,
                        ((h / 60.0) % 60.0) as u32,
                        h % 60.0,
                        c.time,
                        c.cycle_len(),
                        if c.held { ", held" } else { "" },
                        if c.request.get(li).copied().unwrap_or(false) {
                            ", requested"
                        } else {
                            ""
                        }
                    );
                    self.light_prev[ci][li] = s;
                }
            }
        }
    }


    /// Play the `[sound_ai]` sets of the cars near `listener` (others are silenced).
    /// `street_cond` is the state of the road (see `VehicleHost::street_cond`): the stock
    /// AI sound configuration fades `WetLane_1`/`WetLane_2` in with it, which is what a car
    /// driving past through the wet sounds like. `muffled`: the listener (the player) sits in
    /// a cabin right now, so every AI car's sound is heard through that bodywork and glass -
    /// a passing car's horn does not simply sound like the street outside once the windows
    /// are shut.
    pub fn update_audio(
        &mut self,
        audio: &::audio::AudioEngine,
        listener: DVec3,
        street_cond: f32,
        muffled: bool,
    ) {
        let freed = audio.trim_clips(std::time::Duration::from_secs(60));
        if freed > 0 && ::legacy_config::env::var_os("OMSI_PROFILE").is_some() {
            log::info!(
                "sound clips: {:.1} MB nobody used for a minute let go",
                freed as f64 / 1e6
            );
        }
        for mut s in self.orphan_sounds.drain(..) {
            s.stop_all(audio);
        }
        let near = 250.0;
        for c in &mut self.cars {
            let d = (c.vehicle.position - listener).length();
            if d > near * 1.2 {
                if let Some(mut s) = c.sounds.take() {
                    s.stop_all(audio);
                }
                continue;
            }
            if c.sounds.is_none() && d < near {
                let def = &c.vehicle.ty.def;
                let Some(rel) = def.sound_ai.clone().or_else(|| def.sound.clone()) else {
                    continue;
                };
                let path = ::mods::resolve_path(def.dir(), &rel);
                let cfg = self
                    .sound_cfgs
                    .entry(path.clone())
                    .or_insert_with(|| {
                        ::legacy_vehicle::SoundCfg::load(&path)
                            .map_err(|e| log::warn!("{e}"))
                            .ok()
                            .map(Arc::new)
                    })
                    .clone();
                if let Some(cfg) = cfg {
                    let dir = path.parent().map(|p| p.to_path_buf()).unwrap_or_default();
                    // an articulated bus's rear section sounds too (its engine, on a pusher)
                    let mut parts = Vec::new();
                    for (i, t) in c.vehicle.trailers.iter().enumerate() {
                        let def = &t.ty.def;
                        let Some(rel) = def.sound_ai.clone().or_else(|| def.sound.clone()) else {
                            continue;
                        };
                        let path = ::mods::resolve_path(def.dir(), &rel);
                        let part = self
                            .sound_cfgs
                            .entry(path.clone())
                            .or_insert_with(|| {
                                ::legacy_vehicle::SoundCfg::load(&path)
                                    .map_err(|e| log::warn!("{e}"))
                                    .ok()
                                    .map(Arc::new)
                            })
                            .clone();
                        if let Some(part) = part {
                            let dir = path.parent().map(|p| p.to_path_buf()).unwrap_or_default();
                            parts.push((i, part, dir));
                        }
                    }
                    // the clips are read in the background the first time; silent till then
                    let ready = audio.clips_ready(&::audio::SoundSet::clip_paths(&cfg, &dir))
                        && parts.iter().all(|(_, part, dir)| {
                        audio.clips_ready(&::audio::SoundSet::clip_paths(part, dir))
                    });
                    if ready {
                        let number = c.vehicle.number();
                        let mut ss = ::audio::SoundSet::new_exterior(
                            audio,
                            &cfg.chosen_for(&number),
                            &dir,
                        );
                        for (i, part, dir) in &parts {
                            ss.add_part(
                                *i,
                                ::audio::SoundSet::new_exterior(
                                    audio,
                                    &part.chosen_for(&number),
                                    dir,
                                ),
                            );
                        }
                        ss.master = crate::sound_gain(&crate::SOUND_AI);
                        c.vehicle.host.snapshot_triggers =
                            ss.curve_triggers().into_iter().collect();
                        c.sounds = Some(ss);
                    }
                }
            }
            let fired: Vec<::simulation::host::FiredSound> =
                std::mem::take(&mut c.vehicle.host.fired_sounds);
            let fired_vars: Vec<(String, Vec<f32>)> =
                std::mem::take(&mut c.vehicle.host.fired_trigger_vars);
            c.vehicle.host.street_cond = street_cond;
            c.vehicle.set_engine_var("StreetCond", street_cond);
            if let Some(ss) = c.sounds.as_mut() {
                ss.set_muffled(muffled);
                let xf = c.vehicle.world_transform();
                let v = &c.vehicle;
                let events = crate::sound_events::events_from(
                    ::audio::EventSource::Traffic,
                    &fired,
                    &fired_vars,
                );
                ss.update_events(audio, &|n| v.var(n), &xf, &events, &|n| v.var_slot(n));
                ss.update_parts_events(
                    audio,
                    &|n| v.var(n),
                    &|i| v.trailers.get(i).map(|t| t.world_transform()),
                    &events,
                    &|n| v.var_slot(n),
                );
            }
        }
    }


    /// Obstacle boxes of all AI vehicles (for the player's collisions), with the rear
    /// sections of articulated buses and the trailers.
    pub fn boxes(&self, near: DVec3, radius: f64) -> Vec<::simulation::collision::Obb> {
        self.cars
            .iter()
            .filter(|c| (c.vehicle.position - near).length() < radius)
            .flat_map(|c| {
                let bb = c
                    .vehicle
                    .ty
                    .def
                    .bounding_box
                    .unwrap_or([2.0, 4.5, 1.6, 0.0, 0.0, 0.8]);
                // moving, and with a mass of its own: a car that runs into the bus is no
                // bulldozer
                let h = c.vehicle.heading.to_radians();
                let v = glam::DVec2::new(h.sin(), h.cos()) * c.state.speed as f64;
                let (mass, id) = (c.vehicle.physics.mass_kg, c.id.get());
                let rear = c.vehicle.trailers.iter().filter_map(move |t| {
                    t.ty.def.bounding_box.map(|bb| {
                        ::simulation::collision::Obb::from_box(bb, t.position, t.heading)
                            .moving(v, mass, id)
                    })
                });
                std::iter::once(
                    ::simulation::collision::Obb::from_box(bb, c.vehicle.position, c.vehicle.heading)
                        .moving(v, mass, id),
                )
                    .chain(rear)
            })
            .collect()
    }


    /// Position and heading of a car by id (None once it is gone).
    pub fn car_pose(&self, id: VehicleId) -> Option<(DVec3, f64)> {
        self.cars
            .iter()
            .find(|c| c.id == id)
            .map(|c| (c.vehicle.position, c.vehicle.heading))
    }


    /// State of light `li` of controller `c` now and the seconds it keeps showing it (the
    /// pedestrians only start across on a green that lasts).
    pub fn light_state(&self, c: usize, li: usize) -> Option<(i32, f32)> {
        let ctl = self.lights.get(c)?;
        Some((ctl.state(li), ctl.remaining(li)))
    }


    /// Seconds until light `li` of controller `c` lets traffic go (0 while it does).
    pub fn light_until_go(&self, c: usize, li: usize) -> Option<f32> {
        self.lights.get(c)?.time_until_go(li)
    }


    /// How far the frame being drawn is between the last step and the next (the steps'
    /// accumulator over the step, 0..1). Left at 1, `sync` draws the simulated poses.
    pub fn set_tick_fraction(&mut self, alpha: f32) {
        self.drawn_alpha = alpha.clamp(0.0, 1.0);
    }

    /// Draw every vehicle `drawn_alpha` of the way from its pose before the last step to its
    /// pose now, keeping the simulated poses in `sim_poses` for `draw_simulated`. The steps
    /// come at 50 Hz: drawn where the simulation has them, the vehicles stood still on one
    /// frame in six at 60 fps, and on two in three at 144 fps.
    fn draw_between_steps(&mut self) {
        self.sim_poses.clear();
        if self.drawn_alpha >= 1.0 || self.mirror {
            return;
        }
        let alpha = self.drawn_alpha;
        for c in &mut self.cars {
            if c.drawn_from.len() != 1 + c.vehicle.trailers.len() {
                continue;
            }
            let start = self.sim_poses.len();
            push_part_poses(&c.vehicle, &mut self.sim_poses);
            let drawn = c.drawn_from.iter().zip(&self.sim_poses[start..])
                .map(|(&from, &to)| pose_between(from, to, alpha));
            set_part_poses(&mut c.vehicle, drawn);
        }
    }

    /// Put the vehicles back where the simulation has them after `draw_between_steps`
    /// (the cars are the same and in the same order in between).
    fn draw_simulated(&mut self) {
        if self.sim_poses.is_empty() {
            return;
        }
        let mut poses = self.sim_poses.iter().copied();
        for c in &mut self.cars {
            if c.drawn_from.len() != 1 + c.vehicle.trailers.len() {
                continue;
            }
            let n = c.drawn_from.len();
            set_part_poses(&mut c.vehicle, poses.by_ref().take(n));
        }
        self.sim_poses.clear();
    }

    /// Run `f` on the traffic with the vehicles where `sync` draws them, for what else is
    /// drawn on them after `sync` (their lights' coronas and spots).
    pub(crate) fn with_drawn_poses<R>(&mut self, f: impl FnOnce(&Self) -> R) -> R {
        self.draw_between_steps();
        let r = f(self);
        self.draw_simulated();
        r
    }

    /// Remember every vehicle's pose before a step moves it (see `draw_between_steps`).
    pub(crate) fn keep_poses_before_step(&mut self) {
        for c in &mut self.cars {
            c.drawn_from.clear();
            push_part_poses(&c.vehicle, &mut c.drawn_from);
        }
    }

    /// How long the frame being drawn is (s), for the drivers' animations.
    pub fn set_frame_dt(&mut self, dt: f32) {
        self.frame_dt = dt;
    }

    /// The time the drivers' animations move on by this `sync` (a short step before the
    /// first frame has been timed).
    fn driver_dt(&self) -> f32 {
        if self.frame_dt > 0.0 { self.frame_dt } else { 1.0 / 120.0 }
    }


    /// The drivers of the timetable buses near the camera: made when a bus comes within
    /// `DRIVER_NEAR`, posed every sync, let go when it is twice that far or gone.
    pub(crate) fn sync_drivers(&mut self, world: &World, renderer: &Renderer, scene: &mut Scene) {
        let Some(eye) = self.viewer.map(|v| v.pos) else {
            return;
        };
        let dt = self.driver_dt();
        let mut keep: Vec<VehicleId> = Vec::new();
        for c in &self.cars {
            if !c.is_bus() || c.gone {
                continue;
            }
            let d = (c.vehicle.position - eye).length();
            if d > DRIVER_NEAR * 2.0 {
                continue;
            }
            keep.push(c.id);
            if !self.drivers.contains_key(&c.id) {
                if d > DRIVER_NEAR {
                    continue;
                }
                let figure = match self.driver_pool.pop() {
                    Some(mut f) => {
                        if f.attach(&c.vehicle) {
                            Some(f)
                        } else {
                            self.driver_pool.push(f);
                            None
                        }
                    }
                    None => {
                        crate::driver::DriverFigure::new(world, renderer, scene, &c.vehicle, c.id.get())
                    }
                };
                match figure {
                    Some(f) => {
                        self.drivers.insert(c.id, f);
                    }
                    None => continue,
                }
            }
            if let Some(f) = self.drivers.get_mut(&c.id) {
                f.update(renderer, scene, &c.vehicle, &c.render, dt, true, false);
            }
        }
        let gone: Vec<VehicleId> = self
            .drivers
            .keys()
            .copied()
            .filter(|id| !keep.contains(id))
            .collect();
        for id in gone {
            if let Some(mut f) = self.drivers.remove(&id) {
                f.hide(renderer, scene);
                self.driver_pool.push(f);
            }
        }
    }


    /// The `TrafficLightPhase` and `TrafficLightApproach` values of light `li` of
    /// controller `c` (for scenery scripts).
    pub fn light_vars(&self, c: usize, li: usize) -> (f32, f32) {
        self.lights
            .get(c)
            .map(|ctl| {
                (
                    ctl.state(li) as f32,
                    ctl.request.get(li).copied().unwrap_or(false) as i32 as f32,
                )
            })
            .unwrap_or((-1.0, 0.0))
    }


    pub fn sync(&mut self, world: &World, renderer: &Renderer, scene: &mut Scene) {
        // cars that have parked: the parked object stands in their place from now on
        let mut i = 0;
        while i < self.cars.len() {
            match self.cars[i].maneuver.park {
                Some(p) if p.done => {
                    let c = self.cars.swap_remove(i);
                    if world.return_parked(renderer, scene, p.key) {
                        self.refresh_parked_geometry(world);
                        if let Some(list) = self.parked.get_mut(&p.lane) {
                            list.push((p.s, p.lat));
                        } else {
                            self.parked.insert(p.lane, vec![(p.s, p.lat)]);
                        }
                    }
                    if ::legacy_config::env::var_os("OMSI_DEBUG_TRAFFIC").is_some() {
                        log::info!("car {} has parked (space {})", c.id, p.key);
                    }
                    self.orphan_sounds.extend(c.sounds);
                    self.released.push(c.render);
                    self.released.extend(c.trailer_renders);
                }
                _ => i += 1,
            }
        }
        for r in std::mem::take(&mut self.released) {
            world.release_vehicle(renderer, scene, r);
        }
        // (drawn between the last two steps from here on, simulated again at the end)
        self.draw_between_steps();
        self.sync_drivers(world, renderer, scene);
        // traffic light lamps: the lamp's script (or the stock rules) turns the state of its
        // light into the `[visible] red|yellow|green 1` meshes and the coronas, and moves
        // what it animates (a barrier arm); it runs on the time since the last sync (an
        // offscreen run syncs only for its pictures)
        let dt = std::mem::take(&mut self.lamp_dt);
        let near = self.viewer.map(|v| v.pos);
        let debug_lamps = ::legacy_config::env::var_os("OMSI_DEBUG_LAMPS").is_some();
        for lamp in world.light_objects.lock().iter_mut() {
            if debug_lamps && lamp.animated {
                log::info!(
                    "moving lamp at ({:.1}, {:.1}), {:.0} m from the viewer",
                    lamp.pos.x,
                    lamp.pos.y,
                    near.map(|p| (lamp.pos - p).length()).unwrap_or(0.0)
                );
            }
            if let Some(p) = near {
                if (lamp.pos - p).length() > 1200.0 {
                    continue;
                }
            }
            let (state, request) = match self
                .controller_of_object
                .get(&lamp.parent)
                .and_then(|&c| self.lights.get(c))
            {
                Some(ctl) if lamp.any_light && ctl.lights.len() > 1 => {
                    // the most open of the crossing's lights (see `LightObject::any_light`)
                    let open = |s: i32| match s {
                        6..=8 => 3,
                        3..=5 => 2,
                        9..=11 => 1,
                        0..=2 => 0,
                        _ => -1,
                    };
                    let li = (0..ctl.lights.len())
                        .max_by_key(|&i| open(ctl.state(i)))
                        .unwrap_or(0);
                    (ctl.state(li), ctl.request.iter().any(|r| *r))
                }
                Some(ctl) => (
                    ctl.state(lamp.index),
                    ctl.request.get(lamp.index).copied().unwrap_or(false),
                ),
                // a lamp whose crossing has no program stays dark
                None => (-1, false),
            };
            let (r, y, g) = TrafficLightController::lamps(state);
            let value = |lamp: &crate::scene::LightObject, var: &str| -> f32 {
                // Custom signals can shift phases or blink the standard channels (Numazu
                // pedestrian lamps). Use their script outputs whenever they are available.
                let scripted = lamp.script.as_ref().and_then(|script| {
                    let s = script.lock();
                    // A failed/missing script can still have a varlist of zeroes. Keep
                    // stock fallback behaviour if it has no runnable frame block.
                    if s.program.frame.is_empty() {
                        None
                    } else {
                        s.var(var)
                    }
                });
                crate::scene::traffic_lamp_value(
                    var,
                    scripted,
                    crate::scene::standard_traffic_lamp(var, r, y, g, request),
                )
            };
            if let Some(script) = lamp.script.as_ref() {
                let vars = ::simulation::scenery::SceneryVars {
                    nightlight: self.night as i32 as f32,
                    in_use: 1.0,
                    traffic_light_phase: state as f32,
                    traffic_light_approach: request as i32 as f32,
                    switch: None,
                };
                let mut s = script.lock();
                s.update(dt, &vars);
                if near.is_none_or(|p| (lamp.pos - p).length() < 250.0) {
                    let at = lamp.pos;
                    crate::scene::draw_script_texts(
                        renderer,
                        scene,
                        &mut s,
                        &mut lamp.texts.lock(),
                        || format!("lamp_{:.0}_{:.0}", at.x, at.y),
                    );
                }
                // `OMSI_DEBUG_LAMPS`: where the moving lamps are (barriers) and how far their
                // meshes are turned, each time the lamps are updated
                if lamp.animated && debug_lamps {
                    let turn = s
                        .mesh_transforms
                        .iter()
                        .map(|m| {
                            let (_, r, _) = m.to_scale_rotation_translation();
                            r.to_axis_angle().1.to_degrees()
                        })
                        .fold(0.0f32, f32::max);
                    log::info!(
                        "lamp at ({:.1}, {:.1}): light state {:?} (crossing {:?}, light {}{}), meshes turned up to {turn:.0} deg",
                        lamp.pos.x,
                        lamp.pos.y,
                        vars.traffic_light_phase,
                        self.controller_of_object.get(&lamp.parent),
                        lamp.index,
                        if lamp.any_light { ", any" } else { "" }
                    );
                }
                if lamp.animated {
                    for (i, (inst, _)) in lamp.instances.iter().enumerate() {
                        if let Some(m) = s.mesh_transforms.get(i) {
                            renderer.set_transform(scene, *inst, lamp.pos, lamp.xf * *m);
                        }
                    }
                    // the lights go with their meshes (a barrier's lamps rise with its arm)
                    for (c, (mi, local, dir)) in lamp.coronas.iter_mut().zip(&lamp.corona_mesh) {
                        if let Some(m) = s.mesh_transforms.get(*mi) {
                            let xf = lamp.xf * *m;
                            c.0.position = lamp.pos + xf.transform_point3(*local).as_dvec3();
                            if *dir != glam::Vec3::ZERO {
                                c.0.direction = xf.transform_vector3(*dir).normalize_or_zero();
                            }
                        }
                    }
                }
            }
            if !lamp.animated {
                use std::hash::{Hash, Hasher};
                let mut h = std::collections::hash_map::DefaultHasher::new();
                (state, request).hash(&mut h);
                if let Some(script) = lamp.script.as_ref() {
                    for v in &script.lock().state.vars {
                        v.to_bits().hash(&mut h);
                    }
                }
                let sig = h.finish();
                if lamp.shown == Some(sig) {
                    continue;
                }
                lamp.shown = Some(sig);
            }
            // Traffic lamps do not enter World's ordinary scripted-object update path.
            // Switch their materials here too, so [matl_item] nightmaps light the LEDs.
            for (inst, slot, base, item, var, more) in &lamp.variants {
                renderer.set_material(
                    scene,
                    *inst,
                    *slot,
                    crate::scene::pick_variant(value(lamp, var), *base, *item, more),
                );
            }
            for k in 0..lamp.coronas.len() {
                let v = value(lamp, &lamp.coronas[k].1);
                lamp.lit[k] = v;
            }
            for (k, (inst, cond)) in lamp.instances.iter().enumerate() {
                let visible = match cond {
                    Some((var, want)) => (value(lamp, var) - want).abs() < 0.5,
                    None => true,
                };
                // lenses switched by their material instead (`[alphascale]` and
                // `[matl_lightmap]` on the lamp's variables, #826)
                match lamp.slots.get(k).filter(|s| !s.is_empty()) {
                    Some(slots) => {
                        let known = |v: &str| -> Option<f32> {
                            let scripted = lamp.script.as_ref().and_then(|script| {
                                let s = script.lock();
                                if s.program.frame.is_empty() {
                                    None
                                } else {
                                    s.var(v)
                                }
                            });
                            scripted
                                .or_else(|| v.trim().parse::<f32>().ok())
                                .or_else(|| {
                                    crate::scene::standard_traffic_lamp(v, r, y, g, request)
                                })
                        };
                        let (alpha, light) = slots.values(&known);
                        renderer.set_params(scene, *inst, &alpha, visible, &[]);
                        renderer.set_slot_light(scene, *inst, &light);
                    }
                    None => renderer.set_params(scene, *inst, &[], visible, &[]),
                }
            }
        }
        // A far car's script textures (its destination sign) stay as they are drawn: OMSI
        // shows them at any distance its model level has them. (They were stood in for by
        // their mean colour beyond 50 m, and every timetable bus coming up the street had
        // a blank sign until it was almost there.) What a far car's scripts redraw goes to
        // the GPU at most every half second, a slice of the cars per frame.
        let tick = (self.time as f64 * 2.0) as u64;
        let mut budget = SCRIPT_UPLOAD_BUDGET;
        // `Envir_Brightness`, which Omsi.exe sets for every road vehicle as for the
        // player's: the stock buses fade their windows by it at night (left at the engine's
        // default of 1, an AI bus under the street lamps kept its daytime brown glass)
        if let Some(d) = self.daylight {
            for c in self.cars.iter_mut().filter(|c| c.vehicle.ai_visuals) {
                let b = d.envir_brightness(world.light_map_light_at(c.vehicle.position));
                c.vehicle.set_var("Envir_Brightness", b);
            }
        }
        for c in &mut self.cars {
            // out of sight (`tick` decided): hidden once, then left alone until it comes
            // into view again - its many per-mesh updates were a third of this stage
            if !c.vehicle.ai_visuals {
                if !c.render.hidden {
                    c.render.hidden = true;
                    for inst in c
                        .render
                        .instances
                        .iter()
                        .chain(c.trailer_renders.iter().flat_map(|r| r.instances.iter()))
                    {
                        renderer.set_params(scene, *inst, &[], false, &[]);
                    }
                }
                continue;
            }
            c.render.hidden = false;
            if let Some(cam) = self.camera {
                let far = (c.vehicle.position - cam).length() > crate::scene::DISPLAYS_FAR;
                let due = c.render.display_tick != tick;
                c.render.displays_far = far && !due;
                if far && due {
                    c.render.display_tick = tick;
                }
            }
            crate::scene::sync_vehicle_textures(
                renderer,
                scene,
                &mut c.vehicle,
                &c.render,
                &mut budget,
            );
            crate::scene::sync_vehicle_materials(renderer, scene, &c.vehicle, &mut c.render);
            // a coupled part runs no scripts of its own: its plates, its displays and its
            // switched materials follow the leading vehicle's, as the player's own rear
            // sections do (without this an AI bus's rear section kept the blank textures and
            // the unswitched materials it was built with)
            {
                let mut trailers = std::mem::take(&mut c.vehicle.trailers);
                for (t, r) in trailers.iter_mut().zip(c.trailer_renders.iter_mut()) {
                    crate::scene::sync_vehicle_part(renderer, scene, &c.vehicle, t, r);
                }
                c.vehicle.trailers = trailers;
            }
            // an articulated AI bus (timetable or random traffic) bends its bellows like the
            // player's while it is near enough for the fold to show; farther out its shape
            // just stays as it was, which nobody can tell from still following the road
            if !c.render.skinned.is_empty()
                || c.trailer_renders.iter().any(|r| !r.skinned.is_empty())
            {
                let near = self
                    .camera
                    .map(|cam| (c.vehicle.position - cam).length() < SKIN_DISTANCE)
                    .unwrap_or(true);
                if near {
                    crate::scene::sync_skinned(
                        renderer,
                        scene,
                        &mut c.vehicle,
                        &mut c.render,
                        &mut c.trailer_renders,
                    );
                }
            }
            for (i, inst) in c.render.instances.iter().enumerate() {
                renderer.set_transform(
                    scene,
                    *inst,
                    c.vehicle.position,
                    c.vehicle.mesh_local_transform(i),
                );
                let p = &c.vehicle.mesh_props[i];
                let def = &c.vehicle.ty.model.meshes[c.vehicle.ty.meshes[i].def_index];
                let vp = def.viewpoint;
                let vp_ok = vp == 0 || vp & 4 != 0;
                // AI vehicles do not run every cockpit/material script that the player
                // vehicle runs.  Some models consequently leave an `[alphascale]`
                // variable at zero; applying it to an opaque body makes the traffic bus
                // translucent and reveals the interior through its panels.  Opaque slots
                // are never allowed to be faded by a dynamic alpha value; windows and
                // explicitly alpha-tested/blended slots retain their authored behavior.
                let mut alpha = p.slot_alpha.clone();
                for (slot, mat) in scene.instances[*inst].materials.iter().enumerate() {
                    if scene
                        .materials
                        .get(*mat)
                        .is_some_and(|m| m.alpha == ::render::AlphaMode::Opaque)
                    {
                        if let Some(a) = alpha.get_mut(slot) {
                            *a = 1.0;
                        }
                    }
                }
                let shown = p.visible && vp_ok && !c.vehicle.part_hidden(0);
                renderer.set_params(scene, *inst, &alpha, shown, &p.slot_uv);
                renderer.set_slot_light(scene, *inst, &p.slot_light);
                renderer.set_slot_night(scene, *inst, &p.slot_night);
                renderer.set_interior(scene, *inst, p.interior);
                renderer.set_cabin(scene, *inst, !def.illumination_interior.is_empty());
            }
            for (k, (t, r)) in c.vehicle.trailers.iter().zip(&c.trailer_renders).enumerate() {
                for (i, inst) in r.instances.iter().enumerate() {
                    renderer.set_transform(scene, *inst, t.position, t.mesh_local_transform(i));
                    let p = &t.mesh_props[i];
                    let def = &t.ty.model.meshes[t.ty.meshes[i].def_index];
                    let vp = def.viewpoint;
                    let vp_ok = vp == 0 || vp & 4 != 0;
                    let mut alpha = p.slot_alpha.clone();
                    for (slot, mat) in scene.instances[*inst].materials.iter().enumerate() {
                        if scene
                            .materials
                            .get(*mat)
                            .is_some_and(|m| m.alpha == ::render::AlphaMode::Opaque)
                        {
                            if let Some(a) = alpha.get_mut(slot) {
                                *a = 1.0;
                            }
                        }
                    }
                    let shown = p.visible && vp_ok && !c.vehicle.part_hidden(k + 1);
                    renderer.set_params(scene, *inst, &alpha, shown, &p.slot_uv);
                    renderer.set_slot_light(scene, *inst, &p.slot_light);
                    renderer.set_slot_night(scene, *inst, &p.slot_night);
                    renderer.set_interior(scene, *inst, p.interior);
                    renderer.set_cabin(scene, *inst, !def.illumination_interior.is_empty());
                }
            }
        }
        self.draw_simulated();
    }


    /// The light programs of the crossings within `radius` of `near` (host): (crossing
    /// object, position in the cycle, clock held at a stop point).
    pub fn light_states(&self, near: DVec3, radius: f64) -> Vec<(i64, f64, bool)> {
        let mut ctls: Vec<usize> = self
            .net
            .lanes
            .iter()
            .filter(|l| {
                l.traffic_light.is_some()
                    && l.points
                    .first()
                    .map(|p| (*p - near).truncate().length() < radius)
                    .unwrap_or(false)
            })
            .filter_map(|l| l.traffic_light.map(|t| t.0))
            .collect();
        ctls.sort_unstable();
        ctls.dedup();
        self.controller_of_object
            .iter()
            .filter(|(_, c)| ctls.binary_search(c).is_ok())
            .filter_map(|(obj, c)| {
                let ctl = self.lights.get(*c)?;
                Some((*obj, ctl.time, ctl.held))
            })
            .collect()
    }


    /// Where the host's light program of crossing `object` stands (client).
    pub fn set_light_state(&mut self, object: i64, time: f64, held: bool) {
        if let Some(ctl) = self
            .controller_of_object
            .get(&object)
            .and_then(|c| self.lights.get_mut(*c))
        {
            ctl.time = time;
            ctl.held = held;
        }
    }

}

/// The poses of a vehicle and its coupled parts, in order.
fn push_part_poses(v: &VehicleInstance, out: &mut Vec<PartPose>) {
    out.push((v.position, v.heading, v.pitch, v.bank));
    out.extend(v.trailers.iter().map(|t| t.drawn_pose()));
}

/// Draw a vehicle and its coupled parts at `poses`, in the order of `push_part_poses`.
fn set_part_poses(v: &mut VehicleInstance, mut poses: impl Iterator<Item = PartPose>) {
    if let Some((position, heading, pitch, bank)) = poses.next() {
        v.position = position;
        v.heading = heading;
        v.pitch = pitch;
        v.bank = bank;
    }
    for (t, pose) in v.trailers.iter_mut().zip(poses) {
        t.set_drawn_pose(pose);
    }
}

/// Further than this (m) between two steps a part was put somewhere else, not driven
/// there: it is drawn where it is now rather than sliding across.
const DRAWN_JUMP: f64 = 5.0;

/// The pose `alpha` (0..1) of the way from `from` to `to`, the heading the short way round.
fn pose_between(from: PartPose, to: PartPose, alpha: f32) -> PartPose {
    if (to.0 - from.0).length() > DRAWN_JUMP {
        return to;
    }
    let a = alpha as f64;
    let turn = (to.1 - from.1 + 180.0).rem_euclid(360.0) - 180.0;
    (
        from.0.lerp(to.0, a),
        (from.1 + turn * a).rem_euclid(360.0),
        from.2 + (to.2 - from.2) * alpha,
        from.3 + (to.3 - from.3) * alpha,
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use ::traffic::SIM_DT;

    /// The traffic steps at 50 Hz whatever the frame rate; the drivers are drawn every
    /// frame and must move on by the frame's time. On the last step's 20 ms they moved
    /// nearly three times too fast at 144 fps (and on frames without a step as well).
    #[test]
    fn the_drivers_move_on_by_the_frame_not_by_the_last_step() {
        let mut t = Traffic::for_tests(Network::default());
        let mut accum = 0.0f32;
        for k in 0..400 {
            let frame = if k < 200 { 1.0 / 144.0 } else { 1.0 / 240.0 };
            let (steps, step) = ::traffic::advance_fixed_clock(&mut accum, frame, SIM_DT, 2);
            for _ in 0..steps {
                t.tick(step, None);
            }
            t.set_frame_dt(frame);
            assert!((t.driver_dt() - frame).abs() < 1e-7, "drivers moved {} s", t.driver_dt());
        }
    }

    #[test]
    fn a_pose_between_steps_goes_the_short_way_round() {
        let from = (DVec3::new(0.0, 0.0, 1.0), 350.0, 1.0, -2.0);
        let to = (DVec3::new(0.2, 0.4, 1.0), 10.0, 3.0, 2.0);
        assert_eq!(pose_between(from, to, 0.0), from);
        let half = pose_between(from, to, 0.5);
        assert!((half.0 - DVec3::new(0.1, 0.2, 1.0)).length() < 1e-12);
        assert!(half.1.min(360.0 - half.1) < 1e-9, "heading {}", half.1);
        assert_eq!((half.2, half.3), (2.0, 0.0));
        let end = pose_between(from, to, 1.0);
        assert!((end.0 - to.0).length() < 1e-12 && (end.1 - to.1).abs() < 1e-9);
        // put somewhere else between the steps: no slide across
        let far = (DVec3::new(30.0, 0.0, 1.0), 90.0, 0.0, 0.0);
        assert_eq!(pose_between(from, far, 0.3), far);
    }

    #[test]
    fn a_vehicle_drawn_between_steps_gets_its_simulated_pose_back_exactly() {
        let ty = std::sync::Arc::new(::simulation::VehicleType::of_definition(Default::default()));
        let host = ::simulation::VehicleHost::new(::simulation::SimClock::default());
        let mut v = VehicleInstance::new(ty, host);
        (v.position, v.heading, v.pitch, v.bank) = (DVec3::new(10.0, 20.0, 3.0), 359.5, 0.5, 0.25);
        let mut before = Vec::new();
        push_part_poses(&v, &mut before);
        (v.position, v.heading, v.pitch, v.bank) = (DVec3::new(10.1, 20.3, 3.01), 0.7, 0.4, 0.3);
        let mut simulated = Vec::new();
        push_part_poses(&v, &mut simulated);
        set_part_poses(&mut v, before.iter().zip(&simulated).map(|(&a, &b)| pose_between(a, b, 0.25)));
        assert!((v.position - DVec3::new(10.025, 20.075, 3.0025)).length() < 1e-9);
        assert!((v.heading - 359.8).abs() < 1e-9, "heading {}", v.heading);
        set_part_poses(&mut v, simulated.iter().copied());
        assert_eq!((v.position, v.heading, v.pitch, v.bank), simulated[0], "the simulation's pose is untouched");
    }

    /// A car at a steady 50 km/h, stepped at 50 Hz and drawn at 60 and 144 fps: drawn at its
    /// simulated pose, it stood still on every frame without a step; drawn between its last
    /// two poses, it moves on by the frame's time on every frame.
    #[test]
    fn drawn_between_steps_a_steady_car_moves_on_every_frame() {
        let v = 50.0 / 3.6;
        for fps in [60.0f32, 144.0] {
            let frame = 1.0 / fps;
            let (mut accum, mut t) = (0.0f32, 0.0f64);
            let (mut from, mut to) = (0.0f64, 0.0f64);
            let mut last: Option<f64> = None;
            let (mut least, mut most) = (f64::MAX, 0.0f64);
            for k in 0..(fps as usize * 4) {
                let (steps, step) = ::traffic::advance_fixed_clock(&mut accum, frame, SIM_DT, 2);
                for _ in 0..steps {
                    t += step as f64;
                    from = to;
                    to = v * t;
                }
                let alpha = (accum / SIM_DT).clamp(0.0, 1.0) as f64;
                let drawn = from + (to - from) * alpha;
                // (from the second step on: before it there is no pose to come from)
                if let Some(l) = last.filter(|_| k > 4) {
                    least = least.min(drawn - l);
                    most = most.max(drawn - l);
                }
                last = Some(drawn);
            }
            let even = v * frame as f64;
            assert!(least > 0.9 * even && most < 1.1 * even,
                "{fps} fps: moved {least:.4}..{most:.4} m a frame, {even:.4} m expected");
        }
    }
}
