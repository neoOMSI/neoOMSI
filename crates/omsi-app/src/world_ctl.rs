//! The running world: LAN sync, weather, METAR, clock, and placing, swapping and coupling vehicles.

use super::*;

impl App {
    pub(crate) fn tick_lan(&mut self, dt: f32) {
        let walker = self.walker_pose();
        let Some(lan) = self.lan.as_mut() else { return };
        let duty = self
            .duty
            .as_ref()
            .map(|d| &d.trips[d.trip_index])
            .map(|t| (t.line.as_str(), t.terminus.as_str()));
        let frame = lan::Frame {
            audio: self.audio.as_ref(),
            listener: self.camera.as_ref().map(|c| c.position),
            muffled: self.in_cab || self.inside_remote.is_some(),
            riders: self.humans.as_ref().map(|h| h.riding()).unwrap_or(0),
            clock: Some(&self.clock),
            tour: self.duty.as_ref().map(|d| format!("{}/{}", d.line, d.tour)),
            walker,
            inside_of: self.inside_remote,
        };
        let updates = lan::tick(
            lan,
            &mut self.remotes,
            dt,
            &self.args,
            self.player.as_mut(),
            self.world.as_deref(),
            self.renderer.as_ref(),
            self.scene.as_mut(),
            self.traffic.as_mut(),
            self.humans.as_mut(),
            duty,
            &frame,
        );
        for u in updates {
            self.apply_world_update(u);
        }
        let cmds = self
            .lan
            .as_mut()
            .map(|l| l.take_commands())
            .unwrap_or_default();
        for (from, text) in cmds {
            self.lan_command(from, &text);
        }
    }

    pub(crate) fn lan_command(&mut self, from: u32, text: &str) {
        let Some(lan) = self.lan.as_ref() else { return };
        let my_id = lan.my_id;
        if let Some(ev) = text.strip_prefix("trigger ") {
            let aboard = lan
                .peers()
                .find(|p| p.pose.id == from)
                .and_then(|p| p.pose.walker)
                .and_then(|w| w.aboard)
                .map(|a| a.owner == my_id)
                .unwrap_or(false);
            if !aboard {
                log::info!(
                    "LAN: player {from} asked for switch {ev} of our bus from outside it: ignored"
                );
                return;
            }
            if let Some(p) = self.player.as_mut() {
                log::info!("LAN: player {from} works {ev} in our bus");
                p.vehicle.trigger(ev.trim());
            }
            return;
        }
        crate::admin::command(self, from, text);
    }

    pub(crate) fn apply_world_update(&mut self, u: lan::WorldUpdate) {
        match u {
            lan::WorldUpdate::Clock {
                year,
                day_of_year,
                time,
            } => {
                self.clock.year = year;
                self.clock.day_of_year = day_of_year;
                self.clock.time = time;
                if let Some(t) = self.traffic.as_mut() {
                    t.day_time = time;
                }
            }
            lan::WorldUpdate::Slew(s) => {
                self.clock.time = (self.clock.time + s).clamp(0.0, 86399.999);
                if let Some(t) = self.traffic.as_mut() {
                    t.day_time += s;
                }
            }
            lan::WorldUpdate::Weather(w) => {
                log::info!(
                    "LAN: the host's weather: {}",
                    w.as_deref().unwrap_or("the map's default")
                );
                self.change_weather(w, false, 240.0);
            }
            lan::WorldUpdate::Tours(tours) => {
                if let Some(s) = self.schedule.as_mut() {
                    s.set_lan_tours(tours);
                }
            }
        }
        if let Some(p) = self.player.as_mut() {
            p.vehicle.host.clock = self.clock.clone();
        }
    }

