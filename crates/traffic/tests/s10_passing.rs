//! Passing a stopped bus remains available, but scenery, islands and passengers veto it.
mod common;
use common::maneuver::two_way;
use glam::{DVec2, DVec3};
use traffic::perception::SweepSample;
use traffic::*;

fn plan(
    net: &Network,
    people: &[DVec2],
    clear: &dyn Fn(&[SweepSample], &ManeuverActor) -> bool,
    queue: bool,
) -> ManeuverDecision {
    let mut actor = ManeuverActor::new(VehicleId(1), 0, 60.0);
    actor.speed = 6.0;
    actor.stopped = 5.0;
    let actors = [actor];
    let occupancy = Occupancy::build(net.version(), 1, vec![]);
    let scene = ManeuverScene {
        static_clearance: Some(clear),
        net,
        occupancy: &occupancy,
        actors: &actors,
        people,
        time: 1.0,
        dt: 0.02,
        tick: 1,
    };
    let mut inputs = ManeuverInputs::new(0);
    inputs.lead_standing = true;
    inputs.lead_gap = Some(6.0);
    inputs.obstacle_len = 12.0;
    inputs.queue_for_stop = queue;
    ManeuverCoordinator::new().plan(&scene, &mut ManeuverState::default(), &inputs)
}

#[test]
fn a_car_still_passes_a_bus_when_the_entire_way_is_clear() {
    assert_eq!(
        plan(&two_way(), &[], &|_, _| true, false).phase,
        ManeuverPhase::Passing
    );
}
#[test]
fn scenery_vetoes_the_pass_even_after_a_long_wait() {
    assert_ne!(
        plan(&two_way(), &[], &|_, _| false, false).phase,
        ManeuverPhase::Passing
    );
}
#[test]
fn a_divided_carriageway_is_not_an_oncoming_passing_lane_issue_126() {
    let mut net = two_way();
    for p in &mut net.lanes[1].points {
        p.x -= 1.5;
    }
    net.lanes[1].refresh();
    net.link(1.5);
    assert_ne!(
        plan(&net, &[], &|_, _| true, false).phase,
        ManeuverPhase::Passing
    );
}
/// `two_way` with the oncoming lane bent 2 m out round an island between `from` and `to` (y).
fn island(from: f64, to: f64) -> Network {
    let mut net = two_way();
    net.lanes[1] = traffic::network::LaneBuilder::polyline(
        vec![
            DVec3::new(-3.5, 400.0, 0.0),
            DVec3::new(-3.5, to + 5.0, 0.0),
            DVec3::new(-5.5, to, 0.0),
            DVec3::new(-5.5, from, 0.0),
            DVec3::new(-3.5, from - 5.0, 0.0),
            DVec3::new(-3.5, 0.0, 0.0),
        ],
        LaneKind::Street,
        3.0,
    );
    net.link(1.5);
    net
}

#[test]
fn an_island_that_starts_after_the_pull_out_vetoes_the_pass() {
    // the car pulls out at y 60 where the lanes lie side by side; the island begins at 75
    assert_ne!(
        plan(&island(75.0, 100.0), &[], &|_, _| true, false).phase,
        ManeuverPhase::Passing
    );
    // an island well beyond the pass does not
    assert_eq!(
        plan(&island(200.0, 230.0), &[], &|_, _| true, false).phase,
        ManeuverPhase::Passing
    );
}

#[test]
fn narrow_authored_paths_on_an_ordinary_street_still_allow_a_pass() {
    // BRT Berlin, Teltowkanal: 2 m paths whose centres lie 3.2 m apart, no median
    let mut net = two_way();
    net.lanes[1] = traffic::network::LaneBuilder::polyline(
        vec![DVec3::new(-3.2, 400.0, 0.0), DVec3::new(-3.2, 0.0, 0.0)],
        LaneKind::Street,
        2.0,
    );
    net.lanes[0] = traffic::network::LaneBuilder::polyline(
        vec![DVec3::new(0.0, 0.0, 0.0), DVec3::new(0.0, 400.0, 0.0)],
        LaneKind::Street,
        2.0,
    );
    net.link(1.5);
    assert_eq!(
        plan(&net, &[], &|_, _| true, false).phase,
        ManeuverPhase::Passing
    );
}

#[test]
fn passengers_crossing_the_passing_path_are_protected() {
    let people = [DVec2::new(-3.5, 80.0)];
    assert_ne!(
        plan(&two_way(), &people, &|_, _| true, false).phase,
        ManeuverPhase::Passing
    );
}
#[test]
fn another_bus_waiting_for_its_own_berth_does_not_pass_the_queue() {
    assert_ne!(
        plan(&two_way(), &[], &|_, _| true, true).phase,
        ManeuverPhase::Passing
    );
}

