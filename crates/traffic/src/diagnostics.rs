//! Typed diagnostics: why a vehicle is constrained, and what transitions it makes.
//!
//! These types replace ad-hoc string hints as the source of truth for a decision's cause.
//! A projection to the legacy short labels keeps the existing `OMSI_TRACE_AI` output
//! working while callers migrate.

use crate::ids::{LaneId, NetworkVersion, StopId, TripId, VehicleId};

/// Version of the capture schema. Any field addition, removal, or semantic change bumps it.
pub const TRACE_VERSION: u32 = 1;

/// Why a vehicle cannot proceed at full freedom. Every active cause is preserved; one of
/// them is the binding constraint.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Reason {
    RedSignal,
    Amber,
    Yield,
    OccupiedExit,
    JunctionClaim,
    Leader,
    Pedestrian,
    BerthBusy,
    DoorHold,
    StationRelease,
    RoutePending,
    InvalidRoute,
    SpeedLimit,
    Curvature,
    StopTarget,
    Parking,
    PullOut,
    Passing,
    Emergency,
    StaleClaim,
    /// A mechanism whose semantics are not yet established, kept as data.
    Unknown(u16),
}

impl Reason {
    /// The short label used by the existing frame trace.
    pub fn label(self) -> &'static str {
        match self {
            Reason::RedSignal => "red",
            Reason::Amber => "amber",
            Reason::Yield => "yield",
            Reason::OccupiedExit => "exit",
            Reason::JunctionClaim => "claim",
            Reason::Leader => "leader",
            Reason::Pedestrian => "ped",
            Reason::BerthBusy => "berth",
            Reason::DoorHold => "door",
            Reason::StationRelease => "release",
            Reason::RoutePending => "route",
            Reason::InvalidRoute => "bad_route",
            Reason::SpeedLimit => "speed",
            Reason::Curvature => "curve",
            Reason::StopTarget => "stop",
            Reason::Parking => "park",
            Reason::PullOut => "pullout",
            Reason::Passing => "pass",
            Reason::Emergency => "emergency",
            Reason::StaleClaim => "stale",
            Reason::Unknown(_) => "unknown",
        }
    }

    /// Whether a wait for this reason is legitimate service rather than a fault.
    pub fn is_valid_wait(self) -> bool {
        matches!(
            self,
            Reason::RedSignal
                | Reason::Amber
                | Reason::BerthBusy
                | Reason::DoorHold
                | Reason::StationRelease
                | Reason::StopTarget
                | Reason::Parking
        )
    }
}

/// A single active cause with its owner and provenance.
#[derive(Debug, Clone, PartialEq)]
pub struct Constraint {
    pub reason: Reason,
    /// Who or what causes it (blocker/claim owner), when known.
    pub owner: Option<VehicleId>,
    /// A stop the constraint applies to, when relevant.
    pub stop: Option<StopId>,
    /// Route-relative stopping location (m) if the constraint stops the vehicle.
    pub stop_at: Option<f32>,
    /// Speed bound (m/s) if the constraint only limits speed.
    pub speed_bound: Option<f32>,
    /// Where the constraint came from (rule, occupancy, service, ...).
    pub provenance: &'static str,
}

impl Constraint {
    pub fn new(reason: Reason, provenance: &'static str) -> Constraint {
        Constraint {
            reason,
            owner: None,
            stop: None,
            stop_at: None,
            speed_bound: None,
            provenance,
        }
    }

    pub fn with_owner(mut self, owner: VehicleId) -> Constraint {
        self.owner = Some(owner);
        self
    }
}

/// Where a vehicle is in a junction movement.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum JunctionState {
    Approaching,
    Waiting,
    Admitted,
    Inside,
    Cleared,
}

/// Where a scheduled vehicle is in its stop service.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ServicePhase {
    EnRoute,
    Approach,
    WaitingForBerth,
    Docking,
    Boarding,
    ClosingDoors,
    WaitingToMerge,
    Departing,
    Layover,
    NextTrip,
    OutOfService,
    RoutePending,
    ServiceFault(Reason),
}

