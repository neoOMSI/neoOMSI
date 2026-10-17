use super::model::*;
use super::region::{Local, Regional};
use super::sky;
use crate::startup::{CLASSIC, CLOUDS, ENHANCED};
use glam::Vec3;
use std::sync::atomic::Ordering::Relaxed;

pub fn enhanced_sky() -> bool {
    ENHANCED.load(Relaxed)
}

fn dim(l: &mut ::render::Lighting, k: f32) {
    if l.enhanced {
        l.brightness = k.max(0.0);
        return;
    }
    l.sun_intensity *= k;
    l.secondary *= k;
    l.ambient *= k;
    l.sky_color *= k;
    l.fog_color *= k;
}

pub fn lighting(
    daylight: &::simulation::Daylight,
    w: &Weather,
    region: &Regional,
    local: &Local,
    wetness: f32,
    shadows: bool,
) -> ::render::Lighting {
    let clouds = CLOUDS.load(Relaxed);
    let mut l = crate::lights::lighting_from(daylight, local.visibility_m);
    l.enhanced = ENHANCED.load(Relaxed);
    l.classic = CLASSIC.load(Relaxed);
    let thick = if clouds { local.thick } else { 0.0 };
    if clouds {
        let s = sky::render_sky(w, local.precip_rate);
        l.cloud_density = s.density;
        l.cloud_layers = s.layers;
    }
    l.cloud_offset = region.offset();
    crate::lights::apply_weather(
        &mut l,
        thick,
        local.precip_kind,
        local.precip_rate,
        if w.snow_cover { 1.0 } else { 0.0 },
    );
    if w.precip.is_falling() {
        l.rain_area = super::region::areal_fraction(w).clamp(0.02, 1.0);
        l.rain_far = w.precip.intensity.clamp(0.0, 1.0);
    }
    dim(&mut l, w.brightness);
    l.wetness = wetness;
    l.shadows = shadows && thick < 0.85 && local.visibility_m > 350.0;
    l.light_shadows = shadows;
    l
}

pub const RAIN_GRID: usize = 16;
pub const RAIN_GRID_SPAN_M: f64 = 6400.0;

pub fn fill_rain_grid(
    l: &mut ::render::Lighting,
    w: &Weather,
    region: &Regional,
    observer: [f64; 2],
    enhanced: bool,
) {
    l.rain_grid = [0.0; 256];
    if !w.precip.is_falling() {
        return;
    }
    let step = RAIN_GRID_SPAN_M / RAIN_GRID as f64;
    let half = RAIN_GRID as f64 * 0.5;
    for j in 0..RAIN_GRID {
        for i in 0..RAIN_GRID {
            let at = [
                observer[0] + (i as f64 + 0.5 - half) * step,
                observer[1] + (j as f64 + 0.5 - half) * step,
            ];
            let s = region.sample(w, at, enhanced);
            if s.precip_kind == 1 {
                l.rain_grid[j * RAIN_GRID + i] = s.precip_rate;
            }
        }
    }
}

pub fn road_wetness(rate: f32, secs: f64, start: f32) -> f32 {
    let secs = secs as f32;
    if rate > 0.0 {
        (start + secs * rate / 180.0).clamp(0.0, 1.0)
    } else {
        (start - secs / 1200.0).clamp(0.0, 1.0)
    }
}

pub fn initial_wetness(w: &Weather) -> f32 {
    w.ground_wet.clamp(0.0, 1.0)
}

pub fn street_condition(w: &Weather, local: &Local, wetness: f32) -> f32 {
    let wet = wetness.clamp(0.0, 1.0);
    if w.snow_on_road || local.snowing() {
        1.0 + wet
    } else {
        wet
    }
}

pub fn wind_vector(w: &Weather) -> Vec3 {
    let heading = w.wind_dir.to_radians();
    Vec3::new(heading.sin(), heading.cos(), 0.0) * w.wind_speed
}

pub fn apply_to_vehicle(
    v: &mut ::simulation::VehicleInstance,
    w: &Weather,
    local: &Local,
    wetness: f32,
    sheltered: bool,
) {
    let (kind, rate) = local.precip();
    let rate = if sheltered { 0.0 } else { rate };
    v.host.precip_type = kind as f32;
    v.host.precip_rate = rate;
    v.host.wind = wind_vector(w);
    v.host.street_cond = street_condition(w, local, wetness);
    v.set_var("PrecipType", kind as f32);
    v.set_var("PrecipRate", rate);
    v.host.temperature = w.temp_c;
    v.host.abs_humidity = w.abs_humidity();
    ::simulation::host::set_ambient_weather(w.temp_c, w.abs_humidity());
}

pub fn debug_sound_every() -> Option<f32> {
    static EVERY: std::sync::OnceLock<Option<f32>> = std::sync::OnceLock::new();
    *EVERY.get_or_init(|| {
        ::legacy_config::env::var("OMSI_DEBUG_SOUND")
            .ok()
            .map(|v| v.parse::<f32>().ok().filter(|s| *s > 0.0).unwrap_or(5.0))
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn plain_graphics_take_all_the_light_down() {
        let mut l = ::render::Lighting::default();
        dim(&mut l, 0.0);
        assert_eq!(l.sun_intensity, 0.0);
        assert_eq!(l.ambient, Vec3::ZERO);
        assert_eq!(l.sky_color, Vec3::ZERO);
    }

    #[test]
    fn enhanced_graphics_keep_the_mirrors_light() {
        let mut l = ::render::Lighting { enhanced: true, ..Default::default() };
        let before = l.clone();
        dim(&mut l, 0.0);
        assert_eq!(l.brightness, 0.0);
        assert_eq!(l.ambient, before.ambient);
    }

    #[test]
    fn roads_get_wet_in_rain_and_dry_without() {
        assert!(road_wetness(0.6, 120.0, 0.0) > 0.3);
        assert!(road_wetness(0.0, 600.0, 1.0) < 0.6);
    }
}
