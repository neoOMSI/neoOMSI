#![allow(unused_imports)]
use super::types::*;
use ::simulation::collision::Obb;

impl crate::App {
    pub(crate) fn dev_gather(&self) -> Extra {
        let (boxes_on, radius, tours_on) =
            self.devtools.as_ref().map_or((false, 25.0, false), |d| {
                (d.wants_boxes(), d.box_radius(), d.wants_tours())
            });
        let foot = self.on_foot.as_ref().map(|f| FootInfo {
            pos: [f.pos.x, f.pos.y, f.pos.z],
            heading: f.heading,
            vel: [f.vel.x, f.vel.y],
            vz: f.vz,
            on_lane: f.on_lane,
            attached: f.attached,
            seated: f.seat.is_some(),
            inside: f.inside.is_some(),
        });
        let doors = match (self.on_foot.as_ref(), self.humans.as_ref()) {
            (Some(f), Some(hm)) if boxes_on || self.devtools.is_some() => {
                let mut v = Vec::new();
                for bus in hm.bus_ids_near(f.pos, 25.0) {
                    for (inside, outside, _, open) in hm.cabin_doors(bus) {
                        let Some((wi, _)) = hm.cabin_world(bus, inside) else {
                            continue;
                        };
                        let (a, b) = (outside.truncate(), wi.truncate());
                        let ab = b - a;
                        let len = ab.length();
                        if len < 1e-3 {
                            continue;
                        }
                        let dir = ab / len;
                        let rel = f.pos.truncate() - a;
                        let (along, lateral) = (rel.dot(dir), rel.perp_dot(dir).abs());
                        v.push(DoorDbg {
                            outside: [outside.x, outside.y, outside.z],
                            inside: [wi.x, wi.y, wi.z],
                            open,
                            along,
                            len,
                            lateral,
                            in_lane: open && along >= -1.2 && along <= len + 1.0 && lateral <= 1.0,
                        });
                    }
                }
                v
            }
            _ => Vec::new(),
        };
        let boxes = if boxes_on {
            let at = self
                .on_foot
                .as_ref()
                .map(|f| f.pos)
                .or_else(|| self.camera.as_ref().map(|c| c.position));
            at.map(|at| self.dev_hitboxes(at, radius))
                .unwrap_or_default()
        } else {
            Vec::new()
        };
        let blockers = if boxes_on {
            let exempt = doors
                .iter()
                .filter(|d| d.in_lane)
                .min_by(|a, b| {
                    a.lateral
                        .partial_cmp(&b.lateral)
                        .unwrap_or(std::cmp::Ordering::Equal)
                })
                .map(|d| glam::DVec2::new(d.inside[0], d.inside[1]));
            self.dev_blockers(exempt)
        } else {
            Vec::new()
        };
        let lan = self.lan.as_ref().map(|l| LanInfo {
            host: l.role == ::network::Role::Host,
            connected: l.connected,
            code: l.code().map(|c| c.encode()),
            target: self.args.lan_join.clone().or(l.host.map(|a| a.to_string())),
            local: l.local_addr().map(|a| a.to_string()),
            peers: l.peer_count(),
            name: l.my_name.clone(),
            session: l.session,
            rejected: l.rejected.clone(),
            sent: l.sent(),
            map: l.world.map.clone(),
        });
        let mut tours = Vec::new();
        if tours_on {
            if let Some(sch) = self.schedule.as_ref() {
                for line in &sch.data.lines {
                    for t in &line.tours {
                        tours.push(TourRow {
                            line: line.name.clone(),
                            number: t.number.clone(),
                            start: crate::game_lists::tour_start(t).unwrap_or(0.0),
                            trips: t.trips.len(),
                            available: sch.tour_available(t),
                        });
                    }
                }
                tours.sort_by(|a, b| {
                    a.line.cmp(&b.line).then(
                        a.start
                            .partial_cmp(&b.start)
                            .unwrap_or(std::cmp::Ordering::Equal),
                    )
                });
                tours.truncate(3000);
            }
        }
        let quicksave = quicksave_exists(&self.args.root);
        let mut beams: Vec<BeamMark> = Vec::new();
        let ls = crate::lights::settings();
        if ls.beam_marker || ls.spill.marker || ls.spot2.marker {
            if let (Some(scene), Some(cam)) = (self.scene.as_ref(), self.camera.as_ref()) {
                if ls.beam_marker {
                    for c in scene.coronas.iter().filter(|c| c.beam) {
                        beams.push(BeamMark {
                            pos: [c.position.x, c.position.y, c.position.z],
                            dir: c.direction.to_array(),
                            cone: true,
                            tint: None,
                        });
                    }
                    for l in scene
                        .lights
                        .iter()
                        .filter(|l| l.beam != 0.0 && l.direction.length_squared() > 0.1)
                    {
                        beams.push(BeamMark {
                            pos: [l.position.x, l.position.y, l.position.z],
                            dir: l.direction.to_array(),
                            cone: false,
                            tint: None,
                        });
                    }
                }
                if ls.spill.marker {
                    for l in scene
                        .lights
                        .iter()
                        .filter(|l| l.shadow_first && l.direction.length_squared() > 0.1)
                    {
                        beams.push(BeamMark {
                            pos: [l.position.x, l.position.y, l.position.z],
                            dir: l.direction.to_array(),
                            cone: false,
                            tint: Some([1.0, 0.5, 0.1]),
                        });
                    }
                }
                if ls.spot2.marker {
                    if let Some(p) = self.player.as_ref() {
                        let v = &p.vehicle;
                        let body = v.body_rotation();
                        for sp in &v.ty.model.spotlights_2 {
                            let on = sp.variable.trim().parse::<f32>().ok().or_else(|| v.var(sp.variable.trim())).unwrap_or(0.0);
                            if on < 0.5 {
                                continue;
                            }
                            let vals = sp.values;
                            let mirrored = !sp.no_mirror && vals[0].abs() > 0.01;
                            let sides: &[f32] = if mirrored { &[1.0, -1.0] } else { &[1.0] };
                            for side in sides {
                                let at = v.position
                                    + body
                                    .transform_point3(glam::Vec3::new(vals[0] * side, vals[1], vals[2]))
                                    .as_dvec3();
                                let d = body.transform_vector3(glam::Vec3::new(vals[3] * side, vals[4], vals[5]));
                                beams.push(BeamMark {
                                    pos: [at.x, at.y, at.z],
                                    dir: d.to_array(),
                                    cone: false,
                                    tint: Some([0.2, 1.0, 0.2]),
                                });
                            }
                        }
                    }
                }
                let at = cam.position;
                beams.sort_by(|a, b| {
                    let da = (glam::DVec3::from(a.pos) - at).length_squared();
                    let db = (glam::DVec3::from(b.pos) - at).length_squared();
                    da.partial_cmp(&db).unwrap_or(std::cmp::Ordering::Equal)
                });
                beams.truncate(32);
            }
        }
        let vehicle = self.player.as_ref().map(|p| {
            let (walk_points, walk_links) = walk_paths(&p.vehicle.ty.def);
            VehicleInfo {
                actions: p.bound_actions(),
                controls: p.control_list(),
                interior: p
                    .vehicle
                    .ty
                    .model
                    .interior_lights
                    .iter()
                    .map(|l| InteriorInfo {
                        variable: l.variable.clone(),
                        pos: l.pos,
                        color: l.color,
                        range: l.range,
                    })
                    .collect(),
                exterior: p
                    .vehicle
                    .ty
                    .model
                    .meshes
                    .iter()
                    .flat_map(|m| {
                        let a = m.light_enh.iter().map(|l| InteriorInfo {
                            variable: l.variable.clone(),
                            pos: l.pos,
                            color: l.color,
                            range: l.size,
                        });
                        let b = m.light_enh_2.iter().map(|l| InteriorInfo {
                            variable: l.variable.clone(),
                            pos: l.pos,
                            color: l.color,
                            range: l.size,
                        });
                        a.chain(b)
                    })
                    .collect(),
                walk_points,
                walk_links,
            }
        });
        let pose = self.player.as_ref().map(|p| {
            let v = &p.vehicle;
            (
                [v.position.x, v.position.y, v.position.z],
                v.body_rotation(),
            )
        });
        Extra {
            pose,
            beams,
            vehicle,
            map: self.args.map.clone(),
            clock: self.clock.time,
            paused: self.paused,
            cam: self.camera,
            foot,
            boxes,
            doors,
            blockers,
            lan,
            tours,
            quicksave,
            profile: self.profile.iter().map(|(k, v)| (*k, *v)).collect(),
            frames: self.total_frames,
            traffic: self.traffic.as_ref().map(|t| TrafficPerf {
                cars: t.car_count(),
                dormant: t.dormant_count(),
            }),
            traffic_debug: self.devtools.as_ref().and_then(|d| d.traffic_request()).and_then(|(radius, selected)| {
                let at = self.camera.as_ref()?.position;
                self.traffic.as_ref().map(|t| t.debug_frame(at, radius, selected))
            }),
            weather: self.dev_weather_info(),
        }
    }

