//! Reading and writing the values of the sliders and switches of the options windows.

use crate::{controllers, player, weather_setup};
use super::*;

pub(super) const SPEEDS: [f64; 5] = [1.0, 2.0, 4.0, 8.0, 15.0];
pub(crate) const TRAFFIC: [usize; 7] = [0, 10, 20, 30, 50, 80, 120];
pub(super) const PAX: [f32; 6] = [0.25, 0.5, 0.75, 1.0, 1.5, 2.0];
pub(super) const VOLUME: [f32; 6] = [0.0, 0.2, 0.4, 0.6, 0.8, 1.0];
pub(super) const PEDAL: [f32; 7] = [0.5, 0.7, 0.85, 1.0, 1.25, 1.5, 2.0];

pub(crate) fn next_step<T: PartialOrd + Copy>(steps: &[T], now: T) -> T {
    steps.iter().copied().find(|s| *s > now).unwrap_or(steps[0])
}

pub(super) fn steps_of(verb: &str) -> Option<Vec<f32>> {
    Some(match verb {
        "vr_nav_x" | "vr_nav_y" | "vr_nav_z" => (-100..=100).map(|v| v as f32 * 0.02).collect(),
        "vr_nav_width" => (12..=65).map(|v| v as f32 * 0.01).collect(),
        "vr_nav_yaw" | "vr_nav_roll" => (-90..=90).map(|v| v as f32 * 2.0).collect(),
        "vr_nav_tilt" => (-40..=40).map(|v| v as f32 * 2.0).collect(),
        "vr_nav_opacity" => (6..=20).map(|v| v as f32 * 0.05).collect(),
        "speed" => SPEEDS.iter().map(|&v| v as f32).collect(),
        "traffic" => TRAFFIC.iter().map(|&v| v as f32).collect(),
        "pax" => PAX.to_vec(),
        "volume" => VOLUME.to_vec(),
        "led_glow" => (0..16).map(|v| v as f32).collect(),
        "nightmap_glow" => (0..16).map(|v| v as f32).collect(),
        "led_mips" => (0..=80).map(|v| v as f32 * 0.05).collect(),
        "atmosphere_brightness" => (0..=40).map(|v| v as f32 * 0.05).collect(),
        "ui_scale" => (10..=40).map(|v| v as f32 * 0.05).collect(),
        "ui_opacity" => (4..=20).map(|v| v as f32 * 0.05).collect(),
        "vol_ai" | "vol_scenery" => (0..=20).map(|v| v as f32 * 0.05).collect(),
        "wheel_range" => (6..=96).map(|v| v as f32 * 30.0).collect(),
        "wheel_lock" => std::iter::once(0.0)
            .chain((2..=96).map(|v| v as f32 * 30.0))
            .collect(),
        "fov" => std::iter::once(0.0)
            .chain((20..=120).map(|v| v as f32))
            .collect(),
        "steer_look_angle" => (0..=60).map(|v| v as f32).collect(),
        "steer_look_response" => (1..=20).map(|v| v as f32 * 0.05).collect(),
        "pedal_t" | "pedal_b" => PEDAL.to_vec(),
        "mouse_sens" => (10..=300).map(|v| v as f32 / 100.0).collect(),
        "stick_sens" => (2..=40).map(|v| v as f32 * 0.05).collect(),
        "look_sens" => (2..=40).map(|v| v as f32 * 0.05).collect(),
        "seat" => (-50..=50).map(|v| v as f32 / 100.0).collect(),
        "hour" => (0..24).map(|v| v as f32).collect(),
        "minute" => (0..60).map(|v| v as f32).collect(),
        "visibility" => {
            let mut v: Vec<f32> = (0..=120)
                .map(|i| {
                    let x = 100.0 * 500f32.powf(i as f32 / 120.0);
                    let step = if x < 1000.0 {
                        10.0
                    } else if x < 10000.0 {
                        100.0
                    } else {
                        500.0
                    };
                    (x / step).round() * step
                })
                .collect();
            v.dedup();
            v
        }
        "rain_amt" | "wet" => (0..=100).map(|v| v as f32 / 100.0).collect(),
        "brightness" => (0..=30).map(|v| v as f32 * 0.05).collect(),
        "humidity" => (0..=100).map(|v| v as f32).collect(),
        "temp" => (-20..=45).map(|v| v as f32).collect(),
        "wind_speed" => (0..=25).map(|v| v as f32).collect(),
        "wind_dir" => (0..360).map(|v| v as f32).collect(),
        _ => return None,
    })
}

