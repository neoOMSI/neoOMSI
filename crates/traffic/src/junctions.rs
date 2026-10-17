//! Junction coordinator: admission, commitments, fairness, release, and the wait-for graph.
//!
//! Planning reads a frozen [`JunctionScene`] and returns a [`JunctionDecision`]; it never
//! mutates another vehicle. Commitments (claimed lanes, held exit storage) are keyed by
//! [`VehicleId`] and released only on tail clearance, route change, removal or network
//! invalidation, never by a timeout while a body still occupies a conflict area.
//!
//! Content rules:
//! - `[crossingproblem]` marks a keep-clear path: not entered (nor reserved) unless the whole
//!   movement can be cleared.
//! - `[blockpath] <path> <mode>` is a [`BlockMode`]; OMSI only proves the values are stored,
//!   the meaning is a neoOMSI interpretation:
//!   - [`BlockMode::Occupy`] (0): conflict only while a body occupies one of the paths.
//!   - [`BlockMode::Reserve`] (1): a reservation alone refuses the other's claim.
//!   - [`BlockMode::Oncoming`] (2): may reserve, but not enter until the other has committed.
//!
//! The decision-zone constants are provisional neoOMSI tuning.

use crate::diagnostics::{JunctionState, Reason};
use crate::following::{AiState, Lead, MAX_BRAKE};
use crate::ids::{LaneId, VehicleId};
use crate::network::{wrap_deg, BlockRule, Network};
use crate::signals::Aspect;
mod emergency;
use crate::world::Arbiter;
use hashbrown::{HashMap, HashSet};

/// Seconds a vehicle does not reconsider its claim once it is this close to the line.
const DECIDE_MIN: f32 = 20.0;
const DECIDE_MAX: f32 = 70.0;
/// A decision zone this much further out releases an old claim made inside it.
const DECIDE_RELEASE: f32 = 20.0;
/// A meeting place further than this into the junction's way is not weighed at the line.
const INSIDE_MEETING: f32 = 25.0;
/// A vehicle whose front is within this of the line counts as being at it.
const AT_LINE: f32 = 3.0;
/// A vehicle that reached the line on green or yellow still counts as committed to the
/// junction at red only while it moves at least this fast (m/s) or its front is over the line.
const AMBER_COMMIT_SPEED: f32 = 3.0;
/// Traffic ahead slower than this (m/s) is a queue a vehicle does not follow into a junction.
const QUEUE_SPEED: f32 = 1.5;
/// A car entering a roundabout gives way to a ring car standing closer than this (m, its
/// front from the place they meet) as to a moving one.
const RING_QUEUE_KEEP: f32 = 10.0;
/// How far into the meeting place its front must be for a vehicle to count as in it
/// already, not just at its line (m).
const NOSED_IN: f32 = 1.5;
/// Half the width of a footpath crossing a lane (m), for where a vehicle is past it.
const WALK_HALF_WIDTH: f32 = 2.0;
/// Seconds a cycle of standing vehicles that only give way to each other by the rules
/// persists before the one waiting longest is let go (nobody else would end it).
const GRIDLOCK_AFTER: f32 = 8.0;
/// A vehicle standing this close (m) behind a standing one waits for it to move: the link
/// of a queue in the wait-for graph.
const QUEUE_GAP: f32 = 8.0;
/// Seconds a vehicle waits at the line before it makes itself seen: it keeps a claim on its
/// way through while still waiting (see `admit`).
const LONG_WAIT_CLAIM: f32 = 45.0;
/// Seconds that permission lasts: enough to set off and get into the junction.
const GRIDLOCK_PASS: f32 = 12.0;
/// A turn across the oncoming traffic at a signal pulls into the junction to wait at the
/// meeting place when that lies at least this far (m) past the line.
const PULL_IN_MIN: f32 = 3.0;

/// Where a vehicle meets a crossing lane on its way: its lane in the sequence and the distance
/// from the vehicle origin to that lane's start.
#[derive(Debug, Clone)]
pub struct Movement {
    /// `(lane, distance from the vehicle origin to its start)` of the junction's lanes on the
    /// way; the first is where it has to wait.
    pub lanes: Vec<(usize, f32)>,
    /// The lane after the junction and the distance to its start.
    pub exit: Option<(usize, f32)>,
    /// The vehicle is already on one of the junction's lanes.
    pub inside: bool,
    /// Any of the movement's lanes is a `[crossingproblem]` keep-clear path.
    pub keep_clear: bool,
}

/// The junction on a way within the planned lanes: its lanes that cross or meet others (or a
/// footpath), and the lane after it.
pub fn junction_ahead(net: &Network, way: &[(usize, f32)]) -> Option<Movement> {
    let has = |l: usize| !net.crossings[l].is_empty() || !net.walks[l].is_empty();
    let object = |l: usize| {
        net.lanes[l]
            .key
            .filter(|_| net.lanes[l].source == 2)
            .map(|k| (k.tile, k.id))
    };
    let mut j: Option<Movement> = None;
    for (k, &(l, d)) in way.iter().enumerate() {
        match j.as_mut() {
            None => {
                if has(l) {
                    j = Some(Movement {
                        lanes: vec![(l, d)],
                        exit: None,
                        inside: k == 0,
                        keep_clear: net.lanes[l].crossing_problem,
                    });
                }
            }
            Some(jn) => {
                if object(l).is_some() && object(l) == object(jn.lanes[0].0) {
                    jn.lanes.push((l, d));
                    jn.keep_clear |= net.lanes[l].crossing_problem;
                } else {
                    jn.exit = Some((l, d));
                    break;
                }
            }
        }
    }
    j
}

/// Must `a`, on lane `l`, let the vehicle on lane `m` go first? `Network::must_yield`, but
/// a timetable bus coming back out of its bay keeps its way (`Network::returns_beside`).
fn gives_way(net: &Network, a: &JunctionActor, l: usize, m: usize) -> bool {
    net.must_yield(l, m) && !(a.bus && net.returns_beside(l, m))
}

/// How far (m) a vehicle needs to stop braking firmly, as a driver who has to give way
/// at a roundabout entry does when the ring is not clear after all.
fn firm_stop(a: &JunctionActor) -> f32 {
    a.speed * a.speed / (2.0 * MAX_BRAKE * 0.6) + a.speed * 0.3 + 1.0
}

/// The crossing object a lane belongs to (`None` for a spline lane).
fn light_object(net: &Network, lane: usize) -> Option<((i32, i32), i64)> {
    let l = &net.lanes[lane];
    l.key.filter(|_| l.source == 2).map(|k| (k.tile, k.id))
}

/// How far along the way (from the vehicle origin) the last place lies where `jn`'s lanes
/// meet other traffic or a footpath: beyond it the vehicle is out of everybody's way, however
/// long the junction object's paths run on.
fn movement_clear_at(net: &Network, jn: &Movement) -> f32 {
    jn.lanes
        .iter()
        .flat_map(|&(l, dl)| {
            net.crossings[l]
                .iter()
                .map(move |c| dl + c.at + c.after)
                .chain(net.walks[l].iter().map(move |w| dl + w.1 + WALK_HALF_WIDTH))
        })
        .fold(jn.lanes[0].1, f32::max)
}

/// The light that holds a vehicle at the start of `way[k]`, unless it already went through
/// that same light in this crossing object (paths after a stop line carry it on). Other
/// lights of the object still hold it (BRT Berlin cars otherwise ran the second red).
pub fn light_at_entry(net: &Network, way: &[(usize, f32)], k: usize) -> Option<(usize, usize)> {
    let l = way[k].0;
    let light = net.lanes[l].traffic_light?;
    let object = |x: usize| {
        let lane = &net.lanes[x];
        lane.key
            .filter(|_| lane.source == 2)
            .map(|key| (key.tile, key.id))
    };
    let here = object(l)?;
    for &(p, _) in way[..k].iter().rev() {
        if object(p) != Some(here) {
            break;
        }
        if net.lanes[p].traffic_light == Some(light) {
            return None;
        }
    }
    Some(light)
}

/// Seconds until a vehicle `dist` metres from a point gets its front there, from speed `v`
/// with acceleration `a`.
pub fn time_to(dist: f32, v: f32, a: f32) -> f32 {
    if dist <= 0.0 {
        return 0.0;
    }
    let a = a.max(0.3);
    (-v + (v * v + 2.0 * a * dist).sqrt()) / a
}

