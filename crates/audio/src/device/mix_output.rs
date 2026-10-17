//! Fixed-rate stereo output conversion with preallocated buffers and tables.
//! Integer phase keeps callbacks continuous and drift-free.
use crate::dsp::{
    limiter::Limiter,
    resample::{PHASES, fill_sinc_row},
};

const BLOCK_FRAMES: usize = 256;
const BASE_TAPS: usize = 64;

pub(crate) struct MixOutput {
    rate: u32,
    channels: usize,
    stereo: [f32; BLOCK_FRAMES * 2],
    resampler: Option<OutputResampler>,
    limiter: Limiter,
}

impl MixOutput {
    pub(crate) fn new(source_rate: u32, rate: u32, channels: usize) -> Self {
        Self {
            rate,
            channels,
            stereo: [0.0; BLOCK_FRAMES * 2],
            resampler: (source_rate != rate).then(|| OutputResampler::new(source_rate, rate)),
            limiter: Limiter::default(),
        }
    }

    pub(crate) fn render<T: cpal::Sample + cpal::FromSample<f32>>(
        &mut self,
        out: &mut [T],
        render: &mut impl FnMut(&mut [f32]),
    ) {
        for block in out.chunks_mut(BLOCK_FRAMES * self.channels) {
            let frames = block.len() / self.channels;
            let stereo = &mut self.stereo[..frames * 2];
            if let Some(resampler) = &mut self.resampler {
                resampler.render(stereo, render);
            } else {
                render(stereo);
            }
            // Limit reconstruction peaks before sample conversion.
            self.limiter.process(stereo, 2, self.rate);
            for (source, target) in stereo
                .chunks_exact(2)
                .zip(block.chunks_exact_mut(self.channels))
            {
                target.fill(T::EQUILIBRIUM);
                if self.channels == 1 {
                    target[0] = T::from_sample((source[0] + source[1]) * 0.5);
                } else {
                    target[0] = T::from_sample(source[0]);
                    target[1] = T::from_sample(source[1]);
                }
            }
        }
    }
}

struct OutputResampler {
    source_rate: u64,
    rate: u64,
    cursor: u64,
    fraction: u64,
    written: u64,
    taps: usize,
    weights: Box<[f32]>,
    history: Box<[[f32; 2]]>,
    source: [f32; BLOCK_FRAMES * 2],
}

impl OutputResampler {
    fn new(source_rate: u32, rate: u32) -> Self {
        let step = source_rate as f64 / rate as f64;
        // Longer filters preserve downsampling quality and bounded work/sec.
        let taps = ((BASE_TAPS as f64 * step.max(1.0)).ceil() as usize).div_ceil(8) * 8;
        let cutoff = 0.98 / step.max(1.0);
        let mut weights = vec![0.0; (PHASES + 1) * taps];
        for (phase, row) in weights.chunks_exact_mut(taps).enumerate() {
            fill_sinc_row(row, cutoff, phase as f64 / PHASES as f64);
        }
        Self {
            source_rate: source_rate as u64,
            rate: rate as u64,
            cursor: 0,
            fraction: 0,
            written: 0,
            taps,
            weights: weights.into_boxed_slice(),
            history: vec![[0.0; 2]; taps + BLOCK_FRAMES].into_boxed_slice(),
            source: [0.0; BLOCK_FRAMES * 2],
        }
    }

