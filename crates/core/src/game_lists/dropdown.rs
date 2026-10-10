//! Drop-downs over rows of the settings windows (weather, clouds, selects, presets).

use super::*;

pub(super) fn weather_files() -> Vec<String> {
    scan_cached(&WEATHER_SCAN, || {
        let mut files: Vec<String> = ::legacy_config::read_dir_merged("Weather")
            .into_iter()
            .filter(|p| {
                p.extension()
                    .map(|e| e.eq_ignore_ascii_case("owt"))
                    .unwrap_or(false)
            })
            .filter_map(|p| {
                p.file_name()
                    .map(|n| format!("Weather/{}", n.to_string_lossy()))
            })
            .collect();
        files.sort_by(|a, b| {
            bus_cmp(
                a.trim_start_matches("Weather/").trim_start_matches('#'),
                b.trim_start_matches("Weather/").trim_start_matches('#'),
            )
        });
        files.dedup();
        files
    })
}

pub(crate) struct Dropdown {
    pub row: usize,
    pub items: Vec<(String, String)>,
    pub sel: usize,
    pub top: usize,
    pub current: Option<usize>,
    /// Type-to-filter
    pub search: Vec<String>,
    pub all: Vec<(String, String)>,
    pub filter: String,
}

pub(crate) fn value_label(setting: &str, value: &str, label: &str) -> String {
    let key = format!("pause.options.value.{setting}.{}", value.replace('.', "_"));
    let text = ::i18n::translate(&key, &[]);
    if text == key {
        ::user_interface::tr(label).into_owned()
    } else {
        text
    }
}

pub(crate) fn dropdown_for(app: &App, row: usize, id: &str) -> Option<Dropdown> {
    let tr = |t: &str| ::user_interface::tr(t).into_owned();
    if app.metar_locked() && matches!(id, "weather" | "cloudkind" | "precipkind") {
        return None;
    }
    let mut current: Option<usize> = None;
    let items: Vec<(String, String)> = match id {
        "weather" => {
            let now = weather_name(app);
            weather_files()
                .into_iter()
                .enumerate()
                .map(|(i, f)| {
                    let stem = f
                        .rsplit('/')
                        .next()
                        .unwrap_or(&f)
                        .rsplit_once('.')
                        .map(|x| x.0)
                        .unwrap_or(&f)
                        .trim_start_matches('#')
                        .to_string();
                    if stem.eq_ignore_ascii_case(&now) {
                        current = Some(i);
                    }
                    (stem, format!("wx {f}"))
                })
                .collect()
        }
        "metar_src" => {
            let mut v = vec![(tr("Automatic (nearest the map)"), "metar_src ".to_string())];
            let own = &::config::get_string("gameplay", "metar_station").unwrap_or_default();
            current = Some(0);
            for (i, (code, label)) in crate::weather_setup::metar_airports(&app.args.root)
                .into_iter()
                .enumerate()
            {
                if code.eq_ignore_ascii_case(own) {
                    current = Some(i + 1);
                }
                v.push((label, format!("metar_src {code}")));
            }
            v
        }
        "cloudkind" => {
            current = app.weather.as_ref().and_then(|w| cloud_index(&w.clouds.0));
            CLOUD_TYPES
                .iter()
                .enumerate()
                .map(|(i, (_, n))| (tr(*n), format!("cloud {i}")))
                .collect()
        }
        "precipkind" => {
            current = app.weather.as_ref().map(|w| {
                (w.precip.first().copied().unwrap_or(0.0).max(0.0) as usize)
                    .min(PRECIP_KINDS.len() - 1)
            });
            PRECIP_KINDS
                .iter()
                .enumerate()
                .map(|(i, n)| (tr(*n), format!("precip {i}")))
                .collect()
        }
        _ => return settings_dropdown(&app.args.root, row, id),
    };
    dropdown_of(row, items, current)
}

