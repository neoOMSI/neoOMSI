use crate::*;

/// How far around a frosted overlay the picture is taken for its blur (pixels): the blur
/// reaches about this far, so the edge of what was taken never shows under the overlay.
const MARGIN: f32 = 32.0;

/// The frosted backdrops of overlays (`Scene::frosted`): the picture beneath each is
/// copied, shrunk to a quarter, blurred and drawn under the overlay with its corners.
pub(crate) struct Frost {
    shrink: wgpu::RenderPipeline,
    blur: wgpu::RenderPipeline,
    under: wgpu::RenderPipeline,
    sampler: wgpu::Sampler,
    /// One for each frosted overlay of the frame, in their order.
    slots: Vec<Slot>,
}

struct Slot {
    size: (u32, u32),
    format: wgpu::TextureFormat,
    src: wgpu::Texture,
    small: [wgpu::TextureView; 2],
    /// The shrink, the blur across, the blur down and the backdrop drawn under the overlay.
    bufs: [wgpu::Buffer; 4],
    groups: [wgpu::BindGroup; 4],
}

impl Frost {
    pub(crate) fn new(
        device: &wgpu::Device,
        layout: &wgpu::BindGroupLayout,
        format: wgpu::TextureFormat,
    ) -> Frost {
        let shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("frost"),
            source: wgpu::ShaderSource::Wgsl(include_str!("../../shaders/ui/frost.wgsl").into()),
        });
        let pl = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("frost"),
            bind_group_layouts: &[Some(layout)],
            immediate_size: 0,
        });
        let pipeline = |label, vs, fs, format, blend| {
            device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
                label: Some(label),
                layout: Some(&pl),
                vertex: wgpu::VertexState {
                    module: &shader,
                    entry_point: Some(vs),
                    buffers: &[],
                    compilation_options: Default::default(),
                },
                primitive: Default::default(),
                depth_stencil: None,
                multisample: Default::default(),
                fragment: Some(wgpu::FragmentState {
                    module: &shader,
                    entry_point: Some(fs),
                    targets: &[Some(wgpu::ColorTargetState {
                        format,
                        blend,
                        write_mask: wgpu::ColorWrites::ALL,
                    })],
                    compilation_options: Default::default(),
                }),
                multiview_mask: None,
                cache: None,
            })
        };
        let premul = wgpu::BlendState {
            color: wgpu::BlendComponent {
                src_factor: wgpu::BlendFactor::One,
                dst_factor: wgpu::BlendFactor::OneMinusSrcAlpha,
                operation: wgpu::BlendOperation::Add,
            },
            alpha: wgpu::BlendComponent::OVER,
        };
        let small = wgpu::TextureFormat::Rgba16Float;
        Frost {
            shrink: pipeline("frost shrink", "vs_full", "fs_shrink", small, None),
            blur: pipeline("frost blur", "vs_full", "fs_blur", small, None),
            under: pipeline("frost under", "vs_rect", "fs_under", format, Some(premul)),
            sampler: device.create_sampler(&wgpu::SamplerDescriptor {
                label: Some("frost"),
                mag_filter: wgpu::FilterMode::Linear,
                min_filter: wgpu::FilterMode::Linear,
                ..Default::default()
            }),
            slots: Vec::new(),
        }
    }

    /// The backdrop of slot `k` drawn under its overlay, into a pass on the output.
    pub(crate) fn draw_under(&self, pass: &mut wgpu::RenderPass<'_>, k: usize) {
        pass.set_pipeline(&self.under);
        pass.set_bind_group(0, &self.slots[k].groups[3], &[]);
        pass.draw(0..6, 0..1);
    }
}

fn slot(
    device: &wgpu::Device,
    layout: &wgpu::BindGroupLayout,
    sampler: &wgpu::Sampler,
    size: (u32, u32),
    format: wgpu::TextureFormat,
) -> Slot {
    let texture = |label, (w, h): (u32, u32), format, usage| {
        device.create_texture(&wgpu::TextureDescriptor {
            label: Some(label),
            size: wgpu::Extent3d {
                width: w,
                height: h,
                depth_or_array_layers: 1,
            },
            mip_level_count: 1,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format,
            usage,
            view_formats: &[],
        })
    };
    let src = texture(
        "frost source",
        size,
        format,
        wgpu::TextureUsages::COPY_DST | wgpu::TextureUsages::TEXTURE_BINDING,
    );
    let quarter = (size.0.div_ceil(4), size.1.div_ceil(4));
    let small = [0, 1].map(|_| {
        texture(
            "frost blur",
            quarter,
            wgpu::TextureFormat::Rgba16Float,
            wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::TEXTURE_BINDING,
        )
        .create_view(&Default::default())
    });
    let src_view = src.create_view(&Default::default());
    let bufs = [0, 1, 2, 3].map(|_| {
        device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("frost"),
            size: 48,
            usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        })
    });
    // the shrink reads the copy, the blur across the shrunk picture, the blur down and the
    // backdrop what the blur across made of it
    let reads = [&src_view, &small[0], &small[1], &small[0]];
    let groups = [0, 1, 2, 3].map(|k| {
        device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("frost"),
            layout,
            entries: &[
                wgpu::BindGroupEntry {
                    binding: 0,
                    resource: bufs[k].as_entire_binding(),
                },
                wgpu::BindGroupEntry {
                    binding: 1,
                    resource: wgpu::BindingResource::TextureView(reads[k]),
                },
                wgpu::BindGroupEntry {
                    binding: 2,
                    resource: wgpu::BindingResource::Sampler(sampler),
                },
            ],
        })
    });
    Slot {
        size,
        format,
        src,
        small,
        bufs,
        groups,
    }
}

