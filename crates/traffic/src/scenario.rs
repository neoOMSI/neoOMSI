//! Minimal headless scenario runner.
//!
//! A scenario is a fixed-step loop with no renderer and no OMSI assets. The step closure
//! receives the tick index, elapsed simulation time, and the [`Capture`] to record into;
//! it owns the world and pushes snapshots/events. This is the seam the Stage 0 scenarios
//! (S1–S3) and later automatic captures run on.

use crate::diagnostics::{Capture, TraceHeader};

/// Fixed simulation step for the traffic domain (s): decisions run at a stable rate,
/// independent of the render frame rate. Rendering may run at any rate on top.
pub const SIM_DT: f32 = 0.02;
/// Most fixed steps to run in one frame; a long stall is bounded and the rest of the debt
/// is kept (capped) rather than silently lost or turned into one huge step.
pub const MAX_SIM_STEPS: u32 = 8;

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

/// Advance a fixed-step accumulator by one render frame.
///
/// Returns how many fixed `dt` steps to run this frame. The debt is capped at `max_steps`
/// frames' worth, so a long stall is bounded rather than turned into one huge step. The
/// sequence of fixed steps depends only on the elapsed time, not on how it was partitioned
/// into render frames (below the cap), which is what makes replays frame-rate independent.
pub fn advance_fixed_clock(accum: &mut f32, frame_dt: f32, dt: f32, max_steps: u32) -> u32 {
    if dt <= 0.0 {
        return 0;
    }
    // Cap the debt at one step more than a frame may catch up, so a long stall is bounded
    // yet a little debt survives for the next frame instead of being silently dropped.
    *accum = (*accum + frame_dt).min(dt * (max_steps + 1) as f32);
    // A single division avoids the rounding drift of repeated subtraction, which otherwise
    // drops a decision tick over a long run at high frame rates and makes replays diverge.
    let steps = (*accum / dt).floor().min(max_steps as f32) as u32;
    *accum -= dt * steps as f32;
    if *accum < 0.0 {
        *accum = 0.0;
    }
    steps
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

    #[test]
    fn the_same_time_partitioned_into_frames_runs_the_same_ticks() {
        let dt = 0.02f32;
        let mut a = 0.0f32;
        let mut steps_a = 0u32;
        for _ in 0..30 {
            steps_a += advance_fixed_clock(&mut a, 1.0 / 30.0, dt, 8);
        }
        let mut b = 0.0f32;
        let mut steps_b = 0u32;
        for _ in 0..60 {
            steps_b += advance_fixed_clock(&mut b, 1.0 / 60.0, dt, 8);
        }
        assert_eq!(steps_a, steps_b, "frame partition changed the tick count");
        assert_eq!(steps_a, 50);
        assert!((a - b).abs() < 1e-5);
    }

    #[test]
    fn a_long_stall_is_bounded_and_keeps_debt() {
        let dt = 0.02f32;
        let mut accum = 0.0f32;
        let steps = advance_fixed_clock(&mut accum, 10.0, dt, 8);
        assert_eq!(steps, 8, "catch-up must be bounded");
        assert!(accum > 0.0, "remaining debt is kept, not lost");
        assert!(accum <= dt * 8.0);
    }
}
