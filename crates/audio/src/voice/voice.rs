//! A playing voice: the clip (or stream) it reads, its parameters and its position, and the
//! per-block mixing of one voice into the output buffer. The reverb, limiter and cabin blend
//! that surround this are the mixer's job (see [`crate::engine::mixer`]).

use crate::assets::{stream::{StreamBuf, StreamReader}, Clip};
use crate::dsp::{envelope::{self, Envelope}, filter::LowPass, resample};
use crate::spatial::{self, Spatializer};
use crate::voice::params::{Level, Listener, MixParams, VoiceId, DOPPLER};
#[cfg(test)]
use crate::voice::params::VoiceParams;
use std::sync::Arc;
use std::time::Instant;

pub struct Voice {
    id: VoiceId,
    clip: Arc<Clip>,
    /// A voice fed while it plays (internet radio) instead of from `clip`.
    pub(crate) stream: Option<Arc<StreamBuf>>,
    params: MixParams,
    pos: f64,
    kernel: Arc<resample::Kernel>,
    seam: resample::Seam,
    reader: Option<StreamReader>,
    stream_last: [f32; 2],
    envelope: Envelope,
    cur_step: f64,
    finished: bool,
    /// Smoothed gain to avoid clicks.
    cur_gain: f32,
    cur_reverb: f32,
    cur_pan: [f32; 2],
    pan_ready: bool,
    /// One-pole low-pass filter state.
    lp: LowPass,
    /// The Doppler shift: the distance to the listener when the position last came, when,
    /// and the (smoothed) pitch factor it gives.
    doppler: (f32, Option<Instant>, f32),
}

impl Voice {
    /// Direct construction is intended for tools; the mixer supplies its prebuilt kernel.
    pub fn clip_voice(id: VoiceId, clip: Arc<Clip>, params: MixParams) -> Voice {
        Self::clip_with_kernel(id, clip, params, Arc::new(resample::Kernel::default()))
    }

    pub(crate) fn clip_with_kernel(id: VoiceId, clip: Arc<Clip>, params: MixParams,
        kernel: Arc<resample::Kernel>) -> Voice {
        let seam = resample::Seam::new(&clip);
        Voice { id, clip, seam, stream: None, params, pos: 0.0, kernel, reader: None, stream_last: [0.0; 2],
            envelope: Envelope::default(), cur_step: 0.0, finished: false,
            cur_gain: 0.0, cur_reverb: 0.0, cur_pan: [1.0; 2], pan_ready: false, lp: LowPass::default(), doppler: (0.0, None, 1.0) }
    }

    pub fn stream_voice(id: VoiceId, stream: Arc<StreamBuf>, params: MixParams) -> Voice {
        Self::stream_with_kernel(id, stream, params, Arc::new(resample::Kernel::default()))
    }

    pub(crate) fn stream_with_kernel(id: VoiceId, stream: Arc<StreamBuf>, params: MixParams,
        kernel: Arc<resample::Kernel>) -> Voice {
        let reader = stream.reader();
        let mut voice = Self::clip_with_kernel(id, stream.empty_clip.clone(), params, kernel);
        voice.finished = reader.is_none();
        voice.reader = reader;
        voice.stream = Some(stream);
        voice
    }

    pub fn id(&self) -> VoiceId {
        self.id
    }

    pub fn is_finished(&self) -> bool {
        self.finished
    }

    pub fn is_stream(&self) -> bool {
        self.stream.is_some()
    }

    pub fn params(&self) -> MixParams {
        self.params
    }

    pub fn important(&self) -> bool {
        self.params.important
    }

    pub fn finish(&mut self) {
        self.finished = true;
    }

    pub fn stop(&mut self) {

        self.envelope.stop();

    }

    /// A voice whose smoothed gain already sits at `params.gain` (tests that expect the
    /// block to carry a steady level from the first frame).
    #[cfg(test)]
    pub(crate) fn test_voice(id: VoiceId, clip: Arc<Clip>, params: VoiceParams) -> Voice {
        let mix = MixParams::from(params);
        let mut v = Voice::clip_voice(id, clip, mix);
        v.cur_gain = mix.level.gain();
        v.envelope.steady();
        v
    }

