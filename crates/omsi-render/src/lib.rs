pub mod atmosphere;
pub mod clouds;
#[cfg(all(feature = "devtools", debug_assertions))]
pub mod devtools;
mod materials;
mod puddles;
mod targets;
mod textures;

pub use materials::{AlphaMode, Material, MaterialExtra, PbrMaps, TexAddressing};
use materials::{BindKey, MaterialMaps, MaterialUniform};
use targets::{AoTargets, HdrTargets};
pub use textures::{GpuTexture, PreparedTexture, prepare_texture};
use textures::{next_gen, texture_bytes, upload_texture};

use anyhow::{Context, Result, anyhow};
use glam::{DVec3, Mat4, Vec3, Vec4};
use omsi_geometry::MeshData;
use std::collections::HashMap;
use std::sync::Arc;

#[repr(C)]
#[derive(Clone, Copy, bytemuck::Pod, bytemuck::Zeroable)]
pub struct Vertex {
    pub pos: [f32; 3],
    pub normal: [f32; 3],
    pub uv: [f32; 2],
}

#[repr(C)]
#[derive(Clone, Copy, bytemuck::Pod, bytemuck::Zeroable)]
struct CameraUniform {
    view_proj: [[f32; 4]; 4],
    cam_pos: [f32; 4],
    world_origin: [f32; 4],
    sun_dir: [f32; 4],
    ambient: [f32; 4],
    fog: [f32; 4],
    sun_color: [f32; 4],
    sky_color: [f32; 4],
    light_grid: [f32; 4],
    sky: [f32; 4],
    cam_right: [f32; 4],
    cam_up: [f32; 4],
    clouds: [f32; 4],
    light_view_proj: [[f32; 4]; 4],
    light_view_proj_far: [[f32; 4]; 4],
    shadow: [f32; 4],
    post: [f32; 4],
    inside_a: [f32; 4],
    inside_b: [f32; 4],
    inside_c: [f32; 4],
    flags: [f32; 4],
    light_view_proj_close: [[f32; 4]; 4],
    wind: [f32; 4],
    spot_vp: [[[f32; 4]; 4]; SPOT_SLOTS],
    spot_info: [f32; 4],
}

const CLOUD_ORIGIN_PERIOD: f64 = 70000.0;

#[repr(C)]
#[derive(Clone, Copy, bytemuck::Pod, bytemuck::Zeroable)]
struct PostUniform {
    a: [f32; 4],
    b: [f32; 4],
    c: [f32; 4],
}

#[repr(C)]
#[derive(Clone, Copy, bytemuck::Pod, bytemuck::Zeroable)]
struct EnhancedUniform {
    exposure: [f32; 4],
    sun: [f32; 4],
    sh: [[f32; 4]; 9],
    ground: [f32; 4],
    fog: [f32; 4],
    fog_color: [f32; 4],
    weather: [f32; 4],
    lights: [f32; 4],
    sun_disc: [f32; 4],
    debug: [f32; 4],
    eye: [f32; 4],
    led: [f32; 4],
}

struct Probe {
    view: wgpu::TextureView,
    faces: Vec<Vec<wgpu::TextureView>>,
    bind_groups: Vec<[wgpu::BindGroup; 2]>,
    sky_pipeline: wgpu::RenderPipeline,
    filter_pipeline: wgpu::RenderPipeline,
    age: u32,
    scale: f32,
    cube_view: wgpu::TextureView,
    cube_faces: Vec<wgpu::TextureView>,
    cube_bind_groups: Vec<wgpu::BindGroup>,
    cube_pipeline: wgpu::RenderPipeline,
    cube_next: u32,
    cube_filled: bool,
    cube_round: u32,
    cube_wait: u32,
    cube_eye: Option<DVec3>,
    cube_recapture: bool,
}

const SKY_CUBE_SIZE: u32 = 1024;
const SKY_CUBE_ROUNDS: u32 = 8;
const SKY_CUBE_HISTORY: f64 = 0.8;
const SKY_CUBE_EVERY: u32 = 4;

const PROBE_SIZE: u32 = 64;
const PROBE_MIPS: u32 = 6;
const LAMP_E: f32 = 0.0055;
const CABIN_E: f32 = 0.03;
const WINDOW_RADIANCE: f32 = 0.0022;
const METER_GAIN: f32 = 0.4;
const METER_TARGET: f32 = -2.84;
const METER_DARKEN: f32 = 0.6;
const METER_BRIGHTEN: f32 = 0.8;
const NIGHT_VISION: f32 = 0.55;
const SUN_RADIUS: f32 = 0.0065;

#[repr(C)]
#[derive(Clone, Copy, bytemuck::Pod, bytemuck::Zeroable)]
struct SsaoUniform {
    inv_proj: [[f32; 4]; 4],
    params: [f32; 4],
}

#[repr(C)]
#[derive(Clone, Copy, bytemuck::Pod, bytemuck::Zeroable)]
struct GpuPointLight {
    pos: [f32; 4],
    color: [f32; 4],
    dir: [f32; 4],
    extra: [f32; 4],
    occ: [f32; 4],
}

#[repr(C)]
#[derive(Clone, Copy, bytemuck::Pod, bytemuck::Zeroable)]
struct GpuCorona {
    pos: [f32; 3],
    size: f32,
    color: [f32; 4],
    dir: [f32; 4],
    up: [f32; 4],
    extra: [f32; 4],
}

#[derive(Debug, Clone, Copy)]
pub struct Occluder {
    pub center: glam::DVec2,
    pub half: glam::Vec2,
    pub z0: f64,
    pub z1: f64,
    pub heading: f64,
    pub tri: Option<[DVec3; 3]>,
}

#[derive(Debug, Clone, Copy)]
pub struct PointLight {
    pub position: DVec3,
    pub radius: f32,
    pub color: [f32; 3],
    pub intensity: f32,
    pub direction: Vec3,
    pub cone: [f32; 2],
    pub core: f32,
    pub beam: f32,
    pub mode: LightMode,
    pub occ_first: u32,
    pub occ_count: u32,
}

impl Default for PointLight {
    fn default() -> Self {
        Self {
            position: DVec3::ZERO,
            radius: 0.0,
            color: [1.0; 3],
            intensity: 1.0,
            direction: Vec3::ZERO,
            cone: [1.0, 0.0],
            core: 0.0,
            beam: 0.0,
            mode: LightMode::Both,
            occ_first: 0,
            occ_count: 0,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum LightMode {
    #[default]
    Both,
    Vanilla,
    Enhanced,
}

#[derive(Debug, Clone, Copy)]
pub struct SmokeParticle {
    pub position: DVec3,
    pub size: f32,
    pub color: [f32; 3],
    pub alpha: f32,
}

pub const LM_ATLAS_TILES: u32 = 5;
pub const LM_TILE_PX: u32 = 256;

#[derive(Debug, Clone, Copy)]
pub struct Corona {
    pub position: DVec3,
    pub size: f32,
    pub color: [f32; 3],
    pub brightness: f32,
    pub direction: Vec3,
    pub cone_cos: f32,
    pub inner_cos: f32,
    pub rotating: u8,
    pub up: Vec3,
    pub z_offset: f32,
    pub flags: u8,
    pub texture: u16,
    pub beam: bool,
    pub beam_width: f32,
    pub halo: bool,
}

impl Default for Corona {
    fn default() -> Self {
        Corona {
            position: DVec3::ZERO,
            size: 0.1,
            color: [1.0; 3],
            brightness: 0.0,
            direction: Vec3::ZERO,
            cone_cos: -1.0,
            inner_cos: -2.0,
            rotating: 2,
            up: Vec3::Z,
            z_offset: -1.0,
            flags: 0,
            texture: 0,
            beam: false,
            beam_width: 0.0,
            halo: false,
        }
    }
}

const LIGHT_CELL: f32 = 25.0;
const LIGHT_GRID_SIDE: usize = 64;
const LIGHT_CELL_CAP: usize = 32;

#[derive(Debug, Clone, Copy)]
pub struct Camera {
    pub position: DVec3,
    pub yaw: f32,
    pub pitch: f32,
    pub roll: f32,
    pub fov_deg: f32,
    pub near: f32,
    pub far: f32,
}

impl Camera {
    pub fn forward(&self) -> Vec3 {
        let (sy, cy) = self.yaw.to_radians().sin_cos();
        let (sp, cp) = self.pitch.to_radians().sin_cos();
        Vec3::new(sy * cp, cy * cp, sp)
    }
    pub fn right(&self) -> Vec3 {
        let f = self.forward();
        let r0 = Vec3::new(f.y, -f.x, 0.0).normalize_or_zero();
        if self.roll == 0.0 {
            return r0;
        }
        f.cross(self.up()).normalize_or(r0)
    }
    pub fn up(&self) -> Vec3 {
        let f = self.forward();
        let r0 = Vec3::new(f.y, -f.x, 0.0).normalize_or_zero();
        if self.roll == 0.0 || r0 == Vec3::ZERO {
            return Vec3::Z;
        }
        let u0 = r0.cross(f);
        let (s, c) = self.roll.to_radians().sin_cos();
        (u0 * c + r0 * s).normalize_or(Vec3::Z)
    }
    pub fn view_proj(&self, aspect: f32, origin: DVec3) -> Mat4 {
        let view = glam::camera::rh::view::look_to_mat4(
            (self.position - origin).as_vec3(),
            self.forward(),
            self.up(),
        );
        let proj = glam::camera::rh::proj::directx::perspective(
            self.fov_deg.to_radians(),
            aspect,
            self.far,
            self.near,
        );
        proj * view
    }

    pub fn ray(&self, ndc_x: f32, ndc_y: f32, aspect: f32, origin: DVec3) -> (Vec3, Vec3) {
        let inv = self.view_proj(aspect, origin).inverse();
        let p = inv.project_point3(Vec3::new(ndc_x, ndc_y, 0.0));
        let o = (self.position - origin).as_vec3();
        (o, (p - o).normalize_or_zero())
    }
}

#[derive(Clone, Debug)]
pub struct Lighting {
    pub min_obj_size: f32,
    pub sun_dir: Vec3,
    pub sun_intensity: f32,
    pub sun_color: Vec3,
    pub secondary: Vec3,
    pub ambient: Vec3,
    pub fog_color: Vec3,
    pub fog_density: f32,
    pub sky_color: Vec3,
    pub night: f32,
    pub night_maps: Option<f32>,
    pub light_shadows: bool,
    pub sun_azimuth: f32,
    pub sky_weights: [f32; 3],
    pub cloud_density: f32,
    pub cloud_offset: [f32; 2],
    pub shadows: bool,
    pub wetness: f32,
    pub snow: f32,
    pub enhanced: bool,
    pub classic: bool,
    pub inside: Option<(DVec3, f64, [f32; 6])>,
    pub puddle_ground: Option<f64>,
    pub puddle_normal: Vec3,
    pub puddle_parts: Vec<(DVec3, f64, [f32; 6])>,
    pub detail: bool,
    pub overcast: f32,
    pub rain: f32,
    pub fog_base: Option<f64>,
    pub envir_tint: [Vec3; 3],
    pub led_glow: f32,
    pub atmosphere_brightness: f32,
    pub led_mips: f32,
    pub glass_wind: Vec3,
    pub animation_time: Option<f32>,
}

impl Lighting {
    pub fn casts_sun_shadows(&self) -> bool {
        self.shadows
            && self.sun_dir.normalize_or_zero().z > -0.02
            && self.sun_intensity > 0.05
            && omsi_cfg::env::var_os("OMSI_NO_SHADOWS").is_none()
    }
}

impl Default for Lighting {
    fn default() -> Self {
        Self {
            min_obj_size: 0.0,
            sun_dir: Vec3::new(0.3, 0.2, 0.9).normalize(),
            sun_intensity: 0.9,
            sun_color: Vec3::ONE,
            secondary: Vec3::splat(0.15),
            ambient: Vec3::splat(0.25),
            fog_color: Vec3::new(0.70, 0.78, 0.90),
            fog_density: 0.0006,
            sky_color: Vec3::new(0.55, 0.70, 0.92),
            night: 0.0,
            night_maps: None,
            sun_azimuth: 0.0,
            sky_weights: [1.0, 0.0, 0.0],
            cloud_density: 0.0,
            cloud_offset: [0.0; 2],
            shadows: true,
            light_shadows: true,
            wetness: 0.0,
            snow: 0.0,
            enhanced: false,
            classic: false,
            inside: None,
            puddle_ground: None,
            puddle_normal: Vec3::Z,
            puddle_parts: Vec::new(),
            detail: true,
            overcast: 0.0,
            rain: 0.0,
            fog_base: None,
            envir_tint: [Vec3::ONE; 3],
            led_glow: 1.5,
            atmosphere_brightness: 1.0,
            led_mips: 1.3,
            glass_wind: Vec3::ZERO,
            animation_time: None,
        }
    }
}

pub type MeshId = usize;
pub type TextureId = usize;
pub type MaterialId = usize;

pub struct GpuMesh {
    vertex_buf: wgpu::Buffer,
    index_buf: wgpu::Buffer,
    pub ranges: Vec<(u32, u32, u32)>,
    pub bounds_center: Vec3,
    pub bounds_radius: f32,
    pub one_sided: bool,
    pub source: Option<String>,
}

type GlassKey = (bool, u32, u32);

#[derive(Clone, Copy, Default)]
struct InstanceBounds {
    centre: Vec3,
    radius: f32,
    scale: f32,
}

impl InstanceBounds {
    fn new(mesh: &GpuMesh, transform: Mat4) -> Self {
        let scale = transform_scale(transform);
        Self {
            centre: transform.transform_point3(mesh.bounds_center),
            radius: mesh.bounds_radius * scale,
            scale,
        }
    }
}

fn transform_scale(transform: Mat4) -> f32 {
    transform
        .x_axis
        .truncate()
        .length_squared()
        .max(transform.y_axis.truncate().length_squared())
        .max(transform.z_axis.truncate().length_squared())
        .sqrt()
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
#[repr(u8)]
pub enum RenderPhase {
    PreSurface = 0,
    Terrain = 1,
    Surface = 2,
    Spline = 3,
    OnSurface = 4,
    BeforeNormal = 5,
    #[default]
    Normal = 6,
    AfterNormal = 7,
    AfterVehicles = 8,
}

impl RenderPhase {
    const COUNT: usize = 9;
    const DRAW_ORDER: [Self; Self::COUNT] = [
        Self::PreSurface,
        Self::Terrain,
        Self::Surface,
        Self::Spline,
        Self::OnSurface,
        Self::BeforeNormal,
        Self::Normal,
        Self::AfterNormal,
        Self::AfterVehicles,
    ];
}

pub struct Instance {
    pub mesh: MeshId,
    pub transform: Mat4,
    pub origin: DVec3,
    pub materials: Vec<MaterialId>,
    pub slot_alpha: Vec<f32>,
    pub slot_light: Vec<f32>,
    pub slot_night: Vec<f32>,
    pub visible: bool,
    pub slot_uv: Vec<[f32; 2]>,
    pub interior: f32,
    pub interior_lamps: u32,
    base: u32,
    bounds: InstanceBounds,
    pub surface: bool,
    pub presurface: bool,
    pub render_phase: RenderPhase,
    pub surface_bias: bool,
    pub blend_sort_origin: Option<DVec3>,
    pub lod: (f32, f32),
    pub blob: bool,
    pub ground_layer: bool,
    pub decal: bool,
    pub object_radius: f32,
    pub detail: f32,
    pub any_distance: bool,
    pub near_only: Option<[f64; 4]>,
    pub mirror_only: bool,
    pub omsi_caster: bool,
    pub casts_shadow: bool,
    pub roof: Option<f32>,
    pub ordered: bool,
}

pub struct Scene {
    pub meshes: Vec<GpuMesh>,
    pub textures: Vec<GpuTexture>,
    glass_slot: Option<TextureId>,
    glass_key: Option<GlassKey>,
    pub materials: Vec<Material>,
    pub instances: Vec<Instance>,
    pub render_origin: DVec3,
    pub lights: Vec<PointLight>,
    pub occluders: Vec<Occluder>,
    pub interior_lights: Vec<PointLight>,
    interior_free: Vec<(u32, u32)>,
    pub coronas: Vec<Corona>,
    pub smoke: Vec<SmokeParticle>,
    smoke_buf: Option<wgpu::Buffer>,
    smoke_count: u32,
    corona_runs: Vec<(u16, u32, u32)>,
    model_buf: Option<wgpu::Buffer>,
    params_buf: Option<wgpu::Buffer>,
    light_buf: Option<wgpu::Buffer>,
    grid_buf: Option<wgpu::Buffer>,
    corona_buf: Option<wgpu::Buffer>,
    corona_count: u32,
    draw_buf: Option<wgpu::Buffer>,
    camera_bind_group: Option<wgpu::BindGroup>,
    shadow_bind_group: Option<wgpu::BindGroup>,
    spot_bind_groups: Vec<wgpu::BindGroup>,
    sky_bind_group: Option<wgpu::BindGroup>,
    pub overlays: Vec<(TextureId, [f32; 4])>,
    pub premultiplied: std::collections::HashSet<TextureId>,
    overlay_res: Vec<(TextureId, wgpu::Buffer, wgpu::BindGroup, [f32; 8])>,
    dirty: bool,
    uploaded_instances: usize,
    uploaded_entries: u32,
    changed: Vec<usize>,
    changed_mark: Vec<bool>,
    cache_bounds: bool,
    bounds_meshes: Vec<bool>,
    bounds_dirty: bool,
    cpu_models: Vec<[[f32; 4]; 4]>,
    cpu_params: Vec<[f32; 4]>,
    last_grid: Vec<u32>,
    last_lights: Vec<u8>,
    bind_groups: HashMap<BindKey, (wgpu::BindGroup, wgpu::Buffer)>,
    pub pbr_maps: HashMap<TextureId, PbrMaps>,
    pub snow_textures: std::collections::HashSet<TextureId>,
}

impl Scene {
    pub fn gpu_bytes(&self) -> (u64, u64, u64) {
        let tex = self.textures.iter().map(|t| t.bytes).sum();
        let mut seen: std::collections::HashSet<*const ()> = std::collections::HashSet::new();
        let mut mesh = 0u64;
        for m in &self.meshes {
            if m.ranges.is_empty() {
                continue;
            }
            for b in [&m.vertex_buf, &m.index_buf] {
                if seen.insert(b as *const wgpu::Buffer as *const ()) {
                    mesh += b.size();
                }
            }
        }
        let other = [
            &self.model_buf,
            &self.params_buf,
            &self.light_buf,
            &self.grid_buf,
            &self.corona_buf,
            &self.draw_buf,
        ]
            .iter()
            .filter_map(|b| b.as_ref())
            .map(|b| b.size())
            .sum();
        (tex, mesh, other)
    }
}

struct PostPipelines {
    down_first: wgpu::RenderPipeline,
    down: wgpu::RenderPipeline,
    up: wgpu::RenderPipeline,
    meter: wgpu::RenderPipeline,
    adapt: wgpu::RenderPipeline,
    tonemap: wgpu::RenderPipeline,
    tonemap_encoded: wgpu::RenderPipeline,
    fxaa: wgpu::RenderPipeline,
}

struct PassPipelines {
    pipelines: Vec<wgpu::RenderPipeline>,
    wire_pipelines: Option<Vec<wgpu::RenderPipeline>>,
    corona_pipeline: wgpu::RenderPipeline,
    smoke_pipeline: wgpu::RenderPipeline,
    sky_pipeline: wgpu::RenderPipeline,
}

pub static ADAPTER_TEXTURE_MB: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);

static GL_BACKEND: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);

pub fn gl_backend() -> bool {
    GL_BACKEND.load(std::sync::atomic::Ordering::Relaxed)
}

pub fn wait_gpu(
    device: &wgpu::Device,
    submission: Option<wgpu::SubmissionIndex>,
) -> Result<(), wgpu::PollError> {
    if !gl_backend() {
        return device
            .poll(wgpu::PollType::Wait {
                submission_index: submission,
                timeout: None,
            })
            .map(|_| ());
    }
    loop {
        match device.poll(wgpu::PollType::Wait {
            submission_index: submission.clone(),
            timeout: Some(GL_WAIT_SLICE),
        }) {
            Err(wgpu::PollError::Timeout) => std::thread::yield_now(),
            r => return r.map(|_| ()),
        }
    }
}

const GL_WAIT_SLICE: std::time::Duration = std::time::Duration::from_millis(20);

fn gl_worker_turn() -> Option<std::sync::MutexGuard<'static, ()>> {
    static TURN: std::sync::Mutex<()> = std::sync::Mutex::new(());
    gl_backend().then(|| TURN.lock().unwrap_or_else(|e| e.into_inner()))
}

fn dedicated_vram_mb(info: &wgpu::AdapterInfo) -> Option<u64> {
    #[cfg(windows)]
    unsafe {
        use windows::Win32::Graphics::Dxgi::{CreateDXGIFactory1, IDXGIFactory1};
        let f: IDXGIFactory1 = CreateDXGIFactory1().ok()?;
        let mut i = 0;
        while let Ok(a) = f.EnumAdapters1(i) {
            i += 1;
            let Ok(d) = a.GetDesc1() else { continue };
            if d.VendorId == info.vendor && d.DeviceId == info.device {
                return Some(d.DedicatedVideoMemory as u64 >> 20);
            }
        }
        None
    }
    #[cfg(not(windows))]
    {
        let _ = info;
        None
    }
}

pub struct Renderer {
    pub device: wgpu::Device,
    pub queue: wgpu::Queue,
    pub adapter_name: String,
    camera_layout: wgpu::BindGroupLayout,
    material_layout: wgpu::BindGroupLayout,
    pass: PassPipelines,
    hdr_pass: Option<PassPipelines>,
    corona_bind_group: wgpu::BindGroup,
    smoke_bind_group: wgpu::BindGroup,
    corona_textures: Vec<Option<wgpu::BindGroup>>,
    corona_layout: wgpu::BindGroupLayout,
    corona_sampler: wgpu::Sampler,
    sky_layout: wgpu::BindGroupLayout,
    sky_sampler: wgpu::Sampler,
    cloud_shape_view: wgpu::TextureView,
    cloud_detail_view: wgpu::TextureView,
    cloud_sampler: wgpu::Sampler,
    sky_mesh: (wgpu::Buffer, wgpu::Buffer, u32),
    overlay_pipeline: wgpu::RenderPipeline,
    overlay_layout: wgpu::BindGroupLayout,
    sampler: wgpu::Sampler,
    camera_buf: wgpu::Buffer,
    white_texture: GpuTexture,
    black_texture: GpuTexture,
    flat_normal_texture: GpuTexture,
    format: wgpu::TextureFormat,
    depth: Option<(wgpu::Texture, wgpu::TextureView, u32, u32)>,
    msaa_targets: HashMap<(u32, u32), (wgpu::TextureView, wgpu::TextureView)>,
    ao: Option<AoTargets>,
    ao_sampler: wgpu::Sampler,
    ao_layout: wgpu::BindGroupLayout,
    ao_buf: wgpu::Buffer,
    prepass_pipelines: [wgpu::RenderPipeline; 6],
    prepass_msaa_pipelines: Option<[wgpu::RenderPipeline; 6]>,
    ssao_pipeline: Option<wgpu::RenderPipeline>,
    blur_pipeline: Option<wgpu::RenderPipeline>,
    shadow_view: wgpu::TextureView,
    shadow_view_far: wgpu::TextureView,
    shadow_sampler: wgpu::Sampler,
    shadow_layout: wgpu::BindGroupLayout,
    spot_tile: u32,
    spot_state: std::cell::RefCell<SpotShadowState>,
    spot_cam_bufs: Vec<wgpu::Buffer>,
    shadow_pipelines: [wgpu::RenderPipeline; 6],
    pub options: RenderOptions,
    hdr_targets: HashMap<(u32, u32), HdrTargets>,
    puddles: Option<puddles::Pipelines>,
    post: PostPipelines,
    post_layout: wgpu::BindGroupLayout,
    post_buf: wgpu::Buffer,
    post_sampler: wgpu::Sampler,
    meter_view: wgpu::TextureView,
    adapt_views: [wgpu::TextureView; 2],
    adapt_bg: [wgpu::BindGroup; 2],
    adapt_front: usize,
    exposure_log: Option<ExposureLog>,
    enh_buf: wgpu::Buffer,
    sky_lut: wgpu::Texture,
    sky_lut_view: wgpu::TextureView,
    lin_sampler: wgpu::Sampler,
    probe: Option<Probe>,
    sky_state: Option<atmosphere::SkyState>,
    sky_job: Option<(
        atmosphere::SkyInput,
        std::sync::mpsc::Receiver<atmosphere::SkyState>,
    )>,
    exposure: Option<f32>,
    texture_aspect: Option<f32>,
    last_frame: Option<std::time::Instant>,
    pub instant_exposure: bool,
    overlay_pipeline_1x: wgpu::RenderPipeline,
    xr_ui_pipeline: wgpu::RenderPipeline,
    started: std::time::Instant,
    clamp_sampler: wgpu::Sampler,
    mirror_sampler: wgpu::Sampler,
    pub address_next: std::cell::Cell<TexAddressing>,
    pub light_map_next: std::cell::Cell<bool>,
    lm_atlas: wgpu::Texture,
    lm_atlas_view: wgpu::TextureView,
    lm_uniform: wgpu::Buffer,
    lm_place: std::cell::Cell<(f64, f64, f64)>,
    mip_pipeline: wgpu::RenderPipeline,
    mip_layout: wgpu::BindGroupLayout,
    mip_sampler: wgpu::Sampler,
    gpu_error: Arc<std::sync::atomic::AtomicBool>,
    env_heading: std::cell::Cell<Option<f32>>,
    out_of_memory: Arc<std::sync::atomic::AtomicBool>,
    device_lost: Arc<std::sync::Mutex<Option<String>>>,
    upscale_pipeline: wgpu::RenderPipeline,
    upscale_layout: wgpu::BindGroupLayout,
    upscale_buf: wgpu::Buffer,
    scale_targets: HashMap<(u32, u32), (wgpu::TextureView, wgpu::BindGroup)>,
    glass_prev: Option<(wgpu::TextureView, (u32, u32))>,
    glass_live: Option<GlassKey>,
    target_use: HashMap<(u32, u32), std::time::Instant>,
    dynamic_scale: std::cell::Cell<f32>,
    flicker: std::cell::RefCell<HashMap<usize, bool>>,
    cull_drawn: std::cell::RefCell<Vec<u64>>,
    object_sizes: std::cell::RefCell<hashbrown::HashMap<[u64; 4], f32>>,
    object_sizes_scratch: std::cell::RefCell<hashbrown::HashMap<[u64; 4], f32>>,
    shadow_far_cache: std::cell::Cell<(Mat4, u32, DVec3, Vec3)>,
    shadow_near_cache: std::cell::Cell<(Mat4, u32, DVec3, Vec3)>,
    xr_shadow_cache: std::cell::Cell<Option<(DVec3, Vec3, Mat4, Mat4, Mat4)>>,
    shadow_clear_pipeline: wgpu::RenderPipeline,
    pub blend_by_origin: bool,
    pub shadow_blobs: bool,
    gpu_timers: [Option<GpuTimers>; 2],
    pub stats: std::cell::RefCell<std::collections::BTreeMap<&'static str, f64>>,
    pub counts: std::cell::RefCell<std::collections::BTreeMap<&'static str, f64>>,
    profiling: bool,
    draw_audit_at: std::time::Instant,
    encoding_pool: Option<rayon::ThreadPool>,
    _device_poller: Option<DevicePoller>,
    pending_meshes: std::cell::RefCell<Vec<(MeshId, Vec<u8>)>>,
    freed: std::cell::OnceCell<Freed>,
}

struct Freed {
    vertex_buf: wgpu::Buffer,
    index_buf: wgpu::Buffer,
    bind_group: wgpu::BindGroup,
    buf: wgpu::Buffer,
}

pub const DEPTH_FORMAT: wgpu::TextureFormat = wgpu::TextureFormat::Depth32Float;
pub const MSAA: u32 = 4;
pub const SHADOW_SIZE: u32 = 2048;

#[derive(Debug, Clone, Copy)]
pub struct RenderOptions {
    pub msaa: u32,
    pub anisotropy: u16,
    pub shadow_size: u32,
    pub ssao: bool,
    pub render_scale: f32,
    pub compress_textures: bool,
    pub fxaa: bool,
    pub min_obj_size: f32,
    pub max_obj_dist: f32,
    pub omsi_shadow_casters: bool,
    pub shadow_blobs: bool,
    pub reflections: bool,
    pub no_enhanced: bool,
}

impl Default for RenderOptions {
    fn default() -> Self {
        Self {
            msaa: MSAA,
            anisotropy: 8,
            shadow_size: SHADOW_SIZE,
            ssao: true,
            render_scale: 0.0,
            compress_textures: true,
            fxaa: true,
            min_obj_size: 0.013,
            max_obj_dist: 0.0,
            omsi_shadow_casters: false,
            shadow_blobs: true,
            reflections: true,
            no_enhanced: false,
        }
    }
}

pub const AUTO_SCALE_PIXELS: f32 = if cfg!(target_os = "macos") || cfg!(target_os = "android") {
    2_800_000.0
} else {
    8_400_000.0
};

pub const MAX_LAMPS_PER_MESH: u32 = 63;
pub const LAMP_CODE_STRIDE: u32 = 64;

const MASK_FORMAT: wgpu::TextureFormat = wgpu::TextureFormat::Rgba8Unorm;
const HDR_FORMAT: wgpu::TextureFormat = wgpu::TextureFormat::Rgba16Float;

fn color_targets(
    format: wgpu::TextureFormat,
    blend: Option<wgpu::BlendState>,
    write: wgpu::ColorWrites,
    mask: bool,
) -> Vec<Option<wgpu::ColorTargetState>> {
    let mut v = vec![Some(wgpu::ColorTargetState {
        format,
        blend,
        write_mask: write,
    })];
    if format == HDR_FORMAT {
        v.push(Some(wgpu::ColorTargetState {
            format: MASK_FORMAT,
            blend: blend.map(|_| wgpu::BlendState {
                color: wgpu::BlendComponent {
                    src_factor: wgpu::BlendFactor::SrcAlpha,
                    dst_factor: wgpu::BlendFactor::OneMinusSrcAlpha,
                    operation: wgpu::BlendOperation::Add,
                },
                alpha: wgpu::BlendComponent::REPLACE,
            }),
            write_mask: if mask {
                wgpu::ColorWrites::ALL
            } else {
                wgpu::ColorWrites::empty()
            },
        }));
    }
    v
}
pub const SHADOW_RANGE: f32 = 140.0;
pub const SHADOW_RANGE_FAR: f32 = 700.0;
pub const SHADOW_RANGE_CLOSE: f32 = 32.0;
const SHADOW_CLOSE_MAX: u32 = 2048;

const SPOT_SLOTS: usize = 8;
const SPOT_DRAWS_PER_FRAME: usize = 2;
const SPOT_REDRAW_AGE: u32 = 24;
const SPOT_CAM_RANGE: f64 = 70.0;
const SPOT_RANGE_MAX: f32 = 45.0;
const SPOT_NEAR: f32 = 0.8;
const SHADOW_SETS: usize = 3 + SPOT_SLOTS;

#[derive(Clone, Copy)]
struct SpotPose {
    pos: DVec3,
    dir: Vec3,
    fov: f32,
    far: f32,
}

#[derive(Clone, Copy, Default)]
struct SpotSlot {
    seen: Option<SpotPose>,
    drawn: Option<SpotPose>,
    age: u32,
}

#[derive(Default)]
struct SpotShadowState {
    slots: [SpotSlot; SPOT_SLOTS],
    draws: Vec<usize>,
}

fn spot_view_proj(pos: Vec3, dir: Vec3, fov: f32, near: f32, far: f32) -> Mat4 {
    let f = dir.normalize_or_zero();
    let hint = if f.z.abs() > 0.95 { Vec3::Y } else { Vec3::Z };
    let r = f.cross(hint).normalize_or_zero();
    let u = r.cross(f);
    let view = Mat4::from_cols(
        glam::Vec4::new(r.x, u.x, -f.x, 0.0),
        glam::Vec4::new(r.y, u.y, -f.y, 0.0),
        glam::Vec4::new(r.z, u.z, -f.z, 0.0),
        glam::Vec4::new(-r.dot(pos), -u.dot(pos), f.dot(pos), 1.0),
    );
    let t = 1.0 / (fov * 0.5).tan();
    let proj = Mat4::from_cols(
        glam::Vec4::new(t, 0.0, 0.0, 0.0),
        glam::Vec4::new(0.0, t, 0.0, 0.0),
        glam::Vec4::new(0.0, 0.0, far / (near - far), -1.0),
        glam::Vec4::new(0.0, 0.0, near * far / (near - far), 0.0),
    );
    proj * view
}

const FOG_MIN_DENSITY: f32 = 5e-4;

impl Renderer {
    pub async fn new(
        instance: &wgpu::Instance,
        surface: Option<&wgpu::Surface<'_>>,
        format: Option<wgpu::TextureFormat>,
    ) -> Result<Renderer> {
        Self::new_with(instance, surface, format, RenderOptions::default()).await
    }

    pub async fn new_with(
        instance: &wgpu::Instance,
        surface: Option<&wgpu::Surface<'_>>,
        format: Option<wgpu::TextureFormat>,
        options: RenderOptions,
    ) -> Result<Renderer> {
        let adapter = instance
            .request_adapter(&wgpu::RequestAdapterOptions { power_preference: wgpu::PowerPreference::HighPerformance, compatible_surface: surface, force_fallback_adapter: false, ..Default::default() })
            .await
            .map_err(|e| anyhow!("no graphics adapter that can draw the game was found (Metal, Vulkan, DirectX 12 or OpenGL 3.3 or later); updating the graphics driver often helps: {e}"))?;
        Self::new_on(adapter, surface, format, options).await
    }

    pub fn adapters_for(
        instance: &wgpu::Instance,
        surface: &wgpu::Surface<'_>,
    ) -> Vec<wgpu::Adapter> {
        let mut v: Vec<wgpu::Adapter> =
            pollster::block_on(instance.enumerate_adapters(wgpu::Backends::all()))
                .into_iter()
                .filter(|a| a.is_surface_supported(surface))
                .collect();
        let rank = |a: &wgpu::Adapter| match a.get_info().device_type {
            wgpu::DeviceType::DiscreteGpu => 0,
            wgpu::DeviceType::IntegratedGpu => 1,
            wgpu::DeviceType::VirtualGpu | wgpu::DeviceType::Other => 2,
            wgpu::DeviceType::Cpu => 3,
        };
        v.sort_by_key(rank);
        v
    }

    pub async fn new_on(
        adapter: wgpu::Adapter,
        surface: Option<&wgpu::Surface<'_>>,
        format: Option<wgpu::TextureFormat>,
        options: RenderOptions,
    ) -> Result<Renderer> {
        let info = adapter.get_info();
        match omsi_cfg::env::var("OMSI_FAKE_GPU_ERROR").as_deref() {
            Ok("open") => {
                return Err(anyhow!(
                    "test: {} refused (OMSI_FAKE_GPU_ERROR=open)",
                    info.name
                ));
            }
            Ok("open-panic") => panic!(
                "test: {} went down while being opened (OMSI_FAKE_GPU_ERROR=open-panic)",
                info.name
            ),
            _ => {}
        }
        let vram = dedicated_vram_mb(&info);
        let guess_mb: u64 = match info.device_type {
            wgpu::DeviceType::DiscreteGpu => vram.filter(|v| *v >= 512).map_or(1600, |v| {
                if v <= 2560 {
                    v * 35 / 100
                } else {
                    (v * 55 / 100).min(6000)
                }
            }),
            wgpu::DeviceType::IntegratedGpu if info.backend == wgpu::Backend::Metal => 3000,
            wgpu::DeviceType::IntegratedGpu | wgpu::DeviceType::VirtualGpu => 1000,
            _ => 800,
        };
        ADAPTER_TEXTURE_MB.store(guess_mb, std::sync::atomic::Ordering::Relaxed);
        log::info!(
            "graphics adapter: {} ({:?}, {:?}{}), texture memory taken for it: {guess_mb} MB",
            info.name,
            info.device_type,
            info.backend,
            vram.map(|v| format!(", {v} MB of its own"))
                .unwrap_or_default()
        );
        let intel_vulkan_safe = cfg!(windows)
            && info.backend == wgpu::Backend::Vulkan
            && info.vendor == 0x8086
            && omsi_cfg::env::var_os("OMSI_INTEL_FULL_GPU").is_none();
        let options = if intel_vulkan_safe {
            log::warn!(
                "Intel Vulkan adapter detected ({}): using the stable driver profile (1x MSAA, 1x anisotropy, SSAO and runtime texture compression off); set OMSI_INTEL_FULL_GPU=1 after updating the Intel driver to retry the requested settings",
                info.name
            );
            RenderOptions {
                msaa: 1,
                anisotropy: 1,
                ssao: false,
                compress_textures: false,
                ..options
            }
        } else {
            options
        };
        GL_BACKEND.store(
            info.backend == wgpu::Backend::Gl,
            std::sync::atomic::Ordering::Relaxed,
        );
        let full = omsi_cfg::env::var_os("OMSI_FULL_GPU").is_some();
        let weak = !full
            && (info.backend == wgpu::Backend::Gl
            || cfg!(target_os = "android")
            || (info.device_type == wgpu::DeviceType::IntegratedGpu && info.backend != wgpu::Backend::Metal)
            || vram.is_some_and(|v| v <= 2560));
        let modest = !full && !weak && vram.is_some_and(|v| v <= 4200);
        let options = if weak {
            log::warn!(
                "{}: a small or shared graphics chip - no SSAO, no MSAA, shadow maps of at most 1024 (OMSI_FULL_GPU=1 keeps the settings)",
                info.name
            );
            RenderOptions {
                msaa: 1,
                ssao: false,
                shadow_size: options.shadow_size.min(1024),
                ..options
            }
        } else if modest {
            log::info!(
                "{}: {} MB of its own - no SSAO, at most 2x MSAA and 2048 shadow maps (OMSI_FULL_GPU=1 keeps the settings)",
                info.name,
                vram.unwrap_or(0)
            );
            RenderOptions {
                msaa: options.msaa.min(2),
                ssao: false,
                shadow_size: options.shadow_size.min(2048),
                ..options
            }
        } else {
            options
        };
        let shadow_size = options
            .shadow_size
            .clamp(512, if intel_vulkan_safe { 2048 } else { 8192 });
        let mut limits = wgpu::Limits::default().using_resolution(adapter.limits());
        if intel_vulkan_safe {
            limits = wgpu::Limits::default().using_resolution(adapter.limits());
        } else {
            limits.max_storage_buffer_binding_size =
                adapter.limits().max_storage_buffer_binding_size;
            limits.max_buffer_size = adapter.limits().max_buffer_size;
        }
        if !limits.check_limits(&adapter.limits()) {
            log::warn!(
                "{}: below the standard limits; using what it has",
                info.name
            );
            limits = adapter.limits();
        }
        match omsi_cfg::env::var("OMSI_GPU_LIMITS").as_deref() {
            Ok("default") => limits = wgpu::Limits::default(),
            Ok("downlevel") => limits = wgpu::Limits::downlevel_defaults(),
            _ => {}
        }
        let shadow_size = shadow_size
            .min(limits.max_texture_dimension_2d / 2)
            .max(256);
        let format = format
            .or_else(|| {
                surface.map(|s| {
                    let formats = s.get_capabilities(&adapter).formats;
                    if cfg!(target_os = "android") {
                        if let Some(f) = formats.iter().find(|f| f.is_srgb()) {
                            return *f;
                        }
                    }
                    formats[0]
                })
            })
            .unwrap_or(wgpu::TextureFormat::Rgba8UnormSrgb);
        let wanted = match options.msaa {
            1 | 2 | 4 | 8 => options.msaa,
            _ => MSAA,
        };
        let targets = [
            format,
            wgpu::TextureFormat::Rgba16Float,
            DEPTH_FORMAT,
            MASK_FORMAT,
        ];
        let takes = |flags: wgpu::TextureFormatFeatureFlags, f: wgpu::TextureFormat, n: u32| {
            flags.sample_count_supported(n)
                && (n == 1
                || f.is_depth_stencil_format()
                || flags.contains(wgpu::TextureFormatFeatureFlags::MULTISAMPLE_RESOLVE))
        };
        let adapter_table_needed = !targets.iter().all(|&f| {
            takes(
                f.guaranteed_format_features(wgpu::Features::empty()).flags,
                f,
                wanted,
            )
        });
        let mut required_features = if adapter_table_needed && info.backend != wgpu::Backend::Noop {
            adapter.features() & wgpu::Features::TEXTURE_ADAPTER_SPECIFIC_FORMAT_FEATURES
        } else {
            wgpu::Features::empty()
        };
        if omsi_cfg::env::var_os("OMSI_GPU_TIMERS").is_some() {
            required_features |= adapter.features() & wgpu::Features::TIMESTAMP_QUERY;
        }
        if omsi_cfg::env::var_os("OMSI_NO_BC").is_none() {
            required_features |= adapter.features() & wgpu::Features::TEXTURE_COMPRESSION_BC;
        }
        #[cfg(all(feature = "devtools", debug_assertions))]
        {
            required_features |= adapter.features() & wgpu::Features::POLYGON_MODE_LINE;
        }
        if intel_vulkan_safe {
            required_features = wgpu::Features::empty();
        }
        log::info!(
            "opening graphics device: {} ({:?}, vendor {:#06x}, device {:#06x}), features {:?}, max buffer {} MB, max storage binding {} MB",
            info.name,
            info.backend,
            info.vendor,
            info.device,
            required_features,
            limits.max_buffer_size / 1_000_000,
            limits.max_storage_buffer_binding_size as u64 / 1_000_000
        );
        let (device, queue) = adapter
            .request_device(&wgpu::DeviceDescriptor {
                label: Some("omsi"),
                required_features,
                required_limits: limits,
                memory_hints: if weak || modest || vram.is_some_and(|v| v <= 4200) {
                    wgpu::MemoryHints::MemoryUsage
                } else {
                    wgpu::MemoryHints::Performance
                },
                ..Default::default()
            })
            .await
            .context("request_device")?;
        log::info!("graphics device opened; compiling renderer pipelines");
        let adapter_table = device
            .features()
            .contains(wgpu::Features::TEXTURE_ADAPTER_SPECIFIC_FORMAT_FEATURES)
            || !adapter
            .get_downlevel_capabilities()
            .flags
            .contains(wgpu::DownlevelFlags::WEBGPU_TEXTURE_FORMAT_SUPPORT);
        let flags_of = |f: wgpu::TextureFormat| {
            if adapter_table {
                adapter.get_texture_format_features(f).flags
            } else {
                f.guaranteed_format_features(device.features()).flags
            }
        };
        let supported = |n: u32| targets.iter().all(|&f| takes(flags_of(f), f, n));
        let msaa = [wanted, 8, 4, 2, 1]
            .into_iter()
            .filter(|&n| n <= wanted)
            .find(|&n| supported(n))
            .unwrap_or(1);
        if msaa != wanted {
            log::warn!(
                "{}x MSAA is not supported by {} (the device takes {:?} samples for {:?}, {:?} for the HDR target, {:?} for depth); using {}x",
                wanted,
                info.name,
                flags_of(format).supported_sample_counts(),
                format,
                flags_of(targets[1]).supported_sample_counts(),
                flags_of(DEPTH_FORMAT).supported_sample_counts(),
                msaa
            );
        }
        let options = RenderOptions {
            msaa,
            shadow_size,
            anisotropy: options.anisotropy.clamp(1, 16),
            ..options
        };
        let bc = device
            .features()
            .contains(wgpu::Features::TEXTURE_COMPRESSION_BC);
        let compress = bc
            && options.compress_textures
            && omsi_cfg::env::var_os("OMSI_NO_TEXCOMPRESS").is_none();
        omsi_texture::set_gpu_options(omsi_texture::GpuOptions { bc, compress });
        log::info!(
            "renderer: {} ({:?}), {:?}, {}x MSAA{}, anisotropy {}, shadow map {}, SSAO {}, render scale {}, textures {}",
            info.name,
            info.backend,
            format,
            options.msaa,
            if adapter_table {
                " (adapter format table)"
            } else {
                ""
            },
            options.anisotropy,
            options.shadow_size,
            options.ssao,
            if options.render_scale > 0.0 {
                format!("{:.2}", options.render_scale.clamp(0.5, 1.0))
            } else {
                "auto".to_string()
            },
            match (bc, compress) {
                (false, _) => "RGBA (no BC on this device)",
                (true, false) => "DXT as blocks, others RGBA",
                (true, true) => "DXT as blocks, others compressed where close",
            }
        );
        let scope = device.push_error_scope(wgpu::ErrorFilter::Validation);
        let renderer = Self::build(
            device.clone(),
            queue.clone(),
            format!("{} ({:?})", info.name, info.backend),
            format,
            options,
        );
        match scope.pop().await {
            None => Ok(renderer),
            Some(e) if options.msaa > 1 => {
                log::error!(
                    "{}x MSAA failed on {}: {}; drawing without multisampling",
                    options.msaa,
                    info.name,
                    gpu_error_text(&e)
                );
                drop(renderer);
                Ok(Self::build(
                    device,
                    queue,
                    format!("{} ({:?})", info.name, info.backend),
                    format,
                    RenderOptions { msaa: 1, ..options },
                ))
            }
            Some(e) => Err(anyhow!("renderer pipelines: {}", gpu_error_text(&e))),
        }
    }

    fn build(
        device: wgpu::Device,
        queue: wgpu::Queue,
        adapter_name: String,
        format: wgpu::TextureFormat,
        options: RenderOptions,
    ) -> Renderer {
        let (msaa, shadow_size) = (options.msaa, options.shadow_size);
        let gpu_error = Arc::new(std::sync::atomic::AtomicBool::new(false));
        let out_of_memory = Arc::new(std::sync::atomic::AtomicBool::new(false));
        let device_lost: Arc<std::sync::Mutex<Option<String>>> = Default::default();
        {
            let lost = device_lost.clone();
            device.set_device_lost_callback(move |reason, message| {
                if matches!(reason, wgpu::DeviceLostReason::Destroyed) {
                    return;
                }
                log::error!("the graphics device was lost ({reason:?}): {message}");
                *lost.lock().unwrap_or_else(|e| e.into_inner()) =
                    Some(format!("{reason:?}: {message}"));
            });
        }
        {
            let flag = gpu_error.clone();
            let oom = out_of_memory.clone();
            let count = Arc::new(std::sync::atomic::AtomicU64::new(0));
            device.on_uncaptured_error(Arc::new(move |e: wgpu::Error| {
                if matches!(e, wgpu::Error::OutOfMemory { .. }) {
                    oom.store(true, std::sync::atomic::Ordering::Relaxed);
                }
                let n = count.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
                if msaa > 1 && !flag.swap(true, std::sync::atomic::Ordering::Relaxed) {
                    log::error!(
                        "GPU error with {msaa}x MSAA (drawing without it from now on): {}",
                        gpu_error_text(&e)
                    );
                } else if n < 20 || n % 1000 == 0 {
                    log::error!(
                        "GPU error #{} (the game goes on): {}",
                        n + 1,
                        gpu_error_text(&e)
                    );
                }
            }));
        }
        if omsi_cfg::env::var("OMSI_FAKE_GPU_ERROR").as_deref() == Ok("build") && msaa > 1 {
            let _ = device.create_texture(&wgpu::TextureDescriptor {
                label: Some("invalid"),
                size: wgpu::Extent3d {
                    width: 4,
                    height: 4,
                    depth_or_array_layers: 1,
                },
                mip_level_count: 1,
                sample_count: 3,
                dimension: wgpu::TextureDimension::D2,
                format,
                usage: wgpu::TextureUsages::RENDER_ATTACHMENT,
                view_formats: &[],
            });
        }
        log::info!("renderer: compiling the scene shaders");
        let shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("omsi"),
            source: wgpu::ShaderSource::Wgsl(
                scene_shader_source(GL_BACKEND.load(std::sync::atomic::Ordering::Relaxed)).into(),
            ),
        });
        let shadow_layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("shadow camera"),
            entries: &[
                wgpu::BindGroupLayoutEntry {
                    binding: 0,
                    visibility: wgpu::ShaderStages::VERTEX_FRAGMENT,
                    ty: wgpu::BindingType::Buffer {
                        ty: wgpu::BufferBindingType::Uniform,
                        has_dynamic_offset: false,
                        min_binding_size: None,
                    },
                    count: None,
                },
                wgpu::BindGroupLayoutEntry {
                    binding: 1,
                    visibility: wgpu::ShaderStages::VERTEX,
                    ty: wgpu::BindingType::Buffer {
                        ty: wgpu::BufferBindingType::Storage { read_only: true },
                        has_dynamic_offset: false,
                        min_binding_size: None,
                    },
                    count: None,
                },
                wgpu::BindGroupLayoutEntry {
                    binding: 2,
                    visibility: wgpu::ShaderStages::VERTEX,
                    ty: wgpu::BindingType::Buffer {
                        ty: wgpu::BufferBindingType::Storage { read_only: true },
                        has_dynamic_offset: false,
                        min_binding_size: None,
                    },
                    count: None,
                },
                wgpu::BindGroupLayoutEntry {
                    binding: 10,
                    visibility: wgpu::ShaderStages::VERTEX,
                    ty: wgpu::BindingType::Buffer {
                        ty: wgpu::BufferBindingType::Storage { read_only: true },
                        has_dynamic_offset: false,
                        min_binding_size: None,
                    },
                    count: None,
                },
            ],
        });
        let camera_layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("camera"),
            entries: &[
                wgpu::BindGroupLayoutEntry {
                    binding: 0,
                    visibility: wgpu::ShaderStages::VERTEX_FRAGMENT,
                    ty: wgpu::BindingType::Buffer {
                        ty: wgpu::BufferBindingType::Uniform,
                        has_dynamic_offset: false,
                        min_binding_size: None,
                    },
                    count: None,
                },
                wgpu::BindGroupLayoutEntry {
                    binding: 1,
                    visibility: wgpu::ShaderStages::VERTEX,
                    ty: wgpu::BindingType::Buffer {
                        ty: wgpu::BufferBindingType::Storage { read_only: true },
                        has_dynamic_offset: false,
                        min_binding_size: None,
                    },
                    count: None,
                },
                wgpu::BindGroupLayoutEntry {
                    binding: 2,
                    visibility: wgpu::ShaderStages::VERTEX,
                    ty: wgpu::BindingType::Buffer {
                        ty: wgpu::BufferBindingType::Storage { read_only: true },
                        has_dynamic_offset: false,
                        min_binding_size: None,
                    },
                    count: None,
                },
                wgpu::BindGroupLayoutEntry {
                    binding: 3,
                    visibility: wgpu::ShaderStages::FRAGMENT,
                    ty: wgpu::BindingType::Buffer {
                        ty: wgpu::BufferBindingType::Storage { read_only: true },
                        has_dynamic_offset: false,
                        min_binding_size: None,
                    },
                    count: None,
                },
                wgpu::BindGroupLayoutEntry {
                    binding: 4,
                    visibility: wgpu::ShaderStages::FRAGMENT,
                    ty: wgpu::BindingType::Buffer {
                        ty: wgpu::BufferBindingType::Storage { read_only: true },
                        has_dynamic_offset: false,
                        min_binding_size: None,
                    },
                    count: None,
                },
                wgpu::BindGroupLayoutEntry {
                    binding: 5,
                    visibility: wgpu::ShaderStages::FRAGMENT,
                    ty: wgpu::BindingType::Texture {
                        sample_type: wgpu::TextureSampleType::Depth,
                        view_dimension: wgpu::TextureViewDimension::D2,
                        multisampled: false,
                    },
                    count: None,
                },
                wgpu::BindGroupLayoutEntry {
                    binding: 6,
                    visibility: wgpu::ShaderStages::FRAGMENT,
                    ty: wgpu::BindingType::Sampler(wgpu::SamplerBindingType::Comparison),
                    count: None,
                },
                wgpu::BindGroupLayoutEntry {
                    binding: 7,
                    visibility: wgpu::ShaderStages::FRAGMENT,
                    ty: wgpu::BindingType::Texture {
                        sample_type: wgpu::TextureSampleType::Depth,
                        view_dimension: wgpu::TextureViewDimension::D2,
                        multisampled: false,
                    },
                    count: None,
                },
                wgpu::BindGroupLayoutEntry {
                    binding: 8,
                    visibility: wgpu::ShaderStages::FRAGMENT,
                    ty: wgpu::BindingType::Texture {
                        sample_type: wgpu::TextureSampleType::Float { filterable: true },
                        view_dimension: wgpu::TextureViewDimension::D2,
                        multisampled: false,
                    },
                    count: None,
                },
                wgpu::BindGroupLayoutEntry {
                    binding: 9,
                    visibility: wgpu::ShaderStages::FRAGMENT,
                    ty: wgpu::BindingType::Sampler(wgpu::SamplerBindingType::Filtering),
                    count: None,
                },
                wgpu::BindGroupLayoutEntry {
                    binding: 10,
                    visibility: wgpu::ShaderStages::VERTEX,
                    ty: wgpu::BindingType::Buffer {
                        ty: wgpu::BufferBindingType::Storage { read_only: true },
                        has_dynamic_offset: false,
                        min_binding_size: None,
                    },
                    count: None,
                },
                wgpu::BindGroupLayoutEntry {
                    binding: 11,
                    visibility: wgpu::ShaderStages::VERTEX_FRAGMENT,
                    ty: wgpu::BindingType::Buffer {
                        ty: wgpu::BufferBindingType::Uniform,
                        has_dynamic_offset: false,
                        min_binding_size: None,
                    },
                    count: None,
                },
                wgpu::BindGroupLayoutEntry {
                    binding: 12,
                    visibility: wgpu::ShaderStages::FRAGMENT,
                    ty: wgpu::BindingType::Texture {
                        sample_type: wgpu::TextureSampleType::Float { filterable: true },
                        view_dimension: wgpu::TextureViewDimension::Cube,
                        multisampled: false,
                    },
                    count: None,
                },
                wgpu::BindGroupLayoutEntry {
                    binding: 13,
                    visibility: wgpu::ShaderStages::FRAGMENT,
                    ty: wgpu::BindingType::Sampler(wgpu::SamplerBindingType::Filtering),
                    count: None,
                },
                wgpu::BindGroupLayoutEntry {
                    binding: 14,
                    visibility: wgpu::ShaderStages::FRAGMENT,
                    ty: wgpu::BindingType::Texture {
                        sample_type: wgpu::TextureSampleType::Float { filterable: true },
                        view_dimension: wgpu::TextureViewDimension::D2,
                        multisampled: false,
                    },
                    count: None,
                },
                wgpu::BindGroupLayoutEntry {
                    binding: 17,
                    visibility: wgpu::ShaderStages::FRAGMENT,
                    ty: wgpu::BindingType::Texture {
                        sample_type: wgpu::TextureSampleType::Float { filterable: true },
                        view_dimension: wgpu::TextureViewDimension::Cube,
                        multisampled: false,
                    },
                    count: None,
                },
                wgpu::BindGroupLayoutEntry {
                    binding: 18,
                    visibility: wgpu::ShaderStages::FRAGMENT,
                    ty: wgpu::BindingType::Texture {
                        sample_type: wgpu::TextureSampleType::Float { filterable: true },
                        view_dimension: wgpu::TextureViewDimension::D2,
                        multisampled: false,
                    },
                    count: None,
                },
                wgpu::BindGroupLayoutEntry {
                    binding: 19,
                    visibility: wgpu::ShaderStages::FRAGMENT,
                    ty: wgpu::BindingType::Buffer {
                        ty: wgpu::BufferBindingType::Uniform,
                        has_dynamic_offset: false,
                        min_binding_size: None,
                    },
                    count: None,
                },
            ],
        });
        let lm_atlas = device.create_texture(&wgpu::TextureDescriptor {
            label: Some("light map atlas"),
            size: wgpu::Extent3d {
                width: LM_ATLAS_TILES * LM_TILE_PX,
                height: LM_ATLAS_TILES * LM_TILE_PX,
                depth_or_array_layers: 1,
            },
            mip_level_count: 1,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format: wgpu::TextureFormat::Rgba8UnormSrgb,
            usage: wgpu::TextureUsages::TEXTURE_BINDING | wgpu::TextureUsages::COPY_DST,
            view_formats: &[],
        });
        let lm_uniform = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("light map atlas place"),
            size: 16,
            usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });
        let material_layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("material"),
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
                wgpu::BindGroupLayoutEntry {
                    binding: 2,
                    visibility: wgpu::ShaderStages::VERTEX_FRAGMENT,
                    ty: wgpu::BindingType::Buffer {
                        ty: wgpu::BufferBindingType::Uniform,
                        has_dynamic_offset: false,
                        min_binding_size: None,
                    },
                    count: None,
                },
                wgpu::BindGroupLayoutEntry {
                    binding: 3,
                    visibility: wgpu::ShaderStages::FRAGMENT,
                    ty: wgpu::BindingType::Texture {
                        sample_type: wgpu::TextureSampleType::Float { filterable: true },
                        view_dimension: wgpu::TextureViewDimension::D2,
                        multisampled: false,
                    },
                    count: None,
                },
                wgpu::BindGroupLayoutEntry {
                    binding: 4,
                    visibility: wgpu::ShaderStages::FRAGMENT,
                    ty: wgpu::BindingType::Texture {
                        sample_type: wgpu::TextureSampleType::Float { filterable: true },
                        view_dimension: wgpu::TextureViewDimension::D2,
                        multisampled: false,
                    },
                    count: None,
                },
                wgpu::BindGroupLayoutEntry {
                    binding: 5,
                    visibility: wgpu::ShaderStages::FRAGMENT,
                    ty: wgpu::BindingType::Texture {
                        sample_type: wgpu::TextureSampleType::Float { filterable: true },
                        view_dimension: wgpu::TextureViewDimension::D2,
                        multisampled: false,
                    },
                    count: None,
                },
                wgpu::BindGroupLayoutEntry {
                    binding: 6,
                    visibility: wgpu::ShaderStages::FRAGMENT,
                    ty: wgpu::BindingType::Texture {
                        sample_type: wgpu::TextureSampleType::Float { filterable: true },
                        view_dimension: wgpu::TextureViewDimension::D2,
                        multisampled: false,
                    },
                    count: None,
                },
                wgpu::BindGroupLayoutEntry {
                    binding: 7,
                    visibility: wgpu::ShaderStages::FRAGMENT,
                    ty: wgpu::BindingType::Texture {
                        sample_type: wgpu::TextureSampleType::Float { filterable: true },
                        view_dimension: wgpu::TextureViewDimension::D2,
                        multisampled: false,
                    },
                    count: None,
                },
                wgpu::BindGroupLayoutEntry {
                    binding: 8,
                    visibility: wgpu::ShaderStages::FRAGMENT,
                    ty: wgpu::BindingType::Texture {
                        sample_type: wgpu::TextureSampleType::Float { filterable: true },
                        view_dimension: wgpu::TextureViewDimension::D2,
                        multisampled: false,
                    },
                    count: None,
                },
                wgpu::BindGroupLayoutEntry {
                    binding: 9,
                    visibility: wgpu::ShaderStages::FRAGMENT,
                    ty: wgpu::BindingType::Texture {
                        sample_type: wgpu::TextureSampleType::Float { filterable: true },
                        view_dimension: wgpu::TextureViewDimension::D2,
                        multisampled: false,
                    },
                    count: None,
                },
                wgpu::BindGroupLayoutEntry {
                    binding: 10,
                    visibility: wgpu::ShaderStages::FRAGMENT,
                    ty: wgpu::BindingType::Texture {
                        sample_type: wgpu::TextureSampleType::Float { filterable: true },
                        view_dimension: wgpu::TextureViewDimension::D2,
                        multisampled: false,
                    },
                    count: None,
                },
                wgpu::BindGroupLayoutEntry {
                    binding: 11,
                    visibility: wgpu::ShaderStages::FRAGMENT,
                    ty: wgpu::BindingType::Sampler(wgpu::SamplerBindingType::Filtering),
                    count: None,
                },
            ],
        });
        let corona_layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("corona"),
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
            label: Some("omsi"),
            bind_group_layouts: &[Some(&camera_layout), Some(&material_layout)],
            immediate_size: 0,
        });
        let vertex_layout = wgpu::VertexBufferLayout {
            array_stride: std::mem::size_of::<Vertex>() as u64,
            step_mode: wgpu::VertexStepMode::Vertex,
            attributes: &wgpu::vertex_attr_array![0 => Float32x3, 1 => Float32x3, 2 => Float32x2],
        };
        let wire_flag = std::cell::Cell::new(false);
        let make = |format: wgpu::TextureFormat,
                    fs: &str,
                    blend: Option<wgpu::BlendState>,
                    depth_write: bool,
                    cull: bool,
                    bias: i32,
                    alpha_to_coverage: bool| {
            let use_alpha_to_coverage = alpha_to_coverage && msaa > 1;
            device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
                label: Some("omsi"),
                layout: Some(&layout),
                vertex: wgpu::VertexState {
                    module: &shader,
                    entry_point: Some("vs_main"),
                    buffers: &[Some(vertex_layout.clone())],
                    compilation_options: Default::default(),
                },
                primitive: {
                    let mut p = one_sided_primitive(cull);
                    if wire_flag.get() {
                        p.polygon_mode = wgpu::PolygonMode::Line;
                    }
                    p
                },
                depth_stencil: Some(wgpu::DepthStencilState {
                    format: DEPTH_FORMAT,
                    depth_write_enabled: Some(depth_write),
                    depth_compare: Some(wgpu::CompareFunction::GreaterEqual),
                    stencil: Default::default(),
                    bias: wgpu::DepthBiasState {
                        constant: -bias,
                        slope_scale: if bias != 0 {
                            -bias.signum() as f32 * 2.0
                        } else {
                            0.0
                        },
                        clamp: 0.0,
                    },
                }),
                multisample: wgpu::MultisampleState {
                    count: msaa,
                    mask: !0,
                    alpha_to_coverage_enabled: use_alpha_to_coverage,
                },
                fragment: Some(wgpu::FragmentState {
                    module: &shader,
                    entry_point: Some(fs),
                    targets: &color_targets(
                        format,
                        blend,
                        if fs == "fs_surface_depth" {
                            wgpu::ColorWrites::empty()
                        } else {
                            wgpu::ColorWrites::ALL
                        },
                        fs != "fs_surface_depth",
                    ),
                    compilation_options: wgpu::PipelineCompilationOptions {
                        constants: &[
                            ("ALPHA_TEST", if alpha_to_coverage { 1.0 } else { 0.0 }),
                            (
                                "ALPHA_TO_COVERAGE",
                                if use_alpha_to_coverage { 1.0 } else { 0.0 },
                            ),
                        ],
                        ..Default::default()
                    },
                }),
                multiview_mask: None,
                cache: None,
            })
        };
        let bias: i32 = omsi_cfg::env::var("OMSI_SURFACE_BIAS")
            .ok()
            .and_then(|v| v.parse().ok())
            .unwrap_or(-24);
        let scene_pipelines = |f: wgpu::TextureFormat, fs: &str| -> Vec<wgpu::RenderPipeline> {
            let mut out = Vec::with_capacity(PIPE_KINDS as usize * 4);
            for kind in 0..PIPE_KINDS {
                let blend = (kind == PIPE_BLEND || kind == PIPE_BLEND_NO_WRITE)
                    .then_some(wgpu::BlendState::ALPHA_BLENDING);
                let depth_write = kind != PIPE_BLEND_NO_WRITE;
                for cull in [false, true] {
                    for surface in [false, true] {
                        out.push(make(
                            f,
                            if kind == PIPE_SURFACE_DEPTH {
                                "fs_surface_depth"
                            } else {
                                fs
                            },
                            blend,
                            depth_write,
                            cull,
                            if surface { bias } else { 0 },
                            kind == PIPE_ALPHA_TEST,
                        ));
                    }
                }
            }
            out
        };
        let hdr_format = wgpu::TextureFormat::Rgba16Float;
        let spot_tile = (shadow_size / 4).clamp(128, 512);
        let shadow_tex = device.create_texture(&wgpu::TextureDescriptor {
            label: Some("shadow map"),
            size: wgpu::Extent3d {
                width: shadow_size * 2,
                height: shadow_size,
                depth_or_array_layers: 1,
            },
            mip_level_count: 1,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format: DEPTH_FORMAT,
            usage: wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::TEXTURE_BINDING,
            view_formats: &[],
        });
        let shadow_view = shadow_tex.create_view(&Default::default());
        let shadow_tex_far = device.create_texture(&wgpu::TextureDescriptor {
            label: Some("shadow map far"),
            size: wgpu::Extent3d {
                width: shadow_size,
                height: shadow_size + 2 * spot_tile,
                depth_or_array_layers: 1,
            },
            mip_level_count: 1,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format: DEPTH_FORMAT,
            usage: wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::TEXTURE_BINDING,
            view_formats: &[],
        });
        let shadow_view_far = shadow_tex_far.create_view(&Default::default());
        let shadow_sampler = device.create_sampler(&wgpu::SamplerDescriptor {
            address_mode_u: wgpu::AddressMode::ClampToEdge,
            address_mode_v: wgpu::AddressMode::ClampToEdge,
            mag_filter: wgpu::FilterMode::Linear,
            min_filter: wgpu::FilterMode::Linear,
            compare: Some(wgpu::CompareFunction::LessEqual),
            ..Default::default()
        });
        let shadow_pipeline_layout =
            device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
                label: Some("shadow"),
                bind_group_layouts: &[Some(&shadow_layout), Some(&material_layout)],
                immediate_size: 0,
            });
        let make_shadow = |kind: u8, cascade: u8| {
            device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
                label: Some("shadow"),
                layout: Some(&shadow_pipeline_layout),
                vertex: wgpu::VertexState {
                    module: &shader,
                    entry_point: Some(match cascade {
                        0 => "vs_shadow",
                        1 => "vs_shadow_far",
                        _ => "vs_shadow_close",
                    }),
                    buffers: &[Some(vertex_layout.clone())],
                    compilation_options: Default::default(),
                },
                primitive: wgpu::PrimitiveState {
                    topology: wgpu::PrimitiveTopology::TriangleList,
                    cull_mode: None,
                    front_face: wgpu::FrontFace::Cw,
                    ..Default::default()
                },
                depth_stencil: Some(wgpu::DepthStencilState {
                    format: DEPTH_FORMAT,
                    depth_write_enabled: Some(true),
                    depth_compare: Some(wgpu::CompareFunction::LessEqual),
                    stencil: Default::default(),
                    bias: wgpu::DepthBiasState {
                        constant: 4,
                        slope_scale: 3.0,
                        clamp: 0.0,
                    },
                }),
                multisample: Default::default(),
                fragment: Some(wgpu::FragmentState {
                    module: &shader,
                    entry_point: Some(if kind == PIPE_ALPHA_TEST {
                        "fs_shadow_test"
                    } else {
                        "fs_shadow"
                    }),
                    targets: &[],
                    compilation_options: Default::default(),
                }),
                multiview_mask: None,
                cache: None,
            })
        };
        let shadow_clear_pipeline = {
            log::info!("renderer: compiling the shadow clear shader");
            let module = device.create_shader_module(wgpu::ShaderModuleDescriptor {
                label: Some("shadow clear"),
                source: wgpu::ShaderSource::Wgsl(
                    "@vertex fn vs(@builtin(vertex_index) i: u32) -> @builtin(position) vec4<f32> {
                        let x = f32(i32(i & 1u) * 4 - 1);
                        let y = f32(i32(i >> 1u) * 4 - 1);
                        return vec4<f32>(x, y, 1.0, 1.0);
                    }"
                        .into(),
                ),
            });
            let layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
                label: Some("shadow clear"),
                bind_group_layouts: &[],
                immediate_size: 0,
            });
            device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
                label: Some("shadow clear"),
                layout: Some(&layout),
                vertex: wgpu::VertexState {
                    module: &module,
                    entry_point: Some("vs"),
                    buffers: &[],
                    compilation_options: Default::default(),
                },
                primitive: wgpu::PrimitiveState::default(),
                depth_stencil: Some(wgpu::DepthStencilState {
                    format: DEPTH_FORMAT,
                    depth_write_enabled: Some(true),
                    depth_compare: Some(wgpu::CompareFunction::Always),
                    stencil: Default::default(),
                    bias: Default::default(),
                }),
                multisample: Default::default(),
                fragment: None,
                multiview_mask: None,
                cache: None,
            })
        };
        let shadow_pipelines = [
            make_shadow(PIPE_OPAQUE, 0),
            make_shadow(PIPE_ALPHA_TEST, 0),
            make_shadow(PIPE_OPAQUE, 1),
            make_shadow(PIPE_ALPHA_TEST, 1),
            make_shadow(PIPE_OPAQUE, 2),
            make_shadow(PIPE_ALPHA_TEST, 2),
        ];
        let sampler = device.create_sampler(&wgpu::SamplerDescriptor {
            address_mode_u: wgpu::AddressMode::Repeat,
            address_mode_v: wgpu::AddressMode::Repeat,
            address_mode_w: wgpu::AddressMode::Repeat,
            mag_filter: wgpu::FilterMode::Linear,
            min_filter: wgpu::FilterMode::Linear,
            mipmap_filter: wgpu::MipmapFilterMode::Linear,
            anisotropy_clamp: options.anisotropy,
            ..Default::default()
        });
        let clamp_sampler = device.create_sampler(&wgpu::SamplerDescriptor {
            address_mode_u: wgpu::AddressMode::ClampToEdge,
            address_mode_v: wgpu::AddressMode::ClampToEdge,
            address_mode_w: wgpu::AddressMode::ClampToEdge,
            mag_filter: wgpu::FilterMode::Linear,
            min_filter: wgpu::FilterMode::Linear,
            mipmap_filter: wgpu::MipmapFilterMode::Linear,
            anisotropy_clamp: options.anisotropy,
            ..Default::default()
        });
        let mirror_sampler = device.create_sampler(&wgpu::SamplerDescriptor {
            address_mode_u: wgpu::AddressMode::MirrorRepeat,
            address_mode_v: wgpu::AddressMode::MirrorRepeat,
            address_mode_w: wgpu::AddressMode::MirrorRepeat,
            mag_filter: wgpu::FilterMode::Linear,
            min_filter: wgpu::FilterMode::Linear,
            mipmap_filter: wgpu::MipmapFilterMode::Linear,
            anisotropy_clamp: options.anisotropy,
            ..Default::default()
        });
        let camera_buf = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("camera"),
            size: std::mem::size_of::<CameraUniform>() as u64,
            usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });
        let spot_cam_bufs: Vec<wgpu::Buffer> = (0..SPOT_SLOTS)
            .map(|_| {
                device.create_buffer(&wgpu::BufferDescriptor {
                    label: Some("spot shadow camera"),
                    size: std::mem::size_of::<CameraUniform>() as u64,
                    usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
                    mapped_at_creation: false,
                })
            })
            .collect();
        let white = omsi_texture::Image::solid([255, 255, 255, 255]);
        let white_texture = upload_texture(&device, &queue, &white, false);
        let black_texture = upload_texture(
            &device,
            &queue,
            &omsi_texture::Image::solid([0, 0, 0, 255]),
            false,
        );
        let flat_normal_texture = upload_texture(
            &device,
            &queue,
            &omsi_texture::Image::solid([128, 128, 255, 255]),
            false,
        );
        let cs = 64u32;
        let mut corona_img = omsi_texture::Image {
            width: cs,
            height: cs,
            rgba: vec![0; (cs * cs * 4) as usize],
            has_alpha: false,
        };
        for y in 0..cs {
            for x in 0..cs {
                let dx = (x as f32 + 0.5) / cs as f32 * 2.0 - 1.0;
                let dy = (y as f32 + 0.5) / cs as f32 * 2.0 - 1.0;
                let r = (dx * dx + dy * dy).sqrt();
                let v = ((1.0 - r).max(0.0)).powf(1.6) * 255.0;
                let o = ((y * cs + x) * 4) as usize;
                corona_img.rgba[o..o + 4].copy_from_slice(&[v as u8, v as u8, v as u8, 255]);
            }
        }
        let corona_texture = upload_texture(&device, &queue, &corona_img, false);
        let corona_sampler = device.create_sampler(&wgpu::SamplerDescriptor {
            address_mode_u: wgpu::AddressMode::ClampToEdge,
            address_mode_v: wgpu::AddressMode::ClampToEdge,
            mag_filter: wgpu::FilterMode::Linear,
            min_filter: wgpu::FilterMode::Linear,
            ..Default::default()
        });
        let corona_bind_group = device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("corona"),
            layout: &corona_layout,
            entries: &[
                wgpu::BindGroupEntry {
                    binding: 0,
                    resource: wgpu::BindingResource::TextureView(&corona_texture.view),
                },
                wgpu::BindGroupEntry {
                    binding: 1,
                    resource: wgpu::BindingResource::Sampler(&corona_sampler),
                },
            ],
        });
        let mut puff = omsi_texture::Image {
            width: cs,
            height: cs,
            rgba: vec![0; (cs * cs * 4) as usize],
            has_alpha: true,
        };
        for y in 0..cs {
            for x in 0..cs {
                let dx = (x as f32 + 0.5) / cs as f32 * 2.0 - 1.0;
                let dy = (y as f32 + 0.5) / cs as f32 * 2.0 - 1.0;
                let a = (1.0 - (dx * dx + dy * dy).sqrt()).max(0.0).powf(1.2) * 255.0;
                let o = ((y * cs + x) * 4) as usize;
                puff.rgba[o..o + 4].copy_from_slice(&[255, 255, 255, a as u8]);
            }
        }
        let puff_texture = upload_texture(&device, &queue, &puff, false);
        let smoke_bind_group = device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("smoke"),
            layout: &corona_layout,
            entries: &[
                wgpu::BindGroupEntry {
                    binding: 0,
                    resource: wgpu::BindingResource::TextureView(&puff_texture.view),
                },
                wgpu::BindGroupEntry {
                    binding: 1,
                    resource: wgpu::BindingResource::Sampler(&corona_sampler),
                },
            ],
        });
        drop(puff_texture);
        log::info!("renderer: compiling the coronas shaders");
        let corona_shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("corona"),
            source: wgpu::ShaderSource::Wgsl(corona_shader_source().into()),
        });
        let corona_pl = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("corona"),
            bind_group_layouts: &[Some(&camera_layout), Some(&corona_layout)],
            immediate_size: 0,
        });
        let corona_vertex = wgpu::VertexBufferLayout {
            array_stride: std::mem::size_of::<GpuCorona>() as u64,
            step_mode: wgpu::VertexStepMode::Instance,
            attributes: &wgpu::vertex_attr_array![0 => Float32x3, 1 => Float32, 2 => Float32x4, 3 => Float32x4, 4 => Float32x4, 5 => Float32x4],
        };
        let additive = wgpu::BlendState {
            color: wgpu::BlendComponent {
                src_factor: wgpu::BlendFactor::One,
                dst_factor: wgpu::BlendFactor::One,
                operation: wgpu::BlendOperation::Add,
            },
            alpha: wgpu::BlendComponent::REPLACE,
        };
        let screen = wgpu::BlendState {
            color: wgpu::BlendComponent {
                src_factor: wgpu::BlendFactor::One,
                dst_factor: wgpu::BlendFactor::OneMinusSrc,
                operation: wgpu::BlendOperation::Add,
            },
            alpha: wgpu::BlendComponent::REPLACE,
        };
        let alpha_blend = wgpu::BlendState {
            color: wgpu::BlendComponent {
                src_factor: wgpu::BlendFactor::SrcAlpha,
                dst_factor: wgpu::BlendFactor::OneMinusSrcAlpha,
                operation: wgpu::BlendOperation::Add,
            },
            alpha: wgpu::BlendComponent::REPLACE,
        };
        let corona_pipeline_for = |f: wgpu::TextureFormat, fs: &str, blend: wgpu::BlendState| {
            device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
                label: Some("corona"),
                layout: Some(&corona_pl),
                vertex: wgpu::VertexState {
                    module: &corona_shader,
                    entry_point: Some("vs_main"),
                    buffers: &[Some(corona_vertex.clone())],
                    compilation_options: Default::default(),
                },
                primitive: wgpu::PrimitiveState {
                    topology: wgpu::PrimitiveTopology::TriangleList,
                    cull_mode: None,
                    front_face: wgpu::FrontFace::Ccw,
                    ..Default::default()
                },
                depth_stencil: Some(wgpu::DepthStencilState {
                    format: DEPTH_FORMAT,
                    depth_write_enabled: Some(false),
                    depth_compare: Some(wgpu::CompareFunction::GreaterEqual),
                    stencil: Default::default(),
                    bias: Default::default(),
                }),
                multisample: wgpu::MultisampleState {
                    count: msaa,
                    mask: !0,
                    alpha_to_coverage_enabled: false,
                },
                fragment: Some(wgpu::FragmentState {
                    module: &corona_shader,
                    entry_point: Some(fs),
                    targets: &color_targets(f, Some(blend), wgpu::ColorWrites::COLOR, false),
                    compilation_options: Default::default(),
                }),
                multiview_mask: None,
                cache: None,
            })
        };
        drop(corona_texture);
        let sky_layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("sky"),
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
                    ty: wgpu::BindingType::Texture {
                        sample_type: wgpu::TextureSampleType::Float { filterable: true },
                        view_dimension: wgpu::TextureViewDimension::D2,
                        multisampled: false,
                    },
                    count: None,
                },
                wgpu::BindGroupLayoutEntry {
                    binding: 3,
                    visibility: wgpu::ShaderStages::FRAGMENT,
                    ty: wgpu::BindingType::Sampler(wgpu::SamplerBindingType::Filtering),
                    count: None,
                },
                wgpu::BindGroupLayoutEntry {
                    binding: 4,
                    visibility: wgpu::ShaderStages::FRAGMENT,
                    ty: wgpu::BindingType::Texture {
                        sample_type: wgpu::TextureSampleType::Float { filterable: true },
                        view_dimension: wgpu::TextureViewDimension::D2,
                        multisampled: false,
                    },
                    count: None,
                },
                wgpu::BindGroupLayoutEntry {
                    binding: 5,
                    visibility: wgpu::ShaderStages::FRAGMENT,
                    ty: wgpu::BindingType::Sampler(wgpu::SamplerBindingType::Filtering),
                    count: None,
                },
                wgpu::BindGroupLayoutEntry {
                    binding: 6,
                    visibility: wgpu::ShaderStages::FRAGMENT,
                    ty: wgpu::BindingType::Texture {
                        sample_type: wgpu::TextureSampleType::Float { filterable: true },
                        view_dimension: wgpu::TextureViewDimension::D2,
                        multisampled: false,
                    },
                    count: None,
                },
                wgpu::BindGroupLayoutEntry {
                    binding: 7,
                    visibility: wgpu::ShaderStages::FRAGMENT,
                    ty: wgpu::BindingType::Texture {
                        sample_type: wgpu::TextureSampleType::Float { filterable: true },
                        view_dimension: wgpu::TextureViewDimension::D3,
                        multisampled: false,
                    },
                    count: None,
                },
                wgpu::BindGroupLayoutEntry {
                    binding: 8,
                    visibility: wgpu::ShaderStages::FRAGMENT,
                    ty: wgpu::BindingType::Sampler(wgpu::SamplerBindingType::Filtering),
                    count: None,
                },
            ],
        });
        let (cloud_shape_view, cloud_detail_view, cloud_sampler) =
            cloud_noise_textures(&device, &queue);
        log::info!("renderer: compiling the sky and clouds shaders");
        let sky_shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("sky"),
            source: wgpu::ShaderSource::Wgsl(sky_shader_source().into()),
        });
        let sky_pl = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("sky"),
            bind_group_layouts: &[Some(&camera_layout), Some(&sky_layout)],
            immediate_size: 0,
        });
        let sky_vertex = wgpu::VertexBufferLayout {
            array_stride: 12,
            step_mode: wgpu::VertexStepMode::Vertex,
            attributes: &wgpu::vertex_attr_array![0 => Float32x3],
        };
        let sky_pipeline_for = |f: wgpu::TextureFormat, fs: &str| {
            device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
                label: Some("sky"),
                layout: Some(&sky_pl),
                vertex: wgpu::VertexState {
                    module: &sky_shader,
                    entry_point: Some("vs_main"),
                    buffers: &[Some(sky_vertex.clone())],
                    compilation_options: Default::default(),
                },
                primitive: wgpu::PrimitiveState {
                    topology: wgpu::PrimitiveTopology::TriangleList,
                    cull_mode: None,
                    front_face: wgpu::FrontFace::Ccw,
                    ..Default::default()
                },
                depth_stencil: Some(wgpu::DepthStencilState {
                    format: DEPTH_FORMAT,
                    depth_write_enabled: Some(false),
                    depth_compare: Some(wgpu::CompareFunction::Always),
                    stencil: Default::default(),
                    bias: Default::default(),
                }),
                multisample: wgpu::MultisampleState {
                    count: msaa,
                    mask: !0,
                    alpha_to_coverage_enabled: false,
                },
                fragment: Some(wgpu::FragmentState {
                    module: &sky_shader,
                    entry_point: Some(fs),
                    targets: &color_targets(f, None, wgpu::ColorWrites::COLOR, false),
                    compilation_options: Default::default(),
                }),
                multiview_mask: None,
                cache: None,
            })
        };
        let wire_ok = cfg!(all(feature = "devtools", debug_assertions))
            && device.features().contains(wgpu::Features::POLYGON_MODE_LINE);
        let wire_for = |f: wgpu::TextureFormat, fs: &str| -> Option<Vec<wgpu::RenderPipeline>> {
            if !wire_ok {
                return None;
            }
            wire_flag.set(true);
            let v = scene_pipelines(f, fs);
            wire_flag.set(false);
            Some(v)
        };
        #[cfg(all(feature = "devtools", debug_assertions))]
        devtools::set_wireframe_supported(wire_ok);
        let pass = PassPipelines {
            pipelines: scene_pipelines(format, "fs_main"),
            wire_pipelines: wire_for(format, "fs_main"),
            corona_pipeline: corona_pipeline_for(format, "fs_main", screen),
            smoke_pipeline: corona_pipeline_for(format, "fs_smoke", alpha_blend),
            sky_pipeline: sky_pipeline_for(format, "fs_main"),
        };
        let leave_out_enhanced = options.no_enhanced
            && (cfg!(target_os = "android")
            || adapter_name.to_ascii_lowercase().contains("opengl")
            || GL_BACKEND.load(std::sync::atomic::Ordering::Relaxed));
        let hdr_pass = (!leave_out_enhanced).then(|| PassPipelines {
            pipelines: scene_pipelines(hdr_format, "fs_enhanced"),
            wire_pipelines: wire_for(hdr_format, "fs_enhanced"),
            corona_pipeline: corona_pipeline_for(hdr_format, "fs_enhanced", additive),
            smoke_pipeline: corona_pipeline_for(hdr_format, "fs_smoke_enhanced", alpha_blend),
            sky_pipeline: sky_pipeline_for(hdr_format, "fs_enhanced"),
        });
        let sky_sampler = device.create_sampler(&wgpu::SamplerDescriptor {
            address_mode_u: wgpu::AddressMode::Repeat,
            address_mode_v: wgpu::AddressMode::ClampToEdge,
            mag_filter: wgpu::FilterMode::Linear,
            min_filter: wgpu::FilterMode::Linear,
            ..Default::default()
        });
        let (mut sv, mut si): (Vec<[f32; 3]>, Vec<u32>) = (Vec::new(), Vec::new());
        let (rings, segs) = (12u32, 32u32);
        for r in 0..=rings {
            let elev = -0.15 + (std::f32::consts::FRAC_PI_2 + 0.15) * r as f32 / rings as f32;
            for sgm in 0..=segs {
                let az = sgm as f32 / segs as f32 * std::f32::consts::TAU;
                sv.push([elev.cos() * az.sin(), elev.cos() * az.cos(), elev.sin()]);
            }
        }
        for r in 0..rings {
            for sgm in 0..segs {
                let a = r * (segs + 1) + sgm;
                let b = a + segs + 1;
                si.extend_from_slice(&[a, b, a + 1, a + 1, b, b + 1]);
            }
        }
        let sky_vb = buffer_init(
            &device,
            &queue,
            Some("sky vb"),
            bytemuck::cast_slice(&sv),
            wgpu::BufferUsages::VERTEX,
        );
        let sky_ib = buffer_init(
            &device,
            &queue,
            Some("sky ib"),
            bytemuck::cast_slice(&si),
            wgpu::BufferUsages::INDEX,
        );
        let sky_mesh = (sky_vb, sky_ib, si.len() as u32);
        let overlay_layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("overlay"),
            entries: &[
                wgpu::BindGroupLayoutEntry {
                    binding: 0,
                    visibility: wgpu::ShaderStages::VERTEX_FRAGMENT,
                    ty: wgpu::BindingType::Buffer {
                        ty: wgpu::BufferBindingType::Uniform,
                        has_dynamic_offset: false,
                        min_binding_size: None,
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
        log::info!("renderer: compiling the overlays shaders");
        let overlay_shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("overlay"),
            source: wgpu::ShaderSource::Wgsl(include_str!("overlay.wgsl").into()),
        });
        let overlay_pl = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("overlay"),
            bind_group_layouts: &[Some(&overlay_layout)],
            immediate_size: 0,
        });
        let premul = wgpu::BlendState {
            color: wgpu::BlendComponent {
                src_factor: wgpu::BlendFactor::One,
                dst_factor: wgpu::BlendFactor::OneMinusSrcAlpha,
                operation: wgpu::BlendOperation::Add,
            },
            alpha: wgpu::BlendComponent::OVER,
        };
        let overlay_pipeline = device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
            label: Some("overlay"),
            layout: Some(&overlay_pl),
            vertex: wgpu::VertexState {
                module: &overlay_shader,
                entry_point: Some("vs_main"),
                buffers: &[],
                compilation_options: Default::default(),
            },
            primitive: wgpu::PrimitiveState {
                topology: wgpu::PrimitiveTopology::TriangleList,
                cull_mode: None,
                front_face: wgpu::FrontFace::Ccw,
                ..Default::default()
            },
            depth_stencil: Some(wgpu::DepthStencilState {
                format: DEPTH_FORMAT,
                depth_write_enabled: Some(false),
                depth_compare: Some(wgpu::CompareFunction::Always),
                stencil: Default::default(),
                bias: Default::default(),
            }),
            multisample: wgpu::MultisampleState {
                count: msaa,
                mask: !0,
                alpha_to_coverage_enabled: false,
            },
            fragment: Some(wgpu::FragmentState {
                module: &overlay_shader,
                entry_point: Some("fs_main"),
                targets: &[Some(wgpu::ColorTargetState {
                    format,
                    blend: Some(premul),
                    write_mask: wgpu::ColorWrites::ALL,
                })],
                compilation_options: Default::default(),
            }),
            multiview_mask: None,
            cache: None,
        });
        log::info!("renderer: compiling the SSAO shaders");
        let ssao_shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("ssao"),
            source: wgpu::ShaderSource::Wgsl(include_str!("ssao.wgsl").into()),
        });
        let ao_layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("ssao"),
            entries: &[
                wgpu::BindGroupLayoutEntry {
                    binding: 0,
                    visibility: wgpu::ShaderStages::FRAGMENT,
                    ty: wgpu::BindingType::Buffer {
                        ty: wgpu::BufferBindingType::Uniform,
                        has_dynamic_offset: false,
                        min_binding_size: None,
                    },
                    count: None,
                },
                wgpu::BindGroupLayoutEntry {
                    binding: 1,
                    visibility: wgpu::ShaderStages::FRAGMENT,
                    ty: wgpu::BindingType::Texture {
                        sample_type: wgpu::TextureSampleType::Depth,
                        view_dimension: wgpu::TextureViewDimension::D2,
                        multisampled: false,
                    },
                    count: None,
                },
                wgpu::BindGroupLayoutEntry {
                    binding: 2,
                    visibility: wgpu::ShaderStages::FRAGMENT,
                    ty: wgpu::BindingType::Texture {
                        sample_type: wgpu::TextureSampleType::Float { filterable: false },
                        view_dimension: wgpu::TextureViewDimension::D2,
                        multisampled: false,
                    },
                    count: None,
                },
            ],
        });
        let ao_buf = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("ssao params"),
            size: std::mem::size_of::<SsaoUniform>() as u64,
            usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });
        let ao_sampler = device.create_sampler(&wgpu::SamplerDescriptor {
            address_mode_u: wgpu::AddressMode::ClampToEdge,
            address_mode_v: wgpu::AddressMode::ClampToEdge,
            mag_filter: wgpu::FilterMode::Linear,
            min_filter: wgpu::FilterMode::Linear,
            ..Default::default()
        });
        let ao_pl = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("ssao"),
            bind_group_layouts: &[Some(&ao_layout)],
            immediate_size: 0,
        });
        let make_ao = |entry: &str| {
            device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
                label: Some(entry),
                layout: Some(&ao_pl),
                vertex: wgpu::VertexState {
                    module: &ssao_shader,
                    entry_point: Some("vs_main"),
                    buffers: &[],
                    compilation_options: Default::default(),
                },
                primitive: wgpu::PrimitiveState {
                    topology: wgpu::PrimitiveTopology::TriangleList,
                    cull_mode: None,
                    ..Default::default()
                },
                depth_stencil: None,
                multisample: Default::default(),
                fragment: Some(wgpu::FragmentState {
                    module: &ssao_shader,
                    entry_point: Some(entry),
                    targets: &[Some(wgpu::ColorTargetState {
                        format: wgpu::TextureFormat::Rg16Float,
                        blend: None,
                        write_mask: wgpu::ColorWrites::ALL,
                    })],
                    compilation_options: Default::default(),
                }),
                multiview_mask: None,
                cache: None,
            })
        };
        let gl = GL_BACKEND.load(std::sync::atomic::Ordering::Relaxed);
        let ssao_pipeline = (!gl).then(|| make_ao("fs_ssao"));
        let blur_pipeline = (!gl).then(|| make_ao("fs_blur"));
        let prepass_pl = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("prepass"),
            bind_group_layouts: &[Some(&camera_layout), Some(&material_layout)],
            immediate_size: 0,
        });
        let make_prepass_samples = |kind: u8, cull: bool, samples: u32| {
            let fragment = match kind {
                0 => "fs_shadow",
                1 => "fs_shadow_test",
                2 => "fs_transmap_depth",
                _ => unreachable!(),
            };
            device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
                label: Some("depth prepass"),
                layout: Some(&prepass_pl),
                vertex: wgpu::VertexState {
                    module: &shader,
                    entry_point: Some("vs_main"),
                    buffers: &[Some(vertex_layout.clone())],
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
                multisample: wgpu::MultisampleState {
                    count: samples,
                    ..Default::default()
                },
                fragment: Some(wgpu::FragmentState {
                    module: &shader,
                    entry_point: Some(fragment),
                    targets: &[],
                    compilation_options: Default::default(),
                }),
                multiview_mask: None,
                cache: None,
            })
        };
        let make_prepass = |kind: u8, cull: bool| make_prepass_samples(kind, cull, 1);
        let prepass_pipelines = [
            make_prepass(0, false),
            make_prepass(0, true),
            make_prepass(1, false),
            make_prepass(1, true),
            make_prepass(2, false),
            make_prepass(2, true),
        ];
        let prepass_msaa_pipelines = (msaa > 1 && !cfg!(target_vendor = "apple")).then(|| {
            [
                (0, false),
                (0, true),
                (1, false),
                (1, true),
                (2, false),
                (2, true),
            ]
                .map(|(kind, cull)| make_prepass_samples(kind, cull, msaa))
        });
        log::info!("renderer: compiling the mip maps shaders");
        let mip_shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("mip"),
            source: wgpu::ShaderSource::Wgsl(include_str!("mip.wgsl").into()),
        });
        let mip_layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("mip"),
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
        let mip_pl = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("mip"),
            bind_group_layouts: &[Some(&mip_layout)],
            immediate_size: 0,
        });
        let mip_pipeline = device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
            label: Some("mip"),
            layout: Some(&mip_pl),
            vertex: wgpu::VertexState {
                module: &mip_shader,
                entry_point: Some("vs_main"),
                buffers: &[],
                compilation_options: Default::default(),
            },
            primitive: wgpu::PrimitiveState {
                topology: wgpu::PrimitiveTopology::TriangleList,
                cull_mode: None,
                ..Default::default()
            },
            depth_stencil: None,
            multisample: Default::default(),
            fragment: Some(wgpu::FragmentState {
                module: &mip_shader,
                entry_point: Some("fs_main"),
                targets: &[Some(wgpu::ColorTargetState {
                    format: wgpu::TextureFormat::Rgba8UnormSrgb,
                    blend: None,
                    write_mask: wgpu::ColorWrites::ALL,
                })],
                compilation_options: Default::default(),
            }),
            multiview_mask: None,
            cache: None,
        });
        let mip_sampler = device.create_sampler(&wgpu::SamplerDescriptor {
            address_mode_u: wgpu::AddressMode::ClampToEdge,
            address_mode_v: wgpu::AddressMode::ClampToEdge,
            mag_filter: wgpu::FilterMode::Linear,
            min_filter: wgpu::FilterMode::Linear,
            ..Default::default()
        });
        log::info!("renderer: compiling the post passes shaders");
        let post_shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("post"),
            source: wgpu::ShaderSource::Wgsl(include_str!("post.wgsl").into()),
        });
        let float_tex = |binding: u32| wgpu::BindGroupLayoutEntry {
            binding,
            visibility: wgpu::ShaderStages::FRAGMENT,
            ty: wgpu::BindingType::Texture {
                sample_type: wgpu::TextureSampleType::Float { filterable: true },
                view_dimension: wgpu::TextureViewDimension::D2,
                multisampled: false,
            },
            count: None,
        };
        let post_layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("post"),
            entries: &[
                wgpu::BindGroupLayoutEntry {
                    binding: 0,
                    visibility: wgpu::ShaderStages::FRAGMENT,
                    ty: wgpu::BindingType::Buffer {
                        ty: wgpu::BufferBindingType::Uniform,
                        has_dynamic_offset: false,
                        min_binding_size: None,
                    },
                    count: None,
                },
                float_tex(1),
                wgpu::BindGroupLayoutEntry {
                    binding: 2,
                    visibility: wgpu::ShaderStages::FRAGMENT,
                    ty: wgpu::BindingType::Sampler(wgpu::SamplerBindingType::Filtering),
                    count: None,
                },
                float_tex(3),
                float_tex(4),
            ],
        });
        let post_buf = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("post params"),
            size: std::mem::size_of::<PostUniform>() as u64,
            usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });
        let post_sampler = device.create_sampler(&wgpu::SamplerDescriptor {
            address_mode_u: wgpu::AddressMode::ClampToEdge,
            address_mode_v: wgpu::AddressMode::ClampToEdge,
            mag_filter: wgpu::FilterMode::Linear,
            min_filter: wgpu::FilterMode::Linear,
            ..Default::default()
        });
        let post_pl = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("post"),
            bind_group_layouts: &[Some(&post_layout)],
            immediate_size: 0,
        });
        let post_pipeline = |entry: &str, target: wgpu::TextureFormat| {
            device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
                label: Some(entry),
                layout: Some(&post_pl),
                vertex: wgpu::VertexState {
                    module: &post_shader,
                    entry_point: Some("vs_main"),
                    buffers: &[],
                    compilation_options: Default::default(),
                },
                primitive: wgpu::PrimitiveState {
                    topology: wgpu::PrimitiveTopology::TriangleList,
                    cull_mode: None,
                    ..Default::default()
                },
                depth_stencil: None,
                multisample: Default::default(),
                fragment: Some(wgpu::FragmentState {
                    module: &post_shader,
                    entry_point: Some(entry),
                    targets: &[Some(wgpu::ColorTargetState {
                        format: target,
                        blend: None,
                        write_mask: wgpu::ColorWrites::ALL,
                    })],
                    compilation_options: Default::default(),
                }),
                multiview_mask: None,
                cache: None,
            })
        };
        let post = PostPipelines {
            down_first: post_pipeline("fs_down_first", hdr_format),
            down: post_pipeline("fs_down", hdr_format),
            up: post_pipeline("fs_up", hdr_format),
            meter: post_pipeline("fs_meter", hdr_format),
            adapt: post_pipeline("fs_adapt", hdr_format),
            tonemap: post_pipeline("fs_tonemap", format),
            tonemap_encoded: post_pipeline("fs_tonemap_encoded", wgpu::TextureFormat::Rgba8Unorm),
            fxaa: post_pipeline("fs_fxaa", format),
        };
        let one = wgpu::Extent3d {
            width: 1,
            height: 1,
            depth_or_array_layers: 1,
        };
        let tiny = |label: &str| {
            device
                .create_texture(&wgpu::TextureDescriptor {
                    label: Some(label),
                    size: one,
                    mip_level_count: 1,
                    sample_count: 1,
                    dimension: wgpu::TextureDimension::D2,
                    format: hdr_format,
                    usage: wgpu::TextureUsages::RENDER_ATTACHMENT
                        | wgpu::TextureUsages::TEXTURE_BINDING
                        | wgpu::TextureUsages::COPY_SRC,
                    view_formats: &[],
                })
                .create_view(&Default::default())
        };
        let meter_view = tiny("exposure meter");
        let adapt_views = [tiny("exposure a"), tiny("exposure b")];
        let adapt_bg = [0usize, 1].map(|k| {
            device.create_bind_group(&wgpu::BindGroupDescriptor {
                label: Some("exposure"),
                layout: &post_layout,
                entries: &[
                    wgpu::BindGroupEntry {
                        binding: 0,
                        resource: post_buf.as_entire_binding(),
                    },
                    wgpu::BindGroupEntry {
                        binding: 1,
                        resource: wgpu::BindingResource::TextureView(&meter_view),
                    },
                    wgpu::BindGroupEntry {
                        binding: 2,
                        resource: wgpu::BindingResource::Sampler(&post_sampler),
                    },
                    wgpu::BindGroupEntry {
                        binding: 3,
                        resource: wgpu::BindingResource::TextureView(&adapt_views[k]),
                    },
                    wgpu::BindGroupEntry {
                        binding: 4,
                        resource: wgpu::BindingResource::TextureView(&white_texture.view),
                    },
                ],
            })
        });
        let enh_buf = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("enhanced lighting"),
            size: std::mem::size_of::<EnhancedUniform>() as u64,
            usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });
        let sky_lut = device.create_texture(&wgpu::TextureDescriptor {
            label: Some("sky table"),
            size: wgpu::Extent3d {
                width: atmosphere::SKY_LUT_W,
                height: atmosphere::SKY_LUT_H,
                depth_or_array_layers: 1,
            },
            mip_level_count: 1,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format: hdr_format,
            usage: wgpu::TextureUsages::TEXTURE_BINDING | wgpu::TextureUsages::COPY_DST,
            view_formats: &[],
        });
        let sky_lut_view = sky_lut.create_view(&Default::default());
        let lin_sampler = device.create_sampler(&wgpu::SamplerDescriptor {
            address_mode_u: wgpu::AddressMode::ClampToEdge,
            address_mode_v: wgpu::AddressMode::ClampToEdge,
            address_mode_w: wgpu::AddressMode::ClampToEdge,
            mag_filter: wgpu::FilterMode::Linear,
            min_filter: wgpu::FilterMode::Linear,
            mipmap_filter: wgpu::MipmapFilterMode::Linear,
            ..Default::default()
        });
        let uniform_entry = |binding: u32| wgpu::BindGroupLayoutEntry {
            binding,
            visibility: wgpu::ShaderStages::VERTEX_FRAGMENT,
            ty: wgpu::BindingType::Buffer {
                ty: wgpu::BufferBindingType::Uniform,
                has_dynamic_offset: false,
                min_binding_size: None,
            },
            count: None,
        };
        let probe_layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("probe"),
            entries: &[
                uniform_entry(0),
                uniform_entry(11),
                wgpu::BindGroupLayoutEntry {
                    binding: 13,
                    visibility: wgpu::ShaderStages::FRAGMENT,
                    ty: wgpu::BindingType::Sampler(wgpu::SamplerBindingType::Filtering),
                    count: None,
                },
                float_tex(14),
                uniform_entry(15),
                wgpu::BindGroupLayoutEntry {
                    binding: 16,
                    visibility: wgpu::ShaderStages::FRAGMENT,
                    ty: wgpu::BindingType::Texture {
                        sample_type: wgpu::TextureSampleType::Float { filterable: true },
                        view_dimension: wgpu::TextureViewDimension::Cube,
                        multisampled: false,
                    },
                    count: None,
                },
            ],
        });
        let probe = {
            let tex = device.create_texture(&wgpu::TextureDescriptor {
                label: Some("reflection probe"),
                size: wgpu::Extent3d {
                    width: PROBE_SIZE,
                    height: PROBE_SIZE,
                    depth_or_array_layers: 6,
                },
                mip_level_count: PROBE_MIPS,
                sample_count: 1,
                dimension: wgpu::TextureDimension::D2,
                format: hdr_format,
                usage: wgpu::TextureUsages::RENDER_ATTACHMENT
                    | wgpu::TextureUsages::TEXTURE_BINDING,
                view_formats: &[],
            });
            let cube = |base: u32, count: u32| {
                tex.create_view(&wgpu::TextureViewDescriptor {
                    label: Some("probe cube"),
                    dimension: Some(wgpu::TextureViewDimension::Cube),
                    base_mip_level: base,
                    mip_level_count: Some(count),
                    base_array_layer: 0,
                    array_layer_count: Some(6),
                    ..Default::default()
                })
            };
            let view = cube(0, PROBE_MIPS);
            let faces: Vec<Vec<wgpu::TextureView>> = (0..PROBE_MIPS)
                .map(|m| {
                    (0..6)
                        .map(|f| {
                            tex.create_view(&wgpu::TextureViewDescriptor {
                                label: Some("probe face"),
                                dimension: Some(wgpu::TextureViewDimension::D2),
                                base_mip_level: m,
                                mip_level_count: Some(1),
                                base_array_layer: f,
                                array_layer_count: Some(1),
                                ..Default::default()
                            })
                        })
                        .collect()
                })
                .collect();
            let dummy = device.create_texture(&wgpu::TextureDescriptor {
                label: Some("probe placeholder"),
                size: wgpu::Extent3d {
                    width: 1,
                    height: 1,
                    depth_or_array_layers: 6,
                },
                mip_level_count: 1,
                sample_count: 1,
                dimension: wgpu::TextureDimension::D2,
                format: hdr_format,
                usage: wgpu::TextureUsages::TEXTURE_BINDING,
                view_formats: &[],
            });
            let dummy_view = dummy.create_view(&wgpu::TextureViewDescriptor {
                dimension: Some(wgpu::TextureViewDimension::Cube),
                ..Default::default()
            });
            let cube_tex = device.create_texture(&wgpu::TextureDescriptor {
                label: Some("sky cube"),
                size: wgpu::Extent3d {
                    width: SKY_CUBE_SIZE,
                    height: SKY_CUBE_SIZE,
                    depth_or_array_layers: 6,
                },
                mip_level_count: 1,
                sample_count: 1,
                dimension: wgpu::TextureDimension::D2,
                format: hdr_format,
                usage: wgpu::TextureUsages::RENDER_ATTACHMENT
                    | wgpu::TextureUsages::TEXTURE_BINDING,
                view_formats: &[],
            });
            let cube_view = cube_tex.create_view(&wgpu::TextureViewDescriptor {
                label: Some("sky cube"),
                dimension: Some(wgpu::TextureViewDimension::Cube),
                ..Default::default()
            });
            let bind_groups = (0..PROBE_MIPS)
                .map(|m| {
                    let src = if m == 0 {
                        cube_view.clone()
                    } else {
                        cube(0, m)
                    };
                    [0u32, 3].map(|first| {
                        let rough = m as f32 / (PROBE_MIPS - 1) as f32;
                        let buf = buffer_init(
                            &device,
                            &queue,
                            Some("probe pass"),
                            bytemuck::cast_slice(&[
                                first as f32,
                                rough,
                                PROBE_SIZE as f32,
                                m as f32,
                            ]),
                            wgpu::BufferUsages::UNIFORM,
                        );
                        device.create_bind_group(&wgpu::BindGroupDescriptor {
                            label: Some("probe"),
                            layout: &probe_layout,
                            entries: &[
                                wgpu::BindGroupEntry {
                                    binding: 0,
                                    resource: camera_buf.as_entire_binding(),
                                },
                                wgpu::BindGroupEntry {
                                    binding: 11,
                                    resource: enh_buf.as_entire_binding(),
                                },
                                wgpu::BindGroupEntry {
                                    binding: 13,
                                    resource: wgpu::BindingResource::Sampler(&lin_sampler),
                                },
                                wgpu::BindGroupEntry {
                                    binding: 14,
                                    resource: wgpu::BindingResource::TextureView(&sky_lut_view),
                                },
                                wgpu::BindGroupEntry {
                                    binding: 15,
                                    resource: buf.as_entire_binding(),
                                },
                                wgpu::BindGroupEntry {
                                    binding: 16,
                                    resource: wgpu::BindingResource::TextureView(&src),
                                },
                            ],
                        })
                    })
                })
                .collect();
            let probe_pl = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
                label: Some("probe"),
                bind_group_layouts: &[Some(&probe_layout), Some(&sky_layout)],
                immediate_size: 0,
            });
            let target = Some(wgpu::ColorTargetState {
                format: hdr_format,
                blend: None,
                write_mask: wgpu::ColorWrites::ALL,
            });
            let probe_pipeline = |entry: &str| {
                device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
                    label: Some(entry),
                    layout: Some(&probe_pl),
                    vertex: wgpu::VertexState {
                        module: &sky_shader,
                        entry_point: Some("vs_probe"),
                        buffers: &[],
                        compilation_options: Default::default(),
                    },
                    primitive: wgpu::PrimitiveState {
                        topology: wgpu::PrimitiveTopology::TriangleList,
                        cull_mode: None,
                        ..Default::default()
                    },
                    depth_stencil: None,
                    multisample: Default::default(),
                    fragment: Some(wgpu::FragmentState {
                        module: &sky_shader,
                        entry_point: Some(entry),
                        targets: &[target.clone(), target.clone(), target.clone()],
                        compilation_options: Default::default(),
                    }),
                    multiview_mask: None,
                    cache: None,
                })
            };
            let cube_faces: Vec<wgpu::TextureView> = (0..6)
                .map(|f| {
                    cube_tex.create_view(&wgpu::TextureViewDescriptor {
                        label: Some("sky cube face"),
                        dimension: Some(wgpu::TextureViewDimension::D2),
                        base_mip_level: 0,
                        mip_level_count: Some(1),
                        base_array_layer: f,
                        array_layer_count: Some(1),
                        ..Default::default()
                    })
                })
                .collect();
            let cube_bind_groups: Vec<wgpu::BindGroup> = (0..6 * SKY_CUBE_ROUNDS)
                .map(|k| {
                    let (f, round) = (k / SKY_CUBE_ROUNDS, k % SKY_CUBE_ROUNDS);
                    let buf = buffer_init(
                        &device,
                        &queue,
                        Some("sky cube face"),
                        bytemuck::cast_slice(&[f as f32, round as f32, SKY_CUBE_SIZE as f32, 0.0]),
                        wgpu::BufferUsages::UNIFORM,
                    );
                    device.create_bind_group(&wgpu::BindGroupDescriptor {
                        label: Some("sky cube"),
                        layout: &probe_layout,
                        entries: &[
                            wgpu::BindGroupEntry {
                                binding: 0,
                                resource: camera_buf.as_entire_binding(),
                            },
                            wgpu::BindGroupEntry {
                                binding: 11,
                                resource: enh_buf.as_entire_binding(),
                            },
                            wgpu::BindGroupEntry {
                                binding: 13,
                                resource: wgpu::BindingResource::Sampler(&lin_sampler),
                            },
                            wgpu::BindGroupEntry {
                                binding: 14,
                                resource: wgpu::BindingResource::TextureView(&sky_lut_view),
                            },
                            wgpu::BindGroupEntry {
                                binding: 15,
                                resource: buf.as_entire_binding(),
                            },
                            wgpu::BindGroupEntry {
                                binding: 16,
                                resource: wgpu::BindingResource::TextureView(&dummy_view),
                            },
                        ],
                    })
                })
                .collect();
            let cube_pipeline = device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
                label: Some("sky cube"),
                layout: Some(&probe_pl),
                vertex: wgpu::VertexState {
                    module: &sky_shader,
                    entry_point: Some("vs_probe"),
                    buffers: &[],
                    compilation_options: Default::default(),
                },
                primitive: wgpu::PrimitiveState {
                    topology: wgpu::PrimitiveTopology::TriangleList,
                    cull_mode: None,
                    ..Default::default()
                },
                depth_stencil: None,
                multisample: Default::default(),
                fragment: Some(wgpu::FragmentState {
                    module: &sky_shader,
                    entry_point: Some("fs_sky_cube"),
                    targets: &[Some(wgpu::ColorTargetState {
                        format: hdr_format,
                        blend: Some(wgpu::BlendState {
                            color: wgpu::BlendComponent {
                                src_factor: wgpu::BlendFactor::OneMinusConstant,
                                dst_factor: wgpu::BlendFactor::Constant,
                                operation: wgpu::BlendOperation::Add,
                            },
                            alpha: wgpu::BlendComponent {
                                src_factor: wgpu::BlendFactor::OneMinusConstant,
                                dst_factor: wgpu::BlendFactor::Constant,
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
            Probe {
                view,
                faces,
                bind_groups,
                sky_pipeline: probe_pipeline("fs_probe_sky"),
                filter_pipeline: probe_pipeline("fs_probe_filter"),
                age: u32::MAX,
                scale: 1.0,
                cube_view,
                cube_faces,
                cube_bind_groups,
                cube_pipeline,
                cube_next: 0,
                cube_filled: false,
                cube_round: 0,
                cube_wait: 0,
                cube_eye: None,
                cube_recapture: false,
            }
        };
        let overlay_pipeline_1x = device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
            label: Some("overlay 1x"),
            layout: Some(&overlay_pl),
            vertex: wgpu::VertexState {
                module: &overlay_shader,
                entry_point: Some("vs_main"),
                buffers: &[],
                compilation_options: Default::default(),
            },
            primitive: wgpu::PrimitiveState {
                topology: wgpu::PrimitiveTopology::TriangleList,
                cull_mode: None,
                front_face: wgpu::FrontFace::Ccw,
                ..Default::default()
            },
            depth_stencil: None,
            multisample: Default::default(),
            fragment: Some(wgpu::FragmentState {
                module: &overlay_shader,
                entry_point: Some("fs_main"),
                targets: &[Some(wgpu::ColorTargetState {
                    format,
                    blend: Some(premul),
                    write_mask: wgpu::ColorWrites::ALL,
                })],
                compilation_options: Default::default(),
            }),
            multiview_mask: None,
            cache: None,
        });
        log::info!("renderer: compiling the VR interface shaders");
        let xr_ui_shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("OpenXR spatial UI"),
            source: wgpu::ShaderSource::Wgsl(include_str!("xr_ui.wgsl").into()),
        });
        let xr_ui_pipeline = device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
            label: Some("OpenXR spatial UI"),
            layout: Some(&overlay_pl),
            vertex: wgpu::VertexState {
                module: &xr_ui_shader,
                entry_point: Some("vs_main"),
                buffers: &[],
                compilation_options: Default::default(),
            },
            primitive: wgpu::PrimitiveState {
                topology: wgpu::PrimitiveTopology::TriangleList,
                cull_mode: None,
                front_face: wgpu::FrontFace::Ccw,
                ..Default::default()
            },
            depth_stencil: None,
            multisample: Default::default(),
            fragment: Some(wgpu::FragmentState {
                module: &xr_ui_shader,
                entry_point: Some("fs_main"),
                targets: &[Some(wgpu::ColorTargetState {
                    format,
                    blend: Some(premul),
                    write_mask: wgpu::ColorWrites::ALL,
                })],
                compilation_options: Default::default(),
            }),
            multiview_mask: None,
            cache: None,
        });
        log::info!("renderer: compiling the upscaler shaders");
        let upscale_shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("upscale"),
            source: wgpu::ShaderSource::Wgsl(include_str!("upscale.wgsl").into()),
        });
        let upscale_layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("upscale"),
            entries: &[
                wgpu::BindGroupLayoutEntry {
                    binding: 0,
                    visibility: wgpu::ShaderStages::FRAGMENT,
                    ty: wgpu::BindingType::Buffer {
                        ty: wgpu::BufferBindingType::Uniform,
                        has_dynamic_offset: false,
                        min_binding_size: None,
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
        let upscale_pl = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("upscale"),
            bind_group_layouts: &[Some(&upscale_layout)],
            immediate_size: 0,
        });
        let upscale_pipeline = device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
            label: Some("upscale"),
            layout: Some(&upscale_pl),
            vertex: wgpu::VertexState {
                module: &upscale_shader,
                entry_point: Some("vs_main"),
                buffers: &[],
                compilation_options: Default::default(),
            },
            primitive: wgpu::PrimitiveState {
                topology: wgpu::PrimitiveTopology::TriangleList,
                cull_mode: None,
                ..Default::default()
            },
            depth_stencil: None,
            multisample: Default::default(),
            fragment: Some(wgpu::FragmentState {
                module: &upscale_shader,
                entry_point: Some("fs_main"),
                targets: &[Some(wgpu::ColorTargetState {
                    format,
                    blend: None,
                    write_mask: wgpu::ColorWrites::ALL,
                })],
                compilation_options: Default::default(),
            }),
            multiview_mask: None,
            cache: None,
        });
        let upscale_buf = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("upscale params"),
            size: 16,
            usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });
        let gpu_timers = [GpuTimers::new(&device), GpuTimers::new(&device)];
        let puddles = (!gl && !leave_out_enhanced)
            .then(|| puddles::Pipelines::new(&device, &shader, &camera_layout, &material_layout));
        Renderer {
            _device_poller: DevicePoller::start(&device),
            upscale_pipeline,
            upscale_layout,
            upscale_buf,
            scale_targets: HashMap::new(),
            glass_prev: None,
            glass_live: None,
            target_use: HashMap::new(),
            dynamic_scale: std::cell::Cell::new(1.0),
            flicker: std::cell::RefCell::new(HashMap::new()),
            cull_drawn: std::cell::RefCell::new(Vec::new()),
            object_sizes: Default::default(),
            object_sizes_scratch: Default::default(),
            shadow_far_cache: std::cell::Cell::new((Mat4::IDENTITY, 0, DVec3::ZERO, Vec3::ZERO)),
            shadow_near_cache: std::cell::Cell::new((Mat4::IDENTITY, 0, DVec3::ZERO, Vec3::ZERO)),
            xr_shadow_cache: std::cell::Cell::new(None),
            shadow_clear_pipeline,
            mip_pipeline,
            mip_layout,
            mip_sampler,
            clamp_sampler,
            mirror_sampler,
            address_next: std::cell::Cell::new(TexAddressing::Wrap),
            light_map_next: std::cell::Cell::new(false),
            lm_atlas_view: lm_atlas.create_view(&wgpu::TextureViewDescriptor::default()),
            lm_atlas,
            lm_uniform,
            lm_place: std::cell::Cell::new((0.0, 0.0, 0.0)),
            hdr_targets: HashMap::new(),
            puddles,
            post,
            post_layout,
            post_buf,
            post_sampler,
            meter_view,
            adapt_views,
            adapt_bg,
            adapt_front: 0,
            exposure_log: ExposureLog::new(&device),
            enh_buf,
            sky_lut,
            sky_lut_view,
            lin_sampler,
            probe: Some(probe),
            sky_state: None,
            sky_job: None,
            exposure: None,
            texture_aspect: None,
            last_frame: None,
            instant_exposure: false,
            overlay_pipeline_1x,
            xr_ui_pipeline,
            started: std::time::Instant::now(),
            ao: None,
            ao_sampler,
            ao_layout,
            ao_buf,
            prepass_pipelines,
            prepass_msaa_pipelines,
            ssao_pipeline,
            blur_pipeline,
            device,
            queue,
            adapter_name,
            camera_layout,
            material_layout,
            pass,
            hdr_pass,
            corona_bind_group,
            smoke_bind_group,
            corona_textures: Vec::new(),
            corona_layout,
            corona_sampler,
            sky_layout,
            sky_sampler,
            cloud_shape_view,
            cloud_detail_view,
            cloud_sampler,
            sky_mesh,
            overlay_pipeline,
            overlay_layout,
            sampler,
            camera_buf,
            white_texture,
            black_texture,
            flat_normal_texture,
            format,
            depth: None,
            msaa_targets: HashMap::new(),
            shadow_view,
            shadow_view_far,
            shadow_sampler,
            shadow_layout,
            spot_tile,
            spot_state: Default::default(),
            spot_cam_bufs,
            shadow_pipelines,
            shadow_blobs: options.shadow_blobs,
            options,
            gpu_error,
            env_heading: Default::default(),
            out_of_memory,
            device_lost,
            blend_by_origin: false,
            gpu_timers,
            stats: Default::default(),
            counts: Default::default(),
            profiling: omsi_cfg::env::var_os("OMSI_PROFILE").is_some(),
            draw_audit_at: std::time::Instant::now(),
            encoding_pool: if omsi_cfg::env::var_os("OMSI_NO_RENDER_POOL").is_some() {
                None
            } else {
                let workers = std::thread::available_parallelism()
                    .map(|n| n.get() / 2)
                    .unwrap_or(2)
                    .clamp(2, 8);
                rayon::ThreadPoolBuilder::new()
                    .num_threads(workers)
                    .thread_name(|i| format!("omsi-render-{i}"))
                    .build()
                    .ok()
            },
            pending_meshes: Default::default(),
            freed: std::cell::OnceCell::new(),
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

    fn scale_target(&mut self, w: u32, h: u32) -> (wgpu::TextureView, wgpu::BindGroup) {
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
            usage: wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::TEXTURE_BINDING,
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

    pub fn new_scene(&self) -> Scene {
        Scene {
            meshes: Vec::new(),
            textures: Vec::new(),
            glass_slot: None,
            glass_key: None,
            materials: Vec::new(),
            instances: Vec::new(),
            render_origin: DVec3::ZERO,
            lights: Vec::new(),
            interior_lights: Vec::new(),
            interior_free: Vec::new(),
            coronas: Vec::new(),
            occluders: Vec::new(),
            model_buf: None,
            params_buf: None,
            light_buf: None,
            grid_buf: None,
            corona_buf: None,
            corona_count: 0,
            smoke: Vec::new(),
            smoke_buf: None,
            smoke_count: 0,
            corona_runs: Vec::new(),
            draw_buf: None,
            camera_bind_group: None,
            shadow_bind_group: None,
            spot_bind_groups: Vec::new(),
            sky_bind_group: None,
            overlays: Vec::new(),
            premultiplied: Default::default(),
            overlay_res: Vec::new(),
            dirty: true,
            changed: Vec::new(),
            changed_mark: Vec::new(),
            cache_bounds: omsi_cfg::env::var_os("OMSI_NO_BOUNDS_CACHE").is_none(),
            bounds_meshes: Vec::new(),
            bounds_dirty: false,
            uploaded_instances: 0,
            uploaded_entries: 0,
            cpu_models: Vec::new(),
            cpu_params: Vec::new(),
            last_grid: Vec::new(),
            last_lights: Vec::new(),
            bind_groups: HashMap::new(),
            pbr_maps: HashMap::new(),
            snow_textures: Default::default(),
        }
    }

    pub fn set_instance_mesh(&self, scene: &mut Scene, instance: usize, mesh: MeshId) {
        if scene.instances[instance].mesh != mesh {
            scene.instances[instance].mesh = mesh;
            scene.dirty = true;
        }
    }

    pub fn update_mesh(
        &self,
        scene: &mut Scene,
        id: MeshId,
        positions: &[Vec3],
        normals: &[Vec3],
        uvs: &[glam::Vec2],
    ) {
        let verts: Vec<Vertex> = positions
            .iter()
            .zip(normals)
            .zip(uvs)
            .map(|((p, n), uv)| Vertex {
                pos: p.to_array(),
                normal: n.to_array(),
                uv: uv.to_array(),
            })
            .collect();
        let bytes: &[u8] = bytemuck::cast_slice(&verts);
        let m = &mut scene.meshes[id];
        if (m.vertex_buf.size() as usize) < bytes.len() {
            return;
        }
        {
            let mut pending = self.pending_meshes.borrow_mut();
            match pending.iter_mut().find(|(mid, _)| *mid == id) {
                Some(e) => e.1 = bytes.to_vec(),
                None => pending.push((id, bytes.to_vec())),
            }
        }
        let (mut lo, mut hi) = (Vec3::splat(f32::MAX), Vec3::splat(f32::MIN));
        for p in positions {
            lo = lo.min(*p);
            hi = hi.max(*p);
        }
        if !positions.is_empty() {
            m.bounds_center = (lo + hi) * 0.5;
            m.bounds_radius = (hi - m.bounds_center).length();
            Self::mesh_bounds_changed(scene, id);
        }
    }

    fn flush_pending_meshes(&self, scene: &Scene, _encoder: &mut wgpu::CommandEncoder) {
        let pending = std::mem::take(&mut *self.pending_meshes.borrow_mut());
        for (id, b) in &pending {
            let Some(m) = scene.meshes.get(*id) else {
                continue;
            };
            let len = (b.len() as u64) / 4 * 4;
            if len == 0 || m.vertex_buf.size() < len {
                continue;
            }
            self.queue
                .write_buffer(&m.vertex_buf, 0, &b[..len as usize]);
        }
    }

    pub fn add_mesh(&self, scene: &mut Scene, data: &MeshData) -> MeshId {
        scene
            .meshes
            .push(make_mesh(&self.device, &self.queue, data));
        scene.meshes.len() - 1
    }

    pub fn add_prepared_mesh(&self, scene: &mut Scene, mesh: PreparedMesh) -> MeshId {
        scene.meshes.push(mesh.0);
        scene.meshes.len() - 1
    }

    pub fn mesh_bytes(&self, scene: &Scene) -> u64 {
        let freed = self
            .freed
            .get()
            .map(|f| (f.vertex_buf.clone(), f.index_buf.clone()));
        scene
            .meshes
            .iter()
            .filter(|m| freed.as_ref().is_none_or(|(v, _)| m.vertex_buf != *v))
            .map(|m| m.vertex_buf.size() + m.index_buf.size())
            .sum()
    }

    pub fn upload_speed_mb_s(&self) -> f64 {
        let n = 1024u32;
        let size = wgpu::Extent3d {
            width: n,
            height: n,
            depth_or_array_layers: 1,
        };
        let texture = self.device.create_texture(&wgpu::TextureDescriptor {
            label: Some("upload check"),
            size,
            mip_level_count: 1,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format: wgpu::TextureFormat::Rgba8Unorm,
            usage: wgpu::TextureUsages::COPY_DST,
            view_formats: &[],
        });
        let data = vec![0u8; (n * n * 4) as usize];
        let rounds = 4;
        let t = std::time::Instant::now();
        for _ in 0..rounds {
            self.queue.write_texture(
                wgpu::TexelCopyTextureInfo {
                    texture: &texture,
                    mip_level: 0,
                    origin: wgpu::Origin3d::ZERO,
                    aspect: wgpu::TextureAspect::All,
                },
                &data,
                wgpu::TexelCopyBufferLayout {
                    offset: 0,
                    bytes_per_row: Some(n * 4),
                    rows_per_image: Some(n),
                },
                size,
            );
        }
        let secs = t.elapsed().as_secs_f64().max(1e-6);
        self.queue.submit([]);
        texture.destroy();
        (data.len() * rounds) as f64 / 1e6 / secs
    }

    pub fn add_render_texture(&self, scene: &mut Scene, width: u32, height: u32) -> TextureId {
        let size = wgpu::Extent3d {
            width,
            height,
            depth_or_array_layers: 1,
        };
        let texture = self.device.create_texture(&wgpu::TextureDescriptor {
            label: Some("render target"),
            size,
            mip_level_count: 1,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format: self.format,
            usage: wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::TEXTURE_BINDING,
            view_formats: &[],
        });
        let view = texture.create_view(&Default::default());
        let bytes = texture_bytes(self.format, width, height, 1);
        scene.textures.push(GpuTexture {
            texture,
            view,
            size: (width, height),
            bytes,
            generation: next_gen(),
        });
        scene.textures.len() - 1
    }

    pub fn render_to_texture(
        &mut self,
        scene: &mut Scene,
        id: TextureId,
        camera: &Camera,
        lighting: &Lighting,
        aspect: f32,
    ) {
        let Some(t) = scene.textures.get(id) else {
            return;
        };
        let view = t.view.clone();
        let (w, h) = t.size;
        self.texture_aspect = Some(aspect);
        self.render_inner(
            scene,
            &view,
            w,
            h,
            camera,
            lighting,
            false,
            Some(id),
            None,
            false,
        );
        self.texture_aspect = None;
    }

    fn show_glass_behind(&self, scene: &mut Scene, key: GlassKey) {
        let Some(id) = scene.glass_slot else { return };
        if scene.glass_key == Some(key) {
            return;
        }
        let view = if key.0 {
            self.hdr_targets
                .get(&(key.1, key.2))
                .and_then(|h| h.down.first())
                .cloned()
        } else {
            self.glass_prev
                .as_ref()
                .filter(|g| g.1 == (key.1, key.2))
                .map(|g| g.0.clone())
        };
        let Some(view) = view else { return };
        scene.textures[id] =
            GpuTexture::showing(self.black_texture.texture.clone(), view, (key.1, key.2));
        self.rebind_textures(scene, &[id]);
        scene.glass_key = Some(key);
    }

    pub fn set_sky_textures(&self, scene: &mut Scene, textures: [TextureId; 3]) {
        self.set_sky_textures_clouds(scene, textures, None)
    }

    pub fn set_sky_textures_clouds(
        &self,
        scene: &mut Scene,
        textures: [TextureId; 3],
        clouds: Option<TextureId>,
    ) {
        let views: Vec<&wgpu::TextureView> =
            textures.iter().map(|t| &scene.textures[*t].view).collect();
        let bg = self.device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("sky"),
            layout: &self.sky_layout,
            entries: &[
                wgpu::BindGroupEntry {
                    binding: 0,
                    resource: wgpu::BindingResource::TextureView(views[0]),
                },
                wgpu::BindGroupEntry {
                    binding: 1,
                    resource: wgpu::BindingResource::TextureView(views[1]),
                },
                wgpu::BindGroupEntry {
                    binding: 2,
                    resource: wgpu::BindingResource::TextureView(views[2]),
                },
                wgpu::BindGroupEntry {
                    binding: 3,
                    resource: wgpu::BindingResource::Sampler(&self.sky_sampler),
                },
                wgpu::BindGroupEntry {
                    binding: 4,
                    resource: wgpu::BindingResource::TextureView(match clouds {
                        Some(c) => &scene.textures[c].view,
                        None => &self.black_texture.view,
                    }),
                },
                wgpu::BindGroupEntry {
                    binding: 5,
                    resource: wgpu::BindingResource::Sampler(&self.sampler),
                },
                wgpu::BindGroupEntry {
                    binding: 6,
                    resource: wgpu::BindingResource::TextureView(&self.cloud_shape_view),
                },
                wgpu::BindGroupEntry {
                    binding: 7,
                    resource: wgpu::BindingResource::TextureView(&self.cloud_detail_view),
                },
                wgpu::BindGroupEntry {
                    binding: 8,
                    resource: wgpu::BindingResource::Sampler(&self.cloud_sampler),
                },
            ],
        });
        scene.sky_bind_group = Some(bg);
    }

    pub fn add_instance(
        &self,
        scene: &mut Scene,
        mesh: MeshId,
        origin: DVec3,
        transform: Mat4,
        materials: Vec<MaterialId>,
    ) -> usize {
        let slots = scene.meshes[mesh]
            .ranges
            .iter()
            .map(|r| r.2)
            .max()
            .map(|m| m as usize + 1)
            .unwrap_or(1);
        scene.instances.push(Instance {
            mesh,
            transform,
            origin,
            materials,
            slot_alpha: vec![1.0; slots],
            slot_light: vec![1.0; slots],
            slot_night: vec![1.0; slots],
            visible: true,
            slot_uv: vec![[0.0; 2]; slots],
            interior: 0.0,
            interior_lamps: 0,
            base: 0,
            bounds: InstanceBounds::default(),
            surface: false,
            presurface: false,
            render_phase: RenderPhase::Normal,
            surface_bias: false,
            blend_sort_origin: None,
            lod: (0.0, f32::MAX),
            blob: false,
            ground_layer: false,
            decal: false,
            object_radius: 0.0,
            detail: 1.0,
            any_distance: false,
            near_only: None,
            mirror_only: false,
            omsi_caster: false,
            ordered: false,
            casts_shadow: true,
            roof: None,
        });
        scene.instances.len() - 1
    }

    pub fn add_surface_instance(
        &self,
        scene: &mut Scene,
        mesh: MeshId,
        origin: DVec3,
        transform: Mat4,
        materials: Vec<MaterialId>,
    ) -> usize {
        let slots = scene.meshes[mesh]
            .ranges
            .iter()
            .map(|r| r.2)
            .max()
            .map(|m| m as usize + 1)
            .unwrap_or(1);
        scene.instances.push(Instance {
            mesh,
            transform,
            origin,
            materials,
            slot_alpha: vec![1.0; slots],
            slot_light: vec![1.0; slots],
            slot_night: vec![1.0; slots],
            visible: true,
            slot_uv: vec![[0.0; 2]; slots],
            interior: 0.0,
            interior_lamps: 0,
            base: 0,
            bounds: InstanceBounds::default(),
            surface: true,
            presurface: false,
            render_phase: RenderPhase::Normal,
            surface_bias: true,
            blend_sort_origin: None,
            lod: (0.0, f32::MAX),
            blob: false,
            ground_layer: false,
            decal: false,
            object_radius: 0.0,
            detail: 1.0,
            any_distance: false,
            near_only: None,
            mirror_only: false,
            omsi_caster: false,
            ordered: false,
            casts_shadow: false,
            roof: None,
        });
        scene.instances.len() - 1
    }

    pub fn add_shadow_blob_instance(
        &self,
        scene: &mut Scene,
        mesh: MeshId,
        origin: DVec3,
        transform: Mat4,
        materials: Vec<MaterialId>,
    ) -> usize {
        let i = self.add_surface_instance(scene, mesh, origin, transform, materials);
        scene.instances[i].blob = true;
        i
    }

    fn mark_changed(scene: &mut Scene, i: usize) {
        if scene.dirty || i >= scene.uploaded_instances {
            return;
        }
        if scene.changed_mark.len() < scene.instances.len() {
            scene.changed_mark.resize(scene.instances.len(), false);
        }
        if !scene.changed_mark[i] {
            scene.changed_mark[i] = true;
            scene.changed.push(i);
        }
    }

    pub fn alloc_interior_lights(&self, scene: &mut Scene, n: u32) -> u32 {
        if let Some(k) = scene.interior_free.iter().position(|f| f.1 >= n) {
            let (first, len) = scene.interior_free[k];
            if len == n {
                scene.interior_free.remove(k);
            } else {
                scene.interior_free[k] = (first + n, len - n);
            }
            return first;
        }
        let first = scene.interior_lights.len() as u32;
        scene.interior_lights.extend((0..n).map(|_| PointLight {
            intensity: 0.0,
            ..Default::default()
        }));
        first
    }

    pub fn free_interior_lights(&self, scene: &mut Scene, first: u32, n: u32) {
        for l in scene
            .interior_lights
            .iter_mut()
            .skip(first as usize)
            .take(n as usize)
        {
            l.intensity = 0.0;
        }
        scene.interior_free.push((first, n));
    }

    pub fn set_interior_light(&self, scene: &mut Scene, slot: u32, light: PointLight) {
        if let Some(l) = scene.interior_lights.get_mut(slot as usize) {
            *l = light;
        }
    }

    pub fn set_interior_lamps(&self, scene: &mut Scene, instance: usize, first: u32, count: u32) {
        let code = if count == 0 {
            0
        } else {
            first * LAMP_CODE_STRIDE + count.min(MAX_LAMPS_PER_MESH)
        };
        let i = &mut scene.instances[instance];
        if i.interior_lamps != code {
            i.interior_lamps = code;
            Self::mark_changed(scene, instance);
        }
    }

    pub fn set_omsi_caster(&self, scene: &mut Scene, instance: usize, on: bool) {
        if let Some(i) = scene.instances.get_mut(instance) {
            i.omsi_caster = on;
        }
    }

    pub fn set_ordered(&self, scene: &mut Scene, instance: usize, on: bool) {
        if let Some(i) = scene.instances.get_mut(instance) {
            i.ordered = on;
        }
    }

    pub fn set_casts_shadow(&self, scene: &mut Scene, instance: usize, on: bool) {
        if let Some(i) = scene.instances.get_mut(instance) {
            i.casts_shadow = on;
        }
    }

    pub fn set_roof(&self, scene: &mut Scene, instance: usize, roof: Option<f32>) {
        if let Some(i) = scene.instances.get_mut(instance) {
            if i.roof != roof {
                i.roof = roof;
                Self::mark_changed(scene, instance);
            }
        }
    }

    pub fn set_mirror_only(&self, scene: &mut Scene, instance: usize, on: bool) {
        if let Some(i) = scene.instances.get_mut(instance) {
            i.mirror_only = on;
        }
    }

    pub fn set_interior(&self, scene: &mut Scene, instance: usize, interior: f32) {
        let i = &mut scene.instances[instance];
        if (i.interior - interior).abs() > 1e-4 {
            i.interior = interior;
            Self::mark_changed(scene, instance);
        }
    }

    pub fn set_slot_light(&self, scene: &mut Scene, instance: usize, light: &[f32]) {
        let i = &mut scene.instances[instance];
        let mut changed = false;
        for (k, l) in i.slot_light.iter_mut().enumerate() {
            let v = light.get(k).copied().unwrap_or(1.0);
            changed |= *l != v;
            *l = v;
        }
        if changed {
            Self::mark_changed(scene, instance);
        }
    }

    pub fn set_slot_night(&self, scene: &mut Scene, instance: usize, night: &[f32]) {
        let i = &mut scene.instances[instance];
        let mut changed = false;
        for (k, l) in i.slot_night.iter_mut().enumerate() {
            let v = night.get(k).copied().unwrap_or(1.0);
            changed |= *l != v;
            *l = v;
        }
        if changed {
            Self::mark_changed(scene, instance);
        }
    }

    pub fn set_lod_range(&self, scene: &mut Scene, instance: usize, min: f32, max: f32) {
        scene.instances[instance].lod = (min, max);
    }

    pub fn set_object_culling(
        &self,
        scene: &mut Scene,
        instance: usize,
        radius: f32,
        detail: f32,
        any_distance: bool,
    ) {
        let i = &mut scene.instances[instance];
        i.object_radius = radius.max(0.0);
        i.detail = if detail > 0.0 { detail } else { 1.0 };
        i.any_distance = any_distance;
    }

    pub fn set_near_only(&self, scene: &mut Scene, instance: usize, area: Option<[f64; 4]>) {
        scene.instances[instance].near_only = area;
    }

    pub fn set_transform(
        &self,
        scene: &mut Scene,
        instance: usize,
        origin: DVec3,
        transform: Mat4,
    ) {
        let i = &mut scene.instances[instance];
        if i.transform != transform || i.origin != origin {
            i.transform = transform;
            i.origin = origin;
            Self::mark_changed(scene, instance);
        }
    }

    pub fn set_render_origin(&self, scene: &mut Scene, origin: DVec3) {
        if scene.render_origin != origin {
            scene.render_origin = origin;
            scene.dirty = true;
        }
    }

    fn bounding_sphere(scene: &Scene, i: &Instance) -> (Vec3, f32) {
        let b = if scene.cache_bounds {
            i.bounds
        } else {
            InstanceBounds::new(&scene.meshes[i.mesh], i.transform)
        };
        (
            b.centre + (i.origin - scene.render_origin).as_vec3(),
            b.radius,
        )
    }

    fn instance_scale(scene: &Scene, i: &Instance) -> f32 {
        if scene.cache_bounds {
            i.bounds.scale
        } else {
            transform_scale(i.transform)
        }
    }

    fn mesh_bounds_changed(scene: &mut Scene, mesh: MeshId) {
        scene.bounds_meshes.resize(scene.meshes.len(), false);
        scene.bounds_meshes[mesh] = true;
        scene.bounds_dirty = true;
    }

    fn prepare_bounds(scene: &mut Scene) {
        if !scene.cache_bounds {
            return;
        }
        if scene.dirty {
            for i in &mut scene.instances {
                i.bounds = InstanceBounds::new(&scene.meshes[i.mesh], i.transform);
            }
        } else {
            for i in &mut scene.instances[scene.uploaded_instances..] {
                i.bounds = InstanceBounds::new(&scene.meshes[i.mesh], i.transform);
            }
            for &idx in &scene.changed {
                if let Some(i) = scene.instances.get_mut(idx) {
                    i.bounds = InstanceBounds::new(&scene.meshes[i.mesh], i.transform);
                }
            }
            if scene.bounds_dirty {
                for i in &mut scene.instances {
                    if scene.bounds_meshes.get(i.mesh).copied().unwrap_or(false) {
                        i.bounds = InstanceBounds::new(&scene.meshes[i.mesh], i.transform);
                    }
                }
            }
        }
        if scene.bounds_dirty {
            scene.bounds_meshes.fill(false);
            scene.bounds_dirty = false;
        }
    }

    pub fn set_params(
        &self,
        scene: &mut Scene,
        instance: usize,
        slot_alpha: &[f32],
        visible: bool,
        slot_uv: &[[f32; 2]],
    ) {
        let i = &mut scene.instances[instance];
        let mut changed = i.visible != visible;
        for (k, a) in i.slot_alpha.iter_mut().enumerate() {
            let requested = slot_alpha.get(k).copied().unwrap_or(1.0);
            let v = i
                .materials
                .get(k)
                .and_then(|id| scene.materials.get(*id))
                .map_or(requested, |m| {
                    Self::clamp_slot_alpha(requested, m.alpha, m.transmap_declared())
                });
            changed |= *a != v;
            *a = v;
        }
        for (k, u) in i.slot_uv.iter_mut().enumerate() {
            let v = slot_uv.get(k).copied().unwrap_or([0.0; 2]);
            changed |= *u != v;
            *u = v;
        }
        i.visible = visible;
        if changed {
            Self::mark_changed(scene, instance);
        }
    }

    fn install_sky(&mut self, st: atmosphere::SkyState) {
        let mut bytes: Vec<u8> = Vec::with_capacity(st.lut.len() * 8);
        for texel in &st.lut {
            for v in texel {
                bytes.extend_from_slice(&atmosphere::f16_bits(*v).to_le_bytes());
            }
        }
        self.queue.write_texture(
            wgpu::TexelCopyTextureInfo {
                texture: &self.sky_lut,
                mip_level: 0,
                origin: wgpu::Origin3d::ZERO,
                aspect: wgpu::TextureAspect::All,
            },
            &bytes,
            wgpu::TexelCopyBufferLayout {
                offset: 0,
                bytes_per_row: Some(atmosphere::SKY_LUT_W * 8),
                rows_per_image: Some(atmosphere::SKY_LUT_H),
            },
            wgpu::Extent3d {
                width: atmosphere::SKY_LUT_W,
                height: atmosphere::SKY_LUT_H,
                depth_or_array_layers: 1,
            },
        );
        if omsi_cfg::env::var_os("OMSI_DEBUG_SKY").is_some() {
            log::info!(
                "sky: sun {:?} (altitude {:.1}°) sky {:?} ground {:?} exposure {:.3} table scale {:.4} haze {:.2} overcast {:.2} rain {:.2} sun visibility {:.2}",
                st.sun,
                st.input.sun_dir.z.asin().to_degrees(),
                st.sky_horizontal,
                st.ground,
                st.exposure,
                st.lut_scale,
                st.input.haze,
                st.input.overcast,
                st.input.rain,
                st.input.sun_visibility
            );
        }
        if let Some(p) = self.probe.as_mut() {
            p.age = u32::MAX;
        }
        self.sky_state = Some(st);
    }

    fn prepare_enhanced(&mut self, lighting: &Lighting, cam_rel: Vec3, ro: DVec3, dt: f32) -> bool {
        let s = lighting.sun_dir.normalize_or_zero();
        let visibility = 2.3 / lighting.fog_density.max(1e-6);
        let haze = (8000.0 / visibility).clamp(1.0, 6.0) + 2.0 * lighting.rain;
        let wet_cover = (lighting.rain * 1.5).clamp(0.0, 1.0);
        let sun_visibility = lighting.sun_intensity.clamp(0.0, 1.0) * (1.0 - wet_cover);
        let input = atmosphere::SkyInput {
            sun_dir: s,
            sun_visibility,
            overcast: lighting.overcast.clamp(0.0, 1.0).max(wet_cover),
            haze,
            rain: lighting.rain.clamp(0.0, 1.0),
            ground_albedo: 0.2 + 0.45 * lighting.snow.clamp(0.0, 1.0),
            tint: lighting.envir_tint,
            night_light: lighting.atmosphere_brightness,
        };
        if let Some((_, rx)) = &self.sky_job {
            match rx.try_recv() {
                Ok(st) => {
                    self.sky_job = None;
                    self.install_sky(st);
                }
                Err(std::sync::mpsc::TryRecvError::Disconnected) => self.sky_job = None,
                Err(std::sync::mpsc::TryRecvError::Empty) => {}
            }
        }
        let pending = self
            .sky_job
            .as_ref()
            .map(|j| j.0)
            .or(self.sky_state.as_ref().map(|st| st.input));
        if pending
            .map(|i| sky_input_differs(&i, &input))
            .unwrap_or(true)
        {
            if self.instant_exposure || self.sky_state.is_none() {
                self.sky_job = None;
                self.install_sky(atmosphere::SkyState::compute(&input));
            } else {
                let (tx, rx) = std::sync::mpsc::channel();
                std::thread::spawn(move || {
                    let _ = tx.send(atmosphere::SkyState::compute(&input));
                });
                self.sky_job = Some((input, rx));
            }
        }
        let input_overcast = input.overcast;
        let st = self.sky_state.as_ref().expect("sky state");
        let target = st.exposure.max(1e-6).ln();
        let log_exposure = match self.exposure {
            Some(e) if !self.instant_exposure && dt > 0.0 => {
                e + (target - e) * (1.0 - (-dt / 1.5).exp())
            }
            _ => target,
        };
        self.exposure = Some(log_exposure);
        let pre = log_exposure.exp();
        let axes = [Vec3::X, -Vec3::X, Vec3::Y, -Vec3::Y, Vec3::Z, -Vec3::Z];
        let avg = axes
            .iter()
            .map(|a| atmosphere::sh_irradiance(&st.sh, *a))
            .fold(Vec3::ZERO, |a, b| a + b)
            / 6.0;
        let fog_rgb = avg * 0.9 / std::f32::consts::PI;
        let weather_fog = if lighting.fog_density > FOG_MIN_DENSITY {
            lighting.fog_density
        } else {
            0.0
        };
        let clear_air = 0.0;
        let base = match lighting.fog_base.or(lighting.inside.map(|v| v.0.z)) {
            Some(z) => (z - ro.z) as f32,
            None => cam_rel.z - 2.0,
        };
        let mut sh = [[0.0f32; 4]; 9];
        for (k, c) in st.sh.iter().enumerate() {
            sh[k] = c.extend(0.0).to_array();
        }
        let instant = self.instant_exposure;
        let (redraw, probe_scale) = match self.probe.as_mut() {
            Some(p) => {
                let redraw = instant || p.age >= 30;
                if redraw {
                    p.age = 0;
                    p.scale = st.lut_scale;
                } else {
                    p.age += 1;
                }
                (redraw, p.scale)
            }
            None => (false, 1.0),
        };
        let cam_w = ro + cam_rel.as_dvec3();
        let eye_off = match self.probe.as_mut() {
            Some(p) => {
                let to_clouds = (1400.0 - cam_rel.z as f64).max(120.0);
                let far = match p.cube_eye {
                    Some(e) if p.cube_filled => {
                        let m = cam_w - e;
                        (m.truncate().length() * 0.1 + m.z.abs()) / to_clouds > 0.01
                    }
                    _ => true,
                };
                if far {
                    p.cube_eye = Some(cam_w);
                    p.cube_recapture = p.cube_filled;
                }
                (p.cube_eye.unwrap_or(cam_w) - cam_w).as_vec3()
            }
            None => Vec3::ZERO,
        };
        let u = EnhancedUniform {
            exposure: [
                pre,
                2f32.powf(-self.exposure_log.as_ref().map(|l| l.ev).unwrap_or(0.0))
                    .clamp(0.7, 1.6),
                pre * WINDOW_RADIANCE,
                1.6,
            ],
            sun: st.sun.extend(SUN_RADIUS).to_array(),
            sh,
            ground: st.ground.extend(st.lut_scale).to_array(),
            fog: [weather_fog, 1.0 / 300.0, base, clear_air],
            fog_color: fog_rgb.extend(probe_scale).to_array(),
            weather: [
                lighting.wetness.clamp(0.0, 1.0),
                lighting.snow.clamp(0.0, 1.0),
                lighting.rain.clamp(0.0, 1.0),
                input_overcast,
            ],
            lights: [PROBE_MIPS as f32, LAMP_E, CABIN_E, sun_visibility],
            sun_disc: st.sun_disc.extend(dt).to_array(),
            debug: [
                debug_view(),
                omsi_cfg::env::var("OMSI_PUDDLE_F0")
                    .ok()
                    .and_then(|v| v.parse::<f32>().ok())
                    .filter(|v| v.is_finite())
                    .unwrap_or(0.08)
                    .clamp(0.02, 0.2),
                omsi_cfg::env::var("OMSI_ENV_PHOTO")
                    .ok()
                    .and_then(|v| v.parse().ok())
                    .unwrap_or(1.0),
                0.0,
            ],
            eye: eye_off.extend(0.0).to_array(),
            led: [lighting.led_glow, lighting.led_mips, 0.0, 0.0],
        };
        self.queue
            .write_buffer(&self.enh_buf, 0, bytemuck::bytes_of(&u));
        redraw
    }

    #[allow(dead_code)]
    fn ensure_depth(&mut self, w: u32, h: u32) {
        if let Some((_, _, dw, dh)) = &self.depth {
            if *dw == w && *dh == h {
                return;
            }
        }
        let tex = self.device.create_texture(&wgpu::TextureDescriptor {
            label: Some("depth"),
            size: wgpu::Extent3d {
                width: w,
                height: h,
                depth_or_array_layers: 1,
            },
            mip_level_count: 1,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format: DEPTH_FORMAT,
            usage: wgpu::TextureUsages::RENDER_ATTACHMENT,
            view_formats: &[],
        });
        let view = tex.create_view(&Default::default());
        self.depth = Some((tex, view, w, h));
    }

    fn instance_entries(
        i: &Instance,
        ro: DVec3,
        mats: &mut Vec<[[f32; 4]; 4]>,
        params: &mut Vec<[f32; 4]>,
    ) {
        let m =
            (Mat4::from_translation((i.origin - ro).as_vec3()) * i.transform).to_cols_array_2d();
        let vis = if i.visible { 1.0 } else { 0.0 };
        for (k, a) in i.slot_alpha.iter().enumerate() {
            mats.push(m);
            let uv = i.slot_uv.get(k).copied().unwrap_or([0.0; 2]);
            params.push([*a, vis, uv[0], uv[1]]);
            params.push([
                i.slot_light.get(k).copied().unwrap_or(1.0),
                i.slot_night.get(k).copied().unwrap_or(1.0),
                if i.interior_lamps != 0 {
                    i.interior_lamps as f32
                } else {
                    i.interior.min(0.99)
                },
                if let Some(roof) = i.roof.filter(|_| !i.blob && !i.surface) {
                    let z = (i.origin - ro).z as f32
                        + i.transform.transform_point3(Vec3::new(0.0, 0.0, roof)).z;
                    -(5000.0 + z.clamp(-4000.0, 4000.0))
                } else {
                    surface_instance_code(
                        i.blob,
                        i.ground_layer,
                        i.decal,
                        i.surface,
                        i.surface_bias,
                    )
                },
            ]);
        }
    }

    pub fn prepare(&self, scene: &mut Scene) {
        Self::prepare_bounds(scene);
        scene.bind_groups.clear();
        if !scene.dirty
            && scene.instances.len() > scene.uploaded_instances
            && scene.model_buf.is_some()
        {
            let ro = scene.render_origin;
            let mut mats: Vec<[[f32; 4]; 4]> = Vec::new();
            let mut params: Vec<[f32; 4]> = Vec::new();
            let first = scene.uploaded_instances;
            let mut base = scene.uploaded_entries;
            for i in scene.instances[first..].iter_mut() {
                i.base = base;
                Self::instance_entries(i, ro, &mut mats, &mut params);
                base = scene.uploaded_entries + mats.len() as u32;
            }
            let (buf, pbuf) = (
                scene.model_buf.as_ref().unwrap(),
                scene.params_buf.as_ref().unwrap(),
            );
            let mo = scene.uploaded_entries as u64 * 64;
            let po = scene.uploaded_entries as u64 * 32;
            let mb: &[u8] = bytemuck::cast_slice(&mats);
            let pb: &[u8] = bytemuck::cast_slice(&params);
            if mo + mb.len() as u64 <= buf.size()
                && po + pb.len() as u64 <= pbuf.size()
                && scene.cpu_models.len() == scene.uploaded_entries as usize
            {
                if !mb.is_empty() {
                    self.queue.write_buffer(buf, mo, mb);
                    self.queue.write_buffer(pbuf, po, pb);
                }
                scene.cpu_models.extend_from_slice(&mats);
                scene.cpu_params.extend_from_slice(&params);
                scene.uploaded_instances = scene.instances.len();
                scene.uploaded_entries = base;
            } else {
                scene.dirty = true;
            }
        }
        if !scene.dirty {
            if !scene.changed.is_empty() {
                if omsi_cfg::env::var_os("OMSI_DEBUG_DRAWS").is_some() {
                    log::info!(
                        "prepare: {} changed instances of {}",
                        scene.changed.len(),
                        scene.instances.len()
                    );
                }
                if let (Some(buf), Some(pbuf)) = (&scene.model_buf, &scene.params_buf) {
                    let ro = scene.render_origin;
                    const MERGE_GAP: u32 = 4096;
                    scene.changed.sort_unstable_by_key(|&i| {
                        scene.instances.get(i).map(|x| x.base).unwrap_or(u32::MAX)
                    });
                    let mut mats: Vec<[[f32; 4]; 4]> = Vec::new();
                    let mut params: Vec<[f32; 4]> = Vec::new();
                    let mut ranges: Vec<(u32, u32)> = Vec::new();
                    for &idx in &scene.changed {
                        let Some(i) = scene.instances.get(idx) else {
                            continue;
                        };
                        mats.clear();
                        params.clear();
                        Self::instance_entries(i, ro, &mut mats, &mut params);
                        let (b, n) = (i.base as usize, mats.len());
                        if b + n > scene.cpu_models.len() || (b + n) * 2 > scene.cpu_params.len() {
                            continue;
                        }
                        scene.cpu_models[b..b + n].copy_from_slice(&mats);
                        scene.cpu_params[b * 2..(b + n) * 2].copy_from_slice(&params);
                        let (start, end) = (b as u32, (b + n) as u32);
                        match ranges.last_mut() {
                            Some(r) if start <= r.1 + MERGE_GAP => r.1 = r.1.max(end),
                            _ => ranges.push((start, end)),
                        }
                    }
                    for (start, end) in ranges {
                        let mb: &[u8] =
                            bytemuck::cast_slice(&scene.cpu_models[start as usize..end as usize]);
                        let pb: &[u8] = bytemuck::cast_slice(
                            &scene.cpu_params[start as usize * 2..end as usize * 2],
                        );
                        let (mo, po) = (start as u64 * 64, start as u64 * 32);
                        if mo + mb.len() as u64 <= buf.size() && po + pb.len() as u64 <= pbuf.size()
                        {
                            self.queue.write_buffer(buf, mo, mb);
                            self.queue.write_buffer(pbuf, po, pb);
                        }
                    }
                }
                for &idx in &scene.changed {
                    if let Some(m) = scene.changed_mark.get_mut(idx) {
                        *m = false;
                    }
                }
                scene.changed.clear();
            }
            return;
        }
        scene.changed.clear();
        scene.changed_mark.clear();
        let mut mats: Vec<[[f32; 4]; 4]> = Vec::new();
        let mut params: Vec<[f32; 4]> = Vec::new();
        let ro = scene.render_origin;
        for i in scene.instances.iter_mut() {
            i.base = mats.len() as u32;
            Self::instance_entries(i, ro, &mut mats, &mut params);
        }
        if mats.is_empty() {
            mats.push(Mat4::IDENTITY.to_cols_array_2d());
            params.push([1.0, 1.0, 0.0, 0.0]);
            params.push([1.0, 1.0, 0.0, 0.0]);
        }
        scene.uploaded_instances = scene.instances.len();
        scene.uploaded_entries = mats.len() as u32;
        scene.cpu_models = mats;
        scene.cpu_params = params;
        let bytes: &[u8] = bytemuck::cast_slice(&scene.cpu_models);
        let pbytes: &[u8] = bytemuck::cast_slice(&scene.cpu_params);
        if let (Some(buf), Some(pbuf), Some(_)) = (
            &scene.model_buf,
            &scene.params_buf,
            &scene.camera_bind_group,
        ) {
            if buf.size() as usize >= bytes.len() && pbuf.size() as usize >= pbytes.len() {
                self.queue.write_buffer(buf, 0, bytes);
                self.queue.write_buffer(pbuf, 0, pbytes);
                scene.dirty = false;
                return;
            }
        }
        let cap = |n: usize| (((n as f64 * 1.35) as u64 + 65536).max(256)).div_ceil(256) * 256;
        let model_buf = self.device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("models"),
            size: cap(bytes.len()),
            usage: wgpu::BufferUsages::STORAGE | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });
        let params_buf = self.device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("params"),
            size: cap(pbytes.len()),
            usage: wgpu::BufferUsages::STORAGE | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });
        self.queue.write_buffer(&model_buf, 0, bytes);
        self.queue.write_buffer(&params_buf, 0, pbytes);
        scene.model_buf = Some(model_buf);
        scene.params_buf = Some(params_buf);
        self.rebuild_camera_bind_group(scene);
        scene.dirty = false;
    }

    fn rebuild_camera_bind_group(&self, scene: &mut Scene) {
        let (Some(model_buf), Some(params_buf), Some(light_buf), Some(grid_buf), Some(draw_buf)) = (
            &scene.model_buf,
            &scene.params_buf,
            &scene.light_buf,
            &scene.grid_buf,
            &scene.draw_buf,
        ) else {
            return;
        };
        let bg = self.device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("camera"),
            layout: &self.camera_layout,
            entries: &[
                wgpu::BindGroupEntry {
                    binding: 0,
                    resource: self.camera_buf.as_entire_binding(),
                },
                wgpu::BindGroupEntry {
                    binding: 1,
                    resource: model_buf.as_entire_binding(),
                },
                wgpu::BindGroupEntry {
                    binding: 2,
                    resource: params_buf.as_entire_binding(),
                },
                wgpu::BindGroupEntry {
                    binding: 3,
                    resource: light_buf.as_entire_binding(),
                },
                wgpu::BindGroupEntry {
                    binding: 4,
                    resource: grid_buf.as_entire_binding(),
                },
                wgpu::BindGroupEntry {
                    binding: 5,
                    resource: wgpu::BindingResource::TextureView(&self.shadow_view),
                },
                wgpu::BindGroupEntry {
                    binding: 6,
                    resource: wgpu::BindingResource::Sampler(&self.shadow_sampler),
                },
                wgpu::BindGroupEntry {
                    binding: 7,
                    resource: wgpu::BindingResource::TextureView(&self.shadow_view_far),
                },
                wgpu::BindGroupEntry {
                    binding: 8,
                    resource: wgpu::BindingResource::TextureView(
                        self.ao
                            .as_ref()
                            .map(|a| &a.blur_view)
                            .unwrap_or(&self.white_texture.view),
                    ),
                },
                wgpu::BindGroupEntry {
                    binding: 9,
                    resource: wgpu::BindingResource::Sampler(&self.ao_sampler),
                },
                wgpu::BindGroupEntry {
                    binding: 10,
                    resource: draw_buf.as_entire_binding(),
                },
                wgpu::BindGroupEntry {
                    binding: 11,
                    resource: self.enh_buf.as_entire_binding(),
                },
                wgpu::BindGroupEntry {
                    binding: 12,
                    resource: wgpu::BindingResource::TextureView(
                        &self.probe.as_ref().expect("reflection probe").view,
                    ),
                },
                wgpu::BindGroupEntry {
                    binding: 13,
                    resource: wgpu::BindingResource::Sampler(&self.lin_sampler),
                },
                wgpu::BindGroupEntry {
                    binding: 14,
                    resource: wgpu::BindingResource::TextureView(&self.sky_lut_view),
                },
                wgpu::BindGroupEntry {
                    binding: 17,
                    resource: wgpu::BindingResource::TextureView(
                        &self.probe.as_ref().expect("reflection probe").cube_view,
                    ),
                },
                wgpu::BindGroupEntry {
                    binding: 18,
                    resource: wgpu::BindingResource::TextureView(&self.lm_atlas_view),
                },
                wgpu::BindGroupEntry {
                    binding: 19,
                    resource: self.lm_uniform.as_entire_binding(),
                },
            ],
        });
        scene.camera_bind_group = Some(bg);
        let sbg = self.device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("shadow camera"),
            layout: &self.shadow_layout,
            entries: &[
                wgpu::BindGroupEntry {
                    binding: 0,
                    resource: self.camera_buf.as_entire_binding(),
                },
                wgpu::BindGroupEntry {
                    binding: 1,
                    resource: model_buf.as_entire_binding(),
                },
                wgpu::BindGroupEntry {
                    binding: 2,
                    resource: params_buf.as_entire_binding(),
                },
                wgpu::BindGroupEntry {
                    binding: 10,
                    resource: draw_buf.as_entire_binding(),
                },
            ],
        });
        scene.shadow_bind_group = Some(sbg);
        scene.spot_bind_groups = self
            .spot_cam_bufs
            .iter()
            .map(|buf| {
                self.device.create_bind_group(&wgpu::BindGroupDescriptor {
                    label: Some("spot shadow camera"),
                    layout: &self.shadow_layout,
                    entries: &[
                        wgpu::BindGroupEntry {
                            binding: 0,
                            resource: buf.as_entire_binding(),
                        },
                        wgpu::BindGroupEntry {
                            binding: 1,
                            resource: model_buf.as_entire_binding(),
                        },
                        wgpu::BindGroupEntry {
                            binding: 2,
                            resource: params_buf.as_entire_binding(),
                        },
                        wgpu::BindGroupEntry {
                            binding: 10,
                            resource: draw_buf.as_entire_binding(),
                        },
                    ],
                })
            })
            .collect();
    }

    fn plan_spot_shadows(&self, scene: &Scene, cam: DVec3, enhanced: bool, plan: bool) -> Vec<u32> {
        let mut out = vec![0u32; scene.lights.len()];
        let mut st = self.spot_state.borrow_mut();
        let mut cands: Vec<(f32, usize, SpotPose)> = Vec::new();
        for (i, l) in scene.lights.iter().enumerate() {
            if !drawn_by(l, enhanced) {
                continue;
            }
            let d = (l.position - cam).length();
            if d > SPOT_CAM_RANGE {
                continue;
            }
            let far = l.radius.clamp(6.0, SPOT_RANGE_MAX);
            let pose = if l.direction.length_squared() < 1e-6 {
                SpotPose {
                    pos: l.position,
                    dir: -Vec3::Z,
                    fov: 2.5,
                    far,
                }
            } else {
                if l.cone[1] <= -0.99 {
                    continue;
                }
                let half = l.cone[1].clamp(-1.0, 1.0).acos();
                SpotPose {
                    pos: l.position,
                    dir: l.direction.normalize(),
                    fov: (2.0 * half + 0.09).clamp(0.2, 2.6),
                    far,
                }
            };
            let score = l.intensity.max(0.05) * far * far / (1.0 + (d * d) as f32);
            cands.push((score, i, pose));
        }
        cands.sort_by(|a, b| b.0.total_cmp(&a.0));
        cands.truncate(SPOT_SLOTS);
        let mut slots = st.slots;
        let mut claimed = [false; SPOT_SLOTS];
        let mut assign: Vec<Option<usize>> = vec![None; cands.len()];
        for (ci, (_, _, pose)) in cands.iter().enumerate() {
            let mut best: Option<usize> = None;
            let mut best_d = 2.5f64;
            for (k, sl) in slots.iter().enumerate() {
                if claimed[k] {
                    continue;
                }
                if let Some(seen) = sl.seen {
                    let dd = (seen.pos - pose.pos).length();
                    if dd < best_d && seen.dir.dot(pose.dir) > 0.7 {
                        best = Some(k);
                        best_d = dd;
                    }
                }
            }
            if let Some(k) = best {
                claimed[k] = true;
                assign[ci] = Some(k);
            }
        }
        for a in assign.iter_mut() {
            if a.is_none() {
                if let Some(k) = (0..SPOT_SLOTS).find(|&k| !claimed[k]) {
                    claimed[k] = true;
                    slots[k] = SpotSlot::default();
                    *a = Some(k);
                }
            }
        }
        for k in 0..SPOT_SLOTS {
            if !claimed[k] {
                slots[k] = SpotSlot::default();
            }
        }
        if plan {
            let mut wants: Vec<(f32, usize)> = Vec::new();
            for (ci, (score, _, pose)) in cands.iter().enumerate() {
                let Some(k) = assign[ci] else { continue };
                slots[k].age += 1;
                let prio = match slots[k].drawn {
                    None => 1000.0 + *score,
                    Some(d) => {
                        let moved = (d.pos - pose.pos).length() > 0.04 || d.dir.dot(pose.dir) < 0.99999;
                        if moved {
                            10.0 + *score
                        } else if slots[k].age >= SPOT_REDRAW_AGE {
                            1.0 + slots[k].age as f32 * 0.01
                        } else {
                            slots[k].seen = Some(*pose);
                            continue;
                        }
                    }
                };
                slots[k].seen = Some(*pose);
                wants.push((prio, k));
            }
            for (ci, (_, _, pose)) in cands.iter().enumerate() {
                if let Some(k) = assign[ci] {
                    slots[k].seen = Some(*pose);
                }
            }
            wants.sort_by(|a, b| b.0.total_cmp(&a.0));
            st.draws.clear();
            for &(_, k) in wants.iter().take(SPOT_DRAWS_PER_FRAME) {
                slots[k].drawn = slots[k].seen;
                slots[k].age = 0;
                st.draws.push(k);
            }
            st.slots = slots;
            if omsi_cfg::env::var_os("OMSI_DEBUG_LIGHT_SHADOWS").is_some() {
                static LAST: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
                let now = self.started.elapsed().as_secs();
                if LAST.swap(now, std::sync::atomic::Ordering::Relaxed) != now {
                    log::info!(
                        "light shadows: {} lights, {} candidates in reach, {} drawn this frame",
                        scene.lights.len(),
                        cands.len(),
                        st.draws.len()
                    );
                }
            }
        }
        for (ci, (_, li, _)) in cands.iter().enumerate() {
            if let Some(k) = assign[ci] {
                if slots[k].drawn.is_some() {
                    out[*li] = k as u32 + 1;
                }
            }
        }
        out
    }

    fn prepare_lights(
        &self,
        scene: &mut Scene,
        cam_rel: Vec3,
        enhanced: bool,
        plan_spots: bool,
    ) -> [f32; 4] {
        let ro = scene.render_origin;
        let spot_slots = self.plan_spot_shadows(scene, ro + cam_rel.as_dvec3(), enhanced, plan_spots);
        let side = LIGHT_GRID_SIDE;
        let half = side as f32 * LIGHT_CELL * 0.5;
        let origin = [
            ((cam_rel.x - half) / LIGHT_CELL).floor() * LIGHT_CELL,
            ((cam_rel.y - half) / LIGHT_CELL).floor() * LIGHT_CELL,
        ];
        let mut gpu_lights: Vec<GpuPointLight> =
            Vec::with_capacity(scene.interior_lights.len() + scene.lights.len().max(1));
        for l in &scene.interior_lights {
            gpu_lights.push(gpu_light(l, (l.position - ro).as_vec3()));
        }
        let mut grid = vec![u32::MAX; side * side * LIGHT_CELL_CAP];
        let mut occ_users: Vec<(usize, u32)> = Vec::new();
        for (li, l) in scene.lights.iter().enumerate() {
            if !drawn_by(l, enhanced) {
                continue;
            }
            let p = (l.position - ro).as_vec3();
            let x0 = ((p.x - l.radius - origin[0]) / LIGHT_CELL).floor();
            let x1 = ((p.x + l.radius - origin[0]) / LIGHT_CELL).floor();
            let y0 = ((p.y - l.radius - origin[1]) / LIGHT_CELL).floor();
            let y1 = ((p.y + l.radius - origin[1]) / LIGHT_CELL).floor();
            if x1 < 0.0 || y1 < 0.0 || x0 >= side as f32 || y0 >= side as f32 {
                continue;
            }
            let idx = gpu_lights.len() as u32;
            gpu_lights.push(gpu_light(l, p));
            gpu_lights[idx as usize].occ[2] = spot_slots[li] as f32;
            if l.occ_count > 0 && (l.occ_first as usize + l.occ_count as usize) <= scene.occluders.len() {
                occ_users.push((idx as usize, l.occ_first));
                gpu_lights[idx as usize].occ[1] = l.occ_count as f32;
            }
            for y in (y0.max(0.0) as usize)..=(y1.min(side as f32 - 1.0) as usize) {
                for x in (x0.max(0.0) as usize)..=(x1.min(side as f32 - 1.0) as usize) {
                    let base = (y * side + x) * LIGHT_CELL_CAP;
                    if let Some(slot) = grid[base..base + LIGHT_CELL_CAP]
                        .iter()
                        .position(|v| *v == u32::MAX)
                    {
                        grid[base + slot] = idx;
                    } else if omsi_cfg::env::var_os("OMSI_DEBUG_LIGHT_GRID").is_some() {
                        log::info!(
                            "light grid: cell ({x}, {y}) full, light at ({:.1}, {:.1}) radius {:.0} {} left out",
                            p.x,
                            p.y,
                            l.radius,
                            if l.direction.length_squared() > 0.5 {
                                "spot"
                            } else {
                                "point"
                            }
                        );
                    }
                }
            }
        }
        if !occ_users.is_empty() {
            let base = gpu_lights.len() as u32;
            for o in &scene.occluders {
                if let Some(t) = o.tri {
                    let v = t.map(|p| (p - ro).as_vec3());
                    gpu_lights.push(GpuPointLight {
                        pos: [v[0].x, v[0].y, v[0].z, 0.0],
                        color: [v[1].x, v[1].y, v[1].z, 0.0],
                        dir: [v[2].x, v[2].y, v[2].z, 0.0],
                        extra: [1.0, 0.0, 0.0, 0.0],
                        occ: [0.0; 4],
                    });
                    continue;
                }
                let c = o.center - ro.truncate();
                let (sa, ca) = (o.heading.sin() as f32, o.heading.cos() as f32);
                gpu_lights.push(GpuPointLight {
                    pos: [c.x as f32, c.y as f32, (o.z0 - ro.z) as f32, o.half.x],
                    color: [o.half.y, (o.z1 - ro.z) as f32, ca, sa],
                    dir: [0.0; 4],
                    extra: [0.0; 4],
                    occ: [0.0; 4],
                });
            }
            for (i, first) in occ_users {
                gpu_lights[i].occ[0] = (base + first) as f32;
            }
        }
        if gpu_lights.is_empty() {
            gpu_lights.push(GpuPointLight {
                pos: [0.0; 4],
                color: [0.0; 4],
                dir: [0.0; 4],
                extra: [0.0; 4],
                occ: [0.0; 4],
            });
        }
        let lbytes: &[u8] = bytemuck::cast_slice(&gpu_lights);
        let gbytes: &[u8] = bytemuck::cast_slice(&grid);
        let mut rebuilt = false;
        match &scene.light_buf {
            Some(b) if b.size() as usize >= lbytes.len() => {
                if scene.last_lights != lbytes {
                    self.queue.write_buffer(b, 0, lbytes);
                }
            }
            _ => {
                let cap = (lbytes.len() * 2).max(64 * std::mem::size_of::<GpuPointLight>());
                let b = self.device.create_buffer(&wgpu::BufferDescriptor {
                    label: Some("lights"),
                    size: cap as u64,
                    usage: wgpu::BufferUsages::STORAGE | wgpu::BufferUsages::COPY_DST,
                    mapped_at_creation: false,
                });
                self.queue.write_buffer(&b, 0, lbytes);
                scene.light_buf = Some(b);
                rebuilt = true;
            }
        }
        match &scene.grid_buf {
            Some(b) => {
                if scene.last_grid != grid {
                    self.queue.write_buffer(b, 0, gbytes);
                }
            }
            None => {
                scene.grid_buf = Some(buffer_init(
                    &self.device,
                    &self.queue,
                    Some("light grid"),
                    gbytes,
                    wgpu::BufferUsages::STORAGE | wgpu::BufferUsages::COPY_DST,
                ));
                rebuilt = true;
            }
        }
        scene.last_lights.clear();
        scene.last_lights.extend_from_slice(lbytes);
        scene.last_grid = grid;
        if rebuilt {
            self.rebuild_camera_bind_group(scene);
        }
        [origin[0], origin[1], LIGHT_CELL, side as f32]
    }

    fn collect_gpu_timers(&mut self) {
        let period = self.queue.get_timestamp_period() as f64;
        for t in self.gpu_timers.iter_mut().flatten() {
            t.collect(period);
            if t.unresolved {
                t.unresolved = false;
                let n = t.pending.len() as u32 * 2;
                let mut enc = self
                    .device
                    .create_command_encoder(&wgpu::CommandEncoderDescriptor {
                        label: Some("pass timers"),
                    });
                enc.resolve_query_set(&t.set, 0..n, &t.resolve, 0);
                enc.copy_buffer_to_buffer(&t.resolve, 0, &t.read, 0, n as u64 * 8);
                self.queue.submit([enc.finish()]);
                let ready = t.ready.clone();
                t.read.map_async(wgpu::MapMode::Read, .., move |r| {
                    ready.store(r.is_ok(), std::sync::atomic::Ordering::Relaxed)
                });
                t.waiting = true;
            }
        }
    }

    pub fn gpu_pass_times(&self) -> Vec<(String, f64, u32)> {
        let mut out = Vec::new();
        for (k, t) in self.gpu_timers.iter().enumerate() {
            let Some(t) = t else { continue };
            for (label, v) in &t.totals {
                let name = if k == 0 {
                    format!("mirrors: {label}")
                } else {
                    label.to_string()
                };
                out.push((name, v.0 / v.1.max(1) as f64 * 1000.0, v.1));
            }
        }
        out
    }
}

impl GpuTimers {
    fn collect(&mut self, period: f64) {
        let t = self;
        if !t.waiting || !t.ready.swap(false, std::sync::atomic::Ordering::Relaxed) {
            return;
        }
        let n = t.pending.len() * 2;
        {
            let view = t
                .read
                .slice(0..n as u64 * 8)
                .get_mapped_range()
                .expect("mapped range");
            let stamps: &[u64] = bytemuck::cast_slice(&view[..n * 8]);
            if omsi_cfg::env::var_os("OMSI_GPU_TIMERS_RAW").is_some() {
                log::info!(
                    "gpu stamps: {:?}",
                    t.pending
                        .iter()
                        .enumerate()
                        .map(|(k, label)| (*label, stamps[k * 2], stamps[k * 2 + 1]))
                        .collect::<Vec<_>>()
                );
            }
            let mut order: Vec<(u64, u64, &'static str)> = t
                .pending
                .iter()
                .enumerate()
                .map(|(k, label)| (stamps[k * 2], stamps[k * 2 + 1], *label))
                .filter(|(a, b, _)| *b >= *a && *b > 0)
                .collect();
            order.sort_by_key(|(_, b, _)| *b);
            let mut prev: Option<u64> = None;
            for (a, b, label) in &order {
                let from = prev.unwrap_or(*a);
                let e = t.totals.entry(label).or_default();
                e.0 += b.saturating_sub(from) as f64 * period * 1e-9;
                e.1 += 1;
                prev = Some(*b);
            }
            if let (Some(first), Some(last)) = (order.iter().map(|o| o.0).min(), order.last()) {
                let e = t.totals.entry("(all passes)").or_default();
                e.0 += last.1.saturating_sub(first) as f64 * period * 1e-9;
                e.1 += 1;
            }
        }
        t.read.unmap();
        t.waiting = false;
    }
}

impl Renderer {
    fn upload_draw_list(&self, scene: &mut Scene, list: &[u32]) {
        let bytes: &[u8] = bytemuck::cast_slice(if list.is_empty() { &[0u32] } else { list });
        let fits = scene
            .draw_buf
            .as_ref()
            .map(|b| b.size() >= bytes.len() as u64)
            .unwrap_or(false);
        if !fits {
            let cap = (bytes.len() as u64 * 3 / 2).max(1 << 16).div_ceil(4) * 4;
            scene.draw_buf = Some(self.device.create_buffer(&wgpu::BufferDescriptor {
                label: Some("draw list"),
                size: cap,
                usage: wgpu::BufferUsages::STORAGE | wgpu::BufferUsages::COPY_DST,
                mapped_at_creation: false,
            }));
            self.rebuild_camera_bind_group(scene);
        } else if scene.camera_bind_group.is_none() || scene.shadow_bind_group.is_none() {
            self.rebuild_camera_bind_group(scene);
        }
        if let Some(b) = &scene.draw_buf {
            self.queue.write_buffer(b, 0, bytes);
        }
    }

    fn prepare_smoke(&self, scene: &mut Scene, eye: DVec3) {
        let ro = scene.render_origin;
        let mut order: Vec<(f64, usize)> = scene
            .smoke
            .iter()
            .enumerate()
            .filter(|(_, p)| p.alpha > 0.002 && p.size > 0.0)
            .map(|(i, p)| (-(p.position - eye).length_squared(), i))
            .collect();
        order.sort_by(|a, b| a.0.total_cmp(&b.0));
        let data: Vec<GpuCorona> = order
            .iter()
            .map(|&(_, i)| {
                let p = &scene.smoke[i];
                GpuCorona {
                    pos: (p.position - ro).as_vec3().to_array(),
                    size: p.size,
                    color: [p.color[0], p.color[1], p.color[2], p.alpha.clamp(0.0, 1.0)],
                    dir: [0.0, 0.0, 0.0, -1.0],
                    up: [0.0, 0.0, 1.0, 2.0],
                    extra: [-2.0, 0.0, 0.0, 1.0],
                }
            })
            .collect();
        scene.smoke_count = data.len() as u32;
        if data.is_empty() {
            return;
        }
        let bytes: &[u8] = bytemuck::cast_slice(&data);
        match &scene.smoke_buf {
            Some(b) if b.size() as usize >= bytes.len() => self.queue.write_buffer(b, 0, bytes),
            _ => {
                let b = self.device.create_buffer(&wgpu::BufferDescriptor {
                    label: Some("smoke"),
                    size: (bytes.len() * 2) as u64,
                    usage: wgpu::BufferUsages::VERTEX | wgpu::BufferUsages::COPY_DST,
                    mapped_at_creation: false,
                });
                self.queue.write_buffer(&b, 0, bytes);
                scene.smoke_buf = Some(b);
            }
        }
    }

    pub fn set_light_map_tile(&self, slot: (u32, u32), img: Option<&omsi_texture::Image>) {
        let n = LM_TILE_PX as usize;
        let mut px = vec![0u8; n * n * 4];
        if let Some(img) = img.filter(|i| i.width > 0 && i.height > 0) {
            for y in 0..n {
                for x in 0..n {
                    let sx = x * img.width as usize / n;
                    let sy = y * img.height as usize / n;
                    let o = (sy * img.width as usize + sx) * 4;
                    px[(y * n + x) * 4..(y * n + x) * 4 + 4].copy_from_slice(&img.rgba[o..o + 4]);
                }
            }
        }
        self.queue.write_texture(
            wgpu::TexelCopyTextureInfo {
                texture: &self.lm_atlas,
                mip_level: 0,
                origin: wgpu::Origin3d {
                    x: slot.0 * LM_TILE_PX,
                    y: slot.1 * LM_TILE_PX,
                    z: 0,
                },
                aspect: wgpu::TextureAspect::All,
            },
            &px,
            wgpu::TexelCopyBufferLayout {
                offset: 0,
                bytes_per_row: Some(LM_TILE_PX * 4),
                rows_per_image: None,
            },
            wgpu::Extent3d {
                width: LM_TILE_PX,
                height: LM_TILE_PX,
                depth_or_array_layers: 1,
            },
        );
    }

    pub fn set_light_map_place(&self, x: f64, y: f64, side: f64) {
        self.lm_place.set((x, y, side));
    }

    pub fn set_corona_texture(&mut self, id: u16, img: &omsi_texture::Image) {
        let t = upload_texture(&self.device, &self.queue, img, true);
        let bg = self.device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("corona picture"),
            layout: &self.corona_layout,
            entries: &[
                wgpu::BindGroupEntry {
                    binding: 0,
                    resource: wgpu::BindingResource::TextureView(&t.view),
                },
                wgpu::BindGroupEntry {
                    binding: 1,
                    resource: wgpu::BindingResource::Sampler(&self.corona_sampler),
                },
            ],
        });
        let i = id as usize;
        if self.corona_textures.len() <= i {
            self.corona_textures.resize_with(i + 1, || None);
        }
        self.corona_textures[i] = Some(bg);
    }

    pub fn set_smoke_texture(&mut self, img: &omsi_texture::Image) {
        let t = upload_texture(&self.device, &self.queue, img, true);
        self.smoke_bind_group = self.device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("smoke"),
            layout: &self.corona_layout,
            entries: &[
                wgpu::BindGroupEntry {
                    binding: 0,
                    resource: wgpu::BindingResource::TextureView(&t.view),
                },
                wgpu::BindGroupEntry {
                    binding: 1,
                    resource: wgpu::BindingResource::Sampler(&self.corona_sampler),
                },
            ],
        });
    }

    fn prepare_coronas(&self, scene: &mut Scene, night: f32) {
        let ro = scene.render_origin;
        let mut order: Vec<&Corona> = scene
            .coronas
            .iter()
            .filter(|c| c.brightness > 0.001)
            .collect();
        order.sort_by_key(|c| c.texture);
        let mut runs: Vec<(u16, u32, u32)> = Vec::new();
        for (k, c) in order.iter().enumerate() {
            match runs.last_mut() {
                Some(r) if r.0 == c.texture => r.2 += 1,
                _ => runs.push((c.texture, k as u32, 1)),
            }
        }
        if omsi_cfg::env::var_os("OMSI_DEBUG_CONES").is_some() {
            log::info!(
                "coronas: {} in {} runs {:?}, {} beams",
                order.len(),
                runs.len(),
                runs,
                order.iter().filter(|c| c.beam).count()
            );
        }
        scene.corona_runs = runs;
        let data: Vec<GpuCorona> = order
            .into_iter()
            .map(|c| {
                let p = (c.position - ro).as_vec3();
                let b = if c.cone_cos < -1.5 || c.beam || c.halo {
                    c.brightness
                } else {
                    c.brightness * (night * night + 0.8) * 0.6
                };
                GpuCorona {
                    pos: p.to_array(),
                    size: c.size,
                    color: [c.color[0], c.color[1], c.color[2], b],
                    dir: [c.direction.x, c.direction.y, c.direction.z, c.cone_cos],
                    up: [c.up.x, c.up.y, c.up.z, c.rotating as f32],
                    extra: [
                        c.inner_cos,
                        if c.beam || c.halo {
                            c.beam_width
                        } else {
                            c.z_offset
                        },
                        c.flags as f32,
                        if c.beam {
                            1.0
                        } else if c.halo {
                            2.0
                        } else {
                            0.0
                        },
                    ],
                }
            })
            .collect();
        scene.corona_count = data.len() as u32;
        if data.is_empty() {
            return;
        }
        let bytes: &[u8] = bytemuck::cast_slice(&data);
        match &scene.corona_buf {
            Some(b) if b.size() as usize >= bytes.len() => self.queue.write_buffer(b, 0, bytes),
            _ => {
                let b = self.device.create_buffer(&wgpu::BufferDescriptor {
                    label: Some("coronas"),
                    size: (bytes.len() * 2) as u64,
                    usage: wgpu::BufferUsages::VERTEX | wgpu::BufferUsages::COPY_DST,
                    mapped_at_creation: false,
                });
                self.queue.write_buffer(&b, 0, bytes);
                scene.corona_buf = Some(b);
            }
        }
    }

    pub fn lighten(&mut self) -> Option<&'static str> {
        if self.options.ssao {
            self.options.ssao = false;
            return Some("ambient occlusion (SSAO) off");
        }
        None
    }

    fn fall_back_to_single_sample(&mut self, scene: &mut Scene) {
        log::error!(
            "{}x MSAA failed on {}; drawing without multisampling from now on",
            self.options.msaa,
            self.adapter_name
        );
        let options = RenderOptions {
            msaa: 1,
            ..self.options
        };
        *self = Self::build(
            self.device.clone(),
            self.queue.clone(),
            self.adapter_name.clone(),
            self.format,
            options,
        );
        scene.dirty = true;
        scene.model_buf = None;
        scene.params_buf = None;
        scene.camera_bind_group = None;
        scene.shadow_bind_group = None;
        scene.spot_bind_groups.clear();
    }

    pub fn render(
        &mut self,
        scene: &mut Scene,
        target: &wgpu::TextureView,
        width: u32,
        height: u32,
        camera: &Camera,
        lighting: &Lighting,
    ) {
        self.render_inner(
            scene, target, width, height, camera, lighting, true, None, None, false,
        );
    }

    pub fn render_xr_eye(
        &mut self,
        scene: &mut Scene,
        target: &wgpu::TextureView,
        width: u32,
        height: u32,
        camera: &Camera,
        lighting: &Lighting,
        projection: Mat4,
        second_eye: bool,
    ) {
        self.render_inner(
            scene,
            target,
            width,
            height,
            camera,
            lighting,
            false,
            None,
            Some(projection),
            second_eye,
        );
    }

    pub fn render_xr_ui(
        &self,
        scene: &Scene,
        eyes: &[wgpu::TextureView; 2],
        desktop_size: (u32, u32),
        eye_size: (u32, u32),
        menu_range: std::ops::Range<usize>,
        menu_transforms: [Mat4; 2],
        cursor_overlay: Option<usize>,
        tooltip_overlay: Option<usize>,
        cursor_transforms: [Option<Mat4>; 2],
        navigator: Option<(TextureId, [Mat4; 2])>,
    ) {
        let Some(menu) = scene.overlays.get(menu_range) else {
            return;
        };
        if menu.is_empty() && cursor_transforms.iter().all(Option::is_none) && navigator.is_none() {
            return;
        }
        let (w, h) = (desktop_size.0.max(1) as f32, desktop_size.1.max(1) as f32);
        let (eye_w, eye_h) = (eye_size.0.max(1) as f32, eye_size.1.max(1) as f32);
        let prepare = |id: &TextureId, quad: [Vec4; 4]| {
            let texture = scene.textures.get(*id)?;
            let mut uniform = [0.0f32; 20];
            for (index, corner) in quad.iter().enumerate() {
                uniform[index * 4..index * 4 + 4].copy_from_slice(&corner.to_array());
            }
            uniform[16] = scene.premultiplied.contains(id) as u8 as f32;
            let buffer = buffer_init(
                &self.device,
                &self.queue,
                Some("OpenXR menu rectangle"),
                bytemuck::cast_slice(&uniform),
                wgpu::BufferUsages::UNIFORM,
            );
            let group = self.device.create_bind_group(&wgpu::BindGroupDescriptor {
                label: Some("OpenXR menu rectangle"),
                layout: &self.overlay_layout,
                entries: &[
                    wgpu::BindGroupEntry {
                        binding: 0,
                        resource: buffer.as_entire_binding(),
                    },
                    wgpu::BindGroupEntry {
                        binding: 1,
                        resource: wgpu::BindingResource::TextureView(&texture.view),
                    },
                    wgpu::BindGroupEntry {
                        binding: 2,
                        resource: wgpu::BindingResource::Sampler(&self.sky_sampler),
                    },
                ],
            });
            Some((buffer, group))
        };
        let mut prepared = [Vec::new(), Vec::new()];
        for eye in 0..2 {
            if let Some((id, transforms)) = navigator.as_ref() {
                let quad = [
                    Vec4::new(-1.0, 1.0, 0.0, 1.0),
                    Vec4::new(1.0, 1.0, 0.0, 1.0),
                    Vec4::new(1.0, -1.0, 0.0, 1.0),
                    Vec4::new(-1.0, -1.0, 0.0, 1.0),
                ]
                    .map(|p| transforms[eye] * p);
                if let Some(item) = prepare(id, quad) {
                    prepared[eye].push(item);
                }
            }
            for (index, (id, rect)) in menu.iter().enumerate() {
                let transform = if index == 0
                    && rect[0] <= 0.0
                    && rect[1] <= 0.0
                    && rect[2] >= w
                    && rect[3] >= h
                {
                    Mat4::IDENTITY
                } else {
                    menu_transforms[eye]
                };
                let x0 = rect[0] / w * 2.0 - 1.0;
                let y0 = 1.0 - rect[1] / h * 2.0;
                let x1 = rect[2] / w * 2.0 - 1.0;
                let y1 = 1.0 - rect[3] / h * 2.0;
                let quad = [
                    transform * Vec4::new(x0, y0, 0.0, 1.0),
                    transform * Vec4::new(x1, y0, 0.0, 1.0),
                    transform * Vec4::new(x1, y1, 0.0, 1.0),
                    transform * Vec4::new(x0, y1, 0.0, 1.0),
                ];
                if let Some(item) = prepare(id, quad) {
                    prepared[eye].push(item);
                }
            }
            if let (Some(index), Some(transform)) = (cursor_overlay, cursor_transforms[eye]) {
                if let Some((id, rect)) = scene.overlays.get(index) {
                    let half_x = (rect[2] - rect[0]) / eye_w;
                    let half_y = (rect[3] - rect[1]) / eye_h;
                    let quad = [
                        transform * Vec4::new(-half_x, half_y, 0.0, 1.0),
                        transform * Vec4::new(half_x, half_y, 0.0, 1.0),
                        transform * Vec4::new(half_x, -half_y, 0.0, 1.0),
                        transform * Vec4::new(-half_x, -half_y, 0.0, 1.0),
                    ];
                    if let Some(item) = prepare(id, quad) {
                        prepared[eye].push(item);
                    }
                }
                if let Some((id, rect)) = tooltip_overlay.and_then(|i| scene.overlays.get(i)) {
                    let pointer_width = scene
                        .overlays
                        .get(index)
                        .map(|(_, r)| r[2] - r[0])
                        .unwrap_or(24.0);
                    let left = (pointer_width * 0.5 + 8.0) * 2.0 / eye_w;
                    let right = left + (rect[2] - rect[0]) * 2.0 / eye_w;
                    let bottom = -(rect[3] - rect[1]) * 2.0 / eye_h;
                    let quad = [
                        transform * Vec4::new(left, 0.0, 0.0, 1.0),
                        transform * Vec4::new(right, 0.0, 0.0, 1.0),
                        transform * Vec4::new(right, bottom, 0.0, 1.0),
                        transform * Vec4::new(left, bottom, 0.0, 1.0),
                    ];
                    if let Some(item) = prepare(id, quad) {
                        prepared[eye].push(item);
                    }
                }
            }
        }
        let mut encoder = self
            .device
            .create_command_encoder(&wgpu::CommandEncoderDescriptor {
                label: Some("OpenXR menu"),
            });
        for (eye, target) in eyes.iter().enumerate() {
            let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                label: Some("OpenXR menu"),
                color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                    view: target,
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
            pass.set_pipeline(&self.xr_ui_pipeline);
            for (_, group) in &prepared[eye] {
                pass.set_bind_group(0, group, &[]);
                pass.draw(0..6, 0..1);
            }
        }
        self.queue.submit(Some(encoder.finish()));
    }

    #[allow(clippy::too_many_arguments)]
    fn render_inner(
        &mut self,
        scene: &mut Scene,
        target: &wgpu::TextureView,
        width: u32,
        height: u32,
        camera: &Camera,
        lighting: &Lighting,
        with_overlays: bool,
        exclude_texture: Option<TextureId>,
        projection: Option<Mat4>,
        second_eye: bool,
    ) {
        if omsi_cfg::env::var("OMSI_FAKE_GPU_ERROR").as_deref() == Ok("lost")
            && with_overlays
            && self.started.elapsed().as_secs_f32() > 3.0
            && self.device_lost().is_none()
        {
            log::error!("the graphics device was lost (test): OMSI_FAKE_GPU_ERROR=lost");
            self.device.destroy();
            *self.device_lost.lock().unwrap_or_else(|e| e.into_inner()) = Some("test".into());
        }
        if self
            .device_lost
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .is_some()
        {
            return;
        }
        if omsi_cfg::env::var("OMSI_FAKE_GPU_ERROR").as_deref() == Ok("frame")
            && self.options.msaa > 1
            && self.started.elapsed().as_secs_f32() > 3.0
        {
            let _ = self.device.create_texture(&wgpu::TextureDescriptor {
                label: Some("invalid"),
                size: wgpu::Extent3d {
                    width: 4,
                    height: 4,
                    depth_or_array_layers: 1,
                },
                mip_level_count: 1,
                sample_count: 3,
                dimension: wgpu::TextureDimension::D2,
                format: self.format,
                usage: wgpu::TextureUsages::RENDER_ATTACHMENT,
                view_formats: &[],
            });
        }
        if self.gpu_error.load(std::sync::atomic::Ordering::Relaxed) {
            self.fall_back_to_single_sample(scene);
        }
        self.collect_gpu_timers();
        let tset: Option<wgpu::QuerySet> = self.gpu_timers[with_overlays as usize]
            .as_ref()
            .filter(|t| !t.waiting && !t.unresolved)
            .map(|t| t.set.clone());
        let mut timed: Vec<&'static str> = Vec::new();
        let mut stage_t = std::time::Instant::now();
        let mut stage = |r: &Renderer, window: &'static str, mirror: &'static str| {
            if r.profiling {
                let now = std::time::Instant::now();
                *r.stats
                    .borrow_mut()
                    .entry(if with_overlays { window } else { mirror })
                    .or_default() += (now - stage_t).as_secs_f64();
                stage_t = now;
            }
        };
        let ro = (camera.position / 100.0).floor() * 100.0;
        self.set_render_origin(scene, ro);
        let (full_w, full_h) = (width, height);
        let (width, height) = if with_overlays {
            self.scene_size(full_w, full_h)
        } else {
            (full_w, full_h)
        };
        let vanilla_fxaa = with_overlays
            && (width, height) == (full_w, full_h)
            && self.options.fxaa
            && self.options.msaa <= 1
            && !(lighting.enhanced
            && self.hdr_pass.is_some()
            && omsi_cfg::env::var_os("OMSI_NO_ENHANCED").is_none())
            && omsi_cfg::env::var_os("OMSI_NO_FXAA").is_none();
        let enhanced_view = lighting.enhanced
            && self.hdr_pass.is_some()
            && omsi_cfg::env::var_os("OMSI_NO_ENHANCED").is_none();
        let glass_on = with_overlays
            && scene.glass_slot.is_some()
            && (lighting.rain > 0.001 || lighting.wetness > 0.02)
            && omsi_cfg::env::var_os("OMSI_NO_GLASS_PICTURE").is_none();
        let glass_key: Option<GlassKey> = glass_on.then_some((enhanced_view, width, height));
        let glass_ok = with_overlays
            && self
            .glass_live
            .take()
            .is_some_and(|k| Some(k) == scene.glass_key && Some(k) == glass_key);
        let scaled =
            (width, height) != (full_w, full_h) || vanilla_fxaa || (glass_on && !enhanced_view);
        let scene_target: Option<(wgpu::TextureView, wgpu::BindGroup)> = if scaled {
            Some(self.scale_target(width, height))
        } else {
            None
        };
        let scene_view: &wgpu::TextureView = scene_target.as_ref().map(|t| &t.0).unwrap_or(target);
        let aspect = self
            .texture_aspect
            .unwrap_or(width as f32 / height.max(1) as f32);
        let cam_rel = (camera.position - ro).as_vec3();
        let xr_view = projection.is_some();
        let lead_view = with_overlays || (xr_view && !second_eye);
        let enhanced_frame = lighting.enhanced
            && self.hdr_pass.is_some()
            && omsi_cfg::env::var_os("OMSI_NO_ENHANCED").is_none()
            && (with_overlays
            || xr_view
            || omsi_cfg::env::var_os("OMSI_MIRROR_ENHANCED").is_some());
        let enhanced = enhanced_frame;
        let spot_plan = lighting.light_shadows
            && (with_overlays || (xr_view && !second_eye))
            && omsi_cfg::env::var_os("OMSI_NO_LIGHT_SHADOWS").is_none();
        if !lighting.light_shadows {
            *self.spot_state.borrow_mut() = Default::default();
        }
        let grid = self.prepare_lights(scene, cam_rel, enhanced_frame, spot_plan);
        self.prepare_coronas(scene, lighting.night);
        self.prepare_smoke(scene, camera.position);
        let ao_on = with_overlays
            && self.options.ssao
            && self.ssao_pipeline.is_some()
            && omsi_cfg::env::var_os("OMSI_NO_AO").is_none();
        let prepass_on = ao_on || (enhanced && (with_overlays || xr_view));
        if prepass_on && self.ensure_ao(width, height) {
            scene.dirty = true;
            scene.model_buf = None;
            self.hdr_targets.clear();
        }
        if enhanced {
            self.hdr_targets(width, height);
        }
        let dt = {
            let now = std::time::Instant::now();
            let dt = self
                .last_frame
                .map(|t| (now - t).as_secs_f32())
                .unwrap_or(0.0);
            if lead_view {
                self.last_frame = Some(now);
            }
            dt
        };
        stage(self, "setup", "mirror.setup");
        self.prepare(scene);
        stage(self, "prepare", "mirror.prepare");
        let overlays: Vec<(TextureId, [f32; 4])> = if with_overlays {
            scene.overlays.clone()
        } else {
            Vec::new()
        };
        if with_overlays {
            scene.overlay_res.truncate(overlays.len());
            for (k, (tex, r)) in overlays.iter().copied().enumerate() {
                let r = snap_rect(r);
                let ndc = [
                    r[0] / full_w as f32 * 2.0 - 1.0,
                    1.0 - r[1] / full_h as f32 * 2.0,
                    r[2] / full_w as f32 * 2.0 - 1.0,
                    1.0 - r[3] / full_h as f32 * 2.0,
                    scene.premultiplied.contains(&tex) as u8 as f32,
                    0.0,
                    0.0,
                    0.0,
                ];
                if let Some((_, buf, _, last)) = scene.overlay_res.get_mut(k).filter(|o| o.0 == tex)
                {
                    if *last != ndc {
                        self.queue.write_buffer(buf, 0, bytemuck::cast_slice(&ndc));
                        *last = ndc;
                    }
                    continue;
                }
                let buf = buffer_init(
                    &self.device,
                    &self.queue,
                    Some("overlay rect"),
                    bytemuck::cast_slice(&ndc),
                    wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
                );
                let bg = self.device.create_bind_group(&wgpu::BindGroupDescriptor {
                    label: Some("overlay"),
                    layout: &self.overlay_layout,
                    entries: &[
                        wgpu::BindGroupEntry {
                            binding: 0,
                            resource: buf.as_entire_binding(),
                        },
                        wgpu::BindGroupEntry {
                            binding: 1,
                            resource: wgpu::BindingResource::TextureView(&scene.textures[tex].view),
                        },
                        wgpu::BindGroupEntry {
                            binding: 2,
                            resource: wgpu::BindingResource::Sampler(&self.sky_sampler),
                        },
                    ],
                });
                if k < scene.overlay_res.len() {
                    scene.overlay_res[k] = (tex, buf, bg, ndc);
                } else {
                    scene.overlay_res.push((tex, buf, bg, ndc));
                }
            }
        }
        let sun = lighting.sun_dir.normalize_or_zero();
        let shadows = (with_overlays || projection.is_some()) && lighting.casts_sun_shadows();
        let shared_xr_shadows = if second_eye && shadows {
            self.xr_shadow_cache
                .get()
                .filter(|(origin, previous_sun, _, _, _)| {
                    *origin == scene.render_origin && *previous_sun == sun
                })
        } else {
            None
        };
        let draw_shadows = shadows && shared_xr_shadows.is_none();
        let light_matrix = |range: f32| {
            let texel = range * 2.0 / self.options.shadow_size as f32;
            let up = if sun.z.abs() > 0.95 { Vec3::Y } else { Vec3::Z };
            let raw = cam_rel;
            let view0 = glam::camera::rh::view::look_at_mat4(sun * 900.0, Vec3::ZERO, up);
            let ls = view0.transform_point3(raw);
            let snapped = Vec3::new(
                (ls.x / texel).round() * texel,
                (ls.y / texel).round() * texel,
                ls.z,
            );
            let center = view0.inverse().transform_point3(snapped);
            let view = glam::camera::rh::view::look_at_mat4(center + sun * 900.0, center, up);
            let proj = glam::camera::rh::proj::directx::orthographic(
                -range, range, -range, range, 1.0, 2200.0,
            );
            proj * view
        };
        let near_wanted = light_matrix(SHADOW_RANGE);
        let (near_m, near_age, near_origin, near_sun) = self.shadow_near_cache.get();
        let near_jumped =
            (near_m.project_point3(cam_rel) - near_wanted.project_point3(cam_rel)).length() > 0.03;
        let redraw_near = draw_shadows
            && (near_age >= 1
            || near_jumped
            || near_m == Mat4::IDENTITY
            || near_origin != scene.render_origin
            || near_sun.dot(sun) < 0.99999
            || omsi_cfg::env::var_os("OMSI_SHADOW_NEAR_EVERY_FRAME").is_some());
        let light_view_proj = if let Some((_, _, near, _, _)) = shared_xr_shadows {
            near
        } else if !shadows {
            near_wanted
        } else if redraw_near {
            self.shadow_near_cache
                .set((near_wanted, 0, scene.render_origin, sun));
            near_wanted
        } else {
            self.shadow_near_cache
                .set((near_m, near_age + 1, near_origin, near_sun));
            near_m
        };
        let light_view_proj_close = shared_xr_shadows
            .map(|(_, _, _, _, close)| close)
            .unwrap_or_else(|| light_matrix(SHADOW_RANGE_CLOSE));
        let far_wanted = light_matrix(SHADOW_RANGE_FAR);
        let (far_m, far_age, far_origin, far_sun) = self.shadow_far_cache.get();
        let far_moved =
            (far_m.project_point3(cam_rel) - far_wanted.project_point3(cam_rel)).length() > 0.12;
        let redraw_far = draw_shadows
            && (far_age >= 3
            || far_moved
            || far_m == Mat4::IDENTITY
            || far_origin != scene.render_origin
            || far_sun.dot(sun) < 0.99999
            || omsi_cfg::env::var_os("OMSI_SHADOW_FAR_EVERY_FRAME").is_some());
        if redraw_far && omsi_cfg::env::var_os("OMSI_DEBUG_SHADOW_FAR").is_some() {
            log::info!(
                "far shadow redrawn: age {far_age} moved {far_moved} origin {} sun {:.6}",
                far_origin != scene.render_origin,
                far_sun.dot(sun)
            );
        }
        let light_view_proj_far = if let Some((_, _, _, far, _)) = shared_xr_shadows {
            far
        } else if !shadows {
            far_wanted
        } else if redraw_far {
            self.shadow_far_cache
                .set((far_wanted, 0, scene.render_origin, sun));
            far_wanted
        } else {
            self.shadow_far_cache
                .set((far_m, far_age + 1, far_origin, far_sun));
            far_m
        };
        if projection.is_some() && !second_eye && shadows {
            self.xr_shadow_cache.set(Some((
                scene.render_origin,
                sun,
                light_view_proj,
                light_view_proj_far,
                light_view_proj_close,
            )));
        }
        let vp_mat = projection
            .map(|p| {
                p * glam::camera::rh::view::look_to_mat4(
                    (camera.position - ro).as_vec3(),
                    camera.forward(),
                    camera.up(),
                )
            })
            .unwrap_or_else(|| camera.view_proj(aspect, ro));
        let sun_clip = vp_mat * (cam_rel + sun * 5000.0).extend(1.0);
        let sun_ndc = if sun_clip.w > 0.0 {
            Vec3::new(sun_clip.x / sun_clip.w, sun_clip.y / sun_clip.w, 1.0)
        } else {
            Vec3::new(9.0, 9.0, 0.0)
        };
        {
            let (lx, ly, side) = self.lm_place.get();
            let v: [f32; 4] = [
                (lx - ro.x) as f32,
                (ly - ro.y) as f32,
                side as f32,
                if side > 0.0 { 1.0 } else { 0.0 },
            ];
            self.queue
                .write_buffer(&self.lm_uniform, 0, bytemuck::cast_slice(&v));
        }
        let (spot_vp, spot_info) = {
            let st = self.spot_state.borrow();
            let mut m = [[[0.0f32; 4]; 4]; SPOT_SLOTS];
            for (k, sl) in st.slots.iter().enumerate() {
                if let Some(p) = sl.drawn {
                    m[k] = spot_view_proj(
                        (p.pos - scene.render_origin).as_vec3(),
                        p.dir,
                        p.fov,
                        SPOT_NEAR,
                        p.far,
                    )
                        .to_cols_array_2d();
                }
            }
            let sz = self.options.shadow_size as f32;
            let tile = self.spot_tile as f32;
            let h = sz + 2.0 * tile;
            (m, [tile / sz, tile / h, sz / h, tile])
        };
        let cu = CameraUniform {
            post: [
                if enhanced { 1.0 } else { 0.0 },
                lighting
                    .animation_time
                    .unwrap_or_else(|| self.started.elapsed().as_secs_f32()),
                sun_ndc.x,
                self.options.shadow_size.min(SHADOW_CLOSE_MAX) as f32
                    / self.options.shadow_size.max(1) as f32,
            ],
            view_proj: vp_mat.to_cols_array_2d(),
            cam_pos: cam_rel.extend(1.0).to_array(),
            world_origin: [
                ro.x.rem_euclid(1000.0) as f32,
                ro.y.rem_euclid(1000.0) as f32,
                ro.x.rem_euclid(CLOUD_ORIGIN_PERIOD) as f32,
                ro.y.rem_euclid(CLOUD_ORIGIN_PERIOD) as f32,
            ],
            sun_dir: lighting
                .sun_dir
                .normalize()
                .extend(lighting.sun_intensity)
                .to_array(),
            ambient: (lighting.ambient
                * if enhanced {
                1.0
            } else {
                night_scale(lighting.night, lighting.atmosphere_brightness)
            })
                .extend(lighting.snow.clamp(0.0, 1.0))
                .to_array(),
            fog: lighting.fog_color.extend(lighting.fog_density).to_array(),
            sun_color: lighting
                .sun_color
                .extend(lighting.night_maps.unwrap_or(lighting.night))
                .to_array(),
            sky_color: (lighting.secondary
                * if enhanced {
                1.0
            } else {
                night_scale(lighting.night, lighting.atmosphere_brightness)
            })
                .extend(if lighting.classic && !enhanced {
                    1.0
                } else {
                    0.0
                })
                .to_array(),
            light_grid: grid,
            sky: [
                lighting.sun_azimuth,
                lighting.sky_weights[0],
                lighting.sky_weights[1],
                lighting.sky_weights[2],
            ],
            clouds: [
                lighting.cloud_density,
                lighting.cloud_offset[0],
                lighting.cloud_offset[1],
                if ao_on { 1.0 } else { 0.0 },
            ],
            cam_right: camera
                .right()
                .extend(
                    self.env_heading
                        .get()
                        .map(|h| h.to_radians())
                        .unwrap_or(0.0),
                )
                .to_array(),
            cam_up: camera
                .right()
                .cross(camera.forward())
                .normalize_or_zero()
                .extend(if self.env_heading.get().is_some() {
                    1.0
                } else {
                    0.0
                })
                .to_array(),
            light_view_proj: light_view_proj.to_cols_array_2d(),
            light_view_proj_far: light_view_proj_far.to_cols_array_2d(),
            shadow: [
                if shadows { 1.0 } else { 0.0 },
                1.0 / self.options.shadow_size as f32,
                SHADOW_RANGE,
                lighting.wetness.clamp(0.0, 1.0),
            ],
            inside_a: match lighting.inside {
                Some((o, h, _)) => {
                    let r = (o - ro).as_vec3();
                    [r.x, r.y, r.z, (h as f32).to_radians().sin()]
                }
                None => [0.0; 4],
            },
            inside_b: match lighting.inside {
                Some((_, h, bb)) => [
                    (h as f32).to_radians().cos(),
                    bb[0] * 0.5,
                    bb[1] * 0.5,
                    bb[2] * 0.5,
                ],
                None => [1.0, 0.0, 0.0, 0.0],
            },
            inside_c: match lighting.inside {
                Some((_, _, bb)) => [bb[3], bb[4], bb[5], 1.0],
                None => [0.0; 4],
            },
            flags: [
                if lighting.detail { 1.0 } else { 0.0 },
                if enhanced { 1.0 } else { 0.0 },
                if glass_ok { -1.0 } else { 0.0 },
                if shadows { SHADOW_RANGE_CLOSE } else { 0.0 },
            ],
            light_view_proj_close: light_view_proj_close.to_cols_array_2d(),
            wind: [
                lighting.glass_wind.x,
                lighting.glass_wind.y,
                lighting.glass_wind.z,
                1.0,
            ],
            spot_vp,
            spot_info,
        };
        self.queue
            .write_buffer(&self.camera_buf, 0, bytemuck::bytes_of(&cu));
        let probe_redraw = enhanced
            && (lead_view || self.sky_state.is_none())
            && self.prepare_enhanced(lighting, cam_rel, ro, dt);
        let puddles_wanted = enhanced
            && with_overlays
            && self.options.reflections
            && lighting.wetness * (1.0 - lighting.snow.clamp(0.0, 1.0)) > 0.05
            && debug_view() == 0.0
            && omsi_cfg::env::var_os("OMSI_NO_PUDDLE_REFLECTIONS").is_none();
        stage(self, "setup", "mirror.setup");
        let debug_draws = omsi_cfg::env::var_os("OMSI_DEBUG_DRAWS").is_some();
        let debug_cull = omsi_cfg::env::var_os("OMSI_DEBUG_CULL").is_some();
        let mut list: Vec<u32> = Vec::new();
        let mut items: Vec<DrawItem> = Vec::new();
        let mut shadow_batches: [Vec<Batch>; SHADOW_SETS] = std::array::from_fn(|_| Vec::new());
        let kind_of = |alpha: AlphaMode| -> u8 {
            match alpha {
                AlphaMode::Opaque => PIPE_OPAQUE,
                AlphaMode::Test => PIPE_ALPHA_TEST,
                AlphaMode::Blend => PIPE_BLEND,
            }
        };
        let lod_fov = camera.fov_deg.to_radians().max(1e-3);
        let lod_size = |inst: &Instance| -> f32 {
            let scale = Self::instance_scale(scene, inst);
            let radius = if inst.object_radius > 0.0 {
                inst.object_radius
            } else {
                scene.meshes[inst.mesh].bounds_radius
            } * scale;
            let d = ((inst.origin - scene.render_origin).as_vec3() - cam_rel).length();
            if d <= radius {
                f32::MAX
            } else {
                2.0 * radius / (d.max(0.01) * lod_fov)
            }
        };
        let spot_draws: Vec<(usize, SpotPose)> = if spot_plan {
            let st = self.spot_state.borrow();
            st.draws
                .iter()
                .filter_map(|&k| st.slots[k].drawn.map(|p| (k, p)))
                .collect()
        } else {
            Vec::new()
        };
        let spot_cull: Vec<(usize, Vec3, Vec3, f32, f32)> = spot_draws
            .iter()
            .map(|&(k, p)| {
                (
                    k,
                    (p.pos - scene.render_origin).as_vec3(),
                    p.dir,
                    p.fov * 0.5,
                    p.far,
                )
            })
            .collect();
        let mut active = [false; SHADOW_SETS];
        active[0] = draw_shadows && redraw_near;
        active[1] = draw_shadows && redraw_far;
        active[2] = draw_shadows;
        for &(k, _, _, _, _) in &spot_cull {
            active[3 + k] = true;
        }
        let boxes = [
            (SHADOW_RANGE, light_view_proj, 0.4f32),
            (SHADOW_RANGE_FAR, light_view_proj_far, 6.0),
            (SHADOW_RANGE_CLOSE, light_view_proj_close, 0.1),
        ];
        let dbg_shadow = omsi_cfg::env::var_os("OMSI_DEBUG_SHADOW").is_some();
        let dbg_r: f32 = omsi_cfg::env::var("OMSI_DEBUG_SHADOW")
            .ok()
            .and_then(|v| v.trim().parse().ok())
            .unwrap_or(3.0);
        let casters = |span: std::ops::Range<usize>| -> [Vec<DrawItem>; SHADOW_SETS] {
            let mut out: [Vec<DrawItem>; SHADOW_SETS] = std::array::from_fn(|_| Vec::new());
            let mut ranges: Vec<(u8, u32, u32, usize)> = Vec::new();
            for inst in &scene.instances[span] {
                if !inst.visible
                    || !inst.casts_shadow
                    || (self.options.omsi_shadow_casters && !inst.omsi_caster)
                {
                    continue;
                }
                let m = &scene.meshes[inst.mesh];
                if m.ranges.is_empty() {
                    continue;
                }
                let (c, r) = Self::bounding_sphere(scene, inst);
                if inst.lod.0 > 0.0 || inst.lod.1 < f32::MAX {
                    let size = lod_size(inst);
                    if size < inst.lod.0 || (inst.lod.1 < f32::MAX && size >= inst.lod.1) {
                        if dbg_shadow && r >= dbg_r {
                            log::info!(
                                "shadow: mesh r={r:.1} at {:?} is another LOD than the one shown",
                                inst.origin
                            );
                        }
                        continue;
                    }
                }
                ranges.clear();
                for (ri, (_, _, slot)) in m.ranges.iter().enumerate() {
                    let mat_id = inst.materials.get(*slot as usize).copied().unwrap_or(0);
                    let mat = &scene.materials[mat_id];
                    let mut kind = kind_of(mat.alpha);
                    let cut_body = kind == PIPE_BLEND && mat.transmap.is_some() && !mat.no_z_write;
                    if (kind == PIPE_BLEND && !cut_body) || mat.no_z_check {
                        if dbg_shadow && r >= dbg_r {
                            log::info!(
                                "shadow: mesh r={r:.1} at {:?} slot {slot} is blended, never a caster",
                                inst.origin
                            );
                        }
                        continue;
                    }
                    if cut_body {
                        kind = PIPE_ALPHA_TEST;
                    }
                    ranges.push((kind, ri as u32, *slot, mat_id));
                }
                for (cascade, &(range, lvp, min_radius)) in boxes.iter().enumerate() {
                    if !active[cascade] {
                        continue;
                    }
                    let dbg = dbg_shadow && cascade == 0 && r >= dbg_r;
                    if m.bounds_radius > 0.0 && m.bounds_radius < min_radius {
                        if dbg_shadow && cascade == 0 && m.bounds_radius >= dbg_r {
                            log::info!(
                                "shadow: mesh r={:.1} skipped (ranges {})",
                                m.bounds_radius,
                                m.ranges.len()
                            );
                        }
                        continue;
                    }
                    let lc = lvp.project_point3(c);
                    let rr = r / range;
                    if lc.x.abs() > 1.0 + rr || lc.y.abs() > 1.0 + rr {
                        if dbg {
                            log::info!(
                                "shadow: mesh r={r:.1} outside the light box at ({:.2}, {:.2})",
                                lc.x,
                                lc.y
                            );
                        }
                        continue;
                    }
                    if dbg {
                        log::info!(
                            "shadow: caster r={r:.1} at {:?} slots {:?}",
                            inst.origin,
                            ranges.iter().map(|x| x.0).collect::<Vec<_>>()
                        );
                    }
                    for &(kind, ri, slot, mat_id) in &ranges {
                        out[cascade].push(DrawItem {
                            pipe: kind,
                            mesh: inst.mesh as u32,
                            range: ri,
                            material: depth_only_material(kind, mat_id),
                            entry: inst.base + slot,
                        });
                    }
                }
                for &(k, lpos, ldir, half, far) in &spot_cull {
                    let v = c - lpos;
                    let d = v.length();
                    if r < 0.1 || d - r > far {
                        continue;
                    }
                    if d > r * 1.01 {
                        let ang = (v.dot(ldir) / d).clamp(-1.0, 1.0).acos();
                        if ang > half * 1.45 + (r / d).clamp(0.0, 1.0).asin() {
                            continue;
                        }
                    }
                    for &(kind, ri, slot, mat_id) in &ranges {
                        out[3 + k].push(DrawItem {
                            pipe: kind,
                            mesh: inst.mesh as u32,
                            range: ri,
                            material: depth_only_material(kind, mat_id),
                            entry: inst.base + slot,
                        });
                    }
                }
            }
            out
        };
        if active.iter().any(|a| *a) {
            let n = scene.instances.len();
            let parts = (n / 8192).clamp(
                1,
                self.encoding_pool
                    .as_ref()
                    .map_or(3, |p| p.current_num_threads())
                    + 1,
            );
            let chunk = n.div_ceil(parts);
            let mut found: [Vec<DrawItem>; SHADOW_SETS] = std::array::from_fn(|_| Vec::new());
            for part in run_parts(self.encoding_pool.as_ref(), parts, |p| {
                casters(p * chunk..((p + 1) * chunk).min(n))
            }) {
                for (a, b) in found.iter_mut().zip(part) {
                    a.extend(b);
                }
            }
            for cascade in 0..SHADOW_SETS {
                if !active[cascade] {
                    continue;
                }
                if debug_draws {
                    log::info!("shadow cascade {cascade}: {} draws", found[cascade].len());
                }
                batch_items(
                    scene,
                    &mut found[cascade],
                    true,
                    &mut list,
                    &mut shadow_batches[cascade],
                );
            }
        }
        if shadows && debug_draws {
            log::info!(
                "shadow passes: {} batches",
                shadow_batches.iter().map(|b| b.len()).sum::<usize>()
            );
        }
        stage(self, "shadow items", "mirror.shadow items");
        let view = glam::camera::rh::view::look_to_mat4(cam_rel, camera.forward(), camera.up());
        let (tan_x, tan_y) = if let Some(p) = projection {
            (
                ((p.z_axis.x - 1.0) / p.x_axis.x)
                    .abs()
                    .max(((p.z_axis.x + 1.0) / p.x_axis.x).abs()),
                ((p.z_axis.y - 1.0) / p.y_axis.y)
                    .abs()
                    .max(((p.z_axis.y + 1.0) / p.y_axis.y).abs()),
            )
        } else {
            let tan_y = (camera.fov_deg.to_radians() * 0.5).tan();
            (tan_y * aspect, tan_y)
        };
        let cos_y = 1.0 / (1.0 + tan_y * tan_y).sqrt();
        let cos_x = 1.0 / (1.0 + tan_x * tan_x).sqrt();
        let fog_far = if enhanced_frame {
            if lighting.fog_density > FOG_MIN_DENSITY {
                let base = lighting
                    .fog_base
                    .or(lighting.inside.map(|v| v.0.z))
                    .unwrap_or(camera.position.z - 2.0);
                let kh = ((camera.position.z - base).max(0.0) / 300.0) as f32;
                let thin = if kh < 1e-3 {
                    1.0
                } else {
                    (1.0 - (-kh).exp()) / kh
                };
                (4.6 / (lighting.fog_density * thin)).min(camera.far)
            } else {
                camera.far
            }
        } else if lighting.fog_density > FOG_MIN_DENSITY {
            (4.6 / lighting.fog_density).min(camera.far)
        } else {
            camera.far
        };
        if let Some(p) = omsi_cfg::env::var("OMSI_DEBUG_CULL").ok().and_then(|v| {
            let f: Vec<f64> = v.split(',').filter_map(|x| x.trim().parse().ok()).collect();
            (f.len() == 3).then(|| (DVec3::new(f[0], f[1], 0.0), f[2]))
        }) {
            for (i, inst) in scene.instances.iter().enumerate() {
                if (inst.origin.truncate() - p.0.truncate()).length() > p.1 || !with_overlays {
                    continue;
                }
                let m = &scene.meshes[inst.mesh];
                let (c, _) = Self::bounding_sphere(scene, inst);
                let v = view.transform_point3(c);
                log::info!(
                    "cull {i}: mesh {} r {:.2} centre {:?} origin {:?} view {:?} visible {} lod {:?} tan ({tan_x:.2}, {tan_y:.2}) fog {fog_far:.0}",
                    inst.mesh,
                    m.bounds_radius,
                    m.bounds_center,
                    inst.origin,
                    v,
                    inst.visible,
                    inst.lod
                );
            }
        }
        let fov_y = camera.fov_deg.to_radians().max(1e-3);
        let max_obj_dist = self.options.max_obj_dist;
        let min_obj_size = lighting.min_obj_size.max(self.options.min_obj_size);
        let main_view = with_overlays || xr_view;
        let mut drawn_before = if main_view {
            std::mem::take(&mut *self.cull_drawn.borrow_mut())
        } else {
            Vec::new()
        };
        let was_drawn = |i: usize| {
            drawn_before
                .get(i / 64)
                .is_some_and(|w| w & (1u64 << (i % 64)) != 0)
        };
        let (mut sizes_before, mut sizes_now) = if main_view {
            let sizes_before = std::mem::take(&mut *self.object_sizes.borrow_mut());
            let mut sizes_now = std::mem::take(&mut *self.object_sizes_scratch.borrow_mut());
            sizes_now.clear();
            (sizes_before, sizes_now)
        } else {
            Default::default()
        };
        let cull_one = |i: usize, sizes: &mut Vec<([u64; 4], f32)>| -> Option<(usize, f32, bool)> {
            let inst = &scene.instances[i];
            let m = &scene.meshes[inst.mesh];
            if m.ranges.is_empty() || !inst.visible || (inst.mirror_only && main_view) {
                return None;
            }
            if let Some([x0, y0, x1, y1]) = inst.near_only {
                let c = camera.position;
                if c.x < x0 || c.x > x1 || c.y < y0 || c.y > y1 {
                    return None;
                }
            }
            if inst.blob && !self.shadow_blobs {
                return None;
            }
            let (c, r) = Self::bounding_sphere(scene, inst);
            let v = view.transform_point3(c);
            let z = -v.z;
            let inside = v.length() <= r;
            if !inside && z + r < camera.near {
                return None;
            }
            if !inside && z - r > fog_far && !(enhanced_frame && (inst.surface || r > 100.0)) {
                return None;
            }
            if !inside && (v.x.abs() > z * tan_x + r / cos_x || v.y.abs() > z * tan_y + r / cos_y) {
                return None;
            }
            let size = if inst.object_radius > 0.0 {
                let scale = Self::instance_scale(scene, inst);
                let radius = inst.object_radius * scale;
                let ov = view.transform_point3((inst.origin - scene.render_origin).as_vec3());
                let (od, oz) = (ov.length(), -ov.z);
                if od <= radius {
                    f32::MAX
                } else {
                    let reach = if was_drawn(i) {
                        max_obj_dist * 1.05
                    } else {
                        max_obj_dist
                    };
                    if !inst.any_distance
                        && max_obj_dist > 0.0
                        && (od > radius + reach || oz - radius > reach)
                    {
                        return None;
                    }
                    let key = [
                        inst.origin.x.to_bits(),
                        inst.origin.y.to_bits(),
                        inst.origin.z.to_bits(),
                        radius.to_bits() as u64,
                    ];
                    let fresh = 2.0 * radius / (od.max(0.01) * fov_y);
                    let size = match sizes_before.get(&key) {
                        Some(&last) if fresh > last * 0.94 && fresh < last * 1.06 => last,
                        _ => fresh,
                    };
                    if main_view {
                        sizes.push((key, size));
                    }
                    size
                }
            } else if m.bounds_radius > 0.0 {
                2.0 * r / (v.length().max(0.01) * fov_y)
            } else {
                f32::MAX
            };
            let near_dbg =
                debug_cull && with_overlays && (inst.origin - camera.position).length() < 150.0;
            let keep = if was_drawn(i) && inst.object_radius <= 0.0 {
                0.85
            } else {
                1.0
            };
            if !inst.surface && size < min_obj_size * inst.detail * keep {
                if near_dbg {
                    log::info!(
                        "cull: instance {i} mesh {} at {:.0} m: size {size:.4} < {:.4} (radius {:.1}, detail {})",
                        inst.mesh,
                        (inst.origin - camera.position).length(),
                        min_obj_size * inst.detail,
                        inst.object_radius,
                        inst.detail
                    );
                }
                return None;
            }
            if (inst.lod.0 > 0.0 || inst.lod.1 < f32::MAX)
                && (size < inst.lod.0 || (inst.lod.1 < f32::MAX && size >= inst.lod.1))
            {
                if near_dbg && size < inst.lod.0 {
                    log::info!(
                        "cull: instance {i} mesh {} at {:.0} m: lod {:.4}..{:.4}, size {size:.4}",
                        inst.mesh,
                        (inst.origin - camera.position).length(),
                        inst.lod.0,
                        inst.lod.1
                    );
                }
                return None;
            }
            Some((i, z, inside))
        };
        let n = scene.instances.len();
        let parts = (n / 8192).clamp(
            1,
            self.encoding_pool
                .as_ref()
                .map_or(3, |p| p.current_num_threads())
                + 1,
        );
        let chunk = n.div_ceil(parts);
        let (mut visible, mut found): (Vec<(usize, f32, bool)>, Vec<([u64; 4], f32)>) =
            (Vec::new(), Vec::new());
        for (v, sizes) in run_parts(self.encoding_pool.as_ref(), parts, |p| {
            let mut sizes = Vec::new();
            let v: Vec<_> = (p * chunk..((p + 1) * chunk).min(n))
                .filter_map(|i| cull_one(i, &mut sizes))
                .collect();
            (v, sizes)
        }) {
            visible.extend(v);
            found.extend(sizes);
        }
        if main_view {
            sizes_now.extend(found);
            *self.object_sizes.borrow_mut() = sizes_now;
            sizes_before.clear();
            *self.object_sizes_scratch.borrow_mut() = sizes_before;
        }
        if main_view {
            drawn_before.resize(scene.instances.len().div_ceil(64), 0);
            drawn_before.fill(0);
            for &(i, _, _) in &visible {
                drawn_before[i / 64] |= 1u64 << (i % 64);
            }
            *self.cull_drawn.borrow_mut() = drawn_before;
        }
        if with_overlays && omsi_cfg::env::var_os("OMSI_DEBUG_FLICKER").is_some() {
            let drawn: std::collections::HashSet<usize> = visible.iter().map(|v| v.0).collect();
            let mut prev = self.flicker.borrow_mut();
            let mut now: HashMap<usize, bool> = HashMap::new();
            for (i, inst) in scene.instances.iter().enumerate() {
                let m = &scene.meshes[inst.mesh];
                if m.ranges.is_empty() || !inst.visible || inst.surface {
                    continue;
                }
                let (c, r) = Self::bounding_sphere(scene, inst);
                let v = view.transform_point3(c);
                let z = -v.z;
                if v.length() > 150.0
                    || z + r < camera.near
                    || v.x.abs() > z * tan_x + r / cos_x
                    || v.y.abs() > z * tan_y + r / cos_y
                {
                    continue;
                }
                let d = drawn.contains(&i);
                if let Some(&was) = prev.get(&i) {
                    if was != d {
                        let od = (inst.origin - camera.position).length();
                        log::info!(
                            "flicker: instance {i} mesh {} {} at {od:.1} m (view z {z:.1}, r {r:.2}, object r {:.2}, detail {}, lod {:.3}..{:.3})",
                            inst.mesh,
                            if d { "appears" } else { "vanishes" },
                            inst.object_radius,
                            inst.detail,
                            inst.lod.0,
                            inst.lod.1
                        );
                    }
                }
                now.insert(i, d);
            }
            *prev = now;
        }
        stage(self, "cull", "mirror.cull");
        let only_surfaces = omsi_cfg::env::var_os("OMSI_ONLY_SURFACES").is_some();
        let visible: Vec<(usize, f32, bool)> = if only_surfaces {
            visible
                .into_iter()
                .filter(|(i, _, _)| scene.instances[*i].surface)
                .collect()
        } else {
            visible
        };
        if debug_draws {
            let surf = scene.instances.iter().filter(|i| i.surface).count();
            let vis_surf = visible
                .iter()
                .filter(|(i, _, _)| scene.instances[*i].surface)
                .count();
            log::info!(
                "draw: {} instances ({} surface), {} visible ({} surface)",
                scene.instances.len(),
                surf,
                visible.len(),
                vis_surf
            );
        }
        let mut prepass_batches: Vec<Batch> = Vec::new();
        let prepass_job = || -> (Vec<u32>, Vec<Batch>) {
            let mut items: Vec<DrawItem> = Vec::new();
            let mut list: Vec<u32> = Vec::new();
            let mut batches: Vec<Batch> = Vec::new();
            for &(i, _, _) in &visible {
                let inst = &scene.instances[i];
                let cull = culls_back_faces(scene, inst);
                for (ri, (_, _, slot)) in scene.meshes[inst.mesh].ranges.iter().enumerate() {
                    let mat_id = inst.materials.get(*slot as usize).copied().unwrap_or(0);
                    let mat = &scene.materials[mat_id];
                    let kind = kind_of(mat.alpha);
                    if kind == PIPE_BLEND && world_surface_phase(effective_render_phase(inst)) {
                        continue;
                    }
                    if let Some(pre_kind) = depth_prepass_kind(kind, mat, inst.presurface) {
                        items.push(DrawItem {
                            pipe: pre_kind * 2 + cull as u8,
                            mesh: inst.mesh as u32,
                            range: ri as u32,
                            material: depth_only_material(pre_kind, mat_id),
                            entry: inst.base + *slot,
                        });
                    }
                }
            }
            batch_items(scene, &mut items, true, &mut list, &mut batches);
            (list, batches)
        };
        let mut main_batches: Vec<Batch> = Vec::new();
        let mut main_draws = [0usize; 2];
        let has_presurface = visible
            .iter()
            .any(|&(i, _, _)| scene.instances[i].presurface);
        let mut prepass_found: Option<(Vec<u32>, Vec<Batch>)> = None;
        let pool = self.encoding_pool.as_ref();
        in_scope(pool, |scope| {
            if prepass_on {
                let job = &prepass_job;
                let slot = &mut prepass_found;
                scope.spawn(move |_| *slot = Some(job()));
            }
            let mut by_phase: [Vec<(usize, f32, bool)>; RenderPhase::COUNT] =
                std::array::from_fn(|_| Vec::new());
            for &entry in &visible {
                let phase = effective_render_phase(&scene.instances[entry.0]);
                by_phase[phase as usize].push(entry);
            }
            for phase in RenderPhase::DRAW_ORDER {
                if phase == RenderPhase::BeforeNormal {
                    items.clear();
                    for &(i, _, _) in by_phase[..RenderPhase::BeforeNormal as usize]
                        .iter()
                        .flatten()
                    {
                        let inst = &scene.instances[i];
                        for (ri, (_, _, slot)) in scene.meshes[inst.mesh].ranges.iter().enumerate()
                        {
                            let mat_id = inst.materials.get(*slot as usize).copied().unwrap_or(0);
                            let mat = &scene.materials[mat_id];
                            if !surface_depth_coverage(
                                effective_render_phase(inst),
                                mat.alpha,
                                mat.transmap.is_some(),
                                mat.no_z_check,
                            ) || exclude_texture.is_some_and(|t| mat.uses_texture(t))
                            {
                                continue;
                            }
                            items.push(DrawItem {
                                pipe: pipe_code(
                                    PIPE_SURFACE_DEPTH,
                                    culls_back_faces(scene, inst),
                                    instance_depth_bias(inst, mat),
                                ),
                                mesh: inst.mesh as u32,
                                range: ri as u32,
                                material: mat_id as u32,
                                entry: inst.base + *slot,
                            });
                        }
                    }
                    batch_items(scene, &mut items, true, &mut list, &mut main_batches);
                }
                let visible = &by_phase[phase as usize];
                items.clear();
                let mut blended: Vec<usize> = Vec::new();
                for &(i, _, _) in visible {
                    let inst = &scene.instances[i];
                    if inst.ordered {
                        blended.push(i);
                        continue;
                    }
                    let mut has_blend = false;
                    let cull = culls_back_faces(scene, inst);
                    for (ri, (_, _, slot)) in scene.meshes[inst.mesh].ranges.iter().enumerate() {
                        let mat_id = inst.materials.get(*slot as usize).copied().unwrap_or(0);
                        let mat = &scene.materials[mat_id];
                        let kind = kind_of(mat.alpha);
                        if kind == PIPE_BLEND || mat.no_z_check {
                            has_blend = true;
                            continue;
                        }
                        if exclude_texture.is_some_and(|t| mat.uses_texture(t)) {
                            continue;
                        }
                        items.push(DrawItem {
                            pipe: pipe_code(kind, cull, instance_depth_bias(inst, mat)),
                            mesh: inst.mesh as u32,
                            range: ri as u32,
                            material: mat_id as u32,
                            entry: inst.base + *slot,
                        });
                    }
                    if has_blend {
                        blended.push(i);
                    }
                }
                main_draws[0] += items.len();
                batch_items(scene, &mut items, true, &mut list, &mut main_batches);
                let mut holders: Vec<DVec3> = Vec::new();
                for &(i, _, inside) in visible {
                    let inst = &scene.instances[i];
                    if inside && !inst.surface && !holders.contains(&inst.origin) {
                        holders.push(inst.origin);
                    }
                }
                let player = lighting
                    .inside
                    .filter(|v| point_in_vehicle_box(camera.position, v))
                    .map(|v| v.0);
                let near_by_origin = if self.blend_by_origin {
                    HashMap::new()
                } else {
                    nearest_by_origin(blended.iter().filter_map(|&i| {
                        let inst = &scene.instances[i];
                        if inst.surface {
                            return None;
                        }
                        let (c, r) = Self::bounding_sphere(scene, inst);
                        Some((inst.origin, (c - cam_rel).length() - r))
                    }))
                };
                let mut keyed: Vec<(u8, f32, usize)> = blended
                    .iter()
                    .map(|&i| {
                        let inst = &scene.instances[i];
                        let rank = if self.blend_by_origin || inst.surface {
                            0
                        } else if player == Some(inst.origin) {
                            2
                        } else if holders.contains(&inst.origin) {
                            1
                        } else {
                            0
                        };
                        let dist = if let Some(sort_origin) = inst.blend_sort_origin {
                            horizontal_sort_distance(sort_origin, ro, cam_rel)
                        } else if self.blend_by_origin || inst.surface {
                            ((inst.origin - ro).as_vec3() - cam_rel).length()
                        } else {
                            near_by_origin
                                .get(&origin_key(inst.origin))
                                .copied()
                                .unwrap_or(0.0)
                        };
                        (rank, dist, i)
                    })
                    .collect();
                keyed.sort_unstable_by(|a, b| {
                    a.0.cmp(&b.0).then(b.1.total_cmp(&a.1)).then(a.2.cmp(&b.2))
                });
                items.clear();
                for (_, _, i) in keyed {
                    let inst = &scene.instances[i];
                    let cull = culls_back_faces(scene, inst);
                    for (ri, (_, _, slot)) in scene.meshes[inst.mesh].ranges.iter().enumerate() {
                        let mat_id = inst.materials.get(*slot as usize).copied().unwrap_or(0);
                        let mat = &scene.materials[mat_id];
                        if (mat.alpha != AlphaMode::Blend && !mat.no_z_check && !inst.ordered)
                            || exclude_texture.is_some_and(|t| mat.uses_texture(t))
                        {
                            continue;
                        }
                        if mat.alpha == AlphaMode::Blend
                            && inst
                            .slot_alpha
                            .get(*slot as usize)
                            .is_some_and(|a| *a < 1.0 / 512.0)
                        {
                            continue;
                        }
                        let kind = if mat.alpha != AlphaMode::Blend && !mat.no_z_check {
                            kind_of(mat.alpha)
                        } else if mat.no_z_write
                            || mat.no_z_check
                            || (world_surface_phase(inst.render_phase) && !inst.presurface)
                        {
                            PIPE_BLEND_NO_WRITE
                        } else {
                            PIPE_BLEND
                        };
                        items.push(DrawItem {
                            pipe: pipe_code(kind, cull, instance_depth_bias(inst, mat)),
                            mesh: inst.mesh as u32,
                            range: ri as u32,
                            material: mat_id as u32,
                            entry: inst.base + *slot,
                        });
                    }
                }
                main_draws[1] += items.len();
                batch_items(scene, &mut items, false, &mut list, &mut main_batches);
            }
        });
        if let Some((pre_list, mut pre_batches)) = prepass_found {
            let offset = list.len() as u32;
            for b in &mut pre_batches {
                b.instances = b.instances.start + offset..b.instances.end + offset;
            }
            list.extend(pre_list);
            prepass_batches = pre_batches;
        }
        if let Ok(skip) = omsi_cfg::env::var("OMSI_SKIP_PIPE") {
            let skip: Vec<u8> = skip
                .split(',')
                .filter_map(|x| x.trim().parse().ok())
                .collect();
            main_batches.retain(|b| !skip.contains(&(b.pipe / 4)));
        }
        if debug_draws {
            log::info!(
                "  main pass: {} opaque/alpha-tested and {} blended draws in {} batches; prepass {} batches; draw list {} entries",
                main_draws[0],
                main_draws[1],
                main_batches.len(),
                prepass_batches.len(),
                list.len()
            );
        }
        if self.profiling && !with_overlays {
            let mut c = self.counts.borrow_mut();
            *c.entry("mirror pictures").or_default() += 1.0;
            *c.entry("mirror visible instances").or_default() += visible.len() as f64;
        }
        if self.profiling && with_overlays {
            let mut c = self.counts.borrow_mut();
            *c.entry("scene instances").or_default() += scene.instances.len() as f64;
            *c.entry("visible instances").or_default() += visible.len() as f64;
            *c.entry("main draws").or_default() += (main_draws[0] + main_draws[1]) as f64;
            *c.entry("opaque draws").or_default() += main_draws[0] as f64;
            *c.entry("blended draws").or_default() += main_draws[1] as f64;
            *c.entry("main batches").or_default() += main_batches.len() as f64;
            *c.entry("prepass batches").or_default() += prepass_batches.len() as f64;
            *c.entry("shadow batches").or_default() +=
                (shadow_batches[0].len() + shadow_batches[1].len()) as f64;
            let tris = |bs: &[Batch]| {
                bs.iter()
                    .map(|b| b.count as f64 / 3.0 * b.instances.len() as f64)
                    .sum::<f64>()
                    / 1000.0
            };
            *c.entry("ktris main").or_default() += tris(&main_batches);
            *c.entry("ktris prepass").or_default() += tris(&prepass_batches);
            *c.entry("ktris shadow near").or_default() += tris(&shadow_batches[0]);
            *c.entry("ktris shadow far").or_default() += tris(&shadow_batches[1]);
            *c.entry("ktris shadow close").or_default() += tris(&shadow_batches[2]);
        }
        if self.profiling && with_overlays && self.draw_audit_at.elapsed().as_secs() >= 10 {
            self.draw_audit_at = std::time::Instant::now();
            let mut assets: HashMap<&str, (usize, usize, u64)> = HashMap::new();
            for b in &main_batches {
                let source = scene.meshes[b.mesh as usize]
                    .source
                    .as_deref()
                    .unwrap_or("procedural / vehicle");
                let cost = assets.entry(source).or_default();
                cost.0 += 1;
                cost.1 += b.instances.len();
                cost.2 += b.count as u64 / 3 * b.instances.len() as u64;
            }
            let mut assets: Vec<_> = assets.into_iter().collect();
            assets.sort_unstable_by(|a, b| b.1.0.cmp(&a.1.0).then(a.0.cmp(b.0)));
            for (source, (batches, draws, tris)) in assets.iter().take(12) {
                log::info!(
                    "draw audit: {batches} batches, {draws} draws, {tris} triangles: {source}"
                );
            }
            assets.sort_unstable_by(|a, b| b.1.2.cmp(&a.1.2).then(a.0.cmp(b.0)));
            for (source, (batches, draws, tris)) in assets.iter().take(12) {
                log::info!(
                    "triangle audit: {tris} triangles in {draws} draws ({batches} batches): {source}"
                );
            }
        }
        stage(self, "items", "mirror.items");
        self.upload_draw_list(scene, &list);
        stage(self, "upload", "mirror.upload");
        let main_bundles = if omsi_cfg::env::var_os("OMSI_NO_BUNDLES").is_none() {
            let (pp, format) = if enhanced {
                (
                    self.hdr_pass.as_ref().expect("enhanced pipelines"),
                    wgpu::TextureFormat::Rgba16Float,
                )
            } else {
                (&self.pass, self.format)
            };
            record_bundles(
                &self.device,
                self.encoding_pool.as_ref(),
                scene,
                &main_batches,
                pp,
                scene.camera_bind_group.as_ref().expect("camera bind group"),
                format,
                self.options.msaa,
            )
        } else {
            Vec::new()
        };
        stage(self, "bundles", "mirror.bundles");
        let mut shadow_encoder =
            self.device
                .create_command_encoder(&wgpu::CommandEncoderDescriptor {
                    label: Some("shadow maps"),
                });
        self.flush_pending_meshes(scene, &mut shadow_encoder);
        let mut prepass_encoder =
            self.device
                .create_command_encoder(&wgpu::CommandEncoderDescriptor {
                    label: Some("depth prepass"),
                });
        let mut encoder = self
            .device
            .create_command_encoder(&wgpu::CommandEncoderDescriptor {
                label: Some("picture"),
            });
        for cascade in [0usize, 1] {
            if !draw_shadows || (cascade == 1 && !redraw_far) {
                continue;
            }
            let view = if cascade == 0 {
                &self.shadow_view
            } else {
                &self.shadow_view_far
            };
            let keep_near = cascade == 0 && !redraw_near;
            let mut pass = shadow_encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                label: Some("shadow"),
                color_attachments: &[],
                depth_stencil_attachment: Some(wgpu::RenderPassDepthStencilAttachment {
                    view,
                    depth_ops: Some(wgpu::Operations {
                        load: if keep_near || cascade == 1 {
                            wgpu::LoadOp::Load
                        } else {
                            wgpu::LoadOp::Clear(1.0)
                        },
                        store: wgpu::StoreOp::Store,
                    }),
                    stencil_ops: None,
                }),
                timestamp_writes: pass_timer(
                    tset.as_ref(),
                    &mut timed,
                    if cascade == 0 {
                        "shadow near"
                    } else {
                        "shadow far"
                    },
                ),
                occlusion_query_set: None,
                multiview_mask: None,
            });
            pass.set_bind_group(0, scene.shadow_bind_group.as_ref().unwrap(), &[]);
            if cascade == 0 {
                let sz = self.options.shadow_size as f32;
                pass.set_viewport(0.0, 0.0, sz, sz, 0.0, 1.0);
                encode_batches(&mut pass, scene, &shadow_batches[0], |pipe| {
                    &self.shadow_pipelines[pipe as usize]
                });
                let csz = self.options.shadow_size.min(SHADOW_CLOSE_MAX) as f32;
                pass.set_viewport(sz, 0.0, csz, csz, 0.0, 1.0);
                if keep_near {
                    pass.set_pipeline(&self.shadow_clear_pipeline);
                    pass.draw(0..3, 0..1);
                    pass.set_bind_group(0, scene.shadow_bind_group.as_ref().unwrap(), &[]);
                }
                encode_batches(&mut pass, scene, &shadow_batches[2], |pipe| {
                    &self.shadow_pipelines[4 + pipe as usize]
                });
            } else {
                let sz = self.options.shadow_size;
                pass.set_viewport(0.0, 0.0, sz as f32, sz as f32, 0.0, 1.0);
                pass.set_scissor_rect(0, 0, sz, sz);
                pass.set_pipeline(&self.shadow_clear_pipeline);
                pass.draw(0..3, 0..1);
                pass.set_bind_group(0, scene.shadow_bind_group.as_ref().unwrap(), &[]);
                encode_batches(&mut pass, scene, &shadow_batches[cascade], |pipe| {
                    &self.shadow_pipelines[cascade * 2 + pipe as usize]
                });
            }
        }
        for &(k, pose) in &spot_draws {
            let Some(bg) = scene.spot_bind_groups.get(k) else {
                continue;
            };
            let mut cu_spot: CameraUniform = bytemuck::Zeroable::zeroed();
            cu_spot.light_view_proj = spot_view_proj(
                (pose.pos - scene.render_origin).as_vec3(),
                pose.dir,
                pose.fov,
                SPOT_NEAR,
                pose.far,
            )
                .to_cols_array_2d();
            self.queue
                .write_buffer(&self.spot_cam_bufs[k], 0, bytemuck::bytes_of(&cu_spot));
            let tile = self.spot_tile;
            let x = (k as u32 % 4) * tile;
            let y = self.options.shadow_size + (k as u32 / 4) * tile;
            let mut pass = shadow_encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                label: Some("spot shadow"),
                color_attachments: &[],
                depth_stencil_attachment: Some(wgpu::RenderPassDepthStencilAttachment {
                    view: &self.shadow_view_far,
                    depth_ops: Some(wgpu::Operations {
                        load: wgpu::LoadOp::Load,
                        store: wgpu::StoreOp::Store,
                    }),
                    stencil_ops: None,
                }),
                timestamp_writes: None,
                occlusion_query_set: None,
                multiview_mask: None,
            });
            pass.set_viewport(x as f32, y as f32, tile as f32, tile as f32, 0.0, 1.0);
            pass.set_scissor_rect(x, y, tile, tile);
            pass.set_pipeline(&self.shadow_clear_pipeline);
            pass.draw(0..3, 0..1);
            pass.set_bind_group(0, bg, &[]);
            encode_batches(&mut pass, scene, &shadow_batches[3 + k], |pipe| {
                &self.shadow_pipelines[pipe as usize]
            });
        }
        if prepass_on {
            let proj = glam::camera::rh::proj::directx::perspective(
                camera.fov_deg.to_radians(),
                aspect,
                camera.far,
                camera.near,
            );
            let u = SsaoUniform {
                inv_proj: proj.inverse().to_cols_array_2d(),
                params: [
                    1.0,
                    1.4,
                    width.div_ceil(2) as f32,
                    height.div_ceil(2) as f32,
                ],
            };
            self.queue
                .write_buffer(&self.ao_buf, 0, bytemuck::bytes_of(&u));
            let ao = self.ao.as_ref().unwrap();
            {
                let mut pass = prepass_encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                    label: Some("depth prepass"),
                    color_attachments: &[],
                    depth_stencil_attachment: Some(wgpu::RenderPassDepthStencilAttachment {
                        view: &ao.depth_view,
                        depth_ops: Some(wgpu::Operations {
                            load: wgpu::LoadOp::Clear(0.0),
                            store: wgpu::StoreOp::Store,
                        }),
                        stencil_ops: None,
                    }),
                    timestamp_writes: pass_timer(tset.as_ref(), &mut timed, "depth prepass"),
                    occlusion_query_set: None,
                    multiview_mask: None,
                });
                pass.set_bind_group(0, scene.camera_bind_group.as_ref().unwrap(), &[]);
                encode_batches(&mut pass, scene, &prepass_batches, |pipe| {
                    &self.prepass_pipelines[pipe as usize]
                });
            }
            for (pipe, bg, target, pass_label) in [
                (&self.ssao_pipeline, &ao.ssao_bg, &ao.ao_view, "ssao"),
                (&self.blur_pipeline, &ao.blur_bg, &ao.blur_view, "ssao blur"),
            ] {
                let Some(pipe) = pipe.as_ref().filter(|_| ao_on) else {
                    break;
                };
                let mut pass = prepass_encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                    label: Some("ssao"),
                    color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                        view: target,
                        depth_slice: None,
                        resolve_target: None,
                        ops: wgpu::Operations {
                            load: wgpu::LoadOp::Clear(wgpu::Color::WHITE),
                            store: wgpu::StoreOp::Store,
                        },
                    })],
                    depth_stencil_attachment: None,
                    timestamp_writes: pass_timer(tset.as_ref(), &mut timed, pass_label),
                    occlusion_query_set: None,
                    multiview_mask: None,
                });
                pass.set_pipeline(pipe);
                pass.set_bind_group(0, bg, &[]);
                pass.draw(0..3, 0..1);
            }
        }
        if enhanced && (lead_view || probe_redraw) {
            if let (Some(probe), Some(sky_bg)) =
                (self.probe.as_mut(), scene.sky_bind_group.as_ref())
            {
                let full = !probe.cube_filled || self.instant_exposure;
                probe.cube_wait += 1;
                let recapture = std::mem::take(&mut probe.cube_recapture);
                let draws: Vec<(u32, u32, f64)> = if full {
                    (0..6)
                        .flat_map(|f| {
                            (0..SKY_CUBE_ROUNDS).map(move |r| (f, r, r as f64 / (r as f64 + 1.0)))
                        })
                        .collect()
                } else if recapture {
                    let round = (probe.cube_round / 6) % SKY_CUBE_ROUNDS;
                    (0..6).map(|f| (f, round, 0.0)).collect()
                } else if (probe.cube_wait >= SKY_CUBE_EVERY && !redraw_near)
                    || probe.cube_wait >= SKY_CUBE_EVERY * 2
                    || !lead_view
                {
                    vec![(
                        probe.cube_next,
                        (probe.cube_round / 6) % SKY_CUBE_ROUNDS,
                        SKY_CUBE_HISTORY,
                    )]
                } else {
                    Vec::new()
                };
                let single = draws.len() == 1;
                if !draws.is_empty() {
                    probe.cube_wait = 0;
                    probe.cube_next = (probe.cube_next + 1) % 6;
                    probe.cube_round = probe.cube_round.wrapping_add(1);
                }
                probe.cube_filled = true;
                for (f, round, history) in draws {
                    let mut pass = prepass_encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                        label: Some("sky cube"),
                        color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                            view: &probe.cube_faces[f as usize],
                            depth_slice: None,
                            resolve_target: None,
                            ops: wgpu::Operations {
                                load: if history > 0.0 {
                                    wgpu::LoadOp::Load
                                } else {
                                    wgpu::LoadOp::Clear(wgpu::Color::BLACK)
                                },
                                store: wgpu::StoreOp::Store,
                            },
                        })],
                        depth_stencil_attachment: None,
                        timestamp_writes: if single {
                            pass_timer(tset.as_ref(), &mut timed, "sky cube")
                        } else {
                            None
                        },
                        occlusion_query_set: None,
                        multiview_mask: None,
                    });
                    pass.set_pipeline(&probe.cube_pipeline);
                    pass.set_blend_constant(wgpu::Color {
                        r: history,
                        g: history,
                        b: history,
                        a: history,
                    });
                    pass.set_bind_group(
                        0,
                        &probe.cube_bind_groups[(f * SKY_CUBE_ROUNDS + round) as usize],
                        &[],
                    );
                    pass.set_bind_group(1, sky_bg, &[]);
                    pass.draw(0..3, 0..1);
                }
            }
        }
        if probe_redraw {
            if let (Some(probe), Some(sky_bg)) =
                (self.probe.as_ref(), scene.sky_bind_group.as_ref())
            {
                for (m, faces) in probe.faces.iter().enumerate() {
                    for (half, bg) in probe.bind_groups[m].iter().enumerate() {
                        let attachments: Vec<Option<wgpu::RenderPassColorAttachment>> = faces
                            [half * 3..half * 3 + 3]
                            .iter()
                            .map(|v| {
                                Some(wgpu::RenderPassColorAttachment {
                                    view: v,
                                    depth_slice: None,
                                    resolve_target: None,
                                    ops: wgpu::Operations {
                                        load: wgpu::LoadOp::Clear(wgpu::Color::BLACK),
                                        store: wgpu::StoreOp::Store,
                                    },
                                })
                            })
                            .collect();
                        let mut pass =
                            prepass_encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                                label: Some("reflection probe"),
                                color_attachments: &attachments,
                                depth_stencil_attachment: None,
                                timestamp_writes: if m == 0 && half == 0 {
                                    pass_timer(tset.as_ref(), &mut timed, "probe")
                                } else {
                                    None
                                },
                                occlusion_query_set: None,
                                multiview_mask: None,
                            });
                        pass.set_pipeline(if m == 0 {
                            &probe.sky_pipeline
                        } else {
                            &probe.filter_pipeline
                        });
                        pass.set_bind_group(0, bg, &[]);
                        pass.set_bind_group(1, sky_bg, &[]);
                        pass.draw(0..3, 0..1);
                    }
                }
            }
        }
        let single = self.options.msaa <= 1;
        let share_depth = prepass_on && single && self.ao.is_some() && !has_presurface;
        let targets = if share_depth {
            None
        } else {
            Some(self.msaa_targets(width, height))
        };
        let msaa_prepass = enhanced
            && !has_presurface
            && (with_overlays || xr_view)
            && !single
            && prepass_on
            && omsi_cfg::env::var_os("OMSI_NO_MSAA_PREPASS").is_none();
        let parts = if !cfg!(any(target_os = "macos", target_os = "ios"))
            && main_bundles.len() >= 2
            && omsi_cfg::env::var_os("OMSI_NO_MAIN_SPLIT").is_none()
        {
            main_bundles.len().min(2)
        } else {
            1
        };
        let mut lead = (parts > 1).then(|| {
            self.device
                .create_command_encoder(&wgpu::CommandEncoderDescriptor {
                    label: Some("main part"),
                })
        });
        if msaa_prepass {
            if let (Some(pipes), Some(t)) = (self.prepass_msaa_pipelines.as_ref(), targets.as_ref())
            {
                let mut pass = lead.as_mut().unwrap_or(&mut encoder).begin_render_pass(
                    &wgpu::RenderPassDescriptor {
                        label: Some("msaa depth prepass"),
                        color_attachments: &[],
                        depth_stencil_attachment: Some(wgpu::RenderPassDepthStencilAttachment {
                            view: &t.1,
                            depth_ops: Some(wgpu::Operations {
                                load: wgpu::LoadOp::Clear(0.0),
                                store: wgpu::StoreOp::Store,
                            }),
                            stencil_ops: None,
                        }),
                        timestamp_writes: pass_timer(tset.as_ref(), &mut timed, "msaa prepass"),
                        occlusion_query_set: None,
                        multiview_mask: None,
                    },
                );
                pass.set_bind_group(0, scene.camera_bind_group.as_ref().unwrap(), &[]);
                encode_batches_filtered(
                    &mut pass,
                    scene,
                    &prepass_batches,
                    |batch| batch.pipe / 2 != PIPE_ALPHA_TEST,
                    |pipe| &pipes[pipe as usize],
                );
            }
        }
        let msaa_prepass =
            msaa_prepass && self.prepass_msaa_pipelines.is_some() && targets.is_some();
        let mut main_parts: Vec<wgpu::CommandEncoder> = Vec::new();
        {
            let sky = lighting.sky_color;
            let msaa_color = targets.as_ref().map(|t| &t.0);
            let depth_view: &wgpu::TextureView = match &targets {
                Some(t) => &t.1,
                None => &self.ao.as_ref().unwrap().depth_view,
            };
            let hdr = if enhanced {
                self.hdr_targets.get(&(width, height))
            } else {
                None
            };
            let (draw_view, resolve_view): (&wgpu::TextureView, Option<&wgpu::TextureView>) =
                match hdr {
                    Some(h) => match &h.msaa_view {
                        Some(m) => (m, Some(&h.view)),
                        None => (&h.view, None),
                    },
                    None => {
                        if single {
                            (scene_view, None)
                        } else {
                            (msaa_color.expect("multisampled target"), Some(scene_view))
                        }
                    }
                };
            let pp = if enhanced {
                self.hdr_pass.as_ref().expect("enhanced pipelines")
            } else {
                &self.pass
            };
            let mask_attachment = hdr.map(|h| wgpu::RenderPassColorAttachment {
                view: h.mask_msaa.as_ref().unwrap_or(&h.mask),
                depth_slice: None,
                resolve_target: h.mask_msaa.as_ref().map(|_| &h.mask),
                ops: wgpu::Operations {
                    load: if parts > 1 {
                        wgpu::LoadOp::Load
                    } else {
                        wgpu::LoadOp::Clear(wgpu::Color::TRANSPARENT)
                    },
                    store: if h.mask_msaa.is_some() {
                        wgpu::StoreOp::Discard
                    } else {
                        wgpu::StoreOp::Store
                    },
                },
            });
            let per_part = main_bundles.len().div_ceil(parts.max(1));
            let sky_clear = wgpu::LoadOp::Clear(wgpu::Color {
                r: sky.x as f64,
                g: sky.y as f64,
                b: sky.z as f64,
                a: 1.0,
            });
            let depth_first = if share_depth || msaa_prepass {
                wgpu::LoadOp::Load
            } else {
                wgpu::LoadOp::Clear(0.0)
            };
            for g in 0..parts.saturating_sub(1) {
                let first = g == 0;
                let mut part = lead.take().unwrap_or_else(|| {
                    self.device
                        .create_command_encoder(&wgpu::CommandEncoderDescriptor {
                            label: Some("main part"),
                        })
                });
                {
                    let part_colors = [
                        Some(wgpu::RenderPassColorAttachment {
                            view: draw_view,
                            depth_slice: None,
                            resolve_target: None,
                            ops: wgpu::Operations {
                                load: if first { sky_clear } else { wgpu::LoadOp::Load },
                                store: wgpu::StoreOp::Store,
                            },
                        }),
                        hdr.map(|h| wgpu::RenderPassColorAttachment {
                            view: h.mask_msaa.as_ref().unwrap_or(&h.mask),
                            depth_slice: None,
                            resolve_target: None,
                            ops: wgpu::Operations {
                                load: if first {
                                    wgpu::LoadOp::Clear(wgpu::Color::TRANSPARENT)
                                } else {
                                    wgpu::LoadOp::Load
                                },
                                store: wgpu::StoreOp::Store,
                            },
                        }),
                    ];
                    let mut pass = part.begin_render_pass(&wgpu::RenderPassDescriptor {
                        label: Some("main part"),
                        color_attachments: if part_colors[1].is_some() {
                            &part_colors[..]
                        } else {
                            &part_colors[..1]
                        },
                        depth_stencil_attachment: Some(wgpu::RenderPassDepthStencilAttachment {
                            view: depth_view,
                            depth_ops: Some(wgpu::Operations {
                                load: if first {
                                    depth_first
                                } else {
                                    wgpu::LoadOp::Load
                                },
                                store: wgpu::StoreOp::Store,
                            }),
                            stencil_ops: None,
                        }),
                        timestamp_writes: None,
                        occlusion_query_set: None,
                        multiview_mask: None,
                    });
                    pass.set_bind_group(0, scene.camera_bind_group.as_ref().unwrap(), &[]);
                    if first {
                        if let Some(sky) = &scene.sky_bind_group {
                            pass.set_pipeline(&pp.sky_pipeline);
                            pass.set_bind_group(1, sky, &[]);
                            pass.set_vertex_buffer(0, Some(self.sky_mesh.0.slice(..)));
                            pass.set_index_buffer(
                                self.sky_mesh.1.slice(..),
                                wgpu::IndexFormat::Uint32,
                            );
                            pass.draw_indexed(0..self.sky_mesh.2, 0, 0..1);
                        }
                    }
                    pass.execute_bundles(
                        main_bundles[g * per_part..((g + 1) * per_part).min(main_bundles.len())]
                            .iter(),
                    );
                }
                main_parts.push(part);
            }
            let tail = (parts - 1) * per_part;
            let main_attachment = Some(wgpu::RenderPassColorAttachment {
                view: draw_view,
                depth_slice: None,
                resolve_target: resolve_view,
                ops: wgpu::Operations {
                    load: if parts > 1 {
                        wgpu::LoadOp::Load
                    } else {
                        sky_clear
                    },
                    store: if resolve_view.is_none() {
                        wgpu::StoreOp::Store
                    } else {
                        wgpu::StoreOp::Discard
                    },
                },
            });
            let colors = [main_attachment, mask_attachment];
            let colors = if colors[1].is_some() {
                &colors[..]
            } else {
                &colors[..1]
            };
            let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                label: Some("main"),
                color_attachments: colors,
                depth_stencil_attachment: Some(wgpu::RenderPassDepthStencilAttachment {
                    view: depth_view,
                    depth_ops: Some(wgpu::Operations {
                        load: if parts > 1 {
                            wgpu::LoadOp::Load
                        } else {
                            depth_first
                        },
                        store: if share_depth || msaa_prepass || ao_on {
                            wgpu::StoreOp::Store
                        } else {
                            wgpu::StoreOp::Discard
                        },
                    }),
                    stencil_ops: None,
                }),
                timestamp_writes: pass_timer(
                    tset.as_ref(),
                    &mut timed,
                    if with_overlays { "main" } else { "mirror" },
                ),
                occlusion_query_set: None,
                multiview_mask: None,
            });
            pass.set_bind_group(0, scene.camera_bind_group.as_ref().unwrap(), &[]);
            if let Some(sky) = scene.sky_bind_group.as_ref().filter(|_| parts == 1) {
                pass.set_pipeline(&pp.sky_pipeline);
                pass.set_bind_group(1, sky, &[]);
                pass.set_vertex_buffer(0, Some(self.sky_mesh.0.slice(..)));
                pass.set_index_buffer(self.sky_mesh.1.slice(..), wgpu::IndexFormat::Uint32);
                pass.draw_indexed(0..self.sky_mesh.2, 0, 0..1);
            }
            if main_bundles.is_empty() {
                pass.set_bind_group(0, scene.camera_bind_group.as_ref().unwrap(), &[]);
                encode_batches(&mut pass, scene, &main_batches, |pipe| {
                    main_pipeline(pp, pipe)
                });
            } else {
                pass.execute_bundles(main_bundles[tail..].iter());
                pass.set_bind_group(0, scene.camera_bind_group.as_ref().unwrap(), &[]);
            }
            if scene.smoke_count > 0 && omsi_cfg::env::var_os("OMSI_NO_SMOKE").is_none() {
                if let Some(sb) = &scene.smoke_buf {
                    pass.set_pipeline(&pp.smoke_pipeline);
                    pass.set_bind_group(1, &self.smoke_bind_group, &[]);
                    pass.set_vertex_buffer(0, Some(sb.slice(..)));
                    pass.draw(0..6, 0..scene.smoke_count);
                }
            }
            if scene.corona_count > 0 && omsi_cfg::env::var_os("OMSI_NO_CORONAS").is_none() {
                if let Some(cb) = &scene.corona_buf {
                    pass.set_pipeline(&pp.corona_pipeline);
                    pass.set_vertex_buffer(0, Some(cb.slice(..)));
                    for &(tex, first, count) in &scene.corona_runs {
                        let bg = self
                            .corona_textures
                            .get(tex as usize)
                            .and_then(|b| b.as_ref())
                            .unwrap_or(&self.corona_bind_group);
                        pass.set_bind_group(1, bg, &[]);
                        pass.draw(0..6, first..first + count);
                    }
                }
            }
            if !overlays.is_empty() && !enhanced && !scaled {
                pass.set_pipeline(&self.overlay_pipeline);
                for (k, _) in overlays.iter().enumerate() {
                    if let Some((_, _, bg, _)) = scene.overlay_res.get(k) {
                        pass.set_bind_group(0, bg, &[]);
                        pass.draw(0..6, 0..1);
                    }
                }
            }
        }
        let puddles_on = puddles_wanted
            && main_batches
            .iter()
            .any(|b| scene.materials[b.material as usize].uniform.params2[2] > 0.0)
            && self.prepare_puddle_reflections(
            width, height, camera, aspect, projection, &cu, lighting,
        );
        if puddles_on {
            self.encode_puddle_reflections(
                &mut encoder,
                width,
                height,
                scene,
                &main_batches,
                &list,
                lighting,
                camera,
                tset.as_ref(),
                &mut timed,
            );
        }
        if enhanced {
            let secs = |tau: f32| {
                if dt > 0.0 {
                    1.0 - (-dt / tau).exp()
                } else {
                    1.0
                }
            };
            let m = meter_tuning();
            let pu = PostUniform {
                a: [
                    0.035,
                    m[2],
                    m[3],
                    if self.instant_exposure || dt <= 0.0 {
                        1.0
                    } else {
                        0.0
                    },
                ],
                b: [secs(2.5), secs(0.6), m[1], m[4]],
                c: [
                    m[0],
                    m[5],
                    self.exposure.map(f32::exp).unwrap_or(1.0),
                    lighting.led_glow * 10.0,
                ],
            };
            self.queue
                .write_buffer(&self.post_buf, 0, bytemuck::bytes_of(&pu));
            let fxaa = with_overlays
                && self.options.fxaa
                && omsi_cfg::env::var_os("OMSI_NO_FXAA").is_none();
            if let Some(h) = self.hdr_targets.get(&(width, height)) {
                let puddles = h.puddles.as_ref().filter(|_| puddles_on);
                let levels = h.down.len();
                for i in 0..levels {
                    post_pass(
                        &mut encoder,
                        &h.down[i],
                        None,
                        if i == 0 {
                            &self.post.down_first
                        } else {
                            &self.post.down
                        },
                        if i == 0 {
                            puddles.map(|p| &p.down_bg).unwrap_or(&h.down_bg[i])
                        } else {
                            &h.down_bg[i]
                        },
                    );
                }
                if lead_view {
                    post_pass(
                        &mut encoder,
                        &self.meter_view,
                        None,
                        &self.post.meter,
                        &h.meter_bg,
                    );
                    let front = self.adapt_front;
                    post_pass(
                        &mut encoder,
                        &self.adapt_views[1 - front],
                        None,
                        &self.post.adapt,
                        &self.adapt_bg[front],
                    );
                    self.adapt_front = 1 - front;
                }
                let lost = self.device_lost().is_some();
                if let Some(log) = self
                    .exposure_log
                    .as_mut()
                    .filter(|_| with_overlays && !lost)
                {
                    let pre = self.exposure.unwrap_or(0.0) / std::f32::consts::LN_2;
                    log.sample(&mut encoder, &self.adapt_views[self.adapt_front], pre, m);
                }
                for i in (0..levels).rev() {
                    let timer = if i == 0 {
                        pass_timer(tset.as_ref(), &mut timed, "glow+meter")
                    } else {
                        None
                    };
                    post_pass(&mut encoder, &h.up[i], timer, &self.post.up, &h.up_bg[i]);
                }
                let final_view = if fxaa { &h.ldr } else { scene_view };
                let tonemap_bg =
                    &puddles.map(|p| &p.tonemap_bg).unwrap_or(&h.tonemap_bg)[self.adapt_front];
                let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                    label: Some("tone map"),
                    color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                        view: final_view,
                        depth_slice: None,
                        resolve_target: None,
                        ops: wgpu::Operations {
                            load: wgpu::LoadOp::Clear(wgpu::Color::BLACK),
                            store: wgpu::StoreOp::Store,
                        },
                    })],
                    depth_stencil_attachment: None,
                    timestamp_writes: pass_timer(tset.as_ref(), &mut timed, "tone map"),
                    occlusion_query_set: None,
                    multiview_mask: None,
                });
                pass.set_pipeline(if fxaa {
                    &self.post.tonemap_encoded
                } else {
                    &self.post.tonemap
                });
                pass.set_bind_group(0, tonemap_bg, &[]);
                pass.draw(0..3, 0..1);
                if fxaa {
                    drop(pass);
                    pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                        label: Some("fxaa"),
                        color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                            view: scene_view,
                            depth_slice: None,
                            resolve_target: None,
                            ops: wgpu::Operations {
                                load: wgpu::LoadOp::Clear(wgpu::Color::BLACK),
                                store: wgpu::StoreOp::Store,
                            },
                        })],
                        depth_stencil_attachment: None,
                        timestamp_writes: pass_timer(tset.as_ref(), &mut timed, "fxaa"),
                        occlusion_query_set: None,
                        multiview_mask: None,
                    });
                    pass.set_pipeline(&self.post.fxaa);
                    pass.set_bind_group(0, &h.fxaa_bg, &[]);
                    pass.draw(0..3, 0..1);
                }
                if !overlays.is_empty() && !scaled {
                    pass.set_pipeline(&self.overlay_pipeline_1x);
                    for (k, _) in overlays.iter().enumerate() {
                        if let Some((_, _, bg, _)) = scene.overlay_res.get(k) {
                            pass.set_bind_group(0, bg, &[]);
                            pass.draw(0..6, 0..1);
                        }
                    }
                }
            }
        }
        if let Some((_, bg)) = &scene_target {
            let sharpen = (1.0 - width as f32 / full_w as f32) * 2.0;
            self.queue.write_buffer(
                &self.upscale_buf,
                0,
                bytemuck::cast_slice(&[
                    width as f32,
                    height as f32,
                    sharpen.clamp(0.0, 0.8),
                    if vanilla_fxaa { 1.0 } else { 0.0 },
                ]),
            );
            if glass_key.is_some_and(|k| !k.0) {
                if self.glass_prev.as_ref().map(|g| g.1) != Some((width, height)) {
                    let tex = self.device.create_texture(&wgpu::TextureDescriptor {
                        label: Some("picture behind the glass"),
                        size: wgpu::Extent3d {
                            width: (width / 2).max(1),
                            height: (height / 2).max(1),
                            depth_or_array_layers: 1,
                        },
                        mip_level_count: 1,
                        sample_count: 1,
                        dimension: wgpu::TextureDimension::D2,
                        format: self.format,
                        usage: wgpu::TextureUsages::RENDER_ATTACHMENT
                            | wgpu::TextureUsages::TEXTURE_BINDING,
                        view_formats: &[],
                    });
                    self.glass_prev = Some((tex.create_view(&Default::default()), (width, height)));
                }
                let view = &self.glass_prev.as_ref().unwrap().0;
                let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                    label: Some("picture behind the glass"),
                    color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                        view,
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
                pass.set_pipeline(&self.upscale_pipeline);
                pass.set_bind_group(0, bg, &[]);
                pass.draw(0..3, 0..1);
            }
            let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                label: Some("upscale"),
                color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                    view: target,
                    depth_slice: None,
                    resolve_target: None,
                    ops: wgpu::Operations {
                        load: wgpu::LoadOp::Clear(wgpu::Color::BLACK),
                        store: wgpu::StoreOp::Store,
                    },
                })],
                depth_stencil_attachment: None,
                timestamp_writes: pass_timer(tset.as_ref(), &mut timed, "upscale"),
                occlusion_query_set: None,
                multiview_mask: None,
            });
            pass.set_pipeline(&self.upscale_pipeline);
            pass.set_bind_group(0, bg, &[]);
            pass.draw(0..3, 0..1);
            if !overlays.is_empty() {
                pass.set_pipeline(&self.overlay_pipeline_1x);
                for (k, _) in overlays.iter().enumerate() {
                    if let Some((_, _, bg, _)) = scene.overlay_res.get(k) {
                        pass.set_bind_group(0, bg, &[]);
                        pass.draw(0..6, 0..1);
                    }
                }
            }
        }
        if let Some(k) = glass_key {
            self.glass_live = Some(k);
            self.show_glass_behind(scene, k);
        }
        stage(self, "encode", "mirror.encode");
        let big = shadow_batches.iter().map(|b| b.len()).sum::<usize>() + prepass_batches.len()
            > 64
            || !main_parts.is_empty();
        let profiling = self.profiling;
        let finish = |encoder: wgpu::CommandEncoder| {
            let start = profiling.then(std::time::Instant::now);
            let commands = encoder.finish();
            (
                commands,
                start.map(|t| t.elapsed().as_secs_f64()).unwrap_or(0.0),
            )
        };
        let (shadow_commands, prepass_commands, part_commands, commands, finish_times) =
            if big && self.encoding_pool.is_some() {
                self.encoding_pool
                    .as_ref()
                    .unwrap()
                    .in_place_scope_fifo(|scope| {
                        let (shadow_tx, shadow_rx) = std::sync::mpsc::sync_channel(1);
                        let (prepass_tx, prepass_rx) = std::sync::mpsc::sync_channel(1);
                        let parts = main_parts.len();
                        let (part_tx, part_rx) = std::sync::mpsc::sync_channel(parts.max(1));
                        let finish = &finish;
                        scope.spawn_fifo(move |_| {
                            let _ = shadow_tx.send(finish(shadow_encoder));
                        });
                        scope.spawn_fifo(move |_| {
                            let _ = prepass_tx.send(finish(prepass_encoder));
                        });
                        for (k, e) in main_parts.into_iter().enumerate() {
                            let tx = part_tx.clone();
                            scope.spawn_fifo(move |_| {
                                let _ = tx.send((k, finish(e).0));
                            });
                        }
                        let (commands, main_secs) = finish(encoder);
                        let wait = std::time::Instant::now();
                        let (shadow_commands, shadow_secs) =
                            shadow_rx.recv().expect("command encoding worker");
                        let shadow_wait = wait.elapsed().as_secs_f64();
                        let wait = std::time::Instant::now();
                        let (prepass_commands, prepass_secs) =
                            prepass_rx.recv().expect("command encoding worker");
                        let mut part_commands: Vec<Option<wgpu::CommandBuffer>> =
                            (0..parts).map(|_| None).collect();
                        for _ in 0..parts {
                            let (k, c) = part_rx.recv().expect("command encoding worker");
                            part_commands[k] = Some(c);
                        }
                        (
                            shadow_commands,
                            prepass_commands,
                            part_commands
                                .into_iter()
                                .map(|c| c.expect("main pass part"))
                                .collect::<Vec<_>>(),
                            commands,
                            [
                                shadow_secs,
                                prepass_secs,
                                main_secs,
                                shadow_wait,
                                wait.elapsed().as_secs_f64(),
                            ],
                        )
                    })
            } else if big {
                std::thread::scope(|scope| {
                    let finish = &finish;
                    let shadow = scope.spawn(move || finish(shadow_encoder));
                    let prepass = scope.spawn(move || finish(prepass_encoder));
                    let parts: Vec<_> = main_parts
                        .into_iter()
                        .map(|e| scope.spawn(move || finish(e).0))
                        .collect();
                    let (commands, main_secs) = finish(encoder);
                    let wait = std::time::Instant::now();
                    let (shadow_commands, shadow_secs) =
                        shadow.join().expect("command encoding thread");
                    let shadow_wait = wait.elapsed().as_secs_f64();
                    let wait = std::time::Instant::now();
                    let (prepass_commands, prepass_secs) =
                        prepass.join().expect("command encoding thread");
                    (
                        shadow_commands,
                        prepass_commands,
                        parts
                            .into_iter()
                            .map(|h| h.join().expect("command encoding thread"))
                            .collect::<Vec<_>>(),
                        commands,
                        [
                            shadow_secs,
                            prepass_secs,
                            main_secs,
                            shadow_wait,
                            wait.elapsed().as_secs_f64(),
                        ],
                    )
                })
            } else {
                let (shadow, shadow_secs) = finish(shadow_encoder);
                let (prepass, prepass_secs) = finish(prepass_encoder);
                let parts: Vec<_> = main_parts.into_iter().map(|e| finish(e).0).collect();
                let (main, main_secs) = finish(encoder);
                (
                    shadow,
                    prepass,
                    parts,
                    main,
                    [shadow_secs, prepass_secs, main_secs, 0.0, 0.0],
                )
            };
        if self.profiling {
            let keys = if with_overlays {
                [
                    "finish.shadow",
                    "finish.prepass",
                    "finish.main",
                    "finish.wait shadow",
                    "finish.wait prepass",
                ]
            } else {
                [
                    "mirror.finish.shadow",
                    "mirror.finish.prepass",
                    "mirror.finish.main",
                    "mirror.finish.wait shadow",
                    "mirror.finish.wait prepass",
                ]
            };
            for (key, secs) in keys.into_iter().zip(finish_times) {
                *self.stats.borrow_mut().entry(key).or_default() += secs;
            }
        }
        stage(self, "finish", "mirror.finish");
        self.queue.submit(
            [shadow_commands, prepass_commands]
                .into_iter()
                .chain(part_commands)
                .chain([commands]),
        );
        stage(self, "submit", "mirror.submit");
        if let (Some(t), false) = (
            self.gpu_timers[with_overlays as usize].as_mut(),
            timed.is_empty(),
        ) {
            t.pending = timed;
            t.unresolved = true;
        }
    }

    pub fn render_to_image(
        &mut self,
        scene: &mut Scene,
        width: u32,
        height: u32,
        camera: &Camera,
        lighting: &Lighting,
    ) -> Result<Vec<u8>> {
        let tex = self.device.create_texture(&wgpu::TextureDescriptor {
            label: Some("offscreen"),
            size: wgpu::Extent3d {
                width,
                height,
                depth_or_array_layers: 1,
            },
            mip_level_count: 1,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format: self.format,
            usage: wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::COPY_SRC,
            view_formats: &[],
        });
        let view = tex.create_view(&Default::default());
        self.instant_exposure = true;
        self.render(scene, &view, width, height, camera, lighting);
        self.instant_exposure = false;
        let bpr = (width * 4).div_ceil(256) * 256;
        let buf = self.device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("readback"),
            size: (bpr * height) as u64,
            usage: wgpu::BufferUsages::COPY_DST | wgpu::BufferUsages::MAP_READ,
            mapped_at_creation: false,
        });
        let mut enc = self.device.create_command_encoder(&Default::default());
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
                    rows_per_image: Some(height),
                },
            },
            wgpu::Extent3d {
                width,
                height,
                depth_or_array_layers: 1,
            },
        );
        let idx = self.queue.submit([enc.finish()]);
        let slice = buf.slice(..);
        let (tx, rx) = std::sync::mpsc::channel();
        slice.map_async(wgpu::MapMode::Read, move |r| {
            let _ = tx.send(r);
        });
        wait_gpu(&self.device, Some(idx)).map_err(|e| anyhow!("poll: {e:?}"))?;
        rx.recv()
            .context("map")?
            .map_err(|e| anyhow!("map: {e:?}"))?;
        let data = slice.get_mapped_range().expect("mapped range");
        let mut out = Vec::with_capacity((width * height * 4) as usize);
        for row in 0..height {
            let start = (row * bpr) as usize;
            out.extend_from_slice(&data[start..start + (width * 4) as usize]);
        }
        drop(data);
        buf.unmap();
        if matches!(
            self.format,
            wgpu::TextureFormat::Bgra8Unorm | wgpu::TextureFormat::Bgra8UnormSrgb
        ) {
            for px in out.chunks_exact_mut(4) {
                px.swap(0, 2);
            }
        }
        Ok(out)
    }
}

fn buffer_init(
    device: &wgpu::Device,
    queue: &wgpu::Queue,
    label: Option<&str>,
    contents: &[u8],
    usage: wgpu::BufferUsages,
) -> wgpu::Buffer {
    let align = wgpu::COPY_BUFFER_ALIGNMENT as usize;
    let size = contents.len().next_multiple_of(align).max(align);
    let buf = device.create_buffer(&wgpu::BufferDescriptor {
        label,
        size: size as u64,
        usage: usage | wgpu::BufferUsages::COPY_DST,
        mapped_at_creation: false,
    });
    if !contents.is_empty() {
        if contents.len() == size {
            queue.write_buffer(&buf, 0, contents);
        } else {
            let mut padded = contents.to_vec();
            padded.resize(size, 0);
            queue.write_buffer(&buf, 0, &padded);
        }
    }
    buf
}

fn make_mesh(device: &wgpu::Device, queue: &wgpu::Queue, data: &MeshData) -> GpuMesh {
    let verts: Vec<Vertex> = data
        .positions
        .iter()
        .zip(&data.normals)
        .zip(&data.uvs)
        .map(|((p, n), uv)| Vertex {
            pos: p.to_array(),
            normal: n.to_array(),
            uv: uv.to_array(),
        })
        .collect();
    let vertex_buf = buffer_init(
        device,
        queue,
        None,
        bytemuck::cast_slice(&verts),
        wgpu::BufferUsages::VERTEX | wgpu::BufferUsages::COPY_DST,
    );
    let index_buf = buffer_init(
        device,
        queue,
        None,
        bytemuck::cast_slice(&data.indices),
        wgpu::BufferUsages::INDEX,
    );
    let (mut lo, mut hi) = (Vec3::splat(f32::MAX), Vec3::splat(f32::MIN));
    for p in &data.positions {
        lo = lo.min(*p);
        hi = hi.max(*p);
    }
    if data.positions.is_empty() {
        lo = Vec3::ZERO;
        hi = Vec3::ZERO;
    }
    let center = (lo + hi) * 0.5;
    GpuMesh {
        vertex_buf,
        index_buf,
        ranges: data.ranges.clone(),
        bounds_center: center,
        bounds_radius: (hi - center).length(),
        one_sided: data.one_sided,
        source: None,
    }
}

pub struct PreparedMesh(GpuMesh);

pub fn prepare_mesh(device: &wgpu::Device, queue: &wgpu::Queue, data: &MeshData) -> PreparedMesh {
    let _turn = gl_worker_turn();
    PreparedMesh(make_mesh(device, queue, data))
}

fn drawn_by(l: &PointLight, enhanced: bool) -> bool {
    l.radius > 0.0
        && l.intensity > 0.0
        && l.mode
        != if enhanced {
        LightMode::Vanilla
    } else {
        LightMode::Enhanced
    }
}

fn gpu_light(l: &PointLight, p: Vec3) -> GpuPointLight {
    let spot = l.direction.length_squared() > 1e-6;
    let dir = if spot {
        l.direction.normalize().extend(l.cone[1]).to_array()
    } else if l.mode == LightMode::Vanilla {
        [1.0, 0.0, 0.0, -2.0]
    } else {
        [0.0, 0.0, 0.0, -2.0]
    };
    let vanilla_radius = if l.mode == LightMode::Enhanced {
        0.0
    } else {
        l.radius
    };
    GpuPointLight {
        pos: [p.x, p.y, p.z, vanilla_radius],
        color: [l.color[0], l.color[1], l.color[2], l.intensity],
        dir,
        extra: [l.cone[0], l.core, l.beam, l.radius],
        occ: [0.0; 4],
    }
}

fn debug_view() -> f32 {
    #[cfg(all(feature = "devtools", debug_assertions))]
    if let Some(v) = devtools::debug_view_override() {
        return v as f32;
    }
    static VIEW: std::sync::OnceLock<f32> = std::sync::OnceLock::new();
    *VIEW.get_or_init(|| {
        omsi_cfg::env::var("OMSI_DEBUG_ENHANCED")
            .ok()
            .and_then(|v| v.parse().ok())
            .unwrap_or(0.0)
    })
}

fn night_scale(night: f32, brightness: f32) -> f32 {
    1.0 + (brightness.clamp(0.0, 4.0) - 1.0) * night.clamp(0.0, 1.0)
}

fn meter_tuning() -> [f32; 6] {
    static METER: std::sync::OnceLock<[f32; 6]> = std::sync::OnceLock::new();
    *METER.get_or_init(|| {
        let mut m = [
            METER_GAIN,
            METER_TARGET,
            METER_DARKEN,
            METER_BRIGHTEN,
            0.0,
            NIGHT_VISION,
        ];
        if let Ok(v) = omsi_cfg::env::var("OMSI_METER") {
            for (k, x) in v.split(',').take(6).enumerate() {
                if let Ok(x) = x.trim().parse() {
                    m[k] = x;
                }
            }
        }
        m
    })
}

fn sky_input_differs(a: &atmosphere::SkyInput, b: &atmosphere::SkyInput) -> bool {
    let near = |x: f32, y: f32, tol: f32| (x - y).abs() <= tol;
    a.sun_dir.dot(b.sun_dir) < 0.999_998
        || !near(a.sun_visibility, b.sun_visibility, 0.01)
        || !near(a.overcast, b.overcast, 0.01)
        || !near(a.haze, b.haze, 0.02)
        || !near(a.rain, b.rain, 0.01)
        || !near(a.ground_albedo, b.ground_albedo, 0.01)
        || !near(a.night_light, b.night_light, 0.01)
        || a.tint
        .iter()
        .zip(&b.tint)
        .any(|(x, y)| (*x - *y).abs().max_element() > 0.01)
}

fn scene_shader_source(gl: bool) -> String {
    let src = [
        include_str!("shader.wgsl"),
        include_str!("enhanced_common.wgsl"),
        include_str!("puddle_common.wgsl"),
        include_str!("enhanced.wgsl"),
    ]
        .join("\n");
    if !gl {
        return src;
    }
    let clamped = |t: &str| {
        format!(
            "textureSample({t}, s_diffuse, clamp(uv, 0.5 / vec2<f32>(textureDimensions({t})), \
             vec2<f32>(1.0) - 0.5 / vec2<f32>(textureDimensions({t}))))"
        )
    };
    let out = src
        .replace("textureSample(t_trans, s_tile, uv)", &clamped("t_trans"))
        .replace("textureSample(t_night, s_tile, uv)", &clamped("t_night"));
    debug_assert!(!out.contains("s_tile, uv)"));
    out
}

fn cloud_noise_textures(
    device: &wgpu::Device,
    queue: &wgpu::Queue,
) -> (wgpu::TextureView, wgpu::TextureView, wgpu::Sampler) {
    let t0 = std::time::Instant::now();
    let (shape, detail) = std::thread::scope(|s| {
        let a = s.spawn(clouds::shape_map);
        let b = s.spawn(clouds::detail_volume);
        (
            a.join().expect("cloud shape"),
            b.join().expect("cloud detail"),
        )
    });
    let make = |label: &str,
                size: u32,
                dim: wgpu::TextureDimension,
                format: wgpu::TextureFormat,
                bpp: u32,
                levels: &[Vec<u8>]| {
        let depth = if dim == wgpu::TextureDimension::D3 {
            size
        } else {
            1
        };
        let tex = device.create_texture(&wgpu::TextureDescriptor {
            label: Some(label),
            size: wgpu::Extent3d {
                width: size,
                height: size,
                depth_or_array_layers: depth,
            },
            mip_level_count: levels.len() as u32,
            sample_count: 1,
            dimension: dim,
            format,
            usage: wgpu::TextureUsages::TEXTURE_BINDING | wgpu::TextureUsages::COPY_DST,
            view_formats: &[],
        });
        for (m, data) in levels.iter().enumerate() {
            let e = (size >> m).max(1);
            let d = if dim == wgpu::TextureDimension::D3 {
                e
            } else {
                1
            };
            queue.write_texture(
                wgpu::TexelCopyTextureInfo {
                    texture: &tex,
                    mip_level: m as u32,
                    origin: wgpu::Origin3d::ZERO,
                    aspect: wgpu::TextureAspect::All,
                },
                data,
                wgpu::TexelCopyBufferLayout {
                    offset: 0,
                    bytes_per_row: Some(e * bpp),
                    rows_per_image: Some(e),
                },
                wgpu::Extent3d {
                    width: e,
                    height: e,
                    depth_or_array_layers: d,
                },
            );
        }
        tex.create_view(&wgpu::TextureViewDescriptor::default())
    };
    let shape_view = make(
        "cloud shape",
        clouds::SHAPE_SIZE,
        wgpu::TextureDimension::D2,
        wgpu::TextureFormat::Rgba8Unorm,
        4,
        &shape,
    );
    let detail_view = make(
        "cloud detail",
        clouds::DETAIL_SIZE,
        wgpu::TextureDimension::D3,
        wgpu::TextureFormat::R8Unorm,
        1,
        &detail,
    );
    let sampler = device.create_sampler(&wgpu::SamplerDescriptor {
        label: Some("cloud noise"),
        address_mode_u: wgpu::AddressMode::Repeat,
        address_mode_v: wgpu::AddressMode::Repeat,
        address_mode_w: wgpu::AddressMode::Repeat,
        mag_filter: wgpu::FilterMode::Linear,
        min_filter: wgpu::FilterMode::Linear,
        mipmap_filter: wgpu::MipmapFilterMode::Linear,
        ..Default::default()
    });
    log::info!("cloud noise made in {:.2} s", t0.elapsed().as_secs_f32());
    (shape_view, detail_view, sampler)
}

fn sky_shader_source() -> String {
    [
        include_str!("sky.wgsl"),
        include_str!("enhanced_common.wgsl"),
        include_str!("sky_enhanced.wgsl"),
    ]
        .join("\n")
}

fn corona_shader_source() -> String {
    [
        include_str!("corona.wgsl"),
        include_str!("enhanced_common.wgsl"),
    ]
        .join("\n")
}

fn post_pass(
    encoder: &mut wgpu::CommandEncoder,
    view: &wgpu::TextureView,
    timer: Option<wgpu::RenderPassTimestampWrites<'_>>,
    pipeline: &wgpu::RenderPipeline,
    bg: &wgpu::BindGroup,
) {
    let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
        label: Some("post"),
        color_attachments: &[Some(wgpu::RenderPassColorAttachment {
            view,
            depth_slice: None,
            resolve_target: None,
            ops: wgpu::Operations {
                load: wgpu::LoadOp::Clear(wgpu::Color::BLACK),
                store: wgpu::StoreOp::Store,
            },
        })],
        depth_stencil_attachment: None,
        timestamp_writes: timer,
        occlusion_query_set: None,
        multiview_mask: None,
    });
    pass.set_pipeline(pipeline);
    pass.set_bind_group(0, bg, &[]);
    pass.draw(0..3, 0..1);
}

struct ExposureLog {
    buf: wgpu::Buffer,
    ready: Arc<std::sync::atomic::AtomicBool>,
    waiting: bool,
    frame: u64,
    started: std::time::Instant,
    pending: (f32, [f32; 6]),
    ev: f32,
    log: bool,
}

impl ExposureLog {
    fn new(device: &wgpu::Device) -> Option<ExposureLog> {
        let buf = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("exposure readback"),
            size: 256,
            usage: wgpu::BufferUsages::COPY_DST | wgpu::BufferUsages::MAP_READ,
            mapped_at_creation: false,
        });
        Some(ExposureLog {
            buf,
            ready: Arc::new(std::sync::atomic::AtomicBool::new(false)),
            waiting: false,
            frame: 0,
            started: std::time::Instant::now(),
            pending: (0.0, [0.0; 6]),
            ev: 0.0,
            log: omsi_cfg::env::var_os("OMSI_DEBUG_EXPOSURE").is_some(),
        })
    }

    fn sample(
        &mut self,
        encoder: &mut wgpu::CommandEncoder,
        adapted: &wgpu::TextureView,
        pre_log2: f32,
        meter: [f32; 6],
    ) {
        self.frame += 1;
        if self.waiting && self.ready.swap(false, std::sync::atomic::Ordering::Relaxed) {
            {
                let view = self
                    .buf
                    .slice(0..8)
                    .get_mapped_range()
                    .expect("mapped range");
                let bits = u16::from_le_bytes([view[0], view[1]]);
                let metered = half_to_f32(bits);
                let (pre, m) = self.pending;
                let ev = ((m[1] - metered) * m[0]).clamp(-m[2], m[3]) + m[4];
                if ev.is_finite() {
                    self.ev = ev;
                }
                if self.log {
                    log::info!(
                        "exposure t={:.2}s: light model {:+.2} EV, metered picture log2 {:+.2}, correction {:+.2} EV, total {:+.2} EV",
                        self.started.elapsed().as_secs_f32(),
                        pre,
                        metered,
                        ev,
                        pre + ev
                    );
                }
            }
            self.buf.unmap();
            self.waiting = false;
        }
        if self.waiting || self.frame % 8 != 0 {
            return;
        }
        encoder.copy_texture_to_buffer(
            wgpu::TexelCopyTextureInfo {
                texture: adapted.texture(),
                mip_level: 0,
                origin: wgpu::Origin3d::ZERO,
                aspect: wgpu::TextureAspect::All,
            },
            wgpu::TexelCopyBufferInfo {
                buffer: &self.buf,
                layout: wgpu::TexelCopyBufferLayout {
                    offset: 0,
                    bytes_per_row: Some(256),
                    rows_per_image: Some(1),
                },
            },
            wgpu::Extent3d {
                width: 1,
                height: 1,
                depth_or_array_layers: 1,
            },
        );
        self.pending = (pre_log2, meter);
        let ready = self.ready.clone();
        self.waiting = true;
        encoder.map_buffer_on_submit(&self.buf, wgpu::MapMode::Read, .., move |r| {
            ready.store(r.is_ok(), std::sync::atomic::Ordering::Relaxed)
        });
    }
}

fn half_to_f32(b: u16) -> f32 {
    let sign = if b & 0x8000 != 0 { -1.0 } else { 1.0 };
    let e = ((b >> 10) & 0x1f) as i32;
    let m = (b & 0x3ff) as f32;
    match e {
        0 => sign * m / 1024.0 * 2f32.powi(-14),
        31 => sign * f32::INFINITY,
        _ => sign * (1.0 + m / 1024.0) * 2f32.powi(e - 15),
    }
}

struct GpuTimers {
    set: wgpu::QuerySet,
    resolve: wgpu::Buffer,
    read: wgpu::Buffer,
    pending: Vec<&'static str>,
    unresolved: bool,
    waiting: bool,
    ready: Arc<std::sync::atomic::AtomicBool>,
    totals: std::collections::BTreeMap<&'static str, (f64, u32)>,
}

const GPU_TIMER_PASSES: u32 = 16;

impl GpuTimers {
    fn new(device: &wgpu::Device) -> Option<GpuTimers> {
        if omsi_cfg::env::var_os("OMSI_GPU_TIMERS").is_none()
            || !device.features().contains(wgpu::Features::TIMESTAMP_QUERY)
        {
            return None;
        }
        let set = device.create_query_set(&wgpu::QuerySetDescriptor {
            label: Some("pass timers"),
            ty: wgpu::QueryType::Timestamp,
            count: GPU_TIMER_PASSES * 2,
        });
        let size = (GPU_TIMER_PASSES as u64 * 16).div_ceil(wgpu::QUERY_RESOLVE_BUFFER_ALIGNMENT)
            * wgpu::QUERY_RESOLVE_BUFFER_ALIGNMENT;
        let resolve = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("pass timers"),
            size,
            usage: wgpu::BufferUsages::QUERY_RESOLVE | wgpu::BufferUsages::COPY_SRC,
            mapped_at_creation: false,
        });
        let read = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("pass timers read"),
            size,
            usage: wgpu::BufferUsages::MAP_READ | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });
        Some(GpuTimers {
            set,
            resolve,
            read,
            pending: Vec::new(),
            unresolved: false,
            waiting: false,
            ready: Arc::new(std::sync::atomic::AtomicBool::new(false)),
            totals: Default::default(),
        })
    }
}

fn pass_timer<'a>(
    set: Option<&'a wgpu::QuerySet>,
    timed: &mut Vec<&'static str>,
    label: &'static str,
) -> Option<wgpu::RenderPassTimestampWrites<'a>> {
    let set = set?;
    if timed.len() as u32 >= GPU_TIMER_PASSES {
        return None;
    }
    let i = timed.len() as u32 * 2;
    timed.push(label);
    Some(wgpu::RenderPassTimestampWrites {
        query_set: set,
        beginning_of_pass_write_index: Some(i),
        end_of_pass_write_index: Some(i + 1),
    })
}

fn origin_key(origin: DVec3) -> [u64; 3] {
    origin
        .to_array()
        .map(|v| if v == 0.0 { 0 } else { v.to_bits() })
}

fn nearest_by_origin(items: impl IntoIterator<Item = (DVec3, f32)>) -> HashMap<[u64; 3], f32> {
    let mut out: HashMap<[u64; 3], f32> = HashMap::new();
    for (origin, d) in items {
        if origin.is_nan() {
            continue;
        }
        out.entry(origin_key(origin))
            .and_modify(|best| *best = best.min(d))
            .or_insert(d);
    }
    out
}

#[derive(Clone, Copy)]
struct DrawItem {
    pipe: u8,
    mesh: u32,
    range: u32,
    material: u32,
    entry: u32,
}

struct Batch {
    pipe: u8,
    mesh: u32,
    first: u32,
    count: u32,
    material: u32,
    instances: std::ops::Range<u32>,
}

fn batch_items(
    scene: &Scene,
    items: &mut [DrawItem],
    sort: bool,
    list: &mut Vec<u32>,
    out: &mut Vec<Batch>,
) {
    if sort {
        items.sort_unstable_by_key(|d| (d.pipe, d.material, d.mesh, d.range));
    }
    let mut k = 0;
    while k < items.len() {
        let d = items[k];
        let start = list.len() as u32;
        while k < items.len()
            && (
            items[k].pipe,
            items[k].mesh,
            items[k].range,
            items[k].material,
        ) == (d.pipe, d.mesh, d.range, d.material)
        {
            list.push(items[k].entry);
            k += 1;
        }
        let (first, count, _) = scene.meshes[d.mesh as usize].ranges[d.range as usize];
        out.push(Batch {
            pipe: d.pipe,
            mesh: d.mesh,
            first,
            count,
            material: d.material,
            instances: start..list.len() as u32,
        });
    }
}

fn depth_only_material(kind: u8, material: MaterialId) -> u32 {
    if kind == 0 { 0 } else { material as u32 }
}

fn encode_batches<'a, E: wgpu::util::RenderEncoder<'a>>(
    pass: &mut E,
    scene: &'a Scene,
    batches: &[Batch],
    pipeline: impl Fn(u8) -> &'a wgpu::RenderPipeline,
) {
    encode_batches_filtered(pass, scene, batches, |_| true, pipeline);
}

fn encode_batches_filtered<'a, E: wgpu::util::RenderEncoder<'a>>(
    pass: &mut E,
    scene: &'a Scene,
    batches: &[Batch],
    include: impl Fn(&Batch) -> bool,
    pipeline: impl Fn(u8) -> &'a wgpu::RenderPipeline,
) {
    let (mut pipe, mut mesh, mut material) = (u8::MAX, u32::MAX, u32::MAX);
    for b in batches {
        if !include(b) {
            continue;
        }
        if b.pipe != pipe {
            pass.set_pipeline(pipeline(b.pipe));
            pipe = b.pipe;
        }
        if b.mesh != mesh {
            let m = &scene.meshes[b.mesh as usize];
            pass.set_vertex_buffer(0, Some(m.vertex_buf.slice(..)));
            pass.set_index_buffer(m.index_buf.slice(..), wgpu::IndexFormat::Uint32);
            mesh = b.mesh;
        }
        if b.material != material {
            pass.set_bind_group(
                1,
                Some(&scene.materials[b.material as usize].bind_group),
                &[],
            );
            material = b.material;
        }
        pass.draw_indexed(b.first..b.first + b.count, 0, b.instances.clone());
    }
}

const PIPE_OPAQUE: u8 = 0;
const PIPE_ALPHA_TEST: u8 = 1;
const PIPE_BLEND: u8 = 2;
const PIPE_BLEND_NO_WRITE: u8 = 3;
const PIPE_SURFACE_DEPTH: u8 = 4;
const PIPE_KINDS: u8 = 5;

fn effective_render_phase(instance: &Instance) -> RenderPhase {
    if instance.presurface {
        RenderPhase::PreSurface
    } else {
        instance.render_phase
    }
}

fn world_surface_phase(phase: RenderPhase) -> bool {
    matches!(
        phase,
        RenderPhase::PreSurface
            | RenderPhase::Surface
            | RenderPhase::Spline
            | RenderPhase::OnSurface
    )
}

fn surface_depth_coverage(
    phase: RenderPhase,
    alpha: AlphaMode,
    transmap: bool,
    no_z_check: bool,
) -> bool {
    world_surface_phase(phase) && alpha == AlphaMode::Blend && !transmap && !no_z_check
}
fn depth_prepass_kind(kind: u8, material: &Material, presurface: bool) -> Option<u8> {
    if material.no_z_check {
        return None;
    }
    if kind < PIPE_BLEND {
        Some(kind)
    } else if presurface && !material.no_z_write {
        Some(PIPE_OPAQUE)
    } else if kind == PIPE_BLEND && material.transmap.is_some() && !material.no_z_write {
        Some(2)
    } else {
        None
    }
}

fn instance_depth_bias(instance: &Instance, material: &Material) -> bool {
    instance.surface_bias || material.z_bias > 0 || (material.no_z_check && !instance.surface)
}

fn surface_instance_code(
    blob: bool,
    ground_layer: bool,
    decal: bool,
    surface: bool,
    surface_bias: bool,
) -> f32 {
    if blob {
        2.0
    } else if ground_layer {
        0.75
    } else if decal {
        if surface_bias { 1.25 } else { 0.9 }
    } else if surface {
        if surface_bias { 1.0 } else { 0.9 }
    } else {
        0.0
    }
}

fn horizontal_sort_distance(origin: DVec3, render_origin: DVec3, camera_relative: Vec3) -> f32 {
    let p = (origin - render_origin).as_vec3() - camera_relative;
    glam::Vec2::new(p.x, p.y).length()
}

fn pipe_code(kind: u8, cull: bool, surface: bool) -> u8 {
    debug_assert!(kind < PIPE_KINDS);
    kind * 4 + (cull as u8) * 2 + surface as u8
}

fn main_pipeline(pp: &PassPipelines, pipe: u8) -> &wgpu::RenderPipeline {
    #[cfg(all(feature = "devtools", debug_assertions))]
    if devtools::wireframe() {
        if let Some(w) = pp.wire_pipelines.as_ref() {
            return &w[pipe as usize];
        }
    }
    &pp.pipelines[pipe as usize]
}

fn one_sided_primitive(cull: bool) -> wgpu::PrimitiveState {
    wgpu::PrimitiveState {
        topology: wgpu::PrimitiveTopology::TriangleList,
        cull_mode: cull.then_some(wgpu::Face::Back),
        front_face: wgpu::FrontFace::Cw,
        ..Default::default()
    }
}

fn culls_back_faces(scene: &Scene, inst: &Instance) -> bool {
    static NO_CULL: std::sync::OnceLock<bool> = std::sync::OnceLock::new();
    scene.meshes[inst.mesh].one_sided
        && !*NO_CULL.get_or_init(|| omsi_cfg::env::var_os("OMSI_NO_CULL").is_some())
        && glam::Mat3::from_mat4(inst.transform).determinant() > 0.0
}

struct DevicePoller {
    stop: Arc<std::sync::atomic::AtomicBool>,
    thread: Option<std::thread::JoinHandle<()>>,
}

impl DevicePoller {
    fn start(device: &wgpu::Device) -> Option<Self> {
        if cfg!(target_arch = "wasm32") || omsi_cfg::env::var_os("OMSI_NO_POLL_THREAD").is_some() {
            return None;
        }
        let stop = Arc::new(std::sync::atomic::AtomicBool::new(false));
        let (device, flag) = (device.clone(), stop.clone());
        let thread = std::thread::Builder::new()
            .name("omsi-gpu-poll".into())
            .spawn(move || {
                let pause = std::time::Duration::from_millis(if gl_backend() { 5 } else { 1 });
                while !flag.load(std::sync::atomic::Ordering::Relaxed) {
                    let _ = device.poll(wgpu::PollType::Poll);
                    std::thread::sleep(pause);
                }
            })
            .ok()?;
        Some(Self {
            stop,
            thread: Some(thread),
        })
    }
}

impl Drop for DevicePoller {
    fn drop(&mut self) {
        self.stop.store(true, std::sync::atomic::Ordering::Relaxed);
        if let Some(t) = self.thread.take() {
            let _ = t.join();
        }
    }
}

fn in_scope<'s, R>(pool: Option<&rayon::ThreadPool>, op: impl FnOnce(&rayon::Scope<'s>) -> R) -> R {
    match pool {
        Some(p) => p.in_place_scope(op),
        None => rayon::in_place_scope(op),
    }
}

fn run_parts<T: Send>(
    pool: Option<&rayon::ThreadPool>,
    parts: usize,
    f: impl Fn(usize) -> T + Sync,
) -> Vec<T> {
    let Some(pool) = pool else {
        return std::thread::scope(|s| {
            let f = &f;
            let helpers: Vec<_> = (1..parts.max(1)).map(|k| s.spawn(move || f(k))).collect();
            let mut out = vec![f(0)];
            out.extend(helpers.into_iter().map(|h| h.join().expect("render part")));
            out
        });
    };
    let mut out: Vec<Option<T>> = (0..parts.max(1)).map(|_| None).collect();
    pool.in_place_scope(|s| {
        let f = &f;
        let (first, rest) = out.split_first_mut().expect("one part at least");
        for (k, slot) in rest.iter_mut().enumerate() {
            s.spawn(move |_| *slot = Some(f(k + 1)));
        }
        *first = Some(f(0));
    });
    out.into_iter().map(|o| o.expect("render part")).collect()
}

fn record_bundles(
    device: &wgpu::Device,
    pool: Option<&rayon::ThreadPool>,
    scene: &Scene,
    batches: &[Batch],
    pp: &PassPipelines,
    camera: &wgpu::BindGroup,
    format: wgpu::TextureFormat,
    samples: u32,
) -> Vec<wgpu::RenderBundle> {
    let color_formats = [Some(format), Some(MASK_FORMAT)];
    let record = |chunk: &[Batch]| -> wgpu::RenderBundle {
        let mut bundle =
            device.create_render_bundle_encoder(&wgpu::RenderBundleEncoderDescriptor {
                label: Some("main pass part"),
                color_formats: &color_formats[..if format == HDR_FORMAT { 2 } else { 1 }],
                depth_stencil: Some(wgpu::RenderBundleDepthStencil {
                    format: DEPTH_FORMAT,
                    depth_read_only: false,
                    stencil_read_only: true,
                }),
                sample_count: samples,
                multiview: None,
            });
        bundle.set_bind_group(0, camera, &[]);
        encode_batches(&mut bundle, scene, chunk, |pipe| main_pipeline(pp, pipe));
        bundle.finish(&wgpu::RenderBundleDescriptor {
            label: Some("main pass part"),
        })
    };
    let record = |chunk: &[Batch]| -> Option<wgpu::RenderBundle> {
        CATCHING.set(true);
        let r = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| record(chunk)));
        CATCHING.set(false);
        match r {
            Ok(b) => Some(b),
            Err(_) => {
                static SAID: std::sync::atomic::AtomicBool =
                    std::sync::atomic::AtomicBool::new(false);
                if !SAID.swap(true, std::sync::atomic::Ordering::Relaxed) {
                    log::error!(
                        "a part of the picture could not be recorded (the graphics card is out of memory?); left out"
                    );
                }
                None
            }
        }
    };
    let max_parts = pool
        .map(|p| (p.current_num_threads() + 1).clamp(4, 8))
        .unwrap_or(4);
    let parts = (batches.len() / 250).clamp(1, max_parts);
    if parts == 1 {
        return record(batches).into_iter().collect();
    }
    let chunks: Vec<&[Batch]> = batches.chunks(batches.len().div_ceil(parts)).collect();
    run_parts(pool, chunks.len(), |k| record(chunks[k]))
        .into_iter()
        .flatten()
        .collect()
}

fn scene_scale_for(requested: f32, width: u32, height: u32) -> f32 {
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

fn gpu_error_text(e: &wgpu::Error) -> String {
    match e {
        wgpu::Error::Validation { description, .. } | wgpu::Error::Internal { description, .. } => {
            description.trim().replace('\n', " ")
        }
        other => other.to_string(),
    }
}

fn point_in_vehicle_box(p: DVec3, (origin, heading, bb): &(DVec3, f64, [f32; 6])) -> bool {
    let d = (p - *origin).as_vec3();
    let (sh, ch) = (*heading as f32).to_radians().sin_cos();
    let x = d.x * ch - d.y * sh - bb[3];
    let y = d.x * sh + d.y * ch - bb[4];
    let z = d.z - bb[5];
    x.abs() < bb[0] * 0.5 && y.abs() < bb[1] * 0.5 && z.abs() < bb[2] * 0.5
}

pub struct SurfaceState<'w> {
    pub surface: std::mem::ManuallyDrop<wgpu::Surface<'w>>,
    pub config: wgpu::SurfaceConfiguration,
    lost: Arc<std::sync::Mutex<Option<String>>>,
}

impl Drop for SurfaceState<'_> {
    fn drop(&mut self) {
        if !std::thread::panicking()
            && self
            .lost
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .is_none()
        {
            // SAFETY: dropped only here, once
            unsafe { std::mem::ManuallyDrop::drop(&mut self.surface) };
        }
    }
}

impl<'w> SurfaceState<'w> {
    pub fn new(
        instance: &wgpu::Instance,
        window: Arc<winit_window::Window>,
        renderer: &Renderer,
        width: u32,
        height: u32,
    ) -> Result<Self> {
        Self::new_with(instance, window, renderer, width, height, true)
    }

    pub fn new_with(
        instance: &wgpu::Instance,
        window: Arc<winit_window::Window>,
        renderer: &Renderer,
        width: u32,
        height: u32,
        vsync: bool,
    ) -> Result<Self> {
        let surface = instance.create_surface(window).context("create_surface")?;
        let mut config = wgpu::SurfaceConfiguration {
            usage: wgpu::TextureUsages::RENDER_ATTACHMENT,
            format: renderer.format(),
            color_space: wgpu::SurfaceColorSpace::Auto,
            width: width.max(1),
            height: height.max(1),
            present_mode: if vsync {
                wgpu::PresentMode::AutoVsync
            } else {
                wgpu::PresentMode::AutoNoVsync
            },
            desired_maximum_frame_latency: 2,
            alpha_mode: wgpu::CompositeAlphaMode::Auto,
            view_formats: vec![],
        };
        config.width = width.max(1);
        config.height = height.max(1);
        surface.configure(&renderer.device, &config);
        Ok(SurfaceState {
            surface: std::mem::ManuallyDrop::new(surface),
            config,
            lost: renderer.device_lost.clone(),
        })
    }

    pub fn resize(&mut self, renderer: &Renderer, width: u32, height: u32) {
        self.config.width = width.max(1);
        self.config.height = height.max(1);
        if renderer.device_lost().is_some() {
            return;
        }
        self.surface.configure(&renderer.device, &self.config);
    }

    pub fn set_vsync(&mut self, renderer: &Renderer, enabled: bool) {
        let mode = if enabled {
            wgpu::PresentMode::AutoVsync
        } else {
            wgpu::PresentMode::AutoNoVsync
        };
        if self.config.present_mode == mode || renderer.device_lost().is_some() {
            return;
        }
        self.config.present_mode = mode;
        self.config.desired_maximum_frame_latency = 2;
        self.surface.configure(&renderer.device, &self.config);
    }
}

pub mod winit_window {
    pub use winit::window::Window;
}

impl Renderer {
    fn freed(&self, scene: &mut Scene) -> &Freed {
        if self.freed.get().is_none() {
            let address = self.address_next.replace(TexAddressing::Wrap);
            let plain = self.add_material(scene, None, AlphaMode::Opaque, [1.0; 4], false);
            self.address_next.set(address);
            let m = scene.materials.swap_remove(plain);
            let (bind_group, buf) = (m.bind_group, m.buf);
            let vertex_buf = self.device.create_buffer(&wgpu::BufferDescriptor {
                label: Some("freed mesh"),
                size: std::mem::size_of::<Vertex>() as u64,
                usage: wgpu::BufferUsages::VERTEX | wgpu::BufferUsages::COPY_DST,
                mapped_at_creation: false,
            });
            let index_buf = self.device.create_buffer(&wgpu::BufferDescriptor {
                label: Some("freed mesh"),
                size: 4,
                usage: wgpu::BufferUsages::INDEX,
                mapped_at_creation: false,
            });
            let _ = self.freed.set(Freed {
                vertex_buf,
                index_buf,
                bind_group,
                buf,
            });
        }
        self.freed.get().unwrap()
    }

    pub fn free_mesh(&self, scene: &mut Scene, id: MeshId) {
        if id >= scene.meshes.len() {
            return;
        }
        let (vertex_buf, index_buf) = {
            let f = self.freed(scene);
            (f.vertex_buf.clone(), f.index_buf.clone())
        };
        let m = &mut scene.meshes[id];
        m.vertex_buf = vertex_buf;
        m.index_buf = index_buf;
        m.ranges.clear();
        m.bounds_center = Vec3::ZERO;
        m.bounds_radius = 0.0;
        Self::mesh_bounds_changed(scene, id);
    }

    pub fn free_texture(&self, scene: &mut Scene, id: TextureId) {
        scene.snow_textures.remove(&id);
        if let Some(m) = scene.pbr_maps.remove(&id) {
            for t in [m.normal, m.orm].into_iter().flatten() {
                self.free_texture(scene, t);
            }
        }
        if let Some(t) = scene.textures.get_mut(id) {
            *t = GpuTexture {
                texture: self.white_texture.texture.clone(),
                view: self.white_texture.view.clone(),
                size: (1, 1),
                bytes: 0,
                generation: next_gen(),
            };
        }
    }

    pub fn free_material(&self, scene: &mut Scene, id: MaterialId) {
        if id >= scene.materials.len() {
            return;
        }
        let (bind_group, buf) = {
            let f = self.freed(scene);
            (f.bind_group.clone(), f.buf.clone())
        };
        scene.materials[id] = Material {
            texture: None,
            alpha: AlphaMode::Opaque,
            color: [1.0; 4],
            unlit: false,
            no_z_write: false,
            no_z_check: false,
            z_bias: 0,
            nightmap: None,
            lightmap: None,
            envmap: None,
            env_mask: None,
            bump: None,
            emissive: [0.0; 3],
            transmap: None,
            address: TexAddressing::Wrap,
            uniform: <MaterialUniform as bytemuck::Zeroable>::zeroed(),
            buf,
            bind_group,
        };
    }

    pub fn remove_instance(&self, scene: &mut Scene, instance: usize) {
        if instance >= scene.instances.len() {
            return;
        }
        self.set_params(scene, instance, &[], false, &[]);
        scene.instances[instance].lod = (0.0, f32::MAX);
        scene.instances[instance].interior_lamps = 0;
    }

    pub fn truncate(
        &self,
        scene: &mut Scene,
        meshes: usize,
        textures: usize,
        materials: usize,
        instances: usize,
    ) {
        fn cut<T>(v: &mut Vec<T>, n: usize) {
            if n < v.len() {
                v.truncate(n);
                if v.capacity() > v.len() * 2 + 64 {
                    v.shrink_to_fit();
                }
            }
        }
        cut(&mut scene.meshes, meshes);
        cut(&mut scene.textures, textures);
        cut(&mut scene.materials, materials);
        let (nm, nt) = (scene.meshes.len(), scene.materials.len());
        for inst in scene.instances.iter_mut() {
            if inst.mesh >= nm {
                debug_assert!(!inst.visible, "a drawn instance lost its mesh");
                inst.mesh = 0;
                inst.visible = false;
            }
            for m in inst.materials.iter_mut() {
                if *m >= nt {
                    *m = 0;
                }
            }
        }
        if instances < scene.instances.len() {
            cut(&mut scene.instances, instances);
            scene.changed.retain(|i| *i < instances);
            scene.changed_mark.truncate(instances);
            scene.uploaded_instances = scene.uploaded_instances.min(instances);
            scene.dirty = true;
        }
    }

    pub fn recycle_mesh(&self, scene: &mut Scene, new: MeshId, into: MeshId) -> MeshId {
        if new + 1 != scene.meshes.len() || into >= new {
            return new;
        }
        let m = scene.meshes.pop().unwrap();
        scene.meshes[into] = m;
        Self::mesh_bounds_changed(scene, into);
        into
    }

    pub fn recycle_texture(&self, scene: &mut Scene, new: TextureId, into: TextureId) -> TextureId {
        if new + 1 != scene.textures.len() || into >= new {
            return new;
        }
        let t = scene.textures.pop().unwrap();
        scene.textures[into] = t;
        into
    }

    pub fn recycle_material(
        &self,
        scene: &mut Scene,
        new: MaterialId,
        into: MaterialId,
    ) -> MaterialId {
        if new + 1 != scene.materials.len() || into >= new {
            return new;
        }
        let m = scene.materials.pop().unwrap();
        scene.materials[into] = m;
        into
    }

    pub fn recycle_instance(&self, scene: &mut Scene, new: usize, into: usize) -> usize {
        if new + 1 != scene.instances.len() || into >= new || new < scene.uploaded_instances {
            return new;
        }
        if scene.instances[into].slot_alpha.len() != scene.instances[new].slot_alpha.len() {
            return new;
        }
        let mut inst = scene.instances.pop().unwrap();
        inst.base = scene.instances[into].base;
        scene.instances[into] = inst;
        Self::mark_changed(scene, into);
        into
    }

    pub fn instance_slots(&self, scene: &Scene, instance: usize) -> usize {
        scene
            .instances
            .get(instance)
            .map(|i| i.slot_alpha.len())
            .unwrap_or(0)
    }
}

fn snap_rect(r: [f32; 4]) -> [f32; 4] {
    let snap = |v: f32| (v + 0.5).floor();
    let (x0, y0) = (snap(r[0]), snap(r[1]));
    let x1 = if r[2] > r[0] {
        snap(r[2]).max(x0 + 1.0)
    } else {
        snap(r[2])
    };
    let y1 = if r[3] > r[1] {
        snap(r[3]).max(y0 + 1.0)
    } else {
        snap(r[3])
    };
    [x0, y0, x1, y1]
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn overlays_land_on_whole_pixels() {
        assert_eq!(
            snap_rect([25.25, 40.5, 145.25, 66.5]),
            [25.0, 41.0, 145.0, 67.0]
        );
        assert_eq!(
            snap_rect([10.0, 20.0, 30.0, 40.0]),
            [10.0, 20.0, 30.0, 40.0]
        );
        assert_eq!(snap_rect([-0.5, -2.5, 19.5, 7.5]), [0.0, -2.0, 20.0, 8.0]);
        let line = snap_rect([16.0, 100.3, 300.0, 100.9]);
        assert_eq!(line[3] - line[1], 1.0);
        assert_eq!(snap_rect([5.2, 5.2, 5.2, 5.2]), [5.0, 5.0, 5.0, 5.0]);
    }

    #[test]
    #[ignore = "requires a graphics adapter; renders terrain and foliage lighting"]
    fn enhanced_masked_and_uncut_ground_share_lighting() {
        let instance = wgpu::Instance::new(wgpu::InstanceDescriptor::new_without_display_handle());
        let mut renderer = pollster::block_on(Renderer::new_with(
            &instance,
            None,
            Some(wgpu::TextureFormat::Rgba8UnormSrgb),
            RenderOptions {
                msaa: 1,
                ssao: false,
                shadow_size: 1024,
                fxaa: false,
                render_scale: 1.0,
                ..Default::default()
            },
        ))
            .expect("test renderer");
        let mut scene = renderer.new_scene();
        let mut texture = |rgba: [u8; 4]| {
            renderer.add_texture(
                &mut scene,
                &omsi_texture::Image {
                    width: 1,
                    height: 1,
                    rgba: rgba.to_vec(),
                    has_alpha: true,
                },
                false,
            )
        };
        let grey = texture([100, 100, 100, 255]);
        let opaque = texture([255; 4]);
        let transparent = texture([100, 100, 100, 0]);
        let masked = renderer.add_terrain_material(
            &mut scene,
            Some(grey),
            Some(opaque),
            None,
            1.0,
            None,
            0.0,
        );
        let uncut =
            renderer.add_terrain_material(&mut scene, Some(grey), None, None, 1.0, None, 0.0);
        let cut = renderer.add_terrain_material(
            &mut scene,
            Some(grey),
            Some(transparent),
            None,
            1.0,
            None,
            0.0,
        );
        let foliage =
            renderer.add_material(&mut scene, Some(grey), AlphaMode::Test, [1.0; 4], false);
        let cut_foliage = renderer.add_material(
            &mut scene,
            Some(transparent),
            AlphaMode::Test,
            [1.0; 4],
            false,
        );
        let backdrop = renderer.add_material(
            &mut scene,
            None,
            AlphaMode::Opaque,
            [1.0, 0.0, 0.0, 1.0],
            true,
        );
        let mut quad = |left: f32, right: f32, z: f32, material| {
            let mesh = renderer.add_mesh(
                &mut scene,
                &MeshData {
                    positions: vec![
                        Vec3::new(left, -5.0, z),
                        Vec3::new(right, -5.0, z),
                        Vec3::new(right, 5.0, z),
                        Vec3::new(left, 5.0, z),
                    ],
                    normals: vec![Vec3::Z; 4],
                    uvs: vec![glam::Vec2::splat(0.5); 4],
                    indices: vec![0, 1, 2, 0, 2, 3],
                    ranges: vec![(0, 6, 0)],
                    one_sided: false,
                },
            );
            renderer.add_instance(
                &mut scene,
                mesh,
                DVec3::ZERO,
                Mat4::IDENTITY,
                vec![material],
            )
        };
        quad(-6.0, 6.0, -1.0, backdrop);
        let ground = quad(-5.0, -0.5, 0.0, masked);
        let mapped = quad(0.5, 5.0, 0.0, uncut);
        scene.instances[ground].render_phase = RenderPhase::Terrain;
        scene.instances[mapped].render_phase = RenderPhase::Spline;
        scene.instances[mapped].surface = true;
        let camera = Camera {
            position: DVec3::new(0.0, -0.105, 6.0),
            yaw: 0.0,
            pitch: -89.0,
            roll: 0.0,
            fov_deg: 90.0,
            near: 0.1,
            far: 100.0,
        };
        let day = Lighting {
            enhanced: true,
            sun_dir: Vec3::Z,
            sun_intensity: 1.0,
            shadows: false,
            detail: false,
            fog_density: 0.0,
            ..Default::default()
        };
        let night = Lighting {
            sun_dir: -Vec3::Z,
            sun_intensity: 0.0,
            night: 1.0,
            ..day.clone()
        };
        let pixel = |rgba: &[u8], x: usize| -> [u8; 3] {
            rgba[(32 * 64 + x) * 4..(32 * 64 + x) * 4 + 3]
                .try_into()
                .unwrap()
        };
        for (name, lighting) in [("sun", &day), ("lamp", &night)] {
            scene.lights = if name == "lamp" {
                vec![PointLight {
                    position: DVec3::new(0.0, 0.0, 4.0),
                    radius: 20.0,
                    core: 10.0,
                    intensity: 1.0,
                    ..Default::default()
                }]
            } else {
                Vec::new()
            };
            let rgba = renderer
                .render_to_image(&mut scene, 64, 64, &camera, lighting)
                .unwrap();
            let (a, b) = (pixel(&rgba, 16), pixel(&rgba, 47));
            assert!(
                a.iter().all(|v| *v > 10 && *v < 245),
                "lit, unclipped {name}: {a:?}"
            );
            assert!(
                a.iter().zip(b).all(|(a, b)| a.abs_diff(b) <= 2),
                "masked terrain and uncut mapped ground differ under {name}: {a:?} / {b:?}"
            );
        }
        renderer.set_material(&mut scene, ground, 0, cut);
        let rgba = renderer
            .render_to_image(&mut scene, 64, 64, &camera, &night)
            .unwrap();
        let a = pixel(&rgba, 16);
        assert!(
            a[0] > a[1] + 30 && a[0] > a[2] + 30,
            "road cut must reveal red: {a:?}"
        );

        renderer.set_material(&mut scene, ground, 0, masked);
        renderer.set_material(&mut scene, mapped, 0, foliage);
        scene.lights[0].position.z = -4.0;
        let rgba = renderer
            .render_to_image(&mut scene, 64, 64, &camera, &night)
            .unwrap();
        let (a, b) = (pixel(&rgba, 16), pixel(&rgba, 47));
        assert!(
            b[1] > a[1] + 8,
            "foliage retains backlighting: ground {a:?}, foliage {b:?}"
        );
        renderer.set_material(&mut scene, mapped, 0, cut_foliage);
        let rgba = renderer
            .render_to_image(&mut scene, 64, 64, &camera, &night)
            .unwrap();
        let b = pixel(&rgba, 47);
        assert!(
            b[0] > b[1] + 30 && b[0] > b[2] + 30,
            "foliage cutout must reveal red: {b:?}"
        );
    }

    #[test]
    #[ignore = "requires a graphics adapter; run with --ignored on a GPU host"]
    fn cached_bounds_follow_transforms_skinning_and_recycled_resources() {
        let adapter = wgpu::Instance::new(wgpu::InstanceDescriptor::new_without_display_handle());
        let renderer = pollster::block_on(Renderer::new_with(
            &adapter,
            None,
            Some(wgpu::TextureFormat::Rgba8UnormSrgb),
            RenderOptions {
                msaa: 1,
                shadow_size: 1024,
                ..Default::default()
            },
        ))
            .expect("test renderer");
        let mut scene = renderer.new_scene();
        scene.cache_bounds = true;
        let material = renderer.add_material(&mut scene, None, AlphaMode::Opaque, [1.0; 4], true);
        let data = MeshData {
            positions: vec![Vec3::ZERO, Vec3::X, Vec3::Y],
            normals: vec![Vec3::Z; 3],
            uvs: vec![glam::Vec2::ZERO; 3],
            indices: vec![0, 1, 2],
            ranges: vec![(0, 3, 0)],
            ..Default::default()
        };
        let mesh = renderer.add_mesh(&mut scene, &data);
        let origin = DVec3::new(1_000_000.000_001, 2_000_000.0, 12.0);
        let i = renderer.add_instance(&mut scene, mesh, origin, Mat4::IDENTITY, vec![material]);
        let check = |scene: &Scene, i: usize| {
            let inst = &scene.instances[i];
            let expected = InstanceBounds::new(&scene.meshes[inst.mesh], inst.transform);
            assert_eq!(
                Renderer::bounding_sphere(scene, inst),
                (
                    expected.centre + (inst.origin - scene.render_origin).as_vec3(),
                    expected.radius
                )
            );
            assert_eq!(Renderer::instance_scale(scene, inst), expected.scale);
        };
        renderer.prepare(&mut scene);
        check(&scene, i);
        let transform = Mat4::from_scale_rotation_translation(
            Vec3::new(-2.0, 3.0, 4.0),
            glam::Quat::from_rotation_z(0.7),
            Vec3::new(10.0, 20.0, 30.0),
        );
        renderer.set_transform(&mut scene, i, origin, transform);
        renderer.prepare(&mut scene);
        check(&scene, i);
        let posed: Vec<_> = data.positions.iter().map(|p| *p * 5.0 + Vec3::Z).collect();
        renderer.update_mesh(&mut scene, mesh, &posed, &data.normals, &data.uvs);
        renderer.prepare(&mut scene);
        check(&scene, i);
        renderer.set_render_origin(&mut scene, origin - DVec3::new(0.25, 0.5, 0.75));
        renderer.prepare(&mut scene);
        check(&scene, i);
        renderer.free_mesh(&mut scene, mesh);
        renderer.prepare(&mut scene);
        check(&scene, i);
        let new = renderer.add_mesh(&mut scene, &data);
        assert_eq!(renderer.recycle_mesh(&mut scene, new, mesh), mesh);
        renderer.prepare(&mut scene);
        check(&scene, i);
        let replacement =
            renderer.add_instance(&mut scene, mesh, origin, Mat4::IDENTITY, vec![material]);
        assert_eq!(renderer.recycle_instance(&mut scene, replacement, i), i);
        renderer.prepare(&mut scene);
        check(&scene, i);
        let other = renderer.add_mesh(&mut scene, &data);
        renderer.set_instance_mesh(&mut scene, i, other);
        renderer.prepare(&mut scene);
        check(&scene, i);
    }

    #[test]
    #[ignore = "requires a graphics adapter; run with --ignored on a GPU host"]
    fn presurface_reveals_excavation_before_terrain_is_drawn() {
        fn quad(renderer: &Renderer, scene: &mut Scene, y: f32, half: f32) -> MeshId {
            renderer.add_mesh(
                scene,
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
            )
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
        for (msaa, ssao, enhanced) in [
            (1, false, false),
            (1, true, false),
            (1, true, true),
            (4, true, true),
        ] {
            let instance =
                wgpu::Instance::new(wgpu::InstanceDescriptor::new_without_display_handle());
            let mut renderer = pollster::block_on(Renderer::new_with(
                &instance,
                None,
                Some(wgpu::TextureFormat::Rgba8UnormSrgb),
                RenderOptions {
                    msaa,
                    ssao,
                    shadow_size: 1024,
                    fxaa: false,
                    render_scale: 1.0,
                    ..Default::default()
                },
            ))
                .expect("test renderer");
            let mut scene = renderer.new_scene();
            let green = renderer.add_material(
                &mut scene,
                None,
                AlphaMode::Opaque,
                [0.0, 1.0, 0.0, 1.0],
                true,
            );
            let blue = renderer.add_material(
                &mut scene,
                None,
                AlphaMode::Opaque,
                [0.0, 0.0, 1.0, 1.0],
                true,
            );
            let red = renderer.add_material(
                &mut scene,
                None,
                AlphaMode::Opaque,
                [1.0, 0.0, 0.0, 1.0],
                true,
            );
            let texture = renderer.add_texture(
                &mut scene,
                &omsi_texture::Image {
                    width: 1,
                    height: 1,
                    rgba: vec![255, 255, 255, 0],
                    has_alpha: true,
                },
                false,
            );
            let transparent =
                renderer.add_material(&mut scene, Some(texture), AlphaMode::Blend, [1.0; 4], true);
            let cutout =
                renderer.add_material(&mut scene, Some(texture), AlphaMode::Test, [1.0; 4], true);
            let terrain = quad(&renderer, &mut scene, 6.0, 10.0);
            renderer.add_instance(
                &mut scene,
                terrain,
                DVec3::ZERO,
                Mat4::IDENTITY,
                vec![green],
            );
            let floor = quad(&renderer, &mut scene, 8.0, 10.0);
            let floor = renderer.add_surface_instance(
                &mut scene,
                floor,
                DVec3::ZERO,
                Mat4::IDENTITY,
                vec![blue],
            );
            scene.instances[floor].presurface = true;
            let cover_mesh = quad(&renderer, &mut scene, 4.0, 1.5);
            let cover = renderer.add_surface_instance(
                &mut scene,
                cover_mesh,
                DVec3::ZERO,
                Mat4::IDENTITY,
                vec![transparent],
            );
            scene.instances[cover].presurface = true;
            let foreground_mesh = quad(&renderer, &mut scene, 2.0, 0.25);
            let foreground = renderer.add_instance(
                &mut scene,
                foreground_mesh,
                DVec3::ZERO,
                Mat4::IDENTITY,
                vec![red],
            );
            scene.instances[foreground].visible = false;
            let lighting = Lighting {
                enhanced,
                shadows: false,
                fog_density: 0.0,
                ..Default::default()
            };
            let pixel = |rgba: &[u8], x: usize| -> [u8; 3] {
                rgba[(32 * 64 + x) * 4..(32 * 64 + x) * 4 + 3]
                    .try_into()
                    .unwrap()
            };
            let rgba = renderer
                .render_to_image(&mut scene, 64, 64, &camera, &lighting)
                .unwrap();
            let centre = pixel(&rgba, 32);
            assert!(
                centre[2] > centre[1] + 40,
                "floor must show through cover: {centre:?}; {msaa}/{ssao}/{enhanced}"
            );
            let outside = pixel(&rgba, 4);
            assert!(
                outside[1] > outside[2] + 40,
                "terrain outside cover: {outside:?}"
            );
            scene.instances[foreground].visible = true;
            Renderer::mark_changed(&mut scene, foreground);
            let rgba = renderer
                .render_to_image(&mut scene, 64, 64, &camera, &lighting)
                .unwrap();
            let centre = pixel(&rgba, 32);
            assert!(
                centre[0] > centre[2] + 40,
                "foreground stays visible: {centre:?}"
            );
            scene.instances[foreground].visible = false;
            Renderer::mark_changed(&mut scene, foreground);
            for (presurface, alpha, no_z_write) in [
                (false, AlphaMode::Blend, false),
                (true, AlphaMode::Blend, true),
                (true, AlphaMode::Test, false),
            ] {
                scene.instances[cover].presurface = presurface;
                scene.materials[transparent].no_z_write = no_z_write;
                renderer.set_material(
                    &mut scene,
                    cover,
                    0,
                    if alpha == AlphaMode::Test {
                        cutout
                    } else {
                        transparent
                    },
                );
                let rgba = renderer
                    .render_to_image(&mut scene, 64, 64, &camera, &lighting)
                    .unwrap();
                let centre = pixel(&rgba, 32);
                assert!(
                    centre[1] > centre[2] + 40,
                    "terrain should show: {centre:?}; {presurface}/{alpha:?}/{no_z_write}"
                );
            }
        }
    }

    #[test]
    fn noop_backend_initializes_renderer() {
        let mut descriptor = wgpu::InstanceDescriptor::new_without_display_handle();
        descriptor.backends = wgpu::Backends::NOOP;
        descriptor.backend_options.noop = wgpu::NoopBackendOptions::enabled();
        let instance = wgpu::Instance::new(descriptor);
        let res = pollster::block_on(Renderer::new_with(
            &instance,
            None,
            Some(wgpu::TextureFormat::Rgba8UnormSrgb),
            RenderOptions {
                msaa: 2,
                shadow_size: 1024,
                ..Default::default()
            },
        ));
        assert!(
            res.is_ok(),
            "renderer should initialize on noop backend: {:?}",
            res.err()
        );
    }

    #[test]
    fn declared_transmap_ignores_slot_alpha() {
        let mut descriptor = wgpu::InstanceDescriptor::new_without_display_handle();
        descriptor.backends = wgpu::Backends::NOOP;
        descriptor.backend_options.noop = wgpu::NoopBackendOptions::enabled();
        let instance = wgpu::Instance::new(descriptor);
        let renderer = pollster::block_on(Renderer::new_with(
            &instance,
            None,
            Some(wgpu::TextureFormat::Rgba8UnormSrgb),
            RenderOptions {
                msaa: 1,
                shadow_size: 1024,
                ..Default::default()
            },
        ))
        .expect("noop renderer");
        let mut scene = renderer.new_scene();
        let blended =
            |scene: &mut Scene, transmap: Option<(TextureId, bool)>, extra: MaterialExtra| {
                renderer.add_material_extra(
                    scene,
                    None,
                    AlphaMode::Blend,
                    [1.0; 4],
                    true,
                    transmap,
                    None,
                    None,
                    None,
                    [0.0; 3],
                    extra,
                )
            };
        // declared, its file missing: no transmap texture bound
        let declared = blended(
            &mut scene,
            None,
            MaterialExtra {
                transmap_declared: true,
                ..Default::default()
            },
        );
        let map = renderer.add_blank_texture(&mut scene, 1, 1);
        let bound = blended(&mut scene, Some((map, true)), MaterialExtra::default());
        // another bit of the same flags, no transmap
        let metal = blended(
            &mut scene,
            None,
            MaterialExtra {
                metal_ok: true,
                ..Default::default()
            },
        );
        let plain = blended(&mut scene, None, MaterialExtra::default());
        assert_eq!(scene.materials[metal].uniform.params2[3], 4.0);
        let flags: Vec<bool> = [declared, bound, metal, plain]
            .iter()
            .map(|&m| scene.materials[m].transmap_declared())
            .collect();
        assert_eq!(flags, [true, true, false, false]);
        let corner = [Vec3::ZERO, Vec3::X, Vec3::Y];
        let data = MeshData {
            positions: corner.repeat(4),
            normals: vec![Vec3::Z; 12],
            uvs: vec![glam::Vec2::ZERO; 12],
            indices: (0..12).collect(),
            ranges: (0..4).map(|s| (s * 3, 3, s)).collect(),
            ..Default::default()
        };
        let mesh = renderer.add_mesh(&mut scene, &data);
        let i = renderer.add_instance(
            &mut scene,
            mesh,
            DVec3::ZERO,
            Mat4::IDENTITY,
            vec![declared, bound, metal, plain],
        );
        renderer.set_params(&mut scene, i, &[0.0, 0.0, 0.35, 0.35], true, &[]);
        assert_eq!(scene.instances[i].slot_alpha, vec![1.0, 1.0, 0.35, 0.35]);
    }

    #[test]
    fn omsi_render_phases_are_monotonic_and_complete() {
        assert_eq!(
            RenderPhase::DRAW_ORDER.map(|phase| phase as usize),
            [0, 1, 2, 3, 4, 5, 6, 7, 8]
        );
        assert_eq!(RenderPhase::default(), RenderPhase::Normal);
    }

    #[test]
    fn metric_lifted_surfaces_keep_shading_class_without_view_space_pull() {
        assert_eq!(surface_instance_code(false, false, false, true, false), 0.9);
        assert_eq!(surface_instance_code(false, false, false, true, true), 1.0);
        assert_eq!(surface_instance_code(false, false, true, false, false), 0.9);
        assert_eq!(surface_instance_code(false, true, false, true, false), 0.75);
    }

    #[test]
    fn spline_blend_sort_ignores_height_and_camera_pitch() {
        let origin = DVec3::new(120.0, 45.0, 0.0);
        let render_origin = DVec3::new(100.0, 40.0, 0.0);
        let level_camera = Vec3::new(3.0, 1.0, 4.0);
        let high_camera = Vec3::new(3.0, 1.0, 80.0);
        assert_eq!(
            horizontal_sort_distance(origin, render_origin, level_camera),
            horizontal_sort_distance(origin + DVec3::Z * 60.0, render_origin, high_camera)
        );
    }

    #[test]
    fn render_scale_auto_keeps_ordinary_windows_sharp() {
        assert_eq!(scene_scale_for(0.0, 1600, 900), 1.0);
        assert_eq!(scene_scale_for(0.0, 2560, 1080), 1.0);
        let s = scene_scale_for(0.0, 3200, 1800);
        if cfg!(target_os = "macos") || cfg!(target_os = "android") {
            assert!((s - 0.697).abs() < 0.01, "{s}");
            assert!((3200.0 * s * 1800.0 * s - AUTO_SCALE_PIXELS).abs() < 1.0);
        } else {
            assert_eq!(s, 1.0);
        }
        assert_eq!(scene_scale_for(0.0, 16384, 16384), 0.5);
        let s = scene_scale_for(0.75, 3200, 1800);
        if cfg!(target_os = "macos") || cfg!(target_os = "android") {
            assert!((s - 0.697).abs() < 0.01, "{s}");
            assert!((3200.0 * s * 1800.0 * s - AUTO_SCALE_PIXELS).abs() < 1.0);
        } else {
            assert_eq!(s, 0.75);
        }
        assert_eq!(scene_scale_for(0.3, 1600, 900), 0.5);
        assert_eq!(scene_scale_for(1.4, 1600, 900), 1.0);
    }

    #[test]
    fn surface_depth_coverage_excludes_glass_and_terrain_masks() {
        assert!(surface_depth_coverage(
            RenderPhase::Spline,
            AlphaMode::Blend,
            false,
            false
        ));
        for phase in RenderPhase::DRAW_ORDER {
            if !world_surface_phase(phase) {
                assert!(!surface_depth_coverage(
                    phase,
                    AlphaMode::Blend,
                    false,
                    false
                ));
            } else {
                assert!(surface_depth_coverage(
                    phase,
                    AlphaMode::Blend,
                    false,
                    false
                ));
            }
        }
        assert!(!surface_depth_coverage(
            RenderPhase::Spline,
            AlphaMode::Blend,
            true,
            false
        ));
        assert!(!surface_depth_coverage(
            RenderPhase::Spline,
            AlphaMode::Blend,
            false,
            true
        ));
        assert!(!surface_depth_coverage(
            RenderPhase::Spline,
            AlphaMode::Opaque,
            false,
            false
        ));
        assert!(!surface_depth_coverage(
            RenderPhase::Spline,
            AlphaMode::Test,
            false,
            false
        ));
        let order = RenderPhase::DRAW_ORDER;
        assert!(
            order
                .iter()
                .position(|p| *p == RenderPhase::OnSurface)
                .unwrap()
                < order
                .iter()
                .position(|p| *p == RenderPhase::BeforeNormal)
                .unwrap()
        );
    }

    #[test]
    fn pipeline_codes_cover_the_table() {
        let mut seen = std::collections::HashSet::new();
        for kind in 0..PIPE_KINDS {
            for cull in [false, true] {
                for surface in [false, true] {
                    let code = pipe_code(kind, cull, surface);
                    assert!((code as usize) < PIPE_KINDS as usize * 4);
                    assert!(seen.insert(code));
                    assert_eq!(
                        code < pipe_code(PIPE_BLEND, false, false),
                        kind < PIPE_BLEND
                    );
                }
            }
        }
        assert_eq!(seen.len(), PIPE_KINDS as usize * 4);
        assert_eq!(pipe_code(PIPE_OPAQUE, false, false), 0);
        assert!(
            pipe_code(PIPE_ALPHA_TEST, true, true) < pipe_code(PIPE_BLEND_NO_WRITE, false, false)
        );
    }

    #[test]
    fn nearest_by_origin_picks_the_closest_mesh_of_each_object() {
        let bus = DVec3::new(0.0, 0.0, 0.0);
        let car = DVec3::new(1.0, 0.0, 0.0);
        let by_origin = nearest_by_origin([(bus, 14.0), (bus, 3.0), (car, 8.0)]);
        let bus_dist = by_origin[&origin_key(bus)];
        let car_dist = by_origin[&origin_key(car)];
        assert_eq!(
            bus_dist, 3.0,
            "the object's distance is its nearest mesh, not the first or an average"
        );
        assert_eq!(car_dist, 8.0);
        assert!(car_dist > bus_dist);
        assert_eq!(
            by_origin.len(),
            2,
            "one entry per distinct origin, not per mesh"
        );
    }

    #[test]
    fn origin_lookup_keeps_float_equality_and_large_coordinates() {
        let a = DVec3::new(-0.0, 1_000_000.000_001, 2.0);
        let b = DVec3::new(0.0, a.y, 2.0);
        let c = DVec3::new(0.0, a.y + 0.000_001, 2.0);
        let distances = nearest_by_origin([(a, 8.0), (b, 3.0), (c, 5.0), (DVec3::NAN, 1.0)]);
        assert_eq!(distances.len(), 2);
        assert_eq!(distances[&origin_key(a)], 3.0);
        assert_eq!(distances[&origin_key(c)], 5.0);
    }

    #[test]
    fn shaders_validate_and_match_the_uniforms() {
        use wgpu::naga;
        let modules = [
            ("scene", scene_shader_source(false)),
            ("sky", sky_shader_source()),
            ("corona", corona_shader_source()),
            ("post", include_str!("post.wgsl").to_string()),
            ("ssao", include_str!("ssao.wgsl").to_string()),
            ("puddles", puddles::shader_source()),
            ("upscale", include_str!("upscale.wgsl").to_string()),
            ("mip", include_str!("mip.wgsl").to_string()),
            ("xr_ui", include_str!("xr_ui.wgsl").to_string()),
        ];
        let sizes: &[(&str, usize)] = &[
            ("Enhanced", std::mem::size_of::<EnhancedUniform>()),
            ("PostParams", std::mem::size_of::<PostUniform>()),
            ("PuddleParams", std::mem::size_of::<puddles::Uniform>()),
            (
                "VehicleReflection",
                std::mem::size_of::<puddles::VehicleUniform>(),
            ),
            ("PointLight", std::mem::size_of::<GpuPointLight>()),
            ("Camera", std::mem::size_of::<CameraUniform>()),
            ("MaterialParams", std::mem::size_of::<MaterialUniform>()),
        ];
        let mut checked = std::collections::HashSet::new();
        for (name, src) in &modules {
            let module = naga::front::wgsl::parse_str(src)
                .unwrap_or_else(|e| panic!("{name}: {}", e.emit_to_string(src)));
            let info = naga::valid::Validator::new(
                naga::valid::ValidationFlags::all(),
                naga::valid::Capabilities::all(),
            )
                .validate(&module)
                .unwrap_or_else(|e| panic!("{name}: {e:?}"));
            let (module, info) = naga::back::pipeline_constants::process_overrides(
                &module,
                &info,
                None,
                &Default::default(),
            )
                .unwrap_or_else(|e| panic!("{name}: overrides: {e:?}"));
            let (module, info) = (module.into_owned(), info.into_owned());
            #[cfg(target_os = "macos")]
            let options = naga::back::msl::Options {
                lang_version: (2, 4),
                ..Default::default()
            };
            #[cfg(target_os = "macos")]
            naga::back::msl::write_string(
                &module,
                &info,
                &options,
                &naga::back::msl::PipelineOptions::default(),
            )
                .unwrap_or_else(|e| panic!("{name}: Metal: {e:?}"));
            for entry in &module.entry_points {
                let pipeline = naga::back::spv::PipelineOptions {
                    shader_stage: entry.stage,
                    entry_point: entry.name.clone(),
                };
                naga::back::spv::write_vec(&module, &info, &Default::default(), Some(&pipeline))
                    .unwrap_or_else(|e| panic!("{name}/{}: Vulkan: {e:?}", entry.name));
            }
            let mut layouter = naga::proc::Layouter::default();
            layouter.update(module.to_ctx()).expect("layout");
            for (ty_name, rust) in sizes {
                if *ty_name == "Camera" && (*name == "corona" || *name == "sky") {
                    if let Some((h, _)) = module
                        .types
                        .iter()
                        .find(|(_, t)| t.name.as_deref() == Some(*ty_name))
                    {
                        let prefix = if *name == "corona" {
                            std::mem::offset_of!(CameraUniform, clouds)
                        } else {
                            std::mem::offset_of!(CameraUniform, spot_vp)
                        };
                        assert_eq!(
                            layouter[h].size as usize,
                            prefix,
                            "{name}: Camera prefix"
                        );
                    }
                    continue;
                }
                if let Some((h, _)) = module
                    .types
                    .iter()
                    .find(|(_, t)| t.name.as_deref() == Some(*ty_name))
                {
                    assert_eq!(layouter[h].size as usize, *rust, "{name}: {ty_name}");
                    checked.insert(*ty_name);
                }
            }
        }
        assert_eq!(checked.len(), sizes.len(), "structs checked: {checked:?}");
    }

    #[test]
    fn the_scene_shader_translates_to_glsl() {
        use wgpu::naga;
        use wgpu::naga::back::glsl;
        let src = scene_shader_source(true);
        let module = naga::front::wgsl::parse_str(&src)
            .unwrap_or_else(|e| panic!("{}", e.emit_to_string(&src)));
        let info = naga::valid::Validator::new(
            naga::valid::ValidationFlags::all(),
            naga::valid::Capabilities::all(),
        )
            .validate(&module)
            .expect("validate");
        let (module, info) = naga::back::pipeline_constants::process_overrides(
            &module,
            &info,
            None,
            &Default::default(),
        )
            .expect("overrides");
        for version in [
            glsl::Version::Embedded {
                version: 310,
                is_webgl: false,
            },
            glsl::Version::Desktop(430),
        ] {
            let options = glsl::Options {
                version,
                ..Default::default()
            };
            for entry in &module.entry_points {
                let pipeline = glsl::PipelineOptions {
                    shader_stage: entry.stage,
                    entry_point: entry.name.clone(),
                    multiview: None,
                };
                let mut out = String::new();
                glsl::Writer::new(
                    &mut out,
                    &module,
                    &info,
                    &options,
                    &pipeline,
                    Default::default(),
                )
                    .and_then(|mut w| w.write())
                    .unwrap_or_else(|e| panic!("{version:?} {}: {e:?}", entry.name));
                assert!(
                    !out.contains("invariant gl_FragCoord"),
                    "{version:?} {}",
                    entry.name
                );
            }
        }
    }

    #[test]
    fn the_sky_is_recomputed_only_when_it_has_moved_on() {
        let a = atmosphere::SkyInput::default();
        assert!(!sky_input_differs(&a, &a));
        let turn = |deg: f32| {
            glam::Quat::from_axis_angle(a.sun_dir.any_orthonormal_vector(), deg.to_radians())
                * a.sun_dir
        };
        assert!(!sky_input_differs(
            &a,
            &atmosphere::SkyInput {
                sun_dir: turn(0.01),
                tint: [Vec3::splat(1.001); 3],
                ..a
            }
        ));
        assert!(sky_input_differs(
            &a,
            &atmosphere::SkyInput {
                sun_dir: turn(0.25),
                ..a
            }
        ));
        assert!(sky_input_differs(
            &a,
            &atmosphere::SkyInput { rain: 0.3, ..a }
        ));
        assert!(sky_input_differs(
            &a,
            &atmosphere::SkyInput {
                tint: [Vec3::new(1.2, 1.0, 0.9), Vec3::ONE, Vec3::ONE],
                ..a
            }
        ));
    }

    #[test]
    fn each_path_gets_its_own_headlights() {
        let lamp = PointLight {
            radius: 30.0,
            color: [1.0, 0.78, 0.46],
            core: 5.0,
            ..Default::default()
        };
        let stand_in = PointLight {
            radius: 18.0,
            intensity: 0.8,
            mode: LightMode::Vanilla,
            ..Default::default()
        };
        let spot = PointLight {
            radius: 60.0,
            intensity: 20.0,
            direction: Vec3::new(0.0, 2.0, -0.6),
            cone: [0.97, 0.82],
            core: 1.0,
            beam: 24.0,
            mode: LightMode::Enhanced,
            ..Default::default()
        };
        assert!(drawn_by(&lamp, false) && drawn_by(&lamp, true));
        assert!(drawn_by(&stand_in, false) && !drawn_by(&stand_in, true));
        assert!(!drawn_by(&spot, false) && drawn_by(&spot, true));
        assert!(!drawn_by(
            &PointLight {
                intensity: 0.0,
                ..lamp
            },
            true
        ));
        let g = gpu_light(&lamp, Vec3::new(1.0, 2.0, 3.0));
        assert_eq!(g.pos, [1.0, 2.0, 3.0, 30.0]);
        assert_eq!(g.color, [1.0, 0.78, 0.46, 1.0]);
        assert_eq!(g.dir[3], -2.0);
        assert_eq!(g.extra[1..], [5.0, 0.0, 30.0]);
        let g = gpu_light(&spot, Vec3::ZERO);
        assert_eq!(g.pos[3], 0.0);
        assert_eq!(g.extra[3], 60.0);
        assert!(
            (Vec3::from_slice(&g.dir[..3]).length() - 1.0).abs() < 1e-5
                && g.dir[3] == 0.82
                && g.extra[0] == 0.97
        );
        assert_eq!(g.extra[2], 24.0);
        assert_eq!(gpu_light(&lamp, Vec3::ZERO).extra[2], 0.0);
    }

    #[test]
    fn metering_defaults_are_gentle() {
        let m = meter_tuning();
        assert!(m[0] > 0.0 && m[0] < 0.6, "{m:?}");
        assert!(m[2] <= 0.75 && m[3] <= 1.0, "{m:?}");
        let ev = ((m[1] - (m[1] + 1.5)) * m[0]).clamp(-m[2], m[3]);
        assert!(ev < 0.0 && ev >= -0.75, "{ev}");
    }

    #[test]
    fn half_floats_read_back() {
        for v in [0.0f32, 1.0, -2.5, 0.5, 1e-3, -3.46] {
            assert!(
                (half_to_f32(atmosphere::f16_bits(v)) - v).abs() <= v.abs() * 1e-3 + 1e-6,
                "{v}"
            );
        }
    }

    #[test]
    fn vehicle_box_contains_the_driver() {
        let bus = (
            DVec3::new(100.0, 200.0, 30.0),
            90.0,
            [2.5, 12.0, 3.0, 0.0, 0.4, 1.5],
        );
        assert!(point_in_vehicle_box(DVec3::new(104.6, 200.7, 31.8), &bus));
        assert!(!point_in_vehicle_box(DVec3::new(104.6, 197.0, 31.8), &bus));
        assert!(!point_in_vehicle_box(DVec3::new(104.6, 200.0, 34.0), &bus));
        assert!(!point_in_vehicle_box(DVec3::new(93.0, 200.0, 31.0), &bus));
    }

    #[test]
    fn opaque_and_transmapped_materials_ignore_dynamic_alpha() {
        assert_eq!(
            Renderer::clamp_slot_alpha(0.0, AlphaMode::Opaque, false),
            1.0
        );
        assert_eq!(
            Renderer::clamp_slot_alpha(0.35, AlphaMode::Opaque, false),
            1.0
        );
        assert_eq!(Renderer::clamp_slot_alpha(0.0, AlphaMode::Test, false), 1.0);
        assert_eq!(
            Renderer::clamp_slot_alpha(0.35, AlphaMode::Test, false),
            1.0
        );
        assert_eq!(
            Renderer::clamp_slot_alpha(0.85, AlphaMode::Blend, false),
            0.85
        );
        assert_eq!(Renderer::clamp_slot_alpha(0.0, AlphaMode::Blend, true), 1.0);
        assert_eq!(
            Renderer::clamp_slot_alpha(0.0, AlphaMode::Opaque, true),
            1.0
        );
        assert_eq!(Renderer::clamp_slot_alpha(0.0, AlphaMode::Test, true), 1.0);
    }
}

pub fn fit_texture(
    data: &omsi_texture::TextureData,
    max: u32,
) -> Option<omsi_texture::TextureData> {
    use omsi_texture::PixelFormat;
    let (w, h) = (data.width.max(1), data.height.max(1));
    if w <= max && h <= max {
        return None;
    }
    let mut k = 0u32;
    while (w >> k).max(1) > max || (h >> k).max(1) > max {
        k += 1;
    }
    if (k as usize) < data.levels.len() {
        return Some(omsi_texture::TextureData {
            width: (w >> k).max(1),
            height: (h >> k).max(1),
            levels: data.levels[k as usize..].to_vec(),
            ..data.clone()
        });
    }
    let mut rgba = match (data.format, data.levels.first()) {
        (PixelFormat::Rgba8, Some(l)) => l.clone(),
        (f, Some(l)) => omsi_texture::bc::decode(
            l,
            w,
            h,
            match f {
                PixelFormat::Bc1 => omsi_texture::bc::Bc::Bc1 { punch: true },
                PixelFormat::Bc2 => omsi_texture::bc::Bc::Bc2,
                _ => omsi_texture::bc::Bc::Bc3,
            },
        ),
        _ => return None,
    };
    let (mut cw, mut ch) = (w, h);
    for _ in 0..k {
        let (nw, nh) = ((cw / 2).max(1), (ch / 2).max(1));
        let mut next = vec![0u8; (nw * nh * 4) as usize];
        for y in 0..nh {
            for x in 0..nw {
                for c in 0..4 {
                    let at = |xx: u32, yy: u32| {
                        rgba[((yy.min(ch - 1) * cw + xx.min(cw - 1)) * 4 + c) as usize] as u32
                    };
                    let v = at(2 * x, 2 * y)
                        + at(2 * x + 1, 2 * y)
                        + at(2 * x, 2 * y + 1)
                        + at(2 * x + 1, 2 * y + 1);
                    next[((y * nw + x) * 4 + c) as usize] = (v / 4) as u8;
                }
            }
        }
        rgba = next;
        (cw, ch) = (nw, nh);
    }
    Some(omsi_texture::TextureData {
        width: cw,
        height: ch,
        format: PixelFormat::Rgba8,
        levels: vec![rgba],
        has_alpha: data.has_alpha,
        gpu_mips: true,
    })
}

#[cfg(test)]
mod fit_tests {
    #[test]
    fn a_big_picture_is_halved_until_it_fits() {
        let data = omsi_texture::TextureData {
            width: 8,
            height: 4,
            format: omsi_texture::PixelFormat::Rgba8,
            levels: vec![vec![200; 8 * 4 * 4]],
            has_alpha: false,
            gpu_mips: true,
        };
        let small = super::fit_texture(&data, 2).unwrap();
        assert_eq!((small.width, small.height), (2, 1));
        assert_eq!(small.levels[0].len(), 2 * 4);
        assert!(super::fit_texture(&data, 8).is_none());
    }
}

thread_local! {
    static CATCHING: std::cell::Cell<bool> = const { std::cell::Cell::new(false) };
}

pub fn catching() -> bool {
    CATCHING.get()
}

pub fn catch<R>(f: impl FnOnce() -> R) -> Option<R> {
    let was = CATCHING.replace(true);
    let r = std::panic::catch_unwind(std::panic::AssertUnwindSafe(f));
    CATCHING.set(was);
    r.ok()
}