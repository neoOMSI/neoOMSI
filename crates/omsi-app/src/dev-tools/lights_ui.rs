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
            if ui.collapsing_header("Lamp Light (Street Lamps)", imgui::TreeNodeFlags::DEFAULT_OPEN) {
                let m = &mut s.lamp_light;
                ui.checkbox("Lamps throw light##ll", &mut m.on);
                ui.slider("Intensity x##ll", 0.0, 4.0, &mut m.gain);
                ui.slider("Reach (m)##ll", 1.0, 60.0, &mut m.range);
                ui.slider("Core (m)##ll", 0.1, 10.0, &mut m.core);
                ui.slider("Max lamps##ll", 0, 128, &mut m.max);
                if ui.button("Reset Lamp Light##ll") {
                    *m = crate::lights::LampLightCfg::DEFAULT;
                }
            }
            if ui.collapsing_header("Spot Shape (optional, for those lamps)", imgui::TreeNodeFlags::empty()) {
                let m = &mut s.map_spot;
                ui.checkbox("Spot Mode##ms", &mut m.on);
                ui.slider("Intensity x##ms", 0.0, 4.0, &mut m.gain);
                ui.slider("Range x##ms", 0.1, 4.0, &mut m.range);
                ui.slider("Core x##ms", 0.1, 4.0, &mut m.core);
                ui.slider("Inner Angle (deg)##ms", 1.0, 179.0, &mut m.inner);
                ui.slider("Outer Angle (deg)##ms", 1.0, 179.0, &mut m.outer);
                ui.slider("Tilt (deg, 0 = down)##ms", -90.0, 90.0, &mut m.tilt);
                ui.slider("Turn (deg)##ms", -180.0, 180.0, &mut m.yaw);
                ui.slider("Height (m)##ms", -3.0, 3.0, &mut m.height);
                if ui.button("Reset Spot##ms") {
                    *m = crate::lights::MapSpotCfg::DEFAULT;
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
    s.map_spot = d.map_spot;
    s.lamp_light = d.lamp_light;
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

pub(super) fn spots_panel(
    ui: &imgui::Ui,
    s: &mut crate::lights::LightSettings,
    extra: &Extra,
) {
    if ui.collapsing_header("Window Light (lit saloon)", imgui::TreeNodeFlags::DEFAULT_OPEN) {
        let m = &mut s.spill;
        ui.checkbox("Enabled##sp", &mut m.on);
        ui.checkbox("Show Markers (orange)##sp", &mut m.marker);
        ui.slider("Intensity x##sp", 0.0, 4.0, &mut m.gain);
        ui.slider("Range x##sp", 0.1, 4.0, &mut m.range);
        ui.slider("Core x##sp", 0.1, 4.0, &mut m.core);
        ui.slider("Spread x (cone width)##sp", 0.2, 3.0, &mut m.spread);
        ui.slider("Inner Angle +deg##sp", -20.0, 60.0, &mut m.inner_add);
        ui.slider("Outer Angle +deg##sp", -40.0, 90.0, &mut m.outer_add);
        ui.slider("Tilt +deg (down)##sp", -30.0, 60.0, &mut m.tilt_add);
        ui.slider("Shines up to (m from body)##sp", 5.0, 300.0, &mut m.reach);
        ui.slider("Max vehicles##sp", 0, 256, &mut m.vehicles);
        if ui.button("Reset Window Light##sp") {
            *m = crate::lights::SpillCfg::DEFAULT;
        }
    }
    if ui.collapsing_header("Extra Spotlights ([spotlight_2])", imgui::TreeNodeFlags::DEFAULT_OPEN) {
        let m = &mut s.spot2;
        ui.checkbox("Enabled##s2", &mut m.on);
        ui.checkbox("Show Markers (green)##s2", &mut m.marker);
        ui.slider("Intensity x##s2", 0.0, 4.0, &mut m.gain);
        ui.slider("Range x##s2", 0.1, 4.0, &mut m.range);
        ui.slider("Core x##s2", 0.1, 4.0, &mut m.core);
        ui.slider("Inner Angle +deg##s2", -60.0, 60.0, &mut m.inner_add);
        ui.slider("Outer Angle +deg##s2", -60.0, 60.0, &mut m.outer_add);
        ui.slider("Height (m)##s2", -3.0, 3.0, &mut m.height);
        if ui.button("Reset Extra Spotlights##s2") {
            *m = crate::lights::Spot2Cfg::DEFAULT;
        }
    }
    let Some(v) = extra.vehicle.as_ref() else {
        return;
    };
    if ui.collapsing_header("Outside Light Sources", imgui::TreeNodeFlags::DEFAULT_OPEN) {
        ui.text("All outside sources (global)");
        let m = &mut s.src;
        ui.checkbox("Sources throw light##src", &mut m.on);
        ui.slider("Intensity x##src", 0.0, 4.0, &mut m.gain);
        ui.slider("Spread x (how far)##src", 0.1, 4.0, &mut m.spread);
        ui.slider("Core x##src", 0.1, 4.0, &mut m.core);
        ui.checkbox("Only the way it faces (cone)##src", &mut m.directional);
        if m.directional {
            ui.slider("Inner Angle (deg)##src", 1.0, 179.0, &mut m.inner);
            ui.slider("Outer Angle (deg)##src", 1.0, 179.0, &mut m.outer);
        } else {
            ui.text_disabled("Everywhere (all round)");
        }
        if ui.button("Reset Source Light##src") {
            *m = crate::lights::SourceCfg::DEFAULT;
        }
        ui.separator();
        if ui.button("Reset Outside Sources") {
            crate::lights::reset_exterior_cfg();
        }
        ui.same_line();
        ui.text_disabled(format!("{} sources", v.exterior.len()));
        for (i, src) in v.exterior.iter().enumerate() {
            let mut c = crate::lights::exterior_cfg(i);
            let label = format!("E{i} {}##el{i}", src.variable);
            if ui.collapsing_header(label, imgui::TreeNodeFlags::empty()) {
                ui.text_disabled(format!(
                    "pos {:.2} {:.2} {:.2}  size {:.2}  color {:.0} {:.0} {:.0}",
                    src.pos[0],
                    src.pos[1],
                    src.pos[2],
                    src.range,
                    src.color[0],
                    src.color[1],
                    src.color[2]
                ));
                let mut on = !c.off;
                ui.checkbox(format!("Enabled##el{i}"), &mut on);
                c.off = !on;
                ui.slider(format!("Intensity x##el{i}"), 0.0, 4.0, &mut c.gain);
                ui.slider(format!("Size x##el{i}"), 0.0, 4.0, &mut c.size);
                ui.slider(format!("Spread x##el{i}"), 0.1, 4.0, &mut c.spread);
                ui.slider(format!("Right (m)##el{i}"), -3.0, 3.0, &mut c.shift[0]);
                ui.slider(format!("Forward (m)##el{i}"), -3.0, 3.0, &mut c.shift[1]);
                ui.slider(format!("Height (m)##el{i}"), -3.0, 3.0, &mut c.shift[2]);
                ui.color_edit3(format!("Tint##el{i}"), &mut c.color);
                if ui.button(format!("Reset##el{i}")) {
                    c = crate::lights::ExteriorCfg::DEFAULT;
                }
            }
            crate::lights::set_exterior_cfg(i, c);
        }
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
        let (sp, s2) = (s.spill, s.spot2);
        let (wb, wn, co, ms, ll) = (
            s.weather_boost,
            s.weather_night,
            s.corona,
            s.map_spot,
            s.lamp_light,
        );
        *s = d;
        s.map_spot = ms;
        s.spill = sp;
        s.spot2 = s2;
        s.lamp_light = ll;
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
