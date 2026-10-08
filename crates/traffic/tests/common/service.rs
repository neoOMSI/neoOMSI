//! Shared headless fixture for the Stage 6 service scenarios.
//!
//! A single straight lane with one or more buses driving to a stop. The buses are moved by a
//! tiny kinematic model that obeys the `stop_at` distance the `ServiceCoordinator` hands back,
//! and the coordinator reads the realized occupancy built from the same bodies. No renderer,
//! no OMSI assets.

#![allow(dead_code)]

use glam::DVec2;
use traffic::perception::{BodyFootprint, Occupancy, Placement};
use traffic::*;

pub const DT: f32 = 0.02;
pub const LANE: LaneId = LaneId(0);

/// Half length used for the synthetic bus bodies (m).
pub const HALF: f64 = 3.0;

/// One synthetic bus in the fixture.
pub struct Bus {
    pub id: VehicleId,
    pub s: f32,
    pub speed: f32,
    pub lateral: f32,
    pub front: f32,
    pub rear: f32,
    pub state: ServiceState,
    pub layover: bool,
    /// The berths still ahead, front first.
    pub stops: Vec<BerthGeometry>,
    pub demand: StopDemand,
    pub policy: StopPolicy,
    pub feedback: ScriptFeedback,
    /// Stops this bus has served, in order (once-only checks).
    pub served: Vec<StopId>,
    pub last: Option<ServiceDecision>,
}

impl Bus {
    pub fn new(id: u64, s: f32, stops: Vec<BerthGeometry>) -> Bus {
        Bus {
            id: VehicleId(id),
            s,
            speed: 0.0,
            lateral: 0.0,
            front: HALF as f32,
            rear: HALF as f32,
            state: ServiceState::new(),
            layover: false,
            stops,
            demand: StopDemand::default(),
            policy: StopPolicy::default(),
            // The synthetic bus has no door script: the fixed-close fallback applies.
            feedback: ScriptFeedback::Unsupported,
            served: Vec::new(),
            last: None,
        }
    }

    pub fn front_berth(&self) -> Option<BerthGeometry> {
        self.stops.first().copied()
    }

    pub fn phase(&self) -> ServicePhase {
        self.state.phase
    }

    fn actor(&self) -> ServiceActor {
        ServiceActor {
            id: self.id,
            lane: LANE.index(),
            s: self.s,
            front: self.front,
            rear: self.rear,
            length: self.front + self.rear,
            speed: self.speed,
            lateral: self.lateral,
            min_gap: 2.0,
        }
    }

    fn body(&self) -> BodyFootprint {
        let mut f = BodyFootprint::new(
            self.id,
            DVec2::new(self.lateral as f64, self.s as f64),
            DVec2::new(0.0, 1.0),
            HALF,
            1.25,
            0.0,
            3.0,
            self.speed,
        );
        f.current = Some(Placement {
            lane: LANE,
            s: self.s,
            lateral: self.lateral,
            foreign: false,
        });
        f
    }
}

/// The fixture world: a lane, the coordinator, the buses, and optional foreign bodies.
pub struct ServiceWorld {
    pub net: Network,
    pub coord: ServiceCoordinator,
    pub buses: Vec<Bus>,
    pub extras: Vec<BodyFootprint>,
    pub tick: u64,
    pub day_time: f64,
    pub cruise: f32,
    pub merges: usize,
}

impl ServiceWorld {
    pub fn new() -> ServiceWorld {
        let mut net = Network {
            lanes: vec![LaneBuilder::polyline(
                vec![glam::DVec3::new(0.0, 0.0, 0.0), glam::DVec3::new(0.0, 400.0, 0.0)],
                LaneKind::Street,
                3.0,
            )],
            ..Default::default()
        };
        net.link(1.5);
        ServiceWorld {
            net,
            coord: ServiceCoordinator::new(),
            buses: Vec::new(),
            extras: Vec::new(),
            tick: 0,
            day_time: 36000.0,
            cruise: 12.0,
            merges: 0,
        }
    }

    pub fn add(&mut self, bus: Bus) {
        self.buses.push(bus);
    }

    pub fn bus(&self, id: u64) -> &Bus {
        self.buses.iter().find(|b| b.id == VehicleId(id)).expect("bus")
    }

    pub fn bus_mut(&mut self, id: u64) -> &mut Bus {
        self.buses.iter_mut().find(|b| b.id == VehicleId(id)).expect("bus")
    }

    fn occupancy(&self) -> Occupancy {
        let mut feet: Vec<BodyFootprint> = self.buses.iter().map(|b| b.body()).collect();
        feet.extend(self.extras.iter().copied());
        Occupancy::build(self.net.version(), self.tick, feet)
    }

