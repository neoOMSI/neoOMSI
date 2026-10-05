//! Reading and writing the values of the sliders and switches of the options windows.

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
        "led_mips" => (0..=80).map(|v| v as f32 * 0.05).collect(),
        "atmosphere_brightness" => (0..=40).map(|v| v as f32 * 0.05).collect(),
        "ui_scale" => (10..=40).map(|v| v as f32 * 0.05).collect(),
        "ui_opacity" => (4..=20).map(|v| v as f32 * 0.05).collect(),
        "vol_ai" | "vol_scenery" => (0..=20).map(|v| v as f32 * 0.05).collect(),
        "wheel_range" => (6..=60).map(|v| v as f32 * 30.0).collect(),
        "wheel_lock" => std::iter::once(0.0)
            .chain((2..=60).map(|v| v as f32 * 30.0))
            .collect(),
        "fov" => std::iter::once(0.0)
            .chain((20..=120).map(|v| v as f32))
            .collect(),
        "steer_look_angle" => (0..=60).map(|v| v as f32).collect(),
        "steer_look_response" => (1..=20).map(|v| v as f32 * 0.05).collect(),
        "pedal_t" | "pedal_b" => PEDAL.to_vec(),
        "mouse_sens" => (10..=300).map(|v| v as f32 / 100.0).collect(),
        "stick_sens" => (2..=40).map(|v| v as f32 * 0.05).collect(),
        "ctrl_deadzone" => (0..=30).map(|v| v as f32 / 100.0).collect(),
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

