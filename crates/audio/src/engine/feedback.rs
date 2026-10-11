//! The game-side view of playback, and the two bounded channels the audio thread uses to
//! report back. The audio thread owns the voices; the game keeps its own authoritative map of
//! the voices it started (id, parameters, asset), so `is_playing`/`voice_state`/`voice_count`
//! never touch the audio-thread state. When the audio thread ends a voice on its own (a
//! one-shot reaching its end, a stream closing, a device going away) it enqueues the id on the
//! [`Reaper`]; the game drains that (see [`super::AudioEngine::pump`]) and forgets the voice,
//! which is also where the large `Arc<Clip>`/`Arc<StreamBuf>` are finally let go - never on the
//! audio thread.

use crate::assets::{clip::Clip, stream::StreamBuf};
use crate::spatial::distance_gain;
use crate::voice::{MixParams, Voice, VoiceId, VoiceParams};
use glam::Vec3;
use parking_lot::Mutex;
use std::sync::atomic::{AtomicU64, AtomicUsize, Ordering};
use std::sync::Arc;

/// Voice ends the reaper holds before the mixer keeps a finished voice in its list for one
/// more block. That path never allocates and never loses a retirement, it only defers it.
pub(crate) const REAPER_CAPACITY: usize = 1024;

/// Fault counters of the real-time path. Written by the callback (relaxed, no allocation,
/// no blocking) and read by the game; the offline tests assert on them.
#[derive(Default)]
pub(crate) struct Counters {
    /// The command queue was locked by the game when the callback wanted to drain it.
    pub(crate) command_lock_misses: AtomicU64,
    /// Events dropped because the queue was full of events (see `commands`).
    pub(crate) dropped_commands: AtomicU64,
    /// Parameter entries evicted to make room for an event.
    pub(crate) coalesced_evicted: AtomicU64,
    /// 256-frame render fragments in which a stream lacked buffered input.
    pub(crate) stream_underruns: AtomicU64,
    /// Voices retired to the game thread.
    pub(crate) retired: AtomicU64,
    /// The reaper buffer was full; the voice is retired again next block.
    pub(crate) reaper_overflow: AtomicU64,
    /// The reaper was locked by the game when the callback wanted to retire.
    pub(crate) reaper_lock_misses: AtomicU64,
    /// Voices re-issued after a device change (see `AudioEngine::replay`).
    pub(crate) replays: AtomicU64,
}

impl Counters {
    pub(crate) fn snapshot(&self) -> AudioStats {
        AudioStats {
            command_lock_misses: self.command_lock_misses.load(Ordering::Relaxed),
            dropped_commands: self.dropped_commands.load(Ordering::Relaxed),
            coalesced_evicted: self.coalesced_evicted.load(Ordering::Relaxed),
            stream_underruns: self.stream_underruns.load(Ordering::Relaxed),
            retired: self.retired.load(Ordering::Relaxed),
            reaper_overflow: self.reaper_overflow.load(Ordering::Relaxed),
            reaper_lock_misses: self.reaper_lock_misses.load(Ordering::Relaxed),
            replays: self.replays.load(Ordering::Relaxed),
            output_xruns: 0,
            realtime_denials: 0,
        }
    }
}

/// A snapshot of the real-time counters, for tests and diagnostics.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct AudioStats {
    pub command_lock_misses: u64,
    pub dropped_commands: u64,
    pub coalesced_evicted: u64,
    pub stream_underruns: u64,
    pub retired: u64,
    pub reaper_overflow: u64,
    pub reaper_lock_misses: u64,
    pub replays: u64,
    pub output_xruns: u64,
    pub realtime_denials: u64,
}

impl AudioStats {
    /// No command loss, underruns or retirement overflow.
    /// Scheduling refusal alone isn't a playback fault.
    pub fn clean(&self) -> bool {
        self.dropped_commands == 0
            && self.coalesced_evicted == 0
            && self.stream_underruns == 0
            && self.reaper_overflow == 0
            && self.output_xruns == 0
    }
}

/// What a voice reads: a decoded clip, or the buffer of a live stream.
pub(crate) enum VoiceAsset {
    Clip(Arc<Clip>),
    Stream(Arc<StreamBuf>),
    Source(Arc<dyn crate::assets::Source>),
}

impl VoiceAsset {
    /// Fed while it plays: kept over a lost device and played again.
    pub(crate) fn fed(&self) -> bool {
        matches!(self, VoiceAsset::Stream(_) | VoiceAsset::Source(_))
    }
}

/// One voice the game started, as the game remembers it.
pub(crate) struct ActiveVoice {
    pub(crate) params: MixParams,
    pub(crate) asset: VoiceAsset,
}

/// Voice ends reported by the audio thread. Bounded: on overflow the audio thread keeps the
/// finished voice for one more block rather than allocating or dropping the id.
pub(crate) struct Reaper {
    inner: Mutex<Vec<Retired>>,
    queued: AtomicUsize,
}

enum Retired {
    Voice(Voice),
    Rejected(super::commands::Command),
}

