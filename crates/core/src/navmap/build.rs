use std::path::Path;
use std::sync::Arc;

use glam::{DVec2, DVec3, Mat4, Vec2, Vec3};
use hashbrown::HashMap;
use parking_lot::Mutex;
use rayon::prelude::*;

use ::map::{Terrain, tile_size};
use ::simulation::traffic::{Lane, LaneKind};

use super::cache::Deps;
use super::raster::{self, CELL, MARGIN, Raster};
use super::*;
use crate::scene::{World, mesh_path, texture_dirs, tile_companion};

/// A face of the map that faces up: corners in metres from its chunk's corner (heights as
/// they are), its material and its level.
struct Face {
    v: [[f32; 3]; 3],
    /// Texture coordinates at the corners.
    uv: [[f32; 2]; 3],
    material: u32,
    texture: u32,
    level: i8,
}

/// Higher above the ground than this is a bridge deck, lower a tunnel (m).
const LEVEL_GAP: f64 = 4.0;
/// A face is a floor when its normal is at most this far off the vertical (cos).
const UPRIGHT: f32 = 0.8;

struct Source {
    file: String,
    dirs: Vec<std::path::PathBuf>,
    cfg: ::texture::TextureCfg,
    uses: u32,
    texture: u32,
}

#[derive(Default)]
struct Materials {
    ids: HashMap<String, u32>,
    names: Vec<String>,
    sources: Vec<HashMap<u64, Source>>,
    textures: Vec<(String, Vec<std::path::PathBuf>, u32)>,
}

impl Materials {
    fn id(this: &Mutex<Materials>, file: &str, dirs: &[std::path::PathBuf]) -> (u32, u32) {
        use std::hash::{Hash, Hasher};
        let base = file.replace('\\', "/");
        let base = base.rsplit('/').next().unwrap_or("").to_ascii_lowercase();
        let key = base
            .rsplit_once('.')
            .map(|(s, _)| s.to_string())
            .unwrap_or(base);
        let print = {
            let mut h = std::collections::hash_map::DefaultHasher::new();
            (file, dirs).hash(&mut h);
            h.finish()
        };
        let id = {
            let mut m = this.lock();
            let id = match m.ids.get(&key) {
                Some(&id) => id,
                None => {
                    let id = m.names.len() as u32;
                    m.ids.insert(key.clone(), id);
                    m.names.push(key);
                    m.sources.push(HashMap::new());
                    id
                }
            };
            if let Some(s) = m.sources[id as usize].get_mut(&print) {
                s.uses += 1;
                return (id, s.texture);
            }
            id
        };
        let refs: Vec<&Path> = dirs.iter().map(|d| d.as_path()).collect();
        let cfg = ::texture::find_texture(file, &refs)
            .and_then(|p| ::texture::cfg_path(file, &p))
            .map(|c| ::texture::TextureCfg::load(&c))
            .unwrap_or_default();
        let mut m = this.lock();
        let m = &mut *m;
        let s = m.sources[id as usize].entry(print).or_insert_with(|| {
            m.textures.push((file.to_string(), dirs.to_vec(), id));
            Source {
                file: file.to_string(),
                dirs: dirs.to_vec(),
                cfg,
                uses: 0,
                texture: m.textures.len() as u32 - 1,
            }
        });
        s.uses += 1;
        (id, s.texture)
    }

    #[allow(clippy::type_complexity)]
    fn settle(self) -> (Vec<String>, Vec<Source>, Vec<(String, Vec<std::path::PathBuf>, u32)>) {
        let sources = self
            .sources
            .into_iter()
            .map(|all| {
                all.into_values()
                    .min_by(|a, b| {
                        b.uses
                            .cmp(&a.uses)
                            .then_with(|| (&a.file, &a.dirs).cmp(&(&b.file, &b.dirs)))
                    })
                    .expect("a material has a source")
            })
            .collect();
        (self.names, sources, self.textures)
    }
}

/// Where the paths run: segments of the street, pedestrian and rail paths in a grid.
pub(super) struct Evidence {
    segs: Vec<(DVec3, DVec3, f32, u8)>,
    grid: HashMap<(i32, i32), Vec<u32>>,
}

const EV_CELL: f64 = 10.0;
const CAR: u8 = 1;
const WALK: u8 = 2;
const RAIL: u8 = 4;

impl Evidence {
    pub(super) fn new(lanes: &[Lane]) -> Evidence {
        let mut ev = Evidence {
            segs: Vec::new(),
            grid: HashMap::new(),
        };
        // a two-way path is two lanes over the same ground: once is enough
        let mut seen = hashbrown::HashSet::new();
        for l in lanes {
            let bit = match l.kind {
                LaneKind::Street => CAR,
                LaneKind::Sidewalk => WALK,
                LaneKind::Rail => RAIL,
                LaneKind::Air => continue,
            };
            if let Some(k) = l.key {
                if !seen.insert((k, bit, (l.offset * 10.0) as i32)) {
                    continue;
                }
            }
            let half = (l.width.max(if bit == RAIL { 1.5 } else { 2.0 })) * 0.5;
            for ab in l.points.windows(2) {
                let (a, b) = (ab[0], ab[1]);
                let i = ev.segs.len() as u32;
                ev.segs.push((a, b, half, bit));
                let m = half as f64;
                let lo = a.truncate().min(b.truncate()) - DVec2::splat(m);
                let hi = a.truncate().max(b.truncate()) + DVec2::splat(m);
                for y in (lo.y / EV_CELL).floor() as i32..=(hi.y / EV_CELL).floor() as i32 {
                    for x in (lo.x / EV_CELL).floor() as i32..=(hi.x / EV_CELL).floor() as i32 {
                        ev.grid.entry((x, y)).or_default().push(i);
                    }
                }
            }
        }
        ev
    }

    /// The kinds of path running over `p`.
    fn at(&self, p: DVec3) -> u8 {
        let key = ((p.x / EV_CELL).floor() as i32, (p.y / EV_CELL).floor() as i32);
        let Some(ids) = self.grid.get(&key) else {
            return 0;
        };
        let mut bits = 0;
        for &i in ids {
            let (a, b, half, bit) = self.segs[i as usize];
            if bits & bit != 0 {
                continue;
            }
            let ab = (b - a).truncate();
            let t = ((p - a).truncate().dot(ab) / ab.length_squared().max(1e-9)).clamp(0.0, 1.0);
            let q = a.lerp(b, t);
            if (p - q).truncate().length() <= half as f64 + 0.25 && (p.z - q.z).abs() <= 1.2 {
                bits |= bit;
            }
        }
        bits
    }
}

/// The floor faces of an object type in its own frame, with their materials.
struct ObjectFloors {
    faces: Vec<([Vec3; 3], [[f32; 2]; 3], (u32, u32))>,
    /// It carries a street: its floors may be bridge decks high above the ground.
    streets: bool,
    /// `[absolute_height]`: placed at the height the map gives, not on the ground.
    absolute: bool,
    /// Its roof seen from above, in its own frame: a building's outline (see [`roof_of`]).
    roof: Vec<[Vec2; 3]>,
    /// It is a tree: its crown seen from above.
    crown: Option<Crown>,
}

/// A tree's crown seen from above.
#[derive(Clone, Copy)]
enum Crown {
    /// A `[tree]` billboard: as wide as its height times its ratio, which the map gives each
    /// tree (else the middle of the type's ranges: height, ratio).
    Billboard(f32, f32),
    /// A tree with a mesh of its own: its radius (m).
    Mesh(f32),
}

/// How high above its origin a face of an object must lie to be its roof (m): above a
/// storey, not a step, a kerb or a bench.
const ROOF_ABOVE: f32 = 2.5;
/// A roof is at most this steep (cos of the angle off the vertical): walls are not.
const ROOF_SLOPE: f32 = 0.3;
/// The smallest roof that is a building (m²), and how narrow it may be across (m): a lamp's
/// head, a sign or a gantry over the road are not.
const ROOF_LEAST: f32 = 8.0;
const ROOF_NARROW: f32 = 2.0;
/// Cells by which the roofs are opened: strips narrower than twice that go.
const ROOF_THIN: usize = 3;
/// A crown seen from above is a little narrower than its billboard is wide (the leaves
/// thin out to its edge).
const CROWN_SHARE: f32 = 0.85;