    pub(crate) fn edit_weather(&mut self, f: impl FnOnce(&mut omsi_content::weather::Weather)) {
        if self
            .lan
            .as_ref()
            .is_some_and(|l| l.role == omsi_net::Role::Client)
        {
            self.service_msg = Some(("In a LAN session the host sets the weather".into(), 3.0));
            return;
        }
        if self.metar_locked() {
            self.service_msg = Some((
                "The weather cannot be changed while the METAR sync is on".into(),
                3.0,
            ));
            return;
        }
        let mut w = self.weather.clone().unwrap_or_default();
        if w.precip.len() < 5 {
            w.precip.resize(5, 0.0);
        }
        f(&mut w);
        let brightness = crate::weather_setup::custom_weather(self.args.weather.as_deref())
            .map(|c| c.brightness)
            .unwrap_or(1.0);
        let custom =
            crate::weather_setup::CustomWeather::from_weather(&w, brightness, self.wetness);
        self.set_custom_weather(custom);
    }

    pub(crate) fn set_custom_weather(&mut self, mut custom: crate::weather_setup::CustomWeather) {
        if self
            .lan
            .as_ref()
            .is_some_and(|l| l.role == omsi_net::Role::Client)
        {
            self.service_msg = Some(("In a LAN session the host sets the weather".into(), 3.0));
            return;
        }
        if self.metar_locked() {
            self.service_msg = Some((
                "The weather cannot be changed while the METAR sync is on".into(),
                3.0,
            ));
            return;
        }
        custom.normalize();
        self.metar_rx = None;
        self.metar_once = false;
        let spec = custom.encode();
        let to = custom.to_weather();
        let clouds_changed = self
            .weather
            .as_ref()
            .is_none_or(|w| w.clouds.0.trim() != to.clouds.0.trim());
        self.args.weather = Some(spec.clone());
        self.weather_blend = None;
        self.weather_cycle = None;
        self.wetness = custom.road_wetness;
        crate::scene::SNOW_WEATHER.store(to.snow, std::sync::atomic::Ordering::Relaxed);
        omsi_sim::host::set_ambient_weather(to.temp.0, to.temp.1);
        self.weather = Some(to);
        if clouds_changed {
            if let (Some(r), Some(scene)) = (self.renderer.as_ref(), self.scene.as_mut()) {
                crate::weather_setup::setup_sky(
                    &self.args,
                    r,
                    scene,
                    self.envir.as_ref(),
                    self.weather.as_ref(),
                );
            }
        }
        self.follow_date();
        if let Some(l) = self.lan.as_mut().filter(|l| l.role == omsi_net::Role::Host) {
            l.set_weather(&spec);
        }
        self.service_msg = Some(("Weather: Custom weather".into(), 2.0));
    }

    /// Put the vehicle file `bus` down beside the camera or the bus driven, in `paint` (a
    /// scheme's name; None: at random) with the depot file `hof` (None: the map's).
    /// The driven vehicle read again from its files and put where it stands (#728).
    pub(crate) fn reload_driven_vehicle(&mut self) {
        let Some(p) = self.player.as_ref() else {
            self.service_msg = Some(("There is no vehicle to reload: you are on foot".into(), 3.0));
            return;
        };
        let file = &p.vehicle.ty.def.path;
        let bus = omsi_cfg::content_roots()
            .iter()
            .chain(std::iter::once(&self.args.root))
            .find_map(|r| file.strip_prefix(r).ok())
            .unwrap_or(file)
            .to_string_lossy()
            .replace('\\', "/");
        let paint = p
            .vehicle
            .host
            .paint_scheme
            .flatten()
            .and_then(|i| p.vehicle.ty.paint_schemes.get(i))
            .map(|s| s.name.clone());
        let hof = p.vehicle.host.hof.as_ref().and_then(|h| {
            h.path
                .file_stem()
                .map(|s| s.to_string_lossy().to_string())
                .or_else(|| Some(h.name.clone()))
        });
        let before = p.uid;
        self.swap_pending = true;
        self.place_vehicle(&bus, paint, hof);
        if let Some(p) = self.player.as_ref().filter(|p| p.uid != before) {
            let name = format!(
                "{} {}",
                p.vehicle.ty.def.manufacturer, p.vehicle.ty.def.type_name
            );
            self.service_msg = Some((format!("Reloaded from its files: {}", name.trim()), 4.0));
        }
    }

