//! The way between route lanes: joins, detours and gaps.

use super::*;

/// A flight path: aircraft are not tied to the ground under them.
pub(super) fn track_is_air(traffic: &Traffic, lane: usize) -> bool {
    traffic
        .net()
        .lanes
        .get(lane)
        .map(|l| l.kind == ::traffic::LaneKind::Air)
        .unwrap_or(false)
}

/// Where on its route a bus is: the step it is on and how far into it, from the leg it is on
/// (`leg`, `frac` of the way along) and the estimated length of every step (`est`; an absent
/// step has none, so the bus is on the next step there is). None when that is past the end.
pub(super) fn step_at(
    steps: &[Step],
    slots: &[Slot],
    est: &[f64],
    leg: usize,
    frac: f64,
) -> Option<(usize, f64)> {
    let in_leg: Vec<usize> = (0..steps.len()).filter(|&k| steps[k].leg == leg).collect();
    let (mut at, mut offset) = match in_leg.last() {
        // a leg without a station link: the bus is at the start of the next one
        None => (steps.iter().position(|s| s.leg > leg)?, 0.0),
        Some(&last) => {
            let mut target = frac * in_leg.iter().map(|&k| est[k]).sum::<f64>();
            let mut pick = (last, est[last]);
            for &k in &in_leg {
                if est[k] > 0.0 && target <= est[k] {
                    pick = (k, target);
                    break;
                }
                target -= est[k];
            }
            pick
        }
    };
    while slots.get(at) == Some(&Slot::Absent) {
        at += 1;
        offset = 0.0;
    }
    (at < slots.len()).then_some((at, offset))
}

/// The steps around `at` that the network has, up to the steps still to come on either
/// side: (first, end).
pub(super) fn section_around(slots: &[Slot], at: usize) -> (usize, usize) {
    let start = slots[..at]
        .iter()
        .rposition(|s| *s == Slot::Waiting)
        .map(|k| k + 1)
        .unwrap_or(0);
    let end = slots[at..]
        .iter()
        .position(|s| *s == Slot::Waiting)
        .map(|k| at + k)
        .unwrap_or(slots.len());
    (start, end)
}

/// When a bus on its layover at the start of `route` (at `s` on its first lane) leaves for
/// its first stop (`ri`, `ss` on the route) to be there for `depart` (see `STAND_PACE`).
pub(super) fn stand_leave(
    net: &::traffic::Network,
    route: &[usize],
    s: f32,
    (ri, ss): (usize, f32),
    depart: f64,
) -> f64 {
    let mut d = ss - s;
    for k in 0..ri.min(route.len().saturating_sub(1)) {
        if !net.parallel(route[k], route[k + 1]) {
            d += net.lanes[route[k]].length();
        }
    }
    depart - d.max(0.0) as f64 / STAND_PACE - STAND_MARGIN
}