/// The faces that make an object's roof, seen from above in its own frame: those lying
/// [`ROOF_ABOVE`] its origin and not upright, if they cover a building's ground. None for a
/// lamp post, a sign, a gantry - nothing as small or narrow as those.
pub(super) fn roof_of(tris: &[[Vec3; 3]]) -> Vec<[Vec2; 3]> {
    let roof: Vec<[Vec2; 3]> = tris
        .iter()
        .filter(|p| {
            let n = (p[1] - p[0]).cross(p[2] - p[0]);
            let len = n.length();
            len > 1e-6 && (n.z / len).abs() >= ROOF_SLOPE && (p[0].z + p[1].z + p[2].z) / 3.0 >= ROOF_ABOVE
        })
        .map(|p| p.map(|v| v.truncate()))
        .collect();
    let area: f32 = roof
        .iter()
        .map(|p| (p[1] - p[0]).perp_dot(p[2] - p[0]).abs() * 0.5)
        .sum();
    let (lo, hi) = roof.iter().flatten().fold(
        (Vec2::splat(f32::MAX), Vec2::splat(f32::MIN)),
        |(lo, hi), v| (lo.min(*v), hi.max(*v)),
    );
    let size = hi - lo;
    if area < ROOF_LEAST || size.min_element() < ROOF_NARROW {
        return Vec::new();
    }
    roof
}

/// A tree by its name, for one with a mesh rather than a `[tree]` billboard: a word of its
/// file's name, or of the folders it is in under `Sceneryobjects` (else the one it is in),
/// that names a tree
pub(super) fn tree_named(path: &Path) -> bool {
    let words = |s: &str| -> Vec<String> {
        s.to_lowercase()
            .split(|c: char| !c.is_alphabetic())
            .filter(|w| !w.is_empty())
            .map(str::to_string)
            .collect()
    };
    let stem = path.file_stem().map(|s| s.to_string_lossy().into_owned()).unwrap_or_default();
    let folders: Vec<String> = path
        .parent()
        .map(|p| p.iter().map(|c| c.to_string_lossy().into_owned()).collect())
        .unwrap_or_default();
    let folder = match folders.iter().rposition(|c| c.eq_ignore_ascii_case("sceneryobjects")) {
        Some(k) => folders[k + 1..].join("/"),
        None => folders.last().cloned().unwrap_or_default(),
    };
    let whole = [
        "tree", "trees", "bush", "bushes", "shrub", "linde", "eiche", "kastanie", "birke",
        "pappel", "baeume", "bäume", "büsche", "buesche",
    ];
    let compound = ["baum", "busch", "strauch"];
    words(&stem).iter().chain(&words(&folder)).any(|w| {
        whole.contains(&w.as_str()) || compound.iter().any(|c| w.ends_with(c))
    })
}

fn object_floors(
    world: &World,
    file: &str,
    materials: &Mutex<Materials>,
    deps: &Deps,
) -> Option<ObjectFloors> {
    let path = ::mods::resolve_path(&world.root, file);
    deps.add(&path);
    let sco = ::scenery::SceneryObject::load(&path).ok()?;
    let billboard = sco.tree.as_ref().map(|(_, min_h, max_h, min_r, max_r)| {
        let mid = |a: f32, b: f32, or: f32| if a + b > 0.0 { (a + b) * 0.5 } else { or };
        Crown::Billboard(mid(*min_h, *max_h, 10.0), mid(*min_r, *max_r, 1.0))
    });
    if billboard.is_some() {
        return Some(ObjectFloors {
            faces: Vec::new(),
            streets: false,
            absolute: false,
            roof: Vec::new(),
            crown: billboard,
        });
    }
    if sco.only_editor {
        return None;
    }
    let sco_dir = path.parent()?.to_path_buf();
    let (model, model_dir) = match &sco.model_file {
        Some(m) => {
            let mp = ::mods::resolve_path(&sco_dir, m);
            deps.add(&mp);
            (::model::Model::load(&mp).ok()?, mp.parent()?.to_path_buf())
        }
        None => (sco.model.clone(), sco_dir.clone()),
    };
    if model.lods.is_empty() {
        return None;
    }
    let streets = sco.paths.iter().any(|p| p.kind == 0);
    // (a railway bridge, a platform: its deck carries the tracks, it is no house)
    let rails = sco.paths.iter().any(|p| p.kind == 2);
    let dirs = texture_dirs(&world.root, &model_dir);
    let mut faces = Vec::new();
    // every face, for the roof and a tree's crown
    let mut all: Vec<[Vec3; 3]> = Vec::new();
    for md in model.lod_meshes(0) {
        let mp = mesh_path(&model_dir, &md.file);
        deps.add(&mp);
        let Ok(m) = ::legacy_o3d::load_mesh(&mp) else {
            continue;
        };
        let mesh = ::geometry::mesh_from_o3d(&m);
        all.extend(mesh.indices.chunks_exact(3).map(|t| {
            [
                mesh.positions[t[0] as usize],
                mesh.positions[t[1] as usize],
                mesh.positions[t[2] as usize],
            ]
        }));
        for &(first, count, slot) in &mesh.ranges {
            let Some(tex) = m.materials.get(slot as usize).map(|t| t.texture.clone()) else {
                continue;
            };
            if tex.trim().is_empty() {
                continue;
            }
            let mut id = None;
            for t in mesh.indices[first as usize..(first + count) as usize].chunks_exact(3) {
                let p = [
                    mesh.positions[t[0] as usize],
                    mesh.positions[t[1] as usize],
                    mesh.positions[t[2] as usize],
                ];
                let n = (p[1] - p[0]).cross(p[2] - p[0]);
                let len = n.length();
                if len < 1e-6 || (n.z / len).abs() < UPRIGHT {
                    continue;
                }
                let id = *id.get_or_insert_with(|| Materials::id(materials, &tex, &dirs));
                let uv = [t[0], t[1], t[2]].map(|k| mesh.uvs.get(k as usize).map(|v| v.to_array()).unwrap_or([0.0; 2]));
                faces.push((p, uv, id));
            }
        }
    }
    // a tree with a mesh: its crown as wide as the mesh; anything else with a roof high
    // enough is a building - not one that carries a street or tracks or is a surface itself
    let (roof, crown) = if tree_named(&path) {
        let (lo, hi) = all.iter().flatten().fold(
            (Vec2::splat(f32::MAX), Vec2::splat(f32::MIN)),
            |(lo, hi), v| (lo.min(v.truncate()), hi.max(v.truncate())),
        );
        let r = ((hi - lo).max_element() * 0.5).clamp(0.5, 12.0);
        (Vec::new(), (!all.is_empty()).then_some(Crown::Mesh(r)))
    } else if streets || rails || sco.surface {
        (Vec::new(), None)
    } else {
        (roof_of(&all), None)
    };
    (!faces.is_empty() || !roof.is_empty() || crown.is_some()).then_some(ObjectFloors {
        faces,
        streets,
        absolute: sco.absolute_height(),
        roof,
        crown,
    })
}

/// Material statistics: area and the areas under street, pedestrian and rail paths.
/// Per material: its area, the parts of it under street, pedestrian and rail paths, and the
/// part of a street spline at the height of the street paths beside it (m²).
type Stats = HashMap<u32, [f64; 5]>;

/// Points spread over a triangle (barycentric weights), more for a bigger one.
const SPREAD: [[f64; 3]; 7] = [
    [1.0 / 3.0, 1.0 / 3.0, 1.0 / 3.0],
    [0.70, 0.15, 0.15],
    [0.15, 0.70, 0.15],
    [0.15, 0.15, 0.70],
    [0.10, 0.45, 0.45],
    [0.45, 0.10, 0.45],
    [0.45, 0.45, 0.10],
];

/// One painted ground layer of a tile (`[groundtex]`): its alpha mask, rows running north.
struct Paint {
    w: usize,
    h: usize,
    alpha: Vec<u8>,
    material: u32,
    /// The layer's place in `global.cfg`: a later layer is painted over an earlier one.
    layer: usize,
}

impl Paint {
    /// The mask at (x, y) metres into its tile, bilinearly as the ground shader reads it.
    fn at(&self, x: f64, y: f64) -> f32 {
        let ts = tile_size();
        let fx = (x / ts * self.w as f64 - 0.5).clamp(0.0, self.w as f64 - 1.0);
        let fy = (y / ts * self.h as f64 - 0.5).clamp(0.0, self.h as f64 - 1.0);
        let (i0, j0) = (fx.floor() as usize, fy.floor() as usize);
        let (i1, j1) = ((i0 + 1).min(self.w - 1), (j0 + 1).min(self.h - 1));
        let (tx, ty) = ((fx - i0 as f64) as f32, (fy - j0 as f64) as f32);
        let a = |i: usize, j: usize| self.alpha[j * self.w + i] as f32 / 255.0;
        let top = a(i0, j0) * (1.0 - tx) + a(i1, j0) * tx;
        let bottom = a(i0, j1) * (1.0 - tx) + a(i1, j1) * tx;
        top * (1.0 - ty) + bottom * ty
    }

