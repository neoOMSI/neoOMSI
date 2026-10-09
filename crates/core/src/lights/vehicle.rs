use super::beam::headlamps;
use super::config::{LightSettings, exterior_cfg};
use super::fader::Faders;
use super::frame::Frame;
use super::geom::{blocked_by_meshes, bodies, body_hides};
use super::particles::particle_sprites;
use super::sources::corona_lights;
use super::spill::{interior_spill, spill_vehicles};
use super::spot2::{Section, spotlights_2};
use super::tuning::*;
use super::weather::fog_darkness;
use glam::{Mat4, Vec3};
use ::render::{Corona, PointLight, Scene};
use ::simulation::VehicleInstance;
use std::collections::HashMap;

#[derive(Default)]
struct Hiding {
    whole: bool,
    retry: bool,
    lamps: HashMap<[i32; 3], bool>,
    last_seen: u64,
}

#[derive(Default)]
pub(super) struct VehicleCache {
    hiding: HashMap<usize, Hiding>,
}

struct Budget {
    lamp_tests: usize,
    vehicle_tests: usize,
}

pub(super) fn emit_all(
    f: &Frame,
    faders: &mut Faders,
    cache: &mut VehicleCache,
    scene: &mut Scene,
    vehicles: &[&VehicleInstance],
) {
    if cache.hiding.len() > 128 {
        cache
            .hiding
            .retain(|_, h| f.index.saturating_sub(h.last_seen) < 120);
        if cache.hiding.len() > 128 {
            cache.hiding.clear();
        }
    }
    let spill_ok = spill_vehicles(&f.cfg, f.camera, vehicles);
    let mut budget = Budget {
        lamp_tests: MESH_TESTS_PER_FRAME,
        vehicle_tests: VEHICLE_TESTS_PER_FRAME,
    };
    let mut mine: Vec<Corona> = Vec::new();
    let first = scene.lights.len();

    for (vi, v) in vehicles.iter().enumerate() {
        if f.dist(v.position) > f.visible_range {
            continue;
        }
        mine.clear();
        road_and_lamp_lights(f, faders, v, spill_ok[vi], &mut mine, &mut scene.lights);
        corona_lights(&f.cfg, &mine, f.dark, SRC_MAX_VEHICLE, &mut scene.lights);

        let hiding = cache.hiding.entry(super::owner_key(*v)).or_default();
        hiding.last_seen = f.index;
        cull_hidden(f, v, vi, hiding, &mut budget, &mut mine);
        scene.coronas.append(&mut mine);

        if !v.part_hidden(0) {
            particle_sprites(&v.particles, true, &mut scene.smoke, &mut scene.coronas);
        }
        for (k, t) in v.trailers.iter().enumerate() {
            if !v.part_hidden(k + 1) {
                particle_sprites(&t.particles, true, &mut scene.smoke, &mut scene.coronas);
            }
        }
    }
    // (as the map's lamps: a light wholly beyond their range lights only a few far pixels)
    let range = f.light_range();
    let mut k = 0;
    scene.lights.retain(|l| {
        k += 1;
        k <= first || f.dist(l.position) - (l.radius as f64) < range
    });
}

fn cull_hidden(
    f: &Frame,
    v: &VehicleInstance,
    index: usize,
    hiding: &mut Hiding,
    budget: &mut Budget,
    coronas: &mut Vec<Corona>,
) {
    let frame = f.index as usize;
    let body_list = bodies(v);

    let due = (frame + index) % 8 == 0 || hiding.retry;
    if due && budget.vehicle_tests > 0 {
        budget.vehicle_tests -= 1;
        hiding.retry = false;
        hiding.whole = blocked_by_meshes(&f.coll, &f.seen, f.camera, v.position);
    } else if due {
        hiding.retry = true;
    }
    let near = f.dist(v.position) < NEAR_VEHICLE;
    if !near && hiding.whole {
        coronas.clear();
        return;
    }
    if hiding.lamps.len() > 256 {
        hiding.lamps.clear();
    }
    let inv = body_list[0].inv;
    let mut i = 0usize;
    coronas.retain_mut(|c| {
        let n = i;
        i += 1;
        if body_hides(&body_list, f.camera, c.position) {
            return false;
        }
        if near {
            let q = inv.transform_point3((c.position - v.position).as_vec3());
            let key = [
                (q.x * 10.0).round() as i32,
                (q.y * 10.0).round() as i32,
                (q.z * 10.0).round() as i32,
            ];
            if (n + frame) % 8 == 0 && budget.lamp_tests > 0 {
                budget.lamp_tests -= 1;
                hiding.lamps.insert(
                    key,
                    blocked_by_meshes(&f.coll, &f.seen, f.camera, c.position),
                );
            }
            if hiding.lamps.get(&key).copied().unwrap_or(false) {
                return false;
            }
        }
        if c.beam {
            c.position += f.cfg.cone_shift(c.direction);
        } else if !c.halo {
            c.size = c.size.min(0.6);
            c.brightness = c.brightness.min(1.0);
        }
        true
    });
}

