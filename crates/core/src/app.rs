use super::*;

const SLOW_UPLOAD_MB_S: f64 = 300.0;

/// The view and pause state to restore when screenshot mode ends.
pub(crate) struct ScreenshotMode {
    pub(crate) view: String,
    pub(crate) ego: bool,
    pub(crate) paused: bool,
    pub(crate) help_left: f32,
}

pub(crate) struct App {
    pub(crate) args: Args,
    pub(crate) instance: wgpu::Instance,
    pub(crate) window: Option<Arc<Window>>,
    pub(crate) surface: Option<SurfaceState<'static>>,
    pub(crate) renderer: Option<Renderer>,
    /// Pending size during a resize drag.
    pub(crate) resize_pending: Option<(Instant, winit::dpi::PhysicalSize<u32>)>,
    #[cfg(windows)]
    pub(crate) vr: Option<openxr::Vr>,
    pub(crate) scene: Option<Scene>,
    pub(crate) camera: Option<Camera>,
    pub(crate) player: Option<Player>,
    pub(crate) placed: Vec<Player>,
    pub(crate) chooser: Option<usize>,
    pub(crate) editor: Option<editor::Editor>,
    pub(crate) vehicle_list: Vec<(String, String)>,
    pub(crate) dropdown: Option<game_lists::Dropdown>,
    pub(crate) vehicle_meta: std::collections::HashMap<String, (String, String)>,
    pub(crate) world: Option<Arc<World>>,
    pub(crate) streamer: Option<tiles::Streamer>,
    pub(crate) world_day: Option<(i32, Option<String>)>,
    pub(crate) starting: Option<Camera>,
    pub(crate) traffic: Option<traffic::Traffic>,
    pub(crate) schedule: Option<schedule::Schedule>,
    pub(crate) humans: Option<humans::Humans>,
    pub(crate) duty: Option<schedule::PlayerDuty>,
    pub(crate) last_report: Option<run_statistics::Report>,
    pub(crate) report_view: Option<run_statistics::Report>,
    pub(crate) report_pending: bool,
    pub(crate) report_status: String,
    pub(crate) report_save_rx:
        Option<std::sync::mpsc::Receiver<std::io::Result<Option<std::path::PathBuf>>>>,
    /// The duty was told the places of the stops beyond the loaded tiles.
    pub(crate) duty_places: bool,
    pub(crate) hud: Option<hud::Hud>,
    pub(crate) navigator: Option<navigator::Navigator>,
    pub(crate) vr_nav_profiles: vr_navigator::Profiles,
    pub(crate) vr_nav_edit: Option<vr_navigator::Editing>,
    pub(crate) ui: Option<ui::Ui>,
    pub(crate) fps: f32,
    pub(crate) rain: rain::Rain,
    pub(crate) splashes: puddles::Splashes,
    pub(crate) lamps_on: Option<bool>,
    pub(crate) menu: Option<menu::Menu>,
    pub(crate) populate_t: f32,
    pub(crate) humans_populate_t: f32,
    pub(crate) radio: radio::Radio,
    pub(crate) profile: std::collections::BTreeMap<&'static str, f64>,
    pub(crate) profile_prev: std::collections::BTreeMap<&'static str, f64>,
    pub(crate) first_populate: bool,
    pub(crate) envir: Option<::content::Envir>,
    pub(crate) weather: Option<::content::weather::Weather>,
    pub(crate) clock: ::simulation::SimClock,
    pub(crate) started: Instant,
    pub(crate) total_frames: u32,
    pub(crate) mirror_budget: f32,
    pub(crate) mirrors_seen: usize,
    pub(crate) mirror_turn: usize,
    pub(crate) frozen_mirrors: Option<FrozenMirrors>,
    pub(crate) hover_key: Option<(i32, i32, i32, i32)>,
    pub(crate) view: String,
    pub(crate) audio: Option<::audio::AudioEngine>,
    pub(crate) ambience: Option<ambience::Ambience>,
    pub(crate) cursor: (f32, f32),
    #[cfg_attr(not(windows), allow(dead_code))]
    pub(crate) vr_cursor_physical: Option<(f32, f32)>,
    #[cfg_attr(not(windows), allow(dead_code))]
    pub(crate) vr_cursor_warp_pending: Option<(f32, f32)>,
    pub(crate) window_focused: bool,
    pub(crate) keys: hashbrown::HashSet<KeyCode>,
    pub(crate) door_key_triggers: hashbrown::HashMap<KeyCode, Vec<String>>,
    pub(crate) last: Instant,
    pub(crate) speed: f32,
    pub(crate) mouse_look: bool,
    pub(crate) buttons_held: (bool, bool),
    pub(crate) both_drag: Option<(f32, f32)>,
    #[cfg_attr(not(windows), allow(dead_code))]
    pub(crate) vr_zoom_active: bool,
    pub(crate) hover: Option<String>,
    pub(crate) hover_part: Option<String>,
    pub(crate) hover_hand: bool,
    pub(crate) input_script: Vec<(f32, String)>,
    pub(crate) shot: Option<PathBuf>,
    pub(crate) screenshot_mode: Option<ScreenshotMode>,
    pub(crate) paused: bool,
    pub(crate) game_menu: Option<usize>,
    pub(crate) quick_menu_open: bool,
    pub(crate) quick_alt_armed: bool,
    pub(crate) quick_confirm_end_duty: bool,
    pub(crate) quick_menu_session: bool,
    pub(crate) menu_top: Option<f32>,
    pub(crate) menu_scroll_drag: bool,
    pub(crate) pane_scroll: Option<(usize, usize)>,
    pub(crate) menu_edit: Option<String>,
    pub(crate) menu_edit_icao: bool,
    pub(crate) swap_pending: bool,
    pub(crate) menu_drag: Option<usize>,
    pub(crate) menu_kbd: bool,
    pub(crate) plugin_keys: Vec<(String, bool)>,
    pub(crate) clock_hold: f32,
    pub(crate) pad_look: [bool; 4],
    pub(crate) teleport_pick: bool,
    pub(crate) discord: Option<discord::Discord>,
    pub(crate) discord_next_update: Instant,
    pub(crate) headtrack: Option<headtrack::HeadTracker>,
    pub(crate) headtrack_failed: Option<Instant>,
    pub(crate) controllers: Option<controllers::Controllers>,
    pub(crate) mouse_drive: bool,
    pub(crate) mouse_steer: (f32, f32),
    pub(crate) mouse_edge: f32,
    pub(crate) steer_cursor: Option<(f32, f32)>,
    pub(crate) center_cursor: bool,
    pub(crate) cursor_hidden: Option<(f32, f32)>,
    /// Seconds the cursor has been still and not shown as a pointer (hidden after 10).
    pub(crate) cursor_idle: f32,
    /// Raycast camera: the free cursor is on (Left Alt) until it idles.
    pub(crate) free_look: bool,
    /// Raycast camera: the window is set up for it (cursor grabbed and hidden).
    pub(crate) raycast_applied: bool,
    pub(crate) cursor_idle_pos: (f32, f32),
    pub(crate) last_ctl_steer: Option<f32>,
    pub(crate) mouse_pedals: (f32, f32),
    pub(crate) mouse_kmh: f32,
    pub(crate) tutorial: Option<tutorial::Tutorial>,
    pub(crate) ego: bool,
    pub(crate) on_foot: Option<on_foot::OnFoot>,
    pub(crate) remote_walkers: Vec<u32>,
    pub(crate) in_cab: bool,
    pub(crate) inside_remote: Option<u32>,
    pub(crate) is_admin: bool,
    pub(crate) safe_pose: Option<(DVec3, f64)>,
    pub(crate) safe_age: f32,
    pub(crate) wheel_acc: f32,
    pub(crate) editor_drag: bool,
    pub(crate) editor_sync_t: f32,
    pub(crate) remote_added: std::collections::HashMap<i64, scene::TileGpu>,
    pub(crate) placing: Option<placing::Placing>,
    pub(crate) admin_list: Option<Vec<(String, String)>>,
    pub(crate) list_kind: Option<game_lists::ListKind>,
    pub(crate) map_return_tab: usize,
    pub(crate) key_capture: Option<(usize, usize)>,
    pub(crate) key_filter: String,
    pub(crate) key_search: bool,
    pub(crate) route_arrows: route_arrows::RouteArrows,
    pub(crate) game_keys: Vec<::content::KeyBinding>,
    pub(crate) own_keys: std::collections::HashSet<i32>,
    pub(crate) own_shift: std::collections::HashSet<i32>,
    pub(crate) menu_prev_pause: bool,
    pub(crate) info_bar: bool,
    pub(crate) pending_time: Option<f64>,
    pub(crate) autosave_t: f64,
    pub(crate) timetable: bool,
    pub(crate) dragging: bool,
    pub(crate) html_pressed: Option<(usize, f32, f32)>,
    pub(crate) placed_grab: Option<usize>,
    pub(crate) html_object_pressed: Option<(i64, usize, f32, f32)>,
    pub(crate) drag_delta: (f32, f32),
    pub(crate) look: (f32, f32),
    pub(crate) view_looks: std::collections::HashMap<String, (f32, f32)>,
    pub(crate) look_view: String,
    pub(crate) cam_blend: CamBlend,
    pub(crate) view_zoom: std::collections::HashMap<String, f32>,
    pub(crate) orbit: f32,
    pub(crate) frames: u32,
    pub(crate) fps_t: Instant,
    pub(crate) service_msg: Option<(String, f32)>,
    pub(crate) log_state: applog::LogState,
    pub(crate) career: career::Career,
    pub(crate) wetness: f32,
    pub(crate) cloud_drift: [f32; 2],
    pub(crate) weather_blend: Option<weather_cycle::Blend>,
    pub(crate) weather_cycle: Option<weather_cycle::Cycle>,
    pub(crate) metar_rx: Option<std::sync::mpsc::Receiver<Option<::content::weather::Weather>>>,
    pub(crate) metar_once: bool,
    pub(crate) metar_next: f64,
    pub(crate) cursor_kind: u8,
    pub(crate) lan: Option<::network::LanSession>,
    pub(crate) remotes: lan::LanGame,
    pub(crate) spikes: u32,
    pub(crate) worst_ms: f32,
    pub(crate) governor: (f32, u32, f32),
    pub(crate) governor_low: u32,
    pub(crate) governor_wait_prev: f64,
    pub(crate) hidden_frames: u32,
    pub(crate) exiting: bool,
    pub(crate) stand_in: Option<wgpu::Texture>,
    #[cfg(all(feature = "devtools", debug_assertions))]
    pub(crate) devtools: Option<devtools::DevTools>,
    pub(crate) cpu_mark: Option<(f64, Instant, u32)>,
    pub(crate) plugins: Option<::legacy_plugin::Plugins>,
    pub(crate) touch: touch::Touch,
}

