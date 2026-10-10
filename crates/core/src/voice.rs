use crate::App;
use crate::humans::{BusId, remote_bus_id};
use audio::VoiceParams;
use audio::mic::{Mic, Transmit};
use audio::talk::Talker;
use glam::DVec3;
use std::sync::Arc;
use std::time::{Duration, Instant};

const NEAR: f64 = 5.0;
const BOOST: f32 = 1.5;

struct Speaker {
    talker: Arc<Talker>,
    voice: audio::VoiceId,
}

#[derive(Default)]
pub(crate) struct VoiceChat {
    mic: Option<Mic>,
    mic_for: String,
    retry: Option<Instant>,
    pub(crate) error: Option<String>,
    transmit: Option<Transmit>,
    speakers: hashbrown::HashMap<u32, Speaker>,
    pub(crate) volume: hashbrown::HashMap<u32, f32>,
    pub(crate) testing: bool,
    volume_session: Option<u64>,
}

impl VoiceChat {
    pub(crate) fn speaking(&self, id: u32) -> bool {
        self.speakers.get(&id).is_some_and(|s| s.talker.speaking())
    }

    /// Our microphone goes out now: over the radio, or not.
    pub(crate) fn sending(&self) -> Option<bool> {
        self.mic
            .as_ref()
            .filter(|m| m.sending())
            .map(|_| self.transmit == Some(Transmit::Radio))
    }

    fn volume(&self, id: u32) -> f32 {
        self.volume.get(&id).copied().unwrap_or(1.0)
    }

    pub(crate) fn level(&self) -> Option<f32> {
        self.mic.as_ref().map(|m| m.level())
    }

    /// Player ids are handed out again in the next session, so the volumes set for the
    /// players of one session are forgotten when it ends or another one begins.
    fn in_session(&mut self, session: Option<u64>) {
        if self.volume_session != session {
            self.volume.clear();
            self.volume_session = session;
        }
    }

    fn silence(&mut self, audio: Option<&audio::AudioEngine>) {
        for (_, s) in self.speakers.drain() {
            if let Some(a) = audio {
                a.stop(s.voice);
            }
        }
    }
}

struct Prefs {
    on: bool,
    auto: bool,
    mic: String,
    denoise: bool,
    volume: f32,
    gain: f32,
    sensitivity: f32,
}

fn prefs() -> Prefs {
    let f = |k: &str, d: f64| ::config::get_float("voice", k).unwrap_or(d) as f32;
    Prefs {
        on: ::config::get_bool("voice", "enabled").unwrap_or(true),
        auto: ::config::get_string("voice", "mode").as_deref() == Some("auto"),
        mic: ::config::get_string("voice", "mic").unwrap_or_default(),
        denoise: ::config::get_bool("voice", "denoise").unwrap_or(true),
        volume: f("volume", 1.0),
        gain: f("mic_gain", 1.0),
        sensitivity: f("sensitivity", 0.5),
    }
}

pub(crate) fn proximity(d: f64, range: f64) -> f32 {
    let t = ((d - NEAR) / (range - NEAR).max(1.0)).clamp(0.0, 1.0);
    (1.0 - t).powf(1.5) as f32
}

pub(crate) fn through(open: &[f32]) -> (f32, f32) {
    open.iter().fold((1.0, 0.0), |(g, lp), o| {
        let o = o.clamp(0.0, 1.0);
        let wall = 7000.0 + 8000.0 * o;
        let lp = if lp == 0.0 { wall } else { lp.min(wall) * 0.9 };
        (g * (0.95 + 0.05 * o), lp)
    })
}

/// Where a player's mouth is, and the bus (its player) they are in.
struct Spot {
    at: DVec3,
    bus: Option<u32>,
}

impl App {
    pub(crate) fn action_held(&self, action: &str, exact: bool) -> bool {
        let chord = content::input::chord(
            crate::startup::shift_held_now(&self.keys),
            self.ctrl_held(),
            self.alt_held(),
        );
        self.game_keys
            .iter()
            .filter(|b| b.action.eq_ignore_ascii_case(action) && b.scan_code > 0)
            .filter(|b| !exact || b.matches(chord))
            .any(|b| {
                self.keys
                    .iter()
                    .any(|k| crate::keys::dik_code(*k) == Some(b.scan_code))
            })
    }

