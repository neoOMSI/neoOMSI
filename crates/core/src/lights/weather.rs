use super::tuning::CORONA_RANGE;
use glam::Vec3;
use ::render::Lighting;
use ::simulation::Daylight;
use std::sync::atomic::{AtomicU32, Ordering};

static FOG_VISIBILITY: AtomicU32 = AtomicU32::new(0);
static FOG_NIGHT: AtomicU32 = AtomicU32::new(0);

pub fn set_cone_strength(fog_visibility_m: f32, _precip: f32, night: f32) {
    FOG_VISIBILITY.store(fog_visibility_m.to_bits(), Ordering::Relaxed);
    FOG_NIGHT.store(night.clamp(0.0, 1.0).to_bits(), Ordering::Relaxed);
}

pub(super) fn fog_state() -> (f32, f32) {
    let vis = f32::from_bits(FOG_VISIBILITY.load(Ordering::Relaxed));
    let night = f32::from_bits(FOG_NIGHT.load(Ordering::Relaxed));
    (if vis > 0.0 { vis } else { 1.0e6 }, night)
}

pub(super) fn visible_range() -> f64 {
    let (vis, _) = fog_state();
    CORONA_RANGE.min((vis as f64 * 2.0).max(60.0))
}

pub(super) fn fog_darkness() -> f32 {
    let (vis, _) = fog_state();
    (1.0 - vis / 3000.0).clamp(0.0, 1.0)
}

const LUMA: Vec3 = Vec3::new(0.3, 0.59, 0.11);

fn desaturate(c: Vec3, k: f32) -> Vec3 {
    c.lerp(Vec3::splat(c.dot(LUMA)), k)
}

pub fn lighting_from(d: &Daylight, fog_range: f32) -> Lighting {
    let lamps = d.lamps_on || d.night >= 0.5;
    Lighting {
        sun_dir: d.sun_dir,
        sun_intensity: 1.0,
        sun_color: d.sun_color,
        secondary: d.secondary,
        ambient: d.ambient,
        fog_color: d.sky,
        fog_density: (2.3 / fog_range.max(50.0)).max(0.00005),
        sky_color: d.sky,
        night: d.night,
        night_maps: Some(if lamps { 1.0 } else { 0.0 }),
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
    let cloud = cloud_density.clamp(0.0, 1.0);
    let overcast = (cloud - 0.45).max(0.0) / 0.55;
    l.sun_intensity *= 1.0 - 0.85 * overcast;
    l.sun_color = desaturate(l.sun_color, 0.6 * cloud);
    let sky_lum = l.sky_color.dot(LUMA);
    let grey_sky = Vec3::splat(sky_lum * 0.82)
        .lerp(Vec3::new(0.62, 0.65, 0.70) * sky_lum.max(0.25) * 1.3, 0.5);
    l.sky_color = l.sky_color.lerp(grey_sky, overcast * 0.9);
    l.secondary = desaturate(l.secondary, cloud * 0.7) * (1.0 - 0.15 * overcast);
    l.ambient = desaturate(l.ambient, cloud * 0.7) * (1.0 + 0.25 * overcast);
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
    use super::*;

    #[test]
    fn clear_sky_changes_nothing() {
        let mut l = Lighting::default();
        l.sun_intensity = 1.0;
        apply_weather(&mut l, 0.0, 0, 0.0, 0.0);
        assert_eq!(l.sun_intensity, 1.0);
        assert_eq!(l.rain, 0.0);
        assert_eq!(l.snow, 0.0);
    }

    #[test]
    fn rain_dims_the_sun_and_adds_gloom() {
        let mut l = Lighting::default();
        l.sun_intensity = 1.0;
        apply_weather(&mut l, 1.0, 1, 1.0, 0.0);
        assert!(l.sun_intensity < 0.1);
        assert!(l.night >= 0.45);
    }
}
