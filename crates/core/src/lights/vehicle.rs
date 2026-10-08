use super::*;

#[derive(Default)]
struct CookieStore {
    known: std::collections::HashMap<(std::path::PathBuf, String), Option<u8>>,
    by_path: std::collections::HashMap<std::path::PathBuf, u8>,
    images: Vec<::render::CookieTexture>,
    failed: std::collections::HashSet<std::path::PathBuf>,
    warned: std::collections::HashSet<String>,
}

static COOKIE_STORE: std::sync::OnceLock<std::sync::Mutex<CookieStore>> =
    std::sync::OnceLock::new();

fn cookie_slot(ty: &::simulation::VehicleType, name: &str) -> Option<u8> {
    if name.trim().is_empty() || ::legacy_config::env::var_os("OMSI_NO_COOKIES").is_some() {
        return None;
    }
    let key = (ty.model_dir.clone(), name.trim().to_string());
    let mut store = COOKIE_STORE
        .get_or_init(Default::default)
        .lock()
        .unwrap_or_else(|e| e.into_inner());
    if let Some(slot) = store.known.get(&key) {
        return *slot;
    }
    let texture_dir = ::legacy_config::resolve_path(&ty.model_dir, "Texture");
    let mut dirs = vec![texture_dir, ty.model_dir.clone(), ty.def.dir().to_path_buf()];
    dirs.extend(
        ::legacy_config::content_roots()
            .into_iter()
            .map(|root| root.join("Texture")),
    );
    let dir_refs: Vec<&std::path::Path> = dirs.iter().map(|dir| dir.as_path()).collect();
    let Some(path) = ::texture::find_texture(name, &dir_refs) else {
        if store
            .warned
            .insert(format!("{}:{name}", ty.model_dir.display()))
        {
            log::warn!("beam cookie {name:?} is missing under {}", ty.model_dir.display());
        }
        store.known.insert(key, None);
        return None;
    };
    if let Some(slot) = store.by_path.get(&path) {
        let slot = *slot;
        store.known.insert(key, Some(slot));
        return Some(slot);
    }
    if store.failed.contains(&path) {
        store.known.insert(key, None);
        return None;
    }
    let image = match ::texture::decode_file(&path) {
        Ok(image) => image,
        Err(error) => {
            store.failed.insert(path.clone());
            if store.warned.insert(path.to_string_lossy().to_string()) {
                log::warn!("beam cookie {} could not be loaded: {error}", path.display());
            }
            store.known.insert(key, None);
            return None;
        }
    };
    if store.images.len() >= ::render::COOKIE_SLOTS {
        if store.warned.insert("capacity".to_string()) {
            log::warn!("beam cookies support {} images at a time", ::render::COOKIE_SLOTS);
        }
        store.known.insert(key, None);
        return None;
    }
    let slot = store.images.len() as u8 + 1;
    store.images.push(::render::CookieTexture {
        slot,
        generation: 1,
        image: std::sync::Arc::new(image),
    });
    store.by_path.insert(path, slot);
    store.known.insert(key, Some(slot));
    Some(slot)
}

