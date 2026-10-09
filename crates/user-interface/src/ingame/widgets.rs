//! Pieces of the game menu: animated values, logo, headers, key hints and chips.

use super::*;

impl Ui {
    /// `text` at `x`, its middle on `cy`; returns its width.
    pub(super) fn put(
        &mut self,
        r: &Renderer,
        scene: &mut Scene,
        text: &str,
        px: u32,
        color: [u8; 4],
        x: f32,
        cy: f32,
    ) -> f32 {
        let l = self.text.label(r, scene, text, px, color);
        let y = cy - l.h as f32 * 0.5;
        l.place(scene, x, y);
        l.w as f32
    }

    /// The value that eases to `target` (a part of the menu changing its colour or place):
    /// each frame it goes `speed` per second of the way (exponentially). One value per `key`;
    /// the first time it is asked for it is there already.
    pub(super) fn ease(&mut self, key: (u8, &str, usize), target: f32, speed: f32) -> f32 {
        use std::hash::{Hash, Hasher};
        let mut h = std::collections::hash_map::DefaultHasher::new();
        key.hash(&mut h);
        let dt = self.anim_dt;
        let v = self.anim.entry(h.finish()).or_insert(target);
        let step = speed * dt;
        if (target - *v).abs() <= step {
            *v = target;
        } else if target > *v {
            *v += step;
        } else {
            *v -= step;
        }
        *v
    }

    /// `ease`, in eighths: a colour or a light that fades is a texture made once per step
    /// (a plate or a text of that colour), so a fade goes in a few steps, not in hundreds.
    pub(super) fn easeq(&mut self, key: (u8, &str, usize), target: f32, speed: f32) -> f32 {
        quant(self.ease(key, target, speed))
    }

    /// The accent bar at the left edge of a line, growing from its middle as `k` goes 0 to 1.
    pub(super) fn accent_bar(
        &mut self,
        r: &Renderer,
        scene: &mut Scene,
        rect: [f32; 4],
        k: f32,
        danger: bool,
        s: f32,
    ) {
        let full = (rect[3] - rect[1] - 20.0 * s).max(10.0 * s);
        let bh = full * (0.4 + 0.6 * k);
        let by = (rect[1] + rect[3]) * 0.5 - bh * 0.5;
        self.text.rounded(
            r,
            scene,
            [rect[0], by, rect[0] + 2.0 * s, by + bh],
            1.0 * s,
            fade(if danger { DANGER } else { ACCENT }, k),
        );
    }

    pub(super) fn ensure_logo(&mut self) {
        if self.loading_logo.is_none() {
            static LOGO: &[u8] =
                include_bytes!("../../../../assets/logos/wordmark-gradient-dark.png");
            self.loading_logo = Some(image::load_from_memory(LOGO).ok().map(|i| {
                let mut i = i.into_rgba8();
                let (w, h) = i.dimensions();
                let (mut x0, mut y0, mut x1, mut y1) = (w, h, 0, 0);
                for (x, y, p) in i.enumerate_pixels() {
                    if p[3] > 8 {
                        x0 = x0.min(x);
                        y0 = y0.min(y);
                        x1 = x1.max(x + 1);
                        y1 = y1.max(y + 1);
                    }
                }
                if x1 > x0 && y1 > y0 {
                    i = image::imageops::crop_imm(&i, x0, y0, x1 - x0, y1 - y0).to_image();
                }
                let (iw, ih) = i.dimensions();
                self.logo_src = Some(i);
                (0, iw, ih)
            }));
        }
    }

    pub(super) fn logo_at(
        &mut self,
        r: &Renderer,
        scene: &mut Scene,
        h: f32,
    ) -> Option<(TextureId, u32, u32)> {
        let h = (h.round() as u32).clamp(8, 1024);
        if let Some(&(_, tex, w)) = self.logo_cache.iter().find(|c| c.0 == h) {
            return Some((tex, w, h));
        }
        let src = self.logo_src.as_ref()?;
        let w =
            ((src.width() as f32 * h as f32 / src.height().max(1) as f32).round() as u32).max(1);
        let mut pm = src.clone();
        for p in pm.pixels_mut() {
            let a = p[3] as u32;
            for c in 0..3 {
                p[c] = ((p[c] as u32 * a + 127) / 255) as u8;
            }
        }
        let mut d = image::imageops::resize(&pm, w, h, image::imageops::FilterType::Lanczos3);
        for p in d.pixels_mut() {
            let a = p[3] as u32;
            if a > 0 {
                for c in 0..3 {
                    p[c] = ((p[c] as u32 * 255 + a / 2) / a).min(255) as u8;
                }
            }
        }
        let img = ::texture::Image {
            width: w,
            height: h,
            rgba: d.into_raw(),
            has_alpha: true,
        };
        let tex = r.add_texture(scene, &img, false);
        if self.logo_cache.len() >= 6 {
            self.logo_cache.remove(0);
        }
        self.logo_cache.push((h, tex, w));
        Some((tex, w, h))
    }

}