//! Junction coordinator: admission, commitments, fairness, release, and the wait-for graph.
//!
//! One owner decides who may enter a junction and why. Planning reads a frozen [`JunctionScene`]
//! and returns a [`JunctionDecision`]; it never mutates another vehicle. Commitments (the lanes
//! a vehicle has claimed and the exit storage it holds) are keyed by stable [`VehicleId`] and
//! released only on tail clearance, route change, removal, or network invalidation. A clock
//! timeout never erases a body that still occupies a conflict area.
//!
//! The content rules come first:
//! - `[crossingproblem]` (`Lane::crossing_problem`) marks a **keep-clear** path: it may not be
//!   entered (nor speculatively reserved) unless the whole movement can be cleared, so the
//!   junction is not blocked.
//! - `[blockpath] <path> <mode>` is a typed [`BlockMode`]. The reference parser only proves the
//!   two values are stored; the decision meaning below is a documented neoOMSI interpretation
//!   (Stage 5):
//!   - [`BlockMode::Occupy`] (0): the paths conflict while a body actually occupies one of them.
//!   - [`BlockMode::Reserve`] (1): a speculative reservation alone refuses the other's claim
//!     (reservation refusal).
//!   - [`BlockMode::Oncoming`] (2): the other is the oncoming/return path; this vehicle may
//!     reserve but must not enter until the other has committed (entry refusal).

use crate::diagnostics::{JunctionState, Reason};
use crate::following::{AiState, Lead, MAX_BRAKE};
use crate::ids::{LaneId, VehicleId};
use crate::network::{BlockRule, Network};
use crate::signals::Aspect;
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

