//! Shared headless fixture for the Stage 7 maneuver scenarios.
//!
//! A small kinematic world over a synthetic [`Network`] and one [`ManeuverCoordinator`]: each
//! car obeys the lane change, lateral target and stop the coordinator hands back, and the
//! coordinator reads the realized occupancy built from the same bodies. No renderer and no
//! OMSI assets.

#![allow(dead_code)]

use glam::{DVec2, DVec3};
use traffic::perception::{BodyFootprint, Occupancy, Placement};
use traffic::*;

pub const DT: f32 = 0.02;

/// One synthetic car in the fixture.
pub struct Car {
    pub id: VehicleId,
    pub lane: usize,
    pub s: f32,
    pub speed: f32,
    pub lateral: f32,
    pub lateral_target: f32,
    pub odometer: f32,
    pub front: f32,
    pub rear: f32,
    pub half_width: f32,
    pub accel: f32,
    pub change: Option<ChangeInfo>,
    pub route_next: Option<usize>,
    pub planned_next: Option<usize>,
    pub turn_wish: i32,
    pub stopped: f32,
    pub state: ManeuverState,
    /// The requests the test submits for this car this tick.
    pub lead_gap: Option<f32>,
    pub lead_standing: bool,
    pub obstacle_len: f32,
    pub parked: bool,
    pub kerb_swerve: Option<f32>,
    pub service_lateral: Option<f32>,
    pub service_phase: ManeuverPhase,
    pub accel_cap: Option<f32>,
    pub last: Option<ManeuverDecision>,
}

impl Car {
    pub fn new(id: u64, lane: usize, s: f32) -> Car {
        Car {
            id: VehicleId(id),
            lane,
            s,
            speed: 0.0,
            lateral: 0.0,
            lateral_target: 0.0,
            odometer: s,
            front: 2.25,
            rear: 2.25,
            half_width: 1.25,
            accel: 1.5,
            change: None,
            route_next: None,
            planned_next: None,
            turn_wish: 0,
            stopped: 0.0,
            state: ManeuverState::default(),
            lead_gap: None,
            lead_standing: false,
            obstacle_len: 4.8,
            parked: false,
            kerb_swerve: None,
            service_lateral: None,
            service_phase: ManeuverPhase::Idle,
            accel_cap: None,
            last: None,
        }
    }

    pub fn speed(mut self, v: f32) -> Car {
        self.speed = v;
        self
    }

    pub fn change_to(mut self, to: usize, dir: i32) -> Car {
        self.route_next = Some(to);
        self.planned_next = Some(to);
        let _ = dir;
        self
    }

    fn actor(&self) -> ManeuverActor {
        ManeuverActor {
            id: self.id,
            lane: self.lane,
            s: self.s,
            lateral: self.lateral,
            speed: self.speed,
            accel: self.accel,
            decel: 2.5,
            reaction: 0.7,
            desire: 1.0,
            max_speed_kmh: 50.0,
            front: self.front,
            rear: self.rear,
            length: self.front + self.rear,
            half_width: self.half_width,
            odometer: self.odometer,
            min_gap: 2.0,
            veh_type: 0,
            lane_kind: LaneKind::Street,
            planned_next: self.planned_next,
            route_next: self.route_next,
            turn_wish: self.turn_wish,
            change: self.change,
            stopped: self.stopped,
            light_hold: false,
            yielding: false,
            at_stop: false,
            pass_room: 6.0,
            lat_accel: 2.8,
        }
    }

    fn body(&self) -> BodyFootprint {
        let mut f = BodyFootprint::new(
            self.id,
            DVec2::new(self.lateral as f64, self.s as f64),
            DVec2::new(0.0, 1.0),
            (self.front + self.rear) as f64 * 0.5,
            self.half_width as f64,
            0.0,
            3.0,
            self.speed,
        );
        f.current = Some(Placement {
            lane: LaneId(self.lane),
            s: self.s,
            lateral: self.lateral,
            foreign: false,
        });
        f
    }
}

