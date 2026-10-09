//! Mouse and cursor: clicks, dragging, looking round, zoom, hover and the cursor shape.

use super::*;

pub(crate) const HTML_OBJECT_REACH: f32 = 4.0;

/// Degrees the view turns per (logical) pixel of the cursor's way while looking round:
/// Omsi.exe's fov / 78.75 (TForm_main.Panel1MouseMove 0x82c5f8).
pub(crate) fn look_deg_per_px(fov_deg: f32) -> f32 {
    fov_deg / 78.75
}

pub(crate) fn chase_orbit_step(yaw: f32, pitch: f32, dx_px: f32, dy_px: f32) -> (f32, f32) {
    const GAIN: f32 = 0.35;
    (
        (yaw + dx_px * GAIN).rem_euclid(360.0),
        (pitch - dy_px * GAIN).clamp(-60.0, 25.0),
    )
}

#[cfg(test)]
mod look_tests {
    #[test]
    fn a_route_number_takes_digits_and_letters() {
        use winit::keyboard::KeyCode;
        assert_eq!(crate::game_menu::route_char(KeyCode::Digit5), Some('5'));
        assert_eq!(crate::game_menu::route_char(KeyCode::Numpad0), Some('0'));
        assert_eq!(crate::game_menu::route_char(KeyCode::KeyE), Some('E'));
        assert_eq!(crate::game_menu::route_char(KeyCode::NumpadAdd), None);
        assert_eq!(crate::game_menu::route_char(KeyCode::Space), None);
    }

    #[test]
    fn a_cursor_way_of_78_75_px_turns_by_the_field_of_view() {
        assert!((78.75 * super::look_deg_per_px(60.0) - 60.0).abs() < 1e-4);
    }

    #[test]
    fn chase_orbits_at_035_deg_px_with_stops_above_and_below() {
        let (y, p) = super::chase_orbit_step(0.0, 0.0, 100.0, 100.0);
        assert!(
            (y - 35.0).abs() < 1e-4 && (p + 35.0).abs() < 1e-4,
            "{y} {p}"
        );
        assert!((super::chase_orbit_step(350.0, 0.0, 100.0, 0.0).0 - 25.0).abs() < 1e-3);
        assert_eq!(super::chase_orbit_step(0.0, 0.0, 0.0, -1000.0).1, 25.0);
        assert_eq!(super::chase_orbit_step(0.0, 0.0, 0.0, 1000.0).1, -60.0);
    }
}

pub(crate) fn look_key_of(view: &str, cam: Option<(usize, usize)>) -> String {
    match (view, cam) {
        ("driver", Some((d, _))) => format!("driver#{d}"),
        ("pax", Some((_, x))) => format!("pax#{x}"),
        _ => view.to_string(),
    }
}

pub(crate) fn swap_view_look(
    look: &mut (f32, f32),
    looks: &mut std::collections::HashMap<String, (f32, f32)>,
    look_view: &mut String,
    view: &str,
) {
    if look_view != view {
        let old = std::mem::replace(look_view, view.to_string());
        if !old.is_empty() {
            looks.insert(old, *look);
        }
        *look = looks.get(view).copied().unwrap_or((0.0, 0.0));
    }
}

pub(crate) fn part_in_reach(
    eye: glam::DVec3,
    at: glam::DVec3,
    heading: f64,
    bb: Option<[f32; 6]>,
) -> bool {
    let bb = bb.unwrap_or([2.5, 12.0, 3.0, 0.0, 0.0, 1.5]);
    let h = heading.to_radians();
    let (fwd, right) = (
        glam::DVec2::new(h.sin(), h.cos()),
        glam::DVec2::new(h.cos(), -h.sin()),
    );
    let centre = at + (right * bb[3] as f64 + fwd * bb[4] as f64).extend(bb[5] as f64);
    let reach = (bb[0].max(bb[1]) as f64) * 0.5 + 3.0;
    (eye - centre).length() < reach
}

#[cfg(test)]
mod reach_tests {
    use super::part_in_reach;
    use glam::DVec3;

    #[test]
    fn the_rear_section_is_reached_by_its_own_box() {
        let front = [2.5, 11.0, 3.0, 0.0, -3.0, 1.5];
        let rear = [2.5, 7.0, 3.0, 0.0, -3.5, 1.5];
        let eye = DVec3::new(2.0, -17.0, 1.7);
        assert!(!part_in_reach(eye, DVec3::ZERO, 0.0, Some(front)));
        assert!(part_in_reach(
            eye,
            DVec3::new(0.0, -12.0, 0.0),
            0.0,
            Some(rear)
        ));
        assert!(part_in_reach(
            DVec3::new(-2.0, 17.0, 1.7),
            DVec3::new(0.0, 12.0, 0.0),
            180.0,
            Some(rear)
        ));
    }
}

