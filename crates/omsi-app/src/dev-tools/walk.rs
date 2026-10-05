#![allow(unused_imports)]
use super::lights_ui;
use super::types::*;
use super::util::*;
use imgui::Condition;
use omsi_render::devtools as rdev;

pub(super) fn window(ui: &imgui::Ui, open: &mut bool, extra: &Extra) {
    if !*open {
        return;
    }
    ui.window("Player Details")
        .opened(open)
        .size([380.0, 280.0], Condition::FirstUseEver)
        .build(|| {
            match extra.foot.as_ref() {
                Some(f) => {
                    ui.text(format!(
                        "Pos: {:.2} {:.2} {:.2}",
                        f.pos[0], f.pos[1], f.pos[2]
                    ));
                    ui.text(format!("Heading: {:.1}", f.heading));
                    ui.text(format!(
                        "Velocity: {:.2} {:.2}, vertical {:.2}",
                        f.vel[0], f.vel[1], f.vz
                    ));
                    ui.text(format!(
                        "On lane {}, attached {}, seated {}, in bus {}",
                        f.on_lane, f.attached, f.seated, f.inside
                    ));
                }
                None => ui.text("Not walking."),
            }
            ui.separator();
            ui.separator();
            ui.text(format!("Blocking boxes: {} (red)", extra.blockers.len()));
            for o in extra.blockers.iter().take(6) {
                ui.text(format!(
                    "id {} z {:.2}..{:.2} half {:.2} {:.2} {}",
                    o.id,
                    o.z0,
                    o.z1,
                    o.half.x,
                    o.half.y,
                    if o.mass > 0.0 { "vehicle" } else { "scenery" }
                ));
            }
            ui.separator();
            ui.text(format!("Bus doors near: {}", extra.doors.len()));
            for (i, d) in extra.doors.iter().enumerate() {
                ui.text_colored(
                    if d.in_lane {
                        [0.3, 1.0, 0.4, 1.0]
                    } else if d.open {
                        [1.0, 0.8, 0.3, 1.0]
                    } else {
                        [1.0, 0.4, 0.3, 1.0]
                    },
                    format!(
                        "#{i} open {} along {:.1}/{:.1} lateral {:.1} lane {}",
                        d.open, d.along, d.len, d.lateral, d.in_lane
                    ),
                );
            }
            ui.text_disabled(
                "door line: green open, red closed; lane needs along -1.2..len+1, lateral <= 1.0",
            );
            ui.text_disabled("green scenery, orange vehicles, yellow poles");
        });
}
