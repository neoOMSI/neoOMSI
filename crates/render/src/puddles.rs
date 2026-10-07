//! Bounded screen-space puddles, with planar vehicle and unobstructed world captures.
//! Reflection rays and each capture use at most 518400 pixels.

use super::*;

const MAX_TRACE_PIXELS: f32 = 960.0 * 540.0;

#[repr(C)]
#[derive(Clone, Copy, bytemuck::Pod, bytemuck::Zeroable)]
struct VehicleBox {
    a: [f32; 4],
    b: [f32; 4],
    c: [f32; 4],
}

#[repr(C)]
#[derive(Clone, Copy, bytemuck::Pod, bytemuck::Zeroable)]
pub(crate) struct VehicleUniform {
    plane: [f32; 4],
    parts: [VehicleBox; 4],
}

#[repr(C)]
#[derive(Clone, Copy, bytemuck::Pod, bytemuck::Zeroable)]
pub(crate) struct Uniform {
    view_proj: [[f32; 4]; 4],
    inv_view_proj: [[f32; 4]; 4],
    eye_time: [f32; 4],
    origin_rain: [f32; 4],
    sky: [f32; 4],
    projection_trace: [f32; 4],
    vehicle_plane: [f32; 4],
    vehicle_info: [f32; 4],
    vehicle_parts: [VehicleBox; 4],
}

pub(crate) fn shader_source() -> String {
    [
        include_str!("../shaders/puddles/puddle_common.wgsl"),
        include_str!("../shaders/puddles/puddle_reflection.wgsl"),
    ]
    .join("\n")
}

pub(crate) struct Pipelines {
    layout: wgpu::BindGroupLayout,
    params: wgpu::Buffer,
    trace: wgpu::RenderPipeline,
    blur_x: wgpu::RenderPipeline,
    blur_y: wgpu::RenderPipeline,
    resolve: wgpu::RenderPipeline,
    glass: [wgpu::RenderPipeline; 2],
    world: Vec<wgpu::RenderPipeline>,
    vehicle: Vec<wgpu::RenderPipeline>,
    chassis: wgpu::RenderPipeline,
    vehicle_params: wgpu::Buffer,
    vehicle_bg: wgpu::BindGroup,
}

