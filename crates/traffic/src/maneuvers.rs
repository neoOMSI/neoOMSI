//! Lateral maneuvers: lane changes, merges, overtaking, passing, parking, pull-out and the
//! lateral half of bus docking.
//!
//! This is the L4 owner of every lateral intent on the road, mirroring `traffic::junctions`
//! and `traffic::service`. Conflicting behaviors (a route-required change, an overtake, a
//! kerb swerve round a parked car, a bus docking, a park) no longer overwrite
//! `AiState::lateral_target` in different functions: they submit requests, and one
//! [`ManeuverCoordinator`] reads a frozen [`ManeuverScene`] and returns a typed
//! [`ManeuverDecision`] that becomes the single writer of lateral intent.
//!
//! Realization stays in `simulation::ai_motion` (steering, articulation, ground contact).
//! The domain expresses a maneuver as the same primitives the realization already consumes:
//! a lateral target with an S-curve ramp, a committed lane change, a blinker and a stop
//! point. The domain never integrates pose; it reads the realized odometer each tick, so a
//! maneuver has one integrator (the body) and the ramps are recomputed, not re-integrated.
//!
//! Priorities, highest first:
//! 1. safety/allowed space (never start a maneuver whose whole trajectory, including the
//!    return, is not clear) and finishing/aborting a committed maneuver;
//! 2. required maneuvers (a route-required change, docking/departure, a committed park);
//! 3. discretionary maneuvers (overtaking, keeping to the correct lane);
//! 4. optional maneuvers (passing a standing obstruction) - waiting is correct when the
//!    whole outbound+return trajectory cannot be checked clear.
//!
//! ## Tuning provenance
//!
//! The constants below have their unit in the name and their rationale on each item. Their
//! provenance is one of: **content** (imported from map/vehicle data), **observed** (OMSI
//! compatibility), **improvement** (a deliberate neoOMSI behaviour change) or **provisional**
//! (unverified tuning). The maneuver set is **improvement/provisional** (plan section 6) except
//! where a content length is passed in. The full table is in
//! `docs/traffic_refactor/MAINTAINER_GUIDE.md`.

use crate::diagnostics::{ManeuverPhase, Reason};
use crate::following::{arrival_time, ramp_progress_for, smooth01};
use crate::ids::{LaneId, VehicleId};
use crate::network::{LaneKind, Network};
use crate::perception::{Occupancy, SweepSample};
use glam::{DVec2, DVec3};
use hashbrown::HashMap;

// ---- named maneuver geometry and policy (units always in the name) --------------------

/// Clearance a pulling-out body must keep to the obstacle it steers round (m).
pub const PULL_OUT_CLEARANCE: f64 = 0.25;
/// A rolling car watches the wheel-turn phase of a pull-out for this long (s).
pub const PULL_OUT_WAIT: f32 = 0.4;
/// Acceleration while edging out from a standstill (m/s²).
pub const PULL_OUT_ACCEL: f32 = 1.0;
/// Distance of the short transition onto a parallel passing lane (m).
pub const BYPASS_RAMP: f32 = 6.0;
/// Sideways acceleration the return S-curve keeps within (m/s²).
pub const BACK_IN_LAT_ACCEL: f32 = 2.5;
/// A car edging out keeps to `PULL_OUT_ACCEL` until its front is this far past the obstacle
/// rear (m).
pub const CREEP_PAST: f32 = 2.0;
/// Room an oncoming vehicle needs beside a passing car (m).
pub const ONCOMING_ROOM: f32 = 1.15;
/// Seconds of indicating before a discretionary change begins to move over (s).
pub const SIGNAL_BEFORE_CHANGE: f32 = 1.2;
/// A lane change that has just finished may not start another for this long (s).
pub const CHANGE_COOLDOWN: f32 = 6.0;
/// A wish for a discretionary change must persist this long before it commits (s), so a
/// flickering local condition cannot make a car jerk between lanes.
pub const DISCRETIONARY_DWELL: f32 = 0.6;
/// A discretionary change opposite to the last one is discouraged for this long (s).
pub const OSCILLATION_WINDOW: f32 = 8.0;
/// How far ahead a turn lane is entered (m).
pub const TURN_LANE_LOOKAHEAD: f32 = 150.0;
/// A stopped corner-clearance ramp (m). Provisional improvement: lets the wheels
/// turn toward nearby clearance while realization still rejects every collision.
pub const CORNER_RECOVERY_RAMP: f32 = 2.0;

// ---- pure ramp geometry (also used by the realization) --------------------------------

/// S-curve lengths (m) a car should try when pulling out round an obstacle `real` metres
/// ahead: gentlest first, bounded by its own length and room.
pub fn pull_out_ramps(real: f32, front: f32, rolling: bool) -> Vec<f32> {
    let (factors, max): (&[f32], f32) = if rolling {
        (&[1.0, 0.8, 0.6, 0.45], 20.0)
    } else {
        (&[1.5, 1.2, 1.0, 0.8, 0.6], 12.0)
    };
    let base = real + 0.5 * front;
    let mut out: Vec<f32> = Vec::new();
    for &f in factors {
        let len = (base * f).clamp(4.0, max);
        if out.last().map(|l| (l - len).abs() > 0.3).unwrap_or(true) {
            out.push(len);
        }
    }
    out
}

/// Length (m) of an S-curve moving sideways by `offset` m at `speed` m/s that keeps the peak
/// sideways acceleration within `lat_accel`: `max(geometric, dynamic)`, capped at 60.
pub fn back_in_ramp(offset: f32, speed: f32, lat_accel: f32) -> f32 {
    let off = offset.abs();
    let geometric = (off * 3.5).clamp(8.0, 14.0);
    let dynamic = speed * (6.0 * off / lat_accel.max(0.5)).sqrt();
    geometric.max(dynamic).min(60.0)
}

/// A straight pull-out path: `side` is +1 to the left / -1 to the right of the way, `ramp`
/// the S-curve length in metres. Returns the sideways offset at distance `d`.
pub fn straight_pull_out(side: f32, ramp: f32) -> impl Fn(f32) -> f32 {
    move |d: f32| -side * smooth01((d / ramp.max(0.1)).clamp(0.0, 1.0))
}

/// Seconds a car at `speed` needs to drive `dist` metres out onto the other half: the first
/// `creep` metres edging out, the rest speeding up to `v_cap`.
pub fn pass_time(
    dist: f32,
    creep: f32,
    speed: f32,
    reaction: f32,
    accel: f32,
    v_cap: f32,
) -> f32 {
    let wait = if speed < 0.1 { reaction } else { 0.0 };
    let accel = accel * 0.85;
    if creep <= 0.0 || dist <= 0.0 {
        return wait + arrival_time(dist, speed, accel, v_cap);
    }
    let a0 = accel.min(PULL_OUT_ACCEL);
    let first = creep.min(dist);
    let v1 = (speed * speed + 2.0 * a0 * first)
        .sqrt()
        .min(v_cap.max(speed));
    wait + arrival_time(first, speed, a0, v_cap) + arrival_time(dist - first, v1, accel, v_cap)
}

// ---- maneuver lifecycle state ----------------------------------------------------------

/// A car pulling out onto the other half of the road round something standing in its lane.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Passing {
    /// The oncoming lane it moves over onto.
    pub lane: usize,
    /// How far to the left that lane lies (m).
    pub side: f32,
    /// Odometer reading past which it moves back.
    pub until: f32,
    /// Odometer reading at which its front would reach the obstacle.
    pub block: f32,
    /// Length of the S-curve back into the lane (m).
    pub back: f32,
    /// Given up because somebody came the other way.
    pub aborted: bool,
    pub hold: f32,
    /// Started from a standstill: it edges out at `PULL_OUT_ACCEL`.
    pub creep: bool,
}

impl Passing {
    /// Odometer reading at which the car is far enough back to be out of the oncoming lane.
    pub fn clear_at(&self, half_width: f32) -> f32 {
        self.until
            + self.back * ramp_progress_for(self.side, half_width + ONCOMING_ROOM)
    }
}

/// A free parking space beside a lane a car means to park in.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct ParkPlan {
    pub key: i64,
    pub lane: usize,
    /// The space's middle along the lane and its offset to the right (m).
    pub s: f32,
    pub lat: f32,
    pub ramped: bool,
    pub done: bool,
}

/// The per-vehicle maneuvering memory, written only by [`ManeuverCoordinator::plan`].
#[derive(Debug, Clone, Default)]
pub struct ManeuverState {
    /// Seconds before a new lane change may start.
    pub change_cooldown: f32,
    /// Not before this delay after a failed pull-out may it try again.
    pub pass_retry: f32,
    /// The side (1 left, 2 right) of the last completed discretionary change.
    pub last_side: i32,
    /// Time of the last completed lane change (s), for the oscillation guard.
    pub last_change_time: f32,
    /// The lane change currently committed (its target and side), for completion detection.
    pub change_to: Option<usize>,
    pub change_dir: i32,
    pub passing: Option<Passing>,
    pub park: Option<ParkPlan>,
    /// Seconds a parked car still holds before pulling out.
    pub pull_out: f32,
    /// How long the current discretionary wish has been held (s) and which wish it is.
    pub dwell: f32,
    pub dwell_code: i16,
    /// Committed courtesy trajectory; keep its odometer anchor while yielding.
    pub emergency_ramp: Option<(f32, f32, f32, f32)>,
    /// A stopped vehicle's corner-clearance trajectory, anchored until it clears
    /// the parked row. Re-anchoring it every tick prevents steering recovery.
    pub kerb_ramp: Option<(f32, f32, f32, f32)>,
}

