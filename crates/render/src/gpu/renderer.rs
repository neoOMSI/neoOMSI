use crate::*;

pub(crate) struct PostPipelines {
    pub(crate) down_first: wgpu::RenderPipeline,
    pub(crate) down: wgpu::RenderPipeline,
    pub(crate) up: wgpu::RenderPipeline,
    pub(crate) meter: wgpu::RenderPipeline,
    pub(crate) adapt: wgpu::RenderPipeline,
    pub(crate) tonemap: wgpu::RenderPipeline,
    pub(crate) tonemap_encoded: wgpu::RenderPipeline,
    pub(crate) fxaa: wgpu::RenderPipeline,
}

pub(crate) struct PassPipelines {
    /// Rain films are drawn after resolving the scene and its puddle reflections.
    pub(crate) rain_pipelines: Vec<wgpu::RenderPipeline>,
    pub(crate) pipelines: Vec<wgpu::RenderPipeline>,
    pub(crate) wire_pipelines: Option<Vec<wgpu::RenderPipeline>>,
    pub(crate) corona_pipeline: wgpu::RenderPipeline,
    pub(crate) smoke_pipeline: wgpu::RenderPipeline,
    /// Single-sample HDR captures have one colour attachment and no material mask.
    pub(crate) reflection_corona_pipeline: wgpu::RenderPipeline,
    pub(crate) reflection_smoke_pipeline: wgpu::RenderPipeline,
    pub(crate) sky_pipeline: wgpu::RenderPipeline,
    pub(crate) reflection_sky_pipeline: wgpu::RenderPipeline,
}

