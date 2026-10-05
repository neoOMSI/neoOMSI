use super::*;

impl Humans {
    pub(in crate::humans) fn pax_room(
        &mut self,
        dt: f32,
        origins: &[DVec3],
        buses: &[BusNow],
        bus_ix: &HashMap<BusId, usize>,
    ) {
        if !self.ik || dt <= 0.0 {
            return;
        }
        let dt = dt as f64;
        let mut walkers = Vec::new();
        let mut movers = Vec::new();
        for (i, person) in self.people.iter().enumerate() {
            if person.place != Place::Ground || person.remote {
                continue;
            }
            let mut walker = Walker::new(person.position.truncate(), BODY_OUTSIDE, 0);
            walker.fixed = true;
            walker.vel = person.vel;
            if let State::Pax(pax) = &person.state {
                let target = if pax.target_bus {
                    pax.bus
                        .and_then(|id| bus_ix.get(&id))
                        .map_or(pax.target, |&k| buses[k].world(pax.target.as_vec3()))
                } else {
                    pax.target
                };
                if pax.inside.is_none()
                    && pax.doorway.is_none()
                    && matches!(
                        pax.task,
                        Task::ToBus | Task::WalkingToBus | Task::WalkingToBusstop
                    )
                    && pax.speed > 0.05
                    && (target - pax.pos).truncate().length() >= 0.8
                {
                    walker.pos = origins[i].truncate();
                    walker.want = (person.position.truncate() - walker.pos) / dt;
                    walker.vel = walker.want;
                    walker.give = 0.7;
                    walker.fixed = false;
                    movers.push((walkers.len(), i));
                }
            }
            walkers.push(walker);
        }
        if movers.is_empty() {
            return;
        }
        // Vehicle bounds include the entry itself; door routing owns that clearance.
        crowd::step(&mut walkers, &[], &CrowdParams::default(), dt);
        for (k, i) in movers {
            let shift =
                (walkers[k].pos - self.people[i].position.truncate()).clamp_length_max(0.8 * dt);
            self.pax_mut(i).unwrap().pos += shift.extend(0.0);
            self.people[i].position += shift.extend(0.0);
            self.people[i].vel += shift / dt;
        }
    }
}

/// A point of the cabin's path network with its links in the file's order: the point at
/// the other end, the points reached through it (sub_72410c), the link's index, its room
/// height and step sounds.
#[derive(Debug, Clone)]
pub(in crate::humans) struct RouteLink {
    pub to: usize,
    pub reach: Vec<usize>,
    pub link: usize,
}

/// Cache the graph's shortest first steps with original link indices for room heights
/// and footsteps. Unreachable destinations never appear in a route.
pub(in crate::humans) fn build_routes(
    graph: &PathGraph,
    links: &[(i32, i32, bool)],
) -> Vec<Vec<RouteLink>> {
    (0..graph.points.len())
        .map(|from| {
            let (_, first) = graph.routing_from(from);
            let mut routes: Vec<RouteLink> = Vec::new();
            for (target, next) in first.into_iter().enumerate() {
                let Some(to) = next else { continue };
                if let Some(route) = routes.iter_mut().find(|r| r.to == to) {
                    route.reach.push(target);
                } else if let Some(link) = links.iter().position(|&(a, b, one)| {
                    (a == from as i32 && b == to as i32)
                        || (!one && b == from as i32 && a == to as i32)
                }) {
                    routes.push(RouteLink {
                        to,
                        reach: vec![target],
                        link,
                    });
                }
            }
            routes
        })
        .collect()
}

/// sub_7f3a24: the distance with the height difference weighed by `w` (5 everywhere).
fn weighted_dist(a: Vec3, b: Vec3, w: f32) -> f32 {
    let d = a - b;
    Vec3::new(d.x, d.y, d.z * w).length()
}

impl Cabin {
    pub(in crate::humans) fn nearest_exit(
        &self,
        from: usize,
        open: &[bool],
    ) -> Option<(usize, usize)> {
        let distances = self.graph.distances_from(from);
        self.exits
            .iter()
            .enumerate()
            .filter_map(|(door, e)| {
                let point = e.point?;
                let distance = *distances.get(point)?;
                distance.is_finite().then_some((door, point, distance))
            })
            .min_by(|a, b| {
                let closed = |d: usize| !open.get(d).copied().unwrap_or(false);
                closed(a.0)
                    .cmp(&closed(b.0))
                    .then(a.2.total_cmp(&b.2))
                    .then(a.0.cmp(&b.0))
            })
            .map(|(door, point, _)| (door, point))
    }