// ---- frozen per-tick scene and inputs --------------------------------------------------

/// One planned lane of a car's way: the network lane and the distance from the car origin to
/// its start (the first is `(current lane, -s)`).
pub type WayStep = (usize, f32);

/// The realized state of one vehicle a maneuver plan reads.
#[derive(Debug, Clone)]
pub struct ManeuverActor {
    pub id: VehicleId,
    pub lane: usize,
    pub s: f32,
    pub lateral: f32,
    pub speed: f32,
    pub accel: f32,
    pub decel: f32,
    pub reaction: f32,
    pub desire: f32,
    pub max_speed_kmh: f32,
    pub front: f32,
    pub rear: f32,
    pub length: f32,
    pub half_width: f32,
    /// Ground to roof envelope for authored scenery clearance.
    pub height: f32,
    pub odometer: f32,
    pub min_gap: f32,
    pub veh_type: i32,
    pub lane_kind: LaneKind,
    /// The lane the way goes to next (planned), if any.
    pub planned_next: Option<usize>,
    /// The lane the fixed route requires next, if it lies beside this one.
    pub route_next: Option<usize>,
    pub turn_wish: i32,
    /// The lane change in progress, if any.
    pub change: Option<ChangeInfo>,
    pub stopped: f32,
    pub light_hold: bool,
    pub yielding: bool,
    pub at_stop: bool,
    /// Room the vehicle's own steering needs behind an obstacle before it can pull out (m).
    pub pass_room: f32,
    pub lat_accel: f32,
}

impl ManeuverActor {
    /// A minimal actor for tests and adapters.
    pub fn new(id: VehicleId, lane: usize, s: f32) -> ManeuverActor {
        ManeuverActor {
            id,
            lane,
            s,
            lateral: 0.0,
            speed: 0.0,
            accel: 1.5,
            decel: 2.5,
            reaction: 0.7,
            desire: 1.0,
            max_speed_kmh: 50.0,
            front: 2.25,
            rear: 2.25,
            length: 4.5,
            half_width: 1.25,
            height: 3.5,
            odometer: s,
            min_gap: 2.0,
            veh_type: 0,
            lane_kind: LaneKind::Street,
            planned_next: None,
            route_next: None,
            turn_wish: 0,
            change: None,
            stopped: 0.0,
            light_hold: false,
            yielding: false,
            at_stop: false,
            pass_room: 6.0,
            lat_accel: 2.8,
        }
    }

    pub fn index(id: VehicleId) -> usize {
        id.get() as usize
    }
}

/// A lane change in progress, as the realization holds it.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct ChangeInfo {
    pub to: usize,
    pub dir: i32,
    pub t: f32,
    pub length: f32,
    pub s_to: f32,
    pub wait: f32,
    pub bypass: bool,
}

/// The kind of lane change to begin.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ChangeKind {
    /// A discretionary change (overtake/keep to the correct lane).
    Change,
    /// A change the fixed route requires.
    RouteChange,
    /// A short, steep pull-out from behind a standing obstacle.
    Bypass,
}

/// A lane change the coordinator decided the vehicle should begin this tick.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ChangeCommand {
    pub to: usize,
    pub dir: i32,
    pub kind: ChangeKind,
}

impl ChangeCommand {
    pub fn new(to: usize, dir: i32, kind: ChangeKind) -> ChangeCommand {
        ChangeCommand { to, dir, kind }
    }
}

/// The frozen world a maneuver plan reads. Built once per tick.
pub struct ManeuverScene<'a> {
    /// Engine integration checks the full vehicle sweep against authored scenery meshes.
    /// Pure domain fixtures may omit it; the production adapter supplies it every tick.
    pub static_clearance: Option<&'a dyn Fn(&[SweepSample], &ManeuverActor) -> bool>,
    pub net: &'a Network,
    pub occupancy: &'a Occupancy,
    pub actors: &'a [ManeuverActor],
    /// Pedestrians near the road (world positions), for the whole-maneuver check.
    pub people: &'a [DVec2],
    pub time: f32,
    pub dt: f32,
    pub tick: u64,
}

/// One vehicle's maneuver inputs for this tick: the requests other owners submit.
#[derive(Debug, Clone, Copy)]
pub struct ManeuverInputs {
    pub emergency: Option<crate::emergency::EmergencyApproach>,
    pub priority_pass: bool,
    /// A scheduled bus queues for its own occupied stop instead of passing that queue.
    pub queue_for_stop: bool,
    /// Index into `ManeuverScene::actors`.
    pub actor: usize,
    /// A required lateral target from the service owner (docking/merge-out) and its phase.
    pub service_lateral: Option<f32>,
    pub service_phase: ManeuverPhase,
    /// A safety swerve from the world (a parked car at the kerb), or the service `kerb_swerve`.
    pub kerb_swerve: Option<f32>,
    /// The vehicle is a parked car that has just been put on the road to pull out.
    pub pull_out: bool,
    /// The gap from the front bumper to a standing/slow obstruction in its own lane (m).
    pub lead_gap: Option<f32>,
    /// Whether that obstruction is standing (a pass is possible) or just a slow leader.
    pub lead_standing: bool,
    /// The obstruction's length (m).
    pub obstacle_len: f32,
    /// The obstruction is a parked car (the driver may pull out while still rolling).
    pub parked: bool,
}

impl ManeuverInputs {
    /// A minimal input for tests and adapters.
    pub fn new(actor: usize) -> ManeuverInputs {
        ManeuverInputs {
            emergency: None,
            priority_pass: false,
            queue_for_stop: false,
            actor,
            service_lateral: None,
            service_phase: ManeuverPhase::Idle,
            kerb_swerve: None,
            pull_out: false,
            lead_gap: None,
            lead_standing: false,
            obstacle_len: 0.0,
            parked: false,
        }
    }
}

/// The coordinator's decision for one vehicle in one tick: the only writer of lateral intent.
#[derive(Debug, Clone, PartialEq)]
pub struct ManeuverDecision {
    pub phase: ManeuverPhase,
    /// Lateral target across the lane (m, positive = right), if it changes.
    pub lateral_target: Option<f32>,
    /// An explicit S-curve `(from, to, odometer_at_start, length)` when the maneuver needs
    /// one steeper or gentler than the default.
    pub lateral_ramp: Option<(f32, f32, f32, f32)>,
    /// A lane change to begin.
    pub change: Option<ChangeCommand>,
    /// An indicator the driver holds for itself, `(blinker, seconds)`.
    pub signal: Option<(i32, f32)>,
    /// Bounds on the longitudinal command for this maneuver.
    pub accel_cap: Option<f32>,
    /// A stop point the maneuver itself requires (distance from the origin, m).
    pub stop_at: Option<f32>,
    /// The lane a committed change is moving to (for diagnostics).
    pub target_lane: Option<LaneId>,
    pub reasons: Vec<Reason>,
    pub binding: Option<Reason>,
}

impl ManeuverDecision {
    fn new(phase: ManeuverPhase) -> ManeuverDecision {
        ManeuverDecision {
            phase,
            lateral_target: None,
            lateral_ramp: None,
            change: None,
            signal: None,
            accel_cap: None,
            stop_at: None,
            target_lane: None,
            reasons: Vec::new(),
            binding: None,
        }
    }
}

/// A vehicle's wish to move to a lane this tick, so the coordinator can order simultaneous
/// changes by stable id rather than by container position.
#[derive(Debug, Clone, Copy)]
pub struct ManeuverIntent {
    pub vehicle: VehicleId,
    pub target: Option<LaneId>,
    pub required: bool,
    pub s: f32,
}

const CLAIM_SPAN: f32 = 40.0;

// ---- the coordinator -------------------------------------------------------------------

/// The single owner of lane-change commitments and the lateral decision.
#[derive(Debug, Clone, Default)]
pub struct ManeuverCoordinator {
    /// This tick's approved lane change per vehicle (`vehicle -> target lane`).
    approved: HashMap<VehicleId, LaneId>,
    /// The lowest id that asked for each target lane this tick (the merge order).
    claimants: HashMap<LaneId, Vec<(VehicleId, f32)>>,
    tick: u64,
}

impl ManeuverCoordinator {
    pub fn new() -> ManeuverCoordinator {
        ManeuverCoordinator::default()
    }

    /// The lane change approved for `id` this tick, if any.
    pub fn approved(&self, id: VehicleId) -> Option<LaneId> {
        self.approved.get(&id).copied()
    }

    /// Submit optional changes through the same arbitration as route-required changes.
    /// Discovery only reads the frozen scene; trajectory validation happens at commitment.
    pub fn intent(&self, scene: &ManeuverScene, actor: &ManeuverActor, state: &ManeuverState) -> ManeuverIntent {
        let required = actor.change.map(|c| c.to).or_else(||
            if actor.at_stop { None } else { required_target(scene.net, actor) });
        let target = required.or_else(|| self.discretionary_wish(scene, actor, state).map(|w| w.0));
        let s = target.map(|t| scene.net.beside_s(actor.lane, t, actor.s)).unwrap_or(actor.s);
        ManeuverIntent { vehicle: actor.id, target: target.map(LaneId), required: required.is_some(), s }
    }