/// The fixture world: a network, the coordinator, the cars and optional foreign bodies.
pub struct ManeuverWorld {
    pub net: Network,
    pub coord: ManeuverCoordinator,
    pub cars: Vec<Car>,
    pub extras: Vec<BodyFootprint>,
    pub tick: u64,
    pub time: f32,
}

impl ManeuverWorld {
    pub fn new(net: Network) -> ManeuverWorld {
        ManeuverWorld {
            net,
            coord: ManeuverCoordinator::new(),
            cars: Vec::new(),
            extras: Vec::new(),
            tick: 0,
            time: 0.0,
        }
    }

    pub fn add(&mut self, car: Car) {
        self.cars.push(car);
    }

    pub fn car(&self, id: u64) -> &Car {
        self.cars.iter().find(|c| c.id == VehicleId(id)).expect("car")
    }

    pub fn car_mut(&mut self, id: u64) -> &mut Car {
        self.cars
            .iter_mut()
            .find(|c| c.id == VehicleId(id))
            .expect("car")
    }

    fn occupancy(&self) -> Occupancy {
        let mut feet: Vec<BodyFootprint> = self.cars.iter().map(|c| c.body()).collect();
        feet.extend(self.extras.iter().copied());
        Occupancy::build(self.net.version(), self.tick, feet)
    }

    /// Advance one fixed tick.
    pub fn step(&mut self) {
        let occ = self.occupancy();
        let actors: Vec<ManeuverActor> = self.cars.iter().map(|c| c.actor()).collect();
        let intents: Vec<ManeuverIntent> = actors
            .iter()
            .map(|a| ManeuverIntent {
                vehicle: a.id,
                target: a
                    .change
                    .map(|c| LaneId(c.to))
                    .or_else(|| required_target(&self.net, a).map(LaneId)),
                required: true,
            })
            .collect();
        self.coord.begin_tick(&intents, self.tick);
        let decisions: Vec<ManeuverDecision> = {
            let scene = ManeuverScene {
                net: &self.net,
                occupancy: &occ,
                actors: &actors,
                people: &[],
                time: self.time,
                dt: DT,
                tick: self.tick,
            };
            let mut out: Vec<ManeuverDecision> = Vec::with_capacity(self.cars.len());
            for k in 0..self.cars.len() {
                let mut inputs = ManeuverInputs::new(k);
                inputs.service_lateral = self.cars[k].service_lateral;
                inputs.service_phase = self.cars[k].service_phase;
                inputs.kerb_swerve = self.cars[k].kerb_swerve;
                inputs.lead_gap = self.cars[k].lead_gap;
                inputs.lead_standing = self.cars[k].lead_standing;
                inputs.obstacle_len = self.cars[k].obstacle_len;
                inputs.parked = self.cars[k].parked;
                out.push(self.coord.plan(&scene, &mut self.cars[k].state, &inputs));
            }
            out
        };
        for (k, decision) in decisions.into_iter().enumerate() {
            self.apply(k, decision);
        }
        self.tick += 1;
        self.time += DT;
    }

    fn apply(&mut self, k: usize, decision: ManeuverDecision) {
        let (lane, s, speed) = {
            let c = &self.cars[k];
            (c.lane, c.s, c.speed)
        };
        let s_to = decision
            .change
            .map(|cmd| self.net.beside_s(lane, cmd.to, s));
        let car = &mut self.cars[k];
        if let Some(cmd) = decision.change {
            car.change = Some(ChangeInfo {
                to: cmd.to,
                dir: cmd.dir,
                t: 0.0,
                length: (speed * 3.0).max(12.0),
                s_to: s_to.unwrap_or(0.0),
                wait: if cmd.kind == ChangeKind::RouteChange {
                    0.0
                } else {
                    SIGNAL_BEFORE_CHANGE
                },
                bypass: cmd.kind == ChangeKind::Bypass,
            });
            car.state.change_to = Some(cmd.to);
            car.state.change_dir = cmd.dir;
        }
        if let Some(t) = decision.lateral_target {
            car.lateral_target = t;
        }
        if let Some((_from, to, _odo, _len)) = decision.lateral_ramp {
            car.lateral_target = to;
        }
        car.accel_cap = decision.accel_cap;
        car.last = Some(decision.clone());
        // Integrate the realized body.
        let ds = car.speed * DT;
        car.s += ds;
        car.odometer += ds;
        if let Some(mut c) = car.change {
            c.s_to += ds;
            if c.wait > 0.0 {
                c.wait = (c.wait - DT).max(0.0);
            } else {
                c.t += ds / c.length.max(1.0);
            }
            let from_len = self.net.lanes[car.lane].length();
            let to_len = self.net.lanes[c.to].length();
            if c.t >= 1.0 || car.s >= from_len || c.s_to >= to_len {
                car.lane = c.to;
                car.s = c.s_to.min(to_len);
                car.change = None;
            } else {
                car.change = Some(c);
            }
        }
        // A simple lateral mover for the tests' assertions (not the realization).
        let step = 0.6 * DT;
        let d = car.lateral_target - car.lateral;
        if d.abs() <= step {
            car.lateral = car.lateral_target;
        } else {
            car.lateral += step * d.signum();
        }
    }

