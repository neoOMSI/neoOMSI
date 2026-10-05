use super::*;

#[derive(Debug, Clone, Copy, PartialEq)]
pub(super) enum PuppetMode {
    /// The player on foot (or another player's walker): moved by the game, see `avatar`.
    Avatar,
}

/// A person the game moves itself (the player on foot).
#[derive(Debug, Clone, Copy)]
pub(super) struct Puppet {
    pub(super) mode: PuppetMode,
}

// ---------------------------------------------------------------------------------------
// Avatars: the player got up from the seat (`on_foot`), or another player walks about. The
// game moves them; the people's animation poses them - the gait and its feet on the
// ground, sitting down on a seat and getting up - so every change is eased, never a jump.

/// What the game wants of an avatar this frame.
#[derive(Debug, Clone, Copy)]
pub struct AvatarCmd {
    /// The feet (on foot), in the world.
    pub pos: DVec3,
    /// Facing (degrees, OMSI's).
    pub heading: f64,
    /// Velocity over the ground (m/s).
    pub vel: DVec2,
    /// How high the feet are over the ground (a jump).
    pub lift: f64,
    /// Sitting on this seat of this bus.
    pub seat: Option<(BusId, usize)>,
    /// Standing on a vehicle's floor at this height rather than on the ground (walking
    /// inside a bus: the feet stay on its floor, not reaching down to the road).
    pub floor: Option<f64>,
    /// Standing or walking inside this bus at this point of its cabin (bus frame): placed
    /// in the bus's frame as it is this frame, as its passengers are (a world point taken
    /// a frame earlier left the figure trembling behind the moving bus).
    pub aboard: Option<(BusId, Vec3)>,
}

/// A seat an avatar may take: which, in which bus.
#[derive(Debug, Clone, Copy)]
pub struct SeatSpot {
    pub bus: BusId,
    pub seat: usize,
}

impl Humans {
    /// Put avatar `key` where `cmd` says (made on its first call, of figure `kind`).
    pub fn avatar(
        &mut self,
        key: u32,
        world: &World,
        renderer: &Renderer,
        scene: &mut Scene,
        cmd: AvatarCmd,
        kind: u64,
    ) {
        let known = self
            .avatars
            .avatars
            .get(&key)
            .copied()
            .filter(|id| self.people.iter().any(|p| p.id == *id));
        if known.is_none() {
            let state = State::Idle;
            let n = self.types.len().max(1) as u64;
            let Some(i) = self.spawn_as(
                world,
                renderer,
                scene,
                cmd.pos,
                cmd.heading,
                state,
                Some((kind % n) as usize),
            ) else {
                return;
            };
            self.people[i].puppet = Some(Puppet {
                mode: PuppetMode::Avatar,
            });
            self.avatars.avatars.insert(key, self.people[i].id);
        }
        // a seat taken is kept from the passengers; one left is theirs again
        let before = self.avatars.avatar_cmds.get(&key).and_then(|c| c.seat);
        if before != cmd.seat {
            if let Some((b, k)) = before {
                self.free_seat(b, k);
            }
            if let Some((b, k)) = cmd.seat {
                if let Some(t) = self.buses.seats.get_mut(&b).and_then(|v| v.get_mut(k)) {
                    *t = true;
                }
            }
        }
        self.avatars.avatar_cmds.insert(key, cmd);
    }

    /// Take avatar `key` away.
    pub fn avatar_remove(&mut self, key: u32) {
        if let Some(c) = self.avatars.avatar_cmds.remove(&key) {
            if let Some((b, k)) = c.seat {
                self.free_seat(b, k);
            }
        }
        if let Some(id) = self.avatars.avatars.remove(&key) {
            self.avatars.avatar_hidden.remove(&id);
            if let Some(i) = self.people.iter().position(|p| p.id == id) {
                let p = self.people.swap_remove(i);
                self.retire(&p);
            }
        }
    }

    /// Draw avatar `key` or not (the first-person view looks out of its eyes).
    pub fn avatar_show(&mut self, key: u32, show: bool) {
        if let Some(id) = self.avatars.avatars.get(&key) {
            self.avatars.avatar_hidden.insert(*id, !show);
        }
    }

