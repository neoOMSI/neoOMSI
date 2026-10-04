//! Keyboard: the window's key handler, OMSI's global key actions, gears and mouse steering.

use super::*;

pub(crate) fn gate_gear_var(program: &omsi_script::Program) -> Option<String> {
    for known in ["antrieb_getr_aktugang", "antrieb_getr_gang"] {
        if program.var(known).is_some() {
            return Some(known.to_string());
        }
    }
    let mut names: Vec<String> = program
        .var_names()
        .into_iter()
        .filter(|n| n.contains("gang") || n.contains("gear"))
        .filter(|n| {
            let by = program.triggers_setting(n);
            by.iter().any(|t| t == "kw_s_1") && by.iter().any(|t| t == "kw_s_2")
        })
        .collect();
    names.sort_by_key(|n| (n.len(), n.clone()));
    names.into_iter().next()
}

pub(crate) fn is_internal_key_action(action: &str) -> bool {
    matches!(
        action,
        "open_mainmenue"
            | "tutorial_next"
            | "tutorial_back"
            | "tutorial_toggle"
            | "navigator_close"
            | "menu_exit"
            | "vr_nav_confirm"
            | "vr_nav_cancel"
            | "vr_nav_reset"
    )
}

pub(crate) fn is_convenience_action(action: &str) -> bool {
    matches!(
        action,
        "indicator_left" | "indicator_right" | "indicator_hazard" | "saloon_lights"
    )
}

impl App {
    pub(crate) fn ctrl_held(&self) -> bool {
        self.keys
            .iter()
            .any(|k| matches!(k, KeyCode::ControlLeft | KeyCode::ControlRight))
    }

    pub(crate) fn alt_held(&self) -> bool {
        self.keys
            .iter()
            .any(|k| matches!(k, KeyCode::AltLeft | KeyCode::AltRight))
    }

    pub(crate) fn bound_actions(&self, code: KeyCode) -> Vec<String> {
        let Some(scan) = keys::dik_code(code) else {
            return Vec::new();
        };
        let m = omsi_content::input::chord(
            shift_held_now(&self.keys),
            self.ctrl_held(),
            self.alt_held(),
        );
        self.game_keys
            .iter()
            .filter(|b| b.scan_code == scan && b.matches(m))
            .map(|b| b.action.clone())
            .collect()
    }

    // `<trigger>` on key down, `<trigger>_off` on key up: door buttons of automatic-door buses are push buttons
    pub(crate) fn door_key(&mut self, n: usize, code: KeyCode) {
        if self.view == "free" || keys::dik_code(code).is_some_and(|s| self.own_shift.contains(&s))
        {
            return;
        }
        let Some(p) = self.player.as_mut() else {
            return;
        };
        let groups = crate::player::door_keys(&p.vehicle.ty);
        let Some(group) = n.checked_sub(1).and_then(|i| groups.get(i)) else {
            return;
        };
        let fire = crate::player::door_group_to_fire(&mut p.vehicle, group);
        log::info!("door key {n}: {}", fire.join(" + "));
        if group.len() == 1 && group[0] == "bus_dooraft" {
            let v = &mut p.vehicle;
            let release_on = v.var("bremse_halte_sw").is_some_and(|x| x > 0.5);
            let open = v.var("doorTarget_23").is_some_and(|x| x > 0.5);
            if release_on && open && v.var("doorAftLastOpen").is_some() {
                v.set_var("haltewunsch", 0.0);
                v.set_var("doorAftLastOpen", 1000.0);
            }
        }
        for name in &fire {
            p.vehicle.trigger(name);
        }
        self.door_key_triggers.insert(code, fire);
    }

    pub(crate) fn saloon_lights(&mut self) {
        if let Some(p) = self.player.as_mut() {
            let msg = p.toggle_saloon_lights();
            self.service_msg = Some((msg, 3.0));
        }
    }

    pub(crate) fn bus_startup(&mut self) {
        if let Some(p) = self.player.as_mut() {
            let msg = p.start_up();
            self.service_msg = Some((msg, 6.0));
            if let Some(d) = self.duty.as_ref() {
                let (trip, stop) = d.trip_for_ibis();
                if p.auto_ibis {
                    p.set_duty_destination(trip, stop);
                }
            }
        }
    }

