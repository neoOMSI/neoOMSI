//! Stage 7 exit gate — route-required turn lanes.
//!
//! A lane the route requires next is planned early; when it cannot be taken it waits legally
//! before the end of the lane instead of cutting through traffic or jumping to another lane.

mod common;

use common::maneuver::{two_lanes, Car, ManeuverWorld};
use traffic::{ChangeKind, ManeuverPhase};

#[test]
fn a_route_required_change_is_started_when_the_lane_is_clear() {
    let mut w = ManeuverWorld::new(two_lanes());
    w.add(Car::new(1, 0, 50.0).speed(8.0).change_to(1, 1));
    w.step();
    let car = w.car(1);
    let d = car.last.as_ref().unwrap();
    assert_eq!(d.phase, ManeuverPhase::RouteChange);
    let cmd = d.change.expect("the required change should start");
    assert_eq!(cmd.to, 1);
    assert_eq!(cmd.kind, ChangeKind::RouteChange);
    assert_eq!(car.state.change_to, Some(1));
}

#[test]
fn a_blocked_required_change_waits_and_does_not_jump_lanes() {
    let mut w = ManeuverWorld::new(two_lanes());
    w.add(Car::new(1, 0, 50.0).speed(8.0).change_to(1, 1));
    // Somebody already stands where the car would move over: the change is not started.
    w.extras.push(common::maneuver::obstacle(9, 1, 50.0, 0.0));
    w.step();
    let car = w.car(1);
    let d = car.last.as_ref().unwrap();
    assert!(d.change.is_none(), "must not change into an occupied lane");
    assert_eq!(car.lane, 0, "must not jump lanes");
    assert!(d.stop_at.is_some(), "should signal and wait before the lane end");
    assert_eq!(d.phase, ManeuverPhase::RouteChange);
}

#[test]
fn the_change_completes_onto_the_required_lane() {
    let mut w = ManeuverWorld::new(two_lanes());
    w.add(Car::new(1, 0, 50.0).speed(8.0).change_to(1, 1));
    w.run(400);
    assert_eq!(w.car(1).lane, 1, "the car should end on the required lane");
    assert!(w.car(1).change.is_none());
}
