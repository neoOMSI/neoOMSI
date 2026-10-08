//! `Ui::new` and the per-frame drawing: tags, notes, chat, timetable, tutorial and the overlays.

use super::*;

impl Ui {
    pub fn scene_replaced(&mut self) {
        self.text.labels.clear();
        self.images.clear();
        self.loading_bg = None;
        self.loading_art = None;
        self.loading_logo = None;
        self.logo_src = None;
        self.logo_cache.clear();
        self.spinner.clear();
    }

    pub fn new() -> Option<Ui> {
        Some(Ui {
            text: TextCache::new()?,
            chat: ChatWidget::default(),
            menu_rects: Vec::new(),
            quick_rects: Vec::new(),
            quick_confirm_rects: [[0.0; 4]; 2],
            menu_arrows: Vec::new(),
            menu_scroll_thumb: None,
            menu_scroll_track: None,
            menu_ctl: Vec::new(),
            dd_rects: Vec::new(),
            dd_top: 0,
            dd_rows: 8,
            menu_side: Vec::new(),
            menu_pane: Vec::new(),
            menu_pane_start: 0,
            menu_pane_go: None,
            menu_pane_box: None,
            menu_time: Vec::new(),
            anim: Default::default(),
            anim_dt: 0.0,
            menu_overlay_range: 0..0,
            vr_cursor_overlay: None,
            vr_tooltip_overlay: None,
            menu_start: 0,
            menu_rows: 0,
            menu_row_h: 1.0,
            menu_search: None,
            caret_up: false,
            images: Default::default(),
            loading_bg: None,
            loading_art: None,
            loading_logo: None,
            logo_src: None,
            logo_cache: Vec::new(),
            spinner: Vec::new(),
        })
    }