    fn render(&mut self, out: &mut [f32], render: &mut impl FnMut(&mut [f32])) {
        let frames = out.len() / 2;
        if frames == 0 {
            return;
        }
        let left = self.taps / 2 - 1;
        let last_cursor =
            self.cursor + (self.fraction + (frames - 1) as u64 * self.source_rate) / self.rate;
        let end = last_cursor + (self.taps - left) as u64;
        for frame in out.chunks_exact_mut(2) {
            let required = self.cursor + (self.taps - left) as u64;
            while self.written < required {
                // Render only needed input plus lookahead, in bounded batches.
                let count = (end - self.written).min(BLOCK_FRAMES as u64) as usize;
                let source = &mut self.source[..count * 2];
                render(source);
                for source in source.chunks_exact(2) {
                    let slot = (self.written % self.history.len() as u64) as usize;
                    self.history[slot] = [source[0], source[1]];
                    self.written += 1;
                }
            }
            let phase = self.fraction as f64 * PHASES as f64 / self.rate as f64;
            let row = phase as usize;
            let fraction = (phase - row as f64) as f32;
            let weights = &self.weights[row * self.taps..(row + 1) * self.taps];
            let next = &self.weights[(row + 1) * self.taps..(row + 2) * self.taps];
            let mut sum = [[0.0; 4]; 2];
            for tap in (0..self.taps).step_by(4) {
                for lane in 0..4 {
                    let i = tap + lane;
                    let index = if i < left {
                        self.cursor.saturating_sub((left - i) as u64)
                    } else {
                        self.cursor + (i - left) as u64
                    };
                    let source = self.history[(index % self.history.len() as u64) as usize];
                    let weight = weights[i] + (next[i] - weights[i]) * fraction;
                    sum[0][lane] += source[0] * weight;
                    sum[1][lane] += source[1] * weight;
                }
            }
            frame[0] = sum[0].into_iter().sum();
            frame[1] = sum[1].into_iter().sum();
            self.fraction += self.source_rate;
            self.cursor += self.fraction / self.rate;
            self.fraction %= self.rate;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::{
        alloc::{GlobalAlloc, Layout, System},
        cell::Cell,
    };
    thread_local! { static ALLOCATIONS: Cell<Option<usize>> = const { Cell::new(None) }; }
    struct TrackingAllocator;
    fn track() {
        let _ = ALLOCATIONS.try_with(|count| {
            if let Some(value) = count.get() {
                count.set(Some(value + 1));
            }
        });
    }
    // System owns allocations; count only this test thread's calls.
    unsafe impl GlobalAlloc for TrackingAllocator {
        unsafe fn alloc(&self, layout: Layout) -> *mut u8 {
            track();
            unsafe { System.alloc(layout) }
        }
        unsafe fn alloc_zeroed(&self, layout: Layout) -> *mut u8 {
            track();
            unsafe { System.alloc_zeroed(layout) }
        }
        unsafe fn realloc(&self, ptr: *mut u8, layout: Layout, size: usize) -> *mut u8 {
            track();
            unsafe { System.realloc(ptr, layout, size) }
        }
        unsafe fn dealloc(&self, ptr: *mut u8, layout: Layout) {
            unsafe { System.dealloc(ptr, layout) }
        }
    }
    #[global_allocator]
    static ALLOCATOR: TrackingAllocator = TrackingAllocator;
    fn oscillator(rate: u32, hz: f64) -> impl FnMut(&mut [f32]) {
        let mut frame = 0u64;
        move |out| {
            for sample in out.chunks_exact_mut(2) {
                let value =
                    (frame as f64 * hz * std::f64::consts::TAU / rate as f64).sin() as f32 * 0.25;
                sample[0] = value;
                sample[1] = value * 0.5;
                frame += 1;
            }
        }
    }
    #[test]
    fn output_is_continuous_across_arbitrary_callback_partitions() {
        for rate in [8000, 44100, 48000, 96000, 192000, 384000] {
            let mut whole = MixOutput::new(48000, rate, 2);
            let mut divided = MixOutput::new(48000, rate, 2);
            let mut a = vec![0.0f32; 8192];
            let mut b = vec![0.0f32; 8192];
            whole.render(&mut a, &mut oscillator(48000, 1000.0));
            let mut source = oscillator(48000, 1000.0);
            for chunk in b.chunks_mut(74) {
                divided.render(chunk, &mut source);
            }
            assert_eq!(a, b, "output rate {rate}");
        }
    }
    #[test]
    fn rational_rate_position_does_not_drift() {
        let mut resampler = OutputResampler::new(48000, 44100);
        resampler.render(&mut vec![0.0; 44100 * 2], &mut |out| out.fill(0.0));
        assert_eq!(resampler.cursor, 48000);
        assert_eq!(resampler.fraction, 0);
        assert!(resampler.written - resampler.cursor <= (resampler.taps / 2 + BLOCK_FRAMES) as u64);
    }
    #[test]
    fn source_refills_follow_callback_demand_without_a_full_block_burst() {
        let mut output = OutputResampler::new(48000, 192000);
        let mut requests = Vec::new();
        for _ in 0..4 {
            output.render(&mut [0.0; 512], &mut |source| {
                requests.push(source.len() / 2);
                source.fill(0.0);
            });
        }
        assert_eq!(requests, [96, 64, 64, 64]);
    }
    #[test]
    fn refills_are_bounded_and_do_not_process_unused_future_audio() {
        for rate in [8000, 44100, 96000, 192000, 384000] {
            let mut output = OutputResampler::new(48000, rate);
            for frames in [1, 64, 256, 1024, 37, 512] {
                let end = output.cursor
                    + (output.fraction + (frames - 1) as u64 * 48000) / rate as u64
                    + (output.taps / 2 + 1) as u64;
                output.render(&mut vec![0.0; frames * 2], &mut |source| {
                    assert!(source.len() <= BLOCK_FRAMES * 2);
                    source.fill(0.0);
                });
                assert_eq!(output.written, end, "{rate} Hz, {frames} frames");
            }
        }
    }
    #[test]
    fn audible_passband_survives_output_conversion() {
        for rate in [44100, 96000, 192000] {
            for hz in [1000.0, 10000.0, 20000.0] {
                let mut output = MixOutput::new(48000, rate, 2);
                let mut samples = vec![0.0f32; rate as usize / 5 * 2];
                output.render(&mut samples, &mut oscillator(48000, hz));
                let samples = &samples[1024..];
                let rms = (samples
                    .chunks_exact(2)
                    .map(|s| s[0] as f64 * s[0] as f64)
                    .sum::<f64>()
                    / (samples.len() / 2) as f64)
                    .sqrt();
                assert!(
                    (rms - 0.25 / 2.0f64.sqrt()).abs() < 0.005,
                    "{rate} Hz, tone {hz}: {rms}"
                );
            }
        }
    }
    #[test]
    fn low_rate_outputs_reject_folded_high_frequency_tones() {
        let mut output = MixOutput::new(48000, 8000, 2);
        let mut samples = vec![0.0f32; 8000 * 2];
        output.render(&mut samples, &mut oscillator(48000, 5200.0));
        let rms = (samples[1024..].iter().map(|s| s * s).sum::<f32>()
            / (samples.len() - 1024) as f32)
            .sqrt();
        assert!(rms < 0.0001, "folded tone RMS {rms}");
    }
    #[test]
    fn channel_layout_and_unsigned_silence_are_preserved() {
        for channels in [1, 2, 8] {
            let mut output = MixOutput::new(48000, 192000, channels);
            let mut samples = vec![0u16; 500 * channels];
            output.render(&mut samples, &mut |out| out.fill(0.0));
            assert!(samples.iter().all(|s| *s == 32768));
        }
    }
    #[test]
    fn limiter_runs_after_resampling() {
        let mut output = MixOutput::new(48000, 192000, 2);
        let mut samples = vec![0.0f32; 4096];
        output.render(&mut samples, &mut |out| out.fill(100.0));
        assert!(samples.iter().all(|s| s.abs() <= 0.900001));
    }

    #[test]
    fn callback_conversion_does_not_allocate_in_steady_state() {
        for rate in [8000, 44100, 48000, 192000, 384000] {
            let mut output = MixOutput::new(48000, rate, 8);
            let mut samples = vec![0.0f32; 8192];
            let mut source = oscillator(48000, 1000.0);
            output.render(&mut samples, &mut source);
            ALLOCATIONS.with(|count| count.set(Some(0)));
            for _ in 0..20 {
                output.render(&mut samples, &mut source);
            }
            let allocations = ALLOCATIONS.with(|count| count.replace(None).unwrap());
            assert_eq!(allocations, 0, "output rate {rate}");
        }
    }

    #[test]
    fn fixed_rate_mix_preserves_audible_gain_relative_to_96khz_processing() {
        use crate::device::OutputFormat;
        use crate::engine::{
            commands::{Command, CommandQueue},
            feedback::{Counters, Reaper},
            mixer::AudioCore,
        };
        use crate::{Clip, VoiceParams};
        use std::sync::Arc;
        for hz in [1000.0, 10000.0, 20000.0] {
            let clip = Arc::new(Clip {
                sample_rate: 48000,
                channels: 1,
                samples: (0..48000)
                    .map(|i| {
                        ((i as f64 * hz * std::f64::consts::TAU / 48000.0).sin() * 16000.0) as i16
                    })
                    .collect(),
            });
            let rms = |mix_rate| {
                let counters = Arc::new(Counters::default());
                let queue = Arc::new(CommandQueue::new(counters.clone()));
                let mut core = AudioCore::new(
                    Arc::new(OutputFormat::new(mix_rate, 2)),
                    queue.clone(),
                    Arc::new(Reaper::new()),
                    counters,
                    false,
                );
                queue.push(Command::Play {
                    id: 1,
                    clip: clip.clone(),
                    params: VoiceParams {
                        gain: 0.1,
                        looping: true,
                        ..Default::default()
                    }
                    .into(),
                });
                let mut output = MixOutput::new(mix_rate, 192000, 2);
                let mut data = vec![0.0f32; 38400];
                output.render(&mut data, &mut |out| core.render_mix(out));
                let data = &data[19200..];
                (data
                    .chunks_exact(2)
                    .map(|v| v[0] as f64 * v[0] as f64)
                    .sum::<f64>()
                    / (data.len() / 2) as f64)
                    .sqrt()
            };
            let relative_db = 20.0 * (rms(48000) / rms(96000)).log10();
            assert!(
                relative_db.abs() < 0.1,
                "audible gain difference at {hz}: {relative_db} dB"
            );
        }
    }

    #[test]
    fn front_channels_and_mono_average_survive_rate_conversion() {
        for channels in [1, 2, 8] {
            let mut output = MixOutput::new(48000, 192000, channels);
            let mut samples = vec![0.0f32; 500 * channels];
            output.render(&mut samples, &mut |out| {
                for frame in out.chunks_exact_mut(2) {
                    frame.copy_from_slice(&[0.25, -0.5]);
                }
            });
            for frame in samples.chunks_exact(channels) {
                if channels == 1 {
                    assert!((frame[0] + 0.125).abs() < 1e-6);
                } else {
                    assert!((frame[0] - 0.25).abs() < 1e-6);
                    assert!((frame[1] + 0.5).abs() < 1e-6);
                    assert!(frame[2..].iter().all(|sample| *sample == 0.0));
                }
            }
        }
    }
}