impl Pipelines {
    pub(crate) fn new(
        device: &wgpu::Device,
        scene_shader: &wgpu::ShaderModule,
        camera_layout: &wgpu::BindGroupLayout,
        material_layout: &wgpu::BindGroupLayout,
    ) -> Self {
        let texture = |binding, sample_type, view_dimension| wgpu::BindGroupLayoutEntry {
            binding,
            visibility: wgpu::ShaderStages::FRAGMENT,
            ty: wgpu::BindingType::Texture {
                sample_type,
                view_dimension,
                multisampled: false,
            },
            count: None,
        };
        let float = wgpu::TextureSampleType::Float { filterable: true };
        let layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("puddle reflections"),
            entries: &[
                wgpu::BindGroupLayoutEntry {
                    binding: 0,
                    visibility: wgpu::ShaderStages::FRAGMENT,
                    ty: wgpu::BindingType::Buffer {
                        ty: wgpu::BufferBindingType::Uniform,
                        has_dynamic_offset: false,
                        min_binding_size: wgpu::BufferSize::new(size_of::<Uniform>() as u64),
                    },
                    count: None,
                },
                texture(1, float, wgpu::TextureViewDimension::D2),
                texture(2, float, wgpu::TextureViewDimension::D2),
                texture(
                    3,
                    wgpu::TextureSampleType::Depth,
                    wgpu::TextureViewDimension::D2,
                ),
                texture(4, float, wgpu::TextureViewDimension::D2),
                wgpu::BindGroupLayoutEntry {
                    binding: 5,
                    visibility: wgpu::ShaderStages::FRAGMENT,
                    ty: wgpu::BindingType::Sampler(wgpu::SamplerBindingType::Filtering),
                    count: None,
                },
                texture(6, float, wgpu::TextureViewDimension::Cube),
                texture(
                    7,
                    wgpu::TextureSampleType::Depth,
                    wgpu::TextureViewDimension::D2,
                ),
                texture(8, float, wgpu::TextureViewDimension::D2),
                texture(9, float, wgpu::TextureViewDimension::D2),
            ],
        });
        let params = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("puddle reflection params"),
            size: size_of::<Uniform>() as u64,
            usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });
        let shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("puddle reflections"),
            source: wgpu::ShaderSource::Wgsl(shader_source().into()),
        });
        let pipeline_layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("puddle reflections"),
            bind_group_layouts: &[Some(&layout)],
            immediate_size: 0,
        });
        let pipeline = |entry| {
            device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
                label: Some(entry),
                layout: Some(&pipeline_layout),
                vertex: wgpu::VertexState {
                    module: &shader,
                    entry_point: Some("vs_main"),
                    buffers: &[],
                    compilation_options: Default::default(),
                },
                primitive: wgpu::PrimitiveState {
                    cull_mode: None,
                    ..Default::default()
                },
                depth_stencil: None,
                multisample: Default::default(),
                fragment: Some(wgpu::FragmentState {
                    module: &shader,
                    entry_point: Some(entry),
                    targets: &[Some(wgpu::ColorTargetState {
                        format: HDR_FORMAT,
                        blend: None,
                        write_mask: wgpu::ColorWrites::ALL,
                    })],
                    compilation_options: Default::default(),
                }),
                multiview_mask: None,
                cache: None,
            })
        };
        let glass_layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("puddle glass depth"),
            bind_group_layouts: &[Some(camera_layout), Some(material_layout)],
            immediate_size: 0,
        });
        let glass = [false, true].map(|cull| {
            device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
                label: Some("puddle glass depth"),
                layout: Some(&glass_layout),
                vertex: wgpu::VertexState {
                    module: scene_shader,
                    entry_point: Some("vs_main"),
                    buffers: &[Some(wgpu::VertexBufferLayout {
                        array_stride: size_of::<Vertex>() as u64,
                        step_mode: wgpu::VertexStepMode::Vertex,
                        attributes: &wgpu::vertex_attr_array![0 => Float32x3, 1 => Float32x3, 2 => Float32x2],
                    })],
                    compilation_options: Default::default(),
                },
                primitive: one_sided_primitive(cull),
                depth_stencil: Some(wgpu::DepthStencilState {
                    format: DEPTH_FORMAT,
                    depth_write_enabled: Some(true),
                    depth_compare: Some(wgpu::CompareFunction::GreaterEqual),
                    stencil: Default::default(),
                    bias: Default::default(),
                }),
                multisample: Default::default(),
                fragment: Some(wgpu::FragmentState {
                    module: scene_shader,
                    entry_point: Some("fs_puddle_glass_depth"),
                    targets: &[],
                    compilation_options: Default::default(),
                }),
                multiview_mask: None,
                cache: None,
            })
        });
        let vehicle_layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("puddle vehicle plane"),
            entries: &[wgpu::BindGroupLayoutEntry {
                binding: 0,
                visibility: wgpu::ShaderStages::VERTEX_FRAGMENT,
                ty: wgpu::BindingType::Buffer {
                    ty: wgpu::BufferBindingType::Uniform,
                    has_dynamic_offset: false,
                    min_binding_size: wgpu::BufferSize::new(size_of::<VehicleUniform>() as u64),
                },
                count: None,
            }],
        });
        let vehicle_params = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("puddle vehicle plane"),
            size: size_of::<VehicleUniform>() as u64,
            usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });
        let vehicle_bg = device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("puddle vehicle plane"),
            layout: &vehicle_layout,
            entries: &[wgpu::BindGroupEntry {
                binding: 0,
                resource: vehicle_params.as_entire_binding(),
            }],
        });
        let capture_layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("puddle vehicle capture"),
            bind_group_layouts: &[
                Some(camera_layout),
                Some(material_layout),
                Some(&vehicle_layout),
            ],
            immediate_size: 0,
        });
        let mut vehicle = Vec::new();
        let mut world = Vec::new();
        let surface_bias = ::legacy_config::env::var("OMSI_SURFACE_BIAS")
            .ok()
            .and_then(|v| v.parse::<i32>().ok())
            .unwrap_or(-24);
        for kind in 0..PIPE_KINDS {
            for cull in [false, true] {
                for surface in [false, true] {
                    let bias = if surface { surface_bias } else { 0 };
                    world.push(device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
                        label: Some("puddle world without cabin"),
                        layout: Some(&glass_layout),
                        vertex: wgpu::VertexState {
                            module: scene_shader,
                            entry_point: Some("vs_main"),
                            buffers: &[Some(wgpu::VertexBufferLayout {
                                array_stride: size_of::<Vertex>() as u64,
                                step_mode: wgpu::VertexStepMode::Vertex,
                                attributes: &wgpu::vertex_attr_array![0 => Float32x3, 1 => Float32x3, 2 => Float32x2],
                            })],
                            compilation_options: Default::default(),
                        },
                        primitive: one_sided_primitive(cull),
                        depth_stencil: Some(wgpu::DepthStencilState {
                            format: DEPTH_FORMAT,
                            depth_write_enabled: Some(kind != PIPE_BLEND_NO_WRITE),
                            depth_compare: Some(wgpu::CompareFunction::GreaterEqual),
                            stencil: Default::default(),
                            bias: wgpu::DepthBiasState {
                                constant: -bias,
                                slope_scale: if bias != 0 { -bias.signum() as f32 * 2.0 } else { 0.0 },
                                clamp: 0.0,
                            },
                        }),
                        multisample: Default::default(),
                        fragment: Some(wgpu::FragmentState {
                            module: scene_shader,
                            entry_point: Some(if kind == PIPE_SURFACE_DEPTH { "fs_surface_depth" } else { "fs_puddle_world" }),
                            targets: &[Some(wgpu::ColorTargetState {
                                format: HDR_FORMAT,
                                blend: (kind == PIPE_BLEND || kind == PIPE_BLEND_NO_WRITE).then_some(wgpu::BlendState::ALPHA_BLENDING),
                                write_mask: if kind == PIPE_SURFACE_DEPTH { wgpu::ColorWrites::empty() } else { wgpu::ColorWrites::ALL },
                            })],
                            compilation_options: wgpu::PipelineCompilationOptions {
                                constants: &[("ALPHA_TEST", if kind == PIPE_ALPHA_TEST { 1.0 } else { 0.0 })],
                                ..Default::default()
                            },
                        }),
                        multiview_mask: None,
                        cache: None,
                    }));
                    let mut primitive = one_sided_primitive(cull);
                    // Mirroring reverses winding. Keep authored one-sided geometry.
                    primitive.front_face = wgpu::FrontFace::Ccw;
                    vehicle.push(device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
                        label: Some("puddle vehicle capture"),
                        layout: Some(&capture_layout),
                        vertex: wgpu::VertexState {
                            module: scene_shader,
                            entry_point: Some("vs_puddle_vehicle"),
                            buffers: &[Some(wgpu::VertexBufferLayout {
                                array_stride: size_of::<Vertex>() as u64,
                                step_mode: wgpu::VertexStepMode::Vertex,
                                attributes: &wgpu::vertex_attr_array![0 => Float32x3, 1 => Float32x3, 2 => Float32x2],
                            })],
                            compilation_options: Default::default(),
                        },
                        primitive,
                        depth_stencil: Some(wgpu::DepthStencilState {
                            format: DEPTH_FORMAT,
                            depth_write_enabled: Some(kind != PIPE_BLEND_NO_WRITE),
                            depth_compare: Some(wgpu::CompareFunction::GreaterEqual),
                            stencil: Default::default(),
                            bias: Default::default(),
                        }),
                        multisample: Default::default(),
                        fragment: Some(wgpu::FragmentState {
                            module: scene_shader,
                            entry_point: Some("fs_puddle_vehicle"),
                            targets: &[Some(wgpu::ColorTargetState {
                                format: HDR_FORMAT,
                                blend: (kind == PIPE_BLEND || kind == PIPE_BLEND_NO_WRITE).then_some(wgpu::BlendState::ALPHA_BLENDING),
                                write_mask: wgpu::ColorWrites::ALL,
                            })],
                            compilation_options: Default::default(),
                        }),
                        multiview_mask: None,
                        cache: None,
                    }));
                }
            }
        }
        let chassis = device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
            label: Some("puddle closed chassis"),
            layout: Some(&capture_layout),
            vertex: wgpu::VertexState {
                module: scene_shader,
                entry_point: Some("vs_puddle_chassis"),
                buffers: &[],
                compilation_options: Default::default(),
            },
            primitive: wgpu::PrimitiveState {
                cull_mode: None,
                ..Default::default()
            },
            depth_stencil: Some(wgpu::DepthStencilState {
                format: DEPTH_FORMAT,
                depth_write_enabled: Some(true),
                depth_compare: Some(wgpu::CompareFunction::GreaterEqual),
                stencil: Default::default(),
                bias: Default::default(),
            }),
            multisample: Default::default(),
            fragment: Some(wgpu::FragmentState {
                module: scene_shader,
                entry_point: Some("fs_puddle_chassis"),
                targets: &[Some(wgpu::ColorTargetState {
                    format: HDR_FORMAT,
                    blend: None,
                    write_mask: wgpu::ColorWrites::ALL,
                })],
                compilation_options: Default::default(),
            }),
            multiview_mask: None,
            cache: None,
        });
        Self {
            trace: pipeline("fs_trace"),
            blur_x: pipeline("fs_blur_x"),
            blur_y: pipeline("fs_blur_y"),
            resolve: pipeline("fs_resolve"),
            glass,
            world,
            vehicle,
            chassis,
            vehicle_params,
            vehicle_bg,
            layout,
            params,
        }
    }
}

pub(crate) struct Targets {
    size: (u32, u32),
    source_depth: wgpu::Texture,
    world_view: Option<wgpu::TextureView>,
    hit_depth: wgpu::Texture,
    hit_depth_view: wgpu::TextureView,
    vehicle_view: wgpu::TextureView,
    vehicle_depth: wgpu::TextureView,
    trace: wgpu::TextureView,
    blur: wgpu::TextureView,
    filtered: wgpu::TextureView,
    pub(crate) view: wgpu::TextureView,
    trace_bg: wgpu::BindGroup,
    blur_x_bg: wgpu::BindGroup,
    blur_y_bg: wgpu::BindGroup,
    resolve_bg: wgpu::BindGroup,
    pub(crate) down_bg: wgpu::BindGroup,
    pub(crate) tonemap_bg: [wgpu::BindGroup; 2],
    pub(crate) classic_bg: wgpu::BindGroup,
}

fn trace_size(w: u32, h: u32) -> (u32, u32) {
    let scale = 0.5_f32.min((MAX_TRACE_PIXELS / (w.max(1) as f32 * h.max(1) as f32)).sqrt());
    (
        ((w as f32 * scale).floor() as u32).max(1),
        ((h as f32 * scale).floor() as u32).max(1),
    )
}

