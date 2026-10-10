use super::tuning::*;
use glam::{DVec3, Vec3};
use ::render::PointLight;
use std::sync::atomic::{AtomicU32, Ordering};
use std::sync::{Mutex, MutexGuard};

fn locked<T>(m: &Mutex<T>) -> MutexGuard<'_, T> {
    m.lock().unwrap_or_else(|e| e.into_inner())
}

pub(super) fn half_cos(deg: f32) -> f32 {
    (deg.clamp(1.0, 179.0) * 0.5).to_radians().cos()
}

#[derive(Clone, Copy)]
pub(crate) struct BeamCfg {
    pub on: bool,
    pub gain: f32,
    pub range: f32,
    pub core: f32,
    
    pub yaw: f32,
    pub pitch: f32,
    pub color: [f32; 3],
}

impl BeamCfg {
    pub(crate) const DEFAULT: Self = Self {
        on: true,
        gain: 1.0,
        range: 1.0,
        core: 1.0,
        yaw: 0.0,
        pitch: 0.0,
        color: [1.0; 3],
    };
}

#[derive(Clone, Copy)]
pub(crate) struct MapSpotCfg {
    pub on: bool,
    pub gain: f32,
    pub range: f32,
    pub core: f32,
    pub inner: f32,
    pub outer: f32,
    pub tilt: f32,
    pub yaw: f32,
    pub height: f32,
}

impl MapSpotCfg {
    pub(crate) const DEFAULT: Self = Self {
        on: true,
        gain: 1.0,
        range: 1.0,
        core: 1.0,
        inner: 60.0,
        outer: 110.0,
        tilt: 0.0,
        yaw: 0.0,
        height: 0.0,
    };

    pub(super) fn apply(&self, mut l: PointLight) -> PointLight {
        if !self.on {
            return l;
        }
        if l.direction.length_squared() < 1e-6 {
            let rot = glam::Quat::from_rotation_z(self.yaw.to_radians())
                * glam::Quat::from_rotation_x(self.tilt.to_radians());
            l.direction = (rot * Vec3::NEG_Z).normalize_or_zero();
            l.cone = [half_cos(self.inner.min(self.outer)), half_cos(self.outer)];
        }
        l.radius *= self.range.max(0.05);
        l.core = (l.core * self.core.max(0.01)).min(l.radius);
        l.intensity *= self.gain;
        l.position += DVec3::Z * self.height as f64;
        l
    }
}

#[derive(Clone, Copy)]
pub(crate) struct LampLightCfg {
    pub on: bool,
    pub gain: f32,
    pub range: f32,
    pub core: f32,
    pub max: i32,
}

impl LampLightCfg {
    pub(crate) const DEFAULT: Self = Self {
        on: true,
        gain: 1.0,
        range: 14.0,
        core: 1.5,
        max: 48,
    };
}

#[derive(Clone, Copy)]
pub(crate) struct SpillCfg {
    pub on: bool,
    pub gain: f32,
    pub range: f32,
    pub core: f32,
    pub inner_add: f32,
    pub outer_add: f32,
    pub tilt_add: f32,
    pub spread: f32,
    pub reach: f32,
    pub vehicles: i32,
    pub marker: bool,
}

impl SpillCfg {
    pub(crate) const DEFAULT: Self = Self {
        on: true,
        gain: 1.0,
        range: 1.0,
        core: 1.0,
        inner_add: 0.0,
        outer_add: 0.0,
        tilt_add: 0.0,
        spread: 1.0,
        reach: SPILL_RANGE as f32,
        vehicles: SPILL_VEHICLES as i32,
        marker: false,
    };

    pub(super) fn radius(&self) -> f32 {
        SPILL_RADIUS * self.range.max(0.05)
    }
}

#[derive(Clone, Copy)]
pub(crate) struct Spot2Cfg {
    pub on: bool,
    pub gain: f32,
    pub range: f32,
    pub core: f32,
    pub height: f32,
    pub marker: bool,
}