pub(super) const CLOUD_TYPES: [(&str, &str); 5] = [
    ("-1", "None"),
    ("Cumulus 1", "Few clouds"),
    ("Cumulus 2", "Scattered"),
    ("Cumulus 3", "Broken"),
    ("Overcast 1", "Overcast"),
];

pub(super) const PRECIP_KINDS: [&str; 3] = ["None", "Rain", "Snow"];

pub(crate) const CUSTOM_WEATHER: &str = "Custom weather";

pub(super) fn cloud_index(kind: &str) -> Option<usize> {
    let k = kind.trim();
    CLOUD_TYPES.iter().position(|(id, _)| {
        id.eq_ignore_ascii_case(k) || (*id == "-1" && (k.is_empty() || k.starts_with("-1")))
    })
}

pub(super) fn custom_state(app: &App) -> weather_setup::CustomWeather {
    if let Some(mut c) = weather_setup::custom_weather(app.args.weather.as_deref()) {
        // Wetness keeps evolving while driving; never restore an old serialized value just
        // because another custom field (brightness, humidity, etc.) was edited.
        c.road_wetness = app.wetness;
        return c;
    }
    match app.weather.as_ref() {
        Some(w) => weather_setup::CustomWeather::from_weather(w, 1.0, app.wetness),
        None => weather_setup::CustomWeather::default(),
    }
}

pub(crate) fn set_precip(app: &mut App, to: usize) {
    let to = to.min(PRECIP_KINDS.len() - 1);
    app.edit_weather(|w| {
        w.precip[0] = to as f32;
        w.snow = to == 2;
        // (rain or snow with no strength would be nothing: a moderate one)
        if to != 0 && w.precip[1] < 1.0 {
            w.precip[1] = 100.0;
        }
    });
}

pub(crate) fn is_slider(verb: &str) -> bool {
    steps_of(verb).is_some()
}

pub(super) fn nearest(steps: &[f32], now: f32) -> usize {
    steps
        .iter()
        .enumerate()
        .min_by(|a, b| (a.1 - now).abs().total_cmp(&(b.1 - now).abs()))
        .map(|x| x.0)
        .unwrap_or(0)
}

pub(super) fn step_move(steps: &[f32], now: f32, mv: Move) -> f32 {
    let n = steps.len().max(1);
    let i = nearest(steps, now);
    let to = match mv {
        Move::Next => (i + 1) % n,
        Move::Inc => (i + 1).min(n - 1),
        Move::Dec => i.saturating_sub(1),
        Move::To(f) => (f.clamp(0.0, 1.0) * (n - 1) as f32).round() as usize,
    };
    steps.get(to).copied().unwrap_or(now)
}

