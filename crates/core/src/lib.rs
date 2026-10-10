mod admin;
mod ambience;
#[cfg(target_os = "android")]
mod android;
mod camera_arm;
mod career;
mod describe;
#[cfg(all(feature = "devtools", debug_assertions))]
#[path = "dev-tools/mod.rs"]
mod devtools;
mod discord;
mod driver;
mod editor;
mod export;
mod game_lists;
mod headtrack;
mod hud;
mod humans;
mod keys;
mod lan;
mod lan_world;
mod launcher;
mod lights;
#[cfg(target_os = "macos")]
mod mac_hid;
mod menu;
mod money;
mod navigator;
#[cfg(windows)]
mod openxr;
mod placing;
mod platform;
mod radio;
mod rail_drive;
mod touch;
mod updater;
mod vr_navigator;

mod puddles;
mod quit;
mod rain;
mod real_time;
mod run_statistics;
mod scene;
mod schedule;
mod schedule_paper;
mod sound_events;
mod threads;
mod tiles;
mod traffic;
mod traffic_runtime;
mod ui;
mod window_drops;
mod window_wipers;

mod app;
mod app_events;
mod applog;
mod bus_service;
mod camera_tool;
mod camera_util;
mod cli;
mod control;
mod controllers;
#[cfg(windows)]
mod dinput;
mod duty_start;
mod editor_ctl;
#[cfg(all(target_os = "linux", target_pointer_width = "64"))]
mod evdev_ff;
mod game_link;
mod game_menu;
mod lab_menu;
mod lab_options;
mod lab_pads;
mod input_keys;
mod input_mouse;
mod input_script;
mod lan_mods;
mod memory;
mod offscreen;
mod on_foot;
mod pax_pack;
mod player;
mod plugins;
mod route_arrows;
mod server;
mod services;
mod session;
mod situation;
mod spawn;
mod startup;
mod stock_keys;
mod traffic_link;
mod tutorial;
mod weather_cycle;
mod weather_setup;
mod world_ctl;
mod world_load;

pub(crate) fn ui_language(code: &str) {
    let iso = omsi_launcher_lib::language_iso(code);
    i18n::set_language(iso);
    simulation::vehicle_api::set_locale(iso);
}

use anyhow::{Context, Result, anyhow};
use app::*;
use camera_util::*;
use clap::Parser;
use cli::*;
use duty_start::*;
use glam::{DVec3, Vec3};
use input_script::*;
use memory::*;
use offscreen::*;
use ::render::{Camera, Renderer, Scene, SurfaceState};
use player::*;
use scene::World;
use services::*;
use situation::*;
use spawn::*;
use startup::*;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::Instant;
use traffic_link::*;
use weather_setup::*;
use winit::application::ApplicationHandler;
use winit::event::{DeviceEvent, ElementState, WindowEvent};
use winit::event_loop::{ActiveEventLoop, EventLoop};
use winit::keyboard::{KeyCode, PhysicalKey};
use winit::window::{Window, WindowId};
use world_load::*;

pub fn run() -> Result<()> {
    #[cfg(target_os = "macos")]
    restart_with_allocator_settings();
    let protocol = std::env::args().any(|a| a == "--control-protocol");
    // a console would take over the launcher's pipes
    #[cfg(windows)]
    if !protocol {
        attach_parent_console();
    }
    let args = Args::parse();
    let bare = std::env::args().len() == 1;
    logging::init(if protocol {
        "control"
    } else if args.launcher || (bare && !args.menu) {
        "launcher"
    } else {
        "game"
    });
    let default_hook = std::panic::take_hook();
    std::panic::set_hook(Box::new(move |info| {
        if render::catching() {
            log::warn!("caught by the renderer: {info}");
            return;
        }
        logging::crash(
            None,
            &format!(
                "the game stopped on an error (build {BUILD}): {info}\n{}",
                std::backtrace::Backtrace::force_capture()
            ),
        );
        // (other threads' panics are often caught: a damaged tile, a plugin)
        if std::thread::current().name() == Some("main") {
            game_link::failed(&format!("the game stopped on an error: {info}"));
        }
        default_hook(info);
    }));
    if let Err(e) = config::init(config::default_path()) {
        log::warn!("settings not loaded: {e}");
    }
    if protocol {
        return control::run();
    }
    log::info!(
        "neoOMSI {VERSION}, build {BUILD}{}",
        if std::env::var_os("MallocLargeCache").is_some() {
            " (large allocations returned at once)"
        } else {
            ""
        }
    );
    let Some((args, server_cfg)) = prepare(args, bare)? else {
        return Ok(());
    };
    if args.launcher || (bare && !args.menu) {
        // `--launcher` always means the built-in one
        if !args.launcher {
            match omsi_launcher_lib::start_external_launcher(&std::env::current_exe()?) {
                Ok(true) => return Ok(()),
                Ok(false) => {}
                Err(e) => log::warn!("{e:#}: the built-in launcher opens instead"),
            }
        }
        launcher_statics();
        return launcher::run(graphics_instance());
    }
    if server_cfg.is_none() {
        game_link::connect();
    }
    let Some(app) = make_app(args, server_cfg)? else {
        return Ok(());
    };
    let event_loop = EventLoop::new()?;
    let proxy = event_loop.create_proxy();
    quit::install(move |_| {
        let _ = proxy.send_event(());
    });
    let mut app = app;
    let r = event_loop.run_app(&mut app);
    lan_mods::clean_up();
    r?;
    Ok(())
}

