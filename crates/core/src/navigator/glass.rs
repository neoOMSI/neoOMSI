use super::*;

/// How many halvings the blur takes (the card shows the map at 1 / 2^LEVELS).
const LEVELS: usize = 3;

pub(super) struct Glass {
    size: (u32, u32),
    /// The map and its halvings (0: full size).
    views: Vec<wgpu::TextureView>,
    sizes: Vec<(u32, u32)>,
    /// Draws the halvings: no MSAA, so no multisampled target per size.
    chain: Gpu,
    /// The textures in `chain`: level k is read to draw level k + 1.
    chain_ids: Vec<usize>,
    /// The full map and the blurred one in the panel's own `Gpu`.
    pub(super) map_id: usize,
    pub(super) blur_id: usize,
}

impl Glass {
    /// The glass for a panel of `size`, its textures known to `main` (the panel's `Gpu`):
    /// made anew when the panel changes size.
    pub(super) fn ensure(
        glass: &mut Option<Glass>,
        renderer: &Renderer,
        main: &mut Gpu,
        size: (u32, u32),
    ) {
        if glass.as_ref().map(|g| g.size == size).unwrap_or(false) {
            return;
        }
        let device = &renderer.device;
        let format = renderer.format();
        let mut views = Vec::new();
        let mut sizes = Vec::new();
        for k in 0..=LEVELS {
            let (w, h) = ((size.0 >> k).max(1), (size.1 >> k).max(1));
            let t = device.create_texture(&wgpu::TextureDescriptor {
                label: Some("navigator glass"),
                size: wgpu::Extent3d {
                    width: w,
                    height: h,
                    depth_or_array_layers: 1,
                },
                mip_level_count: 1,
                sample_count: 1,
                dimension: wgpu::TextureDimension::D2,
                format,
                usage: wgpu::TextureUsages::RENDER_ATTACHMENT
                    | wgpu::TextureUsages::TEXTURE_BINDING,
                view_formats: &[],
            });
            views.push(t.create_view(&Default::default()));
            sizes.push((w, h));
        }
        match glass.as_mut() {
            Some(g) => {
                for k in 0..LEVELS {
                    g.chain.set_view(device, g.chain_ids[k], &views[k], sizes[k]);
                }
                main.set_view(device, g.map_id, &views[0], sizes[0]);
                main.set_view(device, g.blur_id, &views[LEVELS], sizes[LEVELS]);
                g.views = views;
                g.sizes = sizes;
                g.size = size;
            }
            None => {
                let mut chain = Gpu::new(device, format, 1, 4);
                let chain_ids = (0..LEVELS)
                    .map(|k| chain.add_view(device, &views[k], sizes[k]))
                    .collect();
                let map_id = main.add_view(device, &views[0], sizes[0]);
                let blur_id = main.add_view(device, &views[LEVELS], sizes[LEVELS]);
                *glass = Some(Glass {
                    size,
                    views,
                    sizes,
                    chain,
                    chain_ids,
                    map_id,
                    blur_id,
                });
            }
        }
    }

    /// Where the map is drawn (full size).
    pub(super) fn map_view(&self) -> &wgpu::TextureView {
        &self.views[0]
    }

    /// Scale the map down level by level into the blurred texture.
    pub(super) fn blur(&mut self, renderer: &Renderer) {
        let (device, queue) = (&renderer.device, &renderer.queue);
        for k in 0..LEVELS {
            let (w, h) = self.sizes[k + 1];
            let r = Rect::new(0.0, 0.0, w as f32, h as f32);
            let mut p = Painter::new();
            p.image_rounded(r, 0.0, r, Color::WHITE, true);
            self.chain.upload(device, queue, k, &p.verts);
            let layer = Layer::flat([0.0, 0.0, w as f32, h as f32], 0.0, 1.0);
            let mut enc = device.create_command_encoder(&wgpu::CommandEncoderDescriptor {
                label: Some("navigator glass"),
            });
            // (one submit a level: the layer uniforms are written for each)
            self.chain.render(
                device,
                queue,
                &mut enc,
                &self.views[k + 1],
                (w, h),
                Some(wgpu::Color::TRANSPARENT),
                &[layer],
                &[Draw {
                    buffer: k,
                    range: 0..p.len(),
                    layer: 0,
                    texture: self.chain_ids[k],
                }],
            );
            queue.submit([enc.finish()]);
        }
    }
}
