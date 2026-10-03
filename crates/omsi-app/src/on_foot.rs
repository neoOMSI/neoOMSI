use crate::App;
use crate::humans::{AvatarCmd, BusId, Humans};
use glam::{DVec2, DVec3};
use omsi_sim::collision::Obb;
use winit::keyboard::KeyCode;

pub(crate) const AVATAR_KEY: u32 = 0;
pub(crate) const REMOTE_KEY: u32 = 1_000_000;

const WALK: f64 = 1.45;
const RUN: f64 = 4.3;
const ACCEL: f64 = 7.0;
const JUMP: f64 = 4.0;
const DOOR_REACH: f64 = 3.2;
const RADIUS: f64 = 0.28;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum FootCam {
    First,
    Free,
}

pub(crate) struct OnFoot {
    pub pos: DVec3,
    pub heading: f64,
    pub vel: DVec2,
    pub lift: f64,
    pub vz: f64,
    pub seat: Option<(BusId, usize)>,
    pub inside: Option<(BusId, glam::Vec3)>,
    pub cam: FootCam,
    pub yaw: f32,
    pub pitch: f32,
    pub eye: Option<DVec3>,
    pub eye_yaw: f32,
    pub lag: DVec3,
    pub settle: f32,
    pub view_before: String,
    pub kind: u64,
    pub face_seat: bool,
    pub transit: Option<Transit>,
    pub arrive: Option<Then>,
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub(crate) enum Then {
    Nothing,
    Wheel,
    Placed(u64),
}

#[derive(Debug, Clone, Copy)]
pub(crate) struct Transit {
    pub from: DVec3,
    pub to: DVec3,
    pub t: f32,
    pub dur: f32,
    pub after: Option<(BusId, glam::Vec3)>,
    pub then: Then,
}

impl Transit {
    fn walk(from: DVec3, to: DVec3, after: Option<(BusId, glam::Vec3)>) -> Transit {
        let d = (to - from).length() as f32;
        Transit {
            from,
            to,
            t: 0.0,
            dur: (d / WALK as f32).clamp(0.35, 2.0),
            after,
            then: Then::Nothing,
        }
    }

    fn walk_in(from: DVec3, to: DVec3, then: Then) -> Transit {
        let d = (to - from).length() as f32;
        Transit {
            from,
            to,
            t: 0.0,
            dur: (d / WALK as f32).clamp(0.6, 2.5),
            after: None,
            then,
        }
    }
}

impl OnFoot {
    fn grounded(&self) -> bool {
        self.lift <= 1e-4 && self.vz <= 0.0
    }
}

fn wrap(a: f64) -> f64 {
    (a + 180.0).rem_euclid(360.0) - 180.0
}

const DOOR_OUT_REACH: f64 = 3.2;

fn cabin_local(h: &Humans, bus: BusId, p: DVec2, z: f32) -> Option<glam::Vec3> {
    let (o, _) = h.cabin_world(bus, glam::Vec3::ZERO)?;
    let (ex, _) = h.cabin_world(bus, glam::Vec3::X)?;
    let (ey, _) = h.cabin_world(bus, glam::Vec3::Y)?;
    let (dx, dy) = ((ex - o).truncate(), (ey - o).truncate());
    let r = p - o.truncate();
    Some(glam::Vec3::new(
        (r.dot(dx) / dx.length_squared()) as f32,
        (r.dot(dy) / dy.length_squared()) as f32,
        z,
    ))
}

fn push_out(p: DVec2, o: &Obb, r: f64) -> DVec2 {
    let (s, c) = o.heading.sin_cos();
    let right = DVec2::new(c, -s);
    let fwd = DVec2::new(s, c);
    let d = p - o.center;
    let (lx, ly) = (d.dot(right), d.dot(fwd));
    let (hx, hy) = (o.half.x + r, o.half.y + r);
    if lx.abs() >= hx || ly.abs() >= hy {
        return p;
    }
    if hx - lx.abs() < hy - ly.abs() {
        o.center + right * (hx * lx.signum()) + fwd * ly
    } else {
        o.center + right * lx + fwd * (hy * ly.signum())
    }
}

fn outside_spot(
    h: &mut Humans,
    world: Option<&crate::scene::World>,
    v: &omsi_sim::VehicleInstance,
    others: &[Obb],
    side: f64,
    near: DVec3,
) -> Option<DVec3> {
    let hd = v.heading.to_radians();
    let (fwd, right) = (
        DVec2::new(hd.sin(), hd.cos()),
        DVec2::new(hd.cos(), -hd.sin()),
    );
    let half_w =
        v.ty.def
            .bounding_box
            .map(|b| b[0] as f64 * 0.5)
            .unwrap_or(1.25);
    let door = h
        .vehicle_doors(v)
        .into_iter()
        .filter(|d| (*d - v.position).truncate().dot(right) * side > 0.0)
        .min_by(|a, b| (*a - near).length().total_cmp(&(*b - near).length()));
    let door = door.filter(|d| (*d - near).truncate().length() < DOOR_OUT_REACH)?;
    let _ = (fwd, half_w);
    outside_at(world, v, others, door)
}

fn outside_at(
    world: Option<&crate::scene::World>,
    v: &omsi_sim::VehicleInstance,
    others: &[Obb],
    door: DVec3,
) -> Option<DVec3> {
    let own =
        v.ty.def
            .bounding_box
            .map(|bb| Obb::from_box(bb, v.position, v.heading));
    let mut p = door.truncate();
    if let Some(o) = own.as_ref() {
        p = push_out(p, o, RADIUS + 0.15);
    }
    let z = world
        .and_then(|w| w.walk_height_near(p.x, p.y, v.position.z))
        .unwrap_or(v.position.z);
    let blocked = |q: DVec2| -> bool {
        let walls = world
            .map(|w| {
                let probe = Obb {
                    center: q,
                    half: DVec2::splat(1.0),
                    heading: 0.0,
                    z0: z - 1.0,
                    z1: z + 2.5,
                    velocity: DVec2::ZERO,
                    mass: 0.0,
                    pole: None,
                    id: -1,
                };
                w.collision.lock().obstacles_near(&probe)
            })
            .unwrap_or_default();
        walls
            .iter()
            .filter(|o| o.z0 < z + 1.6 && o.z1 > z + 0.45)
            .chain(others.iter())
            .any(|o| (push_out(q, o, RADIUS) - q).length() > 0.05)
    };
    if blocked(p) {
        return None;
    }
    if (z - v.position.z).abs() > 1.5 {
        return None;
    }
    Some(DVec3::new(p.x, p.y, z))
}

impl App {
    fn vehicle_boxes(&self, at: DVec2, r: f64) -> Vec<Obb> {
        let mut boxes = Vec::new();
        let mut add = |v: &omsi_sim::VehicleInstance| {
            if (v.position.truncate() - at).length() > r {
                return;
            }
            if let Some(bb) = v.ty.def.bounding_box {
                boxes.push(Obb::from_box(bb, v.position, v.heading));
            }
        };
        if let Some(p) = self.player.as_ref() {
            add(&p.vehicle);
        }
        for q in &self.placed {
            add(&q.vehicle);
        }
        if let Some(t) = self.traffic.as_ref() {
            for c in &t.cars {
                add(&c.vehicle);
            }
        }
        for rm in self.remotes.remotes.values() {
            add(rm.vehicle());
        }
        boxes
    }

