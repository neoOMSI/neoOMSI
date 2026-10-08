//! Stage 7 exit gate — no indefinite maneuver oscillation.
//!
//! Under a steady input a discretionary maneuver does not flap back and forth: consecutive
//! changes keep the cooldown and the anti-oscillation window apart.

mod common;

use common::maneuver::{obstacle, two_lanes, Car, ManeuverWorld};
use traffic::OSCILLATION_WINDOW;

#[test]
fn discretionary_changes_do_not_flap() {
    let mut w = ManeuverWorld::new(two_lanes());
    // A standing obstruction in the car's own lane makes it overtake to the left once.
    w.add(Car::new(1, 0, 60.0).speed(8.0));
    w.extras.push(obstacle(9, 0, 80.0, 0.0));
    let mut lane = w.car(1).lane;
    let mut change_times: Vec<f32> = Vec::new();
    for _ in 0..1500 {
        w.step();
        let now = w.car(1).lane;
        if now != lane {
            change_times.push(w.time);
            lane = now;
        }
    }
    assert!(
        change_times.len() <= 2,
        "too many changes under steady input: {change_times:?}"
    );
    for pair in change_times.windows(2) {
        assert!(
            pair[1] - pair[0] >= OSCILLATION_WINDOW - 0.1,
            "changes too close together: {change_times:?}"
        );
    }
}