    pub(crate) fn show_position(&mut self) {
        let Some(cam) = self.camera.as_ref() else {
            return;
        };
        let ts = omsi_map::tile_size();
        let (tx, ty) = (
            (cam.position.x / ts).floor() as i32,
            (cam.position.y / ts).floor() as i32,
        );
        let line = format!(
            "Position {:.0}, {:.0}, {:.1}   tile {tx}_{ty}   heading {:.0} deg",
            cam.position.x, cam.position.y, cam.position.z, cam.yaw
        );
        log::info!(
            "{line}  (--cam {:.0},{:.0},{:.0},{:.0},{:.0})",
            cam.position.x,
            cam.position.y,
            cam.position.z,
            cam.yaw,
            cam.pitch
        );
        self.service_msg = Some((line, 12.0));
    }

    pub(crate) fn save_personnel(&mut self) {
        let line = self.career.summary();
        if self.career.path.is_some() {
            if let Err(e) = self.career.save() {
                log::warn!("writing the personnel file: {e}");
            }
        } else {
            log::info!("this run: {line}");
        }
        self.service_msg = Some((line, 8.0));
    }
}

#[cfg(test)]
mod gear_lever_tests {
    /// The stock cars' gates keep the gear in `antrieb_getr_gang` (#866).
    #[test]
    fn the_gear_is_read_where_the_gates_store_it() {
        let program = |vars: &str, osc: &str| {
            let dir = std::env::temp_dir().join(format!(
                "omsi_gates_{}_{}",
                std::process::id(),
                vars.len()
            ));
            std::fs::create_dir_all(&dir).unwrap();
            let (vl, sc) = (dir.join("varlist.txt"), dir.join("antrieb.osc"));
            std::fs::write(&vl, vars).unwrap();
            std::fs::write(&sc, osc).unwrap();
            let p = omsi_script::compile(&omsi_script::CompileInput {
                varlists: vec![vl],
                scripts: vec![sc],
                ..Default::default()
            });
            let _ = std::fs::remove_dir_all(&dir);
            p
        };
        let stock = program(
            "antrieb_getr_gang\n",
            "{trigger:kw_s_1_fest}\n{trigger:kw_s_1}\n1 (S.L.antrieb_getr_gang)\n{end}\n{end}\n{trigger:kw_s_2}\n2 (S.L.antrieb_getr_gang)\n{end}\n",
        );
        assert_eq!(
            super::gate_gear_var(&stock).as_deref(),
            Some("antrieb_getr_gang")
        );
        let own = program(
            "lever_moved\nmy_gear\n",
            "{trigger:kw_s_1}\n1 (S.L.lever_moved)\n1 (S.L.my_gear)\n{end}\n{trigger:kw_s_2}\n1 (S.L.lever_moved)\n2 (S.L.my_gear)\n{end}\n",
        );
        assert_eq!(super::gate_gear_var(&own).as_deref(), Some("my_gear"));
    }
}

pub(crate) fn flies_free_camera(code: KeyCode) -> bool {
    matches!(
        code,
        KeyCode::KeyW
            | KeyCode::KeyA
            | KeyCode::KeyS
            | KeyCode::KeyD
            | KeyCode::KeyQ
            | KeyCode::KeyE
            | KeyCode::Space
            | KeyCode::ShiftLeft
            | KeyCode::ArrowLeft
            | KeyCode::ArrowRight
            | KeyCode::ArrowUp
            | KeyCode::ArrowDown
    )
}