    /// Advance one fixed tick.
    pub fn step(&mut self) {
        let occupancy = self.occupancy();
        let actors: Vec<ServiceActor> = self.buses.iter().map(|b| b.actor()).collect();
        let intents: Vec<BerthIntent> = self
            .buses
            .iter()
            .filter_map(|b| {
                if let Some(held) = b.state.berth {
                    return Some(BerthIntent {
                        vehicle: b.id,
                        stop: held.stop,
                        occurrence: held.occurrence,
                        holds: true,
                    });
                }
                let t = b.front_berth()?;
                let d = t.s - b.s;
                (d <= STOP_REACH).then_some(BerthIntent {
                    vehicle: b.id,
                    stop: t.stop,
                    occurrence: t.occurrence,
                    holds: false,
                })
            })
            .collect();
        self.coord.begin_tick(&intents, self.tick);

        for k in 0..self.buses.len() {
            let (berth, distance, policy, demand, feedback) = {
                let bus = &self.buses[k];
                let berth = bus.front_berth();
                let distance = berth.map(|b| b.s - bus.s).unwrap_or(f32::MAX);
                (berth, distance, bus.policy.clone(), bus.demand, bus.feedback)
            };
            let inputs = ServiceInputs {
                actor: k,
                berth,
                distance,
                policy,
                demand,
                feedback,
                passing: false,
                kerb_swerve: None,
                junction_first: false,
            };
            let scene = ServiceScene {
                net: &self.net,
                occupancy: &occupancy,
                actors: &actors,
                day_time: self.day_time,
                dt: DT,
                tick: self.tick,
            };
            let decision = self.coord.plan(&scene, &mut self.buses[k].state, &inputs);

            let (id, s_now, front, _rear) = {
                let b = &self.buses[k];
                (b.id, b.s, b.front, b.rear)
            };
            let leader_limit = self
                .buses
                .iter()
                .filter(|o| o.id != id && o.s > s_now)
                .map(|o| o.s - o.rear - front - 2.0)
                .fold(f32::MAX, f32::min);

            // Apply the decision to the content cursor.
            let bus = &mut self.buses[k];
            if decision.consume_stop {
                if let Some(t) = bus.stops.first().copied() {
                    bus.served.push(t.stop);
                }
                bus.stops.remove(0);
            }
            // Kinematic follow of the commanded stop distance.
            let target = decision
                .stop_at
                .map(|at| bus.s + (at - bus.front - traffic::following::STOP_LINE_GAP).max(0.0))
                .map(|t| t.min(leader_limit));
            if let Some(target) = target {
                let d = target - bus.s;
                if d <= 0.05 {
                    bus.s = target;
                    bus.speed = 0.0;
                } else {
                    let stop_speed = (2.0 * 1.5 * d).sqrt();
                    bus.speed = (bus.speed + 1.5 * DT).min(stop_speed).min(self.cruise);
                    bus.s += bus.speed * DT;
                }
            } else {
                bus.speed = (bus.speed + 1.5 * DT).min(self.cruise);
                bus.s += bus.speed * DT;
            }
            bus.s = bus.s.min(leader_limit);
            if let Some(lat) = decision.lateral_target {
                let step = 1.5 * DT;
                if (lat - bus.lateral).abs() <= step {
                    bus.lateral = lat;
                } else {
                    bus.lateral += step * (lat - bus.lateral).signum();
                }
            }
            bus.last = Some(decision);
        }

        self.tick += 1;
        self.day_time += DT as f64;
    }

    pub fn run(&mut self, ticks: u32) {
        for _ in 0..ticks {
            self.step();
        }
    }
}

/// A berth at `s` on the fixture lane, departing at `depart`.
pub fn berth(stop: i64, s: f32, depart: f64) -> BerthGeometry {
    BerthGeometry {
        stop: StopId(stop),
        occurrence: 0,
        lane: LANE.index(),
        route_index: 0,
        side: PlatformSide::Right,
        s,
        bay: 1.6,
        depart,
        stop_length: None,
        boarding_region: DEFAULT_BOARDING_REGION,
        approach_distance: DEFAULT_APPROACH_DISTANCE,
        berths: 1,
    }
}

/// A stationary foreign body (the player, a parked car) occupying the berth at `s`.
pub fn occupier(id: u64, s: f32) -> BodyFootprint {
    let mut f = BodyFootprint::new(
        VehicleId(id),
        DVec2::new(1.6, s as f64),
        DVec2::new(0.0, 1.0),
        2.5,
        1.25,
        0.0,
        3.0,
        0.0,
    );
    f.current = Some(Placement {
        lane: LANE,
        s,
        lateral: 1.6,
        foreign: false,
    });
    f
}
