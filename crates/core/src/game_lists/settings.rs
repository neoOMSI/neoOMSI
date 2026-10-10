//! The launcher's settings file as the lists read it, with delayed writes.

use super::*;

/// The launcher's settings file as the lists show it: read once (until something is
/// written), with the keys still waiting to be written on top.
pub(crate) fn settings_file() -> std::sync::Arc<serde_json::Value> {
    // lock order: MERGED_SETTINGS, then SETTINGS_CACHE and PENDING_SETTINGS
    let mut merged = MERGED_SETTINGS.lock().unwrap_or_else(|e| e.into_inner());
    if let Some(v) = merged.as_ref() {
        return v.clone();
    }
    let mut v = SETTINGS_CACHE
        .lock()
        .unwrap_or_else(|e| e.into_inner())
        .get_or_insert_with(|| {
            omsi_launcher_lib::current_settings()
        })
        .clone();
    let pending = PENDING_SETTINGS.lock().unwrap_or_else(|e| e.into_inner());
    apply_pending(&mut v, &pending.0);
    let v = std::sync::Arc::new(v);
    *merged = Some(v.clone());
    v
}

/// Forget the file as read (and the merged copy of it): it is read again on the next ask.
pub(super) fn invalidate_settings() {
    *SETTINGS_CACHE.lock().unwrap_or_else(|e| e.into_inner()) = None;
    *MERGED_SETTINGS.lock().unwrap_or_else(|e| e.into_inner()) = None;
}

/// A `[graphics]` value as text, as the lists compare it (a switch as 1 or 0).
pub(crate) fn gfx_text(key: &str) -> String {
    if key == "window_mode" && ::config::get_bool("graphics", "triple_screen").unwrap_or(false) {
        return "windowed".into();
    }
    use ::config::Value as T;
    match ::config::get_setting("graphics", key) {
        Some(T::Boolean(b)) => (b as u8).to_string(),
        Some(T::Integer(i)) => i.to_string(),
        Some(T::Float(f)) => f.to_string(),
        Some(T::String(s)) => s,
        _ => String::new(),
    }
}

/// Write a `[graphics]` value from its text, as the type the key has in the config.
pub(crate) fn gfx_set(key: &str, text: &str) {
    use ::config::Value as T;
    let text = if key == "window_mode"
        && ::config::get_bool("graphics", "triple_screen").unwrap_or(false)
    {
        "windowed"
    } else {
        text
    };
    let value = match ::config::get_setting("graphics", key) {
        Some(T::Boolean(_)) => T::Boolean(text == "1" || text == "true"),
        Some(T::Integer(_)) => match text.parse::<i64>() {
            Ok(i) => T::Integer(i),
            Err(_) => return,
        },
        Some(T::Float(_)) => match text.parse::<f64>() {
            Ok(f) => T::Float(f),
            Err(_) => return,
        },
        Some(T::String(_)) => T::String(text.to_string()),
        _ => return,
    };
    ::config::set_setting("graphics", key, value);
}

/// The quality presets: `[graphics]` values as text.
pub(crate) const PRESETS: [(&str, &[(&str, &str)]); 4] = [
    ("Low", &[("msaa", "1"), ("anisotropy", "2"), ("shadow_size", "1024"), ("ssao", "0"), ("shadows", "0"), ("detail_textures", "0"), ("clouds", "0"), ("view_distance", "600"), ("min_obj_size", "0.03"), ("max_obj_dist", "500"), ("mirror_size", "128"), ("mirror_refresh", "eco"), ("render_scale", "0.75"), ("texture_memory", "800")]),
    ("Medium", &[("msaa", "2"), ("anisotropy", "4"), ("shadow_size", "2048"), ("ssao", "0"), ("shadows", "1"), ("detail_textures", "1"), ("clouds", "1"), ("view_distance", "900"), ("min_obj_size", "0.02"), ("max_obj_dist", "750"), ("mirror_size", "256"), ("mirror_refresh", "eco"), ("render_scale", "0"), ("texture_memory", "1200")]),
    ("High", &[("msaa", "4"), ("anisotropy", "8"), ("shadow_size", "2048"), ("ssao", "1"), ("shadows", "1"), ("detail_textures", "1"), ("clouds", "1"), ("view_distance", "0"), ("min_obj_size", "0.013"), ("max_obj_dist", "-1"), ("mirror_size", "256"), ("mirror_refresh", "full"), ("render_scale", "0"), ("texture_memory", "0")]),
    ("Ultra", &[("msaa", "4"), ("anisotropy", "8"), ("shadow_size", "4096"), ("ssao", "1"), ("shadows", "1"), ("detail_textures", "1"), ("clouds", "1"), ("view_distance", "2000"), ("min_obj_size", "0.005"), ("max_obj_dist", "1500"), ("mirror_size", "512"), ("mirror_refresh", "full"), ("render_scale", "0"), ("texture_memory", "0")]),
];

/// The preset the graphics match now.
pub(crate) fn preset_now() -> Option<usize> {
    PRESETS.iter().position(|p| {
        p.1.iter().all(|(k, v)| {
            let cur = gfx_text(k);
            cur == *v
                || cur
                .parse::<f64>()
                .ok()
                .zip(v.parse::<f64>().ok())
                .is_some_and(|(a, b)| (a - b).abs() < 1e-6)
        })
    })
}