    /// How many vehicles hold an approved lane change this tick (a health/leak check).
    pub fn active_count(&self) -> usize {
        self.approved.len()
    }

    /// Begin a tick: order the submitted lane-change wishes by target lane and stable id, so
    /// two vehicles moving into the same lane do not both start. The lowest id wins; the
    /// same inputs decide the same way whatever the storage order.
    pub fn begin_tick(&mut self, intents: &[ManeuverIntent], tick: u64) {
        self.tick = tick;
        self.approved.clear();
        self.claimants.clear();
        let mut ordered: Vec<_> = intents.iter().collect();
        ordered.sort_by_key(|it| (!it.required, it.vehicle));
        for it in ordered {
            let Some(t) = it.target else { continue };
            let list = self.claimants.entry(t).or_default();
            if list.iter().any(|&(_, s)| (s - it.s).abs() < CLAIM_SPAN) {
                continue;
            }
            list.push((it.vehicle, it.s));
            self.approved.insert(it.vehicle, t);
        }
    }

    /// Release the committed maneuver of a vehicle (removal, route change, reset).
    pub fn release(&mut self, id: VehicleId) {
        self.approved.remove(&id);
    }

    /// Approve the lateral request the service owner submitted (docking or merge-out). The
    /// service lateral is a required maneuver, so it wins over any discretionary intent while
    /// the berth is held; the service owner still decides the phase and the berth.
    pub fn service_lateral(&self, lateral: f32, phase: ManeuverPhase) -> ManeuverDecision {
        let mut d = ManeuverDecision::new(phase);
        d.lateral_target = Some(lateral);
        d
    }

    /// A route change drops a committed change whose target no longer lies on the way.
    pub fn retain_on_way(&mut self, id: VehicleId, way: &[usize]) {
        if let Some(l) = self.approved.get(&id) {
            if !way.contains(&l.index()) {
                self.approved.remove(&id);
            }
        }
    }

    /// Network invalidation: every commitment made against the old version is released.
    pub fn invalidate_network(&mut self) {
        self.approved.clear();
        self.claimants.clear();
    }

    // ---- planning ----------------------------------------------------------------------

    /// Plan one vehicle's lateral maneuver for this tick. `state` is the single per-vehicle
    /// memory the coordinator writes; `input` carries the requests submitted for it.
    pub fn plan(
        &mut self,
        scene: &ManeuverScene,
        state: &mut ManeuverState,
        input: &ManeuverInputs,
    ) -> ManeuverDecision {
        let dt = scene.dt;
        state.change_cooldown = (state.change_cooldown - dt).max(0.0);
        // pass_retry is an absolute simulation deadline, not a countdown.
        state.dwell = (state.dwell - dt).max(0.0);
        let Some(actor) = scene.actors.get(input.actor) else {
            return ManeuverDecision::new(ManeuverPhase::Idle);
        };
        let id = actor.id;
        if input.emergency.is_none() {
            state.emergency_ramp = None;
        }
        if input.kerb_swerve.is_none() { state.kerb_ramp = None; }

        // A committed lane change finished or was cancelled: start its cooldown.
        if state.change_to.is_some() && actor.change.is_none() {
            let completed = actor.lane == state.change_to.unwrap_or(usize::MAX);
            if completed {
                state.change_cooldown = CHANGE_COOLDOWN;
                state.last_side = state.change_dir;
                state.last_change_time = scene.time;
            }
            state.change_to = None;
        }

        // A lane change in progress: let it finish (the realization owns its progress).
        if let Some(c) = actor.change {
            // The target's lane index advances too when a change crosses a spline joint.
            state.change_to = Some(c.to);
            let mut d = ManeuverDecision::new(ManeuverPhase::LaneChange);
            d.target_lane = Some(LaneId(c.to));
            if c.bypass { d.accel_cap = Some(PULL_OUT_ACCEL); }
            return d;
        }

        // Parking arrival is a committed maneuver: finish it before anything else.
        if let Some(plan) = state.park {
            return self.plan_parking(scene, actor, state, plan, input);
        }

        // A parked car pulling out: wait, then merge into the lane.
        if input.pull_out {
            state.pull_out = 2.0 + (id.get() % 1000) as f32 / 400.0;
        }
        if state.pull_out > 0.0 {
            state.pull_out = (state.pull_out - dt).max(0.0);
            let mut d = ManeuverDecision::new(ManeuverPhase::PullOut);
            d.lateral_target = Some(0.0);
            d.accel_cap = Some(PULL_OUT_ACCEL);
            d.stop_at = Some(actor.front + 0.1);
            d.reasons.push(Reason::PullOut);
            d.binding = Some(Reason::PullOut);
            return d;
        }

        // A pass already underway: finish or abort it (never snap laterally).
        if let Some(d) = self.plan_passing_active(scene, actor, state) {
            return d;
        }

        // Required service lateral (docking or merge-out): the service owner submits it.
        if input.service_lateral.is_some() {
            let mut d = ManeuverDecision::new(input.service_phase);
            d.lateral_target = input.service_lateral;
            return d;
        }

        // Required route-required change (and the turn lane the way asks for).
        if input.emergency.is_some() && !actor.at_stop {
            return self.yield_to_emergency(scene, actor, state, input);
        }
        if let Some(d) = self.plan_required_change(scene, actor, state) {
            return d;
        }
        if input.priority_pass {
            if let Some(decision) = self.emergency_corridor(scene, actor, state) {
                return decision;
            }
        }

        // A safety swerve round a parked/standing body at the kerb.
        if let Some(lat) = input.kerb_swerve {
            if lat.abs() > 0.3 || state.kerb_ramp.is_some()
                || (actor.speed < 0.1 && actor.stopped > 1.0 && (lat - actor.lateral).abs() > 0.1)
            {
                let mut d = ManeuverDecision::new(ManeuverPhase::Idle);
                if state.kerb_ramp.is_some_and(|r| r.1.signum() != lat.signum()) {
                    state.kerb_ramp = None;
                }
                if let Some(ramp) = state.kerb_ramp.as_mut() {
                    if lat.abs() > ramp.1.abs() + 0.1 { ramp.1 = lat; }
                }
                if let Some(ramp) = state.kerb_ramp.filter(|r| r.1.signum() == lat.signum()) {
                    d.lateral_target = Some(ramp.1);
                    d.lateral_ramp = Some(ramp);
                } else if actor.speed < 0.1 && actor.stopped > 1.0 {
                    let ramp = (actor.lateral, lat, actor.odometer, CORNER_RECOVERY_RAMP);
                    state.kerb_ramp = Some(ramp);
                    d.lateral_target = Some(lat);
                    d.lateral_ramp = Some(ramp);
                } else { d.lateral_target = Some(lat); }
                d.reasons.push(Reason::Parking);
                return d;
            }
        }

        // A bus waiting for its own berth keeps its place after required route changes.
        if input.queue_for_stop {
            return ManeuverDecision::new(ManeuverPhase::Idle);
        }
        // Discretionary change (overtake/keep to the correct lane).
        if let Some(d) = self.plan_discretionary(scene, actor, state) {
            return d;
        }

        // Optional: pull out round something standing, with the whole trajectory checked.
        if let Some(d) = self.plan_passing(scene, actor, state, input) {
            return d;
        }

        // Nothing lateral to do: keep to the middle of the lane, or hold the kerb swerve the
        // world asked for (so a swerve that is no longer needed is given up).
        let mut d = ManeuverDecision::new(ManeuverPhase::Idle);
        d.lateral_target = Some(input.kerb_swerve.unwrap_or(0.0));
        d
    }

    fn yield_to_emergency(&self, scene: &ManeuverScene, actor: &ManeuverActor, state: &mut ManeuverState, input: &ManeuverInputs) -> ManeuverDecision {
        let lane = &scene.net.lanes[actor.lane];
        // On a multilane road the innermost lane goes inward and the remaining lanes
        // outward. On a single lane move to the curb without leaving the authored road.
        let side = if scene.net.left_hand {
            if lane.right.is_none() && lane.left.is_some() { 1.0 } else { -1.0 }
        } else if lane.left.is_none() && lane.right.is_some() { -1.0 } else { 1.0 };
        let target = side * (lane.width * 0.5 - actor.half_width - 0.15).max(0.0);
        // The body steers as it rolls. Braking a stationary queue to zero while asking
        // for an odometer-based lateral ramp can never open a rescue corridor.
        let room = input.lead_gap.map(|gap| (gap - actor.min_gap).max(0.0));
        let ramp = (actor.speed * 2.0).max(2.0).min(room.unwrap_or(f32::MAX).max(1.0));
        let trajectory = state.emergency_ramp.filter(|r| (r.1 - target).abs() < 0.01)
            .unwrap_or((actor.lateral, target, actor.odometer, ramp));
        let remaining = (trajectory.2 + trajectory.3 - actor.odometer).max(0.0);
        let lat = |d: f32| trajectory.0 + (target - trajectory.0)
            * smooth01((actor.odometer + d - trajectory.2) / trajectory.3);
        let mut decision = ManeuverDecision::new(ManeuverPhase::Idle);
        if self.sweep_clear(scene, actor, &lat, remaining, PULL_OUT_CLEARANCE) {
            decision.lateral_target = Some(target);
            state.emergency_ramp = Some(trajectory);
            decision.lateral_ramp = Some(trajectory);
            decision.signal = Some((if side > 0.0 { 2 } else { 1 }, 1.5));
        }
        // Allow a walking-speed roll while moving aside, retaining every ordinary
        // leader and signal constraint. Once aside, wait for the emergency to pass.
        let moving_aside = decision.lateral_target.is_some() && (target - actor.lateral).abs() > 0.1;
        let creep = if moving_aside { 1.0 } else { 0.0 };
        decision.accel_cap = Some(((creep - actor.speed) / 1.5).clamp(-actor.decel, 0.6));
        decision.reasons.push(Reason::EmergencyYield);
        decision.binding = Some(Reason::EmergencyYield);
        decision
    }

