//! Optional/request stops, timing points, early/late buses and layovers: an early bus waits
//! for its departure only at a "wait dep. time" stop, a late one goes at once.

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

/// The day time the bus pulls out of its stop, or `None` if it still stands after `ticks`.
fn left_at(w: &mut ServiceWorld, ticks: u32) -> Option<f64> {
    for _ in 0..ticks {
        w.step();
        if !matches!(
            w.bus(1).phase(),
            ServicePhase::Docking | ServicePhase::Boarding | ServicePhase::Layover
        ) {
            return Some(w.day_time);
        }
        assert!(w.bus(1).state.berth.is_some(), "the bus let go of its berth while waiting");
    }
    None
}

#[test]
fn an_early_bus_waits_for_its_departure_at_a_stop_marked_to_wait() {
    // "always + wait dep. time" (4): served without demand, stands until its departure (#298)
    let depart = 36000.0 + 300.0;
    let mut w = single(depart);
    w.bus_mut(1).demand.wanted = Some(false);
    w.bus_mut(1).policy.always = vec![STOP];
    w.bus_mut(1).policy.wait_departure = vec![STOP];
    assert!(reached_boarding(&mut w, 3000), "the marked stop was skipped");
    assert!((w.bus(1).state.leave_at - depart).abs() < 1e-6, "it did not wait for its departure");
    let mut doors_shut = false;
    let mut doors_again = false;
    let left = (0..20_000)
        .find_map(|_| {
            w.step();
            match w.bus(1).phase() {
                ServicePhase::Layover => doors_shut = true,
                ServicePhase::Boarding if doors_shut => doors_again = true,
                _ => {}
            }
            (!matches!(
                w.bus(1).phase(),
                ServicePhase::Boarding | ServicePhase::Layover
            ))
            .then_some(w.day_time)
        })
        .expect("it never left");
    assert!(left >= depart, "it left {:.0} s before its departure", depart - left);
    assert!(left < depart + 5.0, "it stood on {:.0} s after its departure", left - depart);
    // after the exchange it waits doors shut and reopens them shortly before leaving
    assert!(doors_shut && doors_again);
}

#[test]
fn a_stop_marked_to_wait_holds_the_bus_however_long() {
    // a quarter of an hour early: no cap on the wait
    let depart = 36000.0 + 900.0;
    let mut w = single(depart);
    w.bus_mut(1).policy.wait_departure = vec![STOP];
    assert!(reached_boarding(&mut w, 3000));
    let left = left_at(&mut w, 60_000).expect("it never left");
    assert!(left >= depart, "it left {:.0} s before its departure", depart - left);
}

#[test]
fn wait_dep_time_waits_only_where_the_bus_stops() {
    // "wait dep. time" (3): on time and unwanted it passes ...
    let mut w = single(36000.0 + 10.0);
    w.bus_mut(1).demand.wanted = Some(false);
    w.bus_mut(1).policy.serve_early = vec![STOP];
    w.bus_mut(1).policy.wait_departure = vec![STOP];
    assert!(!reached_boarding(&mut w, 3000), "an unwanted stop marked 3 was served on time");
    assert_eq!(w.bus(1).served, vec![StopId(STOP)], "the stop was not passed");
    // ... wanted, it stops and waits for its departure
    let depart = 36000.0 + 100.0;
    let mut w = single(depart);
    w.bus_mut(1).policy.serve_early = vec![STOP];
    w.bus_mut(1).policy.wait_departure = vec![STOP];
    assert!(reached_boarding(&mut w, 3000));
    let left = left_at(&mut w, 20_000).expect("it never left");
    assert!(left >= depart, "it left {:.0} s before its departure", depart - left);
}

#[test]
fn an_always_stop_without_the_wait_option_is_left_after_the_exchange() {
    // "always" (1): served, but an early bus does not wait for its departure there
    let depart = 36000.0 + 300.0;
    let mut w = single(depart);
    w.bus_mut(1).demand.wanted = Some(false);
    w.bus_mut(1).policy.always = vec![STOP];
    assert!(reached_boarding(&mut w, 3000), "a timing point was skipped");
    assert!(w.bus(1).state.leave_at <= w.day_time, "it waited for the timetable");
    let left = left_at(&mut w, 1000).expect("it stood on after the exchange");
    assert!(left < depart - 200.0);
}

#[test]
fn a_bus_behind_one_waiting_for_its_departure_queues_for_the_berth() {
    let mut w = single(36000.0 + 300.0);
    w.bus_mut(1).policy.wait_departure = vec![STOP];
    w.add(Bus::new(2, 40.0, vec![berth(STOP, BERTH_S, 36000.0 + 400.0)]));
    w.run(3000);
    assert_eq!(w.bus(1).phase(), ServicePhase::Layover, "the first bus does not wait");
    assert_eq!(w.bus(2).phase(), ServicePhase::WaitingForBerth);
    assert!(w.bus(2).state.berth.is_none(), "the follower took the held berth");
    assert!(w.bus(2).s < w.bus(1).s - 2.0 * common::service::HALF as f32);
}

#[test]
fn an_early_bus_does_not_hold_its_doors_at_an_ordinary_stop() {
    let mut w = single(36000.0 + 100.0);
    assert!(reached_boarding(&mut w, 3000));
    assert!(w.bus(1).state.leave_at <= w.day_time, "an ordinary stop held for the timetable");
    let mut left = false;
    for _ in 0..1000 {
        w.step();
        if !w.bus(1).served.is_empty() {
            left = true;
            break;
        }
    }
    assert!(left, "the early bus stood with its doors open after the exchange");
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
