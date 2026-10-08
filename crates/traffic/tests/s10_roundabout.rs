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