    pub(crate) fn get_up(&mut self) {
        if self.on_foot.is_some() {
            return;
        }
        let others: Vec<Obb> = {
            let at = self
                .player
                .as_ref()
                .map(|p| p.vehicle.position.truncate())
                .unwrap_or_default();
            let own = self.player.as_ref().and_then(|p| {
                p.vehicle
                    .ty
                    .def
                    .bounding_box
                    .map(|bb| Obb::from_box(bb, p.vehicle.position, p.vehicle.heading))
            });
            self.vehicle_boxes(at, 30.0)
                .into_iter()
                .filter(|o| {
                    own.map(|w| (w.center - o.center).length() > 0.01)
                        .unwrap_or(true)
                })
                .collect()
        };
        let Some(p) = self.player.as_mut() else {
            return;
        };
        p.axes.release_all();
        if self.humans.is_none() {
            let mut h = crate::humans::Humans::new(&self.args.root);
            h.avatar_only = true;
            h.set_cabin(&mut p.vehicle);
            self.humans = Some(h);
        }
        let v = &p.vehicle;
        let driver_ty = p.driver.as_ref().map(|d| d.human_type());
        let doors: Vec<DVec3> = self
            .humans
            .as_mut()
            .and_then(|h| h.vehicle_driver_door(v))
            .into_iter()
            .collect();
        let h = v.heading.to_radians();
        let (fwd, right) = (DVec2::new(h.sin(), h.cos()), DVec2::new(h.cos(), -h.sin()));
        let half =
            v.ty.def
                .bounding_box
                .map(|b| (b[0] as f64 * 0.5, b[1] as f64 * 0.5 + b[4] as f64))
                .unwrap_or((1.25, 5.5));
        let pos = doors.first().copied().unwrap_or_else(|| {
            let xy = v.position.truncate() + fwd * (half.1 - 1.8) + right * (half.0 + 0.6);
            DVec3::new(xy.x, xy.y, v.position.z)
        });
        let pos = match self
            .world
            .as_ref()
            .and_then(|w| w.walk_height_near(pos.x, pos.y, v.position.z))
        {
            Some(z) if (z - pos.z).abs() < 2.0 => DVec3::new(pos.x, pos.y, z),
            _ => pos,
        };
        let away = (pos.truncate() - v.position.truncate()).dot(right).signum();
        let face = (right * away).x.atan2((right * away).y).to_degrees();
        let kind = match (driver_ty, self.humans.as_mut()) {
            (Some(t), Some(h)) => h.type_index(t) as u64,
            _ => self.args.root.to_string_lossy().len() as u64 * 7 + 3,
        };
        let look_yaw = self
            .camera
            .as_ref()
            .map(|c| c.yaw as f64)
            .unwrap_or(v.heading);
        let ly = look_yaw.to_radians();
        let look = DVec2::new(ly.sin(), ly.cos());
        let stand = self.humans.as_mut().and_then(|h| h.driver_stand(v));
        let seat_w = stand
            .and_then(|l| {
                self.humans
                    .as_mut()
                    .and_then(|h| h.vehicle_cabin_world(v, l))
            })
            .unwrap_or(v.position);
        let side = look.dot(right);
        let cab_door = self.humans.as_mut().and_then(|h| h.vehicle_cab_door(v));
        let outside = match (cab_door, side.abs() > 0.45, self.humans.as_mut()) {
            (Some(d), _, Some(_)) => outside_at(self.world.as_deref(), v, &others, d),
            (None, true, Some(h)) => {
                outside_spot(h, self.world.as_deref(), v, &others, side.signum(), seat_w)
            }
            _ => None,
        };
        let _ = face;
        let (pos, face, inside, transit) = match (outside, stand) {
            (_, Some(l)) => (seat_w, look_yaw, Some((BusId::Player, l)), None),
            (Some(o), None) => (pos, look_yaw, None, Some(Transit::walk(pos, o, None))),
            (None, None) => (pos, look_yaw, None, None),
        };
        self.on_foot = Some(OnFoot {
            pos,
            heading: face,
            vel: DVec2::ZERO,
            lift: 0.0,
            vz: 0.0,
            seat: None,
            inside,
            cam: FootCam::First,
            yaw: face as f32,
            pitch: -8.0,
            eye: self.camera.as_ref().map(|c| c.position),
            eye_yaw: self.camera.as_ref().map(|c| c.yaw).unwrap_or(face as f32),
            lag: DVec3::ZERO,
            settle: if self
                .player
                .as_ref()
                .is_some_and(|p| p.vehicle.physics.velocity_kmh().abs() < 3.0)
            {
                1.0
            } else {
                0.0
            },
            view_before: if self.view == "foot" {
                "driver".into()
            } else {
                self.view.clone()
            },
            kind,
            face_seat: false,
            transit,
            arrive: None,
        });
        self.view = "foot".into();
    }

