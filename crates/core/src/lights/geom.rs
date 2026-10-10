use super::tuning::*;
use ::simulation::collision::{CollisionWorld, Obb};
use ::simulation::VehicleInstance;
use glam::{DVec2, DVec3, Mat4, Vec3};

pub(super) fn seg_hit(a: DVec3, b: DVec3, o: &Obb) -> Option<f64> {
    let [r, f] = o.axes();
    let local = |p: DVec3| {
        let rel = p.truncate() - o.center;
        DVec2::new(rel.dot(r), rel.dot(f))
    };
    let (la, lb) = (local(a), local(b));
    let d = lb - la;
    let (mut t0, mut t1) = (0.0f64, 1.0f64);
    for (p, dd, h) in [(la.x, d.x, o.half.x), (la.y, d.y, o.half.y)] {
        if dd.abs() < 1e-9 {
            if p.abs() > h {
                return None;
            }
            continue;
        }
        let (u0, u1) = ((-h - p) / dd, (h - p) / dd);
        t0 = t0.max(u0.min(u1));
        t1 = t1.min(u0.max(u1));
        if t0 > t1 {
            return None;
        }
    }
    let z_at = |t: f64| a.z + (b.z - a.z) * t;
    let (za, zb) = (z_at(t0), z_at(t1));
    (za.min(zb) < o.z1 && za.max(zb) > o.z0).then_some(t0)
}

fn inside_solid(parts: &[Obb], p: DVec3) -> bool {
    parts
        .iter()
        .any(|o| o.half.x >= 0.1 && o.half.y >= 0.1 && seg_hit(p, p + DVec3::Z * 1e-3, o).is_some())
}

pub(super) fn enclosure(coll: &CollisionWorld, p: DVec3) -> Option<f32> {
    let parts = coll.obstacles_near(&Obb::point(p, ENCL_REACH));
    if parts.is_empty() {
        return None;
    }
    if inside_solid(&parts, p) {
        return Some(0.0);
    }
    let nearest = |dir: DVec3, len: f64| -> Option<f64> {
        let end = p + dir * len;
        parts
            .iter()
            .filter_map(|o| seg_hit(p, end, o))
            .reduce(f64::min)
            .map(|t| t * len)
    };
    nearest(DVec3::Z, ENCL_UP)?;
    let mut walls = 0usize;
    let mut extent = 0.0f64;
    for k in 0..8 {
        let a = k as f64 * std::f64::consts::FRAC_PI_4;
        if let Some(d) = nearest(DVec3::new(a.cos(), a.sin(), 0.0), ENCL_REACH) {
            walls += 1;
            extent = extent.max(d);
        }
    }
    (walls >= ENCL_MIN_WALLS).then_some(extent as f32)
}

pub(super) fn sees(coll: &CollisionWorld, eye: DVec3, p: DVec3) -> bool {
    let d = eye - p;
    let len = d.length();
    if len < 1.0 {
        return true;
    }
    let q = p + d / len * 0.4;
    !coll.ray_blocked(eye, q, OCC_HALF, OCC_HEIGHT)
}

pub(super) fn blocked_by_meshes(
    coll: &CollisionWorld,
    seen: &CollisionWorld,
    eye: DVec3,
    p: DVec3,
) -> bool {
    let d = p - eye;
    let len = d.length();
    if !(3.0..=80.0).contains(&len) {
        return false;
    }
    let dir = d / len;
    let end = p - dir * 0.4;
    let steps = (len / 7.0).ceil() as usize;
    for k in 0..=steps {
        let q = eye + dir * (k as f64 * 7.0).min(len);
        let probe = Obb::point(q, 4.0);
        let near = seen.obstacles_near(&probe);
        let near2 = coll.obstacles_near(&probe);
        for o in near.into_iter().chain(near2) {
            let solid_wall = o.mass == 0.0
                && o.pole.is_none()
                && o.half.x.max(o.half.y) >= 0.1
                && o.z1 - o.z0 >= 0.8;
            if !solid_wall || seg_hit(eye, eye + DVec3::Z * 1e-3, &o).is_some() {
                continue;
            }
            if seg_hit(eye, end, &o).is_some() {
                return true;
            }
        }
    }
    false
}

pub(super) fn body_box(ty: &::simulation::VehicleType) -> Option<[f32; 6]> {
    ty.def.bounding_box.or_else(|| {
        ty.model_box().map(|(lo, hi)| {
            let (size, mid) = (hi - lo, (hi + lo) * 0.5);
            [size.x, size.y, size.z, mid.x, mid.y, mid.z]
        })
    })
}

#[derive(Clone, Copy)]
pub(super) struct Body {
    pub bbox: Option<[f32; 6]>,
    pub inv: Mat4,
    pub origin: DVec3,
}

pub(super) fn bodies(v: &VehicleInstance) -> Vec<Body> {
    let mut out = Vec::with_capacity(1 + v.trailers.len());
    let rot = v.body_rotation();
    out.push(Body {
        bbox: body_box(&v.ty),
        inv: rot.inverse(),
        origin: v.position,
    });
    for t in &v.trailers {
        let rot = t.body_rotation();
        out.push(Body {
            bbox: body_box(&t.ty),
            inv: rot.inverse(),
            origin: t.position,
        });
    }
    out
}

pub(super) fn body_hides(bodies: &[Body], camera: DVec3, c: DVec3) -> bool {
    for body in bodies {
        let Some(b) = body.bbox else { continue };
        let to_local = |w: DVec3| body.inv.transform_point3((w - body.origin).as_vec3());
        let h = Vec3::new(b[0], b[1], b[2]) * 0.5;
        let mid = Vec3::new(b[3], b[4], b[5]);
        let (e, p) = (to_local(camera) - mid, to_local(c) - mid);
        let inside = |q: Vec3, m: f32| q.abs().cmplt(h - Vec3::splat(m)).all();
        if inside(e, -0.2) {
            continue;
        }
        if inside(p, BODY_INNER) {
            return true;
        }
        let (lo, hi) = (-(h - Vec3::splat(BODY_SKIN)), h - Vec3::splat(BODY_SKIN));
        let d = p - e;
        let (mut t0, mut t1) = (0.0f32, 1.0f32);
        let mut hit = true;
        for k in 0..3 {
            if d[k].abs() < 1e-6 {
                if e[k] < lo[k] || e[k] > hi[k] {
                    hit = false;
                    break;
                }
                continue;
            }
            let (u0, u1) = ((lo[k] - e[k]) / d[k], (hi[k] - e[k]) / d[k]);
            t0 = t0.max(u0.min(u1));
            t1 = t1.min(u0.max(u1));
            if t0 > t1 {
                hit = false;
                break;
            }
        }
        if hit && t0 < 1.0 {
            return true;
        }
    }
    false
}

pub(super) fn cell3(p: DVec3, scale: f64) -> [i64; 3] {
    [
        (p.x * scale).round() as i64,
        (p.y * scale).round() as i64,
        (p.z * scale).round() as i64,
    ]
}
