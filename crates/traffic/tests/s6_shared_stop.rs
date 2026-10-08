//! Stage 6 exit gate — three buses sharing one curb stop.
//!
//! One berth, three scheduled buses arriving a few seconds apart. A bus waiting upstream has
//! not served the stop: only the berth owner may board, each bus serves the stop exactly once,
//! and a following bus does not dock through the one that is still departing.

mod common;

use common::service::{berth, Bus, ServiceWorld};
use traffic::ServicePhase;

const STOP: i64 = 7001;
const BERTH_S: f32 = 120.0;

fn three_buses() -> ServiceWorld {
    let mut w = ServiceWorld::new();
    w.add(Bus::new(1, 90.0, vec![berth(STOP, BERTH_S, 36000.0)]));
    w.add(Bus::new(2, 70.0, vec![berth(STOP, BERTH_S, 36000.0)]));
    w.add(Bus::new(3, 50.0, vec![berth(STOP, BERTH_S, 36000.0)]));
    w
}

#[test]
fn s6_one_berth_shared_by_three_buses_served_once_each() {
    let mut w = three_buses();
    let mut max_holders = 0usize;
    let mut max_boarders = 0usize;
    for _ in 0..4000 {
        w.step();
        let holders = w
            .buses
            .iter()
            .filter(|b| b.state.holds_berth())
            .count();
        let boarders = w.buses.iter().filter(|b| b.phase() == ServicePhase::Boarding).count();
        max_holders = max_holders.max(holders);
        max_boarders = max_boarders.max(boarders);
        assert!(holders <= 1, "two berth owners at t={:.1}", w.day_time - 36000.0);
        assert!(boarders <= 1, "two buses boarding at once");
        // A bus may only be boarding when it holds the berth.
        for b in &w.buses {
            if b.phase() == ServicePhase::Boarding {
                assert_eq!(b.state.berth.map(|h| h.stop), Some(traffic::StopId(STOP)));
            }
        }
    }
    assert_eq!(max_holders, 1, "the berth should be held exactly once at a time");
    assert_eq!(max_boarders, 1, "only one bus may ever board");
    for b in &w.buses {
        assert_eq!(b.served, vec![traffic::StopId(STOP)], "each bus serves the stop once");
        assert_eq!(b.stops.len(), 0);
    }
}

#[test]
fn s6_a_follower_does_not_dock_through_a_departing_bus() {
    let mut w = three_buses();
    let mut saw_depart_with_second_approaching = false;
    for _ in 0..4000 {
        w.step();
        let first = w.bus(1);
        if matches!(first.phase(), ServicePhase::Departing) {
            let second = w.bus(2);
            // While the first still physically overlaps the berth region, the second must
            // not be inside it in a berth-holding phase.
            let overlapping = (first.s - BERTH_S).abs() <= traffic::DEFAULT_BOARDING_REGION
                || (first.s - first.rear) < BERTH_S + traffic::DEFAULT_BOARDING_REGION;
            if overlapping {
                assert!(
                    !second.state.holds_berth(),
                    "the follower docked through the departing bus at t={:.1}",
                    w.day_time - 36000.0
                );
                saw_depart_with_second_approaching = true;
            }
        }
    }
    assert!(saw_depart_with_second_approaching, "the departing/queue overlap never happened");
}
