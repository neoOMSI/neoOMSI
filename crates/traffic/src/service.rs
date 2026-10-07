//! Scheduled stop targets as physical boarding geometry.
//!
//! A stop is not a point plus a wait timer: it is a boarding region with a platform side,
//! a longitudinal docking position, and a lateral offset. Keeping those separate from the
//! map object's origin lets docking, passenger registration, and the timetable agree.

use crate::ids::StopId;

/// The side of the road the platform lies on.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PlatformSide {
    /// Right-hand side of the travel direction.
    Right,
    /// The other side (left-hand traffic or an island).
    Left,
    /// Doors on both sides.
    Both,
}

impl PlatformSide {
    /// The map's stop-side code: 0 right, 1 the other, 2 both.
    pub fn from_code(code: f32) -> PlatformSide {
        match code as i32 {
            1 => PlatformSide::Left,
            2 => PlatformSide::Both,
            _ => PlatformSide::Right,
        }
    }

    pub fn code(self) -> f32 {
        match self {
            PlatformSide::Right => 0.0,
            PlatformSide::Left => 1.0,
            PlatformSide::Both => 2.0,
        }
    }
}

/// A specific occurrence of a stop on a route, with its docking geometry.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct StopTarget {
    /// The stop's map object.
    pub stop: StopId,
    /// Which directed occurrence of the route (loops can serve a stop twice).
    pub occurrence: u32,
    pub side: PlatformSide,
    /// Where the front of the vehicle comes to rest along the lane (m).
    pub s: f32,
    /// Lateral offset of the berth from the lane centre (m, positive = right).
    pub bay: f32,
    /// Timetable departure (seconds of the day).
    pub depart: f64,
}

impl StopTarget {
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        stop: StopId,
        occurrence: u32,
        side: PlatformSide,
        s: f32,
        bay: f32,
        depart: f64,
    ) -> StopTarget {
        StopTarget {
            stop,
            occurrence,
            side,
            s,
            bay,
            depart,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn platform_side_round_trips_through_its_code() {
        for code in [0.0f32, 1.0, 2.0] {
            assert_eq!(PlatformSide::from_code(code).code(), code);
        }
    }

    #[test]
    fn a_stop_target_keeps_occurrence_and_geometry_separate() {
        let t = StopTarget::new(StopId(99), 3, PlatformSide::Left, 42.0, -1.8, 36000.0);
        assert_eq!(t.stop, StopId(99));
        assert_eq!(t.occurrence, 3);
        assert_eq!(t.side, PlatformSide::Left);
        assert_eq!(t.s, 42.0);
        assert_eq!(t.bay, -1.8);
    }
}
