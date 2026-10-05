#![allow(unused_imports)]
use super::Show;
use super::lights_ui;
use super::types::*;
use super::util::*;
use imgui::Condition;
use omsi_render::devtools as rdev;

pub(super) fn draw(
    ui: &imgui::Ui,
    show: &mut Show,
    actions: &mut Vec<Action>,
    extra: &Extra,
    editor: &mut super::vehicle_editor::VehicleEditor,
    names: &[String],
    mode_ref: &mut usize,
) {
    let mut mode = *mode_ref;
    if let Some(_bar) = ui.begin_main_menu_bar() {
        if let Some(_m) = ui.begin_menu("Game") {
            if ui.menu_item_config("Map Info").selected(show.map).build() {
                show.map = !show.map;
            }
            if ui
                .menu_item_config("Tour Table")
                .selected(show.tours)
                .build()
            {
                show.tours = !show.tours;
            }
            ui.separator();
            if ui.menu_item_config("Quick Save").shortcut("Ctrl+S").build() {
                actions.push(Action::QuickSave);
            }
            if ui
                .menu_item_config("Load Quick Save")
                .enabled(extra.quicksave)
                .build()
            {
                actions.push(Action::LoadQuickSave);
            }
        }
        if let Some(_m) = ui.begin_menu("Net") {
            if ui
                .menu_item_config("Connect to Server...")
                .enabled(extra.lan.is_none())
                .build()
            {
                show.connect = true;
            }
            if ui
                .menu_item_config("Open LAN...")
                .enabled(extra.lan.is_none())
                .build()
            {
                show.lan = true;
            }
            ui.separator();
            if ui
                .menu_item_config("Server Information")
                .selected(show.server)
                .enabled(extra.lan.is_some())
                .build()
            {
                show.server = !show.server;
            }
            if ui
                .menu_item_config("Copy Server Code")
                .enabled(extra.lan.is_some())
                .build()
            {
                actions.push(Action::CopyCode);
            }
        }
        if let Some(_m) = ui.begin_menu("Graphics") {
            if let Some(_r) = ui.begin_menu("Render Mode") {
                for (i, n) in names.iter().enumerate() {
                    if ui.menu_item_config(n).selected(mode == i).build() {
                        mode = i;
                    }
                }
            }
            if ui
                .menu_item_config("Graphics Window")
                .selected(show.graphics)
                .build()
            {
                show.graphics = !show.graphics;
            }
            if ui
                .menu_item_config("Light Settings")
                .selected(show.lights)
                .build()
            {
                show.lights = !show.lights;
            }
            if ui.menu_item("Reset Light Settings") {
                let mut s = crate::lights::settings();
                lights_ui::reset_global(&mut s);
                crate::lights::set_settings(s);
            }
        }
        if let Some(_m) = ui.begin_menu("Vehicle") {
            if ui
                .menu_item_config("Vehicle Editor")
                .selected(editor.open)
                .build()
            {
                editor.open = !editor.open;
            }
            if ui
                .menu_item_config("Cockpit Buttons")
                .selected(show.cockpit)
                .enabled(extra.vehicle.is_some())
                .build()
            {
                show.cockpit = !show.cockpit;
            }
            if ui
                .menu_item_config("Actions")
                .selected(show.vehicle)
                .enabled(extra.vehicle.is_some())
                .build()
            {
                show.vehicle = !show.vehicle;
            }
        }
        if let Some(_m) = ui.begin_menu("Player") {
            if ui
                .menu_item_config("Player Details")
                .selected(show.walk)
                .build()
            {
                show.walk = !show.walk;
            }
        }
    }
    *mode_ref = mode;
}
