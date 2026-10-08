//! Occupancy and route-relative perception.
//!
//! Every planner reasons about the same physical world through one immutable per-tick
//! [`Occupancy`]. The indexes (per-lane intervals and a spatial grid) are built once from
//! realized bodies and never replace those bodies: the realized geometry is the truth, a
//! lane projection only accelerates a query. Bodies that span more than one lane (a lane
//! change, the rear still on the lane it left), articulated rear sections, and trailers all
//! carry the same stable [`VehicleId`]; trailers and rear sections differ only by `part`.
//!
//! Coordinate conventions:
//! - A [`Placement`] gives the distance `s` along a lane of the vehicle origin and the
//!   signed lateral offset `lateral` (m, positive to the right of the lane direction).
//! - A [`LaneInterval`] additionally carries the front/rear bumper positions along the
//!   lane, so a leader gap is always "from my front bumper to the blocker's rear bumper"
//!   along the planned way, in metres.
//! - Height is a range `z0..z1`: a bridge body above the road does not occupy the road.

use crate::ids::{LaneId, NetworkVersion, VehicleId};
use crate::network::Network;
use glam::{DVec2, DVec3};
use hashbrown::HashMap;

/// Part index of a vehicle's primary body. Trailers and articulated rear sections are 1..
pub const PART_BODY: u16 = 0;

/// Grid cell size of the spatial index (m). Matches `network::GRID_CELL`.
const CELL: f64 = 50.0;

/// Where a realized body projects onto the lane network. This is only a query accelerator:
/// [`BodyFootprint::center`] and its extents are the occupancy truth.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Placement {
    pub lane: LaneId,
    /// Distance along the lane of the vehicle origin (m).
    pub s: f32,
    /// Signed lateral offset of the origin from the lane centre (m, + = right).
    pub lateral: f32,
    /// The oncoming lane a passing vehicle stands on: a foreign body, ordered by the lane.
    pub foreign: bool,
}

/// A realized body seen by perception: a primary body, an articulated rear section, or a
/// trailer, with a height range. `owner` is stable across container reordering; parts of
/// one vehicle share it.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct BodyFootprint {
    pub owner: VehicleId,
    pub part: u16,
    pub center: DVec2,
    pub fwd: DVec2,
    pub right: DVec2,
    pub half_len: f64,
    pub half_w: f64,
    /// Bottom of the body (world z).
    pub z0: f64,
    /// Top of the body (world z).
    pub z1: f64,
    pub speed: f32,
    pub acc: f32,
    /// Origin to front bumper (m, positive).
    pub front: f32,
    /// Origin to rear bumper (m, positive).
    pub rear: f32,
    /// The lane it is on now.
    pub current: Option<Placement>,
    /// The lane its rear still stands on.
    pub prev: Option<Placement>,
    /// The lane a lane change is moving over to.
    pub crossing: Option<Placement>,
    /// The oncoming lane a passing vehicle stands on (a foreign body).
    pub passing: Option<Placement>,
}

