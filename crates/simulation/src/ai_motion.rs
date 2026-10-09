//! How the body of an AI vehicle follows the way its planner lays out
//! (`traffic::AiState::way_point`): a road vehicle is a bicycle model steered by pure
//! pursuit and sitting on its springs, a rail vehicle rides on its two outer axles, an
//! aircraft flies along its path and banks into its turns.
//!
//! Placing the vehicle *on* the lane every frame, turned to the lane's heading, is what made
//! the traffic look wrong: the heading jumped at every lane sample and joint (the steering
//! was worked out from those jumps), the body turned about its middle so that the rear
//! wheels slid sideways through every bend, and a lane change was a sideways glide with a
//! made-up yaw. Here the heading only ever changes by speed × tan(steering) / wheelbase,
//! the rear axle never moves sideways, and the steering itself is rate limited.

use crate::vehicle::VehicleInstance;
use glam::{DVec2, DVec3, Vec3};
use ::legacy_vehicle::Vehicle;
use std::f32::consts::FRAC_PI_2;

/// What kind of way the vehicle follows.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MotionKind {
    Road,
    Rail,
    Air,
}

/// The line the vehicle turns about (`[rot_pnt_long]`, normally the rear axle) and the
/// distance from it to the front axle: (rotation point, wheelbase), both in metres along
/// the body.
pub fn rotation_point(def: &Vehicle) -> (f32, f32) {
    let front = def.axles.iter().map(|a| a.long).fold(f32::MIN, f32::max);
    let rear = def.axles.iter().map(|a| a.long).fold(f32::MAX, f32::min);
    if def.axles.is_empty() {
        return (-1.3, 2.6);
    }
    // a rotation point at (or in front of) the front axle is a file without one
    let rot = if def.axles.len() >= 2 && def.rot_pnt_long < front - 0.5 {
        def.rot_pnt_long
    } else {
        rear
    };
    (rot, (front - rot).max(0.8))
}

/// Clearance a vehicle pulling out keeps from the corner of what it goes round (m).
pub const PULL_OUT_CLEARANCE: f64 = 0.25;
/// Seconds a driver standing behind something turns the wheels before moving off round it
/// (the shortest reaction time of the AI drivers).
pub const PULL_OUT_WAIT: f32 = 0.4;
/// Acceleration (m/s²) of a driver edging out round something standing close ahead, until
/// the front is past its corner: pulling away briskly, a car covered too much ground while
/// the wheels were still turning and came within 0.2 m of the corner.
pub const PULL_OUT_ACCEL: f32 = 1.0;

/// Lengths (m) of the S-curve that takes a vehicle out round something standing `real`
/// metres ahead of its front bumper, gentlest first (`front`: origin to front bumper).
/// From a standstill a driver turns out steeply; rolling up to a parked car they move over
/// well before it.
pub fn pull_out_ramps(real: f32, front: f32, rolling: bool) -> Vec<f32> {
    let base = real + 0.5 * front;
    let (factors, max): (&[f32], f32) = if rolling {
        (&[1.0, 0.8, 0.6, 0.45], 20.0)
    } else {
        (&[1.5, 1.2, 1.0, 0.8, 0.6], 12.0)
    };
    let mut out: Vec<f32> = Vec::new();
    for f in factors {
        let r = (base * f).clamp(4.0, max);
        if !out.iter().any(|&o| (o - r).abs() < 0.3) {
            out.push(r);
        }
    }
    out
}

/// Sideways acceleration (m/s²) a driver allows moving back into the lane after passing.
pub const BACK_IN_LAT_ACCEL: f32 = 2.5;

/// Length (m) of the S-curve that takes a vehicle `offset` metres back into its lane at
/// `speed` (m/s) after passing: a smoothstep of length L over the offset D turns at most
/// 6·D·v²/L², so L = v·√(6·D/a) keeps it to `lat_accel` - about 2.8 s of travel for a lane
/// width. A fixed 8-14 m cut back 3.3 m in under a second at 40 km/h (4-5.6 m/s²).
pub fn back_in_ramp(offset: f32, speed: f32, lat_accel: f32) -> f32 {
    let geometric = (offset.abs() * 3.5).clamp(8.0, 14.0);
    let dynamic = speed.max(0.0) * (6.0 * offset.abs() / lat_accel.max(0.5)).sqrt();
    geometric.max(dynamic).min(60.0)
}

/// A straight way north from the origin that moves `side` metres to the left along an
/// S-curve (smoothstep) `ramp` metres long, as `AiState::way_point` lays out a pull-out.
pub fn straight_pull_out(side: f32, ramp: f32) -> impl Fn(f32) -> DVec3 {
    move |d: f32| {
        let t = (d / ramp.max(0.1)).clamp(0.0, 1.0);
        let lat = -side * t * t * (3.0 - 2.0 * t);
        DVec3::new(lat as f64, d as f64, 0.0)
    }
}

/// How far behind something standing in its lane a vehicle has to stop to get out round
/// it later from a standstill (m, from its front bumper to the other's body), with its own
/// wheelbase, steering lock and steering rate: the shortest gap from which one of the
/// `pull_out_ramps` takes it past the corner of a standing bus (`obstacle_half_width`,
/// in the middle of the lane, the oncoming lane `side` metres over) with
/// `PULL_OUT_CLEARANCE` to spare, and half a metre more for where it comes to rest.
/// `extent` is (origin to front bumper, to rear bumper, half width).
pub fn pull_out_room(
    def: &Vehicle,
    extent: (f32, f32, f32),
    obstacle_half_width: f64,
    side: f32,
) -> f32 {
    let (front, _, _) = extent;
    let bus_half_len = 6.0;
    let mut gap = 0.75f32;
    while gap <= 12.0 {
        let obstacle = crate::collision::Obb::vehicle(
            DVec2::new(0.0, (front + gap) as f64 + bus_half_len),
            0.0,
            bus_half_len,
            bus_half_len,
            obstacle_half_width,
        );
        for ramp in pull_out_ramps(gap, front, false) {
            let way = straight_pull_out(side, ramp);
            let mut body = AiBody::new(def, MotionKind::Road);
            body.place(&way, None, None, 0.0);
            if body.sweep_clearance(
                &way,
                0.0,
                PULL_OUT_WAIT,
                PULL_OUT_ACCEL,
                8.0,
                gap + 6.0,
                extent,
                &[obstacle],
            ) >= PULL_OUT_CLEARANCE
            {
                return gap + 0.5;
            }
        }
        gap += 0.25;
    }
    12.5
}

/// Unit vector (east, north) of a heading in degrees.
fn dir(h: f64) -> DVec2 {
    let r = h.to_radians();
    DVec2::new(r.sin(), r.cos())
}

fn wrap(d: f64) -> f64 {
    (d + 180.0).rem_euclid(360.0) - 180.0
}