/// The drop-downs of the settings that need no running game (the launcher has them too).
pub(crate) fn settings_dropdown(root: &std::path::Path, row: usize, id: &str) -> Option<Dropdown> {
    let tr = |t: &str| ::user_interface::tr(t).into_owned();
    if id.starts_with("pad_") {
        return crate::lab_pads::dropdown(root, row, id);
    }
    let mut current: Option<usize> = None;
    let items: Vec<(String, String)> = match id {
        "preset" => {
            current = preset_now();
            PRESETS
                .iter()
                .enumerate()
                .map(|(i, p)| (value_label("preset", &p.0.to_lowercase(), p.0), format!("preset {i}")))
                .collect()
        }
        "gfxprofile" => ::config::get_subs("graphics_profiles")
            .into_iter()
            .map(|n| (n.clone(), format!("gfxprofile {n}")))
            .collect(),
        "reset" => vec![
            (tr("Cancel"), "noop".to_string()),
            (tr("Reset all settings"), "reset_all".to_string()),
        ],
        key if key.starts_with("sel ") => {
            let key = &key[4..];
            let (options, at, _) = select_state(&settings_file(), key);
            current = at;
            options
                .iter()
                .map(|o| (value_label(key, o.0, o.1), format!("pick {key} {}", o.0)))
                .collect()
        }
        _ => return None,
    };
    dropdown_of(row, items, current)
}

fn dropdown_of(row: usize, items: Vec<(String, String)>, current: Option<usize>) -> Option<Dropdown> {
    if items.is_empty() {
        return None;
    }
    let sel = current.unwrap_or(0);
    Some(Dropdown {
        row,
        items,
        sel,
        top: 0,
        current,
        search: Vec::new(),
        all: Vec::new(),
        filter: String::new(),
    })
}

pub(crate) fn dropdown_apply(app: &mut App, action: &str) {
    let (verb, arg) = action.split_once(' ').unwrap_or((action, ""));
    if verb.starts_with("pad_") {
        crate::lab_pads::apply(app.controllers.as_mut(), verb, arg);
        return;
    }
    if app.metar_locked() && matches!(verb, "wx" | "cloud" | "precip") {
        app.service_msg = Some((
            "The weather cannot be changed while the METAR sync is on".into(),
            3.0,
        ));
        return;
    }
    match verb {
        "wx" => {
            if app
                .lan
                .as_ref()
                .is_some_and(|l| l.role == ::network::Role::Client)
            {
                app.service_msg = Some((::i18n::translate("pause.msg.lan_weather", &[]), 3.0));
            } else {
                app.change_weather(Some(arg.to_string()), true, 1.0);
            }
        }
        "metar_src" => {
            let code: String = arg
                .trim()
                .chars()
                .filter(|c| c.is_ascii_alphabetic())
                .take(4)
                .collect::<String>()
                .to_ascii_uppercase();
            ::config::set_setting("gameplay", "metar_station", code.clone());
            let _ = ::config::save();
            app.metar_rx = None;
            app.metar_once = false;
            // With sync on, the new station is fetched at once. With it off this simply
            // selects the station for "Load current METAR once".
            app.metar_next = 0.0;
        }
        "cloud" => {
            if let Some(i) = arg
                .trim()
                .parse::<usize>()
                .ok()
                .filter(|i| *i < CLOUD_TYPES.len())
            {
                app.edit_weather(|w| {
                    w.clouds.0 = CLOUD_TYPES[i].0.to_string();
                    if i == 0 {
                        w.clouds.1 = 0.0;
                    }
                });
            }
        }
        "precip" => {
            if let Some(i) = arg
                .trim()
                .parse::<usize>()
                .ok()
                .filter(|i| *i < PRECIP_KINDS.len())
            {
                set_precip(app, i);
            }
        }
        _ => {
            if let Some(m) = settings_pick(Some(app), action) {
                app.service_msg = Some((m, 5.0));
            }
        }
    }
}

/// What a drop-down of the settings chose, in a game or in the launcher (no `app`): a
/// message to show when there is one.
pub(crate) fn settings_pick(mut app: Option<&mut App>, action: &str) -> Option<String> {
    let msg = settings_pick_inner(app.as_deref_mut(), action);
    let restart = match action.split_once(' ') {
        Some(("pick", rest)) => rest.split_once(' ').is_some_and(|(k, _)| super::options::RESTART_KEYS.contains(&k)),
        Some(("preset" | "gfxprofile", _)) => true,
        _ => false,
    };
    if let Some(app) = app {
        if restart {
            app.restart_pending = true;
        }
        apply_live_settings(app);
    }
    msg
}

