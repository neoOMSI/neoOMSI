use super::*;
use crate::spatial::Legacy;
fn clip() -> Arc<Clip> { Arc::new(Clip { sample_rate: 48000, channels: 1, samples: vec![16384; 4800] }) }
fn render(v: &mut Voice, frames: usize, rate: u32) -> Vec<f32> {
    let mut out = vec![0.0; frames * 2];
    v.render_into(&mut out, 2, rate, &Listener::default(), &Legacy); out
}
#[test]
fn finite_start_stop_and_natural_tail_fades() {
    let mut voice = Voice::clip_voice(1, clip(), VoiceParams { looping: true, ..Default::default() }.into());
    let start = render(&mut voice, 1000, 48000);
    assert!(start[0].abs() < 0.001);
    assert!(start[999 * 2] > 0.48);
    voice.stop(); let stop = render(&mut voice, 200, 48000);
    assert!(stop[0] > 0.45); assert!(stop[150 * 2..].iter().all(|s| *s == 0.0));
    assert!(voice.is_finished());
    let mut shot = Voice::clip_voice(2, clip(), Default::default());
    let tail = render(&mut shot, 4800, 48000);
    assert!(tail[4798 * 2] < 0.01); assert_eq!(tail[4799 * 2], 0.0);
    assert!(shot.is_finished());
}
#[test]
fn gain_pitch_and_filter_are_invariant_to_block_partition() {
    let params = VoiceParams { looping: true, ..Default::default() };
    let mut whole = Voice::test_voice(1, clip(), params);
    let mut chunks = Voice::test_voice(2, clip(), params);
    for v in [&mut whole, &mut chunks] {
        v.apply_params(VoiceParams { gain: 0.2, pitch: 2.0, lowpass_hz: 700.0,
            looping: true, ..Default::default() }.into(), Instant::now(), glam::Vec3::ZERO, false);
        // Establish old filter/pitch state before applying a transition.
        v.cur_step = 1.0; v.lp.set_target(0.0, 1, 48000.0);
    }
    let all = render(&mut whole, 1024, 48000);
    let mut pieces = Vec::new();
    for _ in 0..16 { pieces.extend(render(&mut chunks, 64, 48000)); }
    assert_eq!(all, pieces);
    assert!(whole.cur_step > 1.5 && whole.cur_step < 2.0);
}
#[test]
fn gain_time_constant_tracks_output_rate() {
    let mut a = Voice::clip_voice(1, clip(), VoiceParams { looping: true, ..Default::default() }.into());
    let mut b = Voice::clip_voice(2, clip(), VoiceParams { looping: true, ..Default::default() }.into());
    render(&mut a, 480, 48000); render(&mut b, 960, 96000);
    assert!((a.cur_gain - b.cur_gain).abs() < 0.0001);
}

#[test]
fn fading_a_loop_to_zero_does_not_leave_subnormal_output() {
    let mut voice = Voice::test_voice(1, clip(), VoiceParams { looping: true, ..Default::default() });
    voice.apply_params(VoiceParams { gain: 0.0, looping: true, ..Default::default() }.into(),
        Instant::now(), glam::Vec3::ZERO, false);
    let out = render(&mut voice, 48_000, 48_000);
    assert_eq!(voice.cur_gain, 0.0);
    assert!(out[47_000 * 2..].iter().all(|sample| *sample == 0.0));
}
#[test]
fn a_discontinuous_loop_endpoint_is_joined_without_a_hole() {
    let mut source = Clip { sample_rate: 48000, channels: 1, samples: vec![0; 480] };
    for (i, sample) in source.samples.iter_mut().enumerate() { *sample = (i as i16) * 32; }
    let mut voice = Voice::test_voice(1, Arc::new(source), VoiceParams { looping: true, ..Default::default() });
    let out = render(&mut voice, 1000, 48000);
    assert!((out[478 * 2] - out[480 * 2]).abs() < 0.03);
}

#[test]
fn panning_turns_smoothly_without_a_channel_step() {
    let params = VoiceParams { position: Some(glam::Vec3::X), looping: true, ..Default::default() };
    let mut voice = Voice::test_voice(1, clip(), params);
    let before = render(&mut voice, 1000, 48000);
    voice.apply_params(VoiceParams { position: Some(-glam::Vec3::X), ..params }.into(), Instant::now(), glam::Vec3::ZERO, false);
    let after = render(&mut voice, 2000, 48000);
    assert!((after[0] - before[1998]).abs() < 0.001);
    assert!((after[1] - before[1999]).abs() < 0.001);
    assert!(after[3998] > after[3999]);
    assert!(after.iter().all(|s| *s > 0.12));
}