/// One wheel on its spring.
#[derive(Debug, Clone, Copy)]
struct Wheel {
    axle: usize,
    side: usize,
    /// Body frame: right and forward (m).
    lat: f32,
    long: f32,
    /// Spring (N/m) and damper (Ns/m) of this wheel.
    k: f32,
    c: f32,
}

/// How far over its way an AI car's wheel climbs onto what is drawn there (m), and how far
/// under the way it goes down to it: the road drawn higher than the lane the map laid out
/// (the car sank into it), not a deck overhead. The bounded, provisional search tolerates
/// imperfect path heights in either direction. Later probes follow previous contacts and
/// local path grade; missing wheels share the supported axle instead of dipping to terrain.
const AI_CONTACT_RANGE: f64 = 1.5;
/// A wider search is needed only when no wheel has found the current road level at all.
/// It corrects displaced path Z without lowering a single unsupported axle onto terrain.
const AI_CONTACT_REACQUIRE: f64 = 3.0;
/// Reject isolated level changes larger than a curb when other tyres still confirm the
/// previous road plane. This is a continuity filter, not a limit on the road's total grade.
const AI_CONTACT_DISCONTINUITY: f64 = 0.35;

#[derive(Debug, Clone)]
pub struct AiBody {
    pub kind: MotionKind,
    rot_long: f32,
    wheelbase: f32,
    /// Metres of close path tracking after a rejected corner step.
    steering_recovery: f32,
    recovery_preview_scale: f32,
    recovery_steer: Option<(f32, f32)>,
    front_long: f32,
    rear_long: f32,
    /// Largest front wheel angle (deg) and how fast the driver turns towards it (deg/s).
    max_steer: f32,
    steer_rate: f32,
    wheels: Vec<Wheel>,
    axle_count: usize,
    mass: f32,
    cog: f32,
    /// Moments of inertia about the lateral (pitch) and longitudinal (roll) axes (kg m²).
    inertia: [f32; 2],
    /// Stiffness and damping of the body in heave (N/m), pitch and roll (Nm/rad).
    spring: [f32; 3],
    damper: [f32; 3],
    started: bool,
    /// World position of the rotation point (x east, y north).
    rear: DVec2,
    /// Heading (deg, clockwise from north).
    pub heading: f64,
    /// Front wheel angle of the bicycle model (deg, + = right), rate limited and smoothed.
    pub steer: f32,
    steer_cmd: f32,
    last_speed: f32,
    /// Distance the body actually moved in the last `step` (m), the realized motion the
    /// traffic domain reads back each tick.
    travelled: f32,
    /// Longitudinal and lateral acceleration (m/s², forward / right positive).
    pub a_long: f32,
    pub a_lat: f32,
    yaw_rate: f32,
    /// The body on its springs: height of the origin, pitch and roll (rad), their rates.
    z: f64,
    vz: f32,
    pitch: f32,
    vpitch: f32,
    roll: f32,
    vroll: f32,
    /// The plane the wheels stand on last frame (height, pitch, roll) and its rates.
    ground: Option<(f64, f32, f32)>,
    ground_rate: [f32; 3],
    /// Pose for the vehicle: origin, pitch and bank (deg, + = nose up / right side down).
    pub position: DVec3,
    pub pitch_deg: f32,
    pub bank_deg: f32,
    /// Per axle (left, right): how far the wheel hangs below its rest position against
    /// the body (m, the `Axle_Suspension_*` convention: negative = compressed).
    pub suspension: Vec<[f32; 2]>,
    /// Each wheel's contact height, followed smoothly (NaN until the first frame). Taken
    /// raw from the road raster, a wheel's travel jumped by its texel steps and by the
    /// sample slipping in and out of the few centimetres around the way from frame to
    /// frame: the wheels of every AI car twitched up and down in their arches while the
    /// body on its springs rode smoothly.
    contact_z: Vec<f64>,
    contact_path_z: Vec<f64>,
    /// At least one axle has valid support. Missing contacts extend that road plane;
    /// complete loss of support is reported instead of substituting authored path height.
    pub ground_supported: bool,
}