    /// Where avatar `key` is drawn: its feet, facing, and its eyes.
    pub fn avatar_body(&self, key: u32) -> Option<(DVec3, f64, DVec3)> {
        let id = self.avatars.avatars.get(&key)?;
        let p = self.people.iter().find(|p| p.id == *id)?;
        let rig = &p.ty.rig;
        let eye_h = (rig.head_top - 0.11 * rig.scale) as f64;
        let eye = match (
            p.place,
            self.avatars.avatar_cmds.get(&key).and_then(|c| c.seat),
        ) {
            (Place::Bus(b, _), Some((_, k))) => {
                let bn = self.buses.last_buses.iter().find(|x| x.id == b)?;
                let s = bn.cabin.seats.get(k)?;
                // sitting: the eyes over the hip, a little back
                let r = s.rot.to_radians();
                bn.world(
                    s.pos
                        + Vec3::new(
                            -r.sin() * 0.05,
                            -r.cos() * 0.05,
                            (eye_h - rig.hip[0].z as f64) as f32 + 0.04,
                        ),
                )
            }
            _ => p.position + DVec3::new(0.0, 0.0, eye_h),
        };
        Some((p.position, p.heading, eye))
    }

    /// The seat nearest `at` with a door of its bus within `reach` of it (people and
    /// the other avatars' seats taken), among the buses of the last tick; `only` limits it
    /// to one bus.
    pub fn seat_near(&self, at: DVec3, reach: f64, only: Option<BusId>) -> Option<SeatSpot> {
        let mut best: Option<(f64, SeatSpot)> = None;
        for bn in &self.buses.last_buses {
            if only.map(|o| o != bn.id).unwrap_or(false) {
                continue;
            }
            // the nearest door (entries and exits: any door will do to get in)
            let door = bn
                .cabin
                .entries
                .iter()
                .chain(bn.cabin.exits.iter())
                .map(|d| bn.world(d.outside))
                .min_by(|a, b| (*a - at).length().total_cmp(&(*b - at).length()));
            let Some(door) = door else { continue };
            let d = (door - at).truncate().length();
            if d > reach {
                continue;
            }
            let taken = self.buses.seats.get(&bn.id);
            let seat = bn
                .cabin
                .seats
                .iter()
                .enumerate()
                .filter(|(k, s)| {
                    s.seated && !taken.and_then(|t| t.get(*k)).copied().unwrap_or(false)
                })
                .min_by(|a, b| {
                    (bn.world(a.1.floor) - door)
                        .length()
                        .total_cmp(&(bn.world(b.1.floor) - door).length())
                })
                .map(|(k, _)| k);
            let Some(seat) = seat else { continue };
            if best.map(|b| d < b.0).unwrap_or(true) {
                best = Some((d, SeatSpot { bus: bn.id, seat }));
            }
        }
        best.map(|b| b.1)
    }

    /// Where the doors of a bus are now (outside, in the world).
    pub fn bus_doors(&self, bus: BusId) -> Vec<DVec3> {
        self.buses
            .last_buses
            .iter()
            .find(|b| b.id == bus)
            .map(|bn| {
                bn.cabin
                    .entries
                    .iter()
                    .chain(bn.cabin.exits.iter())
                    .map(|d| bn.world(d.outside))
                    .collect()
            })
            .unwrap_or_default()
    }

    /// The door of vehicle `v` nearest its driver's seat (outside, in the world): where the
    /// driver gets in and out.
    pub fn vehicle_driver_door(&mut self, v: &VehicleInstance) -> Option<DVec3> {
        // (a van's own cab door first: its driver does not climb in through the sliding door)
        if let Some(d) = self.vehicle_cab_door(v) {
            return Some(d);
        }
        let cabin = self.cabin_for(v)?;
        let seat = cabin
            .data
            .driver_positions
            .first()
            .map(|d| Vec3::from(d.pos))
            .unwrap_or(Vec3::new(-0.8, 4.5, 1.0));
        let door = cabin
            .entries
            .iter()
            .chain(cabin.exits.iter())
            .min_by(|a, b| {
                (a.outside - seat)
                    .truncate()
                    .length()
                    .total_cmp(&(b.outside - seat).truncate().length())
            })?;
        let trailers = part_frames(v, &cabin);
        Some(train_point(
            v.position,
            &v.body_rotation(),
            &trailers,
            door.outside,
        ))
    }

