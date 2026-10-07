//! Scheduled stop targets as physical boarding geometry.
//!
//! A stop is not a point plus a wait timer: it is a boarding region with a platform side,
//! a longitudinal docking position, and a lateral offset. Keeping those separate from the
//! map object's origin lets docking, passenger registration, and the timetable agree.

use crate::capabilities::VehicleCapabilities;
use crate::ids::{LaneId, StopId};
use crate::network::Network;
use glam::DVec3;

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

/// Why a stop could not be compiled into a docking target.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum StopTargetError {
    /// No lane of the route comes within the reach of the stop.
    NotOnRoute,
    /// The stop is on the side of the road the vehicle cannot open its doors on.
    WrongSide,
    /// The docking position lies off the lane (a stop beyond either end).
    Unreachable,
}

/// Compile a stop into a docking target against the occurrence of a route and the
/// serviceable platform side. The result is a physical berth with a departure time, not a
/// point: the caller may reject a malformed or unreachable berth instead of inventing one.
#[allow(clippy::too_many_arguments)]
pub fn compile_stop_target(
    net: &Network,
    route: &[LaneId],
    occurrence: u32,
    stop: StopId,
    at: DVec3,
    side: PlatformSide,
    vehicle: &VehicleCapabilities,
    reach: f32,
    stop_correction: f32,
    depart: f64,
    from: usize,
) -> Result<StopTarget, StopTargetError> {
    let raw: Vec<usize> = route.iter().map(|l| l.index()).collect();
    let (ri, projected_s, lateral) = net
        .project_stop_on_route(&raw, at, Some(reach.max(5.0) as f64), from)
        .ok_or(StopTargetError::NotOnRoute)?;
    // The stop lies right (positive lateral) or left of the travel direction. Whether that
    // is the platform side the vehicle can serve depends on the map's driving side.
    let on_right = lateral > 0.0;
    let right_is_kerb = !net.left_hand;
    match side {
        PlatformSide::Both => {}
        PlatformSide::Right if on_right != right_is_kerb => {
            return Err(StopTargetError::WrongSide)
        }
        PlatformSide::Left if on_right == right_is_kerb => {
            return Err(StopTargetError::WrongSide)
        }
        _ => {}
    }
    let lane = &net.lanes[raw[ri]];
    let s = projected_s - stop_correction;
    if !(0.0..=lane.length()).contains(&s) {
        return Err(StopTargetError::Unreachable);
    }
    let bay = if net.left_hand {
        lateral + vehicle.half_width - 0.3
    } else {
        lateral - vehicle.half_width + 0.3
    };
    Ok(StopTarget::new(stop, occurrence, side, s, bay, depart))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::capabilities::CapabilitySource;
    use crate::network::{LaneBuilder, LaneKind};

    fn caps() -> VehicleCapabilities {
        VehicleCapabilities::from_extents(
            4.0,
            4.0,
            1.25,
            CapabilitySource::BoundingBox,
            2,
            50.0,
            None,
        )
    }

    fn north_lane() -> Network {
        let lane = LaneBuilder::polyline(
            vec![DVec3::new(0.0, 0.0, 0.0), DVec3::new(0.0, 100.0, 0.0)],
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
    fn a_right_side_stop_on_a_right_hand_map_is_serviceable() {
        let net = north_lane();
        let t = compile_stop_target(
            &net,
            &[LaneId(0)],
            0,
            StopId(1),
            DVec3::new(2.0, 50.0, 0.0),
            PlatformSide::Right,
            &caps(),
            20.0,
            0.0,
            36000.0,
            0,
        )
        .expect("a stop beside the kerb");
        assert!((t.s - 50.0).abs() < 0.5, "s={}", t.s);
        assert!(t.bay > 1.0 && t.bay < 2.5, "bay={}", t.bay);
    }

    #[test]
    fn a_stop_on_the_wrong_side_is_reported_not_moved() {
        let net = north_lane();
        let err = compile_stop_target(
            &net,
            &[LaneId(0)],
            0,
            StopId(1),
            DVec3::new(-2.0, 50.0, 0.0),
            PlatformSide::Right,
            &caps(),
            20.0,
            0.0,
            36000.0,
            0,
        )
        .unwrap_err();
        assert_eq!(err, StopTargetError::WrongSide);
    }

    #[test]
    fn a_route_occurrence_is_kept_separate_from_the_geometry() {
        let net = north_lane();
        // The lane twice: from index 1 the projection must pick the second occurrence.
        let route = [LaneId(0), LaneId(0)];
        let t = compile_stop_target(
            &net,
            &route,
            3,
            StopId(2),
            DVec3::new(2.0, 20.0, 0.0),
            PlatformSide::Right,
            &caps(),
            20.0,
            0.0,
            36000.0,
            1,
        )
        .expect("the second occurrence");
        assert_eq!(t.occurrence, 3);
    }

    #[test]
    fn a_stop_off_the_route_is_invalid() {
        let net = north_lane();
        let err = compile_stop_target(
            &net,
            &[LaneId(0)],
            0,
            StopId(1),
            DVec3::new(200.0, 50.0, 0.0),
            PlatformSide::Right,
            &caps(),
            20.0,
            0.0,
            36000.0,
            0,
        )
        .unwrap_err();
        assert_eq!(err, StopTargetError::NotOnRoute);
    }

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
