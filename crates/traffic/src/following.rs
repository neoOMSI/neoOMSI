//! Longitudinal control: speed composition, car following, the comfort envelope, launch
//! behaviour and realized-motion reconciliation.
//!
//! [`BehaviorEnvelope`] owns the shared comfort bounds (comfort acceleration/service braking,
//! the emergency ceiling, jerk, default headway/gap/reaction) with units, and
//! [`LongitudinalDemand`] separates the comfortable command from collision prevention.
//!
//! ## Tuning provenance
//!
//! The constants and envelope defaults here are **provisional/improvement** neoOMSI targets
//! (plan section 6), not values established by the OMSI reference. The one verified content
//! value is `[ai_brakeperformance]` element 4 (the stop-holding correction); the braking
//! strength's remaining values are unresolved and use an explicit provisional class fallback
//! (`BrakingCapability`, `D3`). Driver traits are seeded per vehicle (persistent, not
//! per-frame). The full parameter table with units, rationale and provenance is in
//! `docs/traffic_refactor/MAINTAINER_GUIDE.md`.

use glam::{DVec2, DVec3};
use crate::capabilities::BrakingCapability;
use crate::diagnostics::Reason;
use crate::network::*;
use crate::perception::{RouteFix, project_on_route_indices};

/// How far ahead (m) a car decides which way it goes: far enough to signal a turn in good
/// time and for the steering to look beyond the junction.
const PLAN_AHEAD: f32 = 90.0;
/// At most this many lanes are planned ahead (junction pieces can be a few metres long).
const PLAN_LANES: usize = 10;
/// Linked lanes may end up to 1.5 m apart; over this many metres on either side of a joint
/// the way is bent so that it has no step (half the gap on each side).
const JOINT_BLEND: f32 = 6.0;
/// Seconds of signalling before a lane change begins to move sideways.
const SIGNAL_BEFORE_CHANGE: f32 = 1.2;

/// One AI vehicle moving on the network: where it is along its lanes and which way it is
/// going. The body that follows this way is `ai_motion::AiBody`.
#[derive(Debug, Clone)]
pub struct AiState {
    pub traffic_pool: Option<(usize, std::sync::Arc<Vec<i32>>)>,
    /// The vehicle's `[ai_veh_type]` (0 car, 1 taxi, 2 bus, 3 truck; -1 a timetable bus):
    /// which lanes it may take, see [`Lane::allows`].
    pub veh_type: i32,
    pub lane: usize,
    pub s: f32,
    pub speed: f32,
    pub max_speed_kmh: f32,
    pub accel: f32,
    pub decel: f32,
    pub length: f32,
    pub rng: u64,
    pub blinker: i32,
    pub braking: bool,
    /// Distance travelled, for wheel animation.
    pub odometer: f32,
    /// Lane chosen for after the current one.
    pub planned_next: Option<usize>,
    /// The lanes after `planned_next`, decided `PLAN_AHEAD` metres in advance.
    pub ahead: Vec<usize>,
    /// During a lane change: the way on from the lane the car is moving over to, chosen
    /// when the change begins, so that the steering and the speed for the bends already
    /// know it.
    pub change_plan: Vec<usize>,
    /// The lane the car came from (the way behind it, for the rear axle).
    pub prev_lane: Option<usize>,
    /// Seconds spent waiting to enter a crossing (deadlock breaker).
    pub yield_time: f32,
    /// Fixed lane sequence (timetable track); empty = wander randomly.
    pub route: Vec<usize>,
    /// Position in `route` of the current lane.
    pub route_index: usize,
    /// Lane change in progress.
    pub change: Option<LaneChange>,
    /// Seconds until the next lane change may start.
    pub change_cooldown: f32,
    /// Lateral offset from the lane (m, positive = right): bus bays, swerving round a
    /// parked car. It moves towards `lateral_target` along an S-curve over the distance
    /// driven (`lateral_ramp`), so that the car is straight again when it gets there and
    /// nothing moves while it stands.
    pub lateral: f32,
    pub lateral_target: f32,
    /// The S-curve in progress: (from, to, odometer at its start, its length in m).
    pub lateral_ramp: (f32, f32, f32, f32),
    /// A turn the car has taken a turn lane for (1 left, 2 right): the way out of the
    /// junction is chosen to match.
    pub turn_wish: i32,
    /// Indicator the driver sets for a manoeuvre of its own (pulling away from a stop),
    /// held while `signal_time` runs.
    pub signal: i32,
    pub signal_time: f32,
    /// Sideways acceleration the driver accepts in a bend (m/s²): the speed through a
    /// curve of radius r is at most sqrt(this × r).
    pub lat_accel: f32,
    /// The driver (`TPathInfo`'s rowdy_factor & co): how fast they like to go relative to
    /// the limit, the time gap they keep to the car ahead (s), the distance they stop
    /// behind it (m), the gap in the cross traffic they accept at a junction (s) and how
    /// long they take to move off when the way clears (s). `accel` is how hard they pull
    /// away and `decel` how hard they like to brake.
    pub desire: f32,
    /// Active emergency response; independent of collision-prevention braking.
    pub emergency_drive: bool,
    pub headway: f32,
    pub min_gap: f32,
    pub accept_gap: f32,
    pub reaction: f32,
    /// From the vehicle's origin (the lane position `s`) to its front and rear bumper (m).
    pub front: f32,
    pub rear: f32,
    /// Standing still and held there (by a car ahead, a light, a stop): moving off again
    /// starts after `reaction`; `start_timer` counts it down.
    pub held: bool,
    pub start_timer: f32,
    /// Acceleration of the last step (m/s²).
    pub acc: f32,
    /// At most this much acceleration for now (m/s²): edging out round something standing
    /// close ahead.
    pub accel_cap: Option<f32>,
    /// The driver's calibration envelope (comfort accel/decel/jerk, emergency ceiling).
    pub envelope: BehaviorEnvelope,
    /// The vehicle's braking envelope: the verified stop correction plus the provisional
    /// class braking strength (see [`BrakingCapability`]).
    pub brakes: BrakingCapability,
    /// The body's realized speed (m/s) as fed back by the motion adapter at the end of the
    /// last tick; the command starts from this, not from its own integral.
    pub realized_speed: f32,
    /// Whether the last realized pose was accepted onto the planned route.
    pub reconciled: bool,
    /// Whether the last applied acceleration came from collision prevention rather than the
    /// comfort envelope.
    pub emergency: bool,
}

/// What a car keeps its distance to: the gap from its front bumper to the thing (m), how
/// fast that is moving along the car's way (m/s) and how it is speeding up (m/s²).
#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub struct Lead {
    pub gap: f32,
    pub speed: f32,
    pub acc: f32,
}

impl Lead {
    /// The nearer (more constraining) of two.
    pub fn min(a: Option<Lead>, b: Option<Lead>) -> Option<Lead> {
        match (a, b) {
            (Some(x), Some(y)) => Some(if y.gap < x.gap { y } else { x }),
            (x, None) => x,
            (None, y) => y,
        }
    }
}

