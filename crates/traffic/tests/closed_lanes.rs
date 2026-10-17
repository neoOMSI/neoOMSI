//! Regressions for #125: random traffic drove on into a lane the map closes to it (a lane
//! that goes on as a bus lane, `[rule] no_cars`) instead of merging into the open lanee
use glam::DVec3;
use traffic::*;

/// A two-lane road in three pieces (0..60, 60..100, 100..200 m); in the last piece the
/// right lane is a bus lane. Lanes: right 0, 2, 4 (4 no_cars) and left 1, 3, 5
fn bus_lane_ahead() -> Network {
    let mut net = Network::default();
    for (from, to) in [(0.0, 60.0), (60.0, 100.0), (100.0, 200.0)] {
        for x in [0.0, -3.5] {
            let mut l = LaneBuilder::polyline(
                vec![DVec3::new(x, from, 0.0), DVec3::new(x, to, 0.0)],
                LaneKind::Street,
                3.5,
            );
            l.density = 1.0;
            net.lanes.push(l);
        }
    }
    net.lanes[4].no_cars = true;
    net.link(1.5);
    for (right, left) in [(0, 1), (2, 3), (4, 5)] {
        net.lanes[right].left = Some(left);
        net.lanes[left].right = Some(right);
    }
    net
}

#[test]
fn a_car_does_not_plan_into_a_lane_closed_to_it() {
    let net = bus_lane_ahead();
    let mut car = AiState::new(2, 10.0, 7);
    car.plan_next(&net);
    assert_eq!(car.planned_next, None, "planned on into the bus lane");
    // a timetable bus (-1) still may
    let mut bus = AiState::new(2, 10.0, 7);
    bus.veh_type = -1;
    bus.plan_next(&net);
    assert_eq!(bus.planned_next, Some(4));
}

#[test]
fn a_car_with_no_open_way_on_leaves_instead_of_entering_the_bus_lane() {
    let net = bus_lane_ahead();
    let mut car = AiState::new(2, 39.9, 7);
    car.speed = 10.0;
    car.plan_next(&net);
    let mut alive = true;
    for _ in 0..10 {
        alive = car.drive(&net, 0.02, None, None);
        if !alive {
            break;
        }
    }
    assert!(!alive || car.lane != 4, "drove into the bus lane");
}

#[test]
fn a_lane_closing_ahead_requires_a_change_to_the_open_lane() {
    let net = bus_lane_ahead();
    let mut actor = ManeuverActor::new(VehicleId(1), 0, 10.0);
    actor.planned_next = Some(2);
    // 90 m to the bus lane: the change is required now
    assert_eq!(closed_ahead(&net, 0, 10.0, 0, Some(2)), Some(90.0));
    assert_eq!(required_target(&net, &actor), Some(1));
    // the left lane stays open, and a timetable bus is not closed out
    assert_eq!(closed_ahead(&net, 1, 10.0, 0, None), None);
    assert_eq!(closed_ahead(&net, 0, 10.0, -1, Some(2)), None);
    actor.veh_type = -1;
    assert_eq!(required_target(&net, &actor), None);
}

#[test]
fn a_car_waiting_for_a_gap_stops_before_the_bus_lane_not_at_the_joint() {
    let net = bus_lane_ahead();
    let mut actor = ManeuverActor::new(VehicleId(1), 0, 10.0);
    actor.planned_next = Some(2);
    let actors = [actor];
    let occupancy = Occupancy::default();
    let scene = ManeuverScene { net: &net, occupancy: &occupancy, actors: &actors,
        people: &[], static_clearance: None, time: 0.0, dt: 0.02, tick: 1 };
    // (no begin_tick: the change is not approved, so the car waits)
    let mut coord = ManeuverCoordinator::new();
    let decision = coord.plan(&scene, &mut ManeuverState::default(), &ManeuverInputs::new(0));
    assert_eq!(decision.target_lane, Some(LaneId(1)));
    let stop = decision.stop_at.expect("no wait before the bus lane");
    assert!((stop - 89.0).abs() < 0.01, "stops at {stop}");
}

#[test]
fn a_car_does_not_keep_right_into_a_lane_that_closes() {
    let net = bus_lane_ahead();
    let mut actor = ManeuverActor::new(VehicleId(1), 1, 10.0);
    actor.planned_next = Some(3);
    actor.speed = 12.0;
    let actors = [actor];
    let occupancy = Occupancy::default();
    let scene = ManeuverScene { net: &net, occupancy: &occupancy, actors: &actors,
        people: &[], static_clearance: None, time: 0.0, dt: 0.02, tick: 1 };
    let coord = ManeuverCoordinator::new();
    let intent = coord.intent(&scene, &actors[0], &ManeuverState::default());
    assert_eq!(intent.target, None, "wished into the lane that becomes a bus lane");
}

