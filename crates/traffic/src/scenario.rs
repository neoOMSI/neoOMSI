//! Minimal headless scenario runner.
//!
//! A scenario is a fixed-step loop with no renderer and no OMSI assets. The step closure
//! receives the tick index, elapsed simulation time, and the [`Capture`] to record into;
//! it owns the world and pushes snapshots/events. This is the seam the Stage 0 scenarios
//! (S1–S3) and later automatic captures run on.

use crate::diagnostics::{Capture, TraceHeader};

/// A fixed-step headless scenario description.
#[derive(Debug, Clone)]
pub struct Scenario {
    pub name: String,
    /// Fixed simulation step (s).
    pub dt: f32,
    /// Number of steps to run.
    pub ticks: u64,
    /// Rolling-buffer capacity for the capture.
    pub capacity: usize,
}

impl Scenario {
    pub fn new(name: impl Into<String>, dt: f32, ticks: u64, capacity: usize) -> Scenario {
        Scenario {
            name: name.into(),
            dt,
            ticks,
            capacity,
        }
    }

    /// Total simulated time of the scenario (s).
    pub fn duration(&self) -> f32 {
        self.dt * self.ticks as f32
    }
}

/// Run a scenario, calling `step(tick, sim_time, capture)` each fixed step.
pub fn run<F>(scenario: &Scenario, header: TraceHeader, mut step: F) -> Capture
where
    F: FnMut(u64, f32, &mut Capture),
{
    let mut capture = Capture::new(header, scenario.capacity);
    let mut sim_time = 0.0f32;
    for tick in 0..scenario.ticks {
        step(tick, sim_time, &mut capture);
        sim_time += scenario.dt;
    }
    capture
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::diagnostics::{Reason, TickSnapshot, VehicleSnapshot};
    use crate::ids::{LaneId, NetworkVersion, VehicleId};

    fn header() -> TraceHeader {
        TraceHeader {
            trace_version: crate::diagnostics::TRACE_VERSION,
            source_revision: "test".into(),
            platform: "test".into(),
            seed: 1,
            tick_hz: 50.0,
            network_version: NetworkVersion(1),
            input_digest: 0,
        }
    }

    #[test]
    fn a_scenario_runs_a_fixed_number_of_steps() {
        let scenario = Scenario::new("t", 0.02, 10, 16);
        assert!((scenario.duration() - 0.2).abs() < 1e-6);
        let capture = run(&scenario, header(), |tick, t, cap| {
            cap.push_tick(TickSnapshot {
                tick,
                sim_time: t as f64,
                network_version: NetworkVersion(1),
                vehicles: vec![VehicleSnapshot {
                    id: VehicleId(1),
                    lane: LaneId(0),
                    s: tick as f32,
                    speed: 0.0,
                    front: 2.0,
                    rear: 2.0,
                    constraints: vec![Reason::RedSignal],
                    binding: Some(Reason::RedSignal),
                }],
            });
        });
        assert_eq!(capture.ticks.len(), 10);
        assert_eq!(capture.ticks.front().unwrap().tick, 0);
        assert_eq!(capture.ticks.back().unwrap().tick, 9);
    }
}
