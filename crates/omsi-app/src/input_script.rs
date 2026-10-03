//! `OMSI_INPUT`: scripted keyboard, mouse and camera input for window runs, and its key names.

use super::*;

pub(crate) use crate::{game_menu::*, input_keys::*, input_mouse::*};

pub(crate) fn is_game_action(name: &str) -> bool {
    let name = name.to_ascii_lowercase();
    name.starts_with("view_")
        || matches!(
            name.as_str(),
            "sim_pause" | "quicksave" | "toggel_mouse_ctrl" | "toggel_ctrler"
        )
}

pub(crate) fn dump_display_textures(vehicle: &omsi_sim::VehicleInstance, dir: &Path) {
    let _ = std::fs::create_dir_all(dir);
    for (i, st) in vehicle.host.script_textures.iter().enumerate() {
        let _ = image::save_buffer(
            dir.join(format!("scripttex_{i}.png")),
            &st.rgba,
            st.width,
            st.height,
            image::ColorType::Rgba8,
        );
    }
    for (i, tt) in vehicle.text_textures.iter().enumerate() {
        let text = tt.last_text.clone().unwrap_or_default();
        log::info!(
            "text texture {i} ({} in \"{}\"): {text:?}",
            tt.def.variable,
            tt.def.font
        );
        let (w, h) = (tt.def.width.max(1) as u32, tt.def.height.max(1) as u32);
        if tt.atlas.is_some() {
            let _ = image::save_buffer(
                dir.join(format!("texttex_{i}.png")),
                &tt.image(&text),
                w,
                h,
                image::ColorType::Rgba8,
            );
        }
    }
}

pub(crate) fn parse_input_script() -> Vec<(f32, String)> {
    let Ok(v) = omsi_cfg::env::var("OMSI_INPUT") else {
        return Vec::new();
    };
    let mut out: Vec<(f32, String)> = v
        .split(';')
        .filter_map(|item| {
            let item = item.trim();
            let rest = item.strip_prefix("t=")?;
            let (t, cmd) = rest.split_once(' ')?;
            Some((t.trim().parse::<f32>().ok()?, cmd.trim().to_string()))
        })
        .collect();
    out.sort_by(|a, b| a.0.total_cmp(&b.0));
    out
}

impl App {
    pub(crate) fn script_key(name: &str) -> Option<KeyCode> {
        use KeyCode::*;
        pub(crate) const LETTERS: [KeyCode; 26] = [
            KeyA, KeyB, KeyC, KeyD, KeyE, KeyF, KeyG, KeyH, KeyI, KeyJ, KeyK, KeyL, KeyM, KeyN,
            KeyO, KeyP, KeyQ, KeyR, KeyS, KeyT, KeyU, KeyV, KeyW, KeyX, KeyY, KeyZ,
        ];
        pub(crate) const DIGITS: [KeyCode; 10] = [
            Digit0, Digit1, Digit2, Digit3, Digit4, Digit5, Digit6, Digit7, Digit8, Digit9,
        ];
        pub(crate) const FKEYS: [KeyCode; 12] = [F1, F2, F3, F4, F5, F6, F7, F8, F9, F10, F11, F12];
        let b = name.as_bytes();
        if b.len() == 1 && b[0].is_ascii_alphabetic() {
            return Some(LETTERS[(b[0].to_ascii_uppercase() - b'A') as usize]);
        }
        if b.len() == 1 && b[0].is_ascii_digit() {
            return Some(DIGITS[(b[0] - b'0') as usize]);
        }
        if let Some(n) = name.strip_prefix('F').and_then(|n| n.parse::<usize>().ok()) {
            return FKEYS.get(n.wrapping_sub(1)).copied();
        }
        Some(match name {
            "Shift" => ShiftLeft,
            "Ctrl" => ControlLeft,
            "Alt" => AltLeft,
            "." => Period,
            "," => Comma,
            "Up" => ArrowUp,
            "Down" => ArrowDown,
            "Left" => ArrowLeft,
            "Right" => ArrowRight,
            "Enter" => Enter,
            "Escape" => Escape,
            "Backspace" => Backspace,
            "Space" => Space,
            "PageUp" => PageUp,
            "PageDown" => PageDown,
            "Insert" => Insert,
            "Home" => Home,
            "End" => End,
            "Delete" => Delete,
            "[" => BracketLeft,
            "]" => BracketRight,
            _ => return None,
        })
    }

