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
        cache.hiding.retain(|_, h| f.index.saturating_sub(h.last_seen) < 120);
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

        particle_sprites(&v.particles, true, &mut scene.smoke, &mut scene.coronas);
        for t in &v.trailers {
            particle_sprites(&t.particles, true, &mut scene.smoke, &mut scene.coronas);
        }
    }
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
                hiding
                    .lamps
                    .insert(key, blocked_by_meshes(&f.coll, &f.seen, f.camera, c.position));
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
        t.parse::<f32>().ok().unwrap_or_else(|| v.var(t).unwrap_or(0.0))
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
        if ec.off {
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
    for t in &v.trailers {
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
            &value_of,
            &t.light_fade,
        ));
    }

    let night = f.night.max(fog_darkness() * cfg.weather_night);
    headlamps(cfg, faders, v, night, lights, coronas);
    let mut sections = vec![Section {
        model: &ty.model,
        rot: body_rot,
        origin: v.position,
        owner: super::owner_key(v),
    }];
    sections.extend(v.trailers.iter().map(|t| Section {
        model: &t.ty.model,
        rot: t.body_rotation(),
        origin: t.position,
        owner: super::owner_key(t),
    }));
    for s in &sections {
        spotlights_2(cfg, faders, s, &value_of, night, lights);
    }
    if spill && cfg.spill.on && night > 0.05 {
        interior_spill(cfg, faders, v, &value_of, night, lights);
    }
}
