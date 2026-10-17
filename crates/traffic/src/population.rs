//! Population: demand, admission, the dormant lifecycle and streaming recovery.
//!
//! Demand (what the map and timetable ask for), admission (may a vehicle be placed now:
//! path, continuation, ground, gap, visibility), physical occupancy (read only) and
//! presentation visibility are kept distinct. Admission is a bounded, deterministic queue
//! with per-entrance backoff: a busy entrance retries later instead of stacking vehicles.
//!
//! The dormant registry keeps identity, class and duty of vehicles out of the active area;
//! reactivation is validated (ground, gap, path) before a body is rebuilt, so dormant motion
//! cannot create an overlap. The admission bounds below are provisional neoOMSI targets.

use crate::diagnostics::{Reason, TraceEvent};
use crate::ids::VehicleId;
use crate::network::{LaneKind, Network};
use crate::perception::Occupancy;
use glam::DVec2;
use hashbrown::{HashMap, HashSet};
use std::collections::VecDeque;

/// Most requests examined in one pass; the rest keep their place in the queue.
pub const ADMIT_PER_PASS: usize = 64;
/// Hard bound on the pending request queue: a busy entrance cannot grow it without bound.
pub const QUEUE_MAX: usize = 256;
/// Shortest and longest retry delay at an entrance (s), doubling with each refusal in a row.
pub const RETRY_MIN: f32 = 0.5;
pub const RETRY_MAX: f32 = 6.0;
/// No body may lie within this distance of a candidate (m).
pub const GAP_MARGIN: f64 = 14.0;
/// An entrance: this long a stretch of one lane (m), sharing one backoff.
pub const ENTRANCE_LENGTH: f32 = GAP_MARGIN as f32;
/// The whole-map dormant population is capped at this multiple of the street target.
pub const DORMANT_CAP_FACTOR: f32 = 8.0;
/// A dormant actor is woken only up to this multiple of the target around the centre.
pub const WAKE_BUDGET_FACTOR: f32 = 1.25;

/// A stable identity for one outstanding spawn request.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct SpawnRequestId(pub u64);

/// What budget a request draws on.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SpawnClass {
    /// A timetable duty (only ever admitted through `scheduled_admission`).
    Scheduled,
    /// Random street traffic.
    Unscheduled,
    /// A dormant actor coming back into the active area.
    Dormant,
    /// A parked car pulling out into the lane.
    ParkedPullOut,
    /// An aircraft on its flight path.
    Air,
}

impl SpawnClass {
    /// Whether this class counts against the random street budget.
    pub fn is_random(self) -> bool {
        matches!(
            self,
            SpawnClass::Unscheduled | SpawnClass::ParkedPullOut
        )
    }

    pub fn label(self) -> &'static str {
        match self {
            SpawnClass::Scheduled => "scheduled",
            SpawnClass::Unscheduled => "unscheduled",
            SpawnClass::Dormant => "dormant",
            SpawnClass::ParkedPullOut => "pull_out",
            SpawnClass::Air => "air",
        }
    }
}

/// A demand for one vehicle: the stable request and where it wants to be placed.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct SpawnRequest {
    pub id: SpawnRequestId,
    pub class: SpawnClass,
    pub lane: usize,
    pub s: f32,
}

/// The content/runtime facts about a placement that only the adapter knows.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SpawnFacts {
    pub path_valid: bool,
    pub continuation: bool,
    pub ground: bool,
    pub visible: bool,
}

impl Default for SpawnFacts {
    fn default() -> Self {
        SpawnFacts {
            path_valid: false,
            continuation: false,
            ground: false,
            visible: false,
        }
    }
}

/// A dormant actor as the adapter presents it, with freshly validated environment facts.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct DormantView {
    pub id: VehicleId,
    pub class: SpawnClass,
    pub duty: bool,
    pub lane: usize,
    pub s: f32,
    pub ground: bool,
    pub visible: bool,
}

/// What the population is asked for this pass, split into distinct budgets.
#[derive(Debug, Clone, Copy, Default, PartialEq)]
pub struct PopulationDemand {
    /// Random street vehicles wanted around the centre.
    pub street_target: usize,
    /// Aircraft wanted (a small constant when the map has flight paths).
    pub air_target: usize,
    /// `[AIMaxCountScheduled]` (0 = unlimited) and how many scheduled vehicles exist now.
    pub scheduled_cap: u32,
    pub scheduled_count: usize,
    /// How many unscheduled vehicles are already counted, and the whole-map dormant cap.
    pub unscheduled_count: usize,
    pub dormant_capacity: usize,
}

