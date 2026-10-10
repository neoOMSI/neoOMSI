//! Regressions for docking deadlocks and premature neighbouring-lane occupancy.
mod common;
use common::maneuver::two_lanes;
use common::service::{Bus, ServiceWorld, berth};
use glam::{DVec2, DVec3};
use traffic::*;

#[test]
fn a_route_change_near_a_shared_continuation_sweeps_the_actual_lane_blend() {
    let mut net = Network::default();
    for (x, from, to) in [(0.0, 0.0, 40.0), (-3.5, 0.0, 40.0), (-3.5, 40.0, 100.0)] {
        net.lanes.push(LaneBuilder::polyline(vec![DVec3::new(x, from, 0.0),
            DVec3::new(x, to, 0.0)], LaneKind::Street, 3.5));
    }
    net.link(4.0);
    net.lanes[0].left = Some(1);
    net.lanes[1].right = Some(0);
    net.lanes[0].next = vec![2];
    net.lanes[1].next = vec![2];
    let mut actor = ManeuverActor::new(VehicleId(93), 0, 32.8);
    actor.route_next = Some(1);
    actor.planned_next = Some(2);
    let actors = [actor];
    let occupancy = Occupancy::default();
    // A wall beyond the left edge of the target road. The old constant-offset
    // sweep shifted the already-shared continuation by another whole lane.
    let clearance = |samples: &[SweepSample], _: &ManeuverActor|
        samples.iter().all(|sample| sample.p.x >= -4.0);
    let scene = ManeuverScene { net: &net, occupancy: &occupancy, actors: &actors,
        people: &[], static_clearance: Some(&clearance), time: 0.0, dt: 0.02, tick: 1 };
    let mut coord = ManeuverCoordinator::new();
    let mut memory = ManeuverState::default();
    coord.begin_tick(&[coord.intent(&scene, &actors[0], &memory)], 1);
    let decision = coord.plan(&scene, &mut memory, &ManeuverInputs::new(0));
    assert_eq!(decision.change.expect("phantom scenery blocked the route change").to, 1);
}

#[test]
fn a_stopped_bus_corner_swerve_keeps_its_committed_anchor() {
    let net = two_lanes();
    let mut actor = ManeuverActor::new(VehicleId(93), 0, 60.0);
    actor.lateral = 0.57;
    actor.stopped = 5.0;
    actor.odometer = 60.0;
    let occupancy = Occupancy::default();
    let mut coordinator = ManeuverCoordinator::new();
    let mut memory = ManeuverState::default();
    let mut input = ManeuverInputs::new(0);
    input.kerb_swerve = Some(-0.39);
    let mut committed = None;
    for tick in 0..20 {
        let actors = [actor.clone()];
        let scene = ManeuverScene { net: &net, occupancy: &occupancy, actors: &actors,
            people: &[], static_clearance: None, time: tick as f32 * 0.02, dt: 0.02, tick };
        let decision = coordinator.plan(&scene, &mut memory, &input);
        let ramp = decision.lateral_ramp.expect("stopped bus needs a corner-recovery ramp");
        if let Some(initial) = committed { assert_eq!(ramp, initial); }
        else { committed = Some(ramp); }
        actor.odometer += 0.01;
        actor.lateral -= 0.01;
        input.kerb_swerve = Some(-0.38);
    }
    assert_eq!(committed.unwrap(), (0.57, -0.39, 60.0, 2.0));
}

