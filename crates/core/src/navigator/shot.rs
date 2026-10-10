use super::*;

impl Navigator {
    /// The panel drawn once more for frame `f` and read back as RGBA (width, height,
    /// pixels): the offscreen checks look at what the driver would see.
    pub fn shot(&mut self, renderer: &Renderer, f: &NavFrame) -> Option<(u32, u32, Vec<u8>)> {
        let (_, w, h) = self.target?;
        let device = &renderer.device;
        let tex = device.create_texture(&wgpu::TextureDescriptor {
            label: Some("navigator shot"),
            size: wgpu::Extent3d {
                width: w,
                height: h,
                depth_or_array_layers: 1,
            },
            mip_level_count: 1,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format: renderer.format(),
            usage: wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::COPY_SRC,
            view_formats: &[],
        });
        let view = tex.create_view(&Default::default());
        self.draw(renderer, &view, (w, h), (w as f32 * 0.62).round(), f);
        let bpr = (w * 4).div_ceil(256) * 256;
        let buf = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("navigator shot readback"),
            size: (bpr * h) as u64,
            usage: wgpu::BufferUsages::COPY_DST | wgpu::BufferUsages::MAP_READ,
            mapped_at_creation: false,
        });
        let mut enc = device.create_command_encoder(&Default::default());
        enc.copy_texture_to_buffer(
            wgpu::TexelCopyTextureInfo {
                texture: &tex,
                mip_level: 0,
                origin: wgpu::Origin3d::ZERO,
                aspect: wgpu::TextureAspect::All,
            },
            wgpu::TexelCopyBufferInfo {
                buffer: &buf,
                layout: wgpu::TexelCopyBufferLayout {
                    offset: 0,
                    bytes_per_row: Some(bpr),
                    rows_per_image: Some(h),
                },
            },
            wgpu::Extent3d {
                width: w,
                height: h,
                depth_or_array_layers: 1,
            },
        );
        let idx = renderer.queue.submit([enc.finish()]);
        let slice = buf.slice(..);
        let (tx, rx) = std::sync::mpsc::channel();
        slice.map_async(wgpu::MapMode::Read, move |r| {
            let _ = tx.send(r);
        });
        ::render::wait_gpu(device, Some(idx)).ok()?;
        rx.recv().ok()?.ok()?;
        let data = slice.get_mapped_range().ok()?;
        let mut out = Vec::with_capacity((w * h * 4) as usize);
        for row in 0..h {
            let start = (row * bpr) as usize;
            out.extend_from_slice(&data[start..start + (w * 4) as usize]);
        }
        drop(data);
        buf.unmap();
        let bgra = matches!(
            renderer.format(),
            wgpu::TextureFormat::Bgra8Unorm | wgpu::TextureFormat::Bgra8UnormSrgb
        );
        for px in out.chunks_exact_mut(4) {
            if bgra {
                px.swap(0, 2);
            }
            // premultiplied over a dark ground, as the panel sits on the picture
            let a = px[3] as f32 / 255.0;
            for c in &mut px[..3] {
                *c = (*c as f32 + 40.0 * (1.0 - a)).min(255.0) as u8;
            }
            px[3] = 255;
        }
        Some((w, h, out))
    }
}
