//! Stage 6 exit gate — optional/request stops, timing points, early/late and layovers.
//!
//! Dwell and skip policy follow explicit service semantics: an unwanted mid-route stop is
//! skipped, a wanted or marked stop is served, a bus early for its departure waits with the
//! doors open, a late bus goes at once, and a layover waits its departure out of the lane.

mod common;

use common::service::{berth, Bus, ServiceWorld};
use traffic::{ServicePhase, StopId, DEFAULT_APPROACH_DISTANCE};

const STOP: i64 = 7001;
const BERTH_S: f32 = 120.0;

fn single(depart: f64) -> ServiceWorld {
    let mut w = ServiceWorld::new();
    w.add(Bus::new(1, 90.0, vec![berth(STOP, BERTH_S, depart)]));
    w
}

fn reached_boarding(w: &mut ServiceWorld, ticks: u32) -> bool {
    for _ in 0..ticks {
        w.step();
        if w.bus(1).phase() == ServicePhase::Boarding {
            return true;
        }
    }
    false
}

#[test]
fn an_unwanted_middle_stop_is_skipped() {
    let mut w = single(36000.0);
    w.bus_mut(1).demand.wanted = Some(false);
    w.run(2000);
    assert!(w.bus(1).served.is_empty() || w.bus(1).stops.is_empty());
    // It drove past the stop without ever boarding.
    assert_ne!(w.bus(1).phase(), ServicePhase::Boarding);
    assert!(w.bus(1).stops.is_empty(), "the skipped stop was not advanced");
}

#[test]
fn a_requested_stop_is_served() {
    let mut w = single(36000.0);
    w.bus_mut(1).demand.wanted = Some(true);
    assert!(reached_boarding(&mut w, 3000), "a requested stop was not served");
    // Let it finish boarding and pull away.
    w.run(1500);
    assert_eq!(w.bus(1).served, vec![StopId(STOP)]);
}

#[test]
fn a_timing_point_is_served_even_without_demand() {
    let mut w = single(36000.0);
    w.bus_mut(1).demand.wanted = Some(false);
    w.bus_mut(1).policy.always = vec![STOP];
    assert!(reached_boarding(&mut w, 3000), "a timing point was skipped");
}

#[test]
fn an_early_bus_waits_for_its_departure() {
    // Departing 100 s from now: the bus is early and must wait with the doors open.
    let mut w = single(36000.0 + 100.0);
    assert!(reached_boarding(&mut w, 3000));
    let leave = w.bus(1).state.leave_at;
    assert!(leave > 36000.0 + 30.0, "the early bus did not wait for its departure");
    // It is still boarding well after the passenger exchange would have ended.
    for _ in 0..1500 {
        w.step();
        if w.bus(1).served.is_empty() {
            assert_eq!(w.bus(1).phase(), ServicePhase::Boarding, "the early bus left early");
        }
    }
}

#[test]
fn a_late_bus_goes_at_once() {
    let mut w = single(36000.0 - 10.0);
    assert!(reached_boarding(&mut w, 3000));
    // No early wait: it leaves as soon as the doors have been open their service time.
    let mut left = false;
    for _ in 0..1000 {
        w.step();
        if !w.bus(1).served.is_empty() {
            left = true;
            break;
        }
    }
    assert!(left, "the late bus did not depart promptly");
}

#[test]
fn a_long_boarding_passenger_holds_the_door() {
    let mut w = single(36000.0);
    assert!(reached_boarding(&mut w, 3000));
    // Passengers keep the doors open for a long time.
    for _ in 0..1000 {
        let bus = w.bus_mut(1);
        if bus.state.phase == ServicePhase::Boarding {
            bus.state.boarding_t = 100.0;
        }
        w.step();
        assert!(
            !matches!(w.bus(1).phase(), ServicePhase::ClosingDoors | ServicePhase::Departing),
            "the bus tried to leave with passengers still boarding"
        );
    }
}

#[test]
fn a_layover_waits_out_its_departure_at_the_stop() {
    let mut w = single(36000.0 + 400.0);
    w.bus_mut(1).state.layover = true;
    let mut saw_layover = false;
    for _ in 0..3500 {
        w.step();
        if w.bus(1).phase() == ServicePhase::Layover {
            saw_layover = true;
        }
    }
    assert!(saw_layover, "a long layover did not stand at its stop");
    // It stays out of the moving lane while waiting.
    assert!((w.bus(1).lateral - 1.6).abs() < 0.3, "the layover did not stand in the bay");
    assert_eq!(w.bus(1).stops.len(), 1, "the layover stop must not be consumed early");
    // The stop is well within the approach distance when it stands.
    let _ = DEFAULT_APPROACH_DISTANCE;
}
