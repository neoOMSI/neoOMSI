//! Streaming and AI traffic of a frame.

use super::*;

/// Fixed simulation step for the traffic domain: decisions run at a stable rate,
/// independent of the render frame rate. Rendering may run at any rate on top.
pub(super) const SIM_DT: f32 = 0.02;
/// Most fixed steps to run in one frame; a long stall is bounded and the rest of the
/// debt is kept (capped) rather than silently lost or turned into one huge step.
pub(super) const MAX_SIM_STEPS: u32 = 8;

impl App {
    /// Tile streaming and the AI traffic.
    pub(super) fn redraw_traffic(&mut self, f: &Frame) {
        let audio_inside = self.audio_in_cab();
        let Frame { dt, .. } = *f;
        let __t = Instant::now();
        self.drive_streaming();
        *self.profile.entry("streaming").or_default() += __t.elapsed().as_secs_f64();
        let __t = Instant::now();
        if let (Some(t), Some(w), Some(r), Some(scene)) = (
            self.traffic.as_mut(),
            self.world.as_ref(),
            self.renderer.as_ref(),
            self.scene.as_mut(),
        ) {
            let center = self
                .player
                .as_ref()
                .map(|p| p.vehicle.position)
                .or(self.camera.as_ref().map(|c| c.position))
                .unwrap_or(DVec3::ZERO);
            let aspect = self
                .surface
                .as_ref()
                .map(|s| s.config.width as f64 / s.config.height.max(1) as f64)
                .unwrap_or(16.0 / 9.0);
            let fog = self
                .weather
                .as_ref()
                .map(|w| w.fog.0 as f64)
                .unwrap_or(50000.0);
            traffic_inputs(
                t,
                self.camera.as_ref(),
                aspect,
                fog,
                &self.clock,
                self.humans.as_ref(),
                self.player.as_ref(),
                &r.options,
            );
            self.populate_t -= dt;
            if self.populate_t <= 0.0 && !self.paused {
                self.populate_t = if self
                    .schedule
                    .as_ref()
                    .map(|s| s.pending() > 0)
                    .unwrap_or(false)
                {
                    0.1
                } else {
                    2.0
                };
                let __t5 = Instant::now();
                let view = self.camera.as_ref().map(|c| c.forward().as_dvec3());
                t.populate_seen(w, r, scene, center, view);
                *self.profile.entry("traffic.populate").or_default() +=
                    __t5.elapsed().as_secs_f64();
                let mut keep_clear = self
                    .player
                    .as_ref()
                    .map(|p| traffic::vehicle_bodies(&p.vehicle))
                    .unwrap_or_default();
                keep_clear.extend(
                    self.remotes
                        .remotes
                        .values()
                        .flat_map(|r| traffic::vehicle_bodies(r.vehicle())),
                );
                t.set_keep_clear(keep_clear);
                if let Some(s) = self.schedule.as_mut() {
                    let window = if self.first_populate {
                        20.0 * 60.0
                    } else {
                        2.5
                    };
                    let __t6 = Instant::now();
                    s.tick(w, t, r, scene, t.day_time(), window);
                    *self.profile.entry("traffic.schedule").or_default() +=
                        __t6.elapsed().as_secs_f64();
                }
                self.first_populate = false;
            }
            let gloomy = self
                .weather
                .as_ref()
                .map(|w| {
                    let (kind, rate) = precip_of(w);
                    w.fog.0 < 600.0
                        || (kind != 0 && rate > 0.05)
                        || w.clouds
                        .0
                        .trim()
                        .to_ascii_lowercase()
                        .starts_with("overcast")
                })
                .unwrap_or(false);
            // Omsi switches the AI's lights on below a light value of 0.75, before
            // the street lamps (0.6), and off after them in the morning
            let daylight = ::simulation::Daylight::compute(&self.clock, self.envir.as_ref());
            t.set_night(daylight.brightness < 0.75 || gloomy);
            t.set_daylight(daylight);
            let __t2 = Instant::now();
            let mut others = lan_outlines(&self.remotes);
            others.extend(own_outlines(self.player.as_ref(), &self.placed));
            t.set_external_actors(others);
            if !self.paused {
                t.set_player_priority(
                    self.player
                        .as_ref()
                        .and_then(|p| p.vehicle.var("TrafficPriority"))
                        .is_some_and(|v| v > 0.5),
                );
                self.sim_accum = (self.sim_accum + dt).min(SIM_DT * MAX_SIM_STEPS as f32);
                let mut steps = 0;
                while self.sim_accum >= SIM_DT && steps < MAX_SIM_STEPS {
                    t.tick(SIM_DT, self.player.as_ref().map(|p| player_outline(p)));
                    self.sim_accum -= SIM_DT;
                    steps += 1;
                }
                if let Some(w) = self.world.as_ref() {
                    w.set_switches(&t.switch_requests());
                    let rail = self
                        .player
                        .as_ref()
                        .and_then(|p| p.rail.as_ref())
                        .map(|r| (r.lane, r.along));
                    w.set_signals(&t.signal_aspects(&w.signal_routes, rail));
                }
            }
            *self.profile.entry("traffic.tick").or_default() += __t2.elapsed().as_secs_f64();
            for (k, v) in ["traffic.tick.lanes", "traffic.tick.plan", "traffic.tick.ai"]
                .into_iter()
                .zip(t.tick_split)
            {
                *self.profile.entry(k).or_default() += v;
            }
            if let Some(p) = self.player.as_mut() {
                p.vehicle.dynamic_boxes = if ::config::get_bool("gameplay", "collision_vehicles").unwrap_or(true) {
                    t.boxes(p.vehicle.position, 80.0)
                } else {
                    Vec::new()
                };
            }
            let __t3 = Instant::now();
            if let Some(a) = self.audio.as_ref() {
                let street = self
                    .weather
                    .as_ref()
                    .map(|w| street_condition(w, self.wetness))
                    .unwrap_or(0.0);
                let muffled = audio_inside;
                // heard round the camera (the ear), not round the player's bus: a
                // free camera following an AI bus lost its sound 250 m from the bus
                let ear = self.camera.as_ref().map(|c| c.position).unwrap_or(center);
                t.update_audio(a, ear, street, muffled);
            }
            *self.profile.entry("traffic.audio").or_default() += __t3.elapsed().as_secs_f64();
            let __t4 = Instant::now();
            t.set_camera(self.camera.as_ref().map(|c| c.position));
            t.sync(w, r, scene);
            *self.profile.entry("traffic.sync").or_default() += __t4.elapsed().as_secs_f64();
        }
        *self.profile.entry("traffic").or_default() += __t.elapsed().as_secs_f64();
    }
}