impl App {
    #[cfg(windows)]
    pub(crate) fn vr_active(&self) -> bool {
        self.vr.is_some()
    }

    #[cfg(not(windows))]
    pub(crate) fn vr_active(&self) -> bool {
        false
    }

    pub(crate) fn resumed_impl(&mut self, event_loop: &ActiveEventLoop) {
        if let Some(window) = self.window.clone() {
            if self.surface.is_none() {
                if let Some(r) = self.renderer.as_ref() {
                    let size = window.inner_size();
                    let vsync = ::config::get_bool("graphics", "vsync").unwrap_or(true) && !self.vr_active();
                    self.surface = SurfaceState::new_with(
                        &self.instance,
                        window.clone(),
                        r,
                        size.width.max(1),
                        size.height.max(1),
                        vsync,
                    )
                        .ok();
                    self.last = Instant::now();
                }
            }
            return;
        }
        self.create_window(event_loop, None);
    }

    pub(crate) fn create_window(
        &mut self,
        event_loop: &ActiveEventLoop,
        given: Option<Arc<Window>>,
    ) {
        let (lw, lh) = self
            .args
            .size
            .split_once('x')
            .map(|(a, b)| {
                (
                    a.parse::<u32>().unwrap_or(1600),
                    b.parse::<u32>().unwrap_or(900),
                )
            })
            .unwrap_or((1600, 900));
        let (fit, at) = fit_window(event_loop, lw as f64, lh as f64);
        let mut attrs = Window::default_attributes()
            .with_title("neoOMSI")
            .with_inner_size(fit)
            .with_visible(false)
            .with_window_icon(window_icon());
        if let Some(at) = at {
            attrs = attrs.with_position(at);
        }
        let window_mode = ::config::get_string("graphics", "window_mode")
            .unwrap_or_else(|| "windowed".into());
        let start_exclusive = window_mode == "fullscreen";
        match window_mode.as_str() {
            "borderless" => {
                attrs = attrs.with_fullscreen(Some(winit::window::Fullscreen::Borderless(None)));
            }
            _ => {}
        }
        if ::legacy_config::env::var_os("OMSI_BACKGROUND").is_some() {
            attrs = attrs.with_active(false);
        }
        let window = match given {
            Some(w) => w,
            None => Arc::new(event_loop.create_window(attrs).expect("window")),
        };
        let mut renderer =
            match window_renderer(
                &mut self.instance,
                &window,
                ::render::RenderOptions {
                    msaa: ::config::get_int("graphics", "msaa").unwrap_or(4) as u32,
                    anisotropy: ::config::get_int("graphics", "anisotropy").unwrap_or(8) as u16,
                    shadow_size: ::config::get_int("graphics", "shadow_size").unwrap_or(2048) as u32,
                    ssao: ::config::get_bool("graphics", "ssao").unwrap_or(true),
                    render_scale: ::config::get_float("graphics", "render_scale").unwrap_or(0.0) as f32,
                    compress_textures: ::config::get_bool("graphics", "texture_compression").unwrap_or(true),
                    fxaa: ::config::get_string("graphics", "post_aa").as_deref() != Some("off"),
                    min_obj_size: ::config::get_float("graphics", "min_obj_size").unwrap_or(0.013) as f32,
                    max_obj_dist: match ::config::get_float("graphics", "max_obj_dist").unwrap_or(-1.0) as f32 {
                        d if d >= 0.0 => d,
                        _ => ::config::get_float("graphics", "view_distance").filter(|v| *v > 0.0).map(|v| v as f32).unwrap_or(900.0),
                    },
                    omsi_shadow_casters: ::config::get_string("graphics", "shadow_casters").as_deref() == Some("omsi"),
                    shadow_blobs: ::config::get_bool("graphics", "shadow_blobs").unwrap_or(true),
                    reflections: ::config::get_bool("graphics", "reflections").unwrap_or(true),
                    no_enhanced: ::config::get_string("graphics", "graphics").as_deref() != Some("enhanced"),
                },
            ) {
                Ok(r) => r,
                Err(e) => {
                    fatal_message(&format!("The game cannot draw on this computer: {e:#}"));
                    platform::exit(event_loop);
                    return;
                }
            };
        #[cfg(windows)]
        if cfg!(windows)
            && (::config::get_bool("vr", "enabled").unwrap_or(false)
            || ::legacy_config::env::var_os("OMSI_OPENXR").is_some()) {
            match openxr::Vr::new(
                &renderer,
                ::config::get_float("vr", "scale").unwrap_or(0.65).clamp(0.5, 1.0) as f32,
                ::config::get_bool("vr", "desktop-mirror").unwrap_or(true),
            ) {
                Ok(vr) => self.vr = Some(vr),
                Err(e) => log::error!("OpenXR could not start: {e:#}"),
            }
        }
        let upload = renderer.upload_speed_mb_s();
        log::info!("graphics: {upload:.0} MB/s copied towards the card");
        if upload < SLOW_UPLOAD_MB_S {
            log::error!(
                "graphics: the driver copies only {upload:.0} MB/s towards the card (thousands are usual); every texture and buffer the game sends waits on it, down to a few frames a second - restarting the computer usually brings it back"
            );
            self.service_msg = Some((
                format!(
                    "Graphics driver is slow ({upload:.0} MB/s): the game will stutter. Restarting the computer usually fixes it."
                ),
                30.0,
            ));
        }
        lights::load_smoke_texture(&mut renderer, &self.args.root);
        lights::set_corona_root(&self.args.root);
        let size = window.inner_size();
        let surface = match SurfaceState::new_with(
            &self.instance,
            window.clone(),
            &renderer,
            size.width,
            size.height,
            ::config::get_bool("graphics", "vsync").unwrap_or(true) && !self.vr_active(),
        ) {
            Ok(s) => s,
            Err(e) => {
                fatal_message(&format!("The game's window cannot be drawn into: {e:#}"));
                platform::exit(event_loop);
                return;
            }
        };
        if start_exclusive {
            window.set_fullscreen(Some(crate::game_lists::exclusive_fullscreen(&window)));
        }
        let (sw, sh) = renderer.scene_size(size.width, size.height);
        log::info!(
            "window: {}x{} pixels (scale factor {:.2}), 3D picture {sw}x{sh}, present mode {:?}",
            size.width,
            size.height,
            window.scale_factor(),
            surface.config.present_mode
        );
        let scene = renderer.new_scene();
        self.window = Some(window);
        self.surface = Some(surface);
        self.renderer = Some(renderer);
        self.scene = Some(scene);
        self.present_splash("Starting");
        if self.args.bus.is_some() || self.args.cam.is_some() || self.args.no_menu {
            self.load_world_now(event_loop);
        } else {
            let mut fonts = ::simulation::texttex::FontLibrary::new(&self.args.root);
            self.hud = Some(hud::Hud::new(&mut fonts));
            self.menu = Some(menu::Menu::new(&self.args.root, &self.args.map));
        }
    }

