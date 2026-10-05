#![allow(unused_imports)]
use super::DevTools;
use imgui::{DrawCmd, DrawCmdParams, TextureId};
use omsi_render::Renderer;

const SHADER: &str = r#"
struct U { a: vec4<f32>, b: vec4<f32> };
@group(0) @binding(0) var<uniform> u: U;
@group(0) @binding(1) var t: texture_2d<f32>;
@group(0) @binding(2) var s: sampler;
struct VO {
    @builtin(position) pos: vec4<f32>,
    @location(0) uv: vec2<f32>,
    @location(1) col: vec4<f32>,
};
@vertex
fn vs_main(@location(0) pos: vec2<f32>, @location(1) uv: vec2<f32>, @location(2) col: vec4<f32>) -> VO {
    var o: VO;
    o.pos = vec4<f32>(pos * u.a.xy + u.a.zw, 0.0, 1.0);
    o.uv = uv;
    var c = col;
    if (u.b.x > 0.5) {
        c = vec4<f32>(pow(c.rgb, vec3<f32>(2.2)), c.a);
    }
    o.col = c;
    return o;
}
@fragment
fn fs_main(i: VO) -> @location(0) vec4<f32> {
    return i.col * textureSample(t, s, i.uv);
}
"#;

#[repr(C)]
#[derive(Clone, Copy, bytemuck::Pod, bytemuck::Zeroable)]
struct Vert {
    pos: [f32; 2],
    uv: [f32; 2],
    col: [u8; 4],
}

#[repr(C)]
#[derive(Clone, Copy, bytemuck::Pod, bytemuck::Zeroable)]
struct Uniform {
    a: [f32; 4],
    b: [f32; 4],
}

pub(super) struct Gpu {
    format: wgpu::TextureFormat,
    pipeline: wgpu::RenderPipeline,
    uniform: wgpu::Buffer,
    group: wgpu::BindGroup,
    vbuf: wgpu::Buffer,
    vcap: u64,
    ibuf: wgpu::Buffer,
    icap: u64,
}

