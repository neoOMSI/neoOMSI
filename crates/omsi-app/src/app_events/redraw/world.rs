//! LAN, people, on-foot play, plugins of a frame.

use super::*;

impl App {
    /// LAN, the people and the player on foot, the host's map edits, plugins and watched variables.
    pub(super) fn redraw_world(&mut self, event_loop: &ActiveEventLoop, f: &Frame) {
        let Frame { dt, .. } = *f;
        let __t = Instant::now();
        self.tick_lan(dt);
        *self.profile.entry("lan").or_default() += __t.elapsed().as_secs_f64();
        self.tick_on_foot(if self.paused { 0.0 } else { dt });
        self.sync_remote_walkers();
        let __t = Instant::now();
        if let (Some(h), Some(w), Some(r), Some(scene)) = (
            self.humans.as_mut(),
            self.world.as_ref(),
            self.renderer.as_ref(),
            self.scene.as_mut(),
        ) {
            let center = self
                .player
                .as_ref()
                .map(|p| p.vehicle.position)
                .or(self.camera.as_ref().map(|c| c.position))
                .unwrap_or(DVec3::ZERO);
            if h.stop_targets.is_none() {
                h.stop_targets = self.schedule.as_ref().map(|s| s.stop_targets());
                h.stop_names = self.schedule.as_ref().map(|s| s.stop_names());
                if let Some(t) = &h.stop_targets {
                    log::info!("people: {} bus stops with timetable targets", t.len());
                }
            }
            h.driver_away = self.on_foot.as_ref().is_some_and(|f| {
                let own = Some(humans::BusId::Player);
                f.seat.map(|s| s.0) != own && f.inside.map(|i| i.0) != own
            });
            h.density = w
                .global
                .passenger_density((self.clock.time / 3600.0) as f32)
                * self.settings.pax_density;
            h.time_of_day = self.clock.time;
            h.delay = self
                .duty
                .as_ref()
                .map(|d| d.delay(self.clock.time))
                .unwrap_or(0.0);
            self.humans_populate_t -= dt;
            if self.humans_populate_t <= 0.0 && !self.paused {
                self.humans_populate_t = 2.0;
                h.populate(w, r, scene, center);
            }
            if let (Some(cam), Some(s)) = (self.camera.as_ref(), self.surface.as_ref()) {
                h.eye = Some(humans::Eye::of(
                    cam,
                    s.config.width as f32 / s.config.height.max(1) as f32,
                ));
            }
            h.set_remote_buses(
                self.remotes
                    .remotes
                    .iter()
                    .map(|(id, r)| (*id, r.vehicle())),
            );
            h.set_placed_buses(self.placed.iter().map(|q| (q.uid, &q.vehicle)));
            let took = h.tick(
                if self.paused { 0.0 } else { dt },
                w,
                self.player.as_ref().map(|p| &p.vehicle),
                self.traffic.as_ref(),
                r,
                scene,
            );
            for bus in h.take_stamped() {
                match bus {
                    None => {
                        if let Some(p) = self.player.as_mut() {
                            p.vehicle.host.fired_triggers.push("ev_Stamper".into());
                        }
                    }
                    Some(id) => {
                        if let Some(c) = self
                            .traffic
                            .as_mut()
                            .and_then(|t| t.cars.iter_mut().find(|c| c.id == id))
                        {
                            c.vehicle.host.fired_triggers.push("ev_Stamper".into());
                        }
                    }
                }
            }
            if let Some(t) = self.traffic.as_mut() {
                let (alighting, waiting) = h.stop_wishes();
                t.set_stop_wishes(alighting, waiting);
                for (id, secs) in h.take_holds() {
                    t.hold_boarding(id, secs);
                }
                for (id, entry, exit) in h.take_ai_requests() {
                    t.set_pax_requests(id, &entry, &exit);
                }
            }
            if let Some(m) = h.take_message() {
                self.service_msg = Some((m, 6.0));
            }
            if let Some(p) = self.player.as_mut() {
                if took {
                    p.vehicle.set_var("GivenTicket", -1.0);
                }
                h.give_ticket = std::mem::take(&mut p.give_ticket);
                h.give_change_all = std::mem::take(&mut p.give_change);
                if std::mem::take(&mut p.take_change) {
                    h.take_change_tray();
                }
                if std::mem::take(&mut h.stop_request) {
                    // a passenger's request is the vehicle trigger Omsi.exe fires
                    // (0x62e42c), not the cab's stop button `door_haltewunsch`,
                    // whose switch and brake sounds some buses play
                    p.vehicle.trigger("int_haltewunsch");
                }
                h.write_pax_vars(&mut p.vehicle);
                p.vehicle.host.humans_on_path_link = h.path_link_counts();
                p.vehicle.host.humans_on_seat = h.seat_counts();
                let coins: Vec<usize> = std::mem::take(&mut p.vehicle.host.change_coins);
                h.give_change(w, r, scene, &coins);
                h.sync_money(r, scene, &p.vehicle);
            }
            h.sync(r, scene, center);
        }
        *self.profile.entry("humans").or_default() += __t.elapsed().as_secs_f64();
        self.foot_after_humans();
        if let (Some(d), Some(p), Some(w), false) = (
            self.duty.as_mut(),
            self.player.as_mut(),
            self.world.as_ref(),
            self.paused,
        ) {
            if let Some(stop) = p.html_next_stop.take() {
                if d.skip_to(stop) {
                    let (trip, k) = d.trip_for_ibis();
                    p.ibis_to_stop(trip, k);
                }
            }
            if let Some((arrival, departure)) = d.update(&mut p.vehicle, self.clock.time) {
                self.career.stop_served(arrival, departure);
            }
            if d.take_trip_change() && p.duty_typed {
                let (trip, stop) = d.trip_for_ibis();
                p.set_duty_destination(trip, stop);
            }
            let mut fonts = w.fonts.lock();
            if let Err(e) = schedule_paper::update_vehicle(&mut p.vehicle, d, &mut fonts) {
                log::warn!("driver timetable paper: {e:#}");
            }
        }
        if let Some(p) = self.player.as_mut() {
            let riders = self.humans.as_ref().map(|h| h.riding()).unwrap_or(0);
            p.vehicle.host.humans_count = riders as f32;
            p.vehicle.host.schedule_active = if self.duty.is_some() { 1.0 } else { 0.0 };
            let crash = std::mem::take(&mut p.vehicle.last_crash);
            if !self.exiting && !self.paused {
                self.career.tick(dt, &p.vehicle, riders);
            }
            if crash > 0.0 {
                self.career
                    .crashed(crash, p.vehicle.physics.velocity_kmh() / 3.6);
                self.service_msg = Some((format!("Crash: {:.0} kJ", crash / 1000.0), 6.0));
            }
        }
        if !self.paused {
            admin::guard_fall(self, dt);
        }
        self.placing_frame();
        if self
            .lan
            .as_ref()
            .map(|l| l.role == omsi_net::Role::Host)
            .unwrap_or(false)
        {
            self.editor_sync_t -= dt;
            if self.editor_sync_t <= 0.0 {
                self.editor_sync_t = 10.0;
                self.editor_broadcast(true);
            }
        }
        if self.args.on_foot && self.world.is_some() {
            self.args.on_foot = false;
            if self.player.is_some() {
                self.remove_driven_vehicle();
            } else if let Some(c) = self.camera.as_ref() {
                let p = c.position;
                let z = self
                    .world
                    .as_ref()
                    .and_then(|w| w.walk_height(p.x, p.y))
                    .unwrap_or(p.z - 1.7);
                let yaw = c.yaw as f64;
                self.start_on_foot(DVec3::new(p.x, p.y, z), yaw);
            }
            self.service_msg = Some((
                "On foot: Esc menu, Place a vehicle..., then G at its driver's door to drive it"
                    .into(),
                8.0,
            ));
        }
        #[cfg(not(target_os = "android"))]
        {
            self.discord_t -= dt;
            if self.discord_t <= 0.0 {
                self.discord_t = 5.0;
                if self.args.server.is_none()
                    && self.discord.is_none()
                    && self.settings.discord_status
                {
                    self.discord = discord::Discord::start(&self.settings.discord_app_id);
                }
                if let Some(d) = self.discord.as_ref() {
                    let bus = self.player.as_ref().map(|p| {
                        let definition = &p.vehicle.ty.def;
                        let short = omsi_launcher_lib::vehicle_type_label(
                            &definition.type_name,
                            &definition.path,
                        );
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
                    d.set(discord::Presence::for_game(
                        self.world.as_ref().map(|w| w.global.name.as_str()),
                        bus.as_ref()
                            .map(|(short, full)| (short.as_str(), full.as_str())),
                        duty,
                        self.lan.is_some(),
                    ));
                }
            }
        }
        let plugins = self.plugins.get_or_insert_with(plugins::load);
        if !plugins.is_empty() && !self.paused {
            let info = plugins::game_info(self);
            let keys = std::mem::take(&mut self.plugin_keys);
            let plugins = self.plugins.as_mut().unwrap();
            let mut io = plugins::Io {
                vehicle: self.player.as_mut().map(|p| &mut p.vehicle),
                dt,
                message: None,
                info,
                commands: Vec::new(),
                keys,
            };
            plugins.frame(&mut io);
            let commands = std::mem::take(&mut io.commands);
            if let Some(m) = io.message {
                self.service_msg = Some(m);
            }
            for c in commands {
                if let Some(k) = self.game_menu_items().iter().position(|m| m.0 == c) {
                    let was = self.game_menu;
                    self.menu_prev_pause = self.paused;
                    self.menu_choose(event_loop, k);
                    if self.chooser.is_none() && was.is_none() {
                        self.game_menu = None;
                    }
                } else {
                    self.menu_prev_pause = self.paused;
                    self.page_action(&c);
                }
            }
        } else {
            self.plugin_keys.clear();
        }
        if let (Some(p), Ok(list)) = (self.player.as_ref(), omsi_cfg::env::var("OMSI_WATCH_VARS")) {
            thread_local!(static LAST: std::cell::RefCell<std::collections::HashMap<String, f32>> = Default::default());
            LAST.with(|last| {
                let mut last = last.borrow_mut();
                for n in list.split(',').map(str::trim).filter(|n| !n.is_empty()) {
                    let v = p.vehicle.var(n).unwrap_or(f32::NAN);
                    if last.get(n).is_none_or(|&o| o.to_bits() != v.to_bits()) {
                        log::info!("watch: {n} = {v} at {:.2} s", self.clock.time);
                        last.insert(n.to_string(), v);
                    }
                }
            });
        }
        if let (Some(h), Some(p)) = (self.humans.as_mut(), self.player.as_ref()) {
            self.career.tickets = (h.tickets_sold as i32, h.ticket_cash as f64);
            self.career.boarded = h.boarded as i32;
            self.career.served = h.served as i32;
            self.career.stepped_in = h.stepped_in as i32;
            self.career.content = h.content as i32;
            self.career.ticket_requests = h.ticket_requests as i32;
            self.career.ticket_points = h.ticket_points as i32;
            let hurt = if self.settings.collision_pedestrians {
                h.run_over(&p.vehicle)
            } else {
                0
            };
            if hurt > 0 {
                self.career.crashes[1] += hurt as i32;
                self.service_msg = Some(("Pedestrian knocked down!".into(), 6.0));
            }
        }
    }
}