impl Targets {
    fn new(r: &Renderer, hdr: &HdrTargets, w: u32, h: u32, cabin: bool) -> Self {
        let pipelines = r.puddles.as_ref().unwrap();
        let size = trace_size(w, h);
        let source_depth = r.ao.as_ref().unwrap().depth_view.texture().clone();
        let hit_size = if cabin { size } else { (w, h) };
        let hit_depth = r.device.create_texture(&wgpu::TextureDescriptor {
            label: Some("puddle hit depth including glass"),
            size: wgpu::Extent3d {
                width: hit_size.0,
                height: hit_size.1,
                depth_or_array_layers: 1,
            },
            mip_level_count: 1,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format: DEPTH_FORMAT,
            usage: wgpu::TextureUsages::COPY_DST
                | wgpu::TextureUsages::COPY_SRC
                | wgpu::TextureUsages::RENDER_ATTACHMENT
                | wgpu::TextureUsages::TEXTURE_BINDING,
            view_formats: &[],
        });
        let hit_depth_view = hit_depth.create_view(&Default::default());
        let target = |label, (width, height)| {
            r.device
                .create_texture(&wgpu::TextureDescriptor {
                    label: Some(label),
                    size: wgpu::Extent3d {
                        width,
                        height,
                        depth_or_array_layers: 1,
                    },
                    mip_level_count: 1,
                    sample_count: 1,
                    dimension: wgpu::TextureDimension::D2,
                    format: HDR_FORMAT,
                    usage: wgpu::TextureUsages::RENDER_ATTACHMENT
                        | wgpu::TextureUsages::TEXTURE_BINDING
                        | wgpu::TextureUsages::COPY_SRC,
                    view_formats: &[],
                })
                .create_view(&Default::default())
        };
        let trace = target("puddle reflection rays", size);
        let world_view = cabin.then(|| target("puddle world without cabin", size));
        let vehicle_view = target("puddle player vehicle", size);
        let vehicle_depth = r
            .device
            .create_texture(&wgpu::TextureDescriptor {
                label: Some("puddle player vehicle depth"),
                size: wgpu::Extent3d {
                    width: size.0,
                    height: size.1,
                    depth_or_array_layers: 1,
                },
                mip_level_count: 1,
                sample_count: 1,
                dimension: wgpu::TextureDimension::D2,
                format: DEPTH_FORMAT,
                usage: wgpu::TextureUsages::RENDER_ATTACHMENT,
                view_formats: &[],
            })
            .create_view(&Default::default());
        let blur = target("puddle reflection horizontal filter", size);
        let filtered = target("puddle reflection filtered", size);
        let view = target("scene with puddle reflections", (w, h));
        let classic_bg = r.picture_group(&view);
        let binding = |binding, view| wgpu::BindGroupEntry {
            binding,
            resource: wgpu::BindingResource::TextureView(view),
        };
        let group = |ray_view| {
            r.device.create_bind_group(&wgpu::BindGroupDescriptor {
                label: Some("puddle reflections"),
                layout: &pipelines.layout,
                entries: &[
                    wgpu::BindGroupEntry {
                        binding: 0,
                        resource: pipelines.params.as_entire_binding(),
                    },
                    binding(1, &hdr.view),
                    binding(2, &hdr.mask),
                    binding(3, &r.ao.as_ref().unwrap().depth_view),
                    binding(4, ray_view),
                    wgpu::BindGroupEntry {
                        binding: 5,
                        resource: wgpu::BindingResource::Sampler(&r.post_sampler),
                    },
                    binding(6, &r.probe.as_ref().unwrap().view),
                    binding(7, &hit_depth_view),
                    binding(8, &vehicle_view),
                    binding(9, world_view.as_ref().unwrap_or(&hdr.view)),
                ],
            })
        };
        // The trace target must never appear in its own bind group, including unused slots.
        let trace_bg = group(&r.black_texture.view);
        let blur_x_bg = group(&trace);
        let blur_y_bg = group(&blur);
        let resolve_bg = group(&filtered);
        let post_group = |base, adapt| {
            r.device.create_bind_group(&wgpu::BindGroupDescriptor {
                label: Some("post with puddle reflections"),
                layout: &r.post_layout,
                entries: &[
                    wgpu::BindGroupEntry {
                        binding: 0,
                        resource: r.post_buf.as_entire_binding(),
                    },
                    binding(1, &view),
                    wgpu::BindGroupEntry {
                        binding: 2,
                        resource: wgpu::BindingResource::Sampler(&r.post_sampler),
                    },
                    binding(3, base),
                    binding(4, adapt),
                ],
            })
        };
        let down_bg = post_group(&hdr.mask, &r.white_texture.view);
        let tonemap_bg = [
            post_group(&hdr.up[0], &r.adapt_views[0]),
            post_group(&hdr.up[0], &r.adapt_views[1]),
        ];
        Self {
            size,
            source_depth,
            world_view,
            hit_depth,
            hit_depth_view,
            vehicle_view,
            vehicle_depth,
            trace,
            blur,
            filtered,
            view,
            trace_bg,
            blur_x_bg,
            blur_y_bg,
            resolve_bg,
            down_bg,
            tonemap_bg,
            classic_bg,
        }
    }
}

impl Renderer {
    pub(crate) fn prepare_puddle_reflections(
        &mut self,
        w: u32,
        h: u32,
        camera: &Camera,
        aspect: f32,
        projection: Option<Mat4>,
        cu: &CameraUniform,
        lighting: &Lighting,
    ) -> bool {
        if self.puddles.is_none()
            || self.ao.is_none()
            || self.probe.is_none()
            || (cu.post[0] > 0.5 && self.sky_state.is_none())
        {
            return false;
        }
        let Some(hdr) = self.hdr_targets.get(&(w, h)) else {
            return false;
        };
        // A previously used size can outlive the AO depth after a resize/another view.
        // Rebind the current receiver texture instead of tracing against an old frame.
        let cabin = camera_in_vehicle(lighting, camera);
        if hdr.puddles.as_ref().is_none_or(|t| {
            t.source_depth != *self.ao.as_ref().unwrap().depth_view.texture()
                || t.world_view.is_some() != cabin
        }) {
            let targets = Targets::new(self, hdr, w, h, cabin);
            log::info!(
                "puddle reflections: {}x{} rays, at most 48 steps; resolve {w}x{h}",
                targets.size.0,
                targets.size.1
            );
            self.hdr_targets.get_mut(&(w, h)).unwrap().puddles = Some(targets);
        }
        let vp = Mat4::from_cols_array_2d(&cu.view_proj);
        let proj = projection.unwrap_or_else(|| {
            glam::camera::rh::proj::directx::perspective(
                camera.fov_deg.to_radians(),
                aspect,
                camera.far,
                camera.near,
            )
        });
        let enhanced = cu.post[0] > 0.5;
        let surround = if enhanced {
            let st = self.sky_state.as_ref().unwrap();
            [Vec3::X, -Vec3::X, Vec3::Y, -Vec3::Y, Vec3::Z, -Vec3::Z]
                .into_iter()
                .map(|n| atmosphere::sh_irradiance(&st.sh, n))
                .sum::<Vec3>()
                * (0.25 / (6.0 * std::f32::consts::PI))
        } else {
            lighting.secondary * 0.5 + lighting.sun_color * lighting.sun_intensity * 0.35
        };
        let pre = if enhanced {
            self.exposure.unwrap_or(0.0).exp()
        } else {
            1.0
        };
        let ro = (camera.position / 100.0).floor() * 100.0;
        let origins = vehicle_origins(lighting, camera);
        let mut parts = [<VehicleBox as bytemuck::Zeroable>::zeroed(); 4];
        for (part, (o, h, bb)) in parts
            .iter_mut()
            .zip(lighting.inside.iter().chain(lighting.puddle_parts.iter()))
        {
            let origin = (*o - ro).as_vec3();
            let heading = (*h as f32).to_radians();
            *part = VehicleBox {
                a: [origin.x, origin.y, origin.z, heading.sin()],
                b: [heading.cos(), bb[0] * 0.5, bb[1] * 0.5, bb[2] * 0.5],
                c: [bb[3], bb[4], bb[5], 1.0],
            };
        }
        let normal = lighting.puddle_normal.normalize_or_zero();
        let plane_normal = if normal.z > 0.3 { normal } else { Vec3::Z };
        let plane_point = lighting.inside.map_or(DVec3::ZERO, |(o, _, _)| {
            DVec3::new(o.x, o.y, lighting.puddle_ground.unwrap_or(o.z))
        });
        let plane = reflection_plane(plane_point, plane_normal, ro).to_array();
        let u = Uniform {
            view_proj: cu.view_proj,
            inv_view_proj: vp.inverse().to_cols_array_2d(),
            eye_time: [cu.cam_pos[0], cu.cam_pos[1], cu.cam_pos[2], cu.post[1]],
            origin_rain: [
                cu.world_origin[0],
                cu.world_origin[1],
                lighting.rain.clamp(0.0, 1.0) * (1.0 - lighting.snow.clamp(0.0, 1.0)),
                PROBE_MIPS as f32,
            ],
            sky: (surround * pre)
                .extend(self.probe.as_ref().unwrap().scale * pre)
                .to_array(),
            projection_trace: [
                proj.z_axis.z,
                proj.w_axis.z,
                ::legacy_config::env::var("OMSI_DEBUG_PUDDLES")
                    .ok()
                    .and_then(|v| v.parse().ok())
                    .unwrap_or(0.0),
                ::legacy_config::env::var("OMSI_PUDDLE_THICKNESS")
                    .ok()
                    .and_then(|v| v.parse().ok())
                    .unwrap_or(0.12),
            ],
            vehicle_plane: plane,
            vehicle_info: [
                origins.len() as f32,
                w as f32,
                if enhanced {
                    1.0
                } else if lighting.classic {
                    0.0
                } else {
                    -1.0
                },
                lighting.inside.map_or(0.0, |(_, hd, _)| {
                    let r = (hd as f32).to_radians();
                    lighting.glass_wind.x * r.sin() + lighting.glass_wind.y * r.cos()
                }),
            ],
            vehicle_parts: parts,
        };
        self.queue.write_buffer(
            &self.puddles.as_ref().unwrap().vehicle_params,
            0,
            bytemuck::bytes_of(&VehicleUniform { plane, parts }),
        );
        self.queue.write_buffer(
            &self.puddles.as_ref().unwrap().params,
            0,
            bytemuck::bytes_of(&u),
        );
        true
    }

