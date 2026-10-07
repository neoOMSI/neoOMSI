//! Typed diagnostics: why a vehicle is constrained, and what transitions it makes.
//!
//! These types replace ad-hoc string hints as the source of truth for a decision's cause.
//! A projection to the legacy short labels keeps the existing `OMSI_TRACE_AI` output
//! working while callers migrate.

use crate::ids::{StopId, TripId, VehicleId};

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

#[cfg(test)]
mod tests {
    use super::*;

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