    fn dev_weather_info(&self) -> WeatherInfo {
        let w = self.weather.clone().unwrap_or_default();
        let parsed = crate::weather_setup::custom_weather(self.args.weather.as_deref());
        let is_custom = parsed.is_some();
        let custom = parsed.unwrap_or_else(|| {
            crate::weather_setup::CustomWeather::from_weather(&w, 1.0, self.wetness)
        });
        let density = crate::weather_setup::clouds_of(&w, [0.0; 2]).0;
        let client = self
            .lan
            .as_ref()
            .is_some_and(|l| l.role == ::network::Role::Client);
        WeatherInfo {
            spec: self.args.weather.clone().unwrap_or_default(),
            custom,
            is_custom,
            wetness: self.wetness,
            blend: self
                .weather_blend
                .as_ref()
                .map(|b| (b.progress(), b.target().name.clone())),
            cycle_next: self.weather_cycle.as_ref().map(|c| c.next_in),
            client,
            metar_locked: self.metar_locked(),
            metar_loading: self.metar_rx.is_some(),
            metar_station: self.metar_station(),
            time_locked: client || self.real_time_locked(),
            year: self.clock.year,
            day_of_year: self.clock.day_of_year,
            day_month: self.clock.day_month(),
            density,
            layers: crate::weather_setup::cloud_layers_of(&w),
            precip: crate::weather_setup::precip_of(&w),
            street_cond: crate::weather_setup::street_condition(&w, self.wetness),
            drift: self.cloud_drift,
            weather: w,
        }
    }