impl App {
    pub(crate) fn sync_view_look(&mut self) {
        let key = self.look_key();
        swap_view_look(
            &mut self.look,
            &mut self.view_looks,
            &mut self.look_view,
            &key,
        );
    }

    /// Which camera the look belongs to: the view, and for the driver's and the passengers'
    /// view the camera chosen in it. Each of Omsi.exe's cameras keeps where it was turned
    /// (a `TCamera` has its own yaw and pitch besides the file's, 0x7edde4 resets them): the
    /// look went back to straight ahead whenever the viewpoint changed.
    pub(crate) fn look_key(&self) -> String {
        look_key_of(&self.view, self.player.as_ref().map(|p| p.cam_choice))
    }

    pub(crate) fn zoom_by(&mut self, notches: f32) {
        let z = self.view_zoom.entry(self.view.clone()).or_insert(1.0);
        *z = (*z * (1.0 - 0.08 * notches.clamp(-5.0, 5.0))).clamp(0.2, 1.6);
    }

    pub(crate) fn look_by(&mut self, dx: f32, dy: f32) {
        self.sync_view_look();
        if self.view == "foot" {
            self.foot_look(dx, dy);
            return;
        }
        if self.view == "free" || self.player.is_none() {
            if let Some(cam) = self.camera.as_mut() {
                cam.yaw = (cam.yaw + dx).rem_euclid(360.0);
                cam.pitch = (cam.pitch - dy).clamp(-89.0, 89.0);
            }
        } else if self.view == "outside" {
            self.look.0 = (self.look.0 + dx).rem_euclid(360.0);
            self.look.1 = (self.look.1 - dy).clamp(-60.0, 25.0);
        } else {
            self.look.0 = (self.look.0 + dx).clamp(-140.0, 140.0);
            self.look.1 = (self.look.1 - dy).clamp(-85.0, 85.0);
        }
    }

    pub(crate) fn on_cursor(&mut self, x: f32, y: f32) {
        if self.move_cursor(x, y) {
            self.update_hover();
        }
    }

    pub(crate) fn start_both_drag(&mut self) -> bool {
        if self.game_menu.is_some()
            || self.player.is_none()
            || self.navigator.as_ref().is_some_and(|n| n.map_open())
        {
            return false;
        }
        let value = match self.view.as_str() {
            "outside" => self.orbit,
            "driver" | "pax" => *self.view_zoom.get(&self.view).unwrap_or(&1.0),
            _ => return false,
        };
        self.both_drag = Some((self.cursor.1, value));
        self.mouse_look = false;
        self.update_hover();
        true
    }

    pub(crate) fn mouse_steers_in_view(&self) -> bool {
        self.player.is_some()
            && (matches!(self.view.as_str(), "driver" | "outside" | "pax")
            || (self.view == "free" && !self.ego))
    }

    /// The raycast camera is steering: the mouse turns the view, the middle of the
    /// screen is the cursor.
    pub(crate) fn raycast_active(&self) -> bool {
        ::config::get_bool("camera", "free_look").unwrap_or(false)
            && !self.free_look
            && self.player.is_some()
            && matches!(self.view.as_str(), "driver" | "pax" | "outside" | "foot")
            && self.game_menu.is_none()
            && self.chooser.is_none()
            && self.list_kind.is_none()
            && self.menu.is_none()
            && !self.mouse_drive
            && self.screenshot_mode.is_none()
            && self.vr_nav_edit.is_none()
            && !self.touch.enabled
            && !self.vr_active()
            && self.window_focused
            && !self.navigator.as_ref().is_some_and(|n| n.map_open())
    }

    pub(crate) fn cursor_looks(&self) -> bool {
        self.mouse_look
            && self.game_menu.is_none()
            && self.player.is_some()
            && !matches!(self.view.as_str(), "foot" | "free")
    }

    /// The right button alone zooms, as in Omsi.exe (TForm_main.Panel1MouseMove 0x82c5f8:
    /// ssRight without `[altView]`, or Shift+right with it); otherwise it turns the view.
    pub(crate) fn right_zooms(&self) -> bool {
        !::config::get_bool("camera", "alt_view").unwrap_or(true)
            || self.keys.contains(&KeyCode::ShiftLeft)
            || self.keys.contains(&KeyCode::ShiftRight)
    }