pub(super) fn custom_state(app: &App) -> crate::weather_setup::CustomWeather {
    if let Some(mut c) = crate::weather_setup::custom_weather(app.args.weather.as_deref()) {
        // Wetness keeps evolving while driving; never restore an old serialized value just
        // because another custom field (brightness, humidity, etc.) was edited.
        c.road_wetness = app.wetness;
        return c;
    }
    match app.weather.as_ref() {
        Some(w) => crate::weather_setup::CustomWeather::from_weather(w, 1.0, app.wetness),
        None => crate::weather_setup::CustomWeather::default(),
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
    let s = &app.settings;
    Some(match verb {
        "speed" => s.time_speed as f32,
        "traffic" => app.traffic.as_ref()?.target as f32,
        "pax" => s.pax_density,
        "volume" => s.volume,
        "led_glow" => s.led_glow as f32,
        "led_mips" => s.led_mips,
        "atmosphere_brightness" => s.atmosphere_brightness,
        "pedal_t" => s.pedal_throttle,
        "pedal_b" => s.pedal_brake,
        "mouse_sens" => s.mouse_sens,
        "stick_sens" => s.stick_sens,
        "ctrl_deadzone" => s.ctrl_deadzone,
        "look_sens" => s.look_sens,
        "ui_scale" => s.ui_scale,
        "ui_opacity" => s.ui_opacity,
        "vol_ai" => s.vol_ai,
        "vol_scenery" => s.vol_scenery,
        "wheel_range" => s.wheel_range,
        "wheel_lock" => s.wheel_lock,
        "fov" => s.fov,
        "steer_look_angle" => s.steer_look_angle,
        "steer_look_response" => s.steer_look_response,
        "seat" => s.seat[arg.trim().parse::<usize>().unwrap_or(0).min(2)],
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
            app.settings.time_speed = v as f64;
            Some(("time_speed", app.settings.time_speed.to_string()))
        }
        "traffic" => {
            if let Some(t) = app.traffic.as_mut() {
                t.target = v.round() as usize;
                app.args.traffic = t.target;
            }
            None
        }
        "pax" => {
            app.settings.pax_density = v;
            Some(("pax_density", v.to_string()))
        }
        "volume" => {
            app.settings.volume = v;
            Some(("volume", v.to_string()))
        }
        "led_glow" => {
            app.settings.led_glow = v.round() as _;
            Some(("led_glow", app.settings.led_glow.to_string()))
        }
        "atmosphere_brightness" => {
            app.settings.atmosphere_brightness = v.clamp(0.0, 2.0);
            Some((
                "atmosphere_brightness",
                app.settings.atmosphere_brightness.to_string(),
            ))
        }
        "led_mips" => {
            app.settings.led_mips = v.clamp(0.0, 4.0);
            Some(("led_mips", app.settings.led_mips.to_string()))
        }
        "pedal_t" => {
            app.settings.pedal_throttle = v;
            Some(("pedal_throttle", v.to_string()))
        }
        "pedal_b" => {
            app.settings.pedal_brake = v;
            Some(("pedal_brake", v.to_string()))
        }
        "look_sens" => {
            app.settings.look_sens = (v * 100.0).round() / 100.0;
            Some(("look_sens", app.settings.look_sens.to_string()))
        }
        "mouse_sens" => {
            app.settings.mouse_sens = (v * 100.0).round() / 100.0;
            Some(("mouse_sens", app.settings.mouse_sens.to_string()))
        }
        "stick_sens" => {
            app.settings.stick_sens = (v * 100.0).round() / 100.0;
            Some(("stick_sens", app.settings.stick_sens.to_string()))
        }
        "ctrl_deadzone" => {
            app.settings.ctrl_deadzone = (v * 100.0).round() / 100.0;
            Some(("ctrl_deadzone", app.settings.ctrl_deadzone.to_string()))
        }
        "ui_scale" => {
            app.settings.ui_scale = (v * 100.0).round() / 100.0;
            Some(("ui_scale", app.settings.ui_scale.to_string()))
        }
        "ui_opacity" => {
            app.settings.ui_opacity = (v * 100.0).round() / 100.0;
            Some(("ui_opacity", app.settings.ui_opacity.to_string()))
        }
        "vol_ai" => {
            app.settings.vol_ai = (v * 100.0).round() / 100.0;
            Some(("vol_ai", app.settings.vol_ai.to_string()))
        }
        "vol_scenery" => {
            app.settings.vol_scenery = (v * 100.0).round() / 100.0;
            Some(("vol_scenery", app.settings.vol_scenery.to_string()))
        }
        "wheel_range" => {
            app.settings.wheel_range = v.round();
            Some(("wheel_range", app.settings.wheel_range.to_string()))
        }
        "wheel_lock" => {
            app.settings.wheel_lock = if v < 45.0 { 0.0 } else { v.round() };
            Some(("wheel_lock", app.settings.wheel_lock.to_string()))
        }
        "fov" => {
            app.settings.fov = if v < 20.0 { 0.0 } else { v.round() };
            Some(("fov", app.settings.fov.to_string()))
        }
        "steer_look_angle" => {
            app.settings.steer_look_angle = v.round();
            Some((
                "steer_look_angle",
                app.settings.steer_look_angle.to_string(),
            ))
        }
        "steer_look_response" => {
            app.settings.steer_look_response = (v * 100.0).round() / 100.0;
            Some((
                "steer_look_response",
                app.settings.steer_look_response.to_string(),
            ))
        }
        "seat" => {
            let k: usize = arg.trim().parse().unwrap_or(0).min(2);
            app.settings.seat[k] = (v * 100.0).round() / 100.0;
            Some((
                ["seat_x", "seat_y", "seat_z"][k],
                app.settings.seat[k].to_string(),
            ))
        }
        "hour" | "minute" => {
            if app
                .lan
                .as_ref()
                .is_some_and(|l| l.role == omsi_net::Role::Client)
            {
                app.service_msg = Some(("In a LAN session the host sets the clock".into(), 3.0));
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
    let s = &app.settings;
    Some(match id {
        "navigator" => {
            if app.vr_active() {
                app.vr_nav_profile().enabled
            } else {
                app.navigator.as_ref().is_some_and(|n| n.enabled)
            }
        }
        "nav_ai" => app.navigator.as_ref().map_or(s.nav_ai, |n| n.show_ai),
        "nav_topbar" => app
            .navigator
            .as_ref()
            .map_or(s.nav_topbar, |n| n.show_topbar),
        "nav_turn" => app.navigator.as_ref().map_or(s.nav_turn, |n| n.show_turn),
        "nav_stoplist" => app
            .navigator
            .as_ref()
            .map_or(s.nav_stoplist, |n| n.show_stoplist),
        "nav_stops_ext" => app
            .navigator
            .as_ref()
            .map_or(s.nav_stops_ext, |n| n.schedule),
        "shadows" => s.shadows,
        "head" => s.head_movement,
        "cam_smooth" => s.driverview_smooth,
        "coll_objects" => s.collision_objects,
        "coll_vehicles" => s.collision_vehicles,
        "mouse" => app.mouse_drive,
        "mouse_right" => s.mouse_right_off,
        "blinker_cancel" => s.blinker_cancel,
        "steer_center" => s.steer_center,
        "fps" => s.show_fps,
        "auto_ibis" => s.auto_ibis,
        "time_sync" => s.time_sync,
        "metar_sync" => s.metar_sync,
        "snow_cover" => app.weather.as_ref().is_some_and(|w| w.snow),
        "snow_road" => app.weather.as_ref().is_some_and(|w| w.snow_on_road),
        "camcoll" => s.camera_collision,
        "steer_look" => s.steer_look,
        "hands_in_cab" => s.hands_in_cab,
        "ff" => s.ff_enabled,
        "brake_hold" => s.brake_hold,
        "auto_clutch" => s.auto_clutch,
        "headtrack" => s.head_tracking,
        "timetable_win" => app.timetable,
        "info_bar" => app.info_bar,
        "nav_arrows" => app.navigator.as_ref().map_or(s.nav_arrows, |n| n.arrows),
        "exact_fare" => s.exact_fare,
        "pax_prefer_seats" => s.pax_prefer_seats,
        "collision_pedestrians" => s.collision_pedestrians,
        "ssao" => s.ssao,
        "detail_textures" => s.detail_textures,
        "reflections" => s.reflections,
        "clouds" => s.clouds,
        "fullscreen" => s.fullscreen,
        "vsync" => s.vsync,
        "texture_compression" => s.texture_compression,
        "driver" => s.driver,
        "alt_view" => s.alt_view,
        "free_look" => s.free_look,
        "crosshair" => s.crosshair,
        "vr" => s.vr,
        "vr_desktop_mirror" => s.vr_desktop_mirror,
        "doppler" => s.doppler,
        "steering_linear" => s.steering_linear,
        "old_steering" => s.old_steering,
        "red_steer_spd" => s.red_steer_spd,
        "momentary_gears" => s.momentary_gears,
        "auto_shift" => s.auto_shift,
        "ff_invert" => s.ff_invert,
        "ui_scale_window" => s.ui_scale_window,
        "tooltips" => s.tooltips,
        "notes" => s.notes,
        "chat" => s.chat,
        "name_tags" => s.name_tags,
        _ => return None,
    })
}

pub(super) fn toggle_set(app: &mut App, id: &str, on: bool) -> Option<(&'static str, String)> {
    let bit = (on as u8).to_string();
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
            app.settings.navigator = on;
            Some(("navigator", bit))
        }
        "nav_ai" => {
            if let Some(n) = app.navigator.as_mut() {
                n.show_ai = on;
            }
            app.settings.nav_ai = on;
            Some(("nav_ai", bit))
        }
        "nav_topbar" => {
            if let Some(n) = app.navigator.as_mut() {
                n.show_topbar = on;
            }
            app.settings.nav_topbar = on;
            Some(("nav_topbar", bit))
        }
        "nav_turn" => {
            if let Some(n) = app.navigator.as_mut() {
                n.show_turn = on;
            }
            app.settings.nav_turn = on;
            Some(("nav_turn", bit))
        }
        "nav_stoplist" => {
            if let Some(n) = app.navigator.as_mut() {
                n.show_stoplist = on;
            }
            app.settings.nav_stoplist = on;
            Some(("nav_stoplist", bit))
        }
        "nav_stops_ext" => {
            if let Some(n) = app.navigator.as_mut() {
                n.schedule = on;
            }
            app.settings.nav_stops_ext = on;
            Some(("nav_stops_ext", bit))
        }
        "shadows" => {
            app.settings.shadows = on;
            Some(("shadows", bit))
        }
        "head" => {
            app.settings.head_movement = on;
            Some(("head_movement", bit))
        }
        "cam_smooth" => {
            app.settings.driverview_smooth = on;
            Some(("driverview_smooth", bit))
        }
        // (at once: stuck under a bridge a map made too low, the bus drives on)
        "coll_objects" => {
            app.settings.collision_objects = on;
            let cw = app.world.as_ref().map(|w| w.collision.lock().clone());
            if let Some(p) = app.player.as_mut() {
                p.vehicle.collision = cw.filter(|_| on);
            }
            Some(("collision_objects", bit))
        }
        "coll_vehicles" => {
            app.settings.collision_vehicles = on;
            Some(("collision_vehicles", bit))
        }
        "mouse" => {
            app.mouse_drive = on;
            if !app.mouse_drive {
                crate::player::keep_wheel(app.player.as_mut());
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
            app.settings.steer_center = on;
            Some(("steer_center", bit))
        }
        "blinker_cancel" => {
            app.settings.blinker_cancel = on;
            if let Some(p) = app.player.as_mut() {
                p.blinker_cancel = on;
            }
            Some(("blinker_cancel", bit))
        }
        "mouse_right" => {
            app.settings.mouse_right_off = on;
            Some(("mouse_right_off", bit))
        }
        "auto_ibis" => {
            app.settings.auto_ibis = on;
            if let Some(p) = app.player.as_mut() {
                p.auto_ibis = on;
            }
            Some(("auto_ibis", bit))
        }
        // the real-time sync: the clock takes the device's date and time at once (a host's
        // clock runs at real time while it is on, at its time speed again after)
        "time_sync" => {
            app.settings.time_sync = on;
            if let Some(l) = app.lan.as_mut().filter(|l| l.role == omsi_net::Role::Host) {
                l.clock_speed = if on {
                    1.0
                } else {
                    app.settings.time_speed.clamp(1.0, 30.0)
                };
            }
            app.sync_real_time();
            Some(("time_sync", bit))
        }
        // the METAR sync: the weather goes over to the report of the nearest airport and
        // cannot be changed while it is on (the cycle and a hand-made weather end with it)
        "metar_sync" => {
            app.settings.metar_sync = on;
            app.metar_rx = None;
            app.metar_once = false;
            app.metar_next = 0.0;
            if on {
                app.weather_cycle = None;
                app.weather_blend = None;
            }
            Some(("metar_sync", bit))
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
            app.settings.show_fps = on;
            Some(("show_fps", bit))
        }
        "headtrack" => {
            app.settings.head_tracking = on;
            Some(("head_tracking", bit))
        }
        "camcoll" => {
            app.settings.camera_collision = on;
            Some(("camera_collision", bit))
        }
        "steer_look" => {
            app.settings.steer_look = on;
            Some(("steer_look", bit))
        }
        "hands_in_cab" => {
            app.settings.hands_in_cab = on;
            Some(("hands_in_cab", bit))
        }
        "brake_hold" => {
            app.settings.brake_hold = on;
            Some(("brake_hold", bit))
        }
        "auto_clutch" => {
            app.settings.auto_clutch = on;
            if let Some(p) = app.player.as_mut() {
                p.vehicle.host.auto_clutch = if on { 1.0 } else { 0.0 };
            }
            Some(("auto_clutch", bit))
        }
        "ff" => {
            app.settings.ff_enabled = on;
            Some(("ff_enabled", bit))
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
            app.settings.nav_arrows = on;
            Some(("nav_arrows", bit))
        }
        "exact_fare" => {
            app.settings.exact_fare = on;
            Some(("exact_fare", bit))
        }
        "pax_prefer_seats" => {
            app.settings.pax_prefer_seats = on;
            Some(("pax_prefer_seats", bit))
        }
        "collision_pedestrians" => {
            app.settings.collision_pedestrians = on;
            Some(("collision_pedestrians", bit))
        }
        "ssao" => {
            app.settings.ssao = on;
            Some(("ssao", bit))
        }
        "detail_textures" => {
            app.settings.detail_textures = on;
            Some(("detail_textures", bit))
        }
        "reflections" => {
            app.settings.reflections = on;
            Some(("reflections", bit))
        }
        "clouds" => {
            app.settings.clouds = on;
            Some(("clouds", bit))
        }
        "fullscreen" => {
            app.settings.fullscreen = on;
            if let Some(w) = app.window.as_ref() {
                w.set_fullscreen(on.then_some(winit::window::Fullscreen::Borderless(None)));
            }
            Some(("fullscreen", bit))
        }
        "vsync" => {
            app.settings.vsync = on;
            Some(("vsync", bit))
        }
        "texture_compression" => {
            app.settings.texture_compression = on;
            Some(("texture_compression", bit))
        }
        "driver" => {
            app.settings.driver = on;
            Some(("driver", bit))
        }
        "alt_view" => {
            app.settings.alt_view = on;
            Some(("alt_view", bit))
        }
        "free_look" => {
            app.settings.free_look = on;
            app.free_look = false;
            Some(("free_look", bit))
        }
        "crosshair" => {
            app.settings.crosshair = on;
            Some(("crosshair", bit))
        }
        "vr" => {
            app.settings.vr = on;
            Some(("vr", bit))
        }
        "vr_desktop_mirror" => {
            app.settings.vr_desktop_mirror = on;
            Some(("vr_desktop_mirror", bit))
        }
        "doppler" => {
            app.settings.doppler = on;
            Some(("doppler", bit))
        }
        "steering_linear" => {
            app.settings.steering_linear = on;
            Some(("steering_linear", bit))
        }
        "old_steering" => {
            app.settings.old_steering = on;
            Some(("old_steering", bit))
        }
        "red_steer_spd" => {
            app.settings.red_steer_spd = on;
            Some(("red_steer_spd", bit))
        }
        "momentary_gears" => {
            app.settings.momentary_gears = on;
            Some(("momentary_gears", bit))
        }
        "auto_shift" => {
            app.settings.auto_shift = on;
            if let Some(p) = app.player.as_mut() {
                p.auto_shift = on;
            }
            Some(("auto_shift", bit))
        }
        "ff_invert" => {
            app.settings.ff_invert = on;
            Some(("ff_invert", bit))
        }
        "ui_scale_window" => {
            app.settings.ui_scale_window = on;
            Some(("ui_scale_window", bit))
        }
        "tooltips" => {
            app.settings.tooltips = on;
            Some(("tooltips", bit))
        }
        "notes" => {
            app.settings.notes = on;
            Some(("notes", bit))
        }
        "chat" => {
            app.settings.chat = on;
            Some(("chat", bit))
        }
        "name_tags" => {
            app.settings.name_tags = on;
            Some(("name_tags", bit))
        }
        _ => None,
    }
}
