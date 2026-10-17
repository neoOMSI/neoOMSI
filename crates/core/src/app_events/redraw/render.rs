use super::*;

impl App {
    pub(super) fn redraw_render(
        &mut self,
        event_loop: &ActiveEventLoop,
        f: &Frame,
        daylight: ::simulation::Daylight,
    ) {
        let Frame {
            now, raw_dt, dt, ..
        } = *f;
        // (the last picture is submitted before this frame's pictures write to the card)
        let __t = Instant::now();
        if let Some(r) = self.renderer.as_ref() {
            r.wait_submitted();
            r.flush_deferred();
        }
        *self.profile.entry("wait submit").or_default() += __t.elapsed().as_secs_f64();
        #[cfg(all(feature = "devtools", debug_assertions))]
        self.dev_actions(event_loop);
        #[cfg(all(feature = "devtools", debug_assertions))]
        let dev_extra = self.dev_gather();
        let menu_lines = if self.game_menu.is_some() && self.report_view.is_some() {
            self.game_menu_items()
        } else {
            Vec::new()
        };
        let vr_nav_display = self.vr_nav_display();
        let vr_active = self.vr_active();
        let screenshot_mode = self.screenshot_mode.is_some();
        let triple_projection = self.triple_screen_projection_active();
        let triple_hud = !vr_active
            && ::config::get_bool("graphics", "triple_screen").unwrap_or(false)
            && ::config::get_bool("graphics", "triple_screen_hud").unwrap_or(true)
            && self
            .surface
            .as_ref()
            .is_some_and(|s| s.config.width >= s.config.height.saturating_mul(2));
        let crosshair = !screenshot_mode
            && ::config::get_bool("camera", "crosshair").unwrap_or(true)
            && !matches!(self.view.as_str(), "pax" | "outside")
            && self.raycast_active();
        let screenshot_help = self.screenshot_mode.as_mut().and_then(|mode| {
            mode.help_left = (mode.help_left - dt).max(0.0);
            (mode.help_left > 0.0)
                .then(|| "Screenshot mode: HUD hidden. Press Esc to return.".to_string())
        });
        self.lab_poll();
        self.lab_place_preview(dt);
        self.lab_map_sync();
        self.lab_entries_sync();
        let vehicle_menu: Vec<crate::ui::VehicleGroup> = match self.lab_menu {
            Some(st) if st.page == Some(crate::ui::VEHICLE_PAGE) => game_lists::vehicle_menu(self)
                .into_iter()
                .map(|(id, acts)| crate::ui::VehicleGroup {
                    id,
                    actions: acts
                        .into_iter()
                        .map(|(id, opens)| crate::ui::VehicleAction { id, opens })
                        .collect(),
                })
                .collect(),
            _ => Vec::new(),
        };
        let menu_tabs = match self.list_kind.as_ref() {
            Some(k) if self.chooser.is_some() => game_lists::page_titles(self, k),
            _ => None,
        };
        if let (true, Some(r), Some(scene)) = (
            self.world.is_some(),
            self.renderer.as_ref(),
            self.scene.as_mut(),
        ) {
            let mut lines: Vec<String> = Vec::new();
            let names = describe::names(
                &self.args.root,
                &::config::get_string("ui", "language").unwrap_or_else(|| "en".into()),
            );
            let tooltip = self.hover.as_ref().map(|h| names.control(h));
            if self.editor.is_some() {
                lines.push("Object editor: click picks · drag moves · wheel turns (Shift lifts) · Del · C copy · V variant · Backspace undo · Ctrl+S save · Esc".into());
            }
            if let Some(d) = self.duty.as_ref().filter(|d| d.trip_done()) {
                lines.push(match d.trips.get(d.trip_index + 1) {
                    Some(next) => format!(
                        "End of the trip. Next: {} to {}, from {} at {} (it starts by itself a minute before)",
                        if next.line.trim().is_empty() { "service trip".to_string() } else { format!("line {}", next.line) },
                        next.terminus.strip_prefix(&format!("{} ", next.line)).unwrap_or(&next.terminus),
                        next.stops.first().map(|s| s.name.trim()).unwrap_or("?"),
                        schedule::hhmm(next.departure)
                    ),
                    None => "End of the duty: the tour's last trip is done".into(),
                });
            }
            if let Some(player) = self.player.as_mut() {
                if let Some(msg) =
                    next_script_message(&player.vehicle.host, &mut player.message_serial)
                {
                    self.service_msg = Some((msg, 6.0));
                }
            }
            if let Some((msg, left)) = self.service_msg.as_mut() {
                *left -= dt;
                if *left > 0.0 {
                    lines.push(msg.clone());
                }
            }
            self.service_msg = self.service_msg.take().filter(|(_, l)| *l > 0.0);
            if let Some(lan) = self.lan.as_ref() {
                lines.extend(lan::hud_lines(lan, &self.remotes, self.player.as_ref()));
                match (self.voice.sending(), self.voice.error.as_ref()) {
                    (Some(radio), _) => lines.push(::i18n::translate(
                        if radio { "pause.msg.voice_radio" } else { "pause.msg.voice_talking" },
                        &[],
                    )),
                    (None, Some(e))
                    if ::config::get_bool("voice", "enabled").unwrap_or(true)
                        && lan.voice_allowed() =>
                        {
                            lines.push(::i18n::translate("pause.msg.voice_error", &[("error", e)]))
                        }
                    _ => {}
                }
            }
            if let Some(h) = self.humans.as_ref() {
                if let Some(hint) = h.hint() {
                    lines.push(hint);
                } else if let Some((name, value)) = &h.request {
                    lines.push(format!("Passenger wants: {name}  {value:.2}"));
                }
                if let Some((paid, value)) = h.paid {
                    lines.push(format!(
                        "paid: {paid:.2}  (change {:.2})",
                        (paid - value).max(0.0)
                    ));
                }
                if let Some(owed) = h.change_due {
                    lines.push(format!("Change due: {owed:.2}"));
                }
            }
            let __t = Instant::now();
            scene.overlays.clear();
            let notes = lines;
            if !screenshot_mode {
                if let (Some(nav), Some(p), Some(s)) = (
                    self.navigator.as_mut(),
                    self.player.as_ref(),
                    self.surface.as_ref(),
                ) {
                    let old_enabled = nav.enabled;
                    // (the setting as it is now: it can change while the game runs)
                    nav.opacity = ::config::get_float("ui", "opacity")
                        .unwrap_or(0.85)
                        .clamp(0.2, 1.0) as f32;
                    let old_opacity = nav.opacity;
                    nav.cockpit_display = vr_active;
                    if vr_active {
                        nav.enabled = vr_nav_display.is_some_and(|d| d.placement.enabled);
                        nav.opacity = vr_nav_display.map(|d| d.placement.opacity).unwrap_or(0.95);
                    }
                    if let Some(w) = self.world.as_ref() {
                        nav.start_map(
                            w.clone(),
                            glam::DVec2::new(p.vehicle.position.x, p.vehicle.position.y),
                        );
                    }
                    if let (Some(places), Some(d)) = (nav.places(), self.duty.as_mut()) {
                        if !self.duty_places {
                            self.duty_places = true;
                            d.learn_places(places);
                        }
                    }
                    let (line, terminus, stops, trip) = navigator::duty_parts(self.duty.as_ref());
                    match (
                        trip,
                        self.schedule.as_ref(),
                        self.traffic.as_ref(),
                        self.world.as_ref(),
                    ) {
                        (Some((key, name)), Some(sch), _, _) if nav.map_net().is_some() => {
                            if nav.wants_route(&key, 0) {
                                let lanes = sch.trip_route_in(nav.map_net().unwrap(), &name);
                                let g = nav.global_version + (1 << 40);
                                nav.set_route(&key, lanes, true, g);
                            }
                        }
                        (Some((key, name)), Some(sch), Some(t), Some(w)) => {
                            if nav.wants_route(&key, t.lanes_generation) {
                                let (lanes, complete) = sch.trip_route(w, t, &name);
                                nav.set_route(&key, lanes, complete, t.lanes_generation);
                            }
                        }
                        _ => nav.clear_route(),
                    }
                    let (outside_temp, inside_temp) = vehicle_temperatures(p);
                    let (at, heading) = match self.on_foot.as_ref() {
                        Some(f) => (f.pos, f.heading),
                        None => (p.vehicle.position, p.vehicle.heading),
                    };
                    let ui_lang =
                        ::config::get_string("ui", "language").unwrap_or_else(|| "en".into());
                    let ui_units =
                        ::config::get_string("ui", "units").unwrap_or_else(|| "metric".into());
                    let hud_width = if triple_hud {
                        s.config.width as f32 / 3.0
                    } else {
                        s.config.width as f32
                    };
                    let hud_offset = s.config.width as f32 / 3.0;
                    let frame = navigator::NavFrame {
                        traffic: self.traffic.as_ref(),
                        bus: at,
                        heading,
                        speed_kmh: p.vehicle.physics.velocity_kmh(),
                        outside_temp,
                        inside_temp,
                        line,
                        terminus,
                        stops,
                        delay: self.duty.as_ref().map(|_| p.vehicle.host.tt_delay as f64),
                        passengers: self.humans.as_ref().map(|h| h.riding()),
                        stop_requested: navigator::stop_requested(&p.vehicle),
                        time: self.clock.time,
                        weekday: self.clock.weekday(),
                        language: &ui_lang,
                        units: &ui_units,
                        screen: if vr_active {
                            (1440.0, 1440.0)
                        } else {
                            (hud_width, s.config.height as f32)
                        },
                        ui_scale: if vr_active {
                            1.0
                        } else {
                            ::config::get_float("ui", "scale").unwrap_or(1.0) as f32
                        },
                        follow_window: if vr_active {
                            true
                        } else {
                            ::config::get_bool("ui", "scale_window").unwrap_or(true)
                        },
                        dt,
                    };
                    let __tn = Instant::now();
                    let overlay_start = scene.overlays.len();
                    nav.frame(r, scene, &frame);
                    if triple_hud {
                        for (_, rect) in &mut scene.overlays[overlay_start..] {
                            rect[0] += hud_offset;
                            rect[2] += hud_offset;
                        }
                    }
                    nav.enabled = old_enabled;
                    nav.opacity = old_opacity;
                    *self.profile.entry("hud.navigator").or_default() +=
                        __tn.elapsed().as_secs_f64();
                    if nav.arrows {
                        if let Some(w) = self.world.as_ref() {
                            let spots = nav.arrow_spots(
                                self.traffic.as_ref().map(|t| &t.net),
                                350.0,
                                &|id| w.object_positions.lock().get(&id).map(|p| (p.0, p.1[0])),
                            );
                            self.route_arrows.tick(dt, w, r, scene, &spots);
                        }
                    }
                }
            }
            if let (Some(ui), Some(s)) = (self.ui.as_mut(), self.surface.as_ref()) {
                let scale = self
                    .window
                    .as_ref()
                    .map(|w| w.scale_factor() as f32)
                    .unwrap_or(1.0);
                let (surface_width, h) = (s.config.width as f32, s.config.height as f32);
                let w = if triple_hud {
                    surface_width / 3.0
                } else {
                    surface_width
                };
                let hud_offset = surface_width / 3.0;
                self.remotes.chat.disabled = !::config::get_bool("ui", "chat").unwrap_or(true);
                let chat = (!screenshot_mode
                    && self.lan.is_some()
                    && ::config::get_bool("ui", "chat").unwrap_or(true))
                    .then(|| ui::ChatView {
                        lines: &self.remotes.chat.lines,
                        typing: self.remotes.chat.typing.as_deref(),
                        error: self.remotes.chat.error(),
                    });
                ui.chat.hidden = self.remotes.chat.hidden;
                let triple_views = if triple_projection {
                    self.camera.as_ref().and_then(|camera| {
                        camera_util::triple_screen_cameras(
                            camera,
                            s.config.width,
                            s.config.height,
                        )
                    })
                } else {
                    None
                };
                let speaking = |id| self.voice.speaking(id);
                let tags =
                    if !screenshot_mode && ::config::get_bool("ui", "name_tags").unwrap_or(true) {
                        self.camera.as_ref().map_or_else(Vec::new, |camera| {
                            match triple_views.as_ref() {
                                Some(views) if triple_hud => lan::name_tags_with_projection(
                                    &self.remotes,
                                    &views.cameras[1],
                                    w,
                                    h,
                                    Some(views.projections[1]),
                                    &speaking,
                                ),
                                Some(views) => {
                                    let panel_width = surface_width / 3.0;
                                    let mut tags = Vec::new();
                                    for panel in 0..3 {
                                        tags.extend(
                                            lan::name_tags_with_projection(
                                                &self.remotes,
                                                &views.cameras[panel],
                                                panel_width,
                                                h,
                                                Some(views.projections[panel]),
                                                &speaking,
                                            )
                                                .into_iter()
                                                .filter(|((x, y), ..)| {
                                                    *x >= 0.0
                                                        && *x < panel_width
                                                        && *y >= 0.0
                                                        && *y <= h
                                                })
                                                .map(|((x, y), name, sub, alpha, talking)| {
                                                    (
                                                        (x + panel_width * panel as f32, y),
                                                        name,
                                                        sub,
                                                        alpha,
                                                        talking,
                                                    )
                                                }),
                                        );
                                    }
                                    tags
                                }
                                None => lan::name_tags(&self.remotes, camera, w, h, &speaking),
                            }
                        })
                    } else {
                        Vec::new()
                    };
                let (cx, cy) = self.cursor;
                let ui_cursor = (cx - if triple_hud { hud_offset } else { 0.0 }, cy);
                let map_open = self.navigator.as_ref().is_some_and(|n| n.map_open());
                let covered = self.game_menu.is_some()
                    || self.vr_nav_edit.is_some()
                    || self.chooser.is_some()
                    || ui.chat.hovered
                    || map_open
                    || (!vr_active
                    && self
                    .navigator
                    .as_ref()
                    .is_some_and(|n| n.over_panel(ui_cursor.0, ui_cursor.1)));
                let dropdown = self
                    .dropdown
                    .as_ref()
                    .filter(|_| self.chooser.is_some())
                    .map(|d| ui::DropdownView {
                        row: d.row,
                        items: d.items.iter().map(|x| x.0.as_str()).collect(),
                        sel: d.sel,
                        top: d.top,
                        current: d.current,
                    });
                let chooser_sel = self.chooser;
                let (menu_kind, menu_head, menu_preview) = game_lists::menu_extras(
                    self.list_kind.as_ref(),
                    self.admin_list.as_deref(),
                    chooser_sel,
                    self.schedule.as_ref(),
                    self.clock.time,
                );
                let report_view = self.report_view.as_ref().map(|r| r.view());
                let frame = ui::Frame {
                    scale,
                    ui_scale: ui::size_factor(
                        h,
                        scale,
                        ::config::get_float("ui", "scale").unwrap_or(1.0) as f32,
                        ::config::get_bool("ui", "scale_window").unwrap_or(true),
                    ),
                    opacity: ui::backdrop(
                        ::config::get_float("ui", "opacity")
                            .unwrap_or(0.85)
                            .clamp(0.2, 1.0) as f32,
                    ),
                    width: w,
                    height: h,
                    cursor: ui_cursor,
                    vr: {
                        #[cfg(windows)]
                        {
                            self.vr.is_some()
                        }
                        #[cfg(not(windows))]
                        {
                            false
                        }
                    },
                    crosshair,
                    tooltip: if screenshot_mode {
                        None
                    } else {
                        tooltip.filter(|_| {
                            ::config::get_bool("ui", "tooltips").unwrap_or(true)
                                && !self.dragging
                                && !covered
                                && self.game_menu.is_none()
                        })
                    },
                    notes: if screenshot_mode {
                        screenshot_help.as_slice()
                    } else if ::config::get_bool("ui", "notes").unwrap_or(true)
                        && !map_open
                        && self.game_menu.is_none()
                    {
                        &notes
                    } else {
                        &[]
                    },
                    fps: (!screenshot_mode && ::config::get_bool("ui", "show_fps").unwrap_or(false)).then_some(self.fps),
                    paused: self.paused && !screenshot_mode && self.lab_menu.is_none(),
                    menu: self
                        .game_menu
                        .filter(|_| self.lab_menu.is_none() && report_view.is_some())
                        .map(|k| (k, &menu_lines[..])),
                    menu_kind,
                    report: report_view.as_ref(),
                    touch: !screenshot_mode && platform::touch_controls(),
                    build: BUILD,
                    report_status: &self.report_status,
                    menu_head,
                    menu_preview,
                    pane_first: self
                        .pane_scroll
                        .filter(|p| Some(p.0) == chooser_sel)
                        .map(|p| p.1),
                    menu_tabs,
                    dropdown,
                    menu_kbd: self.menu_kbd,
                    lab: self.lab_menu,
                    vehicle_menu: &vehicle_menu,
                    menu_top: self.menu_top,
                    timetable: (!screenshot_mode && self.timetable && !map_open)
                        .then(|| {
                            timetable_rows(
                                self.duty.as_ref(),
                                self.player.as_ref().map(|p| p.vehicle.host.tt_delay as f64),
                            )
                        })
                        .flatten(),
                    info: (!screenshot_mode && self.info_bar).then(|| {
                        info_line(
                            &self.clock,
                            self.player.as_ref(),
                            self.duty.as_ref(),
                            self.humans.as_ref().map(|h| h.riding()),
                        )
                    }),
                    tutorial: self
                        .tutorial
                        .as_ref()
                        .filter(|t| !screenshot_mode && !t.hidden && self.game_menu.is_none())
                        .and_then(|t| {
                            t.page().map(|p| {
                                (
                                    p.title.as_str(),
                                    p.text.as_str(),
                                    p.image.as_deref(),
                                    t.at,
                                    t.pages.len(),
                                )
                            })
                        }),
                    chat,
                    tags,
                };
                let overlay_start = scene.overlays.len();
                ui.draw(r, scene, &frame, dt);
                if triple_hud {
                    for (_, rect) in &mut scene.overlays[overlay_start..] {
                        rect[0] += hud_offset;
                        rect[2] += hud_offset;
                    }
                }
            }
            *self.profile.entry("hud").or_default() += __t.elapsed().as_secs_f64();
        }

        let mut lighting = match self.weather.as_ref() {
            Some(w) => {
                let enhanced = weather::enhanced_sky();
                let observer = match self.camera.as_ref() {
                    Some(c) => [c.position.x, c.position.y],
                    None => self.weather_observer(),
                };
                if !self.sky_anchored && self.camera.is_some() {
                    self.sky.anchor(w, observer, enhanced);
                    self.sky_anchored = true;
                }
                self.weather_local = self.sky.sample(w, observer, enhanced);
                self.wetness = weather::road_wetness(self.weather_local.precip_rate, dt as f64, self.wetness);
                weather::lighting(
                    &daylight,
                    w,
                    &self.sky,
                    &self.weather_local,
                    self.wetness,
                    ::config::get_bool("graphics", "shadows").unwrap_or(true),
                )
            }
            None => lights::lighting_from(&daylight, 50000.0),
        };
        {
            let h = (self.clock.date_code() as u32).wrapping_mul(2654435761);
            let h2 = h.rotate_left(13).wrapping_mul(2246822519);
            lighting.puddle_seed = [
                ((h >> 8) & 0xffff) as f32 / 65535.0 * 997.0,
                ((h2 >> 8) & 0xffff) as f32 / 65535.0 * 997.0,
            ];
        }
        lighting.wetness = ::legacy_config::env::var("OMSI_WETNESS")
            .ok()
            .and_then(|v| v.parse().ok())
            .unwrap_or(self.wetness);
        lighting.inside = match self
            .inside_remote
            .and_then(|id| self.remotes.remotes.get(&id))
        {
            Some(rv) => rv
                .vehicle()
                .ty
                .def
                .bounding_box
                .map(|bb| (rv.vehicle().position, rv.vehicle().heading, bb)),
            None => self.player.as_ref().and_then(|p| {
                p.vehicle
                    .ty
                    .def
                    .bounding_box
                    .map(|bb| (p.vehicle.position, p.vehicle.heading, bb))
            }),
        };
        let puddle_surface = lighting
            .inside
            .and_then(|(o, _, _)| self.world.as_ref().and_then(|w| w.puddle_surface(o)));
        lighting.puddle_ground = puddle_surface.map(|(h, _)| h);
        lighting.puddle_normal = puddle_surface.map_or(Vec3::Z, |(_, n)| n);
        let puddle_vehicle = self
            .inside_remote
            .and_then(|id| self.remotes.remotes.get(&id))
            .map(|rv| rv.vehicle())
            .or_else(|| self.player.as_ref().map(|p| &p.vehicle));
        lighting.puddle_parts = puddle_vehicle
            .into_iter()
            .flat_map(|v| &v.trailers)
            .filter_map(|t| t.ty.def.bounding_box.map(|bb| (t.position, t.heading, bb)))
            .take(3)
            .collect();
        lighting.detail = ::config::get_bool("graphics", "detail_textures").unwrap_or(true);
        lighting.glass_wind = self
            .player
            .as_ref()
            .map(|p| lights::vehicle_velocity(&p.vehicle))
            .unwrap_or_default()
            - self
            .weather
            .as_ref()
            .map(rain::weather_wind)
            .unwrap_or_default();
        lighting.animation_time = Some(self.clock.run_time as f32);
        lighting.led_glow =
            ::config::get_int("graphics", "led_glow").unwrap_or(6) as u8 as f32 * 0.25;
        lights::set_led_glow(lighting.led_glow);
        lighting.nightmap_glow =
            ::config::get_int("graphics", "nightmap_glow").unwrap_or(6) as u8 as f32 * 0.1875;
        lighting.led_mips = ::config::get_float("graphics", "led_mips").unwrap_or(1.3) as f32;
        lighting.atmosphere_brightness =
            ::config::get_float("graphics", "atmosphere_brightness").unwrap_or(1.0) as f32;
        lighting.html_glow = lights::screen_fx(0);
        lighting.html_light = lights::screen_fx(1);
        lighting.nightmap_gain = lights::settings().nightmap_gain;
        lighting.lightmap_gain = lights::settings().lightmap_gain;
        lighting.script_glow = lights::screen_fx(2);
        lighting.script_light = lights::screen_fx(3);
        let mut finish = false;
        let mut reconfigure = false;
        let shot = self.shot.take();
        if let Some(s) = self.surface.as_ref() {
            let (w, h) = (s.config.width, s.config.height);
            self.touch_prepare(w, h);
        }
        let mut summarize = false;
        if let (Some(s), Some(r), Some(scene), Some(cam), Some(win)) = (
            self.surface.as_ref(),
            self.renderer.as_mut(),
            self.scene.as_mut(),
            self.camera.as_ref(),
            self.window.as_ref(),
        ) {
            if let Some(path) = shot {
                match r.render_to_image(scene, s.config.width, s.config.height, cam, &lighting) {
                    Ok(mut px) => match {
                        if let Some(over) = self.touch.picture(r, s.config.width, s.config.height) {
                            touch::composite(&mut px, &over);
                        }
                        image::save_buffer(
                            &path,
                            &px,
                            s.config.width,
                            s.config.height,
                            image::ColorType::Rgba8,
                        )
                    } {
                        Ok(()) => {
                            log::info!("input script: window picture written to {}", path.display())
                        }
                        Err(e) => {
                            log::warn!("input script: {} could not be written: {e}", path.display())
                        }
                    },
                    Err(e) => {
                        log::warn!("input script: the window picture could not be rendered: {e}")
                    }
                }
            }
            let __t = Instant::now();
            let hide_test = ::legacy_config::env::var("OMSI_HIDE_WINDOW")
                .ok()
                .and_then(|v| {
                    let mut it = v.split(',').filter_map(|x| x.trim().parse::<f32>().ok());
                    Some((it.next()?, it.next()?))
                });
            let hidden_now = hide_test
                .map(|(a, b)| (a..b).contains(&self.started.elapsed().as_secs_f32()))
                .unwrap_or(false);
            let acquired = match s.surface.get_current_texture() {
                wgpu::CurrentSurfaceTexture::Success(_)
                | wgpu::CurrentSurfaceTexture::Suboptimal(_)
                if hidden_now =>
                    {
                        wgpu::CurrentSurfaceTexture::Occluded
                    }
                other => other,
            };
            let (frame, stand_in) = match acquired {
                wgpu::CurrentSurfaceTexture::Success(frame)
                | wgpu::CurrentSurfaceTexture::Suboptimal(frame) => (Some(frame), None),
                wgpu::CurrentSurfaceTexture::Occluded
                if ::legacy_config::env::var_os("OMSI_RENDER_OCCLUDED").is_some() =>
                    {
                        let (w, h) = (s.config.width, s.config.height);
                        if self
                            .stand_in
                            .as_ref()
                            .map(|t| (t.width(), t.height()) != (w, h))
                            .unwrap_or(true)
                        {
                            self.stand_in = Some(r.device.create_texture(&wgpu::TextureDescriptor {
                                label: Some("hidden window"),
                                size: wgpu::Extent3d {
                                    width: w,
                                    height: h,
                                    depth_or_array_layers: 1,
                                },
                                mip_level_count: 1,
                                sample_count: 1,
                                dimension: wgpu::TextureDimension::D2,
                                format: r.format(),
                                usage: wgpu::TextureUsages::RENDER_ATTACHMENT,
                                view_formats: &[],
                            }));
                        }
                        (
                            None,
                            self.stand_in
                                .as_ref()
                                .map(|t| t.create_view(&Default::default())),
                        )
                    }
                wgpu::CurrentSurfaceTexture::Outdated | wgpu::CurrentSurfaceTexture::Lost => {
                    reconfigure = true;
                    (None, None)
                }
                _ => (None, None),
            };
            *self.profile.entry("acquire").or_default() += __t.elapsed().as_secs_f64();
            if frame.is_none() {
                self.hidden_frames += 1;
            }
            let view = frame
                .as_ref()
                .map(|f| f.texture.create_view(&Default::default()))
                .or(stand_in);
            if let Some(view) = view {
                if camera_tool::wants() {
                    if let Some(p) = self.player.as_ref() {
                        publish_camera_info(
                            p,
                            self.camera.as_ref().map(|c| {
                                (*c, s.config.width as f32 / s.config.height.max(1) as f32)
                            }),
                        );
                    }
                }
                let __t = Instant::now();
                // (the mirrors' commands go out with the picture's, on the submit thread)
                r.set_defer_views(frame.is_some());
                if ::config::get_int("graphics", "mirror_size").unwrap_or(256) as u32 == 0 {
                    self.mirror_budget = 0.0;
                    self.mirrors_seen = 0;
                } else if ::config::get_string("graphics", "mirror_refresh")
                    .unwrap_or_else(|| "full".into())
                    == "off"
                {
                    self.mirror_budget = 0.0;
                    if let (Some(w), Some(p)) = (self.world.as_ref(), self.player.as_ref()) {
                        let since = match &self.frozen_mirrors {
                            Some(m) if m.bus == p.uid => m.since,
                            _ => -1.0,
                        };
                        let next = since.max(0.0) + raw_dt.min(0.1);
                        if since < 0.0
                            || (since < MIRROR_FREEZE_REDRAW && next >= MIRROR_FREEZE_REDRAW)
                            || p.mirrors_dirty
                        {
                            self.mirrors_seen =
                                render_mirrors(r, scene, w, p, &lighting, None, None);
                        }
                        self.frozen_mirrors = Some(FrozenMirrors {
                            bus: p.uid,
                            since: next,
                        });
                    }
                } else {
                    let mirrors = self
                        .player
                        .as_ref()
                        .map(|p| mirror_cams(&p.vehicle).len())
                        .unwrap_or(0);
                    #[cfg(windows)]
                    let vr_active = self.vr.is_some();
                    #[cfg(not(windows))]
                    let vr_active = false;
                    let rate = {
                        if vr_active {
                            ::legacy_config::env::var("OMSI_OPENXR_MIRROR_RATE")
                                .ok()
                                .and_then(|s| s.parse::<f32>().ok())
                                .filter(|rate| rate.is_finite() && *rate >= -1.0)
                                .unwrap_or(
                                    ::config::get_float("vr", "mirror-rate")
                                        .unwrap_or(16.0)
                                        .clamp(-1.0, 360.0)
                                        as f32,
                                )
                        } else {
                            let max_hz = if ::config::get_string("graphics", "mirror_refresh")
                                .unwrap_or_else(|| "full".into())
                                == "full"
                            {
                                MIRROR_MAX_HZ_FULL
                            } else {
                                MIRROR_MAX_HZ_ECO
                            };
                            MIRROR_RATE
                                .max(mirrors as f32 * MIRROR_MIN_HZ)
                                .min(max_hz * self.mirrors_seen.max(1) as f32)
                        }
                    };
                    let mirror_view = if vr_active {
                        None
                    } else {
                        Some((*cam, s.config.width as f32 / s.config.height.max(1) as f32))
                    };
                    let near = self
                        .player
                        .as_ref()
                        .zip(self.camera.as_ref())
                        .is_some_and(|(p, c)| (p.vehicle.position - c.position).length() < 12.0);
                    let draw_limit = if vr_active {
                        if self.in_cab || near {
                            vr_mirror_updates(&mut self.mirror_budget, raw_dt, rate, mirrors)
                        } else {
                            self.mirror_budget = 0.0;
                            0
                        }
                    } else {
                        self.mirror_budget = (self.mirror_budget + raw_dt.min(0.1) * rate).min(2.5);
                        self.mirrors_seen.clamp(1, 2)
                    };
                    let mut drawn = 0;
                    if vr_active && draw_limit > 0 && draw_limit == mirrors {
                        if let (Some(w), Some(p)) = (self.world.as_ref(), self.player.as_ref()) {
                            self.mirror_turn = self.mirror_turn.wrapping_add(draw_limit);
                            self.mirrors_seen =
                                render_mirrors(r, scene, w, p, &lighting, None, mirror_view);
                            drawn = draw_limit;
                        }
                    }
                    while (self.in_cab || near)
                        && drawn
                        < (if vr_active {
                        draw_limit
                    } else {
                        self.mirrors_seen.clamp(1, 2)
                    })
                        && (vr_active || self.mirror_budget >= 1.0)
                    {
                        let (Some(w), Some(p)) = (self.world.as_ref(), self.player.as_ref()) else {
                            break;
                        };
                        if !vr_active {
                            self.mirror_budget -= 1.0;
                        }
                        drawn += 1;
                        self.mirror_turn = self.mirror_turn.wrapping_add(1);
                        self.mirrors_seen = render_mirrors(
                            r,
                            scene,
                            w,
                            p,
                            &lighting,
                            Some(self.mirror_turn),
                            mirror_view,
                        );
                    }
                }
                r.set_defer_views(false);
                *self.profile.entry("mirrors").or_default() += __t.elapsed().as_secs_f64();
                let __t = Instant::now();
                #[cfg(windows)]
                let mut mirrored = false;
                #[cfg(not(windows))]
                let mirrored = false;
                #[cfg(windows)]
                if let Some(vr) = self.vr.as_mut() {
                    let menu_range = self
                        .ui
                        .as_ref()
                        .map(|u| u.menu_overlay_range.clone())
                        .unwrap_or(0..0);
                    let cursor_overlay = self
                        .ui
                        .as_ref()
                        .and_then(|u| u.vr_cursor_overlay)
                        .filter(|_| self.vr_nav_edit.is_none());
                    let tooltip_overlay = self
                        .ui
                        .as_ref()
                        .and_then(|u| u.vr_tooltip_overlay)
                        .filter(|_| self.vr_nav_edit.is_none());
                    match vr.render(
                        r,
                        scene,
                        cam,
                        &lighting,
                        &view,
                        (s.config.width, s.config.height),
                        menu_range,
                        cursor_overlay,
                        tooltip_overlay,
                        self.cursor,
                        self.player
                            .as_ref()
                            .map(|p| (p.vehicle.position, p.vehicle.body_rotation())),
                        vr_nav_display
                            .filter(|d| d.placement.enabled)
                            .and_then(|d| {
                                self.navigator
                                    .as_ref()
                                    .and_then(|n| n.panel_overlay)
                                    .map(|index| (index, d))
                            }),
                        self.player.as_ref().map(|p| p.uid),
                        ::config::get_float("vr", "head-smoothing-ms")
                            .unwrap_or(0.0)
                            .clamp(0.0, 30.0) as f32,
                        !self.mouse_drive,
                        self.vr_zoom_active,
                    ) {
                        Ok(visible) => mirrored = visible,
                        Err(e) => {
                            log::error!("OpenXR rendering stopped: {e:#}");
                            self.vr = None;
                        }
                    }
                }
                if !mirrored {
                    #[cfg(windows)]
                    let xr_active = self.vr.is_some();
                    #[cfg(not(windows))]
                    let xr_active = false;
                    if !xr_active
                        && triple_projection
                        && let Some(views) =
                        camera_util::triple_screen_cameras(
                            cam,
                            s.config.width,
                            s.config.height,
                        )
                    {
                        r.render_triple(
                            scene,
                            &view,
                            s.config.width,
                            s.config.height,
                            views.cameras,
                            views.projections,
                            &lighting,
                            ::config::get_bool("graphics", "triple_screen_hud").unwrap_or(true),
                        );
                    } else if frame.is_some() {
                        // (finished, submitted and presented on the submit thread)
                        r.render_deferred(
                            scene,
                            &view,
                            s.config.width,
                            s.config.height,
                            cam,
                            &lighting,
                        );
                    } else {
                        r.render(
                            scene,
                            &view,
                            s.config.width,
                            s.config.height,
                            cam,
                            &lighting,
                        );
                    }
                }
                self.touch.render(r, &view, s.config.width, s.config.height);
                #[cfg(all(feature = "devtools", debug_assertions))]
                {
                    let snap = devtools::Snapshot {
                        adapter: r.adapter_name.clone(),
                        format: r.format(),
                        surface: (s.config.width, s.config.height),
                        dt_ms: raw_dt * 1000.0,
                        fps: self.fps,
                        msaa: r.options.msaa,
                        anisotropy: r.options.anisotropy,
                        shadow_size: r.options.shadow_size,
                        ssao: r.options.ssao,
                        fxaa: r.options.fxaa,
                        reflections: r.options.reflections,
                        render_scale: r.options.render_scale,
                        meshes: scene.meshes.len(),
                        textures: scene.textures.len(),
                        materials: scene.materials.len(),
                        instances: scene.instances.len(),
                        lights: scene.lights.len(),
                        interior_lights: scene.interior_lights.len(),
                        coronas: scene.coronas.len(),
                        gpu_passes: r.gpu_pass_times(),
                        draw_report: r
                            .counts
                            .borrow()
                            .iter()
                            .map(|(k, v)| format!("{k}: {v:.1}"))
                            .collect(),
                        light_stats: devtools::LightStats::gather(
                            scene,
                            &lighting,
                            cam.position,
                        ),
                    };
                    let scale = self
                        .window
                        .as_ref()
                        .map_or(1.0, |w| w.scale_factor() as f32);
                    if self.lab_menu.is_none() {
                        // (drawn over the picture: its commands are queued after the
                        // picture's, see `DevTools::submit`)
                        self.devtools
                            .get_or_insert_with(devtools::DevTools::new)
                            .render(r, &view, scale, &snap, &dev_extra);
                    }
                }
                *self.profile.entry("render").or_default() += __t.elapsed().as_secs_f64();
                if ::legacy_config::env::var_os("OMSI_PROFILE_GPU").is_some() {
                    let __t = Instant::now();
                    let _ = ::render::wait_gpu(&r.device, None);
                    *self.profile.entry("gpu").or_default() += __t.elapsed().as_secs_f64();
                }
                let __t = Instant::now();
                match frame {
                    Some(frame) => {
                        if ::config::get_bool("graphics", "vsync").unwrap_or(true) {
                            win.pre_present_notify();
                        }
                        r.present(frame);
                    }
                    None => {
                        r.flush_deferred();
                        let _ = ::render::wait_gpu(&r.device, None);
                    }
                }
                *self.profile.entry("present").or_default() += __t.elapsed().as_secs_f64();
            } else {
                let __t = Instant::now();
                r.queue.submit(std::iter::empty::<wgpu::CommandBuffer>());
                let _ = r.device.poll(wgpu::PollType::Poll);
                *self.profile.entry("present").or_default() += __t.elapsed().as_secs_f64();
                if let Some(rest) = std::time::Duration::from_millis(16).checked_sub(now.elapsed())
                {
                    std::thread::sleep(rest);
                }
            }
            let max_fps = ::legacy_config::env::var("OMSI_MAX_FPS")
                .ok()
                .and_then(|v| v.parse::<u32>().ok())
                .unwrap_or(::config::get_int("graphics", "max_fps").unwrap_or(0) as u32);
            let max_fps = if max_fps == 0 {
                self.window
                    .as_ref()
                    .and_then(|w| w.current_monitor())
                    .and_then(|m| m.refresh_rate_millihertz())
                    .map(|mhz| (mhz as f64 / 1000.0).round() as u32)
                    .filter(|r| *r >= 30)
                    .unwrap_or(120)
            } else if max_fps >= 1000 {
                0
            } else {
                max_fps
            };
            #[cfg(windows)]
            let vr_active = self.vr.is_some();
            #[cfg(not(windows))]
            let vr_active = false;
            if max_fps > 0 && !vr_active {
                let __t = Instant::now();
                if let Some(rest) = std::time::Duration::from_secs_f64(1.0 / max_fps as f64)
                    .checked_sub(now.elapsed())
                {
                    std::thread::sleep(rest);
                }
                *self.profile.entry("limiter").or_default() += __t.elapsed().as_secs_f64();
            }
            self.frames += 1;
            let profiling = ::legacy_config::env::var_os("OMSI_PROFILE").is_some();
            // (half of an automated run, or 15 s after the bus is there: not the menu or loading)
            let mark_due = match self.args.exit_after {
                Some(l) => self.started.elapsed().as_secs_f32() > (l * 0.5).max(15.0),
                None => {
                    self.player.is_some()
                        && self
                        .play_since
                        .get_or_insert_with(Instant::now)
                        .elapsed()
                        .as_secs_f32()
                        > 15.0
                }
            };
            if profiling && self.cpu_mark.is_none() && self.profile_mark.is_none() && mark_due {
                self.cpu_mark =
                    process_cpu_seconds().map(|c| (c, Instant::now(), self.total_frames));
                self.thread_cpu_mark = thread_cpu_seconds();
                self.profile_mark = Some(ProfileMark {
                    frames: self.total_frames,
                    stages: self.profile.clone(),
                    render: r.stats.borrow().clone(),
                    counts: r.counts.borrow().clone(),
                    gpu: r.gpu_pass_times(),
                });
            }
            if let (Some(limit), false) = (self.args.exit_after, self.exiting) {
                if self.started.elapsed().as_secs_f32() > limit {
                    self.exiting = true;
                    log::info!(
                        "exit after {limit} s: {} frames total ({} with the window hidden{}), {:.1} fps average, {} frames over 50 ms, worst {:.0} ms",
                        self.total_frames,
                        self.hidden_frames,
                        if ::legacy_config::env::var_os("OMSI_RENDER_OCCLUDED").is_some() {
                            ", drawn off-screen"
                        } else {
                            ", not drawn"
                        },
                        self.total_frames as f32 / self.started.elapsed().as_secs_f32(),
                        self.spikes,
                        self.worst_ms
                    );
                    if let (Some(st), Some(w)) = (self.streamer.as_ref(), self.world.as_ref()) {
                        log::info!(
                            "tile streaming: {} tiles loaded now, {} loaded and {} unloaded in all, {:.1} s preparing on the worker, slowest upload {:.0} ms, streaming over 16 ms in {} frames (worst {:.0} ms); {} objects + {} trees, {} rows, {} attached ({} without parent), {} unresolved",
                            w.loaded_tiles().len(),
                            st.loaded_total,
                            st.unloaded_total,
                            st.prepare_secs,
                            st.worst_upload_ms,
                            st.slow_frames,
                            st.worst_frame_ms,
                            st.stats.objects,
                            st.stats.trees,
                            st.stats.rows,
                            st.stats.attached,
                            st.stats.unattached,
                            st.stats.failed_objects
                        );
                        st.stats.log_ground();
                    }
                    summarize = true;
                    finish = true;
                    platform::exit(event_loop);
                }
            }
            self.total_frames += 1;
            if self.fps_t.elapsed().as_secs_f32() >= 1.0 {
                if ::legacy_config::env::var_os("OMSI_PROFILE").is_some() {
                    let secs = self.fps_t.elapsed().as_secs_f32();
                    log::info!(
                        "profile interval: {:.1} fps over {secs:.2} s",
                        self.frames as f32 / secs
                    );
                }
                self.fps = self.frames as f32;
                self.frames = 0;
                self.fps_t = Instant::now();
            }
            win.request_redraw();
        }
        if summarize {
            self.log_profile_summary(false);
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
        if finish {
            self.finish_session();
        }
    }
}

/// The newest `$msg` the player has not yet seen. The serial is independent of the retained
/// message list so a script can deliberately repeat the same text and still have it shown.
impl App {
    /// OMSI_PROFILE: the stage averages since the mark, once: at the end of an automated run,
    /// or when the game closes or goes back to the main menu (`marked`: only when the mark was
    /// taken, after the loading).
    pub(crate) fn log_profile_summary(&mut self, marked: bool) {
        if ::legacy_config::env::var_os("OMSI_PROFILE").is_none()
            || (marked && self.profile_mark.is_none())
        {
            return;
        }
        let profile_mark = self.profile_mark.take();
        let cpu_mark = self.cpu_mark.take();
        let (Some(r), Some(s)) = (self.renderer.as_ref(), self.surface.as_ref()) else {
            return;
        };
        // (over the frames after the mark when there is one: the loading
        // frames' work would be spread over the steady ones)
        type Totals = std::collections::BTreeMap<&'static str, f64>;
        let empty = Totals::new();
        let (n, stages0, render0, counts0) = match &profile_mark {
            Some(m) => {
                log::info!(
                    "profile: stages over the {} frames since the mark",
                    self.total_frames.saturating_sub(m.frames)
                );
                (
                    self.total_frames.saturating_sub(m.frames).max(1) as f64,
                    &m.stages,
                    &m.render,
                    &m.counts,
                )
            }
            None => (self.total_frames.max(1) as f64, &empty, &empty, &empty),
        };
        let since = |k: &&'static str, v: f64, at: &Totals| v - at.get(k).copied().unwrap_or(0.0);
        for (k, v) in &self.profile {
            log::info!(
                "profile {k:10}: {:.2} ms/frame",
                since(k, *v, stages0) / n * 1000.0
            );
        }
        if let Some(h) = self.humans.as_ref() {
            log::info!("profile people: {} ({})", h.people.len(), h.summary());
        }
        for (k, v) in r.stats.borrow().iter() {
            log::info!(
                "profile render.{k:10}: {:.2} ms/frame",
                since(k, *v, render0) / n * 1000.0
            );
        }
        for (k, v) in r.counts.borrow().iter() {
            log::info!(
                "profile count {k}: {:.0} a frame",
                since(k, *v, counts0) / n
            );
        }
        let gpu0 = profile_mark.as_ref().map_or(&[][..], |m| &m.gpu[..]);
        for (pass, ms, frames) in gpu_since(&r.gpu_pass_times(), gpu0) {
            log::info!("profile gpu pass {pass:12}: {ms:.2} ms ({frames} frames measured)");
        }
        if let (Some((c0, t0, f0)), Some(c1)) = (cpu_mark, process_cpu_seconds()) {
            let frames = self.total_frames.saturating_sub(f0).max(1) as f64;
            log::info!(
                "profile: since the mark {:.1} ms wall and {:.1} ms CPU (all threads) per frame, {:.1} cores busy",
                t0.elapsed().as_secs_f64() / frames * 1000.0,
                (c1 - c0) / frames * 1000.0,
                (c1 - c0) / t0.elapsed().as_secs_f64().max(1e-3)
            );
            let mut busy: Vec<(String, f64)> = thread_cpu_seconds()
                .into_iter()
                .map(|(id, name, secs)| {
                    let before = self
                        .thread_cpu_mark
                        .iter()
                        .find(|m| m.0 == id)
                        .map_or(0.0, |m| m.2);
                    let name = if name.is_empty() {
                        format!("#{id}")
                    } else {
                        name
                    };
                    (name, secs - before)
                })
                .filter(|(_, secs)| *secs / frames > 0.05e-3)
                .collect();
            busy.sort_by(|a, b| b.1.total_cmp(&a.1));
            for (name, secs) in busy.iter().take(16) {
                log::info!(
                    "profile thread {name}: {:.2} ms CPU per frame",
                    secs / frames * 1000.0
                );
            }
            if busy.len() > 16 {
                log::info!(
                    "profile thread ({} more): {:.2} ms CPU per frame",
                    busy.len() - 16,
                    busy[16..].iter().map(|b| b.1).sum::<f64>() / frames * 1000.0
                );
            }
        }
        let (sw, sh) = r.scene_size(s.config.width, s.config.height);
        log::info!(
            "profile: window {}x{}, scene drawn at {sw}x{sh}, {}x MSAA",
            s.config.width,
            s.config.height,
            r.options.msaa
        );
    }
}

fn next_script_message(
    host: &::simulation::VehicleHost,
    seen: &mut u64,
) -> Option<String> {
    if host.message_serial == *seen {
        return None;
    }
    *seen = host.message_serial;
    host.messages.last().cloned()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn script_messages_are_shown_once_each_even_when_the_text_repeats() {
        let mut host = ::simulation::VehicleHost::default();
        let mut seen = 0;

        ::legacy_script::Host::message(&mut host, "Destination set");
        assert_eq!(next_script_message(&host, &mut seen).as_deref(), Some("Destination set"));
        assert_eq!(next_script_message(&host, &mut seen), None);

        ::legacy_script::Host::message(&mut host, "Destination set");
        assert_eq!(next_script_message(&host, &mut seen).as_deref(), Some("Destination set"));
    }
}

/// The GPU passes' averages over the frames measured since `mark` (the timers add up from
/// the start of the game: an earlier session and the loading stay out).
fn gpu_since(now: &[(String, f64, u32)], mark: &[(String, f64, u32)]) -> Vec<(String, f64, u32)> {
    now.iter()
        .filter_map(|(pass, ms, frames)| {
            let (ms0, frames0) = mark
                .iter()
                .find(|m| m.0 == *pass)
                .map_or((0.0, 0), |m| (m.1, m.2));
            let n = frames.checked_sub(frames0).filter(|n| *n > 0)?;
            let total = ms * *frames as f64 - ms0 * frames0 as f64;
            Some((pass.clone(), total / n as f64, n))
        })
        .collect()
}

#[cfg(test)]
mod gpu_since_tests {
    use super::gpu_since;

    #[test]
    fn a_gpu_pass_is_averaged_over_the_frames_since_the_mark() {
        let mark = vec![("main".to_string(), 10.0, 100)];
        let now = vec![
            ("main".to_string(), 6.0, 300),
            ("ssao".to_string(), 0.5, 50),
        ];
        let got = gpu_since(&now, &mark);
        // (300 frames at 6 ms, of which the 100 before the mark at 10 ms: 200 at 4 ms)
        assert_eq!(got[0], ("main".to_string(), 4.0, 200));
        assert_eq!(got[1], ("ssao".to_string(), 0.5, 50));
        assert!(gpu_since(&mark, &mark).is_empty());
    }
}