/// The frozen per-pass inputs, built once by the adapter.
pub struct PopulationScene<'a> {
    pub net: &'a Network,
    pub occupancy: &'a Occupancy,
    pub demand: PopulationDemand,
    /// The first population fills the view; after that nothing may pop into sight.
    pub initial: bool,
    pub tick: u64,
}

/// The typed outcome of one request.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum SpawnOutcome {
    /// Place the vehicle now.
    Admit,
    /// Refuse it: the cause is diagnosed and the request is dropped.
    Deny(Reason),
    /// Keep it queued and try again after `after` seconds.
    Retry { after: f32 },
}

/// One admission decision for a random request.
#[derive(Debug, Clone, PartialEq)]
pub struct SpawnDecision {
    pub request: SpawnRequest,
    pub outcome: SpawnOutcome,
    pub reasons: Vec<Reason>,
    pub binding: Option<Reason>,
}

impl SpawnDecision {
    fn new(request: SpawnRequest, outcome: SpawnOutcome) -> SpawnDecision {
        let reason = match outcome {
            SpawnOutcome::Deny(r) => Some(r),
            SpawnOutcome::Retry { .. } => Some(Reason::EntranceBusy),
            SpawnOutcome::Admit => None,
        };
        SpawnDecision {
            request,
            outcome,
            reasons: reason.into_iter().collect(),
            binding: reason,
        }
    }
}

/// A reactivation decision for a dormant actor.
#[derive(Debug, Clone, PartialEq)]
pub struct DormantDecision {
    pub id: VehicleId,
    pub outcome: SpawnOutcome,
}

/// Why a vehicle left the active area, so recovery can be classified by cause.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RemovalCause {
    /// Went out of range: resumed as a dormant actor.
    Dormant,
    /// The ground under it was unloaded.
    Unloaded,
    /// Finished its trip and left the map.
    Finished,
    /// Taken over or explicitly removed.
    TakenOver,
    /// Its scheduled route is invalid.
    InvalidRoute,
}

impl RemovalCause {
    pub fn label(self) -> &'static str {
        match self {
            RemovalCause::Dormant => "dormant",
            RemovalCause::Unloaded => "unloaded",
            RemovalCause::Finished => "finished",
            RemovalCause::TakenOver => "taken_over",
            RemovalCause::InvalidRoute => "invalid_route",
        }
    }
}

/// The realized world position a request wants (x/y), if its lane exists.
fn request_point(net: &Network, req: &SpawnRequest) -> Option<DVec2> {
    let lane = net.lanes.get(req.lane)?;
    let s = req.s.clamp(0.0, (lane.length() - 0.1).max(0.0));
    Some(lane.at(s).0.truncate())
}

#[derive(Debug, Clone, Copy, Default)]
struct Entrance {
    /// Refusals in a row.
    denied: u32,
    /// The coordinator time (s) before which the entrance is not asked again.
    retry_at: f32,
}

/// The entrance a request asks at: its lane and stretch of it.
fn entrance_of(req: &SpawnRequest) -> (usize, i32) {
    (req.lane, (req.s / ENTRANCE_LENGTH).floor() as i32)
}

#[derive(Debug, Clone, Copy, PartialEq)]
struct DormantRecord {
    class: SpawnClass,
    duty: bool,
    lane: usize,
    s: f32,
}

/// The single owner of demand, admission and the dormant registry.
#[derive(Debug, Clone, Default)]
pub struct PopulationCoordinator {
    queue: VecDeque<SpawnRequest>,
    next_id: u64,
    time: f32,
    tick: u64,
    entrance: HashMap<(usize, i32), Entrance>,
    dormant: HashMap<VehicleId, DormantRecord>,
    /// Insertion order of the dormant registry, for deterministic wake order.
    dormant_order: Vec<VehicleId>,
    /// Requests denied for capacity, so the adapter can report diagnosed overload.
    capacity_denied: u64,
    admitted_total: u64,
}

impl PopulationCoordinator {
    pub fn new() -> PopulationCoordinator {
        PopulationCoordinator::default()
    }

    /// Begin a pass at `tick`/`time` (s, the clock the entrance backoff runs on).
    pub fn begin_tick(&mut self, tick: u64, time: f32) {
        self.tick = tick;
        self.time = time;
    }

    pub fn tick(&self) -> u64 {
        self.tick
    }

    pub fn queue_len(&self) -> usize {
        self.queue.len()
    }

