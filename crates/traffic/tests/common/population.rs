//! Shared headless fixture for the Stage 8 population scenarios.
//!
//! A straight street lane (or a few) with a [`PopulationCoordinator`], synthetic realized
//! bodies for the occupancy, and helpers to submit demand and read typed decisions. No
//! renderer and no OMSI assets.

#![allow(dead_code)]

use glam::{DVec2, DVec3};
use hashbrown::HashMap;
use traffic::perception::{BodyFootprint, Occupancy, Placement};
use traffic::*;

/// Half length used for the synthetic bodies (m).
pub const HALF: f64 = 3.0;

/// A network of `lanes` parallel straight street lanes, each 400 m long.
pub fn lane_net(lanes: usize) -> Network {
    let mut net = Network {
        lanes: (0..lanes)
            .map(|i| {
                let x = i as f64 * 4.0;
                LaneBuilder::polyline(
                    vec![DVec3::new(x, 0.0, 0.0), DVec3::new(x, 400.0, 0.0)],
                    LaneKind::Street,
                    3.0,
                )
            })
            .collect(),
        ..Default::default()
    };
    net.link(1.5);
    net
}

/// A stationary realized body of `id` on `lane` at `s`.
pub fn body(id: u64, lane: usize, s: f32) -> BodyFootprint {
    let mut f = BodyFootprint::new(
        VehicleId(id),
        DVec2::new(lane as f64 * 4.0, s as f64),
        DVec2::new(0.0, 1.0),
        HALF,
        1.25,
        0.0,
        3.0,
        0.0,
    );
    f.current = Some(Placement {
        lane: LaneId(lane),
        s,
        lateral: 0.0,
        foreign: false,
    });
    f
}

/// The frozen occupancy for `bodies`.
pub fn occupancy(net: &Network, tick: u64, bodies: &[BodyFootprint]) -> Occupancy {
    Occupancy::build(net.version(), tick, bodies.to_vec())
}

/// A test world that keeps the coordinator, its realized bodies and a clock together.
pub struct PopWorld {
    pub net: Network,
    pub coord: PopulationCoordinator,
    pub bodies: Vec<BodyFootprint>,
    pub tick: u64,
    pub time: f32,
}

/// The frozen scene, built from disjoint field borrows so a test can plan while holding the
/// network.
pub fn scene<'a>(
    net: &'a Network,
    occ: &'a Occupancy,
    initial: bool,
    demand: PopulationDemand,
    tick: u64,
) -> PopulationScene<'a> {
    PopulationScene {
        net,
        occupancy: occ,
        demand,
        initial,
        tick,
    }
}

impl PopWorld {
    pub fn new(lanes: usize) -> PopWorld {
        PopWorld {
            net: lane_net(lanes),
            coord: PopulationCoordinator::new(),
            bodies: Vec::new(),
            tick: 0,
            time: 0.0,
        }
    }

    pub fn occupy(&mut self, id: u64, lane: usize, s: f32) {
        self.bodies.push(body(id, lane, s));
    }

    pub fn occupancy(&self) -> Occupancy {
        occupancy(&self.net, self.tick, &self.bodies)
    }

    /// Annotate every outstanding request as if it were valid, on loaded ground, with the
    /// given visibility.
    pub fn facts(&self, visible: bool) -> HashMap<SpawnRequestId, SpawnFacts> {
        self.coord
            .requests()
            .map(|r| {
                (
                    r.id,
                    SpawnFacts {
                        path_valid: true,
                        continuation: true,
                        ground: true,
                        visible,
                    },
                )
            })
            .collect()
    }

    /// One admission pass: build the occupancy, plan, and advance the clock.
    pub fn step(
        &mut self,
        initial: bool,
        demand: PopulationDemand,
        visible: bool,
    ) -> Vec<SpawnDecision> {
        self.coord.begin_tick(self.tick, self.time);
        let occ = self.occupancy();
        let facts = self.facts(visible);
        let decisions = {
            let scene = scene(&self.net, &occ, initial, demand, self.tick);
            self.coord.plan(&scene, &facts)
        };
        self.tick += 1;
        self.time += 0.02;
        decisions
    }

    /// One dormant-validation pass.
    pub fn plan_dormant(
        &mut self,
        views: &[DormantView],
        initial: bool,
        demand: PopulationDemand,
    ) -> Vec<DormantDecision> {
        self.coord.begin_tick(self.tick, self.time);
        let occ = self.occupancy();
        let decisions = {
            let scene = scene(&self.net, &occ, initial, demand, self.tick);
            self.coord.plan_dormant(&scene, views)
        };
        self.tick += 1;
        self.time += 0.02;
        decisions
    }

    /// The number of admitted decisions in a pass.
    pub fn admitted(decisions: &[SpawnDecision]) -> usize {
        decisions
            .iter()
            .filter(|d| d.outcome == SpawnOutcome::Admit)
            .count()
    }
}

/// A demand for random street traffic with `street_target` and the given existing count.
pub fn demand(street_target: usize, unscheduled_count: usize) -> PopulationDemand {
    PopulationDemand {
        street_target,
        unscheduled_count,
        dormant_capacity: street_target * 8,
        ..Default::default()
    }
}