    // ---- route-required / turn-lane changes --------------------------------------------

    /// Use the gap between the innermost lane and its neighbour once their realized
    /// bodies have made room. Ordinary following still protects every physical gap.
    fn emergency_corridor(&self, scene: &ManeuverScene, actor: &ManeuverActor, state: &ManeuverState) -> Option<ManeuverDecision> {
        if actor.lane_kind != LaneKind::Street || actor.at_stop || state.passing.is_some() {
            return None;
        }
        let net = scene.net;
        let lane = &net.lanes[actor.lane];
        let inner = |l: &crate::network::Lane| if net.left_hand { l.right } else { l.left };
        let outer = |l: &crate::network::Lane| if net.left_hand { l.left } else { l.right };
        let beside = match inner(lane) {
            Some(n) if inner(&net.lanes[n]).is_none() => n,
            None => outer(lane)?,
            _ => return None, // first take a normal safe change toward the inner lanes
        };
        let s = net.beside_s(actor.lane, beside, actor.s);
        let (p, heading) = lane.at(actor.s);
        let q = net.lanes[beside].at(s).0;
        let h = (heading as f64).to_radians();
        let target = ((q - p).truncate().dot(DVec2::new(h.cos(), -h.sin())) * 0.5) as f32;
        let ramp = (actor.speed * 3.0).max(12.0);
        let lat = |d: f32| actor.lateral + (target - actor.lateral) * smooth01(d / ramp);
        if !self.sweep_clear(scene, actor, &lat, ramp + actor.front, PULL_OUT_CLEARANCE) {
            return None;
        }
        let mut decision = ManeuverDecision::new(ManeuverPhase::Idle);
        decision.lateral_target = Some(target);
        Some(decision)
    }

    fn plan_required_change(
        &mut self,
        scene: &ManeuverScene,
        actor: &ManeuverActor,
        state: &mut ManeuverState,
    ) -> Option<ManeuverDecision> {
        let net = scene.net;
        if actor.at_stop { return None; }
        let to = required_target(net, actor)?;
        let kind = if actor.route_next == Some(to) {
            ChangeKind::RouteChange
        } else {
            ChangeKind::Change
        };
        let s_to = net.beside_s(actor.lane, to, actor.s.min(net.lanes[actor.lane].length()));
        let dir = side_of(net, actor.lane, to);
        // (waiting for a gap, the car stops before the lane the map closes to it, which may
        // lie a spline joint or two ahead, else before the end of its lane)
        let wait_at = closed_ahead(net, actor.lane, actor.s, actor.veh_type, actor.planned_next)
            .filter(|_| actor.lane_kind == LaneKind::Street)
            .unwrap_or(net.lanes[actor.lane].length() - actor.s);
        Some(self.commit_or_wait(scene, actor, state, to, dir, kind, s_to, wait_at))
    }

    /// Start the change the way requires, or wait legally before the end of the lane.
    #[allow(clippy::too_many_arguments)]
    fn commit_or_wait(
        &mut self,
        scene: &ManeuverScene,
        actor: &ManeuverActor,
        state: &mut ManeuverState,
        to: usize,
        dir: i32,
        kind: ChangeKind,
        s_to: f32,
        wait_at: f32,
    ) -> ManeuverDecision {
        let approved = self.approved(actor.id) == Some(LaneId(to));
        if approved && self.can_merge(scene, actor, to, s_to) {
            state.change_to = Some(to);
            state.change_dir = dir;
            let mut d = ManeuverDecision::new(match kind {
                ChangeKind::RouteChange => ManeuverPhase::RouteChange,
                _ => ManeuverPhase::LaneChange,
            });
            d.change = Some(ChangeCommand::new(to, dir, kind));
            d.target_lane = Some(LaneId(to));
            return d;
        }
        // Not now: indicate and wait before the end of the lane (a legal wait outcome, never
        // cutting through the queue or jumping to another lane).
        let mut d = ManeuverDecision::new(ManeuverPhase::RouteChange);
        d.signal = Some((dir, 1.0));
        d.stop_at = Some((wait_at - 1.0).max(0.0));
        d.reasons.push(Reason::Yield);
        d.binding = Some(Reason::Yield);
        d.target_lane = Some(LaneId(to));
        d
    }

    // ---- discretionary changes ---------------------------------------------------------

    fn plan_discretionary(
        &mut self,
        scene: &ManeuverScene,
        actor: &ManeuverActor,
        state: &mut ManeuverState,
    ) -> Option<ManeuverDecision> {
        let (to, dir, code) = self.discretionary_wish(scene, actor, state)?;
        if state.dwell_code != code {
            state.dwell = DISCRETIONARY_DWELL;
            state.dwell_code = code;
            return None;
        }
        if state.dwell > 0.0 { return None; }
        if dir != state.last_side && state.last_side != 0
            && scene.time - state.last_change_time < OSCILLATION_WINDOW
        {
            state.dwell = DISCRETIONARY_DWELL;
            return None;
        }
        let s_to = scene.net.beside_s(actor.lane, to, actor.s);
        if self.approved(actor.id) != Some(LaneId(to)) { return None; }
        let ramp = if code == 3 { BYPASS_RAMP } else { (actor.speed * 3.0).max(12.0) };
        if !self.can_merge_ramp(scene, actor, to, s_to, ramp) {
            state.pass_retry = scene.time + 0.5;
            return None;
        }
        state.change_to = Some(to);
        state.change_dir = dir;
        state.dwell_code = 0;
        let mut d = ManeuverDecision::new(ManeuverPhase::LaneChange);
        d.change = Some(ChangeCommand::new(to, dir, if code == 3 { ChangeKind::Bypass } else { ChangeKind::Change }));
        d.target_lane = Some(LaneId(to));
        if code == 3 { d.accel_cap = Some(PULL_OUT_ACCEL); }
        Some(d)
    }

    fn discretionary_wish(&self, scene: &ManeuverScene, actor: &ManeuverActor, state: &ManeuverState) -> Option<(usize, i32, i16)> {
        if actor.lane_kind != LaneKind::Street
            || state.change_cooldown > 0.0
            || actor.at_stop || actor.light_hold || actor.yielding
            || state.park.is_some() || state.passing.is_some() || state.pull_out > 0.0
            || scene.time < state.pass_retry
        {
            return None;
        }
        let net = scene.net;
        let lane = &net.lanes[actor.lane];
        let limit = (lane.speed_limit_kmh * actor.desire).min(actor.max_speed_kmh) / 3.6;
        let lht = net.left_hand;
        let (pass_side, keep_side, pass_dir, keep_dir) = if lht {
            (lane.right, lane.left, 2, 1)
        } else {
            (lane.left, lane.right, 1, 2)
        };
        // Overtake a slow leader on the passing side.
        let mut wish: Option<(usize, i32, i16)> = None;
        if let Some(left) = pass_side {
            let s_left = net.beside_s(actor.lane, left, actor.s);
            if self.open_to(net, actor, left) && self.stays_open(net, actor, left, s_left) {
                if let Some((gap, v, owner)) = self.nearest_ahead_on_way(scene, actor, 45.0) {
                    let standing = v < 0.3 && self.standing_queue(scene, actor, owner);
                    let bypass = standing && (actor.speed > 0.5 || actor.stopped >= 3.0);
                    let room = if bypass {
                        BYPASS_RAMP + actor.front + actor.speed * SIGNAL_BEFORE_CHANGE
                            + 0.5 * actor.accel.max(0.0) * SIGNAL_BEFORE_CHANGE.powi(2) + 1.0
                    } else { (actor.speed * 5.5 + 10.0).max(40.0) };
                    if v < limit * 0.7
                        && v < actor.speed + 1.0
                        && (bypass || (actor.speed >= 4.0 && actor.stopped <= 0.0))
                        && gap > if bypass { actor.pass_room.max(actor.min_gap) - 0.3 } else { 8.0 }
                        && self.parallel_room(scene, actor, left, room)
                        && self.merge_gaps_clear(scene, actor, left, s_left,
                            if bypass { BYPASS_RAMP } else { (actor.speed * 3.0).max(12.0) })
                    {
                        wish = Some((left, pass_dir, if bypass { 3 } else { 1 }));
                    }
                }
            }
        }
        // Keep to the correct side when that lane is free.
        if wish.is_none() && actor.speed >= 4.0 && actor.stopped <= 0.0 {
            if let Some(right) = keep_side {
                let s_right = net.beside_s(actor.lane, right, actor.s);
                if self.open_to(net, actor, right) && self.stays_open(net, actor, right, s_right) {
                    if self.parallel_room(scene, actor, right, (actor.speed * 5.5 + 10.0).max(40.0))
                        && self.lane_clear(scene, actor.id, right, s_right, 30.0, 70.0)
                    {
                        wish = Some((right, keep_dir, 2));
                    }
                }
            }
        }
        wish
    }