    pub(crate) fn present_splash(&mut self, caption: &str) {
        if let (Some(win), Some(s), Some(r)) = (
            self.window.clone(),
            self.surface.as_mut(),
            self.renderer.as_ref(),
        ) {
            let size = win.inner_size();
            s.resize(r, size.width.max(1), size.height.max(1));
        }
        if let (Some(ui), Some(s), Some(win), Some(r), Some(scene)) = (
            self.ui.as_mut(),
            self.surface.as_ref(),
            self.window.as_ref(),
            self.renderer.as_mut(),
            self.scene.as_mut(),
        ) {
            scene.overlays.clear();
            let dpi = win.scale_factor() as f32;
            let scale = dpi
                * ui::size_factor(
                s.config.height as f32,
                dpi,
                ::config::get_float("ui", "scale").unwrap_or(1.0) as f32,
                ::config::get_bool("ui", "scale_window").unwrap_or(true),
            );
            ui.loading_bg = Some(None);
            ui.loading(
                r,
                scene,
                s.config.width as f32,
                s.config.height as f32,
                scale,
                "",
                caption,
                None,
                None,
                0.0,
            );
            ui.loading_bg = None;
            if let wgpu::CurrentSurfaceTexture::Success(frame)
            | wgpu::CurrentSurfaceTexture::Suboptimal(frame) = s.surface.get_current_texture()
            {
                let view = frame.texture.create_view(&Default::default());
                let blank = Camera {
                    position: DVec3::new(0.0, 0.0, -1.0e6),
                    yaw: 0.0,
                    pitch: -89.0,
                    roll: 0.0,
                    fov_deg: 60.0,
                    near: 0.5,
                    far: 10.0,
                };
                let lighting = ::render::Lighting {
                    sky_color: Vec3::new(0.04, 0.045, 0.055),
                    ..Default::default()
                };
                r.render(
                    scene,
                    &view,
                    s.config.width,
                    s.config.height,
                    &blank,
                    &lighting,
                );
                win.pre_present_notify();
                r.queue.present(frame);
            }
            scene.overlays.clear();
        }
        if let Some(win) = self.window.as_ref() {
            win.set_visible(true);
            win.request_redraw();
        }
    }