pub struct Renderer {
    pub device: wgpu::Device,
    pub queue: wgpu::Queue,
    pub adapter_name: String,
    pub(crate) camera_layout: wgpu::BindGroupLayout,
    pub(crate) material_layout: wgpu::BindGroupLayout,
    pub(crate) pass: PassPipelines,
    pub(crate) hdr_pass: Option<PassPipelines>,
    pub(crate) corona_bind_group: wgpu::BindGroup,
    pub(crate) smoke_bind_group: wgpu::BindGroup,
    pub(crate) corona_textures: Vec<Option<wgpu::BindGroup>>,
    pub(crate) corona_layout: wgpu::BindGroupLayout,
    pub(crate) corona_sampler: wgpu::Sampler,
    pub(crate) sky_layout: wgpu::BindGroupLayout,
    pub(crate) sky_sampler: wgpu::Sampler,
    pub(crate) cloud_shape_view: wgpu::TextureView,
    pub(crate) cloud_detail_view: wgpu::TextureView,
    pub(crate) cloud_sampler: wgpu::Sampler,
    pub(crate) sky_mesh: (wgpu::Buffer, wgpu::Buffer, u32),
    pub(crate) overlay_pipeline: wgpu::RenderPipeline,
    pub(crate) overlay_layout: wgpu::BindGroupLayout,
    pub(crate) sampler: wgpu::Sampler,
    pub(crate) camera_buf: wgpu::Buffer,
    pub(crate) white_texture: GpuTexture,
    pub(crate) black_texture: GpuTexture,
    pub(crate) flat_normal_texture: GpuTexture,
    pub(crate) format: wgpu::TextureFormat,
    pub(crate) depth: Option<(wgpu::Texture, wgpu::TextureView, u32, u32)>,
    pub(crate) msaa_targets: HashMap<(u32, u32), (wgpu::TextureView, wgpu::TextureView)>,
    pub(crate) ao: Option<AoTargets>,
    pub(crate) ao_sampler: wgpu::Sampler,
    pub(crate) ao_layout: wgpu::BindGroupLayout,
    pub(crate) ao_buf: wgpu::Buffer,
    pub(crate) prepass_pipelines: [wgpu::RenderPipeline; 6],
    pub(crate) prepass_msaa_pipelines: Option<[wgpu::RenderPipeline; 6]>,
    pub(crate) ssao_pipeline: Option<wgpu::RenderPipeline>,
    pub(crate) blur_pipeline: Option<wgpu::RenderPipeline>,
    pub(crate) shadow_view: wgpu::TextureView,
    pub(crate) shadow_view_far: wgpu::TextureView,
    pub(crate) shadow_sampler: wgpu::Sampler,
    pub(crate) shadow_layout: wgpu::BindGroupLayout,
    pub(crate) spot_tile: u32,
    pub(crate) spot_state: std::cell::RefCell<SpotShadowState>,
    pub(crate) spot_cam_bufs: Vec<wgpu::Buffer>,
    pub(crate) shadow_pipelines: [wgpu::RenderPipeline; 6],
    pub options: RenderOptions,
    pub(crate) hdr_targets: HashMap<(u32, u32), HdrTargets>,
    pub(crate) puddles: Option<puddles::Pipelines>,
    pub(crate) reflection_pass: Option<PassPipelines>,
    pub(crate) post: PostPipelines,
    pub(crate) post_layout: wgpu::BindGroupLayout,
    pub(crate) post_buf: wgpu::Buffer,
    pub(crate) post_sampler: wgpu::Sampler,
    pub(crate) meter_view: wgpu::TextureView,
    pub(crate) adapt_views: [wgpu::TextureView; 2],
    pub(crate) adapt_bg: [wgpu::BindGroup; 2],
    pub(crate) adapt_front: usize,
    pub(crate) exposure_log: Option<ExposureLog>,
    pub(crate) enh_buf: wgpu::Buffer,
    pub(crate) sky_lut: wgpu::Texture,
    pub(crate) sky_lut_view: wgpu::TextureView,
    pub(crate) lin_sampler: wgpu::Sampler,
    pub(crate) probe: Option<Probe>,
    pub(crate) sky_state: Option<atmosphere::SkyState>,
    pub(crate) sky_job: Option<(
        atmosphere::SkyInput,
        std::sync::mpsc::Receiver<atmosphere::SkyState>,
    )>,
    pub(crate) exposure: Option<f32>,
    pub(crate) texture_aspect: Option<f32>,
    pub(crate) last_frame: Option<std::time::Instant>,
    pub instant_exposure: bool,
    pub(crate) overlay_pipeline_1x: wgpu::RenderPipeline,
    pub(crate) xr_ui_pipeline: wgpu::RenderPipeline,
    pub(crate) started: std::time::Instant,
    pub(crate) clamp_sampler: wgpu::Sampler,
    pub(crate) mirror_sampler: wgpu::Sampler,
    pub address_next: std::cell::Cell<TexAddressing>,
    pub light_map_next: std::cell::Cell<bool>,
    pub(crate) lm_atlas: wgpu::Texture,
    pub(crate) lm_atlas_view: wgpu::TextureView,
    pub(crate) lm_uniform: wgpu::Buffer,
    pub(crate) lm_place: std::cell::Cell<(f64, f64, f64)>,
    pub(crate) mip_pipeline: wgpu::RenderPipeline,
    pub(crate) mip_layout: wgpu::BindGroupLayout,
    pub(crate) mip_sampler: wgpu::Sampler,
    pub(crate) gpu_error: Arc<std::sync::atomic::AtomicBool>,
    pub(crate) env_heading: std::cell::Cell<Option<f32>>,
    pub(crate) out_of_memory: Arc<std::sync::atomic::AtomicBool>,
    pub(crate) device_lost: Arc<std::sync::Mutex<Option<String>>>,
    pub(crate) upscale_pipeline: wgpu::RenderPipeline,
    pub(crate) copy_pipeline: wgpu::RenderPipeline,
    pub(crate) glass_snapshot_pipeline: wgpu::RenderPipeline,
    pub(crate) glass_picture_format: wgpu::TextureFormat,
    pub(crate) upscale_layout: wgpu::BindGroupLayout,
    pub(crate) upscale_buf: wgpu::Buffer,
    pub(crate) scale_targets: HashMap<(u32, u32), (wgpu::TextureView, wgpu::BindGroup)>,
    pub(crate) glass_picture: Option<wgpu::TextureView>,
    pub(crate) glass_snapshot_source: Option<(wgpu::TextureView, wgpu::BindGroup)>,
    pub(crate) target_use: HashMap<(u32, u32), std::time::Instant>,
    pub(crate) dynamic_scale: std::cell::Cell<f32>,
    pub(crate) flicker: std::cell::RefCell<HashMap<usize, bool>>,
    pub(crate) cull_drawn: std::cell::RefCell<Vec<u64>>,
    pub(crate) object_sizes: std::cell::RefCell<hashbrown::HashMap<[u64; 4], f32>>,
    pub(crate) object_sizes_scratch: std::cell::RefCell<hashbrown::HashMap<[u64; 4], f32>>,
    pub(crate) shadow_far_cache: std::cell::Cell<(Mat4, u32, DVec3, Vec3)>,
    pub(crate) shadow_near_cache: std::cell::Cell<(Mat4, u32, DVec3, Vec3)>,
    pub(crate) xr_shadow_cache: std::cell::Cell<Option<(DVec3, Vec3, Mat4, Mat4, Mat4)>>,
    pub(crate) shadow_clear_pipeline: wgpu::RenderPipeline,
    pub blend_by_origin: bool,
    pub shadow_blobs: bool,
    pub(crate) gpu_timers: [Option<GpuTimers>; 2],
    pub(crate) mip_encoder: std::cell::RefCell<Option<wgpu::CommandEncoder>>,
    pub stats: std::cell::RefCell<std::collections::BTreeMap<&'static str, f64>>,
    pub counts: std::cell::RefCell<std::collections::BTreeMap<&'static str, f64>>,
    pub(crate) profiling: bool,
    pub(crate) audit_lines: std::cell::RefCell<Vec<String>>,
    pub(crate) draw_audit_at: std::time::Instant,
    pub(crate) encoding_pool: Option<rayon::ThreadPool>,
    pub(crate) _device_poller: Option<DevicePoller>,
    pub(crate) pending_meshes: std::cell::RefCell<Vec<(MeshId, Vec<Vertex>)>>,
    pub(crate) freed: std::cell::OnceCell<Freed>,
}

