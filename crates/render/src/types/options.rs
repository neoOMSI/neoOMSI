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

pub(crate) const MASK_FORMAT: wgpu::TextureFormat = wgpu::TextureFormat::Rgba8Unorm;
pub(crate) const HDR_FORMAT: wgpu::TextureFormat = wgpu::TextureFormat::Rgba16Float;

pub(crate) const FOG_MIN_DENSITY: f32 = 6e-5;
