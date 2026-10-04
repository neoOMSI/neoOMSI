//! Weather, daylight and lights of a frame.

use super::*;

impl App {
    /// METAR, the time of day, lamps, light maps, rain and the scripted objects. Gives the day's light for the picture.
    pub(super) fn redraw_environment(&mut self, f: &Frame) -> omsi_sim::Daylight {
        let Frame { dt, .. } = *f;
        self.tick_metar(dt);
        if !self.paused {
            let speed = self.time_speed();
            self.clock.advance(dt * speed as f32);
            self.sync_real_time();
            if let Some(t) = self.traffic.as_mut() {
                t.time_scale = speed;
            }
            self.tick_weather(dt * speed as f32);
        } else if self.weather_blend.is_some() {
            self.tick_weather(dt);
        }
        let daylight = omsi_sim::Daylight::compute(&self.clock, self.envir.as_ref());
        if self.lamps_on != Some(daylight.lamps_on) {
            self.lamps_on = Some(daylight.lamps_on);
            if let (Some(w), Some(r), Some(scene)) = (
                self.world.as_ref(),
                self.renderer.as_ref(),
                self.scene.as_mut(),
            ) {
                w.set_lamps(r, scene, daylight.lamps_on);
            }
        }
        if self.total_frames % 60 == 0 {
            if let (Some(w), Some(r), Some(scene)) = (
                self.world.as_ref(),
                self.renderer.as_ref(),
                self.scene.as_mut(),
            ) {
                w.update_night_modes(r, scene, &self.clock, daylight.brightness);
            }
            self.follow_date();
        }
        if let Some(p) = self.player.as_mut() {
            let lm = self
                .world
                .as_ref()
                .and_then(|w| w.light_map_light_at(p.vehicle.position));
            p.vehicle
                .set_var("Envir_Brightness", daylight.envir_brightness(lm));
            p.vehicle.host.sun_alt = daylight.altitude_deg;
            if let Some(w) = &self.weather {
                apply_weather(&mut p.vehicle, w, self.wetness);
            }
        }
        for q in self.placed.iter_mut() {
            let lm = self
                .world
                .as_ref()
                .and_then(|w| w.light_map_light_at(q.vehicle.position));
            q.vehicle
                .set_var("Envir_Brightness", daylight.envir_brightness(lm));
            q.vehicle.host.sun_alt = daylight.altitude_deg;
            if let Some(w) = &self.weather {
                apply_weather(&mut q.vehicle, w, self.wetness);
            }
        }
        let __t = Instant::now();
        if let Some(wt) = &self.weather {
            lights::set_cone_strength(wt.fog.0, precip_of(wt).1, daylight.night);
        }
        if let Some(r) = self.renderer.as_mut() {
            lights::upload_corona_textures(r);
        }
        let __ta = Instant::now();
        if let (Some(w), Some(r), Some(cam)) = (
            self.world.as_ref(),
            self.renderer.as_ref(),
            self.camera.as_ref(),
        ) {
            w.update_light_map_atlas(r, cam.position);
        }
        *self.profile.entry("lights.atlas").or_default() += __ta.elapsed().as_secs_f64();
        if let (Some(w), Some(scene), Some(cam)) = (
            self.world.as_ref(),
            self.scene.as_mut(),
            self.camera.as_ref(),
        ) {
            let mut vehicles: Vec<&omsi_sim::VehicleInstance> = Vec::new();
            if let Some(p) = self.player.as_ref() {
                vehicles.push(&p.vehicle);
            }
            vehicles.extend(self.placed.iter().map(|q| &q.vehicle));
            if let Some(t) = self.traffic.as_ref() {
                vehicles.extend(t.cars.iter().map(|c| &c.vehicle));
            }
            vehicles.extend(self.remotes.remotes.values().map(|r| r.vehicle()));
            let __tc = Instant::now();
            lights::collect(w, scene, &daylight, cam.position, &vehicles);
            *self.profile.entry("lights.collect").or_default() += __tc.elapsed().as_secs_f64();
            if let Some(id) = self.editor.as_ref().and_then(|e| e.selected) {
                let at = w.edit_objects.lock().get(&id).map(|o| o.pos);
                let moved = w
                    .object_edits
                    .lock()
                    .get(&id)
                    .map(|e| e.moved)
                    .unwrap_or_default();
                if let Some(p) = at {
                    scene.coronas.push(omsi_render::Corona {
                        position: p + moved + DVec3::Z * 3.0,
                        size: 0.6,
                        color: [1.0, 0.1, 0.9],
                        brightness: 2.0,
                        ..Default::default()
                    });
                }
            }
            if let Some(wt) = &self.weather {
                let (kind, rate) = precip_of(wt);
                self.rain.set(kind, rate);
                let wind = crate::rain::weather_wind(wt);
                // every bus one may ride in keeps the weather out: the own, another
                // player's, a timetable bus - each part of it: an articulated bus's
                // rear section is a coupled part with its own [boundingbox] (#777)
                let boxed = rain::vehicle_boxes;
                let mut buses: Vec<(DVec3, f64, [f32; 6])> = self
                    .player
                    .as_ref()
                    .map(|p| boxed(&p.vehicle))
                    .unwrap_or_default();
                buses.extend(
                    self.remotes
                        .remotes
                        .values()
                        .flat_map(|rv| boxed(rv.vehicle())),
                );
                if let Some(t) = self.traffic.as_ref() {
                    buses.extend(
                        t.cars
                            .iter()
                            .filter(|c| {
                                c.is_bus() && (c.vehicle.position - cam.position).length() < 40.0
                            })
                            .flat_map(|c| boxed(&c.vehicle)),
                    );
                }
                let __tr = Instant::now();
                self.rain.tick(
                    if self.paused { 0.0 } else { dt },
                    cam.position,
                    wind,
                    scene,
                    &buses,
                );
                *self.profile.entry("lights.rain").or_default() += __tr.elapsed().as_secs_f64();
                if kind == 1 {
                    if let Some(p) = self.player.as_ref() {
                        let wheels = puddles::wheel_contacts(&p.vehicle);
                        let speed = p.vehicle.physics.velocity_kmh().abs() / 3.6;
                        let wetness = self.wetness;
                        scene
                            .smoke
                            .extend(self.splashes.update(dt, &wheels, speed, &|x, y| {
                                puddles::puddle_coverage(x, y, w.wet_road_at(x, y, wetness))
                            }));
                    }
                }
                // Passenger dialogue is not ambience: it must remain audible when the
                // ambience subsystem is unavailable or has been disabled.
                let voices = self
                    .humans
                    .as_mut()
                    .map(|h| h.take_voice_lines())
                    .unwrap_or_default();
                if let Some(a) = self.audio.as_ref() {
                    for line in voices {
                        if let Some(clip) = a.load_clip(&line.path) {
                            a.play(
                                clip,
                                omsi_audio::mixer::VoiceParams {
                                    gain: 1.0,
                                    pitch: 1.0,
                                    looping: false,
                                    position: Some(line.position.as_vec3()),
                                    doppler: true,
                                    // A passenger at the rear of a single-decker bus should
                                    // still be clearly heard from the driver's seat.
                                    range: 8.0,
                                    lowpass_hz: 0.0,
                                    important: false,
                                },
                            );
                        }
                    }
                }
                if let (Some(amb), Some(a)) = (self.ambience.as_mut(), self.audio.as_ref()) {
                    let steps = self
                        .humans
                        .as_mut()
                        .map(|h| h.take_footfalls())
                        .unwrap_or_default();
                    let inside = self.in_cab;
                    let __tm = Instant::now();
                    amb.update(
                        a,
                        dt,
                        (kind, rate),
                        inside,
                        street_condition(wt, self.wetness),
                        cam.position,
                        &steps,
                    );
                    *self.profile.entry("lights.ambience").or_default() +=
                        __tm.elapsed().as_secs_f64();
                    if let Some(every) = debug_sound_every() {
                        static LAST: std::sync::atomic::AtomicU32 =
                            std::sync::atomic::AtomicU32::new(u32::MAX);
                        let bucket = (self.clock.time / every as f64) as u32;
                        if LAST.swap(bucket, std::sync::atomic::Ordering::Relaxed) != bucket {
                            log::info!(
                                "sound: environment - {} (precip {kind} {rate:.2}, StreetCond {:.2}, {} voices)",
                                amb.last,
                                street_condition(wt, self.wetness),
                                a.voice_count()
                            );
                        }
                    }
                }
            }
        }
        *self.profile.entry("lights+rain").or_default() += __t.elapsed().as_secs_f64();
        let __t = Instant::now();
        if let (Some(w), Some(r), Some(scene), Some(cam)) = (
            self.world.as_ref(),
            self.renderer.as_ref(),
            self.scene.as_mut(),
            self.camera.as_ref(),
        ) {
            let traffic = self.traffic.as_ref();
            let phase =
                |c: usize, li: usize| traffic.map(|t| t.light_vars(c, li)).unwrap_or((-1.0, 0.0));
            let __tb = Instant::now();
            if let Some(p) = self.player.as_mut() {
                w.sync_html_departures(&mut p.vehicle.host);
            }
            match self.schedule.as_mut() {
                Some(s) => s.update_boards(
                    w,
                    traffic,
                    self.duty.as_ref(),
                    self.player
                        .as_ref()
                        .and_then(|p| p.vehicle.host.hof.as_deref()),
                    &self.clock,
                ),
                None => w.timetable_boards.lock().clock = Some(self.clock.clone()),
            }
            *self.profile.entry("scripted.boards").or_default() += __tb.elapsed().as_secs_f64();
            w.update_scripted(
                r,
                scene,
                dt,
                cam.position,
                daylight.brightness,
                &phase,
                self.audio.as_ref(),
                self.in_cab,
            );
        }
        *self.profile.entry("scripted").or_default() += __t.elapsed().as_secs_f64();
        daylight
    }
}
