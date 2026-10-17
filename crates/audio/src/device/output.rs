//! Opening and re-opening the CPAL output stream. The render callback is handed in by the
//! engine, so this module names neither the mixer state nor the OMSI runtime. The device's
//! state is explicit (see [`DeviceState`]): a lost stream or a changed default is a defined
//! transition, and the engine's `follow_device` drives the reopen.

use crate::device::state::{DeviceState, OutputErrors, OutputFormat};
use cpal::traits::{DeviceTrait, HostTrait, StreamTrait};
use std::cell::{Cell, RefCell};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::Instant;

/// The stream on the device played on now, what it is called, and when it was last opened.
pub struct DeviceOutput {
    stream: RefCell<Option<cpal::Stream>>,
    name: RefCell<String>,
    /// Set when the stream fails (its device went away) or the system's default output
    /// changed (a watcher thread looks every two seconds).
    reopen: Arc<AtomicBool>,
    /// Set when a stream error was a lost device (not just a hiccup), so the engine can tell
    /// a lost device from a changed default.
    lost: Arc<AtomicBool>,
    errors: Arc<OutputErrors>,
    /// When the stream was last opened (at most one new stream a second).
    opened: Cell<Instant>,
    /// The rate and channels of the open stream, read by the mixer.
    format: Arc<OutputFormat>,
    state: Cell<DeviceState>,
}

impl DeviceOutput {
    pub fn new(format: Arc<OutputFormat>, now: Instant) -> DeviceOutput {
        DeviceOutput {
            stream: RefCell::new(None),
            name: RefCell::new(String::new()),
            reopen: Arc::new(AtomicBool::new(false)),
            lost: Arc::new(AtomicBool::new(false)),
            errors: Arc::new(OutputErrors::default()),
            opened: Cell::new(now),
            format,
            state: Cell::new(DeviceState::NoDevice),
        }
    }

    /// The flag the watcher sets when the default output device changed.
    pub fn reopen_flag(&self) -> Arc<AtomicBool> {
        self.reopen.clone()
    }

    /// The flag the stream error callback sets when the device went away.
    pub fn lost_flag(&self) -> Arc<AtomicBool> {
        self.lost.clone()
    }

    pub(crate) fn error_counts(&self) -> (u64, u64) {
        (self.errors.xruns.load(Ordering::Relaxed), self.errors.realtime_denials.load(Ordering::Relaxed))
    }

    /// The rate/channels bridge the mixer reads.
    pub fn format(&self) -> Arc<OutputFormat> {
        self.format.clone()
    }

    pub fn state(&self) -> DeviceState {
        if self.lost.load(Ordering::Relaxed) { DeviceState::Lost }
        else { self.state.get() }
    }

    /// Record that the device went away (before a reopen attempt).
    pub fn note_lost(&self) {
        self.state.set(self.state.get().lost());
    }

    /// Record that the default output changed (before a reopen attempt).
    pub fn note_default_changed(&self) {
        self.state.set(self.state.get().retrying());
    }

    pub fn name(&self) -> String {
        self.name.borrow().clone()
    }

    pub fn is_open(&self) -> bool {
        self.stream.borrow().is_some()
    }

    pub fn opened_age(&self) -> f32 {
        self.opened.get().elapsed().as_secs_f32()
    }

    pub fn mark_opened(&self, now: Instant) {
        self.opened.set(now);
    }

    pub fn clear_name(&self) {
        self.name.borrow_mut().clear();
    }

    /// Open the default output device from now on, feeding it `render`. Returns whether a
    /// stream is playing; the state machine follows along (`Opening` -> `Open`/`NoDevice`/`Lost`).
    pub fn close(&self) { self.stream.borrow_mut().take(); }

