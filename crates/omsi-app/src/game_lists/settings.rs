//! The launcher's settings file as the lists read it, with delayed writes.

use super::*;

/// The launcher's settings file as the lists show it: read once (until something is
/// written), with the keys still waiting to be written on top.
pub(super) fn settings_file() -> std::sync::Arc<serde_json::Value> {
    // lock order: MERGED_SETTINGS, then SETTINGS_CACHE and PENDING_SETTINGS
    let mut merged = MERGED_SETTINGS.lock().unwrap_or_else(|e| e.into_inner());
    if let Some(v) = merged.as_ref() {
        return v.clone();
    }
    let mut v = SETTINGS_CACHE
        .lock()
        .unwrap_or_else(|e| e.into_inner())
        .get_or_insert_with(|| {
            let text =
                std::fs::read_to_string(omsi_launcher_lib::data_dir().join("settings.cfg")).ok();
            omsi_launcher_lib::settings_from_text(text.as_deref())
        })
        .clone();
    let pending = PENDING_SETTINGS.lock().unwrap_or_else(|e| e.into_inner());
    apply_pending(&mut v, &pending.0);
    let v = std::sync::Arc::new(v);
    *merged = Some(v.clone());
    v
}

/// Forget the file as read (and the merged copy of it): it is read again on the next ask.
fn invalidate_settings() {
    *SETTINGS_CACHE.lock().unwrap_or_else(|e| e.into_inner()) = None;
    *MERGED_SETTINGS.lock().unwrap_or_else(|e| e.into_inner()) = None;
}

pub(super) fn store_with(app: &mut App, change: impl FnOnce(&mut serde_json::Value)) {
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

pub(super) fn reload_settings(app: &mut App) {
    flush_settings(true);
    app.settings = crate::settings::Settings::load();
    crate::ui_language(&app.settings.language);
    sync_live(app);
}

pub(super) fn sync_live(app: &mut App) {
    let s = &app.settings;
    crate::startup::SOUND_AI.store(s.vol_ai.to_bits(), std::sync::atomic::Ordering::Relaxed);
    crate::startup::SOUND_SCENERY.store(
        s.vol_scenery.to_bits(),
        std::sync::atomic::Ordering::Relaxed,
    );
    omsi_audio::DOPPLER.store(s.doppler, std::sync::atomic::Ordering::Relaxed);
    if let Some(n) = app.navigator.as_mut() {
        n.arrows = s.nav_arrows;
    }
    if let Some(h) = app.humans.as_mut() {
        h.exact_fare = s.exact_fare;
        h.boarding = s.boarding.clone();
        h.voices = match s.pax_voices.as_str() {
            "off" => 2,
            "tickets" => 1,
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

/// Write one key of `~/.neoomsi/settings.cfg` (the launcher's file; the other lines
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
