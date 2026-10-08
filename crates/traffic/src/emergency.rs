//! Cooperative response to an active emergency drive. Requests follow the actual route,
//! not a radius around a siren (a parallel road is not affected).
use crate::{VehicleId, following::AiState, network::Network};

#[derive(Debug, Clone)]
pub struct EmergencyDrive {
    pub vehicle: VehicleId,
    /// Directed route occurrences and distances from the emergency vehicle's origin.
    pub way: Vec<(usize, f32)>,
    pub front: f32,
    pub speed: f32,
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct EmergencyApproach {
    pub vehicle: VehicleId,
    pub gap: f32,
}

/// Legacy AI cars request their active drive through TrafficPriority; scheduled buses
/// use that variable for other purposes. The explicit extension overrides either case.
pub fn emergency_active(priority: bool, scheduled: bool, explicit: Option<bool>) -> bool {
    explicit.unwrap_or(priority && !scheduled)
}

pub fn approaching_emergency(
    net: &Network,
    ego: VehicleId,
    state: &AiState,
    rear: f32,
    drives: &[EmergencyDrive],
) -> Option<EmergencyApproach> {
    let mut nearest: Option<EmergencyApproach> = None;
    for drive in drives.iter().filter(|d| d.vehicle != ego) {
        for &(lane, offset) in &drive.way {
            if lane != state.lane {
                continue;
            }
            let gap = offset + state.s - rear - drive.front;
            // Once its front is well abreast, keep the lane stable while it completes
            // the pass; drop the request after its body has moved beyond this vehicle.
            let horizon = (drive.speed * 6.0 + 20.0).clamp(30.0, 100.0);
            if gap < -rear - 12.0 || gap > horizon {
                continue;
            }
            if net.lanes.get(lane).is_none() {
                continue;
            }
            let r = EmergencyApproach {
                vehicle: drive.vehicle,
                gap,
            };
            if nearest.is_none_or(|n| {
                gap.abs() < n.gap.abs() || (gap.abs() == n.gap.abs() && r.vehicle < n.vehicle)
            }) {
                nearest = Some(r);
            }
            break;
        }
    }
    nearest
}