impl Spot2Cfg {
    pub(crate) const DEFAULT: Self = Self {
        on: true,
        gain: 1.0,
        range: 1.0,
        core: 1.0,
        height: 0.0,
        marker: false,
    };
}

#[derive(Clone, Copy)]
pub(crate) struct SourceCfg {
    pub on: bool,
    pub gain: f32,
    pub spread: f32,
    pub core: f32,
    pub directional: bool,
    pub inner: f32,
    pub outer: f32,
}

impl SourceCfg {
    pub(crate) const DEFAULT: Self = Self {
        on: true,
        gain: 0.077,
        spread: 0.1,
        core: 0.530,
        directional: true,
        inner: 60.0,
        outer: 140.0,
    };
}

#[derive(Clone, Copy)]
pub(crate) struct LightSettings {
    pub spill: SpillCfg,
    pub src: SourceCfg,
    pub spot2: Spot2Cfg,
    pub lamp_light: LampLightCfg,
    pub map_spot: MapSpotCfg,
    pub low: BeamCfg,
    pub high: BeamCfg,
    pub headlight: f32,
    pub vanilla: f32,
    pub low_beam_gain: f32,
    
    pub cone_offset: f32,
    pub cone_side: f32,
    pub cone_height: f32,
    
    pub lamp_yaw: f32,
    pub lamp_pitch: f32,
    
    pub lamp_color: [f32; 3],
    
    pub beam_marker: bool,
    pub weather_night: f32,
    
    pub nightmap_gain: f32,
    pub lightmap_gain: f32,
    pub corona: f32,
}

impl LightSettings {
    pub(crate) const DEFAULT: Self = Self {
        spill: SpillCfg::DEFAULT,
        src: SourceCfg::DEFAULT,
        spot2: Spot2Cfg::DEFAULT,
        lamp_light: LampLightCfg::DEFAULT,
        map_spot: MapSpotCfg::DEFAULT,
        low: BeamCfg::DEFAULT,
        high: BeamCfg::DEFAULT,
        headlight: HEADLIGHT_INTENSITY,
        vanilla: VANILLA_HEADLIGHT_INTENSITY,
        low_beam_gain: 3.5,
        cone_offset: 0.0,
        cone_side: 0.0,
        cone_height: 0.0,
        lamp_yaw: 0.0,
        lamp_pitch: 0.0,
        lamp_color: [1.0; 3],
        beam_marker: false,
        weather_night: 0.6,
        nightmap_gain: 0.75,
        lightmap_gain: 1.0,
        corona: 1.0,
    };

    pub(crate) fn cone_shift(&self, dir: Vec3) -> DVec3 {
        offset_along(dir, self.cone_offset, self.cone_side, self.cone_height)
    }

    pub(super) fn aim(&self, d: Vec3, beam: &BeamCfg, side: f32) -> Vec3 {
        let d = d.normalize_or_zero();
        let yaw = (self.lamp_yaw + beam.yaw) * side;
        let yawed = glam::Quat::from_rotation_z(yaw.to_radians()) * d;
        let right = yawed.cross(Vec3::Z).normalize_or_zero();
        let pitch = glam::Quat::from_axis_angle(right, (self.lamp_pitch + beam.pitch).to_radians());
        (pitch * yawed).normalize_or_zero()
    }
}

fn offset_along(dir: Vec3, forward: f32, side: f32, up: f32) -> DVec3 {
    let d = dir.normalize_or_zero();
    let right = d.cross(Vec3::Z).normalize_or_zero();
    (d * forward + right * side + Vec3::Z * up).as_dvec3()
}

static SETTINGS: Mutex<LightSettings> = Mutex::new(LightSettings::DEFAULT);

pub(crate) fn settings() -> LightSettings {
    *locked(&SETTINGS)
}

pub(crate) fn set_settings(s: LightSettings) {
    *locked(&SETTINGS) = s;
}

