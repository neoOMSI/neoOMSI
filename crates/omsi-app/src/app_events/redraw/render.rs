//! The interface and the picture of a frame.

use super::*;

impl App {
    /// The interface, the picture itself, screenshots and the end of the session.
    pub(super) fn redraw_render(
        &mut self,
        event_loop: &ActiveEventLoop,
        f: &Frame,
        daylight: omsi_sim::Daylight,
    ) {
        let Frame {
            now, raw_dt, dt, ..
        } = *f;
        let menu_lines = if self.game_menu.is_some() {
            self.game_menu_items()
        } else {
            Vec::new()
        };
        let vr_nav_display = self.vr_nav_display();
        let vr_active = self.vr_active();
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
            if self.paused {
                lines.push(ui::PAUSE_NOTICE.into());
            }
            let names = describe::names(&self.args.root, &self.settings.language);
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
            if let Some((msg, left)) = self.service_msg.as_mut() {
                *left -= dt;
                if *left > 0.0 {
                    lines.push(msg.clone());
                }
            }
            self.service_msg = self.service_msg.take().filter(|(_, l)| *l > 0.0);
            if let Some(lan) = self.lan.as_ref() {
                lines.extend(lan::hud_lines(lan, &self.remotes, self.player.as_ref()));
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
            if let (Some(nav), Some(p), Some(s)) = (
                self.navigator.as_mut(),
                self.player.as_ref(),
                self.surface.as_ref(),
            ) {
                let old_enabled = nav.enabled;
                let old_opacity = nav.opacity;
                nav.cockpit_display = vr_active;
                if vr_active {
                    nav.enabled = vr_nav_display.is_some_and(|d| d.placement.enabled);
                    nav.opacity = vr_nav_display.map(|d| d.placement.opacity).unwrap_or(0.95);
                }
                if let Some(w) = self.world.as_ref() {
                    nav.start_map(w.clone());
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
                    language: &self.settings.language,
                    units: &self.settings.units,
                    screen: if vr_active {
                        (1440.0, 1440.0)
                    } else {
                        (s.config.width as f32, s.config.height as f32)
                    },
                    ui_scale: if vr_active {
                        1.0
                    } else {
                        self.settings.ui_scale
                    },
                    follow_window: if vr_active {
                        true
                    } else {
                        self.settings.ui_scale_window
                    },
                    dt,
                };
                let __tn = Instant::now();
                nav.frame(r, scene, &frame);
                nav.enabled = old_enabled;
                nav.opacity = old_opacity;
                *self.profile.entry("hud.navigator").or_default() += __tn.elapsed().as_secs_f64();
                if nav.arrows {
                    if let Some(w) = self.world.as_ref() {
                        let spots =
                            nav.arrow_spots(self.traffic.as_ref().map(|t| &t.net), 350.0, &|id| {
                                w.object_positions.lock().get(&id).map(|p| (p.0, p.1[0]))
                            });
                        self.route_arrows.tick(dt, w, r, scene, &spots);
                    }
                }
            }
            if let (Some(ui), Some(s)) = (self.ui.as_mut(), self.surface.as_ref()) {
                let scale = self
                    .window
                    .as_ref()
                    .map(|w| w.scale_factor() as f32)
                    .unwrap_or(1.0);
                let (w, h) = (s.config.width as f32, s.config.height as f32);
                self.remotes.chat.disabled = !self.settings.chat;
                let chat = (self.lan.is_some() && self.settings.chat).then(|| ui::ChatView {
                    lines: &self.remotes.chat.lines,
                    typing: self.remotes.chat.typing.as_deref(),
                    error: self.remotes.chat.error(),
                });
                ui.chat.hidden = self.remotes.chat.hidden;
                let tags = if self.settings.name_tags {
                    self.camera
                        .as_ref()
                        .map(|c| lan::name_tags(&self.remotes, c, w, h))
                        .unwrap_or_default()
                } else {
                    Vec::new()
                };
                let (cx, cy) = self.cursor;
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
                            .is_some_and(|n| n.over_panel(cx, cy)));
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
                let chooser_list = self.admin_list.as_ref().unwrap_or(&self.vehicle_list);
                let (chooser_items, chooser_sel): (Vec<(&str, &str)>, Option<usize>) =
                    match self.chooser {
                        Some(sel) => {
                            let items = chooser_list
                                .iter()
                                .map(|(name, path)| (path.as_str(), name.as_str()))
                                .collect();
                            (items, Some(sel))
                        }
                        None => (Vec::new(), None),
                    };
                let menu_disabled: &[&str] = &[];
                let (menu_kind, menu_head, menu_preview) = game_lists::menu_extras(
                    self.list_kind.as_ref(),
                    self.admin_list.as_deref(),
                    chooser_sel,
                    self.schedule.as_ref(),
                    self.clock.time,
                );
                let frame = ui::Frame {
                    scale,
                    ui_scale: ui::size_factor(
                        h,
                        scale,
                        self.settings.ui_scale,
                        self.settings.ui_scale_window,
                    ),
                    opacity: ui::backdrop(self.settings.ui_opacity),
                    width: w,
                    height: h,
                    cursor: self.cursor,
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
                    tooltip: tooltip.filter(|_| {
                        self.settings.tooltips
                            && !self.dragging
                            && !covered
                            && self.game_menu.is_none()
                    }),
                    notes: if self.settings.notes && !map_open && self.game_menu.is_none() {
                        &notes
                    } else {
                        &[]
                    },
                    fps: self.settings.show_fps.then_some(self.fps),
                    paused: self.paused,
                    menu: match chooser_sel {
                        Some(k) => Some((k, &chooser_items[..])),
                        None => self.game_menu.map(|k| (k, &menu_lines[..])),
                    },
                    menu_disabled,
                    menu_kind,
                    menu_head,
                    menu_preview,
                    pane_first: self
                        .pane_scroll
                        .filter(|p| Some(p.0) == chooser_sel)
                        .map(|p| p.1),
                    menu_tabs,
                    dropdown,
                    menu_kbd: self.menu_kbd,
                    menu_top: self.menu_top,
                    timetable: (self.timetable && !map_open)
                        .then(|| {
                            timetable_rows(
                                self.duty.as_ref(),
                                self.player.as_ref().map(|p| p.vehicle.host.tt_delay as f64),
                            )
                        })
                        .flatten(),
                    info: self.info_bar.then(|| {
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
                        .filter(|t| !t.hidden && self.game_menu.is_none())
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
                ui.draw(r, scene, &frame, dt);
            }
            *self.profile.entry("hud").or_default() += __t.elapsed().as_secs_f64();
        }

        let mut lighting = match self.weather.as_ref() {
            Some(w) => {
                self.wetness = road_wetness(precip_of(w).1, dt as f64, self.wetness);
                weather_lighting(
                    &daylight,
                    w,
                    self.cloud_drift,
                    self.wetness,
                    self.settings.shadows,
                )
            }
            None => lights::lighting_from(&daylight, 50000.0),
        };
        lighting.wetness = omsi_cfg::env::var("OMSI_WETNESS")
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
        lighting.detail = self.settings.detail_textures;
        lighting.glass_wind = self
            .player
            .as_ref()
            .map(|p| lights::vehicle_velocity(&p.vehicle))
            .unwrap_or_default()
            - self
                .weather
                .as_ref()
                .map(crate::rain::weather_wind)
                .unwrap_or_default();
        lighting.animation_time = Some(self.clock.run_time as f32);
        lighting.led_glow = self.settings.led_glow as f32 * 0.25;
        lighting.led_mips = self.settings.led_mips;
        lighting.atmosphere_brightness = self.settings.atmosphere_brightness;
        let mut finish = false;
        let mut reconfigure = false;
        let shot = self.shot.take();
        if let Some(s) = self.surface.as_ref() {
            let (w, h) = (s.config.width, s.config.height);
            self.touch_prepare(w, h);
        }
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
            // A window that is hidden (another app covers it, another Space) gets
            // no frames on macOS. OMSI_RENDER_OCCLUDED=1 draws them into a texture
            // of the window's size anyway and waits for the GPU as a present would,
            // so frame times can be measured with the window out of sight.
            let __t = Instant::now();
            // OMSI_HIDE_WINDOW=from,to: treat the window as hidden between these
            // seconds of the session (the frame is acquired and dropped unshown), to
            // check the hidden-window path without covering the window by hand
            let hide_test = omsi_cfg::env::var("OMSI_HIDE_WINDOW").ok().and_then(|v| {
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
                    if omsi_cfg::env::var_os("OMSI_RENDER_OCCLUDED").is_some() =>
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
                let __t = Instant::now();
                // One mirror a turn, in turn, at most MIRROR_RATE pictures a second in
                // all: a mirror costs half the main picture's CPU time, and at 140 fps
                // five mirrors were each redrawn 28 times a second, a small picture
                // that nobody can tell from 15.
                // Every mirror at least MIRROR_MIN_HZ, though: with eight of them (the
                // Procity) at 25 fps each was redrawn three times a second, and the
                // street jerked past in them - up to two a frame then (each costs a
                // few milliseconds of the frame).
                if self.settings.mirror_size == 0 {
                    self.mirror_budget = 0.0;
                    self.mirrors_seen = 0;
                } else if self.settings.mirror_refresh == "off" {
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
                        .map(|p| p.vehicle.ty.def.cameras_reflexion.len())
                        .unwrap_or(0);
                    #[cfg(windows)]
                    let vr_active = self.vr.is_some();
                    #[cfg(not(windows))]
                    let vr_active = false;
                    let rate = {
                        if vr_active {
                            // Preserve the user's total redraw budget. A negative
                            // value explicitly requests every mirror each frame.
                            omsi_cfg::env::var("OMSI_OPENXR_MIRROR_RATE")
                                .ok()
                                .and_then(|s| s.parse::<f32>().ok())
                                .filter(|rate| rate.is_finite() && *rate >= -1.0)
                                .unwrap_or(self.settings.vr_mirror_rate)
                        } else {
                            let max_hz = if self.settings.mirror_refresh == "full" {
                                MIRROR_MAX_HZ_FULL
                            } else {
                                MIRROR_MAX_HZ_ECO
                            };
                            MIRROR_RATE
                                .max(mirrors as f32 * MIRROR_MIN_HZ)
                                .min(max_hz * self.mirrors_seen.max(1) as f32)
                        }
                    };
                    // The desktop camera does not follow the headset. Culling by
                    // its frustum can leave a mirror visible in VR uninitialised
                    // (black). Refresh all bus mirrors in VR, still taking turns
                    // within the configured budget; keep desktop visibility culling.
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
                        self.settings.vr_head_smoothing_ms,
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
                    r.render(
                        scene,
                        &view,
                        s.config.width,
                        s.config.height,
                        cam,
                        &lighting,
                    );
                }
                self.touch.render(r, &view, s.config.width, s.config.height);
                *self.profile.entry("render").or_default() += __t.elapsed().as_secs_f64();
                if omsi_cfg::env::var_os("OMSI_PROFILE_GPU").is_some() {
                    let __t = Instant::now();
                    let _ = omsi_render::wait_gpu(&r.device, None);
                    *self.profile.entry("gpu").or_default() += __t.elapsed().as_secs_f64();
                }
                let __t = Instant::now();
                match frame {
                    Some(frame) => {
                        // (without V-sync max_fps paces the frames: waiting for the compositor's frame callback cost a missed refresh each slow frame)
                        if self.settings.vsync {
                            win.pre_present_notify();
                        }
                        r.queue.present(frame);
                    }
                    None => {
                        let _ = omsi_render::wait_gpu(&r.device, None);
                    }
                }
                *self.profile.entry("present").or_default() += __t.elapsed().as_secs_f64();
            } else {
                // Nothing to draw into (a hidden window): the simulation goes on at
                // a display's pace instead of spinning a core a thousand times a
                // second. What it uploaded (traffic instances, streamed tiles,
                // people, the navigator) waits in wgpu's staging buffers until the
                // next submit, so submit nothing to let them go: without it a hidden
                // window on Ahlheim grew by 100 MB a second (5.6 GB after 55 s).
                let __t = Instant::now();
                r.queue.submit(std::iter::empty::<wgpu::CommandBuffer>());
                let _ = r.device.poll(wgpu::PollType::Poll);
                *self.profile.entry("present").or_default() += __t.elapsed().as_secs_f64();
                if let Some(rest) = std::time::Duration::from_millis(16).checked_sub(now.elapsed())
                {
                    std::thread::sleep(rest);
                }
            }
            let max_fps = omsi_cfg::env::var("OMSI_MAX_FPS")
                .ok()
                .and_then(|v| v.parse::<u32>().ok())
                .unwrap_or(self.settings.max_fps);
            // 0 = the screen's refresh rate: frames the screen never shows only heat the
            // machine (with V-sync off and no limit an M4 drew 300 frames a second in the
            // depot and ran hot); 1000 and more = no limit at all
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
            let profiling = omsi_cfg::env::var_os("OMSI_PROFILE").is_some();
            if profiling && self.cpu_mark.is_none() && self.started.elapsed().as_secs_f32() > 15.0 {
                self.cpu_mark =
                    process_cpu_seconds().map(|c| (c, Instant::now(), self.total_frames));
            }
            if let (Some(limit), false) = (self.args.exit_after, self.exiting) {
                if self.started.elapsed().as_secs_f32() > limit {
                    self.exiting = true;
                    log::info!(
                        "exit after {limit} s: {} frames total ({} with the window hidden{}), {:.1} fps average, {} frames over 50 ms, worst {:.0} ms",
                        self.total_frames,
                        self.hidden_frames,
                        if omsi_cfg::env::var_os("OMSI_RENDER_OCCLUDED").is_some() {
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
                    if omsi_cfg::env::var_os("OMSI_PROFILE").is_some() {
                        let n = self.total_frames.max(1) as f64;
                        for (k, v) in &self.profile {
                            log::info!("profile {k:10}: {:.1} ms/frame", v / n * 1000.0);
                        }
                        if let Some(h) = self.humans.as_ref() {
                            log::info!("profile people: {} ({})", h.people.len(), h.summary());
                        }
                        for (k, v) in r.stats.borrow().iter() {
                            log::info!("profile render.{k:10}: {:.2} ms/frame", v / n * 1000.0);
                        }
                        for (k, v) in r.counts.borrow().iter() {
                            log::info!("profile count {k}: {:.0} a frame", v / n);
                        }
                        for (pass, ms, frames) in r.gpu_pass_times() {
                            log::info!(
                                "profile gpu pass {pass:12}: {ms:.2} ms ({frames} frames measured)"
                            );
                        }
                        if let (Some((c0, t0, f0)), Some(c1)) =
                            (self.cpu_mark, process_cpu_seconds())
                        {
                            let frames = self.total_frames.saturating_sub(f0).max(1) as f64;
                            log::info!(
                                "profile: since 15 s {:.1} ms wall and {:.1} ms CPU (all threads) per frame, {:.1} cores busy",
                                t0.elapsed().as_secs_f64() / frames * 1000.0,
                                (c1 - c0) / frames * 1000.0,
                                (c1 - c0) / t0.elapsed().as_secs_f64().max(1e-3)
                            );
                        }
                        let (sw, sh) = r.scene_size(s.config.width, s.config.height);
                        log::info!(
                            "profile: window {}x{}, scene drawn at {sw}x{sh}, {}x MSAA",
                            s.config.width,
                            s.config.height,
                            r.options.msaa
                        );
                    }
                    finish = true;
                    platform::exit(event_loop);
                }
            }
            self.total_frames += 1;
            if self.fps_t.elapsed().as_secs_f32() >= 1.0 {
                if omsi_cfg::env::var_os("OMSI_PROFILE").is_some() {
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