    /// `at(x, y) >= 0.5`, answered from the four texels where they agree: all below half
    /// (128) or all above it
    fn covers(&self, x: f64, y: f64) -> bool {
        let ts = tile_size();
        let fx = (x / ts * self.w as f64 - 0.5).clamp(0.0, self.w as f64 - 1.0);
        let fy = (y / ts * self.h as f64 - 0.5).clamp(0.0, self.h as f64 - 1.0);
        let (i0, j0) = (fx.floor() as usize, fy.floor() as usize);
        let (i1, j1) = ((i0 + 1).min(self.w - 1), (j0 + 1).min(self.h - 1));
        let t = [
            self.alpha[j0 * self.w + i0],
            self.alpha[j0 * self.w + i1],
            self.alpha[j1 * self.w + i0],
            self.alpha[j1 * self.w + i1],
        ];
        if t.iter().all(|&a| a < 128) {
            return false;
        }
        if t.iter().all(|&a| a >= 128) {
            return true;
        }
        self.at(x, y) >= 0.5
    }

    /// Whether any texel the mask is read from between (x0, y0) and (x1, y1) metres into
    /// its tile is half painted or more: else `covers` is false all over there.
    fn any_within(&self, x0: f64, y0: f64, x1: f64, y1: f64) -> bool {
        let ts = tile_size();
        let ix = |x: f64| (x / ts * self.w as f64 - 0.5).clamp(0.0, self.w as f64 - 1.0).floor() as usize;
        let iy = |y: f64| (y / ts * self.h as f64 - 0.5).clamp(0.0, self.h as f64 - 1.0).floor() as usize;
        // (a texel to spare on each side, against rounding)
        let (i0, i1) = (ix(x0).saturating_sub(1), (ix(x1) + 2).min(self.w - 1));
        let (j0, j1) = (iy(y0).saturating_sub(1), (iy(y1) + 2).min(self.h - 1));
        (j0..=j1).any(|j| self.alpha[j * self.w + i0..=j * self.w + i1].iter().any(|&a| a >= 128))
    }
}

/// The painted ground (`[groundtex]` layers) as a chunk's raster takes it: the tiles' layers
/// and the class of each material
struct PaintedGround<'a> {
    paints: &'a HashMap<(i32, i32), Vec<Paint>>,
    class: &'a [u8],
    ground: &'a (dyn Fn(DVec3) -> f64 + Sync),
}

impl PaintedGround<'_> {
    /// Each cell of chunk `key`'s raster that painted ground of a known class shows on top:
    /// `put(cell, class, height)`
    fn cells(&self, key: (i32, i32), mut put: impl FnMut(usize, u8, f32)) {
        let ts = tile_size();
        let o = SurfaceMap::chunk_origin(key);
        let centre = |i: usize| (i as f64 - MARGIN as f64 + 0.5) * CELL;
        let (lo, hi) = (o + DVec2::splat(centre(0)), o + DVec2::splat(centre(raster::W - 1)));
        // the layers of the tiles under the chunk with paint anywhere near it, in order
        let mut tiles: Vec<((i32, i32), Vec<&Paint>)> = Vec::new();
        for ty in (lo.y / ts).floor() as i32..=(hi.y / ts).floor() as i32 {
            for tx in (lo.x / ts).floor() as i32..=(hi.x / ts).floor() as i32 {
                let Some(layers) = self.paints.get(&(tx, ty)) else {
                    continue;
                };
                let (bx, by) = (tx as f64 * ts, ty as f64 * ts);
                let (x0, y0) = ((lo.x - bx).max(0.0), (lo.y - by).max(0.0));
                let (x1, y1) = ((hi.x - bx).min(ts), (hi.y - by).min(ts));
                let near: Vec<&Paint> = layers.iter().filter(|l| l.any_within(x0, y0, x1, y1)).collect();
                if !near.is_empty() {
                    tiles.push(((tx, ty), near));
                }
            }
        }
        if tiles.is_empty() {
            return;
        }
        for j in 0..raster::W {
            for i in 0..raster::W {
                let p = o + DVec2::new(
                    (i as f64 - MARGIN as f64 + 0.5) * CELL,
                    (j as f64 - MARGIN as f64 + 0.5) * CELL,
                );
                let (tx, ty) = ((p.x / ts).floor() as i32, (p.y / ts).floor() as i32);
                let Some((_, layers)) = tiles.iter().find(|t| t.0 == (tx, ty)) else {
                    continue;
                };
                let (lx, ly) = (p.x - tx as f64 * ts, p.y - ty as f64 * ts);
                // the topmost layer painted here
                let Some(top) = layers.iter().find(|l| l.covers(lx, ly)) else {
                    continue;
                };
                let c = self.class.get(top.material as usize).copied().unwrap_or(0);
                if c != 0 {
                    put(j * raster::W + i, c, ((self.ground)(p.extend(0.0)) - 0.3) as f32);
                }
            }
        }
    }
}

struct Gathered {
    stats: Stats,
    faces: HashMap<(i32, i32), Vec<Face>>,
    paints: Vec<((i32, i32), Paint)>,
    /// Roofs and tree crowns, by chunk.
    shapes: HashMap<(i32, i32), Vec<Shape>>,
}

/// What a chunk shows besides its ground, in metres from the chunk's corner.
#[derive(Clone, Copy, Debug)]
pub(super) enum Shape {
    /// A triangle of a building's roof.
    Roof([[f32; 2]; 3]),
    /// A tree's crown: its middle and radius.
    Crown([f32; 2], f32),
    /// A triangle of ground under a tile's water.
    Water([[f32; 2]; 3]),
}

/// The height of a tile's water at `x`, `y` (tile-local m) from its corners (0, 0), (t, 0),
/// (0, t), (t, t): two triangles, as the game lays the quad (`scene::place_step`).
fn water_at(h: [f32; 4], x: f32, y: f32) -> f32 {
    let t = tile_size() as f32;
    if x + y <= t {
        h[0] + (h[1] - h[0]) * x / t + (h[2] - h[0]) * y / t
    } else {
        h[3] + (h[2] - h[3]) * (t - x) / t + (h[1] - h[3]) * (t - y) / t
    }
}

/// The ground under a tile's water (`h`: its corners), as triangles in tile-local metres:
/// each of the terrain's triangles cut where it rises out of the water.
pub(super) fn under_water(terrain: &Terrain, h: [f32; 4]) -> Vec<[Vec2; 3]> {
    let n = terrain.cells;
    let step = tile_size() as f32 / n as f32;
    let mut out = Vec::new();
    let at = |i: usize, j: usize| {
        let p = Vec2::new(i as f32 * step, j as f32 * step);
        (p, water_at(h, p.x, p.y) - terrain.height_at(i, j))
    };
    for j in 0..n {
        for i in 0..n {
            // (split along the diagonal from (i, j) to (i + 1, j + 1), as the ground is drawn)
            for tri in [
                [at(i, j), at(i + 1, j), at(i + 1, j + 1)],
                [at(i, j), at(i + 1, j + 1), at(i, j + 1)],
            ] {
                // the part where the water is above the ground (depth > 0)
                let mut poly: Vec<Vec2> = Vec::with_capacity(4);
                for k in 0..3 {
                    let (a, da) = tri[k];
                    let (b, db) = tri[(k + 1) % 3];
                    if da > 0.0 {
                        poly.push(a);
                    }
                    if (da > 0.0) != (db > 0.0) {
                        poly.push(a + (b - a) * (da / (da - db)));
                    }
                }
                for k in 1..poly.len().saturating_sub(1) {
                    out.push([poly[0], poly[k], poly[k + 1]]);
                }
            }
        }
    }
    out
}

/// How high above the ground an object's floor may lie and still be on the ground (m).
const RAISED: f64 = 1.5;

/// How far above or below a raised floor that paths run over another one may lie and be
/// part of the same deck (m): its pavements, its median, its slope.
const DECK: f64 = 1.0;

pub(super) fn keep_floors(piece: &mut Vec<PieceFace>, streets: bool, ev: &Evidence) -> bool {
    if streets {
        return false;
    }
    let mid = |f: &PieceFace| (f.w[0] + f.w[1] + f.w[2]) / 3.0;
    let mut carried = false;
    let mut decks: Vec<f64> = Vec::new();
    for f in piece.iter().filter(|f| f.above > RAISED) {
        let on = ev.at(mid(f));
        carried |= on & (CAR | WALK | RAIL) != 0;
        if on & (CAR | WALK) != 0 {
            decks.push(mid(f).z);
        }
    }
    piece.retain(|f| f.above <= RAISED || decks.iter().any(|z| (mid(f).z - z).abs() <= DECK));
    carried
}

