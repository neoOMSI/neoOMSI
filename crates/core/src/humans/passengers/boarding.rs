use super::{
    BusAtStops, Complaint, Doorway, Movement, OUTSIDE_ROOM, Obstruction, Posture, SeatApproach,
    SeatFloor, Task, TicketAction, wrap, yaw_of,
};
use crate::humans::{BusId, BusNow, Cabin, Humans, State, debug_pax, prefer_seated_places};
use crate::scene::World;
use glam::{DVec3, Vec3};
use hashbrown::HashMap;
use ::simulation::human::Activity;
use ::traffic::Network;

fn least_busy_entry(
    points: &[Vec3],
    here: Vec3,
    list: &[Option<usize>],
    buyer: bool,
    flags: &[(bool, bool)],
    open: &[bool],
    queue: &[f32],
) -> Option<usize> {
    let pick = |buyer: bool| {
        list.iter()
            .enumerate()
            .filter_map(|(k, pt)| {
                let pt = (*pt)?;
                let q = *points.get(pt)?;
                let (sells_not, button) = flags.get(k).copied().unwrap_or((false, false));
                if (!open.get(k).copied().unwrap_or(false) && !button) || (buyer && sells_not) {
                    return None;
                }
                let d = here - q;
                let walk = Vec3::new(d.x, d.y, d.z * 5.0).length();
                let sale = if !buyer && !sells_not { 4.0 } else { 0.0 };
                Some((walk + sale + queue.get(k).copied().unwrap_or(0.0), pt))
            })
            .min_by(|a, b| a.0.total_cmp(&b.0))
            .map(|choice| choice.1)
    };
    pick(buyer).or_else(|| if buyer { pick(false) } else { None })
}

