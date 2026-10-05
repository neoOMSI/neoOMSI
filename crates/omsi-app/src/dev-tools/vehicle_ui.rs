#![allow(unused_imports)]
use super::lights_ui;
use super::types::*;
use super::util::*;
use imgui::Condition;
use omsi_render::devtools as rdev;

pub(super) fn cockpit(
    ui: &imgui::Ui,
    open: &mut bool,
    extra: &Extra,
    cockpit_filter: &mut String,
    actions: &mut Vec<Action>,
) {
    if !*open {
        return;
    }
    ui.window("Cockpit Buttons")
        .opened(open)
        .size([360.0, 480.0], Condition::FirstUseEver)
        .position([12.0, 32.0], Condition::FirstUseEver)
        .build(|| {
            let Some(v) = extra.vehicle.as_ref() else {
                ui.text("No vehicle driven");
                return;
            };
            ui.input_text("Filter##cockpit", cockpit_filter).build();
            ui.separator();
            let f = cockpit_filter.to_ascii_lowercase();
            ui.child_window("##cockpit_buttons").build(|| {
                for (i, ev) in v
                    .controls
                    .iter()
                    .filter(|(_, e)| f.is_empty() || e.to_ascii_lowercase().contains(&f))
                {
                    if ui.button(format!("{ev}##c{i}")) {
                        actions.push(Action::Cockpit(*i));
                    }
                }
            });
        });
}

pub(super) fn actions(
    ui: &imgui::Ui,
    open: &mut bool,
    extra: &Extra,
    vehicle_filter: &mut String,
    actions: &mut Vec<Action>,
) {
    if !*open {
        return;
    }
    ui.window("Actions")
        .opened(open)
        .size([360.0, 480.0], Condition::FirstUseEver)
        .position([12.0, 32.0], Condition::FirstUseEver)
        .build(|| {
            let Some(v) = extra.vehicle.as_ref() else {
                ui.text("No vehicle driven");
                return;
            };
            if ui.button("Start Up") {
                actions.push(Action::VehicleStartUp);
            }
            if ui.button("Indicator Left") {
                actions.push(Action::Vehicle("blinker_left_toggle".into()));
            }
            ui.same_line();
            if ui.button("Indicator Right") {
                actions.push(Action::Vehicle("blinker_right_toggle".into()));
            }
            ui.separator();
            ui.input_text("Filter##actions", vehicle_filter).build();
            ui.separator();
            let f = vehicle_filter.to_ascii_lowercase();
            ui.child_window("##vehicle_actions").build(|| {
                for a in v
                    .actions
                    .iter()
                    .filter(|a| f.is_empty() || a.to_ascii_lowercase().contains(&f))
                {
                    if ui.button(a) {
                        actions.push(Action::Vehicle(a.clone()));
                    }
                }
            });
        });
}
