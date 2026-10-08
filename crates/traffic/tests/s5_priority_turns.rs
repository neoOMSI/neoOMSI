//! Stage 5 exit gate — priority turns and oncoming commitment.
//!
//! Content rules decide first: a `[rule] priority` main road goes before an unmarked side
//! road, and a left turn across the oncoming traffic waits for it. The priority is
//! deterministic and does not depend on which vehicle is planned first.

mod common;

use common::{four_way, oncoming_pair, Harness};
use traffic::{JunctionActor, JunctionCoordinator, VehicleId};

fn stopper(id: u64, lane: usize, s: f32) -> JunctionActor {
    let mut a = JunctionActor::new(VehicleId(id), lane, s);
    a.speed = 0.0;
    a
}

#[test]
fn the_priority_road_goes_before_the_side_road() {
    let mut net = four_way();
    net.lanes[1].priority = 192.0; // west->east main road
    net.lanes[5].priority = 64.0; // south->north side road
    let mut h = Harness::new(net);
    let w = h.add(stopper(10, 0, 95.0));
    let s = h.add(stopper(20, 4, 95.0));
    h.approach(w, 1, 5.0);
    h.approach(s, 5, 5.0);

    let mut coord = JunctionCoordinator::new();
    coord.begin_tick(0);
    let dec_w = h.plan(&mut coord, w, None);
    let dec_s = h.plan(&mut coord, s, None);

    assert!(dec_w.yield_at.is_none(), "the priority road yielded: {dec_w:?}");
    assert!(coord.holds(VehicleId(10), 1));
    assert!(dec_s.yield_at.is_some(), "the side road ignored priority: {dec_s:?}");
    assert!(!coord.holds(VehicleId(20), 5));
}

#[test]
fn a_left_turn_waits_for_the_oncoming_traffic() {
    let mut net = oncoming_pair();
    net.lanes[1].turn = 1; // left, across the oncoming lane 3
    let mut h = Harness::new(net);
    let w = h.add(stopper(10, 0, 95.0)); // turns left across lane 3
    let e = h.add(stopper(20, 2, 95.0)); // oncoming, straight on lane 3
    h.approach(w, 1, 5.0);
    h.approach(e, 3, 5.0);

    let mut coord = JunctionCoordinator::new();
    coord.begin_tick(0);
    let dec_e = h.plan(&mut coord, e, None);
    let dec_w = h.plan(&mut coord, w, None);

    assert!(dec_e.yield_at.is_none(), "the oncoming movement yielded: {dec_e:?}");
    assert!(dec_w.yield_at.is_some(), "the left turn crossed the oncoming traffic: {dec_w:?}");
    assert!(!coord.holds(VehicleId(10), 1));
}