    pub(crate) fn loading_preview(&mut self) {
        let map_dir = self.world.as_ref().map(|w| w.map_dir.clone()).or_else(|| {
            self.args
                .root
                .join(&self.args.map)
                .parent()
                .map(|d| d.to_path_buf())
        });
        let name = map_dir
            .as_ref()
            .and_then(|d| d.file_name())
            .map(|n| n.to_string_lossy().into_owned())
            .unwrap_or_default();
        let t = self.started.elapsed().as_secs_f32();
        if let (Some(ui), Some(s), Some(win), Some(r), Some(scene)) = (
            self.ui.as_mut(),
            self.surface.as_ref(),
            self.window.as_ref(),
            self.renderer.as_mut(),
            self.scene.as_mut(),
        ) {
            scene.overlays.clear();
            let dpi = win.scale_factor() as f32;
            let scale = dpi
                * ui::size_factor(
                s.config.height as f32,
                dpi,
                ::config::get_float("ui", "scale").unwrap_or(1.0) as f32,
                ::config::get_bool("ui", "scale_window").unwrap_or(true),
            );
            ui.loading(
                r,
                scene,
                s.config.width as f32,
                s.config.height as f32,
                scale,
                &name,
                "Loading",
                Some((t / 10.0).fract()),
                map_dir.as_deref(),
                t,
            );
            if let wgpu::CurrentSurfaceTexture::Success(frame)
            | wgpu::CurrentSurfaceTexture::Suboptimal(frame) = s.surface.get_current_texture()
            {
                let view = frame.texture.create_view(&Default::default());
                let blank = Camera {
                    position: DVec3::new(0.0, 0.0, -1.0e6),
                    yaw: 0.0,
                    pitch: -89.0,
                    roll: 0.0,
                    fov_deg: 60.0,
                    near: 0.5,
                    far: 10.0,
                };
                let lighting = ::render::Lighting {
                    sky_color: Vec3::new(0.04, 0.045, 0.055),
                    ..Default::default()
                };
                r.render(
                    scene,
                    &view,
                    s.config.width,
                    s.config.height,
                    &blank,
                    &lighting,
                );
                win.pre_present_notify();
                r.queue.present(frame);
            }
            win.request_redraw();
        }
    }

    #[cfg(not(target_os = "android"))]
    pub(crate) fn update_discord(&mut self, loading: bool) {
        if self.args.server.is_some() || !::config::get_bool("discord", "status").unwrap_or(true) {
            if let Some(discord) = self.discord.as_ref() {
                discord.stop();
                if discord.is_finished() {
                    self.discord.take();
                }
            }
            return;
        }
        if let Some(discord) = self.discord.as_ref() {
            if discord.is_stopping() {
                if !discord.is_finished() {
                    return;
                }
                self.discord.take();
                self.discord_next_update = Instant::now();
            }
        }
        let now = Instant::now();
        if now < self.discord_next_update {
            return;
        }
        self.discord_next_update = now + std::time::Duration::from_secs(5);
        if self.discord.is_none() {
            self.discord = discord::Discord::start(&::config::get_string("discord", "app_id").unwrap_or_default());
        }
        if let Some(discord) = self.discord.as_ref() {
            let bus = self.player.as_ref().map(|p| {
                let definition = &p.vehicle.ty.def;
                let short =
                    omsi_launcher_lib::vehicle_type_label(&definition.type_name, &definition.path);
                let full = omsi_launcher_lib::display_bus_name(&format!(
                    "{} {short}",
                    definition.manufacturer
                ));
                (short, full)
            });
            let duty = self
                .duty
                .as_ref()
                .map(|d| (d.line.as_str(), d.tour.as_str()));
            discord.set(discord::Presence::for_game(
                self.world.as_ref().map(|w| w.global.name.as_str()),
                bus.as_ref()
                    .map(|(short, full)| (short.as_str(), full.as_str())),
                duty,
                self.lan.is_some(),
                loading,
                self.paused,
            ));
        }
    }