/// The radius of a tree's crown (m): a billboard as the map sizes the tree (`extra`: its
/// texture, height and ratio, as `scene::place` reads them), else the type's own.
fn crown_radius(c: Crown, extra: &[String]) -> f32 {
    match c {
        Crown::Billboard(h, r) => {
            let num = |k: usize, or: f32| {
                extra
                    .get(k)
                    .map(|s| ::legacy_config::parse_f64(s) as f32)
                    .filter(|v| *v > 0.0)
                    .unwrap_or(or)
            };
            (num(1, h) * num(2, r) * 0.5 * CROWN_SHARE).clamp(0.5, 12.0)
        }
        Crown::Mesh(r) => r,
    }
}

/// A face of a piece (a spline, an object) on its way into [`Gathered`]: its corners, texture
/// coordinates, material and texture, whether it lies level with a street beside it, and how
/// high above the ground its middle is (m).
pub(super) struct PieceFace {
    pub(super) w: [DVec3; 3],
    pub(super) uv: [[f32; 2]; 3],
    pub(super) ids: (u32, u32),
    pub(super) beside: bool,
    pub(super) above: f64,
}

/// How far apart in height (m) two of a piece's faces may lie and still be one surface
/// (see [`piece_levels`]): a deck's slope and camber, not a deck over the ground
const BAND_GAP: f64 = 1.5;

pub(super) fn piece_levels(faces: &[PieceFace]) -> Vec<i8> {
    let mut order: Vec<usize> = (0..faces.len()).collect();
    order.sort_by(|&a, &b| faces[a].above.total_cmp(&faces[b].above));
    let area = |f: &PieceFace| (f.w[1] - f.w[0]).truncate().perp_dot((f.w[2] - f.w[0]).truncate()).abs();
    let mut levels = vec![0i8; faces.len()];
    let mut start = 0;
    while start < order.len() {
        let mut end = start + 1;
        while end < order.len() && faces[order[end]].above - faces[order[end - 1]].above <= BAND_GAP {
            end += 1;
        }
        let band = &order[start..end];
        let (mut sum, mut total) = (0.0, 0.0);
        for &k in band {
            let a = area(&faces[k]);
            sum += faces[k].above * a;
            total += a;
        }
        let above = if total > 0.0 { sum / total } else { faces[band[0]].above };
        let level = if above > LEVEL_GAP {
            1
        } else if above < -LEVEL_GAP {
            -1
        } else {
            0
        };
        for &k in band {
            levels[k] = level;
        }
        start = end;
    }
    levels
}

impl Gathered {
    /// File a shape covering `lo`..`hi` (world) under every chunk it reaches, in its frame.
    fn add_shape(&mut self, lo: DVec2, hi: DVec2, shape: impl Fn(DVec2) -> Shape) {
        let m = MARGIN as f64 * CELL;
        let (x0, y0) = SurfaceMap::chunk_key(lo - DVec2::splat(m));
        let (x1, y1) = SurfaceMap::chunk_key(hi + DVec2::splat(m));
        for y in y0..=y1 {
            for x in x0..=x1 {
                let o = SurfaceMap::chunk_origin((x, y));
                self.shapes.entry((x, y)).or_default().push(shape(o));
            }
        }
    }

    /// An object's roof placed at `pos`, turned by `rot` (see [`roof_of`]).
    fn add_roof(&mut self, roof: &[[Vec2; 3]], pos: DVec3, rot: &Mat4) {
        for t in roof {
            let w = t.map(|v| pos.truncate() + rot.transform_vector3(v.extend(0.0)).truncate().as_dvec2());
            let lo = w[0].min(w[1]).min(w[2]);
            let hi = w[0].max(w[1]).max(w[2]);
            self.add_shape(lo, hi, |o| Shape::Roof(w.map(|p| (p - o).as_vec2().to_array())));
        }
    }

    /// The ground under a tile's water, from the tile's corner `origin` (see [`under_water`]).
    fn add_water(&mut self, tris: &[[Vec2; 3]], origin: DVec2) {
        for t in tris {
            let w = t.map(|v| origin + v.as_dvec2());
            let lo = w[0].min(w[1]).min(w[2]);
            let hi = w[0].max(w[1]).max(w[2]);
            self.add_shape(lo, hi, |o| Shape::Water(w.map(|p| (p - o).as_vec2().to_array())));
        }
    }

    /// A tree's crown of radius `r` at `at`.
    fn add_crown(&mut self, at: DVec2, r: f32) {
        let d = DVec2::splat(r as f64);
        self.add_shape(at - d, at + d, |o| Shape::Crown((at - o).as_vec2().to_array(), r));
    }

    /// Add the faces of one piece - a spline, an object - each surface of it on one level
    /// (see [`piece_levels`]). Taken face by face, a bridge deck some 4 m over a canal's bed
    /// (Berlin's Schlossbrücke) fell to either side of `LEVEL_GAP` triangle by triangle, both
    /// levels broke into shreds too small to draw, and the bridge showed its markings over
    /// nothing.
    fn add_piece(&mut self, faces: &mut Vec<PieceFace>, ev: &Evidence) {
        let levels = piece_levels(faces);
        for (f, level) in faces.drain(..).zip(levels) {
            self.add(f.w, f.uv, f.ids, level, ev, f.beside);
        }
    }

    /// Count a face for its material and file it under every chunk it reaches
    /// (`beside`: the face is part of a street spline, at the height of its street paths;
    /// `level`: of its surface in the piece it belongs to, see [`Gathered::add_piece`])
    fn add(
        &mut self,
        w: [DVec3; 3],
        uv: [[f32; 2]; 3],
        ids: (u32, u32),
        level: i8,
        ev: &Evidence,
        beside: bool,
    ) {
        let (material, texture) = ids;
        let area = (w[1] - w[0]).truncate().perp_dot((w[2] - w[0]).truncate()).abs() * 0.5;
        if area < 1e-4 {
            return;
        }
        let n = if area < 1.0 {
            1
        } else if area < 6.0 {
            4
        } else {
            7
        };
        let st = self.stats.entry(material).or_insert([0.0; 5]);
        st[0] += area;
        if beside {
            st[4] += area;
        }
        for b in &SPREAD[..n] {
            let p = w[0] * b[0] + w[1] * b[1] + w[2] * b[2];
            let bits = ev.at(p);
            let share = area / n as f64;
            if bits & CAR != 0 {
                st[1] += share;
            }
            if bits & WALK != 0 {
                st[2] += share;
            }
            if bits & RAIL != 0 {
                st[3] += share;
            }
        }
        let m = MARGIN as f64 * CELL;
        let lo = w[0].truncate().min(w[1].truncate()).min(w[2].truncate()) - DVec2::splat(m);
        let hi = w[0].truncate().max(w[1].truncate()).max(w[2].truncate()) + DVec2::splat(m);
        let (x0, y0) = SurfaceMap::chunk_key(lo);
        let (x1, y1) = SurfaceMap::chunk_key(hi);
        for y in y0..=y1 {
            for x in x0..=x1 {
                let o = SurfaceMap::chunk_origin((x, y));
                let v = w.map(|p| [(p.x - o.x) as f32, (p.y - o.y) as f32, p.z as f32]);
                self.faces.entry((x, y)).or_default().push(Face {
                    v,
                    uv,
                    material,
                    texture,
                    level,
                });
            }
        }
    }
}

/// How far beside the outermost street path of a spline a strip at its height still counts
/// as part of the carriageway (m): a cycle lane, a gutter.
const BESIDE_STREET: f32 = 4.5;
/// How much higher or lower than a street path a strip beside it may lie and still be on the
/// carriageway (m); a kerb is some 0.1 m or more.
const STREET_LEVEL: f32 = 0.05;

/// Whether every profile of a spline's texture `tex` lies at the height of its street paths
/// (`streets`: their place across and height) and not further beside them than a lane:
/// part of the carriageway, whatever is painted on it.
pub(super) fn level_with_streets(
    profiles: &[::scenery::SplineProfile],
    tex: usize,
    streets: &[(f32, f32)],
) -> bool {
    let Some(lo) = streets.iter().map(|s| s.0).reduce(f32::min) else {
        return false;
    };
    let hi = streets.iter().map(|s| s.0).fold(lo, f32::max);
    let mut points = profiles
        .iter()
        .filter(|p| p.texture == tex)
        .flat_map(|p| &p.points)
        .peekable();
    points.peek().is_some()
        && points.all(|q| {
            (lo - BESIDE_STREET..=hi + BESIDE_STREET).contains(&q.x)
                && streets.iter().any(|s| (q.z - s.1).abs() <= STREET_LEVEL)
        })
}

/// A pedestrian crossing painted on the carriageway, by its texture's name. (Not anything
/// "fussgaenger": a pedestrian zone or a footway is named so too, and is no road.)
fn crossing(name: &str) -> bool {
    ["crosswalk", "zebra", "ueberweg", "überweg", "uberweg"]
        .iter()
        .any(|w| name.contains(w))
}

