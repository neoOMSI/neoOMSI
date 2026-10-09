//! Scheduled stop service: physical boarding geometry, berth assignment, and the explicit
//! [`ServicePhase`] state machine.
//!
//! A stop is not a point plus a wait timer: it is a boarding region with a platform side, a
//! longitudinal docking position, and a lateral offset. [`StopTarget`] is the content input;
//! [`BerthGeometry`] keeps the stop length, boarding region, approach distance and vehicle
//! stop correction separately named so each is measured on its own (backlog `D6`).
//!
//! [`ServiceCoordinator`] is the single writer of the per-vehicle [`ServiceState`] transitions
//! and the owner of the berth commitments, mirroring `traffic::junctions`. Planning reads a
//! frozen [`ServiceScene`] and returns a [`ServiceDecision`]. Berths are granted in stable
//! arrival order (arrival tick, then stable id), held through closing and merge-out, and
//! released only when the rear clears the berth, the route changes, the vehicle is removed, or
//! the network is invalidated. A bus waiting upstream has not served the stop and never opens
//! its doors.
//!
//! ## Tuning provenance
//!
//! The constants below carry their unit in the name and their rationale on each item. The
//! docking tolerances are the plan section 6 neoOMSI targets; the service lengths
//! (`DEFAULT_BOARDING_REGION`, `DEFAULT_APPROACH_DISTANCE`) are **provisional** until a content
//! stop length is imported (`D6`); the timing-point/early/layover rules are **improvement**.
//! The full table is in `docs/traffic_refactor/MAINTAINER_GUIDE.md`.

use crate::capabilities::VehicleCapabilities;
use crate::diagnostics::{Reason, ServicePhase, TraceEvent};
use crate::following::STOP_LINE_GAP;
use crate::ids::{LaneId, StopId, VehicleId};
use crate::network::Network;
use crate::perception::Occupancy;
use glam::DVec3;
use hashbrown::{HashMap, HashSet};

/// The side of the road the platform lies on.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PlatformSide {
    /// Right-hand side of the travel direction.
    Right,
    /// The other side (left-hand traffic or an island).
    Left,
    /// Doors on both sides.
    Both,
}

impl PlatformSide {
    /// The map's stop-side code: 0 right, 1 the other, 2 both.
    pub fn from_code(code: f32) -> PlatformSide {
        match code as i32 {
            1 => PlatformSide::Left,
            2 => PlatformSide::Both,
            _ => PlatformSide::Right,
        }
    }

    pub fn code(self) -> f32 {
        match self {
            PlatformSide::Right => 0.0,
            PlatformSide::Left => 1.0,
            PlatformSide::Both => 2.0,
        }
    }
}

/// A specific occurrence of a stop on a route, with its docking geometry.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct StopTarget {
    /// The stop's map object.
    pub stop: StopId,
    /// Which directed occurrence of the route (loops can serve a stop twice).
    pub occurrence: u32,
    /// Index into the vehicle's route of the lane the stop is on.
    pub route_index: usize,
    pub side: PlatformSide,
    /// Where the vehicle's *origin* comes to rest along the lane (m). The front bumper rest
    /// position therefore also depends on the vehicle's own front offset.
    pub s: f32,
    /// Lateral offset of the berth from the lane centre (m, positive = right).
    pub bay: f32,
    /// Timetable departure (seconds of the day).
    pub depart: f64,
}

impl StopTarget {
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        stop: StopId,
        occurrence: u32,
        route_index: usize,
        side: PlatformSide,
        s: f32,
        bay: f32,
        depart: f64,
    ) -> StopTarget {
        StopTarget {
            stop,
            occurrence,
            route_index,
            side,
            s,
            bay,
            depart,
        }
    }

    /// The legacy schedule tuple `(route_index, s, bay, depart, stop id, side code)`.
    #[allow(clippy::type_complexity)]
    pub fn from_tuple(t: (usize, f32, f32, f64, i64, f32)) -> StopTarget {
        StopTarget {
            stop: StopId(t.4),
            occurrence: 0,
            route_index: t.0,
            side: PlatformSide::from_code(t.5),
            s: t.1,
            bay: t.2,
            depart: t.3,
        }
    }
}

/// Why a stop could not be compiled into a docking target.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum StopTargetError {
    /// No lane of the route comes within the reach of the stop.
    NotOnRoute,
    /// The stop is on the side of the road the vehicle cannot open its doors on.
    WrongSide,
    /// The docking position lies off the lane (a stop beyond either end).
    Unreachable,
}

/// Compile a stop into a docking target against the occurrence of a route and the
/// serviceable platform side. The result is a physical berth with a departure time, not a
/// point: the caller may reject a malformed or unreachable berth instead of inventing one.
#[allow(clippy::too_many_arguments)]
pub fn compile_stop_target(
    net: &Network,
    route: &[LaneId],
    occurrence: u32,
    stop: StopId,
    at: DVec3,
    side: PlatformSide,
    vehicle: &VehicleCapabilities,
    reach: f32,
    stop_correction: f32,
    depart: f64,
    from: usize,
) -> Result<StopTarget, StopTargetError> {
    let raw: Vec<usize> = route.iter().map(|l| l.index()).collect();
    let (ri, projected_s, lateral) = net
        .project_stop_on_route(&raw, at, Some(reach.max(5.0) as f64), from)
        .ok_or(StopTargetError::NotOnRoute)?;
    // The stop lies right (positive lateral) or left of the travel direction. Whether that
    // is the platform side the vehicle can serve depends on the map's driving side.
    let on_right = lateral > 0.0;
    let right_is_kerb = !net.left_hand;
    match side {
        PlatformSide::Both => {}
        PlatformSide::Right if on_right != right_is_kerb => {
            return Err(StopTargetError::WrongSide)
        }
        PlatformSide::Left if on_right == right_is_kerb => {
            return Err(StopTargetError::WrongSide)
        }
        _ => {}
    }
    let lane = &net.lanes[raw[ri]];
    let s = projected_s - stop_correction;
    if !(0.0..=lane.length()).contains(&s) {
        return Err(StopTargetError::Unreachable);
    }
    let bay = if net.left_hand {
        lateral + vehicle.half_width - 0.3
    } else {
        lateral - vehicle.half_width + 0.3
    };
    Ok(StopTarget::new(stop, occurrence, ri, side, s, bay, depart))
}

