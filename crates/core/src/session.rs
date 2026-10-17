//! Session end, saves (quicksave, slots, last situation), screenshots and restart after device loss.

use super::*;

pub(crate) const SAVES: &str = "Saves";

impl App {
    pub(crate) fn finish_session(&mut self) {
        crate::game_lists::flush_settings(true);
        self.exiting = true;
        if let Some(w) = self.world.clone() {
            let mut none = None;
            crate::app::report_missing_content(&w, &mut none);
        }
        if let Some(mut p) = self.plugins.take() {
            p.finalize();
        }
        if self.player.is_none() || self.career.seconds <= 0.0 {
            return;
        }
        self.save_last_situation();
        if self.career.path.is_some() {
            if let Err(e) = self.career.save() {
                log::warn!("writing the personnel file: {e}");
            }
        }
        let bus = self.args.bus.clone().unwrap_or_default();
        if let Err(e) = self.career.write_session(
            &self.args.map,
            &bus,
            self.args.line.as_deref(),
            self.args.tour.as_deref(),
        ) {
            log::warn!("writing the session summary: {e}");
        }
        self.career.seconds = 0.0;
    }

    pub(crate) fn back_to_main_menu(&mut self) {
        // (the session that ends gets its own summary; the next one is measured from its own start)
        self.log_profile_summary(true);
        self.play_since = None;
        self.finish_session();
        self.exiting = false;
        self.present_splash(&::i18n::translate("splash.loading", &[]));
        if let Some(ui) = self.ui.as_mut() {
            ui.scene_replaced();
        }
        self.game_menu = None;
        self.lab_menu = None;
        self.lab_list = None;
        self.lab_load = None;
        self.lab_place = None;
        self.lab_room = None;
        self.lab_map_direct = false;
        self.dropdown = None;
        self.admin_list = None;
        self.list_kind = None;
        self.chooser = None;
        self.report_view = None;
        self.report_pending = false;
        self.restart_pending = false;
        self.restart_prompt = None;
        self.screenshot_mode = None;
        self.menu_edit = None;
        self.menu_edit_icao = false;
        self.menu_edit_date = false;
        self.swap_pending = false;
        self.teleport_pick = false;
        self.placing = None;
        self.editor = None;
        self.tutorial = None;
        self.on_foot = None;
        self.remote_walkers.clear();
        self.inside_remote = None;
        self.is_admin = false;
        self.in_cab = false;
        self.player = None;
        self.placed.clear();
        self.traffic = None;
        self.schedule = None;
        self.humans = None;
        self.duty = None;
        self.hud = None;
        self.navigator = None;
        self.camera = None;
        self.starting = None;
        self.streamer = None;
        self.world = None;
        self.world_day = None;
        self.audio = None;
        self.ambience = None;
        self.plugins = None;
        // (bye bye session)
        if let Some(l) = self.lan.take() {
            l.leave();
        }
        lan_mods::clean_up();
        self.voice = Default::default();
        self.remotes = Default::default();
        self.weather = None;
        self.weather_cycle = None;
        self.weather_blend = None;
        self.envir = None;
        self.remote_added.clear();
        self.paused = false;
        self.menu_prev_pause = false;
        self.service_msg = None;
        self.view = self.args.view.clone();
        self.args.bus = None;
        self.args.cam = None;
        self.args.no_menu = false;
        self.args.situation = None;
        self.args.lan_join = None;
        self.args.lan_host = None;
        self.args.on_foot = false;
        if let (Some((id, _, _)), Some(r), Some(scene)) =
            (self.lab_pic.take(), self.renderer.as_ref(), self.scene.as_mut())
        {
            r.free_texture(scene, id);
        }
        if let Some(r) = self.renderer.as_ref() {
            self.scene = Some(r.new_scene());
        }
        if let Some(u) = self.ui.as_mut() {
            u.start_reset();
        }
        let mut menu = menu::Menu::new(&self.args.root, &self.args.map);
        menu.refresh_continue();
        menu.apply_last_situation();
        menu.recall();
        self.menu = Some(menu);
        self.last = Instant::now();
        log::info!("back to the start menu");
    }