impl Humans {
    /// sub_62e42c: a new task and what it starts with.
    pub(in crate::humans) fn set_task(
        &mut self,
        i: usize,
        t: Task,
        buses: &[BusNow],
        bus_ix: &HashMap<BusId, usize>,
        world: &World,
    ) {
        if self.pax(i).is_none_or(|p| p.task == t) {
            return;
        }
        if debug_pax() {
            log::info!(
                "t={:.1} pax {} {} -> {}",
                self.time,
                self.people[i].label(),
                self.pax(i).unwrap().task.name(),
                t.name()
            );
        }
        let seatheight = self.people[i].ty.def.seat_height;
        if t != Task::InBusToPlace {
            self.cancel_fare(self.people[i].id);
        }
        self.pax_mut(i).unwrap().task = t;
        if t != Task::InBusToExit {
            let p = self.pax_mut(i).unwrap();
            if let (Some(k), Some(b)) = (p.vacating.take(), p.inside.or(p.bus)) {
                self.free_seat(b, k);
            }
        }
        if t != Task::SittingInBus {
            self.pax_mut(i).unwrap().seat_approach = None;
        }
        if t != Task::SittingInBus && t != Task::InBusToExit {
            self.pax_mut(i).unwrap().seat_floor = None;
        }
        match t {
            Task::WaitingForBus => {
                let (stop, spot) = {
                    let p = self.pax(i).unwrap();
                    (p.stop, p.spot)
                };
                let sp = stop
                    .zip(spot)
                    .and_then(|(s, k)| self.stops.get(&s).and_then(|s| s.spots.get(k)).cloned());
                let floor = sp.as_ref().map(|sp| {
                    let near = sp.pos.z - sp.height as f64;
                    world
                        .walk_height_near(sp.pos.x, sp.pos.y, near)
                        .unwrap_or(near)
                });
                let procedural = self.ik;
                let front = self.people[i].ty.rig.seat_front();
                let p = self.pax_mut(i).unwrap();
                p.movement = Movement::Standing;
                match sp {
                    Some(sp) if sp.height != 0.0 && procedural => {
                        p.seat_h = sp.height;
                        p.seat_approach = Some(SeatApproach {
                            target: sp.foot_root(front, floor.unwrap()),
                            yaw: sp.face.to_radians(),
                        });
                        p.posture = Posture::Standing;
                    }
                    Some(sp) if sp.height != 0.0 && !procedural => {
                        p.seat_h = sp.height;
                        p.pos = sp.pos - DVec3::Z * seatheight as f64;
                        p.yaw = sp.face.to_radians();
                        p.posture = Posture::Sitting;
                    }
                    Some(sp) => {
                        p.pos = DVec3::new(sp.pos.x, sp.pos.y, floor.unwrap());
                        p.seat_h = 0.0;
                        p.yaw = sp.face.to_radians();
                        p.posture = Posture::Standing;
                    }
                    None => p.posture = Posture::Standing,
                }
                let position = p.pos;
                let heading = p.yaw.to_degrees();
                self.people[i].position = position;
                self.people[i].heading = heading;
            }
            Task::ToBus => {
                let fare = self
                    .pax(i)
                    .unwrap()
                    .bus
                    .and_then(|b| bus_ix.get(&b).map(|k| &buses[*k]))
                    .map(|bn| self.decide_pax_ticket(i, bn))
                    .unwrap_or((TicketAction::None, 0));
                let price = self
                    .tickets
                    .as_ref()
                    .and_then(|t| t.tickets.get(fare.1.saturating_sub(1) as usize))
                    .map(|t| t.value)
                    .unwrap_or(0.0);
                let (stop, spot) = {
                    let p = self.pax(i).unwrap();
                    (p.stop, p.spot)
                };
                if let (Some(s), Some(k)) = (stop, spot) {
                    self.free_spot(s, k);
                }
                let spread = if self.natural {
                    1.8 * (2.0 * crate::humans::person_hash(self.people[i].id, 5) - 1.0)
                } else {
                    0.0
                };
                let gather = stop.and_then(|s| self.stops.get(&s)).map(|s| {
                    let heading = s.heading.to_radians();
                    s.gather + DVec3::new(heading.sin(), heading.cos(), 0.0) * spread
                });
                let p = self.pax_mut(i).unwrap();
                p.ticket = fare.0;
                p.ticket_id = fare.1;
                p.price = if fare.1 > 0 { price } else { 0.0 };
                p.spot = None;
                if let Some(g) = gather {
                    p.target = g;
                }
                p.target_bus = false;
                p.movement = Movement::ToTarget;
                p.posture = Posture::Walking;
            }
            Task::WalkingToBus => {
                let (stop, spot) = {
                    let p = self.pax(i).unwrap();
                    (p.stop, p.spot)
                };
                if let (Some(s), Some(k)) = (stop, spot) {
                    self.free_spot(s, k);
                }
                let p = self.pax_mut(i).unwrap();
                p.spot = None;
                p.door_wait = 0.0;
                self.choose_entry(i, buses, bus_ix);
                let p = self.pax_mut(i).unwrap();
                p.target_bus = true;
                p.movement = Movement::ToTarget;
                p.posture = Posture::Walking;
            }
            Task::InBusToPlace => {
                let bus = self.pax(i).unwrap().bus;
                let Some(bn) = bus.and_then(|b| bus_ix.get(&b).map(|k| &buses[*k])) else {
                    return;
                };
                let km = self.buses.odometer.get(&bn.id).copied().unwrap_or(0.0);
                let detailed = bn.id == BusId::Player || self.ik || self.natural;
                let all = bn.cabin.all_points();
                let door_idx = self.pax(i).unwrap().door;
                let p = self.pax_mut(i).unwrap();
                p.posture = Posture::Walking;
                p.journey.km_start = km;
                p.door = None;
                // into the bus's frame
                let mut local = bn.to_local(p.pos);
                let door_entry = door_idx.and_then(|d| bn.cabin.boarding_door(d));
                let entry_pt = door_entry.and_then(|e| e.point);
                let pt = entry_pt
                    .or_else(|| bn.cabin.omsi_nearest(local, &all, false, false, None, None));
                if let Some(q) = pt.and_then(|k| bn.cabin.graph.points.get(k)) {
                    local.z = q.z;
                } else if let Some(d) = door_entry {
                    local.z = d.inside.z;
                }
                p.yaw = wrap(p.yaw - bn.heading_at(local).to_radians());
                p.pos = local.as_dvec3();
                p.inside = Some(bn.id);
                p.target_bus = true;
                p.pt = pt;
                p.movement = Movement::AlongPath;
                if !detailed {
                    // a bus not the player's: at the place at once (sub_62a358 + task 7)
                    self.set_task(i, Task::SittingInBus, buses, bus_ix, world);
                    return;
                }
                let ticket = p.ticket;
                match ticket {
                    TicketAction::Stamp => p.pt_target = bn.cabin.stamper.and_then(|s| s.0),
                    TicketAction::Buy => p.pt_target = bn.cabin.sale.and_then(|s| s.0),
                    _ => self.route_to_place(i, bn),
                }
                let p = self.pax_mut(i).unwrap();
                if p.pt_target.is_none() {
                    // (no path point at the device: straight on to the place)
                    p.ticket = TicketAction::None;
                    self.route_to_place(i, bn);
                }
            }
            Task::InBusToExit => {
                let bus = self.pax(i).unwrap().bus.or(self.pax(i).unwrap().inside);
                let Some(bn) = bus.and_then(|b| bus_ix.get(&b).map(|k| &buses[*k])) else {
                    return;
                };
                // the stop button
                if bn.id == BusId::Player {
                    self.stop_request = true;
                }
                let seat = self.pax(i).unwrap().seat;
                let all = bn.cabin.all_points();
                let start = seat
                    .and_then(|k| bn.cabin.seats.get(k))
                    .and_then(|s| s.point)
                    .or_else(|| {
                        bn.cabin.omsi_nearest(
                            self.pax(i).unwrap().pos.as_vec3(),
                            &all,
                            false,
                            true,
                            None,
                            None,
                        )
                    });
                let exit = start.and_then(|from| bn.cabin.nearest_exit(from, &[]));
                let was_seated = seat
                    .and_then(|k| bn.cabin.seats.get(k))
                    .map(|s| s.seated)
                    .unwrap_or(false);
                let ik = self.ik;
                let exit_door = exit.map(|e| e.0);
                let freed_seat = {
                    let p = self.pax_mut(i).unwrap();
                    p.posture = Posture::Walking;
                    p.pt = start;
                    if !ik || !was_seated {
                        if let Some(q) = start.and_then(|k| bn.cabin.graph.points.get(k)) {
                            p.pos = q.as_dvec3();
                        }
                    }
                    // the nearest exit (sub_62a49c / sub_62a5a8)
                    p.pt_target = exit.map(|e| e.1);
                    p.door = exit_door;
                    p.movement = if exit.is_some() {
                        Movement::AlongPath
                    } else {
                        Movement::Standing
                    };
                    p.short = true;
                    p.timer = 1.0;
                    p.door_wait = 0.0;
                    p.seat.take()
                };
                if let Some(d) = exit_door {
                    if let Some((_, x)) = self.buses.pax_req.get_mut(&bn.id) {
                        if let Some(r) = x.get_mut(d) {
                            *r = true;
                        }
                    }
                }
                if let Some(k) = freed_seat {
                    if ik && was_seated {
                        self.pax_mut(i).unwrap().vacating = Some(k);
                    } else {
                        self.free_seat(bn.id, k);
                    }
                }
                self.people[i].activity = Activity::Stand;
            }
            Task::WalkingToBusstop => {
                let reservation = self.pax_mut(i).unwrap().seat.take();
                let old_bus = self.pax(i).unwrap().bus;
                if let (Some(bus), Some(seat)) = (old_bus, reservation) {
                    self.free_seat(bus, seat);
                }
                let r = self.rand_f() as f32;
                let stop = self.pax(i).unwrap().stop;
                {
                    let p = self.pax_mut(i).unwrap();
                    p.short = false;
                    p.door = None;
                    p.bus = None;
                    p.target_bus = false;
                    p.journey.ride_km = r * 19.0 + 1.0;
                }
                // a free waiting place (sub_61fed0)
                let spot = match (stop, self.pax(i).unwrap().spot) {
                    (_, Some(k)) => Some(k),
                    (Some(s), None) => self.take_spot(s, Some(self.people[i].id)),
                    _ => None,
                };
                let sp = stop
                    .zip(spot)
                    .and_then(|(s, k)| self.stops.get(&s).and_then(|s| s.spots.get(k)).cloned());
                let stop_pos = stop.and_then(|s| self.stops.get(&s)).map(|s| s.pos);
                let p = self.pax_mut(i).unwrap();
                p.spot = spot;
                p.target_bus = false;
                match sp {
                    Some(sp) => {
                        // a seat: in front of it, the hip at its height (0x62e5d1)
                        let mut tgt = sp.pos;
                        if sp.height != 0.0 {
                            tgt.z = tgt.z.min(
                                (sp.pos.z - sp.height as f64)
                                    .max(stop_pos.map(|s| s.z).unwrap_or(tgt.z)),
                            );
                        }
                        p.target = tgt;
                        p.target_yaw = sp.face.to_radians();
                    }
                    None => {
                        if let Some(sp) = stop_pos {
                            p.target = sp;
                        }
                        p.target_yaw = 0.0;
                    }
                }
                p.movement = Movement::ToTarget;
            }
            Task::SittingInBus => {
                let bus = self.pax(i).unwrap().inside;
                let Some(bn) = bus.and_then(|b| bus_ix.get(&b).map(|k| &buses[*k])) else {
                    return;
                };
                let seat = self
                    .pax(i)
                    .unwrap()
                    .seat
                    .and_then(|k| bn.cabin.seats.get(k))
                    .cloned();
                let ik = self.ik;
                let seat_front = self.people[i].ty.rig.seat_front() as f64;
                {
                    let p = self.pax_mut(i).unwrap();
                    p.movement = Movement::Standing;
                    if let Some(s) = seat {
                        if s.seated {
                            p.seat_h = s.height;
                            if ik {
                                let r = (s.rot as f64).to_radians();
                                let floor = DVec3::new(
                                    s.pos.x as f64 + r.sin() * seat_front,
                                    s.pos.y as f64 + r.cos() * seat_front,
                                    (s.pos.z - s.height) as f64,
                                );
                                p.seat_approach = Some(SeatApproach {
                                    target: floor,
                                    yaw: r,
                                });
                                p.seat_floor =
                                    ((floor.z - p.pos.z).abs() > 0.03).then(|| SeatFloor {
                                        center: floor.truncate(),
                                        radius: SEAT_FLOOR_RADIUS,
                                        z: floor.z,
                                        around: p.pos.z,
                                    });
                                p.posture = Posture::Standing;
                            } else {
                                p.pos = (s.pos - Vec3::Z * seatheight).as_dvec3();
                                p.posture = Posture::Sitting;
                            }
                        } else {
                            p.pos = s.pos.as_dvec3();
                            p.posture = Posture::Standing;
                        }
                        if !ik || !s.seated {
                            p.yaw = (s.rot as f64).to_radians();
                        }
                    }
                    p.room = OUTSIDE_ROOM;
                    p.reach = false;
                    p.look_driver = false;
                }
            }
            Task::Nothing | Task::AwaitingTransfer => {}
        }
    }

