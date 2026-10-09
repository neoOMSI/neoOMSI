//! A circulating path and an entry path meet at the same downstream lane.
//! Priority comes from authored path rules, independent of driving side and plan order.
mod common;
use common::{Harness, object_lane};
use glam::DVec3;
use traffic::*;

fn world(left_hand: bool) -> Harness {
    let mirror = if left_hand { -1.0 } else { 1.0 };
    let mut ring = object_lane(DVec3::new(-20.0 * mirror, 0.0, 0.0), DVec3::ZERO, 0);
    ring.priority = 192.0;
    ring.next = vec![2];
    let mut entry = object_lane(DVec3::new(-20.0 * mirror, -20.0, 0.0), DVec3::ZERO, 1);
    entry.priority = 64.0;
    entry.next = vec![2];
    let exit = LaneBuilder::polyline(
        vec![DVec3::ZERO, DVec3::new(100.0 * mirror, 0.0, 0.0)],
        LaneKind::Street,
        3.0,
    );
    let mut net = Network {
        lanes: vec![ring, entry, exit],
        left_hand,
        ..Default::default()
    };
    net.compute_conflicts();
    assert!(net.crossings[0][0].merge);
    Harness::new(net)
}

fn add(w: &mut Harness, id: u64, lane: usize, s: f32, speed: f32) -> usize {
    let mut a = JunctionActor::new(VehicleId(id), lane, s);
    a.speed = speed;
    let i = w.add(a);
    w.place(i, lane, s);
    i
}

#[test]
fn circulating_traffic_does_not_yield_to_a_stoppable_entry_on_its_object_path() {
    for left in [false, true] {
        for entry_first in [false, true] {
            let mut w = world(left);
            let ring = add(&mut w, 10, 0, 4.0, 5.0);
            let entry = add(&mut w, 20, 1, 18.0, 2.0);
            let mut c = JunctionCoordinator::new();
            let (r, e) = if entry_first {
                let e = w.plan(&mut c, entry, None);
                (w.plan(&mut c, ring, None), e)
            } else {
                let r = w.plan(&mut c, ring, None);
                (r, w.plan(&mut c, entry, None))
            };
            assert!(r.yield_at.is_none(), "circulating path stopped: {r:?}");
            assert!(e.yield_at.is_some(), "entry ignored ring priority: {e:?}");
        }
    }
}

#[test]
fn a_body_already_in_the_merge_is_never_overridden_by_priority() {
    for left in [false, true] {
        let mut w = world(left);
        let ring = add(&mut w, 10, 0, 4.0, 5.0);
        let at = w.net.lanes[1].length() - 1.0;
        add(&mut w, 20, 1, at, 0.0);
        assert!(
            w.plan(&mut JunctionCoordinator::new(), ring, None)
                .yield_at
                .is_some()
        );
    }
}

#[test]
fn entry_resumes_when_the_ring_gap_is_clear() {
    for left in [false, true] {
        let mut w = world(left);
        let entry = add(&mut w, 20, 1, 4.0, 3.0);
        assert!(
            w.plan(&mut JunctionCoordinator::new(), entry, None)
                .yield_at
                .is_none()
        );
    }
}

#[test]
fn a_speculative_entry_claim_does_not_take_priority_from_the_ring() {
    for left in [false, true] {
        let mut w = world(left);
        let ring = add(&mut w, 10, 0, 4.0, 5.0);
        add(&mut w, 20, 1, 18.0, 2.0);
        let mut c = JunctionCoordinator::new();
        c.restore_claim(VehicleId(20), &[1]);
        assert!(w.plan(&mut c, ring, None).yield_at.is_none());
    }
}

#[test]
fn a_long_wait_at_the_entry_does_not_outrank_a_briefly_stopped_ring() {
    for left in [false, true] {
        let mut w = world(left);
        // the ring car stands for a moment (its own leader held it), not stalled
        let ring = add(&mut w, 10, 0, 4.0, 0.0);
        w.actors[ring].stopped = 1.0;
        w.actors[ring].yield_time = 0.0;
        // the entry has waited long at its give-way line
        let at = w.net.lanes[1].length() - 9.0;
        let entry = add(&mut w, 20, 1, at, 0.0);
        w.actors[entry].yielding = true;
        w.actors[entry].yield_time = 12.0;
        let e = w.plan(&mut JunctionCoordinator::new(), entry, None);
        assert!(e.yield_at.is_some(), "entry jumped ahead of the ring on fairness: {e:?}");
    }
}

