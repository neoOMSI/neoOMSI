use super::model::*;
use rayon::prelude::*;
use std::sync::OnceLock;

pub const FIELD: usize = 512;
pub const COARSE: usize = 64;
pub const ENHANCED_TILE_M: f64 = 70_000.0;
pub const CLASSIC_TILE_M: f64 = 14_000.0;

pub struct Field {
    pub image: ::texture::Image,
    cells: Vec<f32>,
    heaps: Vec<f32>,
}

pub fn field() -> &'static Field {
    static FIELD_ONCE: OnceLock<Field> = OnceLock::new();
    FIELD_ONCE.get_or_init(build)
}

fn lattice(x: i64, y: i64, period: i64, seed: u32) -> f32 {
    let (x, y) = (x.rem_euclid(period) as u32, y.rem_euclid(period) as u32);
    let mut h = x.wrapping_mul(0x27d4_eb2f) ^ y.wrapping_mul(0x1656_67b1) ^ seed.wrapping_mul(0x9e37_79b1);
    h = (h ^ (h >> 15)).wrapping_mul(0x85eb_ca6b);
    h = (h ^ (h >> 13)).wrapping_mul(0xc2b2_ae35);
    ((h ^ (h >> 16)) & 0xffff) as f32 / 65535.0
}

fn tile_noise(u: f32, v: f32, period: i64, seed: u32) -> f32 {
    let (x, y) = (u * period as f32, v * period as f32);
    let (ix, iy) = (x.floor() as i64, y.floor() as i64);
    let (fx, fy) = (x - ix as f32, y - iy as f32);
    let (sx, sy) = (fx * fx * (3.0 - 2.0 * fx), fy * fy * (3.0 - 2.0 * fy));
    let l = |a: i64, b: i64| lattice(ix + a, iy + b, period, seed);
    let top = l(0, 0) + (l(1, 0) - l(0, 0)) * sx;
    let bottom = l(0, 1) + (l(1, 1) - l(0, 1)) * sx;
    top + (bottom - top) * sy
}

fn fbm(u: f32, v: f32, period: i64, octaves: u32, seed: u32) -> f32 {
    let (mut sum, mut amp, mut norm, mut p) = (0.0, 0.5, 0.0, period);
    for o in 0..octaves {
        sum += amp * tile_noise(u, v, p, seed.wrapping_add(o * 131));
        norm += amp;
        amp *= 0.5;
        p *= 2;
    }
    sum / norm
}

fn equalize(values: &mut [f32]) {
    let mut order: Vec<u32> = (0..values.len() as u32).collect();
    order.par_sort_unstable_by(|a, b| values[*a as usize].total_cmp(&values[*b as usize]));
    let last = (values.len() - 1).max(1) as f32;
    let mut out = vec![0.0; values.len()];
    for (rank, &i) in order.iter().enumerate() {
        out[i as usize] = rank as f32 / last;
    }
    values.copy_from_slice(&out);
}

fn stretch(values: &mut [f32]) {
    let (lo, hi) = values
        .iter()
        .fold((f32::MAX, f32::MIN), |(a, b), &x| (a.min(x), b.max(x)));
    for x in values.iter_mut() {
        *x = (*x - lo) / (hi - lo).max(1e-6);
    }
}

fn srgb(x: f32) -> u8 {
    let x = x.clamp(0.0, 1.0);
    let s = if x <= 0.003_130_8 { x * 12.92 } else { 1.055 * x.powf(1.0 / 2.4) - 0.055 };
    (s * 255.0 + 0.5) as u8
}

fn box_down(src: &[f32]) -> Vec<f32> {
    let block = FIELD / COARSE;
    let mut out = vec![0.0; COARSE * COARSE];
    for y in 0..COARSE {
        for x in 0..COARSE {
            let mut s = 0.0;
            for dy in 0..block {
                for dx in 0..block {
                    s += src[(y * block + dy) * FIELD + x * block + dx];
                }
            }
            out[y * COARSE + x] = s / (block * block) as f32;
        }
    }
    out
}

fn build() -> Field {
    let n = FIELD;
    let raw: Vec<[f32; 4]> = (0..n * n)
        .into_par_iter()
        .map(|i| {
            let (u, v) = ((i % n) as f32 / n as f32, (i / n) as f32 / n as f32);
            let cell = fbm(u, v, 3, 3, 5);
            let (wu, wv) = (fbm(u, v, 4, 3, 17) - 0.5, fbm(u, v, 4, 3, 29) - 0.5);
            let heap = fbm(u + wu * 0.1, v + wv * 0.1, 6, 5, 41);
            let mut billow = 0.0;
            let (mut amp, mut p) = (0.5, 24);
            for o in 0..3 {
                billow += amp * (1.0 - (tile_noise(u, v, p, 59 + o) * 2.0 - 1.0).abs());
                amp *= 0.5;
                p *= 2;
            }
            [cell, heap, billow, fbm(u, v, 3, 2, 83)]
        })
        .collect();
    let channel = |c: usize| -> Vec<f32> { raw.iter().map(|p| p[c]).collect() };
    let (mut cells, mut heaps, mut billows, mut tall) = (channel(0), channel(1), channel(2), channel(3));
    equalize(&mut cells);
    equalize(&mut heaps);
    stretch(&mut billows);
    stretch(&mut tall);
    let mut rgba = vec![0u8; n * n * 4];
    for i in 0..n * n {
        rgba[i * 4] = srgb(cells[i]);
        rgba[i * 4 + 1] = srgb(heaps[i]);
        rgba[i * 4 + 2] = srgb(billows[i]);
        rgba[i * 4 + 3] = (tall[i].clamp(0.0, 1.0) * 255.0 + 0.5) as u8;
    }
    let mut coarse_cells = box_down(&cells);
    let mut coarse_heaps = box_down(&heaps);
    equalize(&mut coarse_cells);
    equalize(&mut coarse_heaps);
    Field {
        image: ::texture::Image {
            width: n as u32,
            height: n as u32,
            rgba,
            has_alpha: true,
        },
        cells: coarse_cells,
        heaps: coarse_heaps,
    }
}