pub(super) fn option_now(app: &App, verb: &str, arg: &str) -> Option<f32> {
    if let Some(field) = verb.strip_prefix("vr_nav_") {
        return if app.vr_active() && app.player.is_some() {
            app.vr_nav_profile().value(field)
        } else {
            None
        };
    }
    Some(match verb {
        "speed" => ::config::get_float("gameplay", "time_speed").unwrap_or(1.0) as f32,
        "traffic" => app.traffic.as_ref()?.target as f32,
        "pax" => ::config::get_float("passengers", "density").unwrap_or(1.0) as f32,
        "volume" => ::config::get_float("audio", "master-volume").unwrap_or(1.0) as f32,
        "led_glow" => ::config::get_int("graphics", "led_glow").unwrap_or(6) as u8 as f32,
        "nightmap_glow" => ::config::get_int("graphics", "nightmap_glow").unwrap_or(6) as u8 as f32,
        "led_mips" => ::config::get_float("graphics", "led_mips").unwrap_or(1.3) as f32,
        "atmosphere_brightness" => ::config::get_float("graphics", "atmosphere_brightness").unwrap_or(1.0) as f32,
        "pedal_t" => ::config::get_float("controls", "pedal_throttle").unwrap_or(1.0) as f32,
        "pedal_b" => ::config::get_float("controls", "pedal_brake").unwrap_or(1.0) as f32,
        "mouse_sens" => ::config::get_float("controls", "mouse_sens").unwrap_or(1.0) as f32,
        "stick_sens" => ::config::get_float("controls", "stick_sens").unwrap_or(1.0) as f32,
        "look_sens" => ::config::get_float("camera", "look_sens").unwrap_or(1.0) as f32,
        "ui_scale" => ::config::get_float("ui", "scale").unwrap_or(1.0) as f32,
        "ui_opacity" => ::config::get_float("ui", "opacity").unwrap_or(0.85).clamp(0.2, 1.0) as f32,
        "vol_ai" => ::config::get_float("audio", "ai-volume").unwrap_or(1.0) as f32,
        "vol_scenery" => ::config::get_float("audio", "scenery-volume").unwrap_or(1.0) as f32,
        "wheel_range" => ::config::get_float("controls", "wheel_range").unwrap_or(900.0) as f32,
        "wheel_lock" => ::config::get_float("controls", "wheel_lock").unwrap_or(0.0) as f32,
        "fov" => ::config::get_float("camera", "fov").unwrap_or(0.0) as f32,
        "steer_look_angle" => ::config::get_float("camera", "steer_look_angle").unwrap_or(30.0) as f32,
        "steer_look_response" => ::config::get_float("camera", "steer_look_response").unwrap_or(0.25) as f32,
        "seat" => ["seat_x", "seat_y", "seat_z"].map(|k| ::config::get_float("camera", k).unwrap_or(0.0) as f32)[arg.trim().parse::<usize>().unwrap_or(0).min(2)],
        "hour" => ((app.clock.time / 3600.0) as i64).rem_euclid(24) as f32,
        "minute" => (((app.clock.time / 60.0) as i64) % 60) as f32,
        "visibility" => app.weather.as_ref()?.fog.0,
        "rain_amt" => {
            let w = app.weather.as_ref()?;
            if w.precip.first().copied().unwrap_or(0.0) < 0.5 {
                0.0
            } else {
                (w.precip.get(1).copied().unwrap_or(0.0) / 255.0).clamp(0.0, 1.0)
            }
        }
        "wet" => app.wetness,
        "brightness" => custom_state(app).brightness,
        "humidity" => custom_state(app).humidity,
        "temp" => app.weather.as_ref()?.temp.0,
        "wind_speed" => app.weather.as_ref()?.wind.1,
        "wind_dir" => app.weather.as_ref()?.wind.0.rem_euclid(360.0),
        _ => return None,
    })
}