/// The same road with the bus lane closed by `[rule] trafficdensity 0` instead (#125).
fn no_random_traffic_ahead() -> Network {
    let mut net = bus_lane_ahead();
    net.lanes[4].no_cars = false;
    net.lanes[4].density = 0.0;
    net
}

#[test]
fn a_lane_without_random_traffic_ahead_requires_a_change_to_the_open_lane() {
    let net = no_random_traffic_ahead();
    let mut actor = ManeuverActor::new(VehicleId(1), 0, 10.0);
    actor.planned_next = Some(2);
    assert_eq!(closed_ahead(&net, 0, 10.0, 0, Some(2)), Some(90.0));
    assert_eq!(required_target(&net, &actor), Some(1));
    assert_eq!(closed_ahead(&net, 1, 10.0, 0, None), None);
    // the timetable's buses are what the lane is for
    assert_eq!(closed_ahead(&net, 0, 10.0, -1, Some(2)), None);
    actor.veh_type = -1;
    assert_eq!(required_target(&net, &actor), None);
}

#[test]
fn a_car_waits_for_a_gap_before_a_lane_without_random_traffic() {
    let net = no_random_traffic_ahead();
    let mut actor = ManeuverActor::new(VehicleId(1), 0, 10.0);
    actor.planned_next = Some(2);
    let actors = [actor];
    let occupancy = Occupancy::default();
    let scene = ManeuverScene { net: &net, occupancy: &occupancy, actors: &actors,
        people: &[], static_clearance: None, time: 0.0, dt: 0.02, tick: 1 };
    let mut coord = ManeuverCoordinator::new();
    let decision = coord.plan(&scene, &mut ManeuverState::default(), &ManeuverInputs::new(0));
    assert_eq!(decision.target_lane, Some(LaneId(1)));
    let stop = decision.stop_at.expect("no wait before the bus lane");
    assert!((stop - 89.0).abs() < 0.01, "stops at {stop}");
}

#[test]
fn a_car_does_not_keep_right_into_a_lane_without_random_traffic() {
    let net = no_random_traffic_ahead();
    let mut actor = ManeuverActor::new(VehicleId(1), 1, 10.0);
    actor.planned_next = Some(3);
    actor.speed = 12.0;
    let actors = [actor];
    let occupancy = Occupancy::default();
    let scene = ManeuverScene { net: &net, occupancy: &occupancy, actors: &actors,
        people: &[], static_clearance: None, time: 0.0, dt: 0.02, tick: 1 };
    let coord = ManeuverCoordinator::new();
    let intent = coord.intent(&scene, &actors[0], &ManeuverState::default());
    assert_eq!(intent.target, None, "wished into the lane that becomes a bus lane");
}

#[test]
fn a_car_already_on_a_lane_without_random_traffic_is_not_held_there() {
    let net = no_random_traffic_ahead();
    // on the bus lane itself: nothing ahead closes, it just drives on
    assert_eq!(closed_ahead(&net, 4, 10.0, 0, None), None);
    let actor = ManeuverActor::new(VehicleId(1), 4, 10.0);
    assert_eq!(required_target(&net, &actor), None);
}

#[test]
fn a_road_without_a_lane_to_move_to_is_driven_on_as_before() {
    // one lane only, the last piece without random traffic: with nowhere to go it drives on
    let mut net = Network::default();
    for (from, to) in [(0.0, 60.0), (60.0, 200.0)] {
        let mut l = LaneBuilder::polyline(
            vec![DVec3::new(0.0, from, 0.0), DVec3::new(0.0, to, 0.0)],
            LaneKind::Street,
            3.5,
        );
        l.density = 1.0;
        net.lanes.push(l);
    }
    net.lanes[1].density = 0.0;
    net.link(1.5);
    let mut actor = ManeuverActor::new(VehicleId(1), 0, 10.0);
    actor.planned_next = Some(1);
    assert_eq!(required_target(&net, &actor), None);
    let mut car = AiState::new(0, 55.0, 7);
    car.speed = 10.0;
    car.plan_next(&net);
    for _ in 0..100 {
        assert!(car.drive(&net, 0.02, None, None), "the car was taken off");
    }
    assert_eq!(car.lane, 1);
}
