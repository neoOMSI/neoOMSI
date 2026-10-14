use crate::*;
use ::render::{Camera, Lighting, Renderer, Scene};

pub(super) fn render(
    renderer: &mut Renderer,
    scene: &mut Scene,
    w: u32,
    h: u32,
    cam: &Camera,
    light: &Lighting,
) -> Option<Vec<u8>> {
    let mut l = light.clone();
    l.sky_color = Vec3::ZERO;
    let black = renderer.render_to_image(scene, w, h, cam, &l).ok()?;
    l.sky_color = Vec3::ONE;
    let white = renderer.render_to_image(scene, w, h, cam, &l).ok()?;
    Some(combine(&black, &white, w as usize, h as usize))
}

fn combine(black: &[u8], white: &[u8], w: usize, h: usize) -> Vec<u8> {
    let n = w * h;
    let mut out = vec![0u8; n * 4];
    if n == 0 || black.len() < n * 4 || white.len() < n * 4 {
        return out;
    }
    let corners = [0, w - 1, (h - 1) * w, n - 1];
    let mut span = [1.0f32; 3];
    for c in 0..3 {
        span[c] = corners
            .iter()
            .map(|&i| white[i * 4 + c] as f32 - black[i * 4 + c] as f32)
            .fold(0.0, f32::max);
    }
    let bg = [black[0] as f32, black[1] as f32, black[2] as f32];
    if span.iter().any(|s| *s < 16.0) {
        for i in 0..n {
            out[i * 4..i * 4 + 3].copy_from_slice(&black[i * 4..i * 4 + 3]);
            out[i * 4 + 3] = 255;
        }
        return out;
    }
    for i in 0..n {
        let o = i * 4;
        let mut seen = 0.0f32;
        for c in 0..3 {
            let d = (white[o + c] as f32 - black[o + c] as f32).max(0.0);
            seen += (d / span[c]).min(1.0);
        }
        let a = (1.0 - seen / 3.0).clamp(0.0, 1.0);
        if a < 0.02 {
            continue;
        }
        if a > 0.92 {
            out[o..o + 3].copy_from_slice(&black[o..o + 3]);
            out[o + 3] = 255;
            continue;
        }
        for c in 0..3 {
            let fg = (black[o + c] as f32 - bg[c] * (1.0 - a)) / a;
            out[o + c] = fg.clamp(0.0, 255.0).round() as u8;
        }
        out[o + 3] = (a * 255.0).round() as u8;
    }
    out
}
