//! Planned route progress.
//!
//! A route can visit the same lane more than once (loops, out-and-back services), so a
//! lane id alone cannot identify a position on a route. A `RouteProgress` names the trip,
//! the directed occurrence of the route, the lane, and the distance along it.

use crate::ids::{LaneId, TripId};

/// A position on a planned route.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct RouteProgress {
    /// The trip this progress belongs to.
    pub trip: TripId,
    /// Which directed occurrence of the route (loops make this necessary).
    pub occurrence: u32,
    /// The lane currently occupied.
    pub lane: LaneId,
    /// Distance along that lane, in travel direction (m).
    pub s: f64,
}

impl RouteProgress {
    pub fn new(trip: TripId, occurrence: u32, lane: LaneId, s: f64) -> RouteProgress {
        RouteProgress {
            trip,
            occurrence,
            lane,
            s,
        }
    }

    /// The same progress on a new lane occurrence.
    pub fn moved_to(self, lane: LaneId, s: f64) -> RouteProgress {
        RouteProgress { lane, s, ..self }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn progress_keeps_the_route_occurrence() {
        let p = RouteProgress::new(TripId(4), 2, LaneId(17), 12.5);
        let q = p.moved_to(LaneId(18), 0.0);
        assert_eq!(q.trip, TripId(4));
        assert_eq!(q.occurrence, 2);
        assert_eq!(q.lane, LaneId(18));
        assert_ne!(p, q);
    }
}
