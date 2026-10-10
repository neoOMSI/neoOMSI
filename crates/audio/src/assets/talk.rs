use parking_lot::Mutex;
use std::collections::VecDeque;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::Instant;

pub const RATE: u32 = 48_000;
pub const FRAME: usize = 960;
pub const MAX_PACKET: usize = 400;
const FRAME_S: f64 = 0.02;
const SLOTS: usize = 64;
const MIN_DELAY: f64 = 0.04;
const MAX_DELAY: f64 = 0.12;
const WINDOW: usize = 100;
const END_AFTER: u32 = 5;
const SLACK: f64 = 0.08;

#[derive(Debug, PartialEq)]
pub enum Next<'a> {
    Silence,
    Frame(Vec<u8>, bool),
    Recover(&'a [u8], bool),
    Conceal,
}

pub struct Jitter {
    slots: Vec<Option<(i64, Vec<u8>, bool)>>,
    next: Option<i64>,
    played: i64,
    prime: Option<u32>,
    missing: u32,
    stretch: u32,
    last: Option<(u16, i64, f64)>,
    transit: VecDeque<f64>,
    delay: f64,
}

impl Default for Jitter {
    fn default() -> Self {
        Jitter {
            slots: (0..SLOTS).map(|_| None).collect(),
            next: None,
            played: i64::MIN,
            prime: None,
            missing: 0,
            stretch: 0,
            last: None,
            transit: VecDeque::with_capacity(WINDOW),
            delay: MIN_DELAY,
        }
    }
}

impl Jitter {
    pub fn delay(&self) -> f64 {
        self.delay
    }

    pub fn playing(&self) -> bool {
        self.next.is_some()
    }

    pub fn push(&mut self, seq: u16, data: Vec<u8>, radio: bool, now: f64) {
        if self.last.is_some_and(|l| now - l.2 > 30.0) && self.next.is_none() {
            *self = Jitter::default();
        }
        let ext = match self.last {
            Some((s, e, _)) => e + seq.wrapping_sub(s) as i16 as i64,
            None => seq as i64,
        };
        if self.last.is_none_or(|l| ext > l.1) {
            self.last = Some((seq, ext, now));
        }
        if self.transit.len() == WINDOW {
            self.transit.pop_front();
        }
        self.transit.push_back(now - ext as f64 * FRAME_S);
        let (lo, hi) = self
            .transit
            .iter()
            .fold((f64::MAX, f64::MIN), |(a, b), t| (a.min(*t), b.max(*t)));
        self.delay = (hi - lo + 0.01).clamp(MIN_DELAY, MAX_DELAY);
        if ext <= self.played {
            if self.next.is_some() && ext > self.played - 8 {
                self.stretch = (self.stretch + 1).min(2);
            }
            return;
        }
        if let Some(n) = self.next {
            if ext >= n + SLOTS as i64 {
                self.stop();
            }
        }
        self.slots[ext.rem_euclid(SLOTS as i64) as usize] = Some((ext, data, radio));
        if self.next.is_none() && self.prime.is_none() {
            self.prime = Some((self.delay / FRAME_S - 1e-6).ceil() as u32);
        }
    }

    fn stop(&mut self) {
        self.next = None;
        self.prime = None;
        self.missing = 0;
        self.stretch = 0;
        self.slots.iter_mut().for_each(|s| *s = None);
    }

    fn earliest(&self, from: i64) -> Option<i64> {
        self.slots
            .iter()
            .flatten()
            .map(|s| s.0)
            .filter(|e| *e >= from)
            .min()
    }

    fn held(&self, ext: i64) -> bool {
        self.slots[ext.rem_euclid(SLOTS as i64) as usize]
            .as_ref()
            .is_some_and(|s| s.0 == ext)
    }

