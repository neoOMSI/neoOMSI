//! The bus radio as live internet radio. OMSI itself plays no music: the buses' radios
//! only set variables that plugins turned into sound - `Snd_Radio` (the cassette player of
//! the stock buses and many mods: 1 while it plays) and the Sound Extension radios'
//! `SndExt_Radio` (the station button pressed, 0 = off) with `SndVol_Radio` (the volume
//! knob). Here those variables tune in a list of internet stations, streamed while they
//! play (see `omsi_audio::radio`); Shift+R steps through the list.
//!
//! The stations are kept in `~/.neoomsi/radio.cfg`, one `name = address` per line
//! (MP3, AAC or Ogg streams, or .m3u/.pls playlists), with `volume = 0..1`; the file is
//! written with a default list the first time.

use glam::Vec3;
use omsi_audio::stream::StreamBuf;
use omsi_audio::{AudioEngine, VoiceId, VoiceParams};
use std::path::PathBuf;
use std::sync::Arc;

const DEFAULT_STATIONS: &[(&str, &str)] = &[
    (
        "radioeins",
        "https://dispatcher.rndfnk.com/rbb/radioeins/live/mp3/mid",
    ),
    (
        "Berlin 88.8",
        "https://dispatcher.rndfnk.com/rbb/rbb888/live/mp3/mid",
    ),
    (
        "Deutschlandfunk",
        "https://st01.sslstream.dlf.de/dlf/01/128/mp3/stream.mp3",
    ),
    ("SWR3", "https://liveradio.swr.de/sw282p3/swr3/play.mp3"),
    (
        "Европа Плюс",
        "https://ep256.hostingradio.ru:8052/europaplus256.mp3",
    ),
    (
        "Русское Радио",
        "https://rusradio.hostingradio.ru/rusradio128.mp3",
    ),
    (
        "Ретро FM",
        "https://retro.hostingradio.ru:8043/retro256.mp3",
    ),
    (
        "Наше Радио",
        "https://nashe1.hostingradio.ru:80/nashe-128.mp3",
    ),
    ("Radio Paradise", "https://stream.radioparadise.com/mp3-128"),
];

fn config_path() -> Option<PathBuf> {
    let home = std::env::var_os("HOME").or_else(|| std::env::var_os("USERPROFILE"))?;
    Some(PathBuf::from(home).join(".neoomsi").join("radio.cfg"))
}

/// The stream addresses an OMSI radio plugin keeps in its text files under `plugins`
/// (SuperRadio's `.opl` and its lists; whatever the layout, a line with an http(s) address is
/// a station, named by the text before the address or else by its host): "neoOMSI does not
/// load the stations I defined in the .opl".
fn plugin_stations(dir: &std::path::Path) -> Vec<(String, String)> {
    let mut out = Vec::new();
    let mut files = Vec::new();
    let mut walk = vec![(dir.to_path_buf(), 0)];
    while let Some((d, depth)) = walk.pop() {
        let Ok(rd) = std::fs::read_dir(&d) else {
            continue;
        };
        for e in rd.flatten() {
            let p = e.path();
            if p.is_dir() && depth < 2 {
                walk.push((p, depth + 1));
            } else if p.extension().and_then(|x| x.to_str()).is_some_and(|x| {
                matches!(
                    x.to_ascii_lowercase().as_str(),
                    "opl" | "cfg" | "ini" | "txt" | "m3u" | "pls"
                )
            }) {
                files.push(p);
            }
        }
    }
    files.sort();
    for f in files {
        let Ok(bytes) = std::fs::read(&f) else {
            continue;
        };
        if bytes.len() > 1 << 20 {
            continue;
        }
        let text = String::from_utf8_lossy(&bytes);
        for line in text.lines() {
            let Some(at) = line.find("http://").or_else(|| line.find("https://")) else {
                continue;
            };
            let url: String = line[at..]
                .chars()
                .take_while(|c| !c.is_whitespace() && !matches!(c, '"' | '\'' | ';' | ',' | '|'))
                .collect();
            let before = line[..at]
                .trim()
                .trim_end_matches(['=', ':', '|', ',', ';', '"', '\'', '\t'])
                .trim();
            let before = before.trim_start_matches(|c: char| {
                c.is_ascii_digit() || matches!(c, '.' | ')' | '-' | ' ')
            });
            let name = if before.is_empty() || before.len() > 60 {
                url.split('/').nth(2).unwrap_or(&url).to_string()
            } else {
                before.to_string()
            };
            if url.len() > 12 && !out.iter().any(|(_, u): &(String, String)| u == &url) {
                out.push((name, url));
            }
        }
    }
    out
}

pub struct Radio {
    stations: Vec<(String, String)>,
    volume: f32,
    /// Shift+R: how far the list is turned from the bus's own station numbers.
    offset: usize,
    playing: Option<Playing>,
}

struct Playing {
    station: usize,
    buf: Arc<StreamBuf>,
    voice: VoiceId,
    /// The status last shown on screen (the song, "no signal" ...).
    shown: String,
}

