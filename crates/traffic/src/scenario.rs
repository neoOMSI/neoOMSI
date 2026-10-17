//! Minimal headless scenario runner.
//!
//! A fixed-step loop with no renderer and no OMSI assets; the step closure owns the world
//! and records into the [`Capture`].

use crate::diagnostics::{Capture, TraceHeader};

/// Fixed simulation step for the traffic domain (s), independent of the render frame rate.
pub const SIM_DT: f32 = 0.02;
/// Most steps to run in one frame (25 fps and faster run fixed `SIM_DT` steps); a longer
/// frame is shared out over this many steps, so slow frames cannot spiral the rate down.
pub const MAX_SIM_STEPS: u32 = 2;

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
/// Returns the step count and step length. Up to `max_steps` steps they are fixed `dt` steps
/// (the remainder carries over), so replays do not depend on the frame partition; a longer
/// frame runs `max_steps` equal, longer steps. Either way no time is dropped.
pub fn advance_fixed_clock(accum: &mut f32, frame_dt: f32, dt: f32, max_steps: u32) -> (u32, f32) {
    if dt <= 0.0 || max_steps == 0 {
        return (0, dt);
    }
    *accum += frame_dt.max(0.0);
    // One division: repeated subtraction drifts and drops ticks at high frame rates.
    let due = (*accum / dt).floor();
    if due <= max_steps as f32 {
        let steps = due as u32;
        *accum -= dt * steps as f32;
        if *accum < 0.0 {
            *accum = 0.0;
        }
        return (steps, dt);
    }
    let step = *accum / max_steps as f32;
    *accum = 0.0;
    (max_steps, step)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::diagnostics::{JunctionState, Reason, TickSnapshot, VehicleSnapshot};
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
                    realized_speed: 0.0,
                    accel: 0.0,
                    emergency: false,
                    reconciled: true,
                    front: 2.0,
                    rear: 2.0,
                    junction_state: JunctionState::Cleared,
                    junction_blocker: None,
                    service_phase: crate::diagnostics::ServicePhase::EnRoute,
                    berth_owner: None,
                    service_stop: None,
                    maneuver_phase: crate::diagnostics::ManeuverPhase::Idle,
                    maneuver_target: None,
                    lifecycle: crate::diagnostics::Lifecycle::Active,
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
            let (n, step) = advance_fixed_clock(&mut a, 1.0 / 30.0, dt, MAX_SIM_STEPS);
            assert_eq!(step, dt, "30 fps keeps the fixed step");
            steps_a += n;
        }
        let mut b = 0.0f32;
        let mut steps_b = 0u32;
        for _ in 0..60 {
            let (n, step) = advance_fixed_clock(&mut b, 1.0 / 60.0, dt, MAX_SIM_STEPS);
            assert_eq!(step, dt, "60 fps keeps the fixed step");
            steps_b += n;
        }
        assert_eq!(steps_a, steps_b, "frame partition changed the tick count");
        assert_eq!(steps_a, 50);
        assert!((a - b).abs() < 1e-5);
    }

    #[test]
    fn frames_down_to_25_fps_run_only_fixed_steps() {
        for fps in [25.0f32, 26.0, 30.0, 40.0, 50.0, 60.0, 144.0] {
            let mut accum = 0.0f32;
            for _ in 0..(fps as usize * 10) {
                let (n, step) = advance_fixed_clock(&mut accum, 1.0 / fps, SIM_DT, MAX_SIM_STEPS);
                assert!(n <= MAX_SIM_STEPS);
                assert_eq!(step, SIM_DT, "{fps} fps left the fixed step");
                assert!(accum < SIM_DT + 1e-6, "{fps} fps kept {accum} s of debt");
            }
        }
    }

    #[test]
    fn a_slow_frame_runs_no_more_steps_than_the_cap_and_keeps_its_time() {
        let dt = 0.02f32;
        for frame in [0.07f32, 0.1, 0.16, 0.5, 10.0] {
            let mut accum = 0.004f32;
            let (n, step) = advance_fixed_clock(&mut accum, frame, dt, MAX_SIM_STEPS);
            assert_eq!(n, MAX_SIM_STEPS, "a {frame} s frame must be capped");
            assert_eq!(accum, 0.0, "a capped frame leaves no debt behind");
            assert!(
                (n as f32 * step - (frame + 0.004)).abs() < 1e-5,
                "a {frame} s frame ran {n} x {step} s"
            );
        }
    }

    #[test]
    fn the_traffic_time_keeps_up_with_the_frames_at_any_frame_rate() {
        // A jittery 10-90 fps run: every second the steps taken plus what is left over in the
        // accumulator are exactly the frames' time (to f32 rounding), however slow the frames.
        let mut accum = 0.0f32;
        let (mut stepped, mut elapsed) = (0.0f64, 0.0f64);
        let mut steps_max = 0;
        for k in 0..20_000u32 {
            let frame = 1.0 / (10.0 + (k * 7919 % 81) as f32);
            let (n, step) = advance_fixed_clock(&mut accum, frame, SIM_DT, MAX_SIM_STEPS);
            steps_max = steps_max.max(n);
            stepped += n as f64 * step as f64;
            elapsed += frame as f64;
            let drift = elapsed - (stepped + accum as f64);
            assert!(
                drift.abs() < 1e-3,
                "after {elapsed:.1} s the traffic lost {drift} s"
            );
        }
        assert_eq!(steps_max, MAX_SIM_STEPS);
    }
}
