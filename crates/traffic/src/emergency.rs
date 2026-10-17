//! Cooperative response to an active emergency drive. Requests follow the actual route,
//! not a radius around a siren (a parallel road is not affected).
use crate::{VehicleId, following::AiState, network::Network};

#[derive(Debug, Clone, Default)]
pub struct EmergencyDrive {
    pub vehicle: VehicleId,
    /// Directed route occurrences and distances from the emergency vehicle's origin.
    pub way: Vec<(usize, f32)>,
    pub front: f32,
    pub speed: f32,
    /// Held up behind slow or standing traffic, or passing it on the oncoming lane: the
    /// stretch of the oncoming lane `pass_reach` metres ahead of its front is kept free.
    pub held: bool,
    pub pass_reach: f32,
}

/// The stretch (m) of the oncoming lane an emergency vehicle held up in a queue may use.
pub const EMERGENCY_PASS_ZONE: f32 = 150.0;
/// How far beyond that stretch (m) oncoming traffic stops for it.
pub const EMERGENCY_ONCOMING_HOLD: f32 = 100.0;

/// Where an oncoming vehicle stops (m ahead of its front) for an emergency vehicle passing
/// a queue on a road with one lane each way: before the stretch it passes along (#462).
/// `None`: none, or it is inside that stretch already and drives on to clear it.
pub fn oncoming_emergency(
    net: &Network,
    ego: VehicleId,
    state: &AiState,
    drives: &[EmergencyDrive],
) -> Option<f32> {
    let lane = net.lanes.get(state.lane)?;
    if lane.left.is_some() || lane.right.is_some() {
        return None;
    }
    let (opp, os, side) = net.opposite(state.lane, state.s.clamp(0.0, lane.length()))?;
    if !(2.3..=5.5).contains(&side) {
        return None;
    }
    let mut best: Option<f32> = None;
    for drive in drives.iter().filter(|d| d.vehicle != ego && d.held) {
        let Some(&(_, offset)) = drive.way.iter().find(|w| w.0 == opp) else {
            continue;
        };
        // how far ahead of the emergency vehicle's front this one's front is
        let ahead = offset + os - state.front - drive.front;
        let left = ahead - drive.pass_reach;
        if left <= 0.0 || left > EMERGENCY_ONCOMING_HOLD {
            continue;
        }
        best = Some(best.map_or(left, |b| b.min(left)));
    }
    best
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
            // A rescue corridor needs both sides of the same carriageway to respond.
            // Authored neighbours count; a merely nearby parallel road does not.
            let mut shared = lane == state.lane;
            for left in [true, false] {
                let mut current = lane;
                for _ in 0..net.lanes.len().min(8) {
                    let Some(l) = net.lanes.get(current) else { break };
                    let next = if left { l.left } else { l.right };
                    let Some(next) = next else { break };
                    if !net.parallel(lane, next) { break; }
                    if next == state.lane { shared = true; break; }
                    current = next;
                }
            }
            if !shared {
                continue;
            }
            let s = if lane == state.lane { state.s } else {
                net.beside_s(state.lane, lane, state.s)
            };
            let gap = offset + s - rear - drive.front;
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