    pub fn open<F>(&self, render: F) -> bool
    where F: FnMut(&mut [f32]) + Send + 'static {
        let mut render = Some(render);
        self.open_prepared(None, move || render.take().unwrap())
    }

    /// Prepare outside callbacks; retry rejected formats.
    /// `Some(rate)`: stereo before limiting. `None`: one-shot native renderer.
    pub(crate) fn open_prepared<P, F>(&self, source_rate: Option<u32>, mut prepare: P) -> bool
    where P: FnMut() -> F, F: FnMut(&mut [f32]) + Send + 'static {
        let previous = self.state();
        self.close();
        // The old callback is stopped. Clear its flags before creating the next stream;
        // an error from the new stream must remain visible even during play().
        self.lost.store(false, Ordering::Relaxed);
        self.reopen.store(false, Ordering::Relaxed);
        self.state.set(previous.opening());
        let host = cpal::default_host();
        let Some(dev) = host.default_output_device() else {
            self.state.set(previous.open_failed()); self.clear_name(); return false;
        };
        let name = dev.description().map(|d| d.name().to_string()).unwrap_or_default();
        let default = dev.default_output_config().ok().filter(suitable);
        let needs_negotiation = default.as_ref().is_none_or(|c|
            source_rate.is_some_and(|rate| rate != c.sample_rate()));
        let configs = if needs_negotiation {
            match dev.supported_output_configs() {
                Ok(configs) => configs.collect::<Vec<_>>(),
                Err(error) => { log::warn!("audio: cannot enumerate formats on {name}: {error}"); Vec::new() }
            }
        } else {
            Vec::new()
        };
        let candidates = output_candidates(default, configs.into_iter(), source_rate);
        if candidates.is_empty() { log::warn!("audio: no suitable output format on {name}"); }
        for cfg in candidates {
            self.lost.store(false, Ordering::Relaxed);
            self.reopen.store(false, Ordering::Relaxed);
            self.format.set(cfg.sample_rate(), cfg.channels() as usize);
            let render = prepare();
            let config = cfg.config();
            let reopen = self.reopen.clone(); let lost = self.lost.clone();
            macro_rules! build {
                ($sample:ty) => { super::convert::build::<$sample, _>(&dev, config, render, reopen, lost,
                    self.errors.clone(), source_rate) };
            }
            use cpal::SampleFormat as S;
            let stream = match cfg.sample_format() {
                S::F32 => build!(f32), S::F64 => build!(f64),
                S::I8 => build!(i8), S::I16 => build!(i16), S::I32 => build!(i32), S::I64 => build!(i64),
                S::U8 => build!(u8), S::U16 => build!(u16), S::U32 => build!(u32), S::U64 => build!(u64),
                _ => unreachable!("format selection checked the sample format"),
            };
            if let Ok(stream) = stream {
                if let Err(error) = stream.play() {
                    log::warn!("audio: cannot start {} Hz {:?} stream on {name}: {error}", cfg.sample_rate(), cfg.sample_format());
                    continue;
                }
                log::info!("audio: playing on {name} ({} Hz, {} channels, {:?})",
                    cfg.sample_rate(), cfg.channels(), cfg.sample_format());
                *self.stream.borrow_mut() = Some(stream); *self.name.borrow_mut() = name;
                self.state.set(DeviceState::Open); return true;
            } else if let Err(error) = stream {
                log::warn!("audio: cannot open {} Hz {:?} stream on {name}: {error}", cfg.sample_rate(), cfg.sample_format());
            }
        }
        self.state.set(previous.open_failed()); self.clear_name(); false
    }
}

fn supported(format: cpal::SampleFormat) -> bool {
    use cpal::SampleFormat as S;
    matches!(format, S::F32 | S::F64 | S::I8 | S::I16 | S::I32 | S::I64 | S::U8 | S::U16 | S::U32 | S::U64)
}
fn suitable(config: &cpal::SupportedStreamConfig) -> bool {
    (1..=8).contains(&config.channels()) && config.sample_rate() >= 8000
        && supported(config.sample_format())
}