impl AiBody {
    pub fn new(def: &Vehicle, kind: MotionKind) -> AiBody {
        let (rot_long, wheelbase) = rotation_point(def);
        let front_long = def.axles.iter().map(|a| a.long).fold(f32::MIN, f32::max);
        let rear_long = def.axles.iter().map(|a| a.long).fold(f32::MAX, f32::min);
        let (front_long, rear_long) = if def.axles.len() >= 2 {
            (front_long, rear_long)
        } else {
            (rot_long + wheelbase, rot_long)
        };
        // `[mass]` is in tonnes (a few files give kilograms), `[momentofintertia]` in the
        // same unit times m²: 1 t and 1.49 for a Golf, 10.9 t and 300 for an SD200
        let tonnes = def.mass < 100.0;
        let mass = (if tonnes { def.mass * 1000.0 } else { def.mass }).max(300.0);
        let unit = if tonnes { 1000.0 } else { 1.0 };
        let heavy = mass > 6000.0;
        // inv_min_turnradius = tan(max angle) / wheelbase
        let max_steer = if def.inv_min_turn_radius > 0.0 {
            (def.inv_min_turn_radius * wheelbase)
                .atan()
                .to_degrees()
                .clamp(15.0, 55.0)
        } else {
            35.0
        };
        let mut wheels = Vec::new();
        for (i, a) in def.axles.iter().enumerate() {
            let half = (a.max_width * 0.5).max(0.4);
            for (side, lat) in [(0, -half), (1, half)] {
                // `achse_feder` / `achse_daempfer`: kN/m and kNs/m per side
                wheels.push(Wheel {
                    axle: i,
                    side,
                    lat,
                    long: a.long,
                    k: a.spring.max(0.0) * 1000.0,
                    c: a.damper.max(0.0) * 1000.0,
                });
            }
        }
        let n = wheels.len().max(1) as f32;
        if wheels.iter().any(|w| w.k <= 0.0) {
            // no springs in the file: 1.5 Hz with a third of critical damping
            let k_total = mass * (std::f32::consts::TAU * 1.5).powi(2);
            let c_total = 2.0 * 0.35 * (k_total * mass).sqrt();
            for w in wheels.iter_mut() {
                w.k = k_total / n;
                w.c = c_total / n;
            }
        }
        let mut spring = [0.0f32; 3];
        let mut damper = [0.0f32; 3];
        for w in &wheels {
            spring[0] += w.k;
            spring[1] += w.k * w.long * w.long;
            spring[2] += w.k * w.lat * w.lat;
            damper[0] += w.c;
            damper[1] += w.c * w.long * w.long;
            damper[2] += w.c * w.lat * w.lat;
        }
        let half_track = wheels.iter().map(|w| w.lat.abs()).fold(0.5, f32::max);
        // (pitch and roll: the first and the third value, as Omsi.exe reads them - see
        // `RigidBody::from_definition`)
        let mut inertia = [
            def.moment_of_inertia[0] * unit,
            def.moment_of_inertia[2] * unit,
        ];
        if inertia[0] <= 0.0 {
            inertia[0] = mass * (front_long * front_long + rear_long * rear_long) * 0.5;
        }
        if inertia[1] <= 0.0 {
            inertia[1] = mass * half_track * half_track * 1.2;
        }
        // keep every mode below ~6 Hz: the springs are integrated at a few hundred Hz at most
        for (i, j) in [(1usize, 0usize), (2, 1)] {
            let max_k = inertia[j] * 38.0f32.powi(2);
            if spring[i] > max_k {
                inertia[j] = spring[i] / 38.0f32.powi(2);
            }
        }
        // at least 0.8 of critical damping in every mode: a driven car settles after a
        // bump or a stop instead of swinging on (the files' dampers are tuned for the
        // player's physics, which damps through the tyres as well)
        let heave_c = 2.0 * 0.8 * (spring[0] * mass).sqrt();
        damper[0] = damper[0].max(heave_c);
        for (i, j) in [(1usize, 0usize), (2, 1)] {
            damper[i] = damper[i].max(2.0 * 0.8 * (spring[i] * inertia[j]).sqrt());
        }
        let axle_count = def.axles.len().max(1);
        AiBody {
            kind,
            rot_long,
            wheelbase,
            steering_recovery: 0.0,
            recovery_preview_scale: 1.0,
            recovery_steer: None,
            front_long,
            rear_long,
            max_steer,
            steer_rate: if heavy { 25.0 } else { 40.0 },
            wheels,
            axle_count,
            mass,
            cog: if def.cog_height > 0.0 {
                def.cog_height
            } else {
                0.6
            },
            inertia,
            spring,
            damper,
            started: false,
            rear: DVec2::ZERO,
            heading: 0.0,
            steer: 0.0,
            steer_cmd: 0.0,
            last_speed: 0.0,
            travelled: 0.0,
            a_long: 0.0,
            a_lat: 0.0,
            yaw_rate: 0.0,
            z: 0.0,
            vz: 0.0,
            pitch: 0.0,
            vpitch: 0.0,
            roll: 0.0,
            vroll: 0.0,
            ground: None,
            ground_rate: [0.0; 3],
            position: DVec3::ZERO,
            pitch_deg: 0.0,
            bank_deg: 0.0,
            suspension: vec![[0.0; 2]; axle_count],
            contact_z: Vec::new(),
            contact_path_z: Vec::new(),
            ground_supported: true,
        }
    }

    /// Put the body onto its way, standing still and straight. `way(d)` is the point `d`
    /// metres along the way from the vehicle's origin.
    pub fn place(
        &mut self,
        way: &dyn Fn(f32) -> DVec3,
        ground: Option<&dyn Fn(f64, f64) -> Option<f64>>,
        contact: Option<&dyn crate::rigid::Ground>,
        speed: f32,
    ) {
        self.started = false;
        self.ground = None;
        self.contact_z.clear();
        self.contact_path_z.clear();
        self.last_speed = speed;
        self.step(0.0, speed, way, ground, contact);
    }

    /// Follow the way for `dt` seconds at `speed` (m/s, the planner's speed).
    /// `contact`: what the wheels stand on (the faces under a height, as the player's wheels
    /// ask it); without it the plain height sampler `ground`.
    pub fn step(
        &mut self,
        dt: f32,
        speed: f32,
        way: &dyn Fn(f32) -> DVec3,
        ground: Option<&dyn Fn(f64, f64) -> Option<f64>>,
        contact: Option<&dyn crate::rigid::Ground>,
    ) {
        self.travelled = 0.0;
        if dt > 0.0 {
            let a = (speed - self.last_speed) / dt;
            self.a_long += (a - self.a_long) * (dt / 0.15).min(1.0);
        }
        self.last_speed = speed;
        match self.kind {
            MotionKind::Road => {
                if self.ground_supported || dt == 0.0 {
                    self.drive(dt, speed, way);
                } else {
                    self.travelled = 0.0;
                }
                self.settle(dt, way, ground, contact);
            }
            MotionKind::Rail => self.ride(way),
            MotionKind::Air => self.fly(dt, speed, way),
        }
    }

    /// The speed the body actually realized over `dt` (m/s), from the distance it moved in
    /// the last [`AiBody::step`]. Fed back to the traffic domain's route progress.
    pub fn realized_speed(&self, dt: f32) -> f32 {
        if dt > 0.0 {
            self.travelled / dt
        } else {
            0.0
        }
    }

    /// A rejected physical step has no realized motion, regardless of its proposed speed.
    pub fn stop_motion(&mut self) {
        self.travelled = 0.0;
        self.last_speed = 0.0;
        self.a_long = 0.0;
        self.a_lat = 0.0;
    }

    /// Reject translation/yaw at an obstacle while allowing the driver to keep
    /// turning the wheels. Restoring the steering too repeats the same clipped
    /// front-corner step forever, particularly with a bus's long wheelbase.
    pub fn reject_motion(&mut self, previous: &Self) {
        let (steer, command) = (self.steer, self.steer_cmd);
        self.clone_from(previous);
        self.steer = steer;
        self.steer_cmd = command;
        self.steering_recovery = self.steering_recovery.max(2.0 * self.wheelbase);
        self.stop_motion();
    }

    /// Forecast the actual steering model without scripts or suspension. Distances
    /// are measured from this pose; callers supply ground height and body obstacles.
    pub fn predict_poses(
        &self,
        way: &dyn Fn(f32) -> DVec3,
        speed: f32,
        accel: f32,
        cap: f32,
        distance: f32,
    ) -> Vec<(f32, DVec3, f64)> {
        let mut body = self.clone();
        let dt = 0.05;
        let mut v = speed.max(0.0);
        let mut d = 0.0;
        if v < 0.1 {
            for _ in 0..8 { body.drive(dt, 0.0, way); }
        }
        let mut poses = vec![(0.0, body.position, body.heading)];
        for _ in 0..1200 {
            if d >= distance { break; }
            v = (v + accel.max(0.3) * dt).min(cap.max(0.5));
            d += v * dt;
            body.drive(dt, v, &|offset| way(d + offset));
            let mut position = body.position;
            position.z = way(d).z;
            poses.push((d, position, body.heading));
        }
        poses
    }