    pub fn run(&mut self, ticks: u32) {
        for _ in 0..ticks {
            self.step();
        }
    }
}

/// A single straight lane (no lateral neighbours, no oncoming lane).
pub fn one_way() -> Network {
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

/// A one-way road of two lanes side by side (lane 0 right, lane 1 left), with the lateral
/// neighbours set so a discretionary change is possible.
pub fn two_lanes() -> Network {
    let mut net = Network {
        lanes: vec![
            LaneBuilder::polyline(
                vec![DVec3::new(0.0, 0.0, 0.0), DVec3::new(0.0, 400.0, 0.0)],
                LaneKind::Street,
                3.0,
            ),
            LaneBuilder::polyline(
                vec![DVec3::new(-3.5, 0.0, 0.0), DVec3::new(-3.5, 400.0, 0.0)],
                LaneKind::Street,
                3.0,
            ),
        ],
        ..Default::default()
    };
    net.link(1.5);
    net.lanes[0].left = Some(1);
    net.lanes[1].right = Some(0);
    net
}

/// A two-way street: lane 0 north (the car's lane), lane 1 south (the oncoming lane beside
/// it). No lateral neighbours: the passing path.
pub fn two_way() -> Network {
    let mut net = Network {
        lanes: vec![
            LaneBuilder::polyline(
                vec![DVec3::new(0.0, 0.0, 0.0), DVec3::new(0.0, 400.0, 0.0)],
                LaneKind::Street,
                3.0,
            ),
            LaneBuilder::polyline(
                vec![DVec3::new(-3.5, 400.0, 0.0), DVec3::new(-3.5, 0.0, 0.0)],
                LaneKind::Street,
                3.0,
            ),
        ],
        ..Default::default()
    };
    net.link(1.5);
    net
}

/// A stationary foreign body in `lane` at `s`, with an optional lateral offset.
pub fn obstacle(id: u64, lane: usize, s: f32, lateral: f32) -> BodyFootprint {
    let mut f = BodyFootprint::new(
        VehicleId(id),
        DVec2::new(lateral as f64, s as f64),
        DVec2::new(0.0, 1.0),
        2.25,
        1.25,
        0.0,
        3.0,
        0.0,
    );
    f.current = Some(Placement {
        lane: LaneId(lane),
        s,
        lateral,
        foreign: false,
    });
    f
}

/// A moving body on the oncoming lane of the two-way fixture. `s` is the distance along that
/// southbound lane (0 at y = 400, 400 at y = 0).
pub fn oncoming(id: u64, lane: usize, s: f32, speed: f32) -> BodyFootprint {
    let mut f = BodyFootprint::new(
        VehicleId(id),
        DVec2::new(-3.5, (400.0 - s) as f64),
        DVec2::new(0.0, -1.0),
        2.25,
        1.25,
        0.0,
        3.0,
        speed,
    );
    f.current = Some(Placement {
        lane: LaneId(lane),
        s,
        lateral: 0.0,
        foreign: false,
    });
    f
}