    /// New parameters for this voice, given at `now` with the listener at `listener`: the
    /// Doppler shift from how fast the distance to the listener changes (the bus's own sounds
    /// move with the listener and keep their pitch). `doppler_enabled` is the outward
    /// `DOPPLER` switch read once by the engine.
    pub fn apply_params(
        &mut self,
        params: MixParams,
        now: Instant,
        listener: glam::Vec3,
        doppler_enabled: bool,
    ) {
        if let (Some(p), true) = (params.position, params.doppler && doppler_enabled) {
            let dist = (p - listener).length();
            let (last, at, factor) = self.doppler;
            let mut f = factor;
            if let Some(at) = at {
                let dt = now.saturating_duration_since(at).as_secs_f32();
                if (0.004..0.5).contains(&dt) {
                    let raw = (dist - last) / dt;
                    if raw.abs() < 80.0 {
                        let target = 343.0 / (343.0 + raw);
                        f += (target - f) * (dt / 0.25).min(1.0);
                    }
                }
            }
            self.doppler = (dist, Some(now), f);
        } else {
            self.doppler = (0.0, None, 1.0);
        }
        self.params = params;
    }

    /// How loud this voice reaches `listener` (its level and distance), to rank voices by.
    pub fn heard_gain(&self, listener: &Listener) -> f32 {
        let spatial = self
            .params
            .position
            .map(|p| spatial::distance_gain(self.params.range, (p - listener.position).length()))
            .unwrap_or(1.0);
        self.params.level.gain() * (1.0 + (spatial - 1.0) * self.params.spatial_blend.clamp(0.0, 1.0))
    }

    /// Move the voice on by `frames` output frames without mixing it (looping or ending as
    /// it would have).
    pub fn skip(&mut self, frames: usize, dev_rate: f64) {
        let nframes = self.clip.frames();
        if nframes == 0 { self.finished = true; return; }
        let target = resample::step(self.params.pitch, self.doppler.2, self.clip.sample_rate, dev_rate);
        if self.cur_step == 0.0 { self.cur_step = target; }
        let k = envelope::coefficient(dev_rate as u32, 0.015) as f64;
        for _ in 0..frames {
            self.cur_step += (target - self.cur_step) * k;
            self.pos += self.cur_step;
            self.envelope.next(dev_rate as u32);
        }
        self.cur_gain = 0.0;
        self.pan_ready = false;
        if self.pos >= nframes as f64 {
            if self.params.looping { self.pos %= nframes as f64; }
            else { self.finished = true; }
        }
        self.finished |= self.envelope.ended();
    }

    /// Stereo is mixed into front L/R; mono is the average of both panned channels.
    /// Additional output channels remain silent (no guessed speaker ordering or LFE).
    /// Returns true when any stream frame underruns; position is held during buffering.
    pub fn render_into(&mut self, out: &mut [f32], ch: usize, rate: u32,
        listener: &Listener, spatializer: &dyn Spatializer) -> bool {
        self.render_with_reverb(out, ch, rate, listener, spatializer, None)
    }