    pub(crate) fn remove_driven_vehicle(&mut self) {
        if self.player.is_none() {
            self.service_msg = Some(("There is no vehicle to remove: you are on foot".into(), 3.0));
            return;
        }
        let stand = {
            let p = self.player.as_ref().unwrap();
            let v = &p.vehicle;
            if self.humans.is_none() {
                let mut h = crate::humans::Humans::new(&self.args.root);
                h.avatar_only = true;
                self.humans = Some(h);
            }
            let door = self.humans.as_mut().and_then(|h| h.vehicle_driver_door(v));
            let h = v.heading.to_radians();
            let (fwd, right) = (DVec2::new(h.sin(), h.cos()), DVec2::new(h.cos(), -h.sin()));
            let half =
                v.ty.def
                    .bounding_box
                    .map(|b| (b[0] as f64 * 0.5, b[1] as f64 * 0.5 + b[4] as f64))
                    .unwrap_or((1.25, 5.5));
            let p = door.unwrap_or_else(|| {
                let xy = v.position.truncate() + fwd * (half.1 - 1.8) - right * (half.0 + 0.8);
                DVec3::new(xy.x, xy.y, v.position.z)
            });
            let z = self
                .world
                .as_ref()
                .and_then(|w| w.walk_height_near(p.x, p.y, p.z))
                .unwrap_or(p.z);
            (DVec3::new(p.x, p.y, z), v.heading)
        };
        if let Some(f) = self.on_foot.take() {
            if let Some(h) = self.humans.as_mut() {
                h.avatar_remove(AVATAR_KEY);
            }
            let _ = f;
        }
        let mut p = self.player.take().unwrap();
        if let (Some(a), Some(mut ss)) = (self.audio.as_ref(), p.sounds.take()) {
            ss.stop_all(a);
        }
        if let (Some(w), Some(r), Some(scene)) = (
            self.world.clone(),
            self.renderer.as_ref(),
            self.scene.as_mut(),
        ) {
            if let Some(mut d) = p.driver.take() {
                d.hide(r, scene);
            }
            if let Some(h) = self.humans.as_mut() {
                h.evict(BusId::Player, &w);
            }
            w.release_vehicle(r, scene, p.render);
            for t in p.trailer_renders {
                w.release_vehicle(r, scene, t);
            }
        }
        let name = format!(
            "{} {}",
            p.vehicle.ty.def.manufacturer, p.vehicle.ty.def.type_name
        );
        self.start_on_foot(stand.0, stand.1);
        self.service_msg = Some((
            format!(
                "{} removed: you are on foot (Esc menu: Place a vehicle, or G at a bus's driver's door)",
                name.trim()
            ),
            6.0,
        ));
    }

    pub(crate) fn start_on_foot(&mut self, pos: DVec3, heading: f64) {
        if self.humans.is_none() {
            let mut h = crate::humans::Humans::new(&self.args.root);
            h.avatar_only = true;
            self.humans = Some(h);
        }
        self.on_foot = Some(OnFoot {
            pos,
            heading,
            vel: DVec2::ZERO,
            lift: 0.0,
            vz: 0.0,
            seat: None,
            inside: None,
            cam: FootCam::First,
            yaw: heading as f32,
            pitch: -5.0,
            eye: None,
            eye_yaw: heading as f32,
            lag: DVec3::ZERO,
            settle: 0.0,
            view_before: "driver".into(),
            kind: self.args.root.to_string_lossy().len() as u64 * 7 + 3,
            face_seat: false,
            transit: None,
            arrive: None,
        });
        self.view = "foot".into();
    }

    fn placed_cab_near(&mut self, pos: DVec3) -> Option<usize> {
        let h = self.humans.as_mut()?;
        let mut best: Option<(usize, f64)> = None;
        for (k, q) in self.placed.iter().enumerate() {
            let v = &q.vehicle;
            let door = h
                .vehicle_driver_door(v)
                .or_else(|| h.driver_stand(v).and_then(|l| h.vehicle_cabin_world(v, l)));
            if let Some(d) = door {
                let dist = (d - pos).truncate().length();
                if dist < DOOR_REACH + 0.6 && best.map(|b| dist < b.1).unwrap_or(true) {
                    best = Some((k, dist));
                }
            }
        }
        best.map(|b| b.0)
    }

    pub(crate) fn take_placed(&mut self, k: usize) {
        if k >= self.placed.len() {
            return;
        }
        let mut next = self.placed.remove(k);
        if let Some(a) = self.audio.as_ref() {
            next.load_sounds(a);
        }
        next.vehicle.host.auto_clutch = if self.settings.auto_clutch { 1.0 } else { 0.0 };
        if let Some(now) = self.player.take() {
            let mut now = now;
            if let (Some(a), Some(mut ss)) = (self.audio.as_ref(), now.sounds.take()) {
                ss.stop_all(a);
            }
            if let Some(h) = self.humans.as_mut() {
                h.player_bus_swapped(now.uid, next.uid, &mut next.vehicle);
            }
            self.placed.push(now);
        } else if let Some(h) = self.humans.as_mut() {
            h.player_bus_swapped(0, next.uid, &mut next.vehicle);
        }
        let name = format!(
            "{} {}",
            next.vehicle.ty.def.manufacturer, next.vehicle.ty.def.type_name
        );
        self.player = Some(next);
        if let Some(f) = self.on_foot.take() {
            if let Some(h) = self.humans.as_mut() {
                h.avatar_remove(AVATAR_KEY);
            }
            let _ = f;
        }
        self.view = "driver".into();
        self.sync_view_look();
        self.look = (0.0, 0.0);
        if let (Some(cam), Some(p)) = (self.camera.as_ref(), self.player.as_ref()) {
            self.camera = Some(p.camera("driver", cam));
        }
        self.service_msg = Some((format!("Now driving: {}", name.trim()), 4.0));
    }