impl Radio {
    pub fn load(omsi_root: &std::path::Path) -> Radio {
        let mut stations = Vec::new();
        let mut volume = 0.7f32;
        let path = config_path();
        match path.as_ref().and_then(|p| std::fs::read_to_string(p).ok()) {
            Some(text) => {
                for line in text.lines() {
                    let line = line.trim();
                    if line.is_empty() || line.starts_with('#') {
                        continue;
                    }
                    let Some((name, value)) = line.split_once('=') else {
                        continue;
                    };
                    let (name, value) = (name.trim(), value.trim());
                    if name.eq_ignore_ascii_case("volume") {
                        volume = value
                            .parse::<f32>()
                            .map(|v| v.clamp(0.0, 1.0))
                            .unwrap_or(volume);
                    } else if !value.is_empty() {
                        stations.push((name.to_string(), value.to_string()));
                    }
                }
            }
            None => {
                stations = DEFAULT_STATIONS
                    .iter()
                    .map(|(n, u)| (n.to_string(), u.to_string()))
                    .collect();
                if let Some(p) = &path {
                    let mut text = String::from(
                        "# Internet radio for the buses' radios: one station per line, `name = address`\n\
                         # (an MP3, AAC or Ogg stream, or an .m3u/.pls playlist). A radio's station\n\
                         # button n plays the n-th station, a cassette player the first; Shift+R in the\n\
                         # game steps through the list. volume = 0..1.\n\
                         volume = 0.7\n",
                    );
                    for (n, u) in DEFAULT_STATIONS {
                        text.push_str(&format!("{n} = {u}\n"));
                    }
                    let _ = std::fs::create_dir_all(p.parent().unwrap_or(p));
                    let _ = std::fs::write(p, text);
                }
            }
        }
        // the stations of OMSI's radio plugins (SuperRadio and the like), after the own ones
        let own = stations.len();
        for (name, url) in plugin_stations(&omsi_root.join("plugins")) {
            if !stations.iter().any(|(_, u)| u.eq_ignore_ascii_case(&url)) {
                stations.push((name, url));
            }
        }
        if stations.len() > own {
            log::info!(
                "radio: {} stations of the OMSI radio plugins added",
                stations.len() - own
            );
        }
        if !stations.is_empty() {
            log::info!("radio: {} stations", stations.len());
        }
        Radio {
            stations,
            volume,
            offset: 0,
            playing: None,
        }
    }

    /// The station the bus's radio is tuned to, None while it is off.
    fn wanted(&self, v: &omsi_sim::VehicleInstance) -> Option<usize> {
        if self.stations.is_empty() {
            return None;
        }
        let button = match v.var("SndExt_Radio") {
            Some(x) if x >= 0.5 => Some(x.round() as usize - 1),
            _ => None,
        };
        let button = button.or_else(|| v.var("Snd_Radio").filter(|x| *x >= 0.5).map(|_| 0));
        button.map(|b| (b + self.offset) % self.stations.len())
    }

    /// Follow the player's bus radio; a text for the screen when the station or its song
    /// changes.
    pub fn update(
        &mut self,
        audio: &AudioEngine,
        v: &omsi_sim::VehicleInstance,
        inside: bool,
    ) -> Option<String> {
        let wanted = self.wanted(v);
        if self.playing.as_ref().map(|p| p.station) != wanted {
            self.stop(audio);
            if let Some(station) = wanted {
                let (name, url) = &self.stations[station];
                log::info!("radio: station {} {name} ({url})", station + 1);
                let buf = omsi_audio::radio::open(url);
                let voice = audio.play_stream(
                    buf.clone(),
                    VoiceParams {
                        gain: 0.0,
                        ..Default::default()
                    },
                );
                self.playing = Some(Playing {
                    station,
                    buf,
                    voice,
                    shown: String::new(),
                });
            }
        }
        let p = self.playing.as_mut()?;
        // the volume knob, where the radio has one (0..1, some go to 2)
        let knob = v
            .var("SndVol_Radio")
            .map(|x| x.clamp(0.0, 2.0))
            .unwrap_or(1.0);
        let gain = self.volume * knob;
        // heard through the bodywork from outside
        let pos = v.position.as_vec3() + Vec3::Z * 1.5;
        let params = if inside {
            VoiceParams {
                gain,
                ..Default::default()
            }
        } else {
            VoiceParams {
                gain,
                position: Some(pos),
                range: 4.0,
                lowpass_hz: 700.0,
                ..Default::default()
            }
        };
        audio.set_params(p.voice, params);
        let status = p.buf.status();
        if status != p.shown && !status.is_empty() && status != "connecting …" {
            p.shown = status.clone();
            let line = format!(
                "Radio {}: {} - {status}",
                p.station + 1,
                self.stations[p.station].0
            );
            log::info!("{line}");
            return Some(line);
        }
        None
    }

    /// Shift+R: the next station of the list on every button.
    pub fn next_station(&mut self) -> String {
        if self.stations.is_empty() {
            return "No radio stations (see ~/.neoomsi/radio.cfg)".into();
        }
        self.offset = (self.offset + 1) % self.stations.len();
        match &self.playing {
            Some(p) => {
                let next = (p.station + 1) % self.stations.len();
                format!("Radio {}: {}", next + 1, self.stations[next].0)
            }
            None => "The bus radio is off (switch it on in the cockpit)".into(),
        }
    }

    pub fn stop(&mut self, audio: &AudioEngine) {
        if let Some(p) = self.playing.take() {
            p.buf.close();
            audio.stop(p.voice);
        }
    }
}