#[derive(Clone, Copy)]
pub(crate) struct InteriorCfg {
    pub off: bool,
    pub gain: f32,
    pub range: f32,
    pub color: [f32; 3],
    pub shift: [f32; 3],
}

impl InteriorCfg {
    pub(crate) const DEFAULT: Self = Self {
        off: false,
        gain: 1.0,
        range: 1.0,
        color: [1.0; 3],
        shift: [0.0; 3],
    };
}

#[derive(Clone, Copy)]
pub(crate) struct ExteriorCfg {
    pub off: bool,
    pub gain: f32,
    pub size: f32,
    pub spread: f32,
    pub color: [f32; 3],
    pub shift: [f32; 3],
}

impl ExteriorCfg {
    pub(crate) const DEFAULT: Self = Self {
        off: false,
        gain: 1.0,
        size: 1.0,
        spread: 1.0,
        color: [1.0; 3],
        shift: [0.0; 3],
    };
}

struct Overrides<T: Copy> {
    default: T,
    table: Mutex<Vec<T>>,
}

impl<T: Copy> Overrides<T> {
    const fn new(default: T) -> Self {
        Self {
            default,
            table: Mutex::new(Vec::new()),
        }
    }
    fn get(&self, i: usize) -> T {
        locked(&self.table).get(i).copied().unwrap_or(self.default)
    }
    fn set(&self, i: usize, v: T) {
        let mut t = locked(&self.table);
        if t.len() <= i {
            t.resize(i + 1, self.default);
        }
        t[i] = v;
    }
    fn clear(&self) {
        locked(&self.table).clear();
    }
}

static EXTERIOR: Overrides<ExteriorCfg> = Overrides::new(ExteriorCfg::DEFAULT);
static INTERIOR: Overrides<InteriorCfg> = Overrides::new(InteriorCfg::DEFAULT);

pub(crate) fn exterior_cfg(i: usize) -> ExteriorCfg {
    EXTERIOR.get(i)
}
pub(crate) fn set_exterior_cfg(i: usize, c: ExteriorCfg) {
    EXTERIOR.set(i, c);
}
pub(crate) fn reset_exterior_cfg() {
    EXTERIOR.clear();
}
pub(crate) fn interior_cfg(i: usize) -> InteriorCfg {
    INTERIOR.get(i)
}
pub(crate) fn set_interior_cfg(i: usize, c: InteriorCfg) {
    INTERIOR.set(i, c);
}
pub(crate) fn reset_interior_cfg() {
    INTERIOR.clear();
}

static LED_GLOW: AtomicU32 = AtomicU32::new(0);

pub fn set_led_glow(v: f32) {
    LED_GLOW.store(v.to_bits(), Ordering::Relaxed);
}

pub(super) fn led_glow() -> f32 {
    f32::from_bits(LED_GLOW.load(Ordering::Relaxed))
}

/// 0 html glow, 1 html light, 2 script glow, 3 script light.
static SCREEN_FX: [AtomicU32; 4] = [
    AtomicU32::new(HTML_GLOW.to_bits()),
    AtomicU32::new(HTML_LIGHT.to_bits()),
    AtomicU32::new(SCRIPT_GLOW.to_bits()),
    AtomicU32::new(SCRIPT_LIGHT.to_bits()),
];

pub fn screen_fx(i: usize) -> f32 {
    f32::from_bits(SCREEN_FX[i.min(3)].load(Ordering::Relaxed))
}

pub fn set_screen_fx(i: usize, v: f32) {
    SCREEN_FX[i.min(3)].store(v.to_bits(), Ordering::Relaxed);
}

pub fn reset_screen_fx() {
    for (i, v) in [HTML_GLOW, HTML_LIGHT, SCRIPT_GLOW, SCRIPT_LIGHT]
        .into_iter()
        .enumerate()
    {
        set_screen_fx(i, v);
    }
}
