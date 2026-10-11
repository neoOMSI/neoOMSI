//! Compare a real WAV source with the device-equivalent offline signal path.
//! Usage: clip_render input.wav output.wav [rate=48000] [pitch=1] [gain=1] [content-root ...]
//! Does not open or play to an audio device.
use ::audio::{assets::clip::read_clip, AudioEngine, VoiceParams};
use std::path::Path;

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let args: Vec<String> = std::env::args().skip(1).collect();
    if args.len() < 2 {
        return Err("usage: clip_render input.wav output.wav [rate] [pitch] [gain]".into());
    }
    let rate: u32 = args.get(2).map(|s| s.parse()).transpose()?.unwrap_or(48000);
    let pitch: f32 = args.get(3).map(|s| s.parse()).transpose()?.unwrap_or(1.0);
    let gain: f32 = args.get(4).map(|s| s.parse()).transpose()?.unwrap_or(1.0);
    if !(8000..=192000).contains(&rate) || !pitch.is_finite() || pitch <= 0.0
        || !gain.is_finite() || gain < 0.0 {
        return Err("invalid rate, pitch or gain".into());
    }
    for root in args.iter().skip(5) { ::mods::add_content_root(root.into()); }
    let input = Path::new(&args[0]);
    let input = ::mods::resolve_path(input.parent().ok_or("input has no parent")?,
        input.file_name().ok_or("input has no filename")?.to_str().ok_or("invalid filename")?);
    println!("source: {}", input.display());
    let clip = read_clip(&input).ok_or("cannot decode input WAV")?;
    let seconds = clip.frames() as f64 / clip.sample_rate as f64 / pitch as f64 + 0.02;
    if seconds > 600.0 { return Err("probe output is limited to ten minutes".into()); }
    let frames = (seconds * rate as f64).ceil() as usize;
    let engine = AudioEngine::new_offline(rate, 2);
    engine.play(clip, VoiceParams { pitch, gain, ..Default::default() });
    let mut output = vec![0.0; frames * 2];
    for block in output.chunks_mut(512) { engine.render_offline(block); }
    let bytes: Vec<u8> = output.iter().flat_map(|s|
        ((s.clamp(-1.0, 1.0) * 32768.0).round().clamp(-32768.0, 32767.0) as i16).to_le_bytes()
    ).collect();
    let mut header = Vec::with_capacity(44);
    header.extend_from_slice(b"RIFF");
    header.extend_from_slice(&(36 + bytes.len() as u32).to_le_bytes());
    header.extend_from_slice(b"WAVEfmt ");
    header.extend_from_slice(&16u32.to_le_bytes());
    header.extend_from_slice(&1u16.to_le_bytes());
    header.extend_from_slice(&2u16.to_le_bytes());
    header.extend_from_slice(&rate.to_le_bytes());
    header.extend_from_slice(&(rate * 4).to_le_bytes());
    header.extend_from_slice(&4u16.to_le_bytes());
    header.extend_from_slice(&16u16.to_le_bytes());
    header.extend_from_slice(b"data");
    header.extend_from_slice(&(bytes.len() as u32).to_le_bytes());
    use std::io::Write;
    let mut file = std::fs::File::create(&args[1])?;
    file.write_all(&header)?;
    file.write_all(&bytes)?;
    println!("rendered {frames} frames at {rate} Hz; peak {:.6}; stats {:?}",
        output.iter().map(|s| s.abs()).fold(0.0f32, f32::max), engine.stats());
    Ok(())
}