impl Renderer {
    /// Blur what lies beneath each frosted overlay in `target` (an output of `full` pixels
    /// at `viewport`, if one), before the overlays are drawn: for each overlay, the slot of
    /// its backdrop to draw under it with [`Frost::draw_under`].
    pub(crate) fn frost_backdrops(
        &mut self,
        encoder: &mut wgpu::CommandEncoder,
        scene: &Scene,
        target: &wgpu::TextureView,
        overlays: &[(TextureId, [f32; 4])],
        full: (u32, u32),
        viewport: Option<(u32, u32, u32, u32)>,
    ) -> Vec<Option<usize>> {
        let mut under = vec![None; overlays.len()];
        let out = target.texture();
        if scene.frosted.is_empty()
            || !overlays.iter().any(|(t, _)| scene.frosted.contains_key(t))
            || !out.usage().contains(wgpu::TextureUsages::COPY_SRC)
            || out.sample_count() != 1
            || out.format() != self.format
        {
            return under;
        }
        let frost = self
            .frost
            .get_or_insert_with(|| Frost::new(&self.device, &self.overlay_layout, self.format));
        let (ox, oy) = viewport.map(|v| (v.0, v.1)).unwrap_or((0, 0));
        let (fw, fh) = (full.0 as f32, full.1 as f32);
        let mut used = 0;
        for (k, (tex, rect)) in overlays.iter().enumerate() {
            let Some(&radius) = scene.frosted.get(tex) else {
                continue;
            };
            let r = snap_rect(*rect);
            let x0 = (r[0] - MARGIN).max(0.0).floor();
            let y0 = (r[1] - MARGIN).max(0.0).floor();
            let x1 = (r[2] + MARGIN).min(fw).ceil();
            let y1 = (r[3] + MARGIN).min(fh).ceil();
            if x1 - x0 < 4.0 || y1 - y0 < 4.0 || r[2] <= r[0] || r[3] <= r[1] {
                continue;
            }
            let size = ((x1 - x0) as u32, (y1 - y0) as u32);
            let origin = (x0 as u32 + ox, y0 as u32 + oy);
            if origin.0 + size.0 > out.width() || origin.1 + size.1 > out.height() {
                continue;
            }
            if frost.slots.len() == used {
                frost.slots.push(slot(
                    &self.device,
                    &self.overlay_layout,
                    &frost.sampler,
                    size,
                    self.format,
                ));
            } else if frost.slots[used].size != size || frost.slots[used].format != self.format
            {
                frost.slots[used] = slot(
                    &self.device,
                    &self.overlay_layout,
                    &frost.sampler,
                    size,
                    self.format,
                );
            }
            let s = &frost.slots[used];
            let (sw, sh) = (size.0 as f32, size.1 as f32);
            let (qw, qh) = (size.0.div_ceil(4) as f32, size.1.div_ceil(4) as f32);
            let params: [[f32; 12]; 4] = [
                [1.0 / sw, 1.0 / sh, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0],
                [1.0 / qw, 1.0 / qh, 1.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0],
                [1.0 / qw, 1.0 / qh, 0.0, 1.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0],
                [
                    r[0] / fw * 2.0 - 1.0,
                    1.0 - r[1] / fh * 2.0,
                    r[2] / fw * 2.0 - 1.0,
                    1.0 - r[3] / fh * 2.0,
                    (r[0] - x0) / sw,
                    (r[1] - y0) / sh,
                    (r[2] - x0) / sw,
                    (r[3] - y0) / sh,
                    r[2] - r[0],
                    r[3] - r[1],
                    radius,
                    0.0,
                ],
            ];
            for (buf, p) in s.bufs.iter().zip(&params) {
                self.queue.write_buffer(buf, 0, bytemuck::cast_slice(p));
            }
            encoder.copy_texture_to_texture(
                wgpu::TexelCopyTextureInfo {
                    texture: out,
                    mip_level: 0,
                    origin: wgpu::Origin3d {
                        x: origin.0,
                        y: origin.1,
                        z: 0,
                    },
                    aspect: wgpu::TextureAspect::All,
                },
                wgpu::TexelCopyTextureInfo {
                    texture: &s.src,
                    mip_level: 0,
                    origin: wgpu::Origin3d::ZERO,
                    aspect: wgpu::TextureAspect::All,
                },
                wgpu::Extent3d {
                    width: size.0,
                    height: size.1,
                    depth_or_array_layers: 1,
                },
            );
            for (step, (pipeline, into)) in [
                (&frost.shrink, &s.small[0]),
                (&frost.blur, &s.small[1]),
                (&frost.blur, &s.small[0]),
            ]
            .into_iter()
            .enumerate()
            {
                let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                    label: Some("frost"),
                    color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                        view: into,
                        depth_slice: None,
                        resolve_target: None,
                        ops: wgpu::Operations {
                            load: wgpu::LoadOp::Clear(wgpu::Color::BLACK),
                            store: wgpu::StoreOp::Store,
                        },
                    })],
                    depth_stencil_attachment: None,
                    timestamp_writes: None,
                    occlusion_query_set: None,
                    multiview_mask: None,
                });
                pass.set_pipeline(pipeline);
                pass.set_bind_group(0, &s.groups[step], &[]);
                pass.draw(0..3, 0..1);
            }
            under[k] = Some(used);
            used += 1;
        }
        under
    }
}