#[test]
fn unequal_neighbour_arc_lengths_do_not_finish_a_bypass_at_the_joint() {
    let mut net = Network::default();
    for x in [0.0, -3.0] {
        let points = if x == 0.0 {
            vec![DVec3::ZERO, DVec3::new(x, 30.0, 0.0)]
        } else {
            vec![DVec3::new(x, 0.0, 0.0), DVec3::new(x - 1.0, 15.0, 0.0), DVec3::new(x, 30.0, 0.0)]
        };
        net.lanes.push(LaneBuilder::polyline(points, LaneKind::Street, 3.0));
    }
    for x in [0.0, -3.0] {
        net.lanes.push(LaneBuilder::polyline(
            vec![DVec3::new(x, 30.0, 0.0), DVec3::new(x, 100.0, 0.0)], LaneKind::Street, 3.0));
    }
    net.link(1.5);
    for (right, left) in [(0, 1), (2, 3)] {
        net.lanes[right].left = Some(left);
        net.lanes[left].right = Some(right);
    }
    let mut car = AiState::new(0, 29.99, 167);
    car.plan_next(&net);
    car.speed = 1.0;
    car.accel_cap = Some(0.0);
    car.start_bypass(&net, 1, 1);
    car.change.as_mut().unwrap().wait = 0.0;
    car.drive(&net, 0.01001, None, None);
    assert_eq!(car.lane, 2);
    let change = car.change.expect("spline boundary snapped the change complete");
    assert_eq!(change.to, 3);
    assert!(change.t < 0.01);
}

#[test]
fn angled_bus_corners_do_not_extend_the_clearance_box_past_the_body() {
    let h = 30.0f64.to_radians();
    let body = BodyFootprint::new(
        VehicleId(9),
        DVec2::ZERO,
        DVec2::new(h.sin(), h.cos()),
        6.0,
        1.25,
        0.0,
        3.0,
        0.0,
    );
    let occupancy = Occupancy::build(NetworkVersion(1), 0, vec![body]);
    // The front cross-section of a passing car is wholly beyond the angled bus's
    // rear corner. Growing the bus on only its two local axes creates a phantom hit.
    let sample = SweepSample {
        p: DVec3::new(-2.0, -6.0, 0.0),
        d: 2.0,
        dir: DVec2::Y,
    };
    assert!(occupancy.swept_clearance(&[sample], 0.9, &[]).is_none());
    let touching = SweepSample {
        p: DVec3::new(-2.0, -4.5, 0.0),
        ..sample
    };
    assert!(occupancy.swept_clearance(&[touching], 0.9, &[]).is_some());
}

#[test]
fn corridor_samples_match_physical_cross_sections_at_different_bus_angles() {
    for degrees in [0.0f64, 7.0, 30.0, 75.0, 140.0, 225.0] {
        let h = degrees.to_radians();
        let bus = BodyFootprint::new(
            VehicleId(9),
            DVec2::new(0.25, 0.5),
            DVec2::new(h.sin(), h.cos()),
            6.0,
            1.25,
            0.0,
            3.0,
            0.0,
        );
        let mut contact_body = bus;
        contact_body.half_len -= 0.1;
        contact_body.half_w -= 0.1;
        let occupancy = Occupancy::build(NetworkVersion(1), 0, vec![bus]);
        for width in [0.6, 0.9, 1.25] {
            for x in -16..=16 {
                for y in -16..=16 {
                    let p = DVec3::new(x as f64 * 0.5 + 0.137, y as f64 * 0.5 + 0.219, 0.0);
                    let sample = SweepSample {
                        p,
                        d: 0.0,
                        dir: DVec2::Y,
                    };
                    let cross_section = BodyFootprint::new(
                        VehicleId(1),
                        p.truncate(),
                        DVec2::Y,
                        0.0,
                        width,
                        0.0,
                        2.0,
                        0.0,
                    );
                    assert_eq!(
                        occupancy.swept_clearance(&[sample], width, &[]).is_some(),
                        contact_body.overlaps(&cross_section, 0.0),
                        "bus {degrees} degrees, car half-width {width}, probe {p:?}"
                    );
                }
            }
        }
    }
}

