//! One-pole filtering with a 120 ms per-sample coefficient transition. An open filter
//! follows the input state too, so closing a door never resumes stale filter history.
pub const OPEN_HZ: f32 = 20_000.0;
pub struct LowPass {
    alpha: f32, target: f32, smoothing: f32, initialized: bool, state: [f32; 2],
}

impl Default for LowPass {
    fn default() -> Self {
        Self { alpha: 1.0, target: 1.0, smoothing: 1.0, initialized: false, state: [0.0; 2] }
    }
}
impl LowPass {
    pub fn set_target(&mut self, hz: f32, _frames: usize, rate: f32) {
        self.target = if hz.is_finite() && hz > 0.0 && hz < OPEN_HZ * 0.95 {
            1.0 - (-std::f32::consts::TAU * hz.max(1.0) / rate.max(1.0)).exp()
        } else { 1.0 };
        self.smoothing = 1.0 - (-1.0 / (rate.max(1.0) * 0.12)).exp();
        if !self.initialized { self.alpha = self.target; self.initialized = true; }
    }
    pub fn is_on(&self) -> bool {
        self.alpha < 0.99999
    }
    pub fn alpha(&self, _rate: f32) -> f32 {
        self.alpha
    }
    pub fn process(&mut self, l: f32, r: f32, _alpha: f32) -> (f32, f32) {
        self.alpha += (self.target - self.alpha) * self.smoothing;
        super::envelope::smooth(&mut self.state[0], l, self.alpha);
        super::envelope::smooth(&mut self.state[1], r, self.alpha);
        (self.state[0], self.state[1])
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_filter_tail_reaches_silence_without_subnormal_state() {
        for rate in [44_100, 48_000, 96_000, 192_000] {
            let mut filter = LowPass::default();
            filter.set_target(700.0, 256, rate as f32);
            filter.process(1.0, -0.5, 0.0);
            for _ in 0..rate / 10 {
                filter.process(0.0, 0.0, 0.0);
            }
            assert_eq!(filter.state, [0.0; 2], "silent tail at {rate} Hz");
        }
    }
}
