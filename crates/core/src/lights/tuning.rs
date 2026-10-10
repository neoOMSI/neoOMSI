pub(super) const LED_RANGE: f64 = 40.0;
pub(super) const LED_PANELS: usize = 8;
pub(super) const LED_RADIUS: f32 = 6.0;
pub(super) const LED_INTENSITY: f32 = 0.5;
pub(super) const LED_OUTSET: f64 = 0.3;
pub(super) const LED_COLOR: [f32; 3] = [1.0, 0.62, 0.12];
pub(super) const SCREEN_RADIUS: f32 = 2.5;
pub(super) const SCREEN_INTENSITY: f32 = 0.08;
pub(super) const SCREEN_COLOR: [f32; 3] = [0.82, 0.9, 1.0];

pub(super) const SPILL_LAMP: f32 = 0.25;
pub(super) const SPILL_RADIUS: f32 = 12.0;
pub(super) const SPILL_CORE: f32 = 2.0;
pub(super) const SPILL_WHITE: f32 = 0.6;
pub(super) const SPILL_TILT: f32 = 20.0;
pub(super) const SPILL_INNER: f32 = 15.0;
pub(super) const SPILL_OUTER: f32 = 45.0;
pub(super) const SPILL_RANGE: f64 = 70.0;
pub(super) const SPILL_VEHICLES: usize = 64;

pub(super) const MAP_LIGHT_RANGE: f64 = 300.0;
pub(super) const CORONA_RANGE: f64 = 1500.0;
pub(super) const NEAR_MARGIN: f64 = 100.0;

pub(super) const OCC_HALF: f64 = 1.0;
pub(super) const OCC_HEIGHT: f64 = 2.5;
pub(super) const OCC_LIGHT_RANGE: f64 = 150.0;
pub(super) const OCC_CORONA_RANGE: f64 = 300.0;
pub(super) const OCC_RECHECK: f64 = 6.0;
pub(super) const VIS_PER_FRAME: usize = 16;
pub(super) const LAMP_RAYS_PER_FRAME: usize = 8;
pub(super) const BUILD_BUDGET_US: u128 = 1500;

pub(super) const ENCL_REACH: f64 = 12.0;
pub(super) const ENCL_UP: f64 = 10.0;
pub(super) const ENCL_MIN_WALLS: usize = 4;
pub(super) const ENCL_MIN_RADIUS: f32 = 3.0;

pub(super) const SRC_RADIUS: f32 = 4.5;
pub(super) const SRC_CORE: f32 = 0.4;
pub(super) const SRC_GAIN: f32 = 0.9;
pub(super) const SRC_MAX_VEHICLE: usize = 64;
pub(super) const SRC_MAX_OBJECTS: usize = 12;
pub(super) const SRC_RANGE: f64 = 150.0;
pub(super) const SRC_OUTSET: f32 = 0.15;
pub(super) const SIGNAL_RADIUS: f32 = 3.5;
pub(super) const SIGNAL_GAIN: f32 = 0.45;
pub(super) const SIGNAL_CORE: f32 = 0.3;

pub(super) const LAMP_RISE: f32 = 0.05;
pub(super) const LAMP_FALL: f32 = 0.12;
pub(super) const SALOON_RISE: f32 = 0.08;
pub(super) const SALOON_FALL: f32 = 0.10;
pub(super) const SLOT_SPOT2: u32 = 1000;
pub(super) const SLOT_SALOON: u32 = 2000;

pub(super) const BODY_INNER: f32 = 0.4;
pub(super) const BODY_SKIN: f32 = 0.3;
pub(super) const NEAR_VEHICLE: f64 = 15.0;
pub(super) const MESH_TESTS_PER_FRAME: usize = 3;
pub(super) const VEHICLE_TESTS_PER_FRAME: usize = 2;

pub(super) const SHADOW_RANGE: f64 = 50.0;
pub(super) const SHADOW_REACH: f64 = 25.0;
pub(super) const SHADOW_MAX: usize = 32;
pub(super) const SHADOW_LIGHTS: usize = 32;
pub(super) const POINT_TRI_MAX: usize = 10;
pub(super) const POINT_TRI_MIN_AREA: f64 = 0.4;
pub(super) const SHADOW_SPOTS: usize = 8;
pub(super) const SPOT_SHADOW_RANGE: f64 = 60.0;
pub(super) const SPOT_REACH: f64 = 40.0;
pub(super) const SPOT_MAX: usize = 32;
pub(super) const SPOT_MIN_AREA: f64 = 0.01;
pub(super) const SPOT_MARGIN: f64 = 2.0;
pub(super) const GATHERS_PER_FRAME: usize = 2;
pub(super) const OCC_CACHE_MAX: usize = 4000;

pub(super) const CODE_DIPPED: f32 = 200.0;
pub(super) const CODE_MAIN: f32 = 300.0;

pub(super) const HEADLIGHT_INTENSITY: f32 = 45.0;
pub(super) const VANILLA_HEADLIGHT_INTENSITY: f32 = 0.2;

pub const HTML_GLOW: f32 = 4.0;
pub const HTML_LIGHT: f32 = 4.0;
pub const SCRIPT_GLOW: f32 = 4.0;
pub const SCRIPT_LIGHT: f32 = 4.0;
