use crate::*;

#[repr(C)]
#[derive(Clone, Copy, bytemuck::Pod, bytemuck::Zeroable)]
pub struct Vertex {
    pub pos: [f32; 3],
    pub normal: [f32; 3],
    pub uv: [f32; 2],
}

#[repr(C)]
#[derive(Clone, Copy, bytemuck::Pod, bytemuck::Zeroable)]
pub(crate) struct CameraUniform {
    pub(crate) view_proj: [[f32; 4]; 4],
    pub(crate) cam_pos: [f32; 4],
    pub(crate) world_origin: [f32; 4],
    pub(crate) sun_dir: [f32; 4],
    pub(crate) ambient: [f32; 4],
    pub(crate) fog: [f32; 4],
    pub(crate) sun_color: [f32; 4],
    pub(crate) sky_color: [f32; 4],
    pub(crate) light_grid: [f32; 4],
    pub(crate) sky: [f32; 4],
    pub(crate) cam_right: [f32; 4],
    pub(crate) cam_up: [f32; 4],
    pub(crate) clouds: [f32; 4],
    pub(crate) light_view_proj: [[f32; 4]; 4],
    pub(crate) light_view_proj_far: [[f32; 4]; 4],
    pub(crate) shadow: [f32; 4],
    pub(crate) post: [f32; 4],
    pub(crate) inside_a: [f32; 4],
    pub(crate) inside_b: [f32; 4],
    pub(crate) inside_c: [f32; 4],
    pub(crate) flags: [f32; 4],
    pub(crate) light_view_proj_close: [[f32; 4]; 4],
    pub(crate) wind: [f32; 4],
    pub(crate) spot_vp: [[[f32; 4]; 4]; SPOT_SLOTS],
    pub(crate) spot_info: [f32; 4],
    pub(crate) tune: [f32; 4],
    pub(crate) trailers: [[f32; 4]; 3 * TRAILER_BOXES],
}

/// Trailer boxes (`Lighting::puddle_parts`) in the camera uniform, laid out as `inside_a`..`c`.
pub(crate) const TRAILER_BOXES: usize = 3;

/// The trailer boxes for the fog, relative to the render origin `ro`. Each box is stretched
/// forward to the box ahead of it (up to 2.5 m): the joint between them (an articulated bus's
/// bellows, 1.5 m on the O530 G) is cabin as well.
pub(crate) fn trailer_fog_boxes(
    inside: Option<(DVec3, f64, [f32; 6])>,
    parts: &[(DVec3, f64, [f32; 6])],
    ro: DVec3,
) -> [[f32; 4]; 3 * TRAILER_BOXES] {
    let mut t = [[0.0; 4]; 3 * TRAILER_BOXES];
    let mut ahead = inside;
    for (k, &(o, h, mut bb)) in parts.iter().take(TRAILER_BOXES).enumerate() {
        let (sh, ch) = h.to_radians().sin_cos();
        if let Some((po, ph, pb)) = ahead {
            // the rear face of the box ahead, along this one's axis
            let (psh, pch) = ph.to_radians().sin_cos();
            let back = f64::from(pb[4] - pb[1] * 0.5);
            let rear = po
                + DVec3::new(psh, pch, 0.0) * back
                + DVec3::new(pch, -psh, 0.0) * f64::from(pb[3]);
            let gap =
                ((rear - o).dot(DVec3::new(sh, ch, 0.0)) - f64::from(bb[4] + bb[1] * 0.5)) as f32;
            if gap > 0.0 && gap < 2.5 {
                bb[1] += gap;
                bb[4] += gap * 0.5;
            }
        }
        ahead = Some((o, h, bb));
        let r = (o - ro).as_vec3();
        t[3 * k] = [r.x, r.y, r.z, sh as f32];
        t[3 * k + 1] = [ch as f32, bb[0] * 0.5, bb[1] * 0.5, bb[2] * 0.5];
        t[3 * k + 2] = [bb[3], bb[4], bb[5], 1.0];
    }
    t
}

pub(crate) const CLOUD_ORIGIN_PERIOD: f64 = 70000.0;