    fn parallel_room(&self, scene: &ManeuverScene, actor: &ManeuverActor, to: usize, need: f32) -> bool {
        let net = scene.net;
        let (mut a, mut b) = (actor.lane, to);
        let (mut sa, mut sb) = (actor.s, net.beside_s(a, b, actor.s));
        let mut room = 0.0;
        for _ in 0..12 {
            let (la, lb) = (&net.lanes[a], &net.lanes[b]);
            room += (la.length() - sa).min(lb.length() - sb).max(0.0);
            if room >= need { return true; }
            // Do not treat a signal, a turn or a branching junction as an ordinary joint.
            if la.traffic_light.is_some() || lb.traffic_light.is_some() || la.turn != 0 || lb.turn != 0 {
                return false;
            }
            let Some((na, nb)) = net.parallel_continuation(a, b) else { return false };
            if !self.open_to(net, actor, nb)
                || (a == actor.lane && actor.planned_next.is_some_and(|n| n != na))
            { return false; }
            (a, b, sa, sb) = (na, nb, 0.0, 0.0);
        }
        false
    }

    /// Follow the frozen leader chain, including across spline joints. A car behind a
    /// boarding bus has the same passing opportunity as the first car behind it; a
    /// signal or junction queue does not. Do not depend on last tick's container order.
    fn standing_queue(&self, scene: &ManeuverScene, actor: &ManeuverActor, owner: VehicleId) -> bool {
        let mut owner = owner;
        let mut seen = vec![actor.id];
        for _ in 0..16 {
            if seen.contains(&owner) { return false; }
            seen.push(owner);
            let Some(leader) = scene.actors.iter().find(|a| a.id == owner) else {
                return actor.stopped > 10.0;
            };
            if leader.speed >= 0.3 || leader.light_hold || leader.yielding { return false; }
            if leader.at_stop { return true; }
            let next = self.nearest_ahead_on_way(scene, leader, leader.pass_room.max(8.0) + 2.0);
            let Some((_, speed, next)) = next else { return leader.stopped > 25.0; };
            if leader.stopped < 3.0 || speed >= 0.3 { return false; }
            owner = next;
        }
        false
    }

    // ---- passing -----------------------------------------------------------------------

    fn plan_passing(
        &mut self,
        scene: &ManeuverScene,
        actor: &ManeuverActor,
        state: &mut ManeuverState,
        input: &ManeuverInputs,
    ) -> Option<ManeuverDecision> {
        let net = scene.net;
        let rolling = (input.parked || input.priority_pass) && actor.speed > 0.5;
        if actor.lane_kind != LaneKind::Street
            || actor.change.is_some()
            || (actor.stopped < 3.0 && !rolling)
            || !input.lead_standing
            || actor.yielding
            || actor.at_stop
            || scene.time < state.pass_retry
        {
            return None;
        }
        let gap = input.lead_gap?;
        let lane = &net.lanes[actor.lane];
        // Only the single-lane-each-way case: a road with lanes to change to uses bypass.
        if lane.left.is_some() || lane.right.is_some() {
            return None;
        }
        let real = gap + if input.parked { 2.0 } else { 0.0 };
        let reach = if rolling {
            (actor.speed * actor.speed / (2.0 * actor.decel) + 12.0).clamp(15.0, 40.0)
        } else {
            22.0 + (actor.pass_room - 4.0).max(0.0)
        };
        let outward = actor.lateral * net.oncoming_sign();
        if real > reach || real < 0.3 || outward < -0.5 || outward > 1.6 {
            return None;
        }
        let way = way_of(net, actor, 60.0);
        let Some((opp, os, side)) = net.opposite(actor.lane, actor.s) else {
            return None;
        };
        // A geometrically parallel oncoming path may be across a central island. Its
        // existence does not make the gap between the carriageways a passing lane (#126).
        if side > (lane.width + net.lanes[opp].width) * 0.5 + 0.6 {
            return None;
        }
        if !(2.3..=5.5).contains(&side) {
            return None;
        }
        let obstacle_len = input.obstacle_len;
        let pass_len = real + obstacle_len + actor.front + actor.rear + 6.0;
        // The maneuver must fit before a junction and not run off the end of the road.
        let junction = way
            .iter()
            .skip(1)
            .find(|w| !net.crossings[w.0].is_empty())
            .map(|w| w.1)
            .unwrap_or(f32::MAX);
        let open_road: f32 = way
            .iter()
            .take_while(|w| net.crossings[w.0].is_empty())
            .map(|w| w.1 + net.lanes[w.0].length())
            .fold(0.0, f32::max);
        if open_road.min(junction) < pass_len - if input.parked { 6.0 } else { -8.0 } {
            return None;
        }
        if let Some(&(last, d)) = way.last() {
            if net.lanes[last].next.is_empty()
                && d + net.lanes[last].length() < pass_len + real + 30.0
            {
                return None;
            }
        }
        // Back in 2 m past the obstacle, along an S-curve the speed there asks for.
        let until_d = real + obstacle_len + actor.front + actor.rear + 2.0;
        let merge_at = actor.front + real + obstacle_len;
        let v_cap =
            ((lane.speed_limit_kmh * actor.desire).min(actor.max_speed_kmh) / 3.6).clamp(4.0, 14.0);
        let v_back = (actor.speed * actor.speed + 2.0 * actor.accel * 0.85 * until_d)
            .sqrt()
            .min(v_cap);
        let back_min = back_in_ramp(side, 0.0, BACK_IN_LAT_ACCEL);
        let need_room =
            (actor.length + 4.0 + v_back * v_back / (2.0 * actor.decel.max(1.0))).max(back_min + 2.0);
        let merge_room = self.merge_room(scene, actor, &way, merge_at, need_room);
        if merge_room < need_room {
            return None;
        }
        let back = back_in_ramp(side, v_back, actor.lat_accel.min(BACK_IN_LAT_ACCEL))
            .min((merge_room - 2.0).max(back_min));
        let probe = Passing {
            lane: opp,
            side,
            until: until_d,
            block: real,
            back,
            aborted: false,
            hold: 0.0,
            creep: !rolling,
        };
        let clear_d = probe.clear_at(actor.half_width);
        // Nobody coming may reach where the car will be for the whole time it is out there.
        let t_need = pass_time(
            clear_d,
            if rolling { 0.0 } else { real + CREEP_PAST },
            actor.speed,
            actor.reaction,
            actor.accel,
            v_cap,
        ) + 1.5;
        let from = os - actor.front - clear_d - 2.0;
        let to = os + actor.rear + 8.0;
        if self.oncoming_soon(scene, actor.id, opp, from, to, t_need) {
            return None;
        }
        // The whole trajectory (out and back) must clear every body and pedestrian.
        let target = side * net.oncoming_sign();
        let Some(ramp) = self.choose_ramp(scene, actor, &probe, target, pass_len, rolling) else {
            state.pass_retry = scene.time + 1.0;
            return None;
        };
        let mut d = ManeuverDecision::new(ManeuverPhase::Passing);
        d.lateral_target = Some(target);
        d.lateral_ramp = Some((actor.lateral, target, actor.odometer, ramp));
        d.stop_at = None;
        d.target_lane = Some(LaneId(opp));
        state.passing = Some(Passing {
            until: actor.odometer + until_d,
            block: actor.odometer + real,
            ..probe
        });
        Some(d)
    }

