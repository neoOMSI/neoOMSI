//! Navigator 2.0's picture of the map: the surfaces the map really draws - carriageways,
//! paved areas, footways, track beds, verges - read from the splines' and objects' own
//! geometry, not from the paths the AI drives on. A junction is then one surface with its
//! islands, whatever it is built from (an object's mesh, overlapping asphalt splines or
//! both), and a road no car drives on is still a road.
//!
//! Every textured face of the map that faces up is classified by its material. Which
//! material is a carriageway is learnt per map from the paths: a texture that the street
//! paths (editor-only ones too) run over is road, one the pedestrians walk on is footway,
//! one under the rails is a track bed. The texture's `.cfg` and its name decide the rest.
//! The faces are then rasterised chunk by chunk (the topmost surface wins), closed over the
//! joints, traced into outlines and triangulated.

mod build;
mod cache;
mod earcut;
mod raster;
#[cfg(test)]
mod tests;

use glam::DVec2;
use hashbrown::HashMap;

pub(crate) use self::raster::{CHUNK, on_border};

/// What a stretch of ground is, in drawing order (later ones are drawn over earlier ones).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
#[repr(u8)]
pub enum Surface {
    /// Grass verges, medians, hedges.
    Green = 1,
    /// Pavements, cycle paths, platforms.
    Footway = 2,
    /// Track beds outside the carriageway.
    Track = 3,
    /// Paved ground cars may use without a path on it: yards, car parks, bus stations.
    Paved = 4,
    /// Carriageway: where the street paths run.
    Road = 5,
}

impl Surface {
    pub const ALL: [Surface; 5] = [
        Surface::Green,
        Surface::Footway,
        Surface::Track,
        Surface::Paved,
        Surface::Road,
    ];

    fn from_u8(v: u8) -> Option<Surface> {
        Surface::ALL.get((v as usize).wrapping_sub(1)).copied()
    }
}

/// What the navigator fills, layer by layer (in drawing order).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
#[repr(u8)]
pub enum Layer {
    /// Rivers, canals and lakes: a tile's water where the ground lies below it.
    Water = 0,
    Green = 1,
    /// The crowns of the trees seen from above, over the verges and under every way: as on
    /// a map, a road or a path through an avenue stays a road or a path.
    Canopy = 2,
    Footway = 3,
    Track = 4,
    /// Carriageways and paved ground together, drawn as carriageway: its outline is the
    /// kerb, and no crack between a road and a yard shows the ground through.
    Drivable = 5,
    /// Paved ground cars use without a path on it, drawn over the carriageway.
    Paved = 6,
    /// The outlines of the buildings seen from above (their roofs), over the ground they
    /// stand on: the pavement painted on the ground under a house does not cut into it, and
    /// no house stands on a road.
    Building = 7,
    /// The paint on the road as the map's textures show it.
    Marking = 8,
}

impl Layer {
    pub const ALL: [Layer; 9] = [
        Layer::Water,
        Layer::Green,
        Layer::Canopy,
        Layer::Footway,
        Layer::Track,
        Layer::Drivable,
        Layer::Paved,
        Layer::Building,
        Layer::Marking,
    ];

    /// The surfaces a layer covers.
    fn covers(self, s: Surface) -> bool {
        match self {
            Layer::Green => s == Surface::Green,
            Layer::Footway => s == Surface::Footway,
            Layer::Track => s == Surface::Track,
            Layer::Drivable => matches!(s, Surface::Paved | Surface::Road),
            Layer::Paved => s == Surface::Paved,
            // (read from the textures, the water, the trees and the roofs, not the surfaces)
            Layer::Marking | Layer::Water | Layer::Canopy | Layer::Building => false,
        }
    }

    /// Cells across which the layer is closed: carriageways are mended over the joints
    /// between splines and junction meshes, the rest only over hairlines.
    fn closing(self) -> usize {
        match self {
            Layer::Drivable => 2,
            // (the crowns of a row of trees grow into one canopy, not a chain of circles; the
            // water's edge, cut from the ground's 5 m grid, loses its steps)
            Layer::Canopy => 15,
            Layer::Water => 3,
            _ => 1,
        }
    }

