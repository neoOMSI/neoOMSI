use super::*;

/// The part of a `.map.LM.bmp` that covers its own tile, resampled to the picture's full
/// size. The editor bakes each light map over the tile and its eight neighbours, north at the
/// top row: the tile is the middle third. Neighbouring light maps are the same picture shifted
/// by a third (85 texels between two tiles, 171 between every other one, on all stock maps).
/// Laid over the tile whole, every pool of light came out three times as large and away from
/// its lamp - a filling station's blue light lay on a garden 390 m off.
pub(super) fn own_tile_of_light_map(img: &::texture::Image) -> ::texture::Image {
    let (w, h) = (img.width as usize, img.height as usize);
    if w < 3 || h < 3 {
        return img.clone();
    }
    let texel = |x: usize, y: usize, c: usize| img.rgba[(y * w + x) * 4 + c] as f32;
    let mut rgba = vec![0u8; w * h * 4];
    for y in 0..h {
        // (bilinear, the texel centres of the output spread evenly over the middle third)
        let sy = (h as f32 / 3.0 + (y as f32 + 0.5) / 3.0 - 0.5).clamp(0.0, (h - 1) as f32);
        let (y0, fy) = (sy.floor() as usize, sy.fract());
        let y1 = (y0 + 1).min(h - 1);
        for x in 0..w {
            let sx = (w as f32 / 3.0 + (x as f32 + 0.5) / 3.0 - 0.5).clamp(0.0, (w - 1) as f32);
            let (x0, fx) = (sx.floor() as usize, sx.fract());
            let x1 = (x0 + 1).min(w - 1);
            for c in 0..4 {
                let top = texel(x0, y0, c) * (1.0 - fx) + texel(x1, y0, c) * fx;
                let bottom = texel(x0, y1, c) * (1.0 - fx) + texel(x1, y1, c) * fx;
                rgba[(y * w + x) * 4 + c] = (top * (1.0 - fy) + bottom * fy).round() as u8;
            }
        }
    }
    ::texture::Image {
        width: img.width,
        height: img.height,
        rgba,
        has_alpha: img.has_alpha,
    }
}

/// Give the lamps of a tile the hue its light map (the tile's own part, see
/// [`own_tile_of_light_map`]; north at the top row) shows under them, where the map is lit
/// there at all; their brightness stays.
pub(super) fn tint_lights_from_light_map(
    lights: &mut [::render::PointLight],
    img: &::texture::Image,
    origin: DVec3,
) {
    let ts = ::map::tile_size();
    let (w, h) = (img.width as i64, img.height as i64);
    if w == 0 || h == 0 {
        return;
    }
    for l in lights.iter_mut() {
        let u = (l.position.x - origin.x) / ts;
        let v = (l.position.y - origin.y) / ts;
        if !(0.0..1.0).contains(&u) || !(0.0..1.0).contains(&v) {
            continue;
        }
        let (cx, cy) = ((u * w as f64) as i64, ((1.0 - v) * h as f64) as i64);
        // the brightest texel within a few metres (the lamp stands over its pool's edge)
        let reach = ((6.0 / ts) * w as f64).ceil().max(1.0) as i64;
        let mut best = [0u8; 3];
        for y in (cy - reach).max(0)..=(cy + reach).min(h - 1) {
            for x in (cx - reach).max(0)..=(cx + reach).min(w - 1) {
                let i = ((y * w + x) * 4) as usize;
                let c = [img.rgba[i], img.rgba[i + 1], img.rgba[i + 2]];
                if c.iter().map(|&v| v as u32).sum::<u32>()
                    > best.iter().map(|&v| v as u32).sum::<u32>()
                {
                    best = c;
                }
            }
        }
        let peak = *best.iter().max().unwrap() as f32;
        if peak < 40.0 {
            continue;
        }
        let hue = [
            best[0] as f32 / peak,
            best[1] as f32 / peak,
            best[2] as f32 / peak,
        ];
        let bright = l.color.iter().cloned().fold(0.0f32, f32::max);
        l.color = [hue[0] * bright, hue[1] * bright, hue[2] * bright];
    }
}

impl World {
    /// The colour the tile's night light map (its own part, see [`own_tile_of_light_map`])
    /// has at `pos` (0..1, bilinear), or `None` where no light map is loaded: the light it
    /// throws on a vehicle standing there (Omsi.exe samples it at the vehicle's place,
    /// 0x61378c, for its ambient light and `Envir_Brightness`).
    pub fn light_map_light_at(&self, pos: DVec3) -> Option<glam::Vec3> {
        let ts = tile_size();
        let key = ((pos.x / ts).floor() as i32, (pos.y / ts).floor() as i32);
        let img = self.light_maps.lock().get(&key).cloned()?;
        let (w, h) = (img.width as usize, img.height as usize);
        if w == 0 || h == 0 || img.rgba.len() < w * h * 4 {
            return None;
        }
        let u = ((pos.x / ts - key.0 as f64) * w as f64 - 0.5).clamp(0.0, (w - 1) as f64);
        let v = ((1.0 - (pos.y / ts - key.1 as f64)) * h as f64 - 0.5).clamp(0.0, (h - 1) as f64);
        let (x0, y0) = (u.floor() as usize, v.floor() as usize);
        let (x1, y1) = ((x0 + 1).min(w - 1), (y0 + 1).min(h - 1));
        let (fx, fy) = ((u - x0 as f64) as f32, (v - y0 as f64) as f32);
        let px = |x: usize, y: usize| {
            let i = (y * w + x) * 4;
            glam::Vec3::new(
                img.rgba[i] as f32,
                img.rgba[i + 1] as f32,
                img.rgba[i + 2] as f32,
            ) / 255.0
        };
        let top = px(x0, y0).lerp(px(x1, y0), fx);
        let bottom = px(x0, y1).lerp(px(x1, y1), fx);
        Some(top.lerp(bottom, fy))
    }

    /// Fill the light map atlas with the 5x5 tiles around `eye` (when it moved to another
    /// tile or tiles came or went): the splines and `[LightMapMapping]` objects are lit by it
    /// at night as the terrain is.
    pub fn update_light_map_atlas(&self, renderer: &Renderer, eye: DVec3) {
        // (`OMSI_NO_LIGHT_MAP=1`: the tiles' night light maps left out, for an A/B)
        if ::legacy_config::env::var_os("OMSI_NO_LIGHT_MAP").is_some() {
            return;
        }
        let ts = tile_size();
        let centre = ((eye.x / ts).floor() as i32, (eye.y / ts).floor() as i32);
        let generation = self
            .light_maps_generation
            .load(std::sync::atomic::Ordering::Relaxed);
        let mut last = self.light_map_atlas.lock();
        if *last == Some((centre, generation)) {
            return;
        }
        *last = Some((centre, generation));
        let n = ::render::LM_ATLAS_TILES as i32;
        let maps = self.light_maps.lock();
        for row in 0..n {
            for col in 0..n {
                // column from the west, row from the north
                let key = (centre.0 - n / 2 + col, centre.1 + n / 2 - row);
                renderer.set_light_map_tile(
                    (col as u32, row as u32),
                    maps.get(&key).map(|a| a.as_ref()),
                );
            }
        }
        let sw = (
            (centre.0 - n / 2) as f64 * ts,
            (centre.1 - n / 2) as f64 * ts,
        );
        renderer.set_light_map_place(sw.0, sw.1, n as f64 * ts);
    }
}