impl App {
    pub(crate) fn on_key(
        &mut self,
        event_loop: &ActiveEventLoop,
        code: KeyCode,
        pressed: bool,
        repeat: bool,
    ) {
        if self.vr_nav_edit.is_some() {
            if !pressed {
                self.keys.remove(&code);
            }
            if matches!(
                code,
                KeyCode::ControlLeft
                    | KeyCode::ControlRight
                    | KeyCode::ShiftLeft
                    | KeyCode::ShiftRight
            ) && pressed
            {
                self.keys.insert(code);
            }
            if pressed && !repeat {
                let acts = self.bound_actions(code);
                if acts
                    .iter()
                    .any(|a| a == "vr_nav_confirm" || a == "vr_nav_cancel")
                {
                    self.finish_vr_nav_edit();
                } else if acts.iter().any(|a| a == "vr_nav_reset") {
                    self.vr_nav_adjust("reset", 1.0);
                }
            }
            return;
        }
        if pressed
            && self
                .bound_actions(code)
                .iter()
                .any(|a| a == "navigator_close")
        {
            if let Some(n) = self.navigator.as_mut().filter(|n| n.map_open()) {
                n.toggle_map();
                return;
            }
        }
        let event_key = PhysicalKey::Code(code);
        if let (Some(m), PhysicalKey::Code(code)) = (self.menu.as_mut(), event_key) {
            if pressed {
                if self
                    .game_keys
                    .iter()
                    .any(|b| b.action == "menu_exit" && Some(b.scan_code) == keys::dik_code(code))
                {
                    crate::platform::exit(event_loop);
                }
                m.key(code);
                if m.start {
                    self.args.map = m
                        .maps
                        .get(m.map)
                        .map(|x| x.1.clone())
                        .unwrap_or(self.args.map.clone());
                    self.args.bus = m.vehicles.get(m.vehicle).map(|x| x.1.clone());
                    self.args.time = format!("{:02}:00", m.hour);
                    self.args.traffic = m.traffic;
                    self.args.passengers = m.passengers;
                    self.args.schedule = m.schedule;
                    self.args.day_of_year = Some(m.day);
                    if m.weather > 0 {
                        self.args.weather = m.weathers.get(m.weather).map(|w| w.1.clone());
                    }
                    if m.situation > 0 {
                        self.args.situation = m.situations.get(m.situation).map(|s| s.1.clone());
                        if let Err(e) = apply_situation(&mut self.args) {
                            log::error!("{e:#}");
                        }
                    }
                    self.menu = None;
                    self.hud = None;
                    self.load_world_now(event_loop);
                }
            }
            return;
        }
        if let PhysicalKey::Code(code) = event_key {
            let pressed = pressed;
            // LAN chat: its keys (`chat_open`, '/' or '`', and `chat_toggle`, V, in keyboard.cfg's
            // [game]: the player can move them, #130) open the line and show or hide the
            // chat, and while the line is open the keys are its own
            if let Some(l) = self.lan.as_mut() {
                let held =
                    |a: KeyCode, b: KeyCode| self.keys.contains(&a) || self.keys.contains(&b);
                let chord = omsi_content::input::chord(
                    held(KeyCode::ShiftLeft, KeyCode::ShiftRight),
                    held(KeyCode::ControlLeft, KeyCode::ControlRight),
                    held(KeyCode::AltLeft, KeyCode::AltRight),
                );
                let bound = if held(KeyCode::SuperLeft, KeyCode::SuperRight) {
                    None
                } else {
                    keys::dik_code(code).and_then(|scan| {
                        self.game_keys
                            .iter()
                            .find(|b| {
                                b.scan_code == scan
                                    && b.matches(chord)
                                    && b.action.to_ascii_lowercase().starts_with("chat_")
                            })
                            .map(|b| b.action.clone())
                    })
                };
                if lan::chat_key(
                    l,
                    &mut self.remotes,
                    code,
                    pressed,
                    repeat,
                    bound.as_deref(),
                ) {
                    return;
                }
            }
            if pressed && !repeat {
                self.keys.insert(code);
            } else if !pressed {
                self.keys.remove(&code);
            }
            if pressed
                && !repeat
                && self
                    .bound_actions(code)
                    .iter()
                    .any(|a| a == "toggle_fullscreen")
            {
                self.game_action("toggle_fullscreen");
                return;
            }
            #[cfg(windows)]
            if pressed && !repeat && (self.vr.is_some() || self.settings.vr_requested()) {
                let modifier = omsi_content::input::chord(
                    self.keys.contains(&KeyCode::ShiftLeft)
                        || self.keys.contains(&KeyCode::ShiftRight),
                    self.keys.contains(&KeyCode::ControlLeft)
                        || self.keys.contains(&KeyCode::ControlRight),
                    self.keys.contains(&KeyCode::AltLeft) || self.keys.contains(&KeyCode::AltRight),
                );
                let action = keys::dik_code(code).and_then(|scan| {
                    self.game_keys
                        .iter()
                        .find(|b| {
                            b.scan_code == scan
                                && b.matches(modifier)
                                && b.action.starts_with("vr_")
                        })
                        .map(|b| b.action.clone())
                });
                if let Some(action) = action {
                    if self.game_action(&action) {
                        return;
                    }
                }
            }
            if !pressed {
                let released: Vec<Vec<String>> = if self.door_key_triggers.contains_key(&code) {
                    self.door_key_triggers.remove(&code).into_iter().collect()
                } else if matches!(code, KeyCode::ShiftLeft | KeyCode::ShiftRight) {
                    self.door_key_triggers.drain().map(|(_, g)| g).collect()
                } else {
                    Vec::new()
                };
                if let Some(p) = self.player.as_mut() {
                    for name in released.iter().flatten() {
                        let off = format!("{name}_off");
                        if p.vehicle.ty.program.trigger(&off).is_some() {
                            p.vehicle.trigger(&off);
                        }
                    }
                }
            }
            if self.screenshot_mode.is_some() && pressed && !repeat && code == KeyCode::Escape {
                self.leave_screenshot_mode();
                return;
            }
            if self.game_menu.is_none() && self.placing_key(code, pressed) {
                return;
            }
            if self.game_menu.is_some() {
                if pressed && !repeat {
                    self.menu_key(event_loop, code);
                }
                return;
            }
            if pressed && self.editor.is_some() && self.editor_key(code) {
                return;
            }
            if pressed
                && !repeat
                && self
                    .bound_actions(code)
                    .iter()
                    .any(|a| a == "open_mainmenue")
            {
                self.open_game_menu();
                return;
            }
            if pressed && self.tutorial.is_some() {
                let acts = self.bound_actions(code);
                let lan = self.lan.is_some();
                if let Some(t) = self.tutorial.as_mut() {
                    if acts.iter().any(|a| a == "tutorial_next") && !t.hidden && !lan {
                        t.next();
                        return;
                    }
                    if acts.iter().any(|a| a == "tutorial_back") && !t.hidden {
                        t.back();
                        return;
                    }
                    if acts.iter().any(|a| a == "tutorial_toggle") {
                        t.hidden = !t.hidden;
                        return;
                    }
                }
            }
            let ctrl = self.keys.contains(&KeyCode::ControlLeft)
                || self.keys.contains(&KeyCode::ControlRight);
            let alt =
                self.keys.contains(&KeyCode::AltLeft) || self.keys.contains(&KeyCode::AltRight);
            let shift_now =
                self.keys.contains(&KeyCode::ShiftLeft) || self.keys.contains(&KeyCode::ShiftRight);
            if self.foot_key(code, pressed, repeat, ctrl, shift_now) {
                return;
            }
            if pressed && !repeat {
                let m = omsi_content::input::chord(shift_now, ctrl, alt);
                let own = keys::dik_code(code).is_some_and(|s| self.own_keys.contains(&s));
                let ours = self.args.drive_keys != "omsi"
                    && m == 0
                    && !own
                    && fallback_action(code, &self.args.drive_keys).is_some();
                // (the keys that fly the camera are the camera's, unmodified: S, OMSI's
                // view_toggle_viewpoint, threw the free camera back to the driver's view,
                // and with no bus of one's own every view flies - #868; a chord such as
                // Ctrl+S, OMSI's quicksave, stays a [game] key)
                let flying = m == 0
                    && flies_free_camera(code)
                    && (self.view == "free" || (self.player.is_none() && self.on_foot.is_none()));
                if let Some(scan) = keys::dik_code(code).filter(|_| !ours && !flying) {
                    let actions: Vec<String> = self
                        .game_keys
                        .iter()
                        .filter(|b| {
                            b.scan_code == scan
                                && b.matches(m)
                                && !b.action.starts_with("vr_")
                                && !(own && is_convenience_action(&b.action))
                                && !(alt && b.action.starts_with("view_interiorcam_"))
                        })
                        .map(|b| b.action.clone())
                        .collect();
                    // (a key bound in [game] and in [vehicles] does both, as in Omsi.exe: the
                    // parking brake put on Space, the stock view_reset_all_directions key,
                    // reset the view and never reached the bus - #745)
                    let vehicle_too = self.player.as_ref().is_some_and(|p| {
                        p.bindings
                            .iter()
                            .any(|b| b.scan_code == scan && b.matches(m))
                    });
                    for a in actions {
                        // OMSI's `exit` (Ctrl+Q, or what the player put it on): the game ends as
                        // the menu's Quit ends it (#817)
                        if a == "exit" {
                            self.finish_vr_nav_edit();
                            self.game_menu = None;
                            self.finish_session();
                            crate::platform::exit(event_loop);
                            return;
                        }
                        if is_internal_key_action(&a) {
                            continue;
                        }
                        if let Some(n) = a
                            .strip_prefix("doorkey_")
                            .and_then(|n| n.parse::<usize>().ok())
                        {
                            self.door_key(n, code);
                        } else if self.game_action(&a) && !vehicle_too {
                            return;
                        }
                    }
                }
            }
            let own = keys::dik_code(code).is_some_and(|s| self.own_keys.contains(&s));
            let wheel = self
                .controllers
                .as_ref()
                .is_some_and(|c| c.wheel_steering());
            let wasd = if own {
                "omsi"
            } else if wheel {
                match self.args.drive_keys.as_str() {
                    "arrows" | "omsi" => "omsi",
                    _ => "wasd",
                }
            } else {
                self.args.drive_keys.as_str()
            };
            let shift_held =
                self.keys.contains(&KeyCode::ShiftLeft) || self.keys.contains(&KeyCode::ShiftRight);
            if let Some(p) = self.player.as_mut() {
                let ctrl_alt_held = (self.keys.contains(&KeyCode::ControlLeft)
                    || self.keys.contains(&KeyCode::ControlRight))
                    && (self.keys.contains(&KeyCode::AltLeft)
                        || self.keys.contains(&KeyCode::AltRight));
                if self.view != "free" && !repeat && !shift_held && !(ctrl_alt_held && pressed) {
                    if let Some(a) = fallback_action(code, wasd) {
                        p.axes.set(a, pressed);
                    }
                }
            }
            let shift =
                self.keys.contains(&KeyCode::ShiftLeft) || self.keys.contains(&KeyCode::ShiftRight);
            let covers_vehicle_key = fallback_action(code, wasd).is_some() && self.view != "free";
            let driving_key = covers_vehicle_key && !shift;
            let fly_key = self.view == "free" && flies_free_camera(code);
            if let (Some(p), Some(scan)) = (
                self.player.as_mut(),
                keys::dik_code(code).filter(|_| !driving_key && !fly_key),
            ) {
                if !repeat {
                    let ctrl_or_alt = self.keys.contains(&KeyCode::ControlLeft)
                        || self.keys.contains(&KeyCode::ControlRight)
                        || self.keys.contains(&KeyCode::AltLeft)
                        || self.keys.contains(&KeyCode::AltRight);
                    let m = if covers_vehicle_key && !ctrl_or_alt {
                        0
                    } else {
                        omsi_content::input::chord(
                            shift,
                            self.keys.contains(&KeyCode::ControlLeft)
                                || self.keys.contains(&KeyCode::ControlRight),
                            self.keys.contains(&KeyCode::AltLeft)
                                || self.keys.contains(&KeyCode::AltRight),
                        )
                    };
                    p.key(scan, m, pressed);
                }
            }
        }
    }