    pub(crate) fn load_world_now(&mut self, event_loop: &ActiveEventLoop) {
        #[cfg(not(target_os = "android"))]
        {
            self.discord_next_update = Instant::now();
            self.update_discord(true);
        }
        if let Some(l) = self.lan.as_mut() {
            lan::adopt_host_world(&mut self.args, l, &mut self.remotes);
        }
        if let Some(ui) = self.ui.as_mut() {
            ui.scene_replaced();
        }
        let renderer = self.renderer.take().expect("renderer");
        let mut scene = renderer.new_scene();
        self.envir = ::content::Envir::load(&self.args.root.join("envir.cfg")).ok();
        if weather_cycle::is_cycle(self.args.weather.as_deref()) {
            let seed = std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map(|d| d.as_nanos() as u64)
                .unwrap_or(7);
            let mut c = weather_cycle::Cycle::new(seed);
            let month = start_clock(&self.args).day_month().1;
            let all = weather_cycle::installed();
            let clear = ::content::weather::Weather {
                fog: (50000.0, 1.0),
                ..Default::default()
            };
            let r = c.rand();
            self.args.weather = weather_cycle::pick(&all, &clear, "", month, r);
            log::info!("weather cycle: starting with {:?}", self.args.weather);
            self.weather_cycle = Some(c);
        }
        self.weather = Some(load_weather(&self.args));
        self.wetness = self.weather.as_ref().map(initial_wetness).unwrap_or(0.0);
        self.clock = start_clock(&self.args);
        setup_sky(
            &self.args,
            &renderer,
            &mut scene,
            self.envir.as_ref(),
            self.weather.as_ref(),
        );
        if !self.args.all && self.args.radius.is_none() {
            match open_world(&self.args) {
                Ok((w, cam, _)) => {
                    let w = Arc::new(w);
                    let distance = self
                        .args
                        .view_distance
                        .or_else(|| ::config::get_float("graphics", "view_distance").filter(|v| *v > 0.0))
                        .unwrap_or(900.0)
                        .max(::map::tile_size());
                    w.set_fast_texture_loads(true);
                    w.set_texture_budget(texture_budget());
                    log::info!(
                        "texture budget: {:.0} MB",
                        texture_budget() as f64 / 1e6
                    );
                    self.streamer = Some(tiles::Streamer::new(
                        w.clone(),
                        &start_centers(&self.args, &cam, Some(&w)),
                        distance,
                        700.0,
                    ));
                    self.world = Some(w);
                    self.starting = Some(cam);
                }
                Err(e) => {
                    log::error!("{e:#}");
                    platform::exit(event_loop);
                }
            }
            self.renderer = Some(renderer);
            self.scene = Some(scene);
            self.last = Instant::now();
            return;
        }
        match load_world(&self.args, &renderer, &mut scene) {
            Ok((w, cam)) => self.start_world(Arc::new(w), cam, &renderer, &mut scene),
            Err(e) => {
                log::error!("{e:#}");
                platform::exit(event_loop);
            }
        }
        self.renderer = Some(renderer);
        self.scene = Some(scene);
        self.last = Instant::now();
    }

