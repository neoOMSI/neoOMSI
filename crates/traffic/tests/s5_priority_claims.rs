//! A side-road claim made before priority traffic arrived does not take the right of way.
//!
//! A vehicle on the lower-priority road claims its movement as soon as it is inside its
//! decision zone and nothing is coming. When a vehicle on the priority road turns up while the
//! side-road vehicle can still stop, the priority road keeps the right of way: the early claim
//! is not a commitment the priority road has to wait for.

mod common;

use common::{four_way, Harness};
use traffic::{JunctionActor, JunctionCoordinator, VehicleId};

fn mover(id: u64, lane: usize, s: f32, speed: f32) -> JunctionActor {
    let mut a = JunctionActor::new(VehicleId(id), lane, s);
    a.speed = speed;
    a
}

fn priority_net() -> traffic::Network {
    let mut net = four_way();
    net.lanes[1].priority = 192.0; // west->east main road
    net.lanes[5].priority = 64.0; // south->north side road
    net
}

#[test]
fn an_early_side_road_claim_yields_to_later_priority_traffic() {
    for side_first in [false, true] {
        let mut h = Harness::new(priority_net());
        // side road 25 m before its line at 10 m/s: inside its decision zone, able to stop
        let s = h.add(mover(20, 4, 75.0, 10.0));
        h.approach(s, 5, 25.0);
        let mut coord = JunctionCoordinator::new();
        coord.begin_tick(0);
        let first = h.plan(&mut coord, s, None);
        assert!(first.yield_at.is_none(), "nothing was coming: {first:?}");
        assert!(coord.holds(VehicleId(20), 5));

        // a little later, slower and closer: the bus on the main road arrives
        h.actors[s].s = 85.0;
        h.actors[s].speed = 6.0;
        h.coming.clear();
        h.approach(s, 5, 15.0);
        let w = h.add(mover(10, 0, 70.0, 10.0));
        h.approach(w, 1, 30.0);
        coord.begin_tick(1);
        let (dec_w, dec_s) = if side_first {
            let ds = h.plan(&mut coord, s, None);
            (h.plan(&mut coord, w, None), ds)
        } else {
            let dw = h.plan(&mut coord, w, None);
            (dw, h.plan(&mut coord, s, None))
        };
        assert!(dec_w.yield_at.is_none(), "priority road gave way to a claim: {dec_w:?}");
        assert!(dec_s.yield_at.is_some(), "side road kept its early claim: {dec_s:?}");
    }
}

#[test]
fn a_side_road_vehicle_unable_to_stop_keeps_its_claim() {
    let mut h = Harness::new(priority_net());
    // 3 m before its line at 10 m/s: it cannot stop any more
    let s = h.add(mover(20, 4, 97.0, 10.0));
    h.approach(s, 5, 3.0);
    let mut coord = JunctionCoordinator::new();
    coord.begin_tick(0);
    assert!(h.plan(&mut coord, s, None).yield_at.is_none());

    let w = h.add(mover(10, 0, 80.0, 10.0));
    h.approach(w, 1, 20.0);
    coord.begin_tick(1);
    let dec_w = h.plan(&mut coord, w, None);
    let dec_s = h.plan(&mut coord, s, None);
    assert!(dec_s.yield_at.is_none(), "a vehicle that cannot stop was told to: {dec_s:?}");
    assert!(dec_w.yield_at.is_some(), "priority road drove into a committed body: {dec_w:?}");
}
