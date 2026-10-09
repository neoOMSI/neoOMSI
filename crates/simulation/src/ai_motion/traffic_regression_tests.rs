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

/// A timetable bus round a junction made of short lanes, closed loop: the planner drives,
/// the body follows its way, and the body's pose is fed back every tick, as in the game. The
/// body must move continuously: it was once put back a whole lane when the feedback moved the
/// planner over a joint without repairing the lane behind it.
#[test]
fn a_bus_round_short_junction_lanes_never_jumps() {
    let street = |pts: Vec<DVec3>| LaneBuilder::polyline(pts, LaneKind::Street, 3.0);
    let mut net = Network {
        lanes: vec![
            street(vec![DVec3::new(0.0, -60.0, 0.0), DVec3::new(0.0, 40.0, 0.0)]),
            street(vec![
                DVec3::new(0.0, 40.0, 0.0),
                DVec3::new(0.3, 44.0, 0.0),
                DVec3::new(1.5, 47.0, 0.0),
                DVec3::new(4.0, 49.5, 0.0),
                DVec3::new(8.0, 51.0, 0.0),
                DVec3::new(12.0, 51.5, 0.0),
            ]),
            street(vec![DVec3::new(12.0, 51.5, 0.0), DVec3::new(18.0, 51.5, 0.0)]),
            street(vec![DVec3::new(18.0, 51.5, 0.0), DVec3::new(120.0, 51.5, 0.0)]),
        ],
        ..Default::default()
    };
    net.link(1.5);
    for def in [lorry(), golf()] {
        let mut state = AiState::new(0, 20.0, 3);
        state.front = 5.0;
        state.rear = 5.0;
        state.length = 10.0;
        state.desire = 1.0;
        state.set_route(&net, vec![0, 1, 2, 3], 20.0);
        let mut body = AiBody::new(&def, MotionKind::Road);
        body.place(&|d| state.way_point(&net, d), None, None, 0.0);
        let dt = 0.02;
        let mut last = body.position;
        let mut worst = 0.0f64;
        for tick in 0..4000 {
            state.drive(&net, dt, None, None);
            body.step(dt, state.speed, &|d| state.way_point(&net, d), None, None);
            state.commit_feedback(
                &net,
                RealizedMotion {
                    pose: body.position,
                    heading_deg: body.heading as f32,
                    speed: body.realized_speed(dt),
                    half_width: 1.25,
                },
            );
            let moved = (body.position - last).truncate().length();
            worst = worst.max(moved);
            assert!(moved < 1.0, "tick {tick}: the body jumped {moved:.2} m (lane {})", state.lane);
            last = body.position;
            if state.lane == 3 && state.s > 40.0 {
                break;
            }
        }
        assert_eq!(state.lane, 3, "the bus drove round the corner");
        assert!(worst < 0.8);
    }
}

/// A quarter-circle turn between two straights, then a short lane and a long one: the
/// planner drives a timetable route, the body follows, the body's pose is fed back.
fn corner_net(radius: f64, short: f64) -> Network {
    let street = |pts: Vec<DVec3>| LaneBuilder::polyline(pts, LaneKind::Street, 3.0);
    let arc: Vec<DVec3> = (0..=8)
        .map(|k| {
            let a = k as f64 / 8.0 * std::f64::consts::FRAC_PI_2;
            DVec3::new(radius - radius * a.cos(), 40.0 + radius * a.sin(), 0.0)
        })
        .collect();
    let end = *arc.last().unwrap();
    let mut net = Network {
        lanes: vec![
            street(vec![DVec3::new(0.0, -40.0, 0.0), DVec3::new(0.0, 40.0, 0.0)]),
            street(arc),
            street(vec![end, end + DVec3::new(short, 0.0, 0.0)]),
            street(vec![end + DVec3::new(short, 0.0, 0.0), end + DVec3::new(short + 100.0, 0.0, 0.0)]),
        ],
        ..Default::default()
    };
    net.link(1.5);
    net
}