/// Seconds a vehicle needs to cover `dist` metres from speed `v`, speeding up at `a` to at
/// most `v_max` (the earliest it can be there when nothing holds it back).
pub fn arrival_time(dist: f32, v: f32, a: f32, v_max: f32) -> f32 {
    if dist <= 0.0 {
        return 0.0;
    }
    let (v, a) = (v.max(0.0), a.max(0.05));
    let v_max = v_max.max(v).max(0.1);
    // speeding up to v_max takes (v_max - v) / a seconds and that much road
    let t_up = (v_max - v) / a;
    let d_up = (v + v_max) * 0.5 * t_up;
    if dist <= d_up {
        (-v + (v * v + 2.0 * a * dist).sqrt()) / a
    } else {
        t_up + (dist - d_up) / v_max
    }
}

/// How far along its S-curve (smoothstep, 0..1) a car moving back from the lane `side`
/// metres to the left has to be before its middle is `clear` metres from that lane's
/// middle (out of the way of the traffic there).
pub fn ramp_progress_for(side: f32, clear: f32) -> f32 {
    if clear <= 0.0 {
        return 0.0;
    }
    if side <= clear {
        return 1.0;
    }
    // the offset left is side × (1 − smoothstep(t)); it is clear once smoothstep(t) ≥ clear / side
    let want = clear / side;
    let (mut lo, mut hi) = (0.0f32, 1.0f32);
    for _ in 0..20 {
        let mid = 0.5 * (lo + hi);
        if smooth01(mid) < want {
            lo = mid;
        } else {
            hi = mid;
        }
    }
    hi
}

/// Hardest braking of an AI driver (m/s²): an emergency stop.
pub const MAX_BRAKE: f32 = 8.0;
/// Gap a car leaves before a stop line or a stop point (m).
/// The distance the follower stops its front bumper short of a stop point, as the service
/// owner must account for when it hands a bus-stop target to the controller.
pub const STOP_LINE_GAP: f32 = 0.6;

/// Comfort envelope for ordinary longitudinal control, with units.
///
/// These are neoOMSI targets (plan section 6), not constants established by the reference.
/// Acceleration, service braking, and the rate of change of comfortable acceleration are
/// bounded together; collision prevention is a separate channel ([`MAX_BRAKE`]) that is not
/// restricted by the comfort jerk.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct BehaviorEnvelope {
    /// Comfortable acceleration (m/s²).
    pub comfort_accel: f32,
    /// Comfortable service braking (m/s²).
    pub comfort_decel: f32,
    /// Largest ordinary (non-emergency) braking (m/s²).
    pub max_decel: f32,
    /// Emergency collision-prevention braking ceiling (m/s²).
    pub emergency_decel: f32,
    /// Largest change of comfortable acceleration in one second (m/s³).
    pub max_jerk: f32,
    /// Default time gap to the vehicle ahead (s).
    pub headway: f32,
    /// Default standstill gap (m).
    pub min_gap: f32,
    /// Default reaction time before moving off (s).
    pub reaction: f32,
}

impl Default for BehaviorEnvelope {
    fn default() -> Self {
        BehaviorEnvelope {
            comfort_accel: 1.5,
            comfort_decel: 2.2,
            max_decel: 5.0,
            emergency_decel: MAX_BRAKE,
            max_jerk: 8.0,
            headway: 1.4,
            min_gap: 2.0,
            reaction: 0.7,
        }
    }
}

/// The separated longitudinal request: the comfortable command, plus a harder emergency
/// value when collision prevention needs more than the comfort envelope allows.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct LongitudinalDemand {
    /// The command inside the comfort envelope (m/s²).
    pub comfort: f32,
    /// Collision-prevention braking (m/s²) when it exceeds the comfort envelope.
    pub emergency: Option<f32>,
    /// The reason that most strongly binds the request.
    pub reason: Reason,
}

impl LongitudinalDemand {
    /// The acceleration actually applied: the emergency value when present, else comfort.
    pub fn effective(self) -> f32 {
        match self.emergency {
            Some(e) if e < self.comfort => e,
            _ => self.comfort,
        }
    }

    /// Whether collision prevention is overriding the comfort envelope.
    pub fn is_emergency(self) -> bool {
        matches!(self.emergency, Some(e) if e < self.comfort)
    }
}

/// The realized pose and speed of the body that carries a vehicle, fed back by the motion
/// adapter. The domain commits route progress from this; it never advances its own pose past
/// the realized body.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct RealizedMotion {
    pub pose: DVec3,
    /// Realized heading (deg, clockwise from north).
    pub heading_deg: f32,
    /// Realized speed along the body heading (m/s).
    pub speed: f32,
    /// The body's half width (m), for the route projection.
    pub half_width: f64,
}

/// How far a projected realized body may lie off its route before the projection is
/// rejected (m), so a neighbouring parallel road cannot capture it.
const FEEDBACK_MAX_LATERAL: f64 = 1.0;
/// How far the realized heading may differ from the route before the projection is rejected
/// (deg).
const FEEDBACK_MAX_TURN: f32 = 60.0;

/// A lane change: the car moves over from its lane to `to` along `length` metres of road
/// (by distance, not by time: a car that has to stop halfway stands still, and so does its
/// way).
#[derive(Debug, Clone, Copy)]
pub struct LaneChange {
    pub to: usize,
    /// Progress 0..1.
    pub t: f32,
    pub length: f32,
    /// Position along `to`.
    pub s_to: f32,
    /// 1 left, 2 right.
    pub dir: i32,
    /// Seconds of indicating left before the car starts to move over.
    pub wait: f32,
    /// Pulling out from behind something standing: the car moves off sideways from a
    /// standstill, and what it leaves behind in its lane no longer holds it.
    pub bypass: bool,
}

/// Smoothstep 0..1.
pub(crate) fn smooth01(t: f32) -> f32 {
    let t = t.clamp(0.0, 1.0);
    t * t * (3.0 - 2.0 * t)
}

/// A short sequence of lanes the way runs along, with the lane the car is on at `cur`.
struct LaneSeq {
    lanes: [usize; PLAN_LANES + 2],
    n: usize,
    cur: usize,
}

impl LaneSeq {
    /// Gap from the end of `lanes[i]` to the start of `lanes[i + 1]`, if they are linked
    /// (a gap of a few metres means they are not one road, and nothing is bent).
    fn gap(&self, net: &Network, i: usize) -> Option<DVec3> {
        let (a, b) = (&net.lanes[self.lanes[i]], &net.lanes[self.lanes[i + 1]]);
        let g = b.start() - a.end();
        (g.length() < 3.0).then_some(g)
    }