/// A cycle lane or path, by its texture's name.
fn cycle(name: &str) -> bool {
    ["radweg", "fahrrad", "radfahr", "radstreifen", "schutzstreifen", "bike", "cycle"]
        .iter()
        .any(|w| name.contains(w))
}

/// Words in a texture's name that say what it shows (the evidence of the paths comes first).
fn named(name: &str) -> Option<Surface> {
    let has = |words: &[&str]| words.iter().any(|w| name.contains(w));
    if has(&[
        "gehweg", "fussweg", "fußweg", "sidewalk", "pavement", "chodnik", "radweg", "fahrrad",
        "bike", "cycle", "bordstein", "kerb", "curb", "bahnsteig", "platform", "plattform",
    ]) || name.starts_with("gw_")
        || name.contains("_side")
        || name.starts_with("side")
        || name.contains("str_side")
    {
        return Some(Surface::Footway);
    }
    if has(&["gleis", "schotter", "ballast", "schwelle", "sleeper", "track", "rail"]) {
        return Some(Surface::Track);
    }
    if has(&["gras", "grass", "rasen", "wiese", "hecke", "hedge", "lawn", "moos"]) {
        return Some(Surface::Green);
    }
    if has(&[
        "asph", "strasse", "straße", "road", "street", "fahrbahn", "kopfstein", "kopfst",
        "cobble", "pflaster", "beton", "concrete", "tarmac",
    ]) || name.starts_with("str_")
        || name.starts_with("str ")
    {
        return Some(Surface::Paved);
    }
    None
}

/// The surface a material shows: what the paths over it say, where enough of it was seen,
/// else what its `.cfg` and its name say.
pub(super) fn classify(name: &str, cfg: &::texture::TextureCfg, st: [f64; 5]) -> Option<Surface> {
    let [area, car, walk, rail, level] = st;
    if cfg.terrain_mapping || cfg.surface == 4 {
        return Some(Surface::Green);
    }
    let (car, walk, rail, level) = if area > 0.0 {
        (car / area, walk / area, rail / area, level / area)
    } else {
        (0.0, 0.0, 0.0, 0.0)
    };
    // (a crossing is the carriageway's, with its stripes on it, not a footway across it;
    // so is a cycle lane painted on it, its bicycles and lines read as the road's paint:
    // no car drives on it, but it lies at the height of the street beside it - a cycle
    // track up on the kerb does not)
    if crossing(name) || (cycle(name) && (car >= 0.5 || level >= 0.5)) {
        return Some(Surface::Road);
    }
    let by_name = named(name);
    if area >= 30.0 {
        if car >= 0.25 && car >= walk {
            return Some(Surface::Road);
        }
        // cars and people both cross it: a crossing over the carriageway
        if car >= 0.4 && walk >= 0.4 {
            return Some(Surface::Road);
        }
        if rail >= 0.4 && car < 0.1 {
            return Some(Surface::Track);
        }
        if walk >= 0.25 && by_name != Some(Surface::Green) {
            return Some(Surface::Footway);
        }
    }
    match by_name {
        // gravel no train runs on is a path or a yard, not a track bed
        Some(Surface::Track) if area >= 30.0 && rail < 0.15 => None,
        Some(s) => Some(s),
        None if cfg.puddles => Some(Surface::Paved),
        None if car >= 0.1 => Some(Surface::Paved),
        None => None,
    }
}

/// Build the map's surfaces (see the module) from its tiles and its paths (`lanes`: every
/// lane of the map, editor-only ones too).
pub fn build_surface_map(world: &World, lanes: &[Lane]) -> SurfaceMap {
    let mut all: HashMap<(i32, i32), Arc<Chunk>> = HashMap::new();
    let mut map = build_surface_stream(world, lanes, DVec2::ZERO, &mut |part| {
        all.extend(part);
    });
    map.chunks = all;
    map
}