    /// Choose the pursuit horizon whose realized front corner best clears the
    /// obstacle. Keep steering rate/lock and pose integration identical to driving.
    pub fn recover_corner(&mut self, way: &dyn Fn(f32) -> DVec3, extent: (f32, f32, f32), obstacle: &crate::collision::Obb) {
        let mut best = (f64::MIN, self.recovery_preview_scale, None);
        for scale in [0.5, 0.75, 1.0, 1.25, 1.5] {
            let mut probe = self.clone();
            probe.steering_recovery = 2.0 * self.wheelbase;
            probe.recovery_preview_scale = scale;
            probe.recovery_steer = None;
            for _ in 0..10 { probe.drive(0.1, 0.0, way); }
            probe.drive(0.1, 0.5, way);
            let body = crate::collision::Obb::vehicle(probe.position.truncate(), probe.heading,
                extent.0 as f64, extent.1 as f64, extent.2 as f64);
            let gap = body.separation(obstacle);
            if gap > best.0 { best = (gap, scale, None); }
        }
        // A side contact can require counter-steering briefly (e.g. the middle
        // of a long bus beside a post). A horizon change alone cannot express it.
        for angle in [-self.max_steer, 0.0, self.max_steer] {
            let mut probe = self.clone();
            probe.recovery_steer = Some((angle, 0.75));
            for _ in 0..10 { probe.drive(0.1, 0.0, way); }
            probe.drive(0.1, 0.5, way);
            let body = crate::collision::Obb::vehicle(probe.position.truncate(), probe.heading,
                extent.0 as f64, extent.1 as f64, extent.2 as f64);
            let gap = body.separation(obstacle);
            if gap > best.0 + 1e-4 { best = (gap, 1.0, Some((angle, 0.75))); }
        }
        self.steering_recovery = 2.0 * self.wheelbase;
        self.recovery_preview_scale = best.1;
        self.recovery_steer = best.2;
    }

    /// Bicycle model: the rotation point follows the way, the front wheels steer towards a
    /// point further along it (pure pursuit, looking further ahead the faster the car goes).
    fn drive(&mut self, dt: f32, speed: f32, way: &dyn Fn(f32) -> DVec3) {
        let target = way(self.rot_long).truncate();
        if !self.started || (target - self.rear).length() > 8.0 {
            // spawned, or put somewhere else: stand on the way, straight
            let ahead = way(self.rot_long + 2.0).truncate();
            let d = ahead - target;
            self.heading = if d.length() > 1e-3 {
                d.x.atan2(d.y).to_degrees()
            } else {
                self.heading
            };
            self.rear = target;
            self.steer = 0.0;
            self.steer_cmd = 0.0;
            self.started = true;
        }
        let fwd = dir(self.heading);
        let right = DVec2::new(fwd.y, -fwd.x);
        // Route progress is committed from this body's motion. The old catch-up servo
        // amplified the already advanced command and fed that extra speed into the next
        // tick. Steering follows the way; only the longitudinal owner may choose speed.
        let v = speed.max(0.0);
        let normal_look = (1.2 * self.wheelbase).max(3.5) + 0.6 * speed.min(20.0);
        let look = if self.steering_recovery > 0.0 {
            (normal_look * self.recovery_preview_scale).max(3.5)
        } else { normal_look };
        let g = way(self.rot_long + look).truncate() - self.rear;
        let alpha = (g.dot(right) as f32)
            .atan2(g.dot(fwd) as f32)
            .clamp(-FRAC_PI_2, FRAC_PI_2);
        let reach = (g.length() as f32).max(1.0);
        // The way itself says how tight it bends here: a junction's turn laid tighter than
        // the model's `[inv_min_turnradius]` still has to be followed - capped at the
        // model's lock the car ran wide of it, through the kerb, the corner house and the
        // people on the pavement (#249; OMSI's AI keeps to its path). The lock the bend
        // needs, and a little more, is allowed, and the wheel may turn that much faster.
        let (a, b, c) = (
            way(self.rot_long).truncate(),
            way(self.rot_long + 0.5 * look).truncate(),
            way(self.rot_long + look).truncate(),
        );
        let (u, w) = (b - a, c - b);
        let turn = (u.x * w.y - u.y * w.x).atan2(u.dot(w)).abs() as f32;
        let bend_k = if look > 0.1 { 2.0 * turn / look } else { 0.0 };
        let need = (bend_k * self.wheelbase).atan().to_degrees() * 1.15;
        let limit = if std::env::var_os("OMSI_AI_MODEL_LOCK").is_some() {
            self.max_steer
        } else {
            self.max_steer.max(need.min(60.0))
        };
        // OMSI_DEBUG_AI_WIDE: every tenth of a second a car stands over 1.5 m beside its way
        if std::env::var_os("OMSI_DEBUG_AI_WIDE").is_some() {
            let off = ((target - self.rear).dot(right)).abs();
            if off > 1.5 {
                static N: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
                let n = N.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
                if n % 50 == 0 {
                    log::info!(
                        "ai wide: {off:.1} m beside its way at ({:.0}, {:.0}), bend needs {need:.0} deg, lock {:.0} ({} so far)",
                        self.rear.x,
                        self.rear.y,
                        self.max_steer,
                        n + 1
                    );
                }
            }
        }
        let pursuit = (2.0 * self.wheelbase * alpha.sin() / reach)
            .atan()
            .to_degrees()
            .clamp(-limit, limit);
        let want = self.recovery_steer.map(|r| r.0).unwrap_or(pursuit);
        if dt > 0.0 {
            let rate = self.steer_rate * dt * if limit > self.max_steer { 1.5 } else { 1.0 };
            self.steer_cmd += (want - self.steer_cmd).clamp(-rate, rate);
            self.steer += (self.steer_cmd - self.steer) * (dt / 0.08).min(1.0);
        }
        let k = self.steer.to_radians().tan() / self.wheelbase;
        let dpsi = (v * dt * k) as f64;
        let mid = dir(self.heading + dpsi.to_degrees() * 0.5);
        self.rear += mid * (v * dt) as f64;
        self.travelled += (v * dt).abs();
        self.steering_recovery = (self.steering_recovery - (v * dt).abs()).max(0.0);
        if let Some((angle, remaining)) = self.recovery_steer {
            self.recovery_steer = (remaining > (v * dt).abs())
                .then_some((angle, remaining - (v * dt).abs()));
        }
        self.heading = (self.heading + dpsi.to_degrees()).rem_euclid(360.0);
        self.yaw_rate = if dt > 0.0 {
            (dpsi / dt as f64) as f32
        } else {
            0.0
        };
        self.a_lat = v * v * k;
        let o = self.rear - dir(self.heading) * self.rot_long as f64;
        self.position.x = o.x;
        self.position.y = o.y;
    }

