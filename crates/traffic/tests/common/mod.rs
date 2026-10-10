//! Shared headless fixtures for the Stage 5 junction scenarios.
//!
//! A junction is a synthetic crossing object: lanes with `source == 2` and keys of one
//! tile/object, linked and then conflict-compiled. No renderer and no OMSI assets.

#![allow(dead_code)]

pub mod maneuver;
pub mod service;
pub mod population;

use glam::DVec3;
use hashbrown::HashMap;
use traffic::{
    junction_ahead, JunctionActor, JunctionCoordinator, JunctionDecision, JunctionScene, Lane,
    LaneBuilder, LaneKey, LaneKind, Lead, Network, VehicleId,
};

/// A straight lane of a crossing object (100 m either side of the origin).
pub fn object_lane(from: DVec3, to: DVec3, path: u16) -> Lane {
    let mut l = LaneBuilder::polyline(vec![from, to], LaneKind::Street, 3.0);
    l.source = 2;
    l.key = Some(LaneKey {
        tile: (0, 0),
        id: 1,
        path,
    });
    l
}

/// A plain `+` crossing with one movement in each direction. Lanes 0/2/4/6 are approaches,
/// 1/3/5/7 are the crossing-object lanes (west->east, east->west, south->north,
/// north->south); each approach leads into its object lane at the object's near edge.
pub fn four_way() -> Network {
    let mut net = Network {
        lanes: vec![
            // 0 west approach, 1 west->east object (crossing at its middle, s = 10)
            LaneBuilder::polyline(
                vec![DVec3::new(-110.0, 0.0, 0.0), DVec3::new(-10.0, 0.0, 0.0)],
                LaneKind::Street,
                3.0,
            ),
            object_lane(DVec3::new(-10.0, 0.0, 0.0), DVec3::new(10.0, 0.0, 0.0), 0),
            // 2 east approach, 3 east->west object
            LaneBuilder::polyline(
                vec![DVec3::new(110.0, 0.0, 0.0), DVec3::new(10.0, 0.0, 0.0)],
                LaneKind::Street,
                3.0,
            ),
            object_lane(DVec3::new(10.0, 0.0, 0.0), DVec3::new(-10.0, 0.0, 0.0), 1),
            // 4 south approach, 5 south->north object
            LaneBuilder::polyline(
                vec![DVec3::new(0.0, -110.0, 0.0), DVec3::new(0.0, -10.0, 0.0)],
                LaneKind::Street,
                3.0,
            ),
            object_lane(DVec3::new(0.0, -10.0, 0.0), DVec3::new(0.0, 10.0, 0.0), 2),
            // 6 north approach, 7 north->south object
            LaneBuilder::polyline(
                vec![DVec3::new(0.0, 110.0, 0.0), DVec3::new(0.0, 10.0, 0.0)],
                LaneKind::Street,
                3.0,
            ),
            object_lane(DVec3::new(0.0, 10.0, 0.0), DVec3::new(0.0, -10.0, 0.0), 3),
        ],
        ..Default::default()
    };
    for (approach, object) in [(0, 1), (2, 3), (4, 5), (6, 7)] {
        net.lanes[approach].next = vec![object];
    }
    net.compute_conflicts();
    net
}

/// A junction with a short downstream exit lane, and a side road that makes the object a
/// real crossing: approach 0 -> object 1 -> exit 2, side road 3.
pub fn blocked_exit() -> Network {
    let mut net = Network {
        lanes: vec![
            LaneBuilder::polyline(
                vec![DVec3::new(-110.0, 0.0, 0.0), DVec3::new(-10.0, 0.0, 0.0)],
                LaneKind::Street,
                3.0,
            ),
            object_lane(DVec3::new(-10.0, 0.0, 0.0), DVec3::new(10.0, 0.0, 0.0), 0),
            LaneBuilder::polyline(
                vec![DVec3::new(10.0, 0.0, 0.0), DVec3::new(110.0, 0.0, 0.0)],
                LaneKind::Street,
                3.0,
            ),
            object_lane(DVec3::new(0.0, -10.0, 0.0), DVec3::new(0.0, 10.0, 0.0), 1),
        ],
        ..Default::default()
    };
    net.lanes[0].next = vec![1];
    net.lanes[1].next = vec![2];
    net.compute_conflicts();
    net
}

/// Two roads meeting head-on at a shallow angle (an oncoming pair): approach 0 -> object 1,
/// approach 2 -> object 3, the two object lanes crossing at the origin.
pub fn oncoming_pair() -> Network {
    let mut net = Network {
        lanes: vec![
            LaneBuilder::polyline(
                vec![DVec3::new(-110.0, -3.0, 0.0), DVec3::new(-10.0, -3.0, 0.0)],
                LaneKind::Street,
                3.0,
            ),
            object_lane(DVec3::new(-10.0, -3.0, 0.0), DVec3::new(10.0, 3.0, 0.0), 0),
            LaneBuilder::polyline(
                vec![DVec3::new(110.0, -3.0, 0.0), DVec3::new(10.0, -3.0, 0.0)],
                LaneKind::Street,
                3.0,
            ),
            object_lane(DVec3::new(10.0, -3.0, 0.0), DVec3::new(-10.0, 3.0, 0.0), 1),
        ],
        ..Default::default()
    };
    net.lanes[0].next = vec![1];
    net.lanes[2].next = vec![3];
    net.compute_conflicts();
    net
}