fn settings_pick_inner(mut app: Option<&mut App>, action: &str) -> Option<String> {
    let (verb, arg) = action.split_once(' ').unwrap_or((action, ""));
    match verb {
        "pick" => {
            if let Some((key, value)) = arg.split_once(' ') {
                if key == "boarding" {
                    ::config::set_setting("gameplay", "boarding", value);
                    let _ = ::config::save();
                } else if key == "ai_unsched_factor" {
                    if let Ok(v) = value.parse::<f64>() {
                        ::config::set_setting("ai", "unsched_factor", v / 100.0);
                        let _ = ::config::save();
                    }
                } else if matches!(key, "ai_max_scheduled" | "ai_max_parked") {
                    if let Ok(v) = value.parse::<i64>() {
                        ::config::set_setting("ai", &key[3..], v);
                        let _ = ::config::save();
                    }
                } else if matches!(key, "pax_voices" | "pax_models" | "pax_motion") {
                    ::config::set_setting("passengers", &key[4..], value);
                    let _ = ::config::save();
                    if key == "pax_models" && value == "realistic" {
                        crate::pax_pack::fetch_if_needed(crate::startup::content_dir);
                    }
                } else if key == "language" {
                    ::config::set_setting("ui", "language", crate::describe::language_code(value));
                    let _ = ::config::save();
                } else if key == "units" {
                    ::config::set_setting("ui", "units", value);
                    let _ = ::config::save();
                } else if key == "navigator_corner" {
                    ::config::set_setting("ui", "navigator_corner", value.to_ascii_lowercase());
                    let _ = ::config::save();
                } else if key == "maintenance" {
                    if let Ok(v) = value.parse::<i64>() {
                        ::config::set_setting("gameplay", "maintenance", v);
                        let _ = ::config::save();
                    }
                } else if key == "head_tracking_invert" {
                    ::config::set_setting("camera", "head_tracking_invert", if value == "none" { "" } else { value });
                    let _ = ::config::save();
                } else if key == "head_tracking_port" {
                    if let Ok(v) = value.parse::<i64>() {
                        ::config::set_setting("camera", "head_tracking_port", v);
                        let _ = ::config::save();
                    }
                } else if matches!(key, "vr_scale" | "vr_head_smoothing_ms" | "vr_mirror_rate") {
                    if let Some(v) = value.parse::<f64>().ok().filter(|v| v.is_finite()) {
                        ::config::set_setting("vr", &key[3..].replace('_', "-"), v);
                        let _ = ::config::save();
                    }
                } else if ::config::DEFAULTS.iter().any(|(c, k, _)| *c == "graphics" && *k == key) {
                    gfx_set(key, value);
                    if let Some(app) = app.as_deref().filter(|_| key == "window_mode") {
                        super::options::apply_window_mode(app, &gfx_text(key));
                    }
                    let _ = ::config::save();
                } else {
                    remember_setting(key, value);
                }
                reload_settings(app.as_deref_mut());
                if key == "language" {
                    crate::ui_language(&::config::get_string("ui", "language").unwrap_or_else(|| "ENG".into()));
                }
            }
        }
        "preset" => {
            if let Some(p) = arg.trim().parse::<usize>().ok().and_then(|i| PRESETS.get(i)) {
                for (k, v) in p.1 {
                    gfx_set(k, v);
                }
                let _ = ::config::save();
            }
        }
        "gfxprofile" => {
            let name = arg.trim();
            let profile = ::config::get_table_sub("graphics_profiles", name);
            if profile.is_empty() {
                return Some(::i18n::translate("pause.msg.graphics_missing", &[("name", &name)]));
            } else {
                for (k, v) in profile {
                    if ::config::DEFAULTS.iter().any(|(c, key, _)| *c == "graphics" && *key == k) {
                        ::config::set_setting("graphics", &k, v);
                    }
                }
                let _ = ::config::save();
                sync_live(app);
                LIST_DIRTY.store(true, std::sync::atomic::Ordering::Relaxed);
                return Some(format!(
                    "Graphics profile \"{name}\" loaded: graphics settings apply when the game starts the next time"
                ));
            }
        }
        "reset_all" => {
            for (cat, key, _) in ::config::DEFAULTS {
                if *cat == "graphics" {
                    ::config::reset_setting(cat, key);
                }
            }
            let _ = ::config::save();
            store_with(app, |v| {
                let language = v.get("language").cloned();
                *v = omsi_launcher_lib::default_settings();
                if let Some(l) = language {
                    v["language"] = l;
                }
            })
        }
        _ => {}
    }
    None
}