#[test]
fn an_entry_may_go_past_a_stalled_ring_queue() {
    for left in [false, true] {
        let mut w = world(left);
        // the ring car has stood for a long time: a queue, not traffic about to arrive
        let ring = add(&mut w, 10, 0, 4.0, 0.0);
        w.actors[ring].stopped = 10.0;
        let at = w.net.lanes[1].length() - 9.0;
        let entry = add(&mut w, 20, 1, at, 0.0);
        w.actors[entry].yielding = true;
        w.actors[entry].yield_time = 12.0;
        let e = w.plan(&mut JunctionCoordinator::new(), entry, None);
        assert!(e.yield_at.is_none(), "entry is held by a stalled ring queue: {e:?}");
    }
}

/// A circle of sixteen 5.9 m chords (radius 15 m) driven anticlockwise (clockwise when
/// `left_hand`), with an entry from outside joining it where lane 0 starts. No priorities
/// are authored. Lanes 0..16 are the ring, 16 is the entry.
fn unruled_ring(left_hand: bool) -> Network {
    let sign = if left_hand { -1.0 } else { 1.0 };
    let at = |k: usize| {
        let a = (k % 16) as f64 * std::f64::consts::FRAC_PI_8 * sign;
        DVec3::new(15.0 * a.cos(), 15.0 * a.sin(), 0.0)
    };
    let mut lanes: Vec<Lane> = (0..16)
        .map(|k| LaneBuilder::polyline(vec![at(k), at(k + 1)], LaneKind::Street, 3.0))
        .collect();
    lanes.push(LaneBuilder::polyline(
        vec![DVec3::new(45.0, -10.0 * sign, 0.0), at(0)],
        LaneKind::Street,
        3.0,
    ));
    let mut net = Network { lanes, left_hand, ..Default::default() };
    net.link(1.0);
    net
}

#[test]
fn an_unruled_roundabout_gives_the_ring_priority_over_the_entry() {
    for left in [false, true] {
        let net = unruled_ring(left);
        assert!((0..16).all(|k| net.is_ring(k)), "ring not found (left {left})");
        assert!(!net.is_ring(16));
        // the entry comes from the right of the circulating car: not "rechts vor links" here
        assert!(net.must_yield(16, 15), "entry must give way (left {left})");
        assert!(!net.must_yield(15, 16), "ring must not give way (left {left})");
    }
}

#[test]
fn a_loop_driven_the_wrong_way_round_is_no_roundabout() {
    // the same circle on a map driving on the other side
    let mut net = unruled_ring(false);
    net.left_hand = true;
    net.compute_rings();
    assert!(!(0..16).any(|k| net.is_ring(k)));
}

#[test]
fn a_car_on_the_ring_keeps_going_past_an_entry_claim_the_entrant_can_still_give_up() {
    // the ring and its entry are paths of one roundabout object, no priorities authored
    let at = |k: usize| {
        let a = (k % 16) as f64 * std::f64::consts::FRAC_PI_8;
        DVec3::new(15.0 * a.cos(), 15.0 * a.sin(), 0.0)
    };
    let mut lanes: Vec<Lane> = (0..16).map(|k| object_lane(at(k), at(k + 1), k as u16)).collect();
    lanes.push(object_lane(DVec3::new(25.0, -20.0, 0.0), at(0), 16));
    let mut net = Network { lanes, ..Default::default() };
    net.link(1.0);
    assert!(net.is_ring(15) && !net.is_ring(16));
    assert!(net.crossings[15].iter().any(|c| c.other == 16 && c.merge));
    let mut w = Harness::new(net);
    // the entrant saw nobody on the ring and claimed its entry
    let entry_len = w.net.lanes[16].length();
    let entrant = add(&mut w, 20, 16, entry_len - 16.0, 3.0);
    let mut c = JunctionCoordinator::new();
    assert!(w.plan(&mut c, entrant, None).yield_at.is_none());
    // a car comes round the ring towards the merge
    let ring = add(&mut w, 10, 13, 1.0, 5.0);
    let to_14 = w.net.lanes[13].length() - 1.0;
    w.approach(ring, 14, to_14);
    w.approach(ring, 15, to_14 + w.net.lanes[14].length());
    let r = w.plan(&mut c, ring, None);
    assert!(r.yield_at.is_none(), "the ring gave way to an entering car: {r:?}");
    assert!(w.plan(&mut c, entrant, None).yield_at.is_some(), "entrant ignored the ring");
}
