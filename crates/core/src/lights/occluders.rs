use super::frame::Frame;
use super::geom::{body_box, seg_hit};
use super::tuning::*;
use glam::{DVec2, DVec3, Vec2, Vec3};
use ::render::{Occluder, PointLight, Scene};
use ::simulation::collision::{CollisionWorld, Obb};
use ::simulation::VehicleInstance;
use std::collections::HashMap;

type Key = (i64, i64, i64, u32, i32);

#[derive(Default)]
pub(super) struct OccluderCache {
    generation: u64,
    map: HashMap<Key, Vec<Occluder>>,
}

#[derive(Clone, Copy, PartialEq)]
enum Kind {
    Spill,
    Spot,
    Point,
}

fn box_occluder(o: &Obb) -> Occluder {
    Occluder {
        center: o.center,
        half: Vec2::new(o.half.x as f32, o.half.y as f32),
        z0: o.z0,
        z1: o.z1,
        heading: o.heading,
        tri: None,
    }
}

fn tri_occluder(t: [DVec3; 3]) -> Occluder {
    Occluder {
        center: DVec2::ZERO,
        half: Vec2::ZERO,
        z0: 0.0,
        z1: 0.0,
        heading: 0.0,
        tri: Some(t),
    }
}

fn tri_area(t: &[DVec3; 3]) -> f64 {
    0.5 * (t[1] - t[0]).cross(t[2] - t[0]).length()
}

fn tri_sphere(t: &[DVec3; 3]) -> (DVec3, f64) {
    let c = (t[0] + t[1] + t[2]) / 3.0;
    (c, t.iter().map(|v| (*v - c).length()).fold(0.0, f64::max))
}

fn spot_casters(seen: &CollisionWorld, pos: DVec3, dir: Vec3, radius: f32, cone_out: f32) -> Vec<Occluder> {
    let range = (radius as f64).clamp(2.0, SPOT_REACH);
    let axis = dir.as_dvec3().normalize_or_zero();
    let probe = Obb::point(pos + axis * (range * 0.5), range * 0.5 + SPOT_MARGIN + 2.0);
    let half_angle = (cone_out as f64).clamp(-1.0, 1.0).acos();
    let mut tris: Vec<(f64, [DVec3; 3])> = seen
        .triangles_near(&probe)
        .into_iter()
        .filter_map(|t| {
            let area = tri_area(&t);
            if area < SPOT_MIN_AREA {
                return None;
            }
            let (c, reach) = tri_sphere(&t);
            let rel = c - pos;
            let len = rel.length();
            if len - reach > range + SPOT_MARGIN {
                return None;
            }
            if len > reach + 1.0 {
                let angle = (rel.dot(axis) / len).clamp(-1.0, 1.0).acos();
                if angle > half_angle + (reach / len).atan() + 0.35 {
                    return None;
                }
            }
            Some((area / (len * len + 1.0), t))
        })
        .collect();
    tris.sort_by(|a, b| b.0.total_cmp(&a.0));
    tris.truncate(SPOT_MAX);
    tris.into_iter().map(|(_, t)| tri_occluder(t)).collect()
}

fn point_casters(coll: &CollisionWorld, seen: &CollisionWorld, pos: DVec3, radius: f32) -> Vec<Occluder> {
    let reach = (radius as f64).clamp(2.0, SHADOW_REACH);
    let probe = Obb::point(pos, reach);
    let mut boxes = seen.obstacles_near(&probe);
    boxes.extend(coll.obstacles_near(&probe));
    let mut parts: Vec<(f64, Obb)> = boxes
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
    let mut out: Vec<Occluder> = parts.iter().map(|(_, o)| box_occluder(o)).collect();

    let mut tris: Vec<(f64, [DVec3; 3])> = seen
        .triangles_near(&probe)
        .into_iter()
        .filter_map(|t| {
            let area = tri_area(&t);
            if area < POINT_TRI_MIN_AREA {
                return None;
            }
            let (c, size) = tri_sphere(&t);
            let len = (c - pos).length();
            if len - size > reach + 1.0 || len < size + 0.2 {
                return None;
            }
            Some((area / (len * len + 1.0), t))
        })
        .collect();
    tris.sort_by(|a, b| b.0.total_cmp(&a.0));
    tris.truncate(POINT_TRI_MAX);
    out.extend(tris.into_iter().map(|(_, t)| tri_occluder(t)));
    out
}

fn vehicle_bodies(f: &Frame, vehicles: &[&VehicleInstance]) -> Vec<(Obb, Occluder)> {
    let mut out = Vec::new();
    for v in vehicles
        .iter()
        .filter(|v| f.dist(v.position) < SHADOW_RANGE + 60.0)
    {
        let mut sections = vec![(body_box(&v.ty), v.body_rotation(), v.position)];
        sections.extend(
            v.trailers
                .iter()
                .map(|t| (body_box(&t.ty), t.body_rotation(), t.position)),
        );
        for (bbox, rot, origin) in sections {
            let Some(bbox) = bbox else { continue };
            let fwd = rot.transform_vector3(Vec3::Y);
            let heading = (fwd.x as f64).atan2(fwd.y as f64);
            let o = Obb::from_box(bbox, origin, heading.to_degrees());
            let mut oc = box_occluder(&o);
            oc.half = Vec2::new(o.half.x as f32 - 0.1, o.half.y as f32 - 0.1);
            out.push((o, oc));
        }
    }
    out
}