impl Reaper {
    pub(crate) fn new() -> Reaper {
        Reaper {
            inner: Mutex::new(Vec::with_capacity(REAPER_CAPACITY)),
            queued: AtomicUsize::new(0),
        }
    }

    pub(crate) fn has_pending(&self) -> bool {
        self.queued.load(Ordering::Relaxed) > 0
    }

    /// Audio thread: move finished voices out of the mixer list. Only voices whose id fits are
    /// moved in full (including assets and decoder-reader ownership); the rest wait for
    /// the next block. No asset can be finally freed in the callback after stop/unload.
    pub(crate) fn retire_finished(&self, voices: &mut Vec<Voice>, counters: &Counters) {
        let Some(mut r) = self.inner.try_lock() else {
            counters.reaper_lock_misses.fetch_add(1, Ordering::Relaxed);
            return;
        };
        let mut i = 0;
        while i < voices.len() {
            if voices[i].is_finished() && r.len() < REAPER_CAPACITY {
                r.push(Retired::Voice(voices.swap_remove(i)));
                counters.retired.fetch_add(1, Ordering::Relaxed);
            } else {
                if voices[i].is_finished() { counters.reaper_overflow.fetch_add(1, Ordering::Relaxed); }
                i += 1;
            }
        }
        self.queued.store(r.len(), Ordering::Relaxed);
    }

    /// Hold rejected start assets until the game can free them. On a busy/full reaper
    /// the core retains the command and defers draining more commands (bounded storage).
    pub(crate) fn reject(&self, command: super::commands::Command)
        -> Result<(), super::commands::Command> {
        let Some(mut r) = self.inner.try_lock() else { return Err(command); };
        if r.len() == REAPER_CAPACITY { return Err(command); }
        r.push(Retired::Rejected(command));
        self.queued.store(r.len(), Ordering::Relaxed);
        Ok(())
    }

    /// Game thread: take the reported ids and destroy the retained voices/assets on this side.
    pub(crate) fn drain(&self) -> Vec<VoiceId> {
        let mut q = self.inner.lock();
        if q.is_empty() {
            return Vec::new();
        }
        let out = q.drain(..).filter_map(|item| match item {
            Retired::Voice(v) => Some(v.id()),
            Retired::Rejected(c) => c.voice_id(),
        }).collect();
        self.queued.store(0, Ordering::Relaxed);
        out
    }
}

impl super::AudioEngine {
    /// What a voice plays with now, and how loud it arrives at the listener (gain after
    /// distance), while it plays. Read from the game's own view, never from the mixer.
    pub fn voice_state(&self, id: VoiceId) -> Option<(VoiceParams, f32)> {
        self.pump();
        let listener = self.listener.get();
        let active = self.active.borrow();
        let a = active.get(&id)?;
        let spatial = a
            .params
            .position
            .map(|p| distance_gain(a.params.range, (p - listener.position).length()))
            .unwrap_or(1.0);
        Some((a.params.into(), a.params.level.gain() * (1.0 + (spatial - 1.0) * a.params.spatial_blend.clamp(0.0, 1.0))))
    }

    pub fn is_playing(&self, id: VoiceId) -> bool {
        self.pump();
        self.active.borrow().contains_key(&id)
    }

    pub fn listener_position(&self) -> Vec3 {
        self.listener.get().position
    }

    pub fn voice_count(&self) -> usize {
        self.pump();
        self.active.borrow().len()
    }

    /// The real-time counters since the engine started (see [`AudioStats`]).
    pub fn stats(&self) -> AudioStats {
        let mut stats = self.counters.snapshot();
        if let Some(device) = self.device() {
            (stats.output_xruns, stats.realtime_denials) = device.error_counts();
        }
        stats
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn scheduling_refusal_alone_does_not_make_playback_unclean() {
        let stats = AudioStats { realtime_denials: 1, ..Default::default() };
        assert!(stats.clean());
        assert!(!AudioStats { output_xruns: 1, ..stats }.clean());
    }

    #[test]
    fn the_reaper_defers_when_the_buffer_is_full() {
        let counters = Counters::default();
        let reaper = Reaper::new();
        let mut voices: Vec<Voice> = Vec::new();
        let clip = Arc::new(Clip {
            sample_rate: 48_000,
            channels: 1,
            samples: vec![0; 4],
        });
        for id in 0..(REAPER_CAPACITY as u64 + 3) {
            let mut v = Voice::clip_voice(id, clip.clone(), VoiceParams::default().into());
            v.finish();
            voices.push(v);
        }
        reaper.retire_finished(&mut voices, &counters);
        assert_eq!(reaper.drain().len(), REAPER_CAPACITY);
        assert_eq!(voices.len(), 3, "the rest wait for the next block");
        assert_eq!(counters.reaper_overflow.load(Ordering::Relaxed), 3);
        // the next block retires the leftovers
        reaper.retire_finished(&mut voices, &counters);
        assert_eq!(reaper.drain().len(), 3);
        assert!(voices.is_empty());
    }
}