    pub(crate) fn on_right(&mut self, pressed: bool) {
        self.buttons_held.1 = pressed;
        if pressed && self.buttons_held.0 && !self.dragging && self.start_both_drag() {
            return;
        }
        if pressed && self.dragging {
            return;
        }
        if !pressed {
            self.both_drag = None;
        }
        // a right click lets go of the mouse steering as in OMSI (#162) when the player
        // wants it so; otherwise the right button looks round and the wheel and pedals stay
        // where the mouse left them (it went off with every look round, and with every
        // look round in the pause)
        if pressed
            && self.mouse_drive
            && self.game_menu.is_none()
            && ::config::get_bool("controls", "mouse_right_off").unwrap_or(false)
            && !self.paused
        {
            self.set_mouse_drive(false);
            self.service_msg = Some(("Mouse steering off".into(), 3.0));
        }
        if pressed && self.right_zooms() && self.start_both_drag() {
            return;
        }
        if self.mouse_drive && self.game_menu.is_none() {
            if pressed {
                self.steer_cursor = Some(self.cursor);
            } else if let Some((x, y)) = self.steer_cursor.take() {
                self.cursor = (x, y);
                if let Some(win) = self.window.as_ref() {
                    let _ = win
                        .set_cursor_position(winit::dpi::PhysicalPosition::new(x as f64, y as f64));
                }
            }
        }
        self.mouse_look = pressed;
        self.update_hover();
    }

    pub(crate) fn on_mouse_moved(&mut self, x: f32, y: f32) {
        if self.move_cursor(x, y) {
            self.html_move();
        }
    }

    pub(crate) fn html_move(&mut self) {
        if let Some((id, page, ..)) = self.html_object_pressed {
            let Some((o, d, _)) = self.cursor_ray_now() else {
                return;
            };
            let Some(w) = self.world.clone() else { return };
            if let Some(h) = w
                .html_object_hit(o, d, HTML_OBJECT_REACH)
                .filter(|h| h.map_id == id && h.page == page)
            {
                w.html_object_pointer(id, page, h.u, h.v, ::simulation::htmltex::PointerKind::Move);
                self.html_object_pressed = Some((id, page, h.u, h.v));
            }
            return;
        }
        let Some((page, ..)) = self.html_pressed else {
            return;
        };
        let Some((o, d, _)) = self.cursor_ray_now() else {
            return;
        };
        let Some(p) = self.player.as_mut() else {
            return;
        };
        if let Some((pg, u, v)) = p.html_hit(o, d).filter(|h| h.0 == page) {
            p.html_pointer(pg, u, v, ::simulation::htmltex::PointerKind::Move);
            self.html_pressed = Some((pg, u, v));
        }
    }

    #[cfg(windows)]
    pub(crate) fn reset_vr_pointer(&mut self) {
        if let Some(vr) = self.vr.as_mut() {
            vr.recenter_pointer();
        }
        self.vr_cursor_physical = None;
        self.vr_cursor_warp_pending = None;
    }

    #[cfg(windows)]
    pub(crate) fn on_vr_cursor_moved(&mut self, x: f32, y: f32) {
        if let Some(target) = self.vr_cursor_warp_pending.take() {
            self.vr_cursor_physical = Some((x, y));
            if (x - target.0).abs() < 3.0 && (y - target.1).abs() < 3.0 {
                return;
            }
            return;
        }
        if let Some(previous) = self.vr_cursor_physical {
            self.cursor.0 += x - previous.0;
            self.cursor.1 += y - previous.1;
        }
        self.vr_cursor_physical = Some((x, y));
        self.html_move();
        let Some((width, height)) = self
            .surface
            .as_ref()
            .map(|s| (s.config.width as f32, s.config.height as f32))
        else {
            return;
        };
        if self.window_focused
            && !self.mouse_look
            && (x < 12.0 || x > width - 12.0 || y < 12.0 || y > height - 12.0)
        {
            let center = (width * 0.5, height * 0.5);
            if self.window.as_ref().is_some_and(|window| {
                window
                    .set_cursor_position(winit::dpi::PhysicalPosition::new(
                        center.0 as f64,
                        center.1 as f64,
                    ))
                    .is_ok()
            }) {
                self.vr_cursor_physical = Some(center);
                self.vr_cursor_warp_pending = Some(center);
            }
        }
    }

