//! Stage 7 exit gate — simultaneous lane changes.
//!
//! Two vehicles moving into the same lane this tick are ordered by stable id, so exactly one
//! starts and the outcome does not depend on container order.

mod common;

use common::maneuver::{two_lanes, Car, ManeuverWorld};

fn world(order: &[u64]) -> ManeuverWorld {
    let mut w = ManeuverWorld::new(two_lanes());
    for &id in order {
        let s = if id == 1 { 60.0 } else { 90.0 };
        w.add(Car::new(id, 0, s).speed(8.0).change_to(1, 1));
    }
    w
}

#[test]
fn only_one_of_two_simultaneous_changes_starts() {
    let mut w = world(&[1, 2]);
    w.step();
    let started = w
        .cars
        .iter()
        .filter(|c| c.last.as_ref().and_then(|d| d.change).is_some())
        .count();
    assert_eq!(started, 1, "exactly one change may start");
    assert!(
        w.car(1).last.as_ref().and_then(|d| d.change).is_some(),
        "the lowest id wins the lane"
    );
}

#[test]
fn the_winner_does_not_depend_on_container_order() {
    let mut a = world(&[1, 2]);
    let mut b = world(&[2, 1]);
    a.step();
    b.step();
    let winner = |w: &ManeuverWorld| {
        w.cars
            .iter()
            .find(|c| c.last.as_ref().and_then(|d| d.change).is_some())
            .map(|c| c.id.get())
    };
    assert_eq!(winner(&a), Some(1));
    assert_eq!(winner(&b), Some(1));
}