pub fn build_surface_stream(
    world: &World,
    lanes: &[Lane],
    centre: DVec2,
    emit: &mut dyn FnMut(Vec<((i32, i32), Arc<Chunk>)>),
) -> SurfaceMap {
    let t0 = std::time::Instant::now();
    let ts = tile_size();
    let tiles = world.map_tiles();
    let deps = Deps::default();
    deps.add(&world.map_dir.join("global.cfg"));
    let terrains: HashMap<(i32, i32), Terrain> = tiles
        .par_iter()
        .map(|(_, tx, ty, path)| {
            (
                (*tx, *ty),
                Terrain::load(&tile_companion(path, ".terrain")).unwrap_or_else(|_| Terrain::flat()),
            )
        })
        .collect::<Vec<_>>()
        .into_iter()
        .collect();
    let ground = |p: DVec3| -> f64 {
        let (tx, ty) = ((p.x / ts).floor() as i32, (p.y / ts).floor() as i32);
        terrains
            .get(&(tx, ty))
            .map(|t| {
                let lx = (p.x - tx as f64 * ts).clamp(0.0, ts) as f32;
                let ly = (p.y - ty as f64 * ts).clamp(0.0, ts) as f32;
                t.sample(lx, ly) as f64
            })
            .unwrap_or(p.z)
    };
    let ev = Evidence::new(lanes);
    let materials = Mutex::new(Materials::default());
    // (each type read once: a thread wanting one being read waits for it)
    type Floors = Arc<std::sync::OnceLock<Option<Arc<ObjectFloors>>>>;
    let objects: Mutex<HashMap<String, Floors>> = Mutex::new(HashMap::new());
    let object_of = |file: &str| -> Option<Arc<ObjectFloors>> {
        let key = file.trim().to_ascii_lowercase().replace('\\', "/");
        let cell = objects.lock().entry(key).or_default().clone();
        cell.get_or_init(|| object_floors(world, file, &materials, &deps).map(Arc::new))
            .clone()
    };
    let parts: Vec<Gathered> = tiles
        .par_iter()
        .map(|(_, tx, ty, path)| {
            let mut g = Gathered {
                stats: HashMap::new(),
                faces: HashMap::new(),
                paints: Vec::new(),
                shapes: HashMap::new(),
            };
            deps.add(path);
            deps.add(&tile_companion(path, ".terrain"));
            if let Some(name) = path.file_name().map(|n| n.to_string_lossy().to_string()) {
                for c in world.chrono_dirs.read().iter() {
                    deps.add(&::mods::resolve_path(c, &name));
                }
                // (see `World::load_ground_paint`)
                for layer in 1..world.global.ground_textures.len() {
                    deps.add(&::mods::resolve_path(
                        &world.map_dir,
                        &format!("texture/map/{name}.{layer}.dds"),
                    ));
                }
            }
            let Some(tile) =
                crate::tiles::read_tile(path, &world.chrono_dirs.read(), world.map_detail())
            else {
                return g;
            };
            let origin2 = DVec2::new(*tx as f64 * ts, *ty as f64 * ts);
            let origin = origin2.extend(0.0);
            // the tile's water where the ground lies below it: its corners start at -5 m and
            // take the file's heights only when it gives one set (as `scene::stage` reads it)
            let water_file = std::path::PathBuf::from(format!("{}.water", path.display()));
            deps.add(&water_file);
            if tile.has_water {
                let h = ::legacy_config::vfs::read(&water_file)
                    .ok()
                    .map(|b| ::map::terrain::Water::parse(&b))
                    .filter(|w| w.count == 1 && w.values.len() >= 4)
                    .map(|w| [w.values[0], w.values[1], w.values[2], w.values[3]])
                    .unwrap_or([-5.0; 4]);
                if let Some(t) = terrains.get(&(*tx, *ty)) {
                    g.add_water(&under_water(t, h), origin2);
                }
            }
            // the ground painted with asphalt, cobbles, pavement or ballast
            let paint_dirs = [world.map_dir.clone(), world.root.clone()];
            for (layer, img) in world.load_ground_paint(path) {
                let Some(gt) = world.global.ground_textures.get(layer) else {
                    continue;
                };
                let (material, _) = Materials::id(&materials, &gt.texture, &paint_dirs);
                let (w, h) = (img.width as usize, img.height as usize);
                let alpha: Vec<u8> = img.rgba.chunks_exact(4).map(|p| p[3]).collect();
                if w == 0 || h == 0 || !alpha.iter().any(|&a| a >= 128) {
                    continue;
                }
                let (cw, ch) = (ts / w as f64, ts / h as f64);
                let st = g.stats.entry(material).or_insert([0.0; 5]);
                for j in 0..h {
                    for i in 0..w {
                        if alpha[j * w + i] < 128 {
                            continue;
                        }
                        let p2 = origin2 + DVec2::new((i as f64 + 0.5) * cw, (j as f64 + 0.5) * ch);
                        let bits = ev.at(p2.extend(ground(p2.extend(0.0))));
                        let a = cw * ch;
                        st[0] += a;
                        for (bit, k) in [(CAR, 1), (WALK, 2), (RAIL, 3)] {
                            if bits & bit != 0 {
                                st[k] += a;
                            }
                        }
                    }
                }
                g.paints.push((
                    (*tx, *ty),
                    Paint {
                        w,
                        h,
                        alpha,
                        material,
                        layer,
                    },
                ));
            }
            for sp in tile
                .splines
                .iter()
                .filter(|s| !s.deleted && !s.file.trim().is_empty())
            {
                deps.add(&::mods::resolve_path(&world.root, &sp.file));
                let Some(st) = world.spline_type(&sp.file) else {
                    continue;
                };
                if st.def.only_editor || st.def.profiles.is_empty() {
                    continue;
                }
                let curve = ::geometry::SplineCurve::from_map(sp, origin2);
                let mesh = ::geometry::build_spline_mesh(&st.def, &curve, sp.mirror, origin);
                let dirs = texture_dirs(&world.root, &st.dir);
                let mut piece = Vec::new();
                let streets: Vec<(f32, f32)> = st
                    .def
                    .paths
                    .iter()
                    .filter(|p| p.kind == 0)
                    .map(|p| (p.start[0], p.start[2]))
                    .collect();
                for &(first, count, slot) in &mesh.ranges {
                    let Some(tex) = st.def.textures.get(slot as usize) else {
                        continue;
                    };
                    let beside = level_with_streets(&st.def.profiles, slot as usize, &streets);
                    let mut id = None;
                    for t in mesh.indices[first as usize..(first + count) as usize].chunks_exact(3) {
                        let p = [
                            mesh.positions[t[0] as usize],
                            mesh.positions[t[1] as usize],
                            mesh.positions[t[2] as usize],
                        ];
                        let n = (p[1] - p[0]).cross(p[2] - p[0]);
                        let len = n.length();
                        // (the winding was turned for drawing: an upward face is wound
                        // clockwise seen from above now)
                        if len < 1e-6 || -n.z / len < UPRIGHT {
                            continue;
                        }
                        let id =
                            *id.get_or_insert_with(|| Materials::id(&materials, &tex.file, &dirs));
                        let w = p.map(|v| origin + v.as_dvec3());
                        let mid = (w[0] + w[1] + w[2]) / 3.0;
                        let uv = [t[0], t[1], t[2]].map(|k| mesh.uvs[k as usize].to_array());
                        piece.push(PieceFace {
                            w,
                            uv,
                            ids: id,
                            beside,
                            above: mid.z - ground(mid),
                        });
                    }
                }
                g.add_piece(&mut piece, &ev);
            }
            for o in &tile.objects {
                if o.file.trim().is_empty() {
                    continue;
                }
                let Some(floors) = object_of(&o.file) else {
                    continue;
                };
                let (x, y) = (origin2.x + o.pos[0], origin2.y + o.pos[1]);
                let foot = ground(DVec3::new(x, y, 0.0));
                let pos = DVec3::new(
                    x,
                    y,
                    if floors.absolute { o.pos[2] } else { o.pos[2] + foot },
                );
                let rot: Mat4 = ::geometry::object_rotation(::geometry::map_rotation(o.rot));
                let mut piece = Vec::new();
                for (p, uv, id) in &floors.faces {
                    let w = p.map(|v| pos + rot.transform_point3(v).as_dvec3());
                    let n = (w[1] - w[0]).cross(w[2] - w[0]);
                    if n.length() < 1e-9 || (n.z / n.length()).abs() < UPRIGHT as f64 {
                        continue;
                    }
                    let mid = (w[0] + w[1] + w[2]) / 3.0;
                    let under = ground(mid);
                    piece.push(PieceFace {
                        w,
                        uv: *uv,
                        ids: *id,
                        beside: false,
                        above: mid.z - under,
                    });
                }
                let deck = keep_floors(&mut piece, floors.streets, &ev);
                g.add_piece(&mut piece, &ev);
                if !deck && !floors.roof.is_empty() {
                    g.add_roof(&floors.roof, pos, &rot);
                }
                if let Some(c) = floors.crown {
                    g.add_crown(pos.truncate(), crown_radius(c, &o.extra));
                }
            }
            // the rows hung on splines: arrows on the road, parking bays, kerb pieces
            for a in &tile.spline_attachments {
                if a.file.trim().is_empty() {
                    continue;
                }
                let Some(floors) = object_of(&a.file) else {
                    continue;
                };
                let mut piece = Vec::new();
                for (_, ro) in crate::tiles::tile_row_objects(a, &tile.splines, origin2, None) {
                    let (pos, rot) = (ro.pose.pos, ro.pose.rot);
                    for (p, uv, id) in &floors.faces {
                        let w = p.map(|v| pos + rot.transform_point3(v).as_dvec3());
                        let n = (w[1] - w[0]).cross(w[2] - w[0]);
                        if n.length() < 1e-9 || (n.z / n.length()).abs() < UPRIGHT as f64 {
                            continue;
                        }
                        let mid = (w[0] + w[1] + w[2]) / 3.0;
                        let under = ground(mid);
                        piece.push(PieceFace {
                            w,
                            uv: *uv,
                            ids: *id,
                            beside: false,
                            above: mid.z - under,
                        });
                    }
                    let deck = keep_floors(&mut piece, floors.streets, &ev);
                    g.add_piece(&mut piece, &ev);
                    // (a row of houses along a street: on many maps most of the buildings
                    // are placed so - 1427 of Berlin 156's 1847)
                    if !deck && !floors.roof.is_empty() {
                        g.add_roof(&floors.roof, pos, &rot);
                    }
                    // (a row of trees along a road: each the middle of its type's ranges)
                    if let Some(c) = floors.crown {
                        g.add_crown(pos.truncate(), crown_radius(c, &[]));
                    }
                }
            }
            g
        })
        .collect();
    let mut stats: Stats = HashMap::new();
    let mut faces: HashMap<(i32, i32), Vec<Face>> = HashMap::new();
    let mut shapes: HashMap<(i32, i32), Vec<Shape>> = HashMap::new();
    let mut paints: HashMap<(i32, i32), Vec<Paint>> = HashMap::new();
    for g in parts {
        for (k, p) in g.paints {
            paints.entry(k).or_default().push(p);
        }
        for (k, v) in g.stats {
            let e = stats.entry(k).or_insert([0.0; 5]);
            for i in 0..5 {
                e[i] += v[i];
            }
        }
        for (k, mut v) in g.faces {
            faces.entry(k).or_default().append(&mut v);
        }
        for (k, mut v) in g.shapes {
            shapes.entry(k).or_default().append(&mut v);
        }
    }
    let (names, sources, textures) = materials.into_inner().settle();
    for s in &sources {
        let refs: Vec<&Path> = s.dirs.iter().map(|d| d.as_path()).collect();
        if let Some(p) = ::texture::find_texture(&s.file, &refs) {
            if let Some(c) = ::texture::cfg_path(&s.file, &p) {
                deps.add(&c);
            }
            deps.add(&p);
        }
    }
    let mut infos = Vec::with_capacity(names.len());
    let mut class = Vec::with_capacity(names.len());
    for (i, name) in names.iter().enumerate() {
        let st = stats.get(&(i as u32)).copied().unwrap_or([0.0; 5]);
        let s = classify(name, &sources[i].cfg, st);
        class.push(s.map(|s| s as u8).unwrap_or(0));
        let a = st[0].max(1e-9);
        infos.push(MaterialInfo {
            name: name.clone(),
            surface: s,
            area: st[0],
            car: st[1] / a,
            walk: st[2] / a,
            rail: st[3] / a,
        });
    }
    let t_gather = t0.elapsed().as_secs_f64();
    let mut files: Vec<Option<std::path::PathBuf>> = Vec::with_capacity(textures.len());
    for (file, dirs, material) in &textures {
        let i = *material as usize;
        let road = matches!(
            Surface::from_u8(class[i]),
            Some(Surface::Road | Surface::Paved)
        );
        // the shining heads of tram rails set in the road are not paint
        let name = &infos[i].name;
        let rails = infos[i].rail >= 0.25
            || ["rail", "gleis", "strab", "schiene", "tram"].iter().any(|w| name.contains(w));
        let found = (road && !rails)
            .then(|| {
                let refs: Vec<&Path> = dirs.iter().map(|d| d.as_path()).collect();
                ::texture::find_texture(file, &refs)
            })
            .flatten();
        if let Some(p) = &found {
            deps.add(p);
        }
        files.push(found);
    }
    let mut unique: Vec<&std::path::PathBuf> = files.iter().flatten().collect();
    unique.sort();
    unique.dedup();
    let decoded: HashMap<&std::path::PathBuf, Arc<raster::PaintMask>> = unique
        .into_par_iter()
        .filter_map(|p| {
            let img = ::texture::decode_file(p).ok()?;
            let m = raster::PaintMask::from_rgba(
                img.width as usize,
                img.height as usize,
                &img.rgba,
                img.has_alpha,
            )?;
            Some((p, Arc::new(m)))
        })
        .collect::<Vec<_>>()
        .into_iter()
        .collect();
    // (paved ground half light all over hundreds of square metres is tiled, not painted: a
    // stop line's texture is as light, but on a strip of a few)
    let paint_masks: Vec<Option<Arc<raster::PaintMask>>> = files
        .iter()
        .zip(&textures)
        .map(|(f, (_, _, material))| {
            let m = decoded.get(f.as_ref()?)?;
            let i = *material as usize;
            let tiled = class[i] == Surface::Paved as u8 && infos[i].area > 300.0 && m.share() > 0.4;
            (!tiled).then(|| m.clone())
        })
        .collect();
    let with_paint = paint_masks.iter().filter(|m| m.is_some()).count();
    for v in paints.values_mut() {
        v.sort_by_key(|p| std::cmp::Reverse(p.layer));
    }
    // the chunks of painted tiles too, where nothing else may lie
    for &(tx, ty) in paints.keys() {
        let lo = SurfaceMap::chunk_key(DVec2::new(tx as f64 * ts, ty as f64 * ts));
        let hi = SurfaceMap::chunk_key(DVec2::new(
            (tx + 1) as f64 * ts - 0.01,
            (ty + 1) as f64 * ts - 0.01,
        ));
        for y in lo.1..=hi.1 {
            for x in lo.0..=hi.0 {
                faces.entry((x, y)).or_default();
            }
        }
    }
    let painted = PaintedGround {
        paints: &paints,
        class: &class,
        ground: &ground,
    };
    for k in shapes.keys() {
        faces.entry(*k).or_default();
    }
    let mut todo: Vec<((i32, i32), Vec<Face>)> = faces.into_iter().collect();
    let dist = |k: (i32, i32)| {
        (SurfaceMap::chunk_origin(k) + DVec2::splat(CHUNK * 0.5) - centre).length_squared()
    };
    todo.sort_by(|a, b| dist(a.0).total_cmp(&dist(b.0)));
    let (mut chunk_count, mut area_count) = (0usize, 0usize);
    let (mut done, mut batch) = (0usize, 16usize);
    while done < todo.len() {
        let end = (done + batch).min(todo.len());
        let part: Vec<((i32, i32), Arc<Chunk>)> = todo[done..end]
            .par_iter()
            .filter_map(|(k, faces)| {
                let here = shapes.get(k).map(|v| v.as_slice()).unwrap_or(&[]);
                let ch = build_chunk(*k, faces, here, &class, &paint_masks, &painted);
                (!ch.areas.is_empty()).then(|| (*k, Arc::new(ch)))
            })
            .collect();
        chunk_count += part.len();
        area_count += part.iter().map(|(_, c)| c.areas.len()).sum::<usize>();
        if !part.is_empty() {
            emit(part);
        }
        done = end;
        batch = (batch * 2).min(128);
    }
    let rails = lanes
        .iter()
        .filter(|l| l.kind == LaneKind::Rail && !l.invisible && !l.reversed && l.points.len() >= 2)
        .filter(|l| {
            let mid = l.points[l.points.len() / 2];
            mid.z > ground(mid) - LEVEL_GAP
        })
        .map(|l| l.points.clone())
        .collect();
    let mut by_area: Vec<&MaterialInfo> = infos.iter().collect();
    by_area.sort_by(|a, b| b.area.total_cmp(&a.area));
    for m in by_area.iter().take(40) {
        log::info!(
            "navigator surfaces: {:<32} {:>9.0} m² streets {:>3.0}% walks {:>3.0}% rails {:>3.0}% -> {:?}",
            m.name,
            m.area,
            m.car * 100.0,
            m.walk * 100.0,
            m.rail * 100.0,
            m.surface
        );
    }
    log::info!(
        "navigator surfaces: {} materials ({} textures with road paint), {} object types, {} chunks, {} areas, {:.1} s ({:.1} s gathering)",
        infos.len(),
        with_paint,
        objects.lock().len(),
        chunk_count,
        area_count,
        t0.elapsed().as_secs_f64(),
        t_gather
    );
    SurfaceMap {
        chunks: HashMap::new(),
        read: deps.into_vec(),
        rails,
        materials: infos,
    }
}