/// A street approach through a short object lane, crossed by a rail lane of the same object
/// (a level crossing): approach 0 -> object 1 -> exit 2, rail 3.
pub fn rail_crossing() -> Network {
    let mut rail = object_lane(DVec3::new(0.0, -10.0, 0.0), DVec3::new(0.0, 10.0, 0.0), 1);
    rail.kind = LaneKind::Rail;
    let mut net = Network {
        lanes: vec![
            LaneBuilder::polyline(
                vec![DVec3::new(-110.0, 0.0, 0.0), DVec3::new(-10.0, 0.0, 0.0)],
                LaneKind::Street,
                3.0,
            ),
            object_lane(DVec3::new(-10.0, 0.0, 0.0), DVec3::new(10.0, 0.0, 0.0), 0),
            LaneBuilder::polyline(
                vec![DVec3::new(10.0, 0.0, 0.0), DVec3::new(110.0, 0.0, 0.0)],
                LaneKind::Street,
                3.0,
            ),
            rail,
        ],
        ..Default::default()
    };
    net.lanes[0].next = vec![1];
    net.lanes[1].next = vec![2];
    net.compute_conflicts();
    net
}

/// A single lane, no junction (for release/invalidation checks).
pub fn straight() -> Network {
    let mut net = Network {
        lanes: vec![LaneBuilder::polyline(
            vec![DVec3::new(0.0, 0.0, 0.0), DVec3::new(0.0, 400.0, 0.0)],
            LaneKind::Street,
            3.0,
        )],
        ..Default::default()
    };
    net.link(1.5);
    net
}

/// The frozen view a test builds and the coordinator reads.
pub struct Harness {
    pub net: Network,
    pub actors: Vec<JunctionActor>,
    pub index_of: HashMap<VehicleId, usize>,
    pub on_lane: HashMap<usize, Vec<(usize, f32, f32, bool)>>,
    pub coming: HashMap<usize, Vec<(usize, f32)>>,
    pub walkers: HashMap<usize, Vec<f32>>,
    pub geo_prev: HashMap<VehicleId, Option<VehicleId>>,
    pub aspects: HashMap<(usize, usize), traffic::Aspect>,
    pub time: f32,
    pub tick: u64,
}

impl Harness {
    pub fn new(net: Network) -> Harness {
        Harness {
            net,
            actors: Vec::new(),
            index_of: HashMap::new(),
            on_lane: HashMap::new(),
            coming: HashMap::new(),
            walkers: HashMap::new(),
            geo_prev: HashMap::new(),
            aspects: HashMap::new(),
            time: 0.0,
            tick: 0,
        }
    }

    pub fn add(&mut self, actor: JunctionActor) -> usize {
        let idx = self.actors.len();
        self.index_of.insert(actor.id, idx);
        self.actors.push(actor);
        idx
    }

    /// Put an actor's body on `lane` at `s` (realized occupancy).
    pub fn place(&mut self, idx: usize, lane: usize, s: f32) {
        self.on_lane.entry(lane).or_default().push((idx, s, 0.0, false));
    }

    /// Register an approaching vehicle on `lane` (not physically there yet).
    pub fn approach(&mut self, idx: usize, lane: usize, distance: f32) {
        self.coming.entry(lane).or_default().push((idx, distance));
    }

    pub fn scene(&self) -> JunctionScene<'_> {
        JunctionScene {
            net: &self.net,
            actors: &self.actors,
            index_of: &self.index_of,
            on_lane: &self.on_lane,
            coming: &self.coming,
            walkers: &self.walkers,
            geo_prev: &self.geo_prev,
            aspects: &self.aspects,
            time: self.time,
            tick: self.tick,
        }
    }

    /// The actor's planned way: its own lane, then up to three lanes reached by `next`.
    pub fn way(&self, i: usize) -> Vec<(usize, f32)> {
        let a = &self.actors[i];
        let mut out = vec![(a.lane, -a.s)];
        let mut d = self.net.lanes[a.lane].length() - a.s;
        let mut cur = a.lane;
        for _ in 0..3 {
            let Some(&n) = self.net.lanes[cur].next.first() else {
                break;
            };
            out.push((n, d));
            d += self.net.lanes[n].length();
            cur = n;
        }
        out
    }

    pub fn plan(&self, coord: &mut JunctionCoordinator, i: usize, lead: Option<Lead>) -> JunctionDecision {
        let way = self.way(i);
        let movement = junction_ahead(&self.net, &way);
        let scene = self.scene();
        coord.plan(&scene, i, &way, lead, movement)
    }
}
