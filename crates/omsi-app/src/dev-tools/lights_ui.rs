#![allow(unused_imports)]
use super::types::*;
use imgui::Condition;

pub(super) fn lights_window(ui: &imgui::Ui, open: &mut bool) {
    if !*open {
        return;
    }
    let mut s = crate::lights::settings();
    ui.window("Light Settings")
        .opened(open)
        .size([400.0, 300.0], Condition::FirstUseEver)
        .position([12.0, 32.0], Condition::FirstUseEver)
        .build(|| {
            ui.slider("Weather Boost", 0.0, 4.0, &mut s.weather_boost);
            ui.slider("Weather Night", 0.0, 1.0, &mut s.weather_night);
            ui.slider("Corona / Cone", 0.0, 4.0, &mut s.corona);
            ui.separator();
            ui.text("HTML & Scripting textures");
            for (i, n) in [
                "HTML Texture Glow",
                "HTML Texture Light",
                "Script Texture Glow",
                "Script Texture Light",
            ]
            .iter()
            .enumerate()
            {
                let mut g = crate::lights::screen_fx(i);
                if ui.slider(format!("{n}##fx{i}"), 0.0, 4.0, &mut g) {
                    crate::lights::set_screen_fx(i, g);
                }
            }
            ui.separator();
            if ui.button("Reset") {
                reset_global(&mut s);
                crate::lights::reset_screen_fx();
            }
        });
    crate::lights::set_settings(s);
}

pub(super) fn reset_global(s: &mut crate::lights::LightSettings) {
    let d = crate::lights::LightSettings::DEFAULT;
    s.weather_boost = d.weather_boost;
    s.weather_night = d.weather_night;
    s.corona = d.corona;
}

pub(super) fn interior_panel(ui: &imgui::Ui, extra: &Extra, actions: &mut Vec<Action>) {
    let Some(v) = extra.vehicle.as_ref() else {
        ui.text_disabled("No vehicle driven");
        return;
    };
    if ui.button("Toggle Saloon Lights") {
        actions.push(Action::VehicleSaloonLights);
    }
    ui.same_line();
    if ui.button("Reset Sources") {
        crate::lights::reset_interior_cfg();
    }
    ui.text_disabled(format!("{} sources", v.interior.len()));
    ui.separator();
    for (i, src) in v.interior.iter().enumerate() {
        let mut c = crate::lights::interior_cfg(i);
        let label = format!("#{i} {}##il{i}", src.variable);
        if ui.collapsing_header(label, imgui::TreeNodeFlags::empty()) {
            ui.text_disabled(format!(
                "pos {:.2} {:.2} {:.2}  range {:.2}  color {:.0} {:.0} {:.0}",
                src.pos[0],
                src.pos[1],
                src.pos[2],
                src.range,
                src.color[0],
                src.color[1],
                src.color[2]
            ));
            let mut on = !c.off;
            ui.checkbox(format!("Enabled##il{i}"), &mut on);
            c.off = !on;
            ui.slider(format!("Intensity x##il{i}"), 0.0, 4.0, &mut c.gain);
            ui.slider(format!("Range x##il{i}"), 0.1, 4.0, &mut c.range);
            ui.slider(format!("Right (m)##il{i}"), -3.0, 3.0, &mut c.shift[0]);
            ui.slider(format!("Forward (m)##il{i}"), -3.0, 3.0, &mut c.shift[1]);
            ui.slider(format!("Height (m)##il{i}"), -3.0, 3.0, &mut c.shift[2]);
            ui.color_edit3(format!("Tint##il{i}"), &mut c.color);
            if ui.button(format!("Reset##il{i}")) {
                c = crate::lights::InteriorCfg::DEFAULT;
            }
        }
        crate::lights::set_interior_cfg(i, c);
    }
}

