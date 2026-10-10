use std::path::Path;
use std::sync::Arc;

use glam::{DVec2, DVec3, Mat4, Vec3};
use hashbrown::HashMap;
use parking_lot::Mutex;
use rayon::prelude::*;

use ::map::{Terrain, tile_size};
use ::simulation::traffic::{Lane, LaneKind};

use super::raster::{self, CELL, MARGIN, Raster};
use super::*;
use crate::scene::{World, texture_dirs, tile_companion};

/// A face of the map that faces up: corners in metres from its chunk's corner (heights as
/// they are), its material and its level.
struct Face {
    v: [[f32; 3]; 3],
    /// Texture coordinates at the corners.
    uv: [[f32; 2]; 3],
    material: u32,
    level: i8,
}

/// Higher above the ground than this is a bridge deck, lower a tunnel (m).
const LEVEL_GAP: f64 = 4.0;
/// A face is a floor when its normal is at most this far off the vertical (cos).
const UPRIGHT: f32 = 0.8;

#[derive(Default)]
struct Materials {
    ids: HashMap<String, u32>,
    names: Vec<String>,
    cfgs: Vec<::texture::TextureCfg>,
    /// The texture as first named, and where to look for it.
    sources: Vec<(String, Vec<std::path::PathBuf>)>,
}

impl Materials {
    /// The material of a texture as an asset names it: one per file name, whatever its
    /// folder or extension (a `.bmp` and a `.dds` of the same picture are one).
    fn id(this: &Mutex<Materials>, file: &str, dirs: &[std::path::PathBuf]) -> u32 {
        let base = file.replace('\\', "/");
        let base = base.rsplit('/').next().unwrap_or("").to_ascii_lowercase();
        let key = base
            .rsplit_once('.')
            .map(|(s, _)| s.to_string())
            .unwrap_or(base);
        if let Some(&id) = this.lock().ids.get(&key) {
            return id;
        }
        let refs: Vec<&Path> = dirs.iter().map(|d| d.as_path()).collect();
        let cfg = ::texture::find_texture(file, &refs)
            .and_then(|p| ::texture::cfg_path(file, &p))
            .map(|c| ::texture::TextureCfg::load(&c))
            .unwrap_or_default();
        let mut m = this.lock();
        if let Some(&id) = m.ids.get(&key) {
            return id;
        }
        let id = m.names.len() as u32;
        m.ids.insert(key.clone(), id);
        m.names.push(key);
        m.cfgs.push(cfg);
        m.sources.push((file.to_string(), dirs.to_vec()));
        id
    }
}

/// Where the paths run: segments of the street, pedestrian and rail paths in a grid.
struct Evidence {
    segs: Vec<(DVec3, DVec3, f32, u8)>,
    grid: HashMap<(i32, i32), Vec<u32>>,
}

const EV_CELL: f64 = 10.0;
const CAR: u8 = 1;
const WALK: u8 = 2;
const RAIL: u8 = 4;

