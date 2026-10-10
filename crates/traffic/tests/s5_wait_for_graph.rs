//! Stage 5 exit gate — the wait-for graph records holds and never invents a deadlock.
//!
//! The classification itself (stale claim vs legal congestion vs full capacity) is unit
//! tested in `traffic::junctions`; here the real coordinator is driven and its public
//! wait-for graph is checked.

mod common;

use common::{four_way, Harness};
use traffic::{JunctionActor, JunctionCoordinator, VehicleId, WaitDiagnosis};

fn stopper(id: u64, lane: usize, s: f32) -> JunctionActor {
    let mut a = JunctionActor::new(VehicleId(id), lane, s);
    a.speed = 0.0;
    a
}

#[test]
fn a_yielding_vehicle_records_who_it_waits_for_without_a_false_cycle() {
    let mut h = Harness::new(four_way());
    let w = h.add(stopper(10, 0, 95.0));
    let s = h.add(stopper(20, 4, 95.0));
    h.approach(w, 1, 5.0);
    h.approach(s, 5, 5.0);

    let mut coord = JunctionCoordinator::new();
    coord.begin_tick(0);
    // the right-of-way movement claims first; the other then waits on its claim
    let _ = h.plan(&mut coord, s, None);
    let dec_w = h.plan(&mut coord, w, None);

    assert!(dec_w.yield_at.is_some());
    assert_eq!(
        coord.blocked_by(VehicleId(10)),
        Some(VehicleId(20)),
        "the wait-for graph did not record the hold"
    );
    assert_eq!(coord.blocked_by(VehicleId(20)), None);

    let (diagnoses, _) = coord.classify_waits(&h.scene());
    assert!(
        !diagnoses
            .iter()
            .any(|d| matches!(d, WaitDiagnosis::StaleClaimCycle { .. })),
        "a single legal hold was mislabelled a deadlock: {diagnoses:?}"
    );
}
