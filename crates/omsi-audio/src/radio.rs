//! Internet radio: a station's stream read over HTTP(S) and decoded (MP3, AAC in ADTS, Ogg
//! Vorbis) on a thread of its own into a `StreamBuf` the mixer plays. Nothing is downloaded
//! ahead: it is live, like a car radio. Playlist addresses (.m3u, .pls) are followed, the
//! song titles a Shoutcast/Icecast server sends in between (ICY metadata) are taken out of
//! the sound and kept as the status, and a connection that drops is made again.

use crate::stream::StreamBuf;
use std::io::Read;
use std::sync::Arc;
use std::time::Duration;
use symphonia::core::codecs::audio::AudioDecoderOptions;
use symphonia::core::formats::probe::Hint;
use symphonia::core::formats::{FormatOptions, FormatReader, TrackType};
use symphonia::core::io::{MediaSourceStream, ReadOnlySource};
use symphonia::core::meta::MetadataOptions;

/// Seconds of sound read ahead at most; a server that sends a burst at the start (many do,
/// to fill a player's buffer) is simply read more slowly.
const AHEAD: f32 = 8.0;

/// Start playing `url` into a new buffer; `close()` on it ends the reader.
pub fn open(url: &str) -> Arc<StreamBuf> {
    let buf = Arc::new(StreamBuf::default());
    let (b, url) = (buf.clone(), url.to_string());
    let spawned = std::thread::Builder::new()
        .name("radio".into())
        .spawn(move || run(&b, &url));
    if let Err(e) = spawned {
        buf.set_status(format!("cannot start the radio: {e}"));
    }
    buf
}

fn run(buf: &StreamBuf, url: &str) {
    // (no timeout on the body: a live stream never ends)
    let agent: ureq::Agent = ureq::Agent::config_builder()
        .timeout_connect(Some(Duration::from_secs(8)))
        .timeout_recv_response(Some(Duration::from_secs(15)))
        .user_agent(concat!("neoOMSI/", env!("CARGO_PKG_VERSION")))
        .build()
        .into();
    let mut wait = 2u64;
    while !buf.is_closed() {
        buf.set_status("connecting …");
        match play(&agent, buf, url) {
            Ok(()) => wait = 2,
            Err(e) => {
                if buf.is_closed() {
                    return;
                }
                log::warn!("radio {url}: {e}");
                buf.set_status(format!("no signal ({e})"));
            }
        }
        // the connection ended or failed: try again, less often the longer it fails
        for _ in 0..wait * 10 {
            if buf.is_closed() {
                return;
            }
            std::thread::sleep(Duration::from_millis(100));
        }
        wait = (wait * 2).min(30);
    }
}

/// Where the sound is: `url` itself, or the first address of the playlist it names.
fn resolve(agent: &ureq::Agent, url: &str) -> anyhow::Result<ureq::http::Response<ureq::Body>> {
    let mut url = url.to_string();
    for _ in 0..4 {
        let resp = agent.get(&url).header("Icy-MetaData", "1").call()?;
        let ctype = resp.body().mime_type().unwrap_or("").to_ascii_lowercase();
        let lower = url.to_ascii_lowercase();
        let playlist = ctype.contains("mpegurl")
            || ctype.contains("scpls")
            || ctype.starts_with("text/")
            || lower.ends_with(".m3u")
            || lower.ends_with(".m3u8")
            || lower.ends_with(".pls");
        if !playlist {
            return Ok(resp);
        }
        let mut text = String::new();
        resp.into_body()
            .into_reader()
            .take(64 * 1024)
            .read_to_string(&mut text)?;
        // .m3u: the first line that is an address; .pls: File1=address
        let next = text
            .lines()
            .map(|l| l.trim())
            .map(|l| {
                l.split_once('=')
                    .filter(|(k, _)| k.to_ascii_lowercase().starts_with("file"))
                    .map(|(_, v)| v.trim())
                    .unwrap_or(l)
            })
            .find(|l| l.starts_with("http://") || l.starts_with("https://"))
            .ok_or_else(|| anyhow::anyhow!("the playlist names no stream"))?;
        url = next.to_string();
    }
    anyhow::bail!("playlists nested too deep")
}

