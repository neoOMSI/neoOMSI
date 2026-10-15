use super::{BODY_HEIGHT, CLIMB_RATE, GRAVITY, OnFoot, RADIUS, SNAP_DOWN, STEP_UP};
use crate::App;
use glam::DVec2;
use ::simulation::collision::Obb;

const MAX_SUBSTEP: f64 = 0.12;
const MAX_SEEN: f64 = 1.0;
const TOUCH: f64 = 0.005;
/// Cos of the steepest slope (50°) climbed more than [`STEP_UP`] over [`OnFoot::flat`] (#331).
const MIN_FLOOR_NZ: f32 = 0.643;
/// Rounding margin for a too steep face over the feet (m).
const STEEP_RISE: f64 = 1e-4;
/// Radius in which [`OnFoot::flat`] counts (m).
const FLAT_NEAR: f64 = 1.0;

/// Ground height and face normal (none from the raster).
pub(super) type Floor = (f64, Option<glam::Vec3>);

fn steep(n: Option<glam::Vec3>) -> bool {
    n.is_some_and(|n| n.z < MIN_FLOOR_NZ * n.length())
}

#[allow(dead_code)]
#[derive(Default, Clone)]
pub(crate) struct MoveDbg {
    pub exempt: Option<DVec2>,
    pub boxes: usize,
    pub lost: DVec2,
    pub step_wall: bool,
    pub reset: bool,
    pub floor: Option<f64>,
}

pub(super) fn probe_box(center: DVec2, half: f64, feet: f64) -> Obb {
    Obb {
        center,
        half: DVec2::splat(half),
        heading: 0.0,
        z0: feet - 1.0,
        z1: feet + 2.5,
        velocity: DVec2::ZERO,
        mass: 0.0,
        pole: None,
        id: -1,
    }
}

pub(super) fn push_out(p: DVec2, o: &Obb, r: f64) -> DVec2 {
    let (s, c) = o.heading.sin_cos();
    let right = DVec2::new(c, -s);
    let fwd = DVec2::new(s, c);
    let d = p - o.center;
    let (lx, ly) = (d.dot(right), d.dot(fwd));
    let (hx, hy) = (o.half.x + r, o.half.y + r);
    if lx.abs() >= hx || ly.abs() >= hy {
        return p;
    }
    if hx - lx.abs() < hy - ly.abs() {
        let side = if lx >= 0.0 { 1.0 } else { -1.0 };
        o.center + right * (hx * side) + fwd * ly
    } else {
        let side = if ly >= 0.0 { 1.0 } else { -1.0 };
        o.center + right * lx + fwd * (hy * side)
    }
}

pub(super) fn penetrates(p: DVec2, boxes: &[Obb]) -> bool {
    boxes
        .iter()
        .any(|o| (push_out(p, o, RADIUS) - p).length() > TOUCH)
}

pub(super) fn depenetrate(mut p: DVec2, boxes: &[Obb]) -> Option<DVec2> {
    for _ in 0..6 {
        let mut moved = false;
        for o in boxes {
            let q = push_out(p, o, RADIUS);
            if (q - p).length() > 1e-9 {
                p = q;
                moved = true;
            }
        }
        if !moved {
            return Some(p);
        }
    }
    if penetrates(p, boxes) { None } else { Some(p) }
}

#[allow(dead_code)]
pub(super) struct Slide {
    pub pos: DVec2,
    pub blocked: bool,
}

pub(super) fn slide(from: DVec2, to: DVec2, boxes: &[Obb]) -> Slide {
    let d = to - from;
    let len = d.length();
    if boxes.is_empty() || len < 1e-9 {
        return Slide {
            pos: to,
            blocked: false,
        };
    }
    let n = (len / MAX_SUBSTEP).ceil().clamp(1.0, 32.0) as usize;
    let step = d / n as f64;
    let (mut p, mut blocked) = (from, false);
    for _ in 0..n {
        let want = p + step;
        match depenetrate(want, boxes) {
            Some(q) => {
                if (q - want).length() > 1e-6 {
                    blocked = true;
                }
                p = q;
            }
            None => {
                blocked = true;
                break;
            }
        }
    }
    Slide { pos: p, blocked }
}

pub(super) struct MoveEnv<'a> {
    pub solids: &'a dyn Fn(DVec2, f64) -> Vec<Obb>,
    pub ground: &'a dyn Fn(DVec2, f64, f64) -> Option<Floor>,
}