    /// A door of the driver's own beside the driver's seat (a van's or a coach's cab door:
    /// on the driver's side, level with the seat), in the world, outside.
    pub fn vehicle_cab_door(&mut self, v: &VehicleInstance) -> Option<DVec3> {
        let cabin = self.cabin_for(v)?;
        let seat = cabin
            .data
            .driver_positions
            .first()
            .map(|d| Vec3::from(d.pos))?;
        let door = cabin
            .entries
            .iter()
            .chain(cabin.exits.iter())
            .filter(|d| d.outside.x * seat.x > 0.0 && (d.outside.y - seat.y).abs() < 1.5)
            .min_by(|a, b| {
                (a.outside - seat)
                    .truncate()
                    .length()
                    .total_cmp(&(b.outside - seat).truncate().length())
            })
            .map(|d| d.outside);
        // a van or minibus (the W906: its cabin knows only the sliding door, the passengers'):
        // the driver's door beside the seat, which every such vehicle has
        let door = door.or_else(|| {
            let bb = v.ty.def.bounding_box?;
            (bb[1] < 8.5 && seat.x.abs() > 0.2).then(|| {
                Vec3::new(
                    seat.x.signum() * (bb[0] * 0.5 + bb[3] * seat.x.signum() + 0.45),
                    seat.y,
                    0.0,
                )
            })
        })?;
        let trailers = part_frames(v, &cabin);
        Some(train_point(v.position, &v.body_rotation(), &trailers, door))
    }

    /// Put `ty` among the figures (once) and give its index: the player's own figure.

    /// Where the doors of vehicle `v` are now (outside, in the world), entries first.
    pub fn vehicle_doors(&mut self, v: &VehicleInstance) -> Vec<DVec3> {
        let Some(cabin) = self.cabin_for(v) else {
            return Vec::new();
        };
        let trailers = part_frames(v, &cabin);
        let rot = v.body_rotation();
        cabin
            .entries
            .iter()
            .chain(cabin.exits.iter())
            .map(|d| train_point(v.position, &rot, &trailers, d.outside))
            .collect()
    }

    /// A walker inside bus `bus` moving from cabin point `local` by `step` (bus frame,
    /// metres): kept within a corridor round the cabin's own path network (the aisles,
    /// the door areas, the space by the driver) and on its floor. Gives the new cabin point
    /// and where that is in the world now.
    pub fn cabin_walk(
        &self,
        bus: BusId,
        local: Vec3,
        step: glam::Vec2,
        pass: bool,
    ) -> Option<(Vec3, DVec3)> {
        const WIDTH: f32 = 0.5;
        let bn = self.buses.last_buses.iter().find(|b| b.id == bus)?;
        let pts = &bn.cabin.graph.points;
        let want = glam::Vec2::new(local.x + step.x, local.y + step.y);
        let mut best: Option<(f32, glam::Vec2, f32)> = None;
        for &(a, b, _) in &bn.cabin.links {
            let (Some(pa), Some(pb)) = (pts.get(a.max(0) as usize), pts.get(b.max(0) as usize))
            else {
                continue;
            };
            let (a2, b2) = (pa.truncate(), pb.truncate());
            let ab = b2 - a2;
            let t = if ab.length_squared() > 1e-6 {
                ((want - a2).dot(ab) / ab.length_squared()).clamp(0.0, 1.0)
            } else {
                0.0
            };
            let q = a2 + ab * t;
            let d = (want - q).length() + (local.z - (pa.z + (pb.z - pa.z) * t)).abs();
            if best.map(|x| d < x.0).unwrap_or(true) {
                best = Some((d, q, pa.z + (pb.z - pa.z) * t));
            }
        }
        if best.is_none() {
            for pt in pts {
                let d = (want - pt.truncate()).length();
                if best.map(|x| d < x.0).unwrap_or(true) {
                    best = Some((d, pt.truncate(), pt.z));
                }
            }
        }
        let (d, q, z) = best?;
        let mut xy = if d > WIDTH {
            q + (want - q) / d * WIDTH
        } else {
            want
        };
        if !pass {
            let (eo, xo) = bn
                .walk_open
                .as_ref()
                .map(|w| (&w.0, &w.1))
                .unwrap_or((&bn.entry_open, &bn.exit_open));
            let doors = bn
                .cabin
                .entries
                .iter()
                .enumerate()
                .map(|(k, dr)| (dr, eo.get(k).copied().unwrap_or(false)))
                .chain(
                    bn.cabin
                        .exits
                        .iter()
                        .enumerate()
                        .map(|(k, dr)| (dr, xo.get(k).copied().unwrap_or(false))),
                );
            for (dr, open) in doors {
                if open || (dr.inside.z - z).abs() > 0.8 {
                    continue;
                }
                let p = dr.inside.truncate();
                let out = (dr.outside.truncate() - p).normalize_or_zero();
                if out == glam::Vec2::ZERO {
                    continue;
                }
                let rel = xy - p;
                if rel.dot(glam::Vec2::new(-out.y, out.x)).abs() > 0.9 {
                    continue;
                }
                let along = rel.dot(out);
                if along > -0.1 {
                    xy -= out * (along + 0.1);
                }
            }
        }
        // not through the seats and the driver's place: no nearer to one than 0.38 m
        // (walking away from one that close is let be)
        let from = local.truncate();
        let solid = bn
            .cabin
            .seats
            .iter()
            .filter(|s| s.seated)
            .map(|s| s.pos.truncate())
            .chain(
                bn.cabin
                    .data
                    .driver_positions
                    .iter()
                    .map(|d| glam::Vec2::new(d.pos[0], d.pos[1])),
            );
        for c in solid {
            let (dn, d0) = ((xy - c).length(), (from - c).length());
            if dn < 0.38 && dn < d0 {
                return Some((local, bn.world(local)));
            }
        }
        let l = Vec3::new(xy.x, xy.y, z);
        Some((l, bn.world(l)))
    }