#[repr(C)]
#[derive(Clone, Copy, bytemuck::Pod, bytemuck::Zeroable)]
pub(crate) struct PostUniform {
    pub(crate) a: [f32; 4],
    pub(crate) b: [f32; 4],
    pub(crate) c: [f32; 4],
    pub(crate) d: [f32; 4],
}

#[repr(C)]
#[derive(Clone, Copy, bytemuck::Pod, bytemuck::Zeroable)]
pub(crate) struct EnhancedUniform {
    pub(crate) exposure: [f32; 4],
    pub(crate) sun: [f32; 4],
    pub(crate) sh: [[f32; 4]; 9],
    pub(crate) ground: [f32; 4],
    pub(crate) fog: [f32; 4],
    pub(crate) fog_color: [f32; 4],
    pub(crate) weather: [f32; 4],
    pub(crate) lights: [f32; 4],
    pub(crate) sun_disc: [f32; 4],
    pub(crate) debug: [f32; 4],
    pub(crate) eye: [f32; 4],
    pub(crate) led: [f32; 4],
    pub(crate) layers: [[f32; 4]; 3],
}

pub(crate) struct Probe {
    pub(crate) view: wgpu::TextureView,
    pub(crate) faces: Vec<Vec<wgpu::TextureView>>,
    pub(crate) bind_groups: Vec<[wgpu::BindGroup; 2]>,
    pub(crate) sky_pipeline: wgpu::RenderPipeline,
    pub(crate) filter_pipeline: wgpu::RenderPipeline,
    pub(crate) age: u32,
    pub(crate) scale: f32,
    pub(crate) cube_view: wgpu::TextureView,
    pub(crate) cube_faces: Vec<wgpu::TextureView>,
    pub(crate) cube_bind_groups: Vec<wgpu::BindGroup>,
    pub(crate) cube_pipeline: wgpu::RenderPipeline,
    pub(crate) cube_next: u32,
    pub(crate) cube_filled: bool,
    pub(crate) cube_round: u32,
    pub(crate) cube_wait: u32,
    pub(crate) cube_eye: Option<DVec3>,
    pub(crate) cube_recapture: bool,
    pub(crate) cloud_sig: [f32; 12],
    pub(crate) cloud_age: u32,
}

pub(crate) const SKY_CUBE_SIZE: u32 = 1024;
pub(crate) const SKY_CUBE_ROUNDS: u32 = 8;
pub(crate) const SKY_CUBE_HISTORY: f64 = 0.8;
pub(crate) const SKY_CUBE_EVERY: u32 = 4;

pub(crate) const PROBE_SIZE: u32 = 64;
pub(crate) const PROBE_MIPS: u32 = 6;
pub(crate) const LAMP_E: f32 = 0.0055;
pub(crate) const CABIN_E: f32 = 0.03;
pub(crate) const WINDOW_RADIANCE: f32 = 0.0022;
pub(crate) const METER_GAIN: f32 = 0.4;
pub(crate) const METER_TARGET: f32 = -2.84;
pub(crate) const METER_DARKEN: f32 = 0.6;
pub(crate) const METER_BRIGHTEN: f32 = 0.8;
pub(crate) const NIGHT_VISION: f32 = 0.55;
pub(crate) const SUN_RADIUS: f32 = 0.0065;

#[repr(C)]
#[derive(Clone, Copy, bytemuck::Pod, bytemuck::Zeroable)]
pub(crate) struct SsaoUniform {
    pub(crate) inv_proj: [[f32; 4]; 4],
    pub(crate) params: [f32; 4],
}

#[repr(C)]
#[derive(Clone, Copy, bytemuck::Pod, bytemuck::Zeroable)]
pub(crate) struct GpuPointLight {
    pub(crate) pos: [f32; 4],
    pub(crate) color: [f32; 4],
    pub(crate) dir: [f32; 4],
    pub(crate) extra: [f32; 4],
    pub(crate) occ: [f32; 4],
}

#[repr(C)]
#[derive(Clone, Copy, bytemuck::Pod, bytemuck::Zeroable)]
pub(crate) struct GpuCorona {
    pub(crate) pos: [f32; 3],
    pub(crate) size: f32,
    pub(crate) color: [f32; 4],
    pub(crate) dir: [f32; 4],
    pub(crate) up: [f32; 4],
    pub(crate) extra: [f32; 4],
}
