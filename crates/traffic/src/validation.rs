//! Content validation for a compiled road network.
//!
//! Stage 2 requires wrong or ambiguous topology to be diagnosed at the adapter boundary
//! rather than compensated for inside the decision code. [`Network::validate`] walks the
//! compiled lanes once and returns every defect it can prove, with the lane index as
//! provenance. It never changes the network: decision code and existing tests keep the same
//! geometry; callers report the defects and can reject or repair the content.

use crate::network::{BlockRule, Lane, LaneKey, LaneKind, Network};

/// A provable problem with the compiled network.
#[derive(Debug, Clone, PartialEq)]
pub enum NetworkDefect {
    /// A lane with fewer than two points cannot be interpolated.
    EmptyLane { lane: usize },
    /// A lane with no length.
    ZeroLength { lane: usize },
    /// A lane with no width.
    NonPositiveWidth { lane: usize },
    /// A speed limit outside the plausible range (0, 1000].
    BadSpeedLimit { lane: usize, kmh: f32 },
    /// Two lanes share a map identity and direction.
    DuplicateKey { lane: usize, other: usize, key: LaneKey },
    /// A `[blockpath]` entry whose mode has no established meaning.
    UnresolvedBlockMode { lane: usize, rule: BlockRule },
    /// A `[crossingproblem]` flag whose decision semantics are not established.
    UnresolvedCrossingProblem { lane: usize },
    /// Two successors start at the same point (a duplicate or ambiguous join).
    AmbiguousJoin { lane: usize, other: usize },
}

/// Every defect found in a network, in lane order.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct NetworkDiagnostics {
    pub defects: Vec<NetworkDefect>,
}

impl NetworkDiagnostics {
    pub fn is_clean(&self) -> bool {
        self.defects.is_empty()
    }

    pub fn len(&self) -> usize {
        self.defects.len()
    }

    pub fn is_empty(&self) -> bool {
        self.defects.is_empty()
    }

    /// The defects on one lane.
    pub fn for_lane(&self, lane: usize) -> impl Iterator<Item = &NetworkDefect> {
        self.defects.iter().filter(move |d| defect_lane(d) == Some(lane))
    }
}

fn defect_lane(d: &NetworkDefect) -> Option<usize> {
    match *d {
        NetworkDefect::EmptyLane { lane }
        | NetworkDefect::ZeroLength { lane }
        | NetworkDefect::NonPositiveWidth { lane }
        | NetworkDefect::BadSpeedLimit { lane, .. }
        | NetworkDefect::UnresolvedBlockMode { lane, .. }
        | NetworkDefect::UnresolvedCrossingProblem { lane }
        | NetworkDefect::AmbiguousJoin { lane, .. }
        | NetworkDefect::DuplicateKey { lane, .. } => Some(lane),
    }
}

/// The vertical separation at which two lanes are treated as different roads (m). Callers
/// can compare against the constant `MEET_CLEARANCE` used by conflict compilation.
impl Network {
    /// Find every provable defect in this network.
    pub fn validate(&self) -> NetworkDiagnostics {
        let mut defects = Vec::new();
        let mut seen: std::collections::HashMap<(LaneKey, bool), usize> = std::collections::HashMap::new();

        for (i, l) in self.lanes.iter().enumerate() {
            if l.points.len() < 2 {
                defects.push(NetworkDefect::EmptyLane { lane: i });
                continue;
            }
            let length = l.length();
            if length <= 0.01 {
                defects.push(NetworkDefect::ZeroLength { lane: i });
            }
            if l.width <= 0.0 {
                defects.push(NetworkDefect::NonPositiveWidth { lane: i });
            }
            if !(l.speed_limit_kmh > 0.0 && l.speed_limit_kmh <= 1000.0) {
                defects.push(NetworkDefect::BadSpeedLimit {
                    lane: i,
                    kmh: l.speed_limit_kmh,
                });
            }
            if let Some(key) = l.key {
                if let Some(&other) = seen.get(&(key, l.reversed)) {
                    defects.push(NetworkDefect::DuplicateKey {
                        lane: i,
                        other,
                        key,
                    });
                } else {
                    seen.insert((key, l.reversed), i);
                }
            }
            for rule in &l.blocks {
                if rule.mode != 0 {
                    defects.push(NetworkDefect::UnresolvedBlockMode { lane: i, rule: *rule });
                }
            }
            if l.crossing_problem {
                defects.push(NetworkDefect::UnresolvedCrossingProblem { lane: i });
            }
            // A road leading into the same point twice is a duplicated or ambiguous join.
            for (x, &n) in l.next.iter().enumerate() {
                if n >= self.lanes.len() {
                    continue;
                }
                let ns = self.lanes[n].start();
                for &m in &l.next[x + 1..] {
                    if m >= self.lanes.len() {
                        continue;
                    }
                    if (ns - self.lanes[m].start()).truncate().length() < 0.2 {
                        defects.push(NetworkDefect::AmbiguousJoin {
                            lane: i,
                            other: m,
                        });
                    }
                }
            }
        }
        NetworkDiagnostics { defects }
    }
}

/// A lane that only blocks others through `[blockpath]` on a different kind of path is not
/// a road vehicle's concern; kept here so the intent of the flag stays visible.
pub fn is_road(l: &Lane) -> bool {
    matches!(l.kind, LaneKind::Street)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::network::LaneBuilder;

    fn straight(len: f64) -> Lane {
        LaneBuilder::polyline(
            vec![glam::DVec3::new(0.0, 0.0, 0.0), glam::DVec3::new(0.0, len, 0.0)],
            LaneKind::Street,
            3.0,
        )
    }

    #[test]
    fn a_clean_network_has_no_defects() {
        let net = Network {
            lanes: vec![straight(50.0), straight(40.0)],
            ..Default::default()
        };
        assert!(net.validate().is_clean(), "{:?}", net.validate().defects);
    }

    #[test]
    fn a_degenerate_lane_is_reported() {
        let mut bad = straight(50.0);
        bad.points.truncate(1);
        let net = Network {
            lanes: vec![bad],
            ..Default::default()
        };
        assert!(net
            .validate()
            .defects
            .contains(&NetworkDefect::EmptyLane { lane: 0 }));
    }

    #[test]
    fn an_unresolved_block_mode_is_kept_as_data_and_reported() {
        let mut l = straight(50.0);
        l.blocks.push(BlockRule { path: 1, mode: 3 });
        let net = Network {
            lanes: vec![l],
            ..Default::default()
        };
        let d = net.validate();
        assert!(d.defects.iter().any(|x| matches!(
            x,
            NetworkDefect::UnresolvedBlockMode { rule, .. } if rule.path == 1 && rule.mode == 3
        )));
    }
}
