//! S1 — queue discharge at a red signal.
//!
//! A synthetic, headless scenario: one straight lane, a stop line, and 20 cars queued
//! bumper-to-bumper with distinct driver traits. The signal is red for 20 s, then green
//! for 30 s. No renderer or OMSI assets are involved.
//!
//! What it checks (provisional Stage 1 bounds from `docs/traffic_refactor/SCENARIOS.md`):
//! - no car's front bumper crosses the stop line while the signal is red;
//! - the queue discharges after green;
//! - no AI-created body overlap appears during the run.

use traffic::{
    AiState, Capture, CaptureTrigger, LaneBuilder, LaneId, LaneKind, Network, NetworkVersion,
    Reason, TRACE_VERSION, TickSnapshot, TraceHeader, VehicleId, VehicleSnapshot,
};

const DT: f32 = 1.0 / 50.0;
const RED_SECONDS: f32 = 20.0;
const TOTAL_SECONDS: f32 = 60.0;
const CARS: usize = 20;
const SPACING: f32 = 6.0;
const STOP_GAP: f32 = 0.6;

fn network() -> Network {
    let lane = LaneBuilder::polyline(
        vec![glam::DVec3::new(0.0, 0.0, 0.0), glam::DVec3::new(0.0, 400.0, 0.0)],
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

fn queued_cars(line: f32) -> Vec<AiState> {
    (0..CARS)
        .map(|i| {
            let front = 2.25;
            let rear = 2.25;
            let s = line - STOP_GAP - i as f32 * SPACING - front;
            let mut car = AiState::new(0, s, (i as u64 + 1) * 7919);
            car.front = front;
            car.rear = rear;
            car.length = front + rear;
            car.max_speed_kmh = 50.0;
            car.desire = 0.9 + 0.15 * (i % 5) as f32 / 4.0;
            car.reaction = 0.4 + 0.1 * (i % 6) as f32;
            car.headway = 1.0 + 0.1 * (i % 4) as f32;
            car
        })
        .collect()
}

#[test]
fn s1_no_line_crossing_on_red_then_discharge() {
    let net = network();
    let line = net.lanes[0].length();
    let mut cars = queued_cars(line);

    let mut capture = Capture::new(
        TraceHeader {
            trace_version: TRACE_VERSION,
            source_revision: "stage1-s1".into(),
            platform: std::env::consts::OS.into(),
            seed: 7919,
            tick_hz: 1.0 / DT,
            network_version: NetworkVersion(1),
            input_digest: 0,
        },
        4096,
    );

    let mut max_front_on_red = f32::NEG_INFINITY;
    let mut min_gap_seen = f32::INFINITY;
    let mut tick = 0u64;
    let mut t = 0.0f32;
    let mut first_move_after_green: Option<f32> = None;

    while t < TOTAL_SECONDS {
        let red = t < RED_SECONDS;
        let order = {
            let mut o: Vec<usize> = (0..cars.len()).collect();
            o.sort_by(|&a, &b| cars[b].s.partial_cmp(&cars[a].s).unwrap());
            o
        };
        for rank in 0..order.len() {
            let i = order[rank];
            let obstacle = if rank == 0 {
                None
            } else {
                let lead = &cars[order[rank - 1]];
                Some((lead.s - lead.rear) - cars[i].s)
            };
            let stop_at = if red { Some(line - cars[i].s) } else { None };
            cars[i].advance(&net, DT, obstacle, stop_at);
        }

        if red {
            for c in &cars {
                max_front_on_red = max_front_on_red.max(c.s + c.front);
            }
        } else if first_move_after_green.is_none() && cars.iter().any(|c| c.speed > 0.5) {
            first_move_after_green = Some(t - RED_SECONDS);
        }
        for w in order.windows(2) {
            let (lead, follower) = (&cars[w[0]], &cars[w[1]]);
            let gap = (lead.s - lead.rear) - (follower.s + follower.front);
            min_gap_seen = min_gap_seen.min(gap);
        }

        let vehicles = cars
            .iter()
            .enumerate()
            .map(|(i, c)| VehicleSnapshot {
                id: VehicleId(i as u64 + 1),
                lane: LaneId(0),
                s: c.s,
                speed: c.speed,
                realized_speed: c.speed,
                accel: 0.0,
                emergency: false,
                reconciled: true,
                front: c.front,
                rear: c.rear,
                constraints: if red { vec![Reason::RedSignal] } else { Vec::new() },
                binding: if red { Some(Reason::RedSignal) } else { None },
            })
            .collect();
        capture.push_tick(TickSnapshot {
            tick,
            sim_time: t as f64,
            network_version: NetworkVersion(1),
            vehicles,
        });

        tick += 1;
        t += DT;
    }

    // No car may cross the line while the signal is red (0.05 m tolerance for the model).
    assert!(
        max_front_on_red <= line + 0.05,
        "a car crossed the stop line on red: front {max_front_on_red:.2} vs line {line:.2}"
    );

    // Bodies never overlap (small negative tolerance for numerical settling).
    assert!(
        min_gap_seen > -0.5,
        "body overlap detected: min bumper gap {min_gap_seen:.2} m"
    );

    // The queue discharges after green.
    let delay = first_move_after_green.expect("the queue never moved after green");
    assert!(delay < 3.0, "first movement took {delay:.2}s after green");

    let front_car = cars.iter().max_by(|a, b| a.s.partial_cmp(&b.s).unwrap()).unwrap();
    assert!(front_car.s > line * 0.5, "the front car did not progress: s={:.1}", front_car.s);

    // The capture is self-contained and replay-hashable.
    assert!(!capture.ticks.is_empty());
    assert_eq!(capture.decision_hash(), capture.decision_hash());
    capture.note_trigger(CaptureTrigger::BodyOverlap);
    assert!(capture.captured());
}