    pub(crate) fn back_to_bus(&mut self) {
        if self.on_foot.is_some() && self.player.is_some() {
            self.sit_at_the_wheel();
            self.service_msg = Some(("Back at the wheel".into(), 3.0));
        }
    }

    fn walk_to_wheel(&mut self) {
        let to = self.player.as_ref().and_then(|p| {
            let v = &p.vehicle;
            let h = self.humans.as_mut()?;
            h.driver_stand(v).and_then(|l| h.vehicle_cabin_world(v, l))
        });
        match to {
            Some(to) => self.walk_in(to, Then::Wheel),
            None => self.sit_at_the_wheel(),
        }
    }

    fn walk_in(&mut self, to: DVec3, then: Then) {
        if let Some(f) = self.on_foot.as_mut() {
            f.transit = Some(Transit::walk_in(f.pos, to, then));
            f.seat = None;
            f.inside = None;
            f.vel = DVec2::ZERO;
        }
    }

    fn sit_at_the_wheel(&mut self) {
        let Some(f) = self.on_foot.take() else { return };
        if let Some(h) = self.humans.as_mut() {
            h.avatar_remove(AVATAR_KEY);
        }
        self.view = if f.view_before == "outside" || f.view_before == "driver" {
            f.view_before
        } else {
            "driver".into()
        };
        if self.view == "driver" {
            self.cam_blend.entering = true;
        }
        self.sync_view_look();
        self.look = (0.0, 0.0);
        if self
            .service_msg
            .as_ref()
            .is_some_and(|m| m.0.starts_with("On foot"))
        {
            self.service_msg = None;
        }
    }

    fn use_seat(&mut self) {
        let Some(f) = self.on_foot.as_ref() else {
            return;
        };
        let Some(h) = self.humans.as_ref() else {
            return;
        };
        if let Some((bus, k)) = f.seat {
            if let Some(l) = h.seat_stand(bus, k) {
                if let Some((w, hd)) = h.cabin_world(bus, l) {
                    let f = self.on_foot.as_mut().unwrap();
                    f.pos = w;
                    f.seat = None;
                    f.inside = Some((bus, l));
                    f.vel = DVec2::ZERO;
                    f.heading = hd;
                    return;
                }
            }
            let at = h.avatar_body(AVATAR_KEY).map(|b| b.0).unwrap_or(f.pos);
            let door = h
                .bus_doors(bus)
                .into_iter()
                .min_by(|a, b| (*a - at).length().total_cmp(&(*b - at).length()));
            let Some(door) = door else { return };
            let z = self
                .world
                .as_ref()
                .and_then(|w| w.walk_height_near(door.x, door.y, door.z))
                .unwrap_or(door.z);
            let away = h
                .bus_center(bus)
                .map(|c| door.truncate() - c.truncate())
                .unwrap_or(DVec2::Y);
            let face = away.x.atan2(away.y).to_degrees();
            let f = self.on_foot.as_mut().unwrap();
            f.transit = Some(Transit::walk(at, DVec3::new(door.x, door.y, z), None));
            f.pos = at;
            f.seat = None;
            f.vel = DVec2::ZERO;
            f.heading = face;
            f.yaw = face as f32;
            return;
        }
        let pos = f.pos;
        if let Some((bus, _)) = f.inside {
            if bus == BusId::Player {
                let stand = self.player.as_ref().and_then(|p| {
                    self.humans.as_mut().and_then(|h| {
                        h.driver_stand(&p.vehicle)
                            .and_then(|l| h.vehicle_cabin_world(&p.vehicle, l))
                    })
                });
                if stand
                    .map(|s| (s - pos).truncate().length() < 2.5)
                    .unwrap_or(true)
                {
                    self.sit_at_the_wheel();
                    return;
                }
            }
            let Some(h) = self.humans.as_ref() else {
                return;
            };
            match h.seat_nearest(bus, pos, 2.5) {
                Some(k) => {
                    let f = self.on_foot.as_mut().unwrap();
                    f.seat = Some((bus, k));
                    f.inside = None;
                    f.face_seat = true;
                    f.vel = DVec2::ZERO;
                }
                None => self.service_msg = Some(("No free seat near: walk up to one".into(), 3.0)),
            }
            return;
        }
        if let Some(k) = self.placed_cab_near(pos) {
            let own_near = self
                .player
                .as_ref()
                .and_then(|p| {
                    self.humans
                        .as_mut()
                        .and_then(|h| h.vehicle_driver_door(&p.vehicle))
                })
                .map(|d| (d - pos).truncate().length() < DOOR_REACH)
                .unwrap_or(false);
            if !own_near {
                let uid = self.placed[k].uid;
                let seat = {
                    let v = &self.placed[k].vehicle;
                    self.humans
                        .as_mut()
                        .and_then(|h| h.driver_stand(v).and_then(|l| h.vehicle_cabin_world(v, l)))
                };
                match seat {
                    Some(to) => self.walk_in(to, Then::Placed(uid)),
                    None => self.take_placed(k),
                }
                return;
            }
        }
        let own_front = self.player.as_ref().and_then(|p| {
            self.humans
                .as_mut()
                .and_then(|h| h.vehicle_driver_door(&p.vehicle))
        });
        let Some(h) = self.humans.as_ref() else {
            return;
        };
        if omsi_cfg::env::var_os("OMSI_DEBUG_FOOT").is_some() {
            log::info!(
                "on foot at ({:.1}, {:.1}): G - own bus doors {:?}, a seat near: {:?}",
                pos.x,
                pos.y,
                h.bus_doors(BusId::Player)
                    .iter()
                    .map(|d| ((d.x * 10.0).round() / 10.0, (d.y * 10.0).round() / 10.0))
                    .collect::<Vec<_>>(),
                h.seat_near(pos, DOOR_REACH, None).map(|s| (s.bus, s.seat))
            );
        }
        let own_doors = self
            .humans
            .as_ref()
            .is_some_and(|h| !h.cabin_doors(BusId::Player).is_empty());
        if own_doors && self.player.is_some() {
            self.service_msg = Some(("Walk in through an open door of the bus".into(), 3.0));
            return;
        }
        if let Some(d) = own_front {
            if (d - pos).truncate().length() < DOOR_REACH {
                self.walk_to_wheel();
                return;
            }
        }
        let cab = self.player.as_ref().and_then(|p| {
            let v = &p.vehicle;
            let h = self.humans.as_mut()?;
            h.driver_stand(v).and_then(|l| h.vehicle_cabin_world(v, l))
        });
        if cab
            .map(|c| (c - pos).truncate().length() < 3.8)
            .unwrap_or(false)
        {
            self.walk_to_wheel();
            return;
        }
        self.service_msg = Some((
            "Walk in through an open door of the bus, then G by a seat sits down".into(),
            4.0,
        ));
    }

