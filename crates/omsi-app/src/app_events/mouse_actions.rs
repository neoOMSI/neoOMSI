//! Mouse wheel and left button as the game reacts to them.

use super::*;

impl App {
    pub(crate) fn wheel(&mut self, amount: f32) {
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
        if let Some(n) = self.navigator.as_mut().filter(|n| n.map_open()) {
            n.map_wheel(amount, self.cursor.0, self.cursor.1);
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
                p.occlude_controls = self.view == "outside";
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
        if self.placing.is_some() && self.game_menu.is_none() {
            if state == ElementState::Pressed {
                self.placing_click();
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
                    self.cursor.0 >= r[0]
                        && self.cursor.0 <= r[2]
                        && self.cursor.1 >= r[1]
                        && self.cursor.1 <= r[3]
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
                    if self.cursor.0 >= thumb[0]
                        && self.cursor.0 <= thumb[2]
                        && self.cursor.1 >= thumb[1]
                        && self.cursor.1 <= thumb[3]
                    {
                        self.menu_scroll_drag = true;
                        return;
                    }
                }

                if self.chooser.is_some() {
                    let side = self.ui.as_ref().and_then(|u| {
                        u.menu_side.iter().position(|r| {
                            self.cursor.0 >= r[0]
                                && self.cursor.0 <= r[2]
                                && self.cursor.1 >= r[1]
                                && self.cursor.1 <= r[3]
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
                            self.cursor.0 >= r[0]
                                && self.cursor.0 <= r[2]
                                && self.cursor.1 >= r[1]
                                && self.cursor.1 <= r[3]
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
                            self.cursor.0 >= r[0]
                                && self.cursor.0 <= r[2]
                                && self.cursor.1 >= r[1]
                                && self.cursor.1 <= r[3]
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
                        self.cursor.0 >= r[0]
                            && self.cursor.0 <= r[2]
                            && self.cursor.1 >= r[1]
                            && self.cursor.1 <= r[3]
                    })
                });

                if let Some(row) = hit {
                    let k = row + self.ui.as_ref().map(|u| u.menu_start).unwrap_or(0);
                    let ctl = self
                        .ui
                        .as_ref()
                        .and_then(|u| u.menu_ctl.get(row).copied().flatten());

                    if self.menu_item_off(k) {
                        return;
                    }

                    if let Some(c) = ctl {
                        if self.chooser.is_some() && self.cursor.0 >= c[0] && self.cursor.0 <= c[2]
                        {
                            let fx =
                                ((self.cursor.0 - c[0]) / (c[2] - c[0]).max(1.0)).clamp(0.0, 1.0);
                            self.chooser = Some(k);
                            if self.list_click(k, fx) {
                                self.menu_drag = Some(k);
                            }
                            return;
                        }
                    }

                    if self.chooser.is_none() {
                        self.game_menu = Some(k);
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
                        Some([from, to, _]) if self.cursor.0 >= from && self.cursor.0 < to => {
                            self.chooser_adjust(k, "-")
                        }
                        Some([_, _, plus]) if self.cursor.0 >= plus => self.chooser_adjust(k, "+"),
                        _ => self.menu_choose(event_loop, k),
                    }
                }
            }

            return;
        }
        self.on_left(state == ElementState::Pressed)
    }
}