#[test]
fn scheduled_feedback_does_not_switch_the_source_mid_lane_change() {
    let net = two_lanes();
    let mut car = AiState::new(0, 60.0, 1);
    car.set_route(&net, vec![0, 1], 60.0);
    car.speed = 8.0;
    car.start_route_change(&net, 1, 1);
    car.change.as_mut().unwrap().t = 0.7;
    let before = car.way_point(&net, 0.0);
    let fix = car
        .commit_feedback(
            &net,
            RealizedMotion {
                pose: before,
                heading_deg: 0.0,
                speed: 8.0,
                half_width: 0.9,
            },
        )
        .unwrap();
    assert_eq!(fix.lane, LaneId(0));
    assert_eq!(car.route_index, 0);
    assert!((car.way_point(&net, 0.0) - before).length() < 0.01);
    assert!((car.change.unwrap().s_to - car.s).abs() < 0.01);
    // A long wheelbase may still lag the target after command completion. Do not
    // restart the route change by projecting it back onto the adjacent source.
    car.change = None;
    car.lane = 1;
    car.route_index = 1;
    assert!(
        car.commit_feedback(
            &net,
            RealizedMotion {
                pose: DVec3::new(-1.5, 60.0, 0.0),
                heading_deg: 0.0,
                speed: 8.0,
                half_width: 0.9,
            }
        )
        .is_none()
    );
    assert_eq!(car.lane, 1);
    assert_eq!(car.route_index, 1);
}

#[test]
fn a_reserved_target_retains_the_bodys_actual_lateral_position() {
    let net = two_lanes();
    let mut bus = BodyFootprint::new(
        VehicleId(9),
        DVec2::new(0.0, 62.0),
        DVec2::Y,
        6.0,
        1.25,
        0.0,
        3.0,
        0.0,
    );
    bus.front = 8.0;
    bus.rear = 4.0;
    bus.current = Some(bus.placement_at(&net, LaneId(0), 60.0));
    bus.crossing = Some(bus.placement_at(&net, LaneId(1), 60.0));
    assert!(bus.current.unwrap().lateral.abs() < 0.01);
    assert!((bus.crossing.unwrap().lateral - 3.5).abs() < 0.01);
    let trailer = bus.part_of(1, DVec2::new(0.0, 53.0), DVec2::Y, 4.0, 1.25);
    let occ = Occupancy::build(net.version(), 0, vec![bus, trailer]);
    let free_lane: Vec<_> = (40..90)
        .map(|s| SweepSample {
            p: DVec3::new(-3.5, s as f64, 0.0),
            d: s as f32,
            dir: DVec2::Y,
        })
        .collect();
    assert!(occ.swept_clearance(&free_lane, 0.9, &[]).is_none());
    let own_way = [SweepSample {
        p: DVec3::new(0.0, 53.0, 0.0),
        d: 0.0,
        dir: DVec2::Y,
    }];
    assert!(
        occ.swept_clearance(&own_way, 1.25, &[VehicleId(9)])
            .is_none()
    );
}

#[test]
fn unreachable_docking_poses_release_the_berth_and_allow_the_bus_to_continue() {
    // Past the boarding tolerance in the bay, and at the berth point but out of the bay.
    for (s, lateral) in [(123.0, 1.6), (120.0, 0.0)] {
        let world = ServiceWorld::new();
        let stop = berth(7001, 120.0, 36000.0);
        let mut actor = ServiceActor::new(VehicleId(1), 0, s);
        actor.lateral = lateral;
        let actors = [actor];
        let occupancy = Occupancy::default();
        let scene = ServiceScene {
            net: &world.net,
            occupancy: &occupancy,
            actors: &actors,
            day_time: 36000.0,
            dt: 0.02,
            tick: 0,
        };
        let input = ServiceInputs {
            actor: 0,
            berth: Some(stop),
            distance: 120.0 - s,
            policy: StopPolicy::default(),
            demand: StopDemand::default(),
            feedback: ScriptFeedback::Idle,
            passing: false,
            kerb_swerve: None,
            junction_first: false,
            berth_held_long: false,
        };
        let mut state = ServiceState::new();
        state.phase = ServicePhase::Docking;
        state.berth = Some(stop);
        let mut coordinator = ServiceCoordinator::new();
        let mut recovered = false;
        for _ in 0..500 {
            let decision = coordinator.plan(&scene, &mut state, &input);
            assert!(
                !state.at_station(),
                "unreachable docking must not open doors"
            );
            if decision.consume_stop {
                recovered = true;
                assert!(decision.stop_at.is_none());
                assert_eq!(state.fault, Some(Reason::MissedStop));
                break;
            }
        }
        assert!(recovered, "bus remained stuck at s={s}, lateral={lateral}");
        assert_eq!(coordinator.berth_owner(StopId(7001), 0), None);
    }
}

