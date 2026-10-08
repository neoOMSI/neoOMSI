//! Stage 3 exit gate — exit space is reserved for all admitted vehicles, and simultaneous
//! merges are deterministic.
//!
//! The downstream exit has one free storage distance. Every admitted vehicle reserves its
//! length out of that same distance, so two vehicles cannot each be promised the same
//! empty space.

use traffic::world::Arbiter;
use traffic::{LaneId, VehicleId};

const EXIT: LaneId = LaneId(0);

#[test]
fn exit_space_is_reserved_for_all_admitted_vehicles() {
    // 12 m free at the exit; each bus needs 5 m + 1 m gap.
    let need = 6.0f32;
    let mut arb = Arbiter::new();
    arb.set_storage_capacity(EXIT, 12.0);

    assert!(arb.reserve_storage(EXIT, VehicleId(1), need), "first bus denied");
    assert!(arb.reserve_storage(EXIT, VehicleId(2), need), "second bus denied");
    // 2 x 6 = 12 fills the exit; a third cannot be admitted against the same space.
    assert!(
        !arb.reserve_storage(EXIT, VehicleId(3), need),
        "a third vehicle was admitted into the same empty exit"
    );

    // Once the first bus leaves, its slot is free for the third.
    arb.release_storage(EXIT, VehicleId(1));
    assert!(arb.reserve_storage(EXIT, VehicleId(3), need));
    assert!(arb.storage_reserved(EXIT, VehicleId(3)));
}

#[test]
fn a_simultaneous_merge_is_resolved_by_id() {
    let exit = LaneId(3);
    let candidates = [VehicleId(11), VehicleId(4), VehicleId(8)];
    let mut arb = Arbiter::new();
    // Lowest id wins the tie.
    assert_eq!(arb.merge_winner(exit, &candidates), Some(VehicleId(4)));
    // A claimant is admitted first even when a lower id is present.
    arb.grant(exit, VehicleId(8));
    assert_eq!(arb.merge_winner(exit, &candidates), Some(VehicleId(8)));
    // Releasing the claim restores the plain lowest-id rule.
    arb.release(exit, VehicleId(8));
    assert_eq!(arb.merge_winner(exit, &candidates), Some(VehicleId(4)));
}

#[test]
fn refresh_of_exit_capacity_does_not_lose_committed_slots() {
    let mut arb = Arbiter::new();
    arb.set_storage_capacity(EXIT, 12.0);
    assert!(arb.reserve_storage(EXIT, VehicleId(1), 6.0));
    // The next tick recomputes the free distance (a bus ahead moved on): 18 m now.
    arb.set_storage_capacity(EXIT, 18.0);
    assert!(arb.reserve_storage(EXIT, VehicleId(2), 6.0));
    assert!(arb.reserve_storage(EXIT, VehicleId(3), 6.0));
    assert!(!arb.reserve_storage(EXIT, VehicleId(4), 6.0));
}