/// Seconds until a vehicle reaches a meeting place `distance` metres ahead.
pub fn crossing_arrival(
    st: &AiState,
    distance: f32,
    claimed: bool,
    waits_short: bool,
    stalled: bool,
) -> f32 {
    if distance <= 0.3 {
        return 0.0;
    }
    if claimed {
        return time_to(distance, st.speed, st.accel)
            + if st.speed < 0.1 { st.reaction } else { 0.0 };
    }
    if waits_short {
        return f32::MAX;
    }
    if stalled {
        return if st.speed > 0.0 {
            distance / st.speed
        } else {
            f32::MAX
        };
    }
    if st.speed > 0.5 {
        distance / st.speed
    } else {
        time_to(distance, 0.0, st.accel) + st.reaction
    }
}

/// Typed `[blockpath]` mode. See the module docs for the decision meaning.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BlockMode {
    /// Bodies on the two paths conflict while one actually occupies it.
    Occupy,
    /// A speculative reservation alone refuses the other's claim.
    Reserve,
    /// The other path is oncoming: reserve is allowed, entry waits for its commitment.
    Oncoming,
}

impl BlockMode {
    /// The stored `[blockpath]` second value is an unsigned byte; unknown values are treated
    /// as the conservative [`BlockMode::Occupy`] and reported by `Network::validate`.
    pub fn from_raw(mode: u16) -> BlockMode {
        match mode {
            1 => BlockMode::Reserve,
            2 => BlockMode::Oncoming,
            _ => BlockMode::Occupy,
        }
    }
}

/// Whether `a` declares a `[blockpath]` rule against lane `b`, and with which mode.
pub fn block_mode_between(net: &Network, a: usize, b: usize) -> Option<BlockMode> {
    let path_of = |l: usize| net.lanes[l].key.map(|k| k.path);
    let bp = path_of(b)?;
    let rule: Option<BlockRule> = net.lanes[a].blocks.iter().copied().find(|r| r.path == bp);
    if let Some(r) = rule {
        return Some(BlockMode::from_raw(r.mode));
    }
    // The rule may be declared on the other path; the relation is symmetric in content.
    let ap = path_of(a)?;
    net.lanes[b]
        .blocks
        .iter()
        .copied()
        .find(|r| r.path == ap)
        .map(|r| BlockMode::from_raw(r.mode))
}

/// The junction inputs for one vehicle.
#[derive(Debug, Clone)]
pub struct JunctionActor {
    /// Active emergency drive, distinct from general script/depot priority.
    pub emergency: bool,
    pub id: VehicleId,
    pub lane: usize,
    pub s: f32,
    /// Origin to front bumper (m).
    pub front: f32,
    /// Origin to rear bumper (m).
    pub rear: f32,
    pub length: f32,
    /// Standstill gap the vehicle leaves (m).
    pub min_gap: f32,
    pub speed: f32,
    pub accel: f32,
    pub decel: f32,
    pub reaction: f32,
    pub accept_gap: f32,
    pub yield_time: f32,
    pub stopped: f32,
    pub crawl: f32,
    pub lead_info: Option<(VehicleId, f32)>,
    pub light_hold: bool,
    pub yielding: bool,
    pub wait_at: Option<f32>,
    /// `TrafficPriority` set by the script (an ambulance, the player's bus).
    pub priority: bool,
    /// Out on the oncoming lane, passing the vehicles in front of it.
    pub passing: bool,
    /// A timetable bus.
    pub bus: bool,
}

impl JunctionActor {
    /// A minimal actor for tests and adapters.
    pub fn new(id: VehicleId, lane: usize, s: f32) -> JunctionActor {
        JunctionActor {
            emergency: false,
            id,
            lane,
            s,
            front: 2.25,
            rear: 2.25,
            length: 4.5,
            min_gap: 2.0,
            speed: 0.0,
            accel: 1.5,
            decel: 2.5,
            reaction: 0.7,
            accept_gap: 5.0,
            yield_time: 0.0,
            stopped: 0.0,
            crawl: 0.0,
            lead_info: None,
            light_hold: false,
            yielding: false,
            wait_at: None,
            priority: false,
            passing: false,
            bus: false,
        }
    }
}

/// The frozen world a junction plan reads. `on_lane` and `coming` are keyed by lane index and
/// list `(actor index, position)`, matching the perception views built once per tick.
pub struct JunctionScene<'a> {
    pub net: &'a Network,
    pub actors: &'a [JunctionActor],
    pub index_of: &'a HashMap<VehicleId, usize>,
    /// lane -> `(actor index, s, lateral, foreign)` of realized bodies.
    pub on_lane: &'a HashMap<usize, Vec<(usize, f32, f32, bool)>>,
    /// lane -> `(actor index, distance from its origin to the lane start)` of vehicles coming.
    pub coming: &'a HashMap<usize, Vec<(usize, f32)>>,
    /// footpath lane -> pedestrian positions.
    pub walkers: &'a HashMap<usize, Vec<f32>>,
    pub geo_prev: &'a HashMap<VehicleId, Option<VehicleId>>,
    /// `(crossing object instance, light index)` -> aspect this tick.
    pub aspects: &'a HashMap<(usize, usize), Aspect>,
    /// Simulation time (s), for diagnostics only.
    pub time: f32,
    pub tick: u64,
}

/// The coordinator's decision for one vehicle in one tick.
#[derive(Debug, Clone, PartialEq)]
pub struct JunctionDecision {
    /// Signal stop distance from the vehicle origin (m), if the light holds it.
    pub light: Option<f32>,
    /// Right-of-way stop distance from the vehicle origin (m), if it must yield.
    pub yield_at: Option<f32>,
    pub state: JunctionState,
    /// Every active junction cause.
    pub reasons: Vec<Reason>,
    /// The cause that binds.
    pub binding: Option<Reason>,
}

impl JunctionDecision {
    fn none() -> JunctionDecision {
        JunctionDecision {
            light: None,
            yield_at: None,
            state: JunctionState::Cleared,
            reasons: Vec::new(),
            binding: None,
        }
    }
}

/// A committed junction claim, kept across ticks until released.
#[derive(Debug, Clone, PartialEq)]
struct Commitment {
    /// Lanes whose entry the vehicle has claimed.
    lanes: Vec<usize>,
    /// The exit lane whose storage it reserved, and how much of it (m).
    storage: Option<(usize, f32)>,
    state: JunctionState,
}

impl Default for Commitment {
    fn default() -> Commitment {
        Commitment {
            lanes: Vec::new(),
            storage: None,
            state: JunctionState::Approaching,
        }
    }
}

/// A recovery category for a persistent hold, produced by the wait-for graph.
#[derive(Debug, Clone, PartialEq)]
pub enum Recovery {
    /// Speculative claims in a cycle nobody can satisfy: cancel them and let the
    /// deterministic order retry a valid, safe manoeuvre.
    CancelStaleClaim {
        vehicle: VehicleId,
        lanes: Vec<usize>,
    },
    /// The hold clears if the safe manoeuvre is attempted again (no conflicting body).
    RetrySafeManeuver { vehicle: VehicleId },
    /// The vehicle's route/lifecycle needs attention (route change, removal).
    RequestRouteRecovery { vehicle: VehicleId },
    /// The hold is legal (a red signal or a physically full road): wait, do not force.
    WaitLegal { vehicle: VehicleId },
    /// A rule-only wait cycle stood too long: this one is let go (still waits for moving
    /// vehicles, bodies, people and signals; drives into a full exit up to the queue's tail).
    GridlockPass { vehicle: VehicleId },
}

/// How a persistent hold is classified.
#[derive(Debug, Clone, PartialEq)]
pub enum WaitDiagnosis {
    /// A cyclic set of speculative claims that no member can satisfy.
    StaleClaimCycle { cycle: Vec<VehicleId> },
    /// Legal congestion: at least one member waits on a red signal.
    LegalCongestion { cycle: Vec<VehicleId> },
    /// A cycle held by bodies in the way (or with a member still moving): capacity, not a bug.
    FullCapacity { cycle: Vec<VehicleId> },
    /// Standing vehicles that each wait for the next by the rules alone (right of way, a full
    /// exit, the box rule, or queued behind such a hold). No rule ends it, so one is let go.
    YieldCycle { cycle: Vec<VehicleId> },
    /// A member's route changed or was removed.
    RouteRecovery { vehicle: VehicleId },
}

