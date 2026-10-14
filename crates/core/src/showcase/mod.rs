mod input;
mod light;
mod load;
mod look;
mod matte;
mod view;

use crate::*;
use ::render::Renderer;
use std::sync::mpsc::TryRecvError;

pub use look::Look;

pub struct Showcase {
    model: Option<load::Model>,
    wanted: Option<Look>,
    failed: Option<Look>,
    loading: Option<(Look, load::Job)>,
    pub error: Option<String>,
    view: view::View,
    grab: Option<(f32, f32, bool)>,
    pub auto_turn: bool,
    pub focus_x: f32,
    focus_now: f32,
    pub busy: bool,
    target: Option<(wgpu::Texture, wgpu::TextureView, u32, u32)>,
    dirty: bool,
    snap_size: (u32, u32),
    pub generation: u64,
}

impl Showcase {
    pub fn new() -> Showcase {
        Showcase {
            model: None,
            wanted: None,
            failed: None,
            loading: None,
            error: None,
            view: view::View::new(),
            grab: None,
            auto_turn: false,
            focus_x: 0.5,
            focus_now: 0.5,
            busy: false,
            target: None,
            dirty: true,
            snap_size: (0, 0),
            generation: 0,
        }
    }

    pub fn want(&mut self, look: Look) {
        if self.wanted.as_ref() != Some(&look) {
            self.wanted = Some(look);
        }
    }

    pub fn orbit(&mut self, dx: f32, dy: f32) {
        self.view.orbit(dx, dy);
    }

    pub fn pan(&mut self, dx: f32, dy: f32) {
        let r = self.model.as_ref().map_or(6.0, |m| m.radius);
        self.view.pan(dx, dy, r);
    }

    pub fn begin_grab(&mut self, x: f32, y: f32, pan: bool) {
        self.grab = Some((x, y, pan));
    }

    pub fn drag_to(&mut self, x: f32, y: f32) -> bool {
        let Some((lx, ly, pan)) = self.grab else {
            return false;
        };
        self.grab = Some((x, y, pan));
        if pan {
            self.pan(x - lx, y - ly);
        } else {
            self.orbit(x - lx, y - ly);
        }
        true
    }

    pub fn end_grab(&mut self) -> bool {
        self.grab.take().is_some()
    }

    pub fn zoom_by(&mut self, k: f32) {
        self.view.zoom_by(k);
    }

    pub fn reset_view(&mut self) {
        self.view.reset();
    }

    pub fn update(&mut self, renderer: &Renderer, dt: f32) -> bool {
        let mut swapped = false;
        if let Some((look, job)) = self.loading.as_ref() {
            match job.try_recv() {
                Ok(Ok(ready)) => {
                    self.model = Some(load::place(renderer, ready));
                    self.loading = None;
                    self.error = None;
                    self.dirty = true;
                    swapped = true;
                }
                Ok(Err(e)) => {
                    log::warn!("showroom {}: {e}", look.bus);
                    self.error = Some(e);
                    self.failed = Some(look.clone());
                    self.model = None;
                    self.loading = None;
                }
                Err(TryRecvError::Empty) => {}
                Err(TryRecvError::Disconnected) => {
                    self.failed = Some(look.clone());
                    self.loading = None;
                }
            }
        }
        if let Some(w) = self.wanted.clone() {
            let current = self.model.as_ref().map(|m| m.look.clone());
            let pending = self.loading.as_ref().map(|l| l.0.clone());
            if current.as_ref() != Some(&w)
                && pending.as_ref() != Some(&w)
                && self.failed.as_ref() != Some(&w)
            {
                let same = current.as_ref().is_some_and(|c| c.same_model(&w));
                if let (Some(m), true) = (self.model.as_mut(), same) {
                    m.look = w;
                    self.dirty = true;
                } else if self.loading.is_none() {
                    match load::spawn(renderer, &w) {
                        Ok(job) => self.loading = Some((w, job)),
                        Err(e) => {
                            self.error = Some(e);
                            self.failed = Some(w);
                        }
                    }
                }
            }
        }
        self.busy = self.loading.is_some();

        if self.view.step(dt, self.auto_turn) {
            self.dirty = true;
        }
        self.focus_now += (self.focus_x - self.focus_now) * (1.0 - (-dt / 0.35).exp());
        if (self.focus_x - self.focus_now).abs() > 0.001 {
            self.dirty = true;
        }
        if let (true, Some(m)) = (self.dirty, self.model.as_mut()) {
            sync_vehicle_transforms(
                renderer,
                &mut m.scene,
                &mut m.vehicle,
                &mut m.render,
                &mut m.trailers,
                false,
            );
        }
        swapped
    }

