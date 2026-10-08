//! Stage 7 exit gate — oncoming abort.
//!
//! A pass that is overtaken by oncoming traffic is given up: the car returns along its
//! S-curve while it still can, and never snaps back to the lane.

mod common;

use common::maneuver::{oncoming, two_way, Car, ManeuverWorld};
use traffic::ManeuverPhase;

fn passing_car() -> Car {
    let mut c = Car::new(1, 0, 60.0).speed(6.0);
    c.stopped = 5.0;
    c.lead_gap = Some(6.0);
    c.lead_standing = true;
    c.obstacle_len = 12.0;
    c
}

#[test]
fn an_oncoming_car_makes_the_pass_abort_and_return() {
    let mut w = ManeuverWorld::new(two_way());
    w.add(passing_car());
    w.step();
    assert!(w.car(1).state.passing.is_some(), "the pass should have started");
    // Somebody comes the other way now.
    w.extras.push(oncoming(9, 1, 352.0, 14.0));
    w.step();
    let car = w.car(1);
    let d = car.last.as_ref().unwrap();
    assert_eq!(d.phase, ManeuverPhase::PassingAbort);
    assert_eq!(d.lateral_target, Some(0.0), "must steer back, not snap");
    assert!(d.stop_at.is_some(), "should stop short of the obstruction");
}