// ---- named service geometry and policy ------------------------------------------------

/// Provisional half-length of a stop's boarding region (m), used until a stop's content
/// length is imported. Named separately from the docking reach (`D6`): the stop's own length,
/// the region people board in, how far before it a bus pulls in, and the vehicle's brake
/// correction are four different numbers.
pub const DEFAULT_BOARDING_REGION: f32 = 6.0;
/// Distance before the berth at which a bus pulls into its bay and enters `Approach` (m).
/// This is the concept the legacy `BAY_REACH = 30` approximated; kept named and separate from
/// the stop length and from the docking reach.
pub const DEFAULT_APPROACH_DISTANCE: f32 = 30.0;
/// Brake for a stop from this far (m).
pub const STOP_REACH: f32 = 80.0;
/// This near a stop a bus settles whether it serves it at all (m).
pub const SERVE_DECIDE: f32 = 50.0;
/// Longitudinal docking tolerance: the origin must be this close to the berth point (m).
pub const DOCK_LONG_TOL: f32 = 0.5;
/// Lateral docking tolerance (m).
pub const DOCK_LAT_TOL: f32 = 0.25;
/// Speed below which a vehicle may be considered docked (m/s).
pub const DOCK_SPEED: f32 = 0.1;
/// Recover an unreachable docking pose after standing there this long (s).
pub const DOCK_STALL_TIMEOUT: f32 = 8.0;
/// A bus that came to rest this close to the berth point (m, either way) ...
pub const DOCK_SERVE_LONG: f32 = 2.5;
/// ... and this close to the bay laterally (m) serves the stop from where it stands: people
/// still reach the doors, and a bus released from the queue a few metres short of the berth
/// cannot swing fully into the bay. Skipping waiting passengers is the worse outcome.
pub const DOCK_SERVE_LAT: f32 = 0.8;
/// Seconds at rest in such a serviceable pose before the doors open.
pub const DOCK_SETTLE: f32 = 1.5;
/// Distance upstream of the berth a queueing bus waits at (m). A bus waiting here has not
/// served the stop.
pub const QUEUE_STANDOFF: f32 = 10.0;
/// Minimum service time at a stop, whatever the passenger exchange (s).
pub const MIN_SERVICE: f32 = 5.0;
/// Pulling out: at least this long after the doors were told to close (s).
pub const CLOSE_MIN: f32 = 1.5;
/// ... at most this long waiting for a supported script to answer.
pub const CLOSE_MAX: f32 = 12.0;
/// An early bus waits for its departure at a timing point (a stop the timetable marks
/// `always`), but not for more than this (s). At any other stop it leaves once the passenger
/// exchange is over: doors held open with nobody boarding read as a stuck bus.
pub const EARLY_WAIT: f64 = 40.0;
/// A layover waits for its departure however long (s).
pub const LAYOVER_WAIT: f64 = 1800.0;
/// On a layover, the doors open this long before the departure (s).
pub const LAYOVER_BOARDING: f64 = 45.0;
/// A bus more than this early serves every stop and waits (s).
pub const EARLY_STOP: f64 = 120.0;
/// ... and more than this early at a stop marked for it (s).
pub const EARLY_STOP_SHORT: f64 = 20.0;
/// A following vehicle within this distance behind and moving still blocks a merge-out (m).
pub const MERGE_GAP: f32 = 25.0;
/// A standing body this far ahead in the lane blocks a merge-out too (m).
pub const MERGE_AHEAD: f32 = 8.0;
/// How far the rear must clear the berth point before the berth is released (m).
pub const BERTH_CLEAR: f32 = 1.0;

/// Seconds the doors stay open at a stop without anyone holding them.
pub fn boarding_time(id: u64) -> f32 {
    7.0 + (id % 5) as f32
}

/// The side the bus pulls out towards: left (1) from a bay on the right, else right (2).
pub fn out_signal(bay: f32) -> i32 {
    if bay < -0.1 {
        2
    } else {
        1
    }
}

/// The physical boarding geometry of one stop occurrence, with every length named.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct BerthGeometry {
    pub stop: StopId,
    pub occurrence: u32,
    /// Network lane index the berth lies on.
    pub lane: usize,
    /// Index into the vehicle's route of that lane.
    pub route_index: usize,
    pub side: PlatformSide,
    /// Where the vehicle's origin comes to rest along the lane (m).
    pub s: f32,
    /// Lateral offset of the berth from the lane centre (m, positive = right).
    pub bay: f32,
    /// Timetable departure (seconds of the day).
    pub depart: f64,
    /// The stop's own content length, when known. Kept separate from the approach distance
    /// and the vehicle's stop correction.
    pub stop_length: Option<f32>,
    /// Longitudinal half-length of the boarding region around `s` (m).
    pub boarding_region: f32,
    /// Distance before the stop over which the bus pulls into its bay (m).
    pub approach_distance: f32,
    /// Berths available at this stop. One unless validated content geometry proves more.
    pub berths: u8,
}

impl BerthGeometry {
    /// Build the berth from a compiled stop target and the network lane it lies on. The stop
    /// length is not imported yet, so the boarding region is the documented provisional
    /// fallback and the berth count stays one.
    pub fn from_target(target: &StopTarget, lane: usize) -> BerthGeometry {
        BerthGeometry {
            stop: target.stop,
            occurrence: target.occurrence,
            lane,
            route_index: target.route_index,
            side: target.side,
            s: target.s,
            bay: target.bay,
            depart: target.depart,
            stop_length: None,
            boarding_region: DEFAULT_BOARDING_REGION,
            approach_distance: DEFAULT_APPROACH_DISTANCE,
            berths: 1,
        }
    }

    pub fn key(&self) -> (StopId, u32) {
        (self.stop, self.occurrence)
    }

    /// The boarding region along the lane: `[s - region, s + region]`.
    pub fn region(&self) -> (f32, f32) {
        (self.s - self.boarding_region, self.s + self.boarding_region)
    }
}

/// Per-vehicle demand at the front stop.
#[derive(Debug, Clone, Copy, Default)]
pub struct StopDemand {
    /// Somebody wants to board or alight here. `None`: the passenger simulation is off and
    /// every stop is served.
    pub wanted: Option<bool>,
    /// A rail vehicle (train/tram) keeps to its stations regardless of demand.
    pub rail: bool,
}