    pub(crate) fn load_quicksave(&mut self) -> bool {
        let dir = crate::startup::content_dir()
            .unwrap_or_else(|| self.args.root.clone())
            .join("Situations");
        let file = dir.join("quicksave.osn");
        if !file.exists() {
            self.service_msg = Some(("No quicksave yet (Ctrl+S saves one)".into(), 4.0));
            return false;
        }
        let Ok(exe) = std::env::current_exe() else {
            return false;
        };
        let mut cmd = std::process::Command::new(exe);
        cmd.env_remove("OMSI_INSTANCE");
        cmd.arg("--root")
            .arg(&self.args.root)
            .arg("--no-menu")
            .arg("--situation")
            .arg(&file);
        match cmd.spawn() {
            Ok(_) => {
                log::info!("loading {} in a new game", file.display());
                true
            }
            Err(e) => {
                self.service_msg = Some((format!("Could not start the game again: {e}"), 5.0));
                false
            }
        }
    }

    pub(crate) fn save_last_situation(&mut self) -> Option<std::path::PathBuf> {
        if self.tutorial.is_some() || self.lan.is_some() || self.player.is_none() {
            return None;
        }
        let (Some(w), Some(cam)) = (self.world.as_ref(), self.camera.as_ref()) else {
            return None;
        };
        let dir = std::path::Path::new(&self.args.map.replace('\\', "/"))
            .parent()
            .map(|d| d.to_path_buf())?;
        let base = crate::startup::content_dir()?;
        let dir = base.join(dir);
        let _ = std::fs::create_dir_all(&dir);
        let out = dir.join("laststn.osn");
        let sit = build_situation(
            &self.args,
            w,
            &self.clock,
            self.args.weather.as_deref(),
            self.player.as_ref(),
            &self.placed,
            cam,
            self.duty.as_ref(),
            "Last situation",
        );
        match sit.save(&out) {
            Ok(()) => {
                log::info!("saved the last situation {}", out.display());
                Some(out)
            }
            Err(e) => {
                log::warn!("saving {}: {e}", out.display());
                None
            }
        }
    }

    pub(crate) fn restart_after_device_loss(&mut self) -> bool {
        let n = ::legacy_config::env::var("OMSI_SAFE_GPU")
            .ok()
            .and_then(|v| v.parse::<u32>().ok())
            .unwrap_or(0);
        if n >= 2 {
            return false;
        }
        let Some(file) = self.save_last_situation() else {
            return false;
        };
        let Ok(exe) = std::env::current_exe() else {
            return false;
        };
        let mut cmd = std::process::Command::new(exe);
        cmd.env_remove("OMSI_INSTANCE");
        cmd.arg("--root")
            .arg(&self.args.root)
            .arg("--no-menu")
            .arg("--situation")
            .arg(&file);
        cmd.env("OMSI_SAFE_GPU", (n + 1).to_string());
        // (on Windows the other interface: DirectX 12 after Vulkan, Vulkan after DirectX 12 -
        // an AMD Radeon's DX12 driver lost the device where its Vulkan one did not, #274)
        let name = self
            .renderer
            .as_ref()
            .map(|r| r.adapter_name.clone())
            .unwrap_or_default();
        if cfg!(windows) {
            if name.contains("(Vulkan)") {
                cmd.env("OMSI_BACKEND", "dx12");
            } else if name.contains("(Dx12)") {
                cmd.env("OMSI_BACKEND", "vulkan");
            }
        }
        match cmd.spawn() {
            Ok(_) => {
                log::warn!(
                    "starting again with safer graphics on {} (the graphics device was lost)",
                    file.display()
                );
                true
            }
            Err(e) => {
                log::warn!("could not start the game again: {e}");
                false
            }
        }
    }

    pub(crate) fn restart_game(&mut self, event_loop: &ActiveEventLoop) -> bool {
        game_lists::flush_settings(true);
        let Some(file) = self.save_last_situation() else {
            self.service_msg = Some(("The game cannot be restarted here".into(), 5.0));
            return false;
        };
        let Ok(exe) = std::env::current_exe() else {
            return false;
        };
        let mut cmd = std::process::Command::new(exe);
        cmd.env_remove("OMSI_INSTANCE");
        cmd.arg("--root")
            .arg(&self.args.root)
            .arg("--no-menu")
            .arg("--situation")
            .arg(&file);
        if let Err(e) = cmd.spawn() {
            self.service_msg = Some((format!("Could not start the game again: {e}"), 5.0));
            return false;
        }
        self.game_menu = None;
        self.lab_menu = None;
        self.finish_session();
        platform::exit(event_loop);
        true
    }