fn play(agent: &ureq::Agent, buf: &StreamBuf, url: &str) -> anyhow::Result<()> {
    let resp = resolve(agent, url)?;
    let ctype = resp.body().mime_type().unwrap_or("").to_ascii_lowercase();
    let header = |k: &str| {
        resp.headers()
            .get(k)
            .and_then(|v| v.to_str().ok())
            .map(|v| v.trim().to_string())
    };
    let metaint = header("icy-metaint").and_then(|v| v.parse::<usize>().ok());
    let name = header("icy-name").unwrap_or_default();
    let reader: Box<dyn Read + Send + Sync> = Box::new(resp.into_body().into_reader());
    let title = Arc::new(parking_lot::Mutex::new(String::new()));
    let reader: Box<dyn Read + Send + Sync> = match metaint {
        Some(n) if n > 0 => Box::new(IcyReader {
            inner: reader,
            every: n,
            left: n,
            title: title.clone(),
        }),
        _ => reader,
    };
    let mss = MediaSourceStream::new(Box::new(ReadOnlySource::new(reader)), Default::default());
    let mut hint = Hint::new();
    if ctype.contains("mpeg") || ctype.contains("mp3") {
        hint.with_extension("mp3");
    } else if ctype.contains("aac") {
        hint.with_extension("aac");
    } else if ctype.contains("ogg") {
        hint.with_extension("ogg");
    }
    // AAC is read as ADTS straight away: probed, its frame headers pass for MPEG audio and
    // the MP3 reader takes it
    let mut format: Box<dyn FormatReader> = if ctype.contains("aac") {
        Box::new(symphonia::default::formats::AdtsReader::try_new(
            mss,
            FormatOptions::default(),
        )?)
    } else {
        symphonia::default::get_probe().probe(
            &hint,
            mss,
            FormatOptions::default(),
            MetadataOptions::default(),
        )?
    };
    let (track_id, mut decoder) = {
        let track = format
            .default_track(TrackType::Audio)
            .ok_or_else(|| anyhow::anyhow!("no audio in the stream"))?;
        let params = track
            .codec_params
            .as_ref()
            .and_then(|p| p.audio())
            .ok_or_else(|| anyhow::anyhow!("no audio in the stream"))?;
        (
            track.id,
            symphonia::default::get_codecs()
                .make_audio_decoder(params, &AudioDecoderOptions::default())?,
        )
    };
    let mut samples: Vec<f32> = Vec::new();
    let mut shown = String::new();
    loop {
        if buf.is_closed() {
            return Ok(());
        }
        while buf.buffered() > AHEAD && !buf.is_closed() {
            std::thread::sleep(Duration::from_millis(100));
        }
        let packet = match format.next_packet() {
            Ok(Some(p)) => p,
            Ok(None) => return Ok(()),
            Err(symphonia::core::errors::Error::IoError(e))
                if e.kind() == std::io::ErrorKind::UnexpectedEof =>
            {
                return Ok(());
            }
            Err(e) => return Err(e.into()),
        };
        if packet.track_id != track_id {
            continue;
        }
        let decoded = match decoder.decode(&packet) {
            Ok(d) => d,
            // a damaged frame (a stream joined mid-frame, a glitch): skip it
            Err(symphonia::core::errors::Error::DecodeError(_)) => continue,
            Err(e) => return Err(e.into()),
        };
        let rate = decoded.spec().rate();
        let ch = decoded.spec().channels().count().max(1);
        samples.resize(decoded.samples_interleaved(), 0.0);
        decoded.copy_to_slice_interleaved(&mut samples);
        buf.push(
            rate,
            samples
                .chunks_exact(ch)
                .map(|f| if ch >= 2 { [f[0], f[1]] } else { [f[0], f[0]] }),
        );
        let now = {
            let t = title.lock();
            if t.is_empty() {
                name.clone()
            } else {
                t.clone()
            }
        };
        let status = if buf.is_playing() {
            now
        } else {
            "buffering …".to_string()
        };
        if status != shown {
            buf.set_status(status.clone());
            shown = status;
        }
    }
}

/// The sound of a stream with ICY metadata: every `every` bytes of sound the server puts
/// one length byte (×16) and that many bytes of `StreamTitle='…';` text.
struct IcyReader {
    inner: Box<dyn Read + Send + Sync>,
    every: usize,
    left: usize,
    title: Arc<parking_lot::Mutex<String>>,
}

impl Read for IcyReader {
    fn read(&mut self, out: &mut [u8]) -> std::io::Result<usize> {
        if self.left == 0 {
            let mut len = [0u8; 1];
            self.inner.read_exact(&mut len)?;
            let n = len[0] as usize * 16;
            if n > 0 {
                let mut meta = vec![0u8; n];
                self.inner.read_exact(&mut meta)?;
                let text = String::from_utf8_lossy(&meta);
                if let Some(start) = text.find("StreamTitle='") {
                    let rest = &text[start + 13..];
                    let end = rest.find("';").unwrap_or(rest.len());
                    *self.title.lock() = rest[..end].trim().to_string();
                }
            }
            self.left = self.every;
        }
        let want = out.len().min(self.left);
        let got = self.inner.read(&mut out[..want])?;
        self.left -= got;
        Ok(got)
    }
}
