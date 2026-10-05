use super::*;

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
    coronas.extend(crate::scene::model_lights_faded(
        &ty.model,
        &mesh_xf,
        v.position,
        &value_of,
        &v.light_fade,
    ));
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
    let forced = omsi_cfg::env::var("OMSI_SPOT_SELECT")
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
    if spill && night > 0.05 {
        let mut sections: Vec<(&omsi_model::Model, Option<[f32; 6]>, glam::Mat4, DVec3)> =
            vec![(&ty.model, body_box(ty), body, v.position)];
        for t in &v.trailers {
            sections.push((&t.ty.model, body_box(&t.ty), t.body_rotation(), t.position));
        }
        let tilt = INTERIOR_SPILL_TILT.to_radians();
        let cone = [
            INTERIOR_SPILL_INNER.to_radians().cos(),
            INTERIOR_SPILL_OUTER.to_radians().cos(),
        ];
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
            let color = (color * (1.0 - INTERIOR_SPILL_WHITE)
                + Vec3::splat(color.max_element()) * INTERIOR_SPILL_WHITE)
                .to_array();
            let (half_w, half_l, cx, cy) = match bb {
                Some(b) => (
                    b[0] * 0.5 + INTERIOR_SPILL_OUTSET,
                    b[1] * 0.5 + INTERIOR_SPILL_OUTSET,
                    b[3],
                    b[4],
                ),
                None => (1.25 - INTERIOR_SPILL_INSET, 4.0, 0.0, c.y),
            };
            let strength = (count.min(INTERIOR_SPILL_MAX) as f32 / INTERIOR_SPILL_MAX as f32)
                .max(0.25)
                * night.clamp(0.0, 1.0);
            let mut faces = vec![
                (
                    Vec3::new(c.x, cy + half_l, c.z),
                    Vec3::Y,
                    INTERIOR_SPILL_END,
                ),
                (
                    Vec3::new(c.x, cy - half_l, c.z),
                    -Vec3::Y,
                    INTERIOR_SPILL_END,
                ),
            ];
            for k in 0..INTERIOR_SPILL_ALONG {
                let y = cy
                    + half_l * 0.75 * (2.0 * (k as f32 + 0.5) / INTERIOR_SPILL_ALONG as f32 - 1.0);
                faces.push((Vec3::new(cx + half_w, y, c.z), Vec3::X, INTERIOR_SPILL_SIDE));
                faces.push((
                    Vec3::new(cx - half_w, y, c.z),
                    -Vec3::X,
                    INTERIOR_SPILL_SIDE,
                ));
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
