use super::*;

pub(super) fn indicator_toggle_action(state: &mut u8, lever: Option<u8>, want: u8) -> &'static str {
    if let Some(lever) = lever {
        *state = lever;
    }
    if want == 3 {
        *state = if *state == 3 { 0 } else { 3 };
        "blinker_warn_toggle"
    } else if *state == want {
        *state = 0;
        "blinker_off"
    } else {
        *state = want;
        if want == 1 {
            "blinker_left_set"
        } else {
            "blinker_right_set"
        }
    }
}

pub(super) fn kept_indicator(cancel: bool, before: Option<f32>, after: Option<f32>) -> Option<f32> {
    let (before, after) = (before?.round(), after?.round());
    (!cancel && (before == 1.0 || before == 2.0) && after == 0.0).then_some(before)
}

pub(super) fn is_manual_gate_action(name: &str) -> bool {
    let Some(gate) = name
        .get(..5)
        .filter(|p| p.eq_ignore_ascii_case("kw_s_"))
        .and_then(|_| name.get(5..))
    else {
        return false;
    };
    let gate = gate.strip_suffix("_fest").unwrap_or(gate);
    gate.eq_ignore_ascii_case("r") || gate.eq_ignore_ascii_case("n") || gate.parse::<u32>().is_ok()
}

impl Player {
    pub(crate) fn press_control(&mut self, i: usize) {
        self.release();
        let Some(ev) = self
            .vehicle
            .ty
            .meshes
            .get(i)
            .and_then(|m| self.vehicle.ty.model.meshes.get(m.def_index))
            .and_then(|d| d.mouse_event.clone())
        else {
            return;
        };
        let plain = self.vehicle.trigger(&ev);
        self.repair_roller_blind(&ev);
        self.pressed_mesh = Some(i);
        self.press_info = (plain, 0.0);
        self.auto_drag = None;
        self.release();
    }

    pub(crate) fn control_list(&self) -> Vec<(usize, String)> {
        let mut out: Vec<(usize, String)> = Vec::new();
        for (i, m) in self.vehicle.ty.meshes.iter().enumerate() {
            let Some(ev) = self
                .vehicle
                .ty
                .model
                .meshes
                .get(m.def_index)
                .and_then(|d| d.mouse_event.clone())
            else {
                continue;
            };
            if !out.iter().any(|(_, e)| *e == ev) {
                out.push((i, ev));
            }
        }
        out.sort_by(|a, b| a.1.to_ascii_lowercase().cmp(&b.1.to_ascii_lowercase()));
        out
    }

    pub(crate) fn toggle_indicator(&mut self, want: u8) {
        let lever = if self
            .vehicle
            .var("lights_sw_warnblinker")
            .is_some_and(|v| v > 0.5)
        {
            Some(3)
        } else {
            self.vehicle
                .var("lights_sw_blinker")
                .map(|v| match v.round() as i32 {
                    1 => 1,
                    2 => 2,
                    _ => 0,
                })
        };
        let action = indicator_toggle_action(&mut self.blinker_key_state, lever, want);
        self.action(action, true);
        self.action(action, false);
    }

    pub(crate) fn repair_roller_blind(&mut self, event: &str) {
        let e = event.to_ascii_lowercase();
        if !e.contains("rollo") {
            return;
        }
        if e.ends_with("retract") {
            self.vehicle.set_var("cp_rollo_rastpos", 0.0);
        } else if e.ends_with("drag") {
            if let Some(pos) = self.vehicle.var("cp_rollo_pos") {
                self.vehicle.set_var("cp_rollo_rastpos", pos);
            }
        }
    }

