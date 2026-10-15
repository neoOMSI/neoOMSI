use super::*;

impl App {
    pub(super) fn start_bus_preview(&mut self, dt: f32) {
        let want = self.menu.as_ref().filter(|m| m.page == StartPage::Start && matches!(m.sect, 0 | 2)).and_then(|m| {
            let bus = m.vehicles.get(m.vehicle)?.1.clone();
            let paint = match m.livery {
                0 => String::new(),
                i => m.liveries.get(i - 1).cloned().unwrap_or_default(),
            };
            let map = m.maps.get(m.map).map(|x| x.1.clone()).unwrap_or_else(|| self.args.map.clone());
            Some((bus, paint, map, m.weather_file(), m.hour * 60 + m.minute))
        });
        let size = self.ui.as_ref().map(|u| u.start.pic_size).filter(|s| s.0 > 0 && s.1 > 0);
        let (Some((bus, paint, map, weather, time)), Some((w, h))) = (want, size) else {
            if self.menu.as_ref().is_none_or(|m| m.page != StartPage::Start) {
                self.lab_room = None;
                if let (Some((id, _, _)), Some(r), Some(scene)) = (self.lab_pic.take(), self.renderer.as_ref(), self.scene.as_mut()) {
                    r.free_texture(scene, id);
                }
                if let Some(u) = self.ui.as_mut() {
                    u.start.picture = None;
                }
            }
            return;
        };
        let look = crate::showcase::Look {
            root: self.args.root.clone(),
            map,
            bus,
            paint,
            weather,
            time,
            date: String::new(),
        };
        let room = self.lab_room.get_or_insert_with(|| {
            let mut room = crate::showcase::Showcase::new();
            room.zoom_by(1.15);
            room
        });
        room.want(look);
        let (Some(r), Some(scene), Some(ui)) = (self.renderer.as_mut(), self.scene.as_mut(), self.ui.as_mut()) else {
            return;
        };
        room.update(r, dt);
        if !room.has_scene() {
            ui.start.picture = None;
            ui.start.status = match room.error.as_ref() {
                Some(e) => format!("{}: {e}", ::i18n::translate("start.maps.nopreview", &[])),
                None => String::new(),
            };
            if room.error.is_some() {
                ui.start.picture = None;
            }
            return;
        }
        ui.start.status.clear();
        if let Some((pw, ph, rgba)) = room.snapshot(r, w, h) {
            let id = match self.lab_pic {
                Some((id, tw, th)) if (tw, th) == (pw, ph) => id,
                old => {
                    if let Some((id, _, _)) = old {
                        r.free_texture(scene, id);
                    }
                    let id = r.add_blank_texture(scene, pw, ph);
                    self.lab_pic = Some((id, pw, ph));
                    id
                }
            };
            r.update_texture(scene, id, &::texture::Image { width: pw, height: ph, rgba, has_alpha: true });
        }
        ui.start.picture = self.lab_pic.map(|p| p.0);
    }