/// Content-derived stop policy (the timetable's always/early lists and the trip's ends).
#[derive(Debug, Clone, Default, PartialEq)]
pub struct StopPolicy {
    pub always: Vec<i64>,
    pub serve_early: Vec<i64>,
    pub last_stop: Option<i64>,
    /// This is the last stop of the loaded route (with the route closed).
    pub is_last: bool,
    /// The loaded tiles still carry the route on beyond this stop.
    pub route_open: bool,
}

/// How a supported/unsupported door handshake stands, typed at the adapter boundary.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ScriptFeedback {
    /// Not at a close step (boarding or waiting); nothing to classify.
    Idle,
    /// The script acknowledged the doors are shut.
    Released,
    /// The script does not take part in the handshake (it has no `AI_Scheduled_AtStation`):
    /// the documented fixed close interval is the safe fallback.
    Unsupported,
    /// The script exposes the handshake but has not answered, and its doors are known open:
    /// departure must not happen.
    StuckDoorsOpen,
    /// The script exposes the handshake but has not answered, and its door state is unknown.
    StuckUnknown,
}

/// The frozen per-tick realized bodies a service plan reads.
#[derive(Debug, Clone, Copy)]
pub struct ServiceActor {
    pub id: VehicleId,
    pub lane: usize,
    pub s: f32,
    /// Origin to front bumper (m).
    pub front: f32,
    /// Origin to rear bumper (m).
    pub rear: f32,
    pub length: f32,
    pub speed: f32,
    pub lateral: f32,
    /// Standstill gap the vehicle leaves (m).
    pub min_gap: f32,
}

impl ServiceActor {
    pub fn new(id: VehicleId, lane: usize, s: f32) -> ServiceActor {
        ServiceActor {
            id,
            lane,
            s,
            front: 2.25,
            rear: 2.25,
            length: 4.5,
            speed: 0.0,
            lateral: 0.0,
            min_gap: 2.0,
        }
    }
}

/// The frozen world a service plan reads. Built once per tick.
pub struct ServiceScene<'a> {
    pub net: &'a Network,
    pub occupancy: &'a Occupancy,
    pub actors: &'a [ServiceActor],
    pub day_time: f64,
    pub dt: f32,
    pub tick: u64,
}

impl ServiceScene<'_> {
    /// Who this actor is, by stable id.
    pub fn actor_of(&self, id: VehicleId) -> Option<&ServiceActor> {
        self.actors.iter().find(|a| a.id == id)
    }
}

/// One vehicle's service inputs for this tick.
pub struct ServiceInputs {
    /// Index into `ServiceScene::actors`.
    pub actor: usize,
    /// The berth of the vehicle's front stop, if it has one.
    pub berth: Option<BerthGeometry>,
    /// Distance from the vehicle origin to the berth point along its way (m).
    pub distance: f32,
    pub policy: StopPolicy,
    pub demand: StopDemand,
    pub feedback: ScriptFeedback,
    pub passing: bool,
    pub kerb_swerve: Option<f32>,
    /// A junction lies between the bus and the berth, too close for the S-curve into the bay.
    pub junction_first: bool,
}

/// Where the vehicle is in its stop service.
#[derive(Debug, Clone, PartialEq)]
pub struct ServiceState {
    pub phase: ServicePhase,
    /// Seconds in the current phase.
    pub phase_t: f32,
    /// Continuous standstill at an unreachable docking pose (s).
    pub dock_stall_t: f32,
    /// Seconds of boarding left (passengers at the doors hold it open).
    pub boarding_t: f32,
    /// When it may leave the stop (seconds of the day).
    pub leave_at: f64,
    /// Seconds behind (positive) or ahead of the timetable, as of the last stop.
    pub delay: f64,
    /// Put out before its departure: it waits for it at its first stop.
    pub layover: bool,
    /// The berth it currently holds, with its geometry, or `None`.
    pub berth: Option<BerthGeometry>,
    /// The fault that put it in `ServiceFault`, if any.
    pub fault: Option<Reason>,
    /// Settled whether the front stop is served.
    pub serve_decided: bool,
    pub serve: bool,
}

impl Default for ServiceState {
    fn default() -> ServiceState {
        ServiceState {
            phase: ServicePhase::EnRoute,
            phase_t: 0.0,
            dock_stall_t: 0.0,
            boarding_t: 0.0,
            leave_at: 0.0,
            delay: 0.0,
            layover: false,
            berth: None,
            fault: None,
            serve_decided: false,
            serve: true,
        }
    }
}

impl ServiceState {
    pub fn new() -> ServiceState {
        ServiceState::default()
    }

    /// Doors open for people (`AI_Scheduled_AtStation`).
    pub fn at_station(&self) -> bool {
        self.phase == ServicePhase::Boarding
    }

    /// Standing at a stop (boarding, closing, merging, or a layover).
    pub fn at_stop(&self) -> bool {
        self.phase.at_stop()
    }

    pub fn trip_done(&self) -> bool {
        self.phase.trip_done()
    }

    pub fn holds_berth(&self) -> bool {
        self.phase.holds_berth()
    }

    /// A new trip (the tour's next, or the rest of a trip). Any berth held against the old
    /// stops is released; the coordinator is reset by the caller.
    pub fn restart(&mut self, layover: bool) {
        *self = ServiceState {
            phase: ServicePhase::EnRoute,
            layover,
            ..ServiceState::default()
        };
    }
}

/// The coordinator's decision for one vehicle in one tick.
#[derive(Debug, Clone, PartialEq)]
pub struct ServiceDecision {
    pub phase: ServicePhase,
    /// Distance from the vehicle origin to where it must stop (m).
    pub stop_at: Option<f32>,
    /// Lateral target across the lane (m, positive = right).
    pub lateral_target: Option<f32>,
    /// (blinker, duration) to set when pulling out.
    pub signal: Option<(i32, f32)>,
    /// Which door side to hand the script (map code: 0 right, 1 other, 2 both).
    pub door_side: f32,
    /// Boarding permission: true only at a valid berth.
    pub boarding: bool,
    /// The front stop is served and should be advanced (IBIS/route cursor).
    pub consume_stop: bool,
    /// The berth was released this tick.
    pub release_berth: bool,
    pub events: Vec<TraceEvent>,
    pub reasons: Vec<Reason>,
    pub binding: Option<Reason>,
}