    /// `q` (just spawned where the driven vehicle stands) becomes the one driven, and the
    /// one driven until now goes, with whoever rode in it (#728).
    pub(crate) fn replace_driven_vehicle(&mut self, q: Player) {
        let uid = q.uid;
        self.placed.insert(0, q);
        self.switch_vehicle();
        if !self.player.as_ref().is_some_and(|p| p.uid == uid) {
            return;
        }
        let Some(mut old) = self.placed.pop() else {
            return;
        };
        if let (Some(a), Some(mut ss)) = (self.audio.as_ref(), old.sounds.take()) {
            ss.stop_all(a);
        }
        if let (Some(w), Some(r), Some(scene)) = (
            self.world.clone(),
            self.renderer.as_ref(),
            self.scene.as_mut(),
        ) {
            if let Some(h) = self.humans.as_mut() {
                h.evict(
                    crate::humans::BusId::Ai(crate::humans::placed_bus_id(old.uid)),
                    &w,
                );
            }
            if let Some(mut d) = old.driver.take() {
                d.hide(r, scene);
            }
            w.release_vehicle(r, scene, old.render);
            for t in old.trailer_renders {
                w.release_vehicle(r, scene, t);
            }
        }
    }

    pub(crate) fn place_vehicle(&mut self, bus: &str, paint: Option<String>, hof: Option<String>) {
        let swap = std::mem::take(&mut self.swap_pending) && self.player.is_some();
        let name = self
            .vehicle_list
            .iter()
            .find(|v| v.1 == bus)
            .map(|v| v.0.clone())
            .unwrap_or_else(|| bus.to_string());
        let bus = bus.to_string();
        let (Some(w), Some(r), Some(scene), Some(cam)) = (
            self.world.clone(),
            self.renderer.as_ref(),
            self.scene.as_mut(),
            self.camera.as_ref(),
        ) else {
            return;
        };
        let (x, y, heading) = match (self.view.as_str(), self.player.as_ref()) {
            (_, Some(p)) if swap => (
                p.vehicle.position.x,
                p.vehicle.position.y,
                p.vehicle.heading,
            ),
            ("free", _) | (_, None) => {
                let f = cam.forward();
                let flat = glam::DVec2::new(f.x as f64, f.y as f64).normalize_or_zero();
                let at = cam.position.truncate() + flat * 15.0;
                (at.x, at.y, cam.yaw as f64)
            }
            (_, Some(p)) => {
                let h = p.vehicle.heading.to_radians();
                let right = glam::DVec2::new(h.cos(), -h.sin());
                let at = p.vehicle.position.truncate() + right * 5.0;
                (at.x, at.y, p.vehicle.heading)
            }
        };
        let one = Args {
            bus: Some(bus.clone()),
            spawn: Some(format!("{x},{y},{heading}")),
            situation_vars: Vec::new(),
            situation_strvars: Vec::new(),
            situation_others: Vec::new(),
            line: None,
            tour: None,
            trip: None,
            autostart: false,
            paint,
            hof: hof.or(self.args.hof.clone()),
            ..self.args.clone()
        };
        match spawn_player(&one, &w, r, scene) {
            Ok(Some(q)) if swap => {
                log::info!("{bus} takes the driven vehicle's place at ({x:.1}, {y:.1})");
                self.replace_driven_vehicle(q);
            }
            Ok(Some(q)) => {
                log::info!("placed {bus} at ({x:.1}, {y:.1})");
                let uid = q.uid;
                self.placed.push(q);
                self.begin_placing(uid, heading);
                let _ = name;
            }
            Ok(None) => {}
            Err(e) => self.service_msg = Some((format!("Could not place {name}: {e:#}"), 5.0)),
        }
    }

