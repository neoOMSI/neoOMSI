use super::*;

#[derive(Default)]
pub(super) struct NearLights {
    pub(super) world: usize,
    pub(super) generation: u64,
    pub(super) lamps_on: bool,
    pub(super) centre: DVec3,
    pub(super) counts: (usize, usize),
    pub(super) lights: Vec<PointLight>,
    pub(super) coronas: Vec<Corona>,
    pub(super) build: Option<NearBuild>,
    pub(super) vis_centre: DVec3,
    pub(super) vis_eye: DVec3,
    pub(super) vis_cursor: usize,
    pub(super) vis_valid: bool,
    pub(super) light_vis: Vec<bool>,
    pub(super) corona_vis: Vec<bool>,
}

/// A rebuild of the near lists in progress: the enclosure test of every light costs 300 ms
/// at once, so it goes on for a few milliseconds a frame while the old lists serve.
pub(super) struct NearBuild {
    pub(super) world: usize,
    pub(super) generation: u64,
    pub(super) lamps_on: bool,
    pub(super) centre: DVec3,
    pub(super) counts: (usize, usize),
    pub(super) src_lights: Vec<PointLight>,
    pub(super) src_coronas: Vec<Corona>,
    pub(super) li: usize,
    pub(super) ci: usize,
    pub(super) lights: Vec<PointLight>,
    pub(super) coronas: Vec<Corona>,
}

pub(super) static NEAR_LIGHTS: std::sync::Mutex<Option<NearLights>> = std::sync::Mutex::new(None);

pub(super) type LampVis = (
    Option<DVec3>,
    std::collections::HashMap<[i64; 3], (bool, u32)>,
    u32,
);
pub(super) static LAMP_VIS: std::sync::LazyLock<std::sync::Mutex<LampVis>> =
    std::sync::LazyLock::new(|| std::sync::Mutex::new((None, Default::default(), 0)));

thread_local! {
    static COLLECT_TIMES: std::cell::Cell<[f64; 6]> = const { std::cell::Cell::new([0.0; 6]) };
}

pub fn take_collect_times() -> [f64; 6] {
    COLLECT_TIMES.with(|c| c.replace([0.0; 6]))
}

