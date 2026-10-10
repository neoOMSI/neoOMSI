//! The wgpu side: one pipeline for every [`Vertex`](Vertex) list, drawn in layers
//! (each its own camera, viewport and rounded clip) into a window or a texture, MSAA
//! resolved, premultiplied alpha.

use glam::Mat4;
use std::ops::Range;

use crate::atlas::Atlas;
use crate::paint::Vertex;

/// How one group of draws is seen.
#[derive(Clone, Copy, Debug)]
pub struct Layer {
    pub view_proj: Mat4,
    /// Target pixels the world is drawn into (x, y, w, h); pixel vertices ignore it.
    pub viewport: [f32; 4],
    /// Everything outside this box (x0, y0, x1, y1) with rounded corners fades out.
    pub clip: [f32; 4],
    pub radius: f32,
    pub opacity: f32,
    /// Metres per pixel per metre of depth (`2 tan(fov/2) / viewport height`).
    pub px_scale: f32,
    /// For ribbons drawn with [`crate::Painter::ribbon_along`]: cut off below this distance
    /// along, dimmed beyond the second, faded out over the last 150 m before the third (0:
    /// no fade).
    pub route: [f32; 3],
}

impl Layer {
    /// A `route` that shows the whole of a ribbon.
    pub const WHOLE_ROUTE: [f32; 3] = [-1.0e9, 1.0e9, 0.0];
}

impl Layer {
    /// Pixels only, clipped to `clip`.
    pub fn flat(clip: [f32; 4], radius: f32, opacity: f32) -> Layer {
        Layer {
            view_proj: Mat4::IDENTITY,
            viewport: [0.0; 4],
            clip,
            radius,
            opacity,
            px_scale: 0.0,
            route: Self::WHOLE_ROUTE,
        }
    }
    /// A perspective view of a world drawn into `viewport`.
    pub fn world(
        view: Mat4,
        fov_y: f32,
        viewport: [f32; 4],
        clip: [f32; 4],
        radius: f32,
        opacity: f32,
    ) -> Layer {
        let aspect = viewport[2] / viewport[3].max(1.0);
        let proj = glam::camera::rh::proj::directx::perspective(fov_y, aspect, 1.0, 20000.0);
        Layer {
            view_proj: proj * view,
            viewport,
            clip,
            radius,
            opacity,
            px_scale: 2.0 * (fov_y * 0.5).tan() / viewport[3].max(1.0),
            route: Self::WHOLE_ROUTE,
        }
    }
}

/// Vertices `range` of buffer `buffer`, seen as layer `layer`, with texture `texture`
/// (0 = the atlas).
#[derive(Clone, Debug)]
pub struct Draw {
    pub buffer: usize,
    pub range: Range<u32>,
    pub layer: usize,
    pub texture: usize,
}

#[repr(C)]
#[derive(Clone, Copy, bytemuck::Pod, bytemuck::Zeroable)]
struct Uniform {
    view_proj: [[f32; 4]; 4],
    viewport: [f32; 4],
    target: [f32; 4],
    clip: [f32; 4],
    params: [f32; 4],
}

const SLOT: u64 = 256;
const MAX_LAYERS: u64 = 256;

struct Tex {
    texture: Option<wgpu::Texture>,
    group: wgpu::BindGroup,
    size: (u32, u32),
}

pub struct Gpu {
    pipeline: wgpu::RenderPipeline,
    tex_layout: wgpu::BindGroupLayout,
    uniform: wgpu::Buffer,
    uniform_group: wgpu::BindGroup,
    sampler: wgpu::Sampler,
    /// The vertex buffers and their capacity (None: making it failed; tried again next upload).
    buffers: Vec<(Option<wgpu::Buffer>, u64)>,
    textures: Vec<Option<Tex>>,
    msaa: Option<(wgpu::TextureView, u32, u32)>,
    samples: u32,
    format: wgpu::TextureFormat,
    atlas_size: u32,
    /// (to make the atlas texture anew when the atlas grows)
    device: wgpu::Device,
}