    pub(crate) fn start_frame(&mut self, event_loop: &ActiveEventLoop, dt: f32) {
        if let Some(m) = self.menu.as_mut().filter(|m| m.page == StartPage::Multiplayer) {
            if m.mp_tab == 1 {
                for s in &m.servers {
                    m.probe(&s.1, 20.0);
                }
            }
        }
        let pending = self.menu.as_mut().filter(|m| m.join_pending).and_then(|m| {
            let addr = m.address.trim().to_string();
            match m.probe_of(&addr) {
                Some(Probe::Checking) => None,
                Some(Probe::Online(_)) => {
                    m.join_pending = false;
                    Some(addr)
                }
                _ => {
                    m.join_pending = false;
                    None
                }
            }
        });
        if let Some(addr) = pending {
            self.start_launch(event_loop, None, Some(addr));
            return;
        }
        if self.menu.as_ref().is_some_and(|m| m.page == StartPage::Options) {
            self.lab_entries_sync();
        }
        let root = self.args.root.clone();
        if let Some(m) = self.menu.as_mut().filter(|m| m.page == StartPage::Start) {
            m.refresh_options(&root);
        }
        if let Some(m) = self.menu.as_mut().filter(|m| m.page == StartPage::Start && m.route) {
            m.refresh_lines(&root);
            m.fix_filters();
        }
        if self.menu.is_some() {
            let hwnd = self.window.as_deref().and_then(controllers::window_handle);
            let focused = self.window_focused;
            let ctl = self
                .controllers
                .get_or_insert_with(|| controllers::Controllers::new(&root, hwnd));
            ctl.set_focus(focused);
            let _ = ctl.poll();
            ctl.actions.clear();
            if let Some((device, n)) = ctl.pressed.take().filter(|_| self.key_capture.is_some()) {
                self.capture_button(&device, n);
            }
        }
        self.start_update_tick();
        self.start_bus_preview(dt);
        self.start_route_preview();
        self.start_slider_drag();
        let cursor = self.ui_cursor();
        let caret = self.started.elapsed().as_secs_f32() % 1.0 < 0.55;
        let update = self.start_update_view();
        let (Some(m), Some(ui), Some(s), Some(r), Some(scene), Some(win)) = (
            self.menu.as_ref(),
            self.ui.as_mut(),
            self.surface.as_ref(),
            self.renderer.as_mut(),
            self.scene.as_mut(),
            self.window.as_ref(),
        ) else {
            return;
        };
        let scale = win.scale_factor() as f32;
        let (w, h) = (s.config.width as f32, s.config.height as f32);
        let names = m.map_names();
        let vnames = m.vehicle_names();
        let veh_rows = m.veh_rows();
        let (livery, depot, fleet) = (m.livery_text(), m.depot_text(), m.number_text());
        let rows = m.rows();
        let (time_text, cal_title, cal_cells) = (m.time_text(), m.cal_title(), m.cal_cells());
        let drop_items = if m.drop != 0 { m.drop_items(m.drop) } else { Vec::new() };
        let drop_dim = m.drop_dim(m.drop);
        let (weather_rows, custom_rows) = (m.weather_rows(), m.custom_rows());
        let icao_rows = m.icao_matches(5);
        let icao_status = m.icao_status();
        let custom_fracs = m.custom_fracs();
        let line_rows: Vec<LineRow> = m
            .line_rows()
            .into_iter()
            .map(|(_, name, title)| LineRow { badge: name, label: title })
            .collect();
        let line_sel = m.line_rows().iter().position(|r| r.0 == m.line);
        let tour_rows: Vec<TourRow> = m
            .tour_rows()
            .into_iter()
            .map(|(_, label, span)| TourRow { label, span })
            .collect();
        let tour_sel = m.tour_rows().iter().position(|r| r.0 == m.tour);
        let trip_text = m.trip_text();
        let entry_text = m.entry_text();
        let entry_rows = m.entry_rows();
        let entry_names: Vec<String> = entry_rows.iter().map(|r| r.1.clone()).collect();
        let entry_sel = entry_rows.iter().position(|r| r.0 == m.entry_shown());
        let route_sub = match m.chosen_tour() {
            Some((l, t)) if m.route && !m.trip_text().is_empty() => ::i18n::translate(
                "start.maps.linetourtrip",
                &[("line", &l.name), ("tour", &t.number), ("trip", &m.trip_text())],
            ),
            Some((l, t)) if m.route => {
                ::i18n::translate("start.maps.linetour", &[("line", &l.name), ("tour", &t.number)])
            }
            _ => String::new(),
        };
        let (addr_state, addr_note) = probe_text(m.probe_of(&m.address), false);
        let server_state: Vec<(u8, String)> = m.servers.iter().map(|s| probe_text(m.probe_of(&s.1), true)).collect();
        let profile = profile::load(m.page == StartPage::Profile);
        let drivers = ui::DriverMenu {
            open: m.drv_open,
            names: m.drv_names.clone(),
            current: m.drv_names.iter().position(|n| n.eq_ignore_ascii_case(&profile.name)),
            new: m.drv_new.clone(),
            confirm: m.drv_confirm,
            note: m.drv_note.clone(),
        };
        let view = StartView {
            update: &update,
            profile: &profile,
            drivers: &drivers,
            page: m.page,
            sel: m.sel,
            can_continue: m.continue_sit.is_some(),
            maps: &names,
            map: m.map,
            map_top: m.map_top,
            rows: &rows,
            row: m.row,
            zone: m.zone,
            address: &m.address,
            addr_sel: m.addr_sel,
            addr_focus: m.addr_focus,
            caret,
            mp_tab: m.mp_tab,
            host: m.host,
            servers: &m.servers,
            server_top: m.server_top,
            server_state: &server_state,
            addr_state,
            addr_note: &addr_note,
            maker: m.vehicle_meta.get(m.vehicle).map_or("", |x| x.0.as_str()),
            vehicle: m.vehicle,
            vehicles: m.vehicles.len(),
            free: !m.route,
            on_foot: m.on_foot,
            lines: &line_rows,
            line_sel,
            line_top: m.line_top,
            line_query: &m.line_query,
            tours: &tour_rows,
            tour_sel,
            tour_top: m.tour_top,
            tour_query: &m.tour_query,
            trip_text: &trip_text,
            route_sub: &route_sub,
            entries: &entry_names,
            entry_text: &entry_text,
            entry_sel,
            entry_top: m.entry_top,
            vehicle_names: &vnames,
            veh_rows: &veh_rows,
            veh_top: m.veh_top,
            query: &m.veh_query,
            drop: m.drop,
            drop_items: &drop_items,
            drop_dim: &drop_dim,
            drop_sel: m.drop_sel(m.drop),
            drop_top: m.drop_top,
            edit: m.edit,
            details: m.details,
            livery: &livery,
            depot: &depot,
            fleet: &fleet,
            plate: &m.plate,
            sect: m.sect,
            focus: m.focus,
            season: m.season_mode,
            time_text: &time_text,
            cal_open: m.cal_open,
            cal_title: &cal_title,
            cal_cells: &cal_cells,
            weathers: &weather_rows,
            weather: m.weather,
            weather_top: m.weather_top,
            custom: &custom_rows,
            icao: &m.icao,
            metar_live: m.metar_live,
            custom_fracs: &custom_fracs,
            icao_rows: &icao_rows,
            icao_status: &icao_status,
            icao_top: m.icao_top,
        };
        let frame = ui::Frame::bare(
            scale,
            ui::size_factor(
                h,
                scale,
                ::config::get_float("ui", "scale").unwrap_or(1.0) as f32,
                ::config::get_bool("ui", "scale_window").unwrap_or(true),
            ),
            ui::backdrop(::config::get_float("ui", "opacity").unwrap_or(0.85).clamp(0.2, 1.0) as f32),
            w,
            h,
            cursor,
        );
        scene.overlays.clear();
        ui.draw_start(r, scene, &frame, dt, &view);
        let hand = u8::from(ui.start.hand);
        if let wgpu::CurrentSurfaceTexture::Success(frame) | wgpu::CurrentSurfaceTexture::Suboptimal(frame) =
            s.surface.get_current_texture()
        {
            let view = frame.texture.create_view(&Default::default());
            let cam = Camera {
                position: DVec3::ZERO,
                yaw: 0.0,
                pitch: 0.0,
                roll: 0.0,
                fov_deg: 60.0,
                near: 0.5,
                far: 100.0,
            };
            let lighting = ::render::Lighting {
                sky_color: Vec3::new(0.04, 0.05, 0.07),
                ..Default::default()
            };
            r.render(scene, &view, s.config.width, s.config.height, &cam, &lighting);
            win.pre_present_notify();
            r.queue.present(frame);
        }
        self.set_cursor_kind(hand);
    }
}

