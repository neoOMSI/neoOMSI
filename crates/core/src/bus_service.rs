//! Timetable buses as traffic: the engine-side adapter over the domain service machine.
//!
//! A timetable bus is one of the town's AI cars (`AiCar`, created by the same
//! `Traffic::create_car` as every other car, driven by the same following, lights, junctions,
//! lane changes and passing). Everything that decides *when* it serves a stop - the service
//! phases, berth assignment, docking eligibility, dwell and the door handshake policy - lives
//! in [`::traffic::service`] (`ServiceCoordinator`). This module keeps only what needs engine
//! access: the trip's compiled [`StopTarget`]s, the destination/displays, the always/early
//! policy, and the mapping of script feedback into the typed [`ScriptFeedback`] the domain
//! understands. The body is still moved by the same following controller; this module only
//! says where to stop and what the service state is.

use ::traffic::{ServicePhase, ServiceState, StopTarget};
use std::collections::VecDeque;

/// The engine-side half of a scheduled bus: the content the domain does not own.
#[derive(Debug, Clone)]
pub struct BusService {
    /// The stops still ahead, front first.
    pub stops: VecDeque<StopTarget>,
    /// The domain service state (single writer: `ServiceCoordinator::plan`).
    pub state: ServiceState,
    /// The terminus of its trip, the name the waiting people read off it.
    pub terminus: String,
    /// The trip's last station: always served.
    pub last_stop: Option<i64>,
    /// Stops the timetable has it serve in any case, and those it serves when it would be
    /// more than `EARLY_STOP_SHORT` early (`schedule::TripTimes::kinds`).
    pub always: Vec<i64>,
    pub serve_early: Vec<i64>,
    /// The timetable still carries the route on as tiles bring their lanes.
    pub route_open: bool,
}

impl BusService {
    pub fn new(stops: Vec<StopTarget>) -> BusService {
        BusService {
            stops: stops.into(),
            state: ServiceState::new(),
            terminus: String::new(),
            last_stop: None,
            always: Vec::new(),
            serve_early: Vec::new(),
            route_open: false,
        }
    }

    /// Doors open for people (`AI_Scheduled_AtStation`).
    pub fn at_station(&self) -> bool {
        self.state.at_station()
    }

    /// The side's doors the bus opens at the stop it is at (`AI_Scheduled_AtStation_Side`):
    /// the front stop's while it boards, else 0 (nobody at a stop, nothing to open).
    pub fn at_station_side(&self) -> f32 {
        if self.state.at_station() {
            self.stops.front().map(|s| s.side.code()).unwrap_or(0.0)
        } else {
            0.0
        }
    }

    pub fn trip_done(&self) -> bool {
        self.state.trip_done()
    }

    /// Standing at a stop (boarding, closing, merging or a layover).
    pub fn at_stop(&self) -> bool {
        self.state.at_stop()
    }

    /// Seconds it expects to stand where it is yet (for the traffic behind: worth going
    /// round, or worth waiting for).
    pub fn standing_for(&self, day_time: f64) -> f32 {
        let wait = (self.state.leave_at - day_time).max(0.0) as f32;
        match self.state.phase {
            ServicePhase::EnRoute | ServicePhase::Approach => 0.0,
            ServicePhase::Boarding => self.state.boarding_t.max(0.0) + 2.0 + wait,
            ServicePhase::Layover => wait + 2.0,
            ServicePhase::ClosingDoors | ServicePhase::WaitingToMerge => 1.0,
            ServicePhase::NextTrip | ServicePhase::OutOfService => 600.0,
            _ => wait + 2.0,
        }
    }

    /// Somebody is still at the doors: keep them open for `secs` more.
    pub fn hold(&mut self, secs: f32) {
        if self.state.phase == ServicePhase::Boarding {
            self.state.boarding_t = self.state.boarding_t.max(secs);
        }
    }

    /// A new trip (the tour's next, or the rest of a trip). The berth is released when the
    /// vehicle leaves it; the coordinator is reset by the caller.
    pub fn restart(&mut self, stops: Vec<StopTarget>, layover: bool) {
        self.state.restart(layover);
        self.stops = stops.into();
    }

    /// The berth of the front stop, if any, against the vehicle's planned route. A stop whose
    /// lane has not been loaded yet has no berth (the service reports `RoutePending`).
    pub fn front_berth(&self, route: &[usize]) -> Option<::traffic::BerthGeometry> {
        let target = self.stops.front()?;
        let lane = route.get(target.route_index).copied()?;
        Some(::traffic::BerthGeometry::from_target(target, lane))
    }

    /// The content-derived stop policy for the front stop.
    pub fn policy(&self) -> ::traffic::StopPolicy {
        ::traffic::StopPolicy {
            always: self.always.clone(),
            serve_early: self.serve_early.clone(),
            last_stop: self.last_stop,
            is_last: self.stops.len() <= 1 && !self.route_open,
            route_open: self.route_open,
        }
    }
}

