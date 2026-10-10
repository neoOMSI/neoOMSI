//! A turn across the oncoming traffic at a signal pulls into the junction to wait for a gap
//! at the meeting place instead of at the line; without a signal it waits at the line.

mod common;

use common::{oncoming_pair, Harness};
use traffic::{Aspect, JunctionActor, JunctionCoordinator, Lead, VehicleId};

/// Lane 1 turns across lane 3 (oncoming); `light` puts a signal on lane 1.
fn setup(light: bool) -> (Harness, usize, usize) {
    let mut net = oncoming_pair();
    net.lanes[1].turn = 1;
    if light {
        net.lanes[1].traffic_light = Some((0, 0));
    }
    let mut h = Harness::new(net);
    h.aspects.insert((0, 0), Aspect::Green);
    // the turner stands at the line
    let mut turner = JunctionActor::new(VehicleId(10), 0, 97.0);
    turner.speed = 0.0;
    let t = h.add(turner);
    h.place(t, 0, 97.0);
    h.approach(t, 1, 3.0);
    // oncoming traffic comes on at speed
    let mut on = JunctionActor::new(VehicleId(20), 2, 85.0);
    on.speed = 12.0;
    let o = h.add(on);
    h.place(o, 2, 85.0);
    h.approach(o, 3, 15.0);
    (h, t, o)
}

#[test]
fn a_left_turner_at_a_signal_pulls_into_the_junction_to_wait() {
    let (h, t, o) = setup(true);
    let mut coord = JunctionCoordinator::new();
    coord.begin_tick(0);
    let _ = h.plan(&mut coord, o, None);
    let dec = h.plan(&mut coord, t, None);
    let entry = 3.0;
    let at = dec.yield_at.expect("the turner gives way to the oncoming car");
    assert!(
        at > entry + 3.0,
        "the turner waits at {at:.1} m, not pulled in past the line at {entry:.1} m"
    );
}

#[test]
fn a_left_turner_without_a_signal_waits_at_the_line() {
    let (h, t, o) = setup(false);
    let mut coord = JunctionCoordinator::new();
    coord.begin_tick(0);
    let _ = h.plan(&mut coord, o, None);
    let dec = h.plan(&mut coord, t, None);
    let at = dec.yield_at.expect("the turner gives way to the oncoming car");
    assert!(at <= 3.0 + 0.01, "pulled in without a signal: {at:.1}");
}

#[test]
fn a_second_turner_behind_one_in_the_junction_stays_at_the_line() {
    let (h, t, o) = setup(true);
    let mut coord = JunctionCoordinator::new();
    coord.begin_tick(0);
    let _ = h.plan(&mut coord, o, None);
    // somebody already stands in the junction just past the line
    let lead = Lead {
        gap: 4.0,
        speed: 0.0,
        acc: 0.0,
    };
    let dec = h.plan(&mut coord, t, Some(lead));
    let at = dec.yield_at.expect("the turner gives way to the oncoming car");
    assert!(at <= 3.0 + 0.01, "a second turner pulled in: {at:.1}");
}