    fn dev_weather_action(&mut self, a: Action) {
        match a {
            Action::WeatherPreset(file, secs) => {
                self.change_weather(Some(file), true, secs.max(0.5));
            }
            Action::WeatherNext => self.step_weather(),
            Action::WeatherCustom(c) => self.set_custom_weather(*c),
            Action::WeatherFromCurrent => self.current_weather_as_custom(),
            Action::WeatherMetar(icao) => {
                let icao = icao.trim().to_ascii_uppercase();
                if icao.len() < 3 {
                    self.service_msg = Some(("Enter an ICAO station code".into(), 3.0));
                    return;
                }
                if self
                    .lan
                    .as_ref()
                    .is_some_and(|l| l.role == ::network::Role::Client)
                {
                    self.service_msg =
                        Some(("In a LAN session the host sets the weather".into(), 3.0));
                    return;
                }
                if self.metar_locked() {
                    self.service_msg = Some((
                        "The weather cannot be changed while the METAR sync is on".into(),
                        3.0,
                    ));
                    return;
                }
                let (tx, rx) = std::sync::mpsc::channel();
                self.metar_rx = Some(rx);
                self.metar_once = true;
                let station = icao.clone();
                std::thread::spawn(move || {
                    let _ = tx.send(crate::weather_setup::try_metar(&station));
                });
                self.service_msg = Some((format!("Weather: loading METAR for {icao}"), 4.0));
            }
            Action::WeatherCycle(on) => {
                if on {
                    if self.weather_cycle.is_none() {
                        let seed = std::time::SystemTime::now()
                            .duration_since(std::time::UNIX_EPOCH)
                            .map(|d| d.as_nanos() as u64)
                            .unwrap_or(7);
                        self.weather_cycle = Some(crate::weather_cycle::Cycle::new(seed));
                    }
                } else {
                    self.weather_cycle = None;
                }
            }
            Action::WeatherCycleNow => {
                if let Some(c) = self.weather_cycle.as_mut() {
                    c.next_in = 0.0;
                } else {
                    self.service_msg = Some(("The weather cycle is off".into(), 3.0));
                }
            }
            Action::WeatherWetness(v) => {
                self.wetness = v.clamp(0.0, 1.0);
            }
            Action::WeatherSave(name) => {
                let Some(w) = self.weather.clone() else {
                    return;
                };
                let safe: String = name
                    .trim()
                    .chars()
                    .map(|c| {
                        if c.is_ascii_alphanumeric() || c == '-' || c == ' ' {
                            c
                        } else {
                            '_'
                        }
                    })
                    .collect();
                if safe.is_empty() {
                    self.service_msg = Some(("Give the weather a name".into(), 3.0));
                    return;
                }
                let dir = crate::startup::content_dir()
                    .unwrap_or_else(|| self.args.root.clone())
                    .join("Weather");
                let path = dir.join(format!("{safe}.owt"));
                let text = super::weather_ui::to_owt(&safe, &w);
                let res = std::fs::create_dir_all(&dir).and_then(|_| std::fs::write(&path, text));
                self.service_msg = Some(match res {
                    Ok(_) => (format!("Weather saved: {}", path.display()), 5.0),
                    Err(e) => (format!("Weather not saved: {e}"), 5.0),
                });
            }
            Action::SetTime(t) => {
                self.clock.time = t.rem_euclid(86400.0);
            }
            Action::SetDay(d) => {
                let max = ::simulation::clock::days_in_year(self.clock.year);
                self.clock.day_of_year = d.clamp(1, max);
                self.follow_date();
            }
            _ => {}
        }
    }

