//! The way between route lanes: joins, detours and gaps.

use super::*;

/// Consecutive route lanes that a vehicle can drive from one into the other: linked, a lane
/// change beside it, or starting (almost) where the first ends.
pub(super) fn joins(net: &Network, a: usize, b: usize) -> bool {
    net.lanes[a].next.contains(&b)
        || net.parallel(a, b)
        || (net.lanes[b].start() - net.lanes[a].end())
        .truncate()
        .length()
        < 2.0
}

/// A station link often runs on past its station: the path search that made it went a
/// few paths beyond the stop - into a turning lane, round a corner - before the next link
/// starts back at the stop on another path (Spandau's links end so in 122 of 505 joins, the
/// extra paths mostly listed with length 0). Driven as listed, the bus turned off, then
/// jumped back and drove on the wrong side or against the traffic. Such a detour is passed
/// over (made `Absent`): where the route does not join, the lane a few steps back that
/// the next one continues from - or the lane a few steps on that continues this one - is
/// where the route really goes.
pub(super) fn skip_detours(net: &Network, slots: &mut [Slot]) {
    const REACH: usize = 8;
    let lane_at = |slots: &[Slot], k: usize| match slots[k] {
        Slot::Lane(l) => Some(l),
        _ => None,
    };
    let mut i = 0;
    while i + 1 < slots.len() {
        let (Some(a), Some(b)) = (lane_at(slots, i), lane_at(slots, i + 1)) else {
            i += 1;
            continue;
        };
        if joins(net, a, b) {
            i += 1;
            continue;
        }
        // back: an earlier lane of the route that `b` continues
        let back = (i.saturating_sub(REACH)..i)
            .rev()
            .find(|&k| lane_at(slots, k).map(|x| joins(net, x, b)).unwrap_or(false));
        // on: a later lane that continues `a`
        let on = (i + 2..(i + 2 + REACH).min(slots.len()))
            .find(|&k| lane_at(slots, k).map(|x| joins(net, a, x)).unwrap_or(false));
        match (back, on) {
            (Some(k), Some(m)) if i - k <= m - i - 1 => slots[k + 1..=i].fill(Slot::Absent),
            (_, Some(m)) => slots[i + 1..m].fill(Slot::Absent),
            (Some(k), None) => slots[k + 1..=i].fill(Slot::Absent),
            (None, None) => {}
        }
        i += 1;
    }
}

/// Where consecutive lanes of a route do not join (a path the timetable file names that
/// the map does not have any more, a junction a mod map edited after its tracks were
/// made), the shortest way between them through the network, when there is one not much
/// longer than the gap: the bus drives it instead of jumping across. Returns the lanes and,
/// for each lane given, its index in them.
pub(super) fn bridge_gaps(net: &Network, lanes: &[usize]) -> (Vec<usize>, Vec<usize>) {
    let mut out: Vec<usize> = Vec::with_capacity(lanes.len());
    let mut index = Vec::with_capacity(lanes.len());
    for (k, &b) in lanes.iter().enumerate() {
        if k > 0 {
            let a = lanes[k - 1];
            if !joins(net, a, b) {
                let gap = (net.lanes[b].start() - net.lanes[a].end())
                    .truncate()
                    .length();
                if let Some(way) = way_between(net, a, b, (gap * 2.5 + 60.0) as f32) {
                    out.extend(way);
                }
            }
        }
        index.push(out.len());
        out.push(b);
    }
    (out, index)
}

/// The lanes strictly between `a` and `b` on the shortest way from the end of `a` to the
/// start of `b`, if that is at most `max` metres long.
pub(super) fn way_between(net: &Network, a: usize, b: usize, max: f32) -> Option<Vec<usize>> {
    use std::cmp::Reverse;
    let mut best: HashMap<usize, (f32, usize)> = HashMap::new();
    let mut heap = std::collections::BinaryHeap::new();
    for &n in &net.lanes[a].next {
        heap.push((Reverse(ordered(0.0)), n, a));
    }
    while let Some((Reverse(c), l, from)) = heap.pop() {
        let c = c as f32 / 1000.0;
        if best.contains_key(&l) {
            continue;
        }
        best.insert(l, (c, from));
        if l == b {
            let mut way = Vec::new();
            let mut at = from;
            while at != a {
                way.push(at);
                at = best.get(&at)?.1;
            }
            way.reverse();
            return Some(way);
        }
        let c2 = c + net.lanes[l].length();
        if c2 > max {
            continue;
        }
        for &n in &net.lanes[l].next {
            if !best.contains_key(&n) {
                heap.push((Reverse(ordered(c2)), n, l));
            }
        }
    }
    None
}

/// A distance in millimetres, for ordering.
pub(super) fn ordered(m: f32) -> u64 {
    (m.max(0.0) * 1000.0) as u64
}

/// A flight path: aircraft are not tied to the ground under them.
pub(super) fn track_is_air(traffic: &Traffic, lane: usize) -> bool {
    traffic
        .net
        .lanes
        .get(lane)
        .map(|l| l.kind == ::simulation::traffic::LaneKind::Air)
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
