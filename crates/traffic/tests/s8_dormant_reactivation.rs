//! Stage 8 exit gate — dormant reactivation never creates an overlap.
//!
//! A dormant actor whose reactivation spot is occupied stays dormant; once the gap clears it
//! is placed back, and its identity and duty are preserved exactly once.

mod common;

use common::population::{demand, PopWorld};
use traffic::*;

fn view(id: u64, lane: usize, s: f32) -> DormantView {
    DormantView {
        id: VehicleId(id),
        class: SpawnClass::Unscheduled,
        duty: false,
        lane,
        s,
        ground: true,
        visible: true,
    }
}

#[test]
fn a_dormant_actor_waits_for_a_gap_and_never_overlaps() {
    let mut w = PopWorld::new(1);
    w.coord
        .enter_dormant(VehicleId(5), SpawnClass::Unscheduled, false, 0, 100.0);
    // Somebody stands exactly on the wake spot.
    w.occupy(1000, 0, 100.0);
    let blocked = w.plan_dormant(&[view(5, 0, 100.0)], true, demand(10, 0));
    assert!(
        matches!(blocked[0].outcome, SpawnOutcome::Retry { .. }),
        "an occupied wake spot must wait"
    );
    assert!(w.coord.contains_dormant(VehicleId(5)));

    // The blocker leaves: the same actor is placed back.
    w.bodies.clear();
    let clear = w.plan_dormant(&[view(5, 0, 100.0)], true, demand(10, 0));
    assert_eq!(clear[0].outcome, SpawnOutcome::Admit);
    assert_eq!(clear[0].id, VehicleId(5));
}

#[test]
fn an_ungrounded_wake_spot_is_denied_and_stays_dormant() {
    let mut w = PopWorld::new(1);
    w.coord
        .enter_dormant(VehicleId(7), SpawnClass::Scheduled, true, 0, 20.0);
    let mut v = view(7, 0, 20.0);
    v.ground = false;
    let decisions = w.plan_dormant(&[v], true, demand(10, 0));
    assert_eq!(decisions[0].outcome, SpawnOutcome::Deny(Reason::NoGround));
    assert!(w.coord.contains_dormant(VehicleId(7)));
    assert_eq!(w.coord.dormant_duty_count(), 1);
}