    pub(crate) fn couple(&mut self) {
        let (Some(w), Some(r), Some(scene), Some(p)) = (
            self.world.clone(),
            self.renderer.as_ref(),
            self.scene.as_mut(),
            self.player.as_mut(),
        ) else {
            return;
        };
        let rear = match p.vehicle.trailers.last() {
            Some(t) => {
                let c = if t.reversed {
                    t.ty.def.coupling_front.as_ref()
                } else {
                    t.ty.def.coupling_back.as_ref()
                };
                c.map(|c| {
                    (
                        t.world_transform()
                            .transform_point3(glam::Vec3::from(c.pos)),
                        t.heading,
                    )
                })
            }
            None => p.vehicle.ty.def.coupling_back.as_ref().map(|c| {
                (
                    p.vehicle
                        .world_transform()
                        .transform_point3(glam::Vec3::from(c.pos)),
                    p.vehicle.heading,
                )
            }),
        };
        let Some((rear, rear_heading)) = rear else {
            self.service_msg = Some(("This vehicle has no coupling at its back".into(), 4.0));
            return;
        };
        let found = self.placed.iter().position(|q| {
            let Some(c) = q.vehicle.ty.def.coupling_front.as_ref() else {
                return false;
            };
            let front = q
                .vehicle
                .world_transform()
                .transform_point3(glam::Vec3::from(c.pos));
            let dh = ((q.vehicle.heading - rear_heading + 540.0).rem_euclid(360.0) - 180.0).abs();
            (front - rear).truncate().length() < 2.5 && dh < 35.0
        });
        let Some(k) = found else {
            self.service_msg = Some((
                "Nothing to couple: back up to a trailer's coupling (within 2.5 m, in line)".into(),
                4.0,
            ));
            return;
        };
        let q = self.placed.remove(k);
        let ty = q.vehicle.ty.clone();
        if let (Some(a), Some(mut ss)) = (self.audio.as_ref(), q.sounds) {
            ss.stop_all(a);
        }
        w.release_vehicle(r, scene, q.render);
        for tr in q.trailer_renders {
            w.release_vehicle(r, scene, tr);
        }
        p.vehicle.attach_trailer_ex(ty.clone(), false);
        p.trailer_renders
            .push(w.add_vehicle_part(r, scene, &ty, None, &p.render));
        p.hand_coupled += 1;
        self.service_msg = Some((
            format!("Coupled: {} {}", ty.def.manufacturer, ty.def.type_name),
            3.0,
        ));
    }

    pub(crate) fn uncouple(&mut self) {
        let (Some(w), Some(r), Some(scene)) = (
            self.world.clone(),
            self.renderer.as_ref(),
            self.scene.as_mut(),
        ) else {
            return;
        };
        let Some(p) = self.player.as_mut() else {
            return;
        };
        if p.hand_coupled == 0 {
            self.service_msg = Some((
                "Nothing coupled by hand (an articulated bus's rear section stays)".into(),
                4.0,
            ));
            return;
        }
        let Some(t) = p.vehicle.detach_last_trailer() else {
            return;
        };
        if let Some(tr) = p.trailer_renders.pop() {
            w.release_vehicle(r, scene, tr);
        }
        p.hand_coupled -= 1;
        let one = Args {
            bus: Some(t.ty.def.path.to_string_lossy().to_string()),
            spawn: Some(format!(
                "{},{},{},{}",
                t.position.x, t.position.y, t.heading, t.position.z
            )),
            situation_vars: Vec::new(),
            situation_strvars: Vec::new(),
            situation_others: Vec::new(),
            line: None,
            tour: None,
            trip: None,
            autostart: false,
            paint: None,
            ..self.args.clone()
        };
        match spawn_player(&one, &w, r, scene) {
            Ok(Some(q)) => {
                self.placed.push(q);
                self.service_msg = Some(("Uncoupled".into(), 3.0));
            }
            Ok(None) => {}
            Err(e) => log::warn!("uncoupled part: {e:#}"),
        }
    }

    pub(crate) fn time_speed(&self) -> f64 {
        if self.real_time_locked() {
            return 1.0;
        }
        match self.lan.as_ref() {
            Some(l) => l.clock_speed,
            None => self.settings.time_speed.clamp(1.0, 30.0),
        }
    }

    pub(crate) fn next_weather(&mut self) {
        self.step_weather();
    }

    pub(crate) fn step_weather(&mut self) {
        if self
            .lan
            .as_ref()
            .map(|l| l.role == omsi_net::Role::Client)
            .unwrap_or(false)
        {
            self.service_msg = Some(("In a LAN session the host sets the weather".into(), 3.0));
            return;
        }
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
        files.sort();
        files.dedup();
        if files.is_empty() {
            return;
        }
        let cur = self
            .args
            .weather
            .clone()
            .unwrap_or_default()
            .replace('\\', "/")
            .to_ascii_lowercase();
        let i = files
            .iter()
            .position(|f| f.to_ascii_lowercase() == cur)
            .map(|i| (i + 1) % files.len())
            .unwrap_or(0);
        self.change_weather(Some(files[i].clone()), true, 1.0);
    }