pub(super) fn store_with(app: Option<&mut App>, change: impl FnOnce(&mut serde_json::Value)) {
    flush_settings(true);
    let Ok(mut v) = omsi_launcher_lib::get_settings() else {
        return;
    };
    change(&mut v);
    invalidate_settings();
    match omsi_launcher_lib::save_settings(&v) {
        Ok(()) => reload_settings(app),
        Err(e) => log::warn!("settings not saved: {e:#}"),
    }
}

pub(super) fn reload_settings(app: Option<&mut App>) {
    flush_settings(true);
    crate::ui_language(&::config::get_string("ui", "language").unwrap_or_else(|| "ENG".into()));
    sync_live(app);
}

pub(crate) fn sync_live(app: Option<&mut App>) {
    crate::startup::SOUND_AI.store(
        (::config::get_float("audio", "ai-volume").unwrap_or(1.0) as f32).to_bits(),
        std::sync::atomic::Ordering::Relaxed,
    );
    crate::startup::SOUND_SCENERY.store(
        (::config::get_float("audio", "scenery-volume").unwrap_or(1.0) as f32).to_bits(),
        std::sync::atomic::Ordering::Relaxed,
    );
    ::audio::DOPPLER.store(
        ::config::get_bool("audio", "doppler").unwrap_or(true),
        std::sync::atomic::Ordering::Relaxed,
    );
    let Some(app) = app else {
        return;
    };
    if let Some(n) = app.navigator.as_mut() {
        n.arrows = ::config::get_bool("navigator", "arrows").unwrap_or(false);
    }
    if let Some(h) = app.humans.as_mut() {
        h.exact_fare = ::config::get_bool("gameplay", "exact_fare").unwrap_or(true);
        h.boarding = ::config::get_string("gameplay", "boarding").unwrap_or_else(|| "auto".into());
        h.prefer_seats = ::config::get_bool("gameplay", "pax_prefer_seats").unwrap_or(false);
        h.rear_entry = ::config::get_bool("gameplay", "pax_rear_entry").unwrap_or(true);
        h.set_ik(app.args.pax_ik.unwrap_or(::config::get_bool("passengers", "ik").unwrap_or(true)));
        h.set_natural(::config::get_string("passengers", "motion").as_deref().unwrap_or("natural") == "natural");
        h.voices = match ::config::get_string("passengers", "voices").as_deref() {
            Some("off") => 2,
            Some("tickets") => 1,
            _ => 0,
        };
    }
}

pub(super) static PENDING_SETTINGS: std::sync::Mutex<(
    Vec<(String, String)>,
    Option<std::time::Instant>,
)> = std::sync::Mutex::new((Vec::new(), None));
pub(super) const SETTINGS_FLUSH_MS: u128 = 250;
pub(super) static MERGED_SETTINGS: std::sync::Mutex<Option<std::sync::Arc<serde_json::Value>>> =
    std::sync::Mutex::new(None);
pub(super) static SETTINGS_CACHE: std::sync::Mutex<Option<serde_json::Value>> =
    std::sync::Mutex::new(None);

/// Write one key of the settings file (the other keys
/// stay as they are). The write is delayed a moment and joined with the ones that follow.
pub(crate) fn remember_setting(key: &str, value: &str) {
    {
        let mut p = PENDING_SETTINGS.lock().unwrap_or_else(|e| e.into_inner());
        match p.0.iter_mut().find(|(k, _)| k == key) {
            Some(e) => e.1 = value.to_string(),
            None => p.0.push((key.to_string(), value.to_string())),
        }
    }
    // after the PENDING_SETTINGS lock is released: settings_file takes the two in the other order
    *MERGED_SETTINGS.lock().unwrap_or_else(|e| e.into_inner()) = None;
    flush_settings(false);
}

/// Write the remembered keys out: all of them when `force`, else only when the last write
/// is `SETTINGS_FLUSH_MS` ago. Called every frame, before the file is read and on exit.
pub(crate) fn flush_settings(force: bool) {
    let pending = {
        let mut p = PENDING_SETTINGS.lock().unwrap_or_else(|e| e.into_inner());
        if p.0.is_empty() {
            return;
        }
        if !force
            && p.1
            .is_some_and(|t| t.elapsed().as_millis() < SETTINGS_FLUSH_MS)
        {
            return;
        }
        p.1 = Some(std::time::Instant::now());
        std::mem::take(&mut p.0)
    };
    let Ok(mut v) = omsi_launcher_lib::get_settings() else {
        return;
    };
    apply_pending(&mut v, &pending);
    if let Err(e) = omsi_launcher_lib::save_settings(&v) {
        log::warn!("settings not saved: {e:#}");
    }
    invalidate_settings();
}

pub(super) fn apply_pending(v: &mut serde_json::Value, pending: &[(String, String)]) {
    for (key, value) in pending {
        let parsed: serde_json::Value = value
            .parse::<f64>()
            .map(serde_json::Value::from)
            .unwrap_or_else(|_| serde_json::Value::from(value.as_str()));
        // a switch goes in as true/false, as the launcher's own values are: written as 1 it
        // was read as not set and saved back as its default (the pause menu's options were
        // lost with the next game)
        let parsed = match (&v[key.as_str()], &parsed) {
            (serde_json::Value::Bool(_), serde_json::Value::Number(n)) => {
                serde_json::Value::Bool(n.as_f64().unwrap_or(0.0) > 0.5)
            }
            _ if key == "time_speed" => serde_json::Value::from(value.as_str()),
            _ => parsed,
        };
        v[key.as_str()] = parsed;
    }
}
