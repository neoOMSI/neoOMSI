//! Stage 6 exit gate — berth recovery, occupied stops and overshoot.
//!
//! A stop occupied by somebody else (the player, a parked body) is waited for, not docked
//! through; a bus that overshoots the boarding region records a missed/faulted stop instead
//! of opening its doors somewhere up the queue.

mod common;

use common::service::{berth, occupier, Bus, ServiceWorld};
use traffic::perception::Occupancy;
use traffic::*;

const STOP: i64 = 7001;
const BERTH_S: f32 = 120.0;

#[test]
fn an_occupied_stop_is_waited_for_not_docked_through() {
    let mut w = ServiceWorld::new();
    w.add(Bus::new(1, 90.0, vec![berth(STOP, BERTH_S, 36000.0)]));
    // The player stands on the berth.
    w.extras.push(occupier(9999, BERTH_S));

    for _ in 0..600 {
        w.step();
        assert_ne!(w.bus(1).phase(), ServicePhase::Boarding, "boarded an occupied stop");
        assert!(w.bus(1).served.is_empty(), "an occupied stop was served");
    }

    // The obstruction leaves: the same bus now docks and boards.
    w.extras.clear();
    let mut boarded = false;
    for _ in 0..1200 {
        w.step();
        if w.bus(1).phase() == ServicePhase::Boarding {
            boarded = true;
        }
    }
    assert!(boarded, "the bus never docked after the stop cleared");
    assert_eq!(w.bus(1).served, vec![StopId(STOP)]);
}

#[test]
fn a_berth_held_for_long_is_served_from_behind_and_then_left() {
    let mut w = ServiceWorld::new();
    w.add(Bus::new(1, 90.0, vec![berth(STOP, BERTH_S, 36000.0)]));
    // A bus on its layover stands on the berth for a quarter of an hour.
    w.extras.push(occupier(9999, BERTH_S));
    w.held_long = true;

    let mut boarded_at = None;
    for _ in 0..3000 {
        w.step();
        if boarded_at.is_none() && w.bus(1).phase() == ServicePhase::Boarding {
            boarded_at = Some(w.bus(1).s);
        }
        if !w.bus(1).served.is_empty() {
            break;
        }
    }
    let at = boarded_at.expect("the bus never served the stop from behind the layover");
    assert!(at < BERTH_S, "boarded through the vehicle on the berth (at {at})");
    assert_eq!(w.bus(1).served, vec![StopId(STOP)], "the stop is done as it moves off");
    assert_eq!(w.bus(1).phase(), ServicePhase::EnRoute);
    assert!(!w.bus(1).state.behind);
    assert_eq!(w.coord.berth_count(), 0, "it never took the berth");
}

#[test]
fn overshoot_records_a_missed_stop_without_opening_the_doors() {
    let net = {
        let mut n = Network::default();
        n.lanes.push(LaneBuilder::polyline(
            vec![glam::DVec3::new(0.0, 0.0, 0.0), glam::DVec3::new(0.0, 400.0, 0.0)],
            LaneKind::Street,
            3.0,
        ));
        n.link(1.5);
        n
    };
    let occ = Occupancy::default();
    let actors = vec![ServiceActor::new(VehicleId(1), 0, 140.0)];
    let scene = ServiceScene {
        net: &net,
        occupancy: &occ,
        actors: &actors,
        day_time: 36000.0,
        dt: 0.02,
        tick: 0,
    };
    let b = berth(STOP, BERTH_S, 36000.0);
    let mut coord = ServiceCoordinator::new();
    coord.begin_tick(
        &[BerthIntent {
            vehicle: VehicleId(1),
            stop: StopId(STOP),
            occurrence: 0,
            holds: true,
        }],
        0,
    );
    let mut st = ServiceState::new();
    st.phase = ServicePhase::Docking;
    st.berth = Some(b);
    let inputs = ServiceInputs {
        actor: 0,
        berth: Some(b),
        // 20 m past the berth: well beyond the boarding region.
        distance: -20.0,
        policy: StopPolicy::default(),
        demand: StopDemand::default(),
        feedback: ScriptFeedback::Idle,
        passing: false,
        kerb_swerve: None,
        junction_first: false,
        berth_held_long: false,
    };
    let dec = coord.plan(&scene, &mut st, &inputs);
    assert!(dec.consume_stop, "the missed stop must be advanced, not silently kept");
    assert_eq!(st.phase, ServicePhase::EnRoute);
    assert_eq!(st.fault, Some(Reason::MissedStop));
    assert!(dec.stop_at.is_none(), "the missed stop must not hold traffic forever");
    assert!(dec
        .events
        .iter()
        .any(|e| matches!(e, TraceEvent::Fault { reason: Reason::MissedStop, .. })));
    assert!(!dec.boarding, "doors opened on an overshot stop");
}