#[test]
fn a_standing_car_can_pass_a_real_bus_and_return_to_its_lane() {
    let net = two_way();
    let mut car = ManeuverActor::new(VehicleId(1), 0, 60.0);
    car.front = 2.1;
    car.rear = 2.1;
    car.length = 4.2;
    car.half_width = 0.85;
    car.stopped = 5.0;
    car.pass_room = 5.25;
    let mut bus = ManeuverActor::new(VehicleId(2), 0, 71.35);
    bus.front = 6.0;
    bus.rear = 4.0;
    bus.length = 10.0;
    bus.at_stop = true;
    let mut foot = BodyFootprint::new(bus.id, DVec2::new(0.0, 72.35), DVec2::Y,
        5.0, 1.25, 0.0, 3.5, 0.0);
    foot.front = bus.front;
    foot.rear = bus.rear;
    foot.current = Some(Placement { lane: LaneId(0), s: bus.s, lateral: 0.0, foreign: false });
    let occupancy = Occupancy::build(net.version(), 0, vec![foot]);
    let actors = [car, bus];
    let scene = ManeuverScene { net: &net, occupancy: &occupancy, actors: &actors,
        people: &[], static_clearance: None, time: 5.0, dt: 0.02, tick: 1 };
    let mut input = ManeuverInputs::new(0);
    input.lead_gap = Some(5.25);
    input.lead_standing = true;
    input.obstacle_len = 10.0;
    let decision = ManeuverCoordinator::new().plan(&scene, &mut ManeuverState::default(), &input);
    assert_eq!(decision.phase, ManeuverPhase::Passing);
}

#[test]
fn an_ambulance_kept_back_behind_a_car_making_way_passes_it_soon() {
    // a car has pulled over and stopped for the ambulance on a road with one lane each way
    let net = two_way();
    let mut amb = ManeuverActor::new(VehicleId(1), 0, 60.0);
    amb.front = 3.5;
    amb.rear = 3.5;
    amb.length = 7.0;
    amb.half_width = 1.25;
    amb.stopped = 1.5;
    amb.pass_room = 6.0;
    let mut car = ManeuverActor::new(VehicleId(2), 0, 72.0);
    car.front = 2.1;
    car.rear = 2.1;
    car.length = 4.2;
    let mut foot = BodyFootprint::new(car.id, DVec2::new(0.35, 72.0), DVec2::Y,
        2.1, 0.9, 0.0, 1.5, 0.0);
    foot.front = car.front;
    foot.rear = car.rear;
    foot.current = Some(Placement { lane: LaneId(0), s: car.s, lateral: 0.35, foreign: false });
    let occupancy = Occupancy::build(net.version(), 0, vec![foot]);
    let actors = [amb, car];
    let scene = ManeuverScene { net: &net, occupancy: &occupancy, actors: &actors,
        people: &[], static_clearance: None, time: 5.0, dt: 0.02, tick: 1 };
    let mut input = ManeuverInputs::new(0);
    input.lead_gap = Some(72.0 - 2.1 - 60.0 - 3.5);
    input.lead_standing = true;
    input.priority_pass = true;
    input.obstacle_len = 4.2;
    let d = ManeuverCoordinator::new().plan(&scene, &mut ManeuverState::default(), &input);
    assert_eq!(d.phase, ManeuverPhase::Passing, "{d:?}");
}

#[test]
fn an_ambulance_passes_a_car_still_slowing_down_but_waits_for_oncoming_traffic() {
    let mut net = two_way();
    net.lanes[0].speed_limit_kmh = 50.0;
    let plan_with = |oncoming: bool| {
        let mut amb = ManeuverActor::new(VehicleId(1), 0, 60.0);
        amb.front = 3.5;
        amb.rear = 3.5;
        amb.length = 7.0;
        amb.half_width = 1.25;
        amb.speed = 5.0;
        amb.pass_room = 6.0;
        let mut feet = Vec::new();
        let mut car_foot = BodyFootprint::new(VehicleId(2), DVec2::new(0.35, 76.0), DVec2::Y,
            2.1, 0.9, 0.0, 1.5, 3.0);
        car_foot.front = 2.1;
        car_foot.rear = 2.1;
        car_foot.current = Some(Placement { lane: LaneId(0), s: 76.0, lateral: 0.35, foreign: false });
        feet.push(car_foot);
        if oncoming {
            // a car coming the other way, 60 m off on the oncoming lane
            let mut f = BodyFootprint::new(VehicleId(3), DVec2::new(-3.5, 140.0), -DVec2::Y,
                2.1, 0.9, 0.0, 1.5, 13.0);
            f.front = 2.1;
            f.rear = 2.1;
            f.current = Some(Placement { lane: LaneId(1), s: 400.0 - 140.0, lateral: 0.0, foreign: false });
            feet.push(f);
        }
        let occupancy = Occupancy::build(net.version(), 0, feet);
        let actors = [amb];
        let scene = ManeuverScene { net: &net, occupancy: &occupancy, actors: &actors,
            people: &[], static_clearance: None, time: 5.0, dt: 0.02, tick: 1 };
        let mut input = ManeuverInputs::new(0);
        input.lead_gap = Some(76.0 - 2.1 - 60.0 - 3.5);
        input.lead_speed = 3.0;
        input.lead_standing = true;
        input.priority_pass = true;
        input.obstacle_len = 4.2;
        ManeuverCoordinator::new().plan(&scene, &mut ManeuverState::default(), &input).phase
    };
    assert_eq!(plan_with(false), ManeuverPhase::Passing, "did not pass on a free oncoming lane");
    assert_ne!(plan_with(true), ManeuverPhase::Passing, "pulled out in front of oncoming traffic");
}