    pub(crate) fn foot_key(
        &mut self,
        code: KeyCode,
        pressed: bool,
        repeat: bool,
        ctrl: bool,
        shift: bool,
    ) -> bool {
        if self.on_foot.is_none() {
            if pressed && !repeat && code == KeyCode::KeyG && ctrl && shift && self.player.is_some()
            {
                self.get_up();
                return true;
            }
            return false;
        }
        match code {
            KeyCode::Escape | KeyCode::F12 | KeyCode::KeyP | KeyCode::KeyV | KeyCode::Slash => {
                false
            }
            KeyCode::F1 => {
                if pressed && !repeat {
                    if let Some(f) = self.on_foot.as_mut() {
                        f.cam = FootCam::First;
                        f.eye = None;
                    }
                    self.view = "foot".into();
                }
                true
            }
            KeyCode::F4 => {
                if pressed && !repeat {
                    if let Some(f) = self.on_foot.as_mut() {
                        f.cam = FootCam::Free;
                        f.vel = DVec2::ZERO;
                    }
                    self.view = "free".into();
                    self.ego = false;
                }
                true
            }
            KeyCode::F2 | KeyCode::F3 => true,
            KeyCode::KeyM if shift && !ctrl => {
                if pressed && !repeat {
                    if let Some(n) = self.navigator.as_mut() {
                        n.toggle_map();
                    }
                }
                true
            }
            _ if self
                .on_foot
                .as_ref()
                .map(|f| f.cam == FootCam::Free)
                .unwrap_or(false) =>
            {
                false
            }
            KeyCode::KeyG => {
                if pressed && !repeat {
                    self.use_seat();
                }
                true
            }
            KeyCode::Space => {
                if let (true, false, Some(f)) = (pressed, repeat, self.on_foot.as_mut()) {
                    if f.seat.is_none() && f.grounded() {
                        f.vz = JUMP;
                    }
                }
                true
            }
            _ => true,
        }
    }

    pub(crate) fn foot_look(&mut self, dx: f32, dy: f32) {
        if let Some(f) = self.on_foot.as_mut() {
            f.yaw = (f.yaw + dx).rem_euclid(360.0);
            f.pitch = (f.pitch - dy).clamp(-80.0, 80.0);
        }
    }

