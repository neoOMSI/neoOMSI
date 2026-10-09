//! Cached windowed-sinc: 32 taps, 256 phases, 25 quarter-octave bands (steps <=64).
//! Narrow cutoffs reject aliases; short band blends avoid pitch-change clicks.

use crate::assets::Clip;
const TAPS: usize = 32;
pub(crate) const PHASES: usize = 256;
const BANDS: usize = 25;
const BAND_TRANSITION: f64 = 0.001;

/// Normalize sinc rows before playback.
pub(crate) fn fill_sinc_row(row: &mut [f32], cutoff: f64, fraction: f64) {
    let radius = row.len() as f64 / 2.0;
    let mut sum = 0.0;
    for (tap, weight) in row.iter_mut().enumerate() {
        let x = tap as f64 - (radius - 1.0) - fraction;
        let y = std::f64::consts::PI * x * cutoff;
        let sinc = if y.abs() < 1e-12 { 1.0 } else { y.sin() / y };
        let window = 0.42 + 0.5 * (std::f64::consts::PI * x / radius).cos()
            + 0.08 * (std::f64::consts::TAU * x / radius).cos();
        *weight = (cutoff * sinc * window) as f32;
        sum += *weight;
    }
    for weight in row { *weight /= sum; }
}

pub fn step(pitch: f32, doppler: f32, clip_rate: u32, dev_rate: f64) -> f64 {
    let value = pitch as f64 * doppler as f64 * clip_rate as f64 / dev_rate.max(1.0);
    if value.is_finite() { value.clamp(0.0001, 64.0) } else { 1.0 }
}