pub(crate) fn cookie_textures() -> Vec<::render::CookieTexture> {
    COOKIE_STORE
        .get_or_init(Default::default)
        .lock()
        .unwrap_or_else(|e| e.into_inner())
        .images
        .clone()
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
    let own_rot = v.body_rotation();
    for (mut c, owner) in crate::scene::model_lights_owned(
        &ty.model,
        &mesh_xf,
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
        c.spread = ec.spread.max(0.05);
        c.color = [
            c.color[0] * ec.color[0],
            c.color[1] * ec.color[1],
            c.color[2] * ec.color[2],
        ];
        c.position += own_rot.transform_vector3(Vec3::from(ec.shift)).as_dvec3();
        coronas.push(c);
    }
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
    let forced = ::legacy_config::env::var("OMSI_SPOT_SELECT")
        .ok()
        .and_then(|s| s.trim().parse::<f32>().ok());
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
                let d = body
                    .transform_vector3(Vec3::new(vals[3], vals[4], vals[5]))
                    .normalize_or_zero();
                let high_beam =
                    cfg.force_high_beam || v.var("lights_fern").is_some_and(|x| x > 0.5);
                let bc = if high_beam { cfg.high } else { cfg.low };
                let color = [
                    vals[6] / 255.0 * cfg.lamp_color[0] * bc.color[0],
                    vals[7] / 255.0 * cfg.lamp_color[1] * bc.color[1],
                    vals[8] / 255.0 * cfg.lamp_color[2] * bc.color[2],
                ];
                let mut apex = Vec3::new(vals[0], vals[1], vals[2]);
                let dl = Vec3::new(vals[3], vals[4], vals[5]).normalize_or_zero();
                let nose = lamps.iter().map(|l| l[1]).reduce(f32::max);
                let tail = lamps.iter().map(|l| l[1]).reduce(f32::min);
                let bb = ty
                    .def
                    .bounding_box
                    .map(|bb| (bb[4] + bb[1] * 0.5, bb[4] - bb[1] * 0.5));
                let dir = if dl.y > 0.3 {
                    1.0
                } else if dl.y < -0.3 {
                    -1.0
                } else {
                    0.0
                };
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
                let half_width = ty
                    .def
                    .bounding_box
                    .map_or(1.25, |bb| (bb[0] * 0.5).min(1.25));
                let on_face: Vec<f32> = lamps
                    .iter()
                    .filter(|l| (dl.y > 0.3 || dl.y < -0.3) && (l[1] - apex.y).abs() < 0.35)
                    .map(|l| (l[0] - apex.x).abs())
                    .collect();
                let spread =
                    (on_face.iter().sum::<f32>() / on_face.len().max(1) as f32).min(half_width);
                let right = body.transform_vector3(Vec3::X).normalize_or_zero();
                let apex = body.transform_point3(apex);
                let (inner, outer) = (
                    vals.get(10).copied().unwrap_or(30.0) + cfg.lamp_inner_add + bc.inner_add,
                    vals.get(11).copied().unwrap_or(70.0) + cfg.lamp_outer_add + bc.outer_add,
                );
                let half = |deg: f32| (deg.clamp(1.0, 179.0) * 0.5).to_radians().cos();
                let cone = [half(inner.min(outer)), half(outer)];
                let sides: &[f32] = if spread > 0.1 { &[-1.0, 1.0] } else { &[0.0] };
                for side in sides {
                    if !bc.on {
                        continue;
                    }
                    let at = v.position
                        + (apex + right * spread * cfg.lamp_spread * side).as_dvec3()
                        + lamp_shift(d, &cfg)
                        + shift(d, bc.forward, bc.side, bc.height);
                    let radius = headlight_radius(vals[9])
                        * cfg.lamp_range.max(0.05)
                        * bc.range.max(0.05)
                        * if high_beam { cfg.high_beam_range } else { 1.0 };
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
                        direction: lamp_aim(d, &cfg, &bc),
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
                            * if high_beam { cfg.high_beam } else { 1.0 }
                            * bc.gain,
                        core: headlight_core(radius) * cfg.lamp_core.max(0.01) * bc.core.max(0.01),
                        beam: if high_beam { -1.0 } else { cfg.low_beam_gain },
                        mode: LightMode::Enhanced,
                        ..lamp
                    });
                }
            }
        }
    }
    spotlights_2(&ty.model, body, v.position, &value_of, night, lights);
    cookie_spotlights(
        ty,
        body,
        v.position,
        &value_of,
        &v.cookie_fade,
        lights,
    );
    for t in &v.trailers {
        spotlights_2(&t.ty.model, t.body_rotation(), t.position, &value_of, night, lights);
        cookie_spotlights(
            &t.ty,
            t.body_rotation(),
            t.position,
            &value_of,
            &t.cookie_fade,
            lights,
        );
    }
    if spill && cfg.spill.on && night > 0.05 {
        let mut sections: Vec<(&::model::Model, Option<[f32; 6]>, glam::Mat4, DVec3)> =
            vec![(&ty.model, body_box(ty), body, v.position)];
        for t in &v.trailers {
            sections.push((&t.ty.model, body_box(&t.ty), t.body_rotation(), t.position));
        }
        let sp = cfg.spill;
        let tilt = (INTERIOR_SPILL_TILT + sp.tilt_add).to_radians();
        let cone = [
            ((INTERIOR_SPILL_INNER + sp.inner_add) * sp.spread.max(0.05))
                .clamp(1.0, 179.0)
                .to_radians()
                .cos(),
            ((INTERIOR_SPILL_OUTER + sp.outer_add) * sp.spread.max(0.05))
                .clamp(1.0, 179.0)
                .to_radians()
                .cos(),
        ];
        let spill_r = spill_radius(&sp);
        for (model, _bb, xf, origin) in sections {
            let lit: Vec<usize> = (0..model.interior_lights.len())
                .filter(|&li| {
                    let il = &model.interior_lights[li];
                    value_of(&il.variable) >= 0.5 && !interior_cfg(li).off
                })
                .collect();
            if lit.is_empty() {
                continue;
            }
            let strength = night.clamp(0.0, 1.0) * sp.gain;
            for li in lit {
                let il = &model.interior_lights[li];
                let ic = interior_cfg(li);
                let at = Vec3::from(il.pos) + Vec3::from(ic.shift);
                let col = Vec3::from(il.color) / 255.0 * Vec3::from(ic.color);
                let color = (col * (1.0 - INTERIOR_SPILL_WHITE)
                    + Vec3::splat(col.max_element()) * INTERIOR_SPILL_WHITE)
                    .to_array();
                let sides = [Vec3::X, -Vec3::X];
                let gain = INTERIOR_SPILL_LAMP * ic.gain * strength / sides.len() as f32;
                for out in sides {
                    let dir = (out * tilt.cos() - Vec3::Z * tilt.sin()).normalize();
                    lights.push(PointLight {
                        position: origin + xf.transform_point3(at).as_dvec3(),
                        radius: spill_r,
                        color,
                        intensity: gain,
                        direction: xf.transform_vector3(dir).normalize_or_zero(),
                        cone,
                        core: INTERIOR_SPILL_CORE * sp.core.max(0.01),
                        mode: LightMode::Enhanced,
                        shadow_first: true,
                        ..Default::default()
                    });
                }
            }
        }
    }
}