// Try mixer rate, then 44.1 kHz, then the native default.
fn output_candidates(
    default: Option<cpal::SupportedStreamConfig>,
    configs: impl Iterator<Item = cpal::SupportedStreamConfigRange>,
    source_rate: Option<u32>,
) -> Vec<cpal::SupportedStreamConfig> {
    if let Some(default) = &default {
        if source_rate.is_none_or(|rate| rate == default.sample_rate()) {
            return vec![*default];
        }
    }
    let preferred_rate = source_rate.unwrap_or(48000);
    let layout = default.as_ref().map(|c| c.channels());
    let format = default
        .as_ref()
        .map(|c| c.sample_format())
        .unwrap_or(cpal::SampleFormat::F32);
    let mut candidates: Vec<_> = configs
        .filter(|c| match layout {
            Some(channels) => c.channels() == channels,
            None => (1..=2).contains(&c.channels()),
        })
        .filter(|c| supported(c.sample_format()))
        .flat_map(|c| {
            [preferred_rate, 44100]
                .into_iter()
                .filter_map(move |rate| c.try_with_sample_rate(rate))
        })
        .collect();
    if source_rate.is_none() {
        // Native renderers prepare once.
        return candidates
            .into_iter()
            .max_by_key(|c| {
                (
                    c.channels(),
                    c.sample_rate() == preferred_rate,
                    c.sample_format() == cpal::SampleFormat::F32,
                )
            })
            .into_iter()
            .collect();
    }
    candidates.sort_by_key(|c| {
        (
            c.sample_rate() != preferred_rate,
            std::cmp::Reverse(c.channels()),
            c.sample_format() != format,
            c.sample_format() != cpal::SampleFormat::F32,
            c.sample_format(),
        )
    });
    candidates.dedup_by(|a, b| {
        a.sample_rate() == b.sample_rate()
            && a.channels() == b.channels()
            && a.sample_format() == b.sample_format()
    });
    if let Some(default) = default {
        if !candidates.iter().any(|c| {
            c.sample_rate() == default.sample_rate()
                && c.channels() == default.channels()
                && c.sample_format() == default.sample_format()
        }) {
            candidates.push(default);
        }
    }
    candidates
}

#[cfg(test)]
mod tests {
    use super::*;
    use cpal::{
        SampleFormat, SupportedBufferSize, SupportedStreamConfig, SupportedStreamConfigRange,
    };
    fn range(channels: u16, rate: u32, format: SampleFormat) -> SupportedStreamConfigRange {
        SupportedStreamConfigRange::new(channels, rate, rate, SupportedBufferSize::Unknown, format)
    }
    fn default(channels: u16, rate: u32) -> SupportedStreamConfig {
        SupportedStreamConfig::new(
            channels,
            rate,
            SupportedBufferSize::Unknown,
            SampleFormat::F32,
        )
    }
    #[test]
    fn high_rate_devices_use_a_supported_client_rate_without_changing_layout() {
        for channels in [1, 2, 8] {
            for rate in [32000, 96000, 192000, 352800, 384000] {
                let selected = output_candidates(
                    Some(default(channels, rate)),
                    [
                        range(channels, rate, SampleFormat::F32),
                        range(channels, 48000, SampleFormat::F32),
                    ]
                    .into_iter(),
                    Some(48000),
                )
                .remove(0);
                assert_eq!(selected.sample_rate(), 48000);
                assert_eq!(selected.channels(), channels);
            }
        }
    }
    #[test]
    fn negotiation_prefers_the_mix_rate_then_the_device_format() {
        let selected = output_candidates(
            Some(default(2, 96000)),
            [
                range(2, 48000, SampleFormat::I16),
                range(2, 44100, SampleFormat::F32),
            ]
            .into_iter(),
            Some(48000),
        )
        .remove(0);
        assert_eq!(selected.sample_rate(), 48000);
        assert_eq!(selected.sample_format(), SampleFormat::I16);
        let selected = output_candidates(
            Some(default(2, 96000)),
            [
                range(2, 44100, SampleFormat::I16),
                range(2, 44100, SampleFormat::F32),
            ]
            .into_iter(),
            Some(48000),
        )
        .remove(0);
        assert_eq!(selected.sample_rate(), 44100);
        assert_eq!(selected.sample_format(), SampleFormat::F32);
    }
    #[test]
    fn native_only_devices_retain_their_supported_rate_and_layout() {
        for rate in [8000, 32000, 352800, 768000] {
            let candidates = output_candidates(
                Some(default(8, rate)),
                [range(2, 48000, SampleFormat::F32)].into_iter(),
                Some(48000),
            );
            assert_eq!(candidates.len(), 1);
            assert_eq!(candidates[0].sample_rate(), rate);
            assert_eq!(candidates[0].channels(), 8);
        }
        assert!(output_candidates(None, std::iter::empty(), Some(48000)).is_empty());
    }
    #[test]
    fn native_renderers_keep_the_device_rate_and_are_prepared_once() {
        for rate in [8000, 32000, 44100, 48000, 352800, 768000] {
            let candidates = output_candidates(
                Some(default(2, rate)),
                [range(2, 48000, SampleFormat::F32)].into_iter(),
                None,
            );
            assert_eq!(candidates.len(), 1);
            assert_eq!(candidates[0].sample_rate(), rate);
        }
        let candidates = output_candidates(
            None,
            [
                range(1, 48000, SampleFormat::F32),
                range(2, 44100, SampleFormat::I16),
                range(2, 44100, SampleFormat::F32),
            ]
            .into_iter(),
            None,
        );
        assert_eq!(candidates.len(), 1);
        assert_eq!(candidates[0].channels(), 2);
        assert_eq!(candidates[0].sample_rate(), 44100);
        assert_eq!(candidates[0].sample_format(), SampleFormat::F32);
    }
    #[test]
    fn preferred_format_plan_retains_a_distinct_native_fallback() {
        let candidates = output_candidates(
            Some(default(2, 192000)),
            [
                range(2, 44100, SampleFormat::F32),
                range(2, 48000, SampleFormat::I16),
                range(2, 48000, SampleFormat::F32),
                range(2, 48000, SampleFormat::F32),
                range(2, 192000, SampleFormat::F32),
            ]
            .into_iter(),
            Some(48000),
        );
        assert_eq!(
            candidates
                .iter()
                .map(|c| (c.sample_rate(), c.sample_format()))
                .collect::<Vec<_>>(),
            [
                (48000, SampleFormat::F32),
                (48000, SampleFormat::I16),
                (44100, SampleFormat::F32),
                (192000, SampleFormat::F32)
            ]
        );
        assert!(candidates.iter().all(|c| c.channels() == 2));
        let candidates = output_candidates(
            Some(default(2, 48000)),
            [
                range(2, 48000, SampleFormat::I16),
                range(2, 44100, SampleFormat::F32),
            ]
            .into_iter(),
            Some(48000),
        );
        assert_eq!(candidates.len(), 1);
        assert_eq!(candidates[0].sample_rate(), 48000);
        assert_eq!(candidates[0].sample_format(), SampleFormat::F32);
    }

