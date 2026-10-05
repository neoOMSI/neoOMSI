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

    /// `text` ending at `right`, its middle on `cy`; returns its width.
    pub(super) fn put_right(
        &mut self,
        r: &Renderer,
        scene: &mut Scene,
        text: &str,
        px: u32,
        color: [u8; 4],
        right: f32,
        cy: f32,
    ) -> f32 {
        let l = self.text.label(r, scene, text, px, color);
        let y = cy - l.h as f32 * 0.5;
        scene
            .overlays
            .push((l.tex, [right - l.w as f32, y, right, y + l.h as f32]));
        l.w as f32
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
            static LOGO: &[u8] = include_bytes!("../../../../assets/logos/wordmark-gradient-dark.png");
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
        let img = omsi_texture::Image {
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

    /// The pause menu's header: the wordmark instead of the game's name and "Paused".
    /// False when the logo could not be loaded (the text header is drawn then).
    pub(super) fn menu_logo_header(
        &mut self,
        r: &Renderer,
        scene: &mut Scene,
        x: f32,
        y: f32,
        w: f32,
        header_h: f32,
        s: f32,
    ) -> bool {
        self.ensure_logo();
        if let Some((tex, iw, ih)) = self.logo_at(r, scene, 30.0 * s) {
            let (lw, lh) = (iw as f32, ih as f32);
            let left = (x + (w - lw) * 0.5).round();
            let top = (y + (header_h - 6.0 * s - lh) * 0.5).round();
            scene.overlays.push((tex, [left, top, left + lw, top + lh]));
            true
        } else {
            false
        }
    }

    /// The header of the card at (`x`, `y`) of `w` wide: what the list is of, small and in
    /// capitals, the title large under it, a hairline under both. Its text starts where the
    /// text of the lines does. Returns the middle of the header.
    pub(super) fn menu_header(
        &mut self,
        r: &Renderer,
        scene: &mut Scene,
        x: f32,
        y: f32,
        w: f32,
        header_h: f32,
        title: &str,
        sub: &str,
        s: f32,
    ) -> f32 {
        let left = x + (PAD + TEXT_IN) * s;
        let (eyebrow, eyebrow_ink) = if sub.is_empty() {
            ("neoOMSI".to_string(), txt(ACCENT))
        } else {
            (sub.to_uppercase(), MUTED)
        };
        let e = self
            .text
            .label(r, scene, &eyebrow, (12.0 * s) as u32, eyebrow_ink);
        let title = clip_to(
            &self.text,
            title,
            24.0 * s,
            w - (PAD + TEXT_IN) * 2.0 * s - 80.0 * s,
        );
        let t = self.text.label(r, scene, &title, (24.0 * s) as u32, WHITE);
        let band = header_h - 6.0 * s;
        let top = y + (band - (e.h as f32 + t.h as f32 - 2.0 * s)) * 0.5;
        e.place(scene, left, top);
        let ty = top + e.h as f32 - 2.0 * s;
        t.place(scene, left, ty);
        y + band * 0.5
    }

    /// The key bindings page's own parts: the search field above the list (`label` is the
    /// search line's row: its value is the text typed) and the bar of key hints under it.
    pub(super) fn keys_chrome(
        &mut self,
        r: &Renderer,
        scene: &mut Scene,
        label: &str,
        span: [f32; 2],
        top: f32,
        cursor: (f32, f32),
        _tin: f32,
        s: f32,
    ) {
        let mut parts = label.split('\u{1f}');
        let _ = parts.next();
        let typing = parts.next() == Some("E");
        let text = parts.next().unwrap_or("");
        let [x0, x1] = span;
        let h = 38.0 * s;
        let field = [x0, top + 3.0 * s, x1, top + 3.0 * s + h];
        let cy = (field[1] + field[3]) * 0.5;
        self.menu_search = Some(field);
        let hovered = cursor.0 >= field[0]
            && cursor.0 <= field[2]
            && cursor.1 >= field[1]
            && cursor.1 <= field[3];
        let f = self.easeq((20, "keysearch", 0), if typing { 1.0 } else { 0.0 }, 6.0);
        let hv = self.easeq(
            (21, "keysearch", 0),
            if hovered && !typing { 1.0 } else { 0.0 },
            8.0,
        );
        let lit = f.max(hv * 0.6);
        let rad = h * 0.5;
        let glow = 0.22 * f + 0.07 * hv;
        if glow > 0.0 {
            let g = (4.0 * f + 2.0 * hv) * s;
            self.text.rounded(
                r,
                scene,
                [field[0] - g, field[1] - g, field[2] + g, field[3] + g],
                rad + g,
                fade(ACCENT, glow),
            );
        }
        let fill = mix(PANEL_ALT, [40, 40, 40, 255], lit);
        self.text.rounded(
            r,
            scene,
            [
                field[0] - 1.0,
                field[1] - 1.0,
                field[2] + 1.0,
                field[3] + 1.0,
            ],
            rad + 1.0,
            mix(BORDER, ACCENT, lit),
        );
        self.text.rounded(r, scene, field, rad, fill);
        let ink = mix([142, 142, 142, 255], ACCENT, f.max(hv * 0.8));
        let (mx, my) = (field[0] + 22.0 * s, cy - 1.5 * s);
        self.text.rounded(
            r,
            scene,
            [mx - 6.5 * s, my - 6.5 * s, mx + 6.5 * s, my + 6.5 * s],
            6.5 * s,
            ink,
        );
        self.text.rounded(
            r,
            scene,
            [mx - 4.5 * s, my - 4.5 * s, mx + 4.5 * s, my + 4.5 * s],
            4.5 * s,
            fill,
        );
        for n in 0..4 {
            let o = (5.5 + n as f32 * 1.6) * s;
            self.text.rounded(
                r,
                scene,
                [
                    mx + o - 1.1 * s,
                    my + o - 1.1 * s,
                    mx + o + 1.1 * s,
                    my + o + 1.1 * s,
                ],
                1.1 * s,
                ink,
            );
        }
        let px = (15.0 * s) as u32;
        let tx = field[0] + 42.0 * s;
        let room = field[2] - tx - 18.0 * s;
        let mut caret_x = tx;
        if text.is_empty() {
            let hint = clip_to(&self.text, "Search key bindings", px as f32, room);
            self.put(
                r,
                scene,
                &hint,
                px,
                txt(mix([142, 142, 142, 0], [92, 92, 92, 0], f)),
                tx + 4.0 * s * f,
                cy,
            );
        } else {
            let t = clip_left(&self.text, text, px as f32, room);
            let w = self.put(r, scene, &t, px, WHITE, tx, cy);
            caret_x = tx + w + 2.0 * s;
        }
        if typing {
            let cv = self.ease(
                (23, "keysearch", 0),
                if self.caret_up { 1.0 } else { 0.0 },
                2.5,
            );
            if cv >= 1.0 {
                self.caret_up = false;
            } else if cv <= 0.0 {
                self.caret_up = true;
            }
            let a = quant(cv).max(0.125);
            self.text.rounded(
                r,
                scene,
                [caret_x, cy - 9.0 * s, caret_x + 2.0 * s, cy + 9.0 * s],
                1.0 * s,
                fade([236, 236, 236, 255], a),
            );
        }
    }

    /// A pill (`cap`: a key cap) of `fill` with `text` in it, ending at `right`; returns
    /// its left edge.
    pub(super) fn chip(
        &mut self,
        r: &Renderer,
        scene: &mut Scene,
        text: &str,
        px: u32,
        color: [u8; 4],
        fill: [u8; 4],
        cap: bool,
        right: f32,
        cy: f32,
        s: f32,
    ) -> f32 {
        let l = self.text.label(r, scene, text, px, color);
        let (cw, ch) = (l.w as f32 + 18.0 * s, l.h as f32 + 6.0 * s);
        let x0 = right - cw;
        if cap {
            self.text.rounded(
                r,
                scene,
                [
                    x0 - 1.0,
                    cy - ch * 0.5 - 1.0,
                    right + 1.0,
                    cy + ch * 0.5 + 1.0,
                ],
                5.0 * s,
                [96, 96, 96, 255],
            );
        }
        self.text.rounded(
            r,
            scene,
            [x0, cy - ch * 0.5, right, cy + ch * 0.5],
            if cap { 4.0 * s } else { ROW_R * s },
            fill,
        );
        let (tx, ty) = (x0 + 9.0 * s, cy - l.h as f32 * 0.5);
        l.place(scene, tx, ty);
        x0
    }
}