    pub(in crate::humans) fn leave_seat_floor(&mut self, i: usize) {
        if let Some(p) = self.pax_mut(i)
            && p.task == Task::InBusToExit
            && p.seat_floor.is_some_and(|f| {
                (p.pos.truncate() - f.center).length() > f.radius + SEAT_FLOOR_CLEAR
            })
        {
            p.seat_floor = None;
        }
    }

    pub(in crate::humans) fn advance_seat_approach(&mut self, i: usize, dt: f32) -> bool {
        let Some(approach) = self.pax(i).and_then(|p| p.seat_approach) else {
            return false;
        };
        let p = self.pax_mut(i).unwrap();
        p.moved = 0.0;
        // walk there facing the way (stepping up onto a podium on the way), then turn
        // round with the back to the seat
        let delta = (approach.target - p.pos).truncate();
        let distance = delta.length();
        if distance > 0.02 {
            let dh = wrap(yaw_of(delta) - p.yaw);
            p.yaw = wrap(
                p.yaw + dh.signum() * (dh.abs() * (dt as f64 / 0.12).min(1.0)).min(2.3 * dt as f64),
            );
            let want =
                (0.7f32).min((4.0 * distance as f32).sqrt() + 0.1) * (dh.cos() as f32).max(0.0);
            let diff = want - p.speed;
            let rate = if diff < 0.0 { 3.0 } else { 1.8 };
            p.speed += diff.signum() * diff.abs().min(rate * dt);
            let step = (p.speed.max(0.0) * dt) as f64;
            let step = step.min(distance);
            let flat = p.pos.truncate() + delta * (step / distance);
            let z = match p.seat_floor {
                Some(f) => f.at(flat),
                None => p.pos.z + (approach.target.z - p.pos.z) * (step / distance),
            };
            p.pos = DVec3::new(flat.x, flat.y, z);
            p.moved = step as f32;
            p.movement = Movement::ToTarget;
            return true;
        }
        p.pos = approach.target;
        p.speed = 0.0;
        let turn = wrap(approach.yaw - p.yaw);
        let turn_step = 2.2 * dt as f64;
        p.yaw = wrap(p.yaw + turn.clamp(-turn_step, turn_step));
        p.movement = Movement::Turning;
        if turn.abs() <= turn_step {
            p.yaw = approach.yaw;
            p.seat_approach = None;
            p.movement = Movement::Standing;
            p.posture = Posture::Sitting;
        }
        true
    }