    #[test]
    fn native_sample_format_precedes_float_and_other_formats_in_each_rate_group() {
        let native =
            SupportedStreamConfig::new(8, 192000, SupportedBufferSize::Unknown, SampleFormat::I32);
        let configs = [44100, 48000].into_iter().flat_map(|rate| {
            [
                SampleFormat::U16,
                SampleFormat::I16,
                SampleFormat::F32,
                SampleFormat::I32,
            ]
            .into_iter()
            .map(move |format| range(8, rate, format))
        });
        let expected = [48000, 44100]
            .into_iter()
            .flat_map(|rate| {
                [
                    SampleFormat::I32,
                    SampleFormat::F32,
                    SampleFormat::I16,
                    SampleFormat::U16,
                ]
                .into_iter()
                .map(move |format| (rate, format))
            })
            .chain([(192000, SampleFormat::I32)])
            .collect::<Vec<_>>();
        let candidates = output_candidates(Some(native), configs, Some(48000));
        assert_eq!(
            candidates
                .iter()
                .map(|c| (c.sample_rate(), c.sample_format()))
                .collect::<Vec<_>>(),
            expected
        );
        assert!(candidates.iter().all(|c| c.channels() == 8));
    }

    #[test]
    fn overlapping_rate_ranges_and_the_native_default_do_not_duplicate_attempts() {
        let candidates = output_candidates(
            Some(default(2, 44100)),
            [
                SupportedStreamConfigRange::new(
                    2,
                    44100,
                    48000,
                    SupportedBufferSize::Unknown,
                    SampleFormat::F32,
                ),
                range(2, 44100, SampleFormat::F32),
                range(2, 48000, SampleFormat::F32),
            ]
            .into_iter(),
            Some(48000),
        );
        assert_eq!(
            candidates
                .iter()
                .map(|c| c.sample_rate())
                .collect::<Vec<_>>(),
            [48000, 44100]
        );
    }
}