/// The edges of a chunk's faces of some surfaces, to move outline points onto.
struct Edges {
    segs: Vec<([f32; 2], [f32; 2])>,
    grid: HashMap<(i32, i32), Vec<u32>>,
}

/// How far an outline point may be moved onto a face's edge (m): the raster's error and a
/// little more.
const SNAP: f32 = 0.3;

impl Edges {
    fn new<'a>(faces: impl Iterator<Item = &'a Face>) -> Edges {
        let mut e = Edges {
            segs: Vec::new(),
            grid: HashMap::new(),
        };
        for f in faces {
            for k in 0..3 {
                let (a, b) = (f.v[k], f.v[(k + 1) % 3]);
                let (a, b) = ([a[0], a[1]], [b[0], b[1]]);
                if (a[0] - b[0]).abs() + (a[1] - b[1]).abs() < 1e-4 {
                    continue;
                }
                let i = e.segs.len() as u32;
                e.segs.push((a, b));
                let (x0, x1) = (a[0].min(b[0]) - SNAP, a[0].max(b[0]) + SNAP);
                let (y0, y1) = (a[1].min(b[1]) - SNAP, a[1].max(b[1]) + SNAP);
                for y in y0.floor() as i32..=y1.floor() as i32 {
                    for x in x0.floor() as i32..=x1.floor() as i32 {
                        e.grid.entry((x, y)).or_default().push(i);
                    }
                }
            }
        }
        e
    }

    fn snap(&self, p: [f32; 2]) -> Option<[f32; 2]> {
        let ids = self.grid.get(&(p[0].floor() as i32, p[1].floor() as i32))?;
        let mut best: Option<(f32, [f32; 2])> = None;
        for &i in ids {
            let (a, b) = self.segs[i as usize];
            let (dx, dy) = (b[0] - a[0], b[1] - a[1]);
            let t = (((p[0] - a[0]) * dx + (p[1] - a[1]) * dy) / (dx * dx + dy * dy)).clamp(0.0, 1.0);
            let q = [a[0] + dx * t, a[1] + dy * t];
            let d = (q[0] - p[0]).hypot(q[1] - p[1]);
            if d <= SNAP && best.map(|b| d < b.0).unwrap_or(true) {
                best = Some((d, q));
            }
        }
        best.map(|b| b.1)
    }
}

