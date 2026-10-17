use cpal::traits::{DeviceTrait, HostTrait, StreamTrait};
use nnnoiseless::DenoiseState;
use parking_lot::Mutex;
use std::collections::VecDeque;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, AtomicU8, AtomicU32, Ordering};

use crate::assets::talk::{FRAME, RATE};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(u8)]
pub enum Transmit {
    Off,
    Talk,
    Radio,
    Auto,
}

pub struct Captured {
    pub seq: u16,
    pub radio: bool,
    pub data: Vec<u8>,
}

const BITRATE: i32 = 24_000;
const PTT_TAIL: u32 = 5;
const VAD_HOLD: u32 = 20;
const MAX_QUEUE: usize = 50;

struct Shared {
    transmit: AtomicU8,
    gain: AtomicU32,
    threshold: AtomicU32,
    denoise: AtomicBool,
    level: AtomicU32,
    sending: AtomicBool,
    lost: AtomicBool,
    out: Mutex<VecDeque<Captured>>,
}

fn load(a: &AtomicU32) -> f32 {
    f32::from_bits(a.load(Ordering::Relaxed))
}

fn store(a: &AtomicU32, v: f32) {
    a.store(v.to_bits(), Ordering::Relaxed);
}

struct Chain {
    shared: Arc<Shared>,
    rate: u32,
    channels: usize,
    phase: f64,
    last: f32,
    chunk: Vec<f32>,
    frame: Vec<f32>,
    previous: Vec<f32>,
    denoiser: Box<DenoiseState<'static>>,
    denoised: Vec<f32>,
    speech: f32,
    encoder: opus::Encoder,
    packet: Vec<u8>,
    seq: u16,
    hold: u32,
    tail: (u32, bool),
    meter: f32,
}

impl Chain {
    fn new(shared: Arc<Shared>, rate: u32, channels: usize) -> Result<Chain, String> {
        let mut encoder = opus::Encoder::new(RATE, opus::Channels::Mono, opus::Application::Voip)
            .map_err(|e| format!("no Opus encoder: {e}"))?;
        let _ = encoder.set_bitrate(opus::Bitrate::Bits(BITRATE));
        let _ = encoder.set_inband_fec(true);
        let _ = encoder.set_packet_loss_perc(10);
        let _ = encoder.set_signal(opus::Signal::Voice);
        let _ = encoder.set_complexity(if cfg!(target_os = "android") { 5 } else { 8 });
        Ok(Chain {
            shared,
            rate: rate.max(8000),
            channels: channels.max(1),
            phase: 0.0,
            last: 0.0,
            chunk: Vec::with_capacity(DenoiseState::FRAME_SIZE),
            frame: Vec::with_capacity(FRAME),
            previous: vec![0.0; FRAME],
            denoiser: DenoiseState::new(),
            denoised: vec![0.0; DenoiseState::FRAME_SIZE],
            speech: 0.0,
            encoder,
            packet: vec![0; crate::assets::talk::MAX_PACKET],
            seq: 0,
            hold: 0,
            tail: (0, false),
            meter: 0.0,
        })
    }

    /// Interleaved samples of the device, as floats.
    fn take(&mut self, samples: impl Iterator<Item = f32>) {
        let step = self.rate as f64 / RATE as f64;
        let ch = self.channels;
        let mut acc = 0.0;
        for (i, s) in samples.enumerate() {
            acc += s;
            if i % ch != ch - 1 {
                continue;
            }
            let x = acc / ch as f32;
            acc = 0.0;
            while self.phase < 1.0 {
                let y = self.last + (x - self.last) * self.phase as f32;
                self.push(y);
                self.phase += step;
            }
            self.phase -= 1.0;
            self.last = x;
        }
    }

    fn push(&mut self, x: f32) {
        self.chunk.push(x * load(&self.shared.gain));
        if self.chunk.len() < DenoiseState::FRAME_SIZE {
            return;
        }
        // (RNNoise works on 16-bit sample values)
        self.chunk.iter_mut().for_each(|s| *s *= 32768.0);
        let p = self.denoiser.process_frame(&mut self.denoised, &self.chunk);
        self.speech = self.speech.max(p);
        let clean = self.shared.denoise.load(Ordering::Relaxed);
        let src = if clean { &self.denoised } else { &self.chunk };
        self.frame
            .extend(src.iter().map(|s| (s / 32768.0).clamp(-1.0, 1.0)));
        self.chunk.clear();
        if self.frame.len() >= FRAME {
            self.frame_done();
            self.frame.clear();
        }
    }

