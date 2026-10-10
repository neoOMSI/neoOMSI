//! Stage 7 exit gate — articulated (long/wide) clearance.
//!
//! The swept body of a longer or wider vehicle is checked for the whole passing trajectory,
//! so a maneuver a car can make is not started when the larger body would clip the obstacle.

mod common;

use common::maneuver::{obstacle, two_way, Car, ManeuverWorld};
use traffic::ManeuverPhase;

fn passer(half_width: f32) -> ManeuverWorld {
    let mut w = ManeuverWorld::new(two_way());
    let mut c = Car::new(1, 0, 60.0).speed(6.0);
    c.half_width = half_width;
    c.stopped = 5.0;
    c.lead_gap = Some(6.0);
    c.lead_standing = true;
    c.obstacle_len = 12.0;
    w.add(c);
    // The obstruction itself, so the swept path must clear its body.
    w.extras.push(obstacle(9, 0, 66.0, 0.0));
    w
}

#[test]
fn a_car_clears_the_obstacle() {
    let mut w = passer(1.25);
    w.step();
    assert_eq!(w.car(1).last.as_ref().unwrap().phase, ManeuverPhase::Passing);
}

#[test]
fn a_wider_body_does_not_start_the_same_pass() {
    let mut w = passer(2.5);
    w.step();
    assert!(w.car(1).state.passing.is_none(), "the wider body would clip the obstacle");
    assert_ne!(w.car(1).last.as_ref().unwrap().phase, ManeuverPhase::Passing);
}