    #[cfg(windows)]
    pub(crate) fn poll_vr_cursor_position(&mut self) {
        if self.vr_nav_edit.is_some() {
            return;
        }
        let cockpit = self.vr.is_some()
            && self.game_menu.is_none()
            && self.chooser.is_none()
            && !self.mouse_drive
            && matches!(self.view.as_str(), "driver" | "pax");
        if !cockpit {
            self.vr_cursor_physical = None;
            self.vr_cursor_warp_pending = None;
            return;
        }
        if !self.window_focused || self.mouse_look {
            return;
        }
        let Some(window) = self.window.as_ref() else {
            return;
        };
        let Ok(client_origin) = window.inner_position() else {
            return;
        };
        let mut point = windows::Win32::Foundation::POINT::default();
        if unsafe { windows::Win32::UI::WindowsAndMessaging::GetCursorPos(&mut point) }.is_ok() {
            self.on_vr_cursor_moved(
                (point.x - client_origin.x) as f32,
                (point.y - client_origin.y) as f32,
            );
        }
    }

    pub(crate) fn mouse_past_edge(&mut self, dx: f32) {
        let Some(w) = self.surface.as_ref().map(|s| s.config.width as f32) else {
            return;
        };
        let per_px = 2.0 / w.max(1.0);
        let (at_left, at_right) = (self.cursor.0 <= 2.0, self.cursor.0 >= w - 3.0);
        let before = self.mouse_edge;
        if (at_right && dx > 0.0) || (at_left && dx < 0.0) {
            self.mouse_edge = (self.mouse_edge + dx * per_px).clamp(-2.0, 2.0);
        } else if (self.mouse_edge > 0.0 && dx < 0.0) || (self.mouse_edge < 0.0 && dx > 0.0) {
            let m = self.mouse_edge + dx * per_px;
            self.mouse_edge = if m.signum() != before.signum() {
                0.0
            } else {
                m
            };
            if let Some(win) = self.window.as_ref() {
                let x = if before > 0.0 { w - 2.0 } else { 1.0 };
                let _ = win.set_cursor_position(winit::dpi::PhysicalPosition::new(
                    x as f64,
                    self.cursor.1 as f64,
                ));
                self.cursor.0 = x;
            }
        }
    }

    pub(crate) fn move_cursor(&mut self, x: f32, y: f32) -> bool {
        let last = self.cursor;
        self.cursor = (x, y);
        if self.lab_menu.is_some() {
            if (x, y) != last {
                if let Some(st) = self.lab_menu.filter(|s| s.page.is_none()) {
                    let hit = self.ui.as_ref().and_then(|u| {
                        u.pause_items
                            .iter()
                            .position(|r| x >= r[0] && x < r[2] && y >= r[1] && y < r[3])
                    });
                    if let Some(k) = hit {
                        self.lab_menu = Some(crate::ui::PauseState { sel: k, ..st });
                    }
                }
            }
            if let Some(n) = self.navigator.as_mut().filter(|n| n.city.embed.is_some()) {
                n.map_move(x, y);
            }
            if self.ui.as_ref().is_some_and(|u| u.world_drag.is_some()) {
                self.lab_world_set(x);
            }
            if self.ui.as_ref().is_some_and(|u| u.world_bar_grab.is_some()) {
                self.lab_world_bar_set(y);
            }
        }
        if self.game_menu.is_some() && (x, y) != last {
            self.menu_kbd = false;
        }
        if self.menu_drag.is_some() && self.game_menu.is_none() {
            self.menu_drag = None;
        }
        if let Some(k) = self.menu_drag {
            let c = self.ui.as_ref().and_then(|u| {
                k.checked_sub(u.menu_start)
                    .and_then(|i| u.menu_ctl.get(i).copied().flatten())
            });
            match c {
                Some(c) => {
                    let fx = ((x - c[0]) / (c[2] - c[0]).max(1.0)).clamp(0.0, 1.0);
                    self.list_click(k, fx);
                }
                None => self.menu_drag = None,
            }
            return false;
        }
        if let Some((y0, v0)) = self.both_drag {
            // (0x82c5f8: outside, the distance at the press times 1 + the way up over 500
            // pixels; in the bus the field of view at the press plus the way up over 500
            // pixels times the camera's own, which is also its widest (+0x31c, 0x7edde4):
            // moving up widens the view as it backs the outside camera away)
            if self.view == "outside" {
                let k = (1.0 + (y0 - y) / 500.0).max(0.05);
                self.orbit = (v0 * k).clamp(ORBIT_MIN, ORBIT_MAX);
            } else {
                self.view_zoom.insert(
                    self.view.clone(),
                    (v0 + (y0 - y) / 500.0).clamp(0.2, 1.0_f32.max(v0)),
                );
            }
            return false;
        }
        if self.menu_scroll_drag {
            let Some(ui) = self.ui.as_ref() else {
                self.menu_scroll_drag = false;
                return true;
            };

            if let (Some(track), Some(thumb)) = (ui.menu_scroll_track, ui.menu_scroll_thumb) {
                let track_h = (track[3] - track[1]).max(1.0);
                let thumb_h = (thumb[3] - thumb[1]).max(1.0);
                let travel = (track_h - thumb_h).max(1.0);

                let max_top = (self.menu_len() as f32 - ui.menu_rows as f32).max(0.0);

                if max_top > 0.0 {
                    let delta = (y - last.1) / travel * max_top;

                    self.menu_top = Some(
                        (self.menu_top.unwrap_or(ui.menu_start as f32) + delta).clamp(0.0, max_top),
                    );
                }
            }

            return false;
        }
        if self.editor_drag {
            self.editor_drag_frame();
            return false;
        }
        if let Some(n) = self.navigator.as_mut().filter(|n| n.map_open()) {
            n.map_move(x, y);
            return false;
        }
        // looking round in a view of the bus follows the cursor, as Omsi.exe turns it
        // (0x82c5f8: yaw and pitch at the press plus the cursor's way times fov / 78.75):
        // raw device deltas are no window pixels (a tablet, a remote desktop or a VM
        // reports positions there and spun the view) and did not follow the zoom
        if self.cursor_looks() {
            let scale = self
                .window
                .as_ref()
                .map(|w| w.scale_factor() as f32)
                .unwrap_or(1.0)
                .max(0.1);
            let fov = self.camera.as_ref().map(|c| c.fov_deg).unwrap_or(60.0);
            let k = look_deg_per_px(fov) * (::config::get_float("camera", "look_sens").unwrap_or(1.0) as f32);
            self.look_by((x - last.0) / scale * k, (y - last.1) / scale * k);
        }
        if self.dragging {
            let scale = self
                .window
                .as_ref()
                .map(|w| w.scale_factor() as f32)
                .unwrap_or(1.0)
                .max(0.1);
            self.drag_delta.0 += (self.cursor.0 - last.0) / scale;
            self.drag_delta.1 += (self.cursor.1 - last.1) / scale;
        }
        true
    }