/// The body is the single pose owner and moves continuously: whatever the frame time, the
/// sideways offset of a bay or a swerve, the size of the vehicle or the radius of the turn,
/// it is never put somewhere else. (With the lane behind the bus left stale by the
/// realization feedback, a bus with a 0.8 m offset round a 9 m turn at 20 frames a second
/// jumped eleven metres - a lane length - back or on.)
#[test]
fn no_vehicle_is_put_somewhere_else_round_a_corner() {
    let mut cases = 0;
    for radius in [5.0, 9.0, 14.0] {
        for short in [3.0, 8.0] {
            let net = corner_net(radius, short);
            for dt in [0.02f32, 0.05] {
                for lateral in [0.0f32, 0.8, -0.8] {
                    for stop_at in [0.0f32, 20.0, 45.0] {
                        for def in [lorry(), golf()] {
                            let mut state = AiState::new(0, 10.0, 3);
                            state.front = 5.0;
                            state.rear = 5.0;
                            state.length = 10.0;
                            state.desire = 1.0;
                            state.set_route(&net, vec![0, 1, 2, 3], 10.0);
                            state.lateral_target = lateral;
                            let mut body = AiBody::new(&def, MotionKind::Road);
                            body.place(&|d| state.way_point(&net, d), None, None, 0.0);
                            let mut last = body.position;
                            for tick in 0..8000 {
                                // a red light or a leader for a moment, once past `stop_at`
                                let stop = (stop_at > 0.0
                                    && state.odometer >= stop_at
                                    && state.odometer < stop_at + 1.0
                                    && (tick as f32 * dt) as i32 % 2 == 0)
                                    .then_some(state.front + 0.5);
                                state.drive(&net, dt, None, stop);
                                body.step(dt, state.speed, &|d| state.way_point(&net, d), None, None);
                                state.commit_feedback(
                                    &net,
                                    RealizedMotion {
                                        pose: body.position,
                                        heading_deg: body.heading as f32,
                                        speed: body.realized_speed(dt),
                                        half_width: 1.25,
                                    },
                                );
                                let moved = (body.position - last).truncate().length();
                                assert!(
                                    moved < 0.6,
                                    "r {radius} short {short} dt {dt} lateral {lateral} stop {stop_at}: \
                                     jumped {moved:.2} m at tick {tick} on lane {}",
                                    state.lane
                                );
                                last = body.position;
                                if state.lane == 3 && state.s > 30.0 {
                                    break;
                                }
                            }
                            cases += 1;
                        }
                    }
                }
            }
        }
    }
    assert_eq!(cases, 216);
}

/// What the realization does in the game, in one place: the body looks along its own way for
/// scenery it would touch, the planner stops short of that, and a move into scenery is refused.
#[test]
fn a_bus_stops_short_of_a_post_its_swept_body_would_meet_instead_of_touching_it() {
    use crate::collision::Obb;
    let mut net = Network {
        lanes: vec![LaneBuilder::polyline(
            vec![DVec3::new(0.0, 0.0, 0.0), DVec3::new(0.0, 300.0, 0.0)],
            LaneKind::Street,
            3.5,
        )],
        ..Default::default()
    };
    net.link(1.5);
    let (front, rear, half) = (5.5f32, 3.5f32, 1.25f32);
    // a post at the kerb that the bus's flank just overlaps
    let post = Obb::vehicle(DVec2::new(1.5, 80.0), 0.0, 0.3, 0.3, 0.3);
    let touches = move |b: &AiBody| {
        Obb::vehicle(b.position.truncate(), b.heading, front as f64, rear as f64, half as f64)
            .overlaps_plan(&post)
    };
    let mut state = AiState::new(0, 10.0, 3);
    state.front = front;
    state.rear = rear;
    state.length = front + rear;
    state.desire = 1.0;
    let mut body = AiBody::new(&lorry(), MotionKind::Road);
    body.place(&|d| state.way_point(&net, d), None, None, 0.0);
    let (dt, margin) = (0.02f32, 0.35f32);
    let mut ahead: Option<f32> = None;
    let mut refused = 0;
    for _ in 0..3000 {
        // (realization of the last tick)
        if state.speed > 0.05 || ahead.is_some() {
            let reach = (state.speed * state.speed / 6.0 + 4.0).clamp(4.0, 16.0);
            ahead = body.distance_to_contact(&|d| state.way_point(&net, d), state.speed, reach, &touches);
        }
        let hold = ahead.map(|d| state.front + (d - margin).max(0.0));
        state.drive(&net, dt, None, hold);
        let before = body.clone();
        body.step(dt, state.speed, &|d| state.way_point(&net, d), None, None);
        if touches(&body) && !touches(&before) {
            refused += 1;
            body.reject_motion(&before);
        }
        state.commit_feedback(
            &net,
            RealizedMotion {
                pose: body.position,
                heading_deg: body.heading as f32,
                speed: body.realized_speed(dt),
                half_width: half as f64,
            },
        );
    }
    assert_eq!(refused, 0, "the body was refused a move into the post {refused} times");
    assert!(state.speed < 0.1, "still moving at {} m/s", state.speed);
    let gap = (80.0 - 0.3) - (body.position.y + front as f64);
    assert!(gap > 0.0 && gap < 1.0, "stopped {gap:.2} m short of the post");
}