/// The junction coordinator: the single owner of junction commitments and the wait-for graph.
#[derive(Debug, Clone, Default)]
pub struct JunctionCoordinator {
    emergency_reservations: Vec<emergency::EmergencyReservation>,
    claims: Arbiter,
    amber: HashMap<VehicleId, (usize, usize)>,
    /// The crossing object a vehicle is inside and its lights already driven through (a later
    /// path carrying one of them again does not hold it).
    through_light: HashMap<VehicleId, (((i32, i32), i64), Vec<(usize, usize)>)>,
    commitments: HashMap<VehicleId, Commitment>,
    wait_for: HashMap<VehicleId, VehicleId>,
    /// Every hold this tick (ego -> all it waits for), for the cycle search.
    wait_edges: HashMap<VehicleId, Vec<VehicleId>>,
    /// Vehicles held this tick only by the right of way (no body, signal or full exit).
    rule_waiters: HashSet<VehicleId>,
    /// Vehicles held this tick by a full exit or the box rule (and perhaps the right of
    /// way), but by no body, person or signal.
    exit_waiters: HashSet<VehicleId>,
    /// Since when (s) a vehicle stands in a cycle of rule waiters.
    gridlock_since: HashMap<VehicleId, f32>,
    /// Vehicles let go out of such a cycle, and until when (s).
    gridlock_pass: HashMap<VehicleId, f32>,
    /// Tick a vehicle last entered `Waiting`, for stable fairness ordering.
    waiting_since: HashMap<VehicleId, u64>,
}

impl JunctionCoordinator {
    pub fn new() -> JunctionCoordinator {
        JunctionCoordinator::default()
    }

    /// The lanes a vehicle currently claims.
    pub fn claims_of(&self, id: VehicleId) -> &[usize] {
        self.commitments
            .get(&id)
            .map(|c| c.lanes.as_slice())
            .unwrap_or(&[])
    }

    pub fn state_of(&self, id: VehicleId) -> JunctionState {
        self.commitments
            .get(&id)
            .map(|c| c.state)
            .unwrap_or(JunctionState::Cleared)
    }

    pub fn holds(&self, id: VehicleId, lane: usize) -> bool {
        self.claims_of(id).contains(&lane)
    }

    /// Rehydrate a committed claim without planning (a persisted/LAN commit).
    pub fn restore_claim(&mut self, id: VehicleId, lanes: &[usize]) {
        for &l in lanes {
            self.claims.grant(LaneId(l), id);
        }
        let c = self.commitments.entry(id).or_default();
        for &l in lanes {
            if !c.lanes.contains(&l) {
                c.lanes.push(l);
            }
        }
        c.lanes.sort_unstable();
        c.state = JunctionState::Inside;
    }

    /// The order to plan a tick's vehicles in: claim holders first (their grant stands), then
    /// by stable id, so the outcome does not depend on storage order.
    pub fn plan_order(&self, actors: &[JunctionActor]) -> Vec<usize> {
        let mut order: Vec<usize> = (0..actors.len()).collect();
        order.sort_unstable_by_key(|&i| (self.claims_of(actors[i].id).is_empty(), actors[i].id));
        order
    }

    /// Who this vehicle is currently waiting behind at a junction, if anyone.
    pub fn blocked_by(&self, id: VehicleId) -> Option<VehicleId> {
        self.wait_for.get(&id).copied()
    }

