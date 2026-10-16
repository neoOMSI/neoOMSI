//! Stage 5 — a ring of queues, each waiting for the next junction's exit.
//!
//! Two junctions whose exits lead into each other's approach (the smallest ring of queues
//! round a block): the car at each line waits for its full exit, and one of those exits is
//! full of a queue standing behind the other line's car. Nothing in the rules ever ends
//! that; the wait-for graph has to see the ring through the full exits and the queue, and
//! the coordinator lets one car go into its junction (up to the queue's tail) after a while.

mod common;

use common::object_lane;
use glam::DVec3;
use traffic::{
    JunctionActor, JunctionCoordinator, LaneBuilder, LaneKey, LaneKind, Lead, Network, Recovery,
    VehicleId, WaitDiagnosis,
};

const DT: f32 = 0.1;

/// Lane 0 (8 m) -> junction 1 (lane 1) -> lane 2 (16 m) -> junction 2 (lane 3) -> lane 0;
/// lanes 4 and 5 cross the two junctions.
fn ring() -> Network {
    let street = |a: DVec3, b: DVec3| LaneBuilder::polyline(vec![a, b], LaneKind::Street, 3.0);
    let object = |a: DVec3, b: DVec3, id: i64, path: u16| {
        let mut l = object_lane(a, b, path);
        l.key = Some(LaneKey { tile: (0, 0), id, path });
        l
    };
    let mut net = Network {
        lanes: vec![
            street(DVec3::new(-8.0, 0.0, 0.0), DVec3::ZERO),
            object(DVec3::ZERO, DVec3::new(10.0, 0.0, 0.0), 1, 0),
            street(DVec3::new(84.0, 0.0, 0.0), DVec3::new(100.0, 0.0, 0.0)),
            object(DVec3::new(100.0, 0.0, 0.0), DVec3::new(110.0, 0.0, 0.0), 2, 0),
            object(DVec3::new(5.0, -10.0, 0.0), DVec3::new(5.0, 10.0, 0.0), 1, 1),
            object(DVec3::new(105.0, -10.0, 0.0), DVec3::new(105.0, 10.0, 0.0), 2, 1),
        ],
        ..Default::default()
    };
    for (a, b) in [(0, 1), (1, 2), (2, 3), (3, 0)] {
        net.lanes[a].next = vec![b];
    }
    net.compute_conflicts();
    net
}

fn standing(h: &mut common::Harness, id: u64, lane: usize, s: f32) -> usize {
    let mut a = JunctionActor::new(VehicleId(id), lane, s);
    a.speed = 0.0;
    let i = h.add(a);
    h.place(i, lane, s);
    i
}

#[test]
fn a_ring_of_full_exits_is_found_and_one_car_is_let_go() {
    let mut h = common::Harness::new(ring());
    // at the line of junction 1, its rear 3.5 m into lane 0: junction 2's exit is full
    let first = standing(&mut h, 1, 0, 5.75);
    // at the line of junction 2, and a car queued 2 m behind it whose rear is 5 m into
    // lane 2: junction 1's exit is full
    let second = standing(&mut h, 2, 2, 13.75);
    let queued = standing(&mut h, 3, 2, 7.25);
    h.actors[queued].lead_info = Some((VehicleId(2), 2.0));
    let behind = Lead { gap: 2.0, speed: 0.0, acc: 0.0 };

    let mut coord = JunctionCoordinator::new();
    let mut passes = Vec::new();
    let mut admitted = None;
    for tick in 0..200u64 {
        h.tick = tick;
        h.time = tick as f32 * DT;
        coord.begin_tick(tick);
        let d1 = h.plan(&mut coord, first, None);
        let d2 = h.plan(&mut coord, second, None);
        h.plan(&mut coord, queued, Some(behind));
        if tick == 0 {
            assert_eq!(coord.blocked_by(VehicleId(1)), Some(VehicleId(3)), "{d1:?}");
            assert_eq!(coord.blocked_by(VehicleId(2)), Some(VehicleId(1)), "{d2:?}");
        }
        match (d1.yield_at, d2.yield_at) {
            (Some(_), Some(_)) => {}
            (None, Some(_)) => {
                admitted = Some((tick, VehicleId(1)));
                break;
            }
            (Some(_), None) => {
                admitted = Some((tick, VehicleId(2)));
                break;
            }
            (None, None) => panic!("both cars went into the full ring at t={:.1}", h.time),
        }
        let (diagnoses, recoveries) = coord.classify_waits(&h.scene());
        assert!(
            diagnoses.iter().any(|d| matches!(d, WaitDiagnosis::YieldCycle { cycle } if cycle.len() == 3)),
            "the ring was not seen: {diagnoses:?}"
        );
        passes.extend(recoveries.into_iter().filter_map(|r| match r {
            Recovery::GridlockPass { vehicle } => Some((tick, vehicle)),
            _ => None,
        }));
    }
    let (tick, who) = admitted.expect("nobody was let go out of the ring");
    assert!(tick as f32 * DT >= 8.0, "let go too early, at tick {tick}");
    assert_eq!(passes.len(), 1, "{passes:?}");
    assert_eq!(passes[0].1, who);
    // (both have waited since the first tick: the lower id goes)
    assert_eq!(who, VehicleId(1));
    assert!(coord.holds(VehicleId(1), 1), "the car let go did not claim its junction");
}

#[test]
fn the_box_rule_records_whom_it_waits_for() {
    // a car 5 m before junction 1 behind a car standing 3 m into the junction
    let mut h = common::Harness::new(ring());
    let me = standing(&mut h, 1, 0, 0.75);
    h.actors[me].lead_info = Some((VehicleId(7), 8.0));
    let lead = Lead { gap: 8.0, speed: 0.0, acc: 0.0 };
    let mut coord = JunctionCoordinator::new();
    coord.begin_tick(0);
    let d = h.plan(&mut coord, me, Some(lead));
    assert!(d.yield_at.is_some(), "{d:?}");
    assert_eq!(coord.blocked_by(VehicleId(1)), Some(VehicleId(7)));
}