fn probe_text(p: Option<Probe>, short: bool) -> (u8, String) {
    match p {
        None => (0, String::new()),
        Some(Probe::Checking) => (1, ::i18n::translate("start.join.checking", &[])),
        Some(Probe::Online(i)) => {
            let (pl, mx) = (i.players.to_string(), i.max_players.to_string());
            if short {
                (2, format!("{pl}/{mx}"))
            } else {
                (2, ::i18n::translate("start.join.online", &[("map", &i.map), ("players", &pl), ("max", &mx)]))
            }
        }
        Some(Probe::Offline(e)) => {
            if short {
                (3, ::i18n::translate("start.join.offline_short", &[]))
            } else {
                (3, ::i18n::translate("start.join.offline", &[("error", &e)]))
            }
        }
    }
}

impl App {
    pub(super) fn start_slider_drag(&mut self) {
        let cursor = self.ui_cursor();
        let Some(k) = self.menu.as_ref().and_then(|m| m.slider) else {
            return;
        };
        if k == usize::MAX {
            let bx = self.ui.as_ref().map_or([0.0; 4], |u| u.start.drop_box);
            if let Some(m) = self.menu.as_mut().filter(|m| m.drop != 0 && bx[3] > bx[1]) {
                let max = m.drop_items(m.drop).len().saturating_sub(6);
                let t = ((cursor.1 - bx[1]) / (bx[3] - bx[1])).clamp(0.0, 1.0);
                m.drop_top = (t * max as f32).round() as usize;
            }
            return;
        }
        let rc = self.ui.as_ref().and_then(|u| u.start.sliders.get(k).copied());
        if let (Some(rc), Some(m)) = (rc, self.menu.as_mut()) {
            if rc[2] > rc[0] {
                m.custom_set(k, (cursor.0 - rc[0]) / (rc[2] - rc[0]));
            }
        }
    }
}
