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

#[derive(Default)]
pub(crate) struct LightGrid {
    /// The grid as last sent (empty before the first frame).
    pub(crate) cells: Vec<u32>,
    /// Lights in each cell of `cells`.
    counts: Vec<u16>,
    /// This frame's lights in each cell, and (cell, light) in the order they came.
    next_counts: Vec<u16>,
    entries: Vec<(u32, u32)>,
    /// `entries` grouped by cell, and where each cell's group starts.
    sorted: Vec<u32>,
    starts: Vec<u32>,
    fill: Vec<u32>,
}

impl LightGrid {
    const CELLS: usize = LIGHT_GRID_SIDE * LIGHT_GRID_SIDE;

    pub(crate) fn begin(&mut self) {
        self.next_counts.clear();
        self.next_counts.resize(Self::CELLS, 0);
        self.entries.clear();
    }

    /// Adds `light` to `cell`; false when the cell already holds `LIGHT_CELL_CAP` lights.
    pub(crate) fn add(&mut self, cell: usize, light: u32) -> bool {
        let n = &mut self.next_counts[cell];
        if *n as usize >= LIGHT_CELL_CAP {
            return false;
        }
        *n += 1;
        self.entries.push((cell as u32, light));
        true
    }

    /// Writes this frame's cells into `cells`. Returns the span of `cells` that changed
    /// (everything the first time), or `None` when it is the same as before.
    pub(crate) fn finish(&mut self) -> Option<std::ops::Range<usize>> {
        let mut lo = usize::MAX;
        let mut hi = 0;
        if self.cells.len() != Self::CELLS * LIGHT_CELL_CAP {
            self.cells = vec![u32::MAX; Self::CELLS * LIGHT_CELL_CAP];
            self.counts = vec![0; Self::CELLS];
            (lo, hi) = (0, self.cells.len());
        }
        self.starts.clear();
        let mut at = 0u32;
        for &n in &self.next_counts {
            self.starts.push(at);
            at += n as u32;
        }
        self.sorted.clear();
        self.sorted.resize(self.entries.len(), u32::MAX);
        self.fill.clone_from(&self.starts);
        for &(cell, light) in &self.entries {
            let k = &mut self.fill[cell as usize];
            self.sorted[*k as usize] = light;
            *k += 1;
        }
        for c in 0..Self::CELLS {
            let (old, new) = (self.counts[c] as usize, self.next_counts[c] as usize);
            if old == 0 && new == 0 {
                continue;
            }
            let base = c * LIGHT_CELL_CAP;
            let start = self.starts[c] as usize;
            for j in 0..old.max(new) {
                let v = if j < new {
                    self.sorted[start + j]
                } else {
                    u32::MAX
                };
                let slot = &mut self.cells[base + j];
                if *slot != v {
                    *slot = v;
                    lo = lo.min(base + j);
                    hi = hi.max(base + j + 1);
                }
            }
            self.counts[c] = new as u16;
        }
        (lo < hi).then_some(lo..hi)
    }
}

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

#[cfg(test)]
mod tests {
    use super::*;

    /// The grid as it was made every frame before: a fresh grid, each light in the first
    /// free slot of its cells.
    fn rebuilt(adds: &[(usize, u32)]) -> Vec<u32> {
        let mut grid = vec![u32::MAX; LIGHT_GRID_SIDE * LIGHT_GRID_SIDE * LIGHT_CELL_CAP];
        for &(cell, light) in adds {
            let base = cell * LIGHT_CELL_CAP;
            if let Some(slot) = grid[base..base + LIGHT_CELL_CAP]
                .iter()
                .position(|v| *v == u32::MAX)
            {
                grid[base + slot] = light;
            }
        }
        grid
    }

    #[test]
    fn kept_grid_equals_a_rebuilt_one_and_sends_only_changes() {
        let mut grid = LightGrid::default();
        let mut sent = vec![0u32; LIGHT_GRID_SIDE * LIGHT_GRID_SIDE * LIGHT_CELL_CAP];
        let mut seed = 12345u32;
        let mut rnd = |n: u32| {
            seed = seed.wrapping_mul(1_664_525).wrapping_add(1_013_904_223);
            (seed >> 8) % n
        };
        let mut frames: Vec<Vec<(usize, u32)>> = Vec::new();
        for f in 0..40 {
            let lights = if f % 7 == 3 { 0 } else { rnd(150) };
            let mut adds = Vec::new();
            for light in 0..lights {
                // a crowded corner, so that some cells overflow
                let (cx, cy) = if light % 3 == 0 {
                    (rnd(2) as usize, rnd(2) as usize)
                } else {
                    (rnd(64) as usize, rnd(64) as usize)
                };
                for dy in 0..=rnd(2) as usize {
                    for dx in 0..=rnd(2) as usize {
                        let (x, y) = ((cx + dx).min(63), (cy + dy).min(63));
                        adds.push((y * LIGHT_GRID_SIDE + x, light));
                    }
                }
            }
            frames.push(adds);
        }
        // the same frame twice: nothing to send the second time
        frames.insert(10, frames[9].clone());
        for (f, adds) in frames.iter().enumerate() {
            grid.begin();
            for &(cell, light) in adds {
                grid.add(cell, light);
            }
            let span = grid.finish();
            let want = rebuilt(adds);
            assert_eq!(grid.cells, want, "frame {f}");
            if let Some(span) = span.clone() {
                sent[span.clone()].copy_from_slice(&grid.cells[span]);
            }
            assert_eq!(sent, want, "frame {f}: the card's copy");
            if f == 10 {
                assert_eq!(span, None);
            }
        }
    }
}