    pub(crate) fn quick_save(&mut self) {
        let (Some(w), Some(cam)) = (self.world.as_ref(), self.camera.as_ref()) else {
            return;
        };
        let dir = content_dir()
            .unwrap_or_else(|| self.args.root.clone())
            .join("Situations");
        let _ = std::fs::create_dir_all(&dir);
        let out = dir.join("quicksave.osn");
        let sit = build_situation(
            &self.args,
            w,
            &self.clock,
            self.args.weather.as_deref(),
            self.player.as_ref(),
            &self.placed,
            cam,
            self.duty.as_ref(),
            "Quicksave",
        );
        match sit.save(&out) {
            Ok(()) => {
                log::info!(
                    "saved situation {} ({} vehicles)",
                    out.display(),
                    sit.vehicles.len()
                );
                self.service_msg = Some(("Situation saved (quicksave)".into(), 3.0));
            }
            Err(e) => {
                log::warn!("saving {}: {e}", out.display());
                self.service_msg = Some((format!("Could not save: {e}"), 5.0));
            }
        }
    }

    /// A save of its own (#341): the situation into the next free `Saves/Slot <n>.osn` of
    /// the map's folder in the content folder - none is ever overwritten. The launcher
    /// offers them, with the last situation, to continue from.
    pub(crate) fn save_slot(&mut self) {
        let (Some(w), Some(cam)) = (self.world.as_ref(), self.camera.as_ref()) else {
            return;
        };
        let Some(dir) = crate::startup::content_dir().and_then(|base| {
            std::path::Path::new(&self.args.map.replace('\\', "/"))
                .parent()
                .map(|d| base.join(d).join(SAVES))
        }) else {
            return;
        };
        let _ = std::fs::create_dir_all(&dir);
        let Some(n) = (1..10_000).find(|n| !dir.join(format!("Slot {n}.osn")).exists()) else {
            return;
        };
        let out = dir.join(format!("Slot {n}.osn"));
        let bus = self.player.as_ref().map(|p| {
            let d = &p.vehicle.ty.def;
            if d.type_name.trim().is_empty() {
                d.path
                    .file_stem()
                    .map(|s| s.to_string_lossy().to_string())
                    .unwrap_or_default()
            } else {
                d.type_name.trim().to_string()
            }
        });
        let t = self.clock.time;
        let what = match (bus, self.duty.as_ref()) {
            (Some(b), Some(d)) => format!("{b}, line {} / {}", d.line.trim(), d.tour.trim()),
            (Some(b), None) => b,
            (None, _) => "on foot".to_string(),
        };
        let name = format!(
            "Slot {n}: {what}, {:02}:{:02}",
            (t / 3600.0) as i32 % 24,
            ((t % 3600.0) / 60.0) as i32
        );
        let sit = build_situation(
            &self.args,
            w,
            &self.clock,
            self.args.weather.as_deref(),
            self.player.as_ref(),
            &self.placed,
            cam,
            self.duty.as_ref(),
            &name,
        );
        match sit.save(&out) {
            Ok(()) => {
                log::info!(
                    "saved situation {} ({} vehicles)",
                    out.display(),
                    sit.vehicles.len()
                );
                self.service_msg = Some((
                    format!("Saved as slot {n}: the launcher continues from it"),
                    4.0,
                ));
            }
            Err(e) => {
                log::warn!("saving {}: {e}", out.display());
                self.service_msg = Some((format!("Could not save: {e}"), 5.0));
            }
        }
    }

    pub(crate) fn take_screenshot(&mut self) {
        let dir = crate::startup::content_dir()
            .unwrap_or_else(|| self.args.root.clone())
            .join("Screenshots");
        let _ = std::fs::create_dir_all(&dir);
        let secs = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_secs())
            .unwrap_or(0);
        let path = dir.join(format!("omsi_{secs}.png"));
        if self.screenshot_mode.is_none() {
            self.service_msg = Some((format!("Screenshot: {}", path.display()), 4.0));
        }
        self.shot = Some(path);
    }
}