impl Evidence {
    fn new(lanes: &[Lane]) -> Evidence {
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
    faces: Vec<([Vec3; 3], [[f32; 2]; 3], u32)>,
    /// It carries a street: its floors may be bridge decks high above the ground.
    streets: bool,
    /// `[absolute_height]`: placed at the height the map gives, not on the ground.
    absolute: bool,
}

/// An object type's faces that can be ground: facing up, near the object's foot unless it
/// carries a street (a bridge, a ramp).
fn object_floors(world: &World, file: &str, materials: &Mutex<Materials>) -> Option<ObjectFloors> {
    let path = ::legacy_config::resolve_path(&world.root, file);
    let sco = ::scenery::SceneryObject::load(&path).ok()?;
    if sco.only_editor {
        return None;
    }
    let sco_dir = path.parent()?.to_path_buf();
    let (model, model_dir) = match &sco.model_file {
        Some(m) => {
            let mp = ::legacy_config::resolve_path(&sco_dir, m);
            (::model::Model::load(&mp).ok()?, mp.parent()?.to_path_buf())
        }
        None => (sco.model.clone(), sco_dir.clone()),
    };
    if model.lods.is_empty() {
        return None;
    }
    let streets = sco.paths.iter().any(|p| p.kind == 0);
    let dirs = texture_dirs(&world.root, &model_dir);
    let mut faces = Vec::new();
    for md in model.lod_meshes(0) {
        let mp = ::legacy_config::resolve_path(&::legacy_config::resolve_path(&model_dir, "model"), &md.file);
        let mp = if ::legacy_config::vfs::is_file(&mp) {
            mp
        } else {
            ::legacy_config::resolve_path(&model_dir, &md.file)
        };
        let Ok(m) = ::legacy_o3d::load_mesh(&mp) else {
            continue;
        };
        let mesh = ::geometry::mesh_from_o3d(&m);
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
    (!faces.is_empty()).then_some(ObjectFloors {
        faces,
        streets,
        absolute: sco.absolute_height(),
    })
}

/// Material statistics: area and the areas under street, pedestrian and rail paths.
type Stats = HashMap<u32, [f64; 4]>;

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
}

struct Gathered {
    stats: Stats,
    faces: HashMap<(i32, i32), Vec<Face>>,
    paints: Vec<((i32, i32), Paint)>,
}

impl Gathered {
    /// Count a face for its material and file it under every chunk it reaches.
    fn add(&mut self, w: [DVec3; 3], uv: [[f32; 2]; 3], material: u32, ground: f64, ev: &Evidence) {
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
        let st = self.stats.entry(material).or_insert([0.0; 4]);
        st[0] += area;
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
        let mid = (w[0] + w[1] + w[2]) / 3.0;
        let level = if mid.z > ground + LEVEL_GAP {
            1
        } else if mid.z < ground - LEVEL_GAP {
            -1
        } else {
            0
        };
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
                    level,
                });
            }
        }
    }
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
pub(super) fn classify(name: &str, cfg: &::texture::TextureCfg, st: [f64; 4]) -> Option<Surface> {
    let [area, car, walk, rail] = st;
    if cfg.terrain_mapping || cfg.surface == 4 {
        return Some(Surface::Green);
    }
    let (car, walk, rail) = if area > 0.0 {
        (car / area, walk / area, rail / area)
    } else {
        (0.0, 0.0, 0.0)
    };
    let by_name = named(name);
    if area >= 30.0 {
        if car >= 0.25 && car >= walk {
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
    let t0 = std::time::Instant::now();
    let ts = tile_size();
    let tiles = world.map_tiles();
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
    let objects: Mutex<HashMap<String, Option<Arc<ObjectFloors>>>> = Mutex::new(HashMap::new());
    let object_of = |file: &str| -> Option<Arc<ObjectFloors>> {
        let key = file.trim().to_ascii_lowercase().replace('\\', "/");
        if let Some(v) = objects.lock().get(&key) {
            return v.clone();
        }
        let v = object_floors(world, file, &materials).map(Arc::new);
        objects.lock().insert(key, v.clone());
        v
    };
    let parts: Vec<Gathered> = tiles
        .par_iter()
        .map(|(_, tx, ty, path)| {
            let mut g = Gathered {
                stats: HashMap::new(),
                faces: HashMap::new(),
                paints: Vec::new(),
            };
            let Some(tile) =
                crate::tiles::read_tile(path, &world.chrono_dirs.read(), world.map_detail())
            else {
                return g;
            };
            let origin2 = DVec2::new(*tx as f64 * ts, *ty as f64 * ts);
            let origin = origin2.extend(0.0);
            // the ground painted with asphalt, cobbles, pavement or ballast
            let paint_dirs = [world.map_dir.clone(), world.root.clone()];
            for (layer, img) in world.load_ground_paint(path) {
                let Some(gt) = world.global.ground_textures.get(layer) else {
                    continue;
                };
                let material = Materials::id(&materials, &gt.texture, &paint_dirs);
                let (w, h) = (img.width as usize, img.height as usize);
                let alpha: Vec<u8> = img.rgba.chunks_exact(4).map(|p| p[3]).collect();
                if w == 0 || h == 0 || !alpha.iter().any(|&a| a >= 128) {
                    continue;
                }
                let (cw, ch) = (ts / w as f64, ts / h as f64);
                let st = g.stats.entry(material).or_insert([0.0; 4]);
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
                let Some(st) = world.spline_type(&sp.file) else {
                    continue;
                };
                if st.def.only_editor || st.def.profiles.is_empty() {
                    continue;
                }
                let curve = ::geometry::SplineCurve::from_map(sp, origin2);
                let mesh = ::geometry::build_spline_mesh(&st.def, &curve, sp.mirror, origin);
                let dirs = texture_dirs(&world.root, &st.dir);
                for &(first, count, slot) in &mesh.ranges {
                    let Some(tex) = st.def.textures.get(slot as usize) else {
                        continue;
                    };
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
                        g.add(w, uv, id, ground(mid), &ev);
                    }
                }
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
                for (p, uv, id) in &floors.faces {
                    let w = p.map(|v| pos + rot.transform_point3(v).as_dvec3());
                    let n = (w[1] - w[0]).cross(w[2] - w[0]);
                    if n.length() < 1e-9 || (n.z / n.length()).abs() < UPRIGHT as f64 {
                        continue;
                    }
                    let mid = (w[0] + w[1] + w[2]) / 3.0;
                    let under = ground(mid);
                    // a floor above the ground is a storey or a roof (an underpass below a
                    // bridge object is kept, however deep)
                    if !floors.streets && mid.z > under + 1.5 {
                        continue;
                    }
                    g.add(w, *uv, *id, under, &ev);
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
                        if !floors.streets && mid.z > under + 1.5 {
                            continue;
                        }
                        g.add(w, *uv, *id, under, &ev);
                    }
                }
            }
            g
        })
        .collect();
    let mut stats: Stats = HashMap::new();
    let mut faces: HashMap<(i32, i32), Vec<Face>> = HashMap::new();
    let mut paints: HashMap<(i32, i32), Vec<Paint>> = HashMap::new();
    for g in parts {
        for (k, p) in g.paints {
            paints.entry(k).or_default().push(p);
        }
        for (k, v) in g.stats {
            let e = stats.entry(k).or_insert([0.0; 4]);
            for i in 0..4 {
                e[i] += v[i];
            }
        }
        for (k, mut v) in g.faces {
            faces.entry(k).or_default().append(&mut v);
        }
    }
    let materials = materials.into_inner();
    let mut infos = Vec::with_capacity(materials.names.len());
    let mut class = Vec::with_capacity(materials.names.len());
    for (i, name) in materials.names.iter().enumerate() {
        let st = stats.get(&(i as u32)).copied().unwrap_or([0.0; 4]);
        let s = classify(name, &materials.cfgs[i], st);
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
    // the paint on the carriageways' textures: lines, arrows, stop lines, crossings
    let paint_masks: Vec<Option<raster::PaintMask>> = (0..materials.names.len())
        .into_par_iter()
        .map(|i| {
            let s = Surface::from_u8(class[i])?;
            if !matches!(s, Surface::Road | Surface::Paved) {
                return None;
            }
            // the shining heads of tram rails set in the road are not paint
            let name = &infos[i].name;
            if infos[i].rail >= 0.25
                || ["rail", "gleis", "strab", "schiene", "tram"].iter().any(|w| name.contains(w))
            {
                return None;
            }
            let (file, dirs) = &materials.sources[i];
            let refs: Vec<&Path> = dirs.iter().map(|d| d.as_path()).collect();
            let img = ::texture::decode_file(&::texture::find_texture(file, &refs)?).ok()?;
            raster::PaintMask::from_rgba(
                img.width as usize,
                img.height as usize,
                &img.rgba,
                img.has_alpha,
            )
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
    let painted = |p: DVec2| -> Option<(u8, f64)> {
        let (tx, ty) = ((p.x / ts).floor() as i32, (p.y / ts).floor() as i32);
        let layers = paints.get(&(tx, ty))?;
        let (lx, ly) = (p.x - tx as f64 * ts, p.y - ty as f64 * ts);
        // the topmost layer painted here
        let top = layers.iter().find(|l| l.at(lx, ly) >= 0.5)?;
        let c = class.get(top.material as usize).copied().unwrap_or(0);
        (c != 0).then(|| (c, ground(p.extend(0.0))))
    };
    let chunks: HashMap<(i32, i32), Chunk> = faces
        .into_iter()
        .collect::<Vec<_>>()
        .into_par_iter()
        .filter_map(|(k, faces)| {
            let ch = build_chunk(k, &faces, &class, &paint_masks, &painted);
            (!ch.areas.is_empty()).then_some((k, ch))
        })
        .collect::<Vec<_>>()
        .into_iter()
        .collect();
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
        "navigator surfaces: {} materials ({} with road paint), {} object types, {} chunks, {} areas, {:.1} s ({:.1} s gathering)",
        infos.len(),
        with_paint,
        objects.lock().len(),
        chunks.len(),
        chunks.values().map(|c| c.areas.len()).sum::<usize>(),
        t0.elapsed().as_secs_f64(),
        t_gather
    );
    SurfaceMap {
        chunks,
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
    class: &[u8],
    paint: &[Option<raster::PaintMask>],
    painted: &(dyn Fn(DVec2) -> Option<(u8, f64)> + Sync),
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
            let o = SurfaceMap::chunk_origin(key);
            for j in 0..raster::W {
                for i in 0..raster::W {
                    let p = o + DVec2::new(
                        (i as f64 - MARGIN as f64 + 0.5) * CELL,
                        (j as f64 - MARGIN as f64 + 0.5) * CELL,
                    );
                    if let Some((c, z)) = painted(p) {
                        r.get_or_insert_with(|| Raster::new(raster::COARSE))
                            .put(j * raster::W + i, c, (z - 0.3) as f32);
                    }
                }
            }
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
            chunk.areas.extend(areas_of(
                &mask,
                raster::COARSE,
                Some(&snap),
                layer,
                level,
                layer.tolerance(),
                layer.least(),
            ));
            if let Some((tol, least)) = layer.coarse() {
                chunk.coarse.extend(areas_of(
                    &mask,
                    raster::COARSE,
                    Some(&snap),
                    layer,
                    level,
                    tol,
                    least,
                ));
            }
        }
        // the paint on the carriageway, read from the textures on the fine grid
        let fine = raster::FINE;
        let mut paint_r: Option<Raster> = None;
        for f in faces.iter().filter(|f| f.level == level) {
            let c = class.get(f.material as usize).copied().unwrap_or(0);
            let Some(m) = paint.get(f.material as usize).and_then(|m| m.as_ref()) else {
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
            // only where the carriageway is the topmost surface
            let marks: Vec<bool> = (0..pr.mark.len())
                .map(|k| {
                    if !pr.mark[k] {
                        return false;
                    }
                    let (i, j) = ((k % fw) as isize, (k / fw) as isize);
                    let ci = (i - fine.margin as isize).div_euclid(scale as isize) + MARGIN as isize;
                    let cj = (j - fine.margin as isize).div_euclid(scale as isize) + MARGIN as isize;
                    if ci < 0 || cj < 0 || ci >= cw as isize || cj >= cw as isize {
                        return false;
                    }
                    matches!(
                        Surface::from_u8(r.class[cj as usize * cw + ci as usize]),
                        Some(Surface::Road | Surface::Paved)
                    )
                })
                .collect();
            if marks.iter().any(|&b| b) {
                let l = Layer::Marking;
                let marks = raster::close(&marks, fine, l.closing());
                chunk
                    .areas
                    .extend(areas_of(&marks, fine, None, l, level, l.tolerance(), l.least()));
            }
        }
    }
    chunk
}

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

/// The filled areas of a layer's mask: each outline with the holes in it, triangulated.
fn areas_of(
    mask: &[bool],
    g: raster::Grid,
    snap: Option<raster::Snap>,
    layer: Layer,
    level: i8,
    tol: f32,
    least: f32,
) -> Vec<Area> {
    let rings = raster::outlines(mask, g, tol, snap);
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