    /// Go over to weather `file` (None: the map's default) in `secs` of the day (see
    /// `weather_cycle`); a host tells the others (`share`), who come over to it the same way.
    /// The player's own choice comes at once, as in Omsi.exe (the weather dialog loads the
    /// .owt and applies it straight away, 0x6828e0 -> 0x754c80); the cycle blends it in.
    pub(crate) fn change_weather(&mut self, file: Option<String>, share: bool, secs: f32) {
        if self.metar_locked() {
            self.service_msg = Some((
                "The weather cannot be changed while the METAR sync is on".into(),
                3.0,
            ));
            return;
        }
        self.metar_rx = None;
        self.metar_once = false;
        let from = self.weather.clone().unwrap_or_default();
        self.args.weather = file.clone();
        let to = load_weather(&self.args);
        let name = to.name.clone();
        self.weather_blend = Some(crate::weather_cycle::Blend::new(from, to, secs));
        if share {
            if let (Some(l), Some(f)) = (self.lan.as_mut(), file.as_ref()) {
                l.set_weather(f);
            }
        }
        log::info!("weather: going over to {file:?} ({name})");
        self.service_msg = Some((format!("Weather: {name}"), 4.0));
    }

    pub(crate) fn tick_weather(&mut self, secs: f32) {
        if let Some(b) = self.weather_blend.as_mut() {
            let (w, clouds_changed, done) = b.step(secs);
            self.weather = Some(w);
            if done {
                self.weather_blend = None;
            }
            if clouds_changed {
                if let (Some(r), Some(scene)) = (self.renderer.as_ref(), self.scene.as_mut()) {
                    crate::weather_setup::setup_sky(
                        &self.args,
                        r,
                        scene,
                        self.envir.as_ref(),
                        self.weather.as_ref(),
                    );
                }
            }
        }
        if let Some(w) = self.weather.as_ref() {
            crate::weather_setup::cloud_drift_step(&mut self.cloud_drift, w, secs as f64);
        }
        let follows = self
            .lan
            .as_ref()
            .is_some_and(|l| l.role == omsi_net::Role::Client);
        if follows || self.weather_blend.is_some() {
            return;
        }
        if self.metar_locked() {
            return;
        }
        let Some(c) = self.weather_cycle.as_mut() else {
            return;
        };
        c.next_in -= secs as f64;
        if c.next_in > 0.0 {
            return;
        }
        c.next_in = c.interval();
        let r = c.rand();
        let all = crate::weather_cycle::installed();
        let now = self.weather.clone().unwrap_or_default();
        let now_file = self.args.weather.clone().unwrap_or_default();
        if let Some(next) =
            crate::weather_cycle::pick(&all, &now, &now_file, self.clock.day_month().1, r)
        {
            self.change_weather(Some(next), true, 240.0);
        }
    }

    pub(crate) fn switch_vehicle(&mut self) {
        if self.placed.is_empty() {
            self.service_msg = Some(("There is no other vehicle to drive".into(), 3.0));
            return;
        }
        let Some(mut now) = self.player.take() else {
            self.take_placed(0);
            return;
        };
        if let (Some(a), Some(mut ss)) = (self.audio.as_ref(), now.sounds.take()) {
            ss.stop_all(a);
        }
        let mut next = self.placed.remove(0);
        if let Some(a) = self.audio.as_ref() {
            if let Some(mut s) = next.sounds.take() {
                s.stop_all(a);
            }
            next.load_sounds(a);
        }
        if let Some(h) = self.humans.as_mut() {
            h.player_bus_swapped(now.uid, next.uid, &mut next.vehicle);
        }
        next.vehicle.host.auto_clutch = if self.settings.auto_clutch { 1.0 } else { 0.0 };
        self.placed.push(now);
        let name = format!(
            "{} {}",
            next.vehicle.ty.def.manufacturer, next.vehicle.ty.def.type_name
        );
        if let Some(cam) = self.camera.as_ref() {
            self.camera = Some(next.camera(&self.view, cam));
        }
        self.player = Some(next);
        self.look = (0.0, 0.0);
        self.service_msg = Some((format!("Now driving: {}", name.trim()), 4.0));
    }

