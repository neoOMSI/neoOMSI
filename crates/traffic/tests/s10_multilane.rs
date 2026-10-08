//! Unrequested passing decisions must reach arbitration; a free adjacent lane stays free.
mod common;
use common::maneuver::two_lanes;
use glam::{DVec2, DVec3};
use traffic::*;

fn body(a: &ManeuverActor, net: &Network) -> BodyFootprint {
    let mut b = BodyFootprint::new(
        a.id,
        net.lanes[a.lane].at(a.s).0.truncate() + DVec2::Y * ((a.front - a.rear) * 0.5) as f64,
        DVec2::Y,
        a.length as f64 * 0.5,
        a.half_width as f64,
        0.0,
        3.0,
        a.speed,
    );
    b.front = a.front;
    b.rear = a.rear;
    b.current = Some(Placement {
        lane: LaneId(a.lane),
        s: a.s,
        lateral: 0.0,
        foreign: false,
    });
    b
}
fn scenario(
    speed: f32,
    blocked: bool,
    bus_waiting_for_red: bool,
    static_blocked: bool,
) -> Option<ChangeCommand> {
    let net = two_lanes();
    let mut ego = ManeuverActor::new(VehicleId(1), 0, 60.0);
    ego.speed = speed;
    ego.stopped = if speed == 0.0 { 5.0 } else { 0.0 };
    ego.front = 2.0;
    ego.rear = 2.0;
    ego.length = 4.0;
    ego.half_width = 0.9;
    let mut bus = ManeuverActor::new(VehicleId(2), 0, if speed == 0.0 { 72.0 } else { 90.0 });
    bus.front = 6.0;
    bus.rear = 4.0;
    bus.length = 10.0;
    bus.at_stop = !bus_waiting_for_red;
    bus.light_hold = bus_waiting_for_red;
    let blocker = ManeuverActor::new(VehicleId(3), 1, 60.0);
    let mut actors = vec![ego, bus];
    if blocked {
        actors.push(blocker);
    }
    let occ = Occupancy::build(
        net.version(),
        0,
        actors.iter().map(|a| body(a, &net)).collect(),
    );
    let clear = |_: &[SweepSample], _: &ManeuverActor| !static_blocked;
    let mut c = ManeuverCoordinator::new();
    let mut state = ManeuverState::default();
    for tick in 0..150 {
        let scene = ManeuverScene {
            net: &net,
            occupancy: &occ,
            actors: &actors,
            people: &[],
            static_clearance: Some(&clear),
            time: tick as f32 * 0.02,
            dt: 0.02,
            tick,
        };
        let it = c.intent(&scene, &actors[0], &state);
        c.begin_tick(&[it], tick);
        let mut input = ManeuverInputs::new(0);
        input.lead_standing = !bus_waiting_for_red;
        input.lead_gap = Some(6.0);
        input.obstacle_len = 10.0;
        if let Some(change) = c.plan(&scene, &mut state, &input).change {
            return Some(change);
        }
    }
    None
}

#[test]
fn a_moving_car_requests_a_safe_pass_on_the_parallel_lane() {
    assert_eq!(scenario(8.0, false, false, false).unwrap().to, 1);
}
#[test]
fn a_stopped_car_can_bypass_a_serving_bus() {
    assert_eq!(
        scenario(0.0, false, false, false).unwrap().kind,
        ChangeKind::Bypass
    );
}

