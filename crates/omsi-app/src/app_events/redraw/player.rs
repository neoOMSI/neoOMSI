//! Controllers, mouse steering and the player's vehicle in a frame.

use super::*;

impl App {
    /// Game controllers, mouse steering and the player's vehicle with its frame; the free camera and placed vehicles on foot; radio.
    pub(super) fn redraw_player(&mut self, f: &Frame) {
        let Frame { dt, .. } = *f;
        // The player's vehicle moves before the passengers are placed: they sit in
        // the bus frame, and placing them on the pose of the frame before made everyone
        // aboard tremble at speed (a quarter of a metre behind the seat, every frame).
        let __t = Instant::now();
        self.drag_frame();
        if self.world.is_some() {
            if let Some(n) = self.args.tutorial.take() {
                self.tutorial =
                    crate::tutorial::Tutorial::load(&self.args.root, n, &self.settings.language);
            }
        }
        let hwnd = self
            .window
            .as_deref()
            .and_then(crate::controllers::window_handle);
        let ctl = self
            .controllers
            .get_or_insert_with(|| crate::controllers::Controllers::new(&self.args.root, hwnd));
        ctl.set_focus(self.window_focused);
        ctl.deadzone = self.settings.ctrl_deadzone;
        ctl.pedal_throttle = self.settings.pedal_throttle;
        ctl.pedal_brake = self.settings.pedal_brake;
        ctl.ff_invert = self.settings.ff_invert;
        ctl.ff_enabled = self.settings.ff_enabled;
        ctl.steer_gain = if self.settings.wheel_lock >= 45.0 {
            (self.settings.wheel_range / self.settings.wheel_lock).clamp(0.1, 20.0)
        } else {
            1.0
        };
        if ctl.disabled.is_empty() && !self.settings.ctrl_off.is_empty() {
            ctl.disabled = self
                .settings
                .ctrl_off
                .split('|')
                .map(|s| s.trim().to_string())
                .filter(|s| !s.is_empty())
                .collect();
        }
        let analog = ctl.poll();
        let actions = std::mem::take(&mut ctl.actions);
        let moved = match (analog.steering, self.last_ctl_steer) {
            (Some(x), Some(x0)) => (x - x0).abs() > 0.02,
            _ => false,
        };
        if analog.steering.is_some() && (moved || self.last_ctl_steer.is_none()) {
            self.last_ctl_steer = analog.steering;
        }
        #[cfg(windows)]
        let vr_on = self.vr.is_some();
        #[cfg(not(windows))]
        let vr_on = false;
        let needs_mouse = self.mouse_drive
            || self.game_menu.is_some()
            || self.chooser.is_some()
            || self.list_kind.is_some()
            || self.navigator.as_ref().is_some_and(|n| n.map_open())
            || !matches!(self.view.as_str(), "driver" | "outside" | "pax");
        let hide = (moved || actions.iter().any(|a| a.1)) && !needs_mouse && !vr_on;
        if self.vr_nav_edit.is_none()
            && hide != self.cursor_hidden.is_some()
            && (hide || needs_mouse)
        {
            if let Some(win) = self.window.as_ref() {
                win.set_cursor_visible(!hide);
                self.cursor_hidden = hide.then_some(self.cursor);
            }
        }
        if let Some(n) = ctl.notice.take() {
            self.service_msg = Some((n, 8.0));
        }
        let driving = self
            .player
            .as_ref()
            .filter(|_| matches!(self.view.as_str(), "driver" | "outside" | "pax") && !self.paused);
        let kmh = driving
            .map(|p| p.vehicle.physics.velocity_kmh())
            .unwrap_or(0.0);
        let wheel_bump = ctl.wheel_bump(driving.and_then(|p| p.vehicle.rigid.as_ref()), kmh, dt);
        ctl.feedback(crate::controllers::FfInput {
            on: driving.is_some(),
            kmh,
            lateral_accel: driving
                .and_then(|p| p.vehicle.rigid.as_ref())
                .map(|r| r.accel_body.x)
                .unwrap_or(0.0),
            wheel_bump,
            wheel_bump_age: 0.0,
            vib_amp: driving
                .and_then(|p| p.vehicle.var("FF_Vib_Amp"))
                .unwrap_or(0.0),
            vib_period: driving
                .and_then(|p| p.vehicle.var("FF_Vib_Period"))
                .unwrap_or(0.0),
            dt,
        });
        // Steering as Omsi.exe has it (0x6f4284..0x6f447b): the whole width of the
        // window is the full lock from left to right, divided by the speed in tens
        // of km/h above 10 km/h - at 50 km/h the same hand movement turns the wheel a
        // fifth as far, which is what makes the wheel feel heavier the faster the bus
        // goes. For a second after mouse steering is switched on the wheel eases
        // towards the cursor (a half-life of the time that is left), then follows it.
        let mut analog = analog;
        if analog.look != [0.0, 0.0]
            && self.game_menu.is_none()
            && self.chooser.is_none()
            && !self.paused
        {
            let k = LOOK_STICK_DEG_S * dt * self.settings.look_sens;
            self.look_by(analog.look[0] * k, analog.look[1] * k);
        }
        if analog.stick {
            if let (Some(x), Some(p)) = (analog.steering, self.player.as_ref()) {
                let target = crate::controllers::gamepad_steering(
                    x,
                    p.vehicle.physics.velocity_kmh() as f32,
                );
                let now = p.vehicle.physics.controls.steering;
                let step = dt / 1.2;
                analog.steering = Some(now + (target - now).clamp(-step, step));
            }
        }
        let bus_view = self.mouse_steers_in_view();
        if let (true, Some(s)) = (
            self.mouse_drive && bus_view && !self.mouse_look && self.game_menu.is_none(),
            self.surface.as_ref(),
        ) {
            let (w, h) = (s.config.width as f32, s.config.height as f32);
            if std::mem::take(&mut self.center_cursor) {
                self.cursor = (w * 0.5, h * 0.5);
                if let Some(win) = self.window.as_ref() {
                    let _ = win.set_cursor_position(winit::dpi::PhysicalPosition::new(
                        (w * 0.5) as f64,
                        (h * 0.5) as f64,
                    ));
                }
            }
            // (the speed the divisor takes, smoothed over 0.4 s: the bus's own speed
            // trembles by fractions of a km/h from frame to frame on its springs and
            // tyres, and at 30 km/h the wheel twitched with it by itself)
            let raw_kmh = self
                .player
                .as_ref()
                .map(|p| p.vehicle.physics.velocity_kmh())
                .unwrap_or(0.0);
            let k_v = 1.0 - (-dt / 0.4).exp();
            self.mouse_kmh += (raw_kmh - self.mouse_kmh) * k_v;
            let kmh = self.mouse_kmh;
            let base = (crate::player::mouse_steering(self.cursor.0, w, kmh)
                * self.settings.mouse_sens)
                .clamp(-1.0, 1.0);
            self.mouse_edge = self
                .mouse_edge
                .clamp((-1.0 - base).min(0.0), (1.0 - base).max(0.0));
            let target = (base + self.mouse_edge).clamp(-1.0, 1.0);
            let y = (2.0 * self.cursor.1 / h.max(1.0) - 1.0).clamp(-1.0, 1.0);
            let (pedal_t, pedal_b) = ((-y).max(0.0), y.max(0.0));
            let (steer, fade) = &mut self.mouse_steer;
            // (after the first second the wheel follows the cursor within ~60 ms: the
            // cursor comes in bursts, and taken as it came the wheel moved in steps)
            let k = if *fade > 0.0 {
                (-std::f32::consts::LN_2 / *fade * dt).exp()
            } else {
                (-dt / 0.06).exp()
            };
            *steer = target + (*steer - target) * k;
            let (mt, mb) = &mut self.mouse_pedals;
            *mt = crate::player::mouse_pedal(*mt, pedal_t, k);
            *mb = crate::player::mouse_pedal(*mb, pedal_b, k);
            *fade = (*fade - dt).max(0.0);
            analog.steering = Some(*steer);
            if let Some(path) = omsi_cfg::env::var_os("OMSI_TRACE_STEER") {
                use std::io::Write;
                static TRACE: std::sync::Mutex<Option<std::fs::File>> = std::sync::Mutex::new(None);
                let mut g = TRACE.lock().unwrap_or_else(|e| e.into_inner());
                if g.is_none() {
                    *g = std::fs::File::create(&path).ok();
                    if let Some(f) = g.as_mut() {
                        let _ = writeln!(f, "t,dt,cursor_x,kmh,target,steer,steer_deg");
                    }
                }
                let deg = self
                    .player
                    .as_ref()
                    .map(|p| p.vehicle.physics.steer_deg)
                    .unwrap_or(0.0);
                if let Some(f) = g.as_mut() {
                    let _ = writeln!(
                        f,
                        "{:.3},{:.4},{:.1},{:.2},{:.4},{:.4},{:.3}",
                        self.clock.run_time,
                        dt,
                        self.cursor.0,
                        kmh,
                        target,
                        self.mouse_steer.0,
                        deg
                    );
                }
            }
            // the mouse owns the wheel (OMSI sets the curvature from it every frame):
            // a steering key's leftover turn must not take over whenever the cursor
            // passes the middle - the wheel jumped there; and the pedals, which Omsi.exe
            // writes from the cursor every frame: a brake the keys held stayed on (#395)
            if let Some(p) = self.player.as_mut() {
                p.axes.steering = 0.0;
                p.axes.brake = 0.0;
                p.axes.throttle = 0.0;
            }
            analog.throttle = Some(self.mouse_pedals.0);
            analog.brake = Some(self.mouse_pedals.1);
        } else if self.mouse_drive && bus_view && self.mouse_look && self.game_menu.is_none() {
            analog.steering = Some(self.mouse_steer.0);
            analog.throttle = Some(self.mouse_pedals.0);
            analog.brake = Some(self.mouse_pedals.1);
            if let Some(p) = self.player.as_mut() {
                p.axes.steering = 0.0;
            }
        }
        let mut actions = actions;
        if self.game_menu.is_none() {
            let mut game: Vec<String> = Vec::new();
            actions.retain(|(name, down)| {
                let n = name.to_ascii_lowercase();
                if let Some(k) = [
                    "view_look_left",
                    "view_look_right",
                    "view_look_up",
                    "view_look_down",
                ]
                .iter()
                .position(|x| *x == n)
                {
                    self.pad_look[k] = *down;
                    return false;
                }
                if n == "gear_up" || n == "gear_down" {
                    if *down {
                        game.push(n);
                    }
                    return false;
                }
                if crate::input_script::is_game_action(&n) {
                    if *down {
                        game.push(n);
                    }
                    return false;
                }
                true
            });
            for n in game {
                match n.as_str() {
                    "gear_up" => {
                        self.shift_gear(true);
                    }
                    "gear_down" => {
                        self.shift_gear(false);
                    }
                    _ => {
                        self.game_action(&n);
                    }
                }
            }
        }
        if let Some(p) = self.player.as_mut() {
            p.axes.linear = self.settings.steering_linear;
            p.axes.old_steering = self.settings.old_steering;
            p.axes.red_steer_spd = self.settings.red_steer_spd;
            p.axes.pedal_hold = self.settings.brake_hold;
            p.analog = analog;
            if self.game_menu.is_none() {
                for (name, down) in actions {
                    p.action(&name, down);
                }
            }
        }
        self.touch_frame(dt);
        if let (Some(p), Some(r), Some(scene)) = (
            self.player.as_mut(),
            self.renderer.as_ref(),
            self.scene.as_mut(),
        ) {
            // (while the tile under the bus is being read again - the weather turned
            // to snow and every tile came back with the winter textures - there is
            // no ground under it: it is held where it is rather than falling through
            // the world and being put back somewhere in the sky)
            let ground_here = self.world.as_ref().is_none_or(|w| {
                let at = p.vehicle.position;
                let k = (
                    (at.x / omsi_map::tile_size()).floor() as i32,
                    (at.y / omsi_map::tile_size()).floor() as i32,
                );
                w.terrains.read().contains_key(&k) || w.surfaces.read().contains_key(&k)
            });
            if !self.paused && ground_here {
                p.tick(
                    dt,
                    self.audio.as_ref(),
                    self.in_cab,
                    !matches!(self.view.as_str(), "free" | "foot"),
                );
                // (not in the headset: the player's own head moves there, and a head
                // thrown about by the bus on top of it made the whole cab sway and
                // shift before the eyes)
                #[cfg(windows)]
                let vr_on = self.vr.is_some();
                #[cfg(not(windows))]
                let vr_on = false;
                p.move_head(dt, self.settings.head_movement && !vr_on);
                if let Some(w) = self.world.as_ref() {
                    crate::rail_drive::frame(p, self.traffic.as_ref().map(|t| &t.net), w, dt);
                }
            }
            if let Some(t) = p.vehicle.host.time_written.take() {
                self.pending_time = Some(t);
            }
            let mut placed_boxes = Vec::new();
            for q in self.placed.iter_mut() {
                if !self.paused {
                    q.vehicle.update(dt);
                }
                q.sync_transforms(r, scene, false);
                let f = crate::lan::footprint_of(&q.vehicle, [2.5, 11.5, 3.0, 0.0, 0.0, 1.5]);
                placed_boxes.push(omsi_sim::collision::Obb {
                    center: glam::DVec2::new(f.x, f.y),
                    half: glam::DVec2::new(f.width as f64 * 0.5, f.length as f64 * 0.5),
                    heading: (f.heading as f64).to_radians(),
                    z0: f.z,
                    z1: f.z + 3.0,
                    velocity: glam::DVec2::ZERO,
                    mass: 12_000.0,
                    pole: None,
                    id: -1,
                });
            }
            if !self.placed.is_empty() {
                if self.traffic.is_none() {
                    p.vehicle.dynamic_boxes.clear();
                }
                p.vehicle.dynamic_boxes.extend(placed_boxes);
            }
            if let Some(w) = self.world.as_ref() {
                lay_down_poles(w, r, scene, &mut p.vehicle);
            }
            let inside = self.in_cab;
            p.sync_transforms(r, scene, inside);
            p.sync_driver_hands(
                r,
                scene,
                dt,
                self.settings.driver && self.on_foot.is_none(),
                self.view == "driver",
                self.settings.hands_in_cab,
            );
            if self.view != "free" && self.view != "foot" {
                let key = crate::input_script::look_key_of(&self.view, Some(p.cam_choice));
                crate::input_script::swap_view_look(
                    &mut self.look,
                    &mut self.view_looks,
                    &mut self.look_view,
                    &key,
                );
                if let Some(cam) = self.camera.as_ref() {
                    p.seat = glam::Vec3::from_array(self.settings.seat);
                    if self.settings.head_tracking
                        && self.headtrack.is_none()
                        && self
                            .headtrack_failed
                            .is_none_or(|t| t.elapsed().as_secs_f32() > 5.0)
                    {
                        self.headtrack =
                            crate::headtrack::HeadTracker::start(self.settings.head_tracking_port);
                        self.headtrack_failed =
                            self.headtrack.is_none().then(std::time::Instant::now);
                    }
                    let tracked = self.headtrack.as_ref().and_then(|h| h.pose()).filter(|_| {
                        self.settings.head_tracking
                            && matches!(self.view.as_str(), "driver" | "pax")
                    });
                    #[cfg(windows)]
                    let vr_on = self.vr.is_some();
                    #[cfg(not(windows))]
                    let vr_on = false;
                    p.steer_look = if vr_on || tracked.is_some() {
                        0.0
                    } else {
                        crate::player::steering_view_yaw(
                            p.steer_look,
                            p.vehicle.physics.controls.steering,
                            dt,
                            self.settings.steer_look && self.view == "driver",
                            self.settings.steer_look_angle,
                            self.settings.steer_look_response,
                        )
                    };
                    if let Some(t) = tracked {
                        p.seat += glam::Vec3::new(t.pos[0], -t.pos[2], t.pos[1])
                            .clamp(glam::Vec3::splat(-60.0), glam::Vec3::splat(60.0))
                            / 100.0;
                    }
                    // (the outside view's field of view starts from the plain 60
                    // degrees every frame: taken from the last frame's camera, the
                    // zoom was applied on top of itself and ran off to its narrowest
                    // or widest at once)
                    let prev_cam = *cam;
                    let base = omsi_render::Camera {
                        fov_deg: 60.0,
                        ..*cam
                    };
                    let tracked_rot = tracked.map(|mut t| {
                        for (k, axis) in ["yaw", "pitch", "roll"].iter().enumerate() {
                            if self.settings.head_tracking_invert.contains(axis) {
                                t.rot[k] = -t.rot[k];
                            }
                        }
                        t.rot
                    });
                    let fov_setting = self.settings.fov;
                    let zoom = self.view_zoom.get(&self.view).copied();
                    let finish = move |c: &mut omsi_render::Camera| {
                        if let Some(r) = tracked_rot {
                            c.yaw += r[0].clamp(-170.0, 170.0);
                            c.pitch = (c.pitch + r[1].clamp(-80.0, 80.0)).clamp(-89.0, 89.0);
                            c.roll += r[2].clamp(-60.0, 60.0);
                        }
                        if fov_setting >= 20.0 {
                            c.fov_deg = fov_setting.min(120.0);
                        }
                        if let Some(z) = zoom {
                            c.fov_deg = (c.fov_deg * z).clamp(8.0, 120.0);
                        }
                    };
                    let mut cam = p.camera_look(&self.view, &base, self.look, self.orbit);
                    finish(&mut cam);
                    {
                        let inside_view = self.view == "driver";
                        let entering = std::mem::take(&mut self.cam_blend.entering);
                        let resetting = std::mem::take(&mut self.cam_blend.resetting);
                        let reset_zoom = std::mem::take(&mut self.cam_blend.reset_zoom);
                        let left = self
                            .cam_blend
                            .key
                            .as_ref()
                            .is_some_and(|k| k.0 == self.view && k.1.0 != p.cam_choice.0);
                        let target = if inside_view {
                            p.driver_local(self.look)
                        } else {
                            None
                        };
                        let mut started = false;
                        if let Some(to) = target.as_ref() {
                            if (entering || left || resetting)
                                && crate::app::CAM_BLEND_SECS > 0.0
                                && self.settings.driverview_smooth
                            {
                                let from = if entering {
                                    // (what `driver_world` adds to every frame - the head and the seat - is
                                    // taken off the walker's eyes, and the zoom `finish` applies again off
                                    // its field of view: the first frame then is the walker's picture)
                                    let mut f = p.local_of_world(&prev_cam);
                                    f.pos[0] -= p.head.x + p.seat.x;
                                    f.pos[1] -= p.head.y + p.seat.y;
                                    f.pos[2] -= p.head.z + p.seat.z;
                                    if let Some(z) = zoom.filter(|z| *z > 0.0) {
                                        f.fov /= z;
                                    }
                                    Some(f)
                                } else {
                                    let mut from = self.cam_blend.shown.clone();
                                    if resetting {
                                        if let (Some(from), Some(zoom)) = (&mut from, reset_zoom) {
                                            from.fov *= zoom;
                                        }
                                    }
                                    from
                                };
                                if let Some(from) = from {
                                    let d = glam::Vec3::from_array(from.pos)
                                        - glam::Vec3::from_array(to.pos);
                                    if d.length() < 25.0 {
                                        self.cam_blend.from = Some(from);
                                        self.cam_blend.t = 0.0;
                                        started = true;
                                    }
                                }
                            }
                        }
                        self.cam_blend.key = Some((self.view.clone(), p.cam_choice));
                        let mut shown = target.clone();
                        let from_now = self.cam_blend.from.clone();
                        match (target.as_ref(), from_now.as_ref()) {
                            (Some(to), Some(from)) => {
                                if !started {
                                    self.cam_blend.t += dt.min(crate::app::CAM_BLEND_MAX_DT)
                                        / crate::app::CAM_BLEND_SECS;
                                }
                                if self.cam_blend.t >= 1.0 {
                                    // (the hand-over to the plain camera: the glide ends exactly on it (k = 1),
                                    // so the curve's tail is not left over to twitch; only what the two ways
                                    // of making the camera might still differ in is eased out)
                                    let mut last =
                                        p.driver_world(&crate::app::blend_local(from, to, 1.0));
                                    finish(&mut last);
                                    self.cam_blend.carry =
                                        Some(crate::app::CamCarry::between(&last, &cam));
                                    self.cam_blend.from = None;
                                } else {
                                    let mixed = crate::app::blend_local(
                                        from,
                                        to,
                                        self.cam_blend.progress(),
                                    );
                                    cam = p.driver_world(&mixed);
                                    finish(&mut cam);
                                    shown = Some(mixed);
                                }
                            }
                            _ => self.cam_blend.from = None,
                        }
                        self.cam_blend.shown = shown;
                        if started || !inside_view {
                            self.cam_blend.carry = None;
                        }
                        if let Some(c) = self.cam_blend.carry.as_mut() {
                            c.apply(&mut cam);
                            if !c.decay(dt) {
                                self.cam_blend.carry = None;
                            }
                        }
                    }
                    if self.view == "outside" && self.settings.camera_collision {
                        if let Some(w) = self.world.as_ref() {
                            cam = p.camera_clipped(cam, w, self.orbit, dt);
                        }
                    } else {
                        p.arm.reset();
                    }
                    self.camera = Some(cam);
                }
            } else if let Some(cam) = self.camera.as_mut() {
                let base = if self.settings.fov >= 20.0 {
                    self.settings.fov.min(120.0)
                } else {
                    60.0
                };
                cam.fov_deg = (base * self.view_zoom.get(&self.view).copied().unwrap_or(1.0))
                    .clamp(8.0, 120.0);
            }
            let __th = Instant::now();
            // (the cursor's aim into the cab: again when the cursor or the view
            // turned, else every few frames for switches that moved under it - a ray
            // through every cockpit mesh every frame was a tenth of the frame)
            let key = self.camera.as_ref().map(|c| {
                (
                    self.cursor.0.round() as i32,
                    self.cursor.1.round() as i32,
                    (c.yaw * 4.0).round() as i32,
                    (c.pitch * 4.0).round() as i32,
                )
            });
            // (the cab sways with the suspension: a view that only turned waits a few frames)
            let cursor_moved = key.map(|k| (k.0, k.1)) != self.hover_key.map(|k| (k.0, k.1));
            if cursor_moved
                || (key != self.hover_key && self.total_frames % 6 == 0)
                || self.total_frames % 12 == 0
            {
                self.hover_key = key;
                self.update_hover();
            }
            *self.profile.entry("player.hover").or_default() += __th.elapsed().as_secs_f64();
            if let Some(a) = self.audio.as_ref() {
                a.follow_device();
            }
            if let (Some(a), Some(cam)) = (self.audio.as_ref(), self.camera.as_ref()) {
                let (reverb_time, reverb_mix) = self
                    .world
                    .as_ref()
                    .map(|w| w.reverb_at(cam.position))
                    .unwrap_or((0.0, 0.0));
                a.set_listener(omsi_audio::Listener {
                    position: cam.position.as_vec3(),
                    forward: cam.forward(),
                    right: cam.right(),
                    master: if self.paused {
                        0.0
                    } else {
                        self.settings.volume.clamp(0.0, 1.0)
                    },
                    reverb_time,
                    reverb_mix,
                });
            }
        }
        if self.player.is_none() && matches!(self.view.as_str(), "free" | "foot") {
            if let Some(cam) = self.camera.as_mut() {
                let base = if self.settings.fov >= 20.0 {
                    self.settings.fov.min(120.0)
                } else {
                    60.0
                };
                cam.fov_deg = (base * self.view_zoom.get(&self.view).copied().unwrap_or(1.0))
                    .clamp(8.0, 120.0);
            }
        }
        if let (None, Some(r), Some(scene)) = (
            self.player.as_ref(),
            self.renderer.as_ref(),
            self.scene.as_mut(),
        ) {
            for q in self.placed.iter_mut() {
                if !self.paused {
                    q.vehicle.update(dt);
                }
                q.sync_transforms(r, scene, false);
            }
        }
        if let Some(a) = self.audio.as_ref() {
            match self.player.as_ref() {
                Some(p) => {
                    let inside = self.in_cab;
                    if let Some(m) = self.radio.update(a, &p.vehicle, inside) {
                        self.service_msg = Some((m, 6.0));
                    }
                }
                None => self.radio.stop(a),
            }
        }
        *self.profile.entry("player").or_default() += __t.elapsed().as_secs_f64();
    }
}