    /// Finish or abort a pass already underway. `None` means none is active.
    fn plan_passing_active(
        &mut self,
        scene: &ManeuverScene,
        actor: &ManeuverActor,
        state: &mut ManeuverState,
    ) -> Option<ManeuverDecision> {
        let mut p = state.passing?;
        let net = scene.net;
        let odo = actor.odometer;
        // A car that has stood still before the obstacle edges back in and waits there.
        if !p.aborted && actor.stopped > 8.0 && odo < p.until && actor.lateral.abs() < p.side * 0.5 {
            p.until = odo;
            p.aborted = true;
            p.hold = odo;
        }
        if p.aborted {
            let mut d = ManeuverDecision::new(ManeuverPhase::PassingAbort);
            d.stop_at = Some(actor.front + (p.hold - odo).max(0.0) + 0.6);
            d.reasons.push(Reason::Leader);
            d.binding = Some(Reason::Leader);
            if actor.speed < 0.1 && (odo >= p.hold - 0.3 || actor.stopped > 1.0) {
                state.passing = None;
            } else {
                state.passing = Some(p);
            }
            return Some(d);
        }
        if odo >= p.until {
            // Past the obstacle: move back into the lane along the return S-curve.
            let mut d = ManeuverDecision::new(ManeuverPhase::Passing);
            d.lateral_target = Some(0.0);
            d.lateral_ramp = Some((actor.lateral, 0.0, odo, p.back));
            if actor.lateral.abs() < 0.05 {
                state.passing = None;
            } else {
                state.passing = Some(p);
            }
            return Some(d);
        }
        // Still out there: if somebody is coming who gets where the front is headed before
        // it is back, abort while it still can.
        let r = p.clear_at(actor.half_width) - odo;
        if r > 0.0 {
            if let Some((opp, os, _)) = net.opposite(actor.lane, actor.s) {
                let lane = &net.lanes[actor.lane];
                let v_cap = ((lane.speed_limit_kmh * actor.desire).min(actor.max_speed_kmh) / 3.6)
                    .clamp(4.0, 14.0);
                let creep = if p.creep {
                    (p.block + CREEP_PAST - odo).max(0.0)
                } else {
                    0.0
                };
                let t_me = pass_time(r, creep, actor.speed, actor.reaction, actor.accel, v_cap);
                let from = os - actor.front - r - 1.0;
                let to = os + actor.rear;
                if self.oncoming_soon(scene, actor.id, opp, from, to, t_me + 0.5) {
                    let stop_d = actor.speed * actor.speed / (2.0 * 3.5);
                    let shallow =
                        actor.lateral * net.oncoming_sign() < p.side - actor.half_width - 1.45;
                    let abortable = shallow && odo + stop_d + 0.4 < p.block;
                    if abortable {
                        p.aborted = true;
                        p.until = odo;
                        p.hold = (p.block - actor.pass_room).max(odo + stop_d);
                        state.passing = Some(p);
                        let mut d = ManeuverDecision::new(ManeuverPhase::PassingAbort);
                        d.lateral_target = Some(0.0);
                        d.lateral_ramp =
                            Some((actor.lateral, 0.0, odo, (p.block - odo - 0.5).clamp(2.0, 8.0)));
                        d.stop_at = Some(actor.front + (p.hold - odo).max(0.0) + 0.6);
                        d.reasons.push(Reason::Passing);
                        d.binding = Some(Reason::Passing);
                        return Some(d);
                    }
                }
            }
        }
        // Keep out; the lead checks in the core hold it to the corridor.
        state.passing = Some(p);
        let mut d = ManeuverDecision::new(ManeuverPhase::Passing);
        d.target_lane = Some(LaneId(p.lane));
        d.reasons.push(Reason::Passing);
        // From a standstill the driver edges out at `PULL_OUT_ACCEL` until past the obstacle.
        if p.creep && odo < p.block + CREEP_PAST {
            d.accel_cap = Some(PULL_OUT_ACCEL);
        }
        Some(d)
    }

    /// Choose an S-curve that clears the obstacle for the whole outbound and return path.
    fn choose_ramp(
        &self,
        scene: &ManeuverScene,
        actor: &ManeuverActor,
        probe: &Passing,
        target: f32,
        pass_len: f32,
        rolling: bool,
    ) -> Option<f32> {
        let need = PULL_OUT_CLEARANCE;
        for ramp in pull_out_ramps(probe.block, actor.front, rolling) {
            let lat = |d: f32| {
                if d > probe.until {
                    target * (1.0 - smooth01(((d - probe.until) / probe.back.max(0.1)).clamp(0.0, 1.0)))
                } else {
                    actor.lateral + (target - actor.lateral) * smooth01((d / ramp.max(0.1)).clamp(0.0, 1.0))
                }
            };
            let entire = pass_len.max(probe.until + probe.back + actor.front);
            if self.sweep_clear(scene, actor, &lat, entire, need) {
                return Some(ramp);
            }
        }
        None
    }

    /// Sweep the whole ghost trajectory (out and back) against the realized bodies and
    /// pedestrians in the freeze. `lat(d)` gives the sideways offset at distance `d`.
    fn sweep_clear(
        &self,
        scene: &ManeuverScene,
        actor: &ManeuverActor,
        lat: &impl Fn(f32) -> f32,
        pass_len: f32,
        need: f64,
    ) -> bool {
        let net = scene.net;
        let way = way_of(net, actor, pass_len + 4.0);
        self.sweep_path_clear(scene, actor, &|d| {
            let (lane, u) = way_locate(net, &way, d)?;
            let (p, h) = net.lanes[lane].at(u);
            let hr = (h as f64).to_radians();
            Some(p + DVec3::new(hr.cos(), -hr.sin(), 0.0) * lat(d) as f64)
        }, pass_len, need)
    }

    fn sweep_path_clear(
        &self,
        scene: &ManeuverScene,
        actor: &ManeuverActor,
        point: &impl Fn(f32) -> Option<DVec3>,
        distance: f32,
        need: f64,
    ) -> bool {
        let mut samples: Vec<SweepSample> = Vec::new();
        let mut d = 0.0f32;
        while d <= distance {
            let Some(p) = point(d) else { return false; };
            let Some(q) = point(d + 0.5) else { return false; };
            samples.push(SweepSample {
                p,
                d,
                dir: (q - p).truncate().normalize_or_zero(),
            });
            d += 1.0;
        }
        // Check the whole car, including its swinging front corner. Cross-sections
        // alone can approve a path whose centre clears a bus but whose bumper clips it.
        // Articulated rear sections have their own geometry; total rear extent is
        // used for return room, not a rigid box rotated with the tractor.
        let rear = scene.occupancy.feet().iter().find(|b| b.owner == actor.id && b.part == 0)
            .map(|b| b.rear).unwrap_or(actor.rear);
        for sample in &samples {
            let center = sample.p.truncate()
                + sample.dir * ((actor.front - rear) * 0.5) as f64;
            let probe = crate::BodyFootprint::new(actor.id, center, sample.dir,
                ((actor.front + rear) * 0.5) as f64, actor.half_width as f64 + need,
                sample.p.z, sample.p.z + actor.height as f64, actor.speed);
            let mut contact = false;
            scene.occupancy.near(center, probe.half_len + probe.half_w, |body| {
                contact |= body.owner != actor.id && body.overlaps(&probe, 0.0);
            });
            if contact { return false; }
        }
        if let Some(hit) = scene
            .occupancy
            .swept_clearance(&samples, actor.half_width as f64 + need, &[actor.id])
        {
            // The first body the ghost path meets: if it is the obstacle itself, the ramp did
            // not clear it. Any hit inside the maneuver means the path is not clear.
            if hit.d <= distance {
                return false;
            }
        }
        if scene.static_clearance.is_some_and(|clear| !clear(&samples, actor)) {
            return false;
        }
        if scene.occupancy.pedestrian_clearance(&samples, actor.half_width as f64, scene.people, 0.4).is_some() {
            return false;
        }
        true
    }

    /// Who on the oncoming side reaches the stretch `from..to` of `opp` before `t_need`.
    fn oncoming_soon(
        &self,
        scene: &ManeuverScene,
        id: VehicleId,
        opp: usize,
        from: f32,
        to: f32,
        t_need: f32,
    ) -> bool {
        let net = scene.net;
        let limit = net.lanes[opp].speed_limit_kmh / 3.6;
        let look = (limit.clamp(8.0, 20.0) * (t_need + 1.0) + 20.0).min(300.0);
        for (lane, off, _) in net.upstream(opp, from, look, 48) {
            for iv in scene.occupancy.intervals(LaneId(lane)) {
                if iv.owner == id {
                    continue;
                }
                let c = iv.s + off;
                if iv.foreign {
                    // Someone from this side out on the lane moving along: may be followed.
                    if lane == opp && c > from - 2.0 && c < to && iv.speed <= 2.0 {
                        return true;
                    }
                    continue;
                }
                if c - iv.rear > to {
                    continue;
                }
                let front = iv.front;
                if front > from {
                    return true;
                }
                let dist = from - front;
                if dist > look {
                    continue;
                }
                let v = iv.speed;
                let v_max = (net.lanes[lane].speed_limit_kmh / 3.6).max(v);
                let t = if v < 0.3 {
                    0.7 + arrival_time(dist, 0.0, 1.5, v_max)
                } else {
                    arrival_time(dist, v, 1.5, v_max)
                };
                if t < t_need {
                    return true;
                }
            }
        }
        false
    }

    // ---- parking -----------------------------------------------------------------------