impl Renderer {
    pub(crate) fn picture_group(&self, view: &wgpu::TextureView) -> wgpu::BindGroup {
        self.device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("present picture"),
            layout: &self.upscale_layout,
            entries: &[
                wgpu::BindGroupEntry {
                    binding: 0,
                    resource: self.upscale_buf.as_entire_binding(),
                },
                wgpu::BindGroupEntry {
                    binding: 1,
                    resource: wgpu::BindingResource::TextureView(view),
                },
                wgpu::BindGroupEntry {
                    binding: 2,
                    resource: wgpu::BindingResource::Sampler(&self.post_sampler),
                },
            ],
        })
    }

    pub(crate) fn main_pass(&self, enhanced: bool, reflections: bool) -> &PassPipelines {
        if enhanced {
            self.hdr_pass.as_ref().expect("enhanced pipelines")
        } else if reflections {
            self.reflection_pass.as_ref().expect("reflection pipelines")
        } else {
            &self.pass
        }
    }

    pub fn format(&self) -> wgpu::TextureFormat {
        self.format
    }

    pub fn scene_scale(&self, width: u32, height: u32) -> f32 {
        let s = scene_scale_for(self.options.render_scale, width, height);
        if self.options.render_scale > 0.0 {
            s
        } else {
            (s * self.dynamic_scale.get()).clamp(0.5, 1.0)
        }
    }

    pub fn set_dynamic_scale(&self, s: f32) {
        let s = s.clamp(0.55, 1.0);
        let level = [1.0f32, 0.85, 0.7, 0.55]
            .into_iter()
            .min_by(|a, b| (a - s).abs().total_cmp(&(b - s).abs()))
            .unwrap_or(1.0);
        self.dynamic_scale.set(level);
    }

    pub fn dynamic_scale(&self) -> f32 {
        self.dynamic_scale.get()
    }

    pub fn scene_size(&self, width: u32, height: u32) -> (u32, u32) {
        let s = self.scene_scale(width, height);
        if s >= 0.999 {
            return (width, height);
        }
        (
            ((width as f32 * s).round() as u32).max(1),
            ((height as f32 * s).round() as u32).max(1),
        )
    }

    pub(crate) fn scale_target(&mut self, w: u32, h: u32) -> (wgpu::TextureView, wgpu::BindGroup) {
        self.target_use.insert((w, h), std::time::Instant::now());
        if let Some(t) = self.scale_targets.get(&(w, h)) {
            return t.clone();
        }
        self.evict_targets();
        let tex = self.device.create_texture(&wgpu::TextureDescriptor {
            label: Some("scaled scene"),
            size: wgpu::Extent3d {
                width: w,
                height: h,
                depth_or_array_layers: 1,
            },
            mip_level_count: 1,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format: self.format,
            usage: wgpu::TextureUsages::RENDER_ATTACHMENT
                | wgpu::TextureUsages::TEXTURE_BINDING
                | wgpu::TextureUsages::COPY_SRC,
            view_formats: &[],
        });
        let view = tex.create_view(&Default::default());
        let bg = self.device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("upscale"),
            layout: &self.upscale_layout,
            entries: &[
                wgpu::BindGroupEntry {
                    binding: 0,
                    resource: self.upscale_buf.as_entire_binding(),
                },
                wgpu::BindGroupEntry {
                    binding: 1,
                    resource: wgpu::BindingResource::TextureView(&view),
                },
                wgpu::BindGroupEntry {
                    binding: 2,
                    resource: wgpu::BindingResource::Sampler(&self.post_sampler),
                },
            ],
        });
        self.scale_targets
            .insert((w, h), (view.clone(), bg.clone()));
        (view, bg)
    }
}

pub(crate) fn scene_scale_for(requested: f32, width: u32, height: u32) -> f32 {
    let pixels = width as f32 * height as f32;
    if requested >= 1.0 {
        return 1.0;
    }
    if requested > 0.0 {
        let requested = requested.clamp(0.5, 1.0);
        if (cfg!(target_os = "macos") || cfg!(target_os = "android")) && pixels > AUTO_SCALE_PIXELS
        {
            return requested.min((AUTO_SCALE_PIXELS / pixels).sqrt().clamp(0.5, 1.0));
        }
        return requested;
    }
    if pixels <= AUTO_SCALE_PIXELS {
        1.0
    } else {
        (AUTO_SCALE_PIXELS / pixels).sqrt().clamp(0.5, 1.0)
    }
}

impl Renderer {
    pub fn set_env_heading(&self, heading: Option<f32>) {
        self.env_heading.set(heading);
    }

    pub fn take_out_of_memory(&self) -> bool {
        self.out_of_memory
            .swap(false, std::sync::atomic::Ordering::Relaxed)
    }

    pub fn device_lost(&self) -> Option<String> {
        self.device_lost
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .clone()
    }
}