    /// Whether the vehicle waits by right of way alone (`rule`) or for exit room (`exit`).
    pub fn wait_kind(&self, id: VehicleId) -> Option<&'static str> {
        if self.rule_waiters.contains(&id) {
            Some("rule")
        } else if self.exit_waiters.contains(&id) {
            Some("exit")
        } else {
            None
        }
    }

    /// How many vehicles currently hold a junction commitment (a health/leak check).
    pub fn commitment_count(&self) -> usize {
        self.commitments.len()
    }

    /// Begin a tick: refresh the arbiter from committed claims and exit storage (earlier
    /// admissions count against an exit until they are on it).
    pub fn begin_tick(&mut self, _tick: u64) {
        self.claims = Arbiter::new();
        for (id, c) in &self.commitments {
            for &l in &c.lanes {
                self.claims.grant(LaneId(l), *id);
            }
            if let Some((e, need)) = c.storage {
                self.claims.hold_storage(LaneId(e), *id, need);
            }
        }
        self.wait_for.clear();
        self.wait_edges.clear();
        self.rule_waiters.clear();
        self.exit_waiters.clear();
    }

    /// Record that `me` waits for `other` this tick.
    fn wait_on(&mut self, me: VehicleId, other: VehicleId) {
        self.wait_for.insert(me, other);
        let edges = self.wait_edges.entry(me).or_default();
        if !edges.contains(&other) {
            edges.push(other);
        }
    }

    /// Release every claim and the exit storage of `id`, with a reason.
    pub fn release(&mut self, id: VehicleId, _reason: Reason) {
        self.emergency_reservations.retain(|r| r.owner != id);
        if let Some(c) = self.commitments.remove(&id) {
            for l in c.lanes {
                self.claims.release(LaneId(l), id);
            }
            if let Some((e, _)) = c.storage {
                self.claims.release_storage(LaneId(e), id);
            }
        }
        self.amber.remove(&id);
        self.through_light.remove(&id);
        self.wait_for.remove(&id);
        self.wait_edges.remove(&id);
        self.gridlock_since.remove(&id);
        self.gridlock_pass.remove(&id);
        self.waiting_since.remove(&id);
    }

    /// A route change (or removal) drops claims that no longer lie on the vehicle's way.
    pub fn retain_on_way(&mut self, id: VehicleId, way: &[usize]) {
        let mut empty = false;
        if let Some(c) = self.commitments.get_mut(&id) {
            let before = c.lanes.len();
            c.lanes.retain(|l| way.contains(l));
            if c.lanes.len() != before {
                self.wait_for.remove(&id);
                self.wait_edges.remove(&id);
            }
            empty = c.lanes.is_empty() && c.storage.is_none();
        }
        if empty {
            self.commitments.remove(&id);
            self.waiting_since.remove(&id);
        }
    }

    /// A rebuilt (not merely grown) network releases every claim made against the old one.
    pub fn invalidate_network(&mut self) {
        self.emergency_reservations.clear();
        self.claims = Arbiter::new();
        self.commitments.clear();
        self.amber.clear();
        self.wait_for.clear();
        self.wait_edges.clear();
        self.rule_waiters.clear();
        self.exit_waiters.clear();
        self.gridlock_since.clear();
        self.gridlock_pass.clear();
        self.waiting_since.clear();
        self.through_light.clear();
    }

    /// The current wait-for edges (ego -> blocker).
    pub fn wait_for_graph(&self) -> &HashMap<VehicleId, VehicleId> {
        &self.wait_for
    }

    /// Where the light makes the vehicle stop (distance from its origin), or None. One too
    /// close to stop at yellow drives on and remembers it, so red does not stop it mid-junction.
    fn signal_stop(
        &mut self,
        scene: &JunctionScene,
        i: usize,
        way: &[(usize, f32)],
    ) -> Option<f32> {
        let actor = &scene.actors[i];
        let v = actor.speed;
        let mut stop = None;
        let mut amber = self.amber.get(&actor.id).copied();
        match light_object(scene.net, way[0].0) {
            Some(here) => {
                let entry = self.through_light.entry(actor.id).or_insert((here, Vec::new()));
                if entry.0 != here {
                    *entry = (here, Vec::new());
                }
                if let Some(l) = scene.net.lanes[way[0].0].traffic_light {
                    if !entry.1.contains(&l) {
                        entry.1.push(l);
                    }
                }
            }
            None => {
                self.through_light.remove(&actor.id);
            }
        }
        let passed: &[(usize, usize)] =
            self.through_light.get(&actor.id).map(|t| t.1.as_slice()).unwrap_or(&[]);
        let passed = passed.to_vec();
        for (k, &(_, d)) in way.iter().enumerate().skip(1) {
            if d > 150.0 {
                break;
            }
            let Some((c, li)) = light_at_entry(scene.net, way, k) else {
                continue;
            };
            if passed.contains(&(c, li)) {
                continue;
            }
            let Some(&aspect) = scene.aspects.get(&(c, li)) else {
                continue;
            };
            let gap = d - actor.front;
            let stop_aspect = !matches!(aspect, Aspect::Green | Aspect::Dark);
            if actor.emergency && stop_aspect && self.emergency_owns(actor.id, way[k].0) {
                self.mark_against_signal(actor.id, way[k].0);
            }
            let for_emergency = (actor.emergency && self.emergency_owns(actor.id, way[k].0))
                || self.emergency_leads(actor.id, &way[k..]);
            if for_emergency && gap < 15.0 && actor.speed <= emergency::EMERGENCY_CROSSING_SPEED {
                // Red-light exception for the reserved movement only; admission still checks
                // occupants, pedestrians and full exits.
                continue;
            }
            let comfortable = v * v / (2.0 * actor.decel * 1.4) + 1.0;
            let possible = v * v / (2.0 * MAX_BRAKE * 0.8);
            // Reaching the line on green/yellow commits only while still moving on (or rolling
            // with its front over the line): a car that crept up in a queue stops at red.
            let committed = amber == Some((c, li))
                && (v >= AMBER_COMMIT_SPEED || (gap < 0.0 && v > 1.0));
            let go = match aspect {
                Aspect::Green | Aspect::Dark => {
                    if gap < AT_LINE {
                        amber = Some((c, li));
                    }
                    true
                }
                Aspect::Yellow | Aspect::GreenYellow => {
                    if committed || gap < comfortable {
                        amber = Some((c, li));
                        true
                    } else {
                        false
                    }
                }
                Aspect::Red | Aspect::RedYellow => {
                    (committed && gap < comfortable) || gap < possible - 0.5
                }
            };
            if !go {
                stop = Some(d);
                break;
            }
        }
        // the light the vehicle went through on amber is behind it now
        if let Some(a) = amber {
            if !way.iter().skip(1).any(|&(l, _)| scene.net.lanes[l].traffic_light == Some(a)) {
                amber = None;
            }
        }
        match amber {
            Some(a) => {
                self.amber.insert(actor.id, a);
            }
            None => {
                self.amber.remove(&actor.id);
            }
        }
        stop
    }

    /// Plan one vehicle's junction movement for this tick. `way` is its planned lanes and
    /// distances, `lead` what it follows, `movement` the junction on that way.
    pub fn plan(
        &mut self,
        scene: &JunctionScene,
        i: usize,
        way: &[(usize, f32)],
        lead: Option<Lead>,
        movement: Option<Movement>,
    ) -> JunctionDecision {
        let actor = &scene.actors[i];
        self.release_passed_storage(actor.id, way);
        let light = self.signal_stop(scene, i, way);
        let Some(jn) = movement else {
            // no junction: any claim is behind the vehicle now
            self.drop_claims(actor.id);
            let mut d = JunctionDecision::none();
            d.light = light;
            return d;
        };
        // Yield to an emergency reservation (not on a ring or ahead of it; an admitted one
        // too while it can still stop, #462). Beyond a red line nothing is decided yet.
        let can_stop_before = jn.lanes[0].1 - actor.front >= firm_stop(actor);
        let my_lanes: Vec<usize> = jn.lanes.iter().map(|&(l, _)| l).collect();
        if !jn.inside && !scene.net.is_ring(actor.lane) && self.emergency_reservations.iter().any(|r|
            r.owner != actor.id && !r.ahead.contains(&actor.id)
                && jn.lanes.iter().any(|(l, _)| r.lanes.contains(l))
                && Self::meets_emergency(scene.net, r, &my_lanes))
            && (self.claims_of(actor.id).is_empty() || can_stop_before)
        {
            self.drop_claims(actor.id);
            let mut d = JunctionDecision::none();
            d.light = light;
            d.yield_at = Some(jn.lanes[0].1);
            d.state = JunctionState::Waiting;
            d.reasons.push(Reason::EmergencyYield);
            d.binding = Some(Reason::EmergencyYield);
            return d;
        }
        if let Some(l) = light {
            if !jn.inside && jn.lanes[0].1 >= l - 0.5 {
                let mut d = JunctionDecision::none();
                d.light = light;
                d.state = JunctionState::Approaching;
                return d;
            }
        }
        let (yield_at, reasons, state) = self.admit(scene, i, &jn, way, lead);
        let mut d = JunctionDecision::none();
        d.light = light;
        d.yield_at = yield_at;
        d.state = state;
        d.binding = reasons.first().copied();
        d.reasons = reasons;
        d
    }

    /// Right of way at `jn` for actor `i`: where it has to wait (distance from its origin), or
    /// None when it may go - in which case it claims the junction's lanes.
    fn admit(
        &mut self,
        scene: &JunctionScene,
        i: usize,
        jn: &Movement,
        way: &[(usize, f32)],
        lead: Option<Lead>,
    ) -> (Option<f32>, Vec<Reason>, JunctionState) {
        let actor = &scene.actors[i];
        let v = actor.speed;
        let entry = jn.lanes[0].1;
        let decide = (v * v / (2.0 * actor.decel) + 12.0).clamp(DECIDE_MIN, DECIDE_MAX);
        let me_id = actor.id;
        let mut reasons: Vec<Reason> = Vec::new();
        // too far to decide: a claim made just inside that distance stands
        if !jn.inside && entry - actor.front > decide {
            if entry - actor.front > decide + DECIDE_RELEASE {
                self.drop_claims(me_id);
            }
            return (None, reasons, JunctionState::Approaching);
        }
        // queued behind someone who is not through the junction yet: no claim
        let queued = !jn.inside
            && lead
                .map(|l| l.speed < 1.0 && l.gap < entry - actor.front + 3.0)
                .unwrap_or(false);
        let a_me = actor.accel;
        let committed = self.holds(me_id, jn.lanes[0].0);
        let room = entry - actor.front - 0.6;
        let cannot_stop = !jn.inside && v > 1.0 && room < v * v / (2.0 * MAX_BRAKE * 0.7);
        let cannot_stop_gently = !jn.inside && v > 1.0 && room < v * v / (2.0 * actor.decel * 1.5);
        let mut hard = false;
        // held back by a full exit or the box rule
        let mut ruled = false;
        // held back by the right of way alone
        let mut yields = false;
        // (of those, for a vehicle that is moving)
        let mut yields_moving = false;
        let mut soft: Vec<usize> = Vec::new();
        // (stop point short of the meeting place, and whether the other one is oncoming)
        let mut holds: Vec<(f32, bool)> = Vec::new();
        let mut pedestrian = false;
        let mut stop_at = if jn.inside { None } else { Some(entry) };
        let me_prio = actor.priority;
        let wait = actor.yield_time;
        let patience = 1.0 - (wait / 40.0).min(1.0) / 3.0;
        for &(l, dl) in &jn.lanes {
            for c in &scene.net.crossings[l] {
                let point = dl + c.at;
                let m = c.other;
                if way.iter().any(|w| w.0 == m) {
                    continue;
                }
                let oncoming = wrap_deg(
                    scene.net.lanes[m].start_heading() - scene.net.lanes[l].start_heading(),
                )
                .abs()
                    > 135.0;
                let hold = (point - c.before, oncoming);
                if point + c.after < -actor.rear - 0.3 {
                    continue;
                }
                let on = scene
                    .on_lane
                    .get(&m)
                    .map(|v| v.as_slice())
                    .unwrap_or(&[])
                    .iter()
                    .filter(|e| !e.3)
                    .map(|&(j, sj, _, _)| (j, c.other_at - sj, true));
                let near = scene
                    .coming
                    .get(&m)
                    .map(|v| v.as_slice())
                    .unwrap_or(&[])
                    .iter()
                    .map(|&(j, dj)| (j, dj + c.other_at, false));
                for (j, dj, is_on) in on.chain(near) {
                    if j == i {
                        continue;
                    }
                    let o = &scene.actors[j];
                    if scene.geo_prev.get(&o.id).copied().flatten() == Some(me_id) {
                        continue;
                    }
                    if dj + c.other_after < -o.rear - 0.3 {
                        continue;
                    }
                    let t_clear = time_to(point + c.after + actor.rear + 0.3, v, a_me)
                        + if v < 0.5 { actor.reaction } else { 0.0 };
                    let t_mine = time_to(point - c.before - actor.front, v, a_me)
                        + if v < 0.1 { actor.reaction } else { 0.0 };
                    // (a long waiter at its line keeps its claim, weighed from any distance)
                    let seen = self
                        .commitments
                        .get(&o.id)
                        .is_some_and(|c| c.state == JunctionState::Waiting);
                    if !jn.inside && point - c.before - actor.front > INSIDE_MEETING && !seen {
                        continue;
                    }
                    let stalled = (o.stopped > 4.0 && o.speed < 0.1)
                        || o.crawl >= 8.0
                        || (o.speed < 1.5
                            && o.lead_info.is_some_and(|(lid, gap)| {
                                gap < 8.0
                                    && scene
                                        .index_of
                                        .get(&lid)
                                        .map(|&k| scene.actors[k].speed < 1.0)
                                        .unwrap_or(false)
                            }));
                    // `[blockpath]` beats geometry: `Occupy` ignores claims, `Reserve` refuses
                    // a reservation, `Oncoming` keeps the default commitment rule.
                    let mode = block_mode_between(scene.net, l, m);
                    // (an entry path on the ring's object still gives way at the merge)
                    let (i_yield, they_yield) = (gives_way(scene.net, actor, l, m), gives_way(scene.net, o, m, l));
                    let yielding_merge = c.merge && i_yield && !they_yield;
                    let priority_merge = c.merge && they_yield && !i_yield;
                    // A `[rule] priority` difference is signage: an early claim from the lower
                    // road does not win while it can still stop (same distance on both sides).
                    let signed = (scene.net.lanes[l].priority - scene.net.lanes[m].priority)
                        .abs() > 0.5;
                    let mine_ahead = point - c.before - actor.front;
                    let i_give_way = signed && !jn.inside && i_yield
                        && mine_ahead > stopping_distance(actor);
                    let they_give_way = signed && they_yield
                        && !same_object(scene.net, o.lane, m);
                    let claims_block = mode != Some(BlockMode::Occupy);
                    let claimed = self.claims.holds(LaneId(m), o.id)
                        && (!stalled || seen)
                        && claims_block;
                    let theirs = dj - c.other_before - o.front;
                    let waits_short = o.light_hold
                        || (o.yielding
                            && !claimed
                            && o.wait_at
                                .map(|w| w - 0.6 <= dj - c.other_before)
                                .unwrap_or(false));
                    let t_j = crossing_arrival(
                        &actor_state(scene, o),
                        theirs,
                        claimed,
                        waits_short,
                        stalled,
                    );
                    // reservation refusal: a claim alone holds this vehicle back
                    if mode == Some(BlockMode::Reserve)
                        && self.claims.holds(LaneId(m), o.id)
                        && !stalled
                    {
                        hard = true;
                        holds.push(hold);
                        reasons.push(Reason::JunctionClaim);
                        if jn.inside {
                            stop_at = Some(stop_at.unwrap_or(f32::MAX).min(point - c.before));
                        }
                        self.wait_on(me_id, o.id);
                        continue;
                    }
                    if is_on && theirs <= 0.3 {
                        let mine_in = actor.front - (point - c.before);
                        let theirs_in = -theirs;
                        let ahead = mine_in > 0.0
                            && (mine_in > theirs_in + 0.3
                                || ((mine_in - theirs_in).abs() <= 0.3 && me_id > o.id));
                        if !ahead {
                            hard = true;
                            holds.push(hold);
                            reasons.push(Reason::Yield);
                            self.wait_on(me_id, o.id);
                            if jn.inside {
                                stop_at = Some(stop_at.unwrap_or(f32::MAX).min(point - c.before));
                            }
                        }
                        continue;
                    }
                    // The ring beats an entry of the same object, whatever it claimed, while
                    // the entering car can still stop braking firmly (a gentle-rate test let a
                    // 48 km/h car 40 m out cut in).
                    let ring_first = scene.net.is_ring(l) && !scene.net.is_ring(m)
                        && scene.net.must_yield(m, l);
                    if ring_first && mode.is_none() && !o.emergency && theirs > firm_stop(o) {
                        continue;
                    }
                    // Entering a roundabout, also give way to a ring car standing just before
                    // the entry, or the entries fill the ring (not once nosed in: deadlock).
                    let nosed_in = mine_ahead < -NOSED_IN;
                    let entering_ring = scene.net.is_ring(m) && !scene.net.is_ring(l)
                        && scene.net.must_yield(l, m);
                    if entering_ring && !actor.emergency && o.speed < 1.0
                        && theirs > 0.3 && theirs < RING_QUEUE_KEEP && !nosed_in
                    {
                        yields = true;
                        yields_moving |= o.speed >= 0.3;
                        holds.push(hold);
                        reasons.push(Reason::Yield);
                        self.wait_on(me_id, o.id);
                        if jn.inside {
                            stop_at = Some(stop_at.unwrap_or(f32::MAX).min(point - c.before));
                        }
                        continue;
                    }
                    if (priority_merge || they_give_way) && mode.is_none() && !o.emergency
                        && theirs > stopping_distance(o)
                    {
                        continue;
                    }
                    // Well into the meeting place, a vehicle that has to give way clears it
                    // while the other can still stop.
                    if nosed_in && !o.emergency && mode.is_none() && theirs > stopping_distance(o) {
                        continue;
                    }
                    if claimed || (is_on && o.speed > 0.5 && !waits_short) {
                        let me_decided = (committed || jn.inside) && !yielding_merge && !i_give_way;
                        let first = if me_decided {
                            t_j < t_mine - 0.3 || ((t_j - t_mine).abs() <= 0.3 && o.id < me_id)
                        } else {
                            true
                        };
                        if first && t_j < t_clear * if me_decided { 1.0 } else { patience } + 1.0 {
                            hard = true;
                            holds.push(hold);
                            reasons.push(Reason::JunctionClaim);
                            self.wait_on(me_id, o.id);
                            if jn.inside {
                                stop_at = Some(stop_at.unwrap_or(f32::MAX).min(point - c.before));
                            }
                        }
                        continue;
                    }
                    let o_prio = o.priority;
                    if (jn.inside && !yielding_merge)
                        || (committed && !yielding_merge && !i_give_way)
                        || (me_prio && !o_prio && !yielding_merge)
                        || (!i_yield && !(o_prio && !me_prio))
                    {
                        continue;
                    }
                    if t_j < (actor.accept_gap + 2.0).max(t_clear + 1.0) * patience {
                        // (a priority-road car stopped only briefly still outranks a long
                        // wait; a stalled queue is left to the fairness rule)
                        holds.push(hold);
                        if t_j == f32::MAX || (o.speed < 0.3 && (stalled || !signed)) {
                            soft.push(j);
                        } else {
                            yields = true;
                            yields_moving |= o.speed >= 0.3;
                            reasons.push(Reason::Yield);
                            self.wait_on(me_id, o.id);
                            if jn.inside {
                                stop_at = Some(stop_at.unwrap_or(f32::MAX).min(point - c.before));
                            }
                        }
                    }
                }
            }
            for &(w, at, w_at) in &scene.net.walks[l] {
                let point = dl + at;
                if point < actor.front - 1.0 {
                    continue;
                }
                if scene
                    .walkers
                    .get(&w)
                    .map(|ps| ps.iter().any(|&p| (p - w_at).abs() < 3.0))
                    .unwrap_or(false)
                    && point - actor.front < 30.0
                {
                    hard = true;
                    pedestrian = true;
                    reasons.push(Reason::Pedestrian);
                    let before = point - 2.5;
                    stop_at = Some(stop_at.unwrap_or(before).min(before));
                }
            }
        }
        // Let out of a rule-only gridlock (`classify_waits`): right of way to standing ones, a
        // full exit and the box rule no longer hold it; moving vehicles, bodies, people and
        // signals still do.
        let pass = self.gridlock_pass.get(&me_id).is_some_and(|&until| scene.time < until);
        let mut exit_full = false;
        // (the exit storage this vehicle takes if it is admitted)
        let mut storage: Option<(usize, f32)> = None;
        if !jn.inside && !pass {
            if let Some((e, de)) = jn.exit {
                let room = scene
                    .on_lane
                    .get(&e)
                    .map(|v| v.as_slice())
                    .unwrap_or(&[])
                    .iter()
                    .filter(|x| !x.3 && x.0 != i)
                    .map(|&(j, sj, _, _)| (sj - scene.actors[j].rear, scene.actors[j].speed, j))
                    .fold(None::<(f32, f32, usize)>, |acc, x| {
                        if acc.map(|a| x.0 < a.0).unwrap_or(true) {
                            Some(x)
                        } else {
                            acc
                        }
                    });
                if let Some((space, speed, tail)) = room {
                    if speed < 1.5 && de < 40.0 {
                        self.claims.set_storage_capacity(LaneId(e), space);
                        let need = actor.length + actor.min_gap;
                        let arrived: Vec<VehicleId> = scene.on_lane[&e]
                            .iter()
                            .filter(|x| !x.3)
                            .map(|x| scene.actors[x.0].id)
                            .collect();
                        if space < need
                            || !self.claims.storage_fits(LaneId(e), me_id, need, &arrived)
                        {
                            ruled = true;
                            exit_full = true;
                            reasons.push(Reason::OccupiedExit);
                            // (waits for the queue's tail and the earlier room holders)
                            let holders: Vec<VehicleId> = self
                                .claims
                                .storage_holders(LaneId(e))
                                .filter(|&x| x != me_id && !arrived.contains(&x))
                                .collect();
                            self.wait_on(me_id, scene.actors[tail].id);
                            for x in holders {
                                self.wait_on(me_id, x);
                            }
                        } else {
                            storage = Some((e, need));
                        }
                    }
                }
            }
        }
        // Don't block the box (StVO §11 (1)): with the traffic ahead standing, wait at the
        // line unless the whole vehicle fits beyond the movement's last conflict.
        if !jn.inside && !exit_full && !pass {
            if let Some(l) = lead {
                let lead_rear = actor.front + l.gap;
                let clear_at = movement_clear_at(scene.net, jn);
                if lead_rear > entry
                    && l.speed < QUEUE_SPEED
                    && l.acc < 0.5
                    && lead_rear - clear_at < actor.length + actor.min_gap
                {
                    ruled = true;
                    exit_full = true;
                    reasons.push(Reason::OccupiedExit);
                    if let Some((ahead, _)) = actor.lead_info {
                        self.wait_on(me_id, ahead);
                    }
                }
            }
        }
        // keep-clear `[crossingproblem]` path: the whole movement must be clear to enter
        if jn.keep_clear && !jn.inside {
            let occupied = jn.lanes.iter().any(|&(l, _)| {
                scene
                    .on_lane
                    .get(&l)
                    .map(|v| v.iter().any(|&(j, _, _, foreign)| j != i && !foreign))
                    .unwrap_or(false)
            });
            if occupied {
                hard = true;
                reasons.push(Reason::OccupiedExit);
            }
        }
        if pass {
            yields = yields_moving;
            soft.clear();
        }
        let mut blocked = (hard && !cannot_stop)
            || ((ruled || yields || !soft.is_empty()) && !cannot_stop_gently);
        // An emergency vehicle does not wait for merely standing vehicles (they stand for it;
        // waiting on them too deadlocked it at the line).
        if actor.emergency {
            blocked |= hard || ruled || yields;
        }
        if !hard && !ruled && !yields && !soft.is_empty() && wait > 2.5 + actor.reaction {
            // everybody is waiting for somebody: the longest waiter goes (bounded fairness)
            let wins = soft.iter().all(|&j| {
                let o = &scene.actors[j];
                (o.yielding || o.speed < 0.3)
                    && (wait > o.yield_time + 0.05
                        || ((wait - o.yield_time).abs() <= 0.05 && actor.id < o.id))
            });
            if wins {
                blocked = false;
            }
        }
        let lanes: Vec<usize> = jn.lanes.iter().map(|x| x.0).collect();
        let state = if jn.inside {
            JunctionState::Inside
        } else if blocked || exit_full {
            JunctionState::Waiting
        } else {
            JunctionState::Admitted
        };
        if blocked && !jn.inside {
            // A long waiter makes himself seen: he keeps a claim while waiting, so uncommitted
            // traffic holds back for him (else a side road at a busy main road waits for good).
            if !exit_full && !queued && wait > LONG_WAIT_CLAIM {
                for &l in &lanes {
                    self.claims.grant(LaneId(l), me_id);
                }
                let c = self.commitments.entry(me_id).or_default();
                for &l in &lanes {
                    if !c.lanes.contains(&l) {
                        c.lanes.push(l);
                    }
                }
                c.lanes.sort_unstable();
                c.state = JunctionState::Waiting;
                // (the exit room is only for the admitted)
                if let Some((e, _)) = c.storage.take() {
                    self.claims.release_storage(LaneId(e), me_id);
                }
            } else {
                self.drop_claims(me_id);
            }
            if !hard && !ruled && !exit_full {
                self.rule_waiters.insert(me_id);
            } else if !hard {
                self.exit_waiters.insert(me_id);
            }
            // A turn across oncoming traffic at a signal pulls in to the meeting place and goes
            // when they stop (at the line only one car a cycle got through). One at a time.
            let signalized = jn
                .lanes
                .iter()
                .any(|&(l, _)| scene.net.lanes[l].traffic_light.is_some())
                || scene.net.prev.get(jn.lanes[0].0).is_some_and(|p| {
                    p.iter().any(|&q| scene.net.lanes[q].traffic_light.is_some())
                });
            if signalized
                && !pedestrian
                && !exit_full
                && !jn.keep_clear
                && !actor.light_hold
                && !actor.emergency
                && !holds.is_empty()
                && holds.iter().all(|h| h.1)
            {
                let pull = holds.iter().map(|h| h.0).fold(f32::MAX, f32::min);
                let lead_clear = lead.is_none_or(|l| actor.front + l.gap > pull + 1.0);
                if pull - entry >= PULL_IN_MIN && lead_clear {
                    stop_at = Some(pull);
                }
            }
            self.note_wait(me_id, scene.tick);
            reasons.sort_unstable_by_key(|r| r.label());
            reasons.dedup();
            return (stop_at, reasons, state);
        }
        if blocked {
            // inside the junction: clear safely rather than react to the entry light
            reasons.sort_unstable_by_key(|r| r.label());
            reasons.dedup();
            return (stop_at, reasons, JunctionState::Inside);
        }
        if queued && !jn.inside {
            self.drop_claims(me_id);
            return (None, reasons, JunctionState::Approaching);
        }
        for &l in &lanes {
            self.claims.grant(LaneId(l), me_id);
        }
        let c = self.commitments.entry(me_id).or_default();
        if let Some((e, need)) = storage {
            if let Some((old, _)) = c.storage.filter(|s| s.0 != e) {
                self.claims.release_storage(LaneId(old), me_id);
            }
            self.claims.hold_storage(LaneId(e), me_id, need);
            c.storage = Some((e, need));
        }
        for &l in &lanes {
            if !c.lanes.contains(&l) {
                c.lanes.push(l);
            }
        }
        c.lanes.sort_unstable();
        c.state = if jn.inside {
            JunctionState::Inside
        } else {
            JunctionState::Admitted
        };
        self.waiting_since.remove(&me_id);
        self.gridlock_pass.remove(&me_id);
        (None, reasons, c.state)
    }

    fn drop_claims(&mut self, id: VehicleId) {
        if let Some(c) = self.commitments.remove(&id) {
            for l in c.lanes {
                self.claims.release(LaneId(l), id);
            }
            if let Some((e, _)) = c.storage {
                self.claims.release_storage(LaneId(e), id);
            }
        }
    }

    /// Exit storage is held until the vehicle is on that exit or it is no longer ahead.
    fn release_passed_storage(&mut self, id: VehicleId, way: &[(usize, f32)]) {
        let Some(c) = self.commitments.get_mut(&id) else {
            return;
        };
        let Some((e, _)) = c.storage else {
            return;
        };
        if way.iter().skip(1).any(|w| w.0 == e) {
            return;
        }
        c.storage = None;
        self.claims.release_storage(LaneId(e), id);
        if c.lanes.is_empty() {
            self.commitments.remove(&id);
        }
    }

    fn note_wait(&mut self, id: VehicleId, tick: u64) {
        self.waiting_since.entry(id).or_insert(tick);
    }

    /// Classify persistent wait cycles: standing claim holders with no body in a conflict are
    /// a stale-claim deadlock, rule-only waits (and queues behind them) a yield cycle, and a
    /// cycle with a red signal or a body in the way is legal and not forced.
    pub fn classify_waits(
        &mut self,
        scene: &JunctionScene,
    ) -> (Vec<WaitDiagnosis>, Vec<Recovery>) {
        let mut out = Vec::new();
        for mut cycle in self.wait_cycles(scene) {
            // (deterministic report order)
            cycle.sort_unstable();
            out.push(self.classify_cycle(scene, &cycle));
        }
        let mut recoveries = Vec::new();
        let mut in_yield_cycle: HashSet<VehicleId> = HashSet::new();
        for d in &out {
            match d {
                WaitDiagnosis::StaleClaimCycle { cycle } => {
                    // the longest waiter retries, the rest cancel their claims
                    let sorted = self.by_wait(cycle);
                    for &id in sorted.iter().skip(1) {
                        let lanes = self
                            .commitments
                            .get(&id)
                            .map(|c| c.lanes.clone())
                            .unwrap_or_default();
                        for &l in &lanes {
                            self.claims.release(LaneId(l), id);
                        }
                        if let Some(c) = self.commitments.get_mut(&id) {
                            c.lanes.clear();
                            if let Some((e, _)) = c.storage.take() {
                                self.claims.release_storage(LaneId(e), id);
                            }
                        }
                        recoveries.push(Recovery::CancelStaleClaim { vehicle: id, lanes });
                    }
                    if let Some(&id) = sorted.first() {
                        recoveries.push(Recovery::RetrySafeManeuver { vehicle: id });
                    }
                }
                WaitDiagnosis::YieldCycle { cycle } => {
                    for &id in cycle {
                        in_yield_cycle.insert(id);
                        self.gridlock_since.entry(id).or_insert(scene.time);
                    }
                    let passed = cycle.iter().any(|id| {
                        self.gridlock_pass.get(id).is_some_and(|&until| scene.time < until)
                    });
                    let stood = cycle
                        .iter()
                        .map(|id| scene.time - self.gridlock_since[id])
                        .fold(f32::MAX, f32::min);
                    let first = self.by_wait(cycle).first().copied();
                    if let Some(id) = first.filter(|_| !passed && stood >= GRIDLOCK_AFTER) {
                        self.gridlock_pass.insert(id, scene.time + GRIDLOCK_PASS);
                        recoveries.push(Recovery::GridlockPass { vehicle: id });
                        log::info!(
                            "t={:.1}: junction gridlock of {} vehicles {:?}: vehicle {} goes first",
                            scene.time,
                            cycle.len(),
                            cycle,
                            id
                        );
                    }
                }
                WaitDiagnosis::LegalCongestion { cycle } | WaitDiagnosis::FullCapacity { cycle } => {
                    for &id in cycle {
                        recoveries.push(Recovery::WaitLegal { vehicle: id });
                    }
                }
                WaitDiagnosis::RouteRecovery { vehicle } => {
                    recoveries.push(Recovery::RequestRouteRecovery { vehicle: *vehicle });
                }
            }
        }
        self.gridlock_since.retain(|id, _| in_yield_cycle.contains(id));
        self.gridlock_pass.retain(|_, until| scene.time < *until);
        (out, recoveries)
    }

    /// `ids` ordered by how long they have waited (longest first), then by id.
    fn by_wait(&self, ids: &[VehicleId]) -> Vec<VehicleId> {
        let mut sorted = ids.to_vec();
        sorted.sort_unstable_by_key(|id| {
            (self.waiting_since.get(id).copied().unwrap_or(u64::MAX), *id)
        });
        sorted
    }

    /// The wait-for graph's strongly connected sets of 2+ vehicles (Tarjan), including queue
    /// links (a standing vehicle close behind a standing one waits for it).
    fn wait_cycles(&self, scene: &JunctionScene) -> Vec<Vec<VehicleId>> {
        let mut edges: HashMap<VehicleId, Vec<VehicleId>> = self.wait_edges.clone();
        let queued = scene.actors.iter().filter_map(|a| {
            let (ahead, gap) = a.lead_info?;
            let lead = &scene.actors[*scene.index_of.get(&ahead)?];
            (a.speed < 0.3 && gap < QUEUE_GAP && lead.speed < 0.3).then_some((a.id, ahead))
        });
        for (a, b) in self.wait_for.iter().map(|(&a, &b)| (a, b)).chain(queued) {
            let e = edges.entry(a).or_default();
            if !e.contains(&b) {
                e.push(b);
            }
        }
        let mut nodes: Vec<VehicleId> = edges.keys().copied().collect();
        nodes.sort_unstable();
        struct Tarjan<'a> {
            edges: &'a HashMap<VehicleId, Vec<VehicleId>>,
            index: HashMap<VehicleId, usize>,
            low: HashMap<VehicleId, usize>,
            stack: Vec<VehicleId>,
            on_stack: HashSet<VehicleId>,
            next: usize,
            out: Vec<Vec<VehicleId>>,
        }
        impl Tarjan<'_> {
            fn visit(&mut self, v: VehicleId) {
                self.index.insert(v, self.next);
                self.low.insert(v, self.next);
                self.next += 1;
                self.stack.push(v);
                self.on_stack.insert(v);
                let edges = self.edges;
                for &w in edges.get(&v).map(|e| e.as_slice()).unwrap_or(&[]) {
                    if !self.index.contains_key(&w) {
                        self.visit(w);
                        let lw = self.low[&w];
                        let lv = self.low.get_mut(&v).unwrap();
                        *lv = (*lv).min(lw);
                    } else if self.on_stack.contains(&w) {
                        let iw = self.index[&w];
                        let lv = self.low.get_mut(&v).unwrap();
                        *lv = (*lv).min(iw);
                    }
                }
                if self.low[&v] == self.index[&v] {
                    let mut set = Vec::new();
                    while let Some(w) = self.stack.pop() {
                        self.on_stack.remove(&w);
                        set.push(w);
                        if w == v {
                            break;
                        }
                    }
                    if set.len() >= 2 {
                        self.out.push(set);
                    }
                }
            }
        }
        let mut t = Tarjan {
            edges: &edges,
            index: HashMap::new(),
            low: HashMap::new(),
            stack: Vec::new(),
            on_stack: HashSet::new(),
            next: 0,
            out: Vec::new(),
        };
        for v in nodes {
            if !t.index.contains_key(&v) {
                t.visit(v);
            }
        }
        t.out
    }

    fn classify_cycle(&self, scene: &JunctionScene, cycle: &[VehicleId]) -> WaitDiagnosis {
        // a member whose light is red, or whose claimed exit is physically full, is legal
        let has_red = cycle.iter().any(|id| {
            scene
                .index_of
                .get(id)
                .map(|&i| scene.actors[i].light_hold)
                .unwrap_or(false)
        });
        if has_red {
            return WaitDiagnosis::LegalCongestion {
                cycle: cycle.to_vec(),
            };
        }
        let only_claims = cycle.iter().all(|id| {
            self.commitments
                .get(id)
                .map(|c| !c.lanes.is_empty() || c.storage.is_some())
                .unwrap_or(false)
        });
        if only_claims {
            return WaitDiagnosis::StaleClaimCycle {
                cycle: cycle.to_vec(),
            };
        }
        // (all stand, held by rules alone or queued behind such a hold; one at a junction)
        let by_rule = |id: &VehicleId| {
            self.rule_waiters.contains(id) || self.exit_waiters.contains(id)
        };
        let standing_by_rule = cycle.iter().any(by_rule)
            && cycle.iter().all(|id| {
                (by_rule(id) || !self.wait_edges.contains_key(id))
                    && scene
                        .index_of
                        .get(id)
                        .is_some_and(|&i| scene.actors[i].speed < 0.3)
            });
        if standing_by_rule {
            return WaitDiagnosis::YieldCycle {
                cycle: cycle.to_vec(),
            };
        }
        WaitDiagnosis::FullCapacity {
            cycle: cycle.to_vec(),
        }
    }
}