    /// The body on its springs over the ground under its wheels: the plane through the
    /// contact points is where the body wants to be, braking and cornering lean it off
    /// that plane, and the difference is how far each wheel moves in its arch.
    fn settle(
        &mut self,
        dt: f32,
        way: &dyn Fn(f32) -> DVec3,
        ground: Option<&dyn Fn(f64, f64) -> Option<f64>>,
        contact: Option<&dyn crate::rigid::Ground>,
    ) {
        let fwd = dir(self.heading);
        let right = DVec2::new(fwd.y, -fwd.x);
        let origin = self.position.truncate();
        let query = |p: DVec2, reference: f64, range: f64| match contact {
            Some(c) => c.road_height(p.x, p.y, reference, range),
            None => ground.and_then(|g| g(p.x, p.y))
                .filter(|z| z.is_finite() && (*z - reference).abs() <= range),
        };
        let (grade, crossfall) = self.ground.map(|(_, p, r)| (p.tan() as f64, -r.tan() as f64))
            .unwrap_or((0.0, 0.0));
        // the way's own height at each axle: the road the map says is there, which also
        // decides when a sampled height belongs to something else (a bridge over the road)
        let mut axle_z = vec![f64::NAN; self.axle_count];
        if self.contact_path_z.len() != self.wheels.len() {
            self.contact_path_z = vec![f64::NAN; self.wheels.len()];
        }
        // (lateral, longitudinal, axle, way height, contact reference, actual support)
        let mut samples: Vec<(f32, f32, usize, f64, f64, Option<f64>)> =
            Vec::with_capacity(self.wheels.len());
        for (wi, w) in self.wheels.iter().enumerate() {
            if axle_z[w.axle].is_nan() {
                axle_z[w.axle] = way(w.long).z;
            }
            let path_z = axle_z[w.axle];
            let p = origin + right * w.lat as f64 + fwd * w.long as f64;
            let previous = self.contact_z.get(wi).copied().filter(|z| z.is_finite());
            let path_delta = path_z - self.contact_path_z[wi];
            let reference = previous.map_or(path_z, |z| {
                z + if path_delta.is_finite() { path_delta.clamp(-0.5, 0.5) } else { 0.0 }
            });
            // DriveGround exposes both sides of the probe. Pick the nearest support on
            // this road level, including a drawn road above an imperfect path. Increasing
            // the query top alone would incorrectly prefer a bridge over the same road.
            let drawn = query(p, reference, AI_CONTACT_RANGE);
            samples.push((w.lat, w.long, w.axle, path_z, reference, drawn));
            self.contact_path_z[wi] = path_z;
        }
        if let Some((z, _, _)) = self.ground {
            let expected = |lat: f32, long: f32| z + grade * (long as f64 + self.travelled as f64)
                + crossfall * lat as f64;
            if samples.iter().any(|&(lat, long, _, _, _, h)|
                h.is_some_and(|h| (h - expected(lat, long)).abs() <= AI_CONTACT_DISCONTINUITY))
            {
                for (lat, long, _, _, _, h) in &mut samples {
                    if h.is_some_and(|h| (h - expected(*lat, *long)).abs() > AI_CONTACT_DISCONTINUITY) {
                        *h = None;
                    }
                }
            }
        }
        // Tiny seams can miss all four exact tyre queries. Confirm the same road plane
        // on nearby real faces before allowing a lower terrain/other level to take over.
        if self.ground.is_some() && samples.iter().all(|s| s.5.is_none()) {
            for (lat, long, _, _, reference, drawn) in &mut samples {
                let p = origin + right * *lat as f64 + fwd * *long as f64;
                *drawn = [0.5, -0.5, 1.0, -1.0, 2.0, -2.0].into_iter().find_map(|d|
                    query(p + fwd * d, *reference + grade * d, AI_CONTACT_RANGE)
                        .map(|h| h - grade * d));
            }
        }
        // Reacquire a displaced road only when the entire body has no nearby support.
        // If the rear axle still stands on the street at a road/tile end, its plane carries
        // the front; the lower terrain beyond the asphalt is not a second road level.
        if samples.iter().all(|s| s.5.is_none()) {
            for (lat, long, _, _, reference, drawn) in &mut samples {
                let p = origin + right * *lat as f64 + fwd * *long as f64;
                *drawn = query(p, *reference, AI_CONTACT_REACQUIRE);
            }
        }
        let mut axle_support = vec![None::<f64>; self.axle_count];
        for &(_, _, a, _, _, drawn) in &samples {
            if let Some(h) = drawn {
                axle_support[a] = Some(axle_support[a].map_or(h, |old| old.min(h)));
            }
        }
        let supported = (contact.is_none() && ground.is_none())
            || axle_support.iter().any(Option::is_some);
        if self.ground_supported && !supported && std::env::var_os("OMSI_DEBUG_AI_GROUND").is_some() {
            for &(lat, long, _, path_z, _, drawn) in &samples {
                if drawn.is_none() {
                    let p = origin + right * lat as f64 + fwd * long as f64;
                    let probe = contact.map(|g| g.probe(p.x, p.y, path_z));
                    log::warn!("AI ground lost at ({:.3}, {:.3}), path {path_z:.3}, probe {probe:?}", p.x, p.y);
                }
            }
        }
        self.ground_supported = supported;
        let (sum, count) = samples.iter().filter_map(|&(lat, long, _, _, _, h)|
            h.map(|h| h - grade * long as f64 - crossfall * lat as f64))
            .fold((0.0, 0usize), |(sum, n), h| (sum + h, n + 1));
        let supported_plane = (count > 0).then(|| sum / count as f64);
        let contacts: Vec<(f32, f32, f64)> = samples
            .iter()
            .enumerate()
            .map(|(wi, &(lat, long, a, path_z, _, drawn))| {
                // A missing wheel does not veto the other wheel's real axle support.
                // With no support at all, preserve the last contact through streaming gaps.
                let h = drawn.or(axle_support[a])
                    .or_else(|| supported_plane.map(|z| z + grade * long as f64 + crossfall * lat as f64))
                    .or_else(|| self.contact_z.get(wi).copied().filter(|z| z.is_finite()))
                    .unwrap_or(path_z);
                (lat, long, h)
            })
            .collect();
        let mut contacts = contacts;
        // each contact followed with a short lag (a tyre rolls over a texel step, it does
        // not jump onto it); a car put somewhere else starts from where it stands
        if self.contact_z.len() != contacts.len() {
            self.contact_z = vec![f64::NAN; contacts.len()];
        }
        let follow = if dt > 0.0 {
            (dt / 0.07).min(1.0) as f64
        } else {
            1.0
        };
        for (c, z) in contacts.iter_mut().zip(self.contact_z.iter_mut()) {
            if z.is_nan() || (c.2 - *z).abs() > 0.3 {
                *z = c.2;
            } else {
                *z += (c.2 - *z) * follow;
            }
            c.2 = *z;
        }
        // least-squares plane h = a + b·long + c·lat (the wheels sit symmetrically, so the
        // two slopes separate)
        let n = contacts.len().max(1) as f64;
        let (ml, mt, mh) = contacts.iter().fold((0.0, 0.0, 0.0), |acc, c| {
            (
                acc.0 + c.1 as f64 / n,
                acc.1 + c.0 as f64 / n,
                acc.2 + c.2 / n,
            )
        });
        let (mut sll, mut slh, mut stt, mut sth) = (0.0, 0.0, 0.0, 0.0);
        for c in &contacts {
            let (dl, dt_, dh) = (c.1 as f64 - ml, c.0 as f64 - mt, c.2 - mh);
            sll += dl * dl;
            slh += dl * dh;
            stt += dt_ * dt_;
            sth += dt_ * dh;
        }
        let b = if sll > 1e-6 { slh / sll } else { 0.0 };
        let c = if stt > 1e-6 { sth / stt } else { 0.0 };
        let zg = mh - b * ml - c * mt;
        // pitch up for a road rising ahead; bank positive = right side down (c < 0)
        let pg = b.atan() as f32;
        let rg = (-c).atan() as f32;
        match self.ground {
            Some((z0, p0, r0)) if (zg - self.z).abs() < 1.0 && dt > 0.0 => {
                let k = (dt / 0.1).min(1.0);
                let rates = [
                    ((zg - z0) / dt as f64) as f32,
                    (pg - p0) / dt,
                    (rg - r0) / dt,
                ];
                for (r, new) in self.ground_rate.iter_mut().zip(rates) {
                    *r += (new.clamp(-5.0, 5.0) - *r) * k;
                }
                let steps = 4;
                let h = dt / steps as f32;
                for _ in 0..steps {
                    let az = (self.spring[0] * (zg - self.z) as f32
                        + self.damper[0] * (self.ground_rate[0] - self.vz))
                        / self.mass;
                    let ap = (self.spring[1] * (pg - self.pitch)
                        + self.damper[1] * (self.ground_rate[1] - self.vpitch)
                        + self.mass * self.a_long * self.cog)
                        / self.inertia[0];
                    let ar = (self.spring[2] * (rg - self.roll)
                        + self.damper[2] * (self.ground_rate[2] - self.vroll)
                        - self.mass * self.a_lat * self.cog)
                        / self.inertia[1];
                    self.vz += az * h;
                    self.vpitch += ap * h;
                    self.vroll += ar * h;
                    self.z += (self.vz * h) as f64;
                    self.pitch += self.vpitch * h;
                    self.roll += self.vroll * h;
                }
                self.z = self.z.clamp(zg - 0.15, zg + 0.15);
                self.pitch = self.pitch.clamp(pg - 0.07, pg + 0.07);
                self.roll = self.roll.clamp(rg - 0.09, rg + 0.09);
            }
            _ => {
                // first frame, or the ground under the car changed completely
                self.z = zg;
                self.pitch = pg;
                self.roll = rg;
                self.vz = 0.0;
                self.vpitch = 0.0;
                self.vroll = 0.0;
                self.ground_rate = [0.0; 3];
            }
        }
        self.ground = Some((zg, pg, rg));
        self.position.z = self.z;
        self.pitch_deg = self.pitch.to_degrees();
        self.bank_deg = self.roll.to_degrees();
        let (tp, tr) = (self.pitch.tan(), self.roll.tan());
        for (w, c) in self.wheels.iter().zip(&contacts) {
            let body = self.z + (w.long * tp - w.lat * tr) as f64;
            if let Some(s) = self.suspension.get_mut(w.axle) {
                s[w.side] = ((body - c.2) as f32).clamp(-0.15, 0.15);
            }
        }
    }

