//! The audio engine façade: it owns the output stream, the clip cache, the command channel to
//! the audio thread and the game-side playback map, and is the only type the game talks to.
//! The audio thread (or, offline, the caller's thread) owns the [`mixer::AudioCore`] and its
//! voices; the game never reaches into it. The OMSI runtime talks to the engine through
//! [`Playback`] instead of the concrete type (see [`crate::runtime`]).

pub mod bus;
pub mod commands;
pub mod feedback;
mod output;
pub mod mixer;
pub mod playback;

pub use playback::Playback;

use crate::assets::clip::{Clip, ClipCache};
use crate::assets::stream::StreamBuf;
use crate::clock::Clock;
use crate::device::{watch_default_device, DeviceOutput, OutputFormat};
use crate::device::mix_output::MixOutput;
use crate::engine::commands::{Command, CommandQueue};
use crate::engine::feedback::{ActiveVoice, Counters, Reaper, VoiceAsset};
use crate::engine::mixer::AudioCore;
use crate::voice::{Listener, MixParams, VoiceId, VoiceParams};
use hashbrown::HashMap;
use std::cell::{Cell, RefCell};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;
use std::time::Instant;

/// Where the mixer lives: the output callback owns it with a device, the engine owns it
/// offline (and without a device) so `render_offline` can drive the same code synchronously.
enum Mode {
    /// No callback: `render_offline` mixes on the caller's thread.
    Offline { core: RefCell<AudioCore>, output: Option<RefCell<MixOutput>> },
    /// A callback is (or was) playing; the core lives inside the stream's closure. A device
    /// change builds a fresh core and replays the still-active voices (see `replay`).
    Device(DeviceOutput),
}

/// Owns the output stream. Dropping it stops playback.
pub struct AudioEngine {
    mode: Mode,
    commands: Arc<CommandQueue>,
    counters: Arc<Counters>,
    reaper: Arc<Reaper>,
    /// Clips by file (see [`ClipCache`]).
    cache: Arc<ClipCache>,
    /// The voices the game started and still believes are playing. Written and read on the
    /// game thread only; the audio thread reports ends through the reaper.
    active: RefCell<HashMap<VoiceId, ActiveVoice>>,
    /// The listener the game last set, for [`AudioEngine::listener_position`] and the
    /// replies; the audio thread keeps its own copy, fed by `SetListener`.
    listener: Cell<Listener>,
    next_id: AtomicU64,
    bus_gains: Cell<[f32; bus::BUS_COUNT]>,
    /// Logical playback is enabled even without hardware. Inspect `device_state()` for output.
    pub enabled: bool,
    clock: Clock,
}

impl AudioEngine {
    /// Open the default output device. Returns a silent engine if none is available.
    pub fn new() -> AudioEngine {
        let clock = Clock::real();
        let counters = Arc::new(Counters::default());
        let commands = Arc::new(CommandQueue::new(counters.clone()));
        let reaper = Arc::new(Reaper::new());
        let device = DeviceOutput::new(Arc::new(OutputFormat::new(48_000, 2)), clock.now());
        let engine = AudioEngine {
            mode: Mode::Device(device),
            commands,
            counters,
            reaper,
            cache: Arc::new(ClipCache::new(clock.now())),
            active: RefCell::new(HashMap::new()),
            listener: Cell::new(Listener::default()),
            next_id: AtomicU64::new(1),
            bus_gains: Cell::new(bus::DEFAULT_GAINS),
            enabled: true,
            clock,
        };
        engine.open_default();
        if let Some(d) = engine.device() {
            watch_default_device(d.name(), Arc::downgrade(&d.reopen_flag()));
        }
        engine
    }

