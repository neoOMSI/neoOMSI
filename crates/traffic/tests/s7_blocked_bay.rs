//! Stage 7 exit gate — a blocked bus bay and optional passing.
//!
//! Passing a standing obstruction is optional: it starts only with the whole outbound and
//! return trajectory checked, and waits when the oncoming gap is not there.

mod common;

use common::maneuver::{oncoming, two_way, Car, ManeuverWorld};
use traffic::ManeuverPhase;

fn passer() -> Car {
    let mut c = Car::new(1, 0, 60.0).speed(6.0);
    c.stopped = 5.0;
    c.lead_gap = Some(6.0);
    c.lead_standing = true;
    c.obstacle_len = 12.0;
    c
}

#[test]
fn a_pass_starts_when_the_oncoming_lane_is_clear() {
    let mut w = ManeuverWorld::new(two_way());
    w.add(passer());
    w.step();
    assert_eq!(w.car(1).last.as_ref().unwrap().phase, ManeuverPhase::Passing);
    assert!(w.car(1).state.passing.is_some());
    assert!(w.car(1).lateral_target < -1.0, "pulls over to the oncoming side");
}

#[test]
fn a_pass_waits_while_somebody_is_coming() {
    let mut w = ManeuverWorld::new(two_way());
    w.add(passer());
    // An oncoming car close enough that it would meet the passer.
    w.extras.push(oncoming(9, 1, 355.0, 15.0));
    w.step();
    assert!(w.car(1).state.passing.is_none(), "must not pass into oncoming traffic");
    assert_ne!(w.car(1).last.as_ref().unwrap().phase, ManeuverPhase::Passing);
}