    pub(crate) fn render_with_reverb(&mut self, out: &mut [f32], ch: usize, rate: u32,
        listener: &Listener, spatializer: &dyn Spatializer,
        mut send: Option<&mut [f32]>) -> bool {
        let frames = out.len() / ch;
        let placed = spatializer.place(self.params.position, self.params.range, self.params.pan,
            listener.position, listener.right);
        let distance = 1.0 + (placed.gain - 1.0) * self.params.spatial_blend.clamp(0.0, 1.0);
        if !self.pan_ready {
            self.cur_pan = [placed.left, placed.right];
            self.pan_ready = true;
        }
        let pan_k = envelope::coefficient(rate, 0.025);
        // Clamp the complete OMSI product once, then distance, bus gain, pan, filtering.
        let level = match self.params.level {
            Level::Raw(g) => g * listener.master,
            Level::Omsi { .. } => (self.params.level.product() * listener.master).clamp(0.0, 1.0),
        };
        let target_gain = if level.is_finite() { (level * distance).max(0.0) } else { 0.0 };
        let gain_k = envelope::coefficient(rate, 0.005);
        let reverb_k = envelope::coefficient(rate, 0.02);
        let reverb_target = if send.is_some() && self.params.bus == crate::Bus::Announcement {
            self.params.cabin_reverb.clamp(0.0, 0.5)
        } else { 0.0 };
        let pitch_k = envelope::coefficient(rate, 0.015) as f64;
        self.lp.set_target(self.params.lowpass_hz, frames, rate as f32);
        let target_step = resample::step(self.params.pitch, self.doppler.2, self.clip.sample_rate, rate as f64);
        if self.cur_step == 0.0 { self.cur_step = target_step; }
        let nframes = self.clip.frames();
        let mut stalled = false;
        for f in 0..frames {
            envelope::smooth(&mut self.cur_reverb, reverb_target, reverb_k);
            envelope::smooth(&mut self.cur_gain, target_gain, gain_k);
            envelope::smooth(&mut self.cur_pan[0], placed.left, pan_k);
            envelope::smooth(&mut self.cur_pan[1], placed.right, pan_k);
            self.cur_step += (target_step - self.cur_step) * pitch_k;
            let fade = self.envelope.next(rate);
            if self.envelope.ended() { self.finished = true; break; }
            let (l, r, tail) = if let Some(reader) = self.reader.as_mut() {
                if self.stream.as_ref().is_some_and(|s| s.is_closed()) {
                    // close() also cancels the decoder immediately; fade the last decoded
                    // frame for 3 ms rather than abruptly dropping a radio at full level.
                    self.envelope.stop();
                    (self.stream_last[0], self.stream_last[1], 1.0)
                } else {
                    let Some((l, r)) = reader.next(rate) else {
                        stalled = true; self.cur_gain = 0.0; continue;
                    };
                    self.stream_last = [l, r];
                    (l, r, 1.0)
                }
            } else {
                if nframes == 0 || self.clip.channels == 0 { self.finished = true; break; }
                if self.pos >= nframes as f64 {
                    if !self.params.looping { self.finished = true; break; }
                    self.pos %= nframes as f64;
                }
                let (l, r) = self.kernel.frame_with_seam(&self.clip, self.pos, self.cur_step, self.params.looping, self.seam);
                // Fade a natural one-shot tail in output time; looping clips retain phase.
                let tail = if self.params.looping { 1.0 } else {
                    ((nframes as f64 - self.pos - self.cur_step).max(0.0)
                        / (self.cur_step * rate as f64 * 0.003)).min(1.0) as f32
                };
                self.pos += self.cur_step;
                (l, r, tail)
            };
            let (l, r) = self.lp.process(l, r, 0.0);
            let gain = self.cur_gain * fade * tail;
            let (l, r) = (l * gain * self.cur_pan[0], r * gain * self.cur_pan[1]);
            let wet = if let Some(buffer) = send.as_deref_mut() {
                if ch == 1 { buffer[f] += (l + r) * 0.5 * self.cur_reverb; }
                else { buffer[f * ch] += l * self.cur_reverb; buffer[f * ch + 1] += r * self.cur_reverb; }
                self.cur_reverb
            } else { 0.0 };
            if ch == 1 { out[f] += (l + r) * 0.5 * (1.0 - wet); }
            else { out[f * ch] += l * (1.0 - wet); out[f * ch + 1] += r * (1.0 - wet); }
        }
        // Retire at the exact end even if it coincides with a callback boundary.
        if self.reader.is_none() && !self.params.looping && self.pos >= nframes as f64 {
            self.finished = true;
        }
        stalled
    }

}

/// Read the outward `DOPPLER` switch once, for passing to [`Voice::apply_params`].
pub fn doppler_enabled() -> bool {
    DOPPLER.load(std::sync::atomic::Ordering::Relaxed)
}

#[cfg(test)]
mod tests {
    use super::*;
    use glam::Vec3;

    #[test]
    fn listener_vehicle_keeps_spatial_sound_at_its_original_pitch() {
        let clip = Arc::new(Clip {
            sample_rate: 48_000,
            channels: 1,
            samples: vec![0; 5],
        });
        let params = |position, doppler| VoiceParams {
            position,
            doppler,
            ..Default::default()
        };
        let mut own = Voice::clip_voice(1, clip.clone(), params(None, false).into());
        let mut passing = Voice::clip_voice(2, clip, params(None, false).into());
        let now = Instant::now();
        for (distance, elapsed) in [(2.0, 0), (2.2, 20)] {
            let at = now + std::time::Duration::from_millis(elapsed);
            let position = Some(Vec3::new(distance, 0.0, 0.0));
            own.apply_params(params(position, false).into(), at, Vec3::ZERO, true);
            passing.apply_params(params(position, true).into(), at, Vec3::ZERO, true);
        }
        assert_eq!(own.doppler.2, 1.0);
        assert!(passing.doppler.2 < 1.0);
    }
}

#[cfg(test)]
#[path = "quality_tests.rs"]
mod quality_tests;
