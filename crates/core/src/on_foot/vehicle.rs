use super::collision::{probe_box, push_out};
use super::{AVATAR_KEY, OnFoot, RADIUS, Then, Transit};
use crate::App;
use crate::humans::{BusId, Humans};
use glam::{DVec2, DVec3};
use ::simulation::collision::Obb;

const DOOR_REACH: f64 = 3.2;
const DOOR_OUT_REACH: f64 = 3.2;

pub(super) fn cabin_local(h: &Humans, bus: BusId, p: DVec2, z: f32) -> Option<glam::Vec3> {
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

fn outside_spot(
    h: &mut Humans,
    world: Option<&crate::scene::World>,
    v: &::simulation::VehicleInstance,
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
    v: &::simulation::VehicleInstance,
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
                let probe = probe_box(q, 1.0, z);
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
            (Some(t), Some(h)) => h.avatar_figure(t),
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
        let mut f = OnFoot::new(pos, face);
        f.inside = inside;
        f.yaw = face as f32;
        f.pitch = -8.0;
        f.eye = self.camera.as_ref().map(|c| c.position);
        f.eye_yaw = self.camera.as_ref().map(|c| c.yaw).unwrap_or(face as f32);
        f.settle = if self
            .player
            .as_ref()
            .is_some_and(|p| p.vehicle.physics.velocity_kmh().abs() < 3.0)
        {
            1.0
        } else {
            0.0
        };
        f.view_before = if self.view == "foot" {
            "driver".into()
        } else {
            self.view.clone()
        };
        f.kind = kind;
        f.transit = transit;
        if let (Some((_, l)), Some(seat)) = (
            inside,
            self.player
                .as_ref()
                .and_then(|p| p.driver.as_ref())
                .map(|d| d.seat_point()),
        ) {
            f.cab = Some(super::wheel::CabMove::new(BusId::Player, seat, l, false));
        }
        self.on_foot = Some(f);
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
        let mut f = OnFoot::new(pos, heading);
        f.kind = self.args.root.to_string_lossy().len() as u64 * 7 + 3;
        self.on_foot = Some(f);
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

    pub(crate) fn update_placed_sounds(&mut self) {
        const NEAR: f64 = 250.0;
        let (Some(a), Some(cam)) = (self.audio.as_ref(), self.camera.as_ref()) else {
            return;
        };
        if self.paused {
            return;
        }
        let listener = cam.position;
        let muffled = self.audio_in_cab();
        for q in self.placed.iter_mut() {
            let d = (q.vehicle.position - listener).length();
            if d > NEAR * 1.2 {
                if let Some(mut s) = q.sounds.take() {
                    s.stop_all(a);
                }
                continue;
            }
            if q.sounds.is_none() && d < NEAR {
                q.load_sounds(a);
            }
            q.tick_sounds(Some(a), muffled, false, false);
        }
    }

    pub(crate) fn take_placed(&mut self, k: usize) {
        if k >= self.placed.len() {
            return;
        }
        let mut next = self.placed.remove(k);
        if let Some(a) = self.audio.as_ref() {
            if next.sounds.is_none() {
                next.load_sounds(a);
            }
        }
        next.vehicle.host.auto_clutch = if ::config::get_bool("gameplay", "auto_clutch").unwrap_or(true) { 1.0 } else { 0.0 };
        if let Some(now) = self.player.take() {
            let now = now;
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

    pub(super) fn sit_at_the_wheel(&mut self) {
        let Some(f) = self.on_foot.take() else { return };
        if let Some(h) = self.humans.as_mut() {
            h.avatar_remove(AVATAR_KEY);
        }
        if f.cab.is_some()
            && let Some(d) = self.player.as_mut().and_then(|p| p.driver.as_mut())
        {
            d.take_wheel_from_lap();
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

    pub(super) fn use_seat(&mut self) {
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
        if ::legacy_config::env::var_os("OMSI_DEBUG_FOOT").is_some() {
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

    pub(crate) fn vehicle_boxes(&self, at: DVec2, r: f64) -> Vec<Obb> {
        let mut boxes = Vec::new();
        let mut add = |v: &::simulation::VehicleInstance| {
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
            for c in t.cars() {
                if c.render.hidden || c.gone {
                    continue;
                }
                add(&c.vehicle);
            }
        }
        for rm in self.remotes.remotes.values() {
            add(rm.vehicle());
        }
        boxes
    }
}
