use super::*;

#[derive(Clone, Copy, PartialEq, Eq)]
enum Role {
    Fog,
    Driving,
    Cornering,
    Low,
}

fn role_of(range: f32, outer_deg: f32) -> Role {
    if range >= 80.0 {
        Role::Driving
    } else if range < 15.0 {
        Role::Cornering
    } else if outer_deg >= 70.0 {
        Role::Fog
    } else {
        Role::Low
    }
}

pub fn spotlights_2(
    model: &::model::Model,
    xf: glam::Mat4,
    origin: DVec3,
    key: usize,
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
    // (the lamp positions are walked once, not per lamp and side)
    let nose_front = model
        .meshes
        .iter()
        .flat_map(|m| {
            m.light_enh
                .iter()
                .map(|l| l.pos[1])
                .chain(m.light_enh_2.iter().map(|l| l.pos[1]))
        })
        .fold(None, |a: Option<(f32, f32)>, y| {
            Some(a.map_or((y, y), |(hi, lo)| (hi.max(y), lo.min(y))))
        });
    let nose_of = |dir_y: f32| -> Option<f32> {
        nose_front.map(|(hi, lo)| if dir_y > 0.0 { hi } else { lo })
    };
    let half = |deg: f32| (deg.clamp(1.0, 179.0) * 0.5).to_radians().cos();
    for (si, sp) in model.spotlights_2.iter().enumerate() {
        let target = if value_of(&sp.variable) >= 0.5 { 1.0 } else { 0.0 };
        let level = lamp_level(key, SLOT_SPOT2 + si as u32, target, LAMP_RISE, LAMP_FALL);
        if level < 0.01 {
            continue;
        }
        let vals = sp.values;
        let color = [
            vals[6] / 255.0 * cfg.lamp_color[0] * bc.color[0],
            vals[7] / 255.0 * cfg.lamp_color[1] * bc.color[1],
            vals[8] / 255.0 * cfg.lamp_color[2] * bc.color[2],
        ];
        let (inner, outer) = (vals[10], vals[11]);
        let range = vals[9] * bc.range.max(0.05) * sc.range.max(0.05);
        let role = role_of(range, inner.max(outer));

        let cone = {
            let o = outer.max(inner);
            if vals[4] < -0.3 && role == Role::Driving {
                [half(inner.min(outer)), half(o)]
            } else {
                [half(inner.min(outer).min(o * 0.3)), half(o)]
            }
        };
        let (factor, beam, radius) = match role {
            Role::Fog => (0.6, cfg.low_beam_gain, range.clamp(6.0, 45.0)),
            Role::Driving => (
                1.0,
                beam::beam_code(beam::BeamKind::Main, range),
                spot_reach(range, 60.0),
            ),
            Role::Cornering => (0.35 * short_range_gain(range), 0.0, headlight_radius(range)),
            Role::Low => (1.0, cfg.low_beam_gain, headlight_radius(range)),
        };
        let mirrored = !sp.no_mirror && vals[0].abs() > 0.01;
        let sides: &[f32] = if mirrored { &[1.0, -1.0] } else { &[1.0] };
        for side in sides {
            let mut local_pos = Vec3::new(vals[0] * side, vals[1], vals[2]);
            let mut local_dir =
                Vec3::new(vals[3] * side, vals[4], vals[5]).normalize_or_zero();
            let dir_y = if local_dir.y > 0.3 {
                1.0
            } else if local_dir.y < -0.3 {
                -1.0
            } else {
                0.0
            };
            if dir_y != 0.0 {
                if let Some(face) = spot_face(nose_of(dir_y), None, local_pos.y, dir_y) {
                    local_pos.y = face + dir_y * 0.05;
                }
                let std_drop = beam::aim_drop(local_pos.z);
                let drop = match role {
                    Role::Fog => beam::FOG_DROP.max(std_drop),
                    Role::Driving | Role::Low => std_drop,
                    Role::Cornering => 0.0,
                };
                local_dir = beam::aimed(local_dir, drop);
            } else {
                // (a lamp never shines up into the sky)
                local_dir.z = local_dir.z.min(0.0);
                local_dir = local_dir.normalize_or_zero();
            }
            let d = xf.transform_vector3(local_dir).normalize_or_zero();
            let at = origin
                + xf.transform_point3(local_pos).as_dvec3()
                + DVec3::Z * sc.height as f64;
            let lamp = PointLight {
                position: at,
                radius,
                color,
                direction: {
                    let (mut c2, mut b2) = (cfg, bc);
                    c2.lamp_yaw *= side;
                    b2.yaw *= side;
                    lamp_aim(d, &c2, &b2)
                },
                cone,
                ..Default::default()
            };
            lights.push(PointLight {
                intensity: cfg.vanilla * (0.3 + 0.7 * night) * sc.gain * level,
                mode: LightMode::Vanilla,
                ..lamp
            });
            lights.push(PointLight {
                intensity: cfg.headlight * factor * bc.gain * sc.gain * level,
                core: headlight_core(radius) * bc.core.max(0.01) * sc.core.max(0.01),
                beam,
                mode: LightMode::Enhanced,
                ..lamp
            });
        }
    }
}