/// How a wait is classified, so legitimate service is not treated as an error.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum WaitClass {
    Valid,
    Suspect,
    Error,
}

/// A lifecycle transition, emitted exactly once per change.
#[derive(Debug, Clone, PartialEq)]
pub enum TraceEvent {
    StopArrival {
        vehicle: VehicleId,
        stop: StopId,
    },
    BoardingPermission {
        vehicle: VehicleId,
        stop: StopId,
    },
    CloseRequest {
        vehicle: VehicleId,
    },
    Departure {
        vehicle: VehicleId,
    },
    TripComplete {
        vehicle: VehicleId,
        trip: TripId,
    },
    DutyHandover {
        vehicle: VehicleId,
        duty: u64,
    },
    Fault {
        vehicle: VehicleId,
        reason: Reason,
    },
    Removal {
        vehicle: VehicleId,
        reason: Reason,
    },
    ClaimGranted {
        vehicle: VehicleId,
    },
    ClaimReleased {
        vehicle: VehicleId,
    },
    BerthGranted {
        vehicle: VehicleId,
        stop: StopId,
    },
    BerthReleased {
        vehicle: VehicleId,
        stop: StopId,
    },
    SpawnAdmitted {
        vehicle: VehicleId,
    },
    SpawnDenied {
        reason: Reason,
    },
}

/// A per-vehicle record in one frozen tick.
#[derive(Debug, Clone, PartialEq)]
pub struct VehicleSnapshot {
    pub id: VehicleId,
    pub lane: LaneId,
    pub s: f32,
    pub speed: f32,
    pub front: f32,
    pub rear: f32,
    /// Every active cause, not just the nearest.
    pub constraints: Vec<Reason>,
    /// The cause that currently binds.
    pub binding: Option<Reason>,
}

impl VehicleSnapshot {
    pub fn is_stationary(&self) -> bool {
        self.speed < 0.05
    }
}

/// One frozen tick of the world.
#[derive(Debug, Clone, PartialEq)]
pub struct TickSnapshot {
    pub tick: u64,
    pub sim_time: f64,
    pub network_version: NetworkVersion,
    pub vehicles: Vec<VehicleSnapshot>,
}

/// Header of a self-contained capture.
#[derive(Debug, Clone, PartialEq)]
pub struct TraceHeader {
    pub trace_version: u32,
    pub source_revision: String,
    pub platform: String,
    pub seed: u64,
    pub tick_hz: f32,
    pub network_version: NetworkVersion,
    pub input_digest: u64,
}

/// Why the rolling buffer was persisted.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CaptureTrigger {
    StationaryWithoutReason,
    CyclicBlocker,
    BodyOverlap,
    ContradictoryClaim,
    InvalidRoute,
    ImpossibleService,
}

/// A bounded rolling trace with a decision/event hash and automatic capture.
#[derive(Debug, Clone)]
pub struct Capture {
    pub header: TraceHeader,
    pub capacity: usize,
    pub ticks: std::collections::VecDeque<TickSnapshot>,
    pub events: Vec<TraceEvent>,
    pub trigger: Option<CaptureTrigger>,
    hash: u64,
}

const FNV_OFFSET: u64 = 0xcbf2_9ce4_8422_2325;
const FNV_PRIME: u64 = 0x0000_0100_0000_01b3;

fn fnv(mut h: u64, bytes: &[u8]) -> u64 {
    for &b in bytes {
        h ^= b as u64;
        h = h.wrapping_mul(FNV_PRIME);
    }
    h
}

impl Capture {
    pub fn new(header: TraceHeader, capacity: usize) -> Capture {
        Capture {
            header,
            capacity: capacity.max(1),
            ticks: std::collections::VecDeque::new(),
            events: Vec::new(),
            trigger: None,
            hash: FNV_OFFSET,
        }
    }