    /// sub_625b98: the entry to walk to, every frame on the way (the nearest open one or
    /// one with a button; one selling tickets for a buyer), and its index for the request.
    pub(in crate::humans) fn choose_entry(
        &mut self,
        i: usize,
        buses: &[BusNow],
        bus_ix: &HashMap<BusId, usize>,
    ) {
        let p = self.pax(i).unwrap().clone();
        let Some(bn) = p.bus.and_then(|b| bus_ix.get(&b).map(|k| &buses[*k])) else {
            return;
        };
        let here = match p.inside {
            Some(_) => p.pos.as_vec3(),
            None => bn.to_local(p.pos),
        };
        let mut list = bn.cabin.entry_points();
        let seat = p
            .seat
            .and_then(|k| bn.cabin.seats.get(k))
            .and_then(|s| s.point);
        let reachable =
            |from: usize, to: usize| from == to || bn.cabin.route_next(from, to).is_some();
        let buyer = p.ticket == TicketAction::Buy;
        let sale = bn.cabin.sale.and_then(|s| s.0);
        for entry in &mut list {
            if let Some(from) = *entry {
                if seat.is_some_and(|to| !reachable(from, to))
                    || (buyer && sale.is_some_and(|to| !reachable(from, to)))
                {
                    *entry = None;
                }
            }
        }
        let mut flags = bn.cabin.entry_flags();
        let mut open: Vec<bool> = (0..list.len()).map(|k| bn.boarding_open(k)).collect();
        let pt = if self.natural {
            let stamper = bn
                .cabin
                .stamper
                .and_then(|s| s.0)
                .filter(|_| p.ticket == TicketAction::Stamp);
            if !buyer && self.rear_entry {
                for (k, exit) in bn.cabin.exits.iter().enumerate() {
                    list.push(exit.point.filter(|&from| {
                        bn.cabin.entries.iter().all(|e| e.point != Some(from))
                            && seat.is_none_or(|to| reachable(from, to))
                            && stamper.is_none_or(|to| reachable(from, to))
                    }));
                    flags.push((true, exit.button));
                    open.push(bn.exit_open.get(k).copied().unwrap_or(false));
                }
            }
            let mut queue = self.door_queues(i, bn.id, list.len());
            let entries = bn.cabin.entries.len();
            for (door, cost) in queue.iter_mut().enumerate().skip(entries) {
                *cost += 2.0 * self.alighting_through(bn.id, door - entries) as f32;
            }
            if let Some(to) = stamper {
                let inside = bn.cabin.graph.distances_from(to);
                for (cost, pt) in queue.iter_mut().zip(&list) {
                    *cost += pt
                        .and_then(|pt| inside.get(pt).copied())
                        .filter(|d| d.is_finite())
                        .unwrap_or(0.0);
                }
            }
            let pick = |flags: &[(bool, bool)]| {
                least_busy_entry(
                    &bn.cabin.graph.points,
                    here,
                    &list,
                    buyer,
                    flags,
                    &open,
                    &queue,
                )
            };
            let only_open: Vec<(bool, bool)> = flags.iter().map(|f| (f.0, false)).collect();
            (p.door_wait >= 5.0)
                .then(|| pick(&only_open))
                .flatten()
                .or_else(|| pick(&flags))
        } else {
            None
        }
        .or_else(|| {
            bn.cabin
                .omsi_nearest(here, &list, buyer, false, Some(&flags), Some(&open))
        });
        let p = self.pax_mut(i).unwrap();
        if let Some(q) = pt.and_then(|k| bn.cabin.graph.points.get(k)) {
            p.target = q.as_dvec3();
            p.target_bus = true;
        }
        p.door = pt.and_then(|t| list.iter().position(|e| *e == Some(t)));
    }