pub(super) fn move_body(f: &mut OnFoot, dt: f64, env: &MoveEnv) {
    let from = f.pos.truncate();
    let feet = f.pos.z + f.lift;
    let delta = f.vel * dt;
    let boxes = (env.solids)(from + delta, feet);
    let dir = delta.normalize_or_zero();
    // from the feet, a steep face was climbed a little every frame
    let base = if (f.flat.truncate() - from).length() < FLAT_NEAR {
        f.flat.z.min(feet)
    } else {
        feet
    };
    let steep_wall = |(g, n): Floor| steep(n) && g - feet > STEEP_RISE && g - base > STEP_UP;
    let high = |fl: Floor| fl.0 - feet > STEP_UP || steep_wall(fl);
    let seen = |q: DVec2| (env.ground)(q, feet, MAX_SEEN);
    let walkable = |q: DVec2| {
        let here = seen(q);
        dir == DVec2::ZERO
            || !(here.is_some_and(steep_wall)
                || (here.is_some_and(high) && seen(q + dir * (RADIUS + 0.5)).is_some_and(high)))
    };

    let full = slide(from, from + delta, &boxes);
    let step_wall = !walkable(full.pos);
    let mut p = if !step_wall {
        full.pos
    } else {
        let mut q = from;
        for axis in [DVec2::new(delta.x, 0.0), DVec2::new(0.0, delta.y)] {
            if axis == DVec2::ZERO {
                continue;
            }
            let s = slide(q, q + axis, &boxes);
            if walkable(s.pos) {
                q = s.pos;
            }
        }
        q
    };

    let mut reset = false;
    p = match depenetrate(p, &boxes) {
        Some(q) => q,
        None => {
            reset = true;
            f.safe.truncate()
        }
    };

    let lost = from + delta - p;
    if delta.length_squared() > 1e-12 && lost.length() > 1e-6 {
        let n = lost.normalize();
        let into = f.vel.dot(n);
        if into > 0.0 {
            f.vel -= n * into;
        }
    }

    let floor = (env.ground)(p, feet, STEP_UP).filter(|fl| !high(*fl));
    f.dbg.boxes = boxes.len();
    f.dbg.lost = lost;
    f.dbg.step_wall = step_wall;
    f.dbg.reset = reset;
    f.dbg.floor = floor.map(|(g, _)| g);
    vertical(f, floor.map(|(g, _)| g), dt);
    f.pos.x = p.x;
    f.pos.y = p.y;
    if f.grounded() && floor.is_some_and(|(_, n)| !steep(n)) {
        f.flat = f.pos;
    }

    if f.grounded() && !penetrates(p, &boxes) {
        f.safe = f.pos;
    }
}

pub(super) fn vertical(f: &mut OnFoot, ground: Option<f64>, dt: f64) {
    let was_grounded = f.grounded();
    let mut feet = f.pos.z + f.lift;
    if !was_grounded {
        f.vz = (f.vz - GRAVITY * dt).clamp(-60.0, 20.0);
        feet += f.vz * dt;
    }
    match ground {
        Some(g) => {
            let mut base = g;
            if feet <= g + 1e-3 && f.vz <= 0.0 {
                if was_grounded && g > feet + 1e-3 {
                    base = (feet + CLIMB_RATE * dt).min(g);
                }
                feet = base;
                f.vz = 0.0;
            } else if was_grounded && feet - g < SNAP_DOWN {
                feet = g;
            }
            f.pos.z = base;
            f.lift = (feet - base).max(0.0);
        }
        None => {
            f.lift = (feet - f.pos.z).max(0.0);
            if f.lift == 0.0 && f.vz < 0.0 {
                f.vz = 0.0;
            }
        }
    }
}

impl App {
    pub(super) fn foot_solids(&self, at: DVec2, feet: f64, exempt: Option<DVec2>) -> Vec<Obb> {
        let mut boxes: Vec<Obb> = Vec::new();
        if let Some(w) = self.world.as_ref() {
            let probe = probe_box(at, 3.0, feet);
            boxes.extend(w.collision.lock().obstacles_near(&probe));
        }
        let mut vehicles = self.vehicle_boxes(at, 20.0);
        if let Some(e) = exempt {
            vehicles.retain(|o| (push_out(e, o, 0.8) - e).length() < 1e-6);
        }
        boxes.extend(vehicles);
        boxes.retain(|o| o.z0 < feet + BODY_HEIGHT - 0.2 && o.z1 > feet + STEP_UP);
        boxes
    }