    pub(crate) fn tick_on_foot(&mut self, dt: f32) {
        match self.on_foot.as_mut().and_then(|f| f.arrive.take()) {
            Some(Then::Wheel) if self.player.is_some() => {
                self.sit_at_the_wheel();
                self.service_msg = Some(("Back at the wheel".into(), 2.0));
                return;
            }
            Some(Then::Placed(uid)) => {
                if let Some(k) = self.placed.iter().position(|q| q.uid == uid) {
                    self.take_placed(k);
                    return;
                }
            }
            _ => {}
        }
        let Some(mut f) = self.on_foot.take() else {
            return;
        };
        let dt64 = dt as f64;
        let key = |k: KeyCode| self.keys.contains(&k);
        if key(KeyCode::ArrowLeft) {
            f.yaw -= 90.0 * dt;
        }
        if key(KeyCode::ArrowRight) {
            f.yaw += 90.0 * dt;
        }
        if key(KeyCode::ArrowUp) {
            f.pitch = (f.pitch + 60.0 * dt).min(80.0);
        }
        if key(KeyCode::ArrowDown) {
            f.pitch = (f.pitch - 60.0 * dt).max(-80.0);
        }
        if let (Some((bus, _)), Some(h)) = (f.seat, self.humans.as_ref()) {
            if !h.bus_here(bus) {
                f.seat = None;
            }
        }
        if let (Some((bus, _)), Some(h)) = (f.inside, self.humans.as_ref()) {
            if !h.bus_here(bus) && !(bus == BusId::Player && self.player.is_some()) {
                f.inside = None;
            }
        }
        let free = f.cam == FootCam::Free;
        if let Some(mut tr) = f.transit.filter(|_| !self.paused) {
            tr.t += dt;
            let k = (tr.t / tr.dur).clamp(0.0, 1.0) as f64;
            f.inside = None;
            f.seat = None;
            f.lift = 0.0;
            f.vz = 0.0;
            f.pos = tr.from.lerp(tr.to, k);
            f.vel = (tr.to - tr.from).truncate() / tr.dur as f64;
            if f.vel.length() > 0.1 {
                let course = f.vel.x.atan2(f.vel.y).to_degrees();
                let d = wrap(course - f.heading);
                f.heading = wrap(f.heading + d.clamp(-220.0 * dt64, 220.0 * dt64));
            }
            if k >= 1.0 {
                f.transit = None;
                f.inside = tr.after;
                f.vel = DVec2::ZERO;
                if tr.then != Then::Nothing {
                    f.arrive = Some(tr.then);
                }
            } else {
                f.transit = Some(tr);
            }
        }
        if f.seat.is_none() && !self.paused && !free && f.transit.is_none() {
            let y = (f.yaw as f64).to_radians();
            let (fwd, right) = (DVec2::new(y.sin(), y.cos()), DVec2::new(y.cos(), -y.sin()));
            let mut dir = DVec2::ZERO;
            if key(KeyCode::KeyW) {
                dir += fwd;
            }
            if key(KeyCode::KeyS) {
                dir -= fwd;
            }
            if key(KeyCode::KeyD) {
                dir += right;
            }
            if key(KeyCode::KeyA) {
                dir -= right;
            }
            let run = key(KeyCode::ShiftLeft) || key(KeyCode::ShiftRight);
            let want = dir.normalize_or_zero() * if run { RUN } else { WALK };
            let k = 1.0 - (-dt64 * if f.grounded() { ACCEL } else { 1.0 }).exp();
            f.vel += (want - f.vel) * k;
            if f.vel.length() < 0.02 && want == DVec2::ZERO {
                f.vel = DVec2::ZERO;
            }
            let face = Some(f.yaw as f64);
            if let Some(face) = face {
                let d = wrap(face - f.heading);
                let step = if f.vel.length() < 0.3 { 110.0 } else { 220.0 } * dt64;
                f.heading = wrap(f.heading + d.clamp(-step, step));
            }
            let mut stepped_in = false;
            let mut exempt: Option<DVec2> = None;
            let mut door_path: Option<(DVec2, DVec2, f64, f64, f64)> = None;
            if let (Some((bus, local)), Some(h)) = (f.inside, self.humans.as_ref()) {
                let hd = h
                    .cabin_world(bus, local)
                    .map(|x| x.1)
                    .unwrap_or(0.0)
                    .to_radians();
                let (bf, br) = (
                    DVec2::new(hd.sin(), hd.cos()),
                    DVec2::new(hd.cos(), -hd.sin()),
                );
                let step =
                    glam::Vec2::new((f.vel.dot(br) * dt64) as f32, (f.vel.dot(bf) * dt64) as f32);
                if let Some((l, w)) = h.cabin_walk(bus, local, step) {
                    f.pos = w;
                    f.inside = Some((bus, l));
                    f.lift = 0.0;
                    f.vz = 0.0;
                    let out = h
                        .cabin_doors(bus)
                        .into_iter()
                        .find(|(inside, _, side, open)| {
                            *open
                                && (inside.truncate() - l.truncate()).length() < 0.4
                                && step.x * side > 0.0005
                        });
                    if out.is_some() {
                        f.inside = None;
                        f.pos = w;
                    }
                }
                stepped_in = true;
            } else if let Some(h) = self.humans.as_ref() {
                for bus in h.bus_ids_near(f.pos, 25.0) {
                    for (inside, outside, _, open) in h.cabin_doors(bus) {
                        if !open {
                            continue;
                        }
                        let Some((wi, _)) = h.cabin_world(bus, inside) else {
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
                        if along < -1.2 || along > len + 1.0 || lateral > 1.0 {
                            continue;
                        }
                        exempt = Some(b);
                        let z0 = self
                            .world
                            .as_ref()
                            .and_then(|w| w.walk_height_near(a.x, a.y, outside.z))
                            .unwrap_or(outside.z);
                        door_path = Some((a, dir, len, z0, wi.z));
                        if along >= len - 0.1 && f.vel.dot(dir) > 0.0 {
                            if let Some(l) = cabin_local(h, bus, f.pos.truncate(), inside.z) {
                                f.inside = Some((bus, l));
                                f.lift = 0.0;
                                f.vz = 0.0;
                                stepped_in = true;
                            }
                        }
                    }
                }
            }
            let mut next = if stepped_in {
                f.pos.truncate()
            } else {
                f.pos.truncate() + f.vel * dt64
            };
            if let (false, Some(w)) = (stepped_in, self.world.as_ref()) {
                let z = f.pos.z;
                let probe = Obb {
                    center: next,
                    half: DVec2::splat(3.0),
                    heading: 0.0,
                    z0: z - 1.0,
                    z1: z + 2.5,
                    velocity: DVec2::ZERO,
                    mass: 0.0,
                    pole: None,
                    id: -1,
                };
                let walls = w.collision.lock().obstacles_near(&probe);
                let mut boxes: Vec<Obb> = walls
                    .into_iter()
                    .filter(|o| o.z0 < z + 1.6 + f.lift && o.z1 > z + 0.45 + f.lift)
                    .collect();
                let mut vehicles = self.vehicle_boxes(next, 20.0);
                if let Some(e) = exempt {
                    vehicles.retain(|o| (push_out(e, o, 0.0) - e).length() < 1e-6);
                }
                boxes.extend(vehicles);
                for _ in 0..2 {
                    for o in &boxes {
                        next = push_out(next, o, RADIUS);
                    }
                }
                match w
                    .walk_height_reach(next.x, next.y, z + f.lift, 1.0)
                    .filter(|_| exempt.is_none())
                {
                    Some(g) if g - (z + f.lift) > 0.45 => {
                        next = f.pos.truncate();
                        f.vel *= 0.3;
                    }
                    Some(g) => {
                        let feet = z + f.lift;
                        if g >= feet - 0.02 || f.grounded() && feet - g < 0.4 {
                            f.pos.z = g;
                            if f.vz <= 0.0 {
                                f.lift = 0.0;
                            } else {
                                f.lift = (feet - g).max(0.0);
                            }
                        } else {
                            f.pos.z = g;
                            f.lift = feet - g;
                        }
                    }
                    None => {}
                }
            }
            if let Some((a, dir, len, z0, z1)) = door_path.filter(|_| !stepped_in) {
                let rel = next - a;
                let along = rel.dot(dir);
                let perp = DVec2::new(-dir.y, dir.x);
                if along > 0.0 {
                    next -= perp * rel.dot(perp) * (1.0 - (-dt64 * 8.0).exp());
                }
                f.pos.z = z0 + (z1 - z0) * (along / len).clamp(0.0, 1.0);
                f.lift = 0.0;
                f.vz = 0.0;
            }
            f.pos.x = next.x;
            f.pos.y = next.y;
            if f.inside.is_some() {
                f.vz = 0.0;
                f.lift = 0.0;
            }
            if f.vz > 0.0 || f.lift > 0.0 {
                f.vz -= 9.81 * dt64;
                f.lift += f.vz * dt64;
                if f.lift <= 0.0 {
                    f.lift = 0.0;
                    f.vz = 0.0;
                }
            }
        }
        let show = free;
        if let (Some(h), Some(w), Some(r), Some(scene)) = (
            self.humans.as_mut(),
            self.world.as_ref(),
            self.renderer.as_ref(),
            self.scene.as_mut(),
        ) {
            let cmd = AvatarCmd {
                pos: f.pos,
                heading: f.heading,
                vel: f.vel,
                lift: f.lift,
                seat: f.seat,
                floor: f.inside.map(|_| f.pos.z).or(f.transit.map(|_| f.pos.z)),
                aboard: f.inside,
            };
            h.avatar(AVATAR_KEY, w, r, scene, cmd, f.kind);
            h.avatar_show(AVATAR_KEY, show);
        }
        let body = self.humans.as_ref().and_then(|h| h.avatar_body(AVATAR_KEY));
        if let (Some((feet, heading, _)), Some(_)) = (body, f.seat) {
            f.pos = feet;
            f.heading = heading;
            if f.face_seat {
                let d = wrap(heading - f.yaw as f64);
                f.yaw = (f.yaw as f64 + d * (1.0 - (-dt64 * 4.0).exp())) as f32;
                f.pitch += (-5.0 - f.pitch) * (1.0 - (-dt * 4.0).exp());
                if d.abs() < 2.0 {
                    f.face_seat = false;
                }
            }
        }
        if let (Some(cam), false) = (self.camera.as_mut(), free) {
            let eye = body
                .map(|b| b.2)
                .unwrap_or(f.pos + DVec3::new(0.0, 0.0, 1.62 + f.lift));
            let y = (f.yaw as f64).to_radians();
            let (want, want_yaw, want_pitch) = (
                eye + DVec3::new(y.sin(), y.cos(), 0.0) * 0.08,
                f.yaw,
                f.pitch,
            );
            let settling = f.settle > 0.0;
            f.settle = (f.settle - dt).max(0.0);
            let u = (1.0 - f.settle as f64).clamp(0.0, 1.0);
            let rate = 4.0 + 14.0 * u * u * (3.0 - 2.0 * u);
            let k = 1.0 - (-dt64 * rate).exp();
            let at = match f.eye {
                Some(e) if (e - want).length() < 60.0 => e + (want - e) * k,
                _ => want,
            };
            f.eye = Some(at);
            if settling {
                f.lag = at - want;
            } else {
                f.lag *= (-dt64 * 9.0).exp();
                if f.lag.length() < 0.001 {
                    f.lag = DVec3::ZERO;
                }
            }
            let dy = ((want_yaw - f.eye_yaw + 540.0).rem_euclid(360.0)) - 180.0;
            f.eye_yaw = (f.eye_yaw + dy * k as f32).rem_euclid(360.0);
            cam.position = at;
            cam.yaw = f.eye_yaw;
            cam.pitch += (want_pitch - cam.pitch) * k as f32;
            cam.roll += (0.0 - cam.roll) * k as f32;
        }
        if omsi_cfg::env::var_os("OMSI_DEBUG_FOOT").is_some() && (self.total_frames % 30 == 0) {
            let body = self.humans.as_ref().and_then(|h| h.avatar_body(AVATAR_KEY));
            log::info!(
                "foot: inside {:?} eye {:?} pos ({:.2}, {:.2}, {:.2}) heading {:.0} yaw {:.0} vel ({:.2}, {:.2}) lift {:.2} seat {:?} cam {:?} cam_pos {:?} cam_yaw {:.0} body {:?}",
                f.inside,
                body.map(|b| b.2),
                f.pos.x,
                f.pos.y,
                f.pos.z,
                f.heading,
                f.yaw,
                f.vel.x,
                f.vel.y,
                f.lift,
                f.seat,
                f.cam,
                self.camera.as_ref().map(|c| c.position),
                self.camera.as_ref().map(|c| c.yaw).unwrap_or(0.0),
                body
            );
        }
        self.on_foot = Some(f);
    }

    pub(crate) fn foot_after_humans(&mut self) {
        let Some(f) = self.on_foot.as_mut() else {
            return;
        };
        if f.settle > 0.0 || f.cam != FootCam::First || (f.seat.is_none() && f.inside.is_none()) {
            return;
        }
        let Some(h) = self.humans.as_ref() else {
            return;
        };
        if let Some((bus, l)) = f.inside {
            if let Some((w, _)) = h.cabin_world(bus, l) {
                f.pos = w;
            }
        }
        let eye = h
            .avatar_body(AVATAR_KEY)
            .map(|b| b.2)
            .unwrap_or(f.pos + DVec3::new(0.0, 0.0, 1.62));
        let y = (f.eye_yaw as f64).to_radians();
        let at = eye + DVec3::new(y.sin(), y.cos(), 0.0) * 0.08 + f.lag;
        f.eye = Some(at);
        if let Some(cam) = self.camera.as_mut() {
            cam.position = at;
        }
    }

    pub(crate) fn foot_bus(&self) -> Option<BusId> {
        let f = self.on_foot.as_ref()?;
        f.seat.map(|s| s.0).or(f.inside.map(|i| i.0))
    }

    pub(crate) fn walker_pose(&self) -> Option<omsi_net::Walker> {
        let f = self.on_foot.as_ref()?;
        let my_id = self.lan.as_ref().map(|l| l.my_id).filter(|i| *i != 0);
        let owner = |b: BusId| match b {
            BusId::Player => my_id,
            BusId::Ai(x) => crate::humans::remote_bus_player(x),
        };
        let aboard = match (f.seat, f.inside) {
            (Some((b, k)), _) => owner(b).map(|o| omsi_net::Aboard {
                owner: o,
                local: self
                    .humans
                    .as_ref()
                    .and_then(|h| h.seat_stand(b, k))
                    .map(|l| l.to_array())
                    .unwrap_or_default(),
                seat: Some(k as u16),
            }),
            (None, Some((b, l))) => owner(b).map(|o| omsi_net::Aboard {
                owner: o,
                local: l.to_array(),
                seat: None,
            }),
            _ => None,
        };
        let course = if f.vel.length() > 0.05 {
            f.vel.x.atan2(f.vel.y).to_degrees() as f32
        } else {
            f.heading as f32
        };
        Some(omsi_net::Walker {
            x: f.pos.x,
            y: f.pos.y,
            z: f.pos.z + f.lift,
            heading: f.heading as f32,
            speed: f.vel.length() as f32,
            course,
            seated: f.seat.is_some(),
            aboard,
        })
    }

    pub(crate) fn sync_remote_walkers(&mut self) {
        let walkers: Vec<(u32, Option<omsi_net::Walker>, String)> = self
            .remotes
            .remotes
            .iter()
            .map(|(id, r)| (*id, r.last.walker, r.last.figure.clone()))
            .collect();
        if walkers.iter().all(|w| w.1.is_none()) && self.remote_walkers.is_empty() {
            return;
        }
        if walkers.iter().any(|w| w.1.is_some()) && self.humans.is_none() {
            let mut h = Humans::new(&self.args.root);
            h.avatar_only = true;
            self.humans = Some(h);
        }
        let (Some(h), Some(w), Some(r), Some(scene)) = (
            self.humans.as_mut(),
            self.world.as_ref(),
            self.renderer.as_ref(),
            self.scene.as_mut(),
        ) else {
            return;
        };
        let my_id = self.lan.as_ref().map(|l| l.my_id).unwrap_or(0);
        let mut now = Vec::new();
        for (id, wk, figure) in walkers {
            let Some(wk) = wk else { continue };
            let hh = (if wk.course.is_finite() {
                wk.course
            } else {
                wk.heading
            } as f64)
                .to_radians();
            let kind = omsi_net::human_path(&figure)
                .map(|rel| omsi_cfg::resolve_path(&self.args.root, &rel))
                .filter(|p| omsi_cfg::vfs::exists(p))
                .and_then(|p| crate::driver::cached_type(&p))
                .map(|t| h.type_index(t) as u64)
                .unwrap_or(id as u64 * 13 + 5);
            let aboard = wk.aboard.and_then(|a| {
                let bus = if a.owner == my_id {
                    BusId::Player
                } else {
                    BusId::Ai(crate::humans::remote_bus_id(a.owner))
                };
                let (at, bh) = h.cabin_world(bus, glam::Vec3::from(a.local))?;
                Some((bus, a.seat, at, bh))
            });
            let cmd = match aboard {
                Some((bus, Some(k), at, _)) => AvatarCmd {
                    pos: at,
                    heading: wk.heading as f64,
                    vel: DVec2::ZERO,
                    lift: 0.0,
                    seat: Some((bus, k as usize)),
                    floor: Some(at.z),
                    aboard: None,
                },
                Some((bus, None, at, _)) => AvatarCmd {
                    pos: at,
                    heading: wk.heading as f64,
                    vel: DVec2::new(hh.sin(), hh.cos()) * wk.speed as f64,
                    lift: 0.0,
                    seat: None,
                    floor: Some(at.z),
                    aboard: wk.aboard.map(|a| (bus, glam::Vec3::from(a.local))),
                },
                None if wk.seated => continue,
                None => AvatarCmd {
                    pos: DVec3::new(wk.x, wk.y, wk.z),
                    heading: wk.heading as f64,
                    vel: DVec2::new(hh.sin(), hh.cos()) * wk.speed as f64,
                    lift: 0.0,
                    seat: None,
                    floor: w
                        .walk_height(wk.x, wk.y)
                        .filter(|g| wk.z > g + 0.25)
                        .map(|_| wk.z),
                    aboard: None,
                },
            };
            h.avatar(REMOTE_KEY + id, w, r, scene, cmd, kind);
            if !self.remote_walkers.contains(&id) {
                log::info!(
                    "LAN: player {id} got up and walks at ({:.1}, {:.1})",
                    wk.x,
                    wk.y
                );
            }
            now.push(id);
        }
        for id in std::mem::take(&mut self.remote_walkers) {
            if !now.contains(&id) {
                h.avatar_remove(REMOTE_KEY + id);
            }
        }
        self.remote_walkers = now;
    }
}