    /// The doors of bus `bus`: the threshold in the cabin, where one stands outside (world),
    /// which side of the bus (+1 right) and whether it is open now.
    pub fn cabin_doors(&self, bus: BusId) -> Vec<(Vec3, DVec3, f32, bool)> {
        let Some(bn) = self.buses.last_buses.iter().find(|b| b.id == bus) else {
            return Vec::new();
        };
        let (eo, xo) = bn
            .walk_open
            .as_ref()
            .map(|w| (&w.0, &w.1))
            .unwrap_or((&bn.entry_open, &bn.exit_open));
        let entries = bn
            .cabin
            .entries
            .iter()
            .enumerate()
            .map(|(k, d)| (d, eo.get(k).copied().unwrap_or(false)));
        let exits = bn
            .cabin
            .exits
            .iter()
            .enumerate()
            .map(|(k, d)| (d, xo.get(k).copied().unwrap_or(false)));
        entries
            .chain(exits)
            .map(|(d, open)| (d.inside, bn.world(d.outside), d.side, open))
            .collect()
    }

    /// The buses of the last tick within `r` of `at`, the own first.
    pub fn bus_ids_near(&self, at: DVec3, r: f64) -> Vec<BusId> {
        let mut v: Vec<(BusId, f64)> = self
            .buses
            .last_buses
            .iter()
            .map(|b| (b.id, (b.pos - at).truncate().length()))
            .filter(|x| x.1 < r)
            .collect();
        v.sort_by(|a, b| {
            (a.0 != BusId::Player)
                .cmp(&(b.0 != BusId::Player))
                .then(a.1.total_cmp(&b.1))
        });
        v.into_iter().map(|x| x.0).collect()
    }

    /// Where the cabin point `local` of bus `bus` is in the world now, and the bus's heading.
    pub fn cabin_world(&self, bus: BusId, local: Vec3) -> Option<(DVec3, f64)> {
        let bn = self.buses.last_buses.iter().find(|b| b.id == bus)?;
        Some((bn.world(local), bn.heading))
    }

    /// The free seat of bus `bus` nearest the world point `at` (for a walker inside it).
    pub fn seat_nearest(&self, bus: BusId, at: DVec3, reach: f64) -> Option<usize> {
        let bn = self.buses.last_buses.iter().find(|b| b.id == bus)?;
        let taken = self.buses.seats.get(&bn.id);
        bn.cabin
            .seats
            .iter()
            .enumerate()
            .filter(|(k, s)| s.seated && !taken.and_then(|t| t.get(*k)).copied().unwrap_or(false))
            .map(|(k, s)| (k, (bn.world(s.floor) - at).truncate().length()))
            .filter(|(_, d)| *d < reach)
            .min_by(|a, b| a.1.total_cmp(&b.1))
            .map(|x| x.0)
    }