    /// Whether another request fits in the bounded queue.
    pub fn has_room(&self) -> bool {
        self.queue.len() < QUEUE_MAX
    }

    pub fn admitted_total(&self) -> u64 {
        self.admitted_total
    }

    pub fn capacity_denied(&self) -> u64 {
        self.capacity_denied
    }

    /// Submit a request. `None` when the queue is full: the caller must not stack more demand.
    pub fn request(&mut self, class: SpawnClass, lane: usize, s: f32) -> Option<SpawnRequest> {
        if self.queue.len() >= QUEUE_MAX {
            return None;
        }
        self.next_id += 1;
        let req = SpawnRequest {
            id: SpawnRequestId(self.next_id),
            class,
            lane,
            s,
        };
        self.queue.push_back(req);
        Some(req)
    }

    /// The outstanding requests, in submission order (the adapter annotates them with facts).
    pub fn requests(&self) -> impl Iterator<Item = &SpawnRequest> {
        self.queue.iter()
    }

    /// Admit (or refuse) a scheduled duty against its own budget, outside the random queue.
    pub fn scheduled_admission(&self, demand: &PopulationDemand) -> SpawnOutcome {
        if demand.scheduled_cap > 0 && demand.scheduled_count >= demand.scheduled_cap as usize {
            SpawnOutcome::Deny(Reason::AtCapacity)
        } else {
            SpawnOutcome::Admit
        }
    }

    /// The shared per-class budget test for the random queue.
    fn budget_ok(
        &self,
        class: SpawnClass,
        demand: &PopulationDemand,
        admitted_this_pass: usize,
    ) -> bool {
        match class {
            SpawnClass::Air => demand.air_target > 0,
            _ if class.is_random() => {
                demand.unscheduled_count + admitted_this_pass < demand.street_target
            }
            _ => true,
        }
    }

    /// Drain the queue into decisions in strict submission order; retried requests stay queued.
    pub fn plan(
        &mut self,
        scene: &PopulationScene,
        facts: &HashMap<SpawnRequestId, SpawnFacts>,
    ) -> Vec<SpawnDecision> {
        let mut out = Vec::new();
        let mut kept: VecDeque<SpawnRequest> = VecDeque::with_capacity(self.queue.len());
        let mut attempts = 0usize;
        let mut admitted_random = 0usize;
        // Placements admitted this pass, not in the occupancy yet.
        let mut pass_spots: Vec<DVec2> = Vec::new();
        while let Some(req) = self.queue.pop_front() {
            if attempts >= ADMIT_PER_PASS {
                kept.push_back(req);
                continue;
            }
            // Requests without facts this pass keep their place.
            let Some(&f) = facts.get(&req.id) else {
                kept.push_back(req);
                continue;
            };
            // The entrance is still backing off.
            if self
                .entrance
                .get(&entrance_of(&req))
                .is_some_and(|e| self.time < e.retry_at)
            {
                kept.push_back(req);
                continue;
            }
            attempts += 1;
            if !self.budget_ok(req.class, &scene.demand, admitted_random) {
                self.capacity_denied += 1;
                out.push(SpawnDecision::new(
                    req,
                    SpawnOutcome::Deny(Reason::AtCapacity),
                ));
                continue;
            }
            if !f.path_valid || !f.continuation {
                out.push(SpawnDecision::new(req, SpawnOutcome::Deny(Reason::NoPath)));
                continue;
            }
            if !f.ground {
                out.push(SpawnDecision::new(req, SpawnOutcome::Deny(Reason::NoGround)));
                continue;
            }
            if !scene.initial && !f.visible {
                let after = self.note_denied(&req);
                out.push(SpawnDecision::new(req, SpawnOutcome::Retry { after }));
                kept.push_back(req);
                continue;
            }
            if !self.gap_open(scene, &req, &pass_spots) {
                let after = self.note_denied(&req);
                out.push(SpawnDecision::new(req, SpawnOutcome::Retry { after }));
                kept.push_back(req);
                continue;
            }
            self.entrance.remove(&entrance_of(&req));
            self.admitted_total += 1;
            if req.class.is_random() {
                admitted_random += 1;
            }
            if let Some(p) = request_point(scene.net, &req) {
                pass_spots.push(p);
            }
            out.push(SpawnDecision::new(req, SpawnOutcome::Admit));
        }
        self.queue = kept;
        let asked: HashSet<(usize, i32)> = self.queue.iter().map(entrance_of).collect();
        self.entrance.retain(|k, _| asked.contains(k));
        out
    }