    /// An engine without an output device, for tests and the offline renderer: playback is
    /// driven and mixed with [`AudioEngine::render_offline`]. It runs on a manual clock, so
    /// the caller moves time and a run is reproducible; [`AudioEngine::enabled`] is true so
    /// sound sets run as usual.
    pub fn new_offline(sample_rate: u32, channels: usize) -> AudioEngine {
        let clock = Clock::manual(Instant::now());
        let counters = Arc::new(Counters::default());
        let commands = Arc::new(CommandQueue::new(counters.clone()));
        let reaper = Arc::new(Reaper::new());
        let format = Arc::new(OutputFormat::new(sample_rate, channels));
        let core = AudioCore::new(
            format,
            commands.clone(),
            reaper.clone(),
            counters.clone(),
            mixer::muted(),
        );
        AudioEngine {
            mode: Mode::Offline { core: RefCell::new(core), output: None },
            commands,
            counters,
            reaper,
            cache: Arc::new(ClipCache::new(clock.now())),
            active: RefCell::new(HashMap::new()),
            listener: Cell::new(Listener::default()),
            next_id: AtomicU64::new(1),
            bus_gains: Cell::new(bus::DEFAULT_GAINS),
            enabled: true,
            clock,
        }
    }

    /// Live 48 kHz mixer and device-rate conversion on a manual clock, without hardware.
    pub fn new_offline_output(sample_rate: u32, channels: usize) -> AudioEngine {
        assert!(sample_rate >= 8000 && (1..=8).contains(&channels));
        let mut engine = Self::new_offline(mixer::MIX_SAMPLE_RATE, 2);
        if let Mode::Offline { output, .. } = &mut engine.mode {
            *output = Some(RefCell::new(MixOutput::new(mixer::MIX_SAMPLE_RATE, sample_rate, channels)));
        }
        engine
    }

    /// The clock this engine runs on (see [`AudioEngine::new_offline`]).
    pub fn clock(&self) -> Clock {
        self.clock.clone()
    }

    /// Mix `out.len() / channels` frames straight into `out` (interleaved, device order):
    /// the offline counterpart of the output callback, for tests and [`AudioEngine::new_offline`].
    pub fn render_offline(&self, out: &mut [f32]) {
        self.pump();
        if let Mode::Offline { core, output } = &self.mode {
            let mut core = core.borrow_mut();
            if let Some(output) = output {
                output.borrow_mut().render(out, &mut |source| core.render_mix(source));
            } else { core.render(out); }
        }
    }

    fn device(&self) -> Option<&DeviceOutput> {
        match &self.mode {
            Mode::Device(d) => Some(d),
            Mode::Offline { .. } => None,
        }
    }

    /// The game thread lets go of voices the audio thread has ended. Called at the top of
    /// every public method, so no caller needs a new hook.
    fn pump(&self) {
        if !self.reaper.has_pending() {
            return;
        }
        let ids = self.reaper.drain();
        if ids.is_empty() {
            return;
        }
        let mut active = self.active.borrow_mut();
        for id in ids {
            active.remove(&id);
        }
    }

    fn enqueue(&self, command: Command) {
        if let Some(dropped) = self.commands.push(command) {
            self.active.borrow_mut().remove(&dropped);
        }
    }

    /// Put an already decoded clip into the cache under `path`: later [`AudioEngine::load_clip`]
    /// calls for it return this one instead of reading a file. Tests and the offline renderer
    /// use it to supply synthetic sounds for a sound set.
    pub fn cache_clip(&self, path: impl Into<PathBuf>, clip: Arc<Clip>) {
        self.cache.insert(path, clip);
    }

    /// A clip from the cache, read now if it is not there.
    pub fn load_clip(&self, path: &Path) -> Option<Arc<Clip>> {
        self.cache.load(path)
    }

    /// Let go of the clips nobody holds (no sound set, no voice) and nobody asked for in
    /// `unused`, once every ten seconds at most. Returns the bytes let go.
    pub fn trim_clips(&self, unused: std::time::Duration) -> usize {
        self.cache.trim(unused)
    }

    /// Whether all of `paths` are in the cache (or known to be missing). The ones that are
    /// not are read on a background thread, started on the first call.
    pub fn clips_ready(&self, paths: &[PathBuf]) -> bool {
        ClipCache::ready(&self.cache, paths, self.enabled)
    }

    /// Play `clip` with the application's raw parameters ([`VoiceParams`]); the runtime
    /// builds [`MixParams`] directly with [`AudioEngine::play_mix`].
    pub fn play(&self, clip: Arc<Clip>, params: VoiceParams) -> VoiceId {
        self.play_mix(clip, params.into())
    }