    fn spot_of(&self, id: u32, my_id: u32) -> Option<Spot> {
        let lan = self.lan.as_ref()?;
        let (pose, bus_at) = match self.remotes.remotes.get(&id) {
            Some(r) => (&r.last, Some(r.vehicle().position)),
            None => (
                &lan.peers().find(|p| p.pose.id == id && p.has_pose)?.pose,
                None,
            ),
        };
        match pose.walker {
            Some(w) => match w.aboard {
                Some(a) => {
                    let bus = if a.owner == my_id {
                        BusId::Player
                    } else {
                        BusId::Ai(remote_bus_id(a.owner))
                    };
                    let floor = self
                        .humans
                        .as_ref()
                        .and_then(|h| h.cabin_world(bus, glam::Vec3::from(a.local)))
                        .map(|c| c.0)
                        .unwrap_or(DVec3::new(w.x, w.y, w.z));
                    Some(Spot {
                        at: floor + DVec3::Z * if w.seated { 1.2 } else { 1.6 },
                        bus: Some(a.owner),
                    })
                }
                None => Some(Spot {
                    at: DVec3::new(w.x, w.y, w.z + 1.6),
                    bus: None,
                }),
            },
            None if pose.flags & network::FLAG_VEHICLE != 0 => Some(Spot {
                at: bus_at.unwrap_or(DVec3::new(pose.x, pose.y, pose.z)) + DVec3::Z * 1.8,
                bus: Some(id),
            }),
            None => None,
        }
    }

    /// How far the doors of player `bus`'s bus are open (the widest).
    fn doors_open(&self, bus: u32, my_id: u32) -> f32 {
        let widest = |d: &mut dyn Iterator<Item = f32>| d.fold(0.0f32, f32::max);
        if bus == my_id {
            return self
                .player
                .as_ref()
                .map(|p| {
                    widest(
                        &mut (0..network::wire::MAX_DOORS)
                            .filter_map(|i| p.vehicle.var(&format!("door_{i}"))),
                    )
                })
                .unwrap_or(0.0);
        }
        self.remotes
            .remotes
            .get(&bus)
            .map(|r| widest(&mut r.last.doors.iter().copied()))
            .unwrap_or(0.0)
    }

    pub(crate) fn tick_voice(&mut self) {
        self.voice.testing = self.lab_voice_shown();
        let audio_on = self.audio.as_ref().is_some_and(|a| a.enabled);
        let allowed = self.lan.as_ref().is_some_and(|l| l.voice_allowed());
        let prefs = prefs();
        let on = prefs.on && audio_on;
        let incoming = self
            .lan
            .as_mut()
            .map(|l| l.take_voice())
            .unwrap_or_default();
        self.voice.in_session(self.lan.as_ref().map(|l| l.session));
        if !on || self.lan.is_none() {
            self.voice.silence(self.audio.as_ref());
        }
        self.tick_mic(on && (allowed || self.voice.testing), allowed, &prefs);
        let (Some(lan), Some(audio), true) = (self.lan.as_ref(), self.audio.as_ref(), on) else {
            return;
        };
        for f in incoming {
            if self.voice.volume(f.id) <= 0.0 {
                continue;
            }
            let radio = f.radio();
            let s = self.voice.speakers.entry(f.id).or_insert_with(|| {
                let talker = Arc::new(Talker::default());
                let voice = audio.play_source(
                    talker.clone(),
                    VoiceParams {
                        gain: 0.0,
                        ..Default::default()
                    },
                );
                Speaker { talker, voice }
            });
            s.talker.push(f.seq, f.data, radio);
        }
        let here: hashbrown::HashSet<u32> = lan.peers().map(|p| p.pose.id).collect();
        let volume = &self.voice.volume;
        self.voice.speakers.retain(|id, s| {
            let keep = here.contains(id) && volume.get(id).is_none_or(|v| *v > 0.0);
            if !keep {
                audio.stop(s.voice);
            }
            keep
        });
        let my_id = lan.my_id;
        let range = lan.voice.range as f64;
        let ear = self
            .camera
            .as_ref()
            .map(|c| c.position)
            .unwrap_or(DVec3::ZERO);
        let my_bus = if self.in_cab {
            Some(my_id)
        } else {
            self.inside_remote
        };
        for (id, s) in &self.voice.speakers {
            let gain = BOOST * prefs.volume * self.voice.volume(*id);
            let params = if s.talker.radio() {
                VoiceParams {
                    gain,
                    doppler: false,
                    important: true,
                    ..Default::default()
                }
            } else {
                match self.spot_of(*id, my_id) {
                    Some(spot) => {
                        let d = (spot.at - ear).length();
                        let fade = proximity(d, range);
                        let open: Vec<f32> = if spot.bus == my_bus {
                            Vec::new()
                        } else {
                            [spot.bus, my_bus]
                                .into_iter()
                                .flatten()
                                .map(|b| self.doors_open(b, my_id))
                                .collect()
                        };
                        let (wall, lowpass_hz) = through(&open);
                        VoiceParams {
                            gain: gain * fade * wall,
                            position: Some(spot.at.as_vec3()),
                            range: range as f32,
                            lowpass_hz,
                            doppler: false,
                            important: true,
                            ..Default::default()
                        }
                    }
                    None => VoiceParams {
                        gain: 0.0,
                        ..Default::default()
                    },
                }
            };
            audio.set_params(s.voice, params);
        }
    }