    /// Point and heading `u` metres into `lanes[i]` (beyond the ends of the first and last
    /// lane: straight on), with the joints on both sides smoothed.
    fn at(&self, net: &Network, i: usize, u: f32) -> (DVec3, f32) {
        let l = &net.lanes[self.lanes[i]];
        let (mut p, h) = l.at_ext(u);
        let w = |x: f32| 1.0 - smooth01(x / JOINT_BLEND);
        if i > 0 {
            if let Some(g) = self.gap(net, i - 1) {
                p -= g * (0.5 * w(u.max(0.0))) as f64;
            }
        }
        if i + 1 < self.n {
            if let Some(g) = self.gap(net, i) {
                p += g * (0.5 * w((l.length() - u).max(0.0))) as f64;
            }
        }
        (p, h)
    }

    /// Lane index in `lanes` and distance along it, `d` metres from distance `s` of the
    /// current lane.
    fn locate(&self, net: &Network, s: f32, d: f32) -> (usize, f32) {
        let mut i = self.cur;
        let mut u = s + d;
        while u < 0.0 && i > 0 {
            i -= 1;
            u += net.lanes[self.lanes[i]].length();
        }
        while i + 1 < self.n && u > net.lanes[self.lanes[i]].length() {
            u -= net.lanes[self.lanes[i]].length();
            i += 1;
        }
        (i, u)
    }

    /// Point and heading `d` metres from distance `s` of the current lane.
    fn point(&self, net: &Network, s: f32, d: f32) -> (DVec3, f32) {
        let (i, u) = self.locate(net, s, d);
        self.at(net, i, u)
    }
}

impl AiState {
    pub fn new(lane: usize, s: f32, seed: u64) -> AiState {
        AiState {
            traffic_pool: None,
            veh_type: 0,
            lane,
            s,
            speed: 0.0,
            max_speed_kmh: 50.0,
            accel: 1.2,
            decel: 3.0,
            length: 5.0,
            rng: seed | 1,
            blinker: 0,
            braking: false,
            odometer: 0.0,
            planned_next: None,
            ahead: Vec::new(),
            change_plan: Vec::new(),
            prev_lane: None,
            yield_time: 0.0,
            route: Vec::new(),
            route_index: 0,
            change: None,
            change_cooldown: 5.0,
            lateral: 0.0,
            lateral_target: 0.0,
            lateral_ramp: (0.0, 0.0, 0.0, 1.0),
            turn_wish: 0,
            signal: 0,
            signal_time: 0.0,
            lat_accel: 2.8,
            desire: 1.0,
            emergency_drive: false,
            headway: 1.4,
            min_gap: 2.0,
            accept_gap: 4.0,
            reaction: 0.7,
            front: 2.5,
            rear: 2.5,
            held: false,
            start_timer: 0.0,
            acc: 0.0,
            accel_cap: None,
            envelope: BehaviorEnvelope::default(),
            brakes: BrakingCapability::fallback(crate::capabilities::VehicleClass::Car),
            realized_speed: 0.0,
            reconciled: false,
            emergency: false,
        }
    }

    fn rand(&mut self) -> u64 {
        let mut x = self.rng;
        x ^= x >> 12;
        x ^= x << 25;
        x ^= x >> 27;
        self.rng = x;
        x.wrapping_mul(0x2545_F491_4F6C_DD1D)
    }

