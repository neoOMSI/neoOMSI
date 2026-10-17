use super::*;

impl Player {
    pub(crate) fn toggle_saloon_lights(&mut self) -> String {
        let on = self.saloon_lamps_lit() == 0;
        let done = self.set_saloon_lights(on);
        log::info!(
            "saloon lights {} ({})",
            if on { "on" } else { "off" },
            done.join(", ")
        );
        if self.vehicle.ty.model.interior_lights.is_empty() {
            "This bus has no saloon lights".into()
        } else if !done.is_empty() {
            format!("Saloon lights {}", if on { "on" } else { "off" })
        } else if on {
            "Saloon light switches on - no power (battery off?)".into()
        } else {
            "Saloon lights off".into()
        }
    }

    pub(super) fn saloon_lamps_lit(&self) -> usize {
        self.vehicle
            .ty
            .model
            .interior_lights
            .iter()
            .filter(|l| self.vehicle.var(&l.variable).unwrap_or(0.0) > 0.5)
            .count()
    }

    pub(crate) fn set_saloon_lights(&mut self, on: bool) -> Vec<String> {
        let mut switches: Vec<String> = self
            .bound_actions()
            .into_iter()
            .filter(|a| {
                let a = a.to_ascii_lowercase();
                (a.contains("licht") || a.contains("light"))
                    && a.starts_with("cp_")
                    && !a.contains("fahrer")
                    && !a.contains("driver")
            })
            .collect();
        for extra in ["interior_light_toggle", "interiorlight_toggle"] {
            if self.vehicle.ty.program.triggers.contains_key(extra) {
                switches.push(extra.to_string());
            }
        }
        let total = self.vehicle.ty.model.interior_lights.len();
        let mut done = Vec::new();
        for sw in switches {
            let before = self.saloon_lamps_lit();
            if (on && before == total) || (!on && before == 0) {
                break;
            }
            let saved = self.vehicle.state.vars.clone();
            if !self.action(&sw, true) {
                continue;
            }
            self.action(&sw, false);
            self.vehicle.update_scripts_only(0.0);
            let after = self.saloon_lamps_lit();
            if (on && after < before) || (!on && after > before) {
                self.vehicle.state.vars = saved;
            } else if after != before {
                done.push(sw);
            }
        }
        done
    }

    pub(super) fn outside_lamps_lit(&self) -> usize {
        let v = &self.vehicle;
        v.ty.model
            .meshes
            .iter()
            .flat_map(|m| {
                m.light_enh
                    .iter()
                    .map(|l| l.variable.as_str())
                    .chain(m.light_enh_2.iter().map(|l| l.variable.as_str()))
            })
            .filter(|n| !n.is_empty() && v.var(n).unwrap_or(0.0) > 0.5)
            .count()
    }

    pub(super) fn headlights_with_side_lights(&mut self, before: usize) {
        if !self
            .vehicle
            .ty
            .program
            .triggers
            .contains_key("kw_standlicht_toggle")
        {
            return;
        }
        self.vehicle.update_scripts_only(0.0);
        let after = self.outside_lamps_lit();
        let try_press = |p: &mut Player, better: &dyn Fn(usize) -> bool| -> bool {
            let saved = p.vehicle.state.vars.clone();
            p.vehicle.trigger("kw_standlicht_toggle");
            p.vehicle.trigger("kw_standlicht_toggle_off");
            p.vehicle.update_scripts_only(0.0);
            if better(p.outside_lamps_lit()) {
                true
            } else {
                p.vehicle.state.vars = saved;
                false
            }
        };
        if after > before && !self.side_lights_by_l {
            self.side_lights_by_l = try_press(self, &|n| n > after);
        } else if after < before && after > 0 {
            if try_press(self, &|n| n < after) {
                self.side_lights_by_l = false;
            }
        }
    }

    pub(super) fn saloon_lights_after_dark(&mut self) {
        let light_out = !::simulation::Daylight::compute(&self.vehicle.host.clock, None).lamps_on;
        if light_out {
            return;
        }
        if self.vehicle.ty.model.interior_lights.is_empty() {
            return;
        }
        let done = self.set_saloon_lights(true);
        if !done.is_empty() {
            log::info!(
                "auto-start: saloon lights switched on after dark ({})",
                done.join(", ")
            );
        }
    }
}