impl ServiceDecision {
    fn new(phase: ServicePhase) -> ServiceDecision {
        ServiceDecision {
            phase,
            stop_at: None,
            lateral_target: None,
            signal: None,
            door_side: 0.0,
            boarding: false,
            consume_stop: false,
            release_berth: false,
            events: Vec::new(),
            reasons: Vec::new(),
            binding: None,
        }
    }
}

/// A berth a vehicle wants or holds this tick, so the coordinator can order the queue by
/// stable arrival rather than by container position.
#[derive(Debug, Clone, Copy)]
pub struct BerthIntent {
    pub vehicle: VehicleId,
    pub stop: StopId,
    pub occurrence: u32,
    /// The vehicle currently holds this berth (docking through merge-out).
    pub holds: bool,
}

/// The single owner of berth capacity and the service state transitions.
#[derive(Debug, Clone, Default)]
pub struct ServiceCoordinator {
    /// Berth owner by `(stop, occurrence)`.
    berths: HashMap<(StopId, u32), VehicleId>,
    /// Arrival tick by `(stop, occurrence, vehicle)`: the stable queue order.
    arrivals: HashMap<(StopId, u32, VehicleId), u64>,
    /// This tick's queue order per `(stop, occurrence)`.
    order: HashMap<(StopId, u32), Vec<VehicleId>>,
    tick: u64,
}

impl ServiceCoordinator {
    pub fn new() -> ServiceCoordinator {
        ServiceCoordinator::default()
    }

    /// How many berths are currently assigned (a health/leak check).
    pub fn berth_count(&self) -> usize {
        self.berths.len()
    }

    /// How many vehicles are queued for a berth across every stop (a health/leak check).
    pub fn queued_count(&self) -> usize {
        self.arrivals.len()
    }

    /// Begin a tick: record arrivals, rebuild the per-stop queue order from stable arrival
    /// order, and release berths whose owner is gone.
    pub fn begin_tick(&mut self, intents: &[BerthIntent], tick: u64) {
        self.tick = tick;
        self.order.clear();
        let mut live: HashSet<(StopId, u32, VehicleId)> = HashSet::new();
        let mut by_stop: HashMap<(StopId, u32), Vec<VehicleId>> = HashMap::new();
        for it in intents {
            let key = (it.stop, it.occurrence);
            self.arrivals
                .entry((it.stop, it.occurrence, it.vehicle))
                .or_insert(tick);
            by_stop.entry(key).or_default().push(it.vehicle);
            live.insert((it.stop, it.occurrence, it.vehicle));
        }
        for (key, mut ids) in by_stop {
            ids.sort_by_key(|id| {
                (
                    self.arrivals
                        .get(&(key.0, key.1, *id))
                        .copied()
                        .unwrap_or(tick),
                    *id,
                )
            });
            self.order.insert(key, ids);
        }
        self.arrivals
            .retain(|(s, o, v), _| live.contains(&(*s, *o, *v)));
        self.berths.retain(|key, owner| live.contains(&(key.0, key.1, *owner)));
    }

    /// Who owns the berth at a stop occurrence, if anyone.
    pub fn berth_owner(&self, stop: StopId, occurrence: u32) -> Option<VehicleId> {
        self.berths.get(&(stop, occurrence)).copied()
    }

    /// The stable queue order for a stop occurrence this tick.
    pub fn order_of(&self, stop: StopId, occurrence: u32) -> &[VehicleId] {
        self.order
            .get(&(stop, occurrence))
            .map(|v| v.as_slice())
            .unwrap_or(&[])
    }

    /// Release every berth/arrival record of a vehicle (removal, route change, reset).
    pub fn release(&mut self, id: VehicleId) {
        self.berths.retain(|_, owner| *owner != id);
        self.arrivals.retain(|(_, _, v), _| *v != id);
        for ids in self.order.values_mut() {
            ids.retain(|v| *v != id);
        }
    }

    /// A route change drops a berth that no longer lies on the vehicle's stops.
    pub fn retain_on_way(&mut self, id: VehicleId, stops: &[(StopId, u32)]) {
        self.berths.retain(|key, owner| *owner != id || stops.contains(key));
        self.arrivals
            .retain(|(s, o, v), _| *v != id || stops.contains(&(*s, *o)));
    }

    /// Network invalidation: every berth made against the old version is released.
    pub fn invalidate_network(&mut self) {
        self.berths.clear();
        self.arrivals.clear();
        self.order.clear();
    }

    // ---- planning ----------------------------------------------------------------------