    pub(crate) fn action(&mut self, name: &str, pressed: bool) -> bool {
        if pressed {
            log::info!("action: {name}");
        }
        if name.eq_ignore_ascii_case("blinker_left_toggle")
            || name.eq_ignore_ascii_case("blinker_right_toggle")
        {
            if pressed {
                self.toggle_indicator(if name.eq_ignore_ascii_case("blinker_left_toggle") {
                    1
                } else {
                    2
                });
            }
            return true;
        }
        let suffix = if pressed { "" } else { "_off" };
        let release_gear = !pressed
            && self.momentary_gears
            && self.vehicle.ty.program.manual_gearbox()
            && is_manual_gate_action(name);
        if name.eq_ignore_ascii_case("ticket_give") {
            if pressed {
                self.give_ticket = true;
            }
            return true;
        }
        if name.eq_ignore_ascii_case("change_give") || name.eq_ignore_ascii_case("change_take") {
            if pressed {
                if name.eq_ignore_ascii_case("change_give") {
                    self.give_change = true;
                } else {
                    self.take_change = true;
                }
            }
            self.vehicle.trigger(&format!("{name}{suffix}"));
            return true;
        }
        if pressed {
            self.clutch_for_gate(name);
        }
        let headlights = pressed && name.eq_ignore_ascii_case("kw_scheinwerfer_toggle");
        let lamps_before = if headlights {
            self.outside_lamps_lit()
        } else {
            0
        };
        if self.vehicle.trigger(&format!("{name}{suffix}")) {
            if release_gear {
                self.select_neutral();
            }
            self.repair_roller_blind(&format!("{name}{suffix}"));
            if headlights {
                self.headlights_with_side_lights(lamps_before);
            }
            return true;
        }
        if !pressed && self.vehicle.ty.program.trigger(name).is_some() {
            if release_gear {
                self.select_neutral();
            }
            return true;
        }
        let Some((_, aliases)) = ACTION_ALIASES
            .iter()
            .find(|(a, _)| a.eq_ignore_ascii_case(name))
        else {
            return false;
        };
        for alias in *aliases {
            if self.vehicle.trigger(&format!("{alias}{suffix}")) {
                if release_gear {
                    self.select_neutral();
                }
                self.repair_roller_blind(&format!("{alias}{suffix}"));
                return true;
            }
        }
        let done = self.toggle_as_steps(name, pressed);
        if release_gear {
            self.select_neutral();
        }
        done
    }

    pub(super) fn select_neutral(&mut self) {
        for name in ["kw_s_N", "kw_s_N_fest"] {
            if self.vehicle.ty.program.trigger(name).is_some() && self.vehicle.trigger(name) {
                self.vehicle.trigger(&format!("{name}_off"));
                break;
            }
        }
    }

    pub(crate) fn clutch_for_gate(&mut self, name: &str) {
        let Some(gate) = name
            .get(..5)
            .filter(|p| p.eq_ignore_ascii_case("kw_s_"))
            .map(|_| &name[5..])
        else {
            return;
        };
        let gate = gate.strip_suffix("_fest").unwrap_or(gate);
        let is_gate = gate.eq_ignore_ascii_case("r")
            || gate.eq_ignore_ascii_case("n")
            || gate.parse::<u32>().is_ok();
        let program = &self.vehicle.ty.program;
        if !is_gate
            || !program.manual_gearbox()
            || self.vehicle.host.auto_clutch < 0.5
            || program.trigger(name).is_none()
            || program.reads_sys(::legacy_script::SysVar::AutoClutch)
        {
            return;
        }
        self.vehicle.set_var("Clutch", 1.0);
        self.axes.clutch = 1.0;
    }

    pub(crate) fn toggle_as_steps(&mut self, name: &str, pressed: bool) -> bool {
        let Some(base) = name.strip_suffix("_toggle") else {
            return false;
        };
        let (up, down) = (format!("{base}_up"), format!("{base}_down"));
        let has = |v: &::simulation::VehicleInstance, t: &str| {
            v.ty.program.triggers.contains_key(&t.to_ascii_lowercase())
        };
        if !has(&self.vehicle, &up) || !has(&self.vehicle, &down) {
            return false;
        }
        if !pressed {
            return true;
        }
        let key = base.to_ascii_lowercase();
        let lit = self.outside_lamps_lit() > 0;
        let going_up = if base.to_ascii_lowercase().contains("schein")
            || base.to_ascii_lowercase().contains("licht")
            || base.to_ascii_lowercase().contains("light")
        {
            !lit
        } else {
            !self.toggled_up.contains(&key)
        };
        let step = if going_up { &up } else { &down };
        for _ in 0..2 {
            self.vehicle.trigger(step);
            self.vehicle.trigger(&format!("{step}_off"));
        }
        if going_up {
            self.toggled_up.insert(key);
        } else {
            self.toggled_up.remove(&key);
        }
        true
    }

    pub(crate) fn key(&mut self, scan: i32, modifiers: i32, pressed: bool) {
        let names: Vec<String> = if pressed {
            let n: Vec<String> = self
                .bindings
                .iter()
                .filter(|b| b.scan_code == scan && b.matches(modifiers))
                .map(|b| b.action.clone())
                .collect();
            self.held_keys.insert(scan, n.clone());
            n
        } else {
            match self.held_keys.remove(&scan) {
                Some(n) => n,
                None => self
                    .bindings
                    .iter()
                    .filter(|b| b.scan_code == scan && b.matches(modifiers))
                    .map(|b| b.action.clone())
                    .collect(),
            }
        };
        for name in names {
            if let Some(a) = ::simulation::engine_action(&name) {
                self.axes.set(a, pressed);
            } else {
                self.action(&name, pressed);
            }
        }
    }