pub(super) fn option_set(
    app: &mut App,
    verb: &str,
    arg: &str,
    v: f32,
) -> Option<(&'static str, String)> {
    if let Some(field) = verb.strip_prefix("vr_nav_") {
        app.vr_nav_set(field, v);
        return None; // Stored per bus, never in the desktop settings file.
    }
    match verb {
        "speed" => {
            ::config::set_setting("gameplay", "time_speed", v as f64);
            let _ = ::config::save();
            None
        }
        "traffic" => {
            if let Some(t) = app.traffic.as_mut() {
                t.target = v.round() as usize;
                app.args.traffic = t.target;
            }
            None
        }
        "pax" => {
            ::config::set_setting("passengers", "density", v as f64);
            let _ = ::config::save();
            None
        }
        "volume" => {
            ::config::set_setting("audio", "master-volume", v);
            let _ = ::config::save();
            None
        }
        "led_glow" => {
            ::config::set_setting("graphics", "led_glow", v.round() as i64);
            let _ = ::config::save();
            None
        }
        "nightmap_glow" => {
            ::config::set_setting("graphics", "nightmap_glow", v.round() as i64);
            let _ = ::config::save();
            None
        }
        "atmosphere_brightness" => {
            ::config::set_setting("graphics", "atmosphere_brightness", v.clamp(0.0, 2.0) as f64);
            let _ = ::config::save();
            None
        }
        "led_mips" => {
            ::config::set_setting("graphics", "led_mips", v.clamp(0.0, 4.0) as f64);
            let _ = ::config::save();
            None
        }
        "pedal_t" => {
            ::config::set_setting("controls", "pedal_throttle", v as f64);
            let _ = ::config::save();
            None
        }
        "pedal_b" => {
            ::config::set_setting("controls", "pedal_brake", v as f64);
            let _ = ::config::save();
            None
        }
        "look_sens" => {
            ::config::set_setting("camera", "look_sens", ((v * 100.0).round() / 100.0) as f64);
            let _ = ::config::save();
            None
        }
        "mouse_sens" => {
            ::config::set_setting("controls", "mouse_sens", ((v * 100.0).round() / 100.0) as f64);
            let _ = ::config::save();
            None
        }
        "stick_sens" => {
            ::config::set_setting("controls", "stick_sens", ((v * 100.0).round() / 100.0) as f64);
            let _ = ::config::save();
            None
        }
        "ui_scale" => {
            ::config::set_setting("ui", "scale", ((v * 100.0).round() / 100.0) as f64);
            let _ = ::config::save();
            None
        }
        "ui_opacity" => {
            ::config::set_setting("ui", "opacity", ((v * 100.0).round() / 100.0) as f64);
            let _ = ::config::save();
            None
        }
        "vol_ai" => {
            let v = (v * 100.0).round() / 100.0;
            ::config::set_setting("audio", "ai-volume", v as f64);
            let _ = ::config::save();
            crate::startup::SOUND_AI.store(v.to_bits(), std::sync::atomic::Ordering::Relaxed);
            None
        }
        "vol_scenery" => {
            let v = (v * 100.0).round() / 100.0;
            ::config::set_setting("audio", "scenery-volume", v as f64);
            let _ = ::config::save();
            crate::startup::SOUND_SCENERY.store(v.to_bits(), std::sync::atomic::Ordering::Relaxed);
            None
        }
        "wheel_range" => {
            ::config::set_setting("controls", "wheel_range", v.round() as f64);
            let _ = ::config::save();
            None
        }
        "wheel_lock" => {
            ::config::set_setting("controls", "wheel_lock", (if v < 45.0 { 0.0 } else { v.round() }) as f64);
            let _ = ::config::save();
            None
        }
        "fov" => {
            ::config::set_setting("camera", "fov", (if v < 20.0 { 0.0 } else { v.round() }) as f64);
            let _ = ::config::save();
            None
        }
        "steer_look_angle" => {
            ::config::set_setting("camera", "steer_look_angle", v.round() as f64);
            let _ = ::config::save();
            None
        }
        "steer_look_response" => {
            ::config::set_setting("camera", "steer_look_response", ((v * 100.0).round() / 100.0) as f64);
            let _ = ::config::save();
            None
        }
        "seat" => {
            let k: usize = arg.trim().parse().unwrap_or(0).min(2);
            ::config::set_setting("camera", ["seat_x", "seat_y", "seat_z"][k], ((v * 100.0).round() / 100.0) as f64);
            let _ = ::config::save();
            None
        }
        "hour" | "minute" => {
            if app
                .lan
                .as_ref()
                .is_some_and(|l| l.role == ::network::Role::Client)
            {
                app.service_msg = Some((::i18n::translate("pause.msg.lan_clock", &[]), 3.0));
                return None;
            }
            let t = app.clock.time;
            let (h, m) = (
                ((t / 3600.0) as i64).rem_euclid(24),
                ((t / 60.0) as i64) % 60,
            );
            let (h, m) = if verb == "hour" {
                (v.round() as i64, m)
            } else {
                (h, v.round() as i64)
            };
            let target = (h * 3600 + m * 60) as f64 + t % 60.0;
            app.shift_clock(target - t);
            None
        }
        // the weather, made by hand (what the preset was stays as it was but for this)
        "visibility" => {
            app.edit_weather(|w| w.fog.0 = v);
            None
        }
        "rain_amt" => {
            app.edit_weather(|w| {
                w.precip[1] = (v * 255.0).round();
                if v > 0.0 && w.precip[0] < 0.5 {
                    w.precip[0] = 1.0;
                }
            });
            None
        }
        "wet" => {
            let mut c = custom_state(app);
            c.road_wetness = v;
            app.set_custom_weather(c);
            None
        }
        "brightness" => {
            let mut c = custom_state(app);
            c.brightness = v;
            app.set_custom_weather(c);
            None
        }
        "humidity" => {
            let mut c = custom_state(app);
            c.humidity = v;
            app.set_custom_weather(c);
            None
        }
        "temp" => {
            app.edit_weather(|w| w.temp.0 = v);
            None
        }
        "wind_speed" => {
            app.edit_weather(|w| w.wind.1 = v);
            None
        }
        "wind_dir" => {
            app.edit_weather(|w| w.wind.0 = v);
            None
        }
        _ => None,
    }
}