    /// Queue cost for each entry, with a stable per-person bias and hysteresis.
    pub(in crate::humans) fn door_queues(&self, i: usize, bus: BusId, n: usize) -> Vec<f32> {
        let mut queue = vec![0.0; n];
        for (j, person) in self.people.iter().enumerate() {
            let State::Pax(pax) = &person.state else {
                continue;
            };
            if j != i
                && pax.bus == Some(bus)
                && pax.task == Task::WalkingToBus
                && pax.inside.is_none()
                && let Some(door) = pax.door.filter(|door| *door < n)
            {
                queue[door] += 2.0;
            }
        }
        let id = self.people[i].id;
        for (door, cost) in queue.iter_mut().enumerate() {
            *cost += 3.0 * (crate::humans::person_hash(id, 20 + door as u32) as f32 - 0.5);
        }
        if let Some(door) = self
            .pax(i)
            .and_then(|pax| pax.door)
            .filter(|door| *door < n)
        {
            queue[door] -= 1.5;
        }
        queue
    }

    pub(in crate::humans) fn alighting_through(&self, bus: BusId, exit: usize) -> usize {
        self.people
            .iter()
            .filter(|person| {
                matches!(&person.state, State::Pax(pax)
                    if pax.inside == Some(bus)
                        && pax.task == Task::InBusToExit
                        && pax.door == Some(exit))
            })
            .count()
    }

    /// sub_62a628: along the paths to the place reserved.
    pub(in crate::humans) fn route_to_place(&mut self, i: usize, bn: &BusNow) {
        let seat = self
            .pax(i)
            .unwrap()
            .seat
            .and_then(|k| bn.cabin.seats.get(k))
            .and_then(|s| s.point);
        let p = self.pax_mut(i).unwrap();
        if let Some(point) = seat {
            p.pt_target = Some(point);
        }
        p.movement = Movement::AlongPath;
        p.smooth = false;
    }