impl BodyFootprint {
    /// A primary body at `center` facing `fwd`, with a height range.
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        owner: VehicleId,
        center: DVec2,
        fwd: DVec2,
        half_len: f64,
        half_w: f64,
        z0: f64,
        z1: f64,
        speed: f32,
    ) -> BodyFootprint {
        let fwd = fwd.normalize_or_zero();
        BodyFootprint {
            owner,
            part: PART_BODY,
            center,
            fwd,
            right: DVec2::new(fwd.y, -fwd.x),
            half_len,
            half_w,
            z0,
            z1,
            speed,
            acc: 0.0,
            front: half_len as f32,
            rear: half_len as f32,
            current: None,
            prev: None,
            crossing: None,
            passing: None,
        }
    }

    /// A part of an existing vehicle (rear section, trailer), sharing the owner id.
    pub fn part_of(&self, part: u16, center: DVec2, fwd: DVec2, half_len: f64, half_w: f64) -> BodyFootprint {
        let mut f = *self;
        f.part = part;
        f.center = center;
        f.fwd = fwd.normalize_or_zero();
        f.right = DVec2::new(f.fwd.y, -f.fwd.x);
        f.half_len = half_len;
        f.half_w = half_w;
        f.front = half_len as f32;
        f.rear = half_len as f32;
        f.current = None;
        f.prev = None;
        f.crossing = None;
        f.passing = None;
        f
    }

    pub fn with_acc(mut self, acc: f32) -> BodyFootprint {
        self.acc = acc;
        self
    }

    /// All lane placements this body claims (now, previous lane, lane-change target,
    /// passing).
    pub fn placements(&self) -> impl Iterator<Item = &Placement> {
        [
            self.current.as_ref(),
            self.prev.as_ref(),
            self.crossing.as_ref(),
            self.passing.as_ref(),
        ]
        .into_iter()
        .flatten()
    }

    /// Do the height ranges overlap? A body over another (a bridge) does not occupy it.
    pub fn z_overlaps(&self, other: &BodyFootprint) -> bool {
        self.z0 < other.z1 && other.z0 < self.z1
    }

    /// Does a probe range `[z, z + height]` reach this body?
    pub fn reaches_z(&self, z: f64, height: f64) -> bool {
        self.z0 <= z + height && z <= self.z1
    }

    /// Separating-axis overlap with `other`, both grown by `margin`; height-aware.
    pub fn overlaps(&self, other: &BodyFootprint, margin: f64) -> bool {
        if !self.z_overlaps(other) {
            return false;
        }
        let d = other.center - self.center;
        for axis in [self.fwd, self.right, other.fwd, other.right] {
            let extent = |f: &BodyFootprint| {
                (f.fwd.dot(axis)).abs() * (f.half_len + margin)
                    + (f.right.dot(axis)).abs() * (f.half_w + margin)
            };
            if d.dot(axis).abs() > extent(self) + extent(other) {
                return false;
            }
        }
        true
    }
}

/// One occupancy interval along a lane.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct LaneInterval {
    pub owner: VehicleId,
    pub part: u16,
    /// Distance along the lane of the vehicle origin (m).
    pub s: f32,
    /// Front bumper position along the lane (m).
    pub front: f32,
    /// Rear bumper position along the lane (m).
    pub rear: f32,
    pub lateral: f32,
    /// A passing vehicle counted on the oncoming lane.
    pub foreign: bool,
    pub speed: f32,
    pub acc: f32,
    pub z0: f64,
    pub z1: f64,
}

/// A vehicle observed ahead of another: the gap is front bumper to rear bumper along the
/// observer's planned way (m).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Observation {
    pub owner: VehicleId,
    pub part: u16,
    pub gap: f32,
    pub speed: f32,
    pub acc: f32,
}

/// One sample of a swept corridor: a point of the observer's way, its distance from the
/// observer origin, and the way direction there.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct SweepSample {
    pub p: DVec3,
    pub d: f32,
    pub dir: DVec2,
}

/// A body met by a swept clearance.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Sweep {
    pub owner: VehicleId,
    pub part: u16,
    /// Distance from the observer origin at which the way meets the body (m).
    pub d: f32,
    /// The body's speed along its own heading (m/s).
    pub speed: f32,
}

/// One frozen tick of realized bodies and the indexes over them.
#[derive(Debug, Clone, Default)]
pub struct Occupancy {
    pub tick: u64,
    pub version: NetworkVersion,
    feet: Vec<BodyFootprint>,
    by_lane: HashMap<LaneId, Vec<LaneInterval>>,
    grid: HashMap<(i32, i32), Vec<usize>>,
}

fn cell(p: DVec2) -> (i32, i32) {
    ((p.x / CELL).floor() as i32, (p.y / CELL).floor() as i32)
}

impl Occupancy {
    /// Build both indexes once from the realized bodies. Lane intervals are sorted by
    /// position so queries do not depend on how the bodies were ordered by the container.
    pub fn build(version: NetworkVersion, tick: u64, feet: Vec<BodyFootprint>) -> Occupancy {
        let mut by_lane: HashMap<LaneId, Vec<LaneInterval>> = HashMap::new();
        let mut grid: HashMap<(i32, i32), Vec<usize>> = HashMap::new();
        for (i, f) in feet.iter().enumerate() {
            for p in f.placements() {
                by_lane.entry(p.lane).or_default().push(LaneInterval {
                    owner: f.owner,
                    part: f.part,
                    s: p.s,
                    front: p.s + f.front,
                    rear: p.s - f.rear,
                    lateral: p.lateral,
                    foreign: p.foreign,
                    speed: f.speed,
                    acc: f.acc,
                    z0: f.z0,
                    z1: f.z1,
                });
            }
            grid.entry(cell(f.center)).or_default().push(i);
        }
        for v in by_lane.values_mut() {
            v.sort_by(|a, b| a.s.total_cmp(&b.s));
        }
        Occupancy {
            tick,
            version,
            feet,
            by_lane,
            grid,
        }
    }

