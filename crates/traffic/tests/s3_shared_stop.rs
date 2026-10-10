//! S3 — three buses sharing one curb stop.
//!
//! A headless contract scenario: one berth, three buses. It exercises the service-phase
//! contract added in Stage 1 and asserts the invariants the Stage 6 berth coordinator must
//! preserve. It needs no renderer or OMSI assets.
//!
//! The full berth queueing/arbitration itself is Stage 6; this test pins the *contract*:
//! - exactly one berth holder at any time;
//! - only a bus in `Boarding` may open its doors (never a bus still upstream);
//! - each bus serves the stop exactly once;
//! - a stop target keeps its route occurrence and platform side separate from its geometry.

use traffic::{PlatformSide, ServicePhase, StopId, StopTarget};

fn stop_target() -> StopTarget {
    StopTarget::new(StopId(7001), 0, 0, PlatformSide::Right, 120.0, -1.8, 36000.0)
}

fn berth_holders(phases: &[ServicePhase]) -> usize {
    phases.iter().filter(|p| p.holds_berth()).count()
}

#[test]
fn s3_one_berth_served_once_each() {
    let target = stop_target();
    assert_eq!(target.side, PlatformSide::Right);
    assert_eq!(target.occurrence, 0);

    let mut served = [0u32; 3];
    let mut phase = [
        ServicePhase::Approach,
        ServicePhase::Approach,
        ServicePhase::EnRoute,
    ];

    // Bus 1 docks and boards; buses 2 and 3 wait upstream. Only bus 1 may open its doors.
    phase[0] = ServicePhase::Docking;
    phase[0] = ServicePhase::Boarding;
    phase[1] = ServicePhase::WaitingForBerth;
    phase[2] = ServicePhase::Approach;

    assert_eq!(berth_holders(&phase), 1, "exactly one berth holder");
    for (i, p) in phase.iter().enumerate() {
        if p.may_board() {
            served[i] += 1;
        }
    }
    assert_eq!(served, [1, 0, 0], "only the docked bus boards");

    // Bus 1 closes, merges out and releases the berth; bus 2 docks and boards.
    phase[0] = ServicePhase::ClosingDoors;
    phase[0] = ServicePhase::WaitingToMerge;
    phase[0] = ServicePhase::Departing;
    phase[1] = ServicePhase::Docking;
    phase[1] = ServicePhase::Boarding;
    phase[2] = ServicePhase::WaitingForBerth;

    assert_eq!(berth_holders(&phase), 1, "still exactly one berth holder");
    for (i, p) in phase.iter().enumerate() {
        if p.may_board() {
            served[i] += 1;
        }
    }
    assert_eq!(served, [1, 1, 0], "bus 3 never boards while upstream");

    // Bus 2 leaves; bus 3 finally docks and boards.
    phase[1] = ServicePhase::Departing;
    phase[2] = ServicePhase::Docking;
    phase[2] = ServicePhase::Boarding;

    assert_eq!(berth_holders(&phase), 1);
    for (i, p) in phase.iter().enumerate() {
        if p.may_board() {
            served[i] += 1;
        }
    }
    assert_eq!(served, [1, 1, 1], "each bus served the stop exactly once");
}