    pub(crate) fn dev_actions(&mut self, event_loop: &winit::event_loop::ActiveEventLoop) {
        let (actions, released) = match self.devtools.as_mut() {
            Some(d) => (d.take_actions(), std::mem::take(&mut d.release)),
            None => return,
        };
        if let Some(p) = self.player.as_mut() {
            for name in &released {
                p.action(name, false);
            }
        }
        let mut pressed: Vec<String> = Vec::new();
        for a in actions {
            match a {
                Action::QuickSave => self.quick_save(),
                Action::LoadQuickSave => {
                    if self.load_quicksave() {
                        self.finish_session();
                        crate::platform::exit(event_loop);
                    }
                }
                Action::CopyCode => self.copy_server_code(),
                Action::CopyTrafficReport(report) => {
                    log::info!("{report}");
                    #[cfg(not(target_os = "android"))]
                    {
                        std::thread_local! { static CLIPBOARD: std::cell::RefCell<Option<arboard::Clipboard>> = const { std::cell::RefCell::new(None) }; }
                        CLIPBOARD.with(|cell| {
                            let mut cb = cell.borrow_mut();
                            if cb.is_none() { *cb = arboard::Clipboard::new().ok(); }
                            if let Some(cb) = cb.as_mut() {
                                if let Err(e) = cb.set_text(report) { log::warn!("Traffic report clipboard: {e}"); }
                            }
                        });
                    }
                }
                Action::Vehicle(name) => {
                    if let Some(p) = self.player.as_mut() {
                        p.action(&name, true);
                        pressed.push(name);
                    }
                }
                Action::Cockpit(i) => {
                    if let Some(p) = self.player.as_mut() {
                        p.press_control(i);
                    }
                }
                Action::VehicleSaloonLights => {
                    if let Some(p) = self.player.as_mut() {
                        p.toggle_saloon_lights();
                    }
                }
                Action::VehicleStartUp => {
                    if let Some(p) = self.player.as_mut() {
                        p.start_up();
                    }
                }
                a @ (Action::WeatherPreset(..)
                | Action::WeatherNext
                | Action::WeatherCustom(_)
                | Action::WeatherFromCurrent
                | Action::WeatherMetar(_)
                | Action::WeatherCycle(_)
                | Action::WeatherCycleNow
                | Action::WeatherWetness(_)
                | Action::WeatherSave(_)
                | Action::SetTime(_)
                | Action::SetDay(_)) => self.dev_weather_action(a),
                Action::OpenLan(port) => {
                    if self.lan.is_some() {
                        self.service_msg = Some(("Already in a LAN session".into(), 3.0));
                        continue;
                    }
                    let (p, try_next) = if port == 0 {
                        (::network::DEFAULT_PORT, true)
                    } else {
                        (port, false)
                    };
                    match ::network::LanSession::host(
                        p,
                        &crate::lan::player_name(&self.args),
                        crate::lan::world_info(&self.args),
                        try_next,
                    ) {
                        Ok(s) => {
                            self.lan = Some(s);
                            self.copy_server_code();
                        }
                        Err(e) => {
                            self.service_msg =
                                Some((format!("Cannot open LAN on port {p}: {e}"), 5.0));
                        }
                    }
                }
                Action::Connect(addr) => {
                    let Ok(exe) = std::env::current_exe() else {
                        continue;
                    };
                    let mut cmd = std::process::Command::new(exe);
                    cmd.arg("--root")
                        .arg(&self.args.root)
                        .arg("--no-menu")
                        .arg("--lan-join")
                        .arg(&addr);
                    match cmd.spawn() {
                        Ok(_) => {
                            self.finish_session();
                            crate::platform::exit(event_loop);
                        }
                        Err(e) => {
                            self.service_msg =
                                Some((format!("Could not start the game: {e}"), 5.0));
                        }
                    }
                }
            }
        }
        if let Some(d) = self.devtools.as_mut() {
            d.release.extend(pressed);
        }
    }
}