#[test]
fn waiting_upstream_does_not_skip_a_legitimate_queued_stop() {
    let mut world = ServiceWorld::new();
    let stop = berth(7001, 120.0, 36000.0);
    let mut bus = Bus::new(1, 100.0, vec![stop]);
    bus.state.phase = ServicePhase::Docking;
    bus.state.berth = Some(stop);
    world.add(bus);
    world.cruise = 0.0;
    world.run(1000);
    assert_eq!(world.bus(1).state.phase, ServicePhase::Docking);
    assert_eq!(world.bus(1).stops.len(), 1);
}

#[test]
fn a_bus_aligned_with_the_lane_can_depart_despite_a_moving_follower() {
    let mut world = ServiceWorld::new();
    let mut stop = berth(7001, 120.0, 36000.0);
    stop.bay = 0.0;
    let mut bus = Bus::new(1, 120.0, vec![stop]);
    bus.state.phase = ServicePhase::WaitingToMerge;
    bus.state.berth = Some(stop);
    let mut follower = Bus::new(2, 110.0, vec![]);
    follower.speed = 3.0;
    world.add(bus);
    world.add(follower);
    world.step();
    assert_eq!(world.bus(1).state.phase, ServicePhase::Departing);
}

#[test]
fn emergency_speed_bonus_is_active_only_during_response_and_respects_the_vehicle_cap() {
    let mut net = two_lanes();
    for lane in &mut net.lanes {
        lane.speed_limit_kmh = 50.0;
    }
    let mut car = AiState::new(0, 0.0, 1);
    car.max_speed_kmh = 100.0;
    car.speed = 50.0 / 3.6;
    car.emergency_drive = true;
    assert!(car.desired_demand(&net, None, None).effective() > 0.5);
    car.speed = 65.0 / 3.6;
    assert!(car.desired_demand(&net, None, None).effective().abs() < 0.01);
    car.emergency_drive = false;
    assert!(car.desired_demand(&net, None, None).effective() < -1.0);
    car.emergency_drive = true;
    car.max_speed_kmh = 40.0;
    assert!(car.desired_demand(&net, None, None).effective() < -1.0);
    assert!(
        car.desired_demand(
            &net,
            Some(Lead {
                gap: 1.0,
                speed: 0.0,
                acc: 0.0
            }),
            None
        )
        .is_emergency()
    );
}

#[test]
fn both_lanes_yield_and_the_response_uses_the_gap_between_them() {
    let net = two_lanes();
    let drive = EmergencyDrive {
        vehicle: VehicleId(9),
        way: vec![(0, -50.0)],
        front: 2.0,
        speed: 8.0,
    };
    for lane in [0, 1] {
        let state = AiState::new(lane, 70.0, 1);
        assert!(
            approaching_emergency(
                &net,
                VehicleId(lane as u64 + 1),
                &state,
                2.0,
                &[drive.clone()]
            )
            .is_some()
        );
    }
    let mut responder = ManeuverActor::new(VehicleId(9), 0, 50.0);
    responder.speed = 8.0;
    responder.half_width = 0.85;
    let mut coordinator = ManeuverCoordinator::new();
    let mut memory = ManeuverState::default();
    let mut input = ManeuverInputs::new(0);
    input.priority_pass = true;
    for aside in [false, true] {
        let mut feet = Vec::new();
        for lane in [0, 1] {
            for (k, s) in [70.0, 82.0, 94.0].into_iter().enumerate() {
                let x = if lane == 0 {
                    if aside { 0.5 } else { 0.0 }
                } else {
                    -3.5 - if aside { 0.5 } else { 0.0 }
                };
                feet.push(BodyFootprint::new(
                    VehicleId(10 + lane * 3 + k as u64),
                    DVec2::new(x, s),
                    DVec2::Y,
                    2.2,
                    0.85,
                    0.0,
                    2.0,
                    0.0,
                ));
            }
        }
        let occupancy = Occupancy::build(net.version(), 0, feet);
        let scene = ManeuverScene {
            net: &net,
            occupancy: &occupancy,
            actors: &[responder.clone()],
            people: &[],
            static_clearance: None,
            time: 0.0,
            dt: 0.02,
            tick: 0,
        };
        let decision = coordinator.plan(&scene, &mut memory, &input);
        if aside {
            assert_eq!(decision.lateral_target, Some(-1.75));
            assert!(decision.stop_at.is_none());
        } else {
            assert_ne!(
                decision.lateral_target,
                Some(-1.75),
                "must not drive through the queue before it yields"
            );
        }
    }
}