    pub(crate) fn blinker(&mut self, want: u8) {
        if let Some(player) = self.player.as_mut() {
            player.toggle_indicator(want);
        }
    }

    #[cfg(windows)]
    pub(crate) fn vr_action(&mut self, name: &str) -> bool {
        if name == "vr_toggle_mode" {
            self.vr_zoom_active = false;
            if !self.settings.vr_requested() {
                return false;
            }
            if self.vr.is_some() {
                self.vr = None;
                self.service_msg = Some(("Desktop mode".into(), 2.0));
            } else if let Some(renderer) = self.renderer.as_ref() {
                match crate::openxr::Vr::new(
                    renderer,
                    self.settings.vr_scale,
                    self.settings.vr_desktop_mirror,
                ) {
                    Ok(vr) => {
                        self.vr = Some(vr);
                        self.service_msg = Some(("VR mode".into(), 2.0));
                    }
                    Err(e) => {
                        log::error!("OpenXR could not restart: {e:#}");
                        self.service_msg =
                            Some((format!("{}: {e}", omsi_ui::tr("Could not start VR")), 5.0));
                    }
                }
            }
            self.hover_key = None;
            if matches!(
                self.list_kind,
                Some(crate::game_lists::ListKind::Options(_))
            ) {
                self.refresh_list();
            }
            return true;
        }
        if self.vr.is_none() {
            return false;
        }
        match name {
            "vr_recenter" => {
                self.vr.as_mut().unwrap().recenter();
                self.look = (0.0, 0.0);
                self.service_msg = Some(("VR view recentered".into(), 2.0));
            }
            "vr_toggle_desktop_mirror" => {
                let visible = self.vr.as_mut().unwrap().toggle_desktop_mirror();
                self.settings.vr_desktop_mirror = visible;
                self.service_msg = Some((
                    if visible {
                        "Desktop VR mirror on"
                    } else {
                        "Desktop VR mirror off"
                    }
                    .into(),
                    2.0,
                ));
            }
            "vr_toggle_navigator" => self.vr_nav_adjust("enabled", 1.0),
            "vr_position_navigator" => self.start_vr_nav_edit(),
            _ => return false,
        }
        true
    }

