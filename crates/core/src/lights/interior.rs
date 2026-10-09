use super::*;

pub(super) fn interior_spill(
    v: &VehicleInstance,
    value_of: &dyn Fn(&str) -> f32,
    night: f32,
    lights: &mut Vec<PointLight>,
) {
    let cfg = settings();
    let sp = cfg.spill;
    let mut sections: Vec<(&::model::Model, Option<[f32; 6]>, glam::Mat4, DVec3, usize)> = vec![(
        &v.ty.model,
        body_box(&v.ty),
        v.body_rotation(),
        v.position,
        key_of(v),
    )];
    for t in &v.trailers {
        sections.push((
            &t.ty.model,
            body_box(&t.ty),
            t.body_rotation(),
            t.position,
            key_of(t),
        ));
    }
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
    let strength = night.clamp(0.0, 1.0) * sp.gain;
    for (model, bb, xf, origin, key) in sections {
        for (li, il) in model.interior_lights.iter().enumerate() {
            let ic = interior_cfg(li);
            if ic.off {
                continue;
            }
            let target = if value_of(&il.variable) >= 0.5 { 1.0 } else { 0.0 };
            let level = lamp_level(key, SLOT_SALOON + li as u32, target, SALOON_RISE, SALOON_FALL);
            if level < 0.01 {
                continue;
            }
            let at = Vec3::from(il.pos) + Vec3::from(ic.shift);
            let col = Vec3::from(il.color) / 255.0 * Vec3::from(ic.color);
            let color = (col * (1.0 - INTERIOR_SPILL_WHITE)
                + Vec3::splat(col.max_element()) * INTERIOR_SPILL_WHITE)
                .to_array();

            if let Some(b) = bb {
                if (at.x - b[3]).abs() >= b[0] * 0.5 - 0.3
                    || (at.y - b[4]).abs() >= b[1] * 0.5 - 0.1
                {
                    continue;
                }
            }
            let sides = [Vec3::X, -Vec3::X];
            let gain = INTERIOR_SPILL_LAMP * ic.gain * strength * level / sides.len() as f32;
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
