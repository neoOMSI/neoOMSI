//! Stage 3 exit gate — container reordering and worker scheduling do not change decisions.
//!
//! The same realized bodies and inputs are handed to the perception and arbitration layers
//! in different orders (representing an entity-container reorder) and in different
//! scheduling partitions. Decisions keyed by stable `VehicleId` must be identical.

use traffic::perception::{
    BodyFootprint, Occupancy, Placement, SweepSample,
};
use traffic::world::Arbiter;
use traffic::{LaneId, NetworkVersion, VehicleId};
use glam::{DVec2, DVec3};

/// A straight road, bodies nose-to-tail on it. The container order is the only thing that
/// changes between runs.
fn bodies(order: &[u64]) -> Vec<BodyFootprint> {
    order
        .iter()
        .map(|&id| {
            let s = 20.0 + id as f64 * 10.0;
            let mut f = BodyFootprint::new(
                VehicleId(id),
                DVec2::new(0.0, s),
                DVec2::new(0.0, 1.0),
                2.25,
                1.25,
                0.0,
                3.0,
                5.0,
            );
            f.current = Some(Placement {
                lane: LaneId(0),
                s: s as f32,
                lateral: 0.0,
                foreign: false,
            });
            f
        })
        .collect()
}

/// The decision signature: for each id, who it follows and the gap, plus the merge winner.
fn signature(occurrences: &Occupancy, ids: &[u64]) -> Vec<(u64, Option<u64>, i32)> {
    ids.iter()
        .map(|&id| {
            let front = 20.0 + id as f64 * 10.0 + 2.25;
            let lead = occurrences.nearest_ahead(LaneId(0), front as f32, 0.0, 3.0, false);
            (
                id,
                lead.map(|l| l.owner.get()),
                lead.map(|l| (l.gap * 100.0).round() as i32).unwrap_or(-1),
            )
        })
        .collect()
}

#[test]
fn reordering_the_container_does_not_change_decisions() {
    let ids: Vec<u64> = (1..=6).collect();
    let base_occ = Occupancy::build(NetworkVersion(1), 3, bodies(&ids));
    let base = signature(&base_occ, &ids);

    // Every permutation arm of a container reorder: reverse, rotate, and interleave.
    let orders = [
        ids.iter().rev().copied().collect::<Vec<_>>(),
        {
            let mut o = ids.clone();
            o.rotate_left(3);
            o
        },
        vec![3, 1, 5, 2, 6, 4],
    ];
    for order in orders {
        let occ = Occupancy::build(NetworkVersion(1), 3, bodies(&order));
        assert_eq!(signature(&occ, &ids), base, "reorder changed decisions for {order:?}");
    }
}

#[test]
fn scheduling_partitions_do_not_change_decisions() {
    // "Worker scheduling": build the same world in different chunk partitions.
    let ids: Vec<u64> = (1..=8).collect();
    let all = bodies(&ids);

    let mut whole = Vec::new();
    for f in &all {
        whole.push(*f);
    }
    let base = signature(&Occupancy::build(NetworkVersion(1), 1, whole), &ids);

    // Partitioned insertion with a different chunk size must yield the same index.
    for chunk in [1usize, 3, 4, 8] {
        let mut part: Vec<BodyFootprint> = Vec::new();
        for c in all.chunks(chunk) {
            part.extend_from_slice(c);
        }
        assert_eq!(
            signature(&Occupancy::build(NetworkVersion(1), 1, part), &ids),
            base,
            "chunk size {chunk} changed decisions"
        );
    }
}

#[test]
fn a_merge_winner_does_not_depend_on_candidate_order() {
    let lane = LaneId(7);
    let mut arb = Arbiter::new();
    let mut a = [VehicleId(9), VehicleId(3), VehicleId(6)];
    let win1 = arb.merge_winner(lane, &a);
    a.reverse();
    let win2 = arb.merge_winner(lane, &a);
    assert_eq!(win1, Some(VehicleId(3)));
    assert_eq!(win1, win2, "candidate order changed the merge winner");
    // With a claim, the claimant wins regardless of order.
    arb.grant(lane, VehicleId(6));
    assert_eq!(arb.merge_winner(lane, &a), Some(VehicleId(6)));
}

#[test]
fn a_swept_clearance_does_not_depend_on_body_order() {
    let ids = [4u64, 2, 8];
    let samples = [SweepSample {
        p: DVec3::new(0.0, 25.0, 0.0),
        d: 1.0,
        dir: DVec2::new(0.0, 1.0),
    }];
    let base = Occupancy::build(NetworkVersion(1), 0, bodies(&ids))
        .swept_clearance(&samples, 1.25, &[])
        .map(|s| s.owner);
    for order in [[2u64, 4, 8], [8, 2, 4], [8, 4, 2]] {
        let hit = Occupancy::build(NetworkVersion(1), 0, bodies(&order))
            .swept_clearance(&samples, 1.25, &[])
            .map(|s| s.owner);
        assert_eq!(hit, base, "body order changed the swept clearance");
    }
}