    fn frame_done(&mut self) {
        self.seq = self.seq.wrapping_add(1);
        let rms = (self.frame.iter().map(|x| x * x).sum::<f32>() / FRAME as f32).sqrt();
        let db = 20.0 * rms.max(1e-6).log10();
        let level = ((db + 60.0) / 60.0).clamp(0.0, 1.0);
        self.meter = if level > self.meter {
            level
        } else {
            self.meter * 0.85 + level * 0.15
        };
        store(&self.shared.level, self.meter);
        let speech = std::mem::take(&mut self.speech);
        let mode = self.shared.transmit.load(Ordering::Relaxed);
        let mut onset = false;
        let send = match mode {
            m if m == Transmit::Talk as u8 || m == Transmit::Radio as u8 => {
                self.tail = (PTT_TAIL, m == Transmit::Radio as u8);
                Some(self.tail.1)
            }
            m if m == Transmit::Auto as u8 => {
                if speech >= load(&self.shared.threshold) {
                    onset = self.hold == 0;
                    self.hold = VAD_HOLD;
                }
                self.hold = self.hold.saturating_sub(1);
                (self.hold > 0 || onset).then_some(false)
            }
            _ => {
                self.hold = 0;
                self.tail.0 = self.tail.0.saturating_sub(1);
                (self.tail.0 > 0).then_some(self.tail.1)
            }
        };
        self.shared.sending.store(send.is_some(), Ordering::Relaxed);
        if let Some(radio) = send {
            // (voice activation hears a word once it has begun: its first 20 ms go too)
            if onset {
                let prev = std::mem::take(&mut self.previous);
                self.encode(&prev, self.seq.wrapping_sub(1), radio);
                self.previous = prev;
            }
            let frame = std::mem::take(&mut self.frame);
            self.encode(&frame, self.seq, radio);
            self.frame = frame;
        }
        self.previous.clear();
        self.previous.extend_from_slice(&self.frame[..FRAME]);
    }

    fn encode(&mut self, pcm: &[f32], seq: u16, radio: bool) {
        let Ok(n) = self.encoder.encode_float(&pcm[..FRAME], &mut self.packet) else {
            return;
        };
        let mut q = self.shared.out.lock();
        if q.len() >= MAX_QUEUE {
            q.pop_front();
        }
        q.push_back(Captured {
            seq,
            radio,
            data: self.packet[..n].to_vec(),
        });
    }
}

/// An open microphone. Dropping it closes it.
pub struct Mic {
    _stream: cpal::Stream,
    shared: Arc<Shared>,
    pub device: String,
}

pub fn input_devices() -> Vec<String> {
    let Ok(devices) = cpal::default_host().input_devices() else {
        return Vec::new();
    };
    devices
        .filter_map(|d| d.description().ok().map(|d| d.name().to_string()))
        .collect()
}

impl Mic {
    /// The input device called `name` (the system's default when it is empty or gone).
    pub fn open(name: &str) -> Result<Mic, String> {
        let host = cpal::default_host();
        let named = (!name.is_empty())
            .then(|| {
                host.input_devices()
                    .ok()?
                    .find(|d| d.description().map(|x| x.name() == name).unwrap_or(false))
            })
            .flatten();
        let dev = named
            .or_else(|| host.default_input_device())
            .ok_or("no microphone")?;
        let device = dev
            .description()
            .map(|d| d.name().to_string())
            .unwrap_or_default();
        let cfg = dev
            .default_input_config()
            .map_err(|e| format!("{device}: {e}"))?;
        let shared = Arc::new(Shared {
            transmit: AtomicU8::new(Transmit::Off as u8),
            gain: AtomicU32::new(1.0f32.to_bits()),
            threshold: AtomicU32::new(0.6f32.to_bits()),
            denoise: AtomicBool::new(true),
            level: AtomicU32::new(0),
            sending: AtomicBool::new(false),
            lost: AtomicBool::new(false),
            out: Mutex::new(VecDeque::new()),
        });
        let chain = Chain::new(shared.clone(), cfg.sample_rate(), cfg.channels() as usize)?;
        let lost = shared.clone();
        let on_error = move |e: cpal::Error| {
            if e.kind() == cpal::ErrorKind::DeviceNotAvailable {
                lost.lost.store(true, Ordering::Relaxed);
            }
            log::warn!("microphone: {e}");
        };
        use cpal::SampleFormat as F;
        let stream = match cfg.sample_format() {
            F::I16 => build::<i16>(&dev, &cfg, chain, on_error),
            F::I32 => build::<i32>(&dev, &cfg, chain, on_error),
            F::U16 => build::<u16>(&dev, &cfg, chain, on_error),
            _ => build::<f32>(&dev, &cfg, chain, on_error),
        }
        .map_err(|e| format!("{device}: {e}"))?;
        stream.play().map_err(|e| format!("{device}: {e}"))?;
        log::info!(
            "voice: microphone {device} ({} Hz, {} channel(s))",
            cfg.sample_rate(),
            cfg.channels()
        );
        Ok(Mic {
            _stream: stream,
            shared,
            device,
        })
    }

