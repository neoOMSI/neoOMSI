//! The audio thread's own state, owned exclusively by whoever calls [`AudioCore::render`]:
//! the output callback with a device, or the offline renderer on the caller's thread. It
//! holds the voice list, the listener and the effects, drains the bounded
//! command queue at the top of every block and retires finished voices to the game thread.
//!
//! Real-time constraints (documented once, here): `render` must not block on a game or
//! decoder lock, must not allocate in steady state and must not touch a file. The queue is
//! drained with `try_lock`, the buffers it reuses are preallocated to [`VOICE_CAPACITY`] /
//! [`COMMAND_CAPACITY`], the parameter lookup scans the (bounded) voice list instead of
//! building a `HashMap`, complete finished voices go to the bounded reaper, and radio reads
//! atomic ring slots without a decoder lock.

use crate::device::OutputFormat;
use crate::dsp::limiter::Limiter;
use crate::dsp::{envelope, resample::Kernel};
use crate::voice::bus::{BUS_COUNT, DEFAULT_GAINS, HEADROOM};
use crate::dsp::reverb::Reverb;
use crate::engine::commands::{Command, CommandQueue, COMMAND_CAPACITY};
use crate::engine::feedback::{Counters, Reaper};
use crate::spatial::Legacy;
use crate::voice::{doppler_enabled, Voice};
use std::sync::atomic::Ordering;
use std::sync::Arc;

// Keep the old `::audio::mixer::…` paths working.
pub use crate::assets::Clip;
pub use crate::voice::{Listener, VoiceId, VoiceParams};

/// At most this many clip voices are mixed at once (OMSI's `[sound_maxcount]` default).
pub const MAX_VOICES: usize = 200;

/// Hard storage bound, including streams, virtual voices and stop tails. Excess starts
/// are counted and returned to the game thread without growing the callback storage.
pub(crate) const VOICE_CAPACITY: usize = 512;
pub(crate) const BLOCK_FRAMES: usize = 256;
pub(crate) const MIX_SAMPLE_RATE: u32 = 48000;

pub(crate) struct AudioCore {
    voices: Vec<Voice>,
    kernel: Arc<Kernel>,
    buses: [[f32; BLOCK_FRAMES * 2]; BUS_COUNT],
    bus_gains: [f32; BUS_COUNT],
    bus_targets: [f32; BUS_COUNT],
    stereo: [f32; BLOCK_FRAMES * 2],
    listener: Listener,
    reverb: Reverb,
    pa_reverb: Reverb,
    pa_send: [f32; BLOCK_FRAMES * 2],
    pa_return: f32,
    pa_return_target: f32,
    pa_tail_frames: usize,
    pa_damping: [f32; 2],
    pa_damping_alpha: f32,
    limiter: Limiter,
    spatial: Legacy,
    muted: bool,
    /// The output device's rate and channels, read while rendering.
    format: Arc<OutputFormat>,
    queue: Arc<CommandQueue>,
    reaper: Arc<Reaper>,
    counters: Arc<Counters>,
    /// Reused command drain buffer (see the module note).
    cmds: Vec<Command>,
    rejected: Vec<Command>,
    /// Reused ranking buffers.
    ranked: Vec<(bool, f32, usize)>,
    keep: Vec<bool>,
}

impl AudioCore {
    pub(crate) fn new(
        format: Arc<OutputFormat>,
        queue: Arc<CommandQueue>,
        reaper: Arc<Reaper>,
        counters: Arc<Counters>,
        muted: bool,
    ) -> AudioCore {
        let mut reverb = Reverb::default();
        reverb.prepare(2, format.sample_rate());
        let mut pa_reverb = Reverb::default();
        pa_reverb.prepare(2, format.sample_rate());
        AudioCore {
            voices: Vec::with_capacity(VOICE_CAPACITY),
            kernel: Arc::new(Kernel::default()),
            buses: [[0.0; BLOCK_FRAMES * 2]; BUS_COUNT],
            bus_gains: DEFAULT_GAINS, bus_targets: DEFAULT_GAINS,
            stereo: [0.0; BLOCK_FRAMES * 2],
            listener: Listener::default(),
            reverb,
            pa_reverb, pa_send: [0.0; BLOCK_FRAMES * 2], pa_return: 1.0, pa_return_target: 1.0,
            pa_tail_frames: 0,
            pa_damping: [0.0; 2],
            pa_damping_alpha: 1.0 - (-std::f32::consts::TAU * 4500.0 / format.sample_rate() as f32).exp(),
            limiter: Limiter::default(),
            spatial: Legacy,
            muted,
            format,
            queue,
            reaper,
            counters,
            cmds: Vec::with_capacity(COMMAND_CAPACITY),
            rejected: Vec::with_capacity(COMMAND_CAPACITY),
            ranked: Vec::with_capacity(VOICE_CAPACITY),
            keep: Vec::with_capacity(VOICE_CAPACITY),
        }
    }