    pub(crate) fn start_up(&mut self) -> String {
        if let Some(s) = self.startup.as_ref() {
            if self
                .startup_at
                .is_none_or(|t| t.elapsed().as_secs_f32() < 20.0)
            {
                return if s.shutting_down() {
                    "Switching the vehicle off ..."
                } else {
                    "Putting the vehicle into service ..."
                }
                    .to_string();
            }
            log::info!("auto-start given up after 20 s: begun again");
        }
        let bound = self.bound_actions();
        let s = ::simulation::startup::StartUp::new(&self.vehicle, &bound);
        let shutting_down = s.shutting_down();
        self.startup = Some(s);
        self.startup_at = Some(Instant::now());
        if shutting_down {
            "Switching the vehicle off ...".to_string()
        } else {
            "Putting the vehicle into service ...".to_string()
        }
    }

    pub(crate) fn bound_actions(&self) -> Vec<String> {
        let mut names: Vec<String> = Vec::new();
        for b in &self.bindings {
            if !names.iter().any(|n| n.eq_ignore_ascii_case(&b.action)) {
                names.push(b.action.clone());
            }
        }
        names
    }

    pub(crate) fn tick_startup(&mut self, dt: f32) -> bool {
        self.apply_html_requests();
        self.tick_ibis(dt);
        let Some(mut s) = self.startup.take() else {
            return false;
        };
        let bound = self.bound_actions();
        if s.tick(&mut self.vehicle, &bound, dt) {
            self.startup = Some(s);
            return true;
        }
        log::info!(
            "auto-start finished: {} (engine_n={:?})",
            if s.report.is_empty() {
                "nothing to do".to_string()
            } else {
                s.report.join(", ")
            },
            self.vehicle.var("engine_n")
        );
        if ::simulation::startup::power_on(&self.vehicle) {
            self.saloon_lights_after_dark();
        }
        if let Some((line, terminus, stops, stop)) = self.ibis_duty.take() {
            self.type_destination(&line, &terminus, &stops, (stop.0, &stop.1));
        }
        false
    }
}

#[cfg(test)]
mod kept_indicator_tests {
    use super::kept_indicator;

    #[test]
    fn a_kept_indicator_is_put_back_when_the_script_cancels_it() {
        assert_eq!(kept_indicator(true, Some(1.0), Some(0.0)), None);
        assert_eq!(kept_indicator(false, Some(1.0), Some(0.0)), Some(1.0));
        assert_eq!(kept_indicator(false, Some(2.0), Some(0.0)), Some(2.0));
        assert_eq!(kept_indicator(false, Some(1.0), Some(2.0)), None);
        assert_eq!(kept_indicator(false, Some(0.0), Some(0.0)), None);
        assert_eq!(kept_indicator(false, None, Some(0.0)), None);
    }
}

#[cfg(test)]
mod indicator_tests {
    use super::indicator_toggle_action;

    #[test]
    fn repeated_presses_switch_each_side_on_then_off() {
        for (side, on) in [(1, "blinker_left_set"), (2, "blinker_right_set")] {
            let mut state = 0;
            assert_eq!(indicator_toggle_action(&mut state, None, side), on);
            assert_eq!(
                indicator_toggle_action(&mut state, None, side),
                "blinker_off"
            );
            assert_eq!(state, 0);
        }
    }

    #[test]
    fn changing_side_and_automatic_cancellation_use_the_current_lever() {
        let mut state = 1;
        assert_eq!(
            indicator_toggle_action(&mut state, Some(1), 2),
            "blinker_right_set"
        );
        assert_eq!(
            indicator_toggle_action(&mut state, Some(0), 2),
            "blinker_right_set"
        );
        assert_eq!(
            indicator_toggle_action(&mut state, Some(2), 2),
            "blinker_off"
        );
    }

    #[test]
    fn hazards_keep_their_dedicated_toggle_trigger() {
        let mut state = 0;
        assert_eq!(
            indicator_toggle_action(&mut state, Some(0), 3),
            "blinker_warn_toggle"
        );
        assert_eq!(state, 3);
        assert_eq!(
            indicator_toggle_action(&mut state, Some(3), 3),
            "blinker_warn_toggle"
        );
        assert_eq!(state, 0);
    }
}
