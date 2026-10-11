use crate::*;

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
    /// up to three cloud layers, low to high: base (m), top (m), cover 0..1, shape (0 flat
    /// stratus .. 1 piled cumulus)
    pub cloud_layers: [[f32; 4]; 3],
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
    pub nightmap_glow: f32,
    pub nightmap_gain: f32,
    pub lightmap_gain: f32,
    pub atmosphere_brightness: f32,
    pub html_glow: f32,
    pub html_light: f32,
    pub script_glow: f32,
    pub script_light: f32,
    pub led_mips: f32,
    pub glass_wind: Vec3,
    pub animation_time: Option<f32>,
}

impl Lighting {
    pub fn casts_sun_shadows(&self) -> bool {
        self.shadows
            && self.sun_dir.normalize_or_zero().z > -0.02
            && self.sun_intensity > 0.05
            && ::legacy_config::env::var_os("OMSI_NO_SHADOWS").is_none()
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
            cloud_layers: [[0.0; 4]; 3],
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
            nightmap_glow: 1.5,
            nightmap_gain: 0.75,
            lightmap_gain: 1.0,
            atmosphere_brightness: 1.0,
            html_glow: 1.0,
            html_light: 1.0,
            script_glow: 1.0,
            script_light: 1.0,
            led_mips: 1.3,
            glass_wind: Vec3::ZERO,
            animation_time: None,
        }
    }
}