pub(super) fn toggle_now(app: &App, id: &str) -> Option<bool> {
    Some(match id {
        "navigator" => {
            if app.vr_active() {
                app.vr_nav_profile().enabled
            } else {
                app.navigator.as_ref().is_some_and(|n| n.enabled)
            }
        }
        "nav_ai" => app.navigator.as_ref().map_or(::config::get_bool("navigator", "ai").unwrap_or(true), |n| n.show_ai),
        "nav_topbar" => app
            .navigator
            .as_ref()
            .map_or(::config::get_bool("navigator", "topbar").unwrap_or(true), |n| n.show_topbar),
        "nav_turn" => app.navigator.as_ref().map_or(::config::get_bool("navigator", "turn").unwrap_or(true), |n| n.show_turn),
        "nav_stoplist" => app
            .navigator
            .as_ref()
            .map_or(::config::get_bool("navigator", "stoplist").unwrap_or(true), |n| n.show_stoplist),
        "nav_stops_ext" => app
            .navigator
            .as_ref()
            .map_or(::config::get_bool("navigator", "stops_ext").unwrap_or(false), |n| n.schedule),
        "shadows" => ::config::get_bool("graphics", "shadows").unwrap_or(true),
        "head" => ::config::get_bool("camera", "head_movement").unwrap_or(true),
        "cam_smooth" => ::config::get_bool("camera", "smooth").unwrap_or(true),
        "coll_objects" => ::config::get_bool("gameplay", "collision_objects").unwrap_or(true),
        "coll_vehicles" => ::config::get_bool("gameplay", "collision_vehicles").unwrap_or(true),
        "mouse" => app.mouse_drive,
        "mouse_right" => ::config::get_bool("controls", "mouse_right_off").unwrap_or(false),
        "blinker_cancel" => ::config::get_bool("controls", "blinker_cancel").unwrap_or(true),
        "steer_center" => ::config::get_bool("controls", "steer_center").unwrap_or(true),
        "fps" => ::config::get_bool("ui", "show_fps").unwrap_or(false),
        "auto_ibis" => ::config::get_bool("gameplay", "auto_ibis").unwrap_or(false),
        "time_sync" => ::config::get_bool("gameplay", "time_sync").unwrap_or(false),
        "metar_sync" => ::config::get_bool("gameplay", "metar_sync").unwrap_or(false),
        "snow_cover" => app.weather.as_ref().is_some_and(|w| w.snow),
        "snow_road" => app.weather.as_ref().is_some_and(|w| w.snow_on_road),
        "camcoll" => ::config::get_bool("camera", "collision").unwrap_or(true),
        "steer_look" => ::config::get_bool("camera", "steer_look").unwrap_or(false),
        "hands_in_cab" => ::config::get_bool("gameplay", "hands_in_cab").unwrap_or(false),
        "ff" => controllers::ff_enabled(),
        "brake_hold" => ::config::get_bool("controls", "brake_hold").unwrap_or(true),
        "auto_clutch" => ::config::get_bool("gameplay", "auto_clutch").unwrap_or(true),
        "headtrack" => ::config::get_bool("camera", "head_tracking").unwrap_or(false),
        "timetable_win" => app.timetable,
        "info_bar" => app.info_bar,
        "nav_arrows" => app.navigator.as_ref().map_or(::config::get_bool("navigator", "arrows").unwrap_or(false), |n| n.arrows),
        "exact_fare" => ::config::get_bool("gameplay", "exact_fare").unwrap_or(true),
        "pax_prefer_seats" => ::config::get_bool("gameplay", "pax_prefer_seats").unwrap_or(false),
        "pax_rear_entry" => ::config::get_bool("gameplay", "pax_rear_entry").unwrap_or(true),
        "pax_ik" => app.args.pax_ik.unwrap_or(::config::get_bool("passengers", "ik").unwrap_or(true)),
        "collision_pedestrians" => ::config::get_bool("gameplay", "collision_pedestrians").unwrap_or(true),
        "ssao" => ::config::get_bool("graphics", "ssao").unwrap_or(true),
        "detail_textures" => ::config::get_bool("graphics", "detail_textures").unwrap_or(true),
        "reflections" => ::config::get_bool("graphics", "reflections").unwrap_or(true),
        "clouds" => ::config::get_bool("graphics", "clouds").unwrap_or(true),
        "fullscreen" => ::config::get_string("graphics", "window_mode").as_deref() != Some("windowed"),
        "vsync" => ::config::get_bool("graphics", "vsync").unwrap_or(true),
        "texture_compression" => ::config::get_bool("graphics", "texture_compression").unwrap_or(true),
        "shadow_blobs" => ::config::get_bool("graphics", "shadow_blobs").unwrap_or(true),
        "discord_status" => ::config::get_bool("discord", "status").unwrap_or(true),
        "driver" => ::config::get_bool("gameplay", "driver").unwrap_or(true),
        "alt_view" => ::config::get_bool("camera", "alt_view").unwrap_or(true),
        "free_look" => ::config::get_bool("camera", "free_look").unwrap_or(false),
        "crosshair" => ::config::get_bool("camera", "crosshair").unwrap_or(true),
        "vr" => ::config::get_bool("vr", "enabled").unwrap_or(false),
        "vr_desktop_mirror" => ::config::get_bool("vr", "desktop-mirror").unwrap_or(true),
        "doppler" => ::config::get_bool("audio", "doppler").unwrap_or(true),
        "steering_linear" => ::config::get_bool("controls", "steering_linear").unwrap_or(false),
        "old_steering" => ::config::get_bool("controls", "old_steering").unwrap_or(false),
        "red_steer_spd" => ::config::get_bool("controls", "red_steer_spd").unwrap_or(false),
        "momentary_gears" => ::config::get_bool("gameplay", "momentary_gears").unwrap_or(false),
        "auto_shift" => ::config::get_bool("gameplay", "auto_shift").unwrap_or(false),
        "ff_invert" => controllers::global_ff_invert(),
        "ui_scale_window" => ::config::get_bool("ui", "scale_window").unwrap_or(true),
        "tooltips" => ::config::get_bool("ui", "tooltips").unwrap_or(true),
        "notes" => ::config::get_bool("ui", "notes").unwrap_or(true),
        "chat" => ::config::get_bool("ui", "chat").unwrap_or(true),
        "name_tags" => ::config::get_bool("ui", "name_tags").unwrap_or(true),
        _ => return None,
    })
}

