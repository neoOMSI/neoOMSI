//! Passenger movement runs before lifecycle decisions in each frame.

use super::*;

mod comfort;
mod fares;
mod routing;
mod state;
mod stops;
pub(super) use fares::FareDesk;
mod voices;
pub(super) use voices::PassengerVoices;
mod boarding;
pub(super) use comfort::*;
pub(super) use routing::*;
pub(super) use state::*;
pub(super) use stops::*;

/// What the stops say about a bus this frame (sub_61f238): the stop ahead it is pulling
/// in to (+0x7a0), the stops within 60 m (+0x7a4), and whether it empties (+0x7c5).
#[derive(Debug, Clone, Default)]
pub(super) struct BusAtStops {
    pub next: Option<i64>,
    /// The nearest stop whose service box actually contains the bus.
    pub at: Option<i64>,
    pub near: Vec<i64>,
    pub all_exit: bool,
}

const DOOR_GIVE_UP: f32 = 20.0;

fn wrap(a: f64) -> f64 {
    let mut a = a;
    let pi = std::f64::consts::PI;
    while a > pi {
        a -= 2.0 * pi;
    }
    while a < -pi {
        a += 2.0 * pi;
    }
    a
}

/// The heading (radians, clockwise from forward) of a direction in the plane.
fn yaw_of(d: DVec2) -> f64 {
    d.x.atan2(d.y)
}

impl Humans {
    /// The passenger of person `i`, if it is one.
    pub(super) fn pax(&self, i: usize) -> Option<&Pax> {
        match &self.people[i].state {
            State::Pax(p) => Some(p),
            _ => None,
        }
    }
    pub(super) fn pax_mut(&mut self, i: usize) -> Option<&mut Pax> {
        match &mut self.people[i].state {
            State::Pax(p) => Some(p),
            _ => None,
        }
    }

    /// The world position and heading of a passenger.
    pub(super) fn pax_world(
        &self,
        p: &Pax,
        buses: &[BusNow],
        bus_ix: &HashMap<BusId, usize>,
    ) -> Option<(DVec3, f64)> {
        match p.inside {
            None => Some((p.pos, p.yaw.to_degrees())),
            Some(b) => {
                let bn = bus_ix.get(&b).map(|k| &buses[*k])?;
                let l = p.pos.as_vec3();
                Some((bn.world(l), bn.heading_at(l) + p.yaw.to_degrees()))
            }
        }
    }