    pub(crate) fn game_action(&mut self, name: &str) -> bool {
        #[cfg(windows)]
        if self.vr_action(name) {
            return true;
        }
        match name {
            "sim_pause" => self.toggle_pause(),
            "quicksave" => self.quick_save(),
            "view_set_ego" => {
                if let (Some(cam), Some(p)) = (self.camera.as_mut(), self.player.as_ref()) {
                    if self.view != "free" {
                        let h = (p.vehicle.heading as f32 - 90.0).to_radians();
                        cam.position = p.vehicle.position
                            + glam::DVec3::new(h.sin() as f64, h.cos() as f64, 0.0) * 2.5;
                        cam.yaw = p.vehicle.heading as f32;
                        cam.pitch = 0.0;
                    }
                }
                self.view = "free".into();
                self.ego = true;
                self.service_msg = Some(("On foot: W A S D walk, Shift runs, right mouse button looks (F1 back to the bus)".into(), 5.0));
            }
            "view_set_driver" => self.view = "driver".into(),
            "view_set_passenger" => self.view = "pax".into(),
            "view_set_outside" => self.view = "outside".into(),
            "view_set_map" => {
                if self.view != "free" {
                    if let (Some(cam), Some(p)) = (self.camera.as_mut(), self.player.as_ref()) {
                        let h = (p.vehicle.heading as f32).to_radians();
                        cam.position = p.vehicle.position
                            + glam::DVec3::new(
                                -(h.sin() as f64) * 25.0,
                                -(h.cos() as f64) * 25.0,
                                30.0,
                            );
                        cam.yaw = p.vehicle.heading as f32;
                        cam.pitch = -45.0;
                    }
                }
                self.view = "free".into();
                self.ego = false;
            }
            "view_set_schedule" | "view_set_ticketselling" => {
                let schedule = name == "view_set_schedule";
                if schedule {
                    self.timetable = !self.timetable;
                }
                if let Some(p) = self.player.as_mut() {
                    let def = &p.vehicle.ty.def;
                    let cam = if schedule {
                        def.view_schedule
                    } else {
                        def.view_ticketselling
                    };
                    let n = def.cameras_driver.len().max(1);
                    if let Some(c) = cam
                        .filter(|c| *c < def.cameras_driver.len())
                        .map(|c| (c + n - def.camera_std % n) % n)
                    {
                        let back = p.cam_before_special.take();
                        if self.view == "driver" && p.cam_choice.0 == c {
                            p.cam_choice.0 = back.unwrap_or(0);
                        } else {
                            p.cam_before_special = Some(p.cam_choice.0);
                            p.cam_choice.0 = c;
                            self.view = "driver".into();
                        }
                        self.sync_view_look();
                    } else if !schedule {
                        self.service_msg = Some(("This bus has no ticket desk camera".into(), 3.0));
                    }
                }
            }
            "view_toggle_informationdisplay" => self.info_bar = !self.info_bar,
            // (Omsi.exe's camera reset, 0x7edde4, puts back the field of view with the
            // direction: the zoom goes as well, #244)
            "view_reset_direction" => {
                if self.view == "driver" {
                    self.cam_blend.resetting = true;
                    self.cam_blend.reset_zoom = self.view_zoom.get(&self.view).copied();
                }
                self.look = (0.0, 0.0);
                self.view_zoom.remove(&self.view);
                #[cfg(windows)]
                if let Some(vr) = self.vr.as_mut() {
                    vr.recenter();
                }
            }
            "view_reset_all_directions" => {
                if self.view == "driver" {
                    self.cam_blend.resetting = true;
                    self.cam_blend.reset_zoom = self.view_zoom.get(&self.view).copied();
                }
                self.look = (0.0, 0.0);
                self.view_looks.clear();
                self.view_zoom.clear();
                self.orbit = ORBIT_DEFAULT;
                if let Some(p) = self.player.as_mut() {
                    p.cam_choice = (0, 0);
                }
            }
            // the next (or the previous) view mode, driver - passenger - outside - map and
            // round again; nothing on foot (Omsi.exe 0x706278 @0x70634a: (mode + 1) and 3,
            // @0x706392 the inverse)
            "view_toggle_viewpoint" | "view_toggle_viewpoint_inverse" => {
                if self.ego {
                    return true;
                }
                let mode = match self.view.as_str() {
                    "driver" => 0,
                    "pax" => 1,
                    "outside" => 2,
                    _ => 3,
                };
                let next = if name == "view_toggle_viewpoint" {
                    (mode + 1) % 4
                } else {
                    (mode + 3) % 4
                };
                return self.game_action(
                    [
                        "view_set_driver",
                        "view_set_passenger",
                        "view_set_outside",
                        "view_set_map",
                    ][next],
                );
            }
            "view_interiorcam_plus" | "view_interiorcam_minus" => {
                let Some(p) = self.player.as_mut() else {
                    return true;
                };
                if !matches!(self.view.as_str(), "driver" | "pax") {
                    return true;
                }
                let (count, pax) = if self.view == "pax" {
                    (p.pax_camera_count(), true)
                } else {
                    (p.driver_camera_count(), false)
                };
                if count > 1 {
                    let c = if pax {
                        &mut p.cam_choice.1
                    } else {
                        &mut p.cam_choice.0
                    };
                    *c = if name == "view_interiorcam_minus" {
                        (*c + count - 1) % count
                    } else {
                        (*c + 1) % count
                    };
                    let n = *c + 1;
                    self.sync_view_look();
                    self.service_msg = Some((
                        format!(
                            "{} camera {n} of {count}",
                            if pax { "Passenger" } else { "Driver" }
                        ),
                        2.0,
                    ));
                }
            }
            "toggel_mouse_ctrl" => {
                self.set_mouse_drive(!self.mouse_drive);
                let msg = if self.mouse_drive {
                    "Mouse steering on: across steers, up is the throttle, down the brake (O turns it off)"
                } else {
                    "Mouse steering off"
                };
                self.service_msg = Some((msg.into(), 4.0));
            }
            "toggle_fullscreen" => {
                if let Some(win) = self.window.as_ref() {
                    win.set_fullscreen(if win.fullscreen().is_some() {
                        None
                    } else {
                        Some(winit::window::Fullscreen::Borderless(None))
                    });
                }
            }
            "screenshot" => self.take_screenshot(),
            "toggle_editor" => self.toggle_editor(),
            "gear_up" | "gear_down" => {
                if self.alt_held() {
                    return false;
                }
                self.shift_gear(name == "gear_up");
            }
            "indicator_left" | "indicator_right" | "indicator_hazard" | "saloon_lights" => {
                if self.args.drive_keys == "omsi" {
                    return false;
                }
                if self.view != "free" {
                    match name {
                        "indicator_left" => self.blinker(1),
                        "indicator_right" => self.blinker(2),
                        "indicator_hazard" => self.blinker(3),
                        _ => self.saloon_lights(),
                    }
                }
            }
            "bus_startup" => self.bus_startup(),
            "radio_next" => {
                let msg = self.radio.next_station();
                self.service_msg = Some((msg, 4.0));
            }
            "toggle_city_map" => {
                if let Some(n) = self.navigator.as_mut() {
                    n.toggle_map();
                }
            }
            "show_position" => self.show_position(),
            "save_personnel" => self.save_personnel(),
            "toggel_ctrler" => {
                if let Some(c) = self.controllers.as_mut() {
                    c.enabled = !c.enabled;
                    let msg = if !c.any() {
                        "No game controller found"
                    } else if c.enabled {
                        "Game controller on"
                    } else {
                        "Game controller off"
                    };
                    self.service_msg = Some((msg.into(), 3.0));
                }
            }
            _ => return false,
        }
        true
    }

