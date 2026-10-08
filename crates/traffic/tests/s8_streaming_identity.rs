//! Stage 8 exit gate — repeated streaming and camera-range changes preserve identity.
//!
//! Dormant actors keep their stable id, class and duty ownership across tile load/unload and
//! camera-range changes; a reactivated actor is the same actor, and its removal is once-only.

mod common;

use common::population::{demand, PopWorld};
use traffic::*;

#[test]
fn tile_and_camera_changes_preserve_identity_and_duty_counts() {
    let mut w = PopWorld::new(1);
    w.coord
        .enter_dormant(VehicleId(1), SpawnClass::Unscheduled, false, 0, 10.0);
    w.coord
        .enter_dormant(VehicleId(2), SpawnClass::Scheduled, true, 0, 50.0);
    w.coord
        .enter_dormant(VehicleId(3), SpawnClass::Scheduled, true, 0, 90.0);
    let ids_before: Vec<VehicleId> = w.coord.dormant_ids().to_vec();
    for _ in 0..5 {
        // a tile load/unload rebuilds topology demand but keeps the logical lifecycle
        w.coord.invalidate_network();
        assert_eq!(w.coord.dormant_len(), 3);
        assert_eq!(w.coord.dormant_duty_count(), 2);
        let decisions = w.plan_dormant(&[], true, demand(0, 0));
        assert!(decisions.is_empty());
    }
    assert_eq!(w.coord.dormant_ids(), ids_before.as_slice());
    assert_eq!(w.coord.dormant_len(), 3);
    assert_eq!(w.coord.dormant_duty_count(), 2);
}

#[test]
fn a_reactivated_actor_keeps_its_identity_and_duty() {
    let mut w = PopWorld::new(1);
    w.coord
        .enter_dormant(VehicleId(2), SpawnClass::Scheduled, true, 0, 50.0);
    let view = DormantView {
        id: VehicleId(2),
        class: SpawnClass::Scheduled,
        duty: true,
        lane: 0,
        s: 50.0,
        ground: true,
        visible: true,
    };
    let decisions = w.plan_dormant(&[view], true, demand(10, 0));
    assert_eq!(decisions.len(), 1);
    assert_eq!(decisions[0].id, VehicleId(2));
    assert_eq!(decisions[0].outcome, SpawnOutcome::Admit);
    // Duty ownership is held until the adapter confirms the reactivation.
    assert_eq!(w.coord.dormant_duty_count(), 1);
    w.coord.note_reactivated(VehicleId(2));
    assert!(!w.coord.contains_dormant(VehicleId(2)));
    assert_eq!(w.coord.dormant_duty_count(), 0);
}