    /// A rail vehicle: its outermost axles stand on the track, the body is the line
    /// between them.
    fn ride(&mut self, way: &dyn Fn(f32) -> DVec3) {
        let f = way(self.front_long);
        let r = way(self.rear_long);
        let d = f - r;
        let run = d.truncate().length();
        if run > 1e-3 {
            self.heading = d.x.atan2(d.y).to_degrees().rem_euclid(360.0);
            self.pitch_deg = (d.z / run).atan().to_degrees() as f32;
        }
        let span = (self.front_long - self.rear_long).max(0.1);
        self.position = r + d * ((0.0 - self.rear_long) / span) as f64;
        self.bank_deg = 0.0;
        self.steer = 0.0;
        self.started = true;
    }

    /// An aircraft: on its path, nose along it, banking into a turn as much as the turn
    /// asks for (tan bank = speed × turn rate / g).
    fn fly(&mut self, dt: f32, speed: f32, way: &dyn Fn(f32) -> DVec3) {
        let p = way(0.0);
        let d = way(15.0) - way(-15.0);
        let run = d.truncate().length();
        let (h_t, p_t) = if run > 1e-3 {
            (
                d.x.atan2(d.y).to_degrees(),
                (d.z / run).atan().to_degrees() as f32,
            )
        } else {
            (self.heading, self.pitch_deg)
        };
        if !self.started || dt <= 0.0 {
            self.heading = h_t.rem_euclid(360.0);
            self.pitch_deg = p_t;
            self.bank_deg = 0.0;
            self.yaw_rate = 0.0;
            self.started = true;
        } else {
            let k = (dt / 0.6).min(1.0) as f64;
            let dh = wrap(h_t - self.heading) * k;
            self.heading = (self.heading + dh).rem_euclid(360.0);
            let rate = (dh.to_radians() / dt as f64) as f32;
            self.yaw_rate += (rate - self.yaw_rate) * (dt / 0.5).min(1.0);
            let bank = (speed * self.yaw_rate / 9.81)
                .atan()
                .to_degrees()
                .clamp(-30.0, 30.0);
            let k = (dt / 0.8).min(1.0);
            self.bank_deg += (bank - self.bank_deg) * k;
            self.pitch_deg += (p_t - self.pitch_deg) * k;
        }
        self.position = p;
        self.steer = 0.0;
    }