    pub(crate) fn on_left(&mut self, pressed: bool) {
        if self.vr_nav_edit.is_some() {
            return;
        }
        if self.game_menu.is_none() && self.editor_mouse(pressed) {
            return;
        }
        let (x, y) = self.cursor;
        let vr_active = self.vr_active();
        if let Some(n) = self.navigator.as_mut() {
            if n.map_open() {
                let ctrl = self.keys.contains(&KeyCode::ControlLeft)
                    || self.keys.contains(&KeyCode::ControlRight);
                if pressed && (ctrl || self.teleport_pick) {
                    if let Some(at) = n.map_point(x, y) {
                        if std::mem::take(&mut self.teleport_pick) {
                            n.toggle_map();
                        }
                        self.place_bus_at(at);
                    }
                } else if pressed {
                    n.map_press(x, y);
                } else {
                    n.map_release();
                }
                return;
            }
            if pressed && !vr_active && n.over_panel(x, y) {
                self.open_map_page();
                return;
            }
        }
        if pressed && self.lan.is_some() && ::config::get_bool("ui", "chat").unwrap_or(true) {
            if self.ui.as_ref().map(|u| u.chat.hovered).unwrap_or(false) {
                self.remotes.chat.open();
                return;
            }
            self.remotes.chat.blur();
        }
        if self.view == "foot" && self.inside_remote.is_some() {
            return;
        }
        if self.html_object_click(pressed) {
            return;
        }
        if self.view == "foot" && !self.foot_reaches_bus() {
            return;
        }
        #[cfg(windows)]
        if self.vr.is_some()
            && self.mouse_drive
            && self.game_menu.is_none()
            && matches!(self.view.as_str(), "driver" | "pax")
        {
            if !pressed {
                if let Some(player) = self.player.as_mut() {
                    player.release();
                }
                self.dragging = false;
            }
            return;
        }
        let ray = self
            .camera
            .as_ref()
            .zip(self.surface.as_ref())
            .map(|(cam, s)| self.cockpit_cursor_ray(cam, (s.config.width, s.config.height)));
        {
            let k = if pressed {
                ray.and_then(|(o, d, s)| self.placed_target(o, d, s))
            } else {
                self.placed_grab.take()
            };
            if let (Some(k), Some((o, d, spread))) = (k, ray) {
                self.drag_delta = (0.0, 0.0);
                if let Some(p) = self.placed.get_mut(k) {
                    if pressed {
                        if let Some((page, u, v)) = p.html_hit(o, d) {
                            p.release();
                            p.html_pointer(page, u, v, ::simulation::htmltex::PointerKind::Down);
                            self.html_pressed = Some((page, u, v));
                            self.placed_grab = Some(k);
                            self.dragging = false;
                            return;
                        }
                        self.dragging = p.click(o, d, spread).is_some();
                        self.placed_grab = Some(k);
                    } else {
                        if let Some((page, u, v)) = self.html_pressed.take() {
                            let (u, v) = p
                                .html_hit(o, d)
                                .filter(|h| h.0 == page)
                                .map_or((u, v), |h| (h.1, h.2));
                            p.html_pointer(page, u, v, ::simulation::htmltex::PointerKind::Up);
                        } else {
                            p.release();
                        }
                        self.dragging = false;
                    }
                }
                return;
            }
            if self.player.is_none() {
                return;
            }
        }
        if let (Some(p), Some((o, d, spread))) = (self.player.as_mut(), ray) {
            self.drag_delta = (0.0, 0.0);
            if pressed {
                if let Some((page, u, v)) = p.html_hit(o, d) {
                    p.release();
                    p.html_pointer(page, u, v, ::simulation::htmltex::PointerKind::Down);
                    self.html_pressed = Some((page, u, v));
                    self.dragging = false;
                    return;
                }
                if self
                    .humans
                    .as_mut()
                    .and_then(|h| h.money.as_mut())
                    .is_some_and(|m| m.pick(o, d, spread, || p.body_hit(o, d)))
                {
                    p.release();
                    self.dragging = false;
                    return;
                }
                self.dragging = p.click(o, d, spread).is_some();
            } else {
                if let Some((page, u, v)) = self.html_pressed.take() {
                    let (u, v) = p
                        .html_hit(o, d)
                        .filter(|h| h.0 == page)
                        .map_or((u, v), |h| (h.1, h.2));
                    p.html_pointer(page, u, v, ::simulation::htmltex::PointerKind::Up);
                    self.dragging = false;
                    return;
                }
                if self.dragging && self.buttons_held.1 {
                    p.release_keeping();
                } else {
                    p.release();
                }
                self.dragging = false;
            }
        }
    }

