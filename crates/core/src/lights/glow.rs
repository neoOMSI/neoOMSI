use super::*;
// (the spill lights sit just outside the body's box: inside it they counted as "in the skin" and the body neither held nor shaded their light, so it went through the bodywork)
// (a vehicle farther than this from the camera gets no window light: up to ten lights with
// occluders each, for a glow a few pixels wide - the cost on a weak graphics card)
pub(super) const SPILL_RANGE: f64 = 70.0;
pub(super) const SPILL_VEHICLES: usize = 64;
pub(super) static LED_GLOW: std::sync::atomic::AtomicU32 = std::sync::atomic::AtomicU32::new(0);

pub fn set_led_glow(v: f32) {
    LED_GLOW.store(v.to_bits(), std::sync::atomic::Ordering::Relaxed);
}

// Brightness of HTML / script screens, set here in the code only (1 = as is):
// glow = how bright the picture itself shines, light = how much light the screen throws.
pub const HTML_GLOW: f32 = 4.0;
pub const HTML_LIGHT: f32 = 4.0;
pub const SCRIPT_GLOW: f32 = 4.0;
pub const SCRIPT_LIGHT: f32 = 4.0;

// (the live values: start at the constants above, the dev menu's Light Settings change them)
pub(super) static SCREEN_FX: [std::sync::atomic::AtomicU32; 4] = [
    std::sync::atomic::AtomicU32::new(HTML_GLOW.to_bits()),
    std::sync::atomic::AtomicU32::new(HTML_LIGHT.to_bits()),
    std::sync::atomic::AtomicU32::new(SCRIPT_GLOW.to_bits()),
    std::sync::atomic::AtomicU32::new(SCRIPT_LIGHT.to_bits()),
];

/// 0 html glow, 1 html light, 2 script glow, 3 script light.
pub fn screen_fx(i: usize) -> f32 {
    f32::from_bits(SCREEN_FX[i.min(3)].load(std::sync::atomic::Ordering::Relaxed))
}

pub fn set_screen_fx(i: usize, v: f32) {
    SCREEN_FX[i.min(3)].store(v.to_bits(), std::sync::atomic::Ordering::Relaxed);
}

pub fn reset_screen_fx() {
    for (i, v) in [HTML_GLOW, HTML_LIGHT, SCRIPT_GLOW, SCRIPT_LIGHT]
        .into_iter()
        .enumerate()
    {
        set_screen_fx(i, v);
    }
}

pub(super) const SIGNAL_RADIUS: f32 = 3.5;
pub(super) const SIGNAL_GAIN: f32 = 0.45;
pub(super) const SIGNAL_CORE: f32 = 0.3;

fn is_signal(color: [f32; 3]) -> bool {
    let [r, g, b] = color;
    r >= 0.6 && b <= 0.35 && g <= r * 0.85
}

fn signal_light(c: &Corona, dark: f32) -> PointLight {
    let faces = c.direction.length_squared() > 1e-6;
    let out = if faces {
        c.direction.normalize() * SRC_OUTSET
    } else {
        Vec3::ZERO
    };
    let half = |deg: f32| (deg * 0.5).to_radians().cos();
    let (direction, cone) = if faces {
        (c.direction.normalize(), [half(120.0), half(180.0)])
    } else {
        (Vec3::ZERO, [1.0, 0.0])
    };
    PointLight {
        position: c.position + out.as_dvec3(),
        radius: SIGNAL_RADIUS * (0.7 + 0.3 * c.size.clamp(0.0, 1.0)),
        color: c.color,
        intensity: c.brightness.min(1.0) * SIGNAL_GAIN * (0.5 + 0.5 * dark),
        core: SIGNAL_CORE,
        direction,
        cone,
        mode: LightMode::Both,
        ..Default::default()
    }
}

pub fn corona_light(c: &Corona, dark: f32) -> Option<PointLight> {
    if c.brightness <= 0.01 {
        return None;
    }
    if is_signal(c.color) && !c.beam && !c.halo && c.flags & 8 == 0 {
        return Some(signal_light(c, dark));
    }
    if c.beam || c.halo || c.flags & 8 != 0 {
        return None;
    }
    let sc = settings().src;
    if !sc.on {
        return None;
    }
    let faces = c.direction.length_squared() > 1e-6;
    let out = if faces {
        c.direction.normalize() * SRC_OUTSET
    } else {
        Vec3::ZERO
    };
    let radius = SRC_RADIUS
        * (0.6 + 0.4 * c.size.clamp(0.0, 1.0))
        * c.spread.max(0.05)
        * sc.spread.max(0.05);
    let (direction, cone) = if sc.directional && faces {
        let half = |deg: f32| (deg.clamp(1.0, 179.0) * 0.5).to_radians().cos();
        (
            c.direction.normalize(),
            [half(sc.inner.min(sc.outer)), half(sc.outer)],
        )
    } else {
        (Vec3::ZERO, [1.0, 0.0])
    };
    Some(PointLight {
        position: c.position + out.as_dvec3(),
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

pub fn corona_lights(coronas: &[Corona], dark: f32, max: usize, out: &mut Vec<PointLight>) {
    let mut seen: std::collections::HashSet<[i64; 3]> = Default::default();
    let mut found: Vec<PointLight> = coronas
        .iter()
        .filter_map(|c| corona_light(c, dark))
        .filter(|l| {
            seen.insert([
                (l.position.x * 5.0).round() as i64,
                (l.position.y * 5.0).round() as i64,
                (l.position.z * 5.0).round() as i64,
            ])
        })
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