fn road_and_lamp_lights(
    f: &Frame,
    faders: &mut Faders,
    v: &VehicleInstance,
    spill: bool,
    coronas: &mut Vec<Corona>,
    lights: &mut Vec<PointLight>,
) {
    let cfg: &LightSettings = &f.cfg;
    let ty = &v.ty;
    let value_of = |name: &str| -> f32 {
        let t = name.trim();
        t.parse::<f32>()
            .ok()
            .unwrap_or_else(|| v.var(t).unwrap_or(0.0))
    };
    let transform_of = |def_index: usize| -> Mat4 {
        match ty.meshes.iter().position(|m| m.def_index == def_index) {
            Some(i) => v.mesh_local_transform(i),
            None => v.body_rotation(),
        }
    };
    let body_rot = v.body_rotation();
    let front = body_rot.transform_vector3(Vec3::Y).normalize_or_zero();

    for (mut c, owner) in crate::scene::model_lights_owned(
        &ty.model,
        &transform_of,
        v.position,
        &value_of,
        &v.light_fade,
    ) {
        let ec = exterior_cfg(owner);
        if ec.off || v.part_hidden(0) {
            continue;
        }
        c.brightness *= ec.gain;
        c.size *= ec.size.max(0.0);
        if !c.beam && !c.halo && c.direction.dot(front) > 0.7 {
            c.size *= 0.5;
        }
        c.spread = ec.spread.max(0.05);
        for k in 0..3 {
            c.color[k] *= ec.color[k];
        }
        c.position += body_rot.transform_vector3(Vec3::from(ec.shift)).as_dvec3();
        coronas.push(c);
    }
    for (k, t) in v.trailers.iter().enumerate() {
        if v.part_hidden(k + 1) {
            continue;
        }
        let part_xf = |def_index: usize| -> Mat4 {
            match t.ty.meshes.iter().position(|m| m.def_index == def_index) {
                Some(i) => t.mesh_local_transform(i),
                None => t.body_rotation(),
            }
        };
        coronas.extend(crate::scene::model_lights_faded(
            &t.ty.model,
            &part_xf,
            t.position,
            &|n| t.value(v, n),
            &t.light_fade,
        ));
    }
    let body = v.body_rotation();
    static FORCED: std::sync::OnceLock<Option<f32>> = std::sync::OnceLock::new();
    let forced = *FORCED.get_or_init(|| {
        ::legacy_config::env::var("OMSI_SPOT_SELECT")
            .ok()
            .and_then(|s| s.trim().parse::<f32>().ok())
    });
    let ai_on = v.ai_lights;
    let cfg = settings();
    let bad = weather_darkness();
    let night = night.max(bad * cfg.weather_night);
    let selected = forced
        .or_else(|| v.var("Spot_Select"))
        .filter(|s| *s >= 0.0 || !ai_on);
    if let Some(sel) = selected.or(ai_on.then_some(0.0)) {
        if sel >= 0.0 {
            let lamps: Vec<[f32; 3]> = ty
                .model
                .meshes
                .iter()
                .flat_map(|m| {
                    m.light_enh
                        .iter()
                        .map(|l| l.pos)
                        .chain(m.light_enh_2.iter().map(|l| l.pos))
                })
                .collect();
            let spot = ty
                .model
                .spotlights
                .get(sel as usize)
                .or_else(|| ai_on.then(|| ty.model.spotlights.first()).flatten())
                .map(|sp| sp.values)
                .or_else(|| ai_on.then(|| ai_spotlight(&lamps)).flatten());
            if let Some(vals) = spot {
                if cfg.low.on {
                    let d = body
                        .transform_vector3(Vec3::new(vals[3], vals[4], vals[5]))
                        .normalize_or_zero();
                    let color = [vals[6] / 255.0, vals[7] / 255.0, vals[8] / 255.0];
                    let mut apex = Vec3::new(vals[0], vals[1], vals[2]);
                    let dl = Vec3::new(vals[3], vals[4], vals[5]).normalize_or_zero();
                    let dir_y = if dl.y > 0.3 {
                        1.0
                    } else if dl.y < -0.3 {
                        -1.0
                    } else {
                        0.0
                    };
                    if dir_y != 0.0 {
                        let nose = lamps.iter().map(|l| l[1] * dir_y).reduce(f32::max).map(|m| m * dir_y);
                        let edge = ty.def.bounding_box.map(|bb| bb[4] + dir_y * bb[1] * 0.5);
                        if let Some(face) = spot_face(nose, edge, apex.y, dir_y) {
                            apex.y = face + dir_y * 0.05;
                        }
                    }
                    let half_width = ty
                        .def
                        .bounding_box
                        .map_or(1.25, |bb| (bb[0] * 0.5).min(1.25));
                    let on_face: Vec<f32> = lamps
                        .iter()
                        .filter(|l| dir_y != 0.0 && (l[1] - apex.y).abs() < 0.35)
                        .map(|l| (l[0] - vals[0]).abs())
                        .collect();
                    let spread =
                        (on_face.iter().sum::<f32>() / on_face.len().max(1) as f32).min(half_width);
                    let sides: &[f32] = if spread > 0.1 { &[-1.0, 1.0] } else { &[0.0] };
                    let right = body.transform_vector3(Vec3::X).normalize_or_zero();
                    let base = body.transform_point3(apex);
                    let half = |deg: f32| (deg.clamp(1.0, 179.0) * 0.5).to_radians().cos();
                    let (inner, outer) = (vals[10], vals[11]);
                    let cone = [half(inner.min(outer)), half(outer.max(inner))];
                    let reach_v = spot_reach(vals[9], 45.0);
                    let radius = spot_reach(vals[9], 60.0);
                    let short = short_range_gain(vals[9]);
                    for side in sides {
                        let at = v.position + (base + right * spread * side).as_dvec3();
                        let lamp = PointLight {
                            position: at,
                            color,
                            direction: d,
                            cone,
                            ..Default::default()
                        };
                        lights.push(PointLight {
                            radius: reach_v,
                            intensity: cfg.vanilla / sides.len() as f32
                                * (0.3 + 0.7 * night)
                                * short,
                            mode: LightMode::Vanilla,
                            ..lamp
                        });
                        lights.push(PointLight {
                            radius,
                            intensity: cfg.headlight / sides.len() as f32
                                * (1.0 + bad * cfg.weather_boost)
                                * short,
                            core: (radius * 0.25).clamp(0.1, 1.0),
                            // (100: a dipped beam, which shines by its cookie; below 0: a full beam)
                            beam: if full_beam_gain(vals[9]) < 0.0 {
                                full_beam_gain(vals[9])
                            } else {
                                100.0
                            },
                            mode: LightMode::Enhanced,
                            ..lamp
                        });
                    }
                }
            }
        }
    }
    for (k, t) in v.trailers.iter().enumerate() {
        if v.part_hidden(k + 1) {
            continue;
        }
        let section = Section {
            model: &t.ty.model,
            rot: t.body_rotation(),
            origin: t.position,
            owner: super::owner_key(t),
        };
        spotlights_2(cfg, faders, &section, &|n| t.value(v, n), night, lights);
    }
    if spill && cfg.spill.on && night > 0.05 {
        interior_spill(cfg, faders, v, &value_of, night, lights);
    }
}
