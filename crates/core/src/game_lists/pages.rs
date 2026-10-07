//! Pages of rows for the options, vehicle and world windows.

use super::*;

pub(super) fn row(name: &str, kind: char, value: &str, desc: &str, frac: Option<f32>) -> String {
    format!(
        "{name}\u{1f}{kind}\u{1f}{value}\u{1f}{desc}\u{1f}{}",
        frac.map(|f| format!("{f:.3}")).unwrap_or_default()
    )
}

pub(super) fn opens(name: &str, desc: &str, id: &str) -> (String, String) {
    (row(name, 'o', "", desc, None), id.to_string())
}

pub(super) fn button(name: &str, text: &str, desc: &str, id: &str) -> (String, String) {
    (row(name, 'a', text, desc, None), id.to_string())
}

pub(super) fn switch_row(app: &App, id: &str, name: &str, desc: &str) -> Option<(String, String)> {
    let on = toggle_now(app, id)?;
    Some((
        row(name, 's', if on { "on" } else { "off" }, desc, None),
        id.to_string(),
    ))
}

pub(super) fn slider_row(
    app: &App,
    id: &str,
    name: &str,
    desc: &str,
    fmt: &dyn Fn(f32) -> String,
) -> Option<(String, String)> {
    let (verb, arg) = id.split_once(' ').unwrap_or((id, ""));
    let steps = steps_of(verb)?;
    let now = option_now(app, verb, arg)?;
    let i = nearest(&steps, now);
    let frac = if steps.len() > 1 {
        i as f32 / (steps.len() - 1) as f32
    } else {
        0.0
    };
    Some((row(name, 'v', &fmt(now), desc, Some(frac)), id.to_string()))
}

pub(crate) const MAP_TAB: usize = 99;
pub(crate) const KEYS_TAB: usize = 98;
pub(crate) const LOOK_TAB: usize = 97;

pub(crate) fn is_sub_tab(t: usize) -> bool {
    t == MAP_TAB || t == KEYS_TAB || t == LOOK_TAB
}
pub(super) fn look_options_page(app: &App) -> Page {
    let rows: Vec<(String, String)> = vec![
        switch_row(
            app,
            "free_look",
            "Free look",
            "Mouse turns the view, the screen centre operates things; Left Alt shows the cursor",
        ),
        switch_row(
            app,
            "crosshair",
            "Crosshair",
            "Shows a small ring in the middle of the screen (not in the F2 and F3 views)",
        ),
        switch_row(
            app,
            "tooltips",
            "Names of buttons",
            "Shows the name of what the cursor or the screen centre points at",
        ),
    ]
        .into_iter()
        .flatten()
        .collect();
    ("Free look", rows)
}
pub(super) fn map_options_page(app: &App) -> Page {
    let file = settings_file();
    let rows: Vec<(String, String)> = vec![
        switch_row(app, "navigator", "Map", "Enables/Disables the Minimap"),
        switch_row(
            app,
            "nav_topbar",
            "Top bar",
            "Speed, speed limit and time at the top of the map",
        ),
        switch_row(
            app,
            "nav_turn",
            "Turn indicator",
            "The next turn at the top of the map",
        ),
        switch_row(
            app,
            "nav_stoplist",
            "Stop list",
            "The next stop below the map",
        ),
        switch_row(
            app,
            "nav_stops_ext",
            "Extended stop list",
            "Shows more stops below the next stop instead of only the basic information",
        ),
        switch_row(
            app,
            "nav_ai",
            "AI vehicles",
            "Shows/hides the other (AI) vehicles on the Minimap and the city map",
        ),
        select_row(
            &file,
            "navigator_corner",
            "Corner",
            "Takes effect when the game starts the next time",
        ),
    ]
        .into_iter()
        .flatten()
        .collect();
    ("Map", rows)
}