    /// Record a frozen tick; the oldest record is dropped beyond capacity.
    pub fn push_tick(&mut self, snap: TickSnapshot) {
        let mut h = self.hash;
        h = fnv(h, &snap.tick.to_le_bytes());
        h = fnv(h, &snap.sim_time.to_bits().to_le_bytes());
        for v in &snap.vehicles {
            h = fnv(h, &v.id.get().to_le_bytes());
            h = fnv(h, &v.s.to_bits().to_le_bytes());
            h = fnv(h, &v.speed.to_bits().to_le_bytes());
            for c in &v.constraints {
                h = fnv(h, c.label().as_bytes());
            }
        }
        self.hash = h;
        self.ticks.push_back(snap);
        while self.ticks.len() > self.capacity {
            self.ticks.pop_front();
        }
    }

    /// Emit a lifecycle event exactly once.
    pub fn emit(&mut self, event: TraceEvent) {
        self.hash = fnv(self.hash, format!("{event:?}").as_bytes());
        self.events.push(event);
    }

    /// Mark the buffer for persistence; the first trigger wins.
    pub fn note_trigger(&mut self, trigger: CaptureTrigger) {
        if self.trigger.is_none() {
            self.trigger = Some(trigger);
        }
    }

    /// The rolling hash of granted actions and emitted events.
    pub fn decision_hash(&self) -> u64 {
        self.hash
    }

    pub fn captured(&self) -> bool {
        self.trigger.is_some()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn header() -> TraceHeader {
        TraceHeader {
            trace_version: TRACE_VERSION,
            source_revision: "test".into(),
            platform: "test".into(),
            seed: 1,
            tick_hz: 50.0,
            network_version: NetworkVersion(1),
            input_digest: 0,
        }
    }

    fn snap(tick: u64, s: f32, binding: Option<Reason>) -> TickSnapshot {
        TickSnapshot {
            tick,
            sim_time: tick as f64 * 0.02,
            network_version: NetworkVersion(1),
            vehicles: vec![VehicleSnapshot {
                id: VehicleId(1),
                lane: LaneId(0),
                s,
                speed: 1.0,
                front: 2.0,
                rear: 2.0,
                constraints: binding.into_iter().collect(),
                binding,
            }],
        }
    }

    #[test]
    fn the_rolling_buffer_keeps_only_the_last_n_ticks() {
        let mut c = Capture::new(header(), 3);
        for t in 0..10 {
            c.push_tick(snap(t, t as f32, None));
        }
        assert_eq!(c.ticks.len(), 3);
        assert_eq!(c.ticks.front().unwrap().tick, 7);
        assert_eq!(c.ticks.back().unwrap().tick, 9);
    }

    #[test]
    fn the_same_inputs_hash_the_same() {
        let mut a = Capture::new(header(), 8);
        let mut b = Capture::new(header(), 8);
        for t in 0..5 {
            a.push_tick(snap(t, t as f32, Some(Reason::Leader)));
            b.push_tick(snap(t, t as f32, Some(Reason::Leader)));
        }
        assert_eq!(a.decision_hash(), b.decision_hash());
    }

    #[test]
    fn the_first_trigger_wins_and_persists_the_capture() {
        let mut c = Capture::new(header(), 8);
        assert!(!c.captured());
        c.note_trigger(CaptureTrigger::BodyOverlap);
        c.note_trigger(CaptureTrigger::InvalidRoute);
        assert_eq!(c.trigger, Some(CaptureTrigger::BodyOverlap));
        assert!(c.captured());
    }

    #[test]
    fn red_and_boarding_are_valid_waits_but_stale_claims_are_not() {
        assert!(Reason::RedSignal.is_valid_wait());
        assert!(Reason::BerthBusy.is_valid_wait());
        assert!(!Reason::StaleClaim.is_valid_wait());
        assert!(!Reason::Leader.is_valid_wait());
    }

    #[test]
    fn constraints_keep_owner_and_provenance() {
        let c = Constraint::new(Reason::Leader, "following")
            .with_owner(VehicleId(3));
        assert_eq!(c.reason, Reason::Leader);
        assert_eq!(c.owner, Some(VehicleId(3)));
        assert_eq!(c.provenance, "following");
    }

    #[test]
    fn every_reason_has_a_short_label() {
        for r in [
            Reason::RedSignal,
            Reason::Yield,
            Reason::Unknown(7),
            Reason::Emergency,
        ] {
            assert!(!r.label().is_empty());
        }
    }
}
