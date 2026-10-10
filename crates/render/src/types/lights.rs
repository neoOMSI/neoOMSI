use crate::*;

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
    pub shadow_first: bool,
}

/// The cone of a screen's light (cos of the inner and outer half angle).
pub const SCREEN_CONE: [f32; 2] = [0.85, 0.0];

impl PointLight {
    /// A screen's own light (script / HTML display, LED panel): small, shines to one side
    /// and casts no shadow of its own - it is not worth a shadow map or an occluder walk
    /// that would start over each metre the bus moves.
    pub fn is_screen(&self) -> bool {
        self.mode == LightMode::Enhanced
            && self.cone == SCREEN_CONE
            && self.direction.length_squared() > 1e-6
    }
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
            shadow_first: false,
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
    /// The sprite's turn about the line of sight (radians).
    pub angle: f32,
    /// How far the sprite is drawn towards the viewer (m), so that it does not cut a hard
    /// edge into the ground or the body it stands in.
    pub pull: f32,
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
    pub spread: f32,
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
            spread: 1.0,
        }
    }
}

pub(crate) const LIGHT_CELL: f32 = 25.0;
pub(crate) const LIGHT_GRID_SIDE: usize = 64;
pub(crate) const LIGHT_CELL_CAP: usize = 64;

pub(crate) fn drawn_by(l: &PointLight, enhanced: bool) -> bool {
    l.radius > 0.0
        && l.intensity > 0.0
        && l.mode
        != if enhanced {
        LightMode::Vanilla
    } else {
        LightMode::Enhanced
    }
}

pub(crate) fn gpu_light(l: &PointLight, p: Vec3) -> GpuPointLight {
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
