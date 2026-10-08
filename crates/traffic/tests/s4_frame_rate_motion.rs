//! Stage 4 exit gate — motion is stable across frame rates and lane joints.
//!
//! The test drives a small independent body from the longitudinal controller's command and
//! commits route progress back with `commit_feedback`, exactly the body-owns-pose contract
//! the engine uses. No renderer or OMSI assets.

use glam::DVec3;
use traffic::scenario::advance_fixed_clock;
use traffic::{AiState, LaneBuilder, LaneKind, Network, RealizedMotion};

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

/// Two linked lanes meeting at a shallow bend, so the body crosses a real lane joint.
fn bend() -> Network {
    let a = LaneBuilder::polyline(
        vec![DVec3::new(0.0, 0.0, 0.0), DVec3::new(0.0, 120.0, 0.0)],
        LaneKind::Street,
        3.0,
    );
    let b = LaneBuilder::polyline(
        vec![DVec3::new(0.0, 120.0, 0.0), DVec3::new(24.0, 240.0, 0.0)],
        LaneKind::Street,
        3.0,
    );
    let mut net = Network {
        lanes: vec![a, b],
        ..Default::default()
    };
    net.link(1.5);
    net
}

struct Body {
    pos: DVec3,
}

fn new_car(net: &Network) -> AiState {
    let mut c = AiState::new(0, 0.0, 9);
    c.front = 2.25;
    c.rear = 2.25;
    c.length = 4.5;
    c.max_speed_kmh = 50.0;
    c.plan_next(net);
    c
}

/// One fixed step: the controller commands, the body realizes, the realized pose is fed back.
fn step(net: &Network, car: &mut AiState, body: &mut Body, dt: f32) {
    car.drive(net, dt, None, None);
    let lane = net.lanes[car.lane].length();
    let (_, h) = net.lanes[car.lane].at(car.s.min(lane));
    let r = (h as f64).to_radians();
    let fwd = DVec3::new(r.sin(), r.cos(), 0.0);
    body.pos += fwd * (car.speed as f64 * dt as f64);
    car.commit_feedback(
        net,
        RealizedMotion {
            pose: body.pos,
            heading_deg: h,
            speed: car.speed,
            half_width: 1.25,
        },
    );
}

/// Run `seconds` of motion, stepping `fps` render frames of a fixed clock, and return the
/// committed odometer after every fixed tick.
fn run_partitioned(fps: f32, seconds: f32) -> Vec<f32> {
    let net = straight(600.0);
    let mut car = new_car(&net);
    let mut body = Body {
        pos: net.lanes[0].at(0.0).0,
    };
    let mut accum = 0.0f32;
    let mut odometers = Vec::new();
    let frames = (seconds * fps).round() as u32;
    for _ in 0..frames {
        let n = advance_fixed_clock(&mut accum, 1.0 / fps, DT, 8);
        for _ in 0..n {
            step(&net, &mut car, &mut body, DT);
            odometers.push(car.odometer);
        }
    }
    odometers
}

#[test]
fn the_same_fixed_ticks_give_the_same_motion_at_any_frame_rate() {
    let baseline = run_partitioned(60.0, 8.0);
    for fps in [15.0f32, 30.0, 60.0, 144.0] {
        let run = run_partitioned(fps, 8.0);
        // A partition may land one tick short at an exact boundary; every tick it did run
        // must be decision-for-decision identical to the baseline.
        assert!(
            run.len().abs_diff(baseline.len()) <= 1,
            "{fps} fps ran {} fixed ticks, 60 fps ran {}",
            run.len(),
            baseline.len()
        );
        let common = run.len().min(baseline.len());
        assert_eq!(
            &run[..common],
            &baseline[..common],
            "the committed motion diverged at {fps} fps"
        );
    }
}

#[test]
fn motion_is_stable_across_fixed_steps_and_a_lane_joint() {
    let net = bend();
    let mut car = new_car(&net);
    let mut body = Body {
        pos: net.lanes[0].at(0.0).0,
    };
    let mut min_delta = f32::MAX;
    let mut prev_odo = 0.0f32;
    for _ in 0..1500 {
        step(&net, &mut car, &mut body, DT);
        min_delta = min_delta.min(car.odometer - prev_odo);
        prev_odo = car.odometer;
    }
    assert!(
        min_delta > -1e-3,
        "the committed progress went backwards by {min_delta}"
    );
    assert_eq!(car.lane, 1, "the body never crossed the lane joint");
    assert!(car.odometer > 200.0, "little progress: {:.1} m", car.odometer);

    // A different fixed step over the same simulated time stays close and equally stable.
    let mut fine = new_car(&net);
    let mut fine_body = Body {
        pos: net.lanes[0].at(0.0).0,
    };
    for _ in 0..3000 {
        step(&net, &mut fine, &mut fine_body, DT * 0.5);
    }
    assert_eq!(fine.lane, 1);
    assert!(
        (fine.odometer - car.odometer).abs() < 5.0,
        "the committed progress differed with the step size: {:.2} vs {:.2}",
        fine.odometer,
        car.odometer
    );
    assert!(car.speed.is_finite() && fine.speed.is_finite());
}