    /// Play `clip` with the runtime's separate legacy levels (see [`MixParams`]).
    pub fn play_mix(&self, clip: Arc<Clip>, params: MixParams) -> VoiceId {
        self.pump();
        let id = self.next_id.fetch_add(1, Ordering::Relaxed);
        if self.active.borrow().len() >= mixer::VOICE_CAPACITY {
            self.counters.dropped_commands.fetch_add(1, Ordering::Relaxed);
            return id;
        }
        if !self.output_available() && !params.looping {
            return id;
        }
        self.active.borrow_mut().insert(
            id,
            ActiveVoice {
                params,
                asset: VoiceAsset::Clip(clip.clone()),
            },
        );
        if !self.output_available() {
            return id;
        }
        if let Some(dropped) = self.commands.push(Command::Play { id, clip, params }) {
            self.active.borrow_mut().remove(&dropped);
        }
        id
    }

    /// Play what `stream` is fed with (see [`crate::assets::stream::StreamBuf`]); the voice
    /// ends when the stream is closed.
    pub fn play_stream(&self, stream: Arc<StreamBuf>, params: VoiceParams) -> VoiceId {
        self.play_stream_mix(stream, params.into())
    }

    /// [`AudioEngine::play_stream`] with the runtime's separate levels.
    pub fn play_stream_mix(&self, stream: Arc<StreamBuf>, params: MixParams) -> VoiceId {
        self.pump();
        let id = self.next_id.fetch_add(1, Ordering::Relaxed);
        if self.active.borrow().len() >= mixer::VOICE_CAPACITY {
            self.counters.dropped_commands.fetch_add(1, Ordering::Relaxed);
            return id;
        }
        self.active.borrow_mut().insert(
            id,
            ActiveVoice {
                params,
                asset: VoiceAsset::Stream(stream.clone()),
            },
        );
        if !self.output_available() {
            return id;
        }
        if let Some(dropped) = self.commands.push(Command::PlayStream { id, stream, params }) {
            self.active.borrow_mut().remove(&dropped);
        }
        id
    }

    /// New parameters for a voice: queued for the mixer's next block (a few milliseconds),
    /// so the game never waits for a block being mixed. Given twice before that block, the
    /// later ones win.
    pub fn set_params(&self, id: VoiceId, params: VoiceParams) {
        let mut mix = MixParams::from(params);
        if let Some(a) = self.active.borrow().get(&id) {
            mix.bus = a.params.bus;
        }
        self.set_mix_params(id, mix);
    }

    /// [`AudioEngine::set_params`] with the runtime's separate levels.
    pub fn set_mix_params(&self, id: VoiceId, params: MixParams) {
        self.pump();
        let at = self.clock.now();
        if let Some(a) = self.active.borrow_mut().get_mut(&id) {
            a.params = params;
        }
        if self.output_available() {
            self.enqueue(Command::SetParams { id, params, at });
        }
    }

    pub fn stop(&self, id: VoiceId) {
        self.pump();
        self.active.borrow_mut().remove(&id);
        if self.output_available() {
            self.enqueue(Command::Stop { id });
        }
    }

    pub fn set_listener(&self, l: Listener) {
        self.listener.set(l);
        if self.output_available() {
            self.enqueue(Command::SetListener(l));
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::Duration;

    /// The stream opened again (as when the system's output device changed) keeps playing;
    /// on a machine without an output device there is nothing to follow.
    #[test]
    fn follows_the_output_device() {
        let e = AudioEngine::new();
        if e.device_state() != Some(crate::device::DeviceState::Open) {
            return;
        }
        let device = e.device().expect("a device engine keeps its device");
        let before = device.name();
        device.reopen_flag().store(true, Ordering::Relaxed);
        device.mark_opened(Instant::now() - Duration::from_secs(2));
        e.follow_device();
        assert!(device.is_open());
        assert_eq!(device.name(), before);
        assert!(!device.reopen_flag().load(Ordering::Relaxed));
    }
}