impl Field {
    pub fn grid(&self, enhanced: bool) -> &[f32] {
        if enhanced { &self.cells } else { &self.heaps }
    }

    pub fn offset_scale(enhanced: bool) -> f32 {
        if enhanced { 1.0 } else { 5.0 }
    }

    pub fn tile_m(enhanced: bool) -> f64 {
        if enhanced { ENHANCED_TILE_M } else { CLASSIC_TILE_M }
    }

    pub fn base_uv(pos: [f64; 2], enhanced: bool) -> [f32; 2] {
        let t = Self::tile_m(enhanced);
        [
            (pos[0] / t).rem_euclid(1.0) as f32,
            (pos[1] / t).rem_euclid(1.0) as f32,
        ]
    }

    pub fn at(&self, enhanced: bool, u: f32, v: f32) -> f32 {
        let grid = self.grid(enhanced);
        let n = COARSE as i64;
        let (x, y) = (u * n as f32 - 0.5, v * n as f32 - 0.5);
        let (fx, fy) = (x - x.floor(), y - y.floor());
        let (ix, iy) = (x.floor() as i64, y.floor() as i64);
        let g = |a: i64, b: i64| grid[((b.rem_euclid(n)) * n + a.rem_euclid(n)) as usize];
        let top = g(ix, iy) + (g(ix + 1, iy) - g(ix, iy)) * fx;
        let bottom = g(ix, iy + 1) + (g(ix + 1, iy + 1) - g(ix, iy + 1)) * fx;
        top + (bottom - top) * fy
    }

    pub fn cell(&self, pos: [f64; 2], offset: [f32; 2], enhanced: bool) -> f32 {
        let base = Self::base_uv(pos, enhanced);
        let k = Self::offset_scale(enhanced);
        self.at(
            enhanced,
            (base[0] + offset[0] * k).rem_euclid(1.0),
            (base[1] + offset[1] * k).rem_euclid(1.0),
        )
    }
}

pub fn thickness(cover: f32, cell: f32, enhanced: bool) -> f32 {
    if cover <= 0.01 {
        return 0.0;
    }
    if enhanced {
        let spread = 0.7 * (1.0 - smoothstep(0.85, 1.0, cover));
        ((0.15 + 0.75 * cover + (cell - 0.5) * spread) / 0.9).clamp(0.0, 1.0)
    } else {
        (0.5 + (cell + cover - 1.0) * 2.0).clamp(0.0, 1.0)
    }
}

pub struct RenderSky {
    pub density: f32,
    pub layers: [[f32; 4]; 3],
}

pub fn render_sky(w: &Weather, rain: f32) -> RenderSky {
    let mut layers = [[0.0f32; 4]; 3];
    let mut sorted = w.clouds.clone();
    sorted.sort_by(|a, b| a.base_m.total_cmp(&b.base_m));
    while sorted.len() > 3 {
        let last = sorted.pop().expect("a layer");
        if let Some(l) = sorted.last_mut() {
            l.cover = 1.0 - (1.0 - l.cover) * (1.0 - last.cover);
        }
    }
    for (i, l) in sorted.iter().enumerate() {
        let base = l.base_m.clamp(250.0, 8000.0);
        let (thickness, shape) = match l.kind {
            CloudKind::Storm => (5500.0, 1.0),
            CloudKind::Towering => (3200.0, 0.9),
            CloudKind::Plain if l.cover >= 0.875 => (700.0 + 1400.0 * rain, 0.1),
            CloudKind::Plain => (500.0 + 1400.0 * (1.0 - l.cover), (1.1 - l.cover).clamp(0.2, 0.85)),
        };
        let mut top = base + thickness;
        if let Some(next) = sorted.get(i + 1) {
            if next.base_m - base > 400.0 {
                top = top.min(next.base_m - 100.0);
            }
        }
        layers[i] = [base, top, l.cover.clamp(0.0, 1.0), shape];
    }
    let cover = w.cover();
    RenderSky {
        density: if cover < 0.02 { 0.0 } else { cover },
        layers,
    }
}