    /// Everybody's passenger tick of this frame, in the order of the people (sub_6ffc7c).
    #[allow(clippy::too_many_arguments)]
    pub(super) fn pax_frame(
        &mut self,
        dt: f32,
        world: &World,
        net: Option<&Network>,
        buses: &[BusNow],
        bus_ix: &HashMap<BusId, usize>,
        at_stops: &HashMap<BusId, BusAtStops>,
        given_ticket: Option<f32>,
        place_payment: &mut dyn FnMut(&mut crate::money::Money, &[usize], Vec3, [f32; 2]),
        taken_ticket: &mut bool,
        remove: &mut Vec<usize>,
    ) {
        // the requests the buses' scripts read this frame
        for r in self.entry_req.iter_mut().chain(self.exit_req.iter_mut()) {
            *r = false;
        }
        let mut ai_req: HashMap<BusId, (Vec<bool>, Vec<bool>)> = HashMap::new();
        for bn in buses {
            ai_req.insert(
                bn.id,
                (
                    vec![
                        false;
                        bn.cabin.entries.len() + bn.cabin.exits.len() * self.natural as usize
                    ],
                    vec![false; bn.cabin.exits.len()],
                ),
            );
        }
        self.buses.pax_req = ai_req;
        self.door_occupancy.clear();
        self.alighting_occupancy.clear();
        for person in &self.people {
            if let crate::humans::person::State::Pax(pax) = &person.state {
                if pax.task == Task::WalkingToBus && pax.inside.is_none() {
                    if let (Some(bus), Some(door)) = (pax.bus, pax.door) {
                        *self.door_occupancy.entry((bus, door)).or_default() += 1;
                    }
                } else if pax.task == Task::InBusToExit {
                    if let (Some(bus), Some(door)) = (pax.inside, pax.door) {
                        *self.alighting_occupancy.entry((bus, door)).or_default() += 1;
                    }
                }
            }
        }
        for i in 0..self.people.len() {
            if self.pax(i).is_none() || remove.contains(&i) {
                continue;
            }
            self.pax_tick(
                i,
                dt,
                world,
                net,
                buses,
                bus_ix,
                at_stops,
                given_ticket,
                place_payment,
                taken_ticket,
                remove,
            );
        }
        // the player's bus reads its requests from `entry_req` / `exit_req`
        if let Some((e, x)) = self.buses.pax_req.get(&BusId::Player) {
            self.entry_req = e.clone();
            self.exit_req = x.clone();
        }
        self.buses.ai_requests.clear();
        for (b, (e, x)) in &self.buses.pax_req {
            if let BusId::Ai(id) = b {
                self.buses.ai_requests.push((*id, e.clone(), x.clone()));
            }
        }
        // timetable buses wait while people still get on or off. Not for anybody without a
        // place of their own (at the gather point of a full bus, `Task::ToBus`), outside the
        // stop's box, or given up at a door still shut: the bus held for them waited for good.
        // (`door_wait` is not reset when the door opens: it keeps a natural-mode boarder at
        // the open door, see `choose_entry`)
        for bn in buses {
            let BusId::Ai(id) = bn.id else { continue };
            if bn.speed.abs() > 0.5 {
                continue;
            }
            let mut hold = None;
            for p in &self.people {
                let State::Pax(x) = &p.state else { continue };
                if x.bus != Some(bn.id) {
                    continue;
                }
                let boarding = x.task == Task::WalkingToBus
                    && x.seat.is_some()
                    && (x.door_wait < DOOR_GIVE_UP || x.door.is_some_and(|d| bn.boarding_open(d)))
                    && x.stop.is_some_and(|s| self.in_stop_box(s, bn.id));
                let alighting = x.task == Task::InBusToExit && x.inside == Some(bn.id);
                if alighting && x.doorway.is_some() {
                    hold = Some(true);
                    break;
                }
                if boarding || alighting {
                    hold = Some(false);
                }
            }
            if let Some(in_doorway) = hold {
                self.buses.holds.push((id, 2.5, in_doorway));
            }
        }
    }