pub(super) fn toggle_set(app: &mut App, id: &str, on: bool) -> Option<(&'static str, String)> {
    match id {
        "navigator" => {
            if app.vr_active() {
                if app.vr_nav_profile().enabled != on {
                    app.vr_nav_adjust("enabled", 1.0);
                }
                return None;
            }
            if let Some(n) = app.navigator.as_mut() {
                n.enabled = on;
            }
            ::config::set_setting("ui", "navigator", on);
            let _ = ::config::save();
            None
        }
        "nav_ai" => {
            if let Some(n) = app.navigator.as_mut() {
                n.show_ai = on;
            }
            ::config::set_setting("navigator", "ai", on);
            let _ = ::config::save();
            None
        }
        "nav_topbar" => {
            if let Some(n) = app.navigator.as_mut() {
                n.show_topbar = on;
            }
            ::config::set_setting("navigator", "topbar", on);
            let _ = ::config::save();
            None
        }
        "nav_turn" => {
            if let Some(n) = app.navigator.as_mut() {
                n.show_turn = on;
            }
            ::config::set_setting("navigator", "turn", on);
            let _ = ::config::save();
            None
        }
        "nav_stoplist" => {
            if let Some(n) = app.navigator.as_mut() {
                n.show_stoplist = on;
            }
            ::config::set_setting("navigator", "stoplist", on);
            let _ = ::config::save();
            None
        }
        "nav_stops_ext" => {
            if let Some(n) = app.navigator.as_mut() {
                n.schedule = on;
            }
            ::config::set_setting("navigator", "stops_ext", on);
            let _ = ::config::save();
            None
        }
        "shadows" => {
            ::config::set_setting("graphics", "shadows", on);
            let _ = ::config::save();
            None
        }
        "head" => {
            ::config::set_setting("camera", "head_movement", on);
            let _ = ::config::save();
            None
        }
        "cam_smooth" => {
            ::config::set_setting("camera", "smooth", on);
            let _ = ::config::save();
            None
        }
        // (at once: stuck under a bridge a map made too low, the bus drives on)
        "coll_objects" => {
            ::config::set_setting("gameplay", "collision_objects", on);
            let _ = ::config::save();
            let cw = app.world.as_ref().map(|w| w.collision.lock().clone());
            if let Some(p) = app.player.as_mut() {
                p.vehicle.collision = cw.filter(|_| on);
            }
            None
        }
        "coll_vehicles" => {
            ::config::set_setting("gameplay", "collision_vehicles", on);
            let _ = ::config::save();
            None
        }
        "mouse" => {
            app.mouse_drive = on;
            App::save_mouse_drive(on);
            if !app.mouse_drive {
                player::keep_wheel(app.player.as_mut());
            }
            #[cfg(windows)]
            if !app.mouse_drive {
                app.reset_vr_pointer();
            }
            app.mouse_steer = (
                app.player
                    .as_ref()
                    .map(|p| p.vehicle.physics.controls.steering)
                    .unwrap_or(0.0),
                1.0,
            );
            app.mouse_pedals = app
                .player
                .as_ref()
                .map(|p| {
                    (
                        p.vehicle.physics.controls.throttle,
                        p.vehicle.physics.controls.brake,
                    )
                })
                .unwrap_or((0.0, 0.0));
            None
        }
        "steer_center" => {
            ::config::set_setting("controls", "steer_center", on);
            let _ = ::config::save();
            None
        }
        "blinker_cancel" => {
            ::config::set_setting("controls", "blinker_cancel", on);
            let _ = ::config::save();
            if let Some(p) = app.player.as_mut() {
                p.blinker_cancel = on;
            }
            None
        }
        "mouse_right" => {
            ::config::set_setting("controls", "mouse_right_off", on);
            let _ = ::config::save();
            None
        }
        "auto_ibis" => {
            ::config::set_setting("gameplay", "auto_ibis", on);
            let _ = ::config::save();
            if let Some(p) = app.player.as_mut() {
                p.auto_ibis = on;
            }
            None
        }
        // the real-time sync: the clock takes the device's date and time at once (a host's
        // clock runs at real time while it is on, at its time speed again after)
        "time_sync" => {
            ::config::set_setting("gameplay", "time_sync", on);
            let _ = ::config::save();
            if let Some(l) = app.lan.as_mut().filter(|l| l.role == ::network::Role::Host) {
                l.clock_speed = if on {
                    1.0
                } else {
                    ::config::get_float("gameplay", "time_speed").unwrap_or(1.0).clamp(1.0, 30.0)
                };
            }
            app.sync_real_time();
            None
        }
        // the METAR sync: the weather goes over to the report of the nearest airport and
        // cannot be changed while it is on (the cycle and a hand-made weather end with it)
        "metar_sync" => {
            ::config::set_setting("gameplay", "metar_sync", on);
            let _ = ::config::save();
            app.metar_rx = None;
            app.metar_once = false;
            app.metar_next = 0.0;
            if on {
                app.weather_cycle = None;
                app.weather_blend = None;
            }
            None
        }
        "snow_cover" => {
            app.edit_weather(|w| w.snow = on);
            None
        }
        "snow_road" => {
            app.edit_weather(|w| w.snow_on_road = on);
            None
        }
        "fps" => {
            ::config::set_setting("ui", "show_fps", on);
            let _ = ::config::save();
            None
        }
        "headtrack" => {
            ::config::set_setting("camera", "head_tracking", on);
            let _ = ::config::save();
            None
        }
        "camcoll" => {
            ::config::set_setting("camera", "collision", on);
            let _ = ::config::save();
            None
        }
        "steer_look" => {
            ::config::set_setting("camera", "steer_look", on);
            let _ = ::config::save();
            None
        }
        "hands_in_cab" => {
            ::config::set_setting("gameplay", "hands_in_cab", on);
            let _ = ::config::save();
            None
        }
        "brake_hold" => {
            ::config::set_setting("controls", "brake_hold", on);
            let _ = ::config::save();
            None
        }
        "auto_clutch" => {
            ::config::set_setting("gameplay", "auto_clutch", on);
            let _ = ::config::save();
            if let Some(p) = app.player.as_mut() {
                p.vehicle.host.auto_clutch = if on { 1.0 } else { 0.0 };
            }
            None
        }
        "ff" => {
            controllers::set_ff_enabled(on);
            let _ = ::config::save();
            None
        }
        "timetable_win" => {
            app.timetable = on;
            None
        }
        "info_bar" => {
            app.info_bar = on;
            None
        }
        "nav_arrows" => {
            ::config::set_setting("navigator", "arrows", on);
            let _ = ::config::save();
            None
        }
        "exact_fare" => {
            ::config::set_setting("gameplay", "exact_fare", on);
            let _ = ::config::save();
            None
        }
        "pax_prefer_seats" => {
            ::config::set_setting("gameplay", "pax_prefer_seats", on);
            let _ = ::config::save();
            None
        }
        "pax_rear_entry" => {
            ::config::set_setting("gameplay", "pax_rear_entry", on);
            let _ = ::config::save();
            None
        }
        "pax_ik" => {
            ::config::set_setting("passengers", "ik", on);
            let _ = ::config::save();
            None
        }
        "collision_pedestrians" => {
            ::config::set_setting("gameplay", "collision_pedestrians", on);
            let _ = ::config::save();
            None
        }
        "ssao" => {
            ::config::set_setting("graphics", "ssao", on);
            let _ = ::config::save();
            None
        }
        "detail_textures" => {
            ::config::set_setting("graphics", "detail_textures", on);
            let _ = ::config::save();
            None
        }
        "reflections" => {
            ::config::set_setting("graphics", "reflections", on);
            let _ = ::config::save();
            None
        }
        "clouds" => {
            ::config::set_setting("graphics", "clouds", on);
            let _ = ::config::save();
            None
        }
        "fullscreen" => {
            let mode = if on { "borderless" } else { "windowed" };
            ::config::set_setting("graphics", "window_mode", mode);
            let _ = ::config::save();
            apply_window_mode(app, mode);
            None
        }
        "vsync" => {
            ::config::set_setting("graphics", "vsync", on);
            let _ = ::config::save();
            None
        }
        "shadow_blobs" => {
            ::config::set_setting("graphics", "shadow_blobs", on);
            let _ = ::config::save();
            None
        }
        "discord_status" => {
            ::config::set_setting("discord", "status", on);
            let _ = ::config::save();
            None
        }
        "texture_compression" => {
            ::config::set_setting("graphics", "texture_compression", on);
            let _ = ::config::save();
            None
        }
        "driver" => {
            ::config::set_setting("gameplay", "driver", on);
            let _ = ::config::save();
            None
        }
        "alt_view" => {
            ::config::set_setting("camera", "alt_view", on);
            let _ = ::config::save();
            None
        }
        "free_look" => {
            ::config::set_setting("camera", "free_look", on);
            let _ = ::config::save();
            app.free_look = false;
            None
        }
        "crosshair" => {
            ::config::set_setting("camera", "crosshair", on);
            let _ = ::config::save();
            None
        }
        "vr" => {
            ::config::set_setting("vr", "enabled", on);
            let _ = ::config::save();
            None
        }
        "vr_desktop_mirror" => {
            ::config::set_setting("vr", "desktop-mirror", on);
            let _ = ::config::save();
            None
        }
        "doppler" => {
            ::config::set_setting("audio", "doppler", on);
            let _ = ::config::save();
            ::audio::DOPPLER.store(on, std::sync::atomic::Ordering::Relaxed);
            None
        }
        "steering_linear" => {
            ::config::set_setting("controls", "steering_linear", on);
            let _ = ::config::save();
            None
        }
        "old_steering" => {
            ::config::set_setting("controls", "old_steering", on);
            let _ = ::config::save();
            None
        }
        "red_steer_spd" => {
            ::config::set_setting("controls", "red_steer_spd", on);
            let _ = ::config::save();
            None
        }
        "momentary_gears" => {
            ::config::set_setting("gameplay", "momentary_gears", on);
            let _ = ::config::save();
            None
        }
        "auto_shift" => {
            ::config::set_setting("gameplay", "auto_shift", on);
            let _ = ::config::save();
            if let Some(p) = app.player.as_mut() {
                p.auto_shift = on;
            }
            None
        }
        "ff_invert" => {
            controllers::set_global_ff_invert(on);
            let _ = ::config::save();
            None
        }
        "ui_scale_window" => {
            ::config::set_setting("ui", "scale_window", on);
            let _ = ::config::save();
            None
        }
        "tooltips" => {
            ::config::set_setting("ui", "tooltips", on);
            let _ = ::config::save();
            None
        }
        "notes" => {
            ::config::set_setting("ui", "notes", on);
            let _ = ::config::save();
            None
        }
        "chat" => {
            ::config::set_setting("ui", "chat", on);
            let _ = ::config::save();
            None
        }
        "name_tags" => {
            ::config::set_setting("ui", "name_tags", on);
            let _ = ::config::save();
            None
        }
        _ => None,
    }
}
pub(crate) fn apply_window_mode(app: &App, mode: &str) {
    let Some(window) = app.window.as_ref() else {
        return;
    };
    let fullscreen = match mode {
        "borderless" => Some(winit::window::Fullscreen::Borderless(None)),
        "fullscreen" => Some(exclusive_fullscreen(window)),
        _ => None,
    };
    window.set_fullscreen(fullscreen);
}

/// Enter exclusive fullscreen only at the monitor's current desktop resolution and refresh rate.
pub(crate) fn exclusive_fullscreen(window: &winit::window::Window) -> winit::window::Fullscreen {
    let Some(monitor) = window.current_monitor() else {
        return winit::window::Fullscreen::Borderless(None);
    };
    let size = monitor.size();
    let refresh = monitor.refresh_rate_millihertz();
    let fallback = winit::window::Fullscreen::Borderless(Some(monitor.clone()));
    monitor
        .video_modes()
        .filter(|mode| mode.size() == size)
        .min_by_key(|mode| refresh.map(|rate| mode.refresh_rate_millihertz().abs_diff(rate)))
        .map(winit::window::Fullscreen::Exclusive)
        .unwrap_or(fallback)
}