    /// Whether there is a physical gap at the placement, including this pass's admissions.
    fn gap_open(&self, scene: &PopulationScene, req: &SpawnRequest, extra: &[DVec2]) -> bool {
        let Some(lane) = scene.net.lanes.get(req.lane) else {
            return false;
        };
        // Aircraft share the plan position with the road below but not its space.
        if lane.kind == LaneKind::Air {
            return true;
        }
        let Some(p) = request_point(scene.net, req) else {
            return false;
        };
        if extra.iter().any(|q| (p - *q).length() < GAP_MARGIN) {
            return false;
        }
        let mut blocked = false;
        scene.occupancy.near(p, GAP_MARGIN, |_| blocked = true);
        !blocked
    }

    /// Record a refusal at the request's entrance and return the backed-off retry delay.
    fn note_denied(&mut self, req: &SpawnRequest) -> f32 {
        let time = self.time;
        let e = self.entrance.entry(entrance_of(req)).or_default();
        e.denied = e.denied.saturating_add(1);
        let delay = (RETRY_MIN * 2f32.powi(e.denied.min(16) as i32 - 1)).min(RETRY_MAX);
        e.retry_at = time + delay;
        delay
    }

    /// Validate the dormant registry and return which actors may be placed back now; a
    /// blocked actor stays dormant.
    pub fn plan_dormant(
        &mut self,
        scene: &PopulationScene,
        views: &[DormantView],
    ) -> Vec<DormantDecision> {
        let mut out = Vec::new();
        let mut budget = ((scene.demand.street_target as f32) * WAKE_BUDGET_FACTOR).ceil() as usize;
        budget = budget.saturating_sub(scene.demand.unscheduled_count);
        let mut pass_spots: Vec<DVec2> = Vec::new();
        for v in views {
            let Some(rec) = self.dormant.get(&v.id).copied() else {
                continue;
            };
            if budget == 0 {
                out.push(DormantDecision {
                    id: v.id,
                    outcome: SpawnOutcome::Retry {
                        after: RETRY_MAX,
                    },
                });
                continue;
            }
            if !v.ground {
                out.push(DormantDecision {
                    id: v.id,
                    outcome: SpawnOutcome::Deny(Reason::NoGround),
                });
                continue;
            }
            if !scene.initial && !v.visible {
                out.push(DormantDecision {
                    id: v.id,
                    outcome: SpawnOutcome::Retry {
                        after: RETRY_MIN,
                    },
                });
                continue;
            }
            let req = SpawnRequest {
                id: SpawnRequestId(0),
                class: rec.class,
                lane: v.lane,
                s: v.s,
            };
            if !self.gap_open(scene, &req, &pass_spots) {
                out.push(DormantDecision {
                    id: v.id,
                    outcome: SpawnOutcome::Retry {
                        after: RETRY_MIN,
                    },
                });
                continue;
            }
            if let Some(p) = request_point(scene.net, &req) {
                pass_spots.push(p);
            }
            out.push(DormantDecision {
                id: v.id,
                outcome: SpawnOutcome::Admit,
            });
            budget -= 1;
        }
        out
    }

    /// Register a vehicle that has gone out of range. Identity, class and duty are kept.
    pub fn enter_dormant(
        &mut self,
        id: VehicleId,
        class: SpawnClass,
        duty: bool,
        lane: usize,
        s: f32,
    ) {
        if self.dormant.insert(
            id,
            DormantRecord {
                class,
                duty,
                lane,
                s,
            },
        )
        .is_none()
        {
            self.dormant_order.push(id);
        }
    }

    /// A dormant actor was validated and placed back: drop the registry entry.
    pub fn note_reactivated(&mut self, id: VehicleId) {
        if self.dormant.remove(&id).is_some() {
            self.dormant_order.retain(|v| *v != id);
        }
    }

    /// Update a dormant actor's kinematic progress (kept by the adapter).
    pub fn update_dormant(&mut self, id: VehicleId, lane: usize, s: f32) {
        if let Some(rec) = self.dormant.get_mut(&id) {
            rec.lane = lane;
            rec.s = s;
        }
    }

    pub fn contains_dormant(&self, id: VehicleId) -> bool {
        self.dormant.contains_key(&id)
    }

    pub fn dormant_len(&self) -> usize {
        self.dormant.len()
    }

    pub fn dormant_duty_count(&self) -> usize {
        self.dormant.values().filter(|r| r.duty).count()
    }