    pub(crate) fn shift_gear(&mut self, up: bool) -> bool {
        let names: &[&str] = if up {
            &[
                "kw_s_plus",
                "upshift",
                "gear_up",
                "gearup",
                "shift_up",
                "gang_hoch",
                "schalten_hoch",
                "manual_up",
            ]
        } else {
            &[
                "kw_s_minus",
                "downshift",
                "gear_down",
                "geardown",
                "shift_down",
                "gang_runter",
                "schalten_runter",
                "manual_down",
            ]
        };
        let Some(p) = self.player.as_mut() else {
            return false;
        };
        if p.vehicle.ty.program.trigger("kw_s_1").is_some() {
            let Some(cur) = p.gate_gear() else {
                return false;
            };
            let to = if up { cur + 1 } else { cur - 1 };
            if !p.shift_gate_to(to) {
                return false;
            }
            self.service_msg = Some((
                format!(
                    "Gear {}",
                    match to {
                        0 => "N".to_string(),
                        -1 => "R".to_string(),
                        n => n.to_string(),
                    }
                ),
                1.5,
            ));
            return true;
        }
        let Some(n) = names
            .iter()
            .find(|n| p.vehicle.ty.program.trigger(n).is_some())
        else {
            self.service_msg = Some(("This vehicle has no manual gearbox to shift".into(), 2.0));
            return false;
        };
        p.vehicle.trigger(n);
        p.vehicle.trigger(&format!("{n}_off"));
        true
    }

