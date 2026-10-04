use crate::scene::{LightSwitch, World};
use glam::{DVec3, Vec3};
use omsi_render::{Corona, LightMode, Lighting, PointLight, Scene};

use omsi_sim::{Daylight, VehicleInstance};

const HEADLIGHT_INTENSITY: f32 = 22.0;
const VANILLA_HEADLIGHT_INTENSITY: f32 = 0.2;
const HIGH_BEAM_GAIN: f32 = 0.5;

#[derive(Clone, Copy)]
pub(crate) struct LightSettings {
    pub headlight: f32,
    pub vanilla: f32,
    pub low_beam_gain: f32,
    pub high_beam: f32,
    pub high_beam_range: f32,
    pub high_beam_spread: f32,
    pub force_high_beam: bool,
    pub weather_boost: f32,
    pub weather_night: f32,
    pub corona: f32,
}

impl LightSettings {
    pub(crate) const DEFAULT: Self = Self {
        headlight: HEADLIGHT_INTENSITY,
        vanilla: VANILLA_HEADLIGHT_INTENSITY,
        low_beam_gain: 3.5,
        high_beam: HIGH_BEAM_GAIN,
        high_beam_range: 1.0,
        high_beam_spread: 1.0,
        force_high_beam: false,
        weather_boost: 0.8,
        weather_night: 0.6,
        corona: 1.0,
    };
}

static SETTINGS: std::sync::Mutex<LightSettings> = std::sync::Mutex::new(LightSettings::DEFAULT);

pub(crate) fn settings() -> LightSettings {
    *SETTINGS.lock().unwrap_or_else(|e| e.into_inner())
}

pub(crate) fn set_settings(s: LightSettings) {
    *SETTINGS.lock().unwrap_or_else(|e| e.into_inner()) = s;
}

fn weather_darkness() -> f32 {
    let (vis, _) = cone_weather();
    (1.0 - vis / 3000.0).clamp(0.0, 1.0)
}

/// `[spotlight]` range is content-authored.  In particular, full beams commonly use a
/// substantially longer range than dipped beams, so it must not be capped to the latter.
fn headlight_radius(range: f32) -> f32 {
    range.max(6.0)
}

/// Keep the existing one-metre core for a typical 40 m dipped beam, while making a longer
/// content-authored full beam equally useful at the same fraction of its range.
fn headlight_core(range: f32) -> f32 {
    headlight_radius(range) / 30.0
}

fn ai_spotlight(lamps: &[[f32; 3]]) -> Option<[f32; 12]> {
    let nose = lamps.iter().map(|l| l[1]).reduce(f32::max)?;
    let front: Vec<&[f32; 3]> = lamps.iter().filter(|l| nose - l[1] < 0.4).collect();
    let n = front.len() as f32;
    let z = front.iter().map(|l| l[2]).sum::<f32>() / n;
    Some([0.0, nose, z, 0.0, 1.0, -0.05, 255.0, 245.0, 225.0, 40.0, 30.0, 70.0])
}

fn spot_face(lamp: Option<f32>, edge: Option<f32>, apex_y: f32, dir: f32) -> Option<f32> {
    let fwd = |y: f32| y * dir;
    let lamp = lamp.filter(|l| fwd(*l) > fwd(apex_y));
    let face = match (lamp, edge) {
        (Some(l), Some(e)) => fwd(l).min(fwd(e)),
        (Some(l), None) => fwd(l),
        (None, Some(e)) => fwd(e).min(fwd(apex_y) + 1.5),
        (None, None) => return None,
    };
    Some(face * dir).filter(|f| fwd(*f) > fwd(apex_y))
}

pub fn lighting_from(d: &Daylight, fog_range: f32) -> Lighting {
    let density = (2.3 / fog_range.max(50.0)).max(0.00005);
    Lighting {
        sun_dir: d.sun_dir,
        sun_intensity: 1.0,
        sun_color: d.sun_color,
        secondary: d.secondary,
        ambient: d.ambient,
        fog_color: d.sky,
        fog_density: density,
        sky_color: d.sky,
        night: d.night,
        night_maps: Some(if d.lamps_on || d.night >= 0.5 { 1.0 } else { 0.0 }),
        sun_azimuth: d.azimuth_rad,
        sky_weights: d.sky_weights,
        envir_tint: d.envir_tint,
        ..Default::default()
    }
}

pub fn apply_weather(
    l: &mut Lighting,
    cloud_density: f32,
    precip_kind: i32,
    precip: f32,
    snow: f32,
) {
    let o = cloud_density.clamp(0.0, 1.0);
    let overcast = (o - 0.45).max(0.0) / 0.55;
    let grey = |c: Vec3, k: f32| -> Vec3 {
        let lum = c.dot(Vec3::new(0.3, 0.59, 0.11));
        c.lerp(Vec3::splat(lum), k)
    };
    l.sun_intensity *= 1.0 - 0.85 * overcast;
    l.sun_color = grey(l.sun_color, 0.6 * o);
    let sky_lum = l.sky_color.dot(Vec3::new(0.3, 0.59, 0.11));
    let cloud_sky = Vec3::splat(sky_lum * 0.82)
        .lerp(Vec3::new(0.62, 0.65, 0.70) * sky_lum.max(0.25) * 1.3, 0.5);
    l.sky_color = l.sky_color.lerp(cloud_sky, overcast * 0.9);
    l.secondary = grey(l.secondary, o * 0.7) * (1.0 - 0.15 * overcast);
    l.ambient = grey(l.ambient, o * 0.7) * (1.0 + 0.25 * overcast);
    l.fog_color = l.fog_color.lerp(l.sky_color, overcast);
    let rain = if precip_kind != 0 {
        precip.clamp(0.0, 1.0)
    } else {
        0.0
    };
    l.sun_intensity *= 1.0 - 0.75 * rain;
    l.ambient *= 1.0 - 0.28 * rain;
    l.secondary *= 1.0 - 0.32 * rain;
    l.sky_color *= 1.0 - 0.22 * rain;
    l.fog_color *= 1.0 - 0.15 * rain;
    if rain > 0.0 {
        l.fog_density = l.fog_density.max(2.3 / (5000.0 - 3500.0 * rain));
    }
    l.overcast = overcast;
    l.rain = rain;
    let gloom = (overcast * 0.5 + rain * 0.5).clamp(0.0, 1.0);
    l.night = l.night.max(0.45 * gloom);
    if l.night >= 0.5 {
        l.night_maps = Some(1.0);
    }
    if snow > 0.0 {
        l.ambient *= 1.0 + 0.35 * snow;
        l.secondary *= 1.0 + 0.2 * snow;
        let day = 1.0 - 0.93 * l.night.clamp(0.0, 1.0);
        l.fog_color = l.fog_color.lerp(Vec3::new(0.86, 0.88, 0.92) * day, 0.4 * snow);
    }
    l.snow = snow;
}