    /// Drive a copy of this body along a way it has not taken yet and report how close it
    /// comes to `obstacles`: the smallest plan-view separation (m, negative = the bodies
    /// would touch) between the vehicle's box (`front`/`rear` from its origin, `half_width`)
    /// and the nearest of them while the vehicle covers `distance` metres. `way(d)` is the point `d`
    /// metres along the new way from where the vehicle is now (the same way `step` is given,
    /// e.g. with a pull-out round something standing ahead in it); the vehicle starts at
    /// `speed` (a standing one first turns its wheels for `wait` seconds, as a driver does
    /// before moving off) and speeds up at `accel` to at most `v_max`. The steering is the real one -
    /// its rate limit, its lock and the pure pursuit that cuts in on a steep S-bend - which
    /// is what decides whether a car standing a few metres behind a bus can get its front
    /// corner past the bus's.
    #[allow(clippy::too_many_arguments)]
    pub fn sweep_clearance(
        &self,
        way: &dyn Fn(f32) -> DVec3,
        speed: f32,
        wait: f32,
        accel: f32,
        v_max: f32,
        distance: f32,
        extent: (f32, f32, f32),
        obstacles: &[crate::collision::Obb],
    ) -> f64 {
        let mut body = self.clone();
        let (front, rear, half_width) = extent;
        let dt = 1.0 / 20.0;
        let mut v = speed.max(0.0);
        let mut x = 0.0f32;
        let gap = |b: &AiBody| {
            let me = crate::collision::Obb::vehicle(
                b.position.truncate(),
                b.heading,
                front as f64,
                rear as f64,
                half_width as f64,
            );
            obstacles
                .iter()
                .map(|o| me.separation(o))
                .fold(f64::MAX, f64::min)
        };
        let mut best = gap(&body);
        if v < 0.05 {
            for _ in 0..(wait / dt).round() as usize {
                body.drive(dt, 0.0, way);
            }
        }
        // (at most a minute of simulated time: a car that cannot move is not getting past)
        for _ in 0..1200 {
            if x >= distance {
                break;
            }
            v = (v + accel.max(0.3) * dt).min(v_max.max(0.5));
            let ds = v * dt;
            x += ds;
            let at = x;
            body.drive(dt, v, &|d| way(at + d));
            best = best.min(gap(&body));
        }
        best
    }

