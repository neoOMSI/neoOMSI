//! Mouse wheel and left button as the game reacts to them.

use super::*;

pub(crate) fn triple_screen_hud_offset(
    surface_width: u32,
    surface_height: u32,
    triple_screen: bool,
    triple_screen_hud: bool,
    vr_active: bool,
) -> f32 {
    if !vr_active
        && triple_screen
        && triple_screen_hud
        && surface_width >= surface_height.saturating_mul(2)
    {
        surface_width as f32 / 3.0
    } else {
        0.0
    }
}

impl App {
    pub(crate) fn ui_cursor(&self) -> (f32, f32) {
        let offset = self
            .surface
            .as_ref()
            .map(|s| {
                triple_screen_hud_offset(
                    s.config.width,
                    s.config.height,
                    ::config::get_bool("graphics", "triple_screen").unwrap_or(false),
                    ::config::get_bool("graphics", "triple_screen_hud").unwrap_or(true),
                    self.vr_active(),
                )
            })
            .unwrap_or(0.0);
        (self.cursor.0 - offset, self.cursor.1)
    }

    pub(crate) fn wheel(&mut self, amount: f32) {
        if self.lab_dialog_wheel(amount) {
            return;
        }
        if self.lab_menu.is_some_and(|s| s.page == Some(0)) {
            let (x, y) = self.ui_cursor();
            if let Some(n) = self.navigator.as_mut().filter(|n| n.city.embed.is_some()) {
                let r = n.city.rect;
                if x >= r[0] && x < r[2] && y >= r[1] && y < r[3] {
                    n.map_wheel(amount, x, y);
                }
            }
            return;
        }
        if self.lab_world_wheel(amount) {
            return;
        }
        if self.vr_nav_edit.is_some() {
            self.vr_nav_scroll(amount);
            return;
        }
        if self.game_menu.is_none() && self.editor_wheel(amount) {
            return;
        }
        if self.placing.is_some() && self.game_menu.is_none() {
            self.placing_wheel(amount);
            return;
        }
        if self.game_menu.is_some() {
            self.menu_wheel(amount);
            return;
        }
        let (x, y) = self.ui_cursor();
        if let Some(n) = self.navigator.as_mut().filter(|n| n.map_open()) {
            n.map_wheel(amount, x, y);
            return;
        }
        if let Some(ui) = self.ui.as_mut() {
            if self.lan.is_some() && (ui.chat.hovered || lan::chat_open(&self.remotes)) {
                ui.chat.wheel(self.remotes.chat.lines.len(), amount);
                return;
            }
        }
        if self.hover.is_some() && self.view != "free" {
            let ray = self
                .camera
                .as_ref()
                .zip(self.surface.as_ref())
                .map(|(cam, s)| self.cockpit_cursor_ray(cam, (s.config.width, s.config.height)));
            if let (Some(p), Some((o, d, spread))) = (self.player.as_mut(), ray) {
                if p.pick(o, d, spread).is_some() {
                    // a notch is worth a good push of the mouse: the scripts divide
                    // the movement by 10 (the ignition key), 200 (the parking brake)
                    // or 500 (the driver's window), so a few pixels would do nothing
                    p.wheel(o, d, spread, -amount * 40.0);
                    return;
                }
            }
        }
        let ctrl =
            self.keys.contains(&KeyCode::ControlLeft) || self.keys.contains(&KeyCode::ControlRight);
        if self.view == "outside" && self.player.is_some() && ctrl {
            // Ctrl+wheel: the outside camera stays where it is and narrows its field of view
            // (a telephoto; OMSI's own zoom there only moves the camera, as the wheel does)
            self.zoom_by(amount);
        } else if self.view == "outside" && self.player.is_some() {
            self.orbit = (self.orbit - amount * 1.5).clamp(ORBIT_MIN, ORBIT_MAX);
        } else if matches!(self.view.as_str(), "driver" | "pax") && self.player.is_some() {
            self.zoom_by(amount);
        } else if matches!(self.view.as_str(), "free" | "foot") && !ctrl {
            self.zoom_by(amount);
        } else if let Some(cam) = self.camera.as_mut() {
            let f = cam.forward();
            cam.position += (f * amount * 4.0).as_dvec3();
        }
    }