    pub fn draw(&mut self, r: &Renderer, scene: &mut Scene, f: &Frame, dt: f32) {
        let s = f.scale.max(0.5) * f.ui_scale;
        self.text.backdrop = f.opacity;
        for ((x, y), name, sub, alpha) in &f.tags {
            let a = (alpha.clamp(0.0, 1.0) * 255.0) as u8;
            let l = self
                .text
                .label(r, scene, name, (19.0 * s) as u32, [255, 255, 255, 220]);
            let x0 = x - l.w as f32 * 0.5;
            let y0 = y - l.h as f32;
            if a > 0 {
                l.place(scene, x0, y0);
                if !sub.is_empty() {
                    let m = self
                        .text
                        .label(r, scene, sub, (12.0 * s) as u32, [210, 225, 255, 200]);
                    let mx = x - m.w as f32 * 0.5;
                    m.place(scene, mx, y0 + l.h as f32 - 3.0 * s);
                }
            }
        }
        {
            let text = format!(
                "neoOMSI {} #{}",
                env!("CARGO_PKG_VERSION"),
                f.build.split_whitespace().next().unwrap_or("unknown")
            );

            let was_flat = self.text.flat;
            self.text.flat = true;
            let l = self
                .text
                .label(r, scene, &text, (14.0 * s) as u32, [255, 255, 255, 255]);
            self.text.flat = was_flat;
            let x1 = f.width - 10.0 * s;
            let y1 = f.height - 8.0 * s;
            scene
                .overlays
                .push((l.tex, [x1 - l.w as f32, y1 - l.h as f32, x1, y1]));
        }
        let corner_top = 60.0 * s;
        let notes_bottom = {
            let px = (16.0 * s) as u32;
            let x0 = 16.0 * s;
            let mut y = if f.touch {
                (80.0 * f.scale.max(0.5) * f.ui_scale.max(1.0)).max(corner_top)
            } else {
                corner_top
            };
            for n in f.notes.iter().filter(|n| !n.trim().is_empty()).take(8) {
                let text = crate::tr(n);
                let text = clip_to(&self.text, &text, px as f32, f.width * 0.6);
                let l = self.text.label(r, scene, &text, px, [255, 255, 255, 235]);
                let plate = self.text.plate(r, scene, 7);
                scene.overlays.push((
                    plate,
                    [x0 - 5.0 * s, y, x0 + l.w as f32 + 5.0 * s, y + l.h as f32],
                ));
                l.place(scene, x0, y);
                y += l.h as f32 + 2.0 * s;
            }
            y
        };
        if let Some(c) = f.chat.as_ref().filter(|_| !self.chat.hidden) {
            let px = (17.0 * s) as u32;
            let lh = px as f32 * 1.35;
            let x0 = 14.0 * s;
            let y0 = (96.0 * s).max(notes_bottom + 10.0 * s);
            let width = (460.0 * s).min(f.width * 0.5);
            let open = c.typing.is_some();
            let n = c.lines.len();
            let shown = CHAT_SHOWN.min(n);
            let end = n.saturating_sub(if open || self.chat.hovered {
                self.chat.scroll
            } else {
                0
            });
            let start = end.saturating_sub(shown);
            let box_h = lh * CHAT_SHOWN as f32 + lh * 1.6;
            self.chat.rect = [x0 - 6.0 * s, y0 - 6.0 * s, x0 + width, y0 + box_h];
            self.chat.hovered = self.chat.contains(f.cursor.0, f.cursor.1);
            let show_box = open || self.chat.hovered;
            let mut y = y0 + lh * (CHAT_SHOWN - (end - start)) as f32;
            for line in &c.lines[start..end] {
                let color = if line.starts_with("* ") {
                    [255, 226, 140, 230]
                } else {
                    [255, 255, 255, 230]
                };
                let text = clip_to(&self.text, line, px as f32, width);
                let l = self.text.label(r, scene, &text, px, color);
                l.place(scene, x0, y);
                y += lh;
            }
            if show_box {
                let by = y0 + lh * CHAT_SHOWN as f32 + lh * 0.2;
                let bh = lh * 1.25;
                let plate = self.text.plate(r, scene, 0);
                scene
                    .overlays
                    .push((plate, [x0 - 4.0 * s, by, x0 + width, by + bh]));
                self.chat.caret_t += dt;
                let caret = if open && (self.chat.caret_t % 1.0) < 0.55 {
                    "|"
                } else {
                    ""
                };
                let (text, color) = match c.typing {
                    Some(t) => (format!("{t}{caret}"), [255, 255, 255, 240]),
                    None => (
                        "Click here or press / to chat".to_string(),
                        [190, 190, 190, 200],
                    ),
                };
                let text = clip_left(&self.text, &text, px as f32, width - 10.0 * s);
                let l = self.text.label(r, scene, &text, px, color);
                let ty = by + (bh - l.h as f32) * 0.5;
                l.place(scene, x0 + 2.0 * s, ty);
                if self.chat.scroll > 0 && (open || self.chat.hovered) {
                    let m = self.text.label(
                        r,
                        scene,
                        &format!("{} {}", self.chat.scroll, crate::tr("newer below")),
                        (11.0 * s) as u32,
                        [200, 200, 200, 200],
                    );
                    scene.overlays.push((
                        m.tex,
                        [x0 + width - m.w as f32, by - m.h as f32, x0 + width, by],
                    ));
                }
            }
            if let Some(e) = c.error {
                let l = self.text.label(
                    r,
                    scene,
                    &format!("{}: {}", crate::tr("Not sent"), crate::tr(e)),
                    (12.0 * s) as u32,
                    [255, 150, 150, 220],
                );
                let ey = y0 + box_h;
                l.place(scene, x0, ey);
            }
        } else {
            self.chat.hovered = false;
            self.chat.rect = [0.0; 4];
        }
        if let Some(fps) = f.fps {
            let l = self.text.label(
                r,
                scene,
                &format!("{fps:.0} fps"),
                (13.0 * s) as u32,
                [255, 255, 255, 200],
            );
            let x = f.width - l.w as f32 - 12.0 * s;
            let plate = self.text.plate(r, scene, 7);
            scene.overlays.push((
                plate,
                [
                    x - 5.0 * s,
                    10.0 * s,
                    x + l.w as f32 + 5.0 * s,
                    10.0 * s + l.h as f32,
                ],
            ));
            l.place(scene, x, 10.0 * s);
        }
        if let Some(info) = f.info.as_ref() {
            self.text.flat = true;
            let l = self.text.label(r, scene, info, (15.0 * s) as u32, WHITE);
            let pad = 12.0 * s;
            let (w, h) = (l.w as f32 + pad * 2.0, l.h as f32 + pad * 0.8);
            let x = ((f.width - w) * 0.5).round();
            let y = (8.0 * s).round();
            let radius = ROW_R * s;
            let card = [14, 14, 14, 245];
            self.text.rounded(
                r,
                scene,
                [x - 1.0, y - 1.0, x + w + 1.0, y + h + 1.0],
                radius + 1.0,
                BORDER,
            );
            self.text
                .rounded(r, scene, [x, y, x + w, y + h], radius, card);
            l.place(scene, x + pad, y + pad * 0.4);
            self.text.flat = false;
        }
        let tutorial_w = (420.0 * s).min(f.width * 0.42);
        if let Some((title, rows)) = f.timetable.as_ref() {
            let px = (14.0 * s) as u32;
            let lh = px as f32 * 1.55;
            let w = (340.0 * s).min(f.width * 0.4);
            let shown = rows.len().min(((f.height * 0.7) / lh) as usize).max(1);
            let next = rows.iter().position(|r| r.2 == 1).unwrap_or(0);
            let first = next.saturating_sub(1).min(rows.len().saturating_sub(shown));
            let h = lh * (shown as f32 + 1.6);
            let beside = if f.tutorial.is_some() {
                tutorial_w + 12.0 * s
            } else {
                0.0
            };
            let x = (f.width - w - 16.0 * s - beside).max(16.0 * s);
            let y = corner_top;
            self.text.flat = true;
            let radius = CARD_R * s;
            let card = [14, 14, 14, 245];
            self.text.rounded(
                r,
                scene,
                [x - 1.0, y - 1.0, x + w + 1.0, y + h + 1.0],
                radius + 1.0,
                BORDER,
            );
            self.text
                .rounded(r, scene, [x, y, x + w, y + h], radius, card);
            let t = self.text.label(
                r,
                scene,
                &clip_to(&self.text, title, px as f32 * 1.1, w - 20.0 * s),
                (px as f32 * 1.1) as u32,
                WHITE,
            );
            t.place(scene, x + 10.0 * s, y + 6.0 * s);
            let hair = (y + lh * 1.25).round();
            self.text.rounded(
                r,
                scene,
                [x, hair, x + w, hair + 1.0_f32.max(s).round()],
                0.0,
                BORDER,
            );
            let time_w = rows
                .iter()
                .map(|r| self.text.width(&r.1, px as f32))
                .fold(0.0f32, f32::max)
                .max(40.0 * s);
            let name_x = x + 10.0 * s + time_w + 12.0 * s;
            for (k, (name, time, state)) in rows.iter().skip(first).take(shown).enumerate() {
                let ry = y + lh * (k as f32 + 1.3);
                if *state == 1 {
                    self.text.rounded(
                        r,
                        scene,
                        [
                            x + 4.0 * s,
                            ry - 2.0 * s,
                            x + w - 4.0 * s,
                            ry + lh - 4.0 * s,
                        ],
                        ROW_R * s,
                        ACCENT_SOFT,
                    );
                }
                let color = match state {
                    0 => MUTED,
                    1 => [232, 160, 48, 0],
                    _ => WHITE,
                };
                let tl = self.text.label(r, scene, time, px, color);
                tl.place(scene, x + 10.0 * s, ry);
                let nl = self.text.label(
                    r,
                    scene,
                    &clip_to(&self.text, name, px as f32, x + w - name_x - 10.0 * s),
                    px,
                    color,
                );
                nl.place(scene, name_x, ry);
            }
            self.text.flat = false;
        }
        if let Some((title, text, image, at, count)) = f.tutorial {
            let w = tutorial_w;
            let x = f.width - w - 16.0 * s;
            let mut y = corner_top;
            let top = y;
            let pad = 14.0 * s;
            let mut items: Vec<(TextureId, [f32; 4])> = Vec::new();
            if let Some(p) = image {
                let entry = self.images.entry(p.to_path_buf()).or_insert_with(|| {
                    ::texture::decode_file(p).ok().map(|img| {
                        let (iw, ih) = (img.width, img.height);
                        (r.add_texture(scene, &img, false), iw, ih)
                    })
                });
                if let Some((tex, iw, ih)) = *entry {
                    let dw = w - pad * 2.0;
                    let dh = dw * ih as f32 / iw.max(1) as f32;
                    items.push((tex, [x + pad, y + pad, x + pad + dw, y + pad + dh]));
                    y += dh + pad;
                }
            }
            y += pad * 0.6;
            let tp = (18.0 * s) as u32;
            if !title.is_empty() {
                for line in wrap(&self.text, title, tp as f32, w - pad * 2.0) {
                    let l = self.text.label(r, scene, &line, tp, [255, 200, 110, 0]);
                    items.push((l.tex, [x + pad, y, x + pad + l.w as f32, y + l.h as f32]));
                    y += l.h as f32;
                }
                y += 6.0 * s;
            }
            let bp = (14.0 * s) as u32;
            let max_y = f.height - 60.0 * s;
            'text: for para in text.lines() {
                for line in wrap(&self.text, para, bp as f32, w - pad * 2.0) {
                    if y > max_y {
                        break 'text;
                    }
                    let l = self.text.label(r, scene, &line, bp, [235, 235, 235, 0]);
                    items.push((l.tex, [x + pad, y, x + pad + l.w as f32, y + l.h as f32]));
                    y += l.h as f32 * 0.95;
                }
                y += 5.0 * s;
            }
            let tr = |t: &str| crate::tr(t).into_owned();
            let foot = format!(
                "{} {}/{}   ·   {}   ·   {}   ·   {}",
                tr("Page"),
                at + 1,
                count,
                tr("Enter next"),
                tr("Page Up back"),
                tr("Ctrl+T hide")
            );
            let l = self
                .text
                .label(r, scene, &foot, (12.0 * s) as u32, [150, 150, 150, 0]);
            y += 4.0 * s;
            items.push((l.tex, [x + pad, y, x + pad + l.w as f32, y + l.h as f32]));
            y += l.h as f32 + pad;
            let plate = self.text.plate(r, scene, 3);
            scene.overlays.push((plate, [x, top, x + w, y]));
            scene.overlays.extend(items);
        }
        if f.paused && f.menu.is_none() {
            let l = self.text.label(
                r,
                scene,
                "Paused  ·  P to go on",
                (18.0 * s) as u32,
                [255, 255, 255, 0],
            );
            let pad = 14.0 * s;
            let (w, h) = (l.w as f32 + pad * 2.0, l.h as f32 + pad);
            let x = (f.width - w) * 0.5;
            let y = f.height * 0.2;
            let plate = self.text.plate(r, scene, 3);
            scene.overlays.push((plate, [x, y, x + w, y + h]));
            l.place(scene, x + pad, y + pad * 0.5);
        }
        self.anim_dt = dt.clamp(0.0, 0.1);
        self.text.flat = true;
        self.draw_menu(r, scene, f);
        self.draw_quick_menu(r, scene, f);
        self.text.flat = false;
        self.vr_tooltip_overlay = None;
        if let Some(t) = f.tooltip.as_ref().filter(|t| !t.is_empty()) {
            let l = self
                .text
                .label(r, scene, t, (14.0 * s) as u32, [255, 255, 255, 235]);
            let mut x = f.cursor.0 + 16.0 * s;
            let mut y = f.cursor.1 + 2.0 * s;
            if x + l.w as f32 > f.width {
                x = f.cursor.0 - 8.0 * s - l.w as f32;
            }
            if y + l.h as f32 > f.height {
                y = f.height - l.h as f32;
            }
            if f.vr {
                self.vr_tooltip_overlay = Some(scene.overlays.len());
            } else {
                let plate = self.text.plate(r, scene, 7);
                scene.overlays.push((
                    plate,
                    [x - 5.0 * s, y, x + l.w as f32 + 5.0 * s, y + l.h as f32],
                ));
            }
            l.place(scene, x, y);
        }
        if f.vr {
            let pointer = self.text.vr_pointer(r, scene);
            self.vr_cursor_overlay = Some(scene.overlays.len());
            scene.overlays.push((pointer, [0.0, 0.0, 7.0 * s, 7.0 * s]));
        } else {
            self.vr_cursor_overlay = None;
            if f.crosshair {
                let pointer = self.text.crosshair(r, scene);
                let (cx, cy, d) = (f.width * 0.5, f.height * 0.5, 3.5 * s);
                scene
                    .overlays
                    .push((pointer, [cx - d, cy - d, cx + d, cy + d]));
            }
        }
        self.text.end_frame(r, scene);
    }
}