impl Gpu {
    /// A pipeline drawing into `format` with `samples` MSAA samples, and an atlas texture of
    /// `atlas_size` pixels square (texture 0).
    pub fn new(
        device: &wgpu::Device,
        format: wgpu::TextureFormat,
        samples: u32,
        atlas_size: u32,
    ) -> Gpu {
        let shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("user-interface"),
            source: wgpu::ShaderSource::Wgsl(include_str!("ui.wgsl").into()),
        });
        let u_layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("user-interface uniform"),
            entries: &[wgpu::BindGroupLayoutEntry {
                binding: 0,
                visibility: wgpu::ShaderStages::VERTEX_FRAGMENT,
                ty: wgpu::BindingType::Buffer {
                    ty: wgpu::BufferBindingType::Uniform,
                    has_dynamic_offset: true,
                    min_binding_size: wgpu::BufferSize::new(size_of::<Uniform>() as u64),
                },
                count: None,
            }],
        });
        let tex_layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("user-interface texture"),
            entries: &[
                wgpu::BindGroupLayoutEntry {
                    binding: 0,
                    visibility: wgpu::ShaderStages::FRAGMENT,
                    ty: wgpu::BindingType::Texture {
                        sample_type: wgpu::TextureSampleType::Float { filterable: true },
                        view_dimension: wgpu::TextureViewDimension::D2,
                        multisampled: false,
                    },
                    count: None,
                },
                wgpu::BindGroupLayoutEntry {
                    binding: 1,
                    visibility: wgpu::ShaderStages::FRAGMENT,
                    ty: wgpu::BindingType::Sampler(wgpu::SamplerBindingType::Filtering),
                    count: None,
                },
            ],
        });
        let layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("user-interface"),
            bind_group_layouts: &[Some(&u_layout), Some(&tex_layout)],
            immediate_size: 0,
        });
        let attrs = wgpu::vertex_attr_array![0 => Float32x3, 1 => Float32x2, 2 => Float32x2, 3 => Float32x2, 4 => Float32x4, 5 => Float32x2];
        let pipeline = device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
            label: Some("user-interface"),
            layout: Some(&layout),
            vertex: wgpu::VertexState {
                module: &shader,
                entry_point: Some("vs_main"),
                buffers: &[Some(wgpu::VertexBufferLayout {
                    array_stride: size_of::<Vertex>() as u64,
                    step_mode: wgpu::VertexStepMode::Vertex,
                    attributes: &attrs,
                })],
                compilation_options: Default::default(),
            },
            primitive: wgpu::PrimitiveState {
                topology: wgpu::PrimitiveTopology::TriangleList,
                cull_mode: None,
                ..Default::default()
            },
            depth_stencil: None,
            multisample: wgpu::MultisampleState {
                count: samples,
                mask: !0,
                alpha_to_coverage_enabled: false,
            },
            fragment: Some(wgpu::FragmentState {
                module: &shader,
                entry_point: Some("fs_main"),
                targets: &[Some(wgpu::ColorTargetState {
                    format,
                    blend: Some(wgpu::BlendState {
                        color: wgpu::BlendComponent {
                            src_factor: wgpu::BlendFactor::One,
                            dst_factor: wgpu::BlendFactor::OneMinusSrcAlpha,
                            operation: wgpu::BlendOperation::Add,
                        },
                        alpha: wgpu::BlendComponent {
                            src_factor: wgpu::BlendFactor::One,
                            dst_factor: wgpu::BlendFactor::OneMinusSrcAlpha,
                            operation: wgpu::BlendOperation::Add,
                        },
                    }),
                    write_mask: wgpu::ColorWrites::ALL,
                })],
                compilation_options: Default::default(),
            }),
            multiview_mask: None,
            cache: None,
        });
        let uniform = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("user-interface uniform"),
            size: SLOT * MAX_LAYERS,
            usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });
        let uniform_group = device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("user-interface uniform"),
            layout: &u_layout,
            entries: &[wgpu::BindGroupEntry {
                binding: 0,
                resource: wgpu::BindingResource::Buffer(wgpu::BufferBinding {
                    buffer: &uniform,
                    offset: 0,
                    size: wgpu::BufferSize::new(size_of::<Uniform>() as u64),
                }),
            }],
        });
        let sampler = device.create_sampler(&wgpu::SamplerDescriptor {
            label: Some("user-interface"),
            address_mode_u: wgpu::AddressMode::ClampToEdge,
            address_mode_v: wgpu::AddressMode::ClampToEdge,
            mag_filter: wgpu::FilterMode::Linear,
            min_filter: wgpu::FilterMode::Linear,
            mipmap_filter: wgpu::MipmapFilterMode::Linear,
            ..Default::default()
        });
        let mut g = Gpu {
            pipeline,
            tex_layout,
            uniform,
            uniform_group,
            sampler,
            buffers: Vec::new(),
            textures: Vec::new(),
            msaa: None,
            samples,
            format,
            atlas_size: atlas_size.max(1),
            device: device.clone(),
        };
        let atlas = vec![0u8; (atlas_size * atlas_size * 4) as usize];
        g.add_texture_internal(device, None, atlas_size, atlas_size, &atlas);
        g
    }

    fn add_texture_internal(
        &mut self,
        device: &wgpu::Device,
        queue: Option<&wgpu::Queue>,
        w: u32,
        h: u32,
        rgba: &[u8],
    ) -> usize {
        let texture = device.create_texture(&wgpu::TextureDescriptor {
            label: Some("user-interface image"),
            size: wgpu::Extent3d {
                width: w.max(1),
                height: h.max(1),
                depth_or_array_layers: 1,
            },
            mip_level_count: 1,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format: wgpu::TextureFormat::Rgba8UnormSrgb,
            usage: wgpu::TextureUsages::TEXTURE_BINDING | wgpu::TextureUsages::COPY_DST,
            view_formats: &[],
        });
        if let Some(q) = queue {
            Self::write(q, &texture, w, [0, 0, w, h], rgba);
        }
        let view = texture.create_view(&Default::default());
        let group = self.group(device, &view);
        self.textures.push(Some(Tex {
            texture: Some(texture),
            group,
            size: (w, h),
        }));
        self.textures.len() - 1
    }

    fn group(&self, device: &wgpu::Device, view: &wgpu::TextureView) -> wgpu::BindGroup {
        device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("user-interface texture"),
            layout: &self.tex_layout,
            entries: &[
                wgpu::BindGroupEntry {
                    binding: 0,
                    resource: wgpu::BindingResource::TextureView(view),
                },
                wgpu::BindGroupEntry {
                    binding: 1,
                    resource: wgpu::BindingResource::Sampler(&self.sampler),
                },
            ],
        })
    }

    fn write(queue: &wgpu::Queue, t: &wgpu::Texture, stride_w: u32, r: [u32; 4], rgba: &[u8]) {
        if r[2] == 0 || r[3] == 0 {
            return;
        }
        let offset = ((r[1] * stride_w + r[0]) * 4) as usize;
        queue.write_texture(
            wgpu::TexelCopyTextureInfo {
                texture: t,
                mip_level: 0,
                origin: wgpu::Origin3d {
                    x: r[0],
                    y: r[1],
                    z: 0,
                },
                aspect: wgpu::TextureAspect::All,
            },
            &rgba[offset..],
            wgpu::TexelCopyBufferLayout {
                offset: 0,
                bytes_per_row: Some(stride_w * 4),
                rows_per_image: None,
            },
            wgpu::Extent3d {
                width: r[2],
                height: r[3],
                depth_or_array_layers: 1,
            },
        );
    }

    /// An RGBA picture (straight alpha, sRGB) to draw with; returns its texture index.
    pub fn add_image(
        &mut self,
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        w: u32,
        h: u32,
        rgba: &[u8],
    ) -> usize {
        self.add_texture_internal(device, Some(queue), w, h, rgba)
    }

    /// Replace the pixels of a picture added by `add_image` (same size; else it is made
    /// anew under the same index).
    pub fn update_image(
        &mut self,
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        id: usize,
        w: u32,
        h: u32,
        rgba: &[u8],
    ) {
        let same = matches!(self.textures.get(id), Some(Some(t)) if t.size == (w, h) && t.texture.is_some());
        if same {
            let t = self.textures[id]
                .as_ref()
                .unwrap()
                .texture
                .as_ref()
                .unwrap();
            Self::write(queue, t, w, [0, 0, w, h], rgba);
            return;
        }
        let k = self.add_texture_internal(device, Some(queue), w, h, rgba);
        let t = self.textures.pop().flatten();
        debug_assert_eq!(k, self.textures.len());
        if id < self.textures.len() {
            self.textures[id] = t;
        } else {
            self.textures.push(t);
        }
    }

    /// A texture drawn elsewhere (a render target) to draw with; returns its index.
    pub fn add_view(
        &mut self,
        device: &wgpu::Device,
        view: &wgpu::TextureView,
        size: (u32, u32),
    ) -> usize {
        let group = self.group(device, view);
        self.textures.push(Some(Tex {
            texture: None,
            group,
            size,
        }));
        self.textures.len() - 1
    }

    /// Point `id` at another view (a render target made anew at another size).
    pub fn set_view(
        &mut self,
        device: &wgpu::Device,
        id: usize,
        view: &wgpu::TextureView,
        size: (u32, u32),
    ) {
        let group = self.group(device, view);
        if let Some(slot) = self.textures.get_mut(id) {
            *slot = Some(Tex {
                texture: None,
                group,
                size,
            });
        }
    }

    pub fn free(&mut self, id: usize) {
        if id > 0 {
            if let Some(t) = self.textures.get_mut(id) {
                *t = None;
            }
        }
    }

    /// Send the atlas's changed region to the GPU.
    pub fn upload_atlas(&mut self, queue: &wgpu::Queue, atlas: &mut Atlas) {
        if atlas.size != self.atlas_size {
            // the atlas grew (see `Atlas::begin_frame`): its texture is made anew at its size
            let device = self.device.clone();
            let size = atlas.size;
            let texture = device.create_texture(&wgpu::TextureDescriptor {
                label: Some("user-interface atlas"),
                size: wgpu::Extent3d {
                    width: size,
                    height: size,
                    depth_or_array_layers: 1,
                },
                mip_level_count: 1,
                sample_count: 1,
                dimension: wgpu::TextureDimension::D2,
                format: wgpu::TextureFormat::Rgba8UnormSrgb,
                usage: wgpu::TextureUsages::TEXTURE_BINDING | wgpu::TextureUsages::COPY_DST,
                view_formats: &[],
            });
            let view = texture.create_view(&Default::default());
            let group = self.group(&device, &view);
            if let Some(slot) = self.textures.first_mut() {
                *slot = Some(Tex {
                    texture: Some(texture),
                    group,
                    size: (size, size),
                });
            }
            self.atlas_size = size;
            atlas.mark_all_dirty();
        }
        if let Some(r) = atlas.take_dirty() {
            if let Some(Some(Tex {
                texture: Some(t), ..
            })) = self.textures.first()
            {
                Self::write(queue, t, atlas.size, r, &atlas.rgba);
            }
        }
    }

    /// Put `verts` into vertex buffer `id` (grown as needed).
    pub fn upload(
        &mut self,
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        id: usize,
        verts: &[Vertex],
    ) {
        let bytes = size_of_val(verts) as u64;
        while self.buffers.len() <= id {
            self.buffers.push((None, 0));
        }
        if bytes > self.buffers[id].1 || self.buffers[id].0.is_none() {
            let cap = bytes.next_power_of_two().max(4096);
            self.buffers[id] = (Self::vertex_buffer(device, cap), cap);
        }
        if let (Some(buf), true) = (&self.buffers[id].0, bytes > 0) {
            queue.write_buffer(buf, 0, bytemuck::cast_slice(verts));
        }
    }

    /// A vertex buffer of `size` bytes, made where its failure can be seen: a card out of
    /// memory hands out an invalid buffer, and writing to that every frame flooded the log
    /// with thousands of errors and took the frame rate down with it (#217). A failed one
    /// is None: nothing draws from it, and the next upload tries again.
    fn vertex_buffer(device: &wgpu::Device, size: u64) -> Option<wgpu::Buffer> {
        let oom = device.push_error_scope(wgpu::ErrorFilter::OutOfMemory);
        let valid = device.push_error_scope(wgpu::ErrorFilter::Validation);
        let buf = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("user-interface vertices"),
            size,
            usage: wgpu::BufferUsages::VERTEX | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });
        let invalid = pollster::block_on(valid.pop());
        let out_of_memory = pollster::block_on(oom.pop());
        match invalid.or(out_of_memory) {
            None => Some(buf),
            Some(e) => {
                log::warn!("user-interface: a vertex buffer of {size} bytes could not be made: {e}");
                None
            }
        }
    }

    /// Draw into `target` (`size` pixels): cleared to `clear` first when given.
    #[allow(clippy::too_many_arguments)]
    pub fn render(
        &mut self,
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        encoder: &mut wgpu::CommandEncoder,
        target: &wgpu::TextureView,
        size: (u32, u32),
        clear: Option<wgpu::Color>,
        layers: &[Layer],
        draws: &[Draw],
    ) {
        let (w, h) = (size.0.max(1), size.1.max(1));
        // the layers' uniforms
        let mut data = vec![0u8; (SLOT * MAX_LAYERS) as usize];
        for (k, l) in layers.iter().take(MAX_LAYERS as usize).enumerate() {
            let vp = if l.viewport[2] > 0.0 {
                l.viewport
            } else {
                [0.0, 0.0, w as f32, h as f32]
            };
            let u = Uniform {
                view_proj: l.view_proj.to_cols_array_2d(),
                viewport: vp,
                target: [w as f32, h as f32, l.route[0], l.route[1]],
                clip: l.clip,
                params: [l.radius, l.opacity, l.px_scale, l.route[2]],
            };
            let at = k * SLOT as usize;
            data[at..at + size_of::<Uniform>()].copy_from_slice(bytemuck::bytes_of(&u));
        }
        queue.write_buffer(
            &self.uniform,
            0,
            &data[..(SLOT as usize * layers.len().clamp(1, MAX_LAYERS as usize))],
        );
        if self.samples > 1
            && self
                .msaa
                .as_ref()
                .map(|m| (m.1, m.2) != (w, h))
                .unwrap_or(true)
        {
            let t = device.create_texture(&wgpu::TextureDescriptor {
                label: Some("user-interface msaa"),
                size: wgpu::Extent3d {
                    width: w,
                    height: h,
                    depth_or_array_layers: 1,
                },
                mip_level_count: 1,
                sample_count: self.samples,
                dimension: wgpu::TextureDimension::D2,
                format: self.format,
                usage: wgpu::TextureUsages::RENDER_ATTACHMENT,
                view_formats: &[],
            });
            self.msaa = Some((t.create_view(&Default::default()), w, h));
        }
        let (view, resolve) = match (&self.msaa, self.samples > 1) {
            (Some(m), true) => (&m.0, Some(target)),
            _ => (target, None),
        };
        let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
            label: Some("user-interface"),
            color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                view,
                depth_slice: None,
                resolve_target: resolve,
                ops: wgpu::Operations {
                    load: match (clear, resolve.is_some()) {
                        (Some(c), _) => wgpu::LoadOp::Clear(c),
                        (None, true) => wgpu::LoadOp::Clear(wgpu::Color::TRANSPARENT),
                        (None, false) => wgpu::LoadOp::Load,
                    },
                    store: if resolve.is_some() {
                        wgpu::StoreOp::Discard
                    } else {
                        wgpu::StoreOp::Store
                    },
                },
            })],
            depth_stencil_attachment: None,
            timestamp_writes: None,
            occlusion_query_set: None,
            multiview_mask: None,
        });
        pass.set_pipeline(&self.pipeline);
        for d in draws {
            let (Some((Some(buf), _)), Some(l)) = (self.buffers.get(d.buffer), layers.get(d.layer))
            else {
                continue;
            };
            let Some(Some(tex)) = self.textures.get(d.texture).or(self.textures.first()) else {
                continue;
            };
            if d.range.is_empty() || d.layer as u64 >= MAX_LAYERS {
                continue;
            }
            pass.set_bind_group(0, &self.uniform_group, &[(d.layer as u64 * SLOT) as u32]);
            pass.set_bind_group(1, &tex.group, &[]);
            // A flat layer's `viewport` is the size its pixels are laid out in (an interface in
            // logical pixels): drawn over the whole target, scaled to it. As the rasteriser's
            // viewport it squeezed an interface zoomed by 1.1 into the top-left 90 % of the
            // window while the clips stayed full size (cut-off text, a black band below).
            let flat = l.px_scale == 0.0 && l.view_proj == Mat4::IDENTITY;
            let vp = if l.viewport[2] > 0.0 && !flat {
                l.viewport
            } else {
                [0.0, 0.0, w as f32, h as f32]
            };
            let vx = vp[0].clamp(0.0, w as f32 - 1.0);
            let vy = vp[1].clamp(0.0, h as f32 - 1.0);
            pass.set_viewport(
                vx,
                vy,
                vp[2].min(w as f32 - vx).max(1.0),
                vp[3].min(h as f32 - vy).max(1.0),
                0.0,
                1.0,
            );
            let x0 = l.clip[0].floor().clamp(0.0, w as f32) as u32;
            let y0 = l.clip[1].floor().clamp(0.0, h as f32) as u32;
            let x1 = l.clip[2].ceil().clamp(0.0, w as f32) as u32;
            let y1 = l.clip[3].ceil().clamp(0.0, h as f32) as u32;
            if x1 <= x0 || y1 <= y0 {
                continue;
            }
            pass.set_scissor_rect(x0, y0, x1 - x0, y1 - y0);
            pass.set_vertex_buffer(0, buf.slice(..));
            pass.draw(d.range.clone(), 0..1);
        }
    }
}

#[cfg(test)]
mod tests {
    #[test]
    fn shader_validates() {
        let m = naga::front::wgsl::parse_str(include_str!("ui.wgsl")).expect("parse");
        naga::valid::Validator::new(
            naga::valid::ValidationFlags::all(),
            naga::valid::Capabilities::all(),
        )
        .validate(&m)
        .expect("valid");
    }
}