    pub(crate) fn encode_puddle_reflections(
        &self,
        encoder: &mut wgpu::CommandEncoder,
        w: u32,
        h: u32,
        scene: &Scene,
        batches: &[Batch],
        draw_list: &[u32],
        lighting: &Lighting,
        camera: &Camera,
        queries: Option<&wgpu::QuerySet>,
        timed: &mut Vec<&'static str>,
    ) {
        let pipelines = self.puddles.as_ref().unwrap();
        let targets = self.hdr_targets[&(w, h)].puddles.as_ref().unwrap();
        // Select only this vehicle's existing draw entries. AI instances can share meshes
        // and batches with it; selecting a mesh alone would reflect all of them at one height.
        let origins = vehicle_origins(lighting, camera);
        let vehicle_batches = player_vehicle_batches(scene, batches, draw_list, &origins);
        let world_batches = targets.world_view.as_ref().map(|_| {
            let origins: Vec<_> = lighting
                .inside
                .iter()
                .chain(lighting.puddle_parts.iter())
                .take(4)
                .map(|(o, _, _)| *o)
                .collect();
            let own = vehicle_entries(scene, &origins);
            filter_entry_batches(batches, draw_list, |entry| !own.contains(&entry))
        });
        {
            let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                label: Some("puddle player vehicle"),
                color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                    view: &targets.vehicle_view,
                    resolve_target: None,
                    depth_slice: None,
                    ops: wgpu::Operations {
                        load: wgpu::LoadOp::Clear(wgpu::Color::TRANSPARENT),
                        store: wgpu::StoreOp::Store,
                    },
                })],
                depth_stencil_attachment: Some(wgpu::RenderPassDepthStencilAttachment {
                    view: &targets.vehicle_depth,
                    depth_ops: Some(wgpu::Operations {
                        load: wgpu::LoadOp::Clear(0.0),
                        store: wgpu::StoreOp::Discard,
                    }),
                    stencil_ops: None,
                }),
                timestamp_writes: pass_timer(queries, timed, "puddle vehicle"),
                occlusion_query_set: None,
                multiview_mask: None,
            });
            pass.set_bind_group(0, scene.camera_bind_group.as_ref().unwrap(), &[]);
            pass.set_bind_group(2, &pipelines.vehicle_bg, &[]);
            if ::legacy_config::env::var_os("OMSI_NO_PUDDLE_VEHICLE").is_none() {
                if let Some(first) = vehicle_batches.first() {
                    pass.set_bind_group(
                        1,
                        &scene.materials[first.material as usize].bind_group,
                        &[],
                    );
                    pass.set_pipeline(&pipelines.chassis);
                    pass.draw(0..6, 0..origins.len() as u32);
                }
                encode_batches(&mut pass, scene, &vehicle_batches, |pipe| {
                    &pipelines.vehicle[pipe as usize]
                });
            }
        }
        let source_batches = world_batches.as_deref().unwrap_or(batches);
        if let Some(world_view) = &targets.world_view {
            // Colour and hit depth must describe the same unobstructed world. Merely
            // skipping near depth hits still samples the wiper's colour from the main view.
            let enhanced = lighting.enhanced
                && self.hdr_pass.is_some()
                && ::legacy_config::env::var_os("OMSI_NO_ENHANCED").is_none();
            let pp = self.main_pass(enhanced, !enhanced);
            let sky = if lighting.classic && !enhanced {
                lighting.sky_color.map(|v| {
                    if v <= 0.04045 {
                        v / 12.92
                    } else {
                        ((v + 0.055) / 1.055).powf(2.4)
                    }
                })
            } else {
                lighting.sky_color
            };
            let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                label: Some("puddle world without cabin"),
                color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                    view: world_view,
                    depth_slice: None,
                    resolve_target: None,
                    ops: wgpu::Operations {
                        load: wgpu::LoadOp::Clear(wgpu::Color {
                            r: sky.x as f64,
                            g: sky.y as f64,
                            b: sky.z as f64,
                            a: 1.0,
                        }),
                        store: wgpu::StoreOp::Store,
                    },
                })],
                depth_stencil_attachment: Some(wgpu::RenderPassDepthStencilAttachment {
                    view: &targets.hit_depth_view,
                    depth_ops: Some(wgpu::Operations {
                        load: wgpu::LoadOp::Clear(0.0),
                        store: wgpu::StoreOp::Store,
                    }),
                    stencil_ops: None,
                }),
                timestamp_writes: pass_timer(queries, timed, "puddle world"),
                occlusion_query_set: None,
                multiview_mask: None,
            });
            pass.set_bind_group(0, scene.camera_bind_group.as_ref().unwrap(), &[]);
            if let Some(sky) = &scene.sky_bind_group {
                pass.set_pipeline(&pp.reflection_sky_pipeline);
                pass.set_bind_group(1, sky, &[]);
                pass.set_vertex_buffer(0, Some(self.sky_mesh.0.slice(..)));
                pass.set_index_buffer(self.sky_mesh.1.slice(..), wgpu::IndexFormat::Uint32);
                pass.draw_indexed(0..self.sky_mesh.2, 0, 0..1);
            }
            pass.set_bind_group(0, scene.camera_bind_group.as_ref().unwrap(), &[]);
            encode_batches(&mut pass, scene, source_batches, |pipe| {
                &pipelines.world[pipe as usize]
            });
            // Match the main view's particle order, textures, blending and exposure,
            // using single-sample pipelines against the unobstructed world's depth.
            self.encode_particles(
                &mut pass,
                scene,
                &pp.reflection_smoke_pipeline,
                &pp.reflection_corona_pipeline,
            );
        } else {
            let copy = |texture| wgpu::TexelCopyTextureInfo {
                texture,
                mip_level: 0,
                origin: wgpu::Origin3d::ZERO,
                aspect: wgpu::TextureAspect::DepthOnly,
            };
            encoder.copy_texture_to_texture(
                copy(&targets.source_depth),
                copy(&targets.hit_depth),
                wgpu::Extent3d {
                    width: w,
                    height: h,
                    depth_or_array_layers: 1,
                },
            );
        }
        if ::legacy_config::env::var_os("OMSI_NO_PUDDLE_GLASS_DEPTH").is_none()
            && source_batches
                .iter()
                .any(|b| reflection_glass(&scene.materials[b.material as usize].uniform))
        {
            let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                label: Some("puddle glass depth"),
                color_attachments: &[],
                depth_stencil_attachment: Some(wgpu::RenderPassDepthStencilAttachment {
                    view: &targets.hit_depth_view,
                    depth_ops: Some(wgpu::Operations {
                        load: wgpu::LoadOp::Load,
                        store: wgpu::StoreOp::Store,
                    }),
                    stencil_ops: None,
                }),
                timestamp_writes: pass_timer(queries, timed, "puddle glass"),
                occlusion_query_set: None,
                multiview_mask: None,
            });
            pass.set_bind_group(0, scene.camera_bind_group.as_ref().unwrap(), &[]);
            encode_batches_filtered(
                &mut pass,
                scene,
                source_batches,
                |b| reflection_glass(&scene.materials[b.material as usize].uniform),
                |pipe| &pipelines.glass[(pipe % 4 / 2) as usize],
            );
        }
        post_pass(
            encoder,
            &targets.trace,
            pass_timer(queries, timed, "puddle rays"),
            &pipelines.trace,
            &targets.trace_bg,
        );
        let filter_timer = pass_timer(queries, timed, "puddle filter");
        post_pass(
            encoder,
            &targets.blur,
            filter_timer
                .as_ref()
                .map(|t| wgpu::RenderPassTimestampWrites {
                    query_set: t.query_set,
                    beginning_of_pass_write_index: t.beginning_of_pass_write_index,
                    end_of_pass_write_index: None,
                }),
            &pipelines.blur_x,
            &targets.blur_x_bg,
        );
        post_pass(
            encoder,
            &targets.filtered,
            filter_timer.map(|t| wgpu::RenderPassTimestampWrites {
                query_set: t.query_set,
                beginning_of_pass_write_index: None,
                end_of_pass_write_index: t.end_of_pass_write_index,
            }),
            &pipelines.blur_y,
            &targets.blur_y_bg,
        );
        post_pass(
            encoder,
            &targets.view,
            pass_timer(queries, timed, "puddle resolve"),
            &pipelines.resolve,
            &targets.resolve_bg,
        );
    }
}