    pub(crate) fn left_button(&mut self, event_loop: &ActiveEventLoop, pressed: bool) {
        if let Some(edit) = self.vr_nav_edit.as_mut() {
            edit.moving = pressed;
            return;
        }
        let state = if pressed {
            ElementState::Pressed
        } else {
            ElementState::Released
        };
        let cursor = self.ui_cursor();
        if self.placing.is_some() && self.game_menu.is_none() {
            if state == ElementState::Pressed {
                self.placing_click();
            }
            return;
        }
        if self.lab_menu.is_some() {
            if !pressed {
                self.lab_bar_drag = 0;
                if let Some(u) = self.ui.as_mut() {
                    u.world_drag = None;
                    u.world_bar_grab = None;
                }
            } else if self.lab_bar_press() {
                return;
            }
            if self.lab_map_mouse(Some(pressed)) {
                return;
            }
            if pressed {
                self.lab_click(event_loop);
            }
            return;
        }
        if self.game_menu.is_some() {
            if state == ElementState::Pressed {
                self.menu_kbd = false;
            }
            if state == ElementState::Released {
                self.menu_drag = None;
                if self.menu_scroll_drag {
                    self.menu_scroll_drag = false;
                    self.menu_top = self.menu_top.map(f32::round);
                }
                return;
            }

            if self.dropdown.is_some() {
                let inside = |r: &[f32; 4]| {
                    cursor.0 >= r[0] && cursor.0 <= r[2] && cursor.1 >= r[1] && cursor.1 <= r[3]
                };
                let hit = self.ui.as_ref().and_then(|u| {
                    u.dd_rects
                        .iter()
                        .position(|r| inside(r))
                        .map(|i| i + u.dd_top)
                });
                match hit {
                    Some(i) => self.dropdown_pick(i),
                    None => self.dropdown = None,
                }
                return;
            }

            if state == ElementState::Pressed {
                if let Some(thumb) = self.ui.as_ref().and_then(|u| u.menu_scroll_thumb) {
                    if cursor.0 >= thumb[0]
                        && cursor.0 <= thumb[2]
                        && cursor.1 >= thumb[1]
                        && cursor.1 <= thumb[3]
                    {
                        self.menu_scroll_drag = true;
                        return;
                    }
                }

                if self.chooser.is_some() {
                    let side = self.ui.as_ref().and_then(|u| {
                        u.menu_side.iter().position(|r| {
                            cursor.0 >= r[0]
                                && cursor.0 <= r[2]
                                && cursor.1 >= r[1]
                                && cursor.1 <= r[3]
                        })
                    });
                    if let Some(i) = side {
                        self.settings_side_click(i);
                        return;
                    }
                }

                if self.chooser.is_some() {
                    let pane = self.ui.as_ref().and_then(|u| {
                        let inside = |r: &[f32; 4]| {
                            cursor.0 >= r[0]
                                && cursor.0 <= r[2]
                                && cursor.1 >= r[1]
                                && cursor.1 <= r[3]
                        };
                        if u.menu_pane_go.as_ref().is_some_and(inside) {
                            return Some(usize::MAX);
                        }
                        if let Some(j) = u.menu_time.iter().position(inside) {
                            return Some(usize::MAX - 1 - j);
                        }
                        u.menu_pane
                            .iter()
                            .position(inside)
                            .map(|i| i + u.menu_pane_start)
                    });
                    if let Some(i) = pane {
                        self.tour_pane_click(i);
                        return;
                    }
                }

                if self.chooser.is_some() && self.key_capture.is_none() {
                    let on_field = self
                        .ui
                        .as_ref()
                        .and_then(|u| u.menu_search)
                        .is_some_and(|r| {
                            cursor.0 >= r[0]
                                && cursor.0 <= r[2]
                                && cursor.1 >= r[1]
                                && cursor.1 <= r[3]
                        });
                    if on_field {
                        self.key_search_start();
                        return;
                    }
                    if self.key_search {
                        self.key_search_stop();
                    }
                }

                let hit = self.ui.as_ref().and_then(|u| {
                    u.menu_rects.iter().position(|r| {
                        cursor.0 >= r[0] && cursor.0 <= r[2] && cursor.1 >= r[1] && cursor.1 <= r[3]
                    })
                });

                if let Some(row) = hit {
                    let k = row + self.ui.as_ref().map(|u| u.menu_start).unwrap_or(0);
                    let ctl = self
                        .ui
                        .as_ref()
                        .and_then(|u| u.menu_ctl.get(row).copied().flatten());

                    if let Some(c) = ctl {
                        if self.chooser.is_some() && cursor.0 >= c[0] && cursor.0 <= c[2] {
                            let fx = ((cursor.0 - c[0]) / (c[2] - c[0]).max(1.0)).clamp(0.0, 1.0);
                            self.chooser = Some(k);
                            if self.list_click(k, fx) {
                                self.menu_drag = Some(k);
                            }
                            return;
                        }
                    }

                    if matches!(self.list_kind, Some(game_lists::ListKind::Tours(..)))
                        && game_lists::tour_at(self, k).is_some()
                    {
                        self.chooser = Some(k);
                        if let Some(game_lists::ListKind::Tours(line, _)) = self.list_kind.clone() {
                            self.list_kind = Some(game_lists::ListKind::Tours(line, None));
                        }
                        return;
                    }

                    let arrows = self
                        .ui
                        .as_ref()
                        .and_then(|u| u.menu_arrows.get(row).copied().flatten());
                    match arrows {
                        Some([from, to, _]) if cursor.0 >= from && cursor.0 < to => {
                            self.chooser_adjust(k, "-")
                        }
                        Some([_, _, plus]) if cursor.0 >= plus => self.chooser_adjust(k, "+"),
                        _ if self.chooser.is_some() || self.report_view.is_some() => {
                            self.menu_choose(event_loop, k)
                        }
                        _ => {}
                    }
                }
            }

            return;
        }
        self.on_left(state == ElementState::Pressed)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn triple_screen_hud_offset_centers_hud_for_ultra_wide_aspect() {
        assert_eq!(
            triple_screen_hud_offset(5760, 1080, true, true, false),
            1920.0
        );
        assert_eq!(
            triple_screen_hud_offset(3840, 1080, true, true, false),
            3840.0 / 3.0
        );
    }

    #[test]
    fn triple_screen_hud_offset_disabled_or_vr_leaves_cursor_unadjusted() {
        // Disabled via triple_screen setting
        assert_eq!(
            triple_screen_hud_offset(5760, 1080, false, true, false),
            0.0
        );
        // Disabled via triple_screen_hud setting
        assert_eq!(
            triple_screen_hud_offset(5760, 1080, true, false, false),
            0.0
        );
        // VR active overrides triple-screen HUD offset
        assert_eq!(
            triple_screen_hud_offset(5760, 1080, true, true, true),
            0.0
        );
        // Ordinary single screen (16:9 / aspect < 2:1) does not apply HUD offset
        assert_eq!(
            triple_screen_hud_offset(1920, 1080, true, true, false),
            0.0
        );
        assert_eq!(
            triple_screen_hud_offset(1920, 1080, false, false, false),
            0.0
        );
    }

    #[test]
    fn clicking_visually_centered_ui_element_selects_intended_element() {
        // A visually centered pause menu button on a 1920-wide HUD (e.g. center monitor)
        let hud_element_rect = [860.0, 400.0, 1060.0, 460.0];
        let inside = |r: [f32; 4], p: (f32, f32)| {
            p.0 >= r[0] && p.0 < r[2] && p.1 >= r[1] && p.1 < r[3]
        };

        // On a 5760x1080 triple-screen display, the HUD overlay is drawn shifted by 1920px.
        // Visually, the element is centered at physical window x = 960 + 1920 = 2880, y = 430.
        let physical_cursor = (2880.0, 430.0);

        // Before fix: unadjusted physical window cursor missed HUD-local rect
        assert!(!inside(hud_element_rect, physical_cursor));

        // After fix: triple-screen HUD offset adjusts physical cursor into HUD-local space
        let offset = triple_screen_hud_offset(5760, 1080, true, true, false);
        let adjusted_cursor = (physical_cursor.0 - offset, physical_cursor.1);
        assert_eq!(adjusted_cursor, (960.0, 430.0));
        assert!(inside(hud_element_rect, adjusted_cursor));

        // When triple-screen HUD is disabled on 5760x1080, the interface spans full width
        // and uses unadjusted coordinates.
        let offset_disabled = triple_screen_hud_offset(5760, 1080, true, false, false);
        assert_eq!(offset_disabled, 0.0);

        // On standard 1920x1080 single screen, cursor at (960, 430) hits unchanged.
        let single_offset = triple_screen_hud_offset(1920, 1080, false, true, false);
        let single_cursor = (960.0 - single_offset, 430.0);
        assert_eq!(single_cursor, (960.0, 430.0));
        assert!(inside(hud_element_rect, single_cursor));
    }
}