/// Whether there is a quick save to load, asked of the disk once a second: asked every
/// frame, it took the better part of a millisecond of each.
fn quicksave_exists(root: &std::path::Path) -> bool {
    static LAST: std::sync::Mutex<Option<(std::time::Instant, bool)>> = std::sync::Mutex::new(None);
    let mut last = LAST.lock().unwrap_or_else(|e| e.into_inner());
    match *last {
        Some((at, exists)) if at.elapsed().as_secs_f32() < 1.0 => exists,
        _ => {
            let exists = crate::startup::content_dir()
                .unwrap_or_else(|| root.to_path_buf())
                .join("Situations")
                .join("quicksave.osn")
                .exists();
            *last = Some((std::time::Instant::now(), exists));
            exists
        }
    }
}

fn walk_paths(def: &::legacy_vehicle::Vehicle) -> (Vec<[f32; 3]>, Vec<(i32, i32, bool)>) {
    type Cache = Option<(std::path::PathBuf, Vec<[f32; 3]>, Vec<(i32, i32, bool)>)>;
    static CACHE: std::sync::Mutex<Cache> = std::sync::Mutex::new(None);
    let mut c = CACHE.lock().unwrap_or_else(|e| e.into_inner());
    if let Some((p, pts, links)) = c.as_ref() {
        if *p == def.path {
            return (pts.clone(), links.clone());
        }
    }
    let (pts, links) = def
        .paths
        .as_ref()
        .and_then(|rel| {
            ::legacy_vehicle::VehiclePaths::load(&::legacy_config::resolve_path(def.dir(), rel)).ok()
        })
        .map(|vp| {
            (
                vp.points.iter().map(|q| q.pos).collect::<Vec<[f32; 3]>>(),
                vp.links,
            )
        })
        .unwrap_or_default();
    *c = Some((def.path.clone(), pts.clone(), links.clone()));
    (pts, links)
}
