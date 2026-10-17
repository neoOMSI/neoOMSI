//! Regressions for the on-ramp queue (Gerolstein's motorway, 2026-10): cars of the
//! motorway kept right onto the acceleration lane, drove it to its end and waited there to
//! merge back, and the cars from the ramp queued behind them.
use glam::DVec3;
use traffic::*;

const MBACK: usize = 0;
const LBACK: usize = 1;
const RAMP: usize = 2;
const M0: usize = 3;
const L0: usize = 4;
const ACCEL: usize = 5;
const M1: usize = 6;
const TAPER: usize = 7;
const M2: usize = 8;

fn lane(net: &mut Network, pts: &[(f64, f64)]) -> usize {
    let mut l = LaneBuilder::polyline(
        pts.iter().map(|&(x, y)| DVec3::new(x, y, 0.0)).collect(),
        LaneKind::Street,
        3.5,
    );
    l.density = 1.0;
    net.lanes.push(l);
    net.lanes.len() - 1
}

/// A two-lane motorway northwards (right lane x = 0, left lane x = -3.5) with an
/// acceleration lane (x = 3.5, y 0..100) that tapers into the right lane by y = 125. The
/// acceleration lane comes off a ramp from another road (`bay`: it forks off the right lane
/// just before instead, a bus bay running back into the road).
fn motorway(bay: bool) -> Network {
    motorway_with(bay, 3.5)
}

/// The same, the acceleration lane `side` m right of the right lane.
fn motorway_with(bay: bool, side: f64) -> Network {
    let mut net = Network::default();
    lane(&mut net, &[(0.0, -100.0), (0.0, 0.0)]);
    lane(&mut net, &[(-3.5, -100.0), (-3.5, 0.0)]);
    if bay {
        lane(&mut net, &[(0.0, 0.0), (side, 0.0)]);
    } else {
        lane(&mut net, &[(40.0, -60.0), (side, 0.0)]);
    }
    lane(&mut net, &[(0.0, 0.0), (0.0, 100.0)]);
    lane(&mut net, &[(-3.5, 0.0), (-3.5, 300.0)]);
    lane(&mut net, &[(side, 0.0), (side, 100.0)]);
    lane(&mut net, &[(0.0, 100.0), (0.0, 125.0)]);
    lane(&mut net, &[(side, 100.0), (0.0, 125.0)]);
    lane(&mut net, &[(0.0, 125.0), (0.0, 300.0)]);
    // (the bay forks off where the lane before ends)
    net.link(1.0);
    for (right, left) in [(ACCEL, M0), (M0, L0), (MBACK, LBACK)] {
        net.lanes[right].left = Some(left);
        net.lanes[left].right = Some(right);
    }
    net
}

#[test]
fn the_acceleration_lane_ends_into_the_motorway_and_the_motorway_does_not() {
    let net = motorway(false);
    assert_eq!(net.lanes[MBACK].next, vec![M0]);
    assert_eq!(net.prev[M2].len(), 2, "the taper and the right lane meet");
    let (d, beside) = net.merges_into(ACCEL, 10.0, 400.0).expect("no end seen");
    assert_eq!(beside, M0);
    // (for the cars on it it ends where the taper comes within 3.2 m of the right lane,
    // 2.15 m into it, found to the metre; the taper meets that lane 25 m later)
    assert!((d - 93.0).abs() < 0.01, "ends {d} m ahead");
    assert_eq!(net.merges_into(M0, 10.0, 400.0), None);
    assert_eq!(net.merges_into(L0, 10.0, 400.0), None);
    // out of sight, it does not end yet
    assert_eq!(net.merges_into(ACCEL, 10.0, 100.0), None);
}

#[test]
fn an_acceleration_lane_drawn_close_to_the_road_ends_in_its_taper() {
    // 3 m from the right lane, closer than `MERGE_CLEAR` all along: it does not end right
    // where the car is, so that it never moved over early, but where its taper comes in
    let net = motorway_with(false, 3.0);
    let (d, beside) = net.merges_into(ACCEL, 10.0, 400.0).expect("no end seen");
    assert_eq!(beside, M0);
    assert!(d > 90.0 && d < 100.0, "ends {d} m ahead");
}

#[test]
fn a_relinked_network_works_out_again_where_its_lanes_end() {
    let mut net = motorway(false);
    assert!(net.merges_into(ACCEL, 10.0, 400.0).is_some());
    // the acceleration lane goes on as a lane of its own now
    net.lanes[TAPER] = LaneBuilder::polyline(
        vec![DVec3::new(3.5, 100.0, 0.0), DVec3::new(3.5, 300.0, 0.0)],
        LaneKind::Street,
        3.5,
    );
    net.link(1.0);
    for (right, left) in [(ACCEL, M0), (M0, L0), (MBACK, LBACK)] {
        net.lanes[right].left = Some(left);
        net.lanes[left].right = Some(right);
    }
    assert_eq!(net.merges_into(ACCEL, 10.0, 400.0), None);
}

#[test]
fn the_taper_gives_way_to_the_motorway() {
    let net = motorway(false);
    assert!(net.must_yield(TAPER, M1));
    assert!(!net.must_yield(M1, TAPER));
}

