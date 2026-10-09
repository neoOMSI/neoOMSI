//! Options pages.

use super::*;

pub(crate) fn look_options_page(app: &App) -> Page {
    let rows: Vec<(String, String)> = vec![
        switch_row(
            Some(app),
            "free_look",
            &tx("pause.options.free_look.free_look.name"),
            &tx("pause.options.free_look.free_look.desc"),
        ),
        switch_row(
            Some(app),
            "crosshair",
            &tx("pause.options.free_look.crosshair.name"),
            &tx("pause.options.free_look.crosshair.desc"),
        ),
        switch_row(
            Some(app),
            "tooltips",
            &tx("pause.options.free_look.tooltips.name"),
            &tx("pause.options.free_look.tooltips.desc"),
        ),
    ]
    .into_iter()
    .flatten()
    .collect();
    (tx("pause.options.group.free_look"), rows)
}
pub(crate) fn map_options_page(app: &App) -> Page {
    let file = settings_file();
    let rows: Vec<(String, String)> = vec![
        switch_row(
            Some(app),
            "navigator",
            &tx("pause.options.group.map"),
            &tx("pause.page.text.enables_disables_the_minimap"),
        ),
        switch_row(
            Some(app),
            "nav_topbar",
            &tx("pause.options.map.nav_topbar.name"),
            &tx("pause.options.map.nav_topbar.desc"),
        ),
        switch_row(
            Some(app),
            "nav_turn",
            &tx("pause.options.map.nav_turn.name"),
            &tx("pause.options.map.nav_turn.desc"),
        ),
        switch_row(
            Some(app),
            "nav_stoplist",
            &tx("pause.options.map.nav_stoplist.name"),
            &tx("pause.options.map.nav_stoplist.desc"),
        ),
        switch_row(
            Some(app),
            "nav_stops_ext",
            &tx("pause.options.map.nav_stops_ext.name"),
            &tx("pause.options.map.nav_stops_ext.desc"),
        ),
        switch_row(
            Some(app),
            "nav_ai",
            &tx("pause.options.map.nav_ai.name"),
            &tx("pause.options.map.nav_ai.desc"),
        ),
        select_row(
            &file,
            "navigator_corner",
            &tx("pause.options.map.navigator_corner.name"),
            &tx("pause.options.later"),
        ),
    ]
    .into_iter()
    .flatten()
    .collect();
    (tx("pause.options.group.map"), rows)
}