    /// The lanes ahead, nearest first: `planned_next`, then the rest of the plan.
    pub fn upcoming(&self) -> impl Iterator<Item = usize> + '_ {
        self.planned_next
            .into_iter()
            .chain(self.ahead.iter().copied())
    }

    /// A random way on from the end of `lane`. Lanes the map closes to this vehicle ([rule]
    /// no_cars, bus, trucks: `Lane::allows`) and lanes whose traffic density is zero are not driven
    /// into - filtering them only at spawn still let cars turn into a pedestrian street or a
    /// depot yard from next door. A car that has taken a turn lane takes the turn.
    fn choose_after(&mut self, net: &Network, lane: usize) -> Option<usize> {
        let l = &net.lanes[lane];
        // (a car of a traffic pool - the trucks of a map that keeps them to its port roads -
        // takes the ways its pool may go, as it was put on one; where none of them does, the
        // ways open to cars, then any: it does not stand at the junction for ever)
        let open_to = |pooled: bool| -> Vec<usize> {
            l.next
                .iter()
                .copied()
                .filter(|&n| {
                    let nl = &net.lanes[n];
                    let d = match self.traffic_pool.as_ref().filter(|_| pooled) {
                        Some((p, defaults)) => nl.pool_density(defaults, *p),
                        None => nl.density,
                    };
                    nl.allows(self.veh_type) && d > 0.0
                })
                .collect()
        };
        let pooled = self
            .traffic_pool
            .is_some()
            .then(|| open_to(true))
            .filter(|o| !o.is_empty());
        let weighted = pooled.is_some();
        let open = pooled.unwrap_or_else(|| open_to(false));
        let mut choices = if open.is_empty() {
            l.next.clone()
        } else {
            open
        };
        // and a way that goes on rather than into the end of the network, where there is
        // the choice (the map's edge is where OMSI takes its cars away; a village like
        // Grundorf had a queue of twenty growing at the end of its one outbound road)
        if l.kind == LaneKind::Street && net.reach.len() == net.lanes.len() {
            let through: Vec<usize> = choices
                .iter()
                .copied()
                .filter(|&n| net.reach[n] >= DEAD_END)
                .collect();
            if !through.is_empty() {
                choices = through;
            }
        }
        if self.turn_wish != 0 {
            let wished: Vec<usize> = choices
                .iter()
                .copied()
                .filter(|&n| net.lanes[n].turn == self.turn_wish)
                .collect();
            if !wished.is_empty() {
                choices = wished;
                self.turn_wish = 0;
            }
        }
        if choices.is_empty() {
            None
        } else {
            if let Some((pool, defaults)) = self.traffic_pool.clone().filter(|_| weighted) {
                let total: f32 = choices
                    .iter()
                    .map(|&n| net.lanes[n].pool_density(&defaults, pool))
                    .sum();
                let mut pick = (self.rand() >> 32) as f32 / (u32::MAX as f32 + 1.0) * total;
                for &n in &choices {
                    let weight = net.lanes[n].pool_density(&defaults, pool);
                    if pick < weight {
                        return Some(n);
                    }
                    pick -= weight;
                }
                choices.last().copied()
            } else {
                Some(choices[(self.rand() % choices.len() as u64) as usize])
            }
        }
    }

    /// Decide the way on: the lane after the current one and enough lanes after it to
    /// cover `PLAN_AHEAD` metres (a timetable route simply is that plan).
    pub fn plan_next(&mut self, net: &Network) {
        if !self.route.is_empty() {
            // a route step onto a lane beside this one is a lane change, not the way on
            let mut k = self.route_index + 1;
            if self
                .route
                .get(k)
                .map(|&b| net.parallel(self.lane, b))
                .unwrap_or(false)
            {
                k += 1;
            }
            self.planned_next = self.route.get(k).copied();
            self.ahead = self
                .route
                .iter()
                .skip(k + 1)
                .take(PLAN_LANES)
                .copied()
                .collect();
            return;
        }
        if self.planned_next.is_none() {
            self.ahead.clear();
        }
        let planned: f32 = self.upcoming().map(|l| net.lanes[l].length()).sum();
        let rest = net.lanes[self.lane].length() - self.s;
        if self.planned_next.is_some()
            && (rest + planned >= PLAN_AHEAD || self.ahead.len() >= PLAN_LANES)
        {
            return;
        }
        let mut plan: Vec<usize> = self.upcoming().collect();
        self.extend_plan(net, self.lane, rest, &mut plan);
        self.planned_next = plan.first().copied();
        self.ahead = plan.into_iter().skip(1).collect();
    }

    /// Choose lanes after `from` (which has `rest` metres left) until `plan` covers
    /// `PLAN_AHEAD` metres.
    fn extend_plan(&mut self, net: &Network, from: usize, rest: f32, plan: &mut Vec<usize>) {
        let mut dist = rest + plan.iter().map(|&l| net.lanes[l].length()).sum::<f32>();
        while dist < PLAN_AHEAD && plan.len() <= PLAN_LANES {
            let last = plan.last().copied().unwrap_or(from);
            match self.choose_after(net, last) {
                Some(n) => {
                    dist += net.lanes[n].length();
                    plan.push(n);
                }
                None => break,
            }
        }
    }

    /// Put the car on a fixed route starting at its first lane.
    pub fn set_route(&mut self, net: &Network, route: Vec<usize>, s: f32) {
        self.route = route;
        self.route_index = 0;
        if let Some(&l) = self.route.first() {
            self.lane = l;
            self.s = s;
        }
        self.prev_lane = None;
        self.plan_next(net);
    }

    /// Begin a lane change onto the neighbour `to` (`dir` 1 left, 2 right): the indicator
    /// goes on at once, the car moves over after a moment.
    pub fn start_change(&mut self, net: &Network, to: usize, dir: i32) {
        let Some(l) = net.lanes.get(self.lane) else {
            return;
        };
        let Some(t) = net.lanes.get(to) else { return };
        // two to three and a half seconds at the speed the car has now, at least 12 m
        let duration = (3.5 - self.speed * 0.05).clamp(2.0, 3.5);
        let length = (self.speed * duration).max(12.0);
        // (the same share of a lane that starts beside this one, else the point beside the car)
        let s_to = net.beside_s(self.lane, to, self.s.min(l.length()));
        let rest = t.length() - s_to;
        self.change = Some(LaneChange {
            to,
            t: 0.0,
            length,
            s_to,
            dir,
            wait: SIGNAL_BEFORE_CHANGE,
            bypass: false,
        });
        self.blinker = dir;
        let mut plan = Vec::new();
        if self.route.is_empty() {
            self.extend_plan(net, to, rest, &mut plan);
        } else {
            plan.extend(
                self.route
                    .iter()
                    .skip(self.route_index + 2)
                    .take(PLAN_LANES)
                    .copied(),
            );
        }
        self.change_plan = plan;
    }

    /// How far the car still has to drive to distance `ss` of route lane `ri` (a lane it
    /// changes over from counts as the lane beside it).
    pub fn route_distance(&self, net: &Network, ri: usize, ss: f32) -> f32 {
        let (mut k, mut d) = (self.route_index, -self.s);
        if let Some(c) = self.change {
            if self.route.get(k + 1) == Some(&c.to) {
                k += 1;
                d = -c.s_to;
            }
        }
        while k < ri && k + 1 < self.route.len() {
            if !net.parallel(self.route[k], self.route[k + 1]) {
                d += net.lanes[self.route[k]].length();
            } else {
                // a lane beside that starts earlier is further along at the same place
                d -= net.beside_delta(self.route[k], self.route[k + 1]);
            }
            k += 1;
        }
        d + ss
    }

    /// A timetable route that moves over to the lane beside the current one next: that lane
    /// and the side it lies on (1 left, 2 right). The driver starts the move as soon as the
    /// lane is free (`start_route_change`).
    pub fn route_change_due(&self, net: &Network) -> Option<(usize, i32)> {
        if self.route.is_empty() || self.change.is_some() {
            return None;
        }
        let &b = self.route.get(self.route_index + 1)?;
        if !net.parallel(self.lane, b) {
            return None;
        }
        // a lane beside that begins further on: not before the car is level with its start
        let delta = net.beside_delta(self.lane, b);
        if delta < 0.0 && self.s < -delta + 1.0 {
            return None;
        }
        let (la, lb) = (&net.lanes[self.lane], &net.lanes[b]);
        // (compared where the car is: a lane beside may start well before or after this one)
        let (pa, ha) = la.at(self.s.min(la.length()));
        let pb = lb.at(net.beside_s(self.lane, b, self.s)).0;
        let h = (ha as f64).to_radians();
        let side = (pb - pa).truncate().dot(DVec2::new(h.cos(), -h.sin()));
        Some((
            b,
            if side > 0.0 || (side.abs() < 0.5 && lb.offset > la.offset) {
                2
            } else {
                1
            },
        ))
    }

    /// Pull out into `to` from behind a standing obstacle: a short, steep move.
    pub fn start_bypass(&mut self, net: &Network, to: usize, dir: i32) {
        self.start_change(net, to, dir);
        if let Some(c) = self.change.as_mut() {
            c.length = crate::maneuvers::BYPASS_RAMP;
            c.bypass = true;
        }
    }

    /// Start the lane change a route asks for: at once (the indicator has been on while the
    /// driver waited for a gap) and quick enough to be done well before a fork's branches part.
    pub fn start_route_change(&mut self, net: &Network, to: usize, dir: i32) {
        self.start_change(net, to, dir);
        let left = (net.lanes[self.lane].length() - self.s).max(0.0);
        if let Some(c) = self.change.as_mut() {
            c.wait = 0.0;
            c.length = c.length.min(left * 0.7).max(6.0);
        }
    }

    /// The lane sequence the car is driving along: where it came from, where it is, the plan.
    fn seq(&self) -> LaneSeq {
        let mut q = LaneSeq {
            lanes: [0; PLAN_LANES + 2],
            n: 0,
            cur: 0,
        };
        if let Some(p) = self.prev_lane {
            q.lanes[0] = p;
            q.n = 1;
        }
        q.cur = q.n;
        q.lanes[q.n] = self.lane;
        q.n += 1;
        for l in self.upcoming() {
            if q.n == q.lanes.len() {
                break;
            }
            q.lanes[q.n] = l;
            q.n += 1;
        }
        q
    }

    /// The lane sequence of the lane the car is changing to, with the way on it chose.
    fn change_seq(&self, to: usize) -> LaneSeq {
        let mut q = LaneSeq {
            lanes: [0; PLAN_LANES + 2],
            n: 1,
            cur: 0,
        };
        q.lanes[0] = to;
        for &l in &self.change_plan {
            if q.n == q.lanes.len() {
                break;
            }
            q.lanes[q.n] = l;
            q.n += 1;
        }
        q
    }

    /// The point of the car's way `d` metres ahead of it (behind for negative `d`): along
    /// its lanes with the joints smoothed, over to the new lane during a lane change, and
    /// out to the side where it pulls into a bay or round a parked car. The steering
    /// follows this curve; it has no steps, so neither does the car.
    pub fn way_point(&self, net: &Network, d: f32) -> DVec3 {
        let (mut p, mut h) = self.seq().point(net, self.s, d);
        if let Some(c) = self.change {
            if c.to < net.lanes.len() {
                let (pt, ht) = self.change_seq(c.to).point(net, c.s_to, d);
                // how far the change will have got by the time the car is `d` metres on
                let tau = (d - c.wait * self.speed.max(2.0)) / c.length;
                let k = smooth01(c.t + tau);
                p = p.lerp(pt, k as f64);
                h += wrap_deg(ht - h) * k;
            }
        }
        let lat = self.lateral_at(self.odometer + d);
        if lat.abs() > 1e-3 {
            let hr = (h as f64).to_radians();
            p += DVec3::new(hr.cos(), -hr.sin(), 0.0) * lat as f64;
        }
        p
    }

    /// How fast the car may be going now so that it can take every bend of the next stretch
    /// of its way at `lat_accel`, braking gently (2 m/s²) into the tight ones. Without this
    /// the cars swept round a junction at 40 km/h - well over a g. The bend is the lane's
    /// own curvature or the turn of the way over 6 m, whichever is sharper: lanes are linked
    /// with up to 40° between them, and such a kink is a bend too.
    pub fn curve_speed(&self, net: &Network) -> f32 {
        let mut best = self.curve_speed_on(net, &self.seq(), self.s);
        if let Some(c) = self.change {
            if c.to < net.lanes.len() {
                best = best.min(self.curve_speed_on(net, &self.change_seq(c.to), c.s_to));
            }
        }
        best
    }

    fn curve_speed_on(&self, net: &Network, q: &LaneSeq, s: f32) -> f32 {
        let reach = (self.speed * self.speed / 4.0 + 12.0).min(90.0);
        let mut best = f32::MAX;
        // The samples lie at fixed places of the road (every 2.5 m of the odometer), not at
        // fixed distances ahead of the car: moving with the car, the sample that caught a
        // bend jumped 2.5 m nearer every 2.5 m, the allowed speed fell in steps of 4 m²/s²
        // and the car braked hard, let go and braked hard again with nothing in front of it.
        let first = 2.5 - self.odometer.rem_euclid(2.5);
        let mut d = 0.0f32;
        let mut next = first;
        while d <= reach {
            let (i, u) = q.locate(net, s, d);
            let turn = wrap_deg(q.point(net, s, d + 3.0).1 - q.point(net, s, d - 3.0).1)
                .abs()
                .to_radians()
                / 6.0;
            let k = net.lanes[q.lanes[i]].curvature_at(u).abs().max(turn);
            if k > 1e-4 {
                let v = (self.lat_accel / k).sqrt().max(2.5);
                // (a bend may begin anywhere up to a sample's spacing before the sample
                // that finds it: the speed is taken from there, or the car met the start of
                // a tight turn half a metre after its profile had allowed 1 m/s more)
                best = best.min((v * v + 2.0 * 2.0 * (d - 2.5).max(0.0)).sqrt());
            }
            d = next;
            next += 2.5;
        }
        best
    }

    /// The sideways offset of the way `d` metres ahead of the car (m, + = right).
    pub fn lateral_ahead(&self, d: f32) -> f32 {
        self.lateral_at(self.odometer + d)
    }

    /// The sideways offset of the way when the odometer reads `x`.
    fn lateral_at(&self, x: f32) -> f32 {
        let (from, to, x0, len) = self.lateral_ramp;
        from + (to - from) * smooth01((x - x0) / len.max(0.1))
    }

    /// The first turn (1 left, 2 right) on the way within `within` metres, with the
    /// distance to the start of the turning lane (0 while on it).
    pub fn turn_ahead(&self, net: &Network, within: f32) -> Option<(i32, f32)> {
        let here = &net.lanes[self.lane];
        if here.turn != 0 {
            return Some((here.turn, 0.0));
        }
        let mut dist = here.length() - self.s;
        for l in self.upcoming() {
            if dist > within {
                break;
            }
            let lane = &net.lanes[l];
            if lane.turn != 0 {
                return Some((lane.turn, dist));
            }
            dist += lane.length();
        }
        None
    }

    /// Set the indicator for what the car is doing or about to do: a lane change, its own
    /// manoeuvre (pulling away from a stop), moving sideways into a bay or out round a
    /// parked car and back, and a turn at the junction ahead - from about four seconds
    /// before it (at least 25 m, at most 60 m) until the turn is done.
    pub fn update_blinker(&mut self, net: &Network) {
        self.blinker = if let Some(c) = self.change {
            c.dir
        } else if self.signal != 0 && self.signal_time > 0.0 {
            self.signal
        } else if (self.lateral_target - self.lateral).abs() > 0.3 {
            if self.lateral_target > self.lateral {
                2
            } else {
                1
            }
        } else {
            let within = (self.speed * 4.0).clamp(25.0, 60.0);
            self.turn_ahead(net, within).map(|t| t.0).unwrap_or(0)
        };
    }

    /// Advance along the network. `obstacle` = distance from the car's origin to the rear of
    /// a standing vehicle ahead (m), `stop_at` = distance from its origin to a stop line
    /// (m). False at a dead end (the car is taken off the road).
    pub fn advance(
        &mut self,
        net: &Network,
        dt: f32,
        obstacle: Option<f32>,
        stop_at: Option<f32>,
    ) -> bool {
        let lead = obstacle.map(|d| Lead {
            gap: d - self.front,
            speed: 0.0,
            acc: 0.0,
        });
        self.drive(net, dt, lead, stop_at)
    }

    /// The acceleration the driver wants (the Intelligent Driver Model): towards the
    /// desired speed on a free road, and a braking term for what is ahead that is gentle
    /// (`decel`) while there is room and only as hard as it must be when there is not.
    /// `lead` is the vehicle ahead, `stop` the distance from the car's origin to where it
    /// has to stop (a light, a junction it gives way at, a bus stop).
    pub fn desired_accel(&self, net: &Network, lead: Option<Lead>, stop: Option<f32>) -> f32 {
        self.desired_demand(net, lead, stop).effective()
    }

    /// The separated longitudinal request: the comfortable command plus a collision-
    /// prevention value, and the cause that most strongly binds it.
    pub fn desired_demand(&self, net: &Network, lead: Option<Lead>, stop: Option<f32>) -> LongitudinalDemand {
        let Some(lane) = net.lanes.get(self.lane) else {
            return LongitudinalDemand {
                comfort: 0.0,
                emergency: None,
                reason: Reason::NONE,
            };
        };
        let (a, b) = (self.accel.max(0.1), self.decel.max(0.5));
        let v = self.speed;
        let limit = |l: &Lane| {
            (if self.emergency_drive {
                (l.speed_limit_kmh * 1.3).min(l.speed_limit_kmh + 20.0)
            } else {
                l.speed_limit_kmh * self.desire
            })
                .min(self.max_speed_kmh)
                .max(3.0)
                / 3.6
        };
        let mut v0 = limit(lane);
        // a lower limit on the lanes ahead (a junction's turning lanes, a 30 zone) is
        // reached at that speed, slowing gently (1.2 m/s²) from where it has to: taken only
        // on entering the lane, the new limit threw the model into a -3 m/s² stop at every
        // junction, with nothing in front of the car
        {
            let reach = (v * v / 2.4 + 10.0).min(120.0);
            let mut d = lane.length() - self.s;
            for l in self.upcoming() {
                if d > reach {
                    break;
                }
                if let Some(nl) = net.lanes.get(l) {
                    let vl = limit(nl);
                    if vl < v0 {
                        v0 = v0.min((vl * vl + 2.0 * 1.2 * d.max(0.0)).sqrt());
                    }
                    d += nl.length();
                }
            }
        }
        let mut acc = a * (1.0 - (v / v0).powi(4)).max(-1.5 * b / a);
        let mut reason = Reason::SpeedLimit;
        // bends: follow the speed profile `curve_speed` lays out (it assumes 2 m/s² of
        // braking), blending in over the last metre per second above it
        let bend = self.curve_speed(net);
        if bend < v0 && v > bend - 1.0 {
            let track = -2.0 + (bend - v) / 0.6;
            let k = ((v - (bend - 1.0)) / 1.0).clamp(0.0, 1.0);
            acc = acc.min(acc + (track - acc) * k);
            reason = Reason::Curvature;
        }
        let interaction = |gap: f32, lead_speed: f32, s0: f32, headway: f32| -> f32 {
            let s_star =
                s0 + (v * headway + v * (v - lead_speed) / (2.0 * (a * b).sqrt())).max(0.0);
            -a * (s_star / gap.max(0.05)).powi(2)
        };
        let mut out = acc;
        if let Some(l) = lead {
            let idm = acc + interaction(l.gap, l.speed.max(0.0), self.min_gap, self.headway);
            // The constant-acceleration heuristic (the "ACC" variant of the model): how hard
            // the driver has to brake if the car ahead keeps doing what it does. A car that
            // has just cut in close ahead but drives almost as fast calls for a firm brake
            // and a gap that opens again over the next seconds, not an emergency stop.
            let (vl, s, al) = (l.speed.max(0.0), l.gap.max(0.05), l.acc.min(a));
            let den = vl * vl - 2.0 * s * al;
            let cah = if vl * (v - vl) <= -2.0 * s * al && den > 1e-3 {
                v * v * al / den
            } else {
                al - (v - vl).max(0.0).powi(2) / (2.0 * s)
            };
            // (a car beside or just ahead that drives away faster gives the model a gap of
            // nothing: its braking term is bounded so that the heuristic decides)
            let idm = idm.max(-2.0 * MAX_BRAKE);
            let acc_lead = if idm >= cah {
                idm
            } else {
                0.01 * idm + 0.99 * (cah + b * ((idm - cah) / b).tanh())
            };
            if acc_lead < out {
                reason = Reason::Leader;
            }
            out = out.min(acc_lead);
        }
        if let Some(d) = stop {
            // A stop line: far away the model's braking term (without a time gap, the line
            // does not move off), gently; once the constant deceleration that stops the car
            // at the line reaches half the driver's comfortable braking, that deceleration.
            // The model alone braked twice as hard as needed when a light turned yellow 25 m
            // ahead, and then eased off.
            let room = (d - self.front - STOP_LINE_GAP).max(0.02);
            let need = v * v / (2.0 * room);
            let idm = (acc + interaction(d - self.front, 0.0, STOP_LINE_GAP, 0.2)).max(-0.5 * b);
            let constant = -need * 1.05;
            let k = ((need - 0.35 * b) / (0.15 * b)).clamp(0.0, 1.0);
            let a_stop = idm + (constant - idm) * k;
            if a_stop < out {
                reason = Reason::StopTarget;
            }
            out = out.min(a_stop);
        }
        // Meet a lower limit or speed profile rather than approaching it asymptotically: the
        // free term alone leaves the car a few m/s over a new limit at the joint. A speed
        // proportional correction is enough to shed the difference without a hard clamp.
        if v > v0 {
            let need = -((v - v0) * 2.0).min(MAX_BRAKE);
            if need < out {
                reason = Reason::SpeedLimit;
            }
            out = out.min(need);
        }
        // Separate the collision-prevention channel from the comfort envelope: the comfort
        // command never brakes harder than the vehicle's ordinary maximum, and anything
        // beyond that is an explicit emergency.
        let ceiling = self.envelope.emergency_decel.max(a);
        let out = out.clamp(-ceiling, a);
        let floor = -self.brakes.max_decel.max(0.5);
        if out < floor - 0.05 {
            LongitudinalDemand {
                comfort: floor,
                emergency: Some(out),
                reason,
            }
        } else {
            LongitudinalDemand {
                comfort: out,
                emergency: None,
                reason,
            }
        }
    }

    /// Advance along the network with the car ahead (`lead`) and a stop point (`stop`,
    /// distance from the car's origin). False at a dead end.
    pub fn drive(&mut self, net: &Network, dt: f32, lead: Option<Lead>, stop: Option<f32>) -> bool {
        self.change_cooldown = (self.change_cooldown - dt).max(0.0);
        self.signal_time = (self.signal_time - dt).max(0.0);
        if self.signal_time <= 0.0 {
            self.signal = 0;
        }
        if net.lanes.get(self.lane).is_none() {
            return false;
        }
        let demand = self.desired_demand(net, lead, stop);
        self.emergency = demand.is_emergency();
        let mut acc = demand.effective();
        if let Some(cap) = self.accel_cap {
            acc = acc.min(cap);
        }
        // standing: a driver who is held there moves off only after a moment when the way
        // clears (the wave that runs down a queue at a green light). Only *entering* the
        // hold sets the timer: a re-hold after a brief flicker of a constraint keeps the
        // count, so a junction that is free and not free by turns cannot reset the launch.
        if self.speed < 0.05 {
            if acc <= 0.05 {
                if !self.held {
                    self.start_timer = self.reaction;
                    self.held = true;
                }
                acc = acc.min(0.0);
            } else if self.held {
                self.start_timer -= dt;
                if self.start_timer > 0.0 {
                    acc = 0.0;
                } else {
                    self.held = false;
                }
            }
        } else if self.speed > 0.5 {
            self.held = false;
        }
        // Comfort jerk: bound how fast the comfortable command may change. Emergency
        // collision prevention is not held back by the comfort envelope.
        if !self.emergency && dt > 0.0 {
            let jerk = self.envelope.max_jerk.max(0.1) * dt;
            acc = acc.clamp(self.acc - jerk, self.acc + jerk);
        }
        let v0 = self.speed;
        let v1 = (v0 + acc * dt).max(0.0);
        self.speed = v1;
        self.acc = if dt > 0.0 { (v1 - v0) / dt } else { 0.0 };
        // brake lights: braking, or holding the car on the brake
        self.braking = acc < -0.6 || (v1 < 0.3 && self.held);
        let ds = (v0 + v1) * 0.5 * dt;
        self.s += ds;
        self.odometer += ds;
        // lane change: signal, glide over to the neighbour, then continue there
        if let Some(mut c) = self.change {
            // Neighbouring curved lanes have different arc lengths. Keep their
            // longitudinal fractions aligned, just as realization feedback does.
            let from = &net.lanes[self.lane];
            let target = &net.lanes[c.to];
            let scale = if (target.start() - from.start()).truncate().length() < 8.0 {
                target.length() / from.length().max(0.01)
            } else { 1.0 };
            c.s_to += ds * scale;
            if c.wait > 0.0 {
                c.wait = (c.wait - dt).max(0.0);
            } else {
                c.t += ds / c.length;
            }
            // Random traffic can change across an ordinary spline joint. Advance both
            // longitudinal origins while retaining the blend's progress and indicator.
            // Route changes keep their explicit route-entry completion semantics.
            if c.t < 1.0 && self.route.is_empty() {
                for _ in 0..PLAN_LANES {
                    let from_len = net.lanes[self.lane].length();
                    let to_len = net.lanes[c.to].length();
                    if self.s < from_len && c.s_to < to_len { break; }
                    let Some((from, to)) = net.parallel_continuation(self.lane, c.to) else { break };
                    if self.planned_next != Some(from) || self.change_plan.first() != Some(&to) { break; }
                    // Matched road pieces normally end together. Keep the origins paired
                    // even if their lengths differ slightly at a curved spline joint.
                    if self.s < from_len || c.s_to < to_len { break; }
                    self.s -= from_len;
                    c.s_to -= to_len;
                    self.prev_lane = Some(self.lane);
                    self.lane = from;
                    c.to = to;
                    self.planned_next = if self.ahead.is_empty() { None } else { Some(self.ahead.remove(0)) };
                    self.change_plan.remove(0);
                    self.plan_next(net);
                }
            }
            let from_len = net.lanes[self.lane].length();
            let to_len = net.lanes.get(c.to).map(|l| l.length()).unwrap_or(0.0);
            if c.t >= 1.0 || self.s >= from_len || c.s_to >= to_len {
                // arrived on the new lane: it has no joint behind the car, and the way on
                // is planned afresh from there
                if self.route.get(self.route_index + 1) == Some(&c.to) {
                    self.route_index += 1;
                }
                self.lane = c.to;
                self.s = c.s_to.min(to_len);
                self.change = None;
                self.change_cooldown = 6.0;
                self.prev_lane = None;
                let plan = std::mem::take(&mut self.change_plan);
                self.planned_next = plan.first().copied();
                self.ahead = plan.into_iter().skip(1).collect();
            } else {
                self.change = Some(c);
            }
        }
        // bay offset: a new target starts an S-curve from where the car is now, eight metres
        // long for every metre sideways (at least 8, at most 30)
        if (self.lateral_target - self.lateral_ramp.1).abs() > 1e-3 {
            let from = self.lateral_at(self.odometer - ds);
            let len = ((self.lateral_target - from).abs() * 8.0).clamp(8.0, 30.0);
            self.lateral_ramp = (from, self.lateral_target, self.odometer - ds, len);
        }
        self.lateral = self.lateral_at(self.odometer);
        if self.planned_next.is_none() {
            self.plan_next(net);
        }
        while self.change.is_none() && self.s >= net.lanes[self.lane].length() {
            let l = &net.lanes[self.lane];
            let fallback = if self.planned_next.is_none() && l.kind != LaneKind::Air {
                let seed = self.s.to_bits() as usize ^ self.lane;
                let nexts: Vec<usize> = l
                    .next
                    .iter()
                    .copied()
                    .filter(|&n| net.lanes.get(n).map(|x| x.kind == l.kind).unwrap_or(false))
                    .collect();
                if nexts.is_empty() {
                    None
                } else {
                    Some(nexts[seed % nexts.len()])
                }
            } else {
                None
            };
            let Some(next) = self.planned_next.or(fallback) else {
                // the end of a flight path: the aircraft flies on straight (the way runs on
                // along the end tangent) until the traffic takes it away out of sight
                if l.kind == LaneKind::Air {
                    break;
                }
                return false; // dead end: despawn
            };
            self.s -= l.length();
            self.prev_lane = Some(self.lane);
            self.lane = next;
            if self.route.is_empty() {
                self.planned_next = if self.ahead.is_empty() {
                    None
                } else {
                    Some(self.ahead.remove(0))
                };
            } else {
                // the plan may have stepped over a lane-change entry of the route
                let from = self.route_index + 1;
                self.route_index = (from..(from + 3).min(self.route.len()))
                    .find(|&k| self.route[k] == next)
                    .unwrap_or(from);
            }
            self.plan_next(net);
        }
        // keep the plan PLAN_AHEAD metres long as the car eats into it (a route's plan
        // only moves when the car enters its next lane)
        if self.route.is_empty() {
            self.plan_next(net);
        }
        true
    }

    /// Commit route progress from the realized body.
    ///
    /// The realized body is the single pose owner: this projects its pose onto the planned
    /// route with [`project_on_route_indices`] and adopts the projected lane, distance and
    /// realized speed. A projection that is off to the side, faces another way, or lands on
    /// a lane the planner has already left is rejected, leaving this tick's planner progress
    /// untouched, so a transient mismatch can never teleport the vehicle. The route is the
    /// timetable route from the lane just behind the current one, or (for random traffic) the
    /// current lane plus the planned lanes.
    pub fn commit_feedback(&mut self, net: &Network, realized: RealizedMotion) -> Option<RouteFix> {
        if net.lanes.get(self.lane).is_none() {
            return None;
        }
        // A scheduled route includes the adjacent target lane. Selecting the nearest
        // route lane halfway through a change switches the source underneath the blend,
        // making its remaining trajectory jump sideways. Keep the source until drive
        // completes the change, and reconcile both longitudinal coordinates together.
        if let Some(mut change) = self.change {
            let target = net.lanes.get(change.to)?;
            let (s, _) = net.lanes[self.lane].nearest_point(realized.pose)?;
            let (p, heading) = net.lanes[self.lane].at(s);
            let h = (heading as f64).to_radians();
            let right = glam::DVec2::new(h.cos(), -h.sin());
            let lateral = (realized.pose - p).truncate().dot(right) as f32;
            let target_s = net.beside_s(self.lane, change.to, s);
            let width = (target.at(target_s).0 - p).truncate().dot(right).abs();
            let turn = wrap_deg(heading - realized.heading_deg).abs();
            if !realized.pose.is_finite()
                || lateral.abs() as f64 > width + realized.half_width + FEEDBACK_MAX_LATERAL
                || turn > FEEDBACK_MAX_TURN
                || (realized.pose.z - p.z).abs() > 2.0
            {
                self.reconciled = false;
                return None;
            }
            self.s = s;
            change.s_to = target_s;
            self.change = Some(change);
            if realized.speed.is_finite() && realized.speed >= 0.0 {
                self.speed = realized.speed;
                self.realized_speed = realized.speed;
            }
            self.reconciled = true;
            return Some(RouteFix { lane: crate::LaneId(self.lane), s, lateral });
        }
        let mut buf = [0usize; PLAN_LANES + 1];
        let projected = if !self.route.is_empty() {
            let start = self.route_index.saturating_sub(1);
            project_on_route_indices(
                net,
                &self.route[start..],
                realized.pose,
                realized.heading_deg,
                realized.half_width,
                FEEDBACK_MAX_LATERAL,
                FEEDBACK_MAX_TURN,
            )
            .map(|f| (f, start))
        } else {
            buf[0] = self.lane;
            let mut n = 1;
            for l in self.upcoming() {
                if n == buf.len() {
                    break;
                }
                buf[n] = l;
                n += 1;
            }
            project_on_route_indices(
                net,
                &buf[..n],
                realized.pose,
                realized.heading_deg,
                realized.half_width,
                FEEDBACK_MAX_LATERAL,
                FEEDBACK_MAX_TURN,
            )
            .map(|f| (f, 0))
        };
        let Some((fix, offset)) = projected else {
            self.reconciled = false;
            return None;
        };
        if self.route.is_empty() && fix.lane.index() != self.lane {
            // The body is near a joint the planner has not passed yet: let `drive` make the
            // transition rather than adopting a distance on a lane we are not yet on.
            self.reconciled = false;
            return None;
        }
        if !self.route.is_empty() {
            let ri = offset
                + self
                    .route
                    .iter()
                    .skip(offset)
                    .position(|&l| l == fix.lane.index())?;
            if ri + 1 < self.route_index
                || (ri < self.route_index && net.parallel(fix.lane.index(), self.lane))
            {
                // the body projects onto a lane the planner has already passed
                self.reconciled = false;
                return None;
            }
            if ri != self.route_index {
                self.route_index = ri;
                self.plan_next(net);
            }
        }
        self.lane = fix.lane.index();
        self.s = fix.s;
        self.lateral = fix.lateral;
        if realized.speed.is_finite() && realized.speed >= 0.0 {
            self.speed = realized.speed;
            self.realized_speed = realized.speed;
        }
        self.reconciled = true;
        Some(fix)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::network::{LaneBuilder, LaneKind};
    use glam::DVec3;

    fn straight() -> Network {
        let lane = LaneBuilder::polyline(
            vec![DVec3::new(0.0, 0.0, 0.0), DVec3::new(0.0, 200.0, 0.0)],
            LaneKind::Street,
            3.0,
        );
        let mut net = Network {
            lanes: vec![lane],
            ..Default::default()
        };
        net.link(1.5);
        net
    }

    fn car(net: &Network, s: f32, speed: f32) -> AiState {
        let mut c = AiState::new(0, s, 5);
        c.front = 2.25;
        c.rear = 2.25;
        c.length = 4.5;
        c.speed = speed;
        c.plan_next(net);
        c
    }

    #[test]
    fn a_comfortable_demand_stays_inside_the_envelope() {
        let net = straight();
        let c = car(&net, 0.0, 10.0);
        let d = c.desired_demand(&net, None, None);
        assert!(!d.is_emergency(), "a free road is not an emergency");
        assert_eq!(d.effective(), d.comfort);
        assert!(d.comfort <= c.envelope.comfort_accel + 1e-3);
    }

    #[test]
    fn a_hard_cut_in_separates_the_emergency_channel() {
        let net = straight();
        let c = car(&net, 0.0, 12.0);
        let lead = Lead {
            gap: 4.0,
            speed: 0.0,
            acc: 0.0,
        };
        let d = c.desired_demand(&net, Some(lead), None);
        assert!(d.is_emergency(), "a cut-in at 4 m must engage collision prevention");
        assert_eq!(d.effective(), d.emergency.unwrap());
        assert!(d.comfort >= -c.brakes.max_decel - 1e-3);
    }

    #[test]
    fn feedback_commits_the_realized_pose_on_the_route() {
        let net = straight();
        let mut c = car(&net, 0.0, 8.0);
        let fix = c
            .commit_feedback(
                &net,
                RealizedMotion {
                    pose: DVec3::new(0.0, 50.0, 0.0),
                    heading_deg: 0.0,
                    speed: 7.5,
                    half_width: 1.25,
                },
            )
            .expect("a body on the route must be accepted");
        assert_eq!(fix.lane.index(), 0);
        assert!((c.s - 50.0).abs() < 0.5);
        assert!((c.speed - 7.5).abs() < 1e-3);
        assert!(c.reconciled);
    }

    #[test]
    fn feedback_rejects_a_body_on_a_parallel_road() {
        let net = straight();
        let mut c = car(&net, 0.0, 8.0);
        let before = c.s;
        let fix = c.commit_feedback(
            &net,
            RealizedMotion {
                pose: DVec3::new(4.0, 50.0, 0.0),
                heading_deg: 0.0,
                speed: 8.0,
                half_width: 1.25,
            },
        );
        assert!(fix.is_none(), "a parallel body must not be snapped onto the route");
        assert_eq!(c.s, before, "a rejected projection must not move the planner");
        assert!(!c.reconciled);
    }

    #[test]
    fn a_lower_limit_is_met_rather_than_approached_asymptotically() {
        let lane0 = LaneBuilder::polyline(
            vec![DVec3::new(0.0, 0.0, 0.0), DVec3::new(0.0, 200.0, 0.0)],
            LaneKind::Street,
            3.0,
        );
        let mut lane1 = LaneBuilder::polyline(
            vec![DVec3::new(0.0, 200.0, 0.0), DVec3::new(0.0, 340.0, 0.0)],
            LaneKind::Street,
            3.0,
        );
        lane1.speed_limit_kmh = 30.0;
        let mut net = Network {
            lanes: vec![lane0, lane1],
            ..Default::default()
        };
        net.link(1.5);
        let mut c = car(&net, 0.0, 14.0);
        for _ in 0..1600 {
            c.drive(&net, DT_TEST, None, None);
        }
        // The car ends on the lower-limit lane at roughly that limit, not far above it.
        assert_eq!(c.lane, 1);
        assert!(
            c.speed <= 30.0 / 3.6 + 1.0,
            "entered the 30 zone at {:.2} m/s",
            c.speed
        );
    }

    const DT_TEST: f32 = 1.0 / 50.0;
}