    /// Apply one command, in the order it was queued.
    fn apply(&mut self, cmd: Command) {
        match cmd {
            Command::Play { id, clip, params } => {
                self.voices.push(Voice::clip_with_kernel(id, clip, params, self.kernel.clone()));
            }
            Command::PlayStream { id, stream, params } => {
                self.voices.push(Voice::stream_with_kernel(id, stream, params, self.kernel.clone()));
            }
            Command::PlaySource { id, feed, params } => {
                self.voices.push(Voice::source_with_kernel(id, feed, params, self.kernel.clone()));
            }
            Command::Stop { id } => {
                if let Some(v) = self.voices.iter_mut().find(|v| v.id() == id) {
                    v.stop();
                }
            }
            Command::SetParams { id, params, at } => {
                let listener = self.listener.position;
                let doppler = doppler_enabled();
                if let Some(v) = self.voices.iter_mut().find(|v| v.id() == id) {
                    v.apply_params(params, at, listener, doppler);
                }
            }
            Command::SetListener(l) => self.listener = l,
            Command::SetBus { bus, gain } => self.bus_targets[bus as usize] = gain,
        }
    }

    /// Mix one block into `out` (interleaved, device order).
    pub(crate) fn render(&mut self, out: &mut [f32]) {
        self.render_block(out, true);
    }

    /// Device output limits after resampling.
    pub(crate) fn render_mix(&mut self, out: &mut [f32]) {
        self.render_block(out, false);
    }