pub const COLLECT_NAMES: [&str; 6] = [
    "lights.collect.map",
    "lights.collect.lamp_objects",
    "lights.collect.particles",
    "lights.collect.vehicles",
    "lights.collect.coronas_sort",
    "lights.collect.occluders",
];

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
    ::simulation::particles::set_eye(camera_pos);
    let night = daylight.night;
    let coll = world.collision.lock().clone();
    let map_spot = settings().map_spot;
    let lamp_cfg = settings().lamp_light;
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
                    if on <= 0.0
                        || (c.corona.position - camera_pos).length() > CORONA_RANGE + NEAR_MARGIN
                    {
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
            while b.li < b.src_lights.len() && t_build.elapsed().as_micros() < 1500 {
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
            while b.li >= b.src_lights.len()
                && b.ci < b.src_coronas.len()
                && t_build.elapsed().as_micros() < 1500
            {
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
        if !near.vis_valid
            || (near.vis_cursor >= total && (near.vis_centre - camera_pos).length() > OCC_RECHECK)
        {
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
                near.corona_vis[i - nl] =
                    (p - eye).length() > OCC_CORONA_RANGE || sees(&coll, eye, p);
            }
            near.vis_cursor += 1;
            budget -= 1;
        }
        let light_range2 = MAP_LIGHT_RANGE.min(visible_range).powi(2);
        scene.lights.extend(
            near.lights
                .iter()
                .zip(&near.light_vis)
                .filter(|(l, vis)| {
                    **vis && (l.position - camera_pos).length_squared() < light_range2
                })
                .map(|(l, _)| apply_map_spot(*l, &map_spot)),
        );
        if lamp_cfg.on && lamp_cfg.gain > 0.0 {
            let mut lamps: Vec<(f64, &Corona)> = near
                .coronas
                .iter()
                .zip(&near.corona_vis)
                .filter(|(c, vis)| {
                    **vis && !c.beam && !c.halo && c.flags & 8 == 0 && c.brightness > 0.0
                })
                .map(|(c, _)| ((c.position - camera_pos).length(), c))
                .filter(|(d, _)| *d < MAP_LIGHT_RANGE.min(visible_range))
                .collect();
            lamps.sort_by(|a, b| a.0.total_cmp(&b.0));
            let mut seen_pos: std::collections::HashSet<[i64; 3]> = Default::default();
            let dark = 0.3 + 0.7 * night.clamp(0.0, 1.0);
            for (_, c) in lamps.into_iter().take(lamp_cfg.max.max(0) as usize * 2) {
                let key = [
                    (c.position.x * 10.0).round() as i64,
                    (c.position.y * 10.0).round() as i64,
                    (c.position.z * 10.0).round() as i64,
                ];
                if !seen_pos.insert(key) {
                    continue;
                }
                if seen_pos.len() > lamp_cfg.max.max(0) as usize {
                    break;
                }
                let radius = lamp_cfg.range.max(0.5);
                let l = PointLight {
                    position: c.position,
                    radius,
                    color: c.color,
                    intensity: c.brightness.min(2.0) * lamp_cfg.gain * dark,
                    core: lamp_cfg.core.min(radius),
                    mode: LightMode::Both,
                    ..Default::default()
                };
                scene.lights.push(apply_map_spot(l, &map_spot));
            }
        }
        scene.coronas.extend(
            near.coronas
                .iter()
                .zip(&near.corona_vis)
                .filter(|(c, vis)| {
                    **vis && (c.position - camera_pos).length_squared() <= visible_range * visible_range
                })
                .map(|(c, _)| *c),
        );
    }
    let t_lamps = std::time::Instant::now();
    // (whether a street lamp is seen is asked again only after the camera has moved a few
    // metres, as for the map's own lights: the ray went through the collision world for
    // every lamp in range every frame)
    let mut lamp_vis = LAMP_VIS.lock().unwrap_or_else(|e| e.into_inner());
    if lamp_vis.0.is_none()
        || lamp_vis
        .0
        .map(|c| (c - camera_pos).length() > OCC_RECHECK)
        .unwrap_or(true)
    {
        lamp_vis.0 = Some(camera_pos);
        lamp_vis.2 = lamp_vis.2.wrapping_add(1);
        if lamp_vis.1.len() > 20000 {
            lamp_vis.1.clear();
        }
    }
    let epoch = lamp_vis.2;
    let mut rays = LAMP_RAYS_PER_FRAME;
    let mut obj_lamps: Vec<Corona> = Vec::new();
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
            let entry = lamp_vis
                .1
                .entry(key)
                .or_insert((true, epoch.wrapping_sub(1)));
            if entry.1 != epoch && rays > 0 {
                rays -= 1;
                *entry = (sees(&coll, camera_pos, lamp.pos), epoch);
            }
            let seen_lamp = entry.0;
            if !seen_lamp {
                continue;
            }
        }
        let first_obj = scene.coronas.len();
        for ((c, _), lit) in lamp.coronas.iter().zip(&lamp.lit) {
            if *lit <= 0.0 {
                continue;
            }
            let mut corona = *c;
            corona.brightness *= lit.min(1.0);
            scene.coronas.push(corona);
        }
        if dist < SRC_RANGE {
            obj_lamps.extend_from_slice(&scene.coronas[first_obj..]);
        }
    }
    obj_lamps.sort_by(|a, b| {
        (a.position - camera_pos)
            .length_squared()
            .total_cmp(&(b.position - camera_pos).length_squared())
    });
    obj_lamps.truncate(SRC_MAX_OBJECTS * 3);
    corona_lights(
        &obj_lamps,
        0.3 + 0.7 * night.clamp(0.0, 1.0),
        SRC_MAX_OBJECTS,
        &mut scene.lights,
    );
    let t_lamp_loop = std::time::Instant::now();
    for list in world.particle_objects.lock().values() {
        for po in list {
            if (po.pos - camera_pos).length() < visible_range {
                particle_sprites(&po.set, false, &mut scene.smoke, &mut scene.coronas);
            }
        }
    }
    if ::legacy_config::env::var_os("OMSI_DEBUG_PARTICLES").is_some() {
        if let Some(p) = scene.smoke.first() {
            log::info!(
                "smoke: {} particles from objects, first at ({:.1}, {:.1}, {:.1}) size {:.2} alpha {:.2}",
                scene.smoke.len(),
                p.position.x,
                p.position.y,
                p.position.z,
                p.size,
                p.alpha
            );
        }
    }
    let t_particles = std::time::Instant::now();
    // window light only for the few vehicles nearest the camera (each is up to six lights
    // the shaders test every pixel); OMSI_NO_SPILL=1 switches it off altogether
    let spill_ok: Vec<bool> = {
        static OFF: std::sync::OnceLock<bool> = std::sync::OnceLock::new();
        let sp = settings().spill;
        let off = *OFF.get_or_init(|| ::legacy_config::env::var_os("OMSI_NO_SPILL").is_some()) || !sp.on;
        let mut order: Vec<(f64, usize)> = vehicles
            .iter()
            .enumerate()
            .map(|(i, v)| {
                let half = body_box(&v.ty).map_or(0.0, |b| b[1] as f64 * 0.5);
                (((v.position - camera_pos).length() - half).max(0.0), i)
            })
            .filter(|(d, _)| *d < sp.reach as f64)
            .collect();
        order.sort_by(|a, b| a.0.total_cmp(&b.0));
        let mut ok = vec![false; vehicles.len()];
        if !off {
            for (_, i) in order.into_iter().take(sp.vehicles.max(0) as usize) {
                ok[i] = true;
            }
        }
        ok
    };
    // (the mesh walk per lamp is the costliest part of this loop: a few per frame, the rest
    // of the vehicles' lamps are judged by one test for the whole vehicle)
    // (each vehicle keeps the last answers of its mesh walks and asks again for an eighth of
    // them a frame, the whole-vehicle answer every eighth frame)
    static VEH_OCC: std::sync::LazyLock<
        std::sync::Mutex<std::collections::HashMap<usize, (bool, std::collections::HashMap<[i32; 3], bool>, bool)>>,
    > = std::sync::LazyLock::new(Default::default);
    static OCC_FRAME: std::sync::atomic::AtomicUsize = std::sync::atomic::AtomicUsize::new(0);
    let frame = OCC_FRAME.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
    let mut veh_occ = VEH_OCC.lock().unwrap_or_else(|e| e.into_inner());
    if veh_occ.len() > 128 {
        veh_occ.clear();
    }
    let mut mesh_tests = 3usize;
    let mut veh_tests = 2usize;
    let beam_cfg = settings();
    let seen_world = world.light_occluders.lock().clone();
    let dark_v = 0.3 + 0.7 * night.clamp(0.0, 1.0);
    for (vi, v) in vehicles.iter().enumerate() {
        // (a vehicle out of sight: no lamps, no ray tests, no smoke)
        if (v.position - camera_pos).length() > visible_range {
            continue;
        }
        let first_corona = scene.coronas.len();
        vehicle_lights(
            v,
            &mut scene.coronas,
            &mut scene.lights,
            night,
            spill_ok[vi],
        );
        let sections = body_sections(v);
        let vkey = *v as *const VehicleInstance as usize;
        let entry = veh_occ
            .entry(vkey)
            .or_insert_with(|| (false, Default::default(), false));
        if entry.1.len() > 256 {
            entry.1.clear();
        }
        let inv = sections[0].1;
        let due = (frame + vi) % 8 == 0 || entry.2;
        if due && veh_tests > 0 {
            veh_tests -= 1;
            entry.2 = false;
            entry.0 = blocked_by_meshes(&coll, &seen_world, camera_pos, v.position);
        } else if due {
            entry.2 = true;
        }
        // (the mesh walk costs a probe every few metres: near vehicles test each lamp, far
        // ones once for the whole vehicle)
        let near_v = (v.position - camera_pos).length() < 15.0;
        let far_hidden = !near_v && entry.0;
        // (only this vehicle's own coronas are tested, not every one of the scene so far)
        let mut mine = scene.coronas.split_off(first_corona);
        corona_lights(
            &mine,
            dark_v,
            SRC_MAX_VEHICLE,
            &mut scene.lights,
        );
        let mut ci = 0usize;
        mine.retain_mut(|c| {
            let i = ci;
            ci += 1;
            let q = inv.transform_point3((c.position - v.position).as_vec3());
            let key = [
                (q.x * 10.0).round() as i32,
                (q.y * 10.0).round() as i32,
                (q.z * 10.0).round() as i32,
            ];
            let hidden = body_hides(&sections, camera_pos, c.position);
            if hidden || (far_hidden && !near_v) {
                return false;
            }
            let blocked = near_v && {
                if (i + frame) % 8 == 0 && mesh_tests > 0 {
                    mesh_tests -= 1;
                    let b = blocked_by_meshes(&coll, &seen_world, camera_pos, c.position);
                    entry.1.insert(key, b);
                }
                entry.1.get(&key).copied().unwrap_or(false)
            };
            if far_hidden || blocked {
                return false;
            }
            if !c.beam && !c.halo {
                c.size = c.size.min(0.6);
                c.brightness = c.brightness.min(1.0);
            }
            if c.beam {
                c.position += cone_shift(c.direction, &beam_cfg);
            }
            true
        });
        scene.coronas.extend(mine);
        particle_sprites(&v.particles, true, &mut scene.smoke, &mut scene.coronas);
        for t in &v.trailers {
            particle_sprites(&t.particles, true, &mut scene.smoke, &mut scene.coronas);
        }
    }
    let t_vehicles = std::time::Instant::now();
    let (vis, night) = cone_weather();
    let corona_gain = settings().corona;
    scene.coronas.retain_mut(|c| {
        if !c.beam && !c.halo {
            return true;
        }
        if vis >= 2000.0 {
            return false;
        }
        let glow = (night * night + 0.8) * 0.6 * c.brightness * corona_gain;
        let reach = 3.0 * (100.0 / vis.max(1.0)).sqrt() * glow * c.size;
        c.size = if c.beam { 2.0 * reach } else { reach };
        c.brightness = if c.beam { 0.3 } else { 0.2 };
        c.beam_width = vis.max(1.0);
        c.size > 0.05
    });
    if ::legacy_config::env::var_os("OMSI_DEBUG_CONES").is_some() {
        log::info!(
            "cones: visibility {vis:.0} m, dark {night:.2}, {} cones of {} coronas",
            scene.coronas.iter().filter(|c| c.beam).count(),
            scene.coronas.len()
        );
        for c in scene.coronas.iter().filter(|c| c.beam).take(4) {
            log::info!(
                "  cone at ({:.1}, {:.1}, {:.1}) dir {:?} radius {:.2} half angles {:.0}/{:.0} deg tex {}",
                c.position.x,
                c.position.y,
                c.position.z,
                c.direction,
                c.size,
                c.inner_cos.to_degrees(),
                c.cone_cos.to_degrees(),
                c.texture
            );
        }
    }
    {
        let glow = f32::from_bits(LED_GLOW.load(std::sync::atomic::Ordering::Relaxed));
        // (a switch to measure what the screens' light costs)
        let no_screen_light = ::legacy_config::env::var_os("OMSI_NO_SCREEN_LIGHT").is_some();
        let luma_guard = if no_screen_light {
            None
        } else {
            scene.tex_luma.lock().ok()
        };
        let mut panels: Vec<(f64, DVec3, f32, bool, [f32; 3], Vec3)> = if no_screen_light {
            Vec::new()
        } else {
            scene
                .instances
                .iter()
                .filter(|i| {
                    i.visible
                        && (i.world_centre() - camera_pos).length_squared()
                            < (LED_RANGE + 60.0) * (LED_RANGE + 60.0)
                        && i.materials.iter().any(|m| {
                            scene
                                .materials
                                .get(*m)
                                .is_some_and(|mat| mat.is_screen() || mat.is_led())
                        })
                })
                .filter_map(|i| {
                    // (gate, colour, slot) of the brightest matching screen slot of the instance
                    let gate = |led: bool| {
                        i.materials
                            .iter()
                            .enumerate()
                            .filter(|(_, m)| {
                                scene.materials.get(**m).is_some_and(|m| {
                                    if led {
                                        m.is_led()
                                    } else {
                                        m.is_screen() && !m.is_led()
                                    }
                                })
                            })
                            .map(|(k, m)| {
                                let gate =
                                    i.slot_light.get(k).copied().unwrap_or(1.0).clamp(0.0, 1.0);
                                let seen = |t: Option<usize>| {
                                    t.and_then(|t| {
                                        luma_guard.as_ref().and_then(|l| l.get(&t).copied())
                                    })
                                };
                                // a screen throws only the light it shows: a black or switched-off
                                // script / HTML picture throws none; an LED panel (its dots are the
                                // alpha of its `\S:n` script texture) only as many dots as are lit
                                let mat = scene.materials.get(*m);
                                let (shown, colour) = if led {
                                    (
                                        seen(mat.and_then(|m| m.transmap.map(|t| t.0)))
                                            .map(|(_, a, _)| a)
                                            .unwrap_or(0.0),
                                        LED_COLOR,
                                    )
                                } else {
                                    // (the light takes the colour of the picture)
                                    seen(mat.and_then(|m| m.texture))
                                        .map(|(c, _, rgb)| (c, rgb))
                                        .unwrap_or((0.0, SCREEN_COLOR))
                                };
                                let fx = if led {
                                    1.0
                                } else if mat.is_some_and(|m| m.is_html()) {
                                    screen_fx(1)
                                } else {
                                    screen_fx(3)
                                };
                                (
                                    gate * (shown * if led { 8.0 } else { 3.0 }).clamp(0.0, 1.0)
                                        * fx,
                                    colour,
                                    k,
                                )
                            })
                            .fold((0.0f32, SCREEN_COLOR, usize::MAX), |a, b| {
                                if b.0 > a.0 { b } else { a }
                            })
                    };
                    let led_gate = if glow > 0.0 {
                        gate(true)
                    } else {
                        (0.0, SCREEN_COLOR, usize::MAX)
                    };
                    let (led, (g, colour, slot)) = if led_gate.0 > 0.01 {
                        (true, led_gate)
                    } else {
                        let screen_gate = gate(false);
                        if screen_gate.0 > 0.01 {
                            (false, screen_gate)
                        } else {
                            return None;
                        }
                    };
                    // a screen shines to the side it renders to: its slot's facing direction
                    let face = scene
                        .meshes
                        .get(i.mesh)
                        .and_then(|m| m.slot_faces.get(slot))
                        .filter(|f| f.0 != Vec3::ZERO);
                    let (c, dir) = match face {
                        Some((n, centre)) => {
                            let d = i.transform.transform_vector3(*n).normalize_or_zero();
                            (
                                i.origin + i.transform.transform_point3(*centre).as_dvec3(),
                                d,
                            )
                        }
                        None => (i.world_centre(), Vec3::ZERO),
                    };
                    let dist = (c - camera_pos).length();
                    if dist >= LED_RANGE {
                        return None;
                    }
                    Some((dist, c, g, led, colour, dir))
                })
                .collect()
        };
        panels.sort_by(|a, b| a.0.total_cmp(&b.0));
        for (_, c, gate, led, colour, dir) in panels.into_iter().take(LED_PANELS) {
            let directional = dir != Vec3::ZERO;
            // (without a known facing: the old all-round lamp, pushed out of the vehicle)
            let out = if directional {
                dir.as_dvec3()
            } else {
                vehicles
                    .iter()
                    .min_by(|a, b| {
                        (a.position - c)
                            .length()
                            .total_cmp(&(b.position - c).length())
                    })
                    .filter(|v| (v.position - c).length() < 20.0)
                    .map(|v| {
                        let b =
                            v.ty.def
                                .bounding_box
                                .unwrap_or([2.5, 12.0, 3.0, 0.0, 0.0, 0.0]);
                        let h = v.heading.to_radians();
                        let (fwd, right) = (
                            DVec3::new(h.sin(), h.cos(), 0.0),
                            DVec3::new(h.cos(), -h.sin(), 0.0),
                        );
                        let d = c - v.position;
                        let lx = d.dot(right) - b[3] as f64;
                        let ly = d.dot(fwd) - b[4] as f64;
                        let (ex, ey) = (
                            lx.abs() - (b[0] as f64 * 0.5 - 0.5),
                            ly.abs() - (b[1] as f64 * 0.5 - 0.5),
                        );
                        if ex <= 0.0 && ey <= 0.0 {
                            DVec3::ZERO
                        } else if ex > ey {
                            right * lx.signum()
                        } else {
                            fwd * ly.signum()
                        }
                    })
                    .unwrap_or(DVec3::ZERO)
            };
            let n = night.clamp(0.0, 1.0);
            let (radius, intensity) = if led {
                (LED_RADIUS, LED_INTENSITY * glow * gate * (0.2 + 0.8 * n))
            } else {
                (SCREEN_RADIUS, SCREEN_INTENSITY * gate * n)
            };
            scene.lights.push(PointLight {
                position: c + out * LED_OUTSET,
                radius,
                color: colour,
                intensity,
                direction: if directional { dir } else { Vec3::ZERO },
                // (a lamp of a flat panel: full in front, fading to the panel's plane)
                cone: if directional { SCREEN_CONE } else { [1.0, 0.0] },
                mode: LightMode::Enhanced,
                ..Default::default()
            });
        }
    }
    if ::legacy_config::env::var_os("OMSI_DEBUG_LIGHT").is_some() {
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
    scene.lights.sort_by_cached_key(|l| {
        ((l.position - camera_pos).length_squared() * 16.0) as u64
    });
    let generation = world
        .tiles_generation
        .load(std::sync::atomic::Ordering::Relaxed);
    let seen = seen_world;
    let t_occ = std::time::Instant::now();
    assign_occluders(&coll, &seen, generation, scene, camera_pos, vehicles);
    let total = t_start.elapsed();
    COLLECT_TIMES.with(|c| {
        let mut a = c.get();
        let d = |x: std::time::Instant, y: std::time::Instant| (y - x).as_secs_f64();
        a[0] += d(t_start, t_lamps);
        a[1] += d(t_lamps, t_lamp_loop);
        a[2] += d(t_lamp_loop, t_particles);
        a[3] += d(t_particles, t_vehicles);
        a[4] += d(t_vehicles, t_occ);
        a[5] += t_occ.elapsed().as_secs_f64();
        c.set(a);
    });
    if total.as_millis() > 10 {
        static LAST: std::sync::Mutex<Option<std::time::Instant>> = std::sync::Mutex::new(None);
        let mut last = LAST.lock().unwrap_or_else(|e| e.into_inner());
        if last
            .map(|t| t.elapsed().as_secs_f32() > 2.0)
            .unwrap_or(true)
        {
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