/// How the script's `AI_Scheduled_AtStation` handshake stands, typed for the domain.
pub fn script_feedback(
    vehicle: &::simulation::VehicleInstance,
    phase: ServicePhase,
) -> ::traffic::ScriptFeedback {
    use ::traffic::ScriptFeedback;
    match phase {
        ServicePhase::ClosingDoors | ServicePhase::WaitingToMerge | ServicePhase::Departing => {
            match vehicle.var("AI_Scheduled_AtStation") {
                // The engine always injects the variable, so an absent one only happens for
                // an adapter that never set the station at all: the fixed-close fallback.
                None => ScriptFeedback::Unsupported,
                // The script wrote 0: doors shut, the stop brake is off.
                Some(v) if v.abs() < 0.5 => ScriptFeedback::Released,
                // The script has not answered. If its doors are reported open, driving off
                // would be provably unsafe; otherwise the state is unknown.
                Some(_) if doors_known_open(vehicle) => ScriptFeedback::StuckDoorsOpen,
                Some(_) => ScriptFeedback::StuckUnknown,
            }
        }
        _ => ScriptFeedback::Idle,
    }
}

/// Whether the vehicle's script reports a door as open (a proven unsafe state to drive off
/// with, as distinct from a script that never reports door state at all).
fn doors_known_open(vehicle: &::simulation::VehicleInstance) -> bool {
    for name in ["door_0", "door0", "PAX_Entry0_Open"] {
        if vehicle.var(name).map(|v| v > 0.5).unwrap_or(false) {
            return true;
        }
    }
    false
}

/// How far before a station's point a vehicle stops with its origin, as Omsi.exe measures
/// the way to the station (0x7da4e3): from the origin of the vehicle that leads, less half
/// its length for a train (its front comes to rest at the station, not its middle - the
/// S-Bahn stopped with half a car past the end of the platform), plus the holding point
/// offset of its `[ai_brakeperformance]` ("to correct unprecise braking").
pub fn stop_shift(ty: &::simulation::VehicleType, rail: bool) -> f32 {
    let hold = ty.def.ai_brake_performance.map(|b| b[4]).unwrap_or(0.0);
    let half = if rail {
        ty.half_length().unwrap_or(0.0)
    } else {
        0.0
    };
    half - hold
}

#[cfg(test)]
mod tests {
    use super::*;
    use ::traffic::{PlatformSide, StopId};

    fn stop(id: i64, depart: f64) -> StopTarget {
        StopTarget::from_tuple((0, 0.0, 0.0, depart, id, 0.0))
    }

    #[test]
    fn standing_time_reflects_the_current_phase() {
        let mut s = BusService::new(vec![]);
        assert_eq!(s.standing_for(0.0), 0.0);
        s.state.phase = ServicePhase::Layover;
        s.state.leave_at = 100.0;
        assert!((s.standing_for(40.0) - 62.0).abs() < 1e-3);
        s.state.phase = ServicePhase::NextTrip;
        assert!(s.standing_for(0.0) > 100.0);
    }

    #[test]
    fn station_side_comes_from_the_stop_it_boards_at() {
        let with_side = |side: f32| StopTarget::from_tuple((0, 0.0, 0.0, 0.0, 1, side));
        let mut s = BusService::new(vec![with_side(1.0)]);
        // off a stop: nothing to open
        s.state.phase = ServicePhase::EnRoute;
        assert_eq!(s.at_station_side(), 0.0);
        // boarding: the front stop's side
        s.state.phase = ServicePhase::Boarding;
        assert_eq!(s.at_station_side(), 1.0);
        // waiting to pull out (doors shut): the side is not asked for any more
        s.state.phase = ServicePhase::ClosingDoors;
        assert_eq!(s.at_station_side(), 0.0);
        // an empty queue answers 0, not a panic
        s.state.phase = ServicePhase::Boarding;
        s.stops.clear();
        assert_eq!(s.at_station_side(), 0.0);
    }

    #[test]
    fn policy_marks_the_last_loaded_stop() {
        let mut s = BusService::new(vec![stop(1, 100.0), stop(2, 200.0)]);
        assert!(!s.policy().is_last);
        s.stops = vec![stop(2, 200.0)].into();
        assert!(s.policy().is_last);
        s.route_open = true;
        assert!(!s.policy().is_last, "an open route is not at its last stop yet");
    }

    #[test]
    fn front_berth_uses_the_route_lane() {
        let s = BusService::new(vec![StopTarget::new(
            StopId(9),
            0,
            1,
            PlatformSide::Right,
            12.0,
            -1.6,
            0.0,
        )]);
        let b = s.front_berth(&[4, 7]).expect("a berth");
        assert_eq!(b.lane, 7);
        assert_eq!(b.stop, StopId(9));
    }
}