pub(super) fn weather_name(app: &App) -> String {
    if app
        .weather
        .as_ref()
        .is_some_and(|w| w.name == CUSTOM_WEATHER)
    {
        return CUSTOM_WEATHER.to_string();
    }
    if app
        .args
        .weather
        .as_deref()
        .is_some_and(|p| p.starts_with(crate::weather_setup::REPORT))
    {
        if let Some(n) = app
            .weather
            .as_ref()
            .map(|w| w.name.trim().to_string())
            .filter(|n| !n.is_empty())
        {
            return n;
        }
    }
    match app.args.weather.as_deref() {
        Some(p) => {
            let p = p.replace('\\', "/");
            let file = p.rsplit('/').next().unwrap_or("");
            let stem = file.rsplit_once('.').map(|x| x.0).unwrap_or(file);
            stem.trim_start_matches('#').to_string()
        }
        None => app
            .weather
            .as_ref()
            .map(|w| w.name.trim().to_string())
            .filter(|n| !n.is_empty())
            .unwrap_or_else(|| "Map default".to_string()),
    }
}

pub(super) fn value_text(v: &serde_json::Value) -> String {
    match v {
        serde_json::Value::String(x) => x.clone(),
        serde_json::Value::Bool(b) => (*b as u8).to_string(),
        serde_json::Value::Number(n) => n.to_string(),
        _ => String::new(),
    }
}

pub(super) fn same_value(a: &str, b: &str) -> bool {
    a == b
        || a.parse::<f64>()
        .ok()
        .zip(b.parse::<f64>().ok())
        .is_some_and(|(x, y)| (x - y).abs() < 1e-6)
}

