use super::*;

pub(super) fn seg_hit(a: DVec3, b: DVec3, o: &::simulation::collision::Obb) -> Option<f64> {
    let [r, f] = o.axes();
    let local = |p: DVec3| {
        let rel = p.truncate() - o.center;
        glam::DVec2::new(rel.dot(r), rel.dot(f))
    };
    let (la, lb) = (local(a), local(b));
    let (mut t0, mut t1) = (0.0f64, 1.0f64);
    let dl = lb - la;
    for (p, dd, h) in [(la.x, dl.x, o.half.x), (la.y, dl.y, o.half.y)] {
        if dd.abs() < 1e-9 {
            if p.abs() > h {
                return None;
            }
        } else {
            let (u0, u1) = ((-h - p) / dd, (h - p) / dd);
            t0 = t0.max(u0.min(u1));
            t1 = t1.min(u0.max(u1));
            if t0 > t1 {
                return None;
            }
        }
    }
    let (za, zb) = (a.z + (b.z - a.z) * t0, a.z + (b.z - a.z) * t1);
    if za.min(zb) < o.z1 && za.max(zb) > o.z0 {
        Some(t0)
    } else {
        None
    }
}

/// `Some(extent)` when the point sits in a closed room (roof above and walls round it):
/// `extent` is how far the room reaches (m). `None` = outdoors.
pub(super) fn enclosure(coll: &::simulation::collision::CollisionWorld, p: DVec3) -> Option<f32> {
    let parts = coll.obstacles_near(&::simulation::collision::Obb::point(p, ENCL_REACH));
    if parts.is_empty() {
        return None;
    }
    // a point inside a solid part (a lamp sunk into a wall or a ceiling)
    if parts
        .iter()
        .any(|o| o.half.x >= 0.1 && o.half.y >= 0.1 && seg_hit(p, p + DVec3::Z * 1e-3, o).is_some())
    {
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

pub(super) const SHADOW_RANGE: f64 = 50.0;
pub(super) const SHADOW_REACH: f64 = 25.0;
pub(super) const SHADOW_MAX: usize = 32;
pub(super) const SHADOW_LIGHTS: usize = 32;
pub(super) const POINT_TRI_MAX: usize = 10;
pub(super) const POINT_TRI_MIN_AREA: f64 = 0.4;
pub(super) const SHADOW_SPOTS: usize = 8;
pub(super) const SPOT_SHADOW_RANGE: f64 = 60.0;
pub(super) const SPOT_REACH: f64 = 40.0;
pub(super) const SPOT_MAX: usize = 32;
pub(super) const SPOT_MIN_AREA: f64 = 0.01;
pub(super) const SPOT_MARGIN: f64 = 2.0;
pub(super) const GATHERS_PER_FRAME: usize = 2;
pub(super) const GATHER_BUDGET_S: f64 = 0.0015;

pub(super) type OccKey = (i64, i64, i64, u32, i32);

pub(super) struct OccCache {
    pub(super) generation: u64,
    pub(super) map: std::collections::HashMap<OccKey, Vec<::render::Occluder>>,
}

pub(super) static OCC_CACHE: std::sync::Mutex<Option<OccCache>> = std::sync::Mutex::new(None);

pub(super) fn gather_spot_occluders(
    seen: &::simulation::collision::CollisionWorld,
    pos: DVec3,
    dir: Vec3,
    radius: f32,
    cone_out: f32,
) -> Vec<::render::Occluder> {
    let range = (radius as f64).clamp(2.0, SPOT_REACH);
    let d = dir.as_dvec3().normalize_or_zero();
    let mid = pos + d * (range * 0.5);
    let probe = ::simulation::collision::Obb::point(mid, range * 0.5 + SPOT_MARGIN + 2.0);
    let half_angle = (cone_out as f64).clamp(-1.0, 1.0).acos();
    let mut tris: Vec<(f64, [DVec3; 3])> = seen
        .triangles_near(&probe, SPOT_MIN_AREA)
        .into_iter()
        .filter_map(|t| {
            let area = 0.5 * (t[1] - t[0]).cross(t[2] - t[0]).length();
            if area < SPOT_MIN_AREA {
                return None;
            }
            let c = (t[0] + t[1] + t[2]) / 3.0;
            let reach = t.iter().map(|v| (*v - c).length()).fold(0.0, f64::max);
            let rel = c - pos;
            let len = rel.length();
            if len - reach > range + SPOT_MARGIN {
                return None;
            }
            if len > reach + 1.0 {
                let angle = (rel.dot(d) / len).clamp(-1.0, 1.0).acos();
                if angle > half_angle + (reach / len).atan() + 0.35 {
                    return None;
                }
            }
            Some((area / (len * len + 1.0), t))
        })
        .collect();
    tris.sort_by(|a, b| b.0.total_cmp(&a.0));
    tris.truncate(SPOT_MAX);
    tris.into_iter()
        .map(|(_, t)| ::render::Occluder {
            center: glam::DVec2::ZERO,
            half: glam::Vec2::ZERO,
            z0: 0.0,
            z1: 0.0,
            heading: 0.0,
            tri: Some(t),
        })
        .collect()
}

pub(super) fn gather_occluders(
    coll: &::simulation::collision::CollisionWorld,
    seen: &::simulation::collision::CollisionWorld,
    pos: DVec3,
    radius: f32,
) -> Vec<::render::Occluder> {
    let reach = (radius as f64).clamp(2.0, SHADOW_REACH);
    let probe = ::simulation::collision::Obb::point(pos, reach);
    let mut all = seen.obstacles_near(&probe);
    all.extend(coll.obstacles_near(&probe));
    let mut parts: Vec<(f64, ::simulation::collision::Obb)> = all
        .into_iter()
        .filter(|o| {
            o.mass == 0.0
                && o.pole.is_none()
                && o.half.x.max(o.half.y) >= 0.05
                && o.z1 - o.z0 >= 0.3
                && seg_hit(pos, pos + DVec3::Z * 1e-3, o).is_none()
                && !(o.radius() < 1.0 && (o.center - pos.truncate()).length() < 0.6)
        })
        .map(|o| ((o.center - pos.truncate()).length() - o.radius(), o))
        .filter(|(d, _)| *d < reach)
        .collect();
    parts.sort_by(|a, b| a.0.total_cmp(&b.0));
    parts.truncate(SHADOW_MAX);
    let mut out: Vec<::render::Occluder> = parts
        .into_iter()
        .map(|(_, o)| ::render::Occluder {
            center: o.center,
            half: glam::Vec2::new(o.half.x as f32, o.half.y as f32),
            z0: o.z0,
            z1: o.z1,
            heading: o.heading,
            tri: None,
        })
        .collect();

    let mut tris: Vec<(f64, [DVec3; 3])> = seen
        .triangles_near(&probe, POINT_TRI_MIN_AREA)
        .into_iter()
        .filter_map(|t| {
            let area = 0.5 * (t[1] - t[0]).cross(t[2] - t[0]).length();
            if area < POINT_TRI_MIN_AREA {
                return None;
            }
            let c = (t[0] + t[1] + t[2]) / 3.0;
            let reach_t = t.iter().map(|v| (*v - c).length()).fold(0.0, f64::max);
            let len = (c - pos).length();

            if len - reach_t > reach + 1.0 || len < reach_t + 0.2 {
                return None;
            }
            Some((area / (len * len + 1.0), t))
        })
        .collect();
    tris.sort_by(|a, b| b.0.total_cmp(&a.0));
    tris.truncate(POINT_TRI_MAX);
    out.extend(tris.into_iter().map(|(_, t)| ::render::Occluder {
        center: glam::DVec2::ZERO,
        half: glam::Vec2::ZERO,
        z0: 0.0,
        z1: 0.0,
        heading: 0.0,
        tri: Some(t),
    }));
    out
}

pub(super) fn assign_occluders(
    coll: &::simulation::collision::CollisionWorld,
    seen: &::simulation::collision::CollisionWorld,
    generation: u64,
    scene: &mut Scene,
    camera_pos: DVec3,
    vehicles: &[&VehicleInstance],
) {
    scene.occluders.clear();
    let t_assign = std::time::Instant::now();
    let mut bodies: Vec<(::simulation::collision::Obb, ::render::Occluder)> = Vec::new();
    for v in vehicles
        .iter()
        .filter(|v| (v.position - camera_pos).length() < SHADOW_RANGE + 60.0)
    {
        let mut sections = vec![(body_box(&v.ty), v.body_rotation(), v.position)];
        for t in &v.trailers {
            sections.push((body_box(&t.ty), t.body_rotation(), t.position));
        }
        for (bb, xf, origin) in sections {
            let Some(bb) = bb else { continue };
            let f = xf.transform_vector3(Vec3::Y);
            let heading = (f.x as f64).atan2(f.y as f64);
            let o = ::simulation::collision::Obb::from_box(bb, origin, heading.to_degrees());
            bodies.push((
                o,
                ::render::Occluder {
                    center: o.center,
                    half: glam::Vec2::new(o.half.x as f32 - 0.1, o.half.y as f32 - 0.1),
                    z0: o.z0,
                    z1: o.z1,
                    heading: o.heading,
                    tri: None,
                },
            ));
        }
    }
    let mut guard = OCC_CACHE.lock().unwrap_or_else(|e| e.into_inner());
    let cache = guard.get_or_insert_with(|| OccCache {
        generation,
        map: Default::default(),
    });
    if cache.generation != generation || cache.map.len() > 4000 {
        cache.generation = generation;
        cache.map.clear();
    }
    let mut lights = std::mem::take(&mut scene.lights);
    let mut shadowed = 0usize;
    let mut shadowed_spots = 0usize;
    let mut gathers = 0usize;
    let mut prev_spot: Option<(DVec3, Vec3, bool)> = None;
    for l in lights.iter_mut() {
        l.occ_first = 0;
        l.occ_count = 0;
        let spill = l.shadow_first;
        let spot = !spill && l.direction.length_squared() > 0.5;
        if l.radius <= 0.0 || l.is_screen() {
            continue;
        }
        let mut capped = false;
        if spot {
            if (l.position - camera_pos).length() > SPOT_SHADOW_RANGE {
                continue;
            }
            match prev_spot {
                Some((p, d, c)) if p == l.position && d == l.direction => capped = c,
                _ => {
                    if shadowed_spots >= SHADOW_SPOTS {
                        capped = true;
                    } else {
                        shadowed_spots += 1;
                    }
                    prev_spot = Some((l.position, l.direction, capped));
                }
            }
        } else {
            if (l.position - camera_pos).length() > SHADOW_RANGE {
                continue;
            }
            if shadowed >= SHADOW_LIGHTS {
                capped = true;
            } else {
                shadowed += 1;
            }
        }
        // (a moving vehicle's window light would make a new key every frame at half a metre:
        // it takes 2 m cells and a reach that much longer)
        let (grid, extra) = if spill {
            (0.5, 2.0)
        } else if spot {
            (0.5, 0.0)
        } else {
            (2.0, 0.0)
        };
        let aimed = spot || spill;
        let dir_key = if aimed {
            ((l.direction.x.atan2(l.direction.y).to_degrees() / 10.0).round() as i32) * 64
                + (l.cone[1] * 100.0).round() as i32 * 4096
                + (l.direction.z.clamp(-1.0, 1.0) * 8.0).round() as i32
                + 1
        } else {
            0
        };
        let key = (
            (l.position.x * grid).round() as i64,
            (l.position.y * grid).round() as i64,
            (l.position.z * grid).round() as i64,
            l.radius.to_bits(),
            dir_key,
        );
        if !capped && !cache.map.contains_key(&key) {
            if gathers >= GATHERS_PER_FRAME || t_assign.elapsed().as_secs_f64() > GATHER_BUDGET_S {
                capped = true;
            } else {
                gathers += 1;
                let made = if aimed {
                    gather_spot_occluders(seen, l.position, l.direction, l.radius, l.cone[1])
                } else {
                    gather_occluders(coll, seen, l.position, l.radius + extra)
                };
                cache.map.insert(key, made);
            }
        }
        let first = scene.occluders.len() as u32;
        if !capped {
            if let Some(occ) = cache.map.get(&key) {
                scene.occluders.extend_from_slice(occ);
            }
        }
        for (o, oc) in &bodies {
            if spill {
                break;
            }
            let body_reach = if spot {
                l.radius.min(SPOT_REACH as f32)
            } else {
                l.radius.min(SHADOW_REACH as f32)
            };
            if (o.center - l.position.truncate()).length() < o.radius() + body_reach as f64 {
                let at = (l.position, l.position + DVec3::Z * 1e-3);
                let core = ::simulation::collision::Obb {
                    half: (o.half - glam::DVec2::splat(0.6)).max(glam::DVec2::splat(0.05)),
                    z0: o.z0 + 0.6,
                    z1: o.z1 - 0.2,
                    ..*o
                };
                if seg_hit(at.0, at.1, o).is_none() {
                    scene.occluders.push(*oc);
                } else if seg_hit(at.0, at.1, &core).is_none() {
                    // a lamp in the skin of the body (a head, tail or side light): the body
                    // neither shades nor holds it, but its light must not shine into the body
                    // itself: the part of the box on the inner side of the lamp stops it
                    let [r, f] = o.axes();
                    let rel = l.position.truncate() - o.center;
                    let (lr, lf) = (rel.dot(r), rel.dot(f));
                    let aim = glam::DVec2::new(l.direction.x as f64, l.direction.y as f64);
                    let (hx, hy) = (o.half.x - 0.1, o.half.y - 0.1);
                    let (dx, dy) = (hx - lr.abs(), hy - lf.abs());
                    let by_aim = spot && aim.dot(f).abs() > 0.3 && aim.dot(f).abs() >= aim.dot(r).abs();
                    let side = dx < 0.6 || dx <= dy;
                    let fore = dy < 0.6 || dy < dx || by_aim;
                    let fwd_face = if by_aim { aim.dot(f) > 0.0 } else { lf > 0.0 };
                    if side {
                        let (a, b) = if lr > 0.0 { (-hx, lr - 0.05) } else { (lr + 0.05, hx) };
                        if b - a > 0.1 {
                            let mut part = *oc;
                            part.center = o.center + r * ((a + b) * 0.5);
                            part.half.x = ((b - a) * 0.5) as f32;
                            scene.occluders.push(part);
                        }
                    }
                    if fore {
                        let (a, b) = if fwd_face { (-hy, lf - 0.05) } else { (lf + 0.05, hy) };
                        if b - a > 0.1 {
                            let mut part = *oc;
                            part.center = o.center + f * ((a + b) * 0.5);
                            part.half.y = ((b - a) * 0.5) as f32;
                            scene.occluders.push(part);
                        }
                    }
                } else if spill {
                    // a light inside the body lights only the inside (and a little through
                    // the windows): negative half width marks the box as a container
                    let mut hollow = *oc;
                    hollow.half.x = -(o.half.x as f32);
                    hollow.half.y = o.half.y as f32;
                    scene.occluders.push(hollow);
                }
            }
        }
        let n = scene.occluders.len() as u32 - first;
        if n > 0 {
            l.occ_first = first;
            l.occ_count = n;
        }
    }
    scene.lights = lights;
}

/// The box of a vehicle body `[width, length, height, cx, cy, cz]`: its `[boundingbox]`,
/// else the box of its model. Without one a vehicle had no body for light to be stopped by.
pub(super) fn body_box(ty: &::simulation::VehicleType) -> Option<[f32; 6]> {
    ty.def.bounding_box.or_else(|| {
        ty.model_box().map(|(lo, hi)| {
            let (size, mid) = (hi - lo, (hi + lo) * 0.5);
            [size.x, size.y, size.z, mid.x, mid.y, mid.z]
        })
    })
}

pub(super) const BODY_INNER: f32 = 0.4;
pub(super) const BODY_SKIN: f32 = 0.3;

/// A vehicle's bodies for `body_hides`: box, inverse of the body's turn, origin (made once
/// per vehicle and frame, not per corona).
pub(super) fn body_sections(v: &VehicleInstance) -> Vec<(Option<[f32; 6]>, glam::Mat4, DVec3)> {
    let mut sections = vec![(body_box(&v.ty), v.body_rotation().inverse(), v.position)];
    for t in &v.trailers {
        sections.push((body_box(&t.ty), t.body_rotation().inverse(), t.position));
    }
    sections
}

pub(super) fn body_hides(
    sections: &[(Option<[f32; 6]>, glam::Mat4, DVec3)],
    camera_pos: DVec3,
    c: DVec3,
) -> bool {
    for &(bb, inv, origin) in sections {
        let Some(b) = bb else { continue };
        let e = inv.transform_point3((camera_pos - origin).as_vec3());
        let p = inv.transform_point3((c - origin).as_vec3());
        let h = Vec3::new(b[0], b[1], b[2]) * 0.5;
        let mid = Vec3::new(b[3], b[4], b[5]);
        let (e, p) = (e - mid, p - mid);
        let inside = |q: Vec3, m: f32| q.abs().cmplt(h - Vec3::splat(m)).all();
        if inside(e, -0.2) {
            continue;
        }
        if inside(p, BODY_INNER) {
            return true;
        }
        let lo = -(h - Vec3::splat(BODY_SKIN));
        let hi = h - Vec3::splat(BODY_SKIN);
        let d = p - e;
        let mut t0 = 0.0f32;
        let mut t1 = 1.0f32;
        let mut hit = true;
        for k in 0..3 {
            if d[k].abs() < 1e-6 {
                if e[k] < lo[k] || e[k] > hi[k] {
                    hit = false;
                    break;
                }
            } else {
                let (u0, u1) = ((lo[k] - e[k]) / d[k], (hi[k] - e[k]) / d[k]);
                t0 = t0.max(u0.min(u1));
                t1 = t1.min(u0.max(u1));
                if t0 > t1 {
                    hit = false;
                    break;
                }
            }
        }
        if hit && t0 < 1.0 {
            return true;
        }
    }
    false
}

pub(super) fn blocked_by_meshes(
    coll: &::simulation::collision::CollisionWorld,
    seen: &::simulation::collision::CollisionWorld,
    eye: DVec3,
    p: DVec3,
) -> bool {
    let d = p - eye;
    let len = d.length();
    if len < 3.0 || len > 80.0 {
        return false;
    }
    let dir = d / len;
    let end = p - dir * 0.4;
    let steps = (len / 7.0).ceil() as usize;
    for k in 0..=steps {
        let q = eye + dir * (k as f64 * 7.0).min(len);
        let probe = ::simulation::collision::Obb::point(q, 4.0);
        let parts = seen.obstacles_near(&probe);
        let parts2 = coll.obstacles_near(&probe);
        for o in parts.into_iter().chain(parts2) {
            if o.mass != 0.0
                || o.pole.is_some()
                || o.half.x.max(o.half.y) < 0.1
                || o.z1 - o.z0 < 0.8
            {
                continue;
            }
            if seg_hit(eye, eye + DVec3::Z * 1e-3, &o).is_some() {
                continue;
            }
            if seg_hit(eye, end, &o).is_some() {
                return true;
            }
        }
    }
    false
}

pub(super) fn sees(coll: &::simulation::collision::CollisionWorld, eye: DVec3, p: DVec3) -> bool {
    let d = eye - p;
    let l = d.length();
    if l < 1.0 {
        return true;
    }
    let q = p + d / l * 0.4;
    !coll.ray_blocked(eye, q, OCC_HALF, OCC_HEIGHT)
}
