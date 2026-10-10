//! Text: rendering a string into a texture with the dark outline, and fitting texts to a width.

use super::*;

impl TextCache {
    pub fn new() -> Option<TextCache> {
        let mut font = FontVec::try_from_vec(ROBOTO.to_vec()).ok()?;
        let _ = font.set_variation(b"wght", 500.0);
        Some(TextCache {
            font,
            labels: hashbrown::HashMap::new(),
            frame: 0,
            backdrop: 1.0,
            flat: false,
            alpha: 1.0,
        })
    }

    /// The texture of `text` at `px` pixels in `color` (alpha = opacity of the outline), and
    /// its size.
    pub(super) fn label(
        &mut self,
        r: &Renderer,
        scene: &mut Scene,
        text: &str,
        px: u32,
        color: [u8; 4],
    ) -> Label {
        let color = self.faded(color);
        let color = [
            color[0],
            color[1],
            color[2],
            outline_for(color, if self.flat { 1.0 } else { self.backdrop }),
        ];
        let text = &*crate::tr(text);
        let key = (text.to_string(), px, color);
        if let Some(l) = self.labels.get_mut(&key) {
            l.used = self.frame;
            return *l;
        }
        let img = render_text(&self.font, text, px as f32, color);
        let tex = r.add_texture(scene, &img, false);
        let l = Label {
            tex,
            w: img.width,
            h: img.height,
            used: self.frame,
        };
        self.labels.insert(key, l);
        l
    }

    fn faded(&self, color: [u8; 4]) -> [u8; 4] {
        if self.alpha >= 1.0 {
            return color;
        }
        let k = (self.alpha * 6.0).round() / 6.0;
        let l = |from: u8, to: u8| (from as f32 + (to as f32 - from as f32) * k).round() as u8;
        [l(14, color[0]), l(16, color[1]), l(20, color[2]), color[3]]
    }

    pub(super) fn icon(
        &mut self,
        r: &Renderer,
        scene: &mut Scene,
        name: &str,
        px: u32,
        color: [u8; 4],
    ) -> Option<Label> {
        let c = self.faded(color);
        let key = (format!("\u{0}icon:{name}"), px, [c[0], c[1], c[2], 0]);
        if let Some(l) = self.labels.get_mut(&key) {
            l.used = self.frame;
            return Some(*l);
        }
        let mask = crate::icons::rasterize(name, px)?;
        let rgba = mask.iter().flat_map(|&a| [c[0], c[1], c[2], a]).collect();
        let img = ::texture::Image {
            width: px,
            height: px,
            rgba,
            has_alpha: true,
        };
        let tex = r.add_texture(scene, &img, false);
        let l = Label {
            tex,
            w: px,
            h: px,
            used: self.frame,
        };
        self.labels.insert(key, l);
        Some(l)
    }

    /// Text width in pixels, without rendering it.
    pub fn width(&self, text: &str, px: f32) -> f32 {
        let text = &*crate::tr(text);
        self.width_raw(text, px)
    }

    pub(super) fn width_raw(&self, text: &str, px: f32) -> f32 {
        let mut w = 0.0;
        let mut prev: Option<(ab_glyph::GlyphId, *const FontVec)> = None;
        for c in text.chars() {
            let font = font_for(&self.font, c);
            let f = font.as_scaled(PxScale::from(px));
            let id = f.glyph_id(c);
            if let Some((p, pf)) = prev {
                if std::ptr::eq(pf, font) {
                    w += f.kern(p, id);
                }
            }
            w += f.h_advance(id);
            prev = Some((id, font as *const FontVec));
        }
        w + outline_px(px) * 2.0 + 2.0
    }

    /// End of a frame: labels not used for a few seconds are released.
    pub fn end_frame(&mut self, r: &Renderer, scene: &mut Scene) {
        self.frame += 1;
        if self.frame % 120 == 0 {
            let old: Vec<_> = self
                .labels
                .iter()
                .filter(|(_, l)| self.frame.saturating_sub(l.used) > 240)
                .map(|(k, _)| k.clone())
                .collect();
            for k in old {
                if let Some(l) = self.labels.remove(&k) {
                    r.free_texture(scene, l.tex);
                }
            }
        }
    }
}

/// How opaque a label's dark outline is (the alpha of its `color`): as asked, or - for a
/// panel's text, asked without one - more as the opacity setting thins the panels
/// (`backdrop` below 1): on a see-through panel over a bright sky the dim lines could not be
/// read. Light texts only: round a dark one (the highlighted menu line's, on the solid
/// accent) the outline is as dark as the glyphs and smeared them, like a shadow.
pub(super) fn outline_for(color: [u8; 4], backdrop: f32) -> u8 {
    let light = 0.299 * color[0] as f32 + 0.587 * color[1] as f32 + 0.114 * color[2] as f32 > 100.0;
    match color[3] {
        0 if backdrop < 1.0 && light => (((1.0 - backdrop) * 1.6).min(0.9) * 255.0) as u8,
        a => a,
    }
}

/// The outline (UIStroke) around the glyphs, in pixels.
pub(super) fn outline_px(px: f32) -> f32 {
    (px / 9.0).clamp(1.0, 3.0)
}

/// The font that draws `c`: Roboto, else the system's font for the script (Chinese,
/// Japanese, Korean, Thai, Hindi - the menu was a column of boxes in those languages).
pub(super) fn font_for(roboto: &FontVec, c: char) -> &FontVec {
    if crate::text::needs_fallback(roboto, c) {
        if let Some(f) = crate::text::fallback_font(c) {
            return f;
        }
    }
    roboto
}