pub(crate) fn launcher_statics() {
    ENHANCED.store(
        (config::get_string("graphics", "graphics").as_deref() == Some("enhanced")) || legacy_config::env::var_os("OMSI_ENHANCED").is_some(),
        std::sync::atomic::Ordering::Relaxed,
    );
    CLASSIC.store(config::get_string("graphics", "graphics").as_deref() == Some("vanilla"), std::sync::atomic::Ordering::Relaxed);
    CLOUDS.store(
        config::get_bool("graphics", "clouds").unwrap_or(true) && legacy_config::env::var_os("OMSI_NO_CLOUDS").is_none(),
        std::sync::atomic::Ordering::Relaxed,
    );
}

pub(crate) fn prepare(
    mut args: Args,
    bare: bool,
) -> Result<Option<(Args, Option<server::ServerCfg>)>> {
    ui_language(&config::get_string("ui", "language").unwrap_or_else(|| "ENG".into()));
    let server_cfg = match args.server.clone() {
        Some(p) => match server::prepare(&mut args, &p) {
            Ok(c) => Some(c),
            Err(e) => {
                eprintln!("server: {e:#}");
                std::process::exit(2);
            }
        },
        None => None,
    };
    let seed = legacy_config::env::var("OMSI_SEED")
        .ok()
        .and_then(|s| s.trim().parse::<u64>().ok())
        .unwrap_or_else(|| {
            let t = std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map(|d| d.as_nanos() as u64)
                .unwrap_or(1);
            (t ^ ((std::process::id() as u64) << 32)) % 1_000_000_000
        });
    legacy_script::set_session_seed(seed);
    log::info!("script random seed {seed} (OMSI_SEED={seed} repeats it)");
    let launcher_mode = args.launcher || (bare && !args.menu);
    if !is_omsi_root(&args.root) {
        match find_root() {
            Some(p) => {
                log::info!("OMSI 2 found at {}", p.display());
                args.root = p;
            }
            None if launcher_mode => {
                log::warn!("the original OMSI 2 was not found; the launcher asks for it");
            }
            None => {
                let missing = legacy_config::missing_original_essentials(&args.root);
                let text = format!(
                    "The original OMSI 2 was not found.\n\n\
                     neoOMSI needs a complete installation of the original game (any version). \
                     Choose its folder in the launcher (Setup), or start once with \
                     --root \"/path/to/OMSI 2\", the folder with Omsi.exe, maps and Vehicles in it.\n\n\
                     Missing in {}: {}",
                    args.root.display(),
                    missing.join(", ")
                );
                if server_cfg.is_some() {
                    eprintln!("{text}");
                } else {
                    fatal_dialog("neoOMSI cannot start", &text);
                }
                if cfg!(target_os = "android") {
                    return Ok(None);
                }
                std::process::exit(1);
            }
        }
    }
    if let Some(memo) = root_memo().filter(|_| is_omsi_root(&args.root)) {
        let _ = std::fs::write(memo, args.root.to_string_lossy().as_bytes());
    }
    if let Some(c) = content_dir() {
        match legacy_config::ensure_content_layout(&c) {
            Ok(()) => {
                legacy_config::add_content_root(c.clone());
                log::info!("content folder (mods): {}", c.display());
            }
            Err(e) => log::warn!("content folder {}: {e}", c.display()),
        }
    }
    if config::get_string("passengers", "models").unwrap_or_else(|| "omsi".into()) == "realistic" {
        if let Some(content) = content_dir() {
            let pack = pax_pack::folder(&content);
            if pack.join("Humans").is_dir() {
                legacy_config::add_content_root(pack.clone());
                log::info!("realistic passengers: {}", pack.display());
            } else {
                log::warn!(
                    "RealisticPax is missing at {}; using installed passengers",
                    pack.display()
                );
            }
            if server_cfg.is_none() && args.offscreen.is_none() {
                pax_pack::fetch_if_needed(|| Some(content));
            }
        }
    }
    for z in &args.content_zip {
        if let Err(e) = legacy_config::add_content_zip(z) {
            log::warn!("content zip {}: {e}", z.display());
        }
    }
    legacy_config::vfs::mount_env_zips();
    if let Some(c) = content_dir() {
        legacy_config::vfs::mount_dir_zips(&c.join("Archives"));
    }
    legacy_config::add_content_root(args.root.clone());
    Ok(Some((args, server_cfg)))
}

