#![allow(unused_imports)]
use super::lights_ui;
use super::types::*;
use super::util::*;
use imgui::Condition;
use omsi_render::devtools as rdev;

pub(super) fn graphics(
    ui: &imgui::Ui,
    open: &mut bool,
    snap: &Snapshot,
    history: &[f32],
    names: &[String],
    mode: usize,
) {
    if !*open {
        return;
    }
    ui.window("Graphics")
        .opened(open)
        .size([400.0, 420.0], Condition::FirstUseEver)
        .position([12.0, 32.0], Condition::FirstUseEver)
        .build(|| {
            let api = snap
                .adapter
                .rsplit_once('(')
                .map(|(_, b)| b.trim_end_matches(')').trim())
                .unwrap_or("Unknown");
            ui.text(format!("API: {api}"));
            ui.text(format!("Format: {:?}", snap.format));
            ui.text(format!("Window: {} x {}", snap.surface.0, snap.surface.1));
            ui.text(format!("{:.0} FPS, {:.2} ms", snap.fps, snap.dt_ms));
            let worst = history.iter().cloned().fold(0.0f32, f32::max);
            let overlay = format!("max {worst:.1} ms");
            ui.plot_lines("##ms", history)
                .scale_min(0.0)
                .scale_max(worst.max(1.0))
                .graph_size([0.0, 56.0])
                .overlay_text(&overlay)
                .build();
            ui.separator();
            ui.text(format!("Render mode: {}", names[mode.min(names.len() - 1)]));
            if mode == 1 && !rdev::wireframe_supported() {
                ui.text_colored(
                    [1.0, 0.6, 0.2, 1.0],
                    "Wireframe: GPU lacks POLYGON_MODE_LINE",
                );
            }
            if mode >= 2 {
                ui.text_wrapped("Debug views need the enhanced graphics.");
            }
            ui.separator();
            ui.text(format!(
                "MSAA {}x, AF {}x, Shadows {}, Render scale {}",
                snap.msaa,
                snap.anisotropy,
                snap.shadow_size,
                if snap.render_scale <= 0.0 {
                    "auto".to_string()
                } else {
                    format!("{:.2}", snap.render_scale)
                }
            ));
            ui.text(format!(
                "SSAO {}, FXAA {}, Reflections {}",
                on_off(snap.ssao),
                on_off(snap.fxaa),
                on_off(snap.reflections)
            ));
            ui.separator();
            ui.text(format!(
                "Meshes {}, Textures {}, Materials {}",
                snap.meshes, snap.textures, snap.materials
            ));
            ui.text(format!(
                "Instances {}, Lights {} (interior {}), Coronas {}",
                snap.instances, snap.lights, snap.interior_lights, snap.coronas
            ));
        });
}

pub(super) fn map(ui: &imgui::Ui, open: &mut bool, extra: &Extra, snap: &Snapshot) {
    if !*open {
        return;
    }
    ui.window("Map Info")
        .opened(open)
        .size([380.0, 260.0], Condition::FirstUseEver)
        .build(|| {
            ui.text(format!("Map: {}", extra.map));
            ui.text(format!("Time: {}", hms(extra.clock)));
            ui.text(format!("Paused: {}", extra.paused));
            if let Some(c) = extra.cam.as_ref() {
                ui.text(format!(
                    "Camera: {:.1} {:.1} {:.1}  yaw {:.0} pitch {:.0}",
                    c.position.x, c.position.y, c.position.z, c.yaw, c.pitch
                ));
            }
            ui.separator();
            ui.text(format!(
                "Meshes {}, Textures {}, Materials {}",
                snap.meshes, snap.textures, snap.materials
            ));
            ui.text(format!(
                "Instances {}, Lights {} (interior {}), Coronas {}",
                snap.instances, snap.lights, snap.interior_lights, snap.coronas
            ));
        });
}

pub(super) fn tours(ui: &imgui::Ui, open: &mut bool, extra: &Extra) {
    if !*open {
        return;
    }
    ui.window("Tour Table")
        .opened(open)
        .size([520.0, 360.0], Condition::FirstUseEver)
        .build(|| {
            if extra.tours.is_empty() {
                ui.text("No timetable loaded.");
                return;
            }
            ui.text(format!("Now: {}", hms(extra.clock)));
            ui.columns(5, "##tours", true);
            for h in ["Line", "Tour", "Start", "Trips", "Today"] {
                ui.text(h);
                ui.next_column();
            }
            ui.separator();
            for t in &extra.tours {
                ui.text(&t.line);
                ui.next_column();
                ui.text(&t.number);
                ui.next_column();
                ui.text(hms(t.start));
                ui.next_column();
                ui.text(t.trips.to_string());
                ui.next_column();
                ui.text(if t.available { "yes" } else { "no" });
                ui.next_column();
            }
            ui.columns(1, "##toursend", false);
        });
}
