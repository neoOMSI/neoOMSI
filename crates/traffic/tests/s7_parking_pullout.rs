//! Stage 7 exit gate — parking arrival and pull-out.
//!
//! Parking arrival and a parked car's pull-out go through the same maneuver owner as any
//! other lateral intent.

mod common;

use common::maneuver::{one_way, Car, ManeuverWorld};
use traffic::{ManeuverPhase, ParkPlan};

#[test]
fn parking_arrival_moves_into_the_space_and_finishes() {
    let mut w = ManeuverWorld::new(one_way());
    let mut c = Car::new(1, 0, 120.0);
    c.lateral = 1.6;
    c.state.park = Some(ParkPlan {
        key: 7,
        lane: 0,
        s: 120.0,
        lat: 1.6,
        ramped: false,
        done: false,
    });
    w.add(c);
    w.step();
    let car = w.car(1);
    assert_eq!(car.last.as_ref().unwrap().phase, ManeuverPhase::Parking);
    assert_eq!(car.lateral_target, 1.6, "the lateral target is the space");
    assert!(car.state.park.unwrap().done, "aligned and slow: the space is taken");
}

#[test]
fn a_parked_car_pulls_out_through_the_same_owner() {
    let mut w = ManeuverWorld::new(one_way());
    let mut c = Car::new(1, 0, 40.0);
    c.lateral = 1.6;
    c.state.pull_out = 1.0;
    w.add(c);
    w.step();
    let car = w.car(1);
    assert_eq!(car.last.as_ref().unwrap().phase, ManeuverPhase::PullOut);
    assert_eq!(car.lateral_target, 0.0, "pulls out into the lane");
    assert!(car.state.pull_out < 1.0, "the pull-out hold counts down");
}
