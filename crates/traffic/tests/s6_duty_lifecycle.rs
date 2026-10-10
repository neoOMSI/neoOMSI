//! Stage 6 exit gate — the duty lifecycle: terminus handover, out of service, pending route
//! and no duplicate ownership.
//!
//! The timetable assignment itself stays in `schedule`; this checks the service-side
//! lifecycle contract: exactly one owner per duty, an explicit next-trip/out-of-service state,
//! a diagnosed pending route, and legitimate simultaneous departures from different duties.

mod common;

use common::service::{berth, Bus, ServiceWorld};
use traffic::perception::Occupancy;
use traffic::*;

const BERTH_S: f32 = 120.0;

fn lane_net() -> Network {
    let mut n = Network::default();
    n.lanes.push(LaneBuilder::polyline(
        vec![glam::DVec3::new(0.0, 0.0, 0.0), glam::DVec3::new(0.0, 400.0, 0.0)],
        LaneKind::Street,
        3.0,
    ));
    n.link(1.5);
    n
}

#[test]
fn a_terminus_reaches_next_trip_and_keeps_one_owner() {
    let mut w = ServiceWorld::new();
    let mut bus = Bus::new(1, 90.0, vec![berth(7001, BERTH_S, 36000.0)]);
    bus.policy.is_last = true;
    bus.policy.route_open = false;
    w.add(bus);
    w.run(4000);
    assert!(w.bus(1).state.trip_done(), "the terminus did not reach next-trip");
    assert_eq!(w.bus(1).phase(), ServicePhase::NextTrip);
    assert!(w.coord.berth_owner(StopId(7001), 0).is_none(), "the berth was not released");
}

#[test]
fn out_of_service_is_terminal() {
    let mut st = ServiceState::new();
    st.phase = ServicePhase::OutOfService;
    assert!(st.trip_done());
    assert!(!st.holds_berth());
}

#[test]
fn a_route_that_ends_is_diagnosed_pending_not_hidden() {
    let net = lane_net();
    let occ = Occupancy::default();
    let actors = vec![ServiceActor::new(VehicleId(1), 0, 200.0)];
    let scene = ServiceScene {
        net: &net,
        occupancy: &occ,
        actors: &actors,
        day_time: 36000.0,
        dt: 0.02,
        tick: 0,
    };
    let mut coord = ServiceCoordinator::new();
    coord.begin_tick(&[], 0);
    let mut st = ServiceState::new();
    st.phase = ServicePhase::Approach;
    let inputs = ServiceInputs {
        actor: 0,
        berth: None,
        distance: f32::MAX,
        policy: StopPolicy {
            route_open: true,
            ..StopPolicy::default()
        },
        demand: StopDemand::default(),
        feedback: ScriptFeedback::Idle,
        passing: false,
        kerb_swerve: None,
        junction_first: false,
        berth_held_long: false,
    };
    let dec = coord.plan(&scene, &mut st, &inputs);
    assert_eq!(st.phase, ServicePhase::RoutePending);
    assert_eq!(dec.binding, Some(Reason::RoutePending));
    assert!(Reason::RoutePending.is_valid_wait(), "a pending route is a legitimate wait");
}

#[test]
fn a_next_trip_handover_resets_the_duty_cleanly() {
    let mut w = ServiceWorld::new();
    w.add(Bus::new(1, 90.0, vec![berth(7001, BERTH_S, 36000.0)]));
    // Let it take the berth and board.
    for _ in 0..1500 {
        w.step();
        if w.bus(1).phase() == ServicePhase::Boarding {
            break;
        }
    }
    assert!(w.coord.berth_owner(StopId(7001), 0).is_some());
    // Hand the tour's next trip to this same physical vehicle.
    let id = w.bus(1).id;
    w.coord.release(id);
    w.bus_mut(1).state.restart(false);
    assert!(w.coord.berth_owner(StopId(7001), 0).is_none(), "a stale berth survived handover");
    assert_eq!(w.bus(1).phase(), ServicePhase::EnRoute);
    assert!(!w.bus(1).state.holds_berth());
}

#[test]
fn two_duties_may_depart_simultaneously() {
    let mut w = ServiceWorld::new();
    w.add(Bus::new(1, 90.0, vec![berth(7001, BERTH_S, 36000.0)]));
    w.add(Bus::new(2, 270.0, vec![berth(7002, 300.0, 36000.0)]));
    let mut max_boarders = 0usize;
    for _ in 0..4000 {
        w.step();
        max_boarders = max_boarders.max(
            w.buses
                .iter()
                .filter(|b| b.phase() == ServicePhase::Boarding)
                .count(),
        );
    }
    assert_eq!(max_boarders, 2, "two different duties should serve in parallel");
    assert_eq!(w.bus(1).served, vec![StopId(7001)]);
    assert_eq!(w.bus(2).served, vec![StopId(7002)]);
}
