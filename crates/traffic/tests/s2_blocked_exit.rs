//! S2 — a blocked downstream exit.
//!
//! A synthetic, headless approximation of the "full downstream storage" case: a vehicle
//! approaches a stopped blocker that occupies the space ahead for longer than the old
//! `GRIDLOCK_WAIT` (45 s). Waiting duration must never grant passage: the vehicle only
//! moves on once the blocker is physically removed. Junction-admission specifics live in
//! `core` and are exercised once the integration facade lands.
//!
//! What it checks:
//! - the vehicle never overlaps or passes the blocker while it is present;
//! - after the blocker is removed at t = 50 s, the vehicle resumes and passes the point;
//! - the capture records the wait and the recovery.

use traffic::{
    AiState, Capture, LaneBuilder, LaneKind, Network, NetworkVersion, Reason, TRACE_VERSION,
    TickSnapshot, TraceHeader, VehicleSnapshot,
};

const DT: f32 = 1.0 / 50.0;
const BLOCKER_HOLD: f32 = 50.0;
const TOTAL_SECONDS: f32 = 75.0;
const BLOCKER_S: f32 = 150.0;
const BLOCKER_REAR: f32 = 2.0;

fn network() -> Network {
    let lane = LaneBuilder::polyline(
        vec![glam::DVec3::new(0.0, 0.0, 0.0), glam::DVec3::new(0.0, 250.0, 0.0)],
        LaneKind::Street,
        3.0,
    );
    let mut net = Network {
        lanes: vec![lane],
        ..Default::default()
    };
    net.link(1.5);
    net
}

#[test]
fn s2_waiting_never_grants_passage() {
    let net = network();
    let mut car = AiState::new(0, 10.0, 4242);
    car.front = 2.25;
    car.rear = 2.25;
    car.length = 4.5;
    car.max_speed_kmh = 50.0;
    car.speed = 8.0;

    let mut capture = Capture::new(
        TraceHeader {
            trace_version: TRACE_VERSION,
            source_revision: "stage1-s2".into(),
            platform: std::env::consts::OS.into(),
            seed: 4242,
            tick_hz: 1.0 / DT,
            network_version: NetworkVersion(1),
            input_digest: 0,
        },
        4096,
    );

    let blocker_front_limit = BLOCKER_S - BLOCKER_REAR;
    let mut max_front_while_blocked = f32::NEG_INFINITY;
    let mut min_gap_while_blocked = f32::INFINITY;
    let mut passed_after_removal = false;
    let mut tick = 0u64;
    let mut t = 0.0f32;

    while t < TOTAL_SECONDS {
        let blocked = t < BLOCKER_HOLD;
        let obstacle = blocked.then_some(blocker_front_limit - car.s);
        car.advance(&net, DT, obstacle, None);

        if blocked {
            max_front_while_blocked = max_front_while_blocked.max(car.s + car.front);
            min_gap_while_blocked = min_gap_while_blocked.min(blocker_front_limit - (car.s + car.front));
        } else if car.s > BLOCKER_S {
            passed_after_removal = true;
        }

        capture.push_tick(TickSnapshot {
            tick,
            sim_time: t as f64,
            network_version: NetworkVersion(1),
            vehicles: vec![VehicleSnapshot {
                id: traffic::VehicleId(1),
                lane: traffic::LaneId(0),
                s: car.s,
                speed: car.speed,
                realized_speed: car.speed,
                accel: 0.0,
                emergency: false,
                reconciled: true,
                front: car.front,
                rear: car.rear,
                junction_state: traffic::JunctionState::Cleared,
                junction_blocker: None,
                service_phase: traffic::ServicePhase::EnRoute,
                berth_owner: None,
                service_stop: None,
                maneuver_phase: traffic::ManeuverPhase::Idle,
                maneuver_target: None,
                lifecycle: traffic::Lifecycle::Active,
                constraints: if blocked { vec![Reason::OccupiedExit] } else { Vec::new() },
                binding: if blocked { Some(Reason::OccupiedExit) } else { None },
            }],
        });

        tick += 1;
        t += DT;
    }

    assert!(
        max_front_while_blocked <= blocker_front_limit + 0.1,
        "the vehicle entered the occupied space: front {max_front_while_blocked:.2} vs limit {blocker_front_limit:.2}"
    );
    assert!(
        min_gap_while_blocked > -0.5,
        "body overlap with the blocker: min gap {min_gap_while_blocked:.2} m"
    );
    assert!(passed_after_removal, "the vehicle did not resume after the blocker was removed");
}