#[test]
fn a_bus_pulling_out_of_its_bay_keeps_its_way() {
    // the same lanes, but the "acceleration lane" left the road just before: a bus bay
    let net = motorway(true);
    assert!(net.lanes[MBACK].next.contains(&RAMP));
    assert!(!net.must_yield(TAPER, M1));
    assert!(!net.must_yield(M1, TAPER));
}

#[test]
fn a_car_on_the_acceleration_lane_must_move_over() {
    let net = motorway(false);
    let mut actor = ManeuverActor::new(VehicleId(1), ACCEL, 10.0);
    actor.planned_next = Some(TAPER);
    assert_eq!(required_target(&net, &actor), Some(M0));
    // a timetable bus keeps to its route
    actor.veh_type = -1;
    assert_eq!(required_target(&net, &actor), None);
    // the motorway's own lanes need not
    let actor = ManeuverActor::new(VehicleId(2), M0, 10.0);
    assert_eq!(required_target(&net, &actor), None);
}

#[test]
fn a_car_waiting_for_a_gap_waits_at_the_end_of_the_acceleration_lane() {
    let net = motorway(false);
    let mut actor = ManeuverActor::new(VehicleId(1), ACCEL, 10.0);
    actor.planned_next = Some(TAPER);
    let actors = [actor];
    let occupancy = Occupancy::default();
    let scene = ManeuverScene {
        net: &net,
        occupancy: &occupancy,
        actors: &actors,
        people: &[],
        static_clearance: None,
        time: 0.0,
        dt: 0.02,
        tick: 1,
    };
    // (no begin_tick: the change is not approved, so the car waits)
    let mut coord = ManeuverCoordinator::new();
    let decision = coord.plan(
        &scene,
        &mut ManeuverState::default(),
        &ManeuverInputs::new(0),
    );
    assert_eq!(decision.target_lane, Some(LaneId(M0)));
    let stop = decision.stop_at.expect("no wait before the end");
    // it waits at the end of the lane beside the road, its front (2.25 m ahead of its
    // origin) not yet in the taper where it would stand in the way of the cars going past
    // (waiting at the merge, 114 m ahead, the cars on the road slowed for it)
    assert!((stop - 89.75).abs() < 0.01, "stops at {stop}");
    let front = net.lanes[TAPER].at(10.0 + stop + 2.25 - 100.0).0;
    let beside = [M1, M2]
        .map(|l| net.lanes[l].nearest_point(front).unwrap().1)
        .into_iter()
        .fold(f64::MAX, f64::min);
    assert!(
        beside >= MERGE_CLEAR as f64,
        "its front {beside} m off the right lane"
    );
}

#[test]
fn a_car_already_in_the_taper_is_not_held_there() {
    // it came too far to wait beside the road, its front (4 m ahead) in the taper already:
    // it is not stopped where it stands, in the way of the cars going past, but merges
    // where the two ways meet (giving way there)
    let net = motorway(false);
    let mut actor = ManeuverActor::new(VehicleId(1), ACCEL, 100.0);
    actor.front = 4.0;
    actor.planned_next = Some(TAPER);
    let actors = [actor];
    let occupancy = Occupancy::default();
    let scene = ManeuverScene {
        net: &net,
        occupancy: &occupancy,
        actors: &actors,
        people: &[],
        static_clearance: None,
        time: 0.0,
        dt: 0.02,
        tick: 1,
    };
    let mut coord = ManeuverCoordinator::new();
    let decision = coord.plan(
        &scene,
        &mut ManeuverState::default(),
        &ManeuverInputs::new(0),
    );
    assert!(
        decision.stop_at.is_none_or(|s| s > 15.0),
        "held at {:?}",
        decision.stop_at
    );
}

fn keep_right_intent(net: &Network) -> Option<LaneId> {
    let mut actor = ManeuverActor::new(VehicleId(1), M0, 10.0);
    actor.planned_next = Some(M1);
    actor.speed = 12.0;
    let actors = [actor];
    let occupancy = Occupancy::default();
    let scene = ManeuverScene {
        net,
        occupancy: &occupancy,
        actors: &actors,
        people: &[],
        static_clearance: None,
        time: 0.0,
        dt: 0.02,
        tick: 1,
    };
    let coord = ManeuverCoordinator::new();
    coord
        .intent(&scene, &actors[0], &ManeuverState::default())
        .target
}

#[test]
fn a_car_on_the_motorway_does_not_keep_right_onto_the_acceleration_lane() {
    let net = motorway(false);
    assert_eq!(
        keep_right_intent(&net),
        None,
        "kept right onto the acceleration lane"
    );
    // where that lane goes on as a lane of its own, it does keep right
    let mut through = motorway(false);
    through.lanes[TAPER] = LaneBuilder::polyline(
        vec![DVec3::new(3.5, 100.0, 0.0), DVec3::new(3.5, 300.0, 0.0)],
        LaneKind::Street,
        3.5,
    );
    through.lanes[TAPER].density = 1.0;
    through.link(1.0);
    for (right, left) in [(ACCEL, M0), (M0, L0), (MBACK, LBACK)] {
        through.lanes[right].left = Some(left);
        through.lanes[left].right = Some(right);
    }
    assert_eq!(keep_right_intent(&through), Some(LaneId(ACCEL)));
}