    fn draw(&mut self, renderer: &mut Renderer, w: u32, h: u32) -> Option<Vec<u8>> {
        let m = self.model.as_ref()?;
        let cam = self.view.camera(m.centre, m.radius, w, h, self.focus_now);
        let m = self.model.as_mut()?;
        m.scene.overlays.clear();
        matte::render(renderer, &mut m.scene, w, h, &cam, &m.lighting)
    }

    pub fn preview(&mut self, renderer: &mut Renderer, w: u32, h: u32) -> Option<wgpu::TextureView> {
        self.model.as_ref()?;
        let (w, h) = (w.max(16), h.max(16));
        if self.target.as_ref().is_none_or(|t| (t.2, t.3) != (w, h)) {
            let format = match renderer.format() {
                wgpu::TextureFormat::Bgra8Unorm => wgpu::TextureFormat::Rgba8Unorm,
                wgpu::TextureFormat::Bgra8UnormSrgb => wgpu::TextureFormat::Rgba8UnormSrgb,
                f => f,
            };
            let tex = renderer.device.create_texture(&wgpu::TextureDescriptor {
                label: Some("showcase"),
                size: wgpu::Extent3d { width: w, height: h, depth_or_array_layers: 1 },
                mip_level_count: 1,
                sample_count: 1,
                dimension: wgpu::TextureDimension::D2,
                format,
                usage: wgpu::TextureUsages::TEXTURE_BINDING | wgpu::TextureUsages::COPY_DST,
                view_formats: &[],
            });
            let view = tex.create_view(&Default::default());
            self.target = Some((tex, view, w, h));
            self.generation += 1;
            self.dirty = true;
        }
        if self.dirty {
            self.dirty = false;
            if let Some(px) = self.draw(renderer, w, h) {
                if let Some((tex, ..)) = self.target.as_ref() {
                    renderer.queue.write_texture(
                        wgpu::TexelCopyTextureInfo {
                            texture: tex,
                            mip_level: 0,
                            origin: wgpu::Origin3d::ZERO,
                            aspect: wgpu::TextureAspect::All,
                        },
                        &px,
                        wgpu::TexelCopyBufferLayout {
                            offset: 0,
                            bytes_per_row: Some(w * 4),
                            rows_per_image: Some(h),
                        },
                        wgpu::Extent3d { width: w, height: h, depth_or_array_layers: 1 },
                    );
                }
            }
        }
        self.target.as_ref().map(|t| t.1.clone())
    }

    pub fn snapshot(&mut self, renderer: &mut Renderer, w: u32, h: u32) -> Option<(u32, u32, Vec<u8>)> {
        self.model.as_ref()?;
        let (w, h) = (w.max(16), h.max(16));
        if self.snap_size != (w, h) {
            self.snap_size = (w, h);
            self.dirty = true;
        }
        if !self.dirty {
            return None;
        }
        self.dirty = false;
        self.draw(renderer, w, h).map(|px| (w, h, px))
    }

    pub fn has_scene(&self) -> bool {
        self.model.is_some()
    }

    pub fn has_picture(&self) -> bool {
        self.model.is_some() && self.target.is_some()
    }
}
