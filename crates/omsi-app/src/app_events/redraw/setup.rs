//! Start of a frame: housekeeping and frame timing.

use super::*;

impl App {
    /// The start of a frame: loading screen preview, autosave, a lost or full graphics device. `false`: nothing more to do this frame.
    pub(super) fn redraw_begin(&mut self, event_loop: &ActiveEventLoop) -> bool {
        if omsi_cfg::env::var_os("OMSI_LOADING_SCREEN").is_some() {
            self.loading_preview();
            return false;
        }
        if self.vr_nav_edit.is_some() && (!self.vr_active() || self.view != "driver") {
            self.finish_vr_nav_edit();
        }
        if (!self.vr_active() || self.player.is_none())
            && matches!(self.list_kind, Some(game_lists::ListKind::Options(_)))
            && self
                .admin_list
                .as_ref()
                .is_some_and(|rows| rows.iter().any(|(_, action)| action.starts_with("vr_nav_")))
        {
            self.open_list(game_lists::ListKind::Options(0));
        }
        #[cfg(windows)]
        self.poll_vr_cursor_position();
        if !self.paused && self.player.is_some() && self.clock.run_time - self.autosave_t >= 300.0 {
            self.autosave_t = self.clock.run_time;
            self.save_last_situation();
        }
        if let Some(t) = self
            .pending_time
            .take()
            .filter(|_| !self.real_time_locked())
        {
            let d = (t - self.clock.time + 43_200.0).rem_euclid(86_400.0) - 43_200.0;
            self.shift_clock(d);
        }
        // the graphics device is gone (a driver reset, an external card unplugged):
        // nothing can be drawn again - end the session the ordinary way, so that the
        // summary, the personnel file and the LAN goodbye are not lost
        // the card ran out of memory: fewer textures (the finest levels of the far
        // ones go), before the driver gives the device up
        if self
            .renderer
            .as_ref()
            .is_some_and(|r| r.take_out_of_memory())
        {
            if let Some(w) = self.world.as_ref() {
                let now = w.texture_budget_bytes();
                let less = if now == 0 {
                    600_000_000
                } else {
                    (now * 3 / 5).max(300_000_000)
                };
                w.set_texture_budget(less);
                log::warn!(
                    "the graphics card ran out of memory: textures kept to {:.0} MB from now on",
                    less as f64 / 1e6
                );
            }
        }
        if let Some(why) = self.renderer.as_ref().and_then(|r| r.device_lost()) {
            if self.restart_after_device_loss() {
                log::warn!("device lost ({why}): the game goes on in a new start");
            } else {
                log::error!("ending the session: the graphics device was lost ({why})");
            }
            platform::exit(event_loop);
            return false;
        }
        let desktop_vsync = self.settings.vsync && !self.vr_active();
        if let (Some(surface), Some(renderer)) = (self.surface.as_mut(), self.renderer.as_ref()) {
            surface.set_vsync(renderer, desktop_vsync);
        }
        self.in_cab = matches!(self.view.as_str(), "driver" | "pax")
            || (self.view == "foot" && self.foot_bus() == Some(humans::BusId::Player));
        self.inside_remote = match self.foot_bus() {
            Some(humans::BusId::Ai(x)) if self.view == "foot" => humans::remote_bus_player(x),
            _ => None,
        };
        true
    }