    /// One person's tick (sub_62a6a0 without the street walk).
    #[allow(clippy::too_many_arguments)]
    pub(super) fn pax_tick(
        &mut self,
        i: usize,
        dt: f32,
        world: &World,
        net: Option<&Network>,
        buses: &[BusNow],
        bus_ix: &HashMap<BusId, usize>,
        at_stops: &HashMap<BusId, BusAtStops>,
        given_ticket: Option<f32>,
        place_payment: &mut dyn FnMut(&mut crate::money::Money, &[usize], Vec3, [f32; 2]),
        taken_ticket: &mut bool,
        remove: &mut Vec<usize>,
    ) {
        let dt_ms = dt * 1000.0;
        if self
            .pax(i)
            .is_some_and(|p| p.task == Task::AwaitingTransfer)
        {
            return;
        }
        // sub_62a258: the bus is gone - nothing more to do with it
        if let Some(bus) = self.pax(i).unwrap().bus.filter(|b| !bus_ix.contains_key(b)) {
            if let Some(seat) = self.pax_mut(i).unwrap().seat.take() {
                self.free_seat(bus, seat);
            }
            if let Some(seat) = self.pax_mut(i).unwrap().vacating.take() {
                self.free_seat(bus, seat);
            }
            self.pax_mut(i).unwrap().bus = None;
        }
        // inside a bus that is gone (a timetable bus left the map): gone with it
        if let Some(b) = self.pax(i).unwrap().inside {
            if !bus_ix.contains_key(&b) {
                remove.push(i);
                return;
            }
        }
        {
            let p = self.pax_mut(i).unwrap();
            if p.timer > 0.0 {
                p.timer -= dt;
            }
            if p.dist_timer > 0.0 {
                p.dist_timer -= p.moved;
            }
        }
        // the toll of a bad ride eases off as the bus goes on (0x62d86c: 0.2 a kilometre)
        {
            let speed = self
                .pax(i)
                .unwrap()
                .inside
                .and_then(|b| bus_ix.get(&b))
                .map(|k| buses[*k].speed.abs() as f32);
            let p = self.pax_mut(i).unwrap();
            match speed {
                Some(v) => p.discomfort = (p.discomfort - v * dt / 5000.0).max(0.0),
                None => p.discomfort = 0.0,
            }
        }
        if !self.advance_seat_approach(i, dt) && self.pax(i).unwrap().doorway.is_none() {
            self.pax_move(i, dt, dt_ms, world, buses, bus_ix);
        }
        self.leave_seat_floor(i);
        self.pax_task(
            i,
            dt,
            world,
            net,
            buses,
            bus_ix,
            at_stops,
            given_ticket,
            place_payment,
            taken_ticket,
        );
        // (got off: a pedestrian now)
        let Some(p) = self.pax(i).cloned() else {
            return;
        };
        // (0x62d75b) the stop they boarded at is forgotten once the bus has left it
        if matches!(
            p.task,
            Task::InBusToPlace | Task::InBusToExit | Task::SittingInBus
        ) {
            if let (Some(stop), Some(b)) = (p.stop, p.bus) {
                if !at_stops.get(&b).is_some_and(|r| r.near.contains(&stop)) {
                    let p = self.pax_mut(i).unwrap();
                    p.stop = None;
                }
            }
        }
        // where the person is drawn
        if let Some((w, h)) = self.pax_world(&p, buses, bus_ix) {
            let person = &mut self.people[i];
            person.position = w;
            person.heading = h;
            match p.inside {
                Some(b) => {
                    person.place = Place::Bus(b, p.pos.as_vec3());
                    person.lheading = p.yaw.to_degrees();
                    if let Some(bn) = bus_ix.get(&b).map(|k| &buses[*k]) {
                        person.tilt = bn.tilt_at(p.pos.as_vec3());
                        person.interior = bn.interior;
                    }
                }
                None => {
                    person.place = Place::Ground;
                    person.interior = 0.0;
                }
            }
            let yaw = p.yaw;
            person.vel = if p.movement == Movement::ToTarget || p.movement == Movement::AlongPath {
                DVec2::new(yaw.sin(), yaw.cos()) * p.speed as f64
            } else {
                DVec2::ZERO
            };
        }
    }