    pub(in crate::humans) fn available_places(
        &self,
        taken: &[bool],
        from: Option<usize>,
        seats_only: bool,
    ) -> Vec<usize> {
        let starts: Vec<usize> = match from {
            Some(p) => vec![p],
            None => self.entries.iter().filter_map(|e| e.point).collect(),
        };
        let mut distance = vec![f32::INFINITY; self.graph.points.len()];
        for start in starts {
            for (best, d) in distance.iter_mut().zip(self.graph.distances_from(start)) {
                *best = best.min(d);
            }
        }
        let mut places: Vec<(usize, f32)> = self
            .seats
            .iter()
            .enumerate()
            .filter_map(|(k, seat)| {
                if taken.get(k).copied().unwrap_or(false) || (seats_only && !seat.seated) {
                    return None;
                }
                let point = seat.point?;
                let d = *distance.get(point)?;
                let can_exit = self
                    .exits
                    .iter()
                    .filter_map(|e| e.point)
                    .any(|exit| point == exit || self.route_next(point, exit).is_some());
                if !d.is_finite() || !can_exit {
                    return None;
                }
                Some((k, d))
            })
            .collect();
        places.sort_by(|a, b| {
            if from.is_some() {
                a.1.total_cmp(&b.1).then_with(|| a.0.cmp(&b.0))
            } else {
                a.0.cmp(&b.0)
            }
        });
        places.into_iter().map(|p| p.0).collect()
    }

    /// sub_72506c: the point of `list` nearest `p` (height weighed by 5). `level`: only
    /// points at most 2 m below `p` and not above it. `open`/`flags` (entries): a shut
    /// door counts only with a button; `avoid`: a passenger buying a ticket skips
    /// `{noticketsale}` doors. Nothing found: the first of the list, or the search again
    /// without `avoid`.
    pub(in crate::humans) fn omsi_nearest(
        &self,
        p: Vec3,
        list: &[Option<usize>],
        avoid: bool,
        level: bool,
        flags: Option<&[(bool, bool)]>,
        open: Option<&[bool]>,
    ) -> Option<usize> {
        let pts = &self.graph.points;
        let mut best = 1e12f32;
        let mut found: Option<usize> = None;
        for (k, pt) in list.iter().enumerate() {
            let Some(pt) = *pt else { continue };
            let Some(q) = pts.get(pt) else { continue };
            if level && !(q.z <= p.z && p.z <= q.z + 2.0) {
                continue;
            }
            let d = weighted_dist(p, *q, 5.0);
            let shut_ok = match open {
                Some(o) if o.len() >= list.len() && !o[k] => {
                    flags.is_some_and(|f| f.get(k).is_some_and(|f| f.1))
                }
                _ => true,
            };
            if !shut_ok {
                continue;
            }
            if avoid && flags.is_some_and(|f| f.len() >= list.len() && f[k].0) {
                continue;
            }
            if d < best {
                best = d;
                found = Some(pt);
            }
        }
        if found.is_none() {
            if !(avoid && flags.is_some()) {
                return list.first().copied().flatten();
            }
            return self.omsi_nearest(p, list, false, level, flags, open);
        }
        found
    }

    /// sub_723fac: the next point from `from` towards `to` and the link taken.
    pub(in crate::humans) fn route_next(&self, from: usize, to: usize) -> Option<(usize, usize)> {
        let links = self.routes.get(from)?;
        links
            .iter()
            .find(|l| l.reach.contains(&to))
            .map(|l| (l.to, l.link))
    }

    /// The path points of the entries / exits, in order.
    pub(in crate::humans) fn entry_points(&self) -> Vec<Option<usize>> {
        self.entries.iter().map(|e| e.point).collect()
    }
    #[cfg(test)]
    pub(in crate::humans) fn exit_points(&self) -> Vec<Option<usize>> {
        self.exits.iter().map(|e| e.point).collect()
    }
    /// ({noticketsale}, {withbutton}) of each entry.
    pub(in crate::humans) fn entry_flags(&self) -> Vec<(bool, bool)> {
        self.entries.iter().map(|e| (!e.sells, e.button)).collect()
    }
}

