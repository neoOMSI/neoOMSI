use super::*;

pub(super) fn weather_darkness() -> f32 {
    let (vis, _) = cone_weather();
    (1.0 - vis / 3000.0).clamp(0.0, 1.0)
}

pub(super) fn headlight_radius(range: f32) -> f32 {
    range.max(6.0)
}

pub(super) fn headlight_core(range: f32) -> f32 {
    headlight_radius(range) / 30.0
}

pub(super) fn ai_spotlight(lamps: &[[f32; 3]]) -> Option<[f32; 12]> {
    let nose = lamps.iter().map(|l| l[1]).reduce(f32::max)?;
    let front: Vec<&[f32; 3]> = lamps.iter().filter(|l| nose - l[1] < 0.4).collect();
    let n = front.len() as f32;
    let z = front.iter().map(|l| l[2]).sum::<f32>() / n;
    Some([
        0.0, nose, z, 0.0, 1.0, -0.05, 255.0, 245.0, 225.0, 40.0, 30.0, 70.0,
    ])
}

pub(super) fn spot_face(
    lamp: Option<f32>,
    edge: Option<f32>,
    apex_y: f32,
    dir: f32,
) -> Option<f32> {
    let fwd = |y: f32| y * dir;
    let lamp = lamp.filter(|l| fwd(*l) > fwd(apex_y));
    let face = match (lamp, edge) {
        (Some(l), Some(e)) => fwd(l).min(fwd(e)),
        (Some(l), None) => fwd(l),
        (None, Some(e)) => fwd(e).min(fwd(apex_y) + 1.5),
        (None, None) => return None,
    };
    Some(face * dir).filter(|f| fwd(*f) > fwd(apex_y))
}

pub fn lighting_from(d: &Daylight, fog_range: f32) -> Lighting {
    let density = (2.3 / fog_range.max(50.0)).max(0.00005);
    Lighting {
        sun_dir: d.sun_dir,
        sun_intensity: 1.0,
        sun_color: d.sun_color,
        secondary: d.secondary,
        ambient: d.ambient,
        fog_color: d.sky,
        fog_density: density,
        sky_color: d.sky,
        night: d.night,
        night_maps: Some(if d.lamps_on || d.night >= 0.5 {
            1.0
        } else {
            0.0
        }),
        sun_azimuth: d.azimuth_rad,
        sky_weights: d.sky_weights,
        envir_tint: d.envir_tint,
        ..Default::default()
    }
}

pub fn apply_weather(
    l: &mut Lighting,
    cloud_density: f32,
    precip_kind: i32,
    precip: f32,
    snow: f32,
) {
    let o = cloud_density.clamp(0.0, 1.0);
    let overcast = (o - 0.45).max(0.0) / 0.55;
    let grey = |c: Vec3, k: f32| -> Vec3 {
        let lum = c.dot(Vec3::new(0.3, 0.59, 0.11));
        c.lerp(Vec3::splat(lum), k)
    };
    l.sun_intensity *= 1.0 - 0.85 * overcast;
    l.sun_color = grey(l.sun_color, 0.6 * o);
    let sky_lum = l.sky_color.dot(Vec3::new(0.3, 0.59, 0.11));
    let cloud_sky = Vec3::splat(sky_lum * 0.82)
        .lerp(Vec3::new(0.62, 0.65, 0.70) * sky_lum.max(0.25) * 1.3, 0.5);
    l.sky_color = l.sky_color.lerp(cloud_sky, overcast * 0.9);
    l.secondary = grey(l.secondary, o * 0.7) * (1.0 - 0.15 * overcast);
    l.ambient = grey(l.ambient, o * 0.7) * (1.0 + 0.25 * overcast);
    l.fog_color = l.fog_color.lerp(l.sky_color, overcast);
    let rain = if precip_kind != 0 {
        precip.clamp(0.0, 1.0)
    } else {
        0.0
    };
    l.sun_intensity *= 1.0 - 0.75 * rain;
    l.ambient *= 1.0 - 0.28 * rain;
    l.secondary *= 1.0 - 0.32 * rain;
    l.sky_color *= 1.0 - 0.22 * rain;
    l.fog_color *= 1.0 - 0.15 * rain;
    if rain > 0.0 {
        l.fog_density = l.fog_density.max(2.3 / (5000.0 - 3500.0 * rain));
    }
    l.overcast = overcast;
    l.rain = rain;
    let gloom = (overcast * 0.5 + rain * 0.5).clamp(0.0, 1.0);
    l.night = l.night.max(0.45 * gloom);
    if l.night >= 0.5 {
        l.night_maps = Some(1.0);
    }
    if snow > 0.0 {
        l.ambient *= 1.0 + 0.35 * snow;
        l.secondary *= 1.0 + 0.2 * snow;
        let day = 1.0 - 0.93 * l.night.clamp(0.0, 1.0);
        l.fog_color = l
            .fog_color
            .lerp(Vec3::new(0.86, 0.88, 0.92) * day, 0.4 * snow);
    }
    l.snow = snow;
}

#[cfg(test)]
mod tests {
    use super::{headlight_core, headlight_radius};

    #[test]
    fn full_beam_range_is_not_capped_to_dipped_beam_distance() {
        // Studio Polygon Renown's third `[spotlight]` (full beam) has a 125 m range.
        assert_eq!(headlight_radius(125.0), 125.0);
        assert!((headlight_core(40.0) - 40.0 / 30.0).abs() < 1e-5);
        assert!((headlight_core(125.0) - 125.0 / 30.0).abs() < 1e-5);
    }
}