    pub(crate) fn html_object_click(&mut self, pressed: bool) -> bool {
        let Some(w) = self.world.clone() else {
            return false;
        };
        if !pressed {
            let Some((id, page, u, v)) = self.html_object_pressed.take() else {
                return false;
            };
            let (u, v) = self
                .cursor_ray_now()
                .and_then(|(o, d, _)| w.html_object_hit(o, d, HTML_OBJECT_REACH))
                .filter(|h| h.map_id == id && h.page == page)
                .map_or((u, v), |h| (h.u, h.v));
            w.html_object_pointer(id, page, u, v, ::simulation::htmltex::PointerKind::Up);
            self.dragging = false;
            return true;
        }
        #[cfg(windows)]
        if self.vr.is_some()
            && self.mouse_drive
            && self.game_menu.is_none()
            && matches!(self.view.as_str(), "driver" | "pax")
        {
            return false;
        }
        let Some((o, d, _)) = self.cursor_ray_now() else {
            return false;
        };
        let Some(h) = w.html_object_hit(o, d, HTML_OBJECT_REACH) else {
            return false;
        };
        if self
            .player
            .as_ref()
            .and_then(|p| p.body_hit(o, d))
            .is_some_and(|t| t < h.t)
        {
            return false;
        }
        if let Some(p) = self.player.as_mut() {
            p.release();
        }
        w.html_object_pointer(
            h.map_id,
            h.page,
            h.u,
            h.v,
            ::simulation::htmltex::PointerKind::Down,
        );
        self.html_object_pressed = Some((h.map_id, h.page, h.u, h.v));
        self.dragging = false;
        true
    }

    pub(crate) fn cursor_ray_now(&self) -> Option<(glam::DVec3, glam::Vec3, f32)> {
        let (cam, s) = self.camera.as_ref().zip(self.surface.as_ref())?;
        Some(self.cockpit_cursor_ray(cam, (s.config.width, s.config.height)))
    }

