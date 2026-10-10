use crate::*;

pub type MeshId = usize;
pub type TextureId = usize;
pub type MaterialId = usize;

pub struct GpuMesh {
    pub(crate) vertex_buf: wgpu::Buffer,
    pub(crate) index_buf: wgpu::Buffer,
    pub ranges: Vec<(u32, u32, u32)>,
    pub bounds_center: Vec3,
    pub bounds_radius: f32,
    pub one_sided: bool,
    pub source: Option<String>,
    /// Per material slot: (mean facing direction, centroid) of its triangles in mesh space;
    /// a zero direction when the slot has no triangles.
    pub slot_faces: Vec<(Vec3, Vec3)>,
}

#[derive(Clone, Copy, Default)]
pub(crate) struct InstanceBounds {
    pub(crate) centre: Vec3,
    pub(crate) radius: f32,
    pub(crate) scale: f32,
}

impl InstanceBounds {
    pub(crate) fn new(mesh: &GpuMesh, transform: Mat4) -> Self {
        let scale = transform_scale(transform);
        Self {
            centre: transform.transform_point3(mesh.bounds_center),
            radius: mesh.bounds_radius * scale,
            scale,
        }
    }
}

pub(crate) fn transform_scale(transform: Mat4) -> f32 {
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
    pub(crate) const COUNT: usize = 9;
    pub(crate) const DRAW_ORDER: [Self; Self::COUNT] = [
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
    /// Enclosed cabin mesh, independent of switched interior lamps.
    pub cabin: bool,
    pub(crate) base: u32,
    pub(crate) bounds: InstanceBounds,
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

impl Instance {
    pub fn world_centre(&self) -> DVec3 {
        self.origin + self.bounds.centre.as_dvec3()
    }
}

pub struct Scene {
    pub meshes: Vec<GpuMesh>,
    pub textures: Vec<GpuTexture>,
    /// Mean (light, alpha, colour) of a texture the last time it was uploaded by
    /// `update_texture`: what a script or HTML screen actually shows. light = alpha x brightest
    /// channel, alpha 0..1; colour = the lit pixels' mean hue, its brightest channel 1.
    pub tex_luma: std::sync::Mutex<std::collections::HashMap<TextureId, (f32, f32, [f32; 3])>>,
    pub(crate) glass_slot: Option<TextureId>,
    pub materials: Vec<Material>,
    pub instances: Vec<Instance>,
    pub render_origin: DVec3,
    pub lights: Vec<PointLight>,
    pub occluders: Vec<Occluder>,
    pub interior_lights: Vec<PointLight>,
    pub(crate) interior_free: Vec<(u32, u32)>,
    pub coronas: Vec<Corona>,
    pub smoke: Vec<SmokeParticle>,
    pub(crate) smoke_buf: Option<wgpu::Buffer>,
    pub(crate) smoke_count: u32,
    pub(crate) corona_runs: Vec<(u16, u32, u32)>,
    pub(crate) corona_order: Vec<usize>,
    pub(crate) corona_data: Vec<GpuCorona>,
    pub(crate) model_buf: Option<wgpu::Buffer>,
    pub(crate) params_buf: Option<wgpu::Buffer>,
    pub(crate) light_buf: Option<wgpu::Buffer>,
    pub(crate) grid_buf: Option<wgpu::Buffer>,
    pub(crate) corona_buf: Option<wgpu::Buffer>,
    pub(crate) corona_count: u32,
    pub(crate) draw_buf: Option<wgpu::Buffer>,
    pub(crate) camera_bind_group: Option<wgpu::BindGroup>,
    pub(crate) shadow_bind_group: Option<wgpu::BindGroup>,
    pub(crate) spot_bind_groups: Vec<wgpu::BindGroup>,
    pub(crate) sky_bind_group: Option<wgpu::BindGroup>,
    pub overlays: Vec<(TextureId, [f32; 4])>,
    pub premultiplied: std::collections::HashSet<TextureId>,
    /// Overlays with these textures are drawn over a blurred copy of what lies beneath
    /// them, rounded at the corners by the radius given (pixels).
    pub frosted: HashMap<TextureId, f32>,
    pub(crate) overlay_res: Vec<(TextureId, wgpu::Buffer, wgpu::BindGroup, [f32; 8])>,
    pub(crate) dirty: bool,
    pub(crate) uploaded_instances: usize,
    pub(crate) uploaded_entries: u32,
    pub(crate) changed: Vec<usize>,
    pub(crate) changed_mark: Vec<bool>,
    pub(crate) cache_bounds: bool,
    pub(crate) bounds_meshes: Vec<bool>,
    pub(crate) bounds_dirty: bool,
    pub(crate) cpu_models: Vec<[[f32; 4]; 4]>,
    pub(crate) cpu_params: Vec<[f32; 4]>,
    pub(crate) last_grid: Vec<u32>,
    pub(crate) last_lights: Vec<u8>,
    pub(crate) bind_groups: HashMap<BindKey, (wgpu::BindGroup, wgpu::Buffer)>,
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
