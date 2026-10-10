use super::beam::*;
use super::config::{half_cos, LightSettings};
use super::fader::Faders;
use super::tuning::*;
use glam::{DVec3, Mat4, Vec3};
use ::render::{LightMode, PointLight};

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

pub(super) struct Section<'a> {
    pub model: &'a ::model::Model,
    pub rot: Mat4,
    pub origin: DVec3,
    pub owner: usize,
}

pub(super) fn spotlights_2(
    cfg: &LightSettings,
    faders: &mut Faders,
    s: &Section,
    value_of: &dyn Fn(&str) -> f32,
    night: f32,
    lights: &mut Vec<PointLight>,
) {
    let model = s.model;
    if model.spotlights_2.is_empty() || !cfg.low.on || !cfg.spot2.on {
        return;
    }
    let (bc, sc) = (cfg.low, cfg.spot2);

    let extent = model
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
    let nose_of = |dir_y: f32| extent.map(|(hi, lo)| if dir_y > 0.0 { hi } else { lo });

    for (si, sp) in model.spotlights_2.iter().enumerate() {
        let target = if value_of(&sp.variable) >= 0.5 { 1.0 } else { 0.0 };
        let level = faders.level(s.owner, SLOT_SPOT2 + si as u32, target, LAMP_RISE, LAMP_FALL);
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

        let widest = outer.max(inner);
        let narrow = inner.min(outer);
        let cone = if vals[4] < -0.3 && role == Role::Driving {
            [half_cos(narrow), half_cos(widest)]
        } else {
            [half_cos(narrow.min(widest * 0.3)), half_cos(widest)]
        };
        let (factor, beam, radius) = match role {
            Role::Fog => (0.6, cfg.low_beam_gain, range.clamp(6.0, 45.0)),
            Role::Driving => (1.0, main_beam_code(range), spot_reach(range, 60.0)),
            Role::Cornering => (0.35 * short_range_gain(range), 0.0, headlight_radius(range)),
            Role::Low => (1.0, cfg.low_beam_gain, headlight_radius(range)),
        };

        let mirrored = !sp.no_mirror && vals[0].abs() > 0.01;
        let sides: &[f32] = if mirrored { &[1.0, -1.0] } else { &[1.0] };
        for &side in sides {
            let mut pos = Vec3::new(vals[0] * side, vals[1], vals[2]);
            let mut dir = Vec3::new(vals[3] * side, vals[4], vals[5]).normalize_or_zero();
            let dir_y = facing(dir);
            if dir_y != 0.0 {
                if let Some(face) = spot_face(nose_of(dir_y), None, pos.y, dir_y) {
                    pos.y = face + dir_y * 0.05;
                }
                let std_drop = aim_drop(pos.z);
                let drop = match role {
                    Role::Fog => FOG_DROP.max(std_drop),
                    Role::Driving | Role::Low => std_drop,
                    Role::Cornering => 0.0,
                };
                dir = aimed(dir, drop);
            } else {
                dir.z = dir.z.min(0.0);
                dir = dir.normalize_or_zero();
            }
            let world_dir = s.rot.transform_vector3(dir).normalize_or_zero();
            let lamp = PointLight {
                position: s.origin
                    + s.rot.transform_point3(pos).as_dvec3()
                    + DVec3::Z * sc.height as f64,
                radius,
                color,
                direction: cfg.aim(world_dir, &bc, side),
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn roles_follow_range_and_cone() {
        assert!(role_of(100.0, 40.0) == Role::Driving);
        assert!(role_of(10.0, 40.0) == Role::Cornering);
        assert!(role_of(30.0, 90.0) == Role::Fog);
        assert!(role_of(30.0, 40.0) == Role::Low);
    }
}
