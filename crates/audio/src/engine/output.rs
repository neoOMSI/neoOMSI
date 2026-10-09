//! Output lifecycle: offline is permanent; hardware starts NoDevice, attempts Opening,
//! then Open or NoDevice/Lost. Follow retries failures at most once a second even if the
//! default name did not change. Loops restart at zero; streams retain their bounded ring.
//! One-shots during an outage are discarded, preventing stale horns/steps on reconnect.
use super::{AudioEngine, mixer::{self, AudioCore}, feedback::VoiceAsset, commands::Command};
use crate::device::{DeviceState, OutputFormat};
use std::sync::Arc;
use std::sync::atomic::Ordering;

impl AudioEngine {
    /// `None` means deliberate offline rendering, otherwise the hardware lifecycle state.
    pub fn device_state(&self) -> Option<DeviceState> {
        self.device().map(|d| d.state())
    }
    pub(super) fn output_available(&self) -> bool {
        self.device().is_none_or(|d| d.state() == DeviceState::Open && !d.lost_flag().load(Ordering::Relaxed))
    }
    pub(super) fn open_default(&self) -> bool {
        let Some(device) = self.device() else { return false; };
        let was_lost = device.state() == DeviceState::Lost;
        device.close();
        self.pump();
        if was_lost {
            self.active.borrow_mut().retain(|_, a| a.params.looping || matches!(a.asset, VoiceAsset::Stream(_)));
        }
        self.commands.clear();
        let opened = device.open_prepared(Some(mixer::MIX_SAMPLE_RATE), || {
            let mut core = AudioCore::new(Arc::new(OutputFormat::new(mixer::MIX_SAMPLE_RATE, 2)),
                self.commands.clone(), self.reaper.clone(), self.counters.clone(), mixer::muted());
            move |data: &mut [f32]| core.render_mix(data)
        });
        if opened { self.replay(); }
        else {
            self.active.borrow_mut().retain(|_, a| a.params.looping || matches!(a.asset, VoiceAsset::Stream(_)));
        }
        opened
    }
    fn replay(&self) {
        self.commands.push(Command::SetListener(self.listener.get()));
        for (bus, gain) in [crate::Bus::Vehicle, crate::Bus::Ambience, crate::Bus::Passenger,
            crate::Bus::Announcement, crate::Bus::Radio, crate::Bus::Interface].into_iter().zip(self.bus_gains.get()) {
            self.commands.push(Command::SetBus { bus, gain });
        }
        let active = self.active.borrow();
        let mut ids: Vec<_> = active.keys().copied().collect(); ids.sort_unstable();
        for id in ids {
            let a = &active[&id];
            let cmd = match &a.asset {
                VoiceAsset::Clip(c) => Command::Play { id, clip: c.clone(), params: a.params },
                VoiceAsset::Stream(s) => Command::PlayStream { id, stream: s.clone(), params: a.params },
            };
            self.commands.push(cmd); self.counters.replays.fetch_add(1, Ordering::Relaxed);
        }
    }
    pub fn follow_device(&self) {
        self.pump();
        let Some(device) = self.device() else { return; };
        if !self.enabled || device.opened_age() < 1.0 { return; }
        let requested = device.reopen_flag().swap(false, Ordering::Relaxed);
        if !requested && device.state() == DeviceState::Open { return; }
        let lost = device.lost_flag().swap(false, Ordering::Relaxed);
        device.mark_opened(self.clock.now());
        if lost { device.note_lost(); } else if requested { device.note_default_changed(); }
        self.open_default();
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use super::super::Mode;
    use crate::{Clip, VoiceParams, stream::StreamBuf};
    use crate::device::{DeviceOutput, OutputFormat};
    use std::sync::Arc;
    fn without_device() -> AudioEngine {
        let mut e = AudioEngine::new_offline(48000, 2);
        e.mode = Mode::Device(DeviceOutput::new(Arc::new(OutputFormat::new(48000, 2)), e.clock.now()));
        e
    }
    #[test]
    fn no_device_retains_loops_and_latest_params_but_not_old_one_shots() {
        let e = without_device();
        assert!(e.enabled); assert_eq!(e.device_state(), Some(DeviceState::NoDevice));
        let clip = Arc::new(Clip { sample_rate: 48000, channels: 1, samples: vec![0; 480] });
        let shot = e.play(clip.clone(), VoiceParams::default()); assert!(!e.is_playing(shot));
        let loop_id = e.play(clip, VoiceParams { looping: true, ..Default::default() });
        e.set_params(loop_id, VoiceParams { gain: 0.25, looping: true, ..Default::default() });
        let stream = e.play_stream(Arc::new(StreamBuf::default()), VoiceParams::default());
        let mut queued = Vec::new(); e.commands.drain_into(&mut queued);
        assert!(queued.is_empty(), "there is no stale start queue while hardware is absent");
        e.replay(); e.commands.drain_into(&mut queued);
        assert_eq!(queued.iter().filter(|c| matches!(c, Command::Play { .. } | Command::PlayStream { .. })).count(), 2);
        assert!(queued.iter().any(|c| matches!(c, Command::Play { id, params, .. } if *id == loop_id && params.level.gain() == 0.25)));
        e.stop(loop_id); e.stop(stream); assert_eq!(e.voice_count(), 0);
    }
    #[test]
    fn clearing_old_commands_prevents_duplicate_stream_readers_on_reconnect() {
        let e = AudioEngine::new_offline(48000, 2);
        e.play_stream(Arc::new(StreamBuf::default()), VoiceParams::default());
        e.commands.clear(); e.replay();
        let mut core = AudioCore::new(Arc::new(OutputFormat::new(48000, 2)),
            e.commands.clone(), e.reaper.clone(), e.counters.clone(), false);
        core.render(&mut [0.0; 256]); e.pump();
        assert_eq!(e.voice_count(), 1, "one stream reader, no duplicate start ending its id");
    }
}