fn segmented_bypass(blocked: bool, fork: bool, gap: f64) -> Option<ChangeCommand> {
    let mut net = Network::default();
    for (x, from, to) in [
        (0.0, 0.0, 30.0),
        (-3.0, 0.0, 30.0),
        (0.0, 30.0, 60.0),
        (-3.0, 30.0, 60.0),
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
    if fork {
        net.lanes[1].next.clear();
    }
    let mut ego = ManeuverActor::new(VehicleId(167), 0, 28.3);
    ego.stopped = 5.0;
    ego.planned_next = Some(2);
    ego.front = 2.13;
    ego.rear = 2.11;
    ego.length = 4.24;
    ego.half_width = 0.83;
    ego.pass_room = 5.25;
    let mut bus = ManeuverActor::new(VehicleId(93), 2, (12.23 + gap) as f32);
    bus.at_stop = true;
    bus.front = 5.68;
    bus.rear = 3.88;
    bus.length = 9.56;
    bus.half_width = 1.24;
    let mut actors = vec![ego, bus];
    if blocked {
        actors.push(ManeuverActor::new(VehicleId(7), 3, 3.0));
    }
    let mut feet: Vec<_> = actors.iter().map(|a| body(a, &net)).collect();
    // The articulated rear has no lane placement, but still constrains the sweep.
    feet.push(feet[1].part_of(1, DVec2::new(0.0, 34.03 + gap), DVec2::Y, 3.6, 1.24));
    let occ = Occupancy::build(net.version(), 0, feet);
    let mut coordinator = ManeuverCoordinator::new();
    let mut memory = ManeuverState::default();
    for tick in 0..150 {
        let scene = ManeuverScene {
            net: &net,
            occupancy: &occ,
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
            return Some(change);
        }
    }
    None
}

#[test]
fn bus_bypass_uses_the_continuing_parallel_road_past_a_short_spline() {
    assert_eq!(
        segmented_bypass(false, false, 5.25).unwrap().kind,
        ChangeKind::Bypass
    );
    assert!(
        segmented_bypass(true, false, 5.25).is_none(),
        "car on the next target piece"
    );
    assert!(
        segmented_bypass(false, true, 5.25).is_none(),
        "target lane ends at the joint"
    );
    assert!(
        segmented_bypass(false, false, 2.45).is_none(),
        "too close to the articulated rear to steer out"
    );
}
#[test]
fn no_bypass_through_an_occupied_lane_or_scenery() {
    assert!(scenario(0.0, true, false, false).is_none());
    assert!(scenario(0.0, false, false, true).is_none());
}
#[test]
fn a_red_light_queue_is_not_mistaken_for_a_bus_stop() {
    assert!(scenario(0.0, false, true, false).is_none());
}
#[test]
fn a_stopped_bus_does_not_block_the_corridor_of_the_free_lane() {
    let net = two_lanes();
    let mut bus = ManeuverActor::new(VehicleId(2), 0, 90.0);
    bus.front = 6.0;
    bus.rear = 10.0;
    bus.length = 16.0;
    let occ = Occupancy::build(net.version(), 0, vec![body(&bus, &net)]);
    let samples: Vec<_> = (60..110)
        .map(|s| SweepSample {
            p: DVec3::new(-3.5, s as f64, 0.0),
            d: (s - 60) as f32,
            dir: DVec2::Y,
        })
        .collect();
    assert!(
        occ.swept_clearance(&samples, 0.9, &[VehicleId(1)])
            .is_none()
    );
}

#[test]
fn a_required_route_change_precedes_an_optional_pass() {
    for reverse in [false, true] {
        let mut intents = vec![
            ManeuverIntent {
                vehicle: VehicleId(1),
                target: Some(LaneId(1)),
                required: false,
            },
            ManeuverIntent {
                vehicle: VehicleId(99),
                target: Some(LaneId(1)),
                required: true,
            },
        ];
        if reverse {
            intents.reverse();
        }
        let mut c = ManeuverCoordinator::new();
        c.begin_tick(&intents, 1);
        assert!(c.approved(VehicleId(1)).is_none());
        assert_eq!(c.approved(VehicleId(99)), Some(LaneId(1)));
    }
}

#[test]
fn a_boarding_bus_does_not_reserve_a_future_lane_change() {
    let net = two_lanes();
    let mut bus = ManeuverActor::new(VehicleId(99), 0, 60.0);
    bus.at_stop = true;
    bus.route_next = Some(1);
    let actors = [bus];
    let occ = Occupancy::build(net.version(), 0, vec![body(&actors[0], &net)]);
    let scene = ManeuverScene {
        net: &net,
        occupancy: &occ,
        actors: &actors,
        people: &[],
        static_clearance: None,
        time: 0.0,
        dt: 0.02,
        tick: 0,
    };
    let mut coord = ManeuverCoordinator::new();
    let mut state = ManeuverState::default();
    assert!(coord.intent(&scene, &actors[0], &state).target.is_none());
    assert!(
        coord
            .plan(&scene, &mut state, &ManeuverInputs::new(0))
            .change
            .is_none()
    );
}

#[test]
fn tight_spatial_queries_still_find_long_bodies_and_cell_boundary_contacts() {
    let mut feet: Vec<_> = (-3..=3)
        .map(|i| {
            BodyFootprint::new(
                VehicleId((i + 4) as u64),
                DVec2::new(i as f64 * 50.0, 2.0),
                DVec2::Y,
                4.0,
                1.0,
                0.0,
                3.0,
                0.0,
            )
        })
        .collect();
    feet.push(BodyFootprint::new(
        VehicleId(99),
        DVec2::new(160.0, 0.0),
        DVec2::X,
        130.0,
        1.0,
        0.0,
        3.0,
        0.0,
    ));
    let occ = Occupancy::build(NetworkVersion(1), 0, feet.clone());
    for x in [-50.01, -49.99, 0.0, 49.99, 50.01, 99.99] {
        let p = DVec2::new(x, 0.0);
        let mut found = Vec::new();
        occ.near(p, 2.0, |f| found.push(f.owner));
        let mut expected: Vec<_> = feet
            .iter()
            .filter(|f| (f.center - p).length() <= 2.0 + f.half_len.max(f.half_w))
            .map(|f| f.owner)
            .collect();
        found.sort_unstable();
        expected.sort_unstable();
        assert_eq!(found, expected, "query at {x}");
    }
}