    /// The cabin path point nearest seat `seat` of bus `bus` (where one stands up to).
    pub fn seat_stand(&self, bus: BusId, seat: usize) -> Option<Vec3> {
        let bn = self.buses.last_buses.iter().find(|b| b.id == bus)?;
        let s = bn.cabin.seats.get(seat)?;
        // (on the seat's own deck: in a double-decker the nearest point in plan could be
        // the one straight above or below it)
        let d =
            |a: &Vec3| (a.truncate() - s.floor.truncate()).length() + (a.z - s.floor.z).abs() * 3.0;
        bn.cabin
            .graph
            .points
            .iter()
            .copied()
            .min_by(|a, b| d(a).total_cmp(&d(b)))
    }

    /// Cabin point `local` of vehicle `v` in the world (before the buses' first tick).
    pub fn vehicle_cabin_world(&mut self, v: &VehicleInstance, local: Vec3) -> Option<DVec3> {
        let cabin = self.cabin_for(v)?;
        let trailers = part_frames(v, &cabin);
        Some(train_point(
            v.position,
            &v.body_rotation(),
            &trailers,
            local,
        ))
    }

    /// Where the driver stands up in vehicle `v`'s cabin: the cabin's path point nearest the
    /// driver's seat (bus frame).
    pub fn driver_stand(&mut self, v: &VehicleInstance) -> Option<Vec3> {
        let cabin = self.cabin_for(v)?;
        let seat = cabin
            .data
            .driver_positions
            .first()
            .map(|d| Vec3::from(d.pos))
            .unwrap_or(Vec3::new(-0.8, 4.5, 1.0));
        // the driver's position is the hip, half a metre over the cab floor: a double
        // decker's upper deck lies straight over the cab and was as near in plan, and the
        // driver who got up stood in the roof over the windscreen
        let d = |a: &Vec3| {
            (a.truncate() - seat.truncate()).length() + (a.z - (seat.z - 0.5)).abs() * 3.0
        };
        cabin
            .graph
            .points
            .iter()
            .copied()
            .min_by(|a, b| d(a).total_cmp(&d(b)))
    }

    /// How many people are in (or boarding, riding, leaving) bus `bus`.
    pub fn people_in(&self, bus: BusId) -> usize {
        self.people
            .iter()
            .filter(|p| {
                matches!(p.place, Place::Bus(b, _) if b == bus) || p.state.bus() == Some(bus)
            })
            .count()
    }

    /// Where bus `bus` stands (its origin), as of the last tick.
    pub fn bus_center(&self, bus: BusId) -> Option<DVec3> {
        self.buses
            .last_buses
            .iter()
            .find(|b| b.id == bus)
            .map(|b| b.pos)
    }

    /// Is `bus` among the buses of the last tick?
    pub fn bus_here(&self, bus: BusId) -> bool {
        self.buses.last_buses.iter().any(|b| b.id == bus)
    }