    pub(crate) fn start_world(
        &mut self,
        w: Arc<World>,
        cam: Camera,
        renderer: &Renderer,
        mut scene: &mut Scene,
    ) {
        report_missing_content(&w, &mut self.service_msg);
        {
            {
                let first = spawn_player(&self.args, &w, &renderer, &mut scene);
                let spawned = match first {
                    Err(e) if self.args.bus.is_some() => {
                        log::warn!("the bus could not be put down ({e:#}); trying again");
                        spawn_player(&self.args, &w, &renderer, &mut scene).map_err(|e2| {
                            self.service_msg = Some((
                                format!(
                                    "The bus could not be loaded: {}",
                                    format!("{e2:#}").lines().next().unwrap_or_default()
                                ),
                                15.0,
                            ));
                            e2
                        })
                    }
                    other => other,
                };
                match spawned {
                    Ok(mut p) => {
                        let audio = ::audio::AudioEngine::new();
                        // Queue the user's master before any vehicle can start a voice;
                        // the first redraw must not briefly play at the default full level.
                        audio.set_listener(::audio::Listener {
                            master: if self.paused { 0.0 } else { (::config::get_float("audio", "master-volume").unwrap_or(1.0) as f32).clamp(0.0, 1.0) },
                            ..Default::default()
                        });
                        if let Some(p) = p.as_mut() {
                            p.vehicle.host.auto_clutch =
                                if ::config::get_bool("gameplay", "auto_clutch").unwrap_or(true) { 1.0 } else { 0.0 };
                            p.load_sounds(&audio);
                            p.ibis_background = true;
                            if self.args.autostart {
                                let msg = p.start_up();
                                self.service_msg = Some((msg, 6.0));
                            }
                            if !p.vehicle.ty.missing_packs.is_empty() {
                                let packs: Vec<String> = p
                                    .vehicle
                                    .ty
                                    .missing_packs
                                    .iter()
                                    .map(|(n, _)| n.clone())
                                    .collect();
                                let msg = format!(
                                    "This bus takes parts from vehicle pack(s) that are not installed: {} (install them for its displays and devices)",
                                    packs.join(", ")
                                );
                                self.service_msg = Some(match self.service_msg.take() {
                                    Some((m, _)) => (format!("{m}   |   {msg}"), 12.0),
                                    None => (msg, 12.0),
                                });
                            }
                        }
                        self.ambience = Some(ambience::Ambience::load(&audio, &self.args.root));
                        self.audio = Some(audio);
                        if let Some(p) = &p {
                            if self.args.cam.is_none() && self.args.view != "free" {
                                self.camera = Some(p.camera(&self.args.view, &cam));
                            }
                        }
                        self.player = p;
                    }
                    Err(e) => log::error!("{e:#}"),
                }
                for o in self.args.situation_others.clone() {
                    let one = Args {
                        bus: Some(o.bus.clone()),
                        spawn: Some(o.spawn.clone()),
                        hof: o.hof.clone(),
                        paint: o.paint.clone(),
                        situation_vars: o.vars.clone(),
                        situation_strvars: o.strvars.clone(),
                        situation_others: Vec::new(),
                        line: None,
                        tour: None,
                        trip: None,
                        autostart: false,
                        ..self.args.clone()
                    };
                    match spawn_player(&one, &w, &renderer, &mut scene) {
                        Ok(Some(q)) => {
                            log::info!("situation: {} placed at {}", o.bus, o.spawn);
                            self.placed.push(q);
                        }
                        Ok(None) => {}
                        Err(e) => log::warn!("situation vehicle {}: {e:#}", o.bus),
                    }
                }
                if self.camera.is_none() {
                    self.camera = Some(cam);
                }
                self.navigator = Some(navigator::Navigator::new(
                    ::config::get_bool("ui", "navigator").unwrap_or(true),
                    ::config::get_float("ui", "opacity").unwrap_or(0.85).clamp(0.2, 1.0) as f32,
                    &::config::get_string("ui", "navigator_corner").unwrap_or_else(|| "bottom-left".into()),
                ));
                if let Some(n) = self.navigator.as_mut() {
                    n.arrows = ::config::get_bool("navigator", "arrows").unwrap_or(false);
                    n.show_ai = ::config::get_bool("navigator", "ai").unwrap_or(true);
                    n.show_topbar = ::config::get_bool("navigator", "topbar").unwrap_or(true);
                    n.show_turn = ::config::get_bool("navigator", "turn").unwrap_or(true);
                    n.show_stoplist = ::config::get_bool("navigator", "stoplist").unwrap_or(true);
                    n.schedule = ::config::get_bool("navigator", "stops_ext").unwrap_or(false);
                }
                if let Some(d) = self.args.driver.as_deref() {
                    self.career = career::Career::load(&self.args.root, d);
                }
                if self.args.passengers || self.args.lan_join.is_some() {
                    let mut h = humans::Humans::new(&self.args.root);
                    if let Some(lan) = self.lan.as_ref() {
                        h.set_lan_seed(lan::population_seed(lan));
                    }
                    h.exact_fare = ::config::get_bool("gameplay", "exact_fare").unwrap_or(true);
                    h.boarding = ::config::get_string("gameplay", "boarding").unwrap_or_else(|| "auto".into());
                    h.prefer_seats = ::config::get_bool("gameplay", "pax_prefer_seats").unwrap_or(false);
                    h.rear_entry = ::config::get_bool("gameplay", "pax_rear_entry").unwrap_or(true);
                    h.voices = match ::config::get_string("passengers", "voices").unwrap_or_else(|| "all".into()).as_str() {
                        "off" => 2,
                        "tickets" => 1,
                        _ => 0,
                    };
                    let ik = self.args.pax_ik.unwrap_or(::config::get_bool("passengers", "ik").unwrap_or(true));
                    h.set_ik(ik);
                    h.set_natural(::config::get_string("passengers", "motion").unwrap_or_else(|| "natural".into()) == "natural");
                    if let Some(p) = self.player.as_mut() {
                        h.set_cabin(&mut p.vehicle);
                        h.ticket_key = ticket_key_name(&self.args.root, &p.bindings);
                        h.tickets = p.vehicle.host.tickets.clone();
                        if !w.global.money_system.trim().is_empty() {
                            h.money =
                                Some(money::Money::new(&self.args.root, &w.global.money_system));
                        }
                    }
                    if let Some(p) = self.player.as_ref() {
                        if self.args.riders > 0 {
                            let centre = p.vehicle.position;
                            h.populate(&w, &renderer, &mut scene, centre);
                            h.seed_riders(self.args.riders, &p.vehicle, &w, &renderer, &mut scene);
                        }
                    }
                    self.humans = Some(h);
                }
                let populated = self.args.traffic > 0
                    || self.args.schedule
                    || rail_drive::args_rail(&self.args)
                    || self.args.lan_join.is_some();
                {
                    match traffic::Traffic::new(&self.args.root, &w, self.args.traffic) {
                        Ok(mut t) => {
                            t.lights_only = !populated;
                            if let Some(lan) = self.lan.as_ref() {
                                t.set_lan_seed(lan::population_seed(lan));
                            }
                            if self.args.traffic > 0 {
                                t.precache_random(&w, &renderer, &mut scene);
                            }
                            t.day_time = parse_time(&self.args.time);
                            self.traffic = Some(t);
                        }
                        Err(e) => log::error!("traffic: {e:#}"),
                    }
                    if self.args.schedule {
                        let mut sch =
                            schedule::Schedule::new(&self.args.root, &w, &start_clock(&self.args));
                        sch.precache(
                            &w,
                            &renderer,
                            &mut scene,
                            self.traffic.as_mut(),
                            parse_time(&self.args.time),
                        );
                        if let (Some(line), Some(p)) = (&self.args.line, self.player.as_mut()) {
                            self.duty = match sch.player_duty(
                                &w,
                                line,
                                self.args.tour.as_deref().unwrap_or(""),
                                parse_time(&self.args.time),
                                self.args.trip.as_deref(),
                                self.args.whole_tour,
                            ) {
                                Ok(mut d) => {
                                    if let Some(k) = self.args.duty_trip {
                                        d.start_at(k, self.args.duty_first_stop);
                                    }
                                    Some(d)
                                }
                                Err(e) => {
                                    log::warn!("no player duty: {e}");
                                    self.service_msg = Some((format!("No duty: {e}"), 20.0));
                                    None
                                }
                            };
                            if let (true, Some(d)) = (self.args.autostart, self.duty.as_mut()) {
                                d.update(&mut p.vehicle, parse_time(&self.args.time));
                                let (trip, stop) = d.trip_for_ibis();
                                if p.auto_ibis {
                                    p.set_duty_destination(trip, stop);
                                }
                            }
                            if let Some(d) = self.duty.as_ref() {
                                let mut fonts = w.fonts.lock();
                                if let Err(e) =
                                    schedule_paper::update_vehicle(&mut p.vehicle, d, &mut fonts)
                                {
                                    log::warn!("driver timetable paper: {e:#}");
                                }
                            }
                        }
                        self.schedule = Some(sch);
                    }
                }
                if self.traffic.is_none() {
                    if let Some(n) = self.navigator.as_mut() {
                        n.add_lanes(std::mem::take(&mut *w.lanes.lock()));
                    }
                }
                if let (Some(lan), Some(p)) = (self.lan.as_mut(), self.player.as_mut()) {
                    lan::settle_spawn(
                        lan,
                        &mut self.remotes,
                        p,
                        &self.args,
                        &w,
                        self.traffic.as_ref().map(|t| &t.net),
                        std::time::Duration::from_millis(1500),
                    );
                }
                self.world = Some(w);
            }
        }
        self.last = Instant::now();
    }