    pub(in crate::humans) fn move_to_free_seat(
        &mut self,
        i: usize,
        bn: &BusNow,
        at_stop: bool,
    ) -> bool {
        let Some(p) = self.pax(i) else { return false };
        if p.task != Task::SittingInBus
            || bn.speed.abs() >= 0.1
            || !at_stop
            || !p
                .seat
                .and_then(|k| bn.cabin.seats.get(k))
                .is_some_and(|s| !s.seated)
        {
            return false;
        }
        let from = bn.cabin.omsi_nearest(
            p.pos.as_vec3(),
            &bn.cabin.all_points(),
            false,
            true,
            None,
            None,
        );
        let Some(from) = from else { return false };
        let Some(seat) = self.reserve_place(bn.id, &bn.cabin, Some(from), true) else {
            return false;
        };
        if let Some(old) = self.pax_mut(i).unwrap().seat.replace(seat) {
            self.free_seat(bn.id, old);
        }
        let pp = self.pax_mut(i).unwrap();
        pp.task = Task::InBusToPlace;
        pp.posture = Posture::Walking;
        pp.pt = Some(from);
        pp.short = false;
        self.route_to_place(i, bn);
        true
    }

    /// sub_7e910c: a free place of the bus, at random (none free: nobody gets on).
    pub(in crate::humans) fn reserve_place(
        &mut self,
        bus: BusId,
        cabin: &Cabin,
        from: Option<usize>,
        seats_only: bool,
    ) -> Option<usize> {
        let n = cabin.seats.len();
        let seats = self
            .buses
            .seats
            .entry(bus)
            .or_insert_with(|| vec![false; n]);
        if seats.len() < n {
            seats.resize(n, false);
        }
        let mut free = cabin.available_places(seats, from, seats_only);
        prefer_seated_places(&mut free, &cabin.seats, self.prefer_seats && !seats_only);
        if free.is_empty() {
            return None;
        }
        let k = if from.is_some() {
            free[0]
        } else {
            free[(self.rand() as usize) % free.len()]
        };
        self.buses.seats.get_mut(&bus).unwrap()[k] = true;
        Some(k)
    }

    /// Task 3 (sub_62a6a0 case 3): to the door and in.
    pub(in crate::humans) fn task_to_bus(
        &mut self,
        i: usize,
        dt: f32,
        buses: &[BusNow],
        bus_ix: &HashMap<BusId, usize>,
        world: &World,
    ) {
        let p = self.pax(i).unwrap().clone();
        let Some(bn) = p.bus.and_then(|b| bus_ix.get(&b).map(|k| &buses[*k])) else {
            self.set_task(i, Task::WalkingToBusstop, buses, bus_ix, world);
            return;
        };
        // (by the path point's side, one in the aisle sent people round the road side)
        let door_side = p
            .door
            .and_then(|d| bn.cabin.boarding_door(d))
            .map(|e| e.side)
            .unwrap_or(1.0);
        let open = p.door.is_some_and(|d| bn.boarding_open(d));
        {
            let pp = self.pax_mut(i).unwrap();
            pp.clamp = true;
            pp.clamp_left = door_side < 0.0;
            pp.clamp_x = if pp.clamp_left {
                bn.centre.x - bn.half.x - 0.5
            } else {
                bn.centre.x + bn.half.x + 0.5
            };
            pp.clamp_open = open;
        }
        // a shut door is asked for, from the moment they stand at it
        if p.movement == Movement::AtTarget || p.movement == Movement::ShortOfTarget {
            if let Some(d) = p.door {
                if let Some((e, _)) = self.buses.pax_req.get_mut(&bn.id) {
                    if let Some(r) = e.get_mut(d) {
                        *r = true;
                    }
                }
            }
        }
        if p.seat.is_none() {
            self.set_task(i, Task::WalkingToBusstop, buses, bus_ix, world);
            return;
        }
        let stop = p.stop;
        let ok = bn.speed.abs() < 3.0
            && stop.is_some_and(|s| self.in_stop_box(s, bn.id))
            && !bn.cabin.graph.points.is_empty();
        if ok {
            if p.movement == Movement::AtTarget && !open {
                let pp = self.pax_mut(i).unwrap();
                pp.movement = Movement::ShortOfTarget;
                pp.short = true;
                self.people[i].why = "entry door closed";
                return;
            }
            if p.movement != Movement::AtTarget {
                if p.movement == Movement::ShortOfTarget && !open {
                    self.pax_mut(i).unwrap().door_wait += dt;
                }
                self.choose_entry(i, buses, bus_ix);
                if self.pax(i).unwrap().door.is_none() {
                    self.pax_mut(i).unwrap().movement = Movement::Standing;
                    self.people[i].why = "no reachable entry";
                    return;
                }
                let door = self.pax(i).unwrap().door;
                let open = door.is_some_and(|d| bn.boarding_open(d));
                let let_off_first = door
                    .and_then(|d| d.checked_sub(bn.cabin.entries.len()))
                    .is_some_and(|exit| self.alighting_through(bn.id, exit) > 0);
                let pp = self.pax_mut(i).unwrap();
                pp.short = !open || let_off_first;
                pp.movement = Movement::ToTarget;
                pp.posture = Posture::Walking;
                return;
            }
            // in the doorway: the driver is greeted (the player's bus)
            if bn.id == BusId::Player {
                self.greet_or_complain(i, bn);
            }
            self.set_task(i, Task::InBusToPlace, buses, bus_ix, world);
            return;
        }
        // the bus pulls away again: the place is given back
        if let Some(k) = p.seat {
            self.free_seat(bn.id, k);
        }
        self.pax_mut(i).unwrap().seat = None;
        if bn.speed.abs() >= 3.0 {
            self.set_task(i, Task::ToBus, buses, bus_ix, world);
        } else {
            self.set_task(i, Task::WalkingToBusstop, buses, bus_ix, world);
        }
    }