    /// The player's (or another player's) body on foot, animated as Omsi.exe animates its
    /// people: sitting on a seat (its hip on the `[passpos]`), walking or standing.
    pub(super) fn animate_avatar(
        &mut self,
        i: usize,
        dt: f32,
        world: &World,
        buses: &[BusNow],
        bus_ix: &HashMap<BusId, usize>,
    ) {
        let id = self.people[i].id;
        let Some(key) = self
            .avatars
            .avatars
            .iter()
            .find(|(_, v)| **v == id)
            .map(|(k, _)| *k)
        else {
            return;
        };
        let Some(cmd) = self.avatars.avatar_cmds.get(&key).copied() else {
            return;
        };
        let dt_ms = dt * 1000.0;
        let seated = cmd.seat.and_then(|(b, k)| {
            let bn = bus_ix.get(&b).map(|x| &buses[*x])?;
            let s = bn.cabin.seats.get(k)?.clone();
            Some((b, s, bn))
        });
        let seatheight = self.people[i].ty.def.seat_height;
        let p = &mut self.people[i];
        let input = match seated.as_ref() {
            Some((b, s, bn)) => {
                // on the seat, in its bus's frame (set_task(7): the feet the human's seat
                // height under the seat point, facing the way the seat does)
                let l = if s.seated {
                    s.pos - Vec3::Z * seatheight
                } else {
                    s.pos
                };
                p.place = Place::Bus(*b, l);
                p.lheading = s.rot as f64;
                p.position = bn.world(l);
                p.tilt = bn.tilt_at(l);
                p.heading = bn.heading_at(l) + p.lheading;
                p.interior = bn.interior;
                p.vel = DVec2::ZERO;
                p.activity = if s.seated {
                    Activity::Sit
                } else {
                    Activity::Stand
                };
                AnimInput {
                    kind: if s.seated { 2 } else { 0 },
                    seat_height: s.height,
                    room_height: pax::OUTSIDE_ROOM,
                    dt_ms,
                    ..Default::default()
                }
            }
            None if cmd.aboard.is_some_and(|(b, _)| bus_ix.contains_key(&b)) => {
                let (b, l) = cmd.aboard.unwrap();
                let bn = &buses[bus_ix[&b]];
                let bh = bn.heading_at(l);
                p.place = Place::Bus(b, l);
                p.lheading = wrap_heading(cmd.heading - bh);
                p.position = bn.world(l);
                p.tilt = bn.tilt_at(l);
                p.heading = cmd.heading;
                p.interior = bn.interior;
                p.vel = cmd.vel;
                let v = cmd.vel.length() as f32;
                p.activity = if v > 0.05 {
                    Activity::Walk
                } else {
                    Activity::Stand
                };
                AnimInput {
                    kind: (v > 0.05) as u8,
                    speed: v,
                    moved: v * dt,
                    room_height: pax::OUTSIDE_ROOM,
                    dt_ms,
                    ..Default::default()
                }
            }
            None => {
                p.place = Place::Ground;
                p.tilt = Mat4::IDENTITY;
                p.interior = 0.0;
                let ground = cmd.floor.unwrap_or_else(|| {
                    world.walk_height(cmd.pos.x, cmd.pos.y).unwrap_or(cmd.pos.z)
                });
                let origin = DVec3::new(
                    cmd.pos.x,
                    cmd.pos.y,
                    if cmd.floor.is_some() {
                        ground
                    } else {
                        cmd.pos.z.max(ground)
                    } + cmd.lift.max(0.0),
                );
                p.position = origin;
                p.heading = cmd.heading;
                p.vel = cmd.vel;
                let v = cmd.vel.length() as f32;
                p.activity = if v > 0.05 {
                    Activity::Walk
                } else {
                    Activity::Stand
                };
                AnimInput {
                    kind: (v > 0.05) as u8,
                    speed: v,
                    moved: v * dt,
                    room_height: pax::OUTSIDE_ROOM,
                    dt_ms,
                    ..Default::default()
                }
            }
        };
        let (frame, origin, heading) = match p.place {
            Place::Bus(b, l) => (b.space(), l.as_dvec3(), p.lheading),
            _ => (0, p.position, p.heading),
        };
        let seat_pos = seated.as_ref().and_then(|(_, s, _)| {
            s.seated
                .then(|| model_point(origin, heading, s.pos.as_dvec3()))
        });
        let floor_cb = |_at: DVec2| Some(origin.z);
        let pose_in = omsi_sim::human::PoseInput {
            activity: p.activity,
            origin,
            heading,
            frame,
            velocity: p.vel,
            seat: seat_pos,
            look: None,
            reach: None,
            grips: None,
            grip_frames: None,
            grip_lean: 0.0,
            hold: 0.0,
            sway: glam::Vec2::ZERO,
            floor: Some(&floor_cb),
        };
        p.pose.advance(&p.ty.rig, &pose_in, dt);
        p.finish_animation(self.ik, &input);
    }
}

// ---------------------------------------------------------------------------------------
// LAN play (see `lan_world`): a host keeps people around every player, tells the clients
// where they are and hands the waiting ones over to a client's bus; a client draws the
// host's people instead of its own and simulates only those who board its bus.

pub(in crate::humans) struct Avatars {
    /// Avatars (the player on foot, other players' walkers): key → person id, and what
    /// the game wants of each this frame.
    pub(in crate::humans) avatars: HashMap<u32, u32>,
    pub(in crate::humans) avatar_cmds: HashMap<u32, AvatarCmd>,
    /// Avatars not drawn (the first-person view), by person id.
    pub(in crate::humans) avatar_hidden: HashMap<u32, bool>,
}