/// The light that holds a vehicle at the start of `way[k]`: that lane's light, unless the
/// vehicle has already gone through a light of the same crossing object on the way there.
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
        if net.lanes[p].traffic_light.is_some() {
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

/// Seconds until a vehicle reaches a meeting place `distance` metres ahead (see
/// `core::traffic`'s original `crossing_arrival`, kept here as the domain owner).
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

/// The junction inputs for one vehicle, with the fields the admission rules read.
#[derive(Debug, Clone)]
pub struct JunctionActor {
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
}

impl JunctionActor {
    /// A minimal actor for tests and adapters.
    pub fn new(id: VehicleId, lane: usize, s: f32) -> JunctionActor {
        JunctionActor {
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
    /// The exit lane whose storage it reserved (m).
    storage: Option<usize>,
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
}

/// How a persistent hold is classified.
#[derive(Debug, Clone, PartialEq)]
pub enum WaitDiagnosis {
    /// A cyclic set of speculative claims that no member can satisfy.
    StaleClaimCycle { cycle: Vec<VehicleId> },
    /// Legal congestion: at least one member waits on a red signal.
    LegalCongestion { cycle: Vec<VehicleId> },
    /// A physically full downstream/closed loop: capacity, not a bug.
    FullCapacity { cycle: Vec<VehicleId> },
    /// A member's route changed or was removed.
    RouteRecovery { vehicle: VehicleId },
}

/// The junction coordinator: the single owner of junction commitments and the wait-for graph.
#[derive(Debug, Clone, Default)]
pub struct JunctionCoordinator {
    claims: Arbiter,
    amber: HashMap<VehicleId, (usize, usize)>,
    commitments: HashMap<VehicleId, Commitment>,
    wait_for: HashMap<VehicleId, VehicleId>,
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

    /// Rehydrate a committed claim without planning (restoring a persisted/LAN commit). The
    /// claim is released by the same tail-clearance/route/removal rules as any other.
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

    /// Who this vehicle is currently waiting behind at a junction, if anyone.
    pub fn blocked_by(&self, id: VehicleId) -> Option<VehicleId> {
        self.wait_for.get(&id).copied()
    }

    /// Begin a tick: refresh the arbiter from committed claims and clear the ephemeral exit
    /// storage (it is recomputed against this tick's realized occupancy).
    pub fn begin_tick(&mut self, _tick: u64) {
        self.claims = Arbiter::new();
        for (id, c) in &self.commitments {
            for &l in &c.lanes {
                self.claims.grant(LaneId(l), *id);
            }
        }
        self.wait_for.clear();
    }

    /// Release every claim and the exit storage of `id`, with a reason.
    pub fn release(&mut self, id: VehicleId, _reason: Reason) {
        if let Some(c) = self.commitments.remove(&id) {
            for l in c.lanes {
                self.claims.release(LaneId(l), id);
            }
            if let Some(e) = c.storage {
                self.claims.release_storage(LaneId(e), id);
            }
        }
        self.amber.remove(&id);
        self.wait_for.remove(&id);
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
            }
            empty = c.lanes.is_empty() && c.storage.is_none();
        }
        if empty {
            self.commitments.remove(&id);
            self.waiting_since.remove(&id);
        }
    }

    /// Network invalidation: every claim made against the old version is released.
    pub fn invalidate_network(&mut self) {
        self.claims = Arbiter::new();
        self.commitments.clear();
        self.wait_for.clear();
        self.waiting_since.clear();
    }

    /// The current wait-for edges (ego -> blocker).
    pub fn wait_for_graph(&self) -> &HashMap<VehicleId, VehicleId> {
        &self.wait_for
    }

    // ---- signals -----------------------------------------------------------------------

    /// The signal verdict for the movement ahead: where the vehicle has to stop (distance from
    /// its origin), or None. A vehicle that can stop comfortably stops at yellow; one too close
    /// drives on and remembers it, so the red that follows does not stop it mid-junction.
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
        for (k, &(_, d)) in way.iter().enumerate().skip(1) {
            if d > 150.0 {
                break;
            }
            let Some((c, li)) = light_at_entry(scene.net, way, k) else {
                continue;
            };
            let Some(&aspect) = scene.aspects.get(&(c, li)) else {
                continue;
            };
            let gap = d - actor.front;
            let comfortable = v * v / (2.0 * actor.decel * 1.4) + 1.0;
            let possible = v * v / (2.0 * MAX_BRAKE * 0.8);
            let go = match aspect {
                Aspect::Green | Aspect::Dark => {
                    if gap < AT_LINE {
                        amber = Some((c, li));
                    }
                    true
                }
                Aspect::Yellow | Aspect::GreenYellow => {
                    if amber == Some((c, li)) || gap < comfortable {
                        amber = Some((c, li));
                        true
                    } else {
                        false
                    }
                }
                Aspect::Red | Aspect::RedYellow => {
                    (amber == Some((c, li)) && gap < comfortable) || gap < possible - 0.5
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

    // ---- admission ---------------------------------------------------------------------

    /// Plan one vehicle's junction movement for this tick.
    ///
    /// `way` is the vehicle's own planned way (its lane sequence and distances); `lead` is what
    /// it follows now. `movement` is the junction found on that way, if any.
    pub fn plan(
        &mut self,
        scene: &JunctionScene,
        i: usize,
        way: &[(usize, f32)],
        lead: Option<Lead>,
        movement: Option<Movement>,
    ) -> JunctionDecision {
        let actor = &scene.actors[i];
        let light = self.signal_stop(scene, i, way);
        let Some(jn) = movement else {
            // no junction: any claim is behind the vehicle now
            self.drop_claims(actor.id);
            let mut d = JunctionDecision::none();
            d.light = light;
            return d;
        };
        // a junction beyond a red light's line is not decided yet
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
        let mut ruled = false;
        let mut soft: Vec<usize> = Vec::new();
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
                    if !jn.inside && point - c.before - actor.front > INSIDE_MEETING {
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
                    // The content block rule between the two paths is honored before the
                    // geometric convention. `Occupy` ignores speculative claims (only bodies
                    // conflict); `Reserve` refuses a reservation; `Oncoming` keeps the default
                    // commitment rule (a body or a committed reservation is waited for).
                    let mode = block_mode_between(scene.net, l, m);
                    let claims_block = mode != Some(BlockMode::Occupy);
                    let claimed = self.claims.holds(LaneId(m), o.id) && !stalled && claims_block;
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
                    // reservation refusal: a speculative claim alone holds this vehicle back
                    if mode == Some(BlockMode::Reserve)
                        && self.claims.holds(LaneId(m), o.id)
                        && !stalled
                    {
                        hard = true;
                        reasons.push(Reason::JunctionClaim);
                        if jn.inside {
                            stop_at = Some(stop_at.unwrap_or(f32::MAX).min(point - c.before));
                        }
                        self.wait_for.insert(me_id, o.id);
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
                            reasons.push(Reason::Yield);
                            self.wait_for.insert(me_id, o.id);
                            if jn.inside {
                                stop_at = Some(stop_at.unwrap_or(f32::MAX).min(point - c.before));
                            }
                        }
                        continue;
                    }
                    if claimed || (is_on && o.speed > 0.5 && !waits_short) {
                        let me_decided = committed || jn.inside;
                        let first = if me_decided {
                            t_j < t_mine - 0.3 || ((t_j - t_mine).abs() <= 0.3 && o.id < me_id)
                        } else {
                            true
                        };
                        if first && t_j < t_clear * if me_decided { 1.0 } else { patience } + 1.0 {
                            hard = true;
                            reasons.push(Reason::JunctionClaim);
                            self.wait_for.insert(me_id, o.id);
                            if jn.inside {
                                stop_at = Some(stop_at.unwrap_or(f32::MAX).min(point - c.before));
                            }
                        }
                        continue;
                    }
                    let o_prio = o.priority;
                    if jn.inside
                        || committed
                        || (me_prio && !o_prio)
                        || (!scene.net.must_yield(l, m) && !(o_prio && !me_prio))
                    {
                        continue;
                    }
                    if t_j < (actor.accept_gap + 2.0).max(t_clear + 1.0) * patience {
                        if t_j == f32::MAX || o.speed < 0.3 {
                            soft.push(j);
                        } else {
                            ruled = true;
                            reasons.push(Reason::Yield);
                            self.wait_for.insert(me_id, o.id);
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
                    reasons.push(Reason::Pedestrian);
                    let before = point - 2.5;
                    stop_at = Some(stop_at.unwrap_or(before).min(before));
                }
            }
        }
        let mut exit_full = false;
        if !jn.inside {
            if let Some((e, de)) = jn.exit {
                let room = scene
                    .on_lane
                    .get(&e)
                    .map(|v| v.as_slice())
                    .unwrap_or(&[])
                    .iter()
                    .filter(|x| !x.3 && x.0 != i)
                    .map(|&(j, sj, _, _)| (sj - scene.actors[j].rear, scene.actors[j].speed))
                    .fold(None::<(f32, f32)>, |acc, x| {
                        if acc.map(|a| x.0 < a.0).unwrap_or(true) {
                            Some(x)
                        } else {
                            acc
                        }
                    });
                if let Some((space, speed)) = room {
                    if speed < 1.5 && de < 40.0 {
                        self.claims.set_storage_capacity(LaneId(e), space);
                        let need = actor.length + actor.min_gap;
                        if space < need || !self.claims.reserve_storage(LaneId(e), me_id, need) {
                            ruled = true;
                            exit_full = true;
                            reasons.push(Reason::OccupiedExit);
                        } else if let Some(c) = self.commitments.get_mut(&me_id) {
                            c.storage = Some(e);
                        }
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
        let mut blocked =
            (hard && !cannot_stop) || ((ruled || !soft.is_empty()) && !cannot_stop_gently);
        if !hard && !ruled && !soft.is_empty() && wait > 2.5 + actor.reaction {
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
            let old = self.commitments.remove(&me_id).map(|c| c.lanes).unwrap_or_default();
            for l in old {
                self.claims.release(LaneId(l), me_id);
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
            let old = self.commitments.remove(&me_id).map(|c| c.lanes).unwrap_or_default();
            for l in old {
                self.claims.release(LaneId(l), me_id);
            }
            return (None, reasons, JunctionState::Approaching);
        }
        // claim the way through
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
        c.state = if jn.inside {
            JunctionState::Inside
        } else {
            JunctionState::Admitted
        };
        self.waiting_since.remove(&me_id);
        (None, reasons, c.state)
    }

    fn drop_claims(&mut self, id: VehicleId) {
        if let Some(c) = self.commitments.remove(&id) {
            for l in c.lanes {
                self.claims.release(LaneId(l), id);
            }
            if let Some(e) = c.storage {
                self.claims.release_storage(LaneId(e), id);
            }
        }
    }

    fn note_wait(&mut self, id: VehicleId, tick: u64) {
        self.waiting_since.entry(id).or_insert(tick);
    }

    // ---- wait-for graph -----------------------------------------------------------------

    /// Classify persistent holds. A cyclic set of vehicles each waiting on the next, all
    /// stationary and holding speculative claims but none physically in a conflict, is a
    /// stale-claim deadlock; a cycle that includes a red signal or a physically full
    /// downstream is legal and must not be forced.
    pub fn classify_waits(
        &mut self,
        scene: &JunctionScene,
    ) -> (Vec<WaitDiagnosis>, Vec<Recovery>) {
        let mut out = Vec::new();
        let mut seen: HashSet<VehicleId> = HashSet::new();
        let ids: Vec<VehicleId> = self.wait_for.keys().copied().collect();
        for start in ids {
            if seen.contains(&start) {
                continue;
            }
            let mut path: Vec<VehicleId> = Vec::new();
            let mut on_path: HashSet<VehicleId> = HashSet::new();
            let mut cur = start;
            loop {
                if on_path.contains(&cur) {
                    let at = path.iter().position(|&x| x == cur).unwrap_or(0);
                    let cycle: Vec<VehicleId> = path[at..].to_vec();
                    if cycle.len() >= 2 {
                        for &id in &cycle {
                            seen.insert(id);
                        }
                        out.push(self.classify_cycle(scene, &cycle));
                    }
                    break;
                }
                if path.len() > self.wait_for.len() + 1 {
                    break;
                }
                on_path.insert(cur);
                path.push(cur);
                match self.wait_for.get(&cur) {
                    Some(&n) => cur = n,
                    None => break,
                }
            }
        }
        let mut recoveries = Vec::new();
        for d in &out {
            match d {
                WaitDiagnosis::StaleClaimCycle { cycle } => {
                    // deterministic order: the lowest id keeps the first safe retry, the rest
                    // cancel their speculative claims
                    let mut sorted = cycle.clone();
                    sorted.sort_unstable_by_key(|id| {
                        (self.waiting_since.get(id).copied().unwrap_or(u64::MAX), *id)
                    });
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
                        }
                        recoveries.push(Recovery::CancelStaleClaim { vehicle: id, lanes });
                    }
                    if let Some(&id) = sorted.first() {
                        recoveries.push(Recovery::RetrySafeManeuver { vehicle: id });
                    }
                }
                WaitDiagnosis::LegalCongestion { cycle } => {
                    for &id in cycle {
                        recoveries.push(Recovery::WaitLegal { vehicle: id });
                    }
                }
                WaitDiagnosis::FullCapacity { cycle } => {
                    for &id in cycle {
                        recoveries.push(Recovery::WaitLegal { vehicle: id });
                    }
                }
                WaitDiagnosis::RouteRecovery { vehicle } => {
                    recoveries.push(Recovery::RequestRouteRecovery { vehicle: *vehicle });
                }
            }
        }
        (out, recoveries)
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
        WaitDiagnosis::FullCapacity {
            cycle: cycle.to_vec(),
        }
    }
}

/// The controller `AiState` of an actor, for the crossing-arrival prediction. Junction actors
/// carry their own fields; this reconstructs a minimal state from them.
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

    fn classify_with(
        coord: &mut JunctionCoordinator,
        actors: &[JunctionActor],
        index_of: &HashMap<VehicleId, usize>,
        net: &Network,
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
            time: 0.0,
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
    fn a_network_invalidation_drops_every_claim() {
        let mut c = JunctionCoordinator::new();
        c.restore_claim(VehicleId(10), &[0, 1]);
        assert!(c.holds(VehicleId(10), 0));
        c.invalidate_network();
        assert!(!c.holds(VehicleId(10), 0));
        assert_eq!(c.claims_of(VehicleId(10)).len(), 0);
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