pub(super) fn vehicle_panel(ui: &imgui::Ui, s: &mut crate::lights::LightSettings) {
    if ui.collapsing_header("Headlights", imgui::TreeNodeFlags::DEFAULT_OPEN) {
        ui.checkbox("Force High Beam", &mut s.force_high_beam);
        ui.checkbox("Show Beam Markers", &mut s.beam_marker);
        ui.slider("Headlight", 0.0, 100.0, &mut s.headlight);
        ui.slider("Vanilla Headlight", 0.0, 2.0, &mut s.vanilla);
        ui.separator();
        ui.text_colored([1.0, 0.9, 0.1, 1.0], "Fog Cone start (yellow), m");
        ui.slider("Forward##cone", -20.0, 20.0, &mut s.cone_offset);
        ui.slider("Right##cone", -5.0, 5.0, &mut s.cone_side);
        ui.slider("Height##cone", -5.0, 5.0, &mut s.cone_height);
        ui.separator();
        ui.text_colored([0.1, 0.9, 1.0, 1.0], "Headlight start (cyan), m");
        ui.slider("Forward##lamp", -20.0, 20.0, &mut s.lamp_offset);
        ui.slider("Right##lamp", -5.0, 5.0, &mut s.lamp_side);
        ui.slider("Height##lamp", -5.0, 5.0, &mut s.lamp_height);
        ui.slider("Yaw (deg, left +)##lamp", -45.0, 45.0, &mut s.lamp_yaw);
        ui.slider("Pitch (deg, up +)##lamp", -45.0, 45.0, &mut s.lamp_pitch);
        ui.slider("Lamp Distance x##lamp", 0.0, 3.0, &mut s.lamp_spread);
        ui.slider("Range x##lamp", 0.1, 4.0, &mut s.lamp_range);
        ui.slider("Core x##lamp", 0.1, 10.0, &mut s.lamp_core);
        ui.slider("Inner Angle +deg##lamp", -60.0, 60.0, &mut s.lamp_inner_add);
        ui.slider("Outer Angle +deg##lamp", -60.0, 60.0, &mut s.lamp_outer_add);
        ui.color_edit3("Color Tint##lamp", &mut s.lamp_color);
    }
    beam_panel(ui, s);
    ui.separator();
    if ui.button("Reset Vehicle Lights") {
        let d = crate::lights::LightSettings::DEFAULT;
        let (wb, wn, co) = (s.weather_boost, s.weather_night, s.corona);
        *s = d;
        s.weather_boost = wb;
        s.weather_night = wn;
        s.corona = co;
    }
}

fn beam_panel(ui: &imgui::Ui, s: &mut crate::lights::LightSettings) {
    if ui.collapsing_header("Low Beam", imgui::TreeNodeFlags::DEFAULT_OPEN) {
        ui.slider("Gain##low", 1.0, 10.0, &mut s.low_beam_gain);
        beam_controls(ui, "low", &mut s.low);
    }
    if ui.collapsing_header("High Beam", imgui::TreeNodeFlags::DEFAULT_OPEN) {
        ui.slider("Gain##high", 0.0, 3.0, &mut s.high_beam);
        ui.slider("Range (global x)##high", 0.5, 4.0, &mut s.high_beam_range);
        ui.slider(
            "Spread (global x)##high",
            0.25,
            3.0,
            &mut s.high_beam_spread,
        );
        beam_controls(ui, "high", &mut s.high);
    }
}

fn beam_controls(ui: &imgui::Ui, id: &str, b: &mut crate::lights::BeamCfg) {
    ui.checkbox(format!("Enabled##{id}"), &mut b.on);
    ui.slider(format!("Intensity x##{id}b"), 0.0, 4.0, &mut b.gain);
    ui.slider(format!("Range x##{id}b"), 0.1, 4.0, &mut b.range);
    ui.slider(format!("Core x##{id}b"), 0.1, 4.0, &mut b.core);
    ui.slider(
        format!("Inner Angle +deg##{id}b"),
        -60.0,
        60.0,
        &mut b.inner_add,
    );
    ui.slider(
        format!("Outer Angle +deg##{id}b"),
        -60.0,
        60.0,
        &mut b.outer_add,
    );
    ui.slider(format!("Yaw (deg, left +)##{id}b"), -30.0, 30.0, &mut b.yaw);
    ui.slider(
        format!("Pitch (deg, up +)##{id}b"),
        -30.0,
        30.0,
        &mut b.pitch,
    );
    ui.slider(format!("Forward (m)##{id}b"), -5.0, 5.0, &mut b.forward);
    ui.slider(format!("Right (m)##{id}b"), -3.0, 3.0, &mut b.side);
    ui.slider(format!("Height (m)##{id}b"), -3.0, 3.0, &mut b.height);
    ui.color_edit3(format!("Color##{id}b"), &mut b.color);
}