    pub(super) fn foot_ground(&self, p: DVec2, feet: f64, reach: f64) -> Option<Floor> {
        self.world
            .as_ref()
            .and_then(|w| w.walk_floor_reach(p.x, p.y, feet, reach))
    }
}

#[cfg(all(feature = "devtools", debug_assertions))]
impl App {
    pub(crate) fn dev_hitboxes(&self, at: glam::DVec3, radius: f64) -> Vec<Obb> {
        let at2 = DVec2::new(at.x, at.y);
        let mut boxes: Vec<Obb> = Vec::new();
        if let Some(w) = self.world.as_ref() {
            let probe = probe_box(at2, radius, at.z);
            boxes.extend(w.collision.lock().obstacles_near(&probe));
        }
        boxes.extend(self.vehicle_boxes(at2, radius + 20.0).into_iter().map(|mut o| {
            o.id = -2;
            o
        }));
        boxes.retain(|o| (o.center - at2).length() <= radius + o.half.length());
        boxes.truncate(2000);
        boxes
    }

    pub(crate) fn dev_blockers(&self, exempt: Option<DVec2>) -> Vec<Obb> {
        let Some(f) = self.on_foot.as_ref() else {
            return Vec::new();
        };
        let at = DVec2::new(f.pos.x, f.pos.y);
        let veh = self.vehicle_boxes(at, 20.0);
        let same = |a: &Obb, b: &Obb| {
            (a.center - b.center).length() < 1e-9
                && (a.half - b.half).length() < 1e-9
                && (a.heading - b.heading).abs() < 1e-9
        };
        self.foot_solids(at, f.pos.z + f.lift, exempt)
            .into_iter()
            .filter(|o| (push_out(at, o, RADIUS + 0.3) - at).length() > 1e-6)
            .map(|mut o| {
                if veh.iter().any(|v| same(v, &o)) {
                    o.id = -2;
                }
                o
            })
            .collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use glam::{DVec3, Vec3};

    /// Height and normal `d` m along a `deg` slope.
    fn slope(deg: f64, d: f64) -> (f64, Vec3) {
        let t = deg.to_radians();
        (d * t.tan(), Vec3::new(-t.sin() as f32, 0.0, t.cos() as f32))
    }

    /// Walks east from flat ground onto `profile(x - 1)` past x = 1; returns the feet.
    fn walk_east(secs: f64, hz: f64, profile: impl Fn(f64) -> (f64, Vec3)) -> DVec3 {
        let mut f = OnFoot::new(DVec3::ZERO, 90.0);
        let solids = |_: DVec2, _: f64| Vec::new();
        let ground = |p: DVec2, feet: f64, reach: f64| {
            let (h, n) = profile(p.x - 1.0);
            Some(if p.x > 1.0 && h > 0.0 && h <= feet + reach {
                (h, Some(n))
            } else {
                (0.0, Some(Vec3::Z))
            })
        };
        let env = MoveEnv {
            solids: &solids,
            ground: &ground,
        };
        let dt = 1.0 / hz;
        for _ in 0..(secs * hz) as usize {
            f.vel = DVec2::new(1.45, 0.0);
            move_body(&mut f, dt, &env);
        }
        f.pos
    }

    #[test]
    fn a_walker_does_not_climb_a_steep_slope() {
        // #331: terrain rising to a covered station's deck
        for hz in [30.0, 60.0, 240.0] {
            let p = walk_east(4.0, hz, |d| slope(60.0, d));
            assert!(p.z <= STEP_UP + 1e-6, "{hz} Hz: climbed the wall to {p}");
            assert!(p.x < 1.3, "{hz} Hz: walked into the wall to {p}");
        }
    }

    #[test]
    fn a_walker_goes_up_a_ramp() {
        let p = walk_east(3.0, 60.0, |d| slope(20.0, d));
        assert!(p.x > 4.0 && p.z > 1.0, "stopped on the ramp at {p}");
    }

    #[test]
    fn a_walker_steps_up_a_kerb() {
        let p = walk_east(2.0, 60.0, |_| (0.15, Vec3::Z));
        assert!(p.x > 2.5 && (p.z - 0.15).abs() < 1e-6, "kerb: {p}");
    }

    #[test]
    fn a_walker_steps_up_a_bevelled_kerb() {
        let bevel = |d: f64| match slope(70.0, d) {
            (h, n) if h < 0.15 => (h, n),
            _ => (0.15, Vec3::Z),
        };
        let p = walk_east(2.0, 60.0, bevel);
        assert!(p.x > 2.5 && (p.z - 0.15).abs() < 1e-6, "bevelled kerb: {p}");
    }
}
