//! Stage 1 exit gate — fixed-tick replays match across render-frame partitions.
//!
//! The S1 world (a queue at a red signal) is stepped with the same fixed `SIM_DT` the
//! window and offscreen paths use, but the elapsed time is partitioned into 15/30/60/144
//! FPS frames through [`advance_fixed_clock`]. The sequence of decision ticks, and the
//! rolling decision hash, must be identical regardless of how the time was partitioned.

use traffic::scenario::{advance_fixed_clock, MAX_SIM_STEPS, SIM_DT};
use traffic::{
    AiState, Capture, LaneBuilder, LaneId, LaneKind, Network, NetworkVersion, Reason,
    TRACE_VERSION, TickSnapshot, TraceHeader, VehicleId, VehicleSnapshot,
};

const RED_SECONDS: f32 = 2.0;
const TOTAL_SECONDS: u32 = 6;
const CARS: usize = 20;
const SPACING: f32 = 6.0;
const STOP_GAP: f32 = 0.6;

fn network() -> Network {
    let lane = LaneBuilder::polyline(
        vec![
            glam::DVec3::new(0.0, 0.0, 0.0),
            glam::DVec3::new(0.0, 400.0, 0.0),
        ],
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

struct S1 {
    net: Network,
    line: f32,
    cars: Vec<AiState>,
    tick: u64,
}

impl S1 {
    fn new() -> S1 {
        let net = network();
        let line = net.lanes[0].length();
        let cars = (0..CARS)
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
            .collect();
        S1 {
            net,
            line,
            cars,
            tick: 0,
        }
    }

    fn step(&mut self, cap: &mut Capture) {
        let t = self.tick as f32 * SIM_DT;
        let red = t < RED_SECONDS;
        let order = {
            let mut o: Vec<usize> = (0..self.cars.len()).collect();
            o.sort_by(|&a, &b| self.cars[b].s.partial_cmp(&self.cars[a].s).unwrap());
            o
        };
        for rank in 0..order.len() {
            let i = order[rank];
            let obstacle = if rank == 0 {
                None
            } else {
                let lead = &self.cars[order[rank - 1]];
                Some((lead.s - lead.rear) - self.cars[i].s)
            };
            let stop_at = if red {
                Some(self.line - self.cars[i].s)
            } else {
                None
            };
            self.cars[i].advance(&self.net, SIM_DT, obstacle, stop_at);
        }

        let vehicles = self
            .cars
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
        cap.push_tick(TickSnapshot {
            tick: self.tick,
            sim_time: t as f64,
            network_version: NetworkVersion(1),
            vehicles,
        });
        self.tick += 1;
    }
}

fn header() -> TraceHeader {
    TraceHeader {
        trace_version: TRACE_VERSION,
        source_revision: "stage1-replay".into(),
        platform: std::env::consts::OS.into(),
        seed: 7919,
        tick_hz: 1.0 / SIM_DT,
        network_version: NetworkVersion(1),
        input_digest: 0,
    }
}

/// Run the S1 world with `frame_dt` frames for `TOTAL_SECONDS`, returning the sequence of
/// frozen decision ticks.
fn run_partition(fps: u32) -> Vec<TickSnapshot> {
    let mut sim = S1::new();
    let mut cap = Capture::new(header(), 8192);
    let frame_dt = 1.0f32 / fps as f32;
    let frames = fps * TOTAL_SECONDS;
    let mut accum = 0.0f32;
    for _ in 0..frames {
        let steps = advance_fixed_clock(&mut accum, frame_dt, SIM_DT, MAX_SIM_STEPS);
        for _ in 0..steps {
            sim.step(&mut cap);
        }
    }
    cap.ticks.into_iter().collect()
}

#[test]
fn the_same_world_decides_the_same_at_every_frame_rate() {
    let expected = (TOTAL_SECONDS as f32 / SIM_DT).round() as usize;
    let baseline = run_partition(60);
    assert_eq!(
        baseline.len(),
        expected,
        "60 fps did not run the expected number of fixed ticks"
    );
    for fps in [15, 30, 60, 144] {
        let ticks = run_partition(fps);
        // A partition may land one tick short at an exact tick boundary, but every tick it
        // did run must be decision-for-decision identical to the baseline.
        assert!(
            ticks.len().abs_diff(baseline.len()) <= 1,
            "{fps} fps ran {} fixed ticks, 60 fps ran {}",
            ticks.len(),
            baseline.len()
        );
        let common = ticks.len().min(baseline.len());
        assert_eq!(
            &ticks[..common],
            &baseline[..common],
            "{fps} fps produced a different decision sequence"
        );
    }
}