pub(super) fn options_pages(app: &App) -> Vec<Page> {
    let file = settings_file();
    let pick = |key: &str, name: &str, desc: &str| select_row(&file, key, name, desc);
    let pct = |v: f32| format!("{:.0} %", v * 100.0);
    let cm = |v: f32| format!("{:+.0} cm", v * 100.0);
    let later = "Takes effect when the game starts the next time";
    let game: Vec<(String, String)> = vec![
        switch_row(
            app,
            "auto_ibis",
            "Automatic IBIS",
            "When enabled, the selected tour is automatically entered into IBIS",
        ),
        switch_row(
            app,
            "exact_fare",
            "Passengers pay the exact fare",
            "No change is given at the cash desk",
        ),
        pick("pax_motion", "Passenger movement", "Natural movement or OMSI 2 comparison mode"),
        switch_row(
            app,
            "pax_ik",
            "Procedural passenger animation",
            "Use procedural poses instead of OMSI 2 animation poses",
        ),
        pick("pax_models", "Passenger models", "RealisticPax applies on the next start"),
        pick("boarding", "Boarding", "How passengers get their tickets"),
        switch_row(
            app,
            "pax_prefer_seats",
            "Passengers prefer available seats",
            "Passengers take a free seat when boarding; standing places are used when all seats are taken",
        ),
        pick("maintenance", "Maintenance", later),
        switch_row(
            app,
            "coll_objects",
            "Collisions with objects",
            "Enables/disables collisions with objects such as buildings, streetlights, etc.",
        ),
        switch_row(
            app,
            "coll_vehicles",
            "Collisions with vehicles",
            "Enables/Disables Collisions with Other Vehicles",
        ),
        switch_row(
            app,
            "collision_pedestrians",
            "Collisions with people",
            "Enables/disables knocking down people",
        ),
        pick("ai_unsched_factor", "Random traffic", later),
        pick("ai_max_scheduled", "Timetable vehicles", later),
        pick("ai_max_parked", "Parked cars", later),
    ]
        .into_iter()
        .flatten()
        .collect();
    let driving: Vec<(String, String)> = vec![
        switch_row(app, "auto_clutch", "Automatic clutch", "Automatically operate the clutch for you"),
        switch_row(app, "auto_shift", "Automated manual gearbox", "Shift a manual gearbox's gears for you by the engine speed"),
        switch_row(app, "momentary_gears", "Hold manual gear buttons (release returns to neutral)", later),
        switch_row(app, "brake_hold", "Keyboard brake stays on", "Keep the brake applied until the throttle is pressed"),
        switch_row(app, "blinker_cancel", "Indicators cancel themselves", "The bus's script turns the indicator off after a turn; off: it stays on until you turn it off"),
        switch_row(app, "steering_linear", "Steering linearity (keys at OMSI's steady pace)", "Keyboard steering at OMSI's steady pace"),
        switch_row(app, "old_steering", "Old Steering (the wheel stays, turn it back yourself)", "The wheel stays where the keys left it"),
        switch_row(app, "red_steer_spd", "Dynamic steering (slower keys at speed, OMSI's redSteerSpd)", "The steering keys act slower at speed"),
    ]
        .into_iter()
        .flatten()
        .collect();
    let controls: Vec<(String, String)> = vec![
        pick("drive_keys", "Driving keys", "Which keys drive the vehicle"),
        Some(opens(
            "Key bindings",
            "Set every key of the bus and of the game",
            "keysopts",
        )),
        switch_row(
            app,
            "mouse",
            "Steering with the mouse",
            "Steer and control the pedals using the mouse",
        ),
        switch_row(
            app,
            "mouse_right",
            "A right click ends the mouse steering",
            "As in OMSI; off: the right button only looks round",
        ),
        slider_row(
            app,
            "mouse_sens",
            "Mouse steering sensitivity",
            "Adjust how much the steering wheel turns based on mouse movement",
            &pct,
        ),
        slider_row(
            app,
            "stick_sens",
            "Gamepad steering sensitivity",
            "How much a small stick push turns the wheel; a full push is still full lock",
            &pct,
        ),
        switch_row(
            app,
            "steer_center",
            "Wheel returns to the middle",
            "Steering a hair off the middle counts as straight",
        ),
        slider_row(
            app,
            "pedal_t",
            "Throttle pedal strength",
            "Adjust how strongly pedal input affects the throttle",
            &|v| format!("x{v}"),
        ),
        slider_row(
            app,
            "pedal_b",
            "Brake pedal strength",
            "Adjust how strongly pedal input affects the brake",
            &|v| format!("x{v}"),
        ),
        slider_row(
            app,
            "wheel_range",
            "Wheel rotation",
            "The steering wheel's own rotation, lock to lock",
            &|v| format!("{v:.0}°"),
        ),
        slider_row(
            app,
            "wheel_lock",
            "Full lock at",
            "How far the wheel turns for the vehicle's full lock",
            &|v| {
                if v < 45.0 {
                    "OMSI".to_string()
                } else {
                    format!("{v:.0}°")
                }
            },
        ),
        switch_row(
            app,
            "ff",
            "Force feedback and vibration",
            "Enable force feedback for the steering wheel and vibration for controllers",
        ),
        switch_row(
            app,
            "ff_invert",
            "Invert force feedback by default",
            "For wheels without a saved direction",
        ),
    ]
        .into_iter()
        .flatten()
        .collect();
    let mut camera: Vec<(String, String)> = vec![
        slider_row(
            app,
            "fov",
            "Field of view",
            "The view angle of the views from the vehicle",
            &|v| {
                if v < 20.0 {
                    "Default".to_string()
                } else {
                    format!("{v:.0}°")
                }
            },
        ),
        switch_row(
            app,
            "head",
            "Head movement",
            "The view moves with the vehicle's acceleration",
        ),
        switch_row(
            app,
            "cam_smooth",
            "Smooth viewpoint changes",
            "Enables a smooth transition between camera perspectives",
        ),
        switch_row(
            app,
            "camcoll",
            "Camera collisions",
            "The outside camera cannot pass through objects",
        ),
        slider_row(
            app,
            "look_sens",
            "Mouse look sensitivity",
            "How fast the view turns when looking round with the mouse (100% is OMSI's)",
            &pct,
        ),
        switch_row(
            app,
            "alt_view",
            "Right mouse button turns the view",
            "Shift+right zooms; off: right zooms as in OMSI, the wheel button turns",
        ),
        toggle_now(app, "free_look").map(|on| {
            (
                row(
                    "Free look",
                    'm',
                    if on { "on" } else { "off" },
                    "Here you can configure the free look, the crosshair and the button names",
                    None,
                ),
                "lookopts".to_string(),
            )
        }),
        switch_row(
            app,
            "steer_look",
            "View turns with steering",
            "Camera turns with the steering wheel (cockpit only)",
        ),
        slider_row(
            app,
            "steer_look_angle",
            "Steering view angle",
            "How far the view turns at full steering lock",
            &|v| format!("{v:.0}°"),
        ),
        slider_row(
            app,
            "steer_look_response",
            "Steering view response",
            "How quickly the view follows the steering",
            &|v| format!("{:.0} ms", v * 1000.0),
        ),
        switch_row(
            app,
            "headtrack",
            "Head tracking",
            &format!(
                "Head tracking with opentrack (UDP port {})",
                ::config::get_int("camera", "head_tracking_port").and_then(|v| u16::try_from(v).ok()).unwrap_or(4242)
            ),
        ),
        switch_row(
            app,
            "hands_in_cab",
            "Driver's hands in the cab view",
            "Shows the driver's hand on the steering wheel (Cockpit only)",
        ),
        switch_row(
            app,
            "driver",
            "Driver at the wheel (outside views)",
            "Shows the driver in the outside views and in the mirrors",
        ),
        slider_row(
            app,
            "seat 1",
            "Seat forward and back",
            "Adjust the driver's seat position forward or backward",
            &cm,
        ),
        slider_row(
            app,
            "seat 2",
            "Seat height",
            "Adjust the driver's seat height",
            &cm,
        ),
        slider_row(
            app,
            "seat 0",
            "Seat left and right",
            "Adjust the driver's seat position from side to side",
            &cm,
        ),
    ]
        .into_iter()
        .flatten()
        .collect();
    camera.push(button(
        "Reset the seat position",
        "Reset",
        "Put the seat back where the vehicle has it.",
        "seat_reset",
    ));
    let graphics: Vec<(String, String)> = vec![
        preset_row(
            "Quality preset",
            "Sets most of the graphics options at once",
        ),
        (!::config::get_subs("graphics_profiles").is_empty()).then(|| {
            opens(
                "Load graphics profile",
                "Applies a graphics profile saved in the launcher",
                "gfxprofile",
            )
        }),
        pick("graphics", "Graphics", later),
        pick("msaa", "Anti-aliasing", later),
        pick("render_scale", "Render scale", later),
        pick("anisotropy", "Anisotropic", later),
        switch_row(app, "shadows", "Sun shadows", "Enables/Disabled shadows"),
        pick("shadow_size", "Shadow map", later),
        pick("shadow_casters", "Shadows cast by", later),
        switch_row(app, "ssao", "Ambient occlusion", later),
        switch_row(
            app,
            "reflections",
            "Reflection maps (paint, chrome, glass)",
            later,
        ),
        switch_row(app, "clouds", "Clouds", later),
        switch_row(
            app,
            "detail_textures",
            "Detail texturing up close",
            "The ground and large walls get fine grain when close",
        ),
        pick("map_detail", "Map complexity", later),
        pick("view_distance", "View distance", later),
        pick("max_obj_dist", "Object distance", later),
        pick("min_obj_size", "Small objects", later),
        pick("mirror_size", "Mirrors", later),
        pick("texture_memory", "Texture memory", later),
        switch_row(
            app,
            "texture_compression",
            "Compress textures on loading",
            later,
        ),
        slider_row(
            app,
            "led_glow",
            "LED glow",
            "How strongly the dots of LED destination displays glow",
            &|v| format!("{}/15", v as i64),
        ),
        slider_row(
            app,
            "nightmap_glow",
            "Night map glow",
            "How strongly lit buttons, lamps and windows glow at night",
            &|v| format!("{}/15", v as i64),
        ),
        slider_row(
            app,
            "atmosphere_brightness",
            "Atmosphere brightness",
            "How much light the night has",
            &|v| format!("{v:.2}"),
        ),
        slider_row(
            app,
            "led_mips",
            "LED mask mipmaps",
            "Keep the mip chain of the LED masks (smoother from a distance).",
            &|v| format!("{v:.2}"),
        ),
    ]
        .into_iter()
        .flatten()
        .collect();
    let display: Vec<(String, String)> = vec![
        switch_row(
            app,
            "fullscreen",
            "Fullscreen",
            "Switches the window between windowed and fullscreen",
        ),
        switch_row(app, "vsync", "V-sync", "Waits for the screen's refresh"),
        pick("max_fps", "Frame limit", "Frames a second at most"),
        switch_row(
            app,
            "fps",
            "Frame rate",
            "Show the frames per second in the top right corner",
        ),
    ]
        .into_iter()
        .flatten()
        .collect();
    let sound: Vec<(String, String)> = vec![
        slider_row(
            app,
            "volume",
            "Volume",
            "Set how loud the game should be",
            &pct,
        ),
        slider_row(
            app,
            "vol_ai",
            "Traffic",
            "How loud the other vehicles are",
            &pct,
        ),
        slider_row(
            app,
            "vol_scenery",
            "Surroundings",
            "How loud the sounds of the scenery are",
            &pct,
        ),
        switch_row(
            app,
            "doppler",
            "Doppler effect",
            "Approaching sounds higher, receding ones lower",
        ),
        pick("pax_voices", "Passenger voices", "What passengers say"),
    ]
        .into_iter()
        .flatten()
        .collect();
    let interface: Vec<(String, String)> = vec![
        pick("language", "Language", "The language of the game's interface"),
        pick("units", "Units", "Shows speed, distance and temperature in metric or imperial units"),
        slider_row(app, "ui_scale", "Game interface size", "The size of the texts, the menu, the timetable and the navigator", &pct),
        switch_row(app, "ui_scale_window", "Interface grows with the window", "On a window taller than 1080p the interface grows with it"),
        slider_row(app, "ui_opacity", "Interface opacity", "How much of the interface's backgrounds shows", &pct),
        toggle_now(app, "navigator").map(|on| (row("Navigator", 'm', if on { "on" } else { "off" }, "Here you can configure the Navigator settings", None), "mapopts".to_string())),
        switch_row(app, "nav_arrows", "Route arrows (as in OMSI 2)", "Shows OMSI 2's route arrows over the road"),
        switch_row(app, "info_bar", "Information bar", "Displays information such as the time, speed, and other details at the top of the screen"),
        switch_row(app, "timetable_win", "Timetable window", "Displays a list of all stops (only when a tour is active)"),
        switch_row(app, "notes", "Notes in the top-left corner", "Why the vehicle does not move, the change due, what a service did"),
        switch_row(app, "tooltips", "Name of the button under the mouse", "Shows the name of what the cursor points at"),
        switch_row(app, "chat", "Chat in online games", "Shows the chat of a LAN session"),
        switch_row(app, "name_tags", "Other players' names above their buses", "Shows the names of the other players"),
        Some(opens("Reset all settings...", "Everything but the language, the key bindings and the game folder goes back to how it came", "reset")),
    ]
        .into_iter()
        .flatten()
        .collect();
    let mut vr: Vec<(String, String)> = Vec::new();
    let vr_on = ::config::get_bool("vr", "enabled").unwrap_or(false);
    if cfg!(windows) {
        vr.extend(
            vec![
                switch_row(app, "vr", "Use OpenXR headset", later),
                if vr_on {
                    pick("vr_scale", "Eye resolution", later)
                } else {
                    None
                },
                if vr_on {
                    pick("vr_head_smoothing_ms", "Head tracking smoothing", later)
                } else {
                    None
                },
                if vr_on {
                    pick("vr_mirror_rate", "Bus mirror refresh", later)
                } else {
                    None
                },
                if vr_on {
                    switch_row(
                        app,
                        "vr_desktop_mirror",
                        "Show headset picture on monitor",
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
        let desc = "Navigator position (this bus)";
        vr.extend(switch_row(app, "navigator", "Navigator", desc));
        vr.push(button(
            "Move and rotate with the mouse...",
            "Open",
            desc,
            "vr_nav_edit",
        ));
        for (id, label) in [
            ("x", "Position right / left"),
            ("y", "Position forward / back"),
            ("z", "Position up / down"),
            ("width", "Display width"),
        ] {
            vr.extend(slider_row(app, &format!("vr_nav_{id}"), label, desc, &cm));
        }
        for (id, label) in [
            ("yaw", "Display rotation"),
            ("tilt", "Display tilt"),
            ("roll", "Display roll"),
        ] {
            vr.extend(slider_row(
                app,
                &format!("vr_nav_{id}"),
                label,
                desc,
                &|v| format!("{v:.0}°"),
            ));
        }
        vr.extend(slider_row(
            app,
            "vr_nav_opacity",
            "Interface opacity",
            desc,
            &pct,
        ));
        vr.push(button(
            "Reset navigator position",
            "Reset",
            desc,
            "vr_nav_reset",
        ));
    }
    vec![
        ("Gameplay", game),
        ("Driving", driving),
        ("Controls", controls),
        ("Camera", camera),
        ("Graphics", graphics),
        ("Display", display),
        ("Sound", sound),
        ("Interface", interface),
        ("VR", vr),
    ]
}

pub(super) fn key_rows(app: &App) -> Vec<(String, String)> {
    let Ok(v) = omsi_launcher_lib::get_keybindings() else {
        return vec![(
            row("The key bindings could not be read", 'i', "", "", None),
            "noop".to_string(),
        )];
    };
    let names = crate::describe::names(&app.args.root, &::config::get_string("ui", "language").unwrap_or_else(|| "en".into()));
    let head = |t: &str, n: usize| {
        (
            row(&t.to_uppercase(), 'i', &n.to_string(), "", None),
            HEADING.to_string(),
        )
    };
    let q = app.key_filter.trim().to_lowercase();
    let mut out = vec![(
        row(
            "Find a key binding",
            if app.key_search { 'E' } else { 'a' },
            &app.key_filter,
            "",
            None,
        ),
        "keysearch".to_string(),
    )];
    let mut all: Vec<(usize, usize, String, i64, i64)> = Vec::new();
    for (sec, key) in ["vehicles", "game"].iter().enumerate() {
        if let Some(a) = v.get(*key).and_then(|a| a.as_array()) {
            for (i, b) in a.iter().enumerate() {
                let action = b
                    .get("action")
                    .and_then(|x| x.as_str())
                    .unwrap_or("")
                    .to_string();
                if !action.is_empty() {
                    all.push((
                        sec,
                        i,
                        action,
                        b.get("scan_code").and_then(|x| x.as_i64()).unwrap_or(0),
                        b.get("modifier").and_then(|x| x.as_i64()).unwrap_or(0),
                    ));
                }
            }
        }
    }
    let groups: [(&str, Box<dyn Fn(&(usize, usize, String, i64, i64)) -> bool>); 3] = [
        ("Driving and the bus", Box::new(|b| b.0 == 0)),
        (
            "The game",
            Box::new(|b| b.0 == 1 && !b.2.starts_with("vr_")),
        ),
        (
            "Headset (VR)",
            Box::new(|b| b.0 == 1 && b.2.starts_with("vr_")),
        ),
    ];
    let mut any = false;
    for (title, pick) in groups.iter() {
        let members: Vec<&(usize, usize, String, i64, i64)> = all
            .iter()
            .filter(|b| pick(b))
            .filter(|b| {
                q.is_empty()
                    || app.key_capture == Some((b.0, b.1))
                    || names.control(&b.2).to_lowercase().contains(&q)
                    || b.2.to_lowercase().contains(&q)
                    || crate::keys::key_name(b.3, b.4).to_lowercase().contains(&q)
            })
            .collect();
        if members.is_empty() {
            continue;
        }
        any = true;
        out.push(head(title, members.len()));
        for b in members {
            let (sec, i, action, scan, m) = (b.0, b.1, &b.2, b.3, b.4);
            let other = (scan != 0)
                .then(|| {
                    all.iter()
                        .find(|o| o.0 == sec && o.1 != i && o.3 == scan && (o.4 & 6) == (m & 6))
                })
                .flatten();
            let label = names.control(action);
            if app.key_capture == Some((sec, i)) {
                out.push((
                    row(
                        &label,
                        'E',
                        "press a key...",
                        "Escape leaves it as it is",
                        None,
                    ),
                    format!("keybind {sec} {i} {action}"),
                ));
                continue;
            }
            let value = if scan == 0 {
                "Not set".to_string()
            } else {
                crate::keys::key_name(scan, m)
            };
            let desc = other
                .map(|o| format!("Same key as: {}", names.control(&o.2)))
                .unwrap_or_default();
            out.push((
                row(&label, 'k', &value, &desc, None),
                format!("keybind {sec} {i} {action}"),
            ));
        }
    }
    if !any {
        out.push((
            row("Nothing matches", 'i', "", "Try another name or key", None),
            HEADING.to_string(),
        ));
    }
    out
}

pub(super) fn vehicle_pages(app: &App) -> Vec<Page> {
    let has = app.player.is_some();
    let server = crate::input_script::on_server(&app.args);
    let mut display: Vec<(String, String)> = Vec::new();
    if has {
        display.push(opens(
            "Destination display",
            "Change the current destination",
            "dest",
        ));
        display.push(opens(
            "Depot file (HOF)",
            "Change the current depot file (used for the timetable)",
            "hof",
        ));
        display.push(opens(
            "Fleet number",
            "Change the vehicle's current fleet number",
            "number",
        ));
    }
    let mut fleet: Vec<(String, String)> = Vec::new();
    if has || !app.placed.is_empty() {
        fleet.push(button(
            "Drive the next vehicle",
            "Switch",
            "Take the wheel of another vehicle standing in the world",
            "switch",
        ));
    }
    fleet.push(opens(
        "Place a vehicle",
        "Place a vehicle of your choice",
        "place",
    ));
    if has {
        // (#728: another bus in this one's place, or this one again with its files read
        // anew - a script or a .bus changed - without starting the game again)
        fleet.push(button(
            "Swap for another vehicle",
            "Swap",
            "Put another vehicle in this one's place and drive it",
            "swap",
        ));
        fleet.push(button(
            "Couple",
            "Couple",
            "Couple the vehicle to the one in front of or behind it",
            "couple",
        ));
        fleet.push(button(
            "Uncouple",
            "Uncouple",
            "Separate the coupled vehicles",
            "uncouple",
        ));
        fleet.push(button(
            "Remove this vehicle",
            "Remove",
            "Removes the current vehicle",
            "remove",
        ));
    }
    if !app.placed.is_empty() {
        fleet.push(button(
            "Remove the placed vehicles",
            "Remove",
            "Removes all vehicles you've placed from the world",
            "clearplaced",
        ));
    }
    let mut service: Vec<(String, String)> = Vec::new();
    if has {
        service.push(button(
            "Refuel",
            "Refuel",
            "Fills the tank of the current vehicle",
            "refuel",
        ));
        service.push(button("Wash", "Wash", "Cleans the current vehicle", "wash"));
        service.push(button(
            "Repair",
            "Repair",
            "Repairs the current vehicle",
            "repair",
        ));
        service.push(button(
            "Put back on its wheels",
            "Reset",
            "Return the vehicle to an upright position",
            "reset_vehicle",
        ));
        service.push(button("Reload this vehicle", "Reload", "Read the vehicle's files again (.bus, model and sound configuration, scripts) and drive it from here", "reload"));
    }
    let mut driver: Vec<(String, String)> = Vec::new();
    if !server {
        driver.push(opens(
            "Driver",
            "Change the current driver profile",
            "driver",
        ));
    }
    if has && app.on_foot.is_none() {
        driver.push(button(
            "Get up and out",
            "Get out",
            "Step out of your car and explore the world",
            "getout",
        ));
    }
    let mut teleport: Vec<(String, String)> = Vec::new();
    if has && !server && app.navigator.is_some() {
        teleport.push(button(
            "Move on the map",
            "Pick",
            "Teleports you to any location on the map",
            "teleport",
        ));
        teleport.push(opens(
            "Teleport to a start point",
            "Teleport to a starting point on the map",
            "tplist",
        ));
    }
    vec![
        ("Vehicles", fleet),
        ("Display", display),
        ("Service", service),
        ("Driver", driver),
        ("Teleport", teleport),
    ]
}

pub(super) fn world_pages(app: &App) -> Vec<Page> {
    let client = app
        .lan
        .as_ref()
        .is_some_and(|l| l.role == ::network::Role::Client);
    let pct = |v: f32| format!("{:.0} %", v * 100.0);
    let mut time: Vec<(String, String)> = Vec::new();
    let mut weather: Vec<(String, String)> = Vec::new();
    let mut climate: Vec<(String, String)> = Vec::new();
    let mut tools: Vec<(String, String)> = Vec::new();
    if !client {
        let t = app.clock.time;
        let now = format!(
            "{:02}:{:02}",
            ((t / 3600.0) as i64).rem_euclid(24),
            ((t / 60.0) as i64) % 60
        );
        time.extend(switch_row(
            app,
            "time_sync",
            "Real-time sync",
            "The game follows your device's date and time",
        ));
        if app.real_time_locked() {
            let (d, m) = app.clock.day_month();
            let text = format!(
                "{:04}-{m:02}-{d:02}  {}:{:02}",
                app.clock.year,
                now,
                (t as i64) % 60
            );
            time.push((
                row(
                    "Date and time",
                    'i',
                    &text,
                    "Synchronized with the real time",
                    None,
                ),
                "noop".to_string(),
            ));
        } else {
            match app.menu_edit.as_ref() {
                Some(d) => {
                    let mut c: Vec<char> = d.chars().collect();
                    c.resize(6, '_');
                    let typed = format!("{}{}:{}{}:{}{}", c[0], c[1], c[2], c[3], c[4], c[5]);
                    time.push((
                        row(
                            "Exact time",
                            'E',
                            &typed,
                            "Press Enter to change, Esc to cancel",
                            None,
                        ),
                        "time_edit".to_string(),
                    ));
                }
                None => {
                    let secs = format!("{}:{:02}", now, (t as i64) % 60);
                    time.push((
                        row(
                            "Exact time",
                            'e',
                            &secs,
                            "Change the current time (Press Enter to change)",
                            None,
                        ),
                        "time_edit".to_string(),
                    ));
                }
            }
            time.extend(slider_row(
                app,
                "hour",
                "Hour",
                "Set the hour of the day directly",
                &|v| format!("{:02}", v as i64),
            ));
            time.extend(slider_row(
                app,
                "minute",
                "Minute",
                "Set the minute directly",
                &|v| format!("{:02}", v as i64),
            ));
            for (name, hm, secs) in [
                ("Morning", "06:00", 6 * 3600),
                ("Noon", "12:00", 12 * 3600),
                ("Evening", "18:00", 18 * 3600),
                ("Night", "23:00", 23 * 3600),
            ] {
                time.push(button(
                    name,
                    hm,
                    "Jump to this time of day.",
                    &format!("clock_set {secs}"),
                ));
            }
            if let (Some(_), Some(p)) = (app.duty.as_ref(), app.player.as_ref()) {
                let d = p.vehicle.host.tt_delay as f64;
                if d.abs() >= 1.0 {
                    let text = format!(
                        "{}{}:{:02}",
                        if d < 0.0 { "−" } else { "+" },
                        (d.abs() / 60.0) as i64,
                        d.abs() as i64 % 60
                    );
                    time.push(button(
                        "On time with the timetable",
                        &text,
                        "Move the clock so that the vehicle is on time",
                        "clock_ontime",
                    ));
                }
            }
            if app.lan.is_none() {
                time.extend(slider_row(
                    app,
                    "speed",
                    "Time speed",
                    "How fast the world's clock runs",
                    &|v| format!("x{v}"),
                ));
            }
        }
        weather.extend(switch_row(
            app,
            "metar_sync",
            "METAR sync",
            "The weather follows the real METAR report",
        ));
        let src = if ::config::get_string("gameplay", "metar_station").unwrap_or_default().is_empty() {
            format!("{} ({})", app.metar_station(), ::user_interface::tr("automatic"))
        } else {
            app.metar_station()
        };
        weather.push((
            row(
                "METAR source",
                'o',
                &src,
                "The airport used for real weather.",
                None,
            ),
            "metar_src".to_string(),
        ));
        let typed = if app.menu_edit_icao {
            let mut s = app.menu_edit.clone().unwrap_or_default();
            while s.len() < 4 {
                s.push('_');
            }
            format!("{s}  (typing)")
        } else {
            app.metar_station()
        };
        weather.push((
            row(
                "ICAO",
                if app.menu_edit_icao { 'E' } else { 'e' },
                &typed,
                "Enter any 4-letter ICAO station.",
                None,
            ),
            "metar_icao_edit".to_string(),
        ));
        if app.metar_locked() {
            weather.push(button(
                "METAR report",
                "Refresh now",
                "Fetch the selected station again without waiting for the next automatic update.",
                "metar_refresh",
            ));
        } else {
            weather.push(button(
                "METAR report",
                "Load once",
                "Load the selected station once without enabling continuous METAR sync.",
                "metar_once",
            ));
        }
        weather.push((
            row(
                "Preset",
                'o',
                &weather_name(app),
                "A ready-made weather",
                None,
            ),
            "weather".to_string(),
        ));
        if !app.metar_locked() {
            weather.push(button(
                "Custom weather",
                "Edit current",
                "Freeze the weather currently in force and edit it as a custom weather.",
                "weather_custom",
            ));
        }
        let cloud = app
            .weather
            .as_ref()
            .and_then(|w| cloud_index(&w.clouds.0))
            .map(|i| CLOUD_TYPES[i].1.to_string())
            .or_else(|| app.weather.as_ref().map(|w| w.clouds.0.trim().to_string()))
            .unwrap_or_default();
        weather.push((
            row(
                "Clouds",
                'o',
                &cloud,
                "The kind of clouds in the sky.",
                None,
            ),
            "cloudkind".to_string(),
        ));
        weather.extend(slider_row(
            app,
            "visibility",
            "Visibility",
            "How far one can see; less is fog.",
            &|v| {
                if v >= 1000.0 {
                    format!("{:.1} km", v / 1000.0)
                } else {
                    format!("{} m", v as i64)
                }
            },
        ));
        weather.extend(slider_row(
            app,
            "brightness",
            "Brightness",
            "Brightness of the custom weather lighting.",
            &|v| format!("{:.0} %", v * 100.0),
        ));
        let kind = app
            .weather
            .as_ref()
            .map(|w| {
                (w.precip.first().copied().unwrap_or(0.0).max(0.0) as usize)
                    .min(PRECIP_KINDS.len() - 1)
            })
            .unwrap_or(0);
        weather.push((
            row(
                "Precipitation",
                'o',
                PRECIP_KINDS[kind],
                "Rain or snow.",
                None,
            ),
            "precipkind".to_string(),
        ));
        weather.extend(slider_row(
            app,
            "rain_amt",
            "Precipitation strength",
            "How hard it rains or snows.",
            &pct,
        ));
        weather.extend(slider_row(
            app,
            "wet",
            "Wet roads",
            "How wet the roads are now (they dry in the sun, wet in the rain).",
            &pct,
        ));
        weather.extend(switch_row(
            app,
            "snow_cover",
            "Snow cover",
            "Snow lying on the world and ground.",
        ));
        weather.extend(switch_row(
            app,
            "snow_road",
            "Snow on road",
            "Treat the road surface as snow-covered.",
        ));
        climate.extend(slider_row(
            app,
            "temp",
            "Temperature",
            "The air temperature.",
            &|v| format!("{} °C", v as i64),
        ));
        let dew_temp = app.weather.as_ref().map(|w| w.temp.0).unwrap_or(15.0);
        climate.extend(slider_row(
            app,
            "humidity",
            "Humidity",
            "Relative humidity of the air.",
            &|v| {
                format!(
                    "{:.0} % · dew {:.0} °C",
                    v,
                    crate::weather_setup::dew_point_c(dew_temp, v)
                )
            },
        ));
        climate.extend(slider_row(
            app,
            "wind_speed",
            "Wind speed",
            "How fast the wind blows; it drives the clouds.",
            &|v| format!("{} m/s", v as i64),
        ));
        climate.extend(slider_row(
            app,
            "wind_dir",
            "Wind direction",
            "The direction of the wind in degrees (0 is north).",
            &|v| format!("{}°", v as i64),
        ));
        // the METAR sync on: only its own rows stay (the weather is the report's)
        if app.metar_locked() {
            weather.retain(|r| {
                matches!(
                    r.1.as_str(),
                    "metar_sync" | "metar_src" | "metar_icao_edit" | "metar_refresh"
                )
            });
            climate.clear();
        }
        tools.push(button(
            "Object editor",
            "Open",
            "Place and move objects in the world.",
            "editor",
        ));
    }
    let mut people: Vec<(String, String)> = Vec::new();
    people.extend(slider_row(
        app,
        "traffic",
        "Traffic",
        "How many vehicles drive around the map.",
        &|v| format!("{} vehicles", v as i64),
    ));
    people.extend(slider_row(
        app,
        "pax",
        "Passengers",
        "How many passengers wait at the stops and ride.",
        &pct,
    ));
    vec![
        ("Time", time),
        ("Weather", weather),
        ("Temperature and wind", climate),
        ("Traffic and people", people),
        ("Tools", tools),
    ]
}

pub(super) fn pages_of(app: &App, kind: &ListKind) -> Option<(Vec<Page>, usize)> {
    let (pages, tab) = match kind {
        ListKind::Options(t) if is_sub_tab(*t) => (options_pages(app), app.map_return_tab),
        ListKind::Options(t) => (options_pages(app), *t),
        ListKind::Vehicle(t) => (vehicle_pages(app), *t),
        ListKind::World(t) => (world_pages(app), *t),
        _ => return None,
    };
    let pages: Vec<Page> = pages.into_iter().filter(|p| !p.1.is_empty()).collect();
    let tab = tab.min(pages.len().saturating_sub(1));
    Some((pages, tab))
}

pub(super) type TitlesCache = Option<(ListKind, bool, std::time::Instant, (Vec<String>, usize))>;

thread_local! {
    static TITLES: std::cell::RefCell<TitlesCache> = const { std::cell::RefCell::new(None) };
}

pub(crate) fn forget_page_titles() {
    TITLES.with(|c| *c.borrow_mut() = None);
    *DRIVER_SCAN.lock().unwrap_or_else(|e| e.into_inner()) = None;
    *WEATHER_SCAN.lock().unwrap_or_else(|e| e.into_inner()) = None;
}

pub(crate) fn page_titles(app: &App, kind: &ListKind) -> Option<(Vec<String>, usize)> {
    let vr_nav_available = app.vr_active() && app.player.is_some();
    if let Some(hit) = TITLES.with(|c| {
        c.borrow()
            .as_ref()
            .filter(|(k, vr, t, _)| {
                k == kind && *vr == vr_nav_available && t.elapsed().as_millis() < 5000
            })
            .map(|(_, _, _, r)| r.clone())
    }) {
        return Some(hit);
    }
    let (pages, tab) = pages_of(app, kind)?;
    let r = (
        pages.iter().map(|p| p.0.to_string()).collect::<Vec<_>>(),
        tab,
    );
    TITLES.with(|c| {
        *c.borrow_mut() = Some((
            kind.clone(),
            vr_nav_available,
            std::time::Instant::now(),
            r.clone(),
        ))
    });
    Some(r)
}