    /// Hand the pose to the vehicle: position and attitude, the accelerations its scripts
    /// read and each wheel's travel.
    pub fn apply(&self, v: &mut VehicleInstance) {
        // A road vehicle's body stands on the contact plane of its wheels; its static pose
        // over that plane (`[ai_deltaheight]` and the model's wheel geometry) comes on top,
        // once: the springs here only add their travel about it. Rail and air vehicles
        // follow their path's own height.
        let (lift, delta) = if self.kind == MotionKind::Road {
            v.ai_rest_offset()
        } else {
            (0.0, 0.0)
        };
        v.position = self.position + DVec3::new(0.0, 0.0, lift as f64);
        v.heading = self.heading;
        v.pitch = self.pitch_deg;
        v.bank = self.bank_deg;
        v.physics.accel = Vec3::new(self.a_lat, self.a_long, 0.0);
        for (axle, s) in v.physics.wheels.iter_mut().zip(&self.suspension) {
            axle[0].suspension = s[0] + delta;
            axle[1].suspension = s[1] + delta;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use ::legacy_vehicle::vehicle::Axle;

    #[test]
    fn back_in_ramp_keeps_the_sideways_acceleration() {
        for (offset, kmh) in [(3.28f32, 40.0f32), (3.28, 44.0), (2.5, 34.0), (5.0, 50.0)] {
            let v = kmh / 3.6;
            let len = back_in_ramp(offset, v, BACK_IN_LAT_ACCEL);
            // the peak of the smoothstep's second derivative, at its ends
            let peak = 6.0 * offset * v * v / (len * len);
            assert!(
                peak <= BACK_IN_LAT_ACCEL + 1e-3,
                "{offset} m at {kmh} km/h: {len:.1} m, {peak:.2} m/s²"
            );
            assert!(
                len / v >= 2.0,
                "{offset} m at {kmh} km/h: {len:.1} m is {:.2} s",
                len / v
            );
        }
        // creeping round something: the old geometric length
        assert_eq!(back_in_ramp(3.0, 1.0, BACK_IN_LAT_ACCEL), 10.5);
    }

    pub(super) fn golf() -> Vehicle {
        let mut v = Vehicle {
            mass: 1.0,
            moment_of_inertia: [1.49, 0.40, 1.56],
            cog_height: 0.4,
            rot_pnt_long: -1.304,
            inv_min_turn_radius: 0.1904,
            ..Default::default()
        };
        v.axles = vec![
            Axle {
                long: 1.21,
                max_width: 1.606,
                spring: 40.0,
                damper: 2.0,
                wheel_diameter: 0.503,
                ..Default::default()
            },
            Axle {
                long: -1.304,
                max_width: 1.606,
                spring: 50.0,
                damper: 2.0,
                wheel_diameter: 0.503,
                ..Default::default()
            },
        ];
        v
    }

    /// A way: straight north for 50 m, then a right turn of radius `r`.
    fn bend(r: f64, s: f64) -> DVec3 {
        if s < 50.0 {
            DVec3::new(0.0, s, 10.0)
        } else {
            let a = (s - 50.0) / r;
            DVec3::new(r - r * a.cos(), 50.0 + r * a.sin(), 10.0)
        }
    }

    #[test]
    fn follows_a_bend_smoothly() {
        let def = golf();
        let mut body = AiBody::new(&def, MotionKind::Road);
        let speed = 6.0f32;
        let dt = 1.0 / 30.0;
        let mut s = 20.0f64;
        body.place(&|d| bend(12.0, s + d as f64), None, None, speed);
        let mut last_yaw: Option<f32> = None;
        let mut worst_jump = 0.0f32;
        let mut worst_off = 0.0f64;
        for _ in 0..(12.0 / dt) as usize {
            s += (speed * dt) as f64;
            let at = s;
            body.step(dt, speed, &|d| bend(12.0, at + d as f64), None, None);
            if let Some(y) = last_yaw {
                worst_jump = worst_jump.max((body.yaw_rate - y).abs().to_degrees());
            }
            last_yaw = Some(body.yaw_rate);
            // the rear axle stays on the way (a circle has no steady error; entering it costs a little)
            let rear = body.rear;
            let q = bend(12.0, at + def.rot_pnt_long as f64).truncate();
            worst_off = worst_off.max((rear - q).length());
        }
        // a quarter turn done at 6 m/s: the heading has turned right, the yaw rate never
        // jumps by more than a few degrees per second between frames, the car keeps to its lane
        assert!(
            body.heading > 60.0 && body.heading < 300.0,
            "heading {}",
            body.heading
        );
        assert!(
            worst_jump < 3.0,
            "yaw rate jumps by {worst_jump} deg/s in one frame"
        );
        assert!(worst_off < 0.6, "rear axle {worst_off} m off the way");
    }

    #[test]
    fn brakes_nose_down_and_rolls_out_of_a_turn() {
        let def = golf();
        let mut body = AiBody::new(&def, MotionKind::Road);
        let flat = |_x: f64, _y: f64| Some(10.0);
        body.place(
            &|d| DVec3::new(0.0, d as f64, 10.0),
            Some(&flat),
            None,
            10.0,
        );
        let dt = 1.0 / 30.0;
        let mut speed = 10.0f32;
        let mut s = 0.0f64;
        let mut min_pitch = 0.0f32;
        for _ in 0..60 {
            speed = (speed - 3.0 * dt).max(0.0);
            s += (speed * dt) as f64;
            let at = s;
            body.step(
                dt,
                speed,
                &|d| DVec3::new(0.0, at + d as f64, 10.0),
                Some(&flat),
                None,
            );
            min_pitch = min_pitch.min(body.pitch_deg);
        }
        assert!(min_pitch < -0.1, "braking pitch {min_pitch}");
        assert!((body.position.z - 10.0).abs() < 0.1);
        // the front wheels come up in their arches while the nose dives
        assert!(
            body.suspension[0][0] < 0.0 && body.suspension[1][0] > 0.0,
            "{:?}",
            body.suspension
        );
    }

    /// A two-axle lorry: 4.5 m between the axles, a turning circle of 11 m radius.
    pub(super) fn lorry() -> Vehicle {
        let mut v = Vehicle {
            mass: 12.0,
            moment_of_inertia: [40.0, 10.0, 45.0],
            cog_height: 1.2,
            rot_pnt_long: -2.0,
            inv_min_turn_radius: 0.09,
            ..Default::default()
        };
        v.axles = vec![
            Axle {
                long: 2.5,
                max_width: 2.0,
                spring: 300.0,
                damper: 20.0,
                wheel_diameter: 1.0,
                ..Default::default()
            },
            Axle {
                long: -2.0,
                max_width: 1.8,
                spring: 400.0,
                damper: 25.0,
                wheel_diameter: 1.0,
                ..Default::default()
            },
        ];
        v
    }

    /// A standing bus `half_width` × 2 wide, `gap` metres ahead of a vehicle whose front is `front`
    /// metres ahead of its origin at (0, 0) facing north.
    fn bus_ahead(front: f32, gap: f32, half_width: f64) -> crate::collision::Obb {
        crate::collision::Obb::vehicle(
            DVec2::new(0.0, (front + gap) as f64 + 6.0),
            0.0,
            6.0,
            6.0,
            half_width,
        )
    }

    #[test]
    fn a_car_too_close_behind_a_bus_cannot_steer_round_it() {
        let def = golf();
        let extent = (2.1, 2.2, 0.85);
        let best = |gap: f32| {
            pull_out_ramps(gap, extent.0, false)
                .into_iter()
                .map(|ramp| {
                    let way = straight_pull_out(3.3, ramp);
                    let mut body = AiBody::new(&def, MotionKind::Road);
                    body.place(&way, None, None, 0.0);
                    body.sweep_clearance(
                        &way,
                        0.0,
                        PULL_OUT_WAIT,
                        PULL_OUT_ACCEL,
                        8.0,
                        gap + 6.0,
                        extent,
                        &[bus_ahead(extent.0, gap, 1.25)],
                    )
                })
                .fold(f64::MIN, f64::max)
        };
        // half a metre behind the bus no S-curve gets the front corner past (the lock
        // is 25°, the steering takes a moment to get there); six metres back it is easy
        let close = best(0.5);
        assert!(close < PULL_OUT_CLEARANCE, "clearance {close} from 0.5 m");
        let far = best(6.0);
        assert!(far >= PULL_OUT_CLEARANCE, "clearance {far} from 6 m");
        // and the swept body does leave the lane: after the pull-out it is 3.3 m over
        let way = straight_pull_out(3.3, 8.0);
        let mut body = AiBody::new(&def, MotionKind::Road);
        body.place(&way, None, None, 0.0);
        let mut x = 0.0f32;
        let dt = 1.0 / 20.0;
        for _ in 0..400 {
            x += 4.0 * dt;
            let at = x;
            body.step(dt, 4.0, &|d| way(at + d), None, None);
        }
        assert!(
            (body.position.x + 3.3).abs() < 0.2,
            "ended at x {}",
            body.position.x
        );
    }

    #[test]
    fn room_to_pull_out_depends_on_the_steering() {
        let car = pull_out_room(&golf(), (2.1, 2.2, 0.85), 1.25, 3.3);
        let lorry = pull_out_room(&lorry(), (3.6, 3.4, 1.25), 1.25, 3.3);
        // a bus standing a little further over (or wider) needs more room
        let wide = pull_out_room(&golf(), (2.1, 2.2, 0.85), 1.6, 3.3);
        assert!(wide > car, "{wide} against {car}");
        assert!(car > 1.5 && car < 6.0, "car {car}");
        assert!(
            lorry > car + 0.5 && lorry < 12.5,
            "lorry {lorry}, car {car}"
        );
        // what it found works: from that room less the half metre of slack one of the
        // curves clears, from a metre less none does
        let clears = |gap: f32| {
            pull_out_ramps(gap, 2.1, false).into_iter().any(|ramp| {
                let way = straight_pull_out(3.3, ramp);
                let mut body = AiBody::new(&golf(), MotionKind::Road);
                body.place(&way, None, None, 0.0);
                body.sweep_clearance(
                    &way,
                    0.0,
                    PULL_OUT_WAIT,
                    PULL_OUT_ACCEL,
                    8.0,
                    gap + 6.0,
                    (2.1, 2.2, 0.85),
                    &[bus_ahead(2.1, gap, 1.25)],
                ) >= PULL_OUT_CLEARANCE
            })
        };
        assert!(clears(car - 0.5));
        assert!(!clears(car - 1.0));
    }

    #[test]
    fn pull_out_curves_run_gentle_to_steep() {
        let r = pull_out_ramps(3.0, 2.0, false);
        assert!(r.windows(2).all(|w| w[0] > w[1]), "{r:?}");
        assert!(r.iter().all(|&x| (4.0..=12.0).contains(&x)), "{r:?}");
        let rolling = pull_out_ramps(30.0, 2.0, true);
        assert_eq!(rolling[0], 20.0);
        assert!(
            rolling.iter().all(|&x| (4.0..=20.0).contains(&x)),
            "{rolling:?}"
        );
    }

    #[test]
    fn aircraft_keep_their_height() {
        let mut def = golf();
        def.axles.truncate(1);
        let mut body = AiBody::new(&def, MotionKind::Air);
        let way = |d: f32| DVec3::new(0.0, d as f64, 300.0 - d as f64 * 0.05);
        body.place(&way, None, None, 70.0);
        body.step(1.0 / 30.0, 70.0, &way, None, None);
        assert!((body.position.z - 300.0).abs() < 1e-6);
        assert!(
            body.pitch_deg < -2.0 && body.pitch_deg > -4.0,
            "pitch {}",
            body.pitch_deg
        );
    }
}

#[cfg(test)]
#[path = "ai_motion/ground_tests.rs"]
mod ground_tests;

#[cfg(test)]
#[path = "ai_motion/traffic_regression_tests.rs"]
mod traffic_regression_tests;