    /// Task 5 (case 5): to the exit, out.
    pub(in crate::humans) fn task_to_exit(
        &mut self,
        i: usize,
        dt: f32,
        buses: &[BusNow],
        bus_ix: &HashMap<BusId, usize>,
        at_stops: &HashMap<BusId, BusAtStops>,
        world: &World,
        net: Option<&Network>,
    ) {
        let p = self.pax(i).unwrap().clone();
        let Some(b) = p.inside else { return };
        let Some(bn) = bus_ix.get(&b).map(|k| &buses[*k]) else {
            return;
        };
        let reg = at_stops.get(&b).cloned().unwrap_or_default();
        if let Some(k) = p.vacating {
            let up = p.doorway.is_some()
                || bn
                    .cabin
                    .seats
                    .get(k)
                    .is_none_or(|s| (p.pos.as_vec3() - s.floor).truncate().length() > 0.6);
            if up {
                self.pax_mut(i).unwrap().vacating = None;
                self.free_seat(b, k);
            }
        }
        if bn.speed.abs() >= 1.0 {
            self.pax_mut(i).unwrap().timer = 1.0;
        }
        let door_open = p
            .door
            .map(|d| bn.exit_open.get(d).copied().unwrap_or(false))
            .unwrap_or(false);
        let may_leave = door_open && (reg.at.is_some() || p.complaint == Complaint::Leave);
        self.people[i].why = if p.pt_target.is_none() {
            "no reachable exit"
        } else if bn.speed.abs() >= 1.0 {
            "bus moving"
        } else if reg.at.is_none() && p.complaint != Complaint::Leave {
            "bus outside stop"
        } else if !door_open {
            "exit door closed"
        } else if p.obstruction >= Obstruction::Facing {
            "person blocking exit path"
        } else {
            "walking to exit"
        };
        self.pax_mut(i).unwrap().short = !may_leave;
        let at_exit = p.pt_target.is_some()
            && p.pt == p.pt_target
            && p.pt_target
                .and_then(|pt| bn.cabin.graph.points.get(pt))
                .is_some_and(|q| (p.pos - q.as_dvec3()).length() <= 0.15);
        let out = p.movement == Movement::AtPathEnd && at_exit && bn.speed.abs() < 1.0 && may_leave;
        if !out && p.doorway.is_none() {
            if reg.next.is_none() {
                if bn.id == BusId::Player {
                    self.stop_request = true;
                }
                return;
            }
            if p.timer <= 0.0 {
                // standing a second: perhaps another door opened (0x62d6b1). Once a second:
                // with the timer left run out, the way was found afresh every frame from the
                // nearest point, and whoever had left a point was pulled back to it - the
                // people coming down from the upper deck never got off the stairs.
                self.pax_mut(i).unwrap().timer = 1.0;
                let from = p.pt.or_else(|| {
                    bn.cabin.omsi_nearest(
                        p.pos.as_vec3(),
                        &bn.cabin.all_points(),
                        false,
                        true,
                        None,
                        None,
                    )
                });
                let holds_closed_route = self.natural
                    && !door_open
                    && p.door.zip(p.pt_target).is_some_and(|(door, target)| {
                        bn.cabin.exits.get(door).and_then(|exit| exit.point) == Some(target)
                            && p.pt.is_some_and(|point| {
                                point == target || bn.cabin.route_next(point, target).is_some()
                            })
                    });
                let waiting_at_closed_exit = holds_closed_route
                    && matches!(p.movement, Movement::AtPathEnd | Movement::ShortOfPathEnd);
                let mut replan = !holds_closed_route;
                {
                    let pp = self.pax_mut(i).unwrap();
                    if waiting_at_closed_exit {
                        pp.door_wait += 1.0;
                        replan = pp.door_wait >= 5.0;
                    } else if !holds_closed_route {
                        pp.door_wait = 0.0;
                    }
                }
                if replan {
                    let exit = from.and_then(|from| bn.cabin.nearest_exit(from, &bn.exit_open));
                    let pp = self.pax_mut(i).unwrap();
                    if let Some((door, target)) = exit {
                        if pp.door != Some(door) || pp.pt_target != Some(target) {
                            pp.pt = from;
                            pp.pt_target = Some(target);
                            pp.door = Some(door);
                            pp.movement = Movement::AlongPath;
                        }
                    } else {
                        self.people[i].why = "no reachable exit";
                    }
                }
            }
            if bn.id == BusId::Player {
                self.stop_request = true;
            }
            if let Some(d) = self.pax(i).unwrap().door {
                if let Some((_, x)) = self.buses.pax_req.get_mut(&b) {
                    if let Some(r) = x.get_mut(d) {
                        *r = true;
                    }
                }
            }
            return;
        }
        let Some(exit) = p.door.and_then(|d| bn.cabin.exits.get(d)) else {
            return;
        };
        if p.doorway.is_none() {
            let occupied = self.people.iter().enumerate().any(|(j, person)| {
                j != i
                    && matches!(&person.state, State::Pax(other)
                    if other.inside == Some(b) && other.door == p.door && other.doorway.is_some())
            });
            if occupied {
                return;
            }
            let outside = bn.world(exit.outside);
            let target = world
                .walk_height_near(outside.x, outside.y, outside.z)
                .filter(|floor| (*floor - outside.z).abs() < 1.0)
                .map(|floor| bn.to_local(DVec3::new(outside.x, outside.y, floor)))
                .unwrap_or(exit.outside);
            self.pax_mut(i).unwrap().doorway = Some(Doorway {
                target,
                stop: reg.at,
            });
        }
        let crossing = self.pax(i).unwrap().doorway.unwrap();
        let delta = crossing.target.as_dvec3() - p.pos;
        let distance = delta.length();
        let moved = distance.min(0.85 * dt as f64);
        let pp = self.pax_mut(i).unwrap();
        if distance > 1e-9 {
            pp.pos += delta * (moved / distance);
            let wanted = delta.x.atan2(delta.y);
            let turn = wrap(wanted - pp.yaw);
            pp.yaw = wrap(pp.yaw + turn.clamp(-2.0 * dt as f64, 2.0 * dt as f64));
        }
        pp.movement = Movement::ToTarget;
        pp.posture = Posture::Walking;
        pp.moved = moved as f32;
        pp.speed = if dt > 0.0 { moved as f32 / dt } else { 0.0 };
        if distance > moved {
            return;
        }
        let w = bn.world(pp.pos.as_vec3());
        let h = bn.heading_at(pp.pos.as_vec3()) + pp.yaw.to_degrees();
        // Ticket packs provide a "Thanks" line rather than a dedicated goodbye.  Use it
        // occasionally as the passenger leaves, but never after a serious bad-ride complaint.
        let chat = self.tickets.as_ref().map(|t| t.chattiness).unwrap_or(0.0);
        if bn.id == BusId::Player
            && p.complaint < Complaint::Leave
            && (self.rand_f() as f32) < chat
            && !self.say_ex(i, "Thanks_1", true)
        {
            // A few otherwise compatible packs omit the numbered variant.
            self.say_ex(i, "Thanks", true);
        }
        let stop = crossing.stop;
        let pp = self.pax_mut(i).unwrap();
        pp.inside = None;
        pp.pos = w;
        pp.yaw = h.to_radians();
        if debug_pax() {
            log::info!(
                "t={:.1} pax {} gets off at stop {:?} by exit {:?}",
                self.time,
                self.people[i].label(),
                stop,
                p.door
            );
        }
        self.walk_street(i, w, h, stop, net);
    }
}