    /// Dormant ids in stable registry order.
    pub fn dormant_ids(&self) -> &[VehicleId] {
        &self.dormant_order
    }

    /// Whether the map-wide dormant population may grow by more.
    pub fn dormant_has_room(&self, capacity: usize) -> bool {
        self.dormant.len() < capacity
    }

    /// Release a vehicle. True exactly once, if it was a known dormant actor.
    pub fn release(&mut self, id: VehicleId, _cause: RemovalCause) -> bool {
        let was_dormant = self.dormant.remove(&id).is_some();
        if was_dormant {
            self.dormant_order.retain(|v| *v != id);
        }
        was_dormant
    }

    /// Drop a dormant registration whose lane is no longer on the vehicle's way.
    pub fn retain_on_way(&mut self, id: VehicleId, way: &[usize]) {
        if self
            .dormant
            .get(&id)
            .map(|r| !way.contains(&r.lane))
            .unwrap_or(false)
        {
            self.note_reactivated(id);
        }
    }

    /// The network was rebuilt: drop queued demand; the dormant registry survives (lane
    /// indices are stable).
    pub fn invalidate_network(&mut self) {
        self.queue.clear();
        self.entrance.clear();
    }

    /// Remove every dormant registration (population reset).
    pub fn clear(&mut self) {
        self.queue.clear();
        self.entrance.clear();
        self.dormant.clear();
        self.dormant_order.clear();
    }