    fn plan_parking(
        &self,
        scene: &ManeuverScene,
        actor: &ManeuverActor,
        state: &mut ManeuverState,
        mut plan: ParkPlan,
        _input: &ManeuverInputs,
    ) -> ManeuverDecision {
        let net = scene.net;
        let mut d = ManeuverDecision::new(ManeuverPhase::Parking);
        // The plan lane must still be on the way ahead.
        let Some((_, dl)) = way_of(net, actor, 200.0)
            .into_iter()
            .find(|w| w.0 == plan.lane)
        else {
            if actor.lane != plan.lane {
                state.park = None;
                return ManeuverDecision::new(ManeuverPhase::Idle);
            }
            d.reasons.push(Reason::Parking);
            return d;
        };
        let ahead = if actor.lane == plan.lane {
            plan.s - actor.s
        } else {
            dl + plan.s - actor.s
        };
        if ahead < 80.0 {
            d.stop_at = Some(ahead.max(0.0) + actor.front);
            d.signal = Some((2, 1.0));
            d.reasons.push(Reason::Parking);
            d.binding = Some(Reason::Parking);
            if ahead < (plan.lat.abs() * 6.0).clamp(10.0, 22.0) + 1.0 && ahead > 2.0 && !plan.ramped {
                plan.ramped = true;
                let len = (plan.lat.abs() * 6.0).clamp(10.0, 22.0);
                d.lateral_target = Some(plan.lat);
                d.lateral_ramp = Some((actor.lateral, plan.lat, actor.odometer, (ahead - 0.6).max(4.0).min(len)));
            } else if plan.ramped || ahead.abs() < 1.5 {
                d.lateral_target = Some(plan.lat);
            } else {
                d.lateral_target = Some(0.0);
            }
        }
        if ahead.abs() < 1.5 && actor.speed < 0.2 && (actor.lateral - plan.lat).abs() < 0.3 {
            plan.done = true;
        } else if ahead < -4.0 {
            state.park = None;
            d.lateral_target = Some(0.0);
            d.reasons.clear();
            d.binding = None;
            return d;
        }
        state.park = if plan.done { Some(plan) } else { Some(plan) };
        d
    }

    // ---- feasibility helpers -----------------------------------------------------------

    fn open_to(&self, net: &Network, actor: &ManeuverActor, lane: usize) -> bool {
        let Some(l) = net.lanes.get(lane) else {
            return false;
        };
        !l.no_cars && l.density > 0.0 && l.allows(actor.veh_type)
    }

    /// Does lane `lane` (from `s` on) stay open to `actor` beyond the lookahead? A car does
    /// not keep right into a lane that goes on as a bus lane, only to move back again.
    fn stays_open(&self, net: &Network, actor: &ManeuverActor, lane: usize, s: f32) -> bool {
        closed_ahead(net, lane, s, actor.veh_type, None).is_none()
    }

    /// May `actor` move over into `to` at `s_to` now? Nothing beside or just ahead, and every
    /// vehicle behind can still stop behind it.
    fn can_merge(
        &self,
        scene: &ManeuverScene,
        actor: &ManeuverActor,
        to: usize,
        s_to: f32,
    ) -> bool {
        self.can_merge_ramp(scene, actor, to, s_to, (actor.speed * 3.0).max(12.0))
    }

    fn can_merge_ramp(&self, scene: &ManeuverScene, actor: &ManeuverActor, to: usize, s_to: f32, ramp: f32) -> bool {
        if !self.merge_gaps_clear(scene, actor, to, s_to, ramp) { return false; }
        self.merge_trajectory_clear(scene, actor, to, s_to, ramp)
    }

    fn merge_gaps_clear(&self, scene: &ManeuverScene, actor: &ManeuverActor, to: usize, s_to: f32, ramp: f32) -> bool {
        if to >= scene.net.lanes.len() {
            return false;
        }
        for (lane, off) in lane_window(scene.net, to, s_to, 50.0, ramp + 50.0) {
          for iv in scene.occupancy.intervals(LaneId(lane)) {
            if iv.owner == actor.id {
                continue;
            }
            let (origin, front, rear) = (iv.s + off, iv.front + off, iv.rear + off);
            if rear > s_to + ramp + 50.0 || front < s_to - 50.0 { continue; }
            if iv.foreign {
                return false;
            }
            if origin >= s_to {
                let gap = rear - (s_to + actor.front);
                if gap <= 2.0 + (actor.speed - iv.speed).max(0.0) * 1.5 {
                    return false;
                }
            } else {
                if iv.speed < 0.3 {
                    // A standing body lets a car in only when it is not abreast of it.
                    if front >= s_to - actor.rear && rear <= s_to + actor.front {
                        return false;
                    }
                    continue;
                }
                let gap = (s_to - actor.rear) - front;
                if gap
                    <= 2.0
                        + iv.speed * 0.8
                        + (iv.speed - actor.speed).max(0.0).powi(2)
                            / (2.0 * actor.decel.max(1.0))
                {
                    return false;
                }
            }
          }
        }
        true
    }

    fn merge_trajectory_clear(&self, scene: &ManeuverScene, actor: &ManeuverActor, to: usize, s_to: f32, ramp: f32) -> bool {
        // Include the lateral transition, not only the gaps on its destination lane.
        // Realization blends the source and destination ways. A constant lateral
        // offset is wrong when they curve differently or join one continuation:
        // after that joint it invents another lane's width of sideways travel.
        let net = scene.net;
        let source = way_of(net, actor, ramp + 4.0);
        let mut target_actor = actor.clone();
        target_actor.lane = to;
        target_actor.s = s_to;
        target_actor.planned_next = if actor.route_next == Some(to) {
            actor.planned_next
        } else {
            net.parallel_continuation(actor.lane, to).map(|(_, next)| next)
                .or_else(|| net.lanes[to].next.first().copied())
        };
        let destination = way_of(net, &target_actor, ramp + 4.0);
        let point = |d: f32| {
            let (a, sa) = way_locate(net, &source, d)?;
            let (b, sb) = way_locate(net, &destination, d)?;
            let (p, heading) = net.lanes[a].at(sa);
            let q = net.lanes[b].at(sb).0;
            let k = smooth01((d / ramp).clamp(0.0, 1.0));
            let h = (heading as f64).to_radians();
            Some(p.lerp(q, k as f64) + DVec3::new(h.cos(), -h.sin(), 0.0)
                * (actor.lateral * (1.0 - k)) as f64)
        };
        if !self.sweep_path_clear(scene, actor, &point, ramp, 0.0) {
            return false;
        }
        true
    }

    /// Is the stretch `s - back .. s + ahead` of `lane` free of other bodies?
    fn lane_clear(
        &self,
        scene: &ManeuverScene,
        id: VehicleId,
        lane: usize,
        s: f32,
        back: f32,
        ahead: f32,
    ) -> bool {
        !lane_window(scene.net, lane, s, back, ahead).into_iter().any(|(l, off)|
            scene.occupancy.intervals(LaneId(l)).iter().any(|iv|
                iv.owner != id && iv.front + off > s - back && iv.rear + off < s + ahead))
    }

    /// The nearest body ahead along the actor's way, up to `look` m: `(gap to its rear, its
    /// speed)`.
    fn nearest_ahead_on_way(
        &self,
        scene: &ManeuverScene,
        actor: &ManeuverActor,
        look: f32,
    ) -> Option<(f32, f32, VehicleId)> {
        let way = way_of(scene.net, actor, look + 30.0);
        let mut best: Option<(f32, f32, VehicleId)> = None;
        for &(lane, off) in &way {
            for iv in scene.occupancy.intervals(LaneId(lane)) {
                if iv.owner == actor.id || iv.foreign {
                    continue;
                }
                let gap = off + iv.rear - actor.front;
                if gap < 0.0 || gap > look {
                    continue;
                }
                if best.map(|b| gap < b.0).unwrap_or(true) {
                    best = Some((gap, iv.speed, iv.owner));
                }
            }
        }
        // Rear sections have geometry but no primary lane interval. Include the
        // realized corridor, otherwise a bus's front section overstates pull-out room.
        let mut samples = Vec::new();
        let mut d = actor.front;
        while d <= actor.front + look {
            let (lane, s) = way_locate(scene.net, &way, d)?;
            let (p, h) = scene.net.lanes[lane].at(s);
            let h = (h as f64).to_radians();
            samples.push(SweepSample {
                p: p + DVec3::new(h.cos(), -h.sin(), 0.0) * actor.lateral as f64,
                d, dir: DVec2::new(h.sin(), h.cos()),
            });
            d += 0.5;
        }
        if let Some(hit) = scene.occupancy.swept_clearance(&samples, actor.half_width as f64, &[actor.id]) {
            let gap = (hit.d - actor.front).max(0.0);
            if best.is_none_or(|b| gap < b.0) { best = Some((gap, hit.speed, hit.owner)); }
        }
        best
    }

    /// Free room (m) from `merge_at` along the way before a body that cannot be returned in
    /// front of, bounded by `need + 20`.
    fn merge_room(
        &self,
        scene: &ManeuverScene,
        actor: &ManeuverActor,
        way: &[WayStep],
        merge_at: f32,
        need: f32,
    ) -> f32 {
        let mut room = f32::MAX;
        for &(lane, off) in way {
            for iv in scene.occupancy.intervals(LaneId(lane)) {
                if iv.owner == actor.id {
                    continue;
                }
                let at = off + iv.rear;
                let start = off + iv.s;
                if start < merge_at {
                    continue;
                }
                let gap = at - merge_at;
                if gap > need + 20.0 {
                    continue;
                }
                if iv.speed < 3.0 {
                    room = room.min(gap);
                }
            }
        }
        room
    }
}