    /// Frame time, the quality governor, the menu and its input script and the drive start. `None`: the frame ends here.
    pub(super) fn redraw_timing(&mut self, event_loop: &ActiveEventLoop) -> Option<Frame> {
        let now = Instant::now();
        let raw_dt = (now - self.last).as_secs_f32();
        self.log_frame(raw_dt);
        let profiling = omsi_cfg::env::var_os("OMSI_PROFILE").is_some();
        let waited: f64 = ["acquire", "present", "gpu"]
            .iter()
            .map(|&k| self.profile.get(k).copied().unwrap_or(0.0))
            .sum();
        let wait_this_frame = (waited - self.governor_wait_prev).max(0.0) as f32;
        self.governor_wait_prev = waited;
        if self.total_frames > 60 {
            if raw_dt > 0.05 {
                self.spikes += 1;
                if profiling {
                    let mut parts: Vec<(&'static str, f64)> = self
                        .profile
                        .iter()
                        .map(|(k, v)| (*k, v - self.profile_prev.get(k).copied().unwrap_or(0.0)))
                        .collect();
                    let staged: f64 = parts
                        .iter()
                        .filter(|(k, _)| !k.contains('.'))
                        .map(|p| p.1)
                        .sum();
                    parts.retain(|p| p.1 > 0.002);
                    parts.sort_by(|a, b| b.1.total_cmp(&a.1));
                    let list: Vec<String> = parts
                        .iter()
                        .map(|(k, v)| format!("{k} {:.0}", v * 1000.0))
                        .collect();
                    log::info!(
                        "stutter: frame {} took {:.0} ms ({}; outside the stages {:.0} ms)",
                        self.total_frames,
                        raw_dt * 1000.0,
                        list.join(", "),
                        (raw_dt as f64 - staged).max(0.0) * 1000.0
                    );
                }
            }
            self.worst_ms = self.worst_ms.max(raw_dt * 1000.0);
            // Reduce resolution only when slow frames spend substantial time waiting
            // for presentation or the GPU. Traffic, scripts and tile work can drop
            // the frame rate too, but fewer pixels cannot make those stages faster.
            // Keep the player's chosen scale and explicit fixed-scale override.
            // A fast V-synced frame can wait for the next refresh without being
            // GPU-bound. Count presentation wait only on slow frames.
            if raw_dt > 0.02 {
                self.governor.2 += wait_this_frame;
            }
            self.governor.0 += raw_dt;
            self.governor.1 += 1;
            if self.governor.0 >= 5.0 {
                let fps = self.governor.1 as f32 / self.governor.0;
                let wait_share = self.governor.2 / self.governor.0;
                self.governor = (0.0, 0, 0.0);
                let free = self.settings.render_scale <= 0.0
                    && (self.settings.max_fps == 0 || self.settings.max_fps >= 50)
                    && omsi_cfg::env::var_os("OMSI_FIXED_SCALE").is_none();
                if let (Some(r), true) = (self.renderer.as_mut(), free) {
                    let s = r.dynamic_scale();
                    let step = render_scale_step(fps, wait_share);
                    r.set_dynamic_scale(s + step);
                    if (r.dynamic_scale() - s).abs() > 1e-3 {
                        log::info!(
                            "frame rate {fps:.0} fps (presentation wait {:.0}%): the 3D picture is drawn at {:.0} % of the window now",
                            wait_share * 100.0,
                            r.dynamic_scale() * 100.0
                        );
                        self.governor_low = 0;
                    } else if step < 0.0 {
                        self.governor_low += 1;
                        if self.governor_low >= 2 && fps < 30.0 {
                            self.governor_low = 0;
                            let what = r.lighten().or_else(|| {
                                std::mem::replace(&mut self.settings.shadows, false)
                                    .then_some("shadows off")
                            });
                            if let Some(what) = what {
                                log::warn!(
                                    "frame rate {fps:.0} fps at the smallest render scale: {what} to keep up"
                                );
                                self.service_msg = Some((
                                    format!("The graphics card cannot keep up: {what}"),
                                    4.0,
                                ));
                            }
                        }
                    }
                }
            }
        }
        if profiling {
            self.profile_prev.clone_from(&self.profile);
        }
        let dt = raw_dt.min(0.1);
        self.last = now;
        if self.game_menu.is_some() {
            let kind = self.menu_cursor_kind();
            self.set_cursor_kind(kind);
        }
        self.run_input_script(event_loop);
        if let Some(m) = self.menu.as_ref() {
            if let Some(limit) = self.args.exit_after {
                if self.started.elapsed().as_secs_f32() > limit {
                    log::info!("menu: {} maps, {} vehicles", m.maps.len(), m.vehicles.len());
                    platform::exit(event_loop);
                }
            }
            let lines = m.lines();
            if let (Some(hud), Some(s), Some(r), Some(scene), Some(win)) = (
                self.hud.as_mut(),
                self.surface.as_ref(),
                self.renderer.as_mut(),
                self.scene.as_mut(),
                self.window.as_ref(),
            ) {
                hud.update(r, scene, &lines);
                if let wgpu::CurrentSurfaceTexture::Success(frame)
                | wgpu::CurrentSurfaceTexture::Suboptimal(frame) =
                    s.surface.get_current_texture()
                {
                    let view = frame.texture.create_view(&Default::default());
                    let cam = Camera {
                        position: DVec3::ZERO,
                        yaw: 0.0,
                        pitch: 0.0,
                        roll: 0.0,
                        fov_deg: 60.0,
                        near: 0.5,
                        far: 100.0,
                    };
                    let lighting = omsi_render::Lighting {
                        sky_color: Vec3::new(0.08, 0.10, 0.14),
                        ..Default::default()
                    };
                    r.render(
                        scene,
                        &view,
                        s.config.width,
                        s.config.height,
                        &cam,
                        &lighting,
                    );
                    win.pre_present_notify();
                    r.queue.present(frame);
                }
            }
            return None;
        }
        if !self.drive_start(event_loop) {
            // the session goes on while the map loads: a big map's first area took
            // longer than the host waits for a silent player
            if let Some(l) = self.lan.as_mut() {
                let planned = omsi_net::Pose {
                    bus: self.args.bus.clone().unwrap_or_default().replace('\\', "/"),
                    ..Default::default()
                };
                l.keepalive(dt, &planned);
            }
            return None;
        }
        Some(Frame { now, raw_dt, dt })
    }
}