    pub fn feet(&self) -> &[BodyFootprint] {
        &self.feet
    }

    pub fn lane_intervals(&self) -> &HashMap<LaneId, Vec<LaneInterval>> {
        &self.by_lane
    }

    /// The intervals on `lane`, sorted by position.
    pub fn intervals(&self, lane: LaneId) -> &[LaneInterval] {
        self.by_lane.get(&lane).map(|v| v.as_slice()).unwrap_or(&[])
    }

    /// The legacy flat view of the lane index: `(container index, s, lateral, foreign)`.
    /// Callers that still speak container indices build it from stable ids once per tick.
    pub fn lane_view(
        &self,
        index_of: &HashMap<VehicleId, usize>,
    ) -> HashMap<usize, Vec<(usize, f32, f32, bool)>> {
        self.by_lane
            .iter()
            .map(|(lane, ivs)| {
                (
                    lane.index(),
                    ivs.iter()
                        .filter_map(|iv| {
                            index_of
                                .get(&iv.owner)
                                .map(|&j| (j, iv.s, iv.lateral, iv.foreign))
                        })
                        .collect(),
                )
            })
            .collect()
    }

    /// Visit every realized body whose centre lies within `radius` of `p`.
    pub fn near(&self, p: DVec2, radius: f64, mut visit: impl FnMut(&BodyFootprint)) {
        let r = radius.max(0.0);
        let (cx, cy) = cell(p);
        let reach = (r / CELL).ceil() as i32 + 1;
        for dx in -reach..=reach {
            for dy in -reach..=reach {
                let Some(list) = self.grid.get(&(cx + dx, cy + dy)) else {
                    continue;
                };
                for &i in list {
                    let f = &self.feet[i];
                    if (f.center - p).length() <= r + f.half_len.max(f.half_w) {
                        visit(f);
                    }
                }
            }
        }
    }

    /// The nearest body ahead of `p` along `lane` whose height reaches the probe, together
    /// with its speed. `p` is the observer's front bumper position along the lane.
    pub fn nearest_ahead(
        &self,
        lane: LaneId,
        front_s: f32,
        probe_z: f64,
        probe_height: f64,
        foreign: bool,
    ) -> Option<Observation> {
        let mut best: Option<Observation> = None;
        for iv in self.intervals(lane) {
            if iv.foreign != foreign || !(iv.z0 <= probe_z + probe_height && probe_z <= iv.z1) {
                continue;
            }
            let gap = iv.rear - front_s;
            if gap < 0.0 {
                continue;
            }
            if best.map(|b| gap < b.gap).unwrap_or(true) {
                best = Some(Observation {
                    owner: iv.owner,
                    part: iv.part,
                    gap,
                    speed: iv.speed,
                    acc: iv.acc,
                });
            }
        }
        best
    }

    /// The first body a swept corridor meets, walking the samples in order. `half_width`
    /// is the observer's half width (the corridor is grown round each sample).
    pub fn swept_clearance(
        &self,
        samples: &[SweepSample],
        half_width: f64,
        ignore: &[VehicleId],
    ) -> Option<Sweep> {
        for s in samples {
            let p2 = s.p.truncate();
            let across = DVec2::new(s.dir.y, -s.dir.x);
            let mut hit: Option<Sweep> = None;
            self.near(p2, 8.0 + half_width, |f| {
                if ignore.contains(&f.owner) || !f.reaches_z(s.p.z, 2.0) {
                    return;
                }
                let rel = p2 - f.center;
                let gx = f.half_w + half_width * across.dot(f.right).abs() - 0.1;
                let gy = f.half_len + half_width * across.dot(f.fwd).abs() - 0.1;
                if rel.dot(f.right).abs() <= gx && rel.dot(f.fwd).abs() <= gy {
                    let cand = Sweep {
                        owner: f.owner,
                        part: f.part,
                        d: s.d,
                        speed: f.speed,
                    };
                    if hit.map(|h| cand.d < h.d).unwrap_or(true) {
                        hit = Some(cand);
                    }
                }
            });
            if hit.is_some() {
                return hit;
            }
        }
        None
    }