/// `text` as straight-alpha RGBA: the glyphs in `color` over a dark outline.
pub(super) fn render_text(
    font: &FontVec,
    text: &str,
    px: f32,
    color: [u8; 4],
) -> ::texture::Image {
    let f = font.as_scaled(PxScale::from(px));
    let stroke = outline_px(px);
    let pad = stroke.ceil() as i32 + 1;
    let asc = f.ascent();
    let h = (asc - f.descent()).ceil() as i32 + pad * 2;
    let base = (pad as f32 + asc).round();
    let mut glyphs: Vec<(&FontVec, ab_glyph::Glyph)> = Vec::new();
    let mut x = pad as f32;
    let mut prev: Option<(ab_glyph::GlyphId, *const FontVec)> = None;
    for c in text.chars() {
        let gf = font_for(font, c);
        let sf = gf.as_scaled(PxScale::from(px));
        let id = sf.glyph_id(c);
        if let Some((p, pf)) = prev {
            if std::ptr::eq(pf, gf) {
                x += sf.kern(p, id);
            }
        }
        glyphs.push((
            gf,
            id.with_scale_and_position(PxScale::from(px), ab_glyph::point(x.round(), base)),
        ));
        x += sf.h_advance(id);
        prev = Some((id, gf as *const FontVec));
    }
    let w = (x.ceil() as i32 + pad).max(1);
    let (wu, hu) = (w as usize, h.max(1) as usize);
    let mut cov = vec![0f32; wu * hu];
    for (gf, g) in glyphs {
        if let Some(o) = gf.outline_glyph(g) {
            let b = o.px_bounds();
            o.draw(|gx, gy, c| {
                let xx = b.min.x as i32 + gx as i32;
                let yy = b.min.y as i32 + gy as i32;
                if xx >= 0 && yy >= 0 && (xx as usize) < wu && (yy as usize) < hu {
                    let i = yy as usize * wu + xx as usize;
                    cov[i] = (cov[i] + c).min(1.0);
                }
            });
        }
    }
    let light = 0.299 * color[0] as f32 + 0.587 * color[1] as f32 + 0.114 * color[2] as f32 > 100.0;
    let gamma = if light { 1.45 } else { 0.8 };
    for c in cov.iter_mut() {
        if *c > 0.0 && *c < 1.0 {
            *c = c.powf(gamma);
        }
    }
    let r = stroke;
    let ri = r.ceil() as i32;
    let mut edge = vec![0f32; wu * hu];
    for y in 0..hu as i32 {
        for x in 0..wu as i32 {
            let mut m = 0f32;
            for dy in -ri..=ri {
                for dx in -ri..=ri {
                    let d = ((dx * dx + dy * dy) as f32).sqrt();
                    if d > r + 0.5 {
                        continue;
                    }
                    let (sx, sy) = (x + dx, y + dy);
                    if sx < 0 || sy < 0 || sx >= wu as i32 || sy >= hu as i32 {
                        continue;
                    }
                    let k = (r + 0.5 - d).clamp(0.0, 1.0);
                    m = m.max(cov[sy as usize * wu + sx as usize] * k);
                }
            }
            edge[y as usize * wu + x as usize] = m;
        }
    }
    let oa = color[3] as f32 / 255.0;
    let mut rgba = vec![0u8; wu * hu * 4];
    for i in 0..wu * hu {
        let a_text = cov[i];
        let a_edge = edge[i] * oa;
        let a = a_text + a_edge * (1.0 - a_text);
        if a <= 0.0 {
            continue;
        }
        for c in 0..3 {
            let v = (color[c] as f32 * a_text + 12.0 * a_edge * (1.0 - a_text)) / a;
            rgba[i * 4 + c] = v.round().clamp(0.0, 255.0) as u8;
        }
        rgba[i * 4 + 3] = (a * 255.0).round() as u8;
    }
    ::texture::Image {
        width: wu as u32,
        height: hu as u32,
        rgba,
        has_alpha: true,
    }
}

/// `text` broken into lines of at most `width` pixels, between words.
pub(super) fn wrap(tc: &TextCache, text: &str, px: f32, width: f32) -> Vec<String> {
    let mut out = Vec::new();
    let mut line = String::new();
    for word in text.split_whitespace() {
        let try_line = if line.is_empty() {
            word.to_string()
        } else {
            format!("{line} {word}")
        };
        if tc.width(&try_line, px) > width && !line.is_empty() {
            out.push(std::mem::take(&mut line));
            line = word.to_string();
        } else {
            line = try_line;
        }
    }
    if !line.is_empty() {
        out.push(line);
    }
    out
}

/// `text` cut at the end to fit `width` pixels ("…").
pub(super) fn clip_to(tc: &TextCache, text: &str, px: f32, width: f32) -> String {
    if tc.width(text, px) <= width {
        return text.to_string();
    }
    let chars: Vec<char> = text.chars().collect();
    let head = |n: usize| {
        let mut t: String = chars[..n].iter().collect();
        t.push('…');
        t
    };
    let (mut lo, mut hi) = (0usize, chars.len());
    while lo < hi {
        let mid = (lo + hi + 1) / 2;
        if tc.width_raw(&head(mid), px) <= width {
            lo = mid;
        } else {
            hi = mid - 1;
        }
    }
    head(lo)
}

/// `text` cut at the start to fit (the end of what is being typed stays visible).
pub(super) fn clip_left(tc: &TextCache, text: &str, px: f32, width: f32) -> String {
    let mut t: Vec<char> = text.chars().collect();
    while t.len() > 1 && tc.width(&t.iter().collect::<String>(), px) > width {
        t.remove(0);
    }
    t.into_iter().collect()
}
