use super::config::{half_cos, LightSettings};
use super::geom::cell3;
use super::tuning::*;
use glam::Vec3;
use ::render::{Corona, LightMode, PointLight};

fn is_signal(color: [f32; 3]) -> bool {
    let [r, g, b] = color;
    r >= 0.6 && b <= 0.35 && g <= r * 0.85
}

pub(super) fn corona_light(cfg: &LightSettings, c: &Corona, dark: f32) -> Option<PointLight> {
    if c.brightness <= 0.01 || c.beam || c.halo || c.flags & 8 != 0 {
        return None;
    }
    let facing = c.direction.length_squared() > 1e-6;
    let dir = if facing {
        c.direction.normalize()
    } else {
        Vec3::ZERO
    };
    let position = c.position + (dir * SRC_OUTSET).as_dvec3();

    if is_signal(c.color) {
        let (direction, cone) = if facing {
            (dir, [half_cos(120.0), half_cos(180.0)])
        } else {
            (Vec3::ZERO, [1.0, 0.0])
        };
        return Some(PointLight {
            position,
            radius: SIGNAL_RADIUS * (0.7 + 0.3 * c.size.clamp(0.0, 1.0)),
            color: c.color,
            intensity: c.brightness.min(1.0) * SIGNAL_GAIN * (0.5 + 0.5 * dark),
            core: SIGNAL_CORE,
            direction,
            cone,
            mode: LightMode::Both,
            ..Default::default()
        });
    }

    let sc = cfg.src;
    if !sc.on {
        return None;
    }
    let radius = SRC_RADIUS * (0.6 + 0.4 * c.size.clamp(0.0, 1.0)) * c.spread.max(0.05) * sc.spread.max(0.05);
    let (direction, cone) = if sc.directional && facing {
        (dir, [half_cos(sc.inner.min(sc.outer)), half_cos(sc.outer)])
    } else {
        (Vec3::ZERO, [1.0, 0.0])
    };
    Some(PointLight {
        position,
        radius,
        color: c.color,
        intensity: c.brightness.min(1.5) * SRC_GAIN * dark * sc.gain,
        core: (SRC_CORE * sc.core.max(0.01)).min(radius),
        direction,
        cone,
        mode: LightMode::Both,
        ..Default::default()
    })
}

pub(super) fn corona_lights(
    cfg: &LightSettings,
    coronas: &[Corona],
    dark: f32,
    max: usize,
    out: &mut Vec<PointLight>,
) {
    let mut seen: std::collections::HashSet<[i64; 3]> = Default::default();
    let mut found: Vec<PointLight> = coronas
        .iter()
        .filter_map(|c| corona_light(cfg, c, dark))
        .filter(|l| seen.insert(cell3(l.position, 5.0)))
        .collect();
    found.sort_by(|a, b| {
        b.intensity
            .total_cmp(&a.intensity)
            .then(a.position.x.total_cmp(&b.position.x))
            .then(a.position.y.total_cmp(&b.position.y))
            .then(a.position.z.total_cmp(&b.position.z))
    });
    found.truncate(max);
    out.extend(found);
}