/// The lane the fixed route or a turn lane ahead requires next, if any. This is the target
/// the coordinator serializes simultaneous changes for; core submits it as an intent.
pub fn required_target(net: &Network, actor: &ManeuverActor) -> Option<usize> {
    if let Some(to) = actor.route_next {
        if net.parallel(actor.lane, to) {
            return Some(to);
        }
    }
    let lane = net.lanes.get(actor.lane)?;
    // The way on closes to this vehicle (a lane that goes on as a bus lane): move over to
    // the lane beside that stays open, whatever the turn lanes ask.
    if actor.lane_kind == LaneKind::Street
        && closed_ahead(net, actor.lane, actor.s, actor.veh_type, actor.planned_next).is_some()
    {
        return [lane.left, lane.right].into_iter().flatten().find(|&b| {
            let l = &net.lanes[b];
            l.allows(actor.veh_type)
                && l.density > 0.0
                && closed_ahead(net, b, net.beside_s(actor.lane, b, actor.s), actor.veh_type, None)
                    .is_none()
        });
    }
    let to_junction = lane.length() - actor.s;
    if to_junction >= TURN_LANE_LOOKAHEAD {
        return None;
    }
    let turn = actor
        .planned_next
        .map(|n| net.lanes[n].turn)
        .filter(|t| *t != 0)
        .or(if actor.turn_wish != 0 {
            Some(actor.turn_wish)
        } else {
            None
        })?;
    let want = if turn == 1 { lane.left } else { lane.right }?;
    if net.lanes[want].allows(actor.veh_type)
        && net.lanes[want]
            .next
            .iter()
            .any(|&n| net.lanes[n].turn == turn && net.lanes[n].allows(actor.veh_type))
    {
        Some(want)
    } else {
        None
    }
}

/// How far ahead (from distance `s` along `lane`, within `TURN_LANE_LOOKAHEAD`) the way
/// runs into a joint where every way on is closed to `[ai_veh_type]` `veh_type` (see
/// `Lane::allows`): a lane that goes on as a bus lane, a junction path only the timetable
/// may drive. The way is `first` (the planned next lane) where it is open, then the one
/// open way on at each joint; where there is a choice of open ways, it does not close.
/// The end of the network (no way on at all) is not a closure.
pub fn closed_ahead(
    net: &Network,
    lane: usize,
    s: f32,
    veh_type: i32,
    first: Option<usize>,
) -> Option<f32> {
    let mut cur = lane;
    let mut d = net.lanes.get(lane)?.length() - s;
    let mut first = first;
    for _ in 0..12 {
        if d >= TURN_LANE_LOOKAHEAD {
            return None;
        }
        let l = &net.lanes[cur];
        if l.next.is_empty() {
            return None;
        }
        let next = match first.take().filter(|n| l.next.contains(n) && net.lanes[*n].allows(veh_type)) {
            Some(n) => n,
            None => {
                let mut open = l.next.iter().copied().filter(|&n| net.lanes[n].allows(veh_type));
                match (open.next(), open.next()) {
                    (None, _) => return Some(d.max(0.0)),
                    (Some(n), None) => n,
                    (Some(_), Some(_)) => return None,
                }
            }
        };
        cur = next;
        d += net.lanes[cur].length();
    }
    None
}

/// The side (1 left, 2 right) of lane `to` from `from`, from the geometry where they are.
fn side_of(net: &Network, from: usize, to: usize) -> i32 {
    let (la, lb) = (&net.lanes[from], &net.lanes[to]);
    let (pa, ha) = la.at(la.length() * 0.5);
    let pb = lb.at(lb.length() * 0.5).0;
    let h = (ha as f64).to_radians();
    let side = (pb - pa).truncate().dot(DVec2::new(h.cos(), -h.sin()));
    if side > 0.0 || (side.abs() < 0.5 && lb.offset > la.offset) {
        2
    } else {
        1
    }
}

/// The way of an actor as `(lane, distance from its origin to the lane start)`: the current
/// lane, then `planned_next`, then the first `next` until `look` metres are covered.
pub fn way_of(net: &Network, actor: &ManeuverActor, look: f32) -> Vec<WayStep> {
    let mut out = vec![(actor.lane, -actor.s)];
    let mut d = net.lanes[actor.lane].length() - actor.s;
    let mut cur = actor.planned_next.or_else(|| net.lanes[actor.lane].next.first().copied());
    let mut guard = 0;
    while d < look && guard < 12 {
        let Some(n) = cur else { break };
        out.push((n, d));
        d += net.lanes[n].length();
        cur = net.lanes[n].next.first().copied();
        guard += 1;
    }
    out
}

/// A bounded lane window in the starting lane's coordinates, including spline joints.
fn lane_window(net: &Network, lane: usize, s: f32, back: f32, ahead: f32) -> Vec<WayStep> {
    let mut out: Vec<_> = net.upstream(lane, s, back, 24).into_iter()
        .map(|(l, off, _)| (l, off)).collect();
    let mut pending = vec![(lane, 0.0)];
    while let Some((l, off)) = pending.pop() {
        let next_off = off + net.lanes[l].length();
        if next_off > s + ahead || out.len() >= 48 { continue; }
        for &next in &net.lanes[l].next {
            if !out.iter().any(|&(n, _)| n == next) {
                out.push((next, next_off));
                pending.push((next, next_off));
            }
        }
    }
    out
}

/// The network lane and distance along it of `d` metres from the actor origin along `way`.
pub fn way_locate(net: &Network, way: &[WayStep], d: f32) -> Option<(usize, f32)> {
    let mut prev_start = way.first()?.1;
    for (k, &(lane, start)) in way.iter().enumerate() {
        let len = net.lanes[lane].length();
        if d <= start + len {
            if k == 0 {
                return Some((lane, d - start));
            }
            if d >= start {
                return Some((lane, d - start));
            }
            let _ = prev_start;
            return Some((lane, 0.0));
        }
        prev_start = start;
    }
    let (lane, start) = *way.last()?;
    Some((lane, (d - start).max(0.0)))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::network::LaneBuilder;

    fn straight_lane() -> Network {
        let lane = LaneBuilder::polyline(
            vec![DVec3::new(0.0, 0.0, 0.0), DVec3::new(0.0, 400.0, 0.0)],
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

    #[test]
    fn pull_out_ramps_are_gentle_to_steep() {
        let r = pull_out_ramps(6.0, 2.25, false);
        assert!(!r.is_empty());
        assert!(r.windows(2).all(|w| w[0] >= w[1]));
        assert!(r.iter().all(|&x| (4.0..=12.0).contains(&x)));
    }

    #[test]
    fn back_in_ramp_keeps_the_sideways_acceleration() {
        let len = back_in_ramp(3.0, 5.0, BACK_IN_LAT_ACCEL);
        let peak = 6.0 * 3.0 * 5.0 * 5.0 / (len * len);
        assert!(peak <= BACK_IN_LAT_ACCEL + 1e-3, "peak {peak}");
        assert!(len >= 8.0);
    }

    #[test]
    fn a_route_required_change_waits_when_the_lane_is_not_clear() {
        let net = straight_lane();
        let occ = Occupancy::default();
        let actors = vec![ManeuverActor::new(VehicleId(1), 0, 10.0)];
        let scene = ManeuverScene {
            static_clearance: None,
            net: &net,
            occupancy: &occ,
            actors: &actors,
            people: &[],
            time: 0.0,
            dt: 0.02,
            tick: 0,
        };
        let mut coord = ManeuverCoordinator::new();
        let mut state = ManeuverState::default();
        let mut input = ManeuverInputs::new(0);
        input.lead_gap = Some(5.0);
        // No route_next beside, no turn: nothing to do.
        let d = coord.plan(&scene, &mut state, &input);
        assert_eq!(d.phase, ManeuverPhase::Idle);
    }

    #[test]
    fn simultaneous_changes_go_to_the_lowest_id() {
        let mut coord = ManeuverCoordinator::new();
        let intents = [
            ManeuverIntent { vehicle: VehicleId(7), target: Some(LaneId(3)), required: false, s: 0.0 },
            ManeuverIntent { vehicle: VehicleId(2), target: Some(LaneId(3)), required: false, s: 0.0 },
            ManeuverIntent { vehicle: VehicleId(5), target: Some(LaneId(3)), required: false, s: 0.0 },
        ];
        coord.begin_tick(&intents, 0);
        assert_eq!(coord.approved(VehicleId(2)), Some(LaneId(3)));
        assert_eq!(coord.approved(VehicleId(7)), None);
        assert_eq!(coord.approved(VehicleId(5)), None);
    }

    #[test]
    fn a_pass_is_only_started_with_an_oncoming_lane_and_room() {
        let net = straight_lane();
        // No oncoming lane: no pass.
        let occ = Occupancy::default();
        let actors = vec![{
            let mut a = ManeuverActor::new(VehicleId(1), 0, 100.0);
            a.speed = 6.0;
            a.stopped = 5.0;
            a
        }];
        let scene = ManeuverScene {
            static_clearance: None,
            net: &net,
            occupancy: &occ,
            actors: &actors,
            people: &[],
            time: 0.0,
            dt: 0.02,
            tick: 0,
        };
        let mut coord = ManeuverCoordinator::new();
        let mut state = ManeuverState::default();
        let mut input = ManeuverInputs::new(0);
        input.lead_gap = Some(6.0);
        input.lead_standing = true;
        let d = coord.plan(&scene, &mut state, &input);
        assert_eq!(d.phase, ManeuverPhase::Idle, "no oncoming lane to pass on");
        assert!(state.passing.is_none());
    }
}
