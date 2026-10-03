//! The camera and views of a frame.

use super::*;

impl App {
    /// The view and its look, the camera of every view, zoom.
    pub(super) fn redraw_camera(&mut self, f: &Frame) {
        let Frame { dt, .. } = *f;
        self.sync_view_look();
        if self.player.is_some() && self.view != "free" {
            // looking around with the keyboard: Alt + I/J/K/L (the plain letters
            // belong to the bus - L is the headlights in Inputs/keyboard.cfg)
            let step = 60.0 * dt;
            let ctrl_alt = (self.keys.contains(&KeyCode::ControlLeft)
                || self.keys.contains(&KeyCode::ControlRight))
                && (self.keys.contains(&KeyCode::AltLeft)
                    || self.keys.contains(&KeyCode::AltRight));
            let arrows = [
                KeyCode::ArrowLeft,
                KeyCode::ArrowRight,
                KeyCode::ArrowUp,
                KeyCode::ArrowDown,
            ]
            .map(|k| self.keys.contains(&k));
            if let (true, Some(p), Some(cam)) = (
                ctrl_alt && self.view == "driver" && arrows.iter().any(|a| *a),
                self.player.as_mut(),
                self.camera.as_ref(),
            ) {
                let cams = &p.vehicle.ty.def.cameras_reflexion;
                let f = cam.forward();
                let best = (0..cams.len())
                    .map(|i| {
                        (
                            i,
                            (p.vehicle.camera_world_full(&cams[i]).0 - cam.position)
                                .as_vec3()
                                .normalize_or_zero()
                                .dot(f),
                        )
                    })
                    .max_by(|a, b| a.1.total_cmp(&b.1))
                    .map(|(i, _)| i);
                if let Some(i) = best {
                    if p.mirror_offsets.len() <= i {
                        p.mirror_offsets.resize(cams.len(), [0.0; 2]);
                    }
                    let o = &mut p.mirror_offsets[i];
                    let rate = 12.0 * dt;
                    o[0] = (o[0] + rate * (arrows[1] as i32 - arrows[0] as i32) as f32)
                        .clamp(-45.0, 45.0);
                    o[1] = (o[1] + rate * (arrows[2] as i32 - arrows[3] as i32) as f32)
                        .clamp(-30.0, 30.0);
                    p.mirrors_dirty = true;
                    self.service_msg = Some((
                        format!(
                            "Mirror {}: {:+.1}° across, {:+.1}° up (Ctrl+Alt+arrows)",
                            i + 1,
                            o[0],
                            o[1]
                        ),
                        2.0,
                    ));
                }
            } else if let Some(p) = self.player.as_mut().filter(|p| p.mirrors_dirty) {
                p.mirrors_dirty = false;
                settings::save_mirror_offsets(&p.vehicle.ty.def.path, &p.mirror_offsets);
            }
            self.look.0 += step * 1.5 * (self.pad_look[1] as i32 - self.pad_look[0] as i32) as f32;
            self.look.1 = (self.look.1
                + step * 0.7 * (self.pad_look[2] as i32 - self.pad_look[3] as i32) as f32)
                .clamp(-85.0, 85.0);
            let alt =
                self.keys.contains(&KeyCode::AltLeft) || self.keys.contains(&KeyCode::AltRight);
            if alt && self.keys.contains(&KeyCode::KeyJ) {
                self.look.0 -= step;
            }
            if alt && self.keys.contains(&KeyCode::KeyL) {
                self.look.0 += step;
            }
            if alt && self.keys.contains(&KeyCode::KeyI) {
                self.look.1 = (self.look.1 + step * 0.7).min(85.0);
            }
            if alt && self.keys.contains(&KeyCode::KeyK) {
                self.look.1 = (self.look.1 - step * 0.7).max(-85.0);
            }
            if self.view != "outside" {
                self.look.0 = self.look.0.clamp(-140.0, 140.0);
            }
            {
                let ctrl = self.keys.contains(&KeyCode::ControlLeft)
                    || self.keys.contains(&KeyCode::ControlRight);
                let shift = self.keys.contains(&KeyCode::ShiftLeft)
                    || self.keys.contains(&KeyCode::ShiftRight);
                let dir = match (
                    self.keys.contains(&KeyCode::PageUp),
                    self.keys.contains(&KeyCode::PageDown),
                ) {
                    (true, false) => 1.0,
                    (false, true) => -1.0,
                    _ => 0.0,
                };
                let client = self
                    .lan
                    .as_ref()
                    .is_some_and(|l| l.role == omsi_net::Role::Client);
                if ctrl && shift && dir != 0.0 && !client {
                    if self.clock_hold == 0.0 && self.real_time_locked() {
                        self.shift_clock(dir);
                    }
                    self.clock_hold += dt;
                    if !self.real_time_locked() {
                        let rate = 900.0 * (1.0 + self.clock_hold * 1.5).min(8.0);
                        self.shift_clock(dir * rate as f64 * dt as f64);
                    }
                } else {
                    self.clock_hold = 0.0;
                }
            }
            let zoom_in = self.keys.contains(&KeyCode::Equal) && !self.own_keys.contains(&13);
            let zoom_out = self.keys.contains(&KeyCode::Minus) && !self.own_keys.contains(&12);
            if matches!(self.view.as_str(), "driver" | "pax") {
                if zoom_in {
                    self.zoom_by(3.0 * dt);
                }
                if zoom_out {
                    self.zoom_by(-3.0 * dt);
                }
            }
            if self.view == "outside" {
                if zoom_in || self.keys.contains(&KeyCode::NumpadAdd) {
                    self.orbit = (self.orbit - 12.0 * dt).max(ORBIT_MIN);
                }
                if zoom_out || self.keys.contains(&KeyCode::NumpadSubtract) {
                    self.orbit = (self.orbit + 12.0 * dt).min(ORBIT_MAX);
                }
            }
            // Home held recentres the view - unless keyboard.cfg gives it a job (the
            // stock file makes it the ticket desk camera, which this then turned
            // straight ahead again whenever it was switched to, #733)
            if self.keys.contains(&KeyCode::Home)
                && !self
                    .game_keys
                    .iter()
                    .any(|b| Some(b.scan_code) == keys::dik_code(KeyCode::Home))
            {
                self.look = (0.0, 0.0);
                self.orbit = ORBIT_DEFAULT;
                self.view_zoom.remove(&self.view);
            }
        }
        if self.view != "free" {
            self.ego = false;
        }
        // (the free camera flies; with no bus it is the view too - but not out of the
        // walker's eyes: on foot without a bus of one's own (started on foot, the bus
        // removed) the keys flew the camera on from where the walk had put it every
        // frame, and walking jumped about, the more so the lower the frame rate, #807)
        if let (Some(cam), true) = (
            self.camera.as_mut(),
            self.view == "free" || (self.player.is_none() && self.on_foot.is_none()),
        ) {
            let mut v = Vec3::ZERO;
            let f = cam.forward();
            let r = cam.right();
            if self.keys.contains(&KeyCode::KeyW) {
                v += f;
            }
            if self.keys.contains(&KeyCode::KeyS) {
                v -= f;
            }
            if self.keys.contains(&KeyCode::KeyD) {
                v += r;
            }
            if self.keys.contains(&KeyCode::KeyA) {
                v -= r;
            }
            if self.keys.contains(&KeyCode::KeyE) || self.keys.contains(&KeyCode::Space) {
                v += Vec3::Z;
            }
            if self.keys.contains(&KeyCode::KeyQ) {
                v -= Vec3::Z;
            }
            let boost = if self.keys.contains(&KeyCode::ShiftLeft) {
                5.0
            } else {
                1.0
            };
            if self.ego {
                let flat = Vec3::new(v.x, v.y, 0.0).normalize_or_zero();
                let pace = if boost > 1.0 { 4.5 } else { 1.4 };
                cam.position += (flat * pace * dt).as_dvec3();
                if let Some(g) = self
                    .world
                    .as_ref()
                    .and_then(|w| w.walk_height(cam.position.x, cam.position.y))
                {
                    cam.position.z = g + 1.7;
                }
            } else {
                cam.position += (v.normalize_or_zero() * self.speed * boost * dt).as_dvec3();
            }
            if self.keys.contains(&KeyCode::ArrowLeft) {
                cam.yaw -= 60.0 * dt;
            }
            if self.keys.contains(&KeyCode::ArrowRight) {
                cam.yaw += 60.0 * dt;
            }
            if self.keys.contains(&KeyCode::ArrowUp) {
                cam.pitch = (cam.pitch + 40.0 * dt).min(89.0);
            }
            if self.keys.contains(&KeyCode::ArrowDown) {
                cam.pitch = (cam.pitch - 40.0 * dt).max(-89.0);
            }
        }
    }
}