pub(super) fn select_options(key: &str) -> Vec<(&'static str, &'static str)> {
    match key {
        "graphics" => vec![
            ("vanilla", "Vanilla (as OMSI 2)"),
            ("vanilla_plus", "Vanilla+"),
            ("enhanced", "Enhanced"),
        ],
        "window_mode" => vec![
            ("windowed", "Windowed"),
            ("borderless", "Windowed Borderless"),
            ("fullscreen", "Fullscreen"),
        ],
        "msaa" => vec![
            ("1", "Off"),
            ("2", "2x MSAA"),
            ("4", "4x MSAA"),
            ("8", "8x MSAA"),
        ],
        "render_scale" => vec![
            ("0", "Auto"),
            ("1", "Off (no upscaler)"),
            ("0.85", "85%"),
            ("0.75", "75%"),
            ("0.67", "67%"),
            ("0.5", "50%"),
        ],
        "anisotropy" => vec![("1", "Off"), ("2", "2x"), ("4", "4x"), ("8", "8x")],
        "shadow_size" => vec![("1024", "1024"), ("2048", "2048"), ("4096", "4096")],
        "shadow_casters" => vec![
            ("all", "Every solid mesh"),
            ("omsi", "[shadow] meshes, as OMSI"),
        ],
        "max_fps" => vec![
            ("0", "Screen refresh rate"),
            ("30", "30 fps"),
            ("45", "45 fps"),
            ("60", "60 fps"),
            ("120", "120 fps"),
            ("144", "144 fps"),
            ("1000", "Unlimited"),
        ],
        "map_detail" => vec![
            ("-1", "OMSI setting"),
            ("0", "Low"),
            ("1", "Normal"),
            ("2", "Full"),
            ("255", "All authored levels"),
        ],
        "view_distance" => vec![
            ("0", "Default (900 m)"),
            ("600", "600 m - fastest"),
            ("900", "900 m"),
            ("1200", "1200 m"),
            ("1500", "1500 m"),
            ("2000", "2000 m"),
            ("2500", "2500 m"),
        ],
        "max_obj_dist" => vec![
            ("-1", "Automatic"),
            ("500", "500 m"),
            ("750", "750 m"),
            ("900", "900 m"),
            ("1500", "1500 m"),
            ("3000", "3000 m"),
        ],
        "min_obj_size" => vec![
            ("0.005", "All"),
            ("0.013", "Normal"),
            ("0.02", "Fewer (faster)"),
            ("0.03", "Few (fastest)"),
        ],
        "mirror_size" => vec![
            ("0", "Off"),
            ("128", "Low (128)"),
            ("256", "Normal (256)"),
            ("512", "High (512)"),
            ("1024", "Very high (1024)"),
        ],
        "texture_memory" => vec![
            ("0", "Automatic"),
            ("500", "500 MB"),
            ("1000", "1 GB"),
            ("1500", "1.5 GB"),
            ("2000", "2 GB"),
            ("3000", "3 GB"),
            ("4000", "4 GB"),
            ("6000", "6 GB"),
        ],
        "post_aa" => vec![("fxaa", "FXAA"), ("off", "Off")],
        "mirror_refresh" => vec![("eco", "Economy"), ("full", "Full")],
        "graphics_api" => vec![("auto", "Automatic"), ("vulkan", "Vulkan"), ("dx12", "DirectX 12")],
        "head_tracking_invert" => vec![
            ("none", "None"),
            ("yaw", "Yaw"),
            ("pitch", "Pitch"),
            ("roll", "Roll"),
            ("yaw pitch", "Yaw and pitch"),
            ("yaw roll", "Yaw and roll"),
            ("pitch roll", "Pitch and roll"),
            ("yaw pitch roll", "Yaw, pitch and roll"),
        ],
        "head_tracking_port" => vec![("4242", "4242"), ("4243", "4243"), ("5005", "5005"), ("5555", "5555")],
        "units" => vec![
            ("metric", "Metric (km/h, km, °C)"),
            ("uk", "UK (mph, miles, °C)"),
            ("imperial", "Imperial (mph, miles, °F)"),
        ],
        "navigator_corner" => vec![
            ("top-left", "Top left"),
            ("top-right", "Top right"),
            ("bottom-left", "Bottom left"),
            ("bottom-right", "Bottom right"),
        ],
        "boarding" => vec![
            ("auto", "Pay and take the ticket"),
            ("pay", "The driver sells the ticket"),
            ("walk", "Just walk in"),
        ],
        "pax_models" => vec![("omsi", "OMSI 2"), ("realistic", "Realistic")],
        "pax_motion" => vec![("natural", "Natural"), ("omsi", "OMSI 2")],
        "pax_voices" => vec![
            ("all", "Greetings and tickets"),
            ("tickets", "Only the ticket asked for"),
            ("off", "Silent"),
        ],
        "maintenance" => vec![
            ("0", "Infinite (no wear)"),
            ("1", "Very bad"),
            ("2", "Bad"),
            ("3", "Normal"),
            ("4", "Good"),
        ],
        "ai_unsched_factor" => vec![
            ("25", "25%"),
            ("50", "50%"),
            ("75", "75%"),
            ("100", "100%"),
            ("150", "150%"),
            ("200", "200%"),
        ],
        "ai_max_scheduled" => vec![
            ("0", "All"),
            ("10", "At most 10"),
            ("25", "At most 25"),
            ("50", "At most 50"),
        ],
        "ai_max_parked" => vec![
            ("-1", "None"),
            ("0", "Every space"),
            ("35", "At most 35"),
            ("100", "At most 100"),
            ("250", "At most 250"),
        ],
        "language" => omsi_launcher_lib::LANGUAGES
            .iter()
            .map(|l| (l.0, l.1))
            .collect(),
        "vr_scale" => vec![
            ("0.5", "50%"),
            ("0.65", "65%"),
            ("0.8", "80%"),
            ("1", "100%"),
        ],
        "vr_head_smoothing_ms" => vec![
            ("0", "Off"),
            ("5", "5 ms"),
            ("10", "10 ms"),
            ("20", "20 ms"),
            ("30", "30 ms"),
        ],
        "vr_mirror_rate" => vec![
            ("0", "Off"),
            ("8", "8/s"),
            ("16", "16/s"),
            ("24", "24/s"),
            ("32", "32/s"),
            ("48", "48/s"),
            ("60", "60/s"),
            ("90", "90/s"),
            ("120", "120/s"),
            ("180", "180/s"),
            ("240", "240/s"),
            ("360", "360/s"),
            ("-1", "Every frame"),
        ],
        _ => Vec::new(),
    }
}