    pub(crate) fn drag_frame(&mut self) {
        if !self.dragging {
            return;
        }
        let (dx, dy) = std::mem::take(&mut self.drag_delta);
        if let Some(p) = self.placed_grab.and_then(|k| self.placed.get_mut(k)) {
            p.drag(dx, dy);
        } else if let Some(p) = self.player.as_mut() {
            p.drag(dx, dy);
        }
    }

    pub(crate) fn placed_in_reach(&self) -> Option<usize> {
        let c = self.camera.as_ref()?;
        let mut best: Option<(usize, f64)> = None;
        for (k, q) in self.placed.iter().enumerate() {
            let v = &q.vehicle;
            let near = std::iter::once((v.position, v.heading, v.ty.def.bounding_box))
                .chain(
                    v.trailers
                        .iter()
                        .map(|t| (t.position, t.heading, t.ty.def.bounding_box)),
                )
                .any(|(at, heading, bb)| part_in_reach(c.position, at, heading, bb));
            let d = (v.position - c.position).length();
            if near && best.map(|b| d < b.1).unwrap_or(true) {
                best = Some((k, d));
            }
        }
        best.map(|b| b.0)
    }

    pub(crate) fn placed_target(
        &self,
        o: glam::DVec3,
        d: glam::Vec3,
        spread: f32,
    ) -> Option<usize> {
        if let Some(p) = self.player.as_ref() {
            let (f, hand) = p.hovered_part(o, d, spread);
            if f.is_some() || hand {
                return None;
            }
        }
        let eye = self.camera.as_ref()?.position;
        let mut best: Option<(usize, f64)> = None;
        for (k, q) in self.placed.iter().enumerate() {
            let v = &q.vehicle;
            let near = std::iter::once((v.position, v.heading, v.ty.def.bounding_box))
                .chain(
                    v.trailers
                        .iter()
                        .map(|t| (t.position, t.heading, t.ty.def.bounding_box)),
                )
                .any(|(at, heading, bb)| part_in_reach(eye, at, heading, bb));
            if !near {
                continue;
            }
            let (f, hand) = q.hovered_part(o, d, spread);
            let dist = (v.position - eye).length();
            if (f.is_some() || hand) && best.map(|b| dist < b.1).unwrap_or(true) {
                best = Some((k, dist));
            }
        }
        best.map(|b| b.0)
    }

    pub(crate) fn foot_reaches_bus(&self) -> bool {
        if self.foot_bus() == Some(crate::humans::BusId::Player) {
            return true;
        }
        if self.placed_in_reach().is_some() {
            return true;
        }
        match (self.player.as_ref(), self.camera.as_ref()) {
            // (every part of an articulated bus: a door button of the rear section is in
            // reach standing by that section, however far the front one is - #715)
            (Some(p), Some(c)) => {
                let v = &p.vehicle;
                std::iter::once((v.position, v.heading, v.ty.def.bounding_box))
                    .chain(
                        v.trailers
                            .iter()
                            .map(|t| (t.position, t.heading, t.ty.def.bounding_box)),
                    )
                    .any(|(at, heading, bb)| part_in_reach(c.position, at, heading, bb))
            }
            _ => false,
        }
    }

    pub(crate) fn cockpit_cursor_ray(
        &self,
        cam: &Camera,
        size: (u32, u32),
    ) -> (glam::DVec3, glam::Vec3, f32) {
        #[cfg(windows)]
        if let Some(ray) = self
            .vr
            .as_ref()
            .and_then(|vr| vr.cursor_ray(self.cursor.0, self.cursor.1, size))
        {
            return (ray.0, ray.1, ray.2 * 6.0);
        }
        let (o, d) = cursor_ray(
            cam,
            self.cursor.0,
            self.cursor.1,
            size.0 as f32,
            size.1 as f32,
        );
        (o, d, pixel_angle(cam, size.1 as f32) * 6.0)
    }