fn reflection_plane(point: DVec3, normal: Vec3, origin: DVec3) -> Vec4 {
    normal.extend(normal.dot((point - origin).as_vec3()))
}

// Match the shader's cabin test, including its centre offset and heading. The
// capture is unnecessary on foot, where the player's vehicle should remain visible.
fn camera_in_vehicle(lighting: &Lighting, camera: &Camera) -> bool {
    let Some((origin, heading, bounds)) = lighting.inside else {
        return false;
    };
    let d = (camera.position - origin).as_vec3();
    let (sh, ch) = (heading as f32).to_radians().sin_cos();
    let local = Vec3::new(d.x * ch - d.y * sh, d.x * sh + d.y * ch, d.z)
        - Vec3::new(bounds[3], bounds[4], bounds[5]);
    let half = Vec3::new(bounds[0], bounds[1], bounds[2]) * 0.5;
    local.x.abs() < half.x - 0.03
        && local.y.abs() < half.y - 0.03
        && local.z > -half.z - 0.6
        && local.z < half.z - 0.03
}

fn vehicle_origins(lighting: &Lighting, camera: &Camera) -> Vec<DVec3> {
    if lighting.puddle_ground.is_none()
        || ::legacy_config::env::var_os("OMSI_NO_PUDDLE_VEHICLE").is_some()
        || !lighting
            .inside
            .is_some_and(|(o, _, _)| o.distance(camera.position) < 60.0)
    {
        return Vec::new();
    }
    lighting
        .inside
        .iter()
        .chain(lighting.puddle_parts.iter())
        .take(4)
        .map(|(o, _, _)| *o)
        .collect()
}

fn player_vehicle_batches(
    scene: &Scene,
    batches: &[Batch],
    list: &[u32],
    origins: &[DVec3],
) -> Vec<Batch> {
    selected_entry_batches(batches, list, &vehicle_entries(scene, origins))
}

fn vehicle_entries(scene: &Scene, origins: &[DVec3]) -> std::collections::HashSet<u32> {
    // core::scene's vehicle loader sets roof from the same bounding box that supplies
    // Lighting::inside/puddle_parts. Animated meshes retain their vehicle's origin.
    scene
        .instances
        .iter()
        .filter(|i| {
            i.visible
                && i.roof.is_some()
                && !i.blob
                && !i.surface
                && origins.iter().any(|o| i.origin.distance_squared(*o) < 0.01)
        })
        .flat_map(|i| i.base..i.base + i.materials.len() as u32)
        .collect()
}

fn selected_entry_batches(
    batches: &[Batch],
    list: &[u32],
    entries: &std::collections::HashSet<u32>,
) -> Vec<Batch> {
    filter_entry_batches(batches, list, |entry| entries.contains(&entry))
}

fn filter_entry_batches(
    batches: &[Batch],
    list: &[u32],
    include: impl Fn(u32) -> bool,
) -> Vec<Batch> {
    let mut out = Vec::new();
    for b in batches {
        let mut start = None;
        for index in b.instances.clone() {
            if include(list[index as usize]) {
                start.get_or_insert(index);
            } else if let Some(first) = start.take() {
                out.push(Batch {
                    pipe: b.pipe,
                    mesh: b.mesh,
                    first: b.first,
                    count: b.count,
                    material: b.material,
                    instances: first..index,
                });
            }
        }
        if let Some(first) = start {
            out.push(Batch {
                pipe: b.pipe,
                mesh: b.mesh,
                first: b.first,
                count: b.count,
                material: b.material,
                instances: first..b.instances.end,
            });
        }
    }
    out
}