    pub fn next_frame(&mut self) -> Next<'_> {
        let n = match (self.next, self.prime) {
            (Some(_), _) if self.stretch > 0 => {
                self.stretch -= 1;
                return Next::Conceal;
            }
            (Some(n), _) => n,
            (None, Some(0)) => {
                self.prime = None;
                match self.earliest(self.played + 1) {
                    Some(e) => e,
                    None => return Next::Silence,
                }
            }
            (None, Some(k)) => {
                self.prime = Some(k - 1);
                return Next::Silence;
            }
            (None, None) => return Next::Silence,
        };
        let waiting = self.slots.iter().flatten().filter(|s| s.0 >= n).count();
        let mut n = n;
        if waiting as f64 * FRAME_S > self.delay + SLACK && self.held(n + 1) {
            self.slots[n.rem_euclid(SLOTS as i64) as usize] = None;
            n += 1;
        }
        self.next = Some(n + 1);
        self.played = n;
        let k = n.rem_euclid(SLOTS as i64) as usize;
        if self.held(n) {
            self.missing = 0;
            let (_, data, radio) = self.slots[k].take().unwrap();
            return Next::Frame(data, radio);
        }
        self.missing += 1;
        match self.earliest(n + 1) {
            None if self.missing > END_AFTER => {
                self.stop();
                Next::Silence
            }
            Some(e) if e == n + 1 => {
                let s = self.slots[(e.rem_euclid(SLOTS as i64)) as usize]
                    .as_ref()
                    .unwrap();
                Next::Recover(&s.1, s.2)
            }
            Some(e) if self.missing >= 3 => {
                self.next = Some(e);
                Next::Conceal
            }
            _ => Next::Conceal,
        }
    }
}

/// A two-pole band-pass, a little drive and hiss: a voice over the dispatch radio.
#[derive(Default)]
struct RadioTone {
    hp: (f32, f32),
    lp: [f32; 2],
    noise: u32,
    crackle: u32,
    hiss: f32,
}

impl RadioTone {
    fn apply(&mut self, pcm: &mut [f32]) {
        let hp_a = (-2.0 * std::f32::consts::PI * 350.0 / RATE as f32).exp();
        let lp_a = 1.0 - (-2.0 * std::f32::consts::PI * 2800.0 / RATE as f32).exp();
        let hiss_a = 1.0 - (-2.0 * std::f32::consts::PI * 2000.0 / RATE as f32).exp();
        for x in pcm {
            let hp = hp_a * (self.hp.1 + *x - self.hp.0);
            self.hp = (*x, hp);
            self.lp[0] += (hp - self.lp[0]) * lp_a;
            self.lp[1] += (self.lp[0] - self.lp[1]) * lp_a;
            self.noise ^= self.noise << 13;
            self.noise ^= self.noise >> 17;
            self.noise ^= self.noise << 5;
            let r = self.noise as f32 / u32::MAX as f32;
            let mut white = (r - 0.5) * 0.06;
            if self.crackle > 0 {
                self.crackle -= 1;
                white *= 3.0;
            } else if r < 0.0002 {
                self.crackle = 60 + self.noise % 400;
            }
            self.hiss += (white - self.hiss) * hiss_a;
            let hiss = self.hiss;
            *x = (self.lp[1] * 2.2).tanh() * 0.8 + hiss;
        }
    }
}

struct Playout {
    jitter: Jitter,
    decoder: Option<opus::Decoder>,
    pcm: Box<[f32; FRAME]>,
    at: usize,
    radio: bool,
    tone: RadioTone,
    phase: f64,
    a: f32,
    b: f32,
}

impl Playout {
    fn refill(&mut self) -> (bool, f32) {
        let Playout {
            jitter,
            decoder,
            pcm,
            ..
        } = self;
        let pcm = &mut pcm[..];
        let mut decode = |data: &[u8], fec: bool| match decoder.as_mut() {
            Some(d) => matches!(d.decode_float(data, pcm, fec), Ok(n) if n == FRAME),
            None => false,
        };
        let (ok, radio) = match jitter.next_frame() {
            Next::Silence => (false, false),
            Next::Frame(data, radio) => (decode(&data, false), radio),
            Next::Recover(data, radio) => (decode(data, true), radio),
            Next::Conceal => (decode(&[], false), self.radio),
        };
        if !ok {
            self.pcm.fill(0.0);
        } else if radio {
            self.tone.apply(&mut self.pcm[..]);
        }
        self.radio = radio;
        self.at = 0;
        let rms = (self.pcm.iter().map(|x| x * x).sum::<f32>() / FRAME as f32).sqrt();
        (ok && self.jitter.playing(), rms)
    }