/// Rasterise a chunk's faces level by level and trace each layer's areas.
fn build_chunk(
    key: (i32, i32),
    faces: &[Face],
    shapes: &[Shape],
    class: &[u8],
    paint: &[Option<Arc<raster::PaintMask>>],
    painted: &PaintedGround,
) -> Chunk {
    let mut chunk = Chunk::default();
    let to_cells = |v: [f32; 3]| {
        [
            v[0] / CELL as f32 + MARGIN as f32,
            v[1] / CELL as f32 + MARGIN as f32,
            v[2],
        ]
    };
    for level in [-1i8, 0, 1] {
        let mut r: Option<Raster> = None;
        for f in faces.iter().filter(|f| f.level == level) {
            let c = class.get(f.material as usize).copied().unwrap_or(0);
            if c == 0 {
                continue;
            }
            r.get_or_insert_with(|| Raster::new(raster::COARSE))
                .fill_tri(f.v.map(to_cells), c, None);
        }
        if level == 0 {
            // the painted ground, under anything laid on it
            painted.cells(key, |k, c, z| {
                r.get_or_insert_with(|| Raster::new(raster::COARSE)).put(k, c, z);
            });
        }
        let Some(r) = r else {
            continue;
        };
        // the true edges of the level's faces: a surface ends where its own faces end or
        // where another one lies over it (an island on the asphalt)
        let edges = Edges::new(faces.iter().filter(|f| {
            f.level == level && class.get(f.material as usize).copied().unwrap_or(0) != 0
        }));
        for layer in Layer::ALL {
            let mask: Vec<bool> = r
                .class
                .iter()
                .map(|&c| Surface::from_u8(c).is_some_and(|s| layer.covers(s)))
                .collect();
            if !mask.iter().any(|&b| b) {
                continue;
            }
            let mask = raster::close(&mask, raster::COARSE, layer.closing());
            let snap = |p: [f32; 2]| edges.snap(p);
            // (traced once for the fine and the coarse outline)
            let t = raster::traced(&mask, raster::COARSE, Some(&snap));
            let rings = raster::rings_of(&t, raster::COARSE, layer.tolerance());
            chunk.areas.extend(areas_of(rings, layer, level, layer.least()));
            if let Some((tol, least)) = layer.coarse() {
                let rings = raster::rings_of(&t, raster::COARSE, tol);
                chunk.coarse.extend(areas_of(rings, layer, level, least));
            }
        }
        // the paint on the carriageway, read from the textures on the fine grid
        let fine = raster::FINE;
        let mut paint_r: Option<Raster> = None;
        for f in faces.iter().filter(|f| f.level == level) {
            let c = class.get(f.material as usize).copied().unwrap_or(0);
            let Some(m) = paint.get(f.texture as usize).and_then(|m| m.as_deref()) else {
                continue;
            };
            let to_fine = |v: [f32; 3]| {
                [
                    v[0] / fine.cell as f32 + fine.margin as f32,
                    v[1] / fine.cell as f32 + fine.margin as f32,
                    v[2],
                ]
            };
            paint_r
                .get_or_insert_with(|| Raster::new(fine))
                .fill_tri(f.v.map(to_fine), c, Some((m, f.uv)));
        }
        if let Some(pr) = paint_r {
            let (fw, cw) = (fine.w(), raster::W);
            let scale = fine.n / raster::N;
            // (the fine grid's margin is the coarse one's, cell for cell: fine cell (i, j)
            // lies in coarse cell (i, j) / scale)
            const _: () = assert!(raster::FINE.margin == MARGIN * (raster::FINE.n / raster::N));
            // only where the carriageway is the topmost surface
            let road: Vec<bool> = r
                .class
                .iter()
                .map(|&c| matches!(Surface::from_u8(c), Some(Surface::Road | Surface::Paved)))
                .collect();
            let mut marks = vec![false; pr.mark.len()];
            for j in 0..fw {
                let row = &road[(j / scale) * cw..(j / scale + 1) * cw];
                for (i, m) in pr.mark[j * fw..(j + 1) * fw].iter().enumerate() {
                    if *m && row[i / scale] {
                        marks[j * fw + i] = true;
                    }
                }
            }
            if marks.iter().any(|&b| b) {
                let l = Layer::Marking;
                let marks = raster::close(&marks, fine, l.closing());
                chunk
                    .areas
                    .extend(areas_of(raster::outlines(&marks, fine, l.tolerance(), None), l, level, l.least()));
            }
        }
    }
    add_shapes(&mut chunk, shapes);
    chunk
}

/// The buildings and the trees of a chunk from its roofs and crowns: each shape filled into
/// the chunk's grid, the union traced into outlines - one simple block for a building of a
/// hundred roof faces, one canopy for an avenue of trees.
pub(super) fn add_shapes(chunk: &mut Chunk, shapes: &[Shape]) {
    if shapes.is_empty() {
        return;
    }
    let g = raster::COARSE;
    let w = g.w();
    let to_cells = |p: [f32; 2]| [p[0] / CELL as f32 + MARGIN as f32, p[1] / CELL as f32 + MARGIN as f32, 0.0];
    let mut roofs: Option<Raster> = None;
    let mut water: Option<Raster> = None;
    let mut crowns: Option<Vec<bool>> = None;
    for s in shapes {
        match *s {
            Shape::Roof(t) => roofs
                .get_or_insert_with(|| Raster::new(g))
                .fill_tri(t.map(to_cells), 1, None),
            Shape::Water(t) => water
                .get_or_insert_with(|| Raster::new(g))
                .fill_tri(t.map(to_cells), 1, None),
            Shape::Crown(c, r) => {
                let m = crowns.get_or_insert_with(|| vec![false; w * w]);
                let [cx, cy, _] = to_cells(c);
                let rc = r / CELL as f32;
                let (j0, j1) = ((cy - rc).floor().max(0.0) as usize, ((cy + rc).ceil() as usize).min(w));
                let (i0, i1) = ((cx - rc).floor().max(0.0) as usize, ((cx + rc).ceil() as usize).min(w));
                for j in j0..j1 {
                    for i in i0..i1 {
                        let (dx, dy) = (i as f32 + 0.5 - cx, j as f32 + 0.5 - cy);
                        if dx * dx + dy * dy <= rc * rc {
                            m[j * w + i] = true;
                        }
                    }
                }
            }
        }
    }
    let filled = |r: Raster| r.class.iter().map(|&c| c != 0).collect::<Vec<bool>>();
    let masks = [
        (Layer::Water, water.map(filled)),
        // (a railing, a fence or a lamp's arm along a roof is no building: what is narrower
        // than some 1.2 m is opened away)
        (Layer::Building, roofs.map(filled).map(|m| raster::open(&m, g, ROOF_THIN))),
        (Layer::Canopy, crowns),
    ];
    for (layer, mask) in masks {
        let Some(mask) = mask else { continue };
        let mask = raster::close(&mask, g, layer.closing());
        let t = raster::traced(&mask, g, None);
        // (a bank and a canopy are drawn soft, a building's walls straight)
        let rings_of = |tol: f32| match layer {
            Layer::Water => raster::rings_smoothed(&t, g, WATER_SMOOTH, tol),
            Layer::Canopy => raster::rings_smoothed(&t, g, CANOPY_SMOOTH, tol),
            _ => raster::rings_of(&t, g, tol),
        };
        chunk.areas.extend(areas_of(rings_of(layer.tolerance()), layer, 0, layer.least()));
        if let Some((tol, least)) = layer.coarse() {
            chunk.coarse.extend(areas_of(rings_of(tol), layer, 0, least));
        }
    }
}

/// How far along a river's bank (m) its outline is evened out: further than the ground's
/// 5 m triangles it was cut from, so their teeth go.
const WATER_SMOOTH: f32 = 6.0;
/// The same for the trees' canopy: the dents between the crowns of a row.
const CANOPY_SMOOTH: f32 = 3.0;

fn point_in_ring(p: [f32; 2], ring: &[[f32; 2]]) -> bool {
    let mut inside = false;
    let n = ring.len();
    let mut j = n - 1;
    for i in 0..n {
        let (a, b) = (ring[i], ring[j]);
        if (a[1] > p[1]) != (b[1] > p[1]) && p[0] < (b[0] - a[0]) * (p[1] - a[1]) / (b[1] - a[1]) + a[0] {
            inside = !inside;
        }
        j = i;
    }
    inside
}

/// The filled areas of a layer's outlines: each outline with the holes in it, triangulated.
fn areas_of(rings: Vec<raster::Ring>, layer: Layer, level: i8, least: f32) -> Vec<Area> {
    let (outers, holes): (Vec<_>, Vec<_>) = rings.into_iter().partition(|r| r.outer);
    let outers: Vec<_> = outers.into_iter().filter(|r| r.area >= least).collect();
    let mut inner: Vec<Vec<raster::Ring>> = vec![Vec::new(); outers.len()];
    for h in holes.into_iter().filter(|h| h.area >= least.min(4.0) * 0.6) {
        let best = outers
            .iter()
            .enumerate()
            .filter(|(_, o)| point_in_ring(h.pts[0], &o.pts))
            .min_by(|a, b| a.1.area.total_cmp(&b.1.area));
        if let Some((i, _)) = best {
            inner[i].push(h);
        }
    }
    let mut out = Vec::new();
    for (o, hs) in outers.iter().zip(inner) {
        let mut verts = o.pts.clone();
        let mut starts = Vec::new();
        for h in &hs {
            starts.push(verts.len());
            verts.extend_from_slice(&h.pts);
        }
        let tris = super::earcut::earcut(&verts, &starts);
        if tris.is_empty() {
            continue;
        }
        let mut edges = Vec::new();
        for ring in std::iter::once(o).chain(hs.iter()) {
            edges.extend(open_edges(&ring.pts));
        }
        out.push(Area {
            layer,
            level,
            verts,
            tris,
            edges,
        });
    }
    out
}

/// A ring's outline as polylines, broken where it runs along the chunk's border.
fn open_edges(pts: &[[f32; 2]]) -> Vec<Vec<[f32; 2]>> {
    let n = pts.len();
    let border: Vec<bool> = (0..n).map(|i| on_border(pts[i], pts[(i + 1) % n])).collect();
    let Some(start) = border.iter().position(|&b| b) else {
        let mut ring = pts.to_vec();
        ring.push(pts[0]);
        return vec![ring];
    };
    let mut out = Vec::new();
    let mut cur: Vec<[f32; 2]> = Vec::new();
    for k in 1..=n {
        let i = (start + k) % n;
        if border[i] {
            if cur.len() >= 2 {
                out.push(std::mem::take(&mut cur));
            }
            cur.clear();
        } else {
            if cur.is_empty() {
                cur.push(pts[i]);
            }
            cur.push(pts[(i + 1) % n]);
        }
    }
    out.retain(|l| l.len() >= 2);
    out
}
