use super::config::{interior_cfg, LightSettings};
use super::fader::Faders;
use super::geom::body_box;
use super::tuning::*;
use glam::{DVec3, Mat4, Vec3};
use ::render::{LightMode, PointLight};
use ::simulation::VehicleInstance;

fn spill_cone(cfg: &LightSettings) -> [f32; 2] {
    let sp = cfg.spill;
    let spread = sp.spread.max(0.05);
    let cos_deg = |deg: f32| (deg * spread).clamp(1.0, 179.0).to_radians().cos();
    [
        cos_deg(SPILL_INNER + sp.inner_add),
        cos_deg(SPILL_OUTER + sp.outer_add),
    ]
}

pub(super) fn interior_spill(
    cfg: &LightSettings,
    faders: &mut Faders,
    v: &VehicleInstance,
    value_of: &dyn Fn(&str) -> f32,
    night: f32,
    lights: &mut Vec<PointLight>,
) {
    let sp = cfg.spill;
    let mut sections: Vec<(&::model::Model, Option<[f32; 6]>, Mat4, DVec3, usize)> = vec![(
        &v.ty.model,
        body_box(&v.ty),
        v.body_rotation(),
        v.position,
        super::owner_key(v),
    )];
    for t in &v.trailers {
        sections.push((
            &t.ty.model,
            body_box(&t.ty),
            t.body_rotation(),
            t.position,
            super::owner_key(t),
        ));
    }

    let tilt = (SPILL_TILT + sp.tilt_add).to_radians();
    let cone = spill_cone(cfg);
    let radius = sp.radius();
    let strength = night.clamp(0.0, 1.0) * sp.gain;
    let sides = [Vec3::X, -Vec3::X];

    for (model, bbox, rot, origin, owner) in sections {
        for (li, il) in model.interior_lights.iter().enumerate() {
            let ic = interior_cfg(li);
            if ic.off {
                continue;
            }
            let target = if value_of(&il.variable) >= 0.5 { 1.0 } else { 0.0 };
            let level = faders.level(owner, SLOT_SALOON + li as u32, target, SALOON_RISE, SALOON_FALL);
            if level < 0.01 {
                continue;
            }
            let at = Vec3::from(il.pos) + Vec3::from(ic.shift);
            if let Some(b) = bbox {
                if (at.x - b[3]).abs() >= b[0] * 0.5 - 0.3 || (at.y - b[4]).abs() >= b[1] * 0.5 - 0.1 {
                    continue;
                }
            }
            let col = Vec3::from(il.color) / 255.0 * Vec3::from(ic.color);
            let color = col
                .lerp(Vec3::splat(col.max_element()), SPILL_WHITE)
                .to_array();
            let gain = SPILL_LAMP * ic.gain * strength * level / sides.len() as f32;
            let position = origin + rot.transform_point3(at).as_dvec3();
            for out in sides {
                let dir = (out * tilt.cos() - Vec3::Z * tilt.sin()).normalize();
                lights.push(PointLight {
                    position,
                    radius,
                    color,
                    intensity: gain,
                    direction: rot.transform_vector3(dir).normalize_or_zero(),
                    cone,
                    core: SPILL_CORE * sp.core.max(0.01),
                    mode: LightMode::Enhanced,
                    shadow_first: true,
                    ..Default::default()
                });
            }
        }
    }
}

pub(super) fn spill_vehicles(
    cfg: &LightSettings,
    camera: DVec3,
    vehicles: &[&VehicleInstance],
) -> Vec<bool> {
    static DISABLED: std::sync::OnceLock<bool> = std::sync::OnceLock::new();
    let sp = cfg.spill;
    let off = *DISABLED.get_or_init(|| ::legacy_config::env::var_os("OMSI_NO_SPILL").is_some()) || !sp.on;
    let mut ok = vec![false; vehicles.len()];
    if off {
        return ok;
    }
    let mut order: Vec<(f64, usize)> = vehicles
        .iter()
        .enumerate()
        .map(|(i, v)| {
            let half = body_box(&v.ty).map_or(0.0, |b| b[1] as f64 * 0.5);
            (((v.position - camera).length() - half).max(0.0), i)
        })
        .filter(|(d, _)| *d < sp.reach as f64)
        .collect();
    order.sort_by(|a, b| a.0.total_cmp(&b.0));
    for (_, i) in order.into_iter().take(sp.vehicles.max(0) as usize) {
        ok[i] = true;
    }
    ok
}