    fn sample(&mut self, talker: &Talker) -> f32 {
        if self.at >= FRAME {
            let (playing, rms) = self.refill();
            talker
                .speaking
                .store(playing && rms > 0.004, Ordering::Relaxed);
            talker.radio.store(self.radio, Ordering::Relaxed);
        }
        let s = self.pcm[self.at];
        self.at += 1;
        s
    }
}

/// One remote speaker: frames go in from the game (`push`), sound comes out to the mixer.
pub struct Talker {
    play: Mutex<Playout>,
    since: Instant,
    speaking: AtomicBool,
    radio: AtomicBool,
}

impl Default for Talker {
    fn default() -> Self {
        let decoder = opus::Decoder::new(RATE, opus::Channels::Mono)
            .map_err(|e| log::warn!("voice: no Opus decoder: {e}"))
            .ok();
        Talker {
            play: Mutex::new(Playout {
                jitter: Jitter::default(),
                decoder,
                pcm: Box::new([0.0; FRAME]),
                at: FRAME,
                radio: false,
                tone: RadioTone {
                    noise: 0x2545_F491,
                    ..Default::default()
                },
                phase: 0.0,
                a: 0.0,
                b: 0.0,
            }),
            since: Instant::now(),
            speaking: AtomicBool::new(false),
            radio: AtomicBool::new(false),
        }
    }
}

impl Talker {
    pub fn push(&self, seq: u16, data: Vec<u8>, radio: bool) {
        let now = self.since.elapsed().as_secs_f64();
        self.play.lock().jitter.push(seq, data, radio, now);
    }

    pub fn speaking(&self) -> bool {
        self.speaking.load(Ordering::Relaxed)
    }

    pub fn radio(&self) -> bool {
        self.radio.load(Ordering::Relaxed)
    }
}