    fn tick_mic(&mut self, wanted: bool, allowed: bool, prefs: &Prefs) {
        if !wanted {
            self.voice.mic = None;
            self.voice.transmit = None;
            return;
        }
        let now = Instant::now();
        if self
            .voice
            .mic
            .as_ref()
            .is_some_and(|m| m.lost() || self.voice.mic_for != prefs.mic)
        {
            self.voice.mic = None;
            self.voice.retry = Some(now + Duration::from_secs(1));
        }
        if self.voice.mic.is_none() && self.voice.retry.is_none_or(|t| now >= t) {
            self.voice.retry = Some(now + Duration::from_secs(5));
            #[cfg(target_os = "android")]
            if !crate::android::microphone_allowed() {
                self.voice.error = Some(::i18n::translate("pause.msg.voice_mic_denied", &[]));
                return;
            }
            match Mic::open(&prefs.mic) {
                Ok(m) => {
                    self.voice.mic = Some(m);
                    self.voice.mic_for = prefs.mic.clone();
                    self.voice.error = None;
                }
                Err(e) => {
                    if self.voice.error.as_deref() != Some(e.as_str()) {
                        log::warn!("voice: cannot open the microphone: {e}");
                    }
                    self.voice.error = Some(e);
                }
            }
        }
        let Some(mic) = self.voice.mic.as_ref() else {
            return;
        };
        mic.configure(prefs.gain, prefs.sensitivity, prefs.denoise);
        // (testing on the options page outside a session: the gate works, nothing is sent)
        let t = if !allowed && !self.voice.testing {
            Transmit::Off
        } else if self.action_held("voice_radio", true) {
            Transmit::Radio
        } else if prefs.auto {
            Transmit::Auto
        } else if self.action_held("voice_talk", false) {
            Transmit::Talk
        } else {
            Transmit::Off
        };
        mic.set_transmit(t);
        self.voice.transmit = Some(t);
        let frames = mic.take();
        if let (Some(lan), true) = (self.lan.as_mut(), allowed) {
            for f in frames {
                let flags = if f.radio {
                    network::voice::FLAG_RADIO
                } else {
                    0
                };
                lan.send_voice(f.seq, flags, &f.data);
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::{VoiceChat, proximity, through};

    #[test]
    fn player_volumes_do_not_outlive_their_session() {
        let mut v = VoiceChat::default();
        v.in_session(Some(7));
        v.volume.insert(2, 0.0);
        v.in_session(Some(7));
        assert_eq!(v.volume.get(&2), Some(&0.0));
        v.in_session(None);
        assert!(v.volume.is_empty());
        v.volume.insert(2, 0.0);
        v.in_session(Some(8));
        assert!(v.volume.is_empty());
    }

    #[test]
    fn a_voice_carries_across_a_stop_and_is_faint_near_the_range() {
        assert_eq!(proximity(3.0, 60.0), 1.0);
        assert!(proximity(20.0, 60.0) > 0.5);
        let far = proximity(50.0, 60.0);
        assert!(far > 0.0 && far < 0.12, "{far}");
        assert_eq!(proximity(60.0, 60.0), 0.0);
    }

    #[test]
    fn bodywork_muffles_less_with_the_doors_open() {
        assert_eq!(through(&[]), (1.0, 0.0));
        let (shut, shut_lp) = through(&[0.0]);
        let (open, open_lp) = through(&[1.0]);
        assert!((shut - 0.95).abs() < 1e-6 && shut_lp < 8000.0);
        assert!(open > 0.99 && open_lp > 12000.0);
        let (two, two_lp) = through(&[0.0, 0.0]);
        assert!(two < shut && two_lp < shut_lp);
    }
}