impl DevTools {
    pub(super) fn ensure_gpu(&mut self, r: &Renderer) {
        let format = r.format();
        if self.gpu.as_ref().is_some_and(|g| g.format == format) {
            return;
        }
        let device = &r.device;
        let queue = &r.queue;
        if self.font_texture.is_none() {
            let (fw, fh) = self.font_size;
            let tex = device.create_texture(&wgpu::TextureDescriptor {
                label: Some("devtools font"),
                size: wgpu::Extent3d {
                    width: fw,
                    height: fh,
                    depth_or_array_layers: 1,
                },
                mip_level_count: 1,
                sample_count: 1,
                dimension: wgpu::TextureDimension::D2,
                format: wgpu::TextureFormat::Rgba8Unorm,
                usage: wgpu::TextureUsages::TEXTURE_BINDING | wgpu::TextureUsages::COPY_DST,
                view_formats: &[],
            });
            queue.write_texture(
                wgpu::TexelCopyTextureInfo {
                    texture: &tex,
                    mip_level: 0,
                    origin: wgpu::Origin3d::ZERO,
                    aspect: wgpu::TextureAspect::All,
                },
                &self.font_rgba,
                wgpu::TexelCopyBufferLayout {
                    offset: 0,
                    bytes_per_row: Some(fw * 4),
                    rows_per_image: None,
                },
                wgpu::Extent3d {
                    width: fw,
                    height: fh,
                    depth_or_array_layers: 1,
                },
            );
            self.font_texture = Some(tex.create_view(&Default::default()));
        }
        let font_view = self.font_texture.as_ref().unwrap();
        let shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("devtools"),
            source: wgpu::ShaderSource::Wgsl(SHADER.into()),
        });
        let layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("devtools"),
            entries: &[
                wgpu::BindGroupLayoutEntry {
                    binding: 0,
                    visibility: wgpu::ShaderStages::VERTEX_FRAGMENT,
                    ty: wgpu::BindingType::Buffer {
                        ty: wgpu::BufferBindingType::Uniform,
                        has_dynamic_offset: false,
                        min_binding_size: wgpu::BufferSize::new(size_of::<Uniform>() as u64),
                    },
                    count: None,
                },
                wgpu::BindGroupLayoutEntry {
                    binding: 1,
                    visibility: wgpu::ShaderStages::FRAGMENT,
                    ty: wgpu::BindingType::Texture {
                        sample_type: wgpu::TextureSampleType::Float { filterable: true },
                        view_dimension: wgpu::TextureViewDimension::D2,
                        multisampled: false,
                    },
                    count: None,
                },
                wgpu::BindGroupLayoutEntry {
                    binding: 2,
                    visibility: wgpu::ShaderStages::FRAGMENT,
                    ty: wgpu::BindingType::Sampler(wgpu::SamplerBindingType::Filtering),
                    count: None,
                },
            ],
        });
        let pl = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("devtools"),
            bind_group_layouts: &[Some(&layout)],
            immediate_size: 0,
        });
        let attrs = wgpu::vertex_attr_array![0 => Float32x2, 1 => Float32x2, 2 => Unorm8x4];
        let blend = wgpu::BlendState {
            color: wgpu::BlendComponent {
                src_factor: wgpu::BlendFactor::SrcAlpha,
                dst_factor: wgpu::BlendFactor::OneMinusSrcAlpha,
                operation: wgpu::BlendOperation::Add,
            },
            alpha: wgpu::BlendComponent {
                src_factor: wgpu::BlendFactor::One,
                dst_factor: wgpu::BlendFactor::OneMinusSrcAlpha,
                operation: wgpu::BlendOperation::Add,
            },
        };
        let pipeline = device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
            label: Some("devtools"),
            layout: Some(&pl),
            vertex: wgpu::VertexState {
                module: &shader,
                entry_point: Some("vs_main"),
                buffers: &[Some(wgpu::VertexBufferLayout {
                    array_stride: size_of::<Vert>() as u64,
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
            multisample: wgpu::MultisampleState::default(),
            fragment: Some(wgpu::FragmentState {
                module: &shader,
                entry_point: Some("fs_main"),
                targets: &[Some(wgpu::ColorTargetState {
                    format,
                    blend: Some(blend),
                    write_mask: wgpu::ColorWrites::ALL,
                })],
                compilation_options: Default::default(),
            }),
            multiview_mask: None,
            cache: None,
        });
        let uniform = buffer(
            device,
            wgpu::BufferUsages::UNIFORM,
            size_of::<Uniform>() as u64,
        );
        let sampler = device.create_sampler(&wgpu::SamplerDescriptor {
            label: Some("devtools"),
            address_mode_u: wgpu::AddressMode::ClampToEdge,
            address_mode_v: wgpu::AddressMode::ClampToEdge,
            mag_filter: wgpu::FilterMode::Linear,
            min_filter: wgpu::FilterMode::Linear,
            ..Default::default()
        });
        let group = device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("devtools"),
            layout: &layout,
            entries: &[
                wgpu::BindGroupEntry {
                    binding: 0,
                    resource: uniform.as_entire_binding(),
                },
                wgpu::BindGroupEntry {
                    binding: 1,
                    resource: wgpu::BindingResource::TextureView(font_view),
                },
                wgpu::BindGroupEntry {
                    binding: 2,
                    resource: wgpu::BindingResource::Sampler(&sampler),
                },
            ],
        });
        self.gpu = Some(Gpu {
            format,
            pipeline,
            uniform,
            group,
            vbuf: buffer(device, wgpu::BufferUsages::VERTEX, 65536),
            vcap: 65536,
            ibuf: buffer(device, wgpu::BufferUsages::INDEX, 65536),
            icap: 65536,
        });
    }

    pub(super) fn submit(&mut self, r: &Renderer, view: &wgpu::TextureView, w: u32, h: u32) {
        let draw_data = self.ctx.render();
        let Some(gpu) = self.gpu.as_mut() else {
            return;
        };
        let device = &r.device;
        let queue = &r.queue;
        let mut verts: Vec<Vert> = Vec::with_capacity(draw_data.total_vtx_count as usize);
        let mut idx: Vec<u16> = Vec::with_capacity(draw_data.total_idx_count as usize);
        let mut lists: Vec<(u32, u32)> = Vec::new();
        for list in draw_data.draw_lists() {
            lists.push((verts.len() as u32, idx.len() as u32));
            verts.extend(list.vtx_buffer().iter().map(|v| Vert {
                pos: v.pos,
                uv: v.uv,
                col: v.col,
            }));
            idx.extend_from_slice(list.idx_buffer());
        }
        if verts.is_empty() || idx.is_empty() {
            return;
        }
        if idx.len() % 2 == 1 {
            idx.push(0);
        }
        let vbytes = (verts.len() * size_of::<Vert>()) as u64;
        let ibytes = (idx.len() * 2) as u64;
        if vbytes > gpu.vcap {
            gpu.vcap = (vbytes * 2).next_multiple_of(4);
            gpu.vbuf = buffer(device, wgpu::BufferUsages::VERTEX, gpu.vcap);
        }
        if ibytes > gpu.icap {
            gpu.icap = (ibytes * 2).next_multiple_of(4);
            gpu.ibuf = buffer(device, wgpu::BufferUsages::INDEX, gpu.icap);
        }
        queue.write_buffer(&gpu.vbuf, 0, bytemuck::cast_slice(&verts));
        queue.write_buffer(&gpu.ibuf, 0, bytemuck::cast_slice(&idx));
        let u = Uniform {
            a: [2.0 / w as f32, -2.0 / h as f32, -1.0, 1.0],
            b: [if gpu.format.is_srgb() { 1.0 } else { 0.0 }, 0.0, 0.0, 0.0],
        };
        queue.write_buffer(&gpu.uniform, 0, bytemuck::bytes_of(&u));
        let mut encoder = device.create_command_encoder(&wgpu::CommandEncoderDescriptor {
            label: Some("devtools"),
        });
        {
            let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                label: Some("devtools"),
                color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                    view,
                    depth_slice: None,
                    resolve_target: None,
                    ops: wgpu::Operations {
                        load: wgpu::LoadOp::Load,
                        store: wgpu::StoreOp::Store,
                    },
                })],
                depth_stencil_attachment: None,
                timestamp_writes: None,
                occlusion_query_set: None,
                multiview_mask: None,
            });
            pass.set_pipeline(&gpu.pipeline);
            pass.set_bind_group(0, &gpu.group, &[]);
            pass.set_vertex_buffer(0, gpu.vbuf.slice(..));
            pass.set_index_buffer(gpu.ibuf.slice(..), wgpu::IndexFormat::Uint16);
            for (list, &(vbase, ibase)) in draw_data.draw_lists().zip(&lists) {
                for cmd in list.commands() {
                    if let DrawCmd::Elements {
                        count,
                        cmd_params:
                            DrawCmdParams {
                                clip_rect,
                                vtx_offset,
                                idx_offset,
                                ..
                            },
                    } = cmd
                    {
                        let x0 = clip_rect[0].clamp(0.0, w as f32) as u32;
                        let y0 = clip_rect[1].clamp(0.0, h as f32) as u32;
                        let x1 = clip_rect[2].clamp(0.0, w as f32) as u32;
                        let y1 = clip_rect[3].clamp(0.0, h as f32) as u32;
                        if x1 <= x0 || y1 <= y0 {
                            continue;
                        }
                        pass.set_scissor_rect(x0, y0, x1 - x0, y1 - y0);
                        let first = ibase + idx_offset as u32;
                        pass.draw_indexed(
                            first..first + count as u32,
                            (vbase + vtx_offset as u32) as i32,
                            0..1,
                        );
                    }
                }
            }
        }
        queue.submit(Some(encoder.finish()));
    }
}

fn buffer(device: &wgpu::Device, usage: wgpu::BufferUsages, size: u64) -> wgpu::Buffer {
    device.create_buffer(&wgpu::BufferDescriptor {
        label: Some("devtools"),
        size,
        usage: usage | wgpu::BufferUsages::COPY_DST,
        mapped_at_creation: false,
    })
}
