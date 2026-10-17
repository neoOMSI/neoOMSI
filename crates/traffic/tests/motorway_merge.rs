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
    let mut net = Network::default();
    lane(&mut net, &[(0.0, -100.0), (0.0, 0.0)]);
    lane(&mut net, &[(-3.5, -100.0), (-3.5, 0.0)]);
    if bay {
        lane(&mut net, &[(0.0, 0.0), (3.5, 0.0)]);
    } else {
        lane(&mut net, &[(40.0, -60.0), (3.5, 0.0)]);
    }
    lane(&mut net, &[(0.0, 0.0), (0.0, 100.0)]);
    lane(&mut net, &[(-3.5, 0.0), (-3.5, 300.0)]);
    lane(&mut net, &[(3.5, 0.0), (3.5, 100.0)]);
    lane(&mut net, &[(0.0, 100.0), (0.0, 125.0)]);
    lane(&mut net, &[(3.5, 100.0), (0.0, 125.0)]);
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
    assert!((d - 115.24).abs() < 0.1, "ends {d} m ahead");
    assert_eq!(net.merges_into(M0, 10.0, 400.0), None);
    assert_eq!(net.merges_into(L0, 10.0, 400.0), None);
    // out of sight, it does not end yet
    assert_eq!(net.merges_into(ACCEL, 10.0, 100.0), None);
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
    assert!(
        (stop - 114.24).abs() < 0.1,
        "stops at {stop}, not at the end of the first piece"
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