    /// The first body on foot (`people`, world positions) the swept corridor meets.
    pub fn pedestrian_clearance(
        &self,
        samples: &[SweepSample],
        half_width: f64,
        people: &[DVec2],
        person_radius: f64,
    ) -> Option<(DVec2, f32)> {
        for s in samples {
            let p2 = s.p.truncate();
            for &person in people {
                if (person - p2).length() <= half_width + person_radius {
                    return Some((person, s.d));
                }
            }
        }
        None
    }

    /// Who occupies the berth centred at `center_s` on `lane` (its half length), if anyone?
    pub fn berth_occupancy(
        &self,
        lane: LaneId,
        center_s: f32,
        half_len: f32,
        ignore: &[VehicleId],
    ) -> Option<VehicleId> {
        let lo = center_s - half_len;
        let hi = center_s + half_len;
        self.intervals(lane)
            .iter()
            .filter(|iv| !ignore.contains(&iv.owner))
            .find(|iv| iv.front > lo && iv.rear < hi)
            .map(|iv| iv.owner)
    }

    /// The nearest body approaching a crossing on `lane` before `at_s`, for a look of
    /// `look` metres.
    pub fn crossing_approach(
        &self,
        lane: LaneId,
        at_s: f32,
        look: f32,
        foreign: bool,
    ) -> Option<Observation> {
        let mut best: Option<Observation> = None;
        for iv in self.intervals(lane) {
            if iv.foreign != foreign {
                continue;
            }
            let gap = at_s - iv.front;
            if gap < 0.0 || gap > look {
                continue;
            }
            if best.map(|b| gap < b.gap).unwrap_or(true) {
                best = Some(Observation {
                    owner: iv.owner,
                    part: iv.part,
                    gap,
                    speed: iv.speed,
                    acc: iv.acc,
                });
            }
        }
        best
    }
}

/// A reconciled position on the planned route: the lane, the distance along it, and the
/// lateral offset of the realized body from the lane centre.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct RouteFix {
    pub lane: LaneId,
    pub s: f32,
    pub lateral: f32,
}

/// Reconcile a realized pose with controller progress locally on the planned route.
///
/// The body is projected onto `route`, but the projection is accepted only when it lies on
/// the route itself: close to the lane centre and roughly matching the lane heading. A
/// projection that is off to the side (a nearby parallel road) or faces another way is
/// rejected rather than snapped onto, so progress never jumps between streets.
#[allow(clippy::too_many_arguments)]
pub fn project_on_route_local(
    net: &Network,
    route: &[LaneId],
    pose: DVec3,
    heading_deg: f32,
    half_width: f64,
    max_lateral: f64,
    max_heading_deg: f32,
) -> Option<RouteFix> {
    if route.is_empty() {
        return None;
    }
    let raw: Vec<usize> = route.iter().map(|l| l.index()).collect();
    project_on_route_indices(
        net,
        &raw,
        pose,
        heading_deg,
        half_width,
        max_lateral,
        max_heading_deg,
    )
}