pub(crate) fn make_app(
    mut args: Args,
    server_cfg: Option<server::ServerCfg>,
) -> Result<Option<App>> {
    let _lan_status = lan::StatusFileGuard;
    if let Some(n) = args.tutorial.filter(|n| (1..=4).contains(n)) {
        args.situation = Some(tutorial::SITUATIONS[n - 1].to_string());
    }
    apply_situation(&mut args)?;
    if let Some(t) = args
        .lan_join
        .clone()
        .filter(|t| network::official::is_alias(t))
    {
        match network::official::resolve_target(&t) {
            Ok(url) => {
                log::info!("LAN: the official server is at {url}");
                args.lan_join = Some(url);
            }
            Err(e) => log::warn!("LAN: {e}"),
        }
    }
    if config::get_bool("gameplay", "time_sync").unwrap_or(false)
        && args.lan_join.is_none()
        && args.server.is_none()
        && args.offscreen.is_none()
    {
        real_time::start_at_now(&mut args);
    }
    if args.export_glb.is_none() && args.lan_join.is_none() {
        place_on_duty(&mut args);
    }
    applog::log_system();
    ENHANCED.store(
        (config::get_string("graphics", "graphics").as_deref() == Some("enhanced")) || args.enhanced || legacy_config::env::var_os("OMSI_ENHANCED").is_some(),
        std::sync::atomic::Ordering::Relaxed,
    );
    CLOUDS.store(
        config::get_bool("graphics", "clouds").unwrap_or(true) && legacy_config::env::var_os("OMSI_NO_CLOUDS").is_none(),
        std::sync::atomic::Ordering::Relaxed,
    );
    SOUND_AI.store(
        (config::get_float("audio", "ai-volume").unwrap_or(1.0) as f32).to_bits(),
        std::sync::atomic::Ordering::Relaxed,
    );
    SOUND_SCENERY.store(
        (config::get_float("audio", "scenery-volume").unwrap_or(1.0) as f32).to_bits(),
        std::sync::atomic::Ordering::Relaxed,
    );
    MIRROR_SIZE.store(config::get_int("graphics", "mirror_size").unwrap_or(256) as u32, std::sync::atomic::Ordering::Relaxed);
    audio::DOPPLER.store(config::get_bool("audio", "doppler").unwrap_or(true), std::sync::atomic::Ordering::Relaxed);
    CLASSIC.store(
        (config::get_string("graphics", "graphics").as_deref() == Some("vanilla")) && !ENHANCED.load(std::sync::atomic::Ordering::Relaxed),
        std::sync::atomic::Ordering::Relaxed,
    );
    let mut lan = if args.export_glb.is_none() {
        lan::start(&args)
    } else {
        None
    };
    lan_mods::remove_stale();
    if let Some(l) = lan.as_mut() {
        lan::share_mods(&mut args, l);
        lan::take_host_map(&mut args, l);
    }
    if let (Some(cfg), Some(l)) = (server_cfg.as_ref(), lan.as_ref()) {
        lan::open_public_gateway(l, server::info_of(cfg), cfg.web_port, cfg.tunnel);
        lan::publish_vehicles(args.root.clone(), cfg.vehicles.clone());
        if cfg.tunnel {
            std::thread::spawn(|| {
                for _ in 0..300 {
                    if let Some(u) = lan::tunnel_url() {
                        println!(
                            "\n  Server address for the players: {u}\n  (Multiplayer -> Servers -> Add)\n"
                        );
                        return;
                    }
                    std::thread::sleep(std::time::Duration::from_millis(200));
                }
                println!(
                    "  No tunnel address (is cloudflared installed?): players join at this machine's address and the UDP port"
                );
            });
        }
    }
    if let (Some(l), None) = (lan.as_mut(), server_cfg.as_ref()) {
        if l.role == network::Role::Host {
            l.clock_speed = if config::get_bool("gameplay", "time_sync").unwrap_or(false) {
                1.0
            } else {
                config::get_float("gameplay", "time_speed").unwrap_or(1.0).clamp(1.0, 30.0)
            };
        }
    }
    let mut lan_game = lan::LanGame::default();
    if args.export_glb.is_none() && args.lan_join.is_some() {
        if let Some(t) = lan.as_ref().and_then(lan::host_time_now) {
            args.time = t;
        }
        place_on_duty(&mut args);
    }
    if let Some(out) = args.export_glb.clone() {
        return run_export(&args, &out).map(|_| None);
    }
    if let Some(out) = args.offscreen.clone() {
        if let Some(l) = lan.as_mut() {
            lan::adopt_host_world(&mut args, l, &mut lan_game);
        }
        let r = run_offscreen(&args, &out, lan, lan_game);
        lan_mods::clean_up();
        return r.map(|_| None);
    }
    let view = args.view.clone();
    let args_root_for_keys = args.root.clone();
    let clock_note = args.clock_moved.clone();
    let vehicle_scan = Some(lab_menu::scan_vehicles(args.root.clone(), args.map.clone()));
    let mut app = App {
        args,
        instance: graphics_instance(),
        window: None,
        surface: None,
        renderer: None,
        resize_pending: None,
        pending_triple_screen_span: None,
        #[cfg(windows)]
        vr: None,
        scene: None,
        camera: None,
        player: None,
        placed: Vec::new(),
        chooser: None,
        editor: None,
        vehicle_list: Vec::new(),
        dropdown: None,
        vehicle_meta: std::collections::HashMap::new(),
        world: None,
        streamer: None,
        starting: None,
        traffic: None,
        schedule: None,
        humans: None,
        duty: None,
        last_report: None,
        report_view: None,
        report_pending: false,
        report_status: String::new(),
        report_save_rx: None,
        duty_places: false,
        hud: None,
        navigator: None,
        vr_nav_profiles: vr_navigator::Profiles::load(),
        vr_nav_edit: None,
        ui: ui::Ui::new(),
        fps: 0.0,
        rain: rain::Rain::new(),
        splashes: puddles::Splashes::new(),
        lamps_on: None,
        menu: None,
        populate_t: 0.0,
        humans_populate_t: 0.0,
        radio: radio::Radio::load(&args_root_for_keys),
        profile: Default::default(),
        profile_prev: Default::default(),
        first_populate: true,
        envir: None,
        weather: None,
        clock: simulation::SimClock::default(),
        started: Instant::now(),
        total_frames: 0,
        mirror_budget: 1.0,
        mirrors_seen: 2,
        mirror_turn: 0,
        frozen_mirrors: None,
        hover_key: None,
        view,
        audio: None,
        ambience: None,
        cursor: (0.0, 0.0),
        vr_cursor_physical: None,
        vr_cursor_warp_pending: None,
        window_focused: false,
        keys: Default::default(),
        door_key_triggers: Default::default(),
        last: Instant::now(),
        speed: 30.0,
        mouse_look: false,
        buttons_held: (false, false),
        both_drag: None,
        vr_zoom_active: false,
        hover: None,
        hover_part: None,
        hover_hand: false,
        input_script: parse_input_script(),
        shot: None,
        screenshot_mode: None,
        paused: false,
        sim_accum: 0.0,
        game_menu: None,
        lab_menu: None,
        lab_map_direct: false,
        restart_pending: false,
        restart_prompt: None,
        lab_list: None,
        lab_load: None,
        lab_place: None,
        lab_room: None,
        lab_pic: None,
        vehicle_scan,
        menu_top: None,
        menu_scroll_drag: false,
        lab_bar_drag: 0,
        pane_scroll: None,
        plugin_keys: Vec::new(),
        clock_hold: 0.0,
        pad_look: [false; 4],
        teleport_pick: false,
        discord: None,
        discord_next_update: Instant::now(),
        headtrack: None,
        headtrack_failed: None,
        controllers: None,
        mouse_drive: ::config::get_bool("controls", "mouse_steering").unwrap_or(false),
        mouse_steer: (0.0, 0.0),
        mouse_edge: 0.0,
        steer_cursor: None,
        center_cursor: false,
        cursor_hidden: None,
        cursor_idle: 0.0,
        free_look: false,
        raycast_applied: false,
        cursor_idle_pos: (0.0, 0.0),
        last_ctl_steer: None,
        mouse_pedals: (0.0, 0.0),
        mouse_kmh: 0.0,
        tutorial: None,
        ego: false,
        on_foot: None,
        remote_walkers: Vec::new(),
        in_cab: false,
        inside_remote: None,
        is_admin: false,
        safe_pose: None,
        safe_age: 0.0,
        wheel_acc: 0.0,
        editor_drag: false,
        editor_sync_t: 0.0,
        remote_added: Default::default(),
        placing: None,
        admin_list: None,
        list_kind: None,
        map_return_tab: 0,
        key_capture: None,
        key_filter: String::new(),
        key_search: false,
        route_arrows: Default::default(),
        game_keys: content::KeyboardCfg::load(&keyboard_cfg(&args_root_for_keys))
            .unwrap_or_default()
            .with_game_defaults()
            .with_vr_defaults()
            .game,
        own_keys: own_keys(&args_root_for_keys),
        own_shift: own_bindings(&args_root_for_keys, content::input::KEY_SHIFT),
        menu_prev_pause: false,
        info_bar: false,
        pending_time: None,
        world_day: None,
        autosave_t: 0.0,
        timetable: false,
        dragging: false,
        html_pressed: None,
        placed_grab: None,
        html_object_pressed: None,
        drag_delta: (0.0, 0.0),
        look: (0.0, 0.0),
        view_looks: Default::default(),
        look_view: String::new(),
        cam_blend: Default::default(),
        view_zoom: Default::default(),
        orbit: ORBIT_DEFAULT,
        frames: 0,
        fps_t: Instant::now(),
        service_msg: clock_note.map(|m| (m, 10.0)),
        log_state: Default::default(),
        plugins: None,
        career: Default::default(),
        wetness: 0.0,
        cloud_drift: [0.0; 2],
        menu_edit: None,
        menu_edit_icao: false,
        swap_pending: false,
        menu_drag: None,
        menu_kbd: true,
        weather_blend: None,
        weather_cycle: None,
        metar_rx: None,
        metar_once: false,
        metar_next: 0.0,
        cursor_kind: 0,
        lan: None,
        remotes: Default::default(),
        spikes: 0,
        worst_ms: 0.0,
        governor: (0.0, 0, 0.0),
        governor_low: 0,
        governor_wait_prev: 0.0,
        hidden_frames: 0,
        exiting: false,
        stand_in: None,
        #[cfg(all(feature = "devtools", debug_assertions))]
        devtools: None,
        cpu_mark: None,
        touch: touch::Touch::new(),
    };
    app.lan = lan;
    app.remotes = lan_game;
    std::mem::forget(_lan_status);
    Ok(Some(app))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn streaming_starts_around_the_bus() {
        let cam = Camera {
            position: DVec3::new(5000.0, 6000.0, 50.0),
            yaw: 0.0,
            pitch: 0.0,
            roll: 0.0,
            fov_deg: 60.0,
            near: 0.5,
            far: 100.0,
        };
        let args = |extra: &[&str]| Args::parse_from(["omsi"].iter().chain(extra.iter()).copied());
        assert_eq!(start_centers(&args(&[]), &cam, None), vec![cam.position]);
        let seat = vec![DVec3::new(10.0, 20.0, 0.0)];
        assert_eq!(
            start_centers(
                &args(&["--bus", "Vehicles/x.bus", "--spawn", "10,20,90"]),
                &cam,
                None
            ),
            seat
        );
        assert_eq!(
            start_centers(
                &args(&[
                    "--bus",
                    "Vehicles/x.bus",
                    "--spawn",
                    "10,20,90",
                    "--cam",
                    "5000,6000,50,0,0"
                ]),
                &cam,
                None
            ),
            seat
        );
        let both = start_centers(
            &args(&[
                "--bus",
                "Vehicles/x.bus",
                "--spawn",
                "10,20,90",
                "--cam",
                "5000,6000,50,0,0",
                "--view",
                "free",
            ]),
            &cam,
            None,
        );
        assert_eq!(both, vec![DVec3::new(10.0, 20.0, 0.0), cam.position]);
    }
}
