//! Planned route progress.
//!
//! A route can visit the same lane more than once (loops, out-and-back services), so a
//! lane id alone cannot identify a position on a route. A `RouteProgress` names the trip,
//! the directed occurrence of the route, the lane, and the distance along it.

use crate::ids::{LaneId, TripId};
use crate::network::{LaneKey, Network};

/// Whether a step's tile is loaded, in the map but unloaded, or not in the map.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TileState {
    /// Its lanes are in the network.
    Loaded,
    /// The tile is in the map but its lanes are not loaded yet.
    InMap,
    /// The tile is not part of the map (or the step names no lane at all).
    Unknown,
}

/// One step of a compiled route: the lane chosen, still waiting for its tile, or missing.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RouteStepState {
    /// A loaded lane of the network.
    Lane(LaneId),
    /// The tile is in the map but its lanes are not loaded yet.
    PendingTiles,
    /// No lane and no tile that could bring one.
    Missing,
}

/// A compiled route: one state per step, plus the overall status.
#[derive(Debug, Clone, PartialEq)]
pub struct RouteCompilation {
    pub steps: Vec<RouteStepState>,
    pub status: RouteStatus,
}

impl RouteCompilation {
    /// The loaded lanes of the route, in order.
    pub fn lanes(&self) -> Vec<LaneId> {
        self.steps
            .iter()
            .filter_map(|s| match s {
                RouteStepState::Lane(l) => Some(*l),
                _ => None,
            })
            .collect()
    }

    pub fn waiting(&self) -> usize {
        self.steps
            .iter()
            .filter(|s| matches!(s, RouteStepState::PendingTiles))
            .count()
    }

    pub fn missing(&self) -> usize {
        self.steps
            .iter()
            .filter(|s| matches!(s, RouteStepState::Missing))
            .count()
    }
}

/// Compile a route from the map keys its timetable names.
///
/// Each step resolves to the loaded lane of its key whose direction best joins the lanes
/// before and after it (both directions of a two-way path share a key); a step whose tile is
/// in the map but unloaded becomes `PendingTiles`, and one with no tile becomes `Missing`.
/// Detours (a link that runs on past its station) are dropped. `tile_state` reports what the
/// caller knows of a tile, so the compiler never depends on the map loader.
pub fn compile_route(
    net: &Network,
    keys: &[Option<LaneKey>],
    mut tile_state: impl FnMut((i32, i32)) -> TileState,
    prev: Option<LaneId>,
) -> RouteCompilation {
    let cands: Vec<Result<&Vec<usize>, RouteStepState>> = keys
        .iter()
        .map(|key| {
            let Some(key) = *key else {
                return Err(RouteStepState::Missing);
            };
            match net.by_key.get(&key) {
                Some(c) if !c.is_empty() => Ok(c),
                _ if tile_state(key.tile) == TileState::InMap => Err(RouteStepState::PendingTiles),
                _ => Err(RouteStepState::Missing),
            }
        })
        .collect();
    let mut out = Vec::with_capacity(keys.len());
    let mut last = prev.map(|l| l.index());
    for (i, c) in cands.iter().enumerate() {
        let c = match c {
            Ok(c) => *c,
            Err(slot) => {
                if *slot == RouteStepState::PendingTiles {
                    last = None;
                }
                out.push(*slot);
                continue;
            }
        };
        // the next lanes the route has (not across a gap)
        let next = cands[i + 1..]
            .iter()
            .find_map(|x| match x {
                Ok(n) => Some(Some(*n)),
                Err(RouteStepState::PendingTiles) => Some(None),
                Err(_) => None,
            })
            .flatten();
        let score = |l: usize| -> f64 {
            let mut s = 0.0;
            if let Some(prev) = last {
                s += (net.lanes[l].start() - net.lanes[prev].end()).length();
            }
            if let Some(next) = next {
                let end = net.lanes[l].end();
                s += next
                    .iter()
                    .map(|&n| (net.lanes[n].start() - end).length())
                    .fold(f64::MAX, f64::min);
            }
            s
        };
        let best = c
            .iter()
            .copied()
            .min_by(|a, b| score(*a).total_cmp(&score(*b)))
            .unwrap();
        out.push(RouteStepState::Lane(LaneId(best)));
        last = Some(best);
    }
    skip_detours(net, &mut out);
    let status = if out.iter().any(|s| matches!(s, RouteStepState::PendingTiles)) {
        RouteStatus::PendingTiles
    } else if out.iter().any(|s| matches!(s, RouteStepState::Lane(_))) {
        RouteStatus::Complete
    } else {
        RouteStatus::Invalid
    };
    RouteCompilation { steps: out, status }
}

/// Do lanes `a` and `b` join? A successor, a lane change beside it, or starting (almost)
/// where the first ends.
pub fn joins(net: &Network, a: usize, b: usize) -> bool {
    net.lanes[a].next.contains(&b)
        || net.parallel(a, b)
        || (net.lanes[b].start() - net.lanes[a].end())
            .truncate()
            .length()
            < 2.0
}

/// A station link often runs on past its station: such a detour is passed over (made
/// `Missing`): where the route does not join, the lane a few steps back that the next one
/// continues from - or the lane a few steps on that continues this one - is where the route
/// really goes.
fn skip_detours(net: &Network, slots: &mut [RouteStepState]) {
    const REACH: usize = 8;
    let lane_at = |slots: &[RouteStepState], k: usize| match slots[k] {
        RouteStepState::Lane(l) => Some(l.index()),
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
        let back = (i.saturating_sub(REACH)..i)
            .rev()
            .find(|&k| lane_at(slots, k).map(|x| joins(net, x, b)).unwrap_or(false));
        let on = (i + 2..(i + 2 + REACH).min(slots.len()))
            .find(|&k| lane_at(slots, k).map(|x| joins(net, a, x)).unwrap_or(false));
        match (back, on) {
            (Some(k), Some(m)) if i - k <= m - i - 1 => {
                slots[k + 1..=i].fill(RouteStepState::Missing)
            }
            (_, Some(m)) => slots[i + 1..m].fill(RouteStepState::Missing),
            (Some(k), None) => slots[k + 1..=i].fill(RouteStepState::Missing),
            (None, None) => {}
        }
        i += 1;
    }
}

