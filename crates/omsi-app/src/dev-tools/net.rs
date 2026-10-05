#![allow(unused_imports)]
use super::lights_ui;
use super::types::*;
use super::util::*;
use imgui::Condition;
use omsi_render::devtools as rdev;

pub(super) fn server(ui: &imgui::Ui, open: &mut bool, extra: &Extra, actions: &mut Vec<Action>) {
    if !*open {
        return;
    }
    ui.window("Server Information")
        .opened(open)
        .size([400.0, 260.0], Condition::FirstUseEver)
        .build(|| {
            let Some(l) = extra.lan.as_ref() else {
                ui.text("Not in a LAN session or on a server.");
                return;
            };
            ui.text(format!("Role: {}", if l.host { "Host" } else { "Client" }));
            ui.text(format!("Connected: {}", l.connected));
            ui.text(format!("Name: {}", l.name));
            ui.text(format!("Session: {:016x}", l.session));
            ui.text(format!("Map: {}", l.map));
            ui.text(format!("Other players: {}", l.peers));
            ui.text(format!("Sent: {} KiB", l.sent / 1024));
            if let Some(a) = l.local.as_ref() {
                ui.text(format!("Local address: {a}"));
            }
            if let Some(a) = l.target.as_ref() {
                ui.text(format!("Target: {a}"));
            }
            if let Some(c) = l.code.as_ref() {
                ui.text_wrapped(format!("Code: {c}"));
                if ui.button("Copy code") {
                    actions.push(Action::CopyCode);
                }
            }
            if let Some(r) = l.rejected.as_ref() {
                ui.text_colored([1.0, 0.4, 0.3, 1.0], format!("Rejected: {r}"));
            }
        });
}

pub(super) fn connect(
    ui: &imgui::Ui,
    open: &mut bool,
    connect_addr: &mut String,
    actions: &mut Vec<Action>,
) {
    if !*open {
        return;
    }
    ui.window("Connect to Server")
        .opened(open)
        .size([380.0, 110.0], Condition::FirstUseEver)
        .build(|| {
            ui.text("Address, code or https:// name:");
            ui.set_next_item_width(-1.0);
            ui.input_text("##addr", &mut *connect_addr).build();
            let go = !connect_addr.trim().is_empty();
            if ui.button("Connect") && go {
                actions.push(Action::Connect(connect_addr.trim().to_string()));
            }
            ui.same_line();
            ui.text_disabled("(restarts the game and joins)");
        });
}

pub(super) fn lan(ui: &imgui::Ui, open: &mut bool, lan_port: &mut i32, actions: &mut Vec<Action>) {
    if !*open {
        return;
    }
    ui.window("Open LAN")
        .opened(open)
        .size([320.0, 110.0], Condition::FirstUseEver)
        .build(|| {
            ui.input_int("UDP port (0 = default)", &mut *lan_port)
                .build();
            *lan_port = (*lan_port).clamp(0, 65535);
            if ui.button("Open") {
                actions.push(Action::OpenLan(*lan_port as u16));
            }
        });
}