    pub(crate) fn update_hover(&mut self) {
        if self.vr_nav_edit.is_some() || self.cursor_hidden.is_some() {
            self.hover = None;
            self.hover_part = None;
            self.hover_hand = false;
            return;
        }
        #[cfg(windows)]
        if !self.mouse_drive
            && self.vr.as_ref().is_some_and(|vr| {
            vr.needs_cursor_surface(
                self.cursor,
                self.game_menu.is_some() || self.chooser.is_some(),
            )
        })
        {
            let surface = self
                .player
                .as_ref()
                .zip(self.camera.as_ref())
                .zip(self.surface.as_ref())
                .filter(|_| matches!(self.view.as_str(), "driver" | "pax"))
                .map(|((player, camera), window)| {
                    let (origin, direction, _) = self
                        .cockpit_cursor_ray(camera, (window.config.width, window.config.height));
                    (
                        player.surface_hit(origin, direction),
                        (player.vehicle.position, player.vehicle.body_rotation()),
                    )
                });
            if let Some(vr) = self.vr.as_mut() {
                vr.set_cursor_surface(surface.as_ref().and_then(|s| s.0), surface.map(|s| s.1));
            }
        }
        let found = match (
            self.player.as_ref(),
            self.camera.as_ref(),
            self.surface.as_ref(),
        ) {
            (Some(p), Some(cam), Some(s))
            if self.view != "free"
                && (self.view != "foot" || self.foot_reaches_bus())
                && !(self.vr_active()
                && self.mouse_drive
                && matches!(self.view.as_str(), "driver" | "pax")) =>
                {
                    let (o, d, spread) =
                        self.cockpit_cursor_ray(cam, (s.config.width, s.config.height));
                    let (part, hand) = p.hovered_part(o, d, spread);
                    let coin = !hand
                        && self
                            .humans
                            .as_ref()
                            .and_then(|h| h.money.as_ref())
                            .and_then(|m| m.change_under(o, d, spread, || p.body_hit(o, d)))
                            .is_some();
                    (part, hand || coin)
                }
            _ => (None, false),
        };
        let found = if found.0.is_none() && !found.1 && self.view != "free" {
            match (self.camera.as_ref(), self.surface.as_ref()) {
                (Some(cam), Some(s)) => {
                    let (o, d, spread) =
                        self.cockpit_cursor_ray(cam, (s.config.width, s.config.height));
                    match self.placed_target(o, d, spread) {
                        Some(k) => self.placed[k].hovered_part(o, d, spread),
                        None => found,
                    }
                }
                _ => found,
            }
        } else {
            found
        };
        let (found, hand) = found;
        self.hover_hand = hand;
        match found {
            Some((name, true)) => {
                self.hover = Some(name);
                self.hover_part = None;
            }
            Some((name, false)) => {
                self.hover = None;
                self.hover_part = Some(name);
            }
            None => {
                self.hover = None;
                self.hover_part = None;
            }
        }
        // the cursor itself says when it is over something that can be operated
        // (steering with the mouse: a cross, as OMSI shows it; turning the view with the
        // right button held: the four arrows OMSI shows then, #185)
        // (zooming with the mouse: the up-down arrows, Omsi's crSizeNS)
        let kind: u8 = if self.both_drag.is_some() && self.game_menu.is_none() {
            4
        } else if self.mouse_look && self.game_menu.is_none() {
            3
        } else if self.mouse_drive && self.mouse_steers_in_view() && self.game_menu.is_none() {
            2
        } else if self.game_menu.is_some() {
            self.menu_cursor_kind()
        } else if self.hover.is_some() || self.hover_hand {
            1
        } else {
            0
        };
        self.set_cursor_kind(kind);
    }

    pub(crate) fn set_cursor_kind(&mut self, kind: u8) {
        if kind != self.cursor_kind {
            self.cursor_kind = kind;
            if let Some(w) = self.window.as_ref() {
                w.set_cursor(match kind {
                    4 => winit::window::CursorIcon::NsResize,
                    3 => winit::window::CursorIcon::Move,
                    2 => winit::window::CursorIcon::Crosshair,
                    1 => winit::window::CursorIcon::Pointer,
                    _ => winit::window::CursorIcon::Default,
                });
            }
        }
    }

    pub(crate) fn menu_cursor_kind(&self) -> u8 {
        if self.menu_drag.is_some() || self.menu_scroll_drag {
            return 4;
        }
        let Some(u) = self.ui.as_ref() else { return 0 };
        if self.lab_menu.is_some() {
            return u8::from(u.hand);
        }
        let (x, y) = self.cursor;
        let inside = |r: &[f32; 4]| x >= r[0] && x <= r[2] && y >= r[1] && y <= r[3];
        let clickable = u.menu_scroll_thumb.is_some_and(|r| inside(&r))
            || u.menu_side.iter().any(|r| inside(r))
            || u.menu_pane.iter().any(|r| inside(r))
            || u.menu_pane_go.as_ref().is_some_and(|r| inside(r))
            || u.menu_time.iter().any(|r| inside(r))
            || u.menu_ctl.iter().flatten().any(|r| inside(r))
            || u.menu_rects.iter().any(|r| inside(r));
        if clickable { 1 } else { 0 }
    }
}
