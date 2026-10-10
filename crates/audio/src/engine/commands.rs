//! The game-to-audio command channel. The audio thread owns the voices and the DSP state, so
//! the game only sends it bounded commands: start/stop events keep their order, while
//! parameters may be coalesced. The audio side drains with `try_lock` only, so the callback
//! never waits on the game thread; when the lock is busy it drains next block.
//!
//! Capacity and overflow (documented once, here): the queue holds [`COMMAND_CAPACITY`]
//! entries. A `SetParams` for an id already queued, a `SetListener` or a `SetBus` for a bus
//! already queued is folded into the existing entry, so per-frame parameter traffic cannot
//! grow the queue. When the queue is full, the oldest parameter entry is evicted (counted as
//! `coalesced_evicted`); next anything but a `Stop` is evicted (counted as `dropped_commands`);
//! only a queue of nothing but stops falls back to its oldest entry, never silently. A start
//! dropped this way is recoverable - the game forgets the id and the runtime starts it again -
//! while a dropped `Stop` would strand a looping voice, so stops are kept. The policy is a
//! definition, not a promise that extreme overload is free.

use crate::assets::{clip::Clip, stream::StreamBuf};
use crate::voice::voice::Feed;
use crate::engine::feedback::Counters;
use crate::voice::{Listener, MixParams, VoiceId};
use parking_lot::Mutex;
use std::collections::VecDeque;
use std::sync::atomic::Ordering;
use std::sync::Arc;
use std::time::Instant;

/// How many commands the queue may hold before the eviction rules above apply.
pub(crate) const COMMAND_CAPACITY: usize = 2048;

pub(crate) enum Command {
    Play {
        id: VoiceId,
        clip: Arc<Clip>,
        params: MixParams,
    },
    PlayStream {
        id: VoiceId,
        stream: Arc<StreamBuf>,
        params: MixParams,
    },
    PlaySource {
        id: VoiceId,
        feed: Feed,
        params: MixParams,
    },
    Stop {
        id: VoiceId,
    },
    SetParams {
        id: VoiceId,
        params: MixParams,
        at: Instant,
    },
    SetListener(Listener),
    SetBus { bus: crate::voice::bus::Bus, gain: f32 },
}

impl Command {
    /// The voice a dropped command was about (`None` for the global listener/cabin commands).
    pub(crate) fn voice_id(&self) -> Option<VoiceId> {
        match self {
            Command::Play { id, .. }
            | Command::PlayStream { id, .. }
            | Command::PlaySource { id, .. }
            | Command::Stop { id }
            | Command::SetParams { id, .. } => Some(*id),
            Command::SetListener(_) | Command::SetBus { .. } => None,
        }
    }
}

pub(crate) struct CommandQueue {
    inner: Mutex<VecDeque<Command>>,
    counters: Arc<Counters>,
}

impl CommandQueue {
    pub(crate) fn new(counters: Arc<Counters>) -> CommandQueue {
        CommandQueue {
            inner: Mutex::new(VecDeque::with_capacity(COMMAND_CAPACITY)),
            counters,
        }
    }

    /// Queue a command (game thread; may briefly wait for the audio side's drain). Returns
    /// the id of a voice command that had to be dropped to make room, if any, so the engine
    /// can forget it.
    pub(crate) fn push(&self, cmd: Command) -> Option<VoiceId> {
        let mut q = self.inner.lock();
        match cmd {
            Command::SetBus { bus, gain } => {
                if let Some(slot) = q.iter_mut().find(|c| matches!(c, Command::SetBus { bus: b, .. } if *b == bus)) {
                    *slot = Command::SetBus { bus, gain }; return None;
                }
            }
            Command::SetListener(l) => {
                if let Some(slot) = q.iter_mut().find(|c| matches!(c, Command::SetListener(_))) {
                    *slot = Command::SetListener(l);
                    return None;
                }
            }
            Command::SetParams { id, params, at } => {
                if let Some(slot) = q
                    .iter_mut()
                    .find(|c| matches!(c, Command::SetParams { id: cid, .. } if *cid == id))
                {
                    *slot = Command::SetParams { id, params, at };
                    return None;
                }
            }
            _ => {}
        }
        if q.len() < COMMAND_CAPACITY {
            q.push_back(cmd);
            return None;
        }
        // Full: drop an old parameter first, so start/stop events survive.
        if let Some(pos) = q
            .iter()
            .position(|c| matches!(c, Command::SetParams { .. }))
        {
            q.remove(pos);
            self.counters.coalesced_evicted.fetch_add(1, Ordering::Relaxed);
            q.push_back(cmd);
            return None;
        }
        // Then prefer to evict anything but a `Stop`: a dropped start is restarted by the
        // runtime, but a dropped `Stop` would leave a looping voice with no handle left to
        // end it. Globals (listener/bus) are periodic and cheap to lose, so they go too.
        if let Some(pos) = q
            .iter()
            .position(|c| !matches!(c, Command::Stop { .. }))
        {
            let dropped = q.remove(pos);
            self.counters.dropped_commands.fetch_add(1, Ordering::Relaxed);
            q.push_back(cmd);
            return dropped.and_then(|c| c.voice_id());
        }
        // Nothing but stop events left (pathological): drop the oldest and report it.
        let dropped = q.pop_front();
        self.counters.dropped_commands.fetch_add(1, Ordering::Relaxed);
        q.push_back(cmd);
        dropped.and_then(|c| c.voice_id())
    }