    pub(crate) fn run_input_script(&mut self, event_loop: &ActiveEventLoop) {
        if self.input_script.is_empty() {
            return;
        }
        let t = self.started.elapsed().as_secs_f32();
        let scale = self
            .window
            .as_ref()
            .map(|w| w.scale_factor() as f32)
            .unwrap_or(1.0);
        while let Some((at, cmd)) = self.input_script.first().cloned() {
            if t < at {
                break;
            }
            self.input_script.remove(0);
            let mut parts = cmd.split_whitespace();
            let verb = parts.next().unwrap_or("");
            let arg = parts.next().unwrap_or("");
            let xy = || -> (f32, f32) {
                let mut it = arg.split(',').filter_map(|v| v.trim().parse::<f32>().ok());
                (it.next().unwrap_or(0.0), it.next().unwrap_or(0.0))
            };
            log::info!("input script t={t:.1}: {cmd}");
            match verb {
                "move" => {
                    let (x, y) = xy();
                    self.on_cursor(x * scale, y * scale);
                }
                "weather" => self.next_weather(),
                "rawmouse" => {
                    let (dx, _) = xy();
                    if self.mouse_drive && self.game_menu.is_none() {
                        self.mouse_past_edge(dx);
                    }
                }
                "drag" => {
                    let (dx, dy) = xy();
                    let (x, y) = self.cursor;
                    self.on_cursor(x + dx * scale, y + dy * scale);
                }
                "touch" => {
                    let rest = parts.next().unwrap_or("");
                    let mut it = rest.split(',').filter_map(|v| v.trim().parse::<f32>().ok());
                    let (x, y) = (it.next().unwrap_or(0.0), it.next().unwrap_or(0.0));
                    let id = it.next().unwrap_or(0.0) as u64;
                    self.script_touch(event_loop, arg, x * scale, y * scale, id);
                }
                "look" => self.look = xy(),
                "orbit" => self.orbit = xy().0.clamp(ORBIT_MIN, ORBIT_MAX),
                "set" => {
                    if let (Some((k, v)), Some(p)) = (arg.split_once('='), self.player.as_mut()) {
                        let ok = p.vehicle.set_var(k.trim(), v.trim().parse().unwrap_or(0.0));
                        log::info!("input script: set {k} -> {ok}");
                    }
                }
                "trigger" => {
                    if let Some(p) = self.player.as_mut() {
                        let ok = p.vehicle.trigger(arg);
                        log::info!("input script: trigger {arg} -> {ok}");
                    }
                }
                "wheel" => {
                    let n = xy().0;
                    if self.editor.is_some() && self.game_menu.is_none() {
                        self.editor_wheel(n);
                    } else if self.placing.is_some() && self.game_menu.is_none() {
                        self.placing_wheel(n);
                    } else if self.game_menu.is_some() {
                        self.menu_wheel(n);
                    } else {
                        self.wheel(n);
                    }
                    log::info!(
                        "input script: wheel {n}: menu line {:?}, chooser {:?}, placing heading {:?}",
                        self.game_menu,
                        self.chooser,
                        self.placing.as_ref().map(|p| p.heading)
                    );
                }
                "click" => {
                    if self.placing.is_some() && self.game_menu.is_none() {
                        self.placing_click();
                    } else if self.game_menu.is_some() {
                        self.left_button(event_loop, true);
                        self.left_button(event_loop, false);
                    } else {
                        self.on_left(true);
                        self.on_left(false);
                    }
                    log::info!(
                        "input script: click: placing {:?}, placed at {:?}",
                        self.placing.as_ref().map(|p| (p.at, p.blocked)),
                        self.placed
                            .last()
                            .map(|q| (q.vehicle.position, q.vehicle.heading))
                    );
                }
                "both" => {
                    if arg == "down" {
                        self.buttons_held = (true, true);
                        let started = self.start_both_drag();
                        log::info!("input script: both buttons: zoom drag {started}");
                    } else {
                        self.buttons_held = (false, false);
                        self.both_drag = None;
                        log::info!(
                            "input script: both buttons up: zoom {:?}, orbit {:.1}",
                            self.view_zoom.get(&self.view),
                            self.orbit
                        );
                    }
                }
                "right" => {
                    self.on_right(arg == "down");
                    log::info!(
                        "input script: right button {arg}: zoom drag {}, look {}, zoom {:?}, orbit {:.1}",
                        self.both_drag.is_some(),
                        self.mouse_look,
                        self.view_zoom.get(&self.view),
                        self.orbit
                    );
                }
                "press" => self.on_left(true),
                "release" => self.on_left(false),
                "type" => {
                    let text = cmd.split_once(' ').map(|x| x.1).unwrap_or("");
                    if lan::chat_open(&self.remotes) {
                        lan::chat_type(&mut self.remotes, text);
                    } else {
                        log::warn!("input script: the chat line is not open");
                    }
                }
                "turn" => {
                    let (dx, dy) = xy();
                    self.look_by(dx, dy);
                }
                "key" | "keydown" | "keyup" => {
                    let Some(code) = Self::script_key(arg) else {
                        log::warn!("input script: unknown key {arg}");
                        continue;
                    };
                    if verb != "keyup" {
                        self.on_key(event_loop, code, true, false);
                    }
                    if verb != "keydown" {
                        self.on_key(event_loop, code, false, false);
                    }
                }
                "log" if arg == "pose" => {
                    let bus = self.player.as_ref().map(|p| {
                        (
                            p.vehicle.position,
                            p.vehicle
                                .ground
                                .as_ref()
                                .and_then(|g| g(p.vehicle.position.x, p.vehicle.position.y)),
                        )
                    });
                    let tiles = self
                        .world
                        .as_ref()
                        .map(|w| w.loaded_tiles().len())
                        .unwrap_or(0);
                    log::info!(
                        "input script: bus at {:?} (ground {:?}), camera at {:?}, {tiles} tiles loaded",
                        bus.map(|b| b.0),
                        bus.and_then(|b| b.1),
                        self.camera.as_ref().map(|c| c.position)
                    );
                    if let (Some(p), Some(c)) = (self.player.as_ref(), self.camera.as_ref()) {
                        let d = c.position - p.vehicle.position;
                        let h = p.vehicle.heading.to_radians();
                        let (fwd, right) = (
                            glam::DVec2::new(h.sin(), h.cos()),
                            glam::DVec2::new(h.cos(), -h.sin()),
                        );
                        log::info!(
                            "input script: view {} camera in the bus ({:.2}, {:.2}, {:.2}), on foot {:?}",
                            self.view,
                            d.truncate().dot(right),
                            d.truncate().dot(fwd),
                            d.z,
                            self.on_foot.as_ref().map(|f| f.pos)
                        );
                    }
                }
                "log" if arg == "mouse" => {
                    log::info!(
                        "input script: mouse steering {} look {} menu {:?} paused {} focused {} steer {:.3}",
                        self.mouse_drive,
                        self.mouse_look,
                        self.game_menu,
                        self.paused,
                        self.window_focused,
                        self.mouse_steer.0
                    );
                }
                "log" => {
                    let v = self
                        .player
                        .as_ref()
                        .map(|p| (p.vehicle.var(arg), p.vehicle.str_var(arg)));
                    let names = describe::names(&self.args.root, &self.settings.language);
                    let shown = self
                        .hover
                        .as_deref()
                        .map(|h| names.control(h))
                        .or_else(|| self.hover_part.as_deref().map(|p| names.part(p)));
                    log::info!(
                        "input script: {arg} = {:?}  hover {:?} / {:?} shown as {:?}",
                        v,
                        self.hover,
                        self.hover_part,
                        shown
                    );
                }
                "menu" => {
                    if let Some(n) = arg
                        .strip_prefix("pick:")
                        .and_then(|n| n.parse::<usize>().ok())
                    {
                        self.chooser_pick(n);
                    } else {
                        if self.game_menu.is_none() {
                            self.open_game_menu();
                        }
                        match self.game_menu_items().iter().position(|m| m.0 == arg) {
                            Some(k) => self.menu_choose(event_loop, k),
                            None => {
                                if !self.page_action(arg) {
                                    log::warn!("input script: no menu line {arg}");
                                }
                            }
                        }
                    }
                    let riders = self.humans.as_ref().map(|h| {
                        (
                            h.people_in(crate::humans::BusId::Player),
                            self.placed
                                .iter()
                                .map(|q| {
                                    h.people_in(crate::humans::BusId::Ai(
                                        crate::humans::placed_bus_id(q.uid),
                                    ))
                                })
                                .collect::<Vec<_>>(),
                        )
                    });
                    log::info!(
                        "input script: menu {arg}: player {:?}, on foot {:?}, placed {}, people in the bus / the placed ones {:?}",
                        self.player.as_ref().map(|p| p.vehicle.position),
                        self.on_foot.as_ref().map(|f| f.pos),
                        self.placed.len(),
                        riders
                    );
                }
                "dumptex" => {
                    if let Some(p) = self.player.as_ref() {
                        dump_display_textures(&p.vehicle, Path::new(arg));
                    }
                }
                _ => log::warn!("input script: unknown command {cmd}"),
            }
        }
    }
}