// Match the enhanced shader's glass classification, excluding rain films, decals and
// unlit screens. Their colour is already composited over the actual window surface.
fn reflection_glass(u: &MaterialUniform) -> bool {
    u.extra[0] < 0.5
        && u.params[0] > 1.5
        && u.params[1] < 0.5
        && (u.bump[2] > 0.5 || u.emissive[3] > 0.5)
        && u.bump[3] < 0.5
        && u.emissive[3] < 1.5
        && (u.params2[1] > 0.0 || u.params[2] > 0.5 || u.emissive[3] > 0.5)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reflection_preserves_clearance_at_any_height_grade_and_floating_origin() {
        for height in [-125.0, 0.09, 12.0, 501.0] {
            for normal in [Vec3::Z, Vec3::new(0.1, -0.04, 1.0).normalize()] {
                let ground = DVec3::new(-2165.0, 736.0, height);
                let origin = DVec3::new(-2200.0, 700.0, (height / 100.0).floor() * 100.0);
                let plane = reflection_plane(ground, normal, origin);
                let point = (ground - origin).as_vec3() + normal * 0.4;
                let reflected = point - 2.0 * normal * (normal.dot(point) - plane.w);
                assert!((normal.dot(point) - plane.w - 0.4).abs() < 1e-4);
                assert!((normal.dot(reflected) - plane.w + 0.4).abs() < 1e-4);
                assert!(((point + reflected) * 0.5 - (ground - origin).as_vec3()).length() < 1e-4);
            }
        }
    }

    #[test]
    #[ignore = "requires a graphics adapter; checks transparent window reflection depth"]
    fn glass_hit_depth_preserves_receiver_depth_across_resizes() {
        let instance = wgpu::Instance::new(wgpu::InstanceDescriptor::new_without_display_handle());
        let mut r = pollster::block_on(Renderer::new_with(
            &instance,
            None,
            Some(wgpu::TextureFormat::Rgba8UnormSrgb),
            RenderOptions {
                msaa: 4,
                ssao: false,
                shadow_size: 1024,
                render_scale: 1.0,
                ..Default::default()
            },
        ))
        .expect("test renderer");
        let mut scene = r.new_scene();
        let back = r.add_material_wet(
            &mut scene,
            None,
            AlphaMode::Opaque,
            [1.0; 4],
            false,
            None,
            None,
            None,
            None,
            None,
            [0.0; 3],
            1.0,
        );
        let pane = r.add_material_inner(
            &mut scene,
            None,
            AlphaMode::Blend,
            [0.5, 0.5, 0.5, 0.15],
            false,
            None,
            None,
            None,
            None,
            None,
            [0.0; 3],
            0.0,
            MaterialExtra {
                glass: true,
                ..Default::default()
            },
        );
        for (y, half, material) in [(10.0, 10.0, back), (5.0, 2.0, pane)] {
            let mesh = r.add_mesh(
                &mut scene,
                &MeshData {
                    positions: vec![
                        Vec3::new(-half, y, -half),
                        Vec3::new(half, y, -half),
                        Vec3::new(half, y, half),
                        Vec3::new(-half, y, half),
                    ],
                    normals: vec![-Vec3::Y; 4],
                    uvs: vec![glam::Vec2::ZERO; 4],
                    ranges: vec![(0, 6, 0)],
                    indices: vec![0, 1, 2, 0, 2, 3],
                    one_sided: false,
                },
            );
            let instance = r.add_instance(
                &mut scene,
                mesh,
                DVec3::ZERO,
                Mat4::IDENTITY,
                vec![material],
            );
            if material == pane {
                r.set_roof(&mut scene, instance, Some(6.0));
            }
        }
        let camera = Camera {
            position: DVec3::ZERO,
            yaw: 0.0,
            pitch: 0.0,
            roll: 0.0,
            fov_deg: 90.0,
            near: 0.1,
            far: 100.0,
        };
        let mut lighting = Lighting {
            enhanced: true,
            wetness: 1.0,
            shadows: false,
            ..Default::default()
        };
        fn depth_at_centre(r: &Renderer, texture: &wgpu::Texture) -> f32 {
            let size = texture.width();
            let buffer = r.device.create_buffer(&wgpu::BufferDescriptor {
                label: Some("test reflection depth"),
                size: 256 * size as u64,
                usage: wgpu::BufferUsages::COPY_DST | wgpu::BufferUsages::MAP_READ,
                mapped_at_creation: false,
            });
            let mut encoder = r.device.create_command_encoder(&Default::default());
            encoder.copy_texture_to_buffer(
                wgpu::TexelCopyTextureInfo {
                    texture,
                    mip_level: 0,
                    origin: wgpu::Origin3d::ZERO,
                    aspect: wgpu::TextureAspect::DepthOnly,
                },
                wgpu::TexelCopyBufferInfo {
                    buffer: &buffer,
                    layout: wgpu::TexelCopyBufferLayout {
                        offset: 0,
                        bytes_per_row: Some(256),
                        rows_per_image: Some(size),
                    },
                },
                wgpu::Extent3d {
                    width: size,
                    height: size,
                    depth_or_array_layers: 1,
                },
            );
            r.queue.submit([encoder.finish()]);
            let (tx, rx) = std::sync::mpsc::channel();
            buffer.slice(..).map_async(wgpu::MapMode::Read, move |v| {
                tx.send(v).unwrap();
            });
            r.device.poll(wgpu::PollType::wait_indefinitely()).unwrap();
            rx.recv().unwrap().unwrap();
            let data = buffer.slice(..).get_mapped_range().expect("mapped range");
            let offset = (size / 2 * 256 + size / 2 * 4) as usize;
            let depth = f32::from_le_bytes(data[offset..offset + 4].try_into().unwrap());
            drop(data);
            buffer.unmap();
            depth
        }
        for inside in [false, true, false, true] {
            lighting.inside =
                inside.then_some((DVec3::ZERO, 0.0, [12.0, 12.0, 12.0, 0.0, 0.0, 0.0]));
            for size in [64, 48, 64] {
                r.render_to_image(&mut scene, size, size, &camera, &lighting)
                    .unwrap();
                let targets = r.hdr_targets[&(size, size)].puddles.as_ref().unwrap();
                assert_eq!(
                    targets.source_depth,
                    *r.ao.as_ref().unwrap().depth_view.texture()
                );
                assert_eq!(targets.world_view.is_some(), inside);
                assert_eq!(
                    targets.hit_depth.width(),
                    if inside { size / 2 } else { size }
                );
                let receiver = depth_at_centre(&r, &targets.source_depth);
                let hit = depth_at_centre(&r, &targets.hit_depth);
                assert!(
                    (receiver - 0.009009).abs() < 1e-5,
                    "opaque receiver: {receiver}"
                );
                let expected_hit = if inside { receiver } else { 0.019019 };
                assert!(
                    (hit - expected_hit).abs() < 1e-5,
                    "glass hit: {hit}; inside={inside}"
                );
            }
        }
    }

    #[test]
    #[ignore = "requires a graphics adapter; checks moving cabin geometry in puddles"]
    fn moving_wipers_do_not_change_unoccluded_puddle_reflections() {
        for msaa in [1, 4] {
            let instance =
                wgpu::Instance::new(wgpu::InstanceDescriptor::new_without_display_handle());
            let mut r = pollster::block_on(Renderer::new_with(
                &instance,
                None,
                Some(wgpu::TextureFormat::Rgba8UnormSrgb),
                RenderOptions {
                    msaa,
                    ssao: false,
                    fxaa: false,
                    shadow_size: 1024,
                    render_scale: 1.0,
                    ..Default::default()
                },
            ))
            .expect("test renderer");
            let mut scene = r.new_scene();
            let road = r.add_material_wet(
                &mut scene,
                None,
                AlphaMode::Opaque,
                [0.25, 0.25, 0.25, 1.0],
                false,
                None,
                None,
                None,
                None,
                None,
                [0.0; 3],
                1.0,
            );
            let red = r.add_material(
                &mut scene,
                None,
                AlphaMode::Opaque,
                [1.0, 0.01, 0.01, 1.0],
                true,
            );
            let black = r.add_material(
                &mut scene,
                None,
                AlphaMode::Opaque,
                [0.01, 0.01, 0.01, 1.0],
                true,
            );
            let no_write = r.add_material(
                &mut scene,
                None,
                AlphaMode::Blend,
                [0.01, 0.01, 0.01, 1.0],
                true,
            );
            r.set_no_z_write(&mut scene, no_write, true);
            test_quad(
                &r,
                &mut scene,
                [
                    [-20.0, 0.0, 0.0],
                    [20.0, 0.0, 0.0],
                    [20.0, 30.0, 0.0],
                    [-20.0, 30.0, 0.0],
                ],
                Vec3::Z,
                road,
            );
            test_quad(
                &r,
                &mut scene,
                [
                    [-3.0, 10.0, 0.0],
                    [3.0, 10.0, 0.0],
                    [3.0, 10.0, 5.0],
                    [-3.0, 10.0, 5.0],
                ],
                -Vec3::Y,
                red,
            );
            let wiper = test_quad(
                &r,
                &mut scene,
                [
                    [-0.05, 1.0, 2.0],
                    [0.05, 1.0, 2.0],
                    [0.05, 1.0, 3.0],
                    [-0.05, 1.0, 3.0],
                ],
                -Vec3::Y,
                black,
            );
            r.set_roof(&mut scene, wiper, Some(4.0));
            let camera = Camera {
                position: DVec3::new(0.0, 0.0, 2.0),
                yaw: 0.0,
                pitch: -10.0,
                roll: 0.0,
                fov_deg: 70.0,
                near: 0.1,
                far: 100.0,
            };
            for (enhanced, classic) in [(false, true), (false, false), (true, false)] {
                let lighting = Lighting {
                    enhanced,
                    classic,
                    wetness: 1.0,
                    rain: 0.0,
                    shadows: false,
                    animation_time: Some(0.0),
                    // No planar vehicle capture: isolate the screen-space source.
                    inside: Some((DVec3::ZERO, 0.0, [4.0, 4.0, 4.0, 0.0, 0.0, 2.0])),
                    ..Default::default()
                };
                r.set_params(&mut scene, wiper, &[], false, &[]);
                r.options.reflections = false;
                let dry = r
                    .render_to_image(&mut scene, 128, 128, &camera, &lighting)
                    .unwrap();
                r.options.reflections = true;
                let baseline = r
                    .render_to_image(&mut scene, 128, 128, &camera, &lighting)
                    .unwrap();
                let baseline_trace = reflection_trace_pixels(&r);
                let reflected_red: i64 = baseline[128 * 70 * 4..]
                    .chunks_exact(4)
                    .zip(dry[128 * 70 * 4..].chunks_exact(4))
                    .map(|(a, b)| (a[0] as i64 - a[1] as i64) - (b[0] as i64 - b[1] as i64))
                    .sum();
                assert!(
                    reflected_red > 1000,
                    "reflection must remain visible: {reflected_red}"
                );
                for material in [black, no_write] {
                    r.set_material(&mut scene, wiper, 0, material);
                    r.set_params(&mut scene, wiper, &[], true, &[]);
                    for x in [-0.35, 0.0, 0.35] {
                        r.set_transform(
                            &mut scene,
                            wiper,
                            DVec3::ZERO,
                            Mat4::from_translation(Vec3::new(x, 0.0, 0.0)),
                        );
                        let with = r
                            .render_to_image(&mut scene, 128, 128, &camera, &lighting)
                            .unwrap();
                        assert_ne!(
                            &baseline[..128 * 60 * 4],
                            &with[..128 * 60 * 4],
                            "wiper must still be drawn"
                        );
                        // Compare HDR reflection rays before metering/bloom. A moving
                        // dark blade can legitimately change Enhanced's exposure.
                        assert!(
                            baseline_trace == reflection_trace_pixels(&r),
                            "wiper changed reflection rays; MSAA={msaa}, enhanced={enhanced}, classic={classic}, material={material}, x={x}"
                        );
                    }
                }
            }
        }
    }

    #[test]
    #[ignore = "requires a graphics adapter; checks cabin reflections of coronas and smoke"]
    fn cabin_reflections_preserve_particles_and_world_occlusion() {
        for msaa in [1, 4] {
            let instance =
                wgpu::Instance::new(wgpu::InstanceDescriptor::new_without_display_handle());
            let mut r = pollster::block_on(Renderer::new_with(
                &instance,
                None,
                Some(wgpu::TextureFormat::Rgba8UnormSrgb),
                RenderOptions {
                    msaa,
                    ssao: false,
                    fxaa: false,
                    shadow_size: 1024,
                    render_scale: 1.0,
                    ..Default::default()
                },
            ))
            .expect("test renderer");
            let mut scene = r.new_scene();
            let road = r.add_material_wet(
                &mut scene,
                None,
                AlphaMode::Opaque,
                [0.25, 0.25, 0.25, 1.0],
                false,
                None,
                None,
                None,
                None,
                None,
                [0.0; 3],
                1.0,
            );
            let wall = r.add_material(
                &mut scene,
                None,
                AlphaMode::Opaque,
                [0.1, 0.1, 0.1, 1.0],
                true,
            );
            test_quad(
                &r,
                &mut scene,
                [
                    [-20.0, 0.0, 0.0],
                    [20.0, 0.0, 0.0],
                    [20.0, 30.0, 0.0],
                    [-20.0, 30.0, 0.0],
                ],
                Vec3::Z,
                road,
            );
            test_quad(
                &r,
                &mut scene,
                [
                    [-3.0, 10.0, 0.0],
                    [3.0, 10.0, 0.0],
                    [3.0, 10.0, 5.0],
                    [-3.0, 10.0, 5.0],
                ],
                -Vec3::Y,
                wall,
            );
            let camera = Camera {
                position: DVec3::new(0.0, 0.0, 2.0),
                yaw: 0.0,
                pitch: -10.0,
                roll: 0.0,
                fov_deg: 70.0,
                near: 0.1,
                far: 100.0,
            };
            for (enhanced, classic) in [(false, true), (false, false), (true, false)] {
                let lighting = Lighting {
                    enhanced,
                    classic,
                    wetness: 1.0,
                    shadows: false,
                    animation_time: Some(0.0),
                    inside: Some((DVec3::ZERO, 0.0, [4.0, 4.0, 4.0, 0.0, 0.0, 2.0])),
                    ..Default::default()
                };
                scene.coronas.clear();
                scene.smoke.clear();
                r.render_to_image(&mut scene, 128, 128, &camera, &lighting)
                    .unwrap();
                let baseline = reflection_trace_pixels(&r);
                for smoke in [false, true] {
                    for (y, visible) in [(9.0, true), (12.0, false)] {
                        scene.coronas.clear();
                        scene.smoke.clear();
                        let position = DVec3::new(0.0, y, 2.0);
                        if smoke {
                            scene.smoke.push(SmokeParticle {
                                position,
                                size: 0.8,
                                color: [1.0, 0.0, 0.0],
                                alpha: 1.0,
                            });
                        } else {
                            scene.coronas.push(Corona {
                                position,
                                size: 0.8,
                                color: [1.0, 0.0, 0.0],
                                brightness: 1.0,
                                ..Default::default()
                            });
                        }
                        r.render_to_image(&mut scene, 128, 128, &camera, &lighting)
                            .unwrap();
                        assert!(!r.gpu_error.load(std::sync::atomic::Ordering::Relaxed));
                        // Read reflection rays before exposure adaptation and bloom: changes
                        // here must come from the reflected particle, not its direct image.
                        let changed = baseline != reflection_trace_pixels(&r);
                        assert_eq!(
                            changed, visible,
                            "particle reflection: smoke={smoke}, y={y}, MSAA={msaa}, enhanced={enhanced}, classic={classic}"
                        );
                    }
                }
            }
        }
    }

    fn reflection_trace_pixels(r: &Renderer) -> Vec<u8> {
        let texture = r.hdr_targets[&(128, 128)]
            .puddles
            .as_ref()
            .unwrap()
            .trace
            .texture();
        let (w, h) = (texture.width(), texture.height());
        let stride = (w * 8).div_ceil(256) * 256;
        let buffer = r.device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("test reflection rays"),
            size: (stride * h) as u64,
            usage: wgpu::BufferUsages::COPY_DST | wgpu::BufferUsages::MAP_READ,
            mapped_at_creation: false,
        });
        let mut encoder = r.device.create_command_encoder(&Default::default());
        encoder.copy_texture_to_buffer(
            wgpu::TexelCopyTextureInfo {
                texture,
                mip_level: 0,
                origin: wgpu::Origin3d::ZERO,
                aspect: wgpu::TextureAspect::All,
            },
            wgpu::TexelCopyBufferInfo {
                buffer: &buffer,
                layout: wgpu::TexelCopyBufferLayout {
                    offset: 0,
                    bytes_per_row: Some(stride),
                    rows_per_image: Some(h),
                },
            },
            wgpu::Extent3d {
                width: w,
                height: h,
                depth_or_array_layers: 1,
            },
        );
        r.queue.submit([encoder.finish()]);
        let (tx, rx) = std::sync::mpsc::channel();
        buffer.slice(..).map_async(wgpu::MapMode::Read, move |v| {
            tx.send(v).unwrap();
        });
        r.device.poll(wgpu::PollType::wait_indefinitely()).unwrap();
        rx.recv().unwrap().unwrap();
        let pixels = buffer
            .slice(..)
            .get_mapped_range()
            .expect("mapped reflection rays")
            .to_vec();
        buffer.unmap();
        pixels
    }

    fn test_quad(
        r: &Renderer,
        scene: &mut Scene,
        positions: [[f32; 3]; 4],
        normal: Vec3,
        material: MaterialId,
    ) -> usize {
        let mesh = r.add_mesh(
            scene,
            &MeshData {
                positions: positions.map(Vec3::from).to_vec(),
                normals: vec![normal; 4],
                uvs: vec![
                    glam::Vec2::ZERO,
                    glam::Vec2::X,
                    glam::Vec2::ONE,
                    glam::Vec2::Y,
                ],
                indices: vec![0, 1, 2, 0, 2, 3],
                ranges: vec![(0, 6, 0)],
                one_sided: false,
            },
        );
        r.add_instance(scene, mesh, DVec3::ZERO, Mat4::IDENTITY, vec![material])
    }

    #[test]
    #[ignore = "requires a graphics adapter; renders wet roads through bus glass"]
    fn wet_scene_reflections_survive_bus_glass_in_every_graphics_mode() {
        for msaa in [1, 4] {
            let instance = wgpu::Instance::new(wgpu::InstanceDescriptor::new_without_display_handle());
            let mut r = pollster::block_on(Renderer::new_with(
                &instance,
                None,
                Some(wgpu::TextureFormat::Rgba8UnormSrgb),
                RenderOptions {
                    msaa,
                    ssao: false,
                    fxaa: false,
                    shadow_size: 1024,
                    render_scale: 1.0,
                    ..Default::default()
                },
            ))
            .expect("test renderer");
            let mut scene = r.new_scene();
            let material = |scene: &mut Scene, colour, unlit, moisture, extra: MaterialExtra| {
                r.add_material_inner(
                    scene,
                    None,
                    if extra.glass || extra.rain_film {
                        AlphaMode::Blend
                    } else {
                        AlphaMode::Opaque
                    },
                    colour,
                    unlit,
                    None,
                    None,
                    None,
                    None,
                    None,
                    [0.0; 3],
                    moisture,
                    extra,
                )
            };
            let road = material(
                &mut scene, [0.25, 0.25, 0.25, 1.0], false, 1.0, MaterialExtra::default(),
            );
            let red = material(
                &mut scene, [1.0, 0.01, 0.01, 1.0], true, 0.0, MaterialExtra::default(),
            );
            // Legacy bus panes may write colour-pass depth while remaining transparent.
            let pane = material(
                &mut scene, [0.25, 0.3, 0.35, 0.15], false, 0.0,
                MaterialExtra { glass: true, ..Default::default() },
            );
            let film = material(
                &mut scene, [1.0; 4], false, 0.0,
                MaterialExtra { rain_film: true, no_z_write: true, ..Default::default() },
            );
            test_quad(
                &r, &mut scene,
                [[-20.0, 0.0, 0.0], [20.0, 0.0, 0.0], [20.0, 30.0, 0.0], [-20.0, 30.0, 0.0]],
                Vec3::Z, road,
            );
            test_quad(
                &r, &mut scene,
                [[-3.0, 10.0, 0.0], [3.0, 10.0, 0.0], [3.0, 10.0, 5.0], [-3.0, 10.0, 5.0]],
                -Vec3::Y, red,
            );
            let window = test_quad(
                &r, &mut scene,
                [[-3.0, 1.0, -1.0], [3.0, 1.0, -1.0], [3.0, 1.0, 6.0], [-3.0, 1.0, 6.0]],
                -Vec3::Y, pane,
            );
            let rain = test_quad(
                &r, &mut scene,
                [[-3.0, 0.99, -1.0], [3.0, 0.99, -1.0], [3.0, 0.99, 6.0], [-3.0, 0.99, 6.0]],
                -Vec3::Y, film,
            );
            r.set_roof(&mut scene, window, Some(6.0));
            r.set_roof(&mut scene, rain, Some(6.0));
            let camera = Camera {
                position: DVec3::new(0.0, 0.0, 2.0),
                yaw: 0.0,
                pitch: -10.0,
                roll: 0.0,
                fov_deg: 70.0,
                near: 0.1,
                far: 100.0,
            };
            for (enhanced, classic) in [(false, true), (false, false), (true, false)] {
                let lighting = Lighting {
                    enhanced,
                    classic,
                    wetness: 1.0,
                    rain: 0.8,
                    shadows: false,
                    inside: Some((DVec3::ZERO, 0.0, [4.0, 4.0, 4.0, 0.0, 0.0, 2.0])),
                    ..Default::default()
                };
                for glass in 0..3 {
                    r.set_params(&mut scene, window, &[], glass > 0, &[]);
                    r.set_params(&mut scene, rain, &[], glass == 2, &[]);
                    r.options.reflections = false;
                    let without = r
                        .render_to_image(&mut scene, 128, 128, &camera, &lighting)
                        .unwrap();
                    r.options.reflections = true;
                    let with = r
                        .render_to_image(&mut scene, 128, 128, &camera, &lighting)
                        .unwrap();
                    // The red wall is above this region, so red here must be its reflection.
                    let red_change: i64 = with[128 * 70 * 4..]
                        .chunks_exact(4)
                        .zip(without[128 * 70 * 4..].chunks_exact(4))
                        .map(|(a, b)| (a[0] as i64 - a[1] as i64) - (b[0] as i64 - b[1] as i64))
                        .sum();
                    assert!(
                        red_change > 1000,
                        "missing reflection: {red_change}; MSAA={msaa}, enhanced={enhanced}, classic={classic}, glass={glass}"
                    );
                }
            }
        }
    }

    #[test]
    fn reflection_depth_uses_panes_not_rain_decals_or_terrain() {
        let mut pane = <MaterialUniform as bytemuck::Zeroable>::zeroed();
        pane.params[0] = 2.0;
        pane.bump[2] = 1.0;
        pane.emissive[3] = 1.0;
        assert!(reflection_glass(&pane));
        for (field, value) in [(0, 2.0), (1, 1.0), (2, 1.0), (3, 1.0)] {
            let mut other = pane;
            match field {
                0 => other.emissive[3] = value,
                1 => other.bump[3] = value,
                2 => other.extra[0] = value,
                _ => other.params[1] = value,
            }
            assert!(!reflection_glass(&other));
        }
        pane.emissive[3] = 0.0;
        assert!(!reflection_glass(&pane));
        pane.params2[1] = 0.4;
        assert!(reflection_glass(&pane));
        pane.bump[2] = 0.0;
        assert!(!reflection_glass(&pane));
    }

    #[test]
    fn ray_budget_stays_bounded_at_large_and_odd_sizes() {
        for (w, h) in [
            (1, 1),
            (1279, 719),
            (1920, 1080),
            (3840, 2160),
            (7680, 4320),
            (2160, 3840),
        ] {
            let (tw, th) = trace_size(w, h);
            assert!(tw >= 1 && th >= 1 && tw <= w && th <= h);
            assert!(tw * th <= MAX_TRACE_PIXELS as u32);
            if w >= 2 && h >= 2 {
                assert!(tw <= w / 2 && th <= h / 2);
            }
        }
    }
}
#[test]
fn vehicle_capture_splits_shared_batches_without_drawing_ai_entries() {
    let batch = Batch {
        pipe: 7,
        mesh: 3,
        first: 12,
        count: 24,
        material: 4,
        instances: 0..6,
    };
    let own = [11, 12, 13, 14].into_iter().collect();
    let batches = [batch];
    let list = [11, 12, 32, 13, 45, 14];
    let selected = selected_entry_batches(&batches, &list, &own);
    let world = filter_entry_batches(&batches, &list, |entry| !own.contains(&entry));
    assert_eq!(
        world
            .iter()
            .map(|b| b.instances.clone())
            .collect::<Vec<_>>(),
        vec![2..3, 4..5]
    );
    assert_eq!(
        selected
            .iter()
            .map(|b| b.instances.clone())
            .collect::<Vec<_>>(),
        vec![0..2, 3..4, 5..6]
    );
    assert!(
        selected
            .iter()
            .all(|b| (b.pipe, b.mesh, b.first, b.count, b.material) == (7, 3, 12, 24, 4))
    );
}