#[cfg(test)]
mod tests {
    use super::least_busy_entry;
    use glam::Vec3;

    #[test]
    fn natural_entry_choice_uses_queue_load_after_reachability_and_door_rules() {
        let points = [Vec3::ZERO, Vec3::X, Vec3::Y, Vec3::new(0.0, 5.0, 0.0)];
        let here = Vec3::ZERO;
        let list = [None, Some(1), Some(2), Some(3)];
        let flags = [(false, false), (true, true), (true, false), (false, false)];
        let open = [false, false, true, true];
        let queue = [0.0, 100.0, 0.0, 100.0];

        assert_eq!(
            least_busy_entry(&points, here, &list, false, &flags, &open, &queue),
            Some(2),
            "skip the unreachable nearest door and prefer the open unqueued door"
        );
        assert_eq!(
            least_busy_entry(&points, here, &list, true, &flags, &open, &queue),
            Some(3),
            "a buyer must use the reachable selling door"
        );
        let no_sales = [None, Some(1), Some(2), None];
        assert_eq!(
            least_busy_entry(&points, here, &no_sales, true, &flags, &open, &queue),
            Some(2),
            "a buyer falls back to another eligible door when none can sell"
        );
    }
}

/// How far round the feet's place in front of a seat its own floor reaches.
const SEAT_FLOOR_RADIUS: f64 = 0.35;
const SEAT_FLOOR_CLEAR: f64 = 0.6;
