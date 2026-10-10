//! Stage 6 exit gate — the door/station script handshake.
//!
//! Acknowledged, unsupported and stuck feedback are distinct: an unsupported script uses the
//! documented fixed-close fallback, a stuck script is waited on up to the timeout, and a
//! script that reports its doors open is never driven off.

mod common;

use common::service::berth;
use traffic::perception::Occupancy;
use traffic::*;

const STOP: i64 = 7001;
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

struct Fixture {
    net: Network,
    occ: Occupancy,
    actors: Vec<ServiceActor>,
    coord: ServiceCoordinator,
    b: BerthGeometry,
}

impl Fixture {
    fn new() -> Fixture {
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
        Fixture {
            net: lane_net(),
            occ: Occupancy::default(),
            actors: vec![ServiceActor::new(VehicleId(1), 0, BERTH_S)],
            coord,
            b: berth(STOP, BERTH_S, 36000.0),
        }
    }

    fn plan(
        &mut self,
        phase: ServicePhase,
        phase_t: f32,
        boarding_t: f32,
        feedback: ScriptFeedback,
    ) -> (ServiceState, ServiceDecision) {
        let scene = ServiceScene {
            net: &self.net,
            occupancy: &self.occ,
            actors: &self.actors,
            day_time: 36000.0,
            dt: 0.02,
            tick: 0,
        };
        let mut st = ServiceState::new();
        st.phase = phase;
        st.berth = Some(self.b);
        st.phase_t = phase_t;
        st.boarding_t = boarding_t;
        let inputs = ServiceInputs {
            actor: 0,
            berth: Some(self.b),
            distance: 0.0,
            policy: StopPolicy::default(),
            demand: StopDemand::default(),
            feedback,
            passing: false,
            kerb_swerve: None,
            junction_first: false,
            berth_held_long: false,
        };
        let dec = self.coord.plan(&scene, &mut st, &inputs);
        (st, dec)
    }
}

#[test]
fn acknowledged_doors_depart_after_the_minimum_close_time() {
    let mut f = Fixture::new();
    let (_, dec) = f.plan(
        ServicePhase::ClosingDoors,
        CLOSE_MIN,
        0.0,
        ScriptFeedback::Released,
    );
    assert!(matches!(dec.phase, ServicePhase::Departing | ServicePhase::WaitingToMerge));
}

#[test]
fn unsupported_scripts_use_the_fixed_close_fallback() {
    let mut f = Fixture::new();
    let (_, dec) = f.plan(
        ServicePhase::ClosingDoors,
        CLOSE_MIN,
        0.0,
        ScriptFeedback::Unsupported,
    );
    assert!(
        matches!(dec.phase, ServicePhase::Departing | ServicePhase::WaitingToMerge),
        "an unsupported script did not use the fixed fallback"
    );
}

#[test]
fn a_stuck_script_is_waited_on_then_times_out_with_a_fault() {
    let mut f = Fixture::new();
    let (_, dec) = f.plan(
        ServicePhase::ClosingDoors,
        CLOSE_MAX + 1.0,
        0.0,
        ScriptFeedback::StuckUnknown,
    );
    assert!(
        matches!(dec.phase, ServicePhase::Departing | ServicePhase::WaitingToMerge),
        "the stuck script did not clear after the timeout"
    );
    assert!(dec
        .events
        .iter()
        .any(|e| matches!(e, TraceEvent::Fault { reason: Reason::StationRelease, .. })));
}

#[test]
fn a_script_reporting_open_doors_is_never_driven_off() {
    let mut f = Fixture::new();
    let (_, dec) = f.plan(
        ServicePhase::ClosingDoors,
        CLOSE_MAX + 5.0,
        0.0,
        ScriptFeedback::StuckDoorsOpen,
    );
    assert_eq!(dec.phase, ServicePhase::ClosingDoors, "departed with open doors");
    assert!(dec
        .events
        .iter()
        .any(|e| matches!(e, TraceEvent::Fault { reason: Reason::StationRelease, .. })));
}

#[test]
fn a_doorway_hold_defers_the_close_request() {
    let mut f = Fixture::new();
    let (_, dec) = f.plan(ServicePhase::Boarding, 5.0, 50.0, ScriptFeedback::Idle);
    assert_eq!(dec.phase, ServicePhase::Boarding);
    assert!(dec.boarding, "boarding permission was lost while the doorway was held");
    assert!(
        !dec.events.iter().any(|e| matches!(e, TraceEvent::CloseRequest { .. })),
        "the doors were told to close while somebody was in the doorway"
    );
}
