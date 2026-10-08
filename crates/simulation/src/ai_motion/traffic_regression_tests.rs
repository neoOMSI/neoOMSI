//! Exercise controller decisions through the actual bicycle-model realization.
use super::{AiBody, MotionKind, tests::golf};
use glam::{DVec2, DVec3};
use traffic::*;

fn road() -> Network {
    let mut net = Network {
        lanes: vec![LaneBuilder::polyline(
            vec![DVec3::ZERO, DVec3::new(0.0, 400.0, 0.0)],
            LaneKind::Street,
            3.5,
        )],
        ..Default::default()
    };
    net.link(1.5);
    net
}

#[test]
fn emergency_courtesy_moves_a_standing_real_body_without_teleporting() {
    let net = road();
    let mut state = AiState::new(0, 60.0, 1);
    state.front = 2.1;
    state.rear = 2.2;
    state.length = 4.3;
    state.plan_next(&net);
    let mut body = AiBody::new(&golf(), MotionKind::Road);
    body.place(&|d| state.way_point(&net, d), None, None, 0.0);
    let mut coordinator = ManeuverCoordinator::new();
    let mut memory = ManeuverState::default();
    let dt = 0.02;
    for tick in 0..1200 {
        let mut actor = ManeuverActor::new(VehicleId(1), 0, state.s);
        actor.half_width = 0.85;
        actor.speed = state.speed;
        actor.odometer = state.odometer;
        actor.lateral = body.position.x as f32;
        let actors = [actor];
        let occupancy = Occupancy::default();
        let scene = ManeuverScene {
            net: &net,
            occupancy: &occupancy,
            actors: &actors,
            people: &[],
            static_clearance: None,
            time: tick as f32 * dt,
            dt,
            tick,
        };
        let mut inputs = ManeuverInputs::new(0);
        inputs.emergency = Some(EmergencyApproach {
            vehicle: VehicleId(9),
            gap: 20.0,
        });
        inputs.lead_gap = Some(8.0);
        let decision = coordinator.plan(&scene, &mut memory, &inputs);
        state.lateral_target = decision.lateral_target.unwrap_or(state.lateral_target);
        if let Some(ramp) = decision.lateral_ramp {
            state.lateral_ramp = ramp;
        }
        state.accel_cap = decision.accel_cap;
        let previous_odometer = state.odometer;
        state.drive(&net, dt, None, None);
        let before = body.position;
        body.step(dt, state.speed, &|d| state.way_point(&net, d), None, None);
        assert!(
            (body.position - before).length() < 0.04,
            "courtesy teleported the vehicle"
        );
        let speed = body.realized_speed(dt);
        state.commit_feedback(
            &net,
            RealizedMotion {
                pose: body.position,
                heading_deg: body.heading as f32,
                speed,
                half_width: 0.85,
            },
        );
        state.odometer = previous_odometer + speed * dt;
    }
    assert!(
        body.position.x > 0.5,
        "standing vehicle failed to move aside: {:?}",
        body.position
    );
    assert!(
        body.position.y < 68.0,
        "courtesy used more than the available queue gap"
    );
    assert!(
        state.speed < 0.1,
        "vehicle did not wait once it had moved aside"
    );
}

#[test]
fn scheduled_lane_change_keeps_the_real_body_smooth_through_feedback() {
    let mut net = road();
    net.lanes.push(LaneBuilder::polyline(
        vec![DVec3::new(-3.5, 0.0, 0.0), DVec3::new(-3.5, 400.0, 0.0)],
        LaneKind::Street,
        3.5,
    ));
    net.link(1.5);
    let mut state = AiState::new(0, 60.0, 1);
    state.set_route(&net, vec![0, 1], 60.0);
    state.speed = 8.0;
    let mut body = AiBody::new(&golf(), MotionKind::Road);
    body.place(&|d| state.way_point(&net, d), None, None, state.speed);
    state.start_route_change(&net, 1, 1);
    for _ in 0..500 {
        let before = body.position;
        let previous_odometer = state.odometer;
        state.drive(&net, 0.02, None, None);
        body.step(0.02, state.speed, &|d| state.way_point(&net, d), None, None);
        assert!(
            (body.position - before).length() < 0.4,
            "lane change jumped"
        );
        let speed = body.realized_speed(0.02);
        state.commit_feedback(
            &net,
            RealizedMotion {
                pose: body.position,
                heading_deg: body.heading as f32,
                speed,
                half_width: 0.85,
            },
        );
        state.odometer = previous_odometer + speed * 0.02;
        assert!(
            body.a_lat.abs() < 3.5,
            "abrupt sideways acceleration: {}",
            body.a_lat
        );
    }
    assert_eq!(state.lane, 1);
    assert_eq!(state.route_index, 1);
    assert!(
        (body.position.x + 3.5).abs() < 0.2,
        "did not settle in target lane"
    );
}