/// [`project_on_route_local`] for a route already given as lane indices, so the
/// realization-feedback path does not allocate per tick.
#[allow(clippy::too_many_arguments)]
pub fn project_on_route_indices(
    net: &Network,
    route: &[usize],
    pose: DVec3,
    heading_deg: f32,
    half_width: f64,
    max_lateral: f64,
    max_heading_deg: f32,
) -> Option<RouteFix> {
    if route.is_empty() {
        return None;
    }
    let (ri, s, lateral) = net.project_on_route_lateral(route, pose)?;
    let lane = LaneId(route[ri]);
    if lateral.abs() as f64 > half_width + max_lateral {
        return None;
    }
    let (_, lane_heading) = net.lanes[lane.index()].at(s);
    let turn = ((lane_heading - heading_deg) as f64 + 540.0).rem_euclid(360.0) - 180.0;
    if turn.abs() > max_heading_deg as f64 {
        return None;
    }
    Some(RouteFix {
        lane,
        s,
        lateral,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::network::LaneBuilder;
    use glam::DVec3;

    fn straight() -> Network {
        let lane = LaneBuilder::polyline(
            vec![DVec3::new(0.0, 0.0, 0.0), DVec3::new(0.0, 200.0, 0.0)],
            crate::network::LaneKind::Street,
            3.0,
        );
        let mut net = Network {
            lanes: vec![lane],
            ..Default::default()
        };
        net.link(1.5);
        net
    }

    fn body(id: u64, y: f64, z0: f64, z1: f64) -> BodyFootprint {
        let mut f = BodyFootprint::new(
            VehicleId(id),
            DVec2::new(0.0, y),
            DVec2::new(0.0, 1.0),
            2.25,
            1.25,
            z0,
            z1,
            5.0,
        );
        f.current = Some(Placement {
            lane: LaneId(0),
            s: y as f32,
            lateral: 0.0,
            foreign: false,
        });
        f
    }

    #[test]
    fn the_index_is_sorted_so_container_order_does_not_matter() {
        let a = body(1, 10.0, 0.0, 3.0);
        let b = body(2, 30.0, 0.0, 3.0);
        let first = Occupancy::build(NetworkVersion(1), 2, vec![a, b]);
        let second = Occupancy::build(NetworkVersion(1), 2, vec![b, a]);
        assert_eq!(
            first.intervals(LaneId(0)),
            second.intervals(LaneId(0)),
            "the lane interval order followed the container"
        );
        assert_eq!(
            first.nearest_ahead(LaneId(0), 5.0, 0.0, 2.0, false),
            second.nearest_ahead(LaneId(0), 5.0, 0.0, 2.0, false)
        );
    }

    #[test]
    fn a_bridge_body_does_not_occupy_the_road_below() {
        let on_road = body(1, 20.0, 0.0, 3.0);
        let bridge = body(2, 20.0, 6.0, 9.0);
        let occ = Occupancy::build(NetworkVersion(1), 0, vec![on_road, bridge]);
        // The road body is seen; the bridge above it is not on the sweep of a road car.
        let samples = [SweepSample {
            p: DVec3::new(0.0, 21.0, 0.0),
            d: 1.0,
            dir: DVec2::new(0.0, 1.0),
        }];
        let hit = occ.swept_clearance(&samples, 1.25, &[]).unwrap();
        assert_eq!(hit.owner, VehicleId(1));
    }

    #[test]
    fn a_trailer_part_is_perceived_with_its_owner() {
        let tow = body(7, 10.0, 0.0, 3.0);
        let trailer = tow.part_of(
            1,
            DVec2::new(0.0, 5.0),
            DVec2::new(0.0, 1.0),
            3.0,
            1.25,
        );
        let occ = Occupancy::build(NetworkVersion(1), 0, vec![tow, trailer]);
        let samples = [SweepSample {
            p: DVec3::new(0.0, 6.0, 0.0),
            d: 1.0,
            dir: DVec2::new(0.0, 1.0),
        }];
        let hit = occ.swept_clearance(&samples, 1.25, &[]).unwrap();
        assert_eq!(hit.owner, VehicleId(7));
        assert_eq!(hit.part, 1);
    }

    #[test]
    fn a_projection_off_to_a_parallel_road_is_rejected() {
        let net = straight();
        // A body 4 m to the side of the route: too far, so it is not snapped onto it.
        let fix = project_on_route_local(
            &net,
            &[LaneId(0)],
            DVec3::new(4.0, 50.0, 0.0),
            0.0,
            1.25,
            1.0,
            60.0,
        );
        assert!(fix.is_none(), "a parallel body must not be snapped to the route");
        // The same body on the route is accepted.
        let fix = project_on_route_local(
            &net,
            &[LaneId(0)],
            DVec3::new(0.2, 50.0, 0.0),
            0.0,
            1.25,
            1.0,
            60.0,
        )
        .unwrap();
        assert_eq!(fix.lane, LaneId(0));
        assert!((fix.s - 50.0).abs() < 0.5);
    }

    #[test]
    fn the_index_projection_agrees_with_the_lane_id_projection() {
        let net = straight();
        let pose = DVec3::new(0.2, 50.0, 0.0);
        let by_id = project_on_route_local(&net, &[LaneId(0)], pose, 0.0, 1.25, 1.0, 60.0).unwrap();
        let by_index = project_on_route_indices(&net, &[0], pose, 0.0, 1.25, 1.0, 60.0).unwrap();
        assert_eq!(by_id, by_index);
    }

    #[test]
    fn berth_occupancy_finds_the_docked_body() {
        let a = body(1, 100.0, 0.0, 3.0);
        let occ = Occupancy::build(NetworkVersion(1), 0, vec![a]);
        assert_eq!(occ.berth_occupancy(LaneId(0), 100.0, 6.0, &[]), Some(VehicleId(1)));
        assert_eq!(occ.berth_occupancy(LaneId(0), 150.0, 6.0, &[]), None);
    }
}