impl Humans {
    /// The movement part of the tick (sub_62a6a0, 0x62ad0b - 0x62b966).
    pub(in crate::humans) fn pax_move(
        &mut self,
        i: usize,
        dt: f32,
        dt_ms: f32,
        world: &World,
        buses: &[BusNow],
        bus_ix: &HashMap<BusId, usize>,
    ) {
        let p0 = self.pax(i).unwrap().clone();
        if self.ik && p0.task != Task::SittingInBus && self.people[i].pose.sit_amount() > 0.02 {
            let p = self.pax_mut(i).unwrap();
            p.speed = 0.0;
            p.moved = 0.0;
            return;
        }
        let bn_in = p0.inside.and_then(|b| bus_ix.get(&b).map(|k| &buses[*k]));
        let bn_t = p0.bus.and_then(|b| bus_ix.get(&b).map(|k| &buses[*k]));
        let mut target = p0.target;
        let mut target_bus = p0.target_bus;
        // (kept as the target, +0x5bd: waiting short of the point, state 6, goes on facing
        // it - with the target of before kept instead, a seat or the stop's gather point in
        // another frame, the people waiting at a shut exit were lifted 40 m up in the bus
        // and stood stacked there for good, #709)
        let mut walked_to: Option<DVec3> = None;
        if matches!(
            p0.movement,
            Movement::AlongPath | Movement::ShortOfPathEnd | Movement::AtPathEnd
        ) {
            if let (Some(pt), Some(bn)) = (p0.pt, bn_in) {
                if let Some(q) = bn.cabin.graph.points.get(pt) {
                    target = q.as_dvec3();
                    target_bus = true;
                    walked_to = Some(target);
                }
            }
        }
        // walking to a door from outside: keep off the bus side (0x62ad81)
        if p0.clamp && target_bus {
            if let Some(bn) = bn_t {
                let level = if p0.clamp_open && p0.inside.is_none() {
                    let l = bn.to_local(p0.pos);
                    (l.y as f64 - target.y).abs() <= 1.0
                } else {
                    false
                };
                if !level {
                    if p0.clamp_left {
                        target.x = target.x.min(p0.clamp_x);
                    } else {
                        target.x = target.x.max(p0.clamp_x);
                    }
                }
            }
        }
        let tgt = match (target_bus, p0.inside) {
            (true, Some(_)) => target,
            (true, None) => match bn_t {
                Some(bn) => bn.world(target.as_vec3()),
                None => target,
            },
            (false, Some(_)) => match bn_in {
                Some(bn) => bn.to_local(target).as_dvec3(),
                None => target,
            },
            (false, None) => target,
        };
        let mut d = tgt - p0.pos;
        let mut room = p0.room;
        let mut step_pack = p0.step_pack;
        if p0.inside.is_none() {
            d.z = 0.0;
            step_pack = None;
            room = OUTSIDE_ROOM;
        }
        let dist = d.length() as f32;
        let mut st = p0.movement;
        let mut pt = p0.pt;
        let mut link = p0.link;
        match st {
            Movement::AlongPath => {
                if p0.pt == p0.pt_target && dist <= 0.7 && p0.short {
                    st = Movement::ShortOfPathEnd;
                } else if dist <= 0.1 {
                    let next = match (p0.pt, p0.pt_target, bn_in) {
                        (Some(a), Some(b), Some(bn)) => bn.cabin.route_next(a, b),
                        _ => None,
                    };
                    match next {
                        Some((n, l)) => {
                            pt = Some(n);
                            link = Some(l);
                            if let Some(bn) = bn_in {
                                step_pack = bn.cabin.link_pack.get(l).copied().flatten();
                                room = bn.cabin.link_room.get(l).copied().unwrap_or(2.0);
                            }
                        }
                        None => st = Movement::AtPathEnd,
                    }
                }
            }
            Movement::ToTarget => {
                if dist <= 0.7 && p0.short {
                    st = Movement::ShortOfTarget;
                } else if dist <= 0.1 {
                    st = Movement::AtTarget;
                }
            }
            Movement::ShortOfPathEnd => {
                if !p0.short {
                    st = Movement::AlongPath;
                }
            }
            Movement::ShortOfTarget => {
                if !p0.short {
                    st = Movement::ToTarget;
                }
            }
            _ => {}
        }
        // the people in the way (sub_626860)
        let (mut block, free_r, free_l) = if st == Movement::ToTarget || st == Movement::AlongPath {
            self.pax_blockers(i, buses, bus_ix)
        } else {
            (Obstruction::Clear, true, true)
        };
        let settling = self.ik && self.people[i].pose.settling();
        let natural = self.ik;
        let person = &self.people[i];
        let mut pace = if natural && p0.inside.is_none() && p0.doorway.is_none() {
            crate::humans::natural_pace(&person.ty.def, p0.walk_speed as f64) as f32
        } else {
            p0.walk_speed
        };
        if natural
            && p0.inside.is_none()
            && matches!(p0.task, Task::ToBus | Task::WalkingToBus)
            && bn_t.is_some_and(|b| b.speed.abs() < 0.5)
            && dist > 6.0
        {
            pace = match person.age {
                age if age < 60.0 && crate::humans::person_hash(person.id, 3) < 0.75 => {
                    let base = if age < 13.0 {
                        2.4
                    } else if age < 40.0 {
                        3.0
                    } else {
                        2.5
                    };
                    base * (0.9 + 0.2 * crate::humans::person_hash(person.id, 4) as f32)
                }
                _ => pace * 1.15,
            };
        }
        let p = self.pax_mut(i).unwrap();
        // Inside a bus, people going opposite ways along the aisle or the stairs stood face to
        // face for good (the whole upper deck of a double-decker on its way out, the people
        // coming up stopped on the stairs): held up for two seconds, they squeeze past for a
        // second and a half, as the people on the pavements do.
        if p.inside.is_some() {
            if p.squeeze > 0.0 {
                p.squeeze -= dt;
                block = Obstruction::Clear;
            } else if block == Obstruction::Facing {
                p.jam += dt;
                if p.jam > 0.8 {
                    p.jam = 0.0;
                    p.squeeze = 1.5;
                    block = Obstruction::Clear;
                }
            } else {
                p.jam = 0.0;
            }
        }
        if let Some(t) = walked_to {
            p.target = t;
            p.target_bus = true;
        }
        p.movement = st;
        p.pt = pt;
        p.link = link;
        p.room = room;
        p.step_pack = step_pack;
        p.clamp = false;
        p.clamp_open = false;
        p.clamp_left = false;
        p.clamp_x = -1e9;
        p.moved = 0.0;
        p.speed_des = 0.0;
        p.obstruction = block;
        p.free_r = free_r;
        p.free_l = free_l;
        let mut head_des = p.yaw;
        let mut slope = f64::INFINITY;
        if st == Movement::ToTarget || st == Movement::AlongPath {
            head_des = yaw_of(d.truncate());
            if p.inside.is_some() || p.posture == Posture::Sitting {
                let h = d.truncate().length();
                slope = if h > 0.0 { d.z / h } else { f64::INFINITY };
            }
            p.speed_des = if block < Obstruction::Facing {
                pace
            } else {
                0.0
            };
            if settling {
                p.speed_des = 0.0;
            }
        } else if st == Movement::AtTarget || st == Movement::AtPathEnd {
            head_des = p.target_yaw;
        } else if st == Movement::Turning {
            head_des = yaw_of(d.truncate());
        }
        if st == Movement::Standing {
            // standing: still on the ground under the feet, as Omsi.exe asks for it every
            // tick in every state but turning (0x62b852 -> 0x7aec3c, not when seated); the
            // waiting people stood at the height of their [passpos]'s object - a shelter
            // on the terrain - 25-35 cm down in the platform
            if p.inside.is_none() && p.posture != Posture::Sitting {
                if let Some(g) = world.walk_height_near(p.pos.x, p.pos.y, p.pos.z) {
                    p.pos.z = g;
                }
            }
            return;
        }
        let mut dh = wrap(head_des - p.yaw);
        if st == Movement::ToTarget && p.free_r && p.obstruction == Obstruction::Facing {
            dh = -1.745;
        }
        if natural && st != Movement::Turning {
            if st == Movement::ToTarget || (st == Movement::AlongPath && p.pt == p.pt_target) {
                let left = if p.short { dist - 0.7 } else { dist };
                p.speed_des = p.speed_des.min((4.0 * left.max(0.0)).sqrt() + 0.1);
            }
            p.speed_des *= (dh.cos() as f32).max(0.0);
        }
        if st != Movement::Turning {
            if !natural && dh.abs() > 1.0 {
                p.speed = 0.0;
            }
            let diff = p.speed_des - p.speed;
            let rate = if !natural || block >= Obstruction::Facing {
                5.0
            } else if diff < 0.0 {
                3.0
            } else {
                1.8
            };
            p.speed += diff.signum() * diff.abs().min(rate * dt);
        }
        let turn = dh.signum()
            * if natural {
                (dh.abs() * (dt as f64 / 0.12).min(1.0)).min(4.5 * dt as f64)
            } else {
                dh.abs().min(dt_ms as f64 / 150.0)
            };
        p.yaw = wrap(p.yaw + turn);
        if st == Movement::Turning {
            return;
        }
        let mut moved = p.speed * dt_ms / 1000.0;
        let mut step = DVec3::new(d.x, d.y, 0.0);
        let len = step.length() as f32;
        if len <= moved {
            moved = len;
        } else if len > 0.0 {
            step *= (moved / len) as f64;
        }
        p.moved = moved;
        if p.inside.is_some() || p.posture == Posture::Sitting {
            step.z = if slope.is_finite() {
                moved as f64 * slope
            } else {
                d.z
            };
        } else {
            let at = p.pos + step;
            step.z = match world.walk_height_near(at.x, at.y, p.pos.z) {
                Some(g) => g - p.pos.z,
                None => 0.0,
            };
        }
        p.pos += step;
        if p.inside.is_none()
            && p.posture != Posture::Sitting
            && matches!(p.task, Task::WaitingForBus | Task::WalkingToBusstop)
        {
            if let Some(stop) = p.stop {
                if let Some(s) = self.stops.get(&stop) {
                    let floor = s.pos.z;
                    let p = self.pax_mut(i).unwrap();
                    p.pos.z = p.pos.z.max(floor);
                }
            }
        }
        let _ = dt;
    }

