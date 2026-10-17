//! Stage 4 exit gate — stopped and braking leaders stay collision-free, and hard braking is
//! reserved for real emergencies.
//!
//! Headless: a straight synthetic lane and the real longitudinal controller
//! (`AiState::drive`). No renderer and no OMSI assets.

use glam::DVec3;
use traffic::{AiState, LaneBuilder, LaneKind, Lead, Network};

const DT: f32 = 1.0 / 50.0;
const FRONT: f32 = 2.25;

fn straight() -> Network {
    let lane = LaneBuilder::polyline(
        vec![DVec3::new(0.0, 0.0, 0.0), DVec3::new(0.0, 500.0, 0.0)],
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

fn follower(s: f32, speed: f32, seed: u64) -> AiState {
    let mut car = AiState::new(0, s, seed);
    car.front = FRONT;
    car.rear = FRONT;
    car.length = FRONT * 2.0;
    car.max_speed_kmh = 90.0;
    car.speed = speed;
    car
}

/// The leader brakes comfortably and stops; the follower must follow it without ever using
/// the emergency channel and without a body overlap.
#[test]
fn a_comfortable_leader_brake_needs_no_emergency() {
    let net = straight();
    let mut car = follower(0.0, 12.0, 11);
    car.plan_next(&net);

    let (mut lead_s, mut lead_v) = (30.0f32, 12.0f32);
    let (mut min_gap, mut min_acc) = (f32::MAX, 0.0f32);
    let mut emergency = false;
    let mut stopped_at = None;

    for k in 0..500 {
        if k > 30 {
            lead_v = (lead_v - 3.0 * DT).max(0.0); // a comfortable 3 m/s² stop
        }
        lead_s += lead_v * DT;
        let gap = lead_s - FRONT - (car.s + car.front);
        min_gap = min_gap.min(gap);
        car.drive(
            &net,
            DT,
            Some(Lead {
                gap,
                speed: lead_v,
                acc: if k > 30 && lead_v > 0.0 { -3.0 } else { 0.0 },
            }),
            None,
        );
        min_acc = min_acc.min(car.acc);
        emergency |= car.emergency;
        if stopped_at.is_none() && car.speed < 0.05 && car.s > 5.0 {
            stopped_at = Some(car.s);
        }
    }

    assert!(min_gap > 0.8, "the follower came within {min_gap:.2} m of the leader");
    assert!(car.speed < 0.05, "the follower never stopped: {:.2} m/s", car.speed);
    assert!(!emergency, "a comfortable leader brake must not use the emergency channel");
    assert!(min_acc > -4.5, "the follower braked at {min_acc:.2} m/s²");
    assert!(stopped_at.is_some(), "the follower never came to rest behind the leader");
}

/// A cut-in at a short gap with the leader braking far harder than comfort: the follower must
/// use its emergency channel and still stay collision-free because the physics allow it.
#[test]
fn an_emergency_uses_the_hard_channel_but_stays_collision_free() {
    let net = straight();
    let mut car = follower(0.0, 12.0, 13);
    car.plan_next(&net);

    // Close, stationary relative gap and a leader that brakes at 8 m/s² from the start.
    let (mut lead_s, mut lead_v) = (8.0f32 + FRONT + car.front, 12.0f32);
    let mut min_gap = f32::MAX;
    let mut emergency = false;
    let mut hardest = 0.0f32;

    for _ in 0..400 {
        lead_v = (lead_v - 8.0 * DT).max(0.0);
        lead_s += lead_v * DT;
        let gap = lead_s - FRONT - (car.s + car.front);
        min_gap = min_gap.min(gap);
        car.drive(
            &net,
            DT,
            Some(Lead {
                gap,
                speed: lead_v,
                acc: if lead_v > 0.0 { -8.0 } else { 0.0 },
            }),
            None,
        );
        emergency |= car.emergency;
        hardest = hardest.min(car.acc);
    }

    assert!(emergency, "a hard cut-in must engage collision prevention");
    assert!(hardest <= -4.0, "the emergency channel did not brake hard: {hardest:.2} m/s²");
    assert!(
        min_gap > -0.05,
        "the follower overlapped the leader: min gap {min_gap:.2} m"
    );
    assert!(car.speed < 0.05, "the follower never stopped: {:.2} m/s", car.speed);
}

/// Hard braking is the emergency channel only: the comfort command is bounded by the
/// vehicle's ordinary maximum.
#[test]
fn the_comfort_channel_respects_the_braking_envelope() {
    let net = straight();
    let mut car = follower(0.0, 13.9, 17);
    car.plan_next(&net);
    // A stop line close enough that only gentle braking is ever needed.
    for _ in 0..1500 {
        car.drive(&net, DT, None, Some(50.0 - car.s));
        assert!(!car.emergency, "a distant stop line must not be an emergency");
        assert!(
            car.acc >= -car.brakes.max_decel - 1e-3,
            "comfort braking {} exceeded the vehicle maximum {}",
            car.acc,
            car.brakes.max_decel
        );
    }
    assert!(car.speed < 0.05, "speed {:.3} at s {:.2}", car.speed, car.s);
}