    /// Plan one vehicle's service for this tick. `state` is the single per-vehicle state the
    /// coordinator writes; `input` carries this vehicle's berth, distance and content policy.
    pub fn plan(
        &mut self,
        scene: &ServiceScene,
        state: &mut ServiceState,
        input: &ServiceInputs,
    ) -> ServiceDecision {
        state.phase_t += scene.dt;
        let Some(actor) = scene.actors.get(input.actor).copied() else {
            return ServiceDecision::new(state.phase);
        };
        let id = actor.id;
        let held = state.berth;
        let berth = input.berth.or(held);
        let mut d = ServiceDecision::new(state.phase);
        let Some(berth) = berth else {
            // No stop ahead and none held.
            if matches!(
                state.phase,
                ServicePhase::Approach
                    | ServicePhase::WaitingForBerth
                    | ServicePhase::Docking
                    | ServicePhase::Boarding
                    | ServicePhase::ClosingDoors
                    | ServicePhase::WaitingToMerge
                    | ServicePhase::Layover
            ) {
                state.phase = ServicePhase::RoutePending;
                d.binding = Some(Reason::RoutePending);
                d.reasons.push(Reason::RoutePending);
            }
            d.phase = state.phase;
            return d;
        };

        match state.phase {
            ServicePhase::EnRoute => {
                if input.distance < STOP_REACH {
                    state.phase = ServicePhase::Approach;
                    state.phase_t = 0.0;
                    d.stop_at = Some(input.distance + actor.front + STOP_LINE_GAP);
                    if !input.passing {
                        d.lateral_target = Some(input.kerb_swerve.unwrap_or(0.0));
                    }
                }
            }
            ServicePhase::Approach => {
                if !state.serve_decided && input.distance < SERVE_DECIDE {
                    state.serve_decided = true;
                    state.serve = self.must_serve(state, &berth, scene.day_time, &input.policy, &input.demand);
                }
                if state.serve_decided && !state.serve {
                    state.phase = ServicePhase::EnRoute;
                    state.phase_t = 0.0;
                    state.serve_decided = false;
                    state.serve = true;
                    d.consume_stop = true;
                    d.phase = state.phase;
                    return d;
                }
                d.lateral_target = Some(self.approach_lateral(&actor, &berth, input));
                d.stop_at = Some(input.distance + actor.front + STOP_LINE_GAP);
                if input.distance < berth.approach_distance {
                    if self.berth_free(scene, &berth, id) && self.first_in_order(&berth, id) {
                        self.grant(&berth, id);
                        state.phase = ServicePhase::Docking;
                        state.phase_t = 0.0;
                        state.berth = Some(berth);
                        d.events.push(TraceEvent::BerthGranted {
                            vehicle: id,
                            stop: berth.stop,
                        });
                    } else {
                        state.phase = ServicePhase::WaitingForBerth;
                        state.phase_t = 0.0;
                        d.reasons.push(Reason::BerthBusy);
                        d.binding = Some(Reason::BerthBusy);
                    }
                }
            }
            ServicePhase::WaitingForBerth => {
                if self.berth_free(scene, &berth, id) && self.first_in_order(&berth, id) {
                    self.grant(&berth, id);
                    state.phase = ServicePhase::Docking;
                    state.phase_t = 0.0;
                    state.berth = Some(berth);
                    d.events.push(TraceEvent::BerthGranted {
                        vehicle: id,
                        stop: berth.stop,
                    });
                    d.stop_at = Some(input.distance + actor.front + STOP_LINE_GAP);
                } else {
                    d.reasons.push(Reason::BerthBusy);
                    d.binding = Some(Reason::BerthBusy);
                    d.stop_at = Some(self.queue_hold(&actor, input.distance));
                }
                if !input.passing {
                    d.lateral_target = Some(input.kerb_swerve.unwrap_or(0.0));
                }
            }
            ServicePhase::Docking => {
                d.lateral_target = Some(berth.bay);
                d.stop_at = Some(input.distance + actor.front + STOP_LINE_GAP);
                let lat_err = (actor.lateral - berth.bay).abs();
                let long_ok = input.distance.abs() <= DOCK_LONG_TOL;
                let docked = long_ok && lat_err <= DOCK_LAT_TOL;
                let unreachable = input.distance < -DOCK_LONG_TOL
                    || (long_ok && lat_err > DOCK_LAT_TOL);
                let serviceable =
                    input.distance.abs() <= DOCK_SERVE_LONG && lat_err <= DOCK_SERVE_LAT;
                state.dock_stall_t = if (unreachable || serviceable) && actor.speed < DOCK_SPEED {
                    state.dock_stall_t + scene.dt
                } else {
                    0.0
                };
                if docked && actor.speed < DOCK_SPEED
                    || serviceable && state.dock_stall_t >= DOCK_SETTLE
                {
                    state.dock_stall_t = 0.0;
                    self.enter_boarding(state, &berth, scene.day_time, &actor, &input.policy, &mut d);
                } else if input.distance < -berth.boarding_region || state.dock_stall_t >= DOCK_STALL_TIMEOUT {
                    // The boarding region was overshot: record a missed stop, never open the
                    // doors somewhere up the queue to make it disappear.
                    self.release_key(&berth);
                    state.berth = None;
                    state.fault = Some(Reason::MissedStop);
                    state.serve_decided = false;
                    state.serve = true;
                    d.release_berth = true;
                    d.consume_stop = true;
                    d.events.push(TraceEvent::Fault {
                        vehicle: id,
                        reason: Reason::MissedStop,
                    });
                    // The bus continues to its next stop; it cannot reverse to recover
                    // the pose, and a permanent ServiceFault would block this lane.
                    state.phase = ServicePhase::EnRoute;
                    state.dock_stall_t = 0.0;
                    d.stop_at = None;
                    d.lateral_target = Some(0.0);
                }
            }
            ServicePhase::Boarding => {
                d.boarding = true;
                d.door_side = berth.side.code();
                d.stop_at = Some(actor.front);
                d.lateral_target = Some(berth.bay);
                state.boarding_t -= scene.dt;
                if state.boarding_t <= 0.0 && scene.day_time >= state.leave_at {
                    state.phase = ServicePhase::ClosingDoors;
                    state.phase_t = 0.0;
                    d.events.push(TraceEvent::CloseRequest { vehicle: id });
                }
            }
            ServicePhase::Layover => {
                d.stop_at = Some(actor.front);
                d.lateral_target = Some(berth.bay);
                let board_at = state.leave_at - LAYOVER_BOARDING;
                if scene.day_time >= board_at {
                    state.phase = ServicePhase::Boarding;
                    state.phase_t = 0.0;
                    state.boarding_t = ((state.leave_at - scene.day_time) as f32).max(MIN_SERVICE);
                    d.boarding = true;
                    d.door_side = berth.side.code();
                    d.lateral_target = Some(berth.bay);
                    d.events.push(TraceEvent::BoardingPermission {
                        vehicle: id,
                        stop: berth.stop,
                    });
                }
            }
            ServicePhase::ClosingDoors => {
                d.stop_at = Some(actor.front);
                d.lateral_target = Some(berth.bay);
                d.signal = Some((out_signal(berth.bay), 2.5));
                let (can_depart, fault) = match input.feedback {
                    ScriptFeedback::Released | ScriptFeedback::Unsupported => {
                        (state.phase_t >= CLOSE_MIN, false)
                    }
                    ScriptFeedback::StuckUnknown => (state.phase_t >= CLOSE_MAX, true),
                    ScriptFeedback::StuckDoorsOpen => (false, true),
                    ScriptFeedback::Idle => (false, false),
                };
                if fault && state.phase_t >= CLOSE_MAX && state.fault.is_none() {
                    state.fault = Some(Reason::StationRelease);
                    d.events.push(TraceEvent::Fault {
                        vehicle: id,
                        reason: Reason::StationRelease,
                    });
                }
                if can_depart {
                    if self.merge_blocked(scene, &actor, id) {
                        state.phase = ServicePhase::WaitingToMerge;
                        state.phase_t = 0.0;
                        d.reasons.push(Reason::Leader);
                        d.binding = Some(Reason::Leader);
                    } else {
                        state.phase = ServicePhase::Departing;
                        state.phase_t = 0.0;
                    }
                }
            }
            ServicePhase::WaitingToMerge => {
                d.stop_at = Some(actor.front);
                d.lateral_target = Some(berth.bay);
                d.signal = Some((out_signal(berth.bay), 2.5));
                d.reasons.push(Reason::Leader);
                d.binding = Some(Reason::Leader);
                if !self.merge_blocked(scene, &actor, id) {
                    state.phase = ServicePhase::Departing;
                    state.phase_t = 0.0;
                }
            }
            ServicePhase::Departing => {
                d.lateral_target = Some(0.0);
                d.signal = Some((out_signal(berth.bay), 2.5));
                let held = held.unwrap_or(berth);
                let cleared =
                    actor.lane != held.lane || actor.s - held.s > actor.rear + BERTH_CLEAR;
                if cleared {
                    self.release_key(&held);
                    state.berth = None;
                    d.release_berth = true;
                    d.consume_stop = true;
                    d.events.push(TraceEvent::BerthReleased {
                        vehicle: id,
                        stop: held.stop,
                    });
                    d.events.push(TraceEvent::Departure { vehicle: id });
                    state.phase = if input.policy.is_last && !input.policy.route_open {
                        ServicePhase::NextTrip
                    } else {
                        ServicePhase::EnRoute
                    };
                    state.phase_t = 0.0;
                    state.serve_decided = false;
                    state.serve = true;
                }
            }
            ServicePhase::NextTrip | ServicePhase::OutOfService => {
                d.stop_at = Some(actor.front);
                d.lateral_target = Some(berth.bay);
            }
            ServicePhase::RoutePending => {
                d.stop_at = Some(actor.front);
                d.lateral_target = Some(berth.bay);
                d.reasons.push(Reason::RoutePending);
                d.binding = Some(Reason::RoutePending);
            }
            ServicePhase::ServiceFault(reason) => {
                d.stop_at = Some(actor.front);
                d.binding = Some(reason);
            }
        }
        d.phase = state.phase;
        d
    }