#[test]
fn bus_bypass_keeps_its_curve_and_real_body_across_a_spline_joint() {
    let mut net = Network::default();
    for (x, from, to) in [
        (0.0, 0.0, 30.0),
        (-3.0, 0.0, 30.0),
        (0.0, 30.0, 200.0),
        (-3.0, 30.0, 200.0),
    ] {
        net.lanes.push(LaneBuilder::polyline(
            vec![DVec3::new(x, from, 0.0), DVec3::new(x, to, 0.0)],
            LaneKind::Street,
            3.0,
        ));
    }
    net.link(1.5);
    for (right, left) in [(0, 1), (2, 3)] {
        net.lanes[right].left = Some(left);
        net.lanes[left].right = Some(right);
    }
    let mut state = AiState::new(0, 28.3, 1);
    state.front = 2.1;
    state.rear = 2.1;
    state.length = 4.2;
    state.plan_next(&net);
    let mut body = AiBody::new(&golf(), MotionKind::Road);
    body.place(&|d| state.way_point(&net, d), None, None, 0.0);
    state.start_bypass(&net, 1, 1);
    state.accel_cap = Some(PULL_OUT_ACCEL);
    let bus_parts = [
        BodyFootprint::new(
            VehicleId(93),
            DVec2::new(0.0, 48.4),
            DVec2::Y,
            4.78,
            1.24,
            0.0,
            3.0,
            0.0,
        ),
        BodyFootprint::new(
            VehicleId(93),
            DVec2::new(0.0, 39.3),
            DVec2::Y,
            3.6,
            1.24,
            0.0,
            3.0,
            0.0,
        ),
    ];
    let mut crossed_during_change = false;
    for _ in 0..700 {
        let before = body.position;
        let previous_odometer = state.odometer;
        state.drive(&net, 0.02, None, None);
        if state.lane == 2 && state.change.is_some() {
            crossed_during_change = true;
        }
        body.step(0.02, state.speed, &|d| state.way_point(&net, d), None, None);
        let h = body.heading.to_radians();
        let footprint = BodyFootprint::new(
            VehicleId(167),
            body.position.truncate(),
            DVec2::new(h.sin(), h.cos()),
            2.1,
            0.85,
            0.0,
            1.5,
            state.speed,
        );
        assert!(
            bus_parts.iter().all(|part| !part.overlaps(&footprint, 0.0)),
            "bypass steered into the articulated bus at {:?}",
            body.position
        );
        assert!(
            (body.position - before).length() < 0.4,
            "bypass teleported at the joint"
        );
        assert!(
            body.a_lat.abs() < 3.5,
            "abrupt sideways acceleration: {}",
            body.a_lat
        );
        let speed = body.realized_speed(0.02);
        state.commit_feedback(
            &net,
            RealizedMotion {
                pose: body.position,
                heading_deg: body.heading as f32,
                speed,
                half_width: 0.85,
            },
        );
        state.odometer = previous_odometer + speed * 0.02;
    }
    assert!(
        crossed_during_change,
        "change was snapped complete at the spline end"
    );
    assert_eq!(state.lane, 3);
    assert!(
        (body.position.x + 3.0).abs() < 0.2,
        "did not settle on the left lane"
    );
}
