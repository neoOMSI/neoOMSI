use super::*;

/// A texture of a tile's own (its night light map, the roads' cut, a ground paint mask) for
/// the GPU: one level as always, compressed where the device takes blocks and the picture
/// stays close (`mask`: only the alpha channel is read).
pub(super) fn tile_texture(img: Image, mask: bool) -> TextureData {
    ::texture::gpu::prepare_single_level(img, mask).0
}

/// Edge (texels) a ground paint mask is brought up to before it is smoothed.
pub(super) const PAINT_MASK_MIN: usize = 512;

/// A ground paint mask made ready to be drawn: the editor's brush writes nothing but 0 and
/// 255, one texel every 0.6-3 m (2^params[0] texels a tile). Sampled as it is, the edge of
/// a car park or a field follows the texel grid, and the shader's sharpening (see
/// `fs_main`) turned that into hard steps - a staircase along every painted edge, metres
/// long where the edge runs nearly along the grid. Brought to at least
/// `PAINT_MASK_MIN` texels (bilinearly) and blurred by a little under one of its own
/// texels, the mask becomes a soft ramp whose half-way line is a smooth curve through the
/// steps' middles; the shader's sharpening then gives a crisp edge along that curve.
/// Returns the alpha as RGBA (white) and the new edge lengths.
pub(super) fn smooth_paint_mask(rgba: &[u8], w: usize, h: usize) -> (Vec<u8>, usize, usize) {
    let s = (PAINT_MASK_MIN / w.max(1))
        .max(1)
        .min(PAINT_MASK_MIN / h.max(1))
        .max(1);
    let (dw, dh) = (w * s, h * s);
    let src = |i: isize, j: isize| -> f32 {
        let i = i.clamp(0, w as isize - 1) as usize;
        let j = j.clamp(0, h as isize - 1) as usize;
        rgba[(j * w + i) * 4 + 3] as f32
    };
    // bilinear magnification, texel centres aligned as the GPU samples them
    let mut a = vec![0f32; dw * dh];
    for y in 0..dh {
        let fy = (y as f32 + 0.5) / s as f32 - 0.5;
        let (j0, ty) = (fy.floor() as isize, fy - fy.floor());
        for x in 0..dw {
            let fx = (x as f32 + 0.5) / s as f32 - 0.5;
            let (i0, tx) = (fx.floor() as isize, fx - fx.floor());
            let top = src(i0, j0) * (1.0 - tx) + src(i0 + 1, j0) * tx;
            let bottom = src(i0, j0 + 1) * (1.0 - tx) + src(i0 + 1, j0 + 1) * tx;
            a[y * dw + x] = top * (1.0 - ty) + bottom * ty;
        }
    }
    // separable Gaussian, sigma 0.85 of a source texel (edges clamped)
    let sigma = 0.85 * s as f32;
    let r = (sigma * 3.0).ceil() as isize;
    let kernel: Vec<f32> = (-r..=r)
        .map(|k| (-(k * k) as f32 / (2.0 * sigma * sigma)).exp())
        .collect();
    let norm: f32 = kernel.iter().sum();
    let mut tmp = vec![0f32; dw * dh];
    for y in 0..dh {
        let row = &a[y * dw..][..dw];
        for x in 0..dw {
            let mut acc = 0.0;
            for (k, wk) in kernel.iter().enumerate() {
                let xi = (x as isize + k as isize - r).clamp(0, dw as isize - 1) as usize;
                acc += row[xi] * wk;
            }
            tmp[y * dw + x] = acc / norm;
        }
    }
    let mut out = vec![255u8; dw * dh * 4];
    for y in 0..dh {
        for x in 0..dw {
            let mut acc = 0.0;
            for (k, wk) in kernel.iter().enumerate() {
                let yi = (y as isize + k as isize - r).clamp(0, dh as isize - 1) as usize;
                acc += tmp[yi * dw + x] * wk;
            }
            out[(y * dw + x) * 4 + 3] = (acc / norm).round().clamp(0.0, 255.0) as u8;
        }
    }
    (out, dw, dh)
}

/// The alpha (0..1) of an image at (u, v) in 0..1, sampled bilinearly with the texel
/// centres where the GPU has them (edges clamped).
pub(super) fn bilinear_alpha(img: &Image, u: f32, v: f32) -> f32 {
    let (w, h) = (img.width as isize, img.height as isize);
    let fx = u * w as f32 - 0.5;
    let fy = v * h as f32 - 0.5;
    let (x0, y0) = (fx.floor(), fy.floor());
    let (tx, ty) = (fx - x0, fy - y0);
    let a = |x: isize, y: isize| -> f32 {
        let x = x.clamp(0, w - 1) as usize;
        let y = y.clamp(0, h - 1) as usize;
        img.rgba[(y * w as usize + x) * 4 + 3] as f32 / 255.0
    };
    let (x0, y0) = (x0 as isize, y0 as isize);
    let top = a(x0, y0) * (1.0 - tx) + a(x0 + 1, y0) * tx;
    let bottom = a(x0, y0 + 1) * (1.0 - tx) + a(x0 + 1, y0 + 1) * tx;
    top * (1.0 - ty) + bottom * ty
}