    pub(crate) fn metar_locked(&self) -> bool {
        self.settings.metar_sync
            && !self
                .lan
                .as_ref()
                .is_some_and(|l| l.role == omsi_net::Role::Client)
    }

    pub(crate) fn metar_station(&self) -> String {
        if !self.settings.metar_station.is_empty() {
            return self.settings.metar_station.to_ascii_uppercase();
        }
        match self
            .args
            .weather
            .as_deref()
            .and_then(|w| w.strip_prefix("metar:"))
        {
            Some(code) if !code.trim().is_empty() => code.trim().to_ascii_uppercase(),
            _ => crate::launcher::drive::nearest_airport(
                &self.args.root.to_string_lossy(),
                &self.args.map,
            ),
        }
    }

    pub(crate) fn load_metar_once(&mut self) {
        if self
            .lan
            .as_ref()
            .is_some_and(|l| l.role == omsi_net::Role::Client)
        {
            self.service_msg = Some(("In a LAN session the host sets the weather".into(), 3.0));
            return;
        }
        let icao = self.metar_station();
        self.metar_rx = None;
        self.metar_once = true;
        let (tx, rx) = std::sync::mpsc::channel();
        self.metar_rx = Some(rx);
        std::thread::spawn(move || {
            let _ = tx.send(crate::weather_setup::try_metar(&icao));
        });
        self.service_msg = Some((
            format!("Weather: loading METAR for {}", self.metar_station()),
            4.0,
        ));
    }

    pub(crate) fn refresh_metar_now(&mut self) {
        if !self.metar_locked() {
            self.load_metar_once();
            return;
        }
        self.metar_rx = None;
        self.metar_once = false;
        self.metar_next = 0.0;
        self.service_msg = Some((
            format!("Weather: refreshing METAR for {}", self.metar_station()),
            4.0,
        ));
    }

    pub(crate) fn current_weather_as_custom(&mut self) {
        if self.metar_locked() {
            self.service_msg = Some((
                "Turn METAR sync off before editing its current weather".into(),
                3.0,
            ));
            return;
        }
        let Some(w) = self.weather.as_ref() else {
            return;
        };
        let brightness = crate::weather_setup::custom_weather(self.args.weather.as_deref())
            .map(|c| c.brightness)
            .unwrap_or(1.0);
        let c = crate::weather_setup::CustomWeather::from_weather(w, brightness, self.wetness);
        self.set_custom_weather(c);
    }

    pub(crate) fn tick_metar(&mut self, dt: f32) {
        self.share_start_metar();
        if let Some(rx) = self.metar_rx.as_ref() {
            match rx.try_recv() {
                Ok(report) => {
                    let once = self.metar_once;
                    self.metar_rx = None;
                    self.metar_once = false;
                    match report {
                        Some(w) => self.apply_metar(w),
                        None => {
                            self.service_msg =
                                Some(("Weather: no METAR report could be loaded".into(), 4.0))
                        }
                    }
                    if once {
                        return;
                    }
                }
                Err(std::sync::mpsc::TryRecvError::Empty) => return,
                Err(std::sync::mpsc::TryRecvError::Disconnected) => {
                    let once = self.metar_once;
                    self.metar_rx = None;
                    self.metar_once = false;
                    if once {
                        self.service_msg = Some(("Weather: METAR request failed".into(), 4.0));
                        return;
                    }
                }
            }
        }
        if !self.metar_locked() {
            self.metar_next = 0.0;
            return;
        }
        self.metar_next -= dt as f64;
        if self.metar_next > 0.0 {
            return;
        }
        self.metar_next = 60.0;
        let icao = self.metar_station();
        let (tx, rx) = std::sync::mpsc::channel();
        self.metar_rx = Some(rx);
        self.metar_once = false;
        std::thread::spawn(move || {
            let _ = tx.send(crate::weather_setup::try_metar(&icao));
        });
    }

