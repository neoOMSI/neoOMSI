//! Reproducible synthetic workload, timed OUTSIDE the renderer. Prints CSV for comparison
//! on the same release build/machine. Not a substitute for live map CPU/memory/hearing QA.
//! Example: audio_load 200 192000 256 3750 2
use ::audio::{AudioEngine, Bus, Clip, Listener, VoiceParams};
use std::{
    sync::Arc,
    time::{Duration, Instant},
};
fn main() {
    let count: usize = std::env::args()
        .nth(1)
        .map(|v| v.parse().expect("voice count"))
        .unwrap_or(200);
    assert!(count <= 512);
    let rate: u32 = std::env::args()
        .nth(2)
        .map(|v| v.parse().expect("sample rate"))
        .unwrap_or(48000);
    assert!(rate >= 8000);
    let frames: usize = std::env::args()
        .nth(3)
        .map(|v| v.parse().expect("callback frames"))
        .unwrap_or(256);
    let blocks: usize = std::env::args()
        .nth(4)
        .map(|v| v.parse().expect("callback count"))
        .unwrap_or(375);
    let clip_channels: usize = std::env::args()
        .nth(5)
        .map(|v| v.parse().expect("source channels"))
        .unwrap_or(1);
    assert!(frames > 0 && blocks > 0);
    assert!((1..=2).contains(&clip_channels));
    let engine = AudioEngine::new_offline_output(rate, 2);
    let clip = Arc::new(Clip {
        sample_rate: 44100,
        channels: clip_channels as u16,
        samples: (0..44100)
            .flat_map(|i| {
                (0..clip_channels).map(move |c| {
                    ((i as f32 * (220.0 + c as f32 * 110.0) * std::f32::consts::TAU / 44100.0)
                        .sin()
                        * 12000.0) as i16
                })
            })
            .collect(),
    });
    let mut ids = Vec::new();
    for i in 0..count {
        let params = VoiceParams {
            gain: 0.1,
            pitch: 0.5 + (i % 9) as f32 * 0.25,
            looping: true,
            position: Some(glam::Vec3::new((i % 15) as f32, (i / 15) as f32, 0.0)),
            lowpass_hz: if i % 3 == 0 { 700.0 } else { 0.0 },
            ..Default::default()
        };
        ids.push((engine.play(clip.clone(), params), params));
    }
    // Environment, passenger speech and radio share the same headroom in a dense street.
    for bus in [
        Bus::Ambience,
        Bus::Passenger,
        Bus::Announcement,
        Bus::Radio,
        Bus::Interface,
    ] {
        let mut mix = ::audio::voice::MixParams::from(VoiceParams {
            gain: 0.02,
            looping: true,
            ..Default::default()
        });
        mix.bus = bus;
        if bus == Bus::Announcement {
            mix.cabin_reverb = 0.25;
        }
        engine.play_mix(clip.clone(), mix);
    }
    engine.set_listener(Listener {
        reverb_time: 0.8,
        reverb_mix: 0.2,
        ..Default::default()
    });
    let mut out = vec![0.0; frames * 2];
    let cold_start = Instant::now();
    engine.render_offline(&mut out);
    let cold_ms = cold_start.elapsed().as_secs_f64() * 1000.0;
    for _ in 0..32 {
        engine.render_offline(&mut out);
    }
    let mut times = Vec::new();
    let mut total = Duration::ZERO;
    let mut peak = 0.0f32;
    let mut next_update = 0.0;
    for block in 0..blocks {
        engine
            .clock()
            .advance(Duration::from_secs_f64(frames as f64 / rate as f64));
        let audio_time = block as f64 * frames as f64 / rate as f64;
        if audio_time >= next_update {
            next_update = audio_time + 0.02;
            for (id, params) in &ids {
                let mut p = *params;
                p.pitch *= 1.0 + 0.2 * (audio_time as f32 * 5.625).sin();
                engine.set_params(*id, p);
            }
        }
        let start = Instant::now();
        engine.render_offline(&mut out);
        let elapsed = start.elapsed();
        total += elapsed;
        times.push(elapsed.as_secs_f64() * 1000.0);
        peak = out.iter().map(|s| s.abs()).fold(peak, f32::max);
    }
    times.sort_by(f64::total_cmp);
    let budget_ms = frames as f64 * 1000.0 / rate as f64;
    let over_budget = times.iter().filter(|&&ms| ms > budget_ms).count();
    println!(
        "voices,active,output_rate,source_channels,callback_frames,callbacks,cold_ms,mean_ms,p95_ms,p99_ms,max_ms,callback_budget_ms,render_budget_percent,over_budget_blocks,shared_clip_bytes,peak,dropped,underruns"
    );
    let stats = engine.stats();
    println!(
        "{count},{},{rate},{clip_channels},{frames},{blocks},{cold_ms:.4},{:.4},{:.4},{:.4},{:.4},{budget_ms:.4},{:.2},{over_budget},{},{peak:.6},{},{}",
        engine.voice_count(),
        total.as_secs_f64() * 1000.0 / blocks as f64,
        times[(blocks * 95).div_ceil(100) - 1],
        times[(blocks * 99).div_ceil(100) - 1],
        times[blocks - 1],
        total.as_secs_f64() / (blocks as f64 * frames as f64 / rate as f64) * 100.0,
        clip.samples.len() * 2,
        stats.dropped_commands,
        stats.stream_underruns
    );
}