    // ---- helpers -----------------------------------------------------------------------

    /// Whether a vehicle serves the front stop whoever wants it or not: the trip's first (a
    /// layover) and last, the ones its timetable marks always, and any stop it would reach
    /// more than `EARLY_STOP` early (`EARLY_STOP_SHORT` at a marked stop).
    fn must_serve(
        &self,
        state: &ServiceState,
        berth: &BerthGeometry,
        day_time: f64,
        policy: &StopPolicy,
        demand: &StopDemand,
    ) -> bool {
        if demand.rail {
            return true;
        }
        let stop = berth.stop.get();
        let early = berth.depart - day_time;
        policy.is_last
            || policy.last_stop == Some(stop)
            || state.layover
            || early > EARLY_STOP
            || policy.always.contains(&stop)
            || (early > EARLY_STOP_SHORT && policy.serve_early.contains(&stop))
            || demand.wanted.unwrap_or(true)
    }

    fn enter_boarding(
        &mut self,
        state: &mut ServiceState,
        berth: &BerthGeometry,
        day_time: f64,
        actor: &ServiceActor,
        policy: &StopPolicy,
        d: &mut ServiceDecision,
    ) {
        let layover = state.layover;
        state.layover = false;
        let limit = if layover {
            LAYOVER_WAIT
        } else if policy.always.contains(&berth.stop.get()) {
            EARLY_WAIT
        } else {
            0.0
        };
        let wait = (berth.depart - day_time).clamp(0.0, limit);
        state.leave_at = day_time + wait;
        state.boarding_t = boarding_time(actor.id.get());
        state.delay = (day_time + state.boarding_t.max(wait as f32) as f64) - berth.depart;
        d.events.push(TraceEvent::StopArrival {
            vehicle: actor.id,
            stop: berth.stop,
        });
        if layover && wait > LAYOVER_BOARDING + 10.0 {
            state.phase = ServicePhase::Layover;
            state.phase_t = 0.0;
        } else {
            state.phase = ServicePhase::Boarding;
            state.phase_t = 0.0;
            d.boarding = true;
            d.door_side = berth.side.code();
            d.events.push(TraceEvent::BoardingPermission {
                vehicle: actor.id,
                stop: berth.stop,
            });
        }
    }

    fn approach_lateral(
        &self,
        actor: &ServiceActor,
        berth: &BerthGeometry,
        input: &ServiceInputs,
    ) -> f32 {
        if input.passing {
            return actor.lateral;
        }
        if input.distance < berth.approach_distance && input.distance > -25.0 && !input.junction_first
        {
            berth.bay
        } else {
            input.kerb_swerve.unwrap_or(0.0)
        }
    }

    fn queue_hold(&self, actor: &ServiceActor, distance: f32) -> f32 {
        (distance - QUEUE_STANDOFF).max(0.0) + actor.front + STOP_LINE_GAP
    }

    fn first_in_order(&self, berth: &BerthGeometry, id: VehicleId) -> bool {
        self.order
            .get(&berth.key())
            .and_then(|v| v.first().copied())
            .map(|first| first == id)
            .unwrap_or(true)
    }

    fn grant(&mut self, berth: &BerthGeometry, id: VehicleId) {
        self.berths.insert(berth.key(), id);
    }

    fn release_key(&mut self, berth: &BerthGeometry) {
        self.berths.remove(&berth.key());
    }

    /// Whether a realized body other than `ignore` occupies the berth region. A departing bus
    /// still occupies it until its rear clears, so a follower cannot dock through it.
    fn berth_free(&self, scene: &ServiceScene, berth: &BerthGeometry, ignore: VehicleId) -> bool {
        scene
            .occupancy
            .berth_occupancy(
                LaneId(berth.lane),
                berth.s,
                berth.boarding_region.max(1.0),
                &[ignore],
            )
            .is_none()
    }

