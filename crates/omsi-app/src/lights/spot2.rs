use super::*;

pub fn spotlights_2(
    model: &omsi_model::Model,
    xf: glam::Mat4,
    origin: DVec3,
    value_of: &dyn Fn(&str) -> f32,
    night: f32,
    lights: &mut Vec<PointLight>,
) {
    if model.spotlights_2.is_empty() {
        return;
    }
    let cfg = settings();
    let bc = cfg.low;
    let sc = cfg.spot2;
    if !bc.on || !sc.on {
        return;
    }
    let bad = weather_darkness();
    let half = |deg: f32| (deg.clamp(1.0, 179.0) * 0.5).to_radians().cos();
    for sp in &model.spotlights_2 {
        if value_of(&sp.variable) < 0.5 {
            continue;
        }
        let vals = sp.values;
        let color = [
            vals[6] / 255.0 * cfg.lamp_color[0] * bc.color[0],
            vals[7] / 255.0 * cfg.lamp_color[1] * bc.color[1],
            vals[8] / 255.0 * cfg.lamp_color[2] * bc.color[2],
        ];
        let (inner, outer) = (
            vals[10] + cfg.lamp_inner_add + bc.inner_add + sc.inner_add,
            vals[11] + cfg.lamp_outer_add + bc.outer_add + sc.outer_add,
        );
        let cone = [half(inner.min(outer)), half(outer)];
        let radius = headlight_radius(vals[9]) * cfg.lamp_range.max(0.05) * bc.range.max(0.05) * sc.range.max(0.05);
        let mirrored = !sp.no_mirror && vals[0].abs() > 0.01;
        let sides: &[f32] = if mirrored { &[1.0, -1.0] } else { &[1.0] };
        for side in sides {
            let local_pos = Vec3::new(vals[0] * side, vals[1], vals[2]);
            let local_dir = Vec3::new(vals[3] * side, vals[4], vals[5]);
            let d = xf.transform_vector3(local_dir).normalize_or_zero();
            let at = origin
                + xf.transform_point3(local_pos).as_dvec3()
                + DVec3::Z * sc.height as f64
                + lamp_shift(d, &cfg)
                + shift(d, bc.forward, bc.side, bc.height);
            let lamp = PointLight {
                position: at,
                radius,
                color,
                direction: lamp_aim(d, &cfg, &bc),
                cone,
                ..Default::default()
            };
            lights.push(PointLight {
                intensity: cfg.vanilla * (0.3 + 0.7 * night) * sc.gain,
                mode: LightMode::Vanilla,
                ..lamp
            });
            lights.push(PointLight {
                intensity: cfg.headlight * (1.0 + bad * cfg.weather_boost) * bc.gain * sc.gain,
                core: headlight_core(radius)
                    * cfg.lamp_core.max(0.01)
                    * bc.core.max(0.01)
                    * sc.core.max(0.01),
                beam: cfg.low_beam_gain,
                mode: LightMode::Enhanced,
                ..lamp
            });
        }
    }
}