    /// Forward a spawn decision's typed events to a caller-supplied sink.
    pub fn events_for(decision: &SpawnDecision) -> Vec<TraceEvent> {
        match decision.outcome {
            SpawnOutcome::Admit => Vec::new(),
            SpawnOutcome::Deny(reason) => vec![TraceEvent::SpawnDenied { reason }],
            SpawnOutcome::Retry { .. } => vec![],
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::network::{LaneBuilder, LaneKind};
    use glam::DVec3;

    fn net() -> Network {
        let mut net = Network {
            lanes: vec![LaneBuilder::polyline(
                vec![DVec3::new(0.0, 0.0, 0.0), DVec3::new(0.0, 400.0, 0.0)],
                LaneKind::Street,
                3.0,
            )],
            ..Default::default()
        };
        net.link(1.5);
        net
    }

    fn scene<'a>(net: &'a Network, occ: &'a Occupancy, initial: bool) -> PopulationScene<'a> {
        PopulationScene {
            net,
            occupancy: occ,
            demand: PopulationDemand {
                street_target: 10,
                unscheduled_count: 0,
                dormant_capacity: 80,
                ..Default::default()
            },
            initial,
            tick: 0,
        }
    }

    fn facts(visible: bool) -> SpawnFacts {
        SpawnFacts {
            path_valid: true,
            continuation: true,
            ground: true,
            visible,
        }
    }

    #[test]
    fn a_full_queue_is_bounded() {
        let mut c = PopulationCoordinator::new();
        for i in 0..QUEUE_MAX {
            assert!(c.request(SpawnClass::Unscheduled, 0, i as f32).is_some());
        }
        assert_eq!(c.queue_len(), QUEUE_MAX);
        assert!(c.request(SpawnClass::Unscheduled, 0, 0.0).is_none());
    }

    #[test]
    fn an_initial_spawn_is_admitted_and_leaves_the_queue() {
        let net = net();
        let occ = Occupancy::default();
        let mut c = PopulationCoordinator::new();
        let req = c.request(SpawnClass::Unscheduled, 0, 100.0).unwrap();
        let mut f = HashMap::new();
        f.insert(req.id, facts(true));
        let out = c.plan(&scene(&net, &occ, true), &f);
        assert_eq!(out.len(), 1);
        assert_eq!(out[0].outcome, SpawnOutcome::Admit);
        assert_eq!(c.queue_len(), 0);
    }

    #[test]
    fn a_capacity_over_budget_request_is_denied_not_queued() {
        let net = net();
        let occ = Occupancy::default();
        let mut c = PopulationCoordinator::new();
        let req = c.request(SpawnClass::Unscheduled, 0, 100.0).unwrap();
        let mut f = HashMap::new();
        f.insert(req.id, facts(true));
        let mut s = scene(&net, &occ, true);
        s.demand.street_target = 0;
        s.demand.unscheduled_count = 0;
        let out = c.plan(&s, &f);
        assert_eq!(out[0].outcome, SpawnOutcome::Deny(Reason::AtCapacity));
        assert_eq!(c.queue_len(), 0);
        assert_eq!(c.capacity_denied(), 1);
    }

    #[test]
    fn no_ground_and_bad_path_have_typed_reasons() {
        let net = net();
        let occ = Occupancy::default();
        let mut c = PopulationCoordinator::new();
        let req = c.request(SpawnClass::Unscheduled, 0, 100.0).unwrap();
        let mut f = HashMap::new();
        f.insert(
            req.id,
            SpawnFacts {
                ground: false,
                ..facts(true)
            },
        );
        assert_eq!(
            c.plan(&scene(&net, &occ, true), &f)[0].outcome,
            SpawnOutcome::Deny(Reason::NoGround)
        );

        let req = c.request(SpawnClass::Unscheduled, 0, 100.0).unwrap();
        let mut f = HashMap::new();
        f.insert(
            req.id,
            SpawnFacts {
                path_valid: false,
                ..facts(true)
            },
        );
        assert_eq!(
            c.plan(&scene(&net, &occ, true), &f)[0].outcome,
            SpawnOutcome::Deny(Reason::NoPath)
        );
    }

    #[test]
    fn an_invisible_first_after_load_request_retries_and_stays_queued() {
        let net = net();
        let occ = Occupancy::default();
        let mut c = PopulationCoordinator::new();
        let req = c.request(SpawnClass::Unscheduled, 0, 100.0).unwrap();
        let mut f = HashMap::new();
        f.insert(req.id, facts(false));
        let out = c.plan(&scene(&net, &occ, false), &f);
        assert!(matches!(out[0].outcome, SpawnOutcome::Retry { .. }));
        assert_eq!(c.queue_len(), 1);
    }

    #[test]
    fn scheduled_admission_respects_its_own_cap() {
        let c = PopulationCoordinator::new();
        let d = PopulationDemand {
            scheduled_cap: 2,
            scheduled_count: 2,
            ..Default::default()
        };
        assert_eq!(c.scheduled_admission(&d), SpawnOutcome::Deny(Reason::AtCapacity));
        let d = PopulationDemand {
            scheduled_cap: 0,
            scheduled_count: 99,
            ..Default::default()
        };
        assert_eq!(c.scheduled_admission(&d), SpawnOutcome::Admit);
    }

    #[test]
    fn dormant_identity_survives_and_reactivation_is_once_only() {
        let net = net();
        let occ = Occupancy::default();
        let mut c = PopulationCoordinator::new();
        let id = VehicleId(7);
        c.enter_dormant(id, SpawnClass::Unscheduled, false, 0, 50.0);
        assert!(c.contains_dormant(id));
        assert_eq!(c.dormant_len(), 1);
        c.enter_dormant(id, SpawnClass::Unscheduled, false, 0, 60.0);
        assert_eq!(c.dormant_len(), 1);
        let view = DormantView {
            id,
            class: SpawnClass::Unscheduled,
            duty: false,
            lane: 0,
            s: 60.0,
            ground: true,
            visible: true,
        };
        let out = c.plan_dormant(&scene(&net, &occ, true), &[view]);
        assert_eq!(out[0].outcome, SpawnOutcome::Admit);
        assert!(c.release(id, RemovalCause::Dormant));
        assert!(!c.contains_dormant(id));
        assert!(!c.release(id, RemovalCause::Dormant));
    }

    #[test]
    fn a_dormant_actor_is_removed_once_only() {
        let mut c = PopulationCoordinator::new();
        let id = VehicleId(9);
        c.enter_dormant(id, SpawnClass::Unscheduled, true, 0, 10.0);
        assert_eq!(c.dormant_duty_count(), 1);
        assert!(c.release(id, RemovalCause::Unloaded));
        assert!(!c.release(id, RemovalCause::Unloaded));
        assert_eq!(c.dormant_duty_count(), 0);
    }

    #[test]
    fn a_blocked_dormant_actor_is_not_reactivated() {
        let net = net();
        let occ = Occupancy::default();
        let mut c = PopulationCoordinator::new();
        let id = VehicleId(3);
        c.enter_dormant(id, SpawnClass::Unscheduled, false, 0, 40.0);
        let view = DormantView {
            id,
            class: SpawnClass::Unscheduled,
            duty: false,
            lane: 0,
            s: 40.0,
            ground: false,
            visible: true,
        };
        let out = c.plan_dormant(&scene(&net, &occ, true), &[view]);
        assert_eq!(out[0].outcome, SpawnOutcome::Deny(Reason::NoGround));
        assert!(c.contains_dormant(id));
    }
}