impl crate::assets::Source for Talker {
    fn read(&self, rate: u32, out: &mut [f32]) {
        let step = RATE as f64 / rate.max(1) as f64;
        let mut p = self.play.lock();
        for o in out {
            while p.phase >= 1.0 {
                p.a = p.b;
                p.b = p.sample(self);
                p.phase -= 1.0;
            }
            *o = p.a + (p.b - p.a) * p.phase as f32;
            p.phase += step;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::assets::Source;

    fn frame(n: u8) -> Vec<u8> {
        vec![n]
    }

    fn played(j: &mut Jitter, frames: usize) -> Vec<String> {
        (0..frames)
            .map(|_| match j.next_frame() {
                Next::Silence => "-".into(),
                Next::Frame(d, _) => d[0].to_string(),
                Next::Recover(d, _) => format!("r{}", d[0]),
                Next::Conceal => "c".into(),
            })
            .collect()
    }

    #[test]
    fn frames_play_in_order_after_the_delay() {
        let mut j = Jitter::default();
        for (seq, t) in [(10u16, 0.0), (12, 0.04), (11, 0.041), (13, 0.06)] {
            j.push(seq, frame(seq as u8), false, t);
        }
        assert_eq!(
            played(&mut j, 6),
            ["-", "-", "10", "11", "12", "13"].map(String::from)
        );
    }

    #[test]
    fn a_lost_frame_is_recovered_or_concealed_and_the_spurt_ends() {
        let mut j = Jitter::default();
        for (seq, t) in [(1u16, 0.0), (3, 0.04), (6, 0.1)] {
            j.push(seq, frame(seq as u8), false, t);
        }
        let got = played(&mut j, 15);
        assert_eq!(
            got[..10],
            ["-", "-", "1", "r3", "3", "c", "r6", "6", "c", "c"].map(String::from)
        );
        assert!(got[10..].iter().all(|s| s == "c" || s == "-"));
        assert!(!j.playing());
    }

    #[test]
    fn late_frames_are_dropped_and_raise_the_delay() {
        let mut j = Jitter::default();
        for k in 0..40u16 {
            j.push(k, frame(k as u8), false, k as f64 * 0.02);
        }
        assert!((j.delay() - MIN_DELAY).abs() < 1e-9);
        played(&mut j, 10);
        j.push(1, frame(1), false, 0.8);
        assert!(j.delay() > 0.1, "{}", j.delay());
        assert!(played(&mut j, 40).iter().all(|s| s != "1"));
    }

    #[test]
    fn a_new_spurt_after_silence_starts_over() {
        let mut j = Jitter::default();
        j.push(5, frame(5), false, 0.0);
        assert_eq!(played(&mut j, 3).last().unwrap(), "5");
        played(&mut j, 10);
        assert!(!j.playing());
        // the microphone ran on: two seconds later its numbers are a hundred further
        j.push(105, frame(105), false, 2.0);
        let got = played(&mut j, 3);
        assert_eq!(got.last().unwrap(), "105", "{got:?}");
    }

    #[test]
    fn numbers_wrap() {
        let mut j = Jitter::default();
        for (k, seq) in [65534u16, 65535, 0, 1].into_iter().enumerate() {
            j.push(seq, frame(k as u8), false, k as f64 * 0.02);
        }
        assert_eq!(
            &played(&mut j, 6)[2..],
            ["0", "1", "2", "3"].map(String::from)
        );
    }

    #[test]
    fn a_backlog_is_caught_up() {
        let mut j = Jitter::default();
        j.push(0, frame(0), false, 0.0);
        played(&mut j, 3);
        // a stall on the way, then everything at once
        for k in 1..20u16 {
            j.push(k, frame(k as u8), false, 0.5);
        }
        let got = played(&mut j, 40);
        let heard = got.iter().filter(|s| s.parse::<u8>().is_ok()).count();
        assert!(heard < 19, "{got:?}");
    }

    /// Opus through a network that loses, delays and reorders frames: the voice keeps
    /// coming through, a little late at most.
    #[test]
    fn a_voice_survives_a_bad_network() {
        let mut enc =
            opus::Encoder::new(RATE, opus::Channels::Mono, opus::Application::Voip).unwrap();
        enc.set_inband_fec(true).unwrap();
        enc.set_packet_loss_perc(15).unwrap();
        let talker = Talker::default();
        let mut rng = 0x1234_5678u32;
        let mut rand = move || {
            rng ^= rng << 13;
            rng ^= rng >> 17;
            rng ^= rng << 5;
            rng as f64 / u32::MAX as f64
        };
        let mut sent: Vec<(f64, u16, Vec<u8>)> = Vec::new();
        let mut tone = [0.0f32; FRAME];
        for k in 0..150u16 {
            for (i, s) in tone.iter_mut().enumerate() {
                let t = (k as usize * FRAME + i) as f32 / RATE as f32;
                *s = (t * 440.0 * std::f32::consts::TAU).sin() * 0.3;
            }
            let mut buf = [0u8; 400];
            let n = enc.encode_float(&tone, &mut buf).unwrap();
            if rand() < 0.1 {
                continue;
            }
            let arrival = k as f64 * 0.02 + 0.03 + rand() * 0.05;
            sent.push((arrival, k, buf[..n].to_vec()));
        }
        sent.sort_by(|a, b| a.0.total_cmp(&b.0));
        let mut out = vec![0.0f32; 480];
        let mut energy = Vec::new();
        let mut next = 0;
        for block in 0..400 {
            let now = block as f64 * 0.01;
            while next < sent.len() && sent[next].0 <= now {
                let (t, seq, data) = sent[next].clone();
                talker.play.lock().jitter.push(seq, data, false, t);
                next += 1;
            }
            talker.read(RATE, &mut out);
            energy.push(out.iter().map(|x| x * x).sum::<f32>() / out.len() as f32);
        }
        let first = energy
            .iter()
            .position(|e| *e > 1e-3)
            .expect("the voice is heard");
        assert!(first <= 15, "starts after {first}0 ms");
        let body = &energy[first + 10..first + 280];
        let quiet = body.iter().filter(|e| **e < 0.005).count();
        assert!(
            quiet * 100 < body.len() * 15,
            "{quiet} of {} blocks dropped out",
            body.len()
        );
        assert!(energy[first + 320..].iter().all(|e| *e < 1e-3), "it ends");
    }

    #[test]
    fn the_radio_tone_stays_bounded() {
        let mut t = RadioTone::default();
        let mut pcm = [1.0f32; FRAME];
        t.apply(&mut pcm);
        assert!(pcm.iter().all(|x| x.abs() <= 1.0 && x.is_finite()));
    }
}
