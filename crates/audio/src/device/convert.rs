//! Typed CPAL conversion around the same f32 renderer used offline. Scratch is bounded
//! and allocated when opening the device, never by its callback. Chunk boundaries align
//! with device frames; signed/unsigned formats use CPAL's sample conversion semantics.
use cpal::traits::DeviceTrait;
use super::state::OutputErrors;
use super::mix_output::MixOutput;
use std::sync::{Arc, atomic::{AtomicBool, Ordering}};
const SCRATCH_SAMPLES: usize = 4096;

pub(super) fn build<T, F>(device: &cpal::Device, config: cpal::StreamConfig, mut render: F,
    reopen: Arc<AtomicBool>, lost: Arc<AtomicBool>, errors: Arc<OutputErrors>,
    source_rate: Option<u32>) -> Result<cpal::Stream, cpal::Error>
where T: cpal::SizedSample + cpal::FromSample<f32>, F: FnMut(&mut [f32]) + Send + 'static {
    let mut scratch = if source_rate.is_none() { vec![0.0f32; SCRATCH_SAMPLES].into_boxed_slice() }
        else { Box::default() };
    let channels = config.channels.max(1) as usize;
    let rate = config.sample_rate;
    let mut mixed = source_rate.map(|source| MixOutput::new(source, rate, channels));
    let chunk_size = SCRATCH_SAMPLES / channels * channels;
    device.build_output_stream(config, move |data: &mut [T], _| {
        if let Some(mixed) = &mut mixed { mixed.render(data, &mut render); }
        else {
            for chunk in data.chunks_mut(chunk_size) {
                render(&mut scratch[..chunk.len()]);
                convert(&scratch[..chunk.len()], chunk);
            }
        }
    }, move |error| {
        // Error callbacks must not log or format strings. Retry on the game thread.
        handle_error(error.kind(), &reopen, &lost, &errors);
    }, None)
}

fn handle_error(kind: cpal::ErrorKind, reopen: &AtomicBool, lost: &AtomicBool, errors: &OutputErrors) {
    match kind {
        cpal::ErrorKind::Xrun => { errors.xruns.fetch_add(1, Ordering::Relaxed); }
        cpal::ErrorKind::RealtimeDenied => { errors.realtime_denials.fetch_add(1, Ordering::Relaxed); }
        // CPAL rerouted; the default-device watcher handles renegotiation.
        cpal::ErrorKind::DeviceChanged => {}
        _ => {
            lost.store(true, Ordering::Relaxed);
            reopen.store(true, Ordering::Relaxed);
        }
    }
}

pub fn convert<T: cpal::Sample + cpal::FromSample<f32>>(input: &[f32], out: &mut [T]) {
    for (sample, value) in out.iter_mut().zip(input) {
        let value = if value.is_finite() { value.clamp(-1.0, 1.0) } else { 0.0 };
        *sample = T::from_sample(value);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn recoverable_output_errors_do_not_restart_all_voices() {
        let reopen = AtomicBool::new(false);
        let lost = AtomicBool::new(false);
        let errors = OutputErrors::default();
        for kind in [cpal::ErrorKind::Xrun, cpal::ErrorKind::RealtimeDenied] {
            handle_error(kind, &reopen, &lost, &errors);
            assert!(!reopen.load(Ordering::Relaxed));
            assert!(!lost.load(Ordering::Relaxed));
        }
        assert_eq!(errors.xruns.load(Ordering::Relaxed), 1);
        assert_eq!(errors.realtime_denials.load(Ordering::Relaxed), 1);
        handle_error(cpal::ErrorKind::DeviceChanged, &reopen, &lost, &errors);
        assert!(!reopen.load(Ordering::Relaxed));
        assert!(!lost.load(Ordering::Relaxed));
        handle_error(cpal::ErrorKind::StreamInvalidated, &reopen, &lost, &errors);
        assert!(reopen.load(Ordering::Relaxed));
        assert!(lost.load(Ordering::Relaxed));
    }
    #[test]
    fn signed_unsigned_float_and_invalid_samples() {
        let input = [-1.0, 0.0, 1.0, f32::NAN];
        let mut signed = [0i16; 4]; convert(&input, &mut signed);
        assert_eq!(signed, [i16::MIN, 0, i16::MAX, 0]);
        let mut unsigned = [0u16; 4]; convert(&input, &mut unsigned);
        assert_eq!(unsigned, [0, 32768, 65535, 32768]);
        let mut floats = [0.0f64; 4]; convert(&input, &mut floats);
        assert_eq!(floats, [-1.0, 0.0, 1.0, 0.0]);
    }

    #[test]
    fn all_supported_pcm_widths_clip_and_map_silence_correctly() {
        let input = [-2.0, 0.0, 2.0, f32::INFINITY, f32::NEG_INFINITY];
        macro_rules! signed {
            ($ty:ty) => {{
                let mut out = [0 as $ty; 5];
                convert(&input, &mut out);
                assert_eq!(out, [<$ty>::MIN, 0, <$ty>::MAX, 0, 0]);
            }};
        }
        macro_rules! unsigned {
            ($ty:ty) => {{
                let mut out = [0 as $ty; 5];
                convert(&input, &mut out);
                let silence = <$ty>::MAX / 2 + 1;
                assert_eq!(out, [0, silence, <$ty>::MAX, silence, silence]);
            }};
        }
        signed!(i8); signed!(i16); signed!(i32); signed!(i64);
        unsigned!(u8); unsigned!(u16); unsigned!(u32); unsigned!(u64);
        let mut floats = [0.0f32; 5];
        convert(&input, &mut floats);
        assert_eq!(floats, [-1.0, 0.0, 1.0, 0.0, 0.0]);
    }
}