    /// The task part of the tick (sub_62a6a0 from 0x62b984).
    #[allow(clippy::too_many_arguments)]
    pub(super) fn pax_task(
        &mut self,
        i: usize,
        dt: f32,
        world: &World,
        net: Option<&Network>,
        buses: &[BusNow],
        bus_ix: &HashMap<BusId, usize>,
        at_stops: &HashMap<BusId, BusAtStops>,
        given_ticket: Option<f32>,
        place_payment: &mut dyn FnMut(&mut crate::money::Money, &[usize], Vec3, [f32; 2]),
        taken_ticket: &mut bool,
    ) {
        let p = self.pax(i).unwrap().clone();
        let bn = p.bus.and_then(|b| bus_ix.get(&b).map(|k| &buses[*k]));
        match p.task {
            Task::WaitingForBus => {
                let Some(stop) = p.stop else { return };
                let Some(b) = self.bus_for(i, stop, buses, bus_ix) else {
                    return;
                };
                let Some(bn) = bus_ix.get(&b).map(|k| &buses[*k]) else {
                    return;
                };
                self.pax_mut(i).unwrap().bus = Some(b);
                // still rolling in, or standing in the stop's box: to the gather point
                if bn.speed.abs() <= 2.0 && !self.in_stop_box(stop, b) {
                    return;
                }
                self.set_task(i, Task::ToBus, buses, bus_ix, world);
            }
            Task::ToBus => {
                let Some(stop) = p.stop else { return };
                if let Some(bn) = bn {
                    if bn.speed.abs() < 3.0 && self.in_stop_box(stop, bn.id) {
                        let from = match p.ticket {
                            TicketAction::Buy => bn.cabin.sale.and_then(|s| s.0),
                            TicketAction::Stamp => bn.cabin.stamper.and_then(|s| s.0),
                            _ => None,
                        };
                        if let Some(k) = self.reserve_place(bn.id, &bn.cabin, from, false) {
                            let pp = self.pax_mut(i).unwrap();
                            pp.seat = Some(k);
                            self.set_task(i, Task::WalkingToBus, buses, bus_ix, world);
                        }
                    }
                }
                self.pax_mut(i).unwrap().short = true;
                // the bus is gone from the stop: back to a waiting place
                let gone = match self.pax(i).unwrap().bus {
                    Some(b) => !self.listed_at(stop, b),
                    None => true,
                };
                if gone && self.pax(i).unwrap().task == Task::ToBus {
                    self.set_task(i, Task::WalkingToBusstop, buses, bus_ix, world);
                }
            }
            Task::WalkingToBus => self.task_to_bus(i, dt, buses, bus_ix, world),
            Task::InBusToPlace => self.task_to_place(
                i,
                buses,
                bus_ix,
                world,
                given_ticket,
                place_payment,
                taken_ticket,
            ),
            Task::InBusToExit => self.task_to_exit(i, dt, buses, bus_ix, at_stops, world, net),
            Task::WalkingToBusstop => {
                if p.movement == Movement::AtTarget {
                    self.set_task(i, Task::WaitingForBus, buses, bus_ix, world);
                } else {
                    self.pax_mut(i).unwrap().posture = Posture::Walking;
                }
            }
            Task::SittingInBus => {
                let Some(b) = p.inside else { return };
                let reg = at_stops.get(&b).cloned().unwrap_or_default();
                let km = self.buses.odometer.get(&b).copied().unwrap_or(0.0);
                if reg.all_exit {
                    self.set_task(i, Task::InBusToExit, buses, bus_ix, world);
                    return;
                }
                if let (Some(next), Some(dest)) = (reg.next, p.journey.dest.as_ref()) {
                    let name = self
                        .stops
                        .get(&next)
                        .map(|s| s.name.trim().to_string())
                        .unwrap_or_default();
                    if self.stops.get(&next).is_some_and(|s| s.is_named(dest)) {
                        self.set_task(i, Task::InBusToExit, buses, bus_ix, world);
                        return;
                    }
                    if p.journey.alt.as_ref().is_some_and(|a| a.trim() == name)
                        && !p.journey.alt_seen
                    {
                        let r = self.rand_f() as f32;
                        let pp = self.pax_mut(i).unwrap();
                        pp.journey.alt_seen = true;
                        pp.journey.km_start = km;
                        pp.journey.ride_km = (pp.journey.alt_m / 1000.0) * (0.2 + 0.6 * r);
                    }
                }
                let p = self.pax(i).unwrap();
                if (p.journey.alt_seen || p.journey.dest.is_none())
                    && p.journey.km_start + (p.journey.ride_km as f64) < km
                {
                    self.set_task(i, Task::InBusToExit, buses, bus_ix, world);
                    return;
                }
                let Some(bn) = bus_ix.get(&b).map(|k| &buses[*k]) else {
                    return;
                };
                self.move_to_free_seat(i, bn, reg.at.is_some());
            }
            Task::Nothing | Task::AwaitingTransfer => {}
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Smooth driving upsets nobody; a hard stop, a fast bend and a jerky foot do, as in
    /// Omsi.exe (#862).
    #[test]
    fn the_riders_feel_hard_braking_fast_bends_and_a_jerky_foot() {
        let dt = 0.02;
        let run = |f: &dyn Fn(f64) -> (f32, f32, f32), secs: f64| {
            let mut c = RideComfort::default();
            let mut jolts = Vec::new();
            let mut t = 0.0;
            while t < secs {
                let (v, lat, long) = f(t);
                let k = c.step(dt, (t + 10.0) * 1000.0, v, lat, long, 0);
                if k > 0.0 {
                    jolts.push((t, k));
                }
                t += dt as f64;
            }
            jolts
        };
        // pulling away at 1.2 m/s², cruising, braking at 1.5 m/s² to a stop: nothing
        assert!(
            run(
                &|t| if t < 10.0 {
                    (1.2 * t as f32, 0.0, 1.2)
                } else if t < 20.0 {
                    (12.0, 0.0, 0.0)
                } else if t < 28.0 {
                    (12.0 - 1.5 * (t as f32 - 20.0), 0.0, -1.5)
                } else {
                    (0.0, 0.0, 0.0)
                },
                40.0
            )
            .is_empty()
        );
        // a gentle bend at 1.5 m/s² sideways
        assert!(run(&|_| (10.0, 1.5, 0.0), 10.0).is_empty());
        // an emergency stop at 7 m/s²: one jolt, not one a frame
        let hard = run(
            &|t| {
                if t < 1.0 {
                    (14.0, 0.0, 0.0)
                } else {
                    (14.0, 0.0, -7.0)
                }
            },
            2.0,
        );
        assert_eq!(hard.len(), 1, "{hard:?}");
        assert_eq!(hard[0].1, 0.1);
        // a bend at 4 m/s² held for seconds
        assert_eq!(run(&|_| (12.0, 4.0, 0.0), 6.0).len(), 1);
        // throttle and brake every 1.5 s: the fifth swing on upsets them, each further one too
        let jerky = run(
            &|t| {
                (
                    8.0,
                    0.0,
                    if (t / 1.5).floor() as i64 % 2 == 0 {
                        1.0
                    } else {
                        -1.0
                    },
                )
            },
            15.0,
        );
        assert!(
            jerky.len() >= 4 && jerky.iter().all(|j| j.1 == 0.05) && jerky[0].0 > 5.0,
            "{jerky:?}"
        );
        // standing, nothing counts
        assert!(run(&|_| (0.0, 5.0, -8.0), 5.0).is_empty());

        // A registered impact must count even if the fixed physics frame has already
        // averaged its sharp deceleration away by the time the passengers run.
        let mut impact = RideComfort::default();
        assert_eq!(impact.step(dt, 1_000.0, 10.0, 0.0, 0.0, 0), 0.0);
        assert_eq!(impact.step(dt, 1_020.0, 0.0, 0.0, 0.0, 1), 0.15);
        assert_eq!(impact.step(dt, 1_040.0, 0.0, 0.0, 0.0, 1), 0.0);
    }

    #[test]
    fn complaints_come_worst_first_and_once_each() {
        let at = bad_ride_thresholds([0.5, 0.5, 0.5]);
        assert!(
            (at[0] - 0.05).abs() < 1e-6
                && (at[1] - 0.3).abs() < 1e-6
                && (at[2] - 0.65).abs() < 1e-6
        );
        let lo = bad_ride_thresholds([0.0, 0.0, 0.0]);
        let hi = bad_ride_thresholds([1.0, 1.0, 1.0]);
        assert!(
            lo[1] >= 0.2 - 1e-6
                && hi[1] <= 0.4 + 1e-6
                && lo[2] >= 0.5 - 1e-6
                && hi[2] <= 0.8 + 1e-6
        );
        assert_eq!(bad_ride_complaint(0.01, Complaint::None, at), None);
        assert_eq!(
            bad_ride_complaint(0.1, Complaint::None, at),
            Some(Complaint::Mild)
        );
        assert_eq!(bad_ride_complaint(0.1, Complaint::Mild, at), None);
        assert_eq!(
            bad_ride_complaint(0.35, Complaint::Mild, at),
            Some(Complaint::Strong)
        );
        // a crash straight to the top: the worst at once, then nothing more
        assert_eq!(
            bad_ride_complaint(0.9, Complaint::None, at),
            Some(Complaint::Leave)
        );
        assert_eq!(bad_ride_complaint(0.95, Complaint::Leave, at), None);
        // three emergency stops in a row take a rider from nothing past 0.27
        let mut x = 0.0f32;
        for _ in 0..3 {
            x += (1.0 - x) * 0.1;
        }
        assert!((x - 0.271).abs() < 1e-3);
    }

    #[test]
    fn routing_a_cyclic_cabin_reaches_the_destination() {
        let links = [(0, 1, false), (1, 2, false), (2, 0, false)];
        let graph = PathGraph::new(vec![Vec3::ZERO, Vec3::X, Vec3::Y], &links);
        let routes = build_routes(&graph, &links);
        for from in 0..3 {
            for to in 0..3 {
                let mut at = from;
                for _ in 0..3 {
                    if at == to {
                        break;
                    }
                    at = routes[at]
                        .iter()
                        .find(|l| l.reach.contains(&to))
                        .expect("reachable")
                        .to;
                }
                assert_eq!(at, to, "route {from} -> {to} must not loop");
            }
        }
    }

    #[test]
    fn a_stop_answers_to_its_label_and_its_timetable_name() {
        let stop = |alias: &str| PaxStop {
            name: "Königsrath, Bf. Ausstieg".into(),
            alias: alias.into(),
            pos: DVec3::ZERO,
            heading: 0.0,
            gather: DVec3::ZERO,
            spots: Vec::new(),
            taken: Vec::new(),
            enter_max: 1.0,
            enter_min: 0.0,
            length: 30.0,
            lane: None,
            was_near: false,
            near: false,
            clock_ms: 0.0,
            want: 0,
            factor: 1.0,
            buses: Vec::new(),
            dests: Vec::new(),
            lines: Vec::new(),
        };
        let s = stop("Koenigsrath Bf Ausstieg");
        assert!(s.is_named("Königsrath, Bf. Ausstieg "));
        assert!(
            s.is_named("Koenigsrath Bf Ausstieg"),
            "the timetable's spelling"
        );
        assert!(!s.is_named("Königsrath, Bf. Pause"));
        // a stop the timetable does not know: its id, as the riders' destinations then are
        assert!(stop("4711").is_named("4711"));
        assert!(!stop("").is_named(""), "no timetable name: no empty match");
    }

    #[test]
    pub(super) fn routes_follow_the_link_order_and_one_way_links() {
        // 0 - 1 - 2, and 2 -> 0 one way
        let links = [(0, 1, false), (1, 2, false), (2, 0, true)];
        let graph = PathGraph::new(vec![Vec3::ZERO, Vec3::X, Vec3::X * 2.0], &links);
        let r = build_routes(&graph, &links);
        // from 0 to 2: through 1 (0 cannot use the one-way link back from 0 to 2)
        let next = |a: usize, b: usize| r[a].iter().find(|l| l.reach.contains(&b)).map(|l| l.to);
        assert_eq!(next(0, 2), Some(1));
        assert_eq!(next(2, 0), Some(0));
        assert_eq!(next(1, 0), Some(0));
        assert_eq!(next(1, 2), Some(2));
    }

    #[test]
    fn routing_never_walks_a_one_way_link_backwards() {
        let links = [(0, 1, true)];
        let graph = PathGraph::new(vec![Vec3::ZERO, Vec3::X], &links);
        let routes = build_routes(&graph, &links);
        assert!(routes[0].iter().any(|r| r.reach.contains(&1)));
        assert!(routes[1].is_empty());
    }

    #[test]
    fn routing_tables_agree_with_the_graph_and_never_cycle() {
        let pairs = [(0, 1), (0, 2), (0, 3), (1, 2), (1, 3), (2, 3)];
        for zero_length in [false, true] {
            let points = if zero_length {
                vec![Vec3::ZERO; 4]
            } else {
                vec![Vec3::ZERO, Vec3::X, Vec3::Y, Vec3::X + Vec3::Y]
            };
            for mask in 0..4096usize {
                let mut bits = mask;
                let mut links = Vec::new();
                for (a, b) in pairs {
                    match bits % 4 {
                        1 => links.push((a, b, false)),
                        2 => links.push((a, b, true)),
                        3 => links.push((b, a, true)),
                        _ => {}
                    }
                    bits /= 4;
                }
                let graph = PathGraph::new(points.clone(), &links);
                let routes = build_routes(&graph, &links);
                for from in 0..4 {
                    for to in 0..4 {
                        let expected = graph.distance(from, to);
                        let mut at = from;
                        let mut length = 0.0;
                        let mut visited = [false; 4];
                        while at != to {
                            assert!(!visited[at], "cycle: {links:?}, {from}->{to}");
                            visited[at] = true;
                            let Some(route) = routes[at].iter().find(|r| r.reach.contains(&to))
                            else {
                                break;
                            };
                            let (a, b, one) = links[route.link];
                            assert!(
                                (a == at as i32 && b == route.to as i32)
                                    || (!one && b == at as i32 && a == route.to as i32)
                            );
                            length += points[at].distance(points[route.to]);
                            at = route.to;
                        }
                        assert_eq!(at == to, expected.is_finite(), "{links:?}, {from}->{to}");
                        if at == to {
                            assert!((length - expected).abs() < 1e-4);
                        }
                    }
                }
            }
        }
    }
}