    /// How far (m) the outline may stray from the raster's: the kerb closely, the ground
    /// around it loosely (fewer triangles, and the painted ground's steps smoothed out).
    fn tolerance(self) -> f32 {
        match self {
            Layer::Marking => 0.03,
            Layer::Drivable => 0.12,
            Layer::Paved | Layer::Track => 0.2,
            Layer::Building => 0.25,
            Layer::Footway => 0.3,
            Layer::Green => 0.4,
            Layer::Canopy => 0.6,
            Layer::Water => 0.8,
        }
    }

    /// The smallest piece (m²) worth drawing.
    fn least(self) -> f32 {
        match self {
            Layer::Marking => 0.01,
            Layer::Drivable => 1.0,
            Layer::Building => 6.0,
            _ => 3.0,
        }
    }

    /// Outline tolerance (m) and smallest piece (m²) of the coarse version, if it has one.
    fn coarse(self) -> Option<(f32, f32)> {
        match self {
            Layer::Drivable => Some((0.6, 6.0)),
            Layer::Track => Some((0.8, 20.0)),
            Layer::Green => Some((1.2, 40.0)),
            Layer::Building => Some((0.8, 20.0)),
            Layer::Canopy => Some((1.5, 30.0)),
            Layer::Water => Some((1.5, 40.0)),
            _ => None,
        }
    }
}

/// One filled piece of a chunk: triangles in metres from the chunk's corner, and its
/// outline without the stretches along the chunk's border.
#[derive(Debug, Clone)]
pub struct Area {
    pub layer: Layer,
    /// 0 on the ground, 1 a bridge deck, -1 in a tunnel.
    pub level: i8,
    pub verts: Vec<[f32; 2]>,
    pub tris: Vec<u32>,
    pub edges: Vec<Vec<[f32; 2]>>,
}

#[derive(Debug, Clone, Default)]
pub struct Chunk {
    pub areas: Vec<Area>,
    /// The same coarsely, for a map seen from far: carriageways, track beds, green, buildings
    /// and trees only.
    pub coarse: Vec<Area>,
}

/// How a material was classified, and on what grounds (for the log and the checks).
#[derive(Debug, Clone)]
pub struct MaterialInfo {
    pub name: String,
    pub surface: Option<Surface>,
    /// Faces' area (m²) and the shares of it under street, pedestrian and rail paths.
    pub area: f64,
    pub car: f64,
    pub walk: f64,
    pub rail: f64,
}

/// The map's surfaces, chunk by chunk.
#[derive(Default, Clone)]
pub struct SurfaceMap {
    pub chunks: HashMap<(i32, i32), std::sync::Arc<Chunk>>,
    /// Rails (centre lines of the tracks) outside tunnels.
    pub rails: Vec<Vec<glam::DVec3>>,
    pub materials: Vec<MaterialInfo>,
    /// The files the build read (see `cache`)
    pub read: Vec<std::path::PathBuf>,
}

impl SurfaceMap {
    pub fn chunk_key(p: DVec2) -> (i32, i32) {
        ((p.x / CHUNK).floor() as i32, (p.y / CHUNK).floor() as i32)
    }

    pub fn chunk_origin(k: (i32, i32)) -> DVec2 {
        DVec2::new(k.0 as f64 * CHUNK, k.1 as f64 * CHUNK)
    }

    /// The chunks with a part within `radius` of `c`.
    pub fn chunks_near(&self, c: DVec2, radius: f64) -> Vec<((i32, i32), &Chunk)> {
        let (x0, y0) = Self::chunk_key(c - DVec2::splat(radius));
        let (x1, y1) = Self::chunk_key(c + DVec2::splat(radius));
        let mut out = Vec::new();
        for y in y0..=y1 {
            for x in x0..=x1 {
                if let Some(ch) = self.chunks.get(&(x, y)) {
                    out.push(((x, y), &**ch));
                }
            }
        }
        out
    }
}

pub use self::build::{build_surface_map, build_surface_stream};
pub use self::cache::{load as load_cached, save as save_cached};
