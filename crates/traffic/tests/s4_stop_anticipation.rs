//! Stage 4 exit gate — stops and speed changes are anticipated, without unexplained creep or
//! brake/accelerate flicker, and a stop is met within docking tolerance.
//!
//! Headless: synthetic lanes and the real longitudinal controller. No renderer or assets.

use glam::DVec3;
use traffic::{AiState, LaneBuilder, LaneKind, Network};

const DT: f32 = 1.0 / 50.0;

fn straight(length: f64) -> Network {
    let lane = LaneBuilder::polyline(
        vec![DVec3::new(0.0, 0.0, 0.0), DVec3::new(0.0, length, 0.0)],
        LaneKind::Street,
        3.0,
    );
    let mut net = Network {
        lanes: vec![lane],
        ..Default::default()
    };
    net.link(1.5);
    net
}

fn car(s: f32, speed: f32) -> AiState {
    let mut c = AiState::new(0, s, 5);
    c.front = 2.25;
    c.rear = 2.25;
    c.length = 4.5;
    c.max_speed_kmh = 90.0;
    c.speed = speed;
    c
}

#[test]
fn a_stop_is_reached_without_creep_or_flicker() {
    let net = straight(200.0);
    let line = 60.0f32;
    let mut c = car(0.0, 13.9);
    c.plan_next(&net);

    let mut stopped = None;
    let mut restarted = false;
    let mut flickers = 0u32;
    let mut last_sign = 0i32;

    for _ in 0..1200 {
        c.drive(&net, DT, None, Some(line - c.s));
        if c.speed < 0.05 {
            stopped.get_or_insert(c.s);
        }
        // Once it has stopped at the line it must not creep forward again.
        if stopped.is_some() && c.speed > 0.5 {
            restarted = true;
        }
        // A brake/accelerate flicker is a sign change between meaningful commands.
        let sign = if c.acc < -0.3 {
            -1
        } else if c.acc > 0.3 {
            1
        } else {
            0
        };
        if sign != 0 && last_sign != 0 && sign != last_sign {
            flickers += 1;
        }
        if sign != 0 {
            last_sign = sign;
        }
    }

    let at = stopped.expect("the car never stopped at the line");
    let front = at + c.front;
    assert!(
        front <= line + 0.05 && front > line - 0.9,
        "the stop missed tolerance: front {front:.2} vs line {line:.2}"
    );
    assert!(!restarted, "the car crept forward again after stopping at the line");
    assert!(
        flickers <= 1,
        "the car flickered between braking and accelerating {flickers} times"
    );
}

#[test]
fn a_lower_speed_limit_ahead_is_anticipated() {
    let lane0 = LaneBuilder::polyline(
        vec![DVec3::new(0.0, 0.0, 0.0), DVec3::new(0.0, 260.0, 0.0)],
        LaneKind::Street,
        3.0,
    );
    let mut lane1 = LaneBuilder::polyline(
        vec![DVec3::new(0.0, 260.0, 0.0), DVec3::new(0.0, 420.0, 0.0)],
        LaneKind::Street,
        3.0,
    );
    lane1.speed_limit_kmh = 30.0;
    let mut net = Network {
        lanes: vec![lane0, lane1],
        ..Default::default()
    };
    net.link(1.5);

    let mut c = car(0.0, 14.0);
    c.plan_next(&net);

    let mut braked_early = false;
    let mut hardest = 0.0f32;
    let mut speed_at_joint = None;
    for _ in 0..1600 {
        if c.lane == 0 && net.lanes[0].length() - c.s > 30.0 && c.acc < -0.2 {
            braked_early = true; // slowed well before the lower limit
        }
        c.drive(&net, DT, None, None);
        hardest = hardest.min(c.acc);
        if c.lane == 1 && speed_at_joint.is_none() {
            speed_at_joint = Some(c.speed);
        }
    }

    assert!(braked_early, "the car did not anticipate the lower limit ahead");
    assert!(
        hardest > -3.0,
        "anticipating a speed limit braked too hard: {hardest:.2} m/s²"
    );
    let v = speed_at_joint.expect("the car never reached the lower-limit lane");
    assert!(
        v <= 30.0 / 3.6 + 0.7,
        "the car entered the 30 zone at {v:.2} m/s (limit {:.2})",
        30.0 / 3.6
    );
}
