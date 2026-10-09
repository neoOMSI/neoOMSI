//! The loading screen.

use super::*;

impl Ui {
    pub fn loading(
        &mut self,
        r: &Renderer,
        scene: &mut Scene,
        width: f32,
        height: f32,
        scale: f32,
        title: &str,
        caption: &str,
        progress: Option<f32>,
        _map_dir: Option<&std::path::Path>,
        t: f32,
    ) {
        let s = scale.max(0.5);
        let black = self.text.plate(r, scene, 8);
        scene.overlays.push((black, [0.0, 0.0, width, height]));
        if self.loading_art.is_none() {
            static ART: &[u8] =
                include_bytes!("../../../../assets/backgrounds/loading-screen/image1.png");
            self.loading_art = Some(image::load_from_memory(ART).ok().map(|i| {
                let i = i.into_rgba8();
                let (iw, ih) = i.dimensions();
                let img = ::texture::Image {
                    width: iw,
                    height: ih,
                    rgba: i.into_raw(),
                    has_alpha: false,
                };
                (r.add_texture(scene, &img, false), iw, ih)
            }));
        }
        if let Some(Some((tex, iw, ih))) = self.loading_art {
            let k = (width / iw.max(1) as f32).max(height / ih.max(1) as f32);
            let (dw, dh) = (iw as f32 * k, ih as f32 * k);
            let (x0, y0) = ((width - dw) * 0.5, (height - dh) * 0.5);
            scene.overlays.push((tex, [x0, y0, x0 + dw, y0 + dh]));
        }
        let m = 40.0 * s;
        self.ensure_logo();
        let logo_px = self.logo_at(r, scene, 52.0 * s);
        match logo_px {
            Some((tex, lw, lh)) => {
                let (lx, ly) = (m.round(), (height - m).round() - lh as f32);
                scene
                    .overlays
                    .push((tex, [lx, ly, lx + lw as f32, ly + lh as f32]));
            }
            _ => {
                let logo =
                    self.text
                        .label(r, scene, "neoOMSI", (64.0 * s) as u32, [255, 255, 255, 0]);
                scene.overlays.push((
                    logo.tex,
                    [m, height - m - logo.h as f32, m + logo.w as f32, height - m],
                ));
            }
        }
        if self.spinner.is_empty() {
            const N: usize = 24;
            const SZ: usize = 96;
            for k in 0..N {
                let head = k as f32 * std::f32::consts::TAU / N as f32;
                let mut data = vec![255u8; SZ * SZ * 4];
                for py in 0..SZ {
                    for px in 0..SZ {
                        let (dx, dy) = (
                            px as f32 + 0.5 - SZ as f32 * 0.5,
                            py as f32 + 0.5 - SZ as f32 * 0.5,
                        );
                        let rr = (dx * dx + dy * dy).sqrt();
                        let ring =
                            (45.0 - rr + 0.5).clamp(0.0, 1.0) * (rr - 31.0 + 0.5).clamp(0.0, 1.0);
                        let ang = dx.atan2(-dy).rem_euclid(std::f32::consts::TAU);
                        let behind = (head - ang).rem_euclid(std::f32::consts::TAU);
                        let tail = std::f32::consts::PI * 1.1;
                        let arc = if behind < tail {
                            (1.0 - behind / tail).powf(0.8)
                        } else {
                            0.0
                        };
                        let a = ring * (0.28 + 0.72 * arc);
                        data[(py * SZ + px) * 4 + 3] = (a * 255.0).round() as u8;
                    }
                }
                let img = ::texture::Image {
                    width: SZ as u32,
                    height: SZ as u32,
                    rgba: data,
                    has_alpha: true,
                };
                let tex = r.add_texture(scene, &img, false);
                self.spinner.push(tex);
            }
        }
        let pct = progress
            .map(|p| format!("   {:.0} %", p.clamp(0.0, 1.0) * 100.0))
            .unwrap_or_default();
        let compose = |pct: &str| {
            if title.is_empty() {
                format!("{caption}{pct}")
            } else {
                format!("{caption} {title}{pct}")
            }
        };
        let line = compose(&pct);
        let widest = compose(if progress.is_some() { "   100 %" } else { "" });
        let px_text = (18.0 * s) as u32;
        let text_w = self.text.width(&widest, px_text as f32);
        let ring = 22.0 * s;
        let (pad_x, pad_y, gap) = (16.0 * s, 10.0 * s, 16.0 * s);
        let c = self
            .text
            .label(r, scene, &line, px_text, [235, 235, 235, 0]);
        let box_h = (c.h as f32).max(ring) + pad_y * 2.0;
        let box_w = text_w + gap + ring + pad_x * 2.0;
        let (bx1, by1) = (width - m, height - m);
        let (bx0, by0) = (bx1 - box_w, by1 - box_h);
        self.text
            .rounded(r, scene, [bx0, by0, bx1, by1], 3.0 * s, [0, 0, 0, 240]);
        let cy = (by0 + by1) * 0.5;
        scene.overlays.push((
            c.tex,
            [
                bx0 + pad_x,
                cy - c.h as f32 * 0.5,
                bx0 + pad_x + c.w as f32,
                cy + c.h as f32 * 0.5,
            ],
        ));
        let frame = (t * 26.0) as usize % self.spinner.len().max(1);
        if let Some(tex) = self.spinner.get(frame).copied() {
            scene.overlays.push((
                tex,
                [
                    bx1 - pad_x - ring,
                    cy - ring * 0.5,
                    bx1 - pad_x,
                    cy + ring * 0.5,
                ],
            ));
        }
        let bh = (2.0 * s).max(2.0);
        let accent = self.text.plate(r, scene, 4);
        match progress {
            Some(p) => {
                let p = p.clamp(0.0, 1.0);
                if p > 0.0 {
                    scene
                        .overlays
                        .push((accent, [0.0, height - bh, width * p, height]));
                }
            }
            None => {
                let seg = 0.2;
                let head = -seg + (t * 0.5).fract() * (1.0 + seg);
                let (a, b) = (head.max(0.0), (head + seg).min(1.0));
                if b > a {
                    scene
                        .overlays
                        .push((accent, [width * a, height - bh, width * b, height]));
                }
            }
        }
        self.text.end_frame(r, scene);
    }
}
