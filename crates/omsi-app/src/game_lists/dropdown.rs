//! Drop-downs over rows of the settings windows (weather, clouds, selects, presets).

use super::*;

pub(super) fn weather_files() -> Vec<String> {
    scan_cached(&WEATHER_SCAN, || {
        let mut files: Vec<String> = omsi_cfg::read_dir_merged("Weather")
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
}

pub(crate) fn dropdown_for(app: &App, row: usize, id: &str) -> Option<Dropdown> {
    let tr = |t: &str| omsi_ui::tr(t).into_owned();
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
            let own = &app.settings.metar_station;
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
        "preset" => {
            current = preset_now(&settings_file());
            presets()
                .iter()
                .enumerate()
                .map(|(i, p)| (tr(p.0), format!("preset {i}")))
                .collect()
        }
        "gfxprofile" => omsi_launcher_lib::graphics_profiles()
            .into_keys()
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
                .map(|o| (tr(o.1), format!("pick {key} {}", o.0)))
                .collect()
        }
        _ => return None,
    };
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
    })
}

pub(crate) fn dropdown_apply(app: &mut App, action: &str) {
    let (verb, arg) = action.split_once(' ').unwrap_or((action, ""));
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
                .is_some_and(|l| l.role == omsi_net::Role::Client)
            {
                app.service_msg = Some(("In a LAN session the host sets the weather".into(), 3.0));
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
            app.settings.metar_station = code.clone();
            remember_setting("metar_station", &code);
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
        "pick" => {
            if let Some((key, value)) = arg.split_once(' ') {
                remember_setting(key, value);
                reload_settings(app);
            }
        }
        "preset" => {
            if let Some(p) = arg
                .trim()
                .parse::<usize>()
                .ok()
                .and_then(|i| presets().into_iter().nth(i))
            {
                store_with(app, |v| {
                    if let Some(o) = p.1.as_object() {
                        for (k, x) in o {
                            v[k.as_str()] = x.clone();
                        }
                    }
                });
            }
        }
        "gfxprofile" => {
            let name = arg.trim();
            match omsi_launcher_lib::graphics_profiles().get(name) {
                Some(p) => {
                    store_with(app, |v| omsi_launcher_lib::apply_graphics_profile(p, v));
                    sync_live(app);
                    LIST_DIRTY.store(true, std::sync::atomic::Ordering::Relaxed);
                    app.service_msg = Some((
                        format!(
                            "Graphics profile \"{name}\" loaded: graphics settings apply when the game starts the next time"
                        ),
                        5.0,
                    ));
                }
                None => {
                    app.service_msg = Some((format!("Graphics profile \"{name}\" not found"), 4.0))
                }
            }
        }
        "reset_all" => store_with(app, |v| {
            let language = v.get("language").cloned();
            *v = omsi_launcher_lib::settings_from_text(None);
            if let Some(l) = language {
                v["language"] = l;
            }
        }),
        _ => {}
    }
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
        "msaa" => vec![
            ("1", "Off"),
            ("2", "2x MSAA"),
            ("4", "4x MSAA"),
            ("8", "8x MSAA"),
        ],
        "render_scale" => vec![
            ("auto", "Auto"),
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
            ("0", "Low"),
            ("1", "Normal"),
            ("2", "Full"),
            ("255", "All authored levels"),
        ],
        "view_distance" => vec![
            ("auto", "Default (900 m)"),
            ("600", "600 m - fastest"),
            ("900", "900 m"),
            ("1200", "1200 m"),
            ("1500", "1500 m"),
            ("2000", "2000 m"),
            ("2500", "2500 m"),
        ],
        "max_obj_dist" => vec![
            ("auto", "Automatic"),
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
        "drive_keys" => vec![
            ("omsi", "Custom controls (Controls page)"),
            ("simple", "W A S D + arrows"),
            ("wasd", "W A S D only"),
            ("arrows", "Arrow keys only"),
        ],
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
    let cur = value_text(file.get(key).unwrap_or(&serde_json::Value::Null));
    let at = options.iter().position(|o| same_value(o.0, &cur));
    (options, at, cur)
}

pub(super) fn select_row(
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
        .map(|i| omsi_ui::tr(options[i].1).into_owned())
        .unwrap_or(cur);
    Some((row(name, 'o', &label, desc, None), format!("sel {key}")))
}

pub(super) fn presets() -> [(&'static str, serde_json::Value); 4] {
    [
        (
            "Low",
            serde_json::json!({"msaa": 1, "anisotropy": 2, "shadow_size": 1024, "ssao": false, "shadows": false, "detail_textures": false, "clouds": false, "view_distance": "600", "min_obj_size": 0.03, "max_obj_dist": "500", "mirror_size": 128, "render_scale": "0.75", "texture_memory": 800}),
        ),
        (
            "Medium",
            serde_json::json!({"msaa": 2, "anisotropy": 4, "shadow_size": 2048, "ssao": false, "shadows": true, "detail_textures": true, "clouds": true, "view_distance": "900", "min_obj_size": 0.02, "max_obj_dist": "750", "mirror_size": 256, "render_scale": "auto", "texture_memory": 1200}),
        ),
        (
            "High",
            serde_json::json!({"msaa": 4, "anisotropy": 8, "shadow_size": 2048, "ssao": true, "shadows": true, "detail_textures": true, "clouds": true, "view_distance": "auto", "min_obj_size": 0.013, "max_obj_dist": "auto", "mirror_size": 256, "render_scale": "auto", "texture_memory": 0}),
        ),
        (
            "Ultra",
            serde_json::json!({"msaa": 4, "anisotropy": 8, "shadow_size": 4096, "ssao": true, "shadows": true, "detail_textures": true, "clouds": true, "view_distance": "2000", "min_obj_size": 0.005, "max_obj_dist": "1500", "mirror_size": 512, "render_scale": "auto", "texture_memory": 0}),
        ),
    ]
}

pub(super) fn preset_now(file: &serde_json::Value) -> Option<usize> {
    presets().iter().position(|p| {
        p.1.as_object().is_some_and(|o| {
            o.iter().all(|(k, v)| {
                same_value(
                    &value_text(v),
                    &value_text(file.get(k).unwrap_or(&serde_json::Value::Null)),
                )
            })
        })
    })
}

pub(super) fn preset_row(
    file: &serde_json::Value,
    name: &str,
    desc: &str,
) -> Option<(String, String)> {
    let label =
        omsi_ui::tr(preset_now(file).map(|i| presets()[i].0).unwrap_or("Custom")).into_owned();
    Some((row(name, 'o', &label, desc, None), "preset".to_string()))
}