    /// sub_626860: whether somebody within 0.6 m stands in the way.
    pub(in crate::humans) fn pax_blockers(
        &self,
        i: usize,
        buses: &[BusNow],
        bus_ix: &HashMap<BusId, usize>,
    ) -> (Obstruction, bool, bool) {
        let me = self.pax(i).unwrap();
        let Some((my_pos, my_head)) = self.pax_world(me, buses, bus_ix) else {
            return (Obstruction::Clear, true, true);
        };
        let fs = {
            let h = my_head.to_radians();
            DVec2::new(h.sin(), h.cos())
        };
        let (mut block, mut free_r, mut free_l) = (Obstruction::Clear, true, true);
        for (j, o) in self.people.iter().enumerate() {
            if j == i || o.puppet.is_some() {
                continue;
            }
            // who counts: the passengers of the same bus or stop, and the people on foot
            // when this one has no bus yet
            let (o_task, o_movement, o_bus, o_stop, o_sub, o_block) = match &o.state {
                State::Pax(x) => (
                    Some(x.task),
                    Some(x.movement),
                    x.bus,
                    x.stop,
                    x.fare_phase,
                    x.obstruction,
                ),
                // a pedestrian on a path: state 8 with a path, no bus, no stop
                State::Strolling(_) => {
                    (None, None, None, None, FarePhase::None, Obstruction::Clear)
                }
                _ => continue,
            };
            if matches!(o_movement, Some(Movement::Standing | Movement::AtTarget)) {
                continue;
            }
            if o_task == Some(Task::ToBus) && me.task == Task::WalkingToBus {
                continue;
            }
            if !(o_bus == me.bus || (o_stop.is_some() && o_stop == me.stop)) {
                continue;
            }
            let d = o.position - my_pos;
            if d.z >= 2.0 {
                continue;
            }
            let d2 = d.truncate();
            let dist = d2.length();
            if !(dist < 0.6) {
                continue;
            }
            let dn = if dist > 0.0 { d2 / dist } else { DVec2::ZERO };
            let fo = {
                let h = o.heading.to_radians();
                DVec2::new(h.sin(), h.cos())
            };
            if fs.dot(dn) < 0.0 {
                if block == Obstruction::Clear {
                    block = Obstruction::Behind;
                }
                continue;
            }
            // (D3DXVec3Cross(fs, d).y in the left-handed frame)
            let side = fs.y * dn.x - fs.x * dn.y < 0.0;
            let crossing_our_exit = me.task == Task::InBusToExit
                && me.door.is_some()
                && matches!(&o.state, State::Pax(other)
                    if other.task == Task::InBusToExit && other.door == me.door && other.doorway.is_some());
            let facing = !crossing_our_exit && (fo.dot(fs) < 0.2 || dn.dot(fo) <= 0.0);
            if facing && o_sub == FarePhase::None {
                if o_block == Obstruction::Clear {
                    block = block.max(Obstruction::Facing);
                    if side {
                        free_r = false;
                    } else {
                        free_l = false;
                    }
                }
                continue;
            }
            block = block.max(Obstruction::Busy);
        }
        (block, free_r, free_l)
    }
}