    pub(crate) fn set_mouse_drive(&mut self, on: bool) {
        self.mouse_drive = on;
        if !on {
            crate::player::keep_wheel(self.player.as_mut());
            // the brake the mouse held stays on, as the brake key leaves it (OMSI has one
            // brake for both): the bus rolled off when the mouse let go of it (#517, #760)
            if let Some(p) = self.player.as_mut() {
                p.axes.brake = p.axes.brake.max(self.mouse_pedals.1);
            }
            #[cfg(windows)]
            self.reset_vr_pointer();
        }
        self.mouse_steer = (
            self.player
                .as_ref()
                .map(|p| p.vehicle.physics.controls.steering)
                .unwrap_or(0.0),
            1.0,
        );
        self.mouse_pedals = self
            .player
            .as_ref()
            .map(|p| {
                (
                    p.vehicle.physics.controls.throttle,
                    p.vehicle.physics.controls.brake,
                )
            })
            .unwrap_or((0.0, 0.0));
        if self.settings.mouse_steering != on {
            self.settings.mouse_steering = on;
            crate::game_lists::remember_setting("mouse_steering", if on { "1" } else { "0" });
        }
    }

    pub(crate) fn toggle_pause(&mut self) {
        if self.lan.is_some() {
            self.service_msg = Some(("A LAN session cannot be paused".into(), 3.0));
            return;
        }
        self.paused = !self.paused;
        if self.game_menu.is_some() {
            self.menu_prev_pause = self.paused;
        }
    }
}