    pub(crate) fn drive_start(&mut self, event_loop: &ActiveEventLoop) -> bool {
        let Some(cam) = self.starting.take() else {
            return true;
        };
        let (Some(renderer), Some(mut scene)) = (self.renderer.take(), self.scene.take()) else {
            self.starting = Some(cam);
            return true;
        };
        let centers = start_centers(&self.args, &cam, self.world.as_deref());
        let progress = match self.streamer.as_mut() {
            Some(streamer) => {
                streamer.update(
                    &renderer,
                    &mut scene,
                    &centers,
                    std::time::Duration::from_millis(30),
                    None,
                );
                streamer.initial_progress()
            }
            None => None,
        };
        let Some((done, total)) = progress else {
            let w = self.world.clone().expect("world");
            log::info!(
                "map ready after {:.2} s: {} tiles loaded",
                self.started.elapsed().as_secs_f64(),
                w.loaded_tiles().len()
            );
            self.start_world(w, cam, &renderer, &mut scene);
            self.renderer = Some(renderer);
            self.scene = Some(scene);
            return true;
        };
        let name = self
            .world
            .as_ref()
            .map(|w| {
                if w.global.friendly_name.trim().is_empty() {
                    w.global.name.clone()
                } else {
                    w.global.friendly_name.clone()
                }
            })
            .unwrap_or_default();
        let map_dir = self.world.as_ref().map(|w| w.map_dir.clone());
        let mut reconfigure = false;
        if let (Some(ui), Some(s), Some(win)) = (
            self.ui.as_mut(),
            self.surface.as_ref(),
            self.window.as_ref(),
        ) {
            scene.overlays.clear();
            let dpi = win.scale_factor() as f32;
            let scale = dpi
                * ui::size_factor(
                s.config.height as f32,
                dpi,
                ::config::get_float("ui", "scale").unwrap_or(1.0) as f32,
                ::config::get_bool("ui", "scale_window").unwrap_or(true),
            );
            ui.loading(
                &renderer,
                &mut scene,
                s.config.width as f32,
                s.config.height as f32,
                scale,
                name.trim(),
                "Loading",
                Some(done as f32 / total.max(1) as f32),
                map_dir.as_deref(),
                self.started.elapsed().as_secs_f32(),
            );
            let acquired = s.surface.get_current_texture();
            reconfigure = matches!(
                acquired,
                wgpu::CurrentSurfaceTexture::Outdated | wgpu::CurrentSurfaceTexture::Lost
            );
            if let wgpu::CurrentSurfaceTexture::Success(frame)
            | wgpu::CurrentSurfaceTexture::Suboptimal(frame) = acquired
            {
                let view = frame.texture.create_view(&Default::default());
                let blank = Camera {
                    position: DVec3::new(0.0, 0.0, -1.0e6),
                    yaw: 0.0,
                    pitch: -89.0,
                    roll: 0.0,
                    fov_deg: 60.0,
                    near: 0.5,
                    far: 10.0,
                };
                let lighting = ::render::Lighting {
                    sky_color: Vec3::new(0.08, 0.10, 0.14),
                    ..Default::default()
                };
                let mut renderer = renderer;
                renderer.render(
                    &mut scene,
                    &view,
                    s.config.width,
                    s.config.height,
                    &blank,
                    &lighting,
                );
                win.pre_present_notify();
                renderer.queue.present(frame);
                self.renderer = Some(renderer);
            } else {
                self.renderer = Some(renderer);
            }
            win.request_redraw();
        } else {
            self.renderer = Some(renderer);
        }
        if reconfigure {
            if let (Some(s), Some(r), Some(win)) = (
                self.surface.as_mut(),
                self.renderer.as_ref(),
                self.window.as_ref(),
            ) {
                let size = win.inner_size();
                s.resize(r, size.width, size.height);
            }
        }
        self.scene = Some(scene);
        self.starting = Some(cam);
        if let Some(limit) = self.args.exit_after {
            if self.started.elapsed().as_secs_f32() > limit {
                log::info!("exit after {limit} s while loading: {done} of {total} tiles");
                platform::exit(event_loop);
            }
        }
        false
    }

    pub(crate) fn drive_streaming(&mut self) {
        let mut centers: Vec<DVec3> = self.camera.iter().map(|c| c.position).collect();
        centers.extend(self.player.iter().map(|p| p.vehicle.position));
        if self
            .lan
            .as_ref()
            .map(|l| l.role == ::network::Role::Host)
            .unwrap_or(false)
        {
            centers.extend(self.remotes.remotes.values().map(|r| r.vehicle().position));
        }
        let (Some(streamer), Some(w), Some(r), Some(scene)) = (
            self.streamer.as_mut(),
            self.world.as_ref(),
            self.renderer.as_ref(),
            self.scene.as_mut(),
        ) else {
            return;
        };
        w.apply_texture_upgrades(
            r,
            scene,
            Some(Instant::now() + std::time::Duration::from_millis(2)),
        );
        w.update_texture_budget(r, scene, &centers, false);
        if centers.is_empty()
            || !streamer.update(
            r,
            scene,
            &centers,
            std::time::Duration::from_millis(6),
            self.audio.as_ref(),
        )
        {
            return;
        }
        if let Some(p) = self.player.as_mut() {
            let objects = ::config::get_bool("gameplay", "collision_objects").unwrap_or(true);
            p.vehicle.collision = objects.then(|| w.collision.lock().clone());
            p.vehicle.wheel_walls = objects;
        }
        match self.traffic.as_mut() {
            Some(t) => {
                t.add_tiles(w);
            }
            None => {
                let lanes = std::mem::take(&mut *w.lanes.lock());
                if let Some(n) = self.navigator.as_mut() {
                    n.add_lanes(lanes);
                }
            }
        }
        if let Some(on) = self.lamps_on {
            w.set_lamps(r, scene, on);
        }
    }
}

pub(crate) fn start_centers(args: &Args, cam: &Camera, world: Option<&World>) -> Vec<DVec3> {
    let mut out: Vec<DVec3> = spawn_point(args, world).into_iter().collect();
    let view_in_bus = args.bus.is_some() && args.view != "free";
    if out.is_empty() || !view_in_bus {
        out.push(cam.position);
    }
    out
}