    /// Game thread after the old device callback has stopped: discard stale commands.
    pub(crate) fn clear(&self) { self.inner.lock().clear(); }

    /// Take everything queued for the block about to be mixed (audio thread; never blocks).
    /// `out` is a reused buffer so a steady-state render does not allocate.
    pub(crate) fn drain_into(&self, out: &mut Vec<Command>) {
        let Some(mut q) = self.inner.try_lock() else {
            self.counters.command_lock_misses.fetch_add(1, Ordering::Relaxed);
            return;
        };
        out.extend(q.drain(..));
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::voice::{MixParams, VoiceParams};

    fn queue() -> CommandQueue {
        CommandQueue::new(Arc::new(Counters::default()))
    }

    fn play(id: VoiceId) -> Command {
        Command::Play {
            id,
            clip: Arc::new(Clip {
                sample_rate: 48_000,
                channels: 1,
                samples: vec![0; 4],
            }),
            params: MixParams::default(),
        }
    }

    fn params(id: VoiceId, gain: f32) -> Command {
        Command::SetParams {
            id,
            params: MixParams::from(VoiceParams {
                gain,
                ..Default::default()
            }),
            at: Instant::now(),
        }
    }

    fn drain(q: &CommandQueue) -> Vec<Command> {
        let mut out = Vec::new();
        q.drain_into(&mut out);
        out
    }

    #[test]
    fn events_keep_their_order() {
        let q = queue();
        q.push(play(1));
        q.push(Command::Stop { id: 1 });
        q.push(play(2));
        let got = drain(&q);
        assert!(matches!(got[0], Command::Play { id: 1, .. }));
        assert!(matches!(got[1], Command::Stop { id: 1 }));
        assert!(matches!(got[2], Command::Play { id: 2, .. }));
    }

    #[test]
    fn parameters_coalesce_in_place() {
        let q = queue();
        q.push(play(7));
        q.push(params(7, 0.25));
        q.push(params(7, 0.75));
        let got = drain(&q);
        assert_eq!(got.len(), 2, "the two parameter sets folded into one");
        match &got[1] {
            Command::SetParams { params, .. } => {
                assert_eq!(params.level.gain(), 0.75)
            }
            _ => panic!("expected the coalesced parameters"),
        }
    }

    #[test]
    fn overflow_evicts_a_parameter_first_and_counts_it() {
        let counters = Arc::new(Counters::default());
        let q = CommandQueue::new(counters.clone());
        for k in 0..COMMAND_CAPACITY {
            q.push(params(k as VoiceId, 1.0));
        }
        let dropped = q.push(play(9_999));
        assert_eq!(dropped, None, "a parameter was evicted instead of the start");
        assert_eq!(counters.coalesced_evicted.load(Ordering::Relaxed), 1);
        assert_eq!(counters.dropped_commands.load(Ordering::Relaxed), 0);
    }

    #[test]
    fn overflow_evicts_a_start_rather_than_a_stop() {
        let counters = Arc::new(Counters::default());
        let q = CommandQueue::new(counters.clone());
        q.push(Command::Stop { id: 0 });
        for k in 1..COMMAND_CAPACITY {
            q.push(play(k as VoiceId));
        }
        let dropped = q.push(play(9_999));
        assert_eq!(dropped, Some(1), "an older start is reported, not the stop");
        assert_eq!(counters.dropped_commands.load(Ordering::Relaxed), 1);
        let got = drain(&q);
        assert!(
            got.iter().any(|c| matches!(c, Command::Stop { id } if *id == 0)),
            "the stop survives the overflow"
        );
    }

    #[test]
    fn a_full_queue_of_events_drops_the_oldest_and_reports_it() {
        let counters = Arc::new(Counters::default());
        let q = CommandQueue::new(counters.clone());
        for k in 0..COMMAND_CAPACITY {
            q.push(play(k as VoiceId));
        }
        let dropped = q.push(play(9_999));
        assert_eq!(dropped, Some(0), "the oldest start is reported, not lost silently");
        assert_eq!(counters.dropped_commands.load(Ordering::Relaxed), 1);
    }
}