pub(super) fn select_state(
    file: &serde_json::Value,
    key: &str,
) -> (Vec<(&'static str, &'static str)>, Option<usize>, String) {
    let options = select_options(key);
    let cur = if key == "boarding" {
        ::config::get_string("gameplay", "boarding").unwrap_or_default()
    } else if key == "ai_unsched_factor" {
        ::config::get_float("ai", "unsched_factor")
            .map(|v| ((v * 100.0).round() as i64).to_string())
            .unwrap_or_default()
    } else if matches!(key, "ai_max_scheduled" | "ai_max_parked") {
        ::config::get_int("ai", &key[3..])
            .map(|v| v.to_string())
            .unwrap_or_default()
    } else if matches!(key, "pax_voices" | "pax_models" | "pax_motion") {
        ::config::get_string("passengers", &key[4..]).unwrap_or_default()
    } else if key == "language" {
        ::config::get_string("ui", "language").unwrap_or_default()
    } else if key == "units" {
        ::config::get_string("ui", "units").unwrap_or_default()
    } else if key == "navigator_corner" {
        ::config::get_string("ui", "navigator_corner").unwrap_or_default()
    } else if key == "maintenance" {
        ::config::get_int("gameplay", "maintenance")
            .map(|v| v.to_string())
            .unwrap_or_default()
    } else if key == "head_tracking_invert" {
        let v = ::config::get_string("camera", "head_tracking_invert").unwrap_or_default();
        if v.trim().is_empty() { "none".to_string() } else { v }
    } else if key == "head_tracking_port" {
        ::config::get_int("camera", "head_tracking_port").map(|v| v.to_string()).unwrap_or_default()
    } else if matches!(key, "vr_scale" | "vr_head_smoothing_ms" | "vr_mirror_rate") {
        ::config::get_float("vr", &key[3..].replace('_', "-"))
            .map(|v| v.to_string())
            .unwrap_or_default()
    } else if ::config::DEFAULTS.iter().any(|(c, k, _)| *c == "graphics" && *k == key) {
        gfx_text(key)
    } else {
        value_text(file.get(key).unwrap_or(&serde_json::Value::Null))
    };
    let at = options.iter().position(|o| same_value(o.0, &cur));
    (options, at, cur)
}

pub(crate) fn select_row(
    file: &serde_json::Value,
    key: &str,
    name: &str,
    desc: &str,
) -> Option<(String, String)> {
    let (options, at, cur) = select_state(file, key);
    if options.is_empty() {
        return None;
    }
    let label = at
        .map(|i| value_label(key, options[i].0, options[i].1))
        .unwrap_or(cur);
    Some((row(name, 'o', &label, desc, None), format!("sel {key}")))
}

/// The realistic passengers' download, under the choice of the models.
pub(crate) fn pax_pack_row() -> (String, String) {
    use crate::pax_pack::Status;
    let t = |k: &str| ::i18n::translate(&format!("pause.options.gameplay.pax_pack.{k}"), &[]);
    let (button, desc) = match crate::pax_pack::status(crate::startup::content_dir) {
        Status::Missing => (t("download"), t("missing")),
        Status::Outdated => (t("update"), t("outdated")),
        Status::Downloading { done, total } => (
            format!("{} %", (done * 100).checked_div(total).unwrap_or(0)),
            t("downloading"),
        ),
        Status::Installing => (String::new(), t("installing")),
        Status::Installed => (t("installed"), t("next_start")),
        Status::Failed(e) => (t("retry"), e),
    };
    (row(&t("name"), 'a', &button, &desc, None), "pax_pack_get".to_string())
}

pub(crate) fn preset_row(
    name: &str,
    desc: &str,
) -> Option<(String, String)> {
    let label = match preset_now() {
        Some(i) => value_label("preset", &PRESETS[i].0.to_lowercase(), PRESETS[i].0),
        None => value_label("preset", "custom", "Custom"),
    };
    Some((row(name, 'o', &label, desc, None), "preset".to_string()))
}

#[cfg(test)]
mod tests {
    use super::select_options;

    #[test]
    fn window_mode_choices_remain_distinct() {
        assert_eq!(
            select_options("window_mode"),
            vec![
                ("windowed", "Windowed"),
                ("borderless", "Windowed Borderless"),
                ("fullscreen", "Fullscreen"),
            ]
        );
    }
}
