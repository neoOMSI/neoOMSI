//! Stage 3 exit gate — container reordering and worker scheduling do not change decisions.
//!
//! The same bodies and inputs, handed over in different orders and scheduling partitions,
//! must give identical decisions keyed by stable `VehicleId`.

mod common;

use traffic::perception::{
    BodyFootprint, Occupancy, Placement, SweepSample,
};
use traffic::world::Arbiter;
use traffic::{JunctionActor, JunctionCoordinator, LaneId, NetworkVersion, VehicleId};
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

/// The junction scene of `junction_planning_does_not_depend_on_container_order`, cars stored
/// in `order`: a queue on the exit leaves room for one car, and three cars come up to it.
fn junction_scene(order: &[u64]) -> common::Harness {
    let mut h = common::Harness::new(common::blocked_exit());
    for &id in order {
        let (lane, s, speed) = match id {
            99 => (2, 10.25, 0.0),
            4 => (0, 95.0, 0.0),
            7 => (0, 95.0, 0.0),
            2 => (0, 85.0, 6.0),
            _ => unreachable!(),
        };
        let mut a = JunctionActor::new(VehicleId(id), lane, s);
        a.speed = speed;
        let i = h.add(a);
        h.place(i, lane, s);
    }
    h
}

#[test]
fn junction_planning_does_not_depend_on_container_order() {
    // what each car was told and holds, by id, over a few ticks
    let run = |order: &[u64]| {
        let h = junction_scene(order);
        let mut coord = JunctionCoordinator::new();
        let mut out = Vec::new();
        for tick in 0..3 {
            coord.begin_tick(tick);
            let mut decided: Vec<_> = coord
                .plan_order(&h.actors)
                .into_iter()
                .map(|i| (h.actors[i].id, h.plan(&mut coord, i, None)))
                .collect();
            decided.sort_by_key(|d| d.0);
            for (id, d) in decided {
                out.push((tick, id, d.yield_at, d.state, d.reasons, coord.claims_of(id).to_vec()));
            }
        }
        out
    };
    let base = run(&[99, 2, 4, 7]);
    // one of them gets the exit's room, the others wait for it
    let admitted = base
        .iter()
        .filter(|d| d.0 == 0 && d.1 != VehicleId(99) && d.2.is_none())
        .count();
    assert_eq!(admitted, 1, "{base:?}");
    for order in [[7, 4, 2, 99], [4, 99, 7, 2], [2, 7, 99, 4]] {
        assert_eq!(run(&order), base, "the container order {order:?} changed the decisions");
    }
}
