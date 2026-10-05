// (the spill lights sit just outside the body's box: inside it they counted as "in the skin" and the body neither held nor shaded their light, so it went through the bodywork)
// (a vehicle farther than this from the camera gets no window light: up to ten lights with
// occluders each, for a glow a few pixels wide - the cost on a weak graphics card)
pub(super) const SPILL_RANGE: f64 = 30.0;
pub(super) const SPILL_VEHICLES: usize = 3;
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