pub(crate) fn options_pages(app: &App) -> Vec<Page> {
    let file = settings_file();
    let pick = |key: &str, name: &str, desc: &str| select_row(&file, key, name, desc);
    let pct = |v: f32| format!("{:.0} %", v * 100.0);
    let cm = |v: f32| format!("{:+.0} cm", v * 100.0);
    let later_text = tx("pause.options.later");
    let later = later_text.as_str();
    let game: Vec<(String, String)> = vec![
        switch_row(
            Some(app),
            "auto_ibis",
            &tx("pause.options.gameplay.auto_ibis.name"),
            &tx("pause.options.gameplay.auto_ibis.desc"),
        ),
        switch_row(
            Some(app),
            "exact_fare",
            &tx("pause.options.gameplay.exact_fare.name"),
            &tx("pause.options.gameplay.exact_fare.desc"),
        ),
        pick("pax_motion", &tx("pause.options.gameplay.pax_motion.name"), &tx("pause.options.gameplay.pax_motion.desc")),
        switch_row(
            Some(app),
            "pax_ik",
            &tx("pause.options.gameplay.pax_ik.name"),
            &tx("pause.options.gameplay.pax_ik.desc"),
        ),
        pick("pax_models", &tx("pause.options.gameplay.pax_models.name"), &tx("pause.options.gameplay.pax_models.desc")),
        pick("boarding", &tx("pause.options.gameplay.boarding.name"), &tx("pause.options.gameplay.boarding.desc")),
        switch_row(
            Some(app),
            "pax_prefer_seats",
            &tx("pause.options.gameplay.pax_prefer_seats.name"),
            &tx("pause.options.gameplay.pax_prefer_seats.desc"),
        ),
        switch_row(
            Some(app),
            "pax_rear_entry",
            &tx("pause.page.text.boarding_at_the_rear_doors"),
            &tx("pause.page.text.passengers_who_need_no_ticket_from_the_driver_also_get_on_at_the_rear_doors"),
        ),
        pick("maintenance", &tx("pause.options.gameplay.maintenance.name"), later),
        switch_row(
            Some(app),
            "coll_objects",
            &tx("pause.options.gameplay.coll_objects.name"),
            &tx("pause.options.gameplay.coll_objects.desc"),
        ),
        switch_row(
            Some(app),
            "coll_vehicles",
            &tx("pause.options.gameplay.coll_vehicles.name"),
            &tx("pause.page.text.enables_disables_collisions_with_other_vehicles"),
        ),
        switch_row(
            Some(app),
            "collision_pedestrians",
            &tx("pause.options.gameplay.collision_pedestrians.name"),
            &tx("pause.options.gameplay.collision_pedestrians.desc"),
        ),
        pick("ai_unsched_factor", &tx("pause.options.gameplay.ai_unsched_factor.name"), later),
        pick("ai_max_scheduled", &tx("pause.options.gameplay.ai_max_scheduled.name"), later),
        pick("ai_max_parked", &tx("pause.options.gameplay.ai_max_parked.name"), later),
    ]
        .into_iter()
        .flatten()
        .collect();
    let driving: Vec<(String, String)> = vec![
        switch_row(
            Some(app),
            "auto_clutch",
            &tx("pause.options.driving.auto_clutch.name"),
            &tx("pause.options.driving.auto_clutch.desc"),
        ),
        switch_row(
            Some(app),
            "auto_shift",
            &tx("pause.options.driving.auto_shift.name"),
            &tx("pause.options.driving.auto_shift.desc"),
        ),
        switch_row(
            Some(app),
            "momentary_gears",
            &tx("pause.options.driving.momentary_gears.name"),
            later,
        ),
        switch_row(
            Some(app),
            "brake_hold",
            &tx("pause.options.driving.brake_hold.name"),
            &tx("pause.options.driving.brake_hold.desc"),
        ),
        switch_row(
            Some(app),
            "blinker_cancel",
            &tx("pause.options.driving.blinker_cancel.name"),
            &tx("pause.options.driving.blinker_cancel.desc"),
        ),
        switch_row(
            Some(app),
            "steering_linear",
            &tx("pause.options.driving.steering_linear.name"),
            &tx("pause.options.driving.steering_linear.desc"),
        ),
        switch_row(
            Some(app),
            "old_steering",
            &tx("pause.options.driving.old_steering.name"),
            &tx("pause.options.driving.old_steering.desc"),
        ),
        switch_row(
            Some(app),
            "red_steer_spd",
            &tx("pause.options.driving.red_steer_spd.name"),
            &tx("pause.options.driving.red_steer_spd.desc"),
        ),
    ]
    .into_iter()
    .flatten()
    .collect();
    let controls: Vec<(String, String)> = vec![
        Some(opens(
            &tx("pause.options.controls.keybinds.name"),
            &tx("pause.options.controls.keybinds.desc"),
            "keysopts",
        )),
        switch_row(
            Some(app),
            "mouse",
            &tx("pause.options.controls.mouse.name"),
            &tx("pause.options.controls.mouse.desc"),
        ),
        switch_row(
            Some(app),
            "mouse_right",
            &tx("pause.options.controls.mouse_right.name"),
            &tx("pause.options.controls.mouse_right.desc"),
        ),
        slider_row(
            Some(app),
            "mouse_sens",
            &tx("pause.options.controls.mouse_sens.name"),
            &tx("pause.options.controls.mouse_sens.desc"),
            &pct,
        ),
        slider_row(
            Some(app),
            "stick_sens",
            &tx("pause.options.controls.stick_sens.name"),
            &tx("pause.options.controls.stick_sens.desc"),
            &pct,
        ),
        switch_row(
            Some(app),
            "steer_center",
            &tx("pause.options.controls.steer_center.name"),
            &tx("pause.options.controls.steer_center.desc"),
        ),
        slider_row(
            Some(app),
            "pedal_t",
            &tx("pause.options.controls.pedal_t.name"),
            &tx("pause.options.controls.pedal_t.desc"),
            &|v| format!("x{v}"),
        ),
        slider_row(
            Some(app),
            "pedal_b",
            &tx("pause.options.controls.pedal_b.name"),
            &tx("pause.options.controls.pedal_b.desc"),
            &|v| format!("x{v}"),
        ),
        slider_row(
            Some(app),
            "wheel_range",
            &tx("pause.options.controls.wheel_range.name"),
            &tx("pause.options.controls.wheel_range.desc"),
            &|v| format!("{v:.0}°"),
        ),
        slider_row(
            Some(app),
            "wheel_lock",
            &tx("pause.options.controls.wheel_lock.name"),
            &tx("pause.options.controls.wheel_lock.desc"),
            &|v| {
                if v < 45.0 {
                    "OMSI".to_string()
                } else {
                    format!("{v:.0}°")
                }
            },
        ),
        switch_row(
            Some(app),
            "ff",
            &tx("pause.options.controls.ff.name"),
            &tx("pause.options.controls.ff.desc"),
        ),
        switch_row(
            Some(app),
            "ff_invert",
            &tx("pause.options.controls.ff_invert.name"),
            &tx("pause.options.controls.ff_invert.desc"),
        ),
    ]
    .into_iter()
    .flatten()
    .collect();
    let mut camera: Vec<(String, String)> = vec![
        slider_row(
            Some(app),
            "fov",
            &tx("pause.options.camera.fov.name"),
            &tx("pause.options.camera.fov.desc"),
            &|v| {
                if v < 20.0 {
                    tx("pause.options.fmt.default")
                } else {
                    format!("{v:.0}°")
                }
            },
        ),
        switch_row(
            Some(app),
            "head",
            &tx("pause.options.camera.head.name"),
            &tx("pause.options.camera.head.desc"),
        ),
        switch_row(
            Some(app),
            "cam_smooth",
            &tx("pause.options.camera.cam_smooth.name"),
            &tx("pause.options.camera.cam_smooth.desc"),
        ),
        switch_row(
            Some(app),
            "camcoll",
            &tx("pause.options.camera.camcoll.name"),
            &tx("pause.options.camera.camcoll.desc"),
        ),
        slider_row(
            Some(app),
            "look_sens",
            &tx("pause.options.camera.look_sens.name"),
            &tx("pause.options.camera.look_sens.desc"),
            &pct,
        ),
        switch_row(
            Some(app),
            "alt_view",
            &tx("pause.options.camera.alt_view.name"),
            &tx("pause.options.camera.alt_view.desc"),
        ),
        toggle_now(Some(app), "free_look").map(|on| {
            (
                row(
                    &tx("pause.options.free_look.free_look.name"),
                    'm',
                    if on { "on" } else { "off" },
                    &tx("pause.options.camera.free_look.desc"),
                    None,
                ),
                "lookopts".to_string(),
            )
        }),
        switch_row(
            Some(app),
            "steer_look",
            &tx("pause.options.camera.steer_look.name"),
            &tx("pause.options.camera.steer_look.desc"),
        ),
        slider_row(
            Some(app),
            "steer_look_angle",
            &tx("pause.options.camera.steer_look_angle.name"),
            &tx("pause.options.camera.steer_look_angle.desc"),
            &|v| format!("{v:.0}°"),
        ),
        slider_row(
            Some(app),
            "steer_look_response",
            &tx("pause.options.camera.steer_look_response.name"),
            &tx("pause.options.camera.steer_look_response.desc"),
            &|v| format!("{:.0} ms", v * 1000.0),
        ),
        switch_row(
            Some(app),
            "headtrack",
            &tx("pause.options.camera.headtrack.name"),
            &::i18n::translate(
                "pause.options.camera.headtrack.desc_port",
                &[(
                    "port",
                    &::config::get_int("camera", "head_tracking_port")
                        .and_then(|v| u16::try_from(v).ok())
                        .unwrap_or(4242),
                )],
            ),
        ),
        switch_row(
            Some(app),
            "hands_in_cab",
            &tx("pause.options.camera.hands_in_cab.name"),
            &tx("pause.options.camera.hands_in_cab.desc"),
        ),
        switch_row(
            Some(app),
            "driver",
            &tx("pause.options.camera.driver.name"),
            &tx("pause.options.camera.driver.desc"),
        ),
        slider_row(
            Some(app),
            "seat 1",
            &tx("pause.options.camera.seat_1.name"),
            &tx("pause.options.camera.seat_1.desc"),
            &cm,
        ),
        slider_row(
            Some(app),
            "seat 2",
            &tx("pause.options.camera.seat_2.name"),
            &tx("pause.options.camera.seat_2.desc"),
            &cm,
        ),
        slider_row(
            Some(app),
            "seat 0",
            &tx("pause.options.camera.seat_0.name"),
            &tx("pause.options.camera.seat_0.desc"),
            &cm,
        ),
    ]
    .into_iter()
    .flatten()
    .collect();
    camera.push(button(
        &tx("pause.options.camera.seat_reset.name"),
        &tx("pause.options.button.reset"),
        &tx("pause.options.camera.seat_reset.desc"),
        "seat_reset",
    ));
    let graphics: Vec<(String, String)> = vec![
        preset_row(
            &tx("pause.options.graphics.preset.name"),
            &tx("pause.options.graphics.preset.desc"),
        ),
        (!::config::get_subs("graphics_profiles").is_empty()).then(|| {
            opens(
                &tx("pause.options.graphics.gfxprofile.name"),
                &tx("pause.options.graphics.gfxprofile.desc"),
                "gfxprofile",
            )
        }),
        pick(
            "graphics",
            &tx("pause.options.graphics.graphics.name"),
            later,
        ),
        pick("msaa", &tx("pause.options.graphics.msaa.name"), later),
        pick(
            "render_scale",
            &tx("pause.options.graphics.render_scale.name"),
            later,
        ),
        pick(
            "anisotropy",
            &tx("pause.options.graphics.anisotropy.name"),
            later,
        ),
        switch_row(
            Some(app),
            "shadows",
            &tx("pause.options.graphics.shadows.name"),
            &tx("pause.page.text.enables_disabled_shadows"),
        ),
        pick(
            "shadow_size",
            &tx("pause.options.graphics.shadow_size.name"),
            later,
        ),
        pick(
            "shadow_casters",
            &tx("pause.options.graphics.shadow_casters.name"),
            later,
        ),
        switch_row(Some(app), "ssao", &tx("pause.options.graphics.ssao.name"), later),
        switch_row(
            Some(app),
            "reflections",
            &tx("pause.options.graphics.reflections.name"),
            later,
        ),
        switch_row(
            Some(app),
            "clouds",
            &tx("pause.options.graphics.clouds.name"),
            later,
        ),
        switch_row(
            Some(app),
            "detail_textures",
            &tx("pause.options.graphics.detail_textures.name"),
            &tx("pause.options.graphics.detail_textures.desc"),
        ),
        pick(
            "map_detail",
            &tx("pause.options.graphics.map_detail.name"),
            later,
        ),
        pick(
            "view_distance",
            &tx("pause.options.graphics.view_distance.name"),
            later,
        ),
        pick(
            "max_obj_dist",
            &tx("pause.options.graphics.max_obj_dist.name"),
            later,
        ),
        pick(
            "min_obj_size",
            &tx("pause.options.graphics.min_obj_size.name"),
            later,
        ),
        pick(
            "mirror_size",
            &tx("pause.options.graphics.mirror_size.name"),
            later,
        ),
        pick(
            "texture_memory",
            &tx("pause.options.graphics.texture_memory.name"),
            later,
        ),
        switch_row(
            Some(app),
            "texture_compression",
            &tx("pause.options.graphics.texture_compression.name"),
            later,
        ),
        slider_row(
            Some(app),
            "led_glow",
            &tx("pause.options.graphics.led_glow.name"),
            &tx("pause.options.graphics.led_glow.desc"),
            &|v| format!("{}/15", v as i64),
        ),
        slider_row(
            Some(app),
            "nightmap_glow",
            &tx("pause.options.graphics.nightmap_glow.name"),
            &tx("pause.options.graphics.nightmap_glow.desc"),
            &|v| format!("{}/15", v as i64),
        ),
        slider_row(
            Some(app),
            "atmosphere_brightness",
            &tx("pause.options.graphics.atmosphere_brightness.name"),
            &tx("pause.options.graphics.atmosphere_brightness.desc"),
            &|v| format!("{v:.2}"),
        ),
        slider_row(
            Some(app),
            "led_mips",
            &tx("pause.options.graphics.led_mips.name"),
            &tx("pause.options.graphics.led_mips.desc"),
            &|v| format!("{v:.2}"),
        ),
    ]
    .into_iter()
    .flatten()
    .collect();
    let display: Vec<(String, String)> = vec![
        pick(
            "window_mode",
            &tx("pause.options.display.window_mode.name"),
            &tx("pause.options.display.window_mode.desc"),
        ),
        switch_row(
            Some(app),
            "triple_screen",
            &tx("pause.options.display.triple_screen.name"),
            &tx("pause.options.display.triple_screen.desc"),
        ),
        switch_row(
            Some(app),
            "triple_screen_span",
            &tx("pause.options.display.triple_screen_span.name"),
            &tx("pause.options.display.triple_screen_span.desc"),
        ),
        switch_row(
            Some(app),
            "triple_screen_hud",
            &tx("pause.options.display.triple_screen_hud.name"),
            &tx("pause.options.display.triple_screen_hud.desc"),
        ),
        slider_row(
            Some(app),
            "triple_screen_width_mm",
            &tx("pause.options.display.triple_screen_width_mm.name"),
            &tx("pause.options.display.triple_screen_width_mm.desc"),
            &|v| format!("{v:.0} mm"),
        ),
        slider_row(
            Some(app),
            "triple_screen_distance_mm",
            &tx("pause.options.display.triple_screen_distance_mm.name"),
            &tx("pause.options.display.triple_screen_distance_mm.desc"),
            &|v| format!("{v:.0} mm"),
        ),
        slider_row(
            Some(app),
            "triple_screen_bezel_mm",
            &tx("pause.options.display.triple_screen_bezel_mm.name"),
            &tx("pause.options.display.triple_screen_bezel_mm.desc"),
            &|v| format!("{v:.0} mm"),
        ),
        slider_row(
            Some(app),
            "triple_screen_left_angle",
            &tx("pause.options.display.triple_screen_left_angle.name"),
            &tx("pause.options.display.triple_screen_left_angle.desc"),
            &|v| format!("{v:.0}°"),
        ),
        slider_row(
            Some(app),
            "triple_screen_right_angle",
            &tx("pause.options.display.triple_screen_right_angle.name"),
            &tx("pause.options.display.triple_screen_right_angle.desc"),
            &|v| format!("{v:.0}°"),
        ),
        slider_row(
            Some(app),
            "triple_screen_eye_height_mm",
            &tx("pause.options.display.triple_screen_eye_height_mm.name"),
            &tx("pause.options.display.triple_screen_eye_height_mm.desc"),
            &|v| format!("{v:.0} mm"),
        ),
        switch_row(
            Some(app),
            "vsync",
            &tx("pause.options.display.vsync.name"),
            &tx("pause.options.display.vsync.desc"),
        ),
        pick(
            "max_fps",
            &tx("pause.options.display.max_fps.name"),
            &tx("pause.options.display.max_fps.desc"),
        ),
        switch_row(
            Some(app),
            "fps",
            &tx("pause.options.display.fps.name"),
            &tx("pause.options.display.fps.desc"),
        ),
    ]
    .into_iter()
    .flatten()
    .collect();
    let sound: Vec<(String, String)> = vec![
        slider_row(
            Some(app),
            "volume",
            &tx("pause.options.sound.volume.name"),
            &tx("pause.options.sound.volume.desc"),
            &pct,
        ),
        slider_row(
            Some(app),
            "vol_ai",
            &tx("pause.options.sound.vol_ai.name"),
            &tx("pause.options.sound.vol_ai.desc"),
            &pct,
        ),
        slider_row(
            Some(app),
            "vol_scenery",
            &tx("pause.options.sound.vol_scenery.name"),
            &tx("pause.options.sound.vol_scenery.desc"),
            &pct,
        ),
        switch_row(
            Some(app),
            "doppler",
            &tx("pause.options.sound.doppler.name"),
            &tx("pause.options.sound.doppler.desc"),
        ),
        pick(
            "pax_voices",
            &tx("pause.options.sound.pax_voices.name"),
            &tx("pause.options.sound.pax_voices.desc"),
        ),
    ]
    .into_iter()
    .flatten()
    .collect();
    let interface: Vec<(String, String)> = vec![
        pick(
            "language",
            &tx("pause.options.interface.language.name"),
            &tx("pause.options.interface.language.desc"),
        ),
        pick(
            "units",
            &tx("pause.options.interface.units.name"),
            &tx("pause.options.interface.units.desc"),
        ),
        slider_row(
            Some(app),
            "ui_scale",
            &tx("pause.options.interface.ui_scale.name"),
            &tx("pause.options.interface.ui_scale.desc"),
            &pct,
        ),
        switch_row(
            Some(app),
            "ui_scale_window",
            &tx("pause.options.interface.ui_scale_window.name"),
            &tx("pause.options.interface.ui_scale_window.desc"),
        ),
        slider_row(
            Some(app),
            "ui_opacity",
            &tx("pause.options.interface.ui_opacity.name"),
            &tx("pause.options.interface.ui_opacity.desc"),
            &pct,
        ),
        toggle_now(Some(app), "navigator").map(|on| {
            (
                row(
                    &tx("pause.options.vr.navigator.name"),
                    'm',
                    if on { "on" } else { "off" },
                    &tx("pause.options.interface.navigator.desc"),
                    None,
                ),
                "mapopts".to_string(),
            )
        }),
        switch_row(
            Some(app),
            "nav_arrows",
            &tx("pause.options.interface.nav_arrows.name"),
            &tx("pause.options.interface.nav_arrows.desc"),
        ),
        switch_row(
            Some(app),
            "info_bar",
            &tx("pause.options.interface.info_bar.name"),
            &tx("pause.options.interface.info_bar.desc"),
        ),
        switch_row(
            Some(app),
            "timetable_win",
            &tx("pause.options.interface.timetable_win.name"),
            &tx("pause.options.interface.timetable_win.desc"),
        ),
        switch_row(
            Some(app),
            "notes",
            &tx("pause.options.interface.notes.name"),
            &tx("pause.options.interface.notes.desc"),
        ),
        switch_row(
            Some(app),
            "tooltips",
            &tx("pause.options.interface.tooltips.name"),
            &tx("pause.options.interface.tooltips.desc"),
        ),
        switch_row(
            Some(app),
            "chat",
            &tx("pause.options.interface.chat.name"),
            &tx("pause.options.interface.chat.desc"),
        ),
        switch_row(
            Some(app),
            "name_tags",
            &tx("pause.options.interface.name_tags.name"),
            &tx("pause.options.interface.name_tags.desc"),
        ),
        Some(opens(
            &tx("pause.options.interface.reset.name"),
            &tx("pause.options.interface.reset.desc"),
            "reset",
        )),
    ]
    .into_iter()
    .flatten()
    .collect();
    let mut vr: Vec<(String, String)> = Vec::new();
    let vr_on = ::config::get_bool("vr", "enabled").unwrap_or(false);
    if cfg!(windows) {
        vr.extend(
            vec![
                switch_row(Some(app), "vr", &tx("pause.options.vr.vr.name"), later),
                if vr_on {
                    pick("vr_scale", &tx("pause.options.vr.vr_scale.name"), later)
                } else {
                    None
                },
                if vr_on {
                    pick(
                        "vr_head_smoothing_ms",
                        &tx("pause.options.vr.vr_head_smoothing_ms.name"),
                        later,
                    )
                } else {
                    None
                },
                if vr_on {
                    pick(
                        "vr_mirror_rate",
                        &tx("pause.options.vr.vr_mirror_rate.name"),
                        later,
                    )
                } else {
                    None
                },
                if vr_on {
                    switch_row(
                        Some(app),
                        "vr_desktop_mirror",
                        &tx("pause.options.vr.vr_desktop_mirror.name"),
                        later,
                    )
                } else {
                    None
                },
            ]
            .into_iter()
            .flatten(),
        );
    }
    if app.vr_active() && app.player.is_some() {
        let desc = &tx("pause.options.vr.navigator.desc");
        vr.extend(switch_row(
            Some(app),
            "navigator",
            &tx("pause.options.vr.navigator.name"),
            desc,
        ));
        vr.push(button(
            &tx("pause.options.vr.vr_nav_edit.name"),
            &tx("pause.options.button.open"),
            desc,
            "vr_nav_edit",
        ));
        for (id, label) in [
            ("x", &tx("pause.options.vr.vr_nav_x.name")),
            ("y", &tx("pause.options.vr.vr_nav_y.name")),
            ("z", &tx("pause.options.vr.vr_nav_z.name")),
            ("width", &tx("pause.options.vr.vr_nav_width.name")),
        ] {
            vr.extend(slider_row(Some(app), &format!("vr_nav_{id}"), label, desc, &cm));
        }
        for (id, label) in [
            ("yaw", &tx("pause.options.vr.vr_nav_yaw.name")),
            ("tilt", &tx("pause.options.vr.vr_nav_tilt.name")),
            ("roll", &tx("pause.options.vr.vr_nav_roll.name")),
        ] {
            vr.extend(slider_row(
                Some(app),
                &format!("vr_nav_{id}"),
                label,
                desc,
                &|v| format!("{v:.0}°"),
            ));
        }
        vr.extend(slider_row(
            Some(app),
            "vr_nav_opacity",
            &tx("pause.options.interface.ui_opacity.name"),
            desc,
            &pct,
        ));
        vr.push(button(
            &tx("pause.options.vr.vr_nav_reset.name"),
            &tx("pause.options.button.reset"),
            desc,
            "vr_nav_reset",
        ));
    }
    vec![
        (tx("pause.options.group.gameplay"), game),
        (tx("pause.options.group.driving"), driving),
        (tx("pause.options.group.controls"), controls),
        (tx("pause.options.group.camera"), camera),
        (tx("pause.options.group.graphics"), graphics),
        (tx("pause.options.group.display"), display),
        (tx("pause.options.group.sound"), sound),
        (tx("pause.options.group.interface"), interface),
        (tx("pause.options.group.vr"), vr),
    ]
}
