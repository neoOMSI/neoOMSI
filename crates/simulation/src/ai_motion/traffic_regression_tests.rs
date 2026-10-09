//! Exercise controller decisions through the actual bicycle-model realization.
use super::{AiBody, MotionKind, tests::golf, tests::lorry};
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
fn a_blocked_long_vehicle_can_turn_its_wheels_without_moving_through_the_obstacle() {
    let mut body = AiBody::new(&lorry(), MotionKind::Road);
    let way = super::straight_pull_out(3.0, 12.0);
    body.place(&way, None, None, 0.0);
    let before = body.clone();
    for _ in 0..100 {
        body.step(0.02, 1.0, &way, None, None);
        body.reject_motion(&before);
        assert_eq!(body.position, before.position);
        assert_eq!(body.heading, before.heading);
        assert_eq!(body.realized_speed(0.02), 0.0);
    }
    assert!(body.steer < -10.0, "rollback also erased the wheel turn: {}", body.steer);
    body.step(0.02, 1.0, &way, None, None);
    assert!(body.heading > 359.0, "turned wheels should steer immediately when room becomes available");
    assert!(body.realized_speed(0.02) > 0.9);
}

#[test]
fn a_long_bus_recovers_at_a_parked_cars_corner_by_finishing_its_wheel_turn() {
    use crate::collision::Obb;
    let mut def = lorry();
    def.rot_pnt_long = -3.0;
    def.axles[0].long = 2.9;
    def.axles[1].long = -3.0;
    let extent = (5.68, 3.88, 1.24);
    let parked = Obb::from_box([1.7, 4.4, 1.5, 0.0, 0.0, 0.75],
        DVec3::new(2.8, 7.39, 0.0), 60.0);
    let curve = super::straight_pull_out(4.0, 6.0);
    let mut body = AiBody::new(&def, MotionKind::Road);
    body.place(&curve, None, None, 0.0);
    let footprint = |b: &AiBody| Obb::vehicle(b.position.truncate(), b.heading,
        extent.0, extent.1, extent.2);
    assert!(!footprint(&body).overlaps_plan(&parked));
    let mut distance = 0.0;
    let mut rejected = 0;
    for _ in 0..1000 {
        let before = body.clone();
        body.step(0.02, 1.0, &|d| curve(distance + d), None, None);
        if footprint(&body).overlaps_plan(&parked) {
            body.reject_motion(&before);
            body.recover_corner(&|d| curve(distance + d),
                (extent.0 as f32, extent.1 as f32, extent.2 as f32), &parked);
            rejected += 1;
        }
        assert!(!footprint(&body).overlaps_plan(&parked), "bus drove through the parked car");
        distance += body.realized_speed(0.02) * 0.02;
    }
    assert!(rejected > 0, "fixture must exercise a blocked corner");
    assert!(distance > 15.0, "bus remained stuck after turning its wheels: {distance}, heading {}, steer {}", body.heading, body.steer);
}

#[test]
fn a_real_car_passes_a_stopped_bus_on_the_oncoming_lane_and_returns_without_contact() {
    let mut net = road();
    net.lanes.push(LaneBuilder::polyline(
        vec![DVec3::new(-3.5, 400.0, 0.0), DVec3::new(-3.5, 0.0, 0.0)],
        LaneKind::Street, 3.5));
    net.link(1.5);
    let mut state = AiState::new(0, 60.0, 1);
    state.front = 2.1;
    state.rear = 2.1;
    state.length = 4.2;
    state.plan_next(&net);
    let mut body = AiBody::new(&golf(), MotionKind::Road);
    body.place(&|d| state.way_point(&net, d), None, None, 0.0);
    let bus = BodyFootprint::new(VehicleId(2), DVec2::new(0.0, 72.35), DVec2::Y,
        5.0, 1.25, 0.0, 3.5, 0.0);
    let occupancy = Occupancy::build(net.version(), 0, vec![bus]);
    let mut coordinator = ManeuverCoordinator::new();
    let mut memory = ManeuverState::default();
    let mut started = false;
    let mut returned = false;
    for tick in 0..1600 {
        let mut actor = ManeuverActor::new(VehicleId(1), 0, state.s);
        actor.front = state.front;
        actor.rear = state.rear;
        actor.length = state.length;
        actor.half_width = 0.85;
        actor.speed = state.speed;
        actor.odometer = state.odometer;
        actor.lateral = state.lateral;
        actor.stopped = if !started { 5.0 } else { 0.0 };
        let actors = [actor];
        let scene = ManeuverScene { net: &net, occupancy: &occupancy, actors: &actors,
            people: &[], static_clearance: None, time: tick as f32 * 0.02, dt: 0.02, tick };
        let mut input = ManeuverInputs::new(0);
        input.lead_gap = Some(67.35 - state.s - state.front);
        input.lead_standing = true;
        input.obstacle_len = 10.0;
        let decision = coordinator.plan(&scene, &mut memory, &input);
        started |= memory.passing.is_some();
        if let Some(target) = decision.lateral_target { state.lateral_target = target; }
        if let Some(ramp) = decision.lateral_ramp { state.lateral_ramp = ramp; }
        state.accel_cap = decision.accel_cap;
        let previous = state.odometer;
        state.drive(&net, 0.02, None, decision.stop_at);
        body.step(0.02, state.speed, &|d| state.way_point(&net, d), None, None);
        let h = body.heading.to_radians();
        let foot = BodyFootprint::new(VehicleId(1), body.position.truncate(),
            DVec2::new(h.sin(), h.cos()), 2.1, 0.85, 0.0, 1.5, state.speed);
        assert!(!foot.overlaps(&bus, 0.0), "car clipped the bus at {:?}", body.position);
        let speed = body.realized_speed(0.02);
        state.commit_feedback(&net, RealizedMotion { pose: body.position,
            heading_deg: body.heading as f32, speed, half_width: 0.85 });
        state.odometer = previous + speed * 0.02;
        if started && memory.passing.is_none() && body.position.y > 90.0 && body.position.x.abs() < 0.2 {
            returned = true;
            break;
        }
    }
    assert!(started, "passing did not start");
    assert!(returned, "car did not return from the oncoming lane: {:?}", body.position);
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
