//! Engine-side AI car queries (`AiCar`) used by passengers, schedule and LAN.

use super::*;

impl AiCar {
    /// A timetable bus (in service or on its way off after its trip).
    pub fn is_bus(&self) -> bool {
        self.bus.is_some()
    }


    /// Bound to rails (a train, a tram).
    pub fn is_rail(&self) -> bool {
        self.body.kind == MotionKind::Rail
    }


    /// Boarding at a stop: the script is told to open the doors (`AI_Scheduled_AtStation`).
    pub fn at_station(&self) -> bool {
        self.bus.as_ref().map(|b| b.at_station()).unwrap_or(false)
    }


    /// Boarding permission shared with the passenger simulation: true only when the service
    /// owner has docked at a valid berth (`ServicePhase::Boarding`), so passengers and the
    /// bus cannot disagree about whether the stop is being served.
    pub fn boarding_permission(&self) -> bool {
        self.bus
            .as_ref()
            .map(|b| b.state.phase == ServicePhase::Boarding)
            .unwrap_or(false)
    }


    /// The side's doors to open at the stop it is boarding at (`AI_Scheduled_AtStation_Side`).
    pub fn at_station_side(&self) -> f32 {
        self.bus
            .as_ref()
            .map(|b| b.at_station_side())
            .unwrap_or(0.0)
    }


    /// Standing at one of its stops (doors open, waiting for the departure, pulling out).
    pub fn at_stop(&self) -> bool {
        self.bus.as_ref().map(|b| b.at_stop()).unwrap_or(false)
    }


    pub fn trip_done(&self) -> bool {
        self.bus.as_ref().map(|b| b.trip_done()).unwrap_or(false)
    }


    pub fn route_open(&self) -> bool {
        self.bus.as_ref().map(|b| b.route_open).unwrap_or(false)
    }


    /// The next stop: (route index, distance along that lane).
    pub fn next_stop(&self) -> Option<(usize, f32)> {
        self.bus.as_ref()?.stops.front().map(|s| (s.route_index, s.s))
    }


    /// Seconds it will still stand at its stop.
    pub fn standing_for(&self, day_time: f64) -> f32 {
        self.bus
            .as_ref()
            .map(|b| b.standing_for(day_time))
            .unwrap_or(0.0)
    }

}