fn cookie_spotlights(
    ty: &::simulation::VehicleType,
    rot: glam::Mat4,
    origin: DVec3,
    value_of: &impl Fn(&str) -> f32,
    fades: &[f32],
    lights: &mut Vec<PointLight>,
) {
    let up = rot.transform_vector3(Vec3::Z).normalize_or_zero();
    let right = rot.transform_vector3(Vec3::X).normalize_or_zero();
    for (i, sp) in ty.model.spotlights_cookie.iter().enumerate() {
        let level = fades
            .get(i)
            .copied()
            .unwrap_or_else(|| value_of(&sp.variable).clamp(0.0, 1.0))
            .clamp(0.0, 1.0);
        let finite_params = sp
            .position
            .iter()
            .chain(sp.direction.iter())
            .all(|v| v.is_finite());
        if !level.is_finite()
            || level <= 0.001
            || !sp.range.is_finite()
            || sp.range <= 0.0
            || Vec3::from(sp.direction).length_squared() <= 1e-6
            || !finite_params
        {
            continue;
        }
        let slot = cookie_slot(ty, &sp.texture).unwrap_or(0);
        let h = finite_value(value_of(&sp.h_offset)).to_radians();
        let v = finite_value(value_of(&sp.v_offset)).to_radians();
        let (sin_h, cos_h) = (-h).sin_cos();
        let (sin_v, cos_v) = v.sin_cos();
        let mirrors: &[f32] = if sp.mirrored { &[-1.0, 1.0] } else { &[0.0] };
        for mirror in mirrors {
            let local = Vec3::new(
                if *mirror == 0.0 { sp.position[0] } else { sp.position[0] * *mirror },
                sp.position[1],
                sp.position[2],
            );
            let local_dir = Vec3::new(
                if *mirror == 0.0 { sp.direction[0] } else { sp.direction[0] * *mirror },
                sp.direction[1],
                sp.direction[2],
            )
            .normalize_or_zero();
            let yawed = Vec3::new(
                local_dir.x * cos_h - local_dir.y * sin_h,
                local_dir.x * sin_h + local_dir.y * cos_h,
                local_dir.z,
            );
            let pitch_axis = Vec3::new(right.x, right.y, right.z);
            let dir0 = rot.transform_vector3(yawed).normalize_or_zero();
            let axis = pitch_axis;
            let dir = (dir0 * cos_v
                + axis.cross(dir0) * sin_v
                + axis * axis.dot(dir0) * (1.0 - cos_v))
            .normalize_or_zero();
            let at = origin + rot.transform_point3(local).as_dvec3();
            let (color, cone, cookie) = if slot == 0 {
                (
                    [1.0, 1.0, 233.0 / 255.0],
                    [15.0f32.to_radians().cos(), 50.0f32.to_radians().cos()],
                    0,
                )
            } else {
                ([1.0; 3], [1.0, 0.0], slot)
            };
            lights.push(PointLight {
                position: at,
                radius: sp.range,
                color,
                intensity: level,
                direction: dir,
                cone,
                core: (sp.range * 0.125).max(0.01),
                mode: LightMode::Enhanced,
                cookie,
                cookie_up: up,
                ..Default::default()
            });
        }
    }
}

fn finite_value(value: f32) -> f32 {
    if value.is_finite() {
        value
    } else {
        0.0
    }
}