    fn render_block(&mut self, out: &mut [f32], limit: bool) {
        for s in out.iter_mut() {
            *s = 0.0;
        }
        // Commands first: start/stop/parameters take effect this block, in order.
        let mut cmds = std::mem::take(&mut self.cmds);
        let mut rejected = std::mem::take(&mut self.rejected);
        for cmd in rejected.drain(..) {
            if let Err(cmd) = self.reaper.reject(cmd) { cmds.push(cmd); }
        }
        std::mem::swap(&mut cmds, &mut rejected);
        if rejected.is_empty() {
            self.queue.drain_into(&mut cmds);
            for cmd in cmds.drain(..) {
                if self.voices.len() >= VOICE_CAPACITY && matches!(cmd, Command::Play { .. } | Command::PlayStream { .. } | Command::PlaySource { .. }) {
                    self.counters.dropped_commands.fetch_add(1, Ordering::Relaxed);
                    if let Err(cmd) = self.reaper.reject(cmd) { rejected.push(cmd); }
                } else { self.apply(cmd); }
            }
        }
        self.cmds = cmds;
        self.rejected = rejected;

        let listener = self.listener;
        let ch = self.format.channels();
        let rate = self.format.sample_rate();
        let dev_rate = rate as f64;
        // More voices than OMSI's `[sound_maxcount]` (200 by default): keep `[important]`
        // sounds first, then the ordinary voices that reach the listener loudest. Voices left
        // out still advance in time, so a loop comes back at the right phase.
        let over = self
            .voices
            .iter()
            .filter(|v| !v.is_finished() && !v.is_stream())
            .count()
            > MAX_VOICES;
        let mixed = if over {
            // Rank by what the listener actually hears: a voice on a muted (gain 0) bus must
            // not outrank an audible one and take a mixed slot.
            let bus_targets = self.bus_targets;
            self.ranked.clear();
            self.ranked.extend(
                self.voices
                    .iter()
                    .enumerate()
                    .filter(|(_, v)| !v.is_finished() && !v.is_stream())
                    .map(|(i, v)| {
                        let bus = v.params().bus as usize;
                        (v.important(), v.heard_gain(&listener) * bus_targets[bus], i)
                    }),
            );
            self.ranked
                .sort_unstable_by(|a, b| b.0.cmp(&a.0).then_with(|| b.1.total_cmp(&a.1)).then_with(|| a.2.cmp(&b.2)));
            if self.keep.len() < self.voices.len() {
                self.keep.resize(self.voices.len(), false);
            }
            for k in self.keep.iter_mut() {
                *k = false;
            }
            for (_, _, i) in self.ranked.iter().take(MAX_VOICES) {
                self.keep[*i] = true;
            }
            true
        } else {
            false
        };
        // The cabin flag is a bodywork hint, not a reverb preset: the forced 0.22 wet mix
        // coloured every switch/button with a synthetic tail, so only explicitly authored
        // world/trigger-box reverb (the listener's own params) drives the shared effect.
        let (rt, mix) = (listener.reverb_time, listener.reverb_mix);
        let bus_k = envelope::coefficient(rate, 0.02);
        // Hold return visibility after a voice ends so its room tail can finish. While
        // an announcement runs outside, smoothly hide the interior return.
        let mut pa_active = false;
        let mut pa_inside = false;
        for voice in &self.voices {
            if !voice.is_finished() && voice.params().bus == crate::Bus::Announcement {
                pa_active = true;
                pa_inside |= voice.params().cabin_reverb > 0.001;
            }
        }
        if pa_active { self.pa_return_target = if pa_inside { 1.0 } else { 0.0 }; }
        let mut stalls = 0u64;
        for chunk in out.chunks_mut(BLOCK_FRAMES * ch) {
            let count = chunk.len() / ch;
            let samples = count * 2;
            for bus in &mut self.buses { bus[..samples].fill(0.0); }
            self.pa_send[..samples].fill(0.0);
            for (i, voice) in self.voices.iter_mut().enumerate() {
                if voice.is_finished() { continue; }
                if !voice.is_stream() && mixed && !self.keep[i] { voice.skip(count, dev_rate); continue; }
                let bus = voice.params().bus as usize;
                let send = if bus == crate::Bus::Announcement as usize {
                    Some(&mut self.pa_send[..samples])
                } else { None };
                if voice.render_with_reverb(&mut self.buses[bus][..samples], 2, rate,
                    &listener, &self.spatial, send) { stalls += 1; }
            }
            // Process silence during the tail too, but do not charge ordinary traffic
            // for an idle PA effect. Two seconds is >4 RT60 intervals at this preset.
            if self.pa_send[..samples].iter().any(|x| x.abs() > 1e-10) { self.pa_tail_frames = rate as usize * 2; }
            if self.pa_tail_frames > 0 {
                self.pa_reverb.process_wet(&mut self.pa_send[..samples], 2, rate, 0.45);
                for frame in self.pa_send[..samples].chunks_exact_mut(2) {
                    for c in 0..2 {
                        envelope::smooth(&mut self.pa_damping[c], frame[c], self.pa_damping_alpha);
                        frame[c] = self.pa_damping[c];
                    }
                }
                self.pa_tail_frames = self.pa_tail_frames.saturating_sub(count);
            }
            self.stereo[..samples].fill(0.0);
            for f in 0..count {
                envelope::smooth(&mut self.pa_return, self.pa_return_target, bus_k);
                for bus in 0..BUS_COUNT {
                    envelope::smooth(&mut self.bus_gains[bus], self.bus_targets[bus], bus_k);
                    for c in 0..2 {
                        self.stereo[f * 2 + c] += self.buses[bus][f * 2 + c] * self.bus_gains[bus] * HEADROOM;
                    }
                }
                let pa_gain = self.bus_gains[crate::Bus::Announcement as usize] * HEADROOM * self.pa_return;
                if listener.master > 0.0 {
                    for c in 0..2 { self.stereo[f * 2 + c] += self.pa_send[f * 2 + c] * pa_gain; }
                }
            }
            if mix > 0.001 && rt > 0.05 {
                self.reverb.process(&mut self.stereo[..samples], 2, rate, rt.min(3.0), mix.min(1.0));
            }
            if limit { self.limiter.process(&mut self.stereo[..samples], 2, rate); }
            for f in 0..count {
                let (l, r) = (self.stereo[f * 2], self.stereo[f * 2 + 1]);
                if !self.muted {
                    if ch == 1 { chunk[f] = (l + r) * 0.5; }
                    else { chunk[f * ch] = l; chunk[f * ch + 1] = r; }
                }
            }
        }
        if stalls > 0 { self.counters.stream_underruns.fetch_add(stalls, Ordering::Relaxed); }
        self.reaper.retire_finished(&mut self.voices, &self.counters);

    }
}

impl Drop for AudioCore {
    fn drop(&mut self) {
        // A device change drops a core on the game thread; report whatever ended so the game
        // can restart (see `AudioEngine::replay`). Active voices are replayed, not reported.
        self.reaper.retire_finished(&mut self.voices, &self.counters);
    }
}

/// Whether `OMSI_MUTE` is set (see [`AudioCore`]).
pub(crate) fn muted() -> bool {
    ::legacy_config::env::var_os("OMSI_MUTE").is_some()
}

#[cfg(test)]
#[path = "mixer_tests.rs"]
mod tests;
