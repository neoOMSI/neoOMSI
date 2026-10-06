#![allow(unused_imports)]
use super::types::*;
use omsi_sim::collision::Obb;

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
            host: l.role == omsi_net::Role::Host,
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
        let quicksave = crate::startup::content_dir()
            .unwrap_or_else(|| self.args.root.clone())
            .join("Situations")
            .join("quicksave.osn")
            .exists();
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
                    let r = crate::lights::spill_radius(&ls.spill);
                    for l in scene
                        .lights
                        .iter()
                        .filter(|l| l.radius == r && l.direction.length_squared() > 0.1)
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
        let vehicle = self.player.as_ref().map(|p| VehicleInfo {
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
            walk_points: walk_paths(&p.vehicle.ty.def).0,
            walk_links: walk_paths(&p.vehicle.ty.def).1,
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
                Action::OpenLan(port) => {
                    if self.lan.is_some() {
                        self.service_msg = Some(("Already in a LAN session".into(), 3.0));
                        continue;
                    }
                    let (p, try_next) = if port == 0 {
                        (omsi_net::DEFAULT_PORT, true)
                    } else {
                        (port, false)
                    };
                    match omsi_net::LanSession::host(
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

fn walk_paths(def: &omsi_vehicle::Vehicle) -> (Vec<[f32; 3]>, Vec<(i32, i32, bool)>) {
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
            omsi_vehicle::VehiclePaths::load(&omsi_cfg::resolve_path(def.dir(), rel)).ok()
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