#[test]
fn ordinary_traffic_keeps_to_the_driving_side_when_that_lane_is_free() {
    for left_hand in [false, true] {
        let mut net = two_lanes();
        net.left_hand = left_hand;
        let lane = if left_hand { 0 } else { 1 };
        let mut actor = ManeuverActor::new(VehicleId(1), lane, 60.0);
        actor.speed = 8.0;
        let actors = [actor];
        let occupancy = Occupancy::default();
        let mut coordinator = ManeuverCoordinator::new();
        let mut memory = ManeuverState::default();
        let mut chosen = None;
        for tick in 0..150 {
            let scene = ManeuverScene {
                net: &net,
                occupancy: &occupancy,
                actors: &actors,
                people: &[],
                static_clearance: None,
                time: tick as f32 * 0.02,
                dt: 0.02,
                tick,
            };
            let intent = coordinator.intent(&scene, &actors[0], &memory);
            coordinator.begin_tick(&[intent], tick);
            if let Some(change) = coordinator
                .plan(&scene, &mut memory, &ManeuverInputs::new(0))
                .change
            {
                chosen = Some(change.to);
                break;
            }
        }
        assert_eq!(chosen, Some(1 - lane));
    }
}

#[test]
fn a_bus_resting_close_to_its_berth_serves_the_stop_instead_of_missing_it() {
    // Released from the queue a few metres short, it could not swing fully into the bay
    // (Berlin 156: lateral 1.19 against a 1.6 bay). People still reach its doors.
    for (s, lateral) in [(121.0, 1.6), (118.5, 1.6), (120.0, 1.0)] {
        let world = ServiceWorld::new();
        let stop = berth(7001, 120.0, 36000.0);
        let mut actor = ServiceActor::new(VehicleId(1), 0, s);
        actor.lateral = lateral;
        let actors = [actor];
        let occupancy = Occupancy::default();
        let scene = ServiceScene {
            net: &world.net,
            occupancy: &occupancy,
            actors: &actors,
            day_time: 36000.0,
            dt: 0.02,
            tick: 0,
        };
        let input = ServiceInputs {
            actor: 0,
            berth: Some(stop),
            distance: 120.0 - s,
            policy: StopPolicy::default(),
            demand: StopDemand::default(),
            feedback: ScriptFeedback::Idle,
            passing: false,
            kerb_swerve: None,
            junction_first: false,
            berth_held_long: false,
        };
        let mut state = ServiceState::new();
        state.phase = ServicePhase::Docking;
        state.berth = Some(stop);
        let mut coordinator = ServiceCoordinator::new();
        for _ in 0..500 {
            let decision = coordinator.plan(&scene, &mut state, &input);
            assert!(!decision.consume_stop, "a serviceable pose was recorded as a missed stop");
            if state.at_station() {
                break;
            }
        }
        assert!(state.at_station(), "no boarding at s={s}, lateral={lateral}");
        assert_eq!(state.fault, None);
    }
}