pub fn vehicle_lights(
    v: &VehicleInstance,
    coronas: &mut Vec<Corona>,
    lights: &mut Vec<PointLight>,
    night: f32,
    spill: bool,
) {
    let ty = &v.ty;
    let value_of = |name: &str| -> f32 {
        let t = name.trim();
        if let Ok(x) = t.parse::<f32>() {
            return x;
        }
        v.var(t).unwrap_or(0.0)
    };
    let mesh_xf = |def_index: usize| -> glam::Mat4 {
        match ty.meshes.iter().position(|m| m.def_index == def_index) {
            Some(i) => v.mesh_local_transform(i),
            None => v.body_rotation(),
        }
    };
    coronas.extend(crate::scene::model_lights_faded(&ty.model, &mesh_xf, v.position, &value_of, &v.light_fade));
    for t in &v.trailers {
        let part_mesh_xf = |def_index: usize| -> glam::Mat4 {
            match t.ty.meshes.iter().position(|m| m.def_index == def_index) {
                Some(i) => t.mesh_local_transform(i),
                None => t.body_rotation(),
            }
        };
        coronas.extend(crate::scene::model_lights_faded(
            &t.ty.model,
            &part_mesh_xf,
            t.position,
            &value_of,
            &t.light_fade,
        ));
    }
    let body = v.body_rotation();
    let forced = omsi_cfg::env::var("OMSI_SPOT_SELECT").ok().and_then(|s| s.trim().parse::<f32>().ok());
    let ai_on = v.ai_lights;
    let cfg = settings();
    let bad = weather_darkness();
    let night = night.max(bad * cfg.weather_night);
    let selected = forced.or_else(|| v.var("Spot_Select")).filter(|s| *s >= 0.0 || !ai_on);
    if let Some(sel) = selected.or(ai_on.then_some(0.0)) {
        if sel >= 0.0 {
            let lamps: Vec<[f32; 3]> = ty
                .model
                .meshes
                .iter()
                .flat_map(|m| m.light_enh.iter().map(|l| l.pos).chain(m.light_enh_2.iter().map(|l| l.pos)))
                .collect();
            let spot = ty
                .model
                .spotlights
                .get(sel as usize)
                .or_else(|| ai_on.then(|| ty.model.spotlights.first()).flatten())
                .map(|sp| sp.values)
                .or_else(|| ai_on.then(|| ai_spotlight(&lamps)).flatten());
            if let Some(vals) = spot {
                let d = body
                    .transform_vector3(Vec3::new(vals[3], vals[4], vals[5]))
                    .normalize_or_zero();
                let color = [vals[6] / 255.0, vals[7] / 255.0, vals[8] / 255.0];
                let mut apex = Vec3::new(vals[0], vals[1], vals[2]);
                let dl = Vec3::new(vals[3], vals[4], vals[5]).normalize_or_zero();
                let nose = lamps.iter().map(|l| l[1]).reduce(f32::max);
                let tail = lamps.iter().map(|l| l[1]).reduce(f32::min);
                let bb = ty.def.bounding_box.map(|bb| (bb[4] + bb[1] * 0.5, bb[4] - bb[1] * 0.5));
                let dir = if dl.y > 0.3 { 1.0 } else if dl.y < -0.3 { -1.0 } else { 0.0 };
                if dir != 0.0 {
                    let (lamp, edge) = if dir > 0.0 {
                        (nose, bb.map(|b| b.0))
                    } else {
                        (tail, bb.map(|b| b.1))
                    };
                    if let Some(face) = spot_face(lamp, edge, apex.y, dir) {
                        apex.y = face + dir * 0.05;
                    }
                }
                let half_width = ty.def.bounding_box.map_or(1.25, |bb| (bb[0] * 0.5).min(1.25));
                let on_face: Vec<f32> = lamps
                    .iter()
                    .filter(|l| (dl.y > 0.3 || dl.y < -0.3) && (l[1] - apex.y).abs() < 0.35)
                    .map(|l| (l[0] - apex.x).abs())
                    .collect();
                let spread = (on_face.iter().sum::<f32>() / on_face.len().max(1) as f32).min(half_width);
                let right = body.transform_vector3(Vec3::X).normalize_or_zero();
                let apex = body.transform_point3(apex);
                let (inner, outer) = (
                    vals.get(10).copied().unwrap_or(30.0),
                    vals.get(11).copied().unwrap_or(70.0),
                );
                let half = |deg: f32| (deg.clamp(1.0, 179.0) * 0.5).to_radians().cos();
                let cone = [half(inner.min(outer)), half(outer)];
                let sides: &[f32] = if spread > 0.1 { &[-1.0, 1.0] } else { &[0.0] };
                for side in sides {
                    let at = v.position + (apex + right * spread * side).as_dvec3();
                    let high_beam = cfg.force_high_beam || v.var("lights_fern").is_some_and(|x| x > 0.5);
                    let radius = headlight_radius(vals[9]) * if high_beam { cfg.high_beam_range } else { 1.0 };
                    let cone = if high_beam {
                        let k = cfg.high_beam_spread.max(0.1);
                        [1.0 - (1.0 - cone[0]) * k, 1.0 - (1.0 - cone[1]) * k]
                    } else {
                        cone
                    };
                    let lamp = PointLight {
                        position: at,
                        radius,
                        color,
                        direction: d,
                        cone,
                        ..Default::default()
                    };
                    lights.push(PointLight {
                        intensity: cfg.vanilla / sides.len() as f32 * (0.3 + 0.7 * night),
                        mode: LightMode::Vanilla,
                        ..lamp
                    });
                    lights.push(PointLight {
                        intensity: cfg.headlight / sides.len() as f32
                            * (1.0 + bad * cfg.weather_boost)
                            * if high_beam { cfg.high_beam } else { 1.0 },
                        core: headlight_core(radius),
                        beam: if high_beam { -1.0 } else { cfg.low_beam_gain },
                        mode: LightMode::Enhanced,
                        ..lamp
                    });
                }
            }
        }
    }
    if spill && night > 0.05 {
        let mut sections: Vec<(&omsi_model::Model, Option<[f32; 6]>, glam::Mat4, DVec3)> =
            vec![(&ty.model, body_box(ty), body, v.position)];
        for t in &v.trailers {
            sections.push((&t.ty.model, body_box(&t.ty), t.body_rotation(), t.position));
        }
        let tilt = INTERIOR_SPILL_TILT.to_radians();
        let cone = [INTERIOR_SPILL_INNER.to_radians().cos(), INTERIOR_SPILL_OUTER.to_radians().cos()];
        for (model, bb, xf, origin) in sections {
            let mut count = 0usize;
            let mut sum = Vec3::ZERO;
            let mut color = Vec3::ZERO;
            for il in &model.interior_lights {
                if value_of(&il.variable) >= 0.5 {
                    count += 1;
                    sum += Vec3::from(il.pos);
                    color += Vec3::from(il.color);
                }
            }
            if count == 0 {
                continue;
            }
            let c = sum / count as f32;
            let c = Vec3::new(c.x, c.y, c.z.min(INTERIOR_SPILL_HEIGHT));
            let color = color / count as f32 / 255.0;
            let color = (color * (1.0 - INTERIOR_SPILL_WHITE) + Vec3::splat(color.max_element()) * INTERIOR_SPILL_WHITE).to_array();
            let (half_w, half_l, cx, cy) = match bb {
                Some(b) => (b[0] * 0.5 + INTERIOR_SPILL_OUTSET, b[1] * 0.5 + INTERIOR_SPILL_OUTSET, b[3], b[4]),
                None => (1.25 - INTERIOR_SPILL_INSET, 4.0, 0.0, c.y),
            };
            let strength = (count.min(INTERIOR_SPILL_MAX) as f32 / INTERIOR_SPILL_MAX as f32).max(0.25) * night.clamp(0.0, 1.0);
            let mut faces = vec![
                (Vec3::new(c.x, cy + half_l, c.z), Vec3::Y, INTERIOR_SPILL_END),
                (Vec3::new(c.x, cy - half_l, c.z), -Vec3::Y, INTERIOR_SPILL_END),
            ];
            for k in 0..INTERIOR_SPILL_ALONG {
                let y = cy + half_l * 0.75 * (2.0 * (k as f32 + 0.5) / INTERIOR_SPILL_ALONG as f32 - 1.0);
                faces.push((Vec3::new(cx + half_w, y, c.z), Vec3::X, INTERIOR_SPILL_SIDE));
                faces.push((Vec3::new(cx - half_w, y, c.z), -Vec3::X, INTERIOR_SPILL_SIDE));
            }
            for (at, out, gain) in faces {
                let dir = (out * tilt.cos() - Vec3::Z * tilt.sin()).normalize();
                lights.push(PointLight {
                    position: origin + xf.transform_point3(at).as_dvec3(),
                    radius: INTERIOR_SPILL_RADIUS,
                    color,
                    intensity: gain * strength,
                    direction: xf.transform_vector3(dir).normalize_or_zero(),
                    cone,
                    core: INTERIOR_SPILL_CORE,
                    mode: LightMode::Enhanced,
                    ..Default::default()
                });
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::{headlight_core, headlight_radius};

    #[test]
    fn full_beam_range_is_not_capped_to_dipped_beam_distance() {
        // Studio Polygon Renown's third `[spotlight]` (full beam) has a 125 m range.
        assert_eq!(headlight_radius(125.0), 125.0);
        assert!((headlight_core(40.0) - 40.0 / 30.0).abs() < 1e-5);
        assert!((headlight_core(125.0) - 125.0 / 30.0).abs() < 1e-5);
    }
}

// (the spill lights sit just outside the body's box: inside it they counted as "in the skin" and the body neither held nor shaded their light, so it went through the bodywork)
// (a vehicle farther than this from the camera gets no window light: up to ten lights with
// occluders each, for a glow a few pixels wide - the cost on a weak graphics card)
const SPILL_RANGE: f64 = 30.0;
const SPILL_VEHICLES: usize = 3;
const INTERIOR_SPILL_OUTSET: f32 = 0.25;
const INTERIOR_SPILL_SIDE: f32 = 0.45;
const INTERIOR_SPILL_END: f32 = 0.3;
const INTERIOR_SPILL_ALONG: usize = 2;
const INTERIOR_SPILL_RADIUS: f32 = 9.0;
const INTERIOR_SPILL_CORE: f32 = 1.2;
const INTERIOR_SPILL_HEIGHT: f32 = 1.8;
const INTERIOR_SPILL_INSET: f32 = 0.5;
const INTERIOR_SPILL_WHITE: f32 = 0.6;
const INTERIOR_SPILL_MAX: usize = 8;
const INTERIOR_SPILL_TILT: f32 = 40.0;
const INTERIOR_SPILL_INNER: f32 = 30.0;
const INTERIOR_SPILL_OUTER: f32 = 80.0;

const MAP_LIGHT_RANGE: f64 = 300.0;
const CORONA_RANGE: f64 = 1500.0;
const NEAR_MARGIN: f64 = 100.0;
const OCC_HALF: f64 = 1.0;
const OCC_HEIGHT: f64 = 2.5;
const OCC_LIGHT_RANGE: f64 = 150.0;
const OCC_CORONA_RANGE: f64 = 300.0;
const OCC_RECHECK: f64 = 3.0;
const VIS_PER_FRAME: usize = 48;
const LAMP_RAYS_PER_FRAME: usize = 32;

const ENCL_REACH: f64 = 12.0;
const ENCL_UP: f64 = 10.0;
const ENCL_MIN_WALLS: usize = 4;
const ENCL_MIN_RADIUS: f32 = 3.0;

fn seg_hit(a: DVec3, b: DVec3, o: &omsi_sim::collision::Obb) -> Option<f64> {
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
fn enclosure(coll: &omsi_sim::collision::CollisionWorld, p: DVec3) -> Option<f32> {
    let parts = coll.obstacles_near(&omsi_sim::collision::Obb::point(p, ENCL_REACH));
    if parts.is_empty() {
        return None;
    }
    // a point inside a solid part (a lamp sunk into a wall or a ceiling)
    if parts.iter().any(|o| {
        o.half.x >= 0.1 && o.half.y >= 0.1 && seg_hit(p, p + DVec3::Z * 1e-3, o).is_some()
    }) {
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

const SHADOW_RANGE: f64 = 50.0;
const SHADOW_REACH: f64 = 25.0;
const SHADOW_MAX: usize = 32;
const SHADOW_LIGHTS: usize = 16;
const SHADOW_SPOTS: usize = 8;
const SPOT_SHADOW_RANGE: f64 = 60.0;
const SPOT_REACH: f64 = 40.0;
const SPOT_MAX: usize = 32;
const SPOT_MIN_AREA: f64 = 0.01;
const SPOT_MARGIN: f64 = 2.0;
const GATHERS_PER_FRAME: usize = 6;

type OccKey = (i64, i64, i64, u32, i32);

struct OccCache {
    generation: u64,
    map: std::collections::HashMap<OccKey, Vec<omsi_render::Occluder>>,
}

static OCC_CACHE: std::sync::Mutex<Option<OccCache>> = std::sync::Mutex::new(None);

fn gather_spot_occluders(
    seen: &omsi_sim::collision::CollisionWorld,
    pos: DVec3,
    dir: Vec3,
    radius: f32,
    cone_out: f32,
) -> Vec<omsi_render::Occluder> {
    let range = (radius as f64).clamp(2.0, SPOT_REACH);
    let d = dir.as_dvec3().normalize_or_zero();
    let mid = pos + d * (range * 0.5);
    let probe = omsi_sim::collision::Obb::point(mid, range * 0.5 + SPOT_MARGIN + 2.0);
    let half_angle = (cone_out as f64).clamp(-1.0, 1.0).acos();
    let mut tris: Vec<(f64, [DVec3; 3])> = seen
        .triangles_near(&probe)
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
        .map(|(_, t)| omsi_render::Occluder {
            center: glam::DVec2::ZERO,
            half: glam::Vec2::ZERO,
            z0: 0.0,
            z1: 0.0,
            heading: 0.0,
            tri: Some(t),
        })
        .collect()
}

fn gather_occluders(
    coll: &omsi_sim::collision::CollisionWorld,
    seen: &omsi_sim::collision::CollisionWorld,
    pos: DVec3,
    radius: f32,
) -> Vec<omsi_render::Occluder> {
    let reach = (radius as f64).clamp(2.0, SHADOW_REACH);
    let probe = omsi_sim::collision::Obb::point(pos, reach);
    let mut all = seen.obstacles_near(&probe);
    all.extend(coll.obstacles_near(&probe));
    let mut parts: Vec<(f64, omsi_sim::collision::Obb)> = all
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
    parts
        .into_iter()
        .map(|(_, o)| omsi_render::Occluder {
            center: o.center,
            half: glam::Vec2::new(o.half.x as f32, o.half.y as f32),
            z0: o.z0,
            z1: o.z1,
            heading: o.heading,
            tri: None,
        })
        .collect()
}

fn assign_occluders(
    coll: &omsi_sim::collision::CollisionWorld,
    seen: &omsi_sim::collision::CollisionWorld,
    generation: u64,
    scene: &mut Scene,
    camera_pos: DVec3,
    vehicles: &[&VehicleInstance],
) {
    scene.occluders.clear();
    let mut bodies: Vec<(omsi_sim::collision::Obb, omsi_render::Occluder)> = Vec::new();
    for v in vehicles.iter().filter(|v| (v.position - camera_pos).length() < SHADOW_RANGE + 60.0) {
        let mut sections = vec![(body_box(&v.ty), v.body_rotation(), v.position)];
        for t in &v.trailers {
            sections.push((body_box(&t.ty), t.body_rotation(), t.position));
        }
        for (bb, xf, origin) in sections {
            let Some(bb) = bb else { continue };
            let f = xf.transform_vector3(Vec3::Y);
            let heading = (f.x as f64).atan2(f.y as f64);
            let o = omsi_sim::collision::Obb::from_box(bb, origin, heading.to_degrees());
            bodies.push((
                o,
                omsi_render::Occluder {
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
    let cache = guard.get_or_insert_with(|| OccCache { generation, map: Default::default() });
    if cache.generation != generation || cache.map.len() > 4000 {
        cache.generation = generation;
        cache.map.clear();
    }
    let mut lights = std::mem::take(&mut scene.lights);
    let mut shadowed = 0usize;
    let mut shadowed_spots = 0usize;
    let mut gathers = 0usize;
    for l in lights.iter_mut() {
        l.occ_first = 0;
        l.occ_count = 0;
        let spill = l.radius == INTERIOR_SPILL_RADIUS;
        let spot = !spill && l.direction.length_squared() > 0.5;
        if l.radius <= 0.0 {
            continue;
        }
        if spot {
            if shadowed_spots >= SHADOW_SPOTS || (l.position - camera_pos).length() > SPOT_SHADOW_RANGE {
                continue;
            }
            shadowed_spots += 1;
        } else {
            if shadowed >= SHADOW_LIGHTS || (l.position - camera_pos).length() > SHADOW_RANGE {
                continue;
            }
            shadowed += 1;
        }
        // (a moving vehicle's window light would make a new key every frame at half a metre:
        // it takes 2 m cells and a reach that much longer)
        let (grid, extra) = if spill { (0.5, 2.0) } else if spot { (1.0, 0.0) } else { (2.0, 0.0) };
        let dir_key = if spot {
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
        if !cache.map.contains_key(&key) {
            if gathers >= GATHERS_PER_FRAME {
                continue;
            }
            gathers += 1;
            let made = if spill {
                Vec::new()
            } else if spot {
                gather_spot_occluders(seen, l.position, l.direction, l.radius, l.cone[1])
            } else {
                gather_occluders(coll, seen, l.position, l.radius + extra)
            };
            cache.map.insert(key, made);
        }
        let occ = &cache.map[&key];
        let first = scene.occluders.len() as u32;
        scene.occluders.extend_from_slice(occ);
        for (o, oc) in &bodies {
            let body_reach = if spot { l.radius.min(SPOT_REACH as f32) } else { l.radius.min(SHADOW_REACH as f32) };
            if (o.center - l.position.truncate()).length() < o.radius() + body_reach as f64 {
                let at = (l.position, l.position + DVec3::Z * 1e-3);
                let core = omsi_sim::collision::Obb {
                    half: (o.half - glam::DVec2::splat(0.6)).max(glam::DVec2::splat(0.05)),
                    z0: o.z0 + 0.6,
                    z1: o.z1 - 0.2,
                    ..*o
                };
                if seg_hit(at.0, at.1, o).is_none() {
                    scene.occluders.push(*oc);
                } else if seg_hit(at.0, at.1, &core).is_none() {
                    // a lamp in the skin of the body (a head or tail light): the body
                    // neither shades nor holds it
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
fn body_box(ty: &omsi_sim::VehicleType) -> Option<[f32; 6]> {
    ty.def.bounding_box.or_else(|| {
        ty.model_box().map(|(lo, hi)| {
            let (size, mid) = (hi - lo, (hi + lo) * 0.5);
            [size.x, size.y, size.z, mid.x, mid.y, mid.z]
        })
    })
}

const BODY_INNER: f32 = 0.25;
const BODY_SKIN: f32 = 0.15;

/// A vehicle's bodies for `body_hides`: box, inverse of the body's turn, origin (made once
/// per vehicle and frame, not per corona).
fn body_sections(v: &VehicleInstance) -> Vec<(Option<[f32; 6]>, glam::Mat4, DVec3)> {
    let mut sections = vec![(body_box(&v.ty), v.body_rotation().inverse(), v.position)];
    for t in &v.trailers {
        sections.push((body_box(&t.ty), t.body_rotation().inverse(), t.position));
    }
    sections
}

fn body_hides(sections: &[(Option<[f32; 6]>, glam::Mat4, DVec3)], camera_pos: DVec3, c: DVec3) -> bool {
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

fn blocked_by_meshes(
    coll: &omsi_sim::collision::CollisionWorld,
    seen: &omsi_sim::collision::CollisionWorld,
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
        let probe = omsi_sim::collision::Obb::point(q, 4.0);
        let mut parts = seen.obstacles_near(&probe);
        parts.extend(coll.obstacles_near(&probe));
        for o in parts {
            if o.mass != 0.0 || o.pole.is_some() || o.half.x.max(o.half.y) < 0.1 || o.z1 - o.z0 < 0.8 {
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

fn sees(coll: &omsi_sim::collision::CollisionWorld, eye: DVec3, p: DVec3) -> bool {
    let d = eye - p;
    let l = d.length();
    if l < 1.0 {
        return true;
    }
    let q = p + d / l * 0.4;
    !coll.ray_blocked(eye, q, OCC_HALF, OCC_HEIGHT)
}

#[derive(Default)]
struct NearLights {
    world: usize,
    generation: u64,
    lamps_on: bool,
    centre: DVec3,
    counts: (usize, usize),
    lights: Vec<PointLight>,
    coronas: Vec<Corona>,
    build: Option<NearBuild>,
    vis_centre: DVec3,
    vis_eye: DVec3,
    vis_cursor: usize,
    vis_valid: bool,
    light_vis: Vec<bool>,
    corona_vis: Vec<bool>,
}

/// A rebuild of the near lists in progress: the enclosure test of every light costs 300 ms
/// at once, so it goes on for a few milliseconds a frame while the old lists serve.
struct NearBuild {
    world: usize,
    generation: u64,
    lamps_on: bool,
    centre: DVec3,
    counts: (usize, usize),
    src_lights: Vec<PointLight>,
    src_coronas: Vec<Corona>,
    li: usize,
    ci: usize,
    lights: Vec<PointLight>,
    coronas: Vec<Corona>,
}

static NEAR_LIGHTS: std::sync::Mutex<Option<NearLights>> = std::sync::Mutex::new(None);

type LampVis = (Option<DVec3>, std::collections::HashMap<[i64; 3], (bool, u32)>, u32);
static LAMP_VIS: std::sync::LazyLock<std::sync::Mutex<LampVis>> =
    std::sync::LazyLock::new(|| std::sync::Mutex::new((None, Default::default(), 0)));

pub fn collect(
    world: &World,
    scene: &mut Scene,
    daylight: &Daylight,
    camera_pos: DVec3,
    vehicles: &[&VehicleInstance],
) {
    let t_start = std::time::Instant::now();
    scene.lights.clear();
    scene.coronas.clear();
    scene.smoke.clear();
    // nothing is lit, glowing or smoking beyond what can be seen (fog included)
    let visible_range = visible_range();
    omsi_sim::particles::set_eye(camera_pos);
    let night = daylight.night;
    let coll = world.collision.lock().clone();
    {
        let mut guard = NEAR_LIGHTS.lock().unwrap_or_else(|e| e.into_inner());
        let near = guard.get_or_insert_with(NearLights::default);
        let static_lights = world.static_lights.lock();
        let static_coronas = world.static_coronas.lock();
        let world_id = world as *const World as usize;
        let generation = world
            .tiles_generation
            .load(std::sync::atomic::Ordering::Relaxed);
        let stale = near.world != world_id
            || near.generation != generation
            || near.lamps_on != daylight.lamps_on
            || near.counts != (static_lights.len(), static_coronas.len())
            || (near.centre - camera_pos).length() > NEAR_MARGIN;
        let restart = near.build.as_ref().is_some_and(|b| {
            b.world != world_id || b.generation != generation || b.lamps_on != daylight.lamps_on
        });
        if restart {
            near.build = None;
        }
        if stale && near.build.is_none() {
            let src_lights: Vec<PointLight> = if daylight.lamps_on {
                static_lights
                    .iter()
                    .filter(|l| (l.position - camera_pos).length() < MAP_LIGHT_RANGE + NEAR_MARGIN)
                    .copied()
                    .collect()
            } else {
                Vec::new()
            };
            let src_coronas: Vec<Corona> = static_coronas
                .iter()
                .filter_map(|c| {
                    let on = match &c.switch {
                        LightSwitch::Constant(x) => *x,
                        LightSwitch::Night => daylight.lamps_on as i32 as f32,
                        LightSwitch::Variable(_) => daylight.lamps_on as i32 as f32,
                    };
                    if on <= 0.0 || (c.corona.position - camera_pos).length() > CORONA_RANGE + NEAR_MARGIN {
                        return None;
                    }
                    let mut corona = c.corona;
                    corona.brightness *= on.min(1.0);
                    Some(corona)
                })
                .collect();
            near.build = Some(NearBuild {
                world: world_id,
                generation,
                lamps_on: daylight.lamps_on,
                centre: camera_pos,
                counts: (static_lights.len(), static_coronas.len()),
                src_lights,
                src_coronas,
                li: 0,
                ci: 0,
                lights: Vec::new(),
                coronas: Vec::new(),
            });
        }
        if let Some(mut b) = near.build.take() {
            let t_build = std::time::Instant::now();
            while b.li < b.src_lights.len() && t_build.elapsed().as_micros() < 4000 {
                let mut l = b.src_lights[b.li];
                b.li += 1;
                if let Some(ext) = enclosure(&coll, l.position) {
                    let r = (ext + 1.0).max(ENCL_MIN_RADIUS);
                    l.radius = l.radius.min(r);
                    if l.core > l.radius {
                        l.core = l.radius;
                    }
                }
                b.lights.push(l);
            }
            while b.li >= b.src_lights.len() && b.ci < b.src_coronas.len() && t_build.elapsed().as_micros() < 4000 {
                let c = b.src_coronas[b.ci];
                b.ci += 1;
                if enclosure(&coll, c.position).is_none() {
                    b.coronas.push(c);
                }
            }
            if b.li >= b.src_lights.len() && b.ci >= b.src_coronas.len() {
                *near = NearLights {
                    world: b.world,
                    generation: b.generation,
                    lamps_on: b.lamps_on,
                    centre: b.centre,
                    counts: b.counts,
                    lights: b.lights,
                    coronas: b.coronas,
                    ..Default::default()
                };
            } else {
                near.build = Some(b);
            }
        }
        // (the ray tests are spread over frames: all at once they cost 250-700 ms)
        near.light_vis.resize(near.lights.len(), true);
        near.corona_vis.resize(near.coronas.len(), true);
        let total = near.lights.len() + near.coronas.len();
        if !near.vis_valid || (near.vis_cursor >= total && (near.vis_centre - camera_pos).length() > OCC_RECHECK) {
            near.vis_eye = camera_pos;
            near.vis_centre = camera_pos;
            near.vis_cursor = 0;
            near.vis_valid = true;
        }
        let mut budget = VIS_PER_FRAME;
        let eye = near.vis_eye;
        while near.vis_cursor < total && budget > 0 {
            let i = near.vis_cursor;
            let nl = near.lights.len();
            if i < nl {
                let p = near.lights[i].position;
                near.light_vis[i] = (p - eye).length() > OCC_LIGHT_RANGE || sees(&coll, eye, p);
            } else {
                let p = near.coronas[i - nl].position;
                near.corona_vis[i - nl] = (p - eye).length() > OCC_CORONA_RANGE || sees(&coll, eye, p);
            }
            near.vis_cursor += 1;
            budget -= 1;
        }
        scene.lights.extend(
            near.lights
                .iter()
                .zip(&near.light_vis)
                .filter(|(l, vis)| **vis && (l.position - camera_pos).length() < MAP_LIGHT_RANGE.min(visible_range))
                .map(|(l, _)| *l),
        );
        scene.coronas.extend(
            near.coronas
                .iter()
                .zip(&near.corona_vis)
                .filter(|(c, vis)| **vis && (c.position - camera_pos).length() <= visible_range)
                .map(|(c, _)| *c),
        );
    }
    let t_lamps = std::time::Instant::now();
    // (whether a street lamp is seen is asked again only after the camera has moved a few
    // metres, as for the map's own lights: the ray went through the collision world for
    // every lamp in range every frame)
    let mut lamp_vis = LAMP_VIS.lock().unwrap_or_else(|e| e.into_inner());
    if lamp_vis.0.is_none() || lamp_vis.0.map(|c| (c - camera_pos).length() > OCC_RECHECK).unwrap_or(true) {
        lamp_vis.0 = Some(camera_pos);
        lamp_vis.2 = lamp_vis.2.wrapping_add(1);
        if lamp_vis.1.len() > 20000 {
            lamp_vis.1.clear();
        }
    }
    let epoch = lamp_vis.2;
    let mut rays = LAMP_RAYS_PER_FRAME;
    for lamp in world.light_objects.lock().iter() {
        let dist = (lamp.pos - camera_pos).length();
        if dist > visible_range {
            continue;
        }
        if dist < OCC_CORONA_RANGE {
            let key = [
                (lamp.pos.x * 2.0).round() as i64,
                (lamp.pos.y * 2.0).round() as i64,
                (lamp.pos.z * 2.0).round() as i64,
            ];
            let entry = lamp_vis.1.entry(key).or_insert((true, epoch.wrapping_sub(1)));
            if entry.1 != epoch && rays > 0 {
                rays -= 1;
                *entry = (sees(&coll, camera_pos, lamp.pos), epoch);
            }
            let seen_lamp = entry.0;
            if !seen_lamp {
                continue;
            }
        }
        for ((c, _), lit) in lamp.coronas.iter().zip(&lamp.lit) {
            if *lit <= 0.0 {
                continue;
            }
            let mut corona = *c;
            corona.brightness *= lit.min(1.0);
            scene.coronas.push(corona);
        }
    }
    let t_lamp_loop = std::time::Instant::now();
    for list in world.particle_objects.lock().values() {
        for po in list {
            if (po.pos - camera_pos).length() < visible_range {
                particle_sprites(&po.set, &mut scene.smoke, &mut scene.coronas);
            }
        }
    }
    if omsi_cfg::env::var_os("OMSI_DEBUG_PARTICLES").is_some() {
        if let Some(p) = scene.smoke.first() {
            log::info!("smoke: {} particles from objects, first at ({:.1}, {:.1}, {:.1}) size {:.2} alpha {:.2}", scene.smoke.len(), p.position.x, p.position.y, p.position.z, p.size, p.alpha);
        }
    }
    let t_particles = std::time::Instant::now();
    // window light only for the few vehicles nearest the camera (each is up to six lights
    // the shaders test every pixel); OMSI_NO_SPILL=1 switches it off altogether
    let spill_ok: Vec<bool> = {
        static OFF: std::sync::OnceLock<bool> = std::sync::OnceLock::new();
        let off = *OFF.get_or_init(|| omsi_cfg::env::var_os("OMSI_NO_SPILL").is_some());
        let mut order: Vec<(f64, usize)> = vehicles
            .iter()
            .enumerate()
            .map(|(i, v)| ((v.position - camera_pos).length(), i))
            .filter(|(d, _)| *d < SPILL_RANGE)
            .collect();
        order.sort_by(|a, b| a.0.total_cmp(&b.0));
        let mut ok = vec![false; vehicles.len()];
        if !off {
            for (_, i) in order.into_iter().take(SPILL_VEHICLES) {
                ok[i] = true;
            }
        }
        ok
    };
    // (the mesh walk per lamp is the costliest part of this loop: a few per frame, the rest
    // of the vehicles' lamps are judged by one test for the whole vehicle)
    // (each vehicle keeps the last answers of its mesh walks and asks again for an eighth of
    // them a frame, the whole-vehicle answer every eighth frame)
    static VEH_OCC: std::sync::LazyLock<std::sync::Mutex<std::collections::HashMap<usize, (bool, Vec<bool>)>>> =
        std::sync::LazyLock::new(Default::default);
    static OCC_FRAME: std::sync::atomic::AtomicUsize = std::sync::atomic::AtomicUsize::new(0);
    let frame = OCC_FRAME.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
    let mut veh_occ = VEH_OCC.lock().unwrap_or_else(|e| e.into_inner());
    if veh_occ.len() > 128 {
        veh_occ.clear();
    }
    let mut mesh_tests = 8usize;
    for (vi, v) in vehicles.iter().enumerate() {
        // (a vehicle out of sight: no lamps, no ray tests, no smoke)
        if (v.position - camera_pos).length() > visible_range {
            continue;
        }
        let first_corona = scene.coronas.len();
        vehicle_lights(v, &mut scene.coronas, &mut scene.lights, night, spill_ok[vi]);
        let seen_world = world.light_occluders.lock().clone();
        let sections = body_sections(v);
        let vkey = *v as *const VehicleInstance as usize;
        let entry = veh_occ.entry(vkey).or_insert_with(|| (false, Vec::new()));
        if (frame + vi) % 8 == 0 {
            entry.0 = blocked_by_meshes(&coll, &seen_world, camera_pos, v.position);
        }
        // (the mesh walk costs a probe every few metres: near vehicles test each lamp, far
        // ones once for the whole vehicle)
        let near_v = (v.position - camera_pos).length() < 15.0;
        let far_hidden = !near_v && entry.0;
        // (only this vehicle's own coronas are tested, not every one of the scene so far)
        let mut mine = scene.coronas.split_off(first_corona);
        entry.1.resize(mine.len(), false);
        let mut ci = 0usize;
        mine.retain_mut(|c| {
            let i = ci;
            ci += 1;
            let blocked = near_v && !body_hides(&sections, camera_pos, c.position) && {
                if (i + frame) % 8 == 0 && mesh_tests > 0 {
                    mesh_tests -= 1;
                    entry.1[i] = blocked_by_meshes(&coll, &seen_world, camera_pos, c.position);
                }
                entry.1[i]
            };
            if body_hides(&sections, camera_pos, c.position) || far_hidden || blocked {
                return false;
            }
            if !c.beam && !c.halo {
                c.size = c.size.min(0.6);
                c.brightness = c.brightness.min(1.0);
            }
            true
        });
        scene.coronas.extend(mine);
        particle_sprites(&v.particles, &mut scene.smoke, &mut scene.coronas);
        for t in &v.trailers {
            particle_sprites(&t.particles, &mut scene.smoke, &mut scene.coronas);
        }
    }
    let t_vehicles = std::time::Instant::now();
    let (vis, night) = cone_weather();
    scene.coronas.retain_mut(|c| {
        if !c.beam && !c.halo {
            return true;
        }
        if vis >= 2000.0 {
            return false;
        }
        let glow = (night * night + 0.8) * 0.6 * c.brightness * settings().corona;
        let reach = 3.0 * (100.0 / vis.max(1.0)).sqrt() * glow * c.size;
        c.size = if c.beam { 2.0 * reach } else { reach };
        c.brightness = if c.beam { 0.3 } else { 0.2 };
        c.beam_width = vis.max(1.0);
        c.size > 0.05
    });
    if omsi_cfg::env::var_os("OMSI_DEBUG_CONES").is_some() {
        log::info!("cones: visibility {vis:.0} m, dark {night:.2}, {} cones of {} coronas", scene.coronas.iter().filter(|c| c.beam).count(), scene.coronas.len());
        for c in scene.coronas.iter().filter(|c| c.beam).take(4) {
            log::info!("  cone at ({:.1}, {:.1}, {:.1}) dir {:?} radius {:.2} half angles {:.0}/{:.0} deg tex {}", c.position.x, c.position.y, c.position.z, c.direction, c.size, c.inner_cos.to_degrees(), c.cone_cos.to_degrees(), c.texture);
        }
    }
    if omsi_cfg::env::var_os("OMSI_DEBUG_LIGHT").is_some() {
        scene.lights.push(PointLight {
            position: camera_pos + DVec3::new(0.0, 15.0, -2.0),
            radius: 40.0,
            color: [1.0, 0.9, 0.7],
            intensity: 2.0,
            ..Default::default()
        });
        scene.coronas.push(Corona {
            position: camera_pos + DVec3::new(0.0, 15.0, 0.0),
            size: 1.0,
            color: [1.0, 0.9, 0.7],
            brightness: 1.0,
            direction: Vec3::ZERO,
            cone_cos: -1.0,
            ..Default::default()
        });
        log::info!(
            "static lights: {:?}",
            world
                .static_lights
                .lock()
                .iter()
                .take(3)
                .collect::<Vec<_>>()
        );
        log::info!(
            "static coronas: {:?}",
            world
                .static_coronas
                .lock()
                .iter()
                .take(3)
                .collect::<Vec<_>>()
        );
    }
    scene.lights.sort_by(|a, b| (a.position - camera_pos).length_squared().total_cmp(&(b.position - camera_pos).length_squared()));
    let generation = world
        .tiles_generation
        .load(std::sync::atomic::Ordering::Relaxed);
    let seen = world.light_occluders.lock().clone();
    let t_occ = std::time::Instant::now();
    assign_occluders(&coll, &seen, generation, scene, camera_pos, vehicles);
    let total = t_start.elapsed();
    if total.as_millis() > 40 {
        static LAST: std::sync::Mutex<Option<std::time::Instant>> = std::sync::Mutex::new(None);
        let mut last = LAST.lock().unwrap_or_else(|e| e.into_inner());
        if last.map(|t| t.elapsed().as_secs_f32() > 2.0).unwrap_or(true) {
            *last = Some(std::time::Instant::now());
            log::info!(
                "lights.collect {:.0} ms: map lights {:.0}, lamp objects + vehicles {:.0} (lamp loop {:.0}, particles {:.0}, vehicles {:.0}), occluders {:.0} ({} lights, {} coronas, {} occluders, {} vehicles)",
                total.as_secs_f64() * 1000.0,
                (t_lamps - t_start).as_secs_f64() * 1000.0,
                (t_occ - t_lamps).as_secs_f64() * 1000.0,
                (t_lamp_loop - t_lamps).as_secs_f64() * 1000.0,
                (t_particles - t_lamp_loop).as_secs_f64() * 1000.0,
                (t_vehicles - t_particles).as_secs_f64() * 1000.0,
                t_occ.elapsed().as_secs_f64() * 1000.0,
                scene.lights.len(),
                scene.coronas.len(),
                scene.occluders.len(),
                vehicles.len()
            );
        }
    }
}

pub fn particle_sprites(set: &omsi_sim::particles::ParticleSet, smoke: &mut Vec<omsi_render::SmokeParticle>, coronas: &mut Vec<Corona>) {
    for (p, def) in set.particles() {
        let alpha = p.alpha();
        if alpha <= 0.002 {
            continue;
        }
        if def.emissive {
            coronas.push(Corona {
                position: p.pos,
                size: (p.size() * 0.5).max(0.02),
                color: p.color,
                brightness: alpha,
                direction: Vec3::ZERO,
                cone_cos: -1.0,
                z_offset: 0.0,
                ..Default::default()
            });
        } else {
            smoke.push(omsi_render::SmokeParticle { position: p.pos, size: p.size() * 0.5, color: p.color, alpha });
        }
    }
}

pub fn load_smoke_texture(renderer: &mut omsi_render::Renderer, root: &std::path::Path) {
    let path = omsi_cfg::resolve_path(root, "Texture/rauch.tga");
    match omsi_texture::decode_file(&path) {
        Ok(img) => renderer.set_smoke_texture(&img),
        Err(e) => log::warn!("smoke texture {}: {e}", path.display()),
    }
}

struct CoronaTextures {
    ids: std::collections::HashMap<std::path::PathBuf, u16>,
    pending: Vec<(u16, std::path::PathBuf)>,
    root: Option<std::path::PathBuf>,
}

static CORONA_TEXTURES: std::sync::Mutex<Option<CoronaTextures>> = std::sync::Mutex::new(None);

pub fn set_corona_root(root: &std::path::Path) {
    let mut g = CORONA_TEXTURES.lock().unwrap_or_else(|e| e.into_inner());
    let t = g.get_or_insert_with(|| CoronaTextures { ids: Default::default(), pending: Vec::new(), root: None });
    t.root = Some(root.to_path_buf());
}

fn texture_id_of(path: std::path::PathBuf) -> u16 {
    let mut g = CORONA_TEXTURES.lock().unwrap_or_else(|e| e.into_inner());
    let t = g.get_or_insert_with(|| CoronaTextures { ids: Default::default(), pending: Vec::new(), root: None });
    if let Some(id) = t.ids.get(&path) {
        return *id;
    }
    let id = (t.ids.len() + 1).min(u16::MAX as usize) as u16;
    t.ids.insert(path.clone(), id);
    t.pending.push((id, path));
    id
}

pub fn corona_texture_id(model_dir: &std::path::Path, name: &str) -> u16 {
    static KNOWN: std::sync::Mutex<Option<std::collections::HashMap<(std::path::PathBuf, String), u16>>> = std::sync::Mutex::new(None);
    let key = (model_dir.to_path_buf(), name.to_string());
    if let Some(&id) = KNOWN.lock().unwrap_or_else(|e| e.into_inner()).as_ref().and_then(|m| m.get(&key)) {
        return id;
    }
    let root = CORONA_TEXTURES.lock().unwrap_or_else(|e| e.into_inner()).as_ref().and_then(|t| t.root.clone()).unwrap_or_default();
    let mut id = 0;
    for d in crate::scene::texture_dirs(&root, model_dir) {
        let p = omsi_cfg::resolve_path(&d, name);
        if omsi_cfg::vfs::is_file(&p) {
            id = texture_id_of(p);
            break;
        }
    }
    KNOWN.lock().unwrap_or_else(|e| e.into_inner()).get_or_insert_with(Default::default).insert(key, id);
    id
}

fn stock_texture_id(name: &str) -> u16 {
    static KNOWN: std::sync::Mutex<Option<std::collections::HashMap<(std::path::PathBuf, String), u16>>> = std::sync::Mutex::new(None);
    let root = CORONA_TEXTURES.lock().unwrap_or_else(|e| e.into_inner()).as_ref().and_then(|t| t.root.clone()).unwrap_or_default();
    let key = (root, name.to_string());
    if let Some(&id) = KNOWN.lock().unwrap_or_else(|e| e.into_inner()).as_ref().and_then(|m| m.get(&key)) {
        return id;
    }
    let p = omsi_cfg::resolve_path(&key.0, &format!("Texture/{name}"));
    let id = if omsi_cfg::vfs::is_file(&p) { texture_id_of(p) } else { 0 };
    KNOWN.lock().unwrap_or_else(|e| e.into_inner()).get_or_insert_with(Default::default).insert(key, id);
    id
}

pub fn cone_texture_id() -> u16 {
    stock_texture_id("light_cone.bmp")
}

pub fn glow_texture_id() -> u16 {
    stock_texture_id("licht.bmp")
}

pub fn star_texture_id() -> u16 {
    stock_texture_id("light_effect1.bmp")
}

pub fn upload_corona_textures(renderer: &mut omsi_render::Renderer) {
    let pending = {
        let mut g = CORONA_TEXTURES.lock().unwrap_or_else(|e| e.into_inner());
        match g.as_mut() {
            Some(t) => std::mem::take(&mut t.pending),
            None => return,
        }
    };
    for (id, path) in pending {
        match omsi_texture::decode_file(&path) {
            Ok(img) => renderer.set_corona_texture(id, &img),
            Err(e) => log::warn!("corona picture {}: {e}", path.display()),
        }
    }
}

static CONE: std::sync::atomic::AtomicU32 = std::sync::atomic::AtomicU32::new(0);

static CONE_NIGHT: std::sync::atomic::AtomicU32 = std::sync::atomic::AtomicU32::new(0);

pub fn set_cone_strength(fog_visibility_m: f32, _precip: f32, night: f32) {
    CONE.store(fog_visibility_m.to_bits(), std::sync::atomic::Ordering::Relaxed);
    CONE_NIGHT.store(night.clamp(0.0, 1.0).to_bits(), std::sync::atomic::Ordering::Relaxed);
}

fn cone_weather() -> (f32, f32) {
    let vis = f32::from_bits(CONE.load(std::sync::atomic::Ordering::Relaxed));
    let night = f32::from_bits(CONE_NIGHT.load(std::sync::atomic::Ordering::Relaxed));
    (if vis > 0.0 { vis } else { 1.0e6 }, night)
}

/// How far lights, coronas and particles are worth making: the loaded area, and no further
/// than the weather's fog lets anything be seen (it swallows 99 % at twice the visibility).
fn visible_range() -> f64 {
    let (vis, _) = cone_weather();
    CORONA_RANGE.min((vis as f64 * 2.0).max(60.0))
}

pub fn vehicle_velocity(v: &omsi_sim::VehicleInstance) -> glam::Vec3 {
    let h = v.heading.to_radians();
    glam::Vec3::new(h.sin() as f32, h.cos() as f32, 0.0) * v.physics.speed
}