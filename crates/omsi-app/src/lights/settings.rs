use super::*;

pub(super) const HEADLIGHT_INTENSITY: f32 = 22.0;
pub(super) const VANILLA_HEADLIGHT_INTENSITY: f32 = 0.2;
pub(super) const HIGH_BEAM_GAIN: f32 = 0.5;

/// Per-beam tweaks for vehicle headlights (applied on top of the global lamp settings).
#[derive(Clone, Copy)]
pub(crate) struct BeamCfg {
    pub on: bool,
    /// Intensity multiplier.
    pub gain: f32,
    /// Range multiplier.
    pub range: f32,
    /// Core multiplier.
    pub core: f32,
    /// Cone angles added (degrees).
    pub inner_add: f32,
    pub outer_add: f32,
    /// Aim added (degrees): left (positive) / up (positive).
    pub yaw: f32,
    pub pitch: f32,
    /// Start shift added, metres: forward, right, up.
    pub forward: f32,
    pub side: f32,
    pub height: f32,
    pub color: [f32; 3],
}

impl BeamCfg {
    pub(crate) const DEFAULT: Self = Self {
        on: true,
        gain: 1.0,
        range: 1.0,
        core: 1.0,
        inner_add: 0.0,
        outer_add: 0.0,
        yaw: 0.0,
        pitch: 0.0,
        forward: 0.0,
        side: 0.0,
        height: 0.0,
        color: [1.0, 1.0, 1.0],
    };
}

#[derive(Clone, Copy)]
pub(crate) struct LightSettings {
    pub low: BeamCfg,
    pub high: BeamCfg,
    pub headlight: f32,
    pub vanilla: f32,
    pub low_beam_gain: f32,
    pub high_beam: f32,
    pub high_beam_range: f32,
    pub high_beam_spread: f32,
    pub force_high_beam: bool,
    /// Fog cone start of vehicles (the yellow marker), metres: along its direction
    /// (negative: back), to the right, up.
    pub cone_offset: f32,
    pub cone_side: f32,
    pub cone_height: f32,
    /// Headlight (the actual light, the cyan marker) start, metres, same axes.
    pub lamp_offset: f32,
    pub lamp_side: f32,
    pub lamp_height: f32,
    /// Headlight aim, degrees: turned left (positive) / up (positive) from the authored axis.
    pub lamp_yaw: f32,
    pub lamp_pitch: f32,
    /// Headlight range (x authored range), core (x), left/right lamp distance (x), cone
    /// angles (added, degrees) and colour tint.
    pub lamp_range: f32,
    pub lamp_core: f32,
    pub lamp_spread: f32,
    pub lamp_inner_add: f32,
    pub lamp_outer_add: f32,
    pub lamp_color: [f32; 3],
    /// Show where the beams start (ImGui markers).
    pub beam_marker: bool,
    pub weather_boost: f32,
    pub weather_night: f32,
    pub corona: f32,
}

impl LightSettings {
    pub(crate) const DEFAULT: Self = Self {
        low: BeamCfg::DEFAULT,
        high: BeamCfg::DEFAULT,
        headlight: HEADLIGHT_INTENSITY,
        vanilla: VANILLA_HEADLIGHT_INTENSITY,
        low_beam_gain: 3.5,
        high_beam: HIGH_BEAM_GAIN,
        high_beam_range: 1.0,
        high_beam_spread: 1.0,
        force_high_beam: false,
        cone_offset: 0.0,
        cone_side: 0.0,
        cone_height: 0.0,
        lamp_offset: 0.0,
        lamp_side: 0.0,
        lamp_height: -0.246,
        lamp_yaw: 0.0,
        lamp_pitch: 0.0,
        lamp_range: 0.468,
        lamp_core: 1.277,
        lamp_spread: 0.951,
        lamp_inner_add: 0.0,
        lamp_outer_add: 6.885,
        lamp_color: [1.0, 1.0, 1.0],
        beam_marker: false,
        weather_boost: 0.8,
        weather_night: 0.6,
        corona: 1.0,
    };
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
        color: [1.0, 1.0, 1.0],
        shift: [0.0; 3],
    };
}

pub(super) static INTERIOR: std::sync::Mutex<Vec<InteriorCfg>> = std::sync::Mutex::new(Vec::new());

pub(crate) fn interior_cfg(i: usize) -> InteriorCfg {
    INTERIOR
        .lock()
        .unwrap_or_else(|e| e.into_inner())
        .get(i)
        .copied()
        .unwrap_or(InteriorCfg::DEFAULT)
}

pub(crate) fn set_interior_cfg(i: usize, c: InteriorCfg) {
    let mut v = INTERIOR.lock().unwrap_or_else(|e| e.into_inner());
    if v.len() <= i {
        v.resize(i + 1, InteriorCfg::DEFAULT);
    }
    v[i] = c;
}

pub(crate) fn reset_interior_cfg() {
    INTERIOR.lock().unwrap_or_else(|e| e.into_inner()).clear();
}

pub(super) static SETTINGS: std::sync::Mutex<LightSettings> =
    std::sync::Mutex::new(LightSettings::DEFAULT);

pub(crate) fn settings() -> LightSettings {
    *SETTINGS.lock().unwrap_or_else(|e| e.into_inner())
}

pub(crate) fn set_settings(s: LightSettings) {
    *SETTINGS.lock().unwrap_or_else(|e| e.into_inner()) = s;
}

pub(super) fn shift(dir: Vec3, forward: f32, side: f32, up: f32) -> DVec3 {
    let d = dir.normalize_or_zero();
    let right = d.cross(Vec3::Z).normalize_or_zero();
    (d * forward + right * side + Vec3::Z * up).as_dvec3()
}

pub(crate) fn cone_shift(dir: Vec3, cfg: &LightSettings) -> DVec3 {
    shift(dir, cfg.cone_offset, cfg.cone_side, cfg.cone_height)
}

pub(super) fn lamp_aim(d: Vec3, cfg: &LightSettings, bc: &BeamCfg) -> Vec3 {
    let d = d.normalize_or_zero();
    let yawed = glam::Quat::from_rotation_z((cfg.lamp_yaw + bc.yaw).to_radians()) * d;
    let right = yawed.cross(Vec3::Z).normalize_or_zero();
    (glam::Quat::from_axis_angle(right, (cfg.lamp_pitch + bc.pitch).to_radians()) * yawed)
        .normalize_or_zero()
}

pub(crate) fn lamp_shift(dir: Vec3, cfg: &LightSettings) -> DVec3 {
    shift(dir, cfg.lamp_offset, cfg.lamp_side, cfg.lamp_height)
}