/// Where consecutive lanes of a route do not join (a path the timetable names that the map
/// no longer has, or a junction a mod map edited after its tracks were made), the shortest
/// way between them through the network, when there is one not much longer than the gap.
/// Returns the lanes and, for each lane given, its index in them.
pub fn bridge_gaps(net: &Network, lanes: &[usize]) -> (Vec<usize>, Vec<usize>) {
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
pub fn way_between(net: &Network, a: usize, b: usize, max: f32) -> Option<Vec<usize>> {
    use std::cmp::Reverse;
    use std::collections::HashMap;
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
fn ordered(m: f32) -> u64 {
    (m.max(0.0) * 1000.0) as u64
}

/// The outcome of compiling a timetable trip onto the loaded network.
///
/// `Invalid` and `PendingTiles` are different states: an invalid trip has no loaded lane
/// and no tile that could bring one, while a pending trip is still waiting for its tiles.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RouteStatus {
    /// Every step resolved to a lane.
    Complete,
    /// Some step's tile is in the map but its lanes are not loaded yet.
    PendingTiles,
    /// No step could be resolved: the trip does not exist or its content is missing.
    Invalid,
}

impl RouteStatus {
    pub fn is_complete(self) -> bool {
        self == RouteStatus::Complete
    }

    pub fn is_invalid(self) -> bool {
        self == RouteStatus::Invalid
    }

    pub fn is_pending(self) -> bool {
        self == RouteStatus::PendingTiles
    }
}

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
    fn a_looped_route_keeps_each_occurrence() {
        use crate::network::{LaneBuilder, LaneKind};
        use glam::DVec3;
        let key = LaneKey {
            tile: (0, 0),
            id: 1,
            path: 0,
        };
        let lane = LaneBuilder::polyline(
            vec![DVec3::new(0.0, 0.0, 0.0), DVec3::new(0.0, 50.0, 0.0)],
            LaneKind::Street,
            3.0,
        );
        let mut lane = lane;
        lane.key = Some(key);
        let mut net = Network {
            lanes: vec![lane],
            ..Default::default()
        };
        net.link(1.5);
        // The same lane twice: two steps, each naming it (a loop).
        let comp = compile_route(&net, &[Some(key), Some(key)], |_| TileState::Unknown, None);
        assert_eq!(comp.status, RouteStatus::Complete);
        assert_eq!(comp.lanes(), vec![LaneId(0), LaneId(0)]);
    }

    #[test]
    fn pending_tiles_and_missing_steps_are_different() {
        use crate::network::{LaneBuilder, LaneKind};
        use glam::DVec3;
        let in_map = LaneKey {
            tile: (0, 0),
            id: 1,
            path: 0,
        };
        let elsewhere = LaneKey {
            tile: (9, 9),
            id: 2,
            path: 0,
        };
        let lane = LaneBuilder::polyline(
            vec![DVec3::new(0.0, 0.0, 0.0), DVec3::new(0.0, 50.0, 0.0)],
            LaneKind::Street,
            3.0,
        );
        let net = Network {
            lanes: vec![lane],
            ..Default::default()
        };
        let comp = compile_route(
            &net,
            &[Some(in_map), Some(elsewhere)],
            |tile| {
                if tile == (0, 0) {
                    TileState::InMap
                } else {
                    TileState::Unknown
                }
            },
            None,
        );
        assert_eq!(comp.status, RouteStatus::PendingTiles);
        assert_eq!(comp.waiting(), 1);
        assert_eq!(comp.missing(), 1);
        // No step at all: invalid, not pending.
        let none = compile_route(&net, &[None], |_| TileState::Unknown, None);
        assert_eq!(none.status, RouteStatus::Invalid);
    }

    #[test]
    fn bridge_gaps_inserts_the_way_between_unjoined_lanes() {
        use crate::network::{LaneBuilder, LaneKind};
        use glam::DVec3;
        let mk = |a: f64, b: f64| {
            LaneBuilder::polyline(
                vec![DVec3::new(0.0, a, 0.0), DVec3::new(0.0, b, 0.0)],
                LaneKind::Street,
                3.0,
            )
        };
        let mut net = Network {
            lanes: vec![mk(0.0, 10.0), mk(10.0, 20.0), mk(20.0, 30.0)],
            ..Default::default()
        };
        net.link(1.5);
        // 0 -> 1 -> 2: 0 and 2 do not join, so the way between them is lane 1.
        let (lanes, index) = bridge_gaps(&net, &[0, 2]);
        assert_eq!(lanes, vec![0, 1, 2]);
        assert_eq!(index, vec![0, 2]);
    }

    #[test]
    fn route_status_distinguishes_pending_from_invalid() {
        assert!(RouteStatus::Complete.is_complete());
        assert!(!RouteStatus::Complete.is_invalid());
        assert!(RouteStatus::PendingTiles.is_pending());
        assert!(!RouteStatus::PendingTiles.is_invalid());
        assert!(RouteStatus::Invalid.is_invalid());
        assert_ne!(RouteStatus::PendingTiles, RouteStatus::Invalid);
    }

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