    pub(crate) fn share_start_metar(&mut self) {
        let Some(l) = self.lan.as_mut().filter(|l| l.role == omsi_net::Role::Host) else {
            return;
        };
        if !l.weather().to_ascii_lowercase().starts_with("metar:") {
            return;
        }
        if let Some(wire) = self
            .weather
            .as_ref()
            .and_then(crate::weather_setup::report_wire)
        {
            l.set_weather(&wire);
        }
    }

    pub(crate) fn apply_metar(&mut self, to: omsi_content::weather::Weather) {
        self.metar_next = 600.0;
        let file = to.path.to_string_lossy().to_string();
        let wire = crate::weather_setup::report_wire(&to).unwrap_or_else(|| file.clone());
        let name = to.name.clone();
        let from = self.weather.clone().unwrap_or_default();
        crate::scene::SNOW_WEATHER.store(to.snow, std::sync::atomic::Ordering::Relaxed);
        omsi_sim::host::set_ambient_weather(to.temp.0, to.temp.1);
        self.args.weather = Some(file.clone());
        self.weather_cycle = None;
        self.weather_blend = Some(crate::weather_cycle::Blend::new(from, to, 60.0));
        if let Some(l) = self.lan.as_mut().filter(|l| l.role == omsi_net::Role::Host) {
            l.set_weather(&wire);
        }
        log::info!("weather: METAR sync, going over to {file} ({name})");
        self.service_msg = Some((format!("Weather: {name}"), 4.0));
    }

    pub(crate) fn real_time_locked(&self) -> bool {
        self.settings.time_sync
            && !self
                .lan
                .as_ref()
                .is_some_and(|l| l.role == omsi_net::Role::Client)
    }

    pub(crate) fn sync_real_time(&mut self) {
        if !self.real_time_locked() {
            return;
        }
        let Some(real) = crate::real_time::clock_now(&self.clock) else {
            return;
        };
        let gap = crate::real_time::gap(&self.clock, &real);
        if gap.abs() < 0.25 {
            return;
        }
        self.clock.year = real.year;
        self.clock.day_of_year = real.day_of_year;
        self.clock.time = real.time;
        if let Some(tr) = self.traffic.as_mut() {
            tr.day_time += gap;
        }
        if let Some(p) = self.player.as_mut() {
            p.vehicle.host.clock = self.clock.clone();
        }
    }

    pub(crate) fn shift_clock(&mut self, secs: f64) {
        if self.real_time_locked() {
            self.service_msg = Some((
                "The time cannot be changed while the real-time sync is on".into(),
                3.0,
            ));
            return;
        }
        let mut t = self.clock.time + secs;
        while t < 0.0 {
            t += 86400.0;
            self.clock.day_of_year = if self.clock.day_of_year > 1 {
                self.clock.day_of_year - 1
            } else {
                omsi_sim::clock::days_in_year(self.clock.year - 1)
            };
        }
        while t >= 86400.0 {
            t -= 86400.0;
            self.clock.day_of_year =
                self.clock.day_of_year % omsi_sim::clock::days_in_year(self.clock.year) + 1;
        }
        self.clock.time = t;
        if let Some(tr) = self.traffic.as_mut() {
            tr.day_time += secs;
        }
        let mirrored = self.traffic.as_ref().is_some_and(|tr| tr.is_mirror());
        if !mirrored {
            let day_time = self.traffic.as_ref().map(|tr| tr.day_time);
            if let (Some(w), Some(tr), Some(r), Some(scene)) = (
                self.world.as_ref(),
                self.traffic.as_mut(),
                self.renderer.as_ref(),
                self.scene.as_mut(),
            ) {
                tr.reset_population(w, r, scene);
            }
            if let (Some(s), Some(day_time)) = (self.schedule.as_mut(), day_time) {
                s.refresh_time(&self.clock, day_time);
            }
            if let Some(h) = self.humans.as_mut() {
                h.reset_population();
            }
            self.populate_t = 0.0;
            self.humans_populate_t = 0.0;
            self.first_populate = true;
        }
        if let Some(p) = self.player.as_mut() {
            p.vehicle.host.clock = self.clock.clone();
        }
        let h = (t / 3600.0) as u32;
        self.service_msg = Some((
            format!("Clock: {h:02}:{:02}", ((t / 60.0) as u32) % 60),
            3.0,
        ));
    }