pub(crate) fn report_missing_content(w: &World, msg: &mut Option<(String, f32)>) {
    let (files, textures) = w.missing_content();
    if files.is_empty() && textures.is_empty() {
        return;
    }
    let addon = |f: &str| f.split('/').take(2).collect::<Vec<_>>().join("/");
    let mut by_addon: std::collections::BTreeMap<String, Vec<String>> = Default::default();
    for (f, what) in &files {
        by_addon
            .entry(addon(f))
            .or_default()
            .push(format!("{what}: {f}"));
    }
    let mut text = format!(
        "neoOMSI: content this map uses that is not installed\nmap: {}\n\n",
        w.map_dir.display()
    );
    for (a, list) in &by_addon {
        text.push_str(&format!("{a} ({} files)\n", list.len()));
        for l in list {
            text.push_str(&format!("  {l}\n"));
        }
    }
    if !textures.is_empty() {
        text.push_str(&format!("\ntextures not found ({}):\n", textures.len()));
        for t in &textures {
            text.push_str(&format!("  {t}\n"));
        }
    }
    let Some(dir) = lan::data_dir() else { return };
    let path = dir.join("missing_content.txt");
    let _ = std::fs::write(&path, text);
    let objects = files.iter().filter(|(_, w)| *w != "spline").count();
    let splines = files.len() - objects;
    let addons: Vec<&String> = by_addon.keys().take(4).collect();
    let more = if by_addon.len() > 4 {
        format!(" and {} more", by_addon.len() - 4)
    } else {
        String::new()
    };
    log::warn!(
        "missing content: {objects} objects, {splines} splines, {} textures (list: {})",
        textures.len(),
        path.display()
    );
    if !files.is_empty() {
        *msg = Some((
            format!(
                "This map uses {objects} objects and {splines} splines that are not installed (add-ons: {}{more}). The list is in {}",
                addons
                    .iter()
                    .map(|a| a.as_str())
                    .collect::<Vec<_>>()
                    .join(", "),
                path.display()
            ),
            15.0,
        ));
    }
}

pub(crate) const CAM_BLEND_SECS: f32 = 0.6;
pub(crate) const CAM_BLEND_MAX_DT: f32 = 1.0 / 30.0;

fn wrap_deg(a: f32) -> f32 {
    (a + 180.0).rem_euclid(360.0) - 180.0
}

pub(crate) fn blend_local(
    a: &::legacy_vehicle::Camera,
    b: &::legacy_vehicle::Camera,
    k: f32,
) -> ::legacy_vehicle::Camera {
    let k = k.clamp(0.0, 1.0);
    if k >= 1.0 {
        return b.clone();
    }
    let rest = 1.0 - k;
    let l = |x: f32, y: f32| y + (x - y) * rest;
    let dir = |c: &::legacy_vehicle::Camera| {
        let (sy, cy) = c.yaw.to_radians().sin_cos();
        let (sp, cp) = c.pitch.to_radians().sin_cos();
        Vec3::new(sy * cp, cy * cp, sp)
    };
    let (fa, fb) = (dir(a), dir(b));
    let dot = fa.dot(fb).clamp(-1.0, 1.0);
    let (yaw, pitch) = if dot < -0.9995 {
        (b.yaw - wrap_deg(b.yaw - a.yaw) * rest, l(a.pitch, b.pitch))
    } else {
        let f = if dot > 0.9995 {
            (fa * rest + fb * k).normalize_or(fb)
        } else {
            let theta = dot.acos();
            let s = theta.sin();
            ((fa * ((rest * theta).sin() / s)) + (fb * ((k * theta).sin() / s))).normalize_or(fb)
        };
        (
            f.x.atan2(f.y).to_degrees(),
            f.z.clamp(-1.0, 1.0).asin().to_degrees(),
        )
    };
    ::legacy_vehicle::Camera {
        pos: [
            l(a.pos[0], b.pos[0]),
            l(a.pos[1], b.pos[1]),
            l(a.pos[2], b.pos[2]),
        ],
        dist: l(a.dist, b.dist),
        fov: l(a.fov, b.fov),
        yaw,
        pitch,
        extra: b.extra,
    }
}

pub(crate) struct FrozenMirrors {
    pub(crate) bus: u64,
    pub(crate) since: f32,
}

#[derive(Default)]
pub(crate) struct CamBlend {
    pub key: Option<(String, (usize, usize))>,
    pub resetting: bool,
    pub reset_zoom: Option<f32>,
    pub from: Option<::legacy_vehicle::Camera>,
    pub shown: Option<::legacy_vehicle::Camera>,
    pub entering: bool,
    pub t: f32,
    pub carry: Option<CamCarry>,
}

#[derive(Clone, Copy)]
pub(crate) struct CamCarry {
    pub pos: DVec3,
    pub yaw: f32,
    pub pitch: f32,
    pub roll: f32,
    pub fov: f32,
}

impl CamCarry {
    pub fn between(a: &Camera, b: &Camera) -> Self {
        Self {
            pos: a.position - b.position,
            yaw: wrap_deg(a.yaw - b.yaw),
            pitch: a.pitch - b.pitch,
            roll: wrap_deg(a.roll - b.roll),
            fov: a.fov_deg - b.fov_deg,
        }
    }

    pub fn apply(&self, c: &mut Camera) {
        c.position += self.pos;
        c.yaw += self.yaw;
        c.pitch = (c.pitch + self.pitch).clamp(-89.0, 89.0);
        c.roll += self.roll;
        c.fov_deg += self.fov;
    }

    pub fn decay(&mut self, dt: f32) -> bool {
        let k = (-dt.clamp(0.0, 0.1) * 12.0).exp();
        self.pos *= k as f64;
        self.yaw *= k;
        self.pitch *= k;
        self.roll *= k;
        self.fov *= k;
        self.pos.length() > 1e-4
            || self.yaw.abs() > 0.01
            || self.pitch.abs() > 0.01
            || self.roll.abs() > 0.01
            || self.fov.abs() > 0.01
    }
}

impl CamBlend {
    pub fn progress(&self) -> f32 {
        let t = self.t.clamp(0.0, 1.0);
        t * t * t * (t * (t * 6.0 - 15.0) + 10.0)
    }
}