pub struct Kernel { weights: Vec<[f32; TAPS]>, steps: [f64; BANDS] }
impl Default for Kernel {
    fn default() -> Self {
        let mut weights = Vec::with_capacity(BANDS * (PHASES + 1));
        for band in 0..BANDS {
            let cutoff = 0.94 / 2.0f64.powf(band as f64 / 4.0);
            for phase in 0..=PHASES {
                let fraction = phase as f64 / PHASES as f64;
                let mut row = [0.0; TAPS];
                fill_sinc_row(&mut row, cutoff, fraction);
                weights.push(row);
            }
        }
        Self { weights, steps: std::array::from_fn(|i| 2.0f64.powf(i as f64 / 4.0)) }
    }
}
/// Cached discontinuity data, prepared when constructing a voice, with no allocation.
#[derive(Clone, Copy)]
pub struct Seam { start: usize, span: usize, delta: [f32; 2] }
impl Seam {
    pub fn new(clip: &Clip) -> Self {
        let frames = clip.frames();
        let span = (clip.sample_rate as usize / 1000).min(frames / 4);
        let mut seam = Self { start: frames.saturating_sub(span), span, delta: [0.0; 2] };
        if frames < 2 || clip.channels == 0 { return seam; }
        for c in 0..2 {
            let read = |i| clip.samples[i * clip.channels as usize + c.min(clip.channels as usize - 1)] as f32 / 32768.0;
            let last = frames - 1;
            let delta = read(last) - read(0);
            let slope = (read(last) - read(last - 1)).abs().max((read(1) - read(0)).abs());
            // A seam larger than twice the adjacent slope and 2% of full scale is a
            // discontinuity; ordinary periodic endpoints keep their natural sample slope.
            if delta.abs() > (slope * 2.0).max(0.02) { seam.delta[c] = delta; }
        }
        seam
    }
}
impl Kernel {
    pub fn frame(&self, clip: &Clip, pos: f64, step: f64, looping: bool) -> (f32, f32) {
        self.frame_with_seam(clip, pos, step, looping, Seam::new(clip))
    }
    pub fn frame_with_seam(&self, clip: &Clip, pos: f64, step: f64, looping: bool, seam: Seam) -> (f32, f32) {
        let frames = clip.frames();
        let ch = clip.channels as usize;
        if frames == 0 || ch == 0 { return (0.0, 0.0); }
        // Binary search cached step thresholds instead of log2 in every audio frame.
        let band = if step <= self.steps[0] { 0 }
            else { self.steps.partition_point(|s| *s < step).min(BANDS - 1) };
        // Interpolate adjacent phase rows. Rounding to the nearest of 256 phases
        // quantized playback time and added periodic high-frequency modulation/spurs.
        let phase = pos.fract() * PHASES as f64;
        let row = (phase as usize).min(PHASES - 1);
        let mut fraction = (phase - row as f64) as f32;
        let mut weights = &self.weights[band * (PHASES + 1) + row];
        let mut next = &self.weights[band * (PHASES + 1) + row + 1];
        let mut transition = [0.0; TAPS];
        if band > 0 {
            let lower = self.steps[band - 1];
            let blend = (step - lower) / (lower * BAND_TRANSITION);
            if blend < 1.0 {
                // Blend only 0.1% above boundaries; 0.94 * 1.001 stays below Nyquist.
                let previous = &self.weights[(band - 1) * (PHASES + 1) + row];
                let previous_next = &self.weights[(band - 1) * (PHASES + 1) + row + 1];
                for i in 0..TAPS {
                    let a = previous[i] + (previous_next[i] - previous[i]) * fraction;
                    let b = weights[i] + (next[i] - weights[i]) * fraction;
                    transition[i] = a + (b - a) * blend as f32;
                }
                weights = &transition;
                next = &transition;
                fraction = 0.0;
            }
        }
        let mut index = pos.floor() as i64 - (TAPS / 2 - 1) as i64;
        // Contiguous windows skip wrapping, clamping and seam correction.
        let end = index + TAPS as i64;
        if index >= 0 && end <= frames as i64
            && (!looping || seam.span <= 1 || seam.delta == [0.0; 2] || end <= seam.start as i64) {
            let samples = &clip.samples[index as usize * ch..end as usize * ch];
            if ch == 1 {
                let samples: &[i16; TAPS] = samples.try_into().unwrap();
                // Independent sums allow SIMD across taps.
                let mut out = [0.0; 4];
                for tap in (0..TAPS).step_by(4) {
                    for lane in 0..4 {
                        let i = tap + lane;
                        let weight = weights[i] + (next[i] - weights[i]) * fraction;
                        out[lane] += samples[i] as f32 * weight;
                    }
                }
                let out = out.into_iter().sum::<f32>() / 32768.0;
                return (out, out);
            }
            if ch == 2 {
                let samples: &[i16; TAPS * 2] = samples.try_into().unwrap();
                let mut out = [[0.0; 4]; 2];
                for tap in (0..TAPS).step_by(4) {
                    for lane in 0..4 {
                        let i = tap + lane;
                        let weight = weights[i] + (next[i] - weights[i]) * fraction;
                        out[0][lane] += samples[i * 2] as f32 * weight;
                        out[1][lane] += samples[i * 2 + 1] as f32 * weight;
                    }
                }
                return (out[0].into_iter().sum::<f32>() / 32768.0,
                    out[1].into_iter().sum::<f32>() / 32768.0);
            }
            let mut out = [0.0; 2];
            for ((a, b), frame) in weights.iter().zip(next).zip(samples.chunks_exact(ch)) {
                let weight = a + (b - a) * fraction;
                out[0] += (frame[0] as f32 / 32768.0) * weight;
                out[1] += (frame[1] as f32 / 32768.0) * weight;
            }
            return (out[0], out[1]);
        }
        if looping { index = index.rem_euclid(frames as i64); }
        let mut out = [0.0; 2];
        for (a, b) in weights.iter().zip(next) {
            let weight = a + (b - a) * fraction;
            let i = index.clamp(0, frames as i64 - 1) as usize;
            let base = i * ch;
            let mut l = clip.samples[base] as f32 / 32768.0;
            let mut r = if ch == 1 { l } else { clip.samples[base + 1] as f32 / 32768.0 };
            if looping && seam.span > 1 && i >= seam.start {
                let blend = (i - seam.start) as f32 / (seam.span - 1) as f32;
                l -= seam.delta[0] * blend; r -= seam.delta[1] * blend;
            }
            out[0] += l * weight; out[1] += r * weight;
            index += 1;
            if looping && index == frames as i64 { index = 0; }
        }
        (out[0], out[1])
    }
}
fn sample(clip: &Clip, index: usize) -> (f32, f32) {
    let ch = clip.channels as usize;
    let l = clip.samples[index * ch] as f32 / 32768.0;
    let r = clip.samples[index * ch + (ch - 1).min(1)] as f32 / 32768.0;
    (l, r)
}
/// Kept for callers of the former linear interpolator; voices use `Kernel::frame`.
pub fn frame(clip: &Clip, i0: usize, i1: usize, t: f32, _cch: usize) -> (f32, f32) {
    let a = sample(clip, i0);
    let b = sample(clip, i1);
    (a.0 + (b.0 - a.0) * t, a.1 + (b.1 - a.1) * t)
}
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn varying_pitch_does_not_step_the_signal_at_cutoff_boundaries() {
        let kernel = Kernel::default();
        for &boundary in &kernel.steps[..BANDS - 1] {
            let clip = Clip { sample_rate: 48000, channels: 1,
                samples: (0..512).map(|i| ((i as f64 * 0.4 / boundary * std::f64::consts::TAU).sin() * 16000.0) as i16).collect() };
            for pos in 32..160 {
                let a = kernel.frame(&clip, pos as f64 + 0.37, boundary * (1.0 - 1e-8), false).0;
                let b = kernel.frame(&clip, pos as f64 + 0.37, boundary * (1.0 + 1e-8), false).0;
                assert!((a - b).abs() < 0.0001, "cutoff boundary {boundary}: {a} -> {b}");
            }
        }
    }

    // Scalar reference for endpoint and loop-seam behavior.
    fn reference(kernel: &Kernel, clip: &Clip, pos: f64, step: f64, looping: bool) -> (f32, f32) {
        let frames = clip.frames();
        if frames == 0 || clip.channels == 0 { return (0.0, 0.0); }
        let seam = Seam::new(clip);
        let lower = ((step.max(1.0).log2() * 4.0).floor() as usize).min(BANDS - 1);
        let upper = if step <= 1.0 { lower } else { (lower + 1).min(BANDS - 1) };
        let blend = if lower == upper { 0.0 } else {
            ((step - kernel.steps[lower]) / (kernel.steps[lower] * BAND_TRANSITION)).min(1.0) as f32
        };
        let phase = pos.fract() * PHASES as f64;
        let row = (phase as usize).min(PHASES - 1);
        let fraction = (phase - row as f64) as f32;
        let mut out = [0.0; 2];
        for tap in 0..TAPS {
            let index = pos.floor() as i64 - (TAPS / 2 - 1) as i64 + tap as i64;
            let index = if looping { index.rem_euclid(frames as i64) }
                else { index.clamp(0, frames as i64 - 1) } as usize;
            let a = kernel.weights[lower * (PHASES + 1) + row][tap];
            let b = kernel.weights[lower * (PHASES + 1) + row + 1][tap];
            let c = kernel.weights[upper * (PHASES + 1) + row][tap];
            let d = kernel.weights[upper * (PHASES + 1) + row + 1][tap];
            let first = a + (b - a) * fraction;
            let second = c + (d - c) * fraction;
            let weight = first + (second - first) * blend;
            for (c, output) in out.iter_mut().enumerate() {
                let channel = c.min(clip.channels as usize - 1);
                let mut sample = clip.samples[index * clip.channels as usize + channel] as f32 / 32768.0;
                if looping && seam.span > 1 && index >= seam.start {
                    sample -= seam.delta[c] * ((index - seam.start) as f32 / (seam.span - 1) as f32);
                }
                *output += sample * weight;
            }
        }
        (out[0], out[1])
    }

    #[test]
    fn contiguous_windows_and_boundaries_match_scalar_convolution() {
        let kernel = Kernel::default();
        for channels in [1, 2, 4] {
            for frames in [0, 1, 5, 31, 32, 33, 64, 480] {
                let clip = Clip { sample_rate: 48000, channels,
                    samples: (0..frames * channels as usize)
                        .map(|i| ((i * 7919 + 12345) % 65536) as i32 as i16).collect() };
                for looping in [false, true] {
                    for step in [0.0001, 0.91875, 1.0, 1.0005, 1.001, 1.37, 2.0, 2.0005, 2.002, 64.0] {
                        for i in 0..frames + 32 {
                            let pos = i as f64 + 0.371;
                            let got = kernel.frame(&clip, pos, step, looping);
                            let expected = reference(&kernel, &clip, pos, step, looping);
                            assert!((got.0 - expected.0).abs() < 1e-6 && (got.1 - expected.1).abs() < 1e-6,
                                "channels={channels}, frames={frames}, loop={looping}, step={step}, pos={pos}: {got:?} != {expected:?}");
                        }
                    }
                }
            }
        }
    }
    #[test]
    fn rejects_aliases_and_preserves_passband() {
        let kernel = Kernel::default();
        let energy = |frequency: f64| {
            let clip = Clip { sample_rate: 48000, channels: 1,
                samples: (0..4096).map(|i| ((i as f64 * frequency * std::f64::consts::TAU).sin() * 16000.0) as i16).collect() };
            (32..2000).map(|i| kernel.frame(&clip, i as f64 * 2.0, 2.0, false).0.powi(2)).sum::<f32>() / 1968.0
        };
        assert!(energy(0.1) > 0.10);
        assert!(energy(0.4) < energy(0.1) * 0.001);
    }
    #[test]
    fn cutoff_transitions_keep_the_existing_alias_rejection() {
        let kernel = Kernel::default();
        let energy = |frequency: f64, step: f64| {
            let clip = Clip { sample_rate: 48000, channels: 1,
                samples: (0..8192).map(|i| ((i as f64 * frequency * std::f64::consts::TAU).sin() * 16000.0) as i16).collect() };
            (32..2000).map(|i| kernel.frame(&clip, i as f64 * step, step, false).0.powi(2)).sum::<f32>() / 1968.0
        };
        for step in [2.0, 2.00001, 2.0005, 2.001, 2.002] {
            assert!(energy(0.1, step) > 0.10);
            assert!(energy(0.4, step) < energy(0.1, step) * 0.001, "alias rejection at step {step}");
        }
    }
    #[test]
    fn wrapped_constant_has_no_gap_at_fractional_pitch() {
        let kernel = Kernel::default();
        let clip = Clip { sample_rate: 48000, channels: 1, samples: vec![16384; 5] };
        for i in 0..100 { assert!((kernel.frame(&clip, i as f64 * 1.37, 1.37, true).0 - 0.5).abs() < 1e-5); }
    }
    #[test]
    fn fractional_positions_do_not_quantize_to_the_same_sample() {
        let kernel = Kernel::default();
        let clip = Clip { sample_rate: 44100, channels: 1,
            samples: (0..512).map(|i| ((i as f64 * 0.03 * std::f64::consts::TAU).sin() * 16000.0) as i16).collect() };
        let a = kernel.frame(&clip, 100.0001, 0.91875, false).0;
        let b = kernel.frame(&clip, 100.0002, 0.91875, false).0;
        assert!((b - a).abs() > 1e-6, "fractional timing must not form a staircase");
        assert!((b - a).abs() < 0.0001);
    }

}