/// How far an actor travels before it stands, reacting and braking at its own deceleration.
fn stopping_distance(a: &JunctionActor) -> f32 {
    a.speed * a.reaction + a.speed * a.speed / (2.0 * a.decel.max(0.1))
}

/// Whether two lanes belong to the same crossing object.
fn same_object(net: &Network, a: usize, b: usize) -> bool {
    let object = |l: usize| {
        let lane = &net.lanes[l];
        lane.key.filter(|_| lane.source == 2).map(|k| (k.tile, k.id))
    };
    object(a).is_some() && object(a) == object(b)
}

/// A minimal `AiState` of an actor, for the crossing-arrival prediction.
fn actor_state(_scene: &JunctionScene, o: &JunctionActor) -> AiState {
    let mut st = AiState::new(o.lane, o.s, 0);
    st.speed = o.speed;
    st.accel = o.accel;
    st.decel = o.decel;
    st.reaction = o.reaction;
    st.accept_gap = o.accept_gap;
    st.front = o.front;
    st.rear = o.rear;
    st
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A bay forking off the road and running back into it 60 m on: a car coming out gives
    /// way, a timetable bus keeps its way (StVO §20 (5)).
    #[test]
    fn a_timetable_bus_keeps_its_way_out_of_its_bay() {
        use crate::network::{LaneBuilder, LaneKind};
        use glam::DVec3;
        let street = |p: Vec<(f64, f64)>| {
            LaneBuilder::polyline(p.into_iter().map(|(x, y)| DVec3::new(x, y, 0.0)).collect(), LaneKind::Street, 3.0)
        };
        let mut net = Network {
            lanes: vec![
                street(vec![(0.0, -100.0), (0.0, 0.0)]),
                street(vec![(0.0, 0.0), (0.0, 60.0)]),
                street(vec![(0.0, 0.0), (3.5, 10.0), (3.5, 45.0), (0.0, 60.0)]),
                street(vec![(0.0, 60.0), (0.0, 200.0)]),
            ],
            ..Default::default()
        };
        net.link(1.0);
        assert!(net.returns_beside(2, 1));
        let mut car = JunctionActor::new(VehicleId(1), 2, 40.0);
        assert!(gives_way(&net, &car, 2, 1));
        car.bus = true;
        assert!(!gives_way(&net, &car, 2, 1));
        // the road's own traffic does not give way to it either way
        assert!(!gives_way(&net, &JunctionActor::new(VehicleId(2), 1, 40.0), 1, 2));
    }

    fn classify_with(
        coord: &mut JunctionCoordinator,
        actors: &[JunctionActor],
        index_of: &HashMap<VehicleId, usize>,
        net: &Network,
    ) -> (Vec<WaitDiagnosis>, Vec<Recovery>) {
        classify_at(coord, actors, index_of, net, 0.0)
    }

    fn classify_at(
        coord: &mut JunctionCoordinator,
        actors: &[JunctionActor],
        index_of: &HashMap<VehicleId, usize>,
        net: &Network,
        time: f32,
    ) -> (Vec<WaitDiagnosis>, Vec<Recovery>) {
        let on_lane = HashMap::new();
        let coming = HashMap::new();
        let walkers = HashMap::new();
        let geo_prev = HashMap::new();
        let aspects = HashMap::new();
        let scene = JunctionScene {
            net,
            actors,
            index_of,
            on_lane: &on_lane,
            coming: &coming,
            walkers: &walkers,
            geo_prev: &geo_prev,
            aspects: &aspects,
            time,
            tick: 0,
        };
        coord.classify_waits(&scene)
    }

    fn index2() -> HashMap<VehicleId, usize> {
        let mut m = HashMap::new();
        m.insert(VehicleId(10), 0);
        m.insert(VehicleId(20), 1);
        m
    }

    #[test]
    fn block_modes_map_from_stored_values() {
        assert_eq!(BlockMode::from_raw(0), BlockMode::Occupy);
        assert_eq!(BlockMode::from_raw(1), BlockMode::Reserve);
        assert_eq!(BlockMode::from_raw(2), BlockMode::Oncoming);
        assert_eq!(BlockMode::from_raw(9), BlockMode::Occupy);
    }

    #[test]
    fn a_cyclic_speculative_claim_is_stale() {
        let mut c = JunctionCoordinator::new();
        c.restore_claim(VehicleId(10), &[0]);
        c.restore_claim(VehicleId(20), &[1]);
        c.wait_for.insert(VehicleId(10), VehicleId(20));
        c.wait_for.insert(VehicleId(20), VehicleId(10));
        let actors = vec![
            JunctionActor::new(VehicleId(10), 0, 0.0),
            JunctionActor::new(VehicleId(20), 1, 0.0),
        ];
        let net = Network::default();
        let (diagnoses, recoveries) = classify_with(&mut c, &actors, &index2(), &net);
        assert!(diagnoses
            .iter()
            .any(|d| matches!(d, WaitDiagnosis::StaleClaimCycle { .. })));
        assert!(recoveries
            .iter()
            .any(|r| matches!(r, Recovery::CancelStaleClaim { .. })));
        assert!(recoveries
            .iter()
            .any(|r| matches!(r, Recovery::RetrySafeManeuver { .. })));
    }

    #[test]
    fn a_red_held_cycle_is_legal_congestion_not_forced() {
        let mut c = JunctionCoordinator::new();
        c.restore_claim(VehicleId(10), &[0]);
        c.restore_claim(VehicleId(20), &[1]);
        c.wait_for.insert(VehicleId(10), VehicleId(20));
        c.wait_for.insert(VehicleId(20), VehicleId(10));
        let mut a = JunctionActor::new(VehicleId(10), 0, 0.0);
        a.light_hold = true;
        let actors = vec![a, JunctionActor::new(VehicleId(20), 1, 0.0)];
        let net = Network::default();
        let (diagnoses, recoveries) = classify_with(&mut c, &actors, &index2(), &net);
        assert!(diagnoses
            .iter()
            .any(|d| matches!(d, WaitDiagnosis::LegalCongestion { .. })));
        assert!(recoveries
            .iter()
            .all(|r| matches!(r, Recovery::WaitLegal { .. })));
    }

    #[test]
    fn a_cycle_without_claims_is_full_capacity() {
        let mut c = JunctionCoordinator::new();
        c.wait_for.insert(VehicleId(10), VehicleId(20));
        c.wait_for.insert(VehicleId(20), VehicleId(10));
        let actors = vec![
            JunctionActor::new(VehicleId(10), 0, 0.0),
            JunctionActor::new(VehicleId(20), 1, 0.0),
        ];
        let net = Network::default();
        let (diagnoses, _) = classify_with(&mut c, &actors, &index2(), &net);
        assert!(diagnoses
            .iter()
            .any(|d| matches!(d, WaitDiagnosis::FullCapacity { .. })));
    }

    #[test]
    fn a_cycle_through_a_vehicles_second_hold_is_found() {
        let mut c = JunctionCoordinator::new();
        // 10 waits for 20 and for 30 (the last recorded); 20 waits for 10
        c.wait_on(VehicleId(10), VehicleId(20));
        c.wait_on(VehicleId(10), VehicleId(30));
        c.wait_on(VehicleId(20), VehicleId(10));
        let actors = vec![
            JunctionActor::new(VehicleId(10), 0, 0.0),
            JunctionActor::new(VehicleId(20), 1, 0.0),
        ];
        let net = Network::default();
        let (diagnoses, _) = classify_with(&mut c, &actors, &index2(), &net);
        assert_eq!(diagnoses.len(), 1, "{diagnoses:?}");
    }

    #[test]
    fn a_standing_yield_cycle_lets_one_vehicle_go_after_a_while() {
        let mut c = JunctionCoordinator::new();
        let actors = vec![
            JunctionActor::new(VehicleId(10), 0, 0.0),
            JunctionActor::new(VehicleId(20), 1, 0.0),
        ];
        let net = Network::default();
        let mut passes = Vec::new();
        for k in 0..=10 {
            c.begin_tick(k);
            c.wait_on(VehicleId(10), VehicleId(20));
            c.wait_on(VehicleId(20), VehicleId(10));
            c.rule_waiters.insert(VehicleId(10));
            c.rule_waiters.insert(VehicleId(20));
            let (diagnoses, recoveries) = classify_at(&mut c, &actors, &index2(), &net, k as f32);
            assert!(diagnoses
                .iter()
                .all(|d| matches!(d, WaitDiagnosis::YieldCycle { .. })));
            passes.extend(recoveries.into_iter().filter_map(|r| match r {
                Recovery::GridlockPass { vehicle } => Some((k, vehicle)),
                _ => None,
            }));
        }
        assert_eq!(passes.len(), 1, "exactly one vehicle let go: {passes:?}");
        assert!(passes[0].0 as f32 >= GRIDLOCK_AFTER, "let go too early: {passes:?}");
        assert!(c.gridlock_pass.contains_key(&passes[0].1));
    }

    #[test]
    fn a_moving_member_is_no_yield_cycle() {
        let mut c = JunctionCoordinator::new();
        let mut a = JunctionActor::new(VehicleId(10), 0, 0.0);
        a.speed = 5.0;
        let actors = vec![a, JunctionActor::new(VehicleId(20), 1, 0.0)];
        let net = Network::default();
        c.wait_on(VehicleId(10), VehicleId(20));
        c.wait_on(VehicleId(20), VehicleId(10));
        c.rule_waiters.insert(VehicleId(10));
        c.rule_waiters.insert(VehicleId(20));
        let (diagnoses, _) = classify_at(&mut c, &actors, &index2(), &net, 100.0);
        assert!(diagnoses
            .iter()
            .all(|d| matches!(d, WaitDiagnosis::FullCapacity { .. })));
        assert!(c.gridlock_pass.is_empty());
    }

    #[test]
    fn a_network_invalidation_drops_every_claim() {
        let mut c = JunctionCoordinator::new();
        c.restore_claim(VehicleId(10), &[0, 1]);
        assert!(c.holds(VehicleId(10), 0));
        c.amber.insert(VehicleId(10), (0, 0));
        c.invalidate_network();
        assert!(!c.holds(VehicleId(10), 0));
        assert_eq!(c.claims_of(VehicleId(10)).len(), 0);
        assert!(c.amber.is_empty(), "a light gone through on amber outlived the network");
    }

    #[test]
    fn leaving_the_way_releases_the_claim() {
        let mut c = JunctionCoordinator::new();
        c.restore_claim(VehicleId(10), &[0, 1]);
        c.retain_on_way(VehicleId(10), &[1]);
        assert!(!c.holds(VehicleId(10), 0));
        assert!(c.holds(VehicleId(10), 1));
    }
}