fn body_casters(l: &PointLight, o: &Obb, oc: &Occluder, spot: bool, out: &mut Vec<Occluder>) {
    let at = (l.position, l.position + DVec3::Z * 1e-3);
    if seg_hit(at.0, at.1, o).is_none() {
        out.push(*oc);
        return;
    }
    let core = Obb {
        half: (o.half - DVec2::splat(0.6)).max(DVec2::splat(0.05)),
        z0: o.z0 + 0.6,
        z1: o.z1 - 0.2,
        ..*o
    };
    if seg_hit(at.0, at.1, &core).is_some() {
        return;
    }
    let [r, fw] = o.axes();
    let rel = l.position.truncate() - o.center;
    let (lr, lf) = (rel.dot(r), rel.dot(fw));
    let aim = DVec2::new(l.direction.x as f64, l.direction.y as f64);
    let (hx, hy) = (o.half.x - 0.1, o.half.y - 0.1);
    let (dx, dy) = (hx - lr.abs(), hy - lf.abs());
    let by_aim = spot && aim.dot(fw).abs() > 0.3 && aim.dot(fw).abs() >= aim.dot(r).abs();
    let on_side = dx < 0.6 || dx <= dy;
    let on_end = dy < 0.6 || dy < dx || by_aim;
    let front = if by_aim { aim.dot(fw) > 0.0 } else { lf > 0.0 };
    if on_side {
        let (a, b) = if lr > 0.0 { (-hx, lr - 0.05) } else { (lr + 0.05, hx) };
        if b - a > 0.1 {
            let mut part = *oc;
            part.center = o.center + r * ((a + b) * 0.5);
            part.half.x = ((b - a) * 0.5) as f32;
            out.push(part);
        }
    }
    if on_end {
        let (a, b) = if front { (-hy, lf - 0.05) } else { (lf + 0.05, hy) };
        if b - a > 0.1 {
            let mut part = *oc;
            part.center = o.center + fw * ((a + b) * 0.5);
            part.half.y = ((b - a) * 0.5) as f32;
            out.push(part);
        }
    }
}

fn key_of(l: &PointLight, kind: Kind) -> Key {
    let (grid, aimed) = match kind {
        Kind::Spill => (0.5, true),
        Kind::Spot => (0.5, true),
        Kind::Point => (2.0, false),
    };
    let aim_key = if aimed {
        ((l.direction.x.atan2(l.direction.y).to_degrees() / 10.0).round() as i32) * 64
            + (l.cone[1] * 100.0).round() as i32 * 4096
            + (l.direction.z.clamp(-1.0, 1.0) * 8.0).round() as i32
            + 1
    } else {
        0
    };
    (
        (l.position.x * grid).round() as i64,
        (l.position.y * grid).round() as i64,
        (l.position.z * grid).round() as i64,
        l.radius.to_bits(),
        aim_key,
    )
}

pub(super) fn assign(
    f: &Frame,
    cache: &mut OccluderCache,
    scene: &mut Scene,
    vehicles: &[&VehicleInstance],
) {
    scene.occluders.clear();
    if cache.generation != f.generation || cache.map.len() > OCC_CACHE_MAX {
        cache.generation = f.generation;
        cache.map.clear();
    }
    let bodies = vehicle_bodies(f, vehicles);
    let mut lights = std::mem::take(&mut scene.lights);
    let (mut points, mut spots, mut gathers) = (0usize, 0usize, 0usize);
    let mut prev_spot: Option<(DVec3, Vec3, bool)> = None;

    for l in lights.iter_mut() {
        l.occ_first = 0;
        l.occ_count = 0;
        if l.radius <= 0.0 || l.is_screen() {
            continue;
        }
        let kind = if l.shadow_first {
            Kind::Spill
        } else if l.direction.length_squared() > 0.5 {
            Kind::Spot
        } else {
            Kind::Point
        };
        let dist = f.dist(l.position);
        let mut capped = false;
        match kind {
            Kind::Spot => {
                if dist > SPOT_SHADOW_RANGE {
                    continue;
                }
                match prev_spot {
                    Some((p, d, c)) if p == l.position && d == l.direction => capped = c,
                    _ => {
                        capped = spots >= SHADOW_SPOTS;
                        spots += !capped as usize;
                        prev_spot = Some((l.position, l.direction, capped));
                    }
                }
            }
            Kind::Point | Kind::Spill => {
                if dist > SHADOW_RANGE {
                    continue;
                }
                capped = points >= SHADOW_LIGHTS;
                points += !capped as usize;
            }
        }

        let key = key_of(l, kind);
        if !capped && !cache.map.contains_key(&key) {
            if gathers >= GATHERS_PER_FRAME {
                capped = true;
            } else {
                gathers += 1;
                let made = match kind {
                    Kind::Point => point_casters(&f.coll, &f.seen, l.position, l.radius),
                    Kind::Spot | Kind::Spill => {
                        spot_casters(&f.seen, l.position, l.direction, l.radius, l.cone[1])
                    }
                };
                cache.map.insert(key, made);
            }
        }

        let first = scene.occluders.len() as u32;
        if !capped {
            if let Some(cached) = cache.map.get(&key) {
                scene.occluders.extend_from_slice(cached);
            }
        }
        if kind != Kind::Spill {
            let reach = match kind {
                Kind::Spot => l.radius.min(SPOT_REACH as f32),
                _ => l.radius.min(SHADOW_REACH as f32),
            } as f64;
            for (o, oc) in &bodies {
                if (o.center - l.position.truncate()).length() < o.radius() + reach {
                    body_casters(l, o, oc, kind == Kind::Spot, &mut scene.occluders);
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