    pub(crate) fn run_service(&mut self, kind: &str) {
        let Some(w) = self.world.clone() else { return };
        let Some(p) = self.player.as_mut() else {
            return;
        };
        let one = Args {
            refuel: kind == "refuel",
            wash: kind == "wash",
            repair: kind == "repair",
            ..self.args.clone()
        };
        let at_station = at_petrol_station(&w, &p.vehicle);
        let mut clock = self.clock.clone();
        let msg = run_services(
            &one,
            &mut p.vehicle,
            &mut clock,
            w.global.repair_time_min,
            at_station,
        );
        while clock.time >= 86400.0 {
            clock.time -= 86400.0;
            clock.day_of_year = clock.day_of_year % omsi_sim::clock::days_in_year(clock.year) + 1;
        }
        if !self.settings.time_sync
            || self
                .lan
                .as_ref()
                .is_some_and(|l| l.role == omsi_net::Role::Client)
        {
            self.clock = clock;
        }
        p.vehicle.host.clock = self.clock.clone();
        for line in &msg {
            log::info!("{line}");
        }
        if let Some(line) = msg.into_iter().next() {
            self.service_msg = Some((line, 6.0));
        }
    }

    pub(crate) fn place_bus_at(&mut self, at: glam::DVec2) {
        if self
            .lan
            .as_ref()
            .is_some_and(|l| l.role == omsi_net::Role::Client)
        {
            self.service_msg = Some((
                "In a LAN session only the host moves vehicles on the map".into(),
                4.0,
            ));
            return;
        }
        let p = glam::DVec3::new(at.x, at.y, 0.0);
        // the traffic's lanes (the tiles loaded around the bus), else the navigator's of the
        // whole map: a street far off on a big map was "no street" until the bus had been
        // flown there (#235). (the height of the point does not matter: the nearest by the
        // ground plan)
        let nets = [
            self.traffic.as_ref().map(|t| &t.net),
            self.navigator.as_ref().and_then(|n| n.map_net()),
        ];
        let Some((net, (lane, s, _))) = nets.into_iter().flatten().find_map(|net| {
            net.nearest_lane(p, omsi_sim::traffic::LaneKind::Street)
                .filter(|(_, _, d)| *d <= 300.0)
                .map(|f| (net, f))
        }) else {
            self.service_msg = Some(("No street near that point".into(), 3.0));
            return;
        };
        let l = &net.lanes[lane];
        let (pos, heading) = l.at(s);
        let heading = heading as f64;
        crate::admin::teleport(self, pos, heading);
        self.service_msg = Some(("The bus stands where the map was clicked".into(), 3.0));
    }

    pub(crate) fn follow_date(&mut self) {
        let Some(w) = self.world.clone() else { return };
        let date = self.clock.date_code();
        let snow = self.weather.as_ref().is_some_and(|x| x.snow);
        let season = crate::world_load::season_folder_on(
            &self.args,
            &w.global,
            self.clock.day_of_year,
            snow,
        )
        .1;
        let Some((was_date, was_season)) = self.world_day.clone() else {
            self.world_day = Some((date, omsi_texture::season_folder()));
            return;
        };
        if was_date == date && was_season == season {
            return;
        }
        self.world_day = Some((date, season.clone()));
        let changed = if was_date != date {
            w.set_date(date)
        } else {
            Vec::new()
        };
        let (Some(st), Some(r), Some(scene)) = (
            self.streamer.as_mut(),
            self.renderer.as_ref(),
            self.scene.as_mut(),
        ) else {
            return;
        };
        if was_season != season {
            log::info!(
                "season: the textures of {:?} now (were {:?})",
                season,
                was_season
            );
            omsi_texture::set_season_folder(season);
            omsi_cfg::content_changed();
            st.reload(r, scene, None, self.audio.as_ref());
        } else if !changed.is_empty() {
            st.reload(r, scene, Some(&changed), self.audio.as_ref());
        }
    }
}
