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
        // OMSI reserves the first two classic `[spotlight]` entries for dipped and full beam.
        // In particular, the 400MMC uses entry 3 for its DRLs.  A daytime-running light is a
        // visible lamp, not a headlight that should illuminate the road.
        if (0.0..2.0).contains(&sel) {
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
                    // A classic `[spotlight]` often names only one centre point.  Prefer the
                    // actual left/right lamp positions from `[light_enh_2]` when the model
                    // exposes them, so each cookie begins at its physical dipped/full beam.
                    // Older buses keep the former, inferred placement as a fallback.
                    let sources = classic_beam_sources(&ty.model, sel as usize);
                    let sources = if sources.is_empty() {
                        let half_width = ty
                            .def
                            .bounding_box
                            .map_or(1.25, |bb| (bb[0] * 0.5).min(1.25));
                        let on_face: Vec<f32> = lamps
                            .iter()
                            .filter(|l| dir_y != 0.0 && (l[1] - apex.y).abs() < 0.35)
                            .map(|l| (l[0] - vals[0]).abs())
                            .collect();
                        let spread = (on_face.iter().sum::<f32>()
                            / on_face.len().max(1) as f32)
                            .min(half_width);
                        let sides: &[f32] = if spread > 0.1 { &[-1.0, 1.0] } else { &[0.0] };
                        sides
                            .iter()
                            .map(|side| apex + Vec3::X * spread * *side)
                            .collect()
                    } else {
                        sources
                    };
                    let half = |deg: f32| (deg.clamp(1.0, 179.0) * 0.5).to_radians().cos();
                    let (inner, outer) = (vals[10], vals[11]);
                    let cone = [half(inner.min(outer)), half(outer.max(inner))];
                    let reach_v = spot_reach(vals[9], 45.0);
                    let radius = spot_reach(vals[9], 60.0);
                    let short = short_range_gain(vals[9]);
                    let source_count = sources.len() as f32;
                    for source in sources {
                        let at = v.position + body.transform_point3(source).as_dvec3();
                        let lamp = PointLight {
                            position: at,
                            color,
                            direction: d,
                            cone,
                            ..Default::default()
                        };
                        lights.push(PointLight {
                            radius: reach_v,
                            intensity: cfg.vanilla / source_count
                                * (0.3 + 0.7 * night)
                                * short,
                            mode: LightMode::Vanilla,
                            ..lamp
                        });
                        lights.push(PointLight {
                            radius,
                            intensity: cfg.headlight / source_count
                                * (1.0 + bad * cfg.weather_boost)
                                * short,
                            core: (radius * 0.25).clamp(0.1, 1.0),
                            // 100 selects the dipped-beam cookie; below 0 is a full beam.
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
    spotlights_2(&ty.model, body, v.position, &value_of, night, lights);
    for t in &v.trailers {
        spotlights_2(&t.ty.model, t.body_rotation(), t.position, &value_of, night, lights);
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
        for (model, bb, xf, origin) in sections {
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
                // only a lamp inside the saloon is a window light: one in or outside the body's
                // walls (a door step light, an outside lamp) is no saloon source at all
                if let Some(b) = bb {
                    if (at.x - b[3]).abs() >= b[0] * 0.5 - 0.3
                        || (at.y - b[4]).abs() >= b[1] * 0.5 - 0.1
                    {
                        continue;
                    }
                }
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

/// Physical left/right lenses for classic OMSI dipped/full beams.  Their `[spotlight]` section
/// is commonly centred between the lamps, while `[light_enh_2]` records the actual lenses.
fn classic_beam_sources(model: &::model::Model, selected: usize) -> Vec<Vec3> {
    let needle = match selected {
        0 => "mainbeam",
        1 => "highbeam",
        _ => return Vec::new(),
    };
    let mut sources = Vec::new();
    for light in model.meshes.iter().flat_map(|mesh| mesh.light_enh_2.iter()) {
        if !light.variable.trim().to_ascii_lowercase().contains(needle) {
            continue;
        }
        let pos = Vec3::from(light.pos);
        if !sources
            .iter()
            .any(|known: &Vec3| known.distance_squared(pos) < 0.0001)
        {
            sources.push(pos);
        }
    }
    sources
}