    /// Whether pulling out of the berth is blocked by a vehicle in the lane: a faster body
    /// close behind, or a standing body ahead within the pull-out ramp.
    fn merge_blocked(&self, scene: &ServiceScene, actor: &ServiceActor, id: VehicleId) -> bool {
        // A bus already aligned with the lane simply pulls away. Followers behind it
        // do not need to grant a lateral merge; following controls the front gap.
        if actor.lateral.abs() <= DOCK_LAT_TOL {
            return false;
        }
        for iv in scene.occupancy.intervals(LaneId(actor.lane)) {
            if iv.owner == id || iv.foreign {
                continue;
            }
            let gap_behind = actor.s - iv.front;
            if gap_behind > -actor.rear && gap_behind < MERGE_GAP && iv.speed > 1.0 {
                return true;
            }
            if iv.front > actor.s && iv.front < actor.s + MERGE_AHEAD && iv.speed < 1.0 {
                return true;
            }
        }
        false
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::capabilities::CapabilitySource;
    use crate::network::{LaneBuilder, LaneKind};

    fn caps() -> VehicleCapabilities {
        VehicleCapabilities::from_extents(
            4.0,
            4.0,
            1.25,
            CapabilitySource::BoundingBox,
            2,
            50.0,
            None,
        )
    }

    fn north_lane() -> Network {
        let lane = LaneBuilder::polyline(
            vec![DVec3::new(0.0, 0.0, 0.0), DVec3::new(0.0, 100.0, 0.0)],
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
    fn a_right_side_stop_on_a_right_hand_map_is_serviceable() {
        let net = north_lane();
        let t = compile_stop_target(
            &net,
            &[LaneId(0)],
            0,
            StopId(1),
            DVec3::new(2.0, 50.0, 0.0),
            PlatformSide::Right,
            &caps(),
            20.0,
            0.0,
            36000.0,
            0,
        )
        .expect("a stop beside the kerb");
        assert!((t.s - 50.0).abs() < 0.5, "s={}", t.s);
        assert!(t.bay > 1.0 && t.bay < 2.5, "bay={}", t.bay);
    }

    #[test]
    fn a_stop_on_the_wrong_side_is_reported_not_moved() {
        let net = north_lane();
        let err = compile_stop_target(
            &net,
            &[LaneId(0)],
            0,
            StopId(1),
            DVec3::new(-2.0, 50.0, 0.0),
            PlatformSide::Right,
            &caps(),
            20.0,
            0.0,
            36000.0,
            0,
        )
        .unwrap_err();
        assert_eq!(err, StopTargetError::WrongSide);
    }

    #[test]
    fn a_route_occurrence_is_kept_separate_from_the_geometry() {
        let net = north_lane();
        let route = [LaneId(0), LaneId(0)];
        let t = compile_stop_target(
            &net,
            &route,
            3,
            StopId(2),
            DVec3::new(2.0, 20.0, 0.0),
            PlatformSide::Right,
            &caps(),
            20.0,
            0.0,
            36000.0,
            1,
        )
        .expect("the second occurrence");
        assert_eq!(t.occurrence, 3);
    }

    #[test]
    fn a_stop_off_the_route_is_invalid() {
        let net = north_lane();
        let err = compile_stop_target(
            &net,
            &[LaneId(0)],
            0,
            StopId(1),
            DVec3::new(200.0, 50.0, 0.0),
            PlatformSide::Right,
            &caps(),
            20.0,
            0.0,
            36000.0,
            0,
        )
        .unwrap_err();
        assert_eq!(err, StopTargetError::NotOnRoute);
    }

    #[test]
    fn platform_side_round_trips_through_its_code() {
        for code in [0.0f32, 1.0, 2.0] {
            assert_eq!(PlatformSide::from_code(code).code(), code);
        }
    }

    #[test]
    fn a_stop_target_keeps_occurrence_and_geometry_separate() {
        let t = StopTarget::new(StopId(99), 3, 5, PlatformSide::Left, 42.0, -1.8, 36000.0);
        assert_eq!(t.stop, StopId(99));
        assert_eq!(t.occurrence, 3);
        assert_eq!(t.route_index, 5);
        assert_eq!(t.side, PlatformSide::Left);
        assert_eq!(t.s, 42.0);
        assert_eq!(t.bay, -1.8);
    }

    // ---- coordinator ---------------------------------------------------------------

    fn berth(s: f32) -> BerthGeometry {
        BerthGeometry {
            stop: StopId(7001),
            occurrence: 0,
            lane: 0,
            route_index: 0,
            side: PlatformSide::Right,
            s,
            bay: 1.6,
            depart: 36000.0,
            stop_length: None,
            boarding_region: DEFAULT_BOARDING_REGION,
            approach_distance: DEFAULT_APPROACH_DISTANCE,
            berths: 1,
        }
    }

    fn scene<'a>(net: &'a Network, occ: &'a Occupancy, actors: &'a [ServiceActor]) -> ServiceScene<'a> {
        ServiceScene {
            net,
            occupancy: occ,
            actors,
            day_time: 36000.0,
            dt: 0.02,
            tick: 0,
        }
    }

    fn inputs(actor: usize, b: Option<BerthGeometry>, dist: f32) -> ServiceInputs {
        ServiceInputs {
            actor,
            berth: b,
            distance: dist,
            policy: StopPolicy::default(),
            demand: StopDemand {
                wanted: Some(true),
                rail: false,
            },
            feedback: ScriptFeedback::Idle,
            passing: false,
            kerb_swerve: None,
            junction_first: false,
        }
    }

    #[test]
    fn a_stop_is_not_served_from_upstream() {
        let net = north_lane();
        let occ = Occupancy::default();
        // Three buses approaching the same stop.
        let actors = vec![
            ServiceActor::new(VehicleId(1), 0, 60.0),
            ServiceActor::new(VehicleId(2), 0, 50.0),
            ServiceActor::new(VehicleId(3), 0, 40.0),
        ];
        let sc = scene(&net, &occ, &actors);
        let mut coord = ServiceCoordinator::new();
        let intents = [
            BerthIntent { vehicle: VehicleId(1), stop: StopId(7001), occurrence: 0, holds: false },
            BerthIntent { vehicle: VehicleId(2), stop: StopId(7001), occurrence: 0, holds: false },
            BerthIntent { vehicle: VehicleId(3), stop: StopId(7001), occurrence: 0, holds: false },
        ];
        coord.begin_tick(&intents, 0);
        // The earliest arrival (lowest id here) is first in the stable order.
        assert_eq!(coord.order_of(StopId(7001), 0)[0], VehicleId(1));
        let mut states: Vec<ServiceState> = (0..3)
            .map(|_| {
                let mut s = ServiceState::new();
                s.phase = ServicePhase::Approach;
                s
            })
            .collect();
        for (i, dist) in [(0usize, 10.0f32), (1, 25.0), (2, 35.0)] {
            let mut inp = inputs(i, Some(berth(70.0)), dist);
            inp.demand.wanted = Some(false);
            inp.policy.always = vec![7001];
            let _ = coord.plan(&sc, &mut states[i], &inp);
        }
        assert!(states[0].berth.is_some(), "the nearest bus should claim the berth");
        assert!(states[1].berth.is_none(), "a queueing bus must not hold the berth");
        assert!(states[2].berth.is_none(), "a queueing bus must not hold the berth");
        assert_eq!(states[1].phase, ServicePhase::WaitingForBerth);
        assert_eq!(states[2].phase, ServicePhase::Approach);
    }

    #[test]
    fn boarding_needs_a_valid_berth() {
        let net = north_lane();
        let occ = Occupancy::default();
        let actors = vec![{
            let mut a = ServiceActor::new(VehicleId(1), 0, 60.0);
            a.lateral = 1.6;
            a
        }];
        let sc = scene(&net, &occ, &actors);
        let mut coord = ServiceCoordinator::new();
        coord.begin_tick(
            &[BerthIntent {
                vehicle: VehicleId(1),
                stop: StopId(7001),
                occurrence: 0,
                holds: false,
            }],
            0,
        );
        let mut st = ServiceState::new();
        st.phase = ServicePhase::Docking;
        st.berth = Some(berth(70.0));
        let mut inp = inputs(0, Some(berth(70.0)), 0.1);
        inp.feedback = ScriptFeedback::Idle;
        let dec = coord.plan(&sc, &mut st, &inp);
        assert!(dec.boarding, "aligned and slow: boarding may start");
        assert_eq!(st.phase, ServicePhase::Boarding);
        // Speed too high: no permission yet.
        let mut st2 = ServiceState::new();
        st2.phase = ServicePhase::Docking;
        st2.berth = Some(berth(70.0));
        let actors2 = vec![{
            let mut a = ServiceActor::new(VehicleId(2), 0, 60.0);
            a.speed = 3.0;
            a.lateral = 1.6;
            a
        }];
        let sc2 = scene(&net, &occ, &actors2);
        let mut coord2 = ServiceCoordinator::new();
        coord2.begin_tick(
            &[BerthIntent {
                vehicle: VehicleId(2),
                stop: StopId(7001),
                occurrence: 0,
                holds: true,
            }],
            0,
        );
        let _ = coord2.plan(&sc2, &mut st2, &inputs(0, Some(berth(70.0)), 0.1));
        assert_eq!(st2.phase, ServicePhase::Docking, "fast bus must not board");
    }

    #[test]
    fn overshoot_records_a_missed_stop_not_a_queued_door() {
        let net = north_lane();
        let occ = Occupancy::default();
        let actors = vec![ServiceActor::new(VehicleId(1), 0, 90.0)];
        let sc = scene(&net, &occ, &actors);
        let mut coord = ServiceCoordinator::new();
        coord.begin_tick(&[], 0);
        let mut st = ServiceState::new();
        st.phase = ServicePhase::Docking;
        st.berth = Some(berth(70.0));
        let dec = coord.plan(&sc, &mut st, &inputs(0, Some(berth(70.0)), -10.0));
        assert!(dec.consume_stop);
        assert!(dec.events.iter().any(|e| matches!(
            e,
            TraceEvent::Fault { reason: Reason::MissedStop, .. }
        )));
    }

    #[test]
    fn an_unsupported_script_departs_after_the_fixed_close_and_a_stuck_open_door_does_not() {
        let net = north_lane();
        let occ = Occupancy::default();
        let actors = vec![ServiceActor::new(VehicleId(1), 0, 70.0)];
        let sc = scene(&net, &occ, &actors);
        let mut coord = ServiceCoordinator::new();
        coord.begin_tick(&[], 0);
        // Unsupported: a fixed close interval suffices.
        let mut st = ServiceState::new();
        st.phase = ServicePhase::ClosingDoors;
        st.berth = Some(berth(70.0));
        st.phase_t = CLOSE_MIN;
        let mut inp = inputs(0, Some(berth(70.0)), 0.0);
        inp.feedback = ScriptFeedback::Unsupported;
        let dec = coord.plan(&sc, &mut st, &inp);
        assert!(matches!(dec.phase, ServicePhase::Departing | ServicePhase::WaitingToMerge));
        // Stuck with known-open doors: never depart.
        let mut st = ServiceState::new();
        st.phase = ServicePhase::ClosingDoors;
        st.berth = Some(berth(70.0));
        st.phase_t = CLOSE_MAX + 1.0;
        let mut inp = inputs(0, Some(berth(70.0)), 0.0);
        inp.feedback = ScriptFeedback::StuckDoorsOpen;
        let dec = coord.plan(&sc, &mut st, &inp);
        assert_eq!(dec.phase, ServicePhase::ClosingDoors);
        assert!(dec.events.iter().any(|e| matches!(e, TraceEvent::Fault { .. })));
    }

    #[test]
    fn an_early_bus_waits_and_a_marked_stop_always_serves() {
        let net = north_lane();
        let occ = Occupancy::default();
        let actors = vec![ServiceActor::new(VehicleId(1), 0, 60.0)];
        let sc = scene(&net, &occ, &actors);
        let mut coord = ServiceCoordinator::new();
        let b = berth(70.0);

        // On time, nobody aboard, not a terminus: skipped.
        let mut st = ServiceState::new();
        st.phase = ServicePhase::Approach;
        let mut inp = inputs(0, Some(b), 10.0);
        inp.demand.wanted = Some(false);
        let dec = coord.plan(&sc, &mut st, &inp);
        assert!(dec.consume_stop, "an unwanted stop in the middle is skipped");
        assert_eq!(st.phase, ServicePhase::EnRoute);

        // More than two minutes early: always served.
        let early = BerthGeometry {
            depart: 36000.0 + EARLY_STOP + 1.0,
            ..b
        };
        let mut st = ServiceState::new();
        st.phase = ServicePhase::Approach;
        let mut inp = inputs(0, Some(early), 10.0);
        inp.demand.wanted = Some(false);
        let dec = coord.plan(&sc, &mut st, &inp);
        assert!(!dec.consume_stop, "a very early bus serves the stop");
        assert_eq!(st.phase, ServicePhase::Docking);
    }
}