    pub fn set_transmit(&self, t: Transmit) {
        self.shared.transmit.store(t as u8, Ordering::Relaxed);
    }

    pub fn configure(&self, gain: f32, sensitivity: f32, denoise: bool) {
        store(&self.shared.gain, gain.clamp(0.0, 8.0));
        store(
            &self.shared.threshold,
            0.95 - sensitivity.clamp(0.0, 1.0) * 0.85,
        );
        self.shared.denoise.store(denoise, Ordering::Relaxed);
    }

    pub fn take(&self) -> Vec<Captured> {
        self.shared.out.lock().drain(..).collect()
    }

    /// The input level, 0 (-60 dB and below) to 1 (full scale).
    pub fn level(&self) -> f32 {
        load(&self.shared.level)
    }

    pub fn sending(&self) -> bool {
        self.shared.sending.load(Ordering::Relaxed)
    }

    /// The device went away (unplugged): open the microphone again.
    pub fn lost(&self) -> bool {
        self.shared.lost.load(Ordering::Relaxed)
    }
}

fn build<T>(
    dev: &cpal::Device,
    cfg: &cpal::SupportedStreamConfig,
    mut chain: Chain,
    on_error: impl FnMut(cpal::Error) + Send + 'static,
) -> Result<cpal::Stream, cpal::Error>
where
    T: cpal::SizedSample,
    f32: cpal::FromSample<T>,
{
    dev.build_input_stream::<T, _, _>(
        cfg.config(),
        move |data: &[T], _| chain.take(data.iter().map(|s| s.to_sample::<f32>())),
        on_error,
        None,
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    fn chain() -> Chain {
        let shared = Arc::new(Shared {
            transmit: AtomicU8::new(Transmit::Off as u8),
            gain: AtomicU32::new(1.0f32.to_bits()),
            threshold: AtomicU32::new(0.6f32.to_bits()),
            denoise: AtomicBool::new(true),
            level: AtomicU32::new(0),
            sending: AtomicBool::new(false),
            lost: AtomicBool::new(false),
            out: Mutex::new(VecDeque::new()),
        });
        Chain::new(shared, 44_100, 2).unwrap()
    }

    fn feed(c: &mut Chain, seconds: f32) {
        let n = (44_100.0 * seconds) as usize;
        let s = (0..n).flat_map(|i| {
            let v = (i as f32 / 44_100.0 * 300.0 * std::f32::consts::TAU).sin() * 0.2;
            [v, v]
        });
        c.take(s);
    }

    #[test]
    fn push_to_talk_sends_numbered_frames_and_a_tail() {
        let mut c = chain();
        c.shared.denoise.store(false, Ordering::Relaxed);
        feed(&mut c, 0.2);
        assert!(c.shared.out.lock().is_empty(), "nothing goes out unasked");
        c.shared
            .transmit
            .store(Transmit::Radio as u8, Ordering::Relaxed);
        feed(&mut c, 0.2);
        c.shared
            .transmit
            .store(Transmit::Off as u8, Ordering::Relaxed);
        feed(&mut c, 0.4);
        let out: Vec<Captured> = c.shared.out.lock().drain(..).collect();
        assert!((13..=16).contains(&out.len()), "{} frames", out.len());
        assert!(
            out.iter()
                .all(|f| f.radio && !f.data.is_empty() && f.data.len() < 200)
        );
        assert!(out.windows(2).all(|w| w[1].seq == w[0].seq.wrapping_add(1)));
        // (the frames not sent were counted all the same)
        assert!(out[0].seq >= 10, "{}", out[0].seq);
        assert!(c.shared.level.load(Ordering::Relaxed) > 0);
    }

    #[test]
    fn voice_activation_stays_shut_on_silence() {
        let mut c = chain();
        c.shared
            .transmit
            .store(Transmit::Auto as u8, Ordering::Relaxed);
        c.take(std::iter::repeat_n(0.0, 44_100));
        assert!(c.shared.out.lock().is_empty());
        assert!(!c.shared.sending.load(Ordering::Relaxed));
    }
}
