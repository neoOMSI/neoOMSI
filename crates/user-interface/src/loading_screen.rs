use crate::paint::Align;
use crate::{Atlas, Color, Draw, Fonts, Gpu, Layer, Painter, Rect, Weight};
use glam::Vec2;

pub struct LoadingScreen {
    gpu: Gpu,
    atlas: Atlas,
    fonts: Fonts,
    bg: Option<(usize, u32, u32)>,
    logo: Option<(usize, u32, u32)>,
}

impl LoadingScreen {
    pub fn new(device: &wgpu::Device, queue: &wgpu::Queue, format: wgpu::TextureFormat) -> Self {
        let mut gpu = Gpu::new(device, format, 1, 512);
        static ART: &[u8] = include_bytes!("../../../assets/backgrounds/loading-screen/comp1.png");
        static LOGO: &[u8] = include_bytes!("../../../assets/logos/wordmark-gradient-dark.png");
        let bg = image::load_from_memory(ART).ok().map(|i| {
            let i = i.into_rgba8();
            let (w, h) = i.dimensions();
            (gpu.add_image(device, queue, w, h, i.as_raw()), w, h)
        });
        let logo = image::load_from_memory(LOGO).ok().and_then(|i| {
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
            let (w, h) = i.dimensions();
            Some((gpu.add_image(device, queue, w, h, i.as_raw()), w, h))
        });
        LoadingScreen {
            gpu,
            atlas: Atlas::new(512),
            fonts: Fonts::new(),
            bg,
            logo,
        }
    }

    #[allow(clippy::too_many_arguments)]
    pub fn draw(
        &mut self,
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        target: &wgpu::TextureView,
        size: (u32, u32),
        caption: &str,
        progress: Option<f32>,
        t: f32,
    ) {
        let (w, h) = (size.0.max(1) as f32, size.1.max(1) as f32);
        let s = (h / 1080.0).clamp(0.75, 2.5);
        let m = 40.0 * s;
        self.atlas.begin_frame();
        let mut bg = Painter::new();
        if let Some((_, iw, ih)) = self.bg {
            let k = (w / iw as f32).max(h / ih as f32);
            let (dw, dh) = (iw as f32 * k, ih as f32 * k);
            let r = Rect::new((w - dw) * 0.5, (h - dh) * 0.5, dw, dh);
            bg.image_rounded(r, 0.5, r, Color::WHITE, false);
        }
        let mut logo = Painter::new();
        if let Some((_, lw, lh)) = self.logo {
            let th = 52.0 * s;
            let tw = lw as f32 * th / lh as f32;
            let r = Rect::new(m, h - m - th, tw, th);
            logo.image_rounded(r, 0.5, r, Color::WHITE, false);
        }
        let mut ui = Painter::new();
        let px = 18.0 * s;
        let text = match progress {
            Some(p) => format!("{caption}   {:.0} %", p.clamp(0.0, 1.0) * 100.0),
            None => caption.to_string(),
        };
        let text_w = self.fonts.width(&text, px, Weight::Regular);
        let ring = 22.0 * s;
        let (pad_x, pad_y, gap) = (16.0 * s, 10.0 * s, 16.0 * s);
        let box_h = self.fonts.line_height(px, Weight::Regular).max(ring) + pad_y * 2.0;
        let box_w = text_w + gap + ring + pad_x * 2.0;
        let (bx1, by1) = (w - m, h - m);
        let (bx0, by0) = (bx1 - box_w, by1 - box_h);
        ui.rounded(
            Rect::new(bx0, by0, box_w, box_h),
            3.0 * s,
            Color::rgba(0, 0, 0, 240.0 / 255.0),
        );
        ui.text_in(
            &mut self.atlas,
            &self.fonts,
            &text,
            px,
            Weight::Regular,
            Rect::new(bx0 + pad_x, by0, text_w + 2.0, box_h),
            Align::Left,
            Color::rgba(235, 235, 235, 1.0),
        );
        let c = Vec2::new(bx1 - pad_x - ring * 0.5, (by0 + by1) * 0.5);
        let head = t * std::f32::consts::TAU * 0.8;
        ui.arc(c, ring * 0.36, ring * 0.5, 0.0, std::f32::consts::TAU, Color::rgba(255, 255, 255, 0.28));
        ui.arc(
            c,
            ring * 0.36,
            ring * 0.5,
            head - std::f32::consts::PI,
            head,
            Color::WHITE,
        );
        let bh = (2.0 * s).max(2.0);
        let accent = Color::hex(0xE22622);
        match progress {
            Some(p) => {
                let p = p.clamp(0.0, 1.0);
                if p > 0.0 {
                    ui.rect(Rect::new(0.0, h - bh, w * p, bh), accent);
                }
            }
            None => {
                let seg = 0.2;
                let hd = -seg + (t * 0.5).fract() * (1.0 + seg);
                let (a, b) = (hd.max(0.0), (hd + seg).min(1.0));
                if b > a {
                    ui.rect(Rect::new(w * a, h - bh, w * (b - a), bh), accent);
                }
            }
        }
        self.gpu.upload(device, queue, 0, &bg.verts);
        self.gpu.upload(device, queue, 1, &logo.verts);
        self.gpu.upload(device, queue, 2, &ui.verts);
        self.gpu.upload_atlas(queue, &mut self.atlas);
        let layers = [Layer::flat([0.0, 0.0, w, h], 0.0, 1.0)];
        let draws = [
            Draw {
                buffer: 0,
                range: 0..bg.len(),
                layer: 0,
                texture: self.bg.map(|b| b.0).unwrap_or(0),
            },
            Draw {
                buffer: 1,
                range: 0..logo.len(),
                layer: 0,
                texture: self.logo.map(|b| b.0).unwrap_or(0),
            },
            Draw {
                buffer: 2,
                range: 0..ui.len(),
                layer: 0,
                texture: 0,
            },
        ];
        let mut enc = device.create_command_encoder(&wgpu::CommandEncoderDescriptor {
            label: Some("loading screen"),
        });
        self.gpu.render(
            device,
            queue,
            &mut enc,
            target,
            size,
            Some(wgpu::Color {
                r: 0.0056,
                g: 0.0056,
                b: 0.0056,
                a: 1.0,
            }),
            &layers,
            &draws,
        );
        queue.submit([enc.finish()]);
    }
}
