use super::*;

/// A loaded scenery object type: model meshes + material descriptions.
pub struct ObjectType {
    pub sco: SceneryObject,
    /// The `[sound]` config's file, found the first time it is needed.
    pub sound_path: std::sync::OnceLock<Option<PathBuf>>,
    pub model: Model,
    pub model_dir: PathBuf,
    /// LOD 0 meshes (mesh data, o3d materials, model material overrides).
    pub meshes: Vec<(MeshData, Vec<::legacy_o3d::Material>, Vec<MaterialDef>)>,
    /// `.surf` maps for the first `[mesh]` of a `[surface]` object.
    pub surface_maps: Option<Arc<SurfFaces>>,
    /// `[visible] var value` per mesh, parallel to `meshes`.
    pub mesh_visible: Vec<Option<(String, f32)>>,
    /// Model mesh definition index and pivot per loaded mesh (parallel to `meshes`).
    pub mesh_def_index: Vec<usize>,
    pub mesh_pivots: Vec<Mat4>,
    /// `[isshadow]` per loaded mesh: a flat shadow blob drawn on the ground.
    pub mesh_shadow: Vec<bool>,
    /// `[shadow]` per loaded mesh: the meshes OMSI casts (stencil) shadows from.
    pub mesh_casts: Vec<bool>,
    /// Compiled scripts when the object is scripted or animated.
    pub program: Option<Arc<::legacy_script::Program>>,
    /// Further `[LOD]` levels: (min screen size, meshes), in model order after LOD 0.
    pub lower_lods: Vec<(
        f32,
        Vec<(MeshData, Vec<::legacy_o3d::Material>, Vec<MaterialDef>)>,
    )>,
    /// Min screen size of LOD 0 (0 = always).
    pub lod0_min: f32,
    /// Number of `[CTC]` paint schemes the object offers.
    pub paint_scheme_count: usize,
    /// Runtime scenery texture groups (`[CTC]` and `[texchanges]`), each selected by its own
    /// script variable and supplying one or more material texture replacements per choice.
    pub dynamic_textures: Vec<DynamicTextureGroup>,
    /// `[terrainhole]` meshes of the model: where they lie the ground is taken away.
    pub holes: Vec<MeshData>,
    /// `[crossing_heightdeformation]`: the mesh a crossing presses the terrain into, so
    /// the junction plate and the roads that meet it sit on one surface.
    pub deform: Option<MeshData>,
    /// `[collision_mesh]`: what vehicles actually hit (often much plainer than the model).
    pub collision: Option<MeshData>,
    /// What of the type stops the outside camera (decided on first use).
    pub camera: std::sync::OnceLock<crate::camera_arm::BlockerShape>,
    /// The collision mesh as the vehicles meet it (built on first use).
    pub collision_shape: std::sync::OnceLock<Arc<::simulation::collision::MeshShape>>,
    /// The visible meshes as boxes, for the shadows of the point lights (built on first use).
    pub light_shape: std::sync::OnceLock<Arc<::simulation::collision::MeshShape>>,
}

impl ObjectType {
    /// What stops a point light's rays: the opaque triangles of the LOD 0 meshes.
    pub fn light_occluder_shape(&self) -> Arc<::simulation::collision::MeshShape> {
        self.light_shape
            .get_or_init(|| {
                let mut tris: Vec<[glam::DVec3; 3]> = Vec::new();
                let mut total = 0usize;
                for (i, (m, mats, _)) in self.meshes.iter().enumerate() {
                    total += m.indices.len() / 3;
                    if self.mesh_shadow.get(i).copied().unwrap_or(false) {
                        continue;
                    }
                    let ranges: Vec<(u32, u32, u32)> = if m.ranges.is_empty() {
                        vec![(0, m.indices.len() as u32, 0)]
                    } else {
                        m.ranges.clone()
                    };
                    for (first, count, slot) in ranges {
                        if mats
                            .get(slot as usize)
                            .map_or(false, |x| x.diffuse[3] < 0.95)
                        {
                            continue;
                        }
                        let end = ((first + count) as usize).min(m.indices.len());
                        for t in m.indices[(first as usize).min(end)..end].chunks_exact(3) {
                            let (a, b, c) = (t[0] as usize, t[1] as usize, t[2] as usize);
                            if a < m.positions.len()
                                && b < m.positions.len()
                                && c < m.positions.len()
                            {
                                tris.push([
                                    m.positions[a].as_dvec3(),
                                    m.positions[b].as_dvec3(),
                                    m.positions[c].as_dvec3(),
                                ]);
                            }
                        }
                    }
                }
                if total > 40000 {
                    return Arc::new(Default::default());
                }
                Arc::new(::simulation::collision::MeshShape::from_triangles(
                    tris.into_iter(),
                    LOW_OBJECT as f64,
                ))
            })
            .clone()
    }
}

/// One scenery texture selector and its indexed replacement sets.
#[derive(Clone)]
pub struct DynamicTextureGroup {
    pub variable: String,
    /// Each replacement is (the material's default texture key, replacement file, folder).
    pub choices: Vec<Vec<(String, String, PathBuf)>>,
}

/// `reflexionN.bmp`: the texture drawn by reflection camera N of the vehicle.
pub(super) fn mirror_index(name: &str) -> Option<usize> {
    let n = name.trim().to_ascii_lowercase();
    let rest = n.strip_prefix("reflexion")?;
    let digits: String = rest.chars().take_while(|c| c.is_ascii_digit()).collect();
    digits.parse().ok()
}

pub(super) fn is_led_name(name: &str) -> bool {
    let n = name.to_ascii_lowercase();
    n.split(|c: char| !c.is_ascii_alphanumeric()).any(|t| {
        t == "led"
            || t.starts_with("ledmatrix")
            || t.starts_with("ledpanel")
            || t.starts_with("ledanzeige")
            || t.strip_prefix("led")
            .is_some_and(|r| !r.is_empty() && r.chars().all(|c| c.is_ascii_digit()))
    })
}

/// A light map that is white all over (the LED panels' `vmatrix_leer_led_LM.png`, one white
/// pixel): the surface is all its own light. A flipdot panel carries the same `\S:n` mask,
/// but its light map is a picture of the lamps over it (`vmatrix_leer_LM.bmp`).
pub(super) fn is_white_lightmap(rgba: &[u8]) -> bool {
    !rgba.is_empty()
        && rgba
        .chunks_exact(4)
        .all(|p| p[0] >= 242 && p[1] >= 242 && p[2] >= 242)
}

/// [`is_white_lightmap`] of the light map `name` (found in `dirs`, read once per file);
/// `None` when the file is not there.
pub(super) fn lightmap_is_white(name: &str, dirs: &[&Path]) -> Option<bool> {
    static WHITE: std::sync::OnceLock<Mutex<HashMap<PathBuf, bool>>> = std::sync::OnceLock::new();
    let path = ::texture::find_texture(name, dirs)?;
    let cache = WHITE.get_or_init(|| Mutex::new(HashMap::new()));
    if let Some(w) = cache.lock().get(&path) {
        return Some(*w);
    }
    let w = ::texture::decode_file(&path)
        .ok()
        .map(|i| is_white_lightmap(&i.rgba))?;
    cache.lock().insert(path, w);
    Some(w)
}

impl ObjectType {
    /// The model's own extents as a `[boundingbox]` would give them (width, length, height,
    /// centre x, y, z), for an object that has none.
    pub fn local_box(&self) -> Option<[f32; 6]> {
        let mut lo = glam::Vec3::splat(f32::MAX);
        let mut hi = glam::Vec3::splat(f32::MIN);
        for p in self.meshes.iter().flat_map(|m| m.0.positions.iter()) {
            lo = lo.min(*p);
            hi = hi.max(*p);
        }
        if lo.x > hi.x {
            return None;
        }
        let (s, c) = (hi - lo, (hi + lo) * 0.5);
        Some([s.x, s.y, s.z, c.x, c.y, c.z])
    }

    /// Bytes of the meshes the type keeps on the CPU.
    pub fn mesh_bytes(&self) -> usize {
        self.meshes
            .iter()
            .chain(self.lower_lods.iter().flat_map(|l| l.1.iter()))
            .map(|m| m.0.heap_bytes())
            .sum::<usize>()
            + self.holes.iter().map(|m| m.heap_bytes()).sum::<usize>()
            + self.deform.as_ref().map(|m| m.heap_bytes()).unwrap_or(0)
            + self.collision.as_ref().map(|m| m.heap_bytes()).unwrap_or(0)
    }

    /// The type's solid shape when it stops the outside camera.
    pub fn camera_shape(&self) -> Option<&crate::camera_arm::BlockerShape> {
        Some(
            self.camera
                .get_or_init(|| crate::camera_arm::classify(self)),
        )
            .filter(|s| s.blocks)
    }

    /// (definition, pivot) per loaded mesh, for the scenery script runtime.
    pub fn mesh_defs(&self) -> Vec<(&MeshDef, Mat4)> {
        self.mesh_def_index
            .iter()
            .zip(&self.mesh_pivots)
            .map(|(d, p)| (&self.model.meshes[*d], *p))
            .collect()
    }
}

/// A placed scenery object with a running script / animations.
pub struct ScriptedObject {
    pub ty: Arc<ObjectType>,
    pub pos: DVec3,
    pub xf: Mat4,
    /// Render instance per loaded mesh.
    pub instances: Vec<usize>,
    pub inst: ::simulation::scenery::SceneryInstance,
    /// Traffic light program of this object (crossings) or of its parent (lamps).
    pub controller: Option<usize>,
    pub light_index: usize,
    pub map_id: i64,
    pub variants: Vec<SceneryVariant>,
    /// `[sound]` config of the object, loaded when the listener comes near.
    pub sounds: Option<::audio::SoundSet>,
    /// The tile it belongs to (it goes when the tile is unloaded).
    pub tile: (i32, i32),
    /// `[varparent]`: the object whose data this one shows (a departure display's stop).
    pub var_parent: Option<i64>,
    /// `[texttexture]`s drawn from the script's string variables: (texture, state).
    pub texts: Vec<(TextureId, ::simulation::texttex::TextTextureState)>,
    /// The script asks for the buses due at its stop (`GetArrBus*`).
    pub arrivals: bool,
    /// `[htmltexture]` pages shown on the object: (script texture index, texture). The
    /// pages themselves are `inst.html_textures`.
    pub htmls: Vec<(usize, TextureId)>,
    /// Previous dynamic texture selection, to skip unchanged work.
    pub last_tex_selection: Option<Vec<usize>>,
    /// Dynamic material overrides by (instance, slot) from dynamic_texture_variant.
    pub dynamic_materials: HashMap<(usize, usize), Vec<MaterialId>>,
}

/// Where a ray lands on a page (`[htmltexture]`) of a scenery object: see
/// [`World::html_object_hit`].
#[derive(Clone, Copy, Debug)]
pub struct PageHit {
    /// Distance (m) along the ray.
    pub t: f32,
    pub map_id: i64,
    /// The page's script texture index.
    pub page: usize,
    /// 0..1 across the page, `v` down from the top.
    pub u: f32,
    pub v: f32,
}

/// What the timetable tells the scenery: the time of day, and the buses due at the stops
/// whose departure displays are near (see [`World::timetable_boards`]).
#[derive(Default)]
pub struct StopBoards {
    /// The simulation clock the boards were made at (None before a timetable or clock ran:
    /// the scenery scripts then keep their own).
    pub clock: Option<::simulation::SimClock>,
    /// Per bus stop (map object id): the buses due, soonest first, as (line, terminus,
    /// expected arrival in seconds of the day).
    pub by_stop: HashMap<i64, Vec<(String, String, f64)>>,
    /// The stops whose displays asked in the last scenery update.
    pub wanted: Vec<i64>,
    /// The stop names the HTML pages asked departures for (`omsi.getDepartures`): trimmed,
    /// lower case.
    pub wanted_names: Vec<String>,
    /// Per stop name of `wanted_names`: the departures of the next two hours, soonest first,
    /// at most 20, as (line, destination, timestamp).
    pub departures: std::collections::HashMap<String, Vec<(String, String, f64)>>,
    /// Counts up whenever `departures` was made anew.
    pub departures_gen: u64,
}

/// A placed `[trafficlight]` object: its render instances follow the light state of
/// light `index` of the crossing `parent`.
#[derive(Clone)]
pub struct LightObject {
    pub parent: i64,
    pub index: usize,
    /// The map names no light for it (no string, or an empty one): a gate that is its own
    /// crossing, such as the Spandau depot's barrier (`Omnibushof_S_1`/`_S_2`, one arm for
    /// the way in and one for the way out, both moved by one script from one
    /// `TrafficLightPhase`). It is shown the most open state of its crossing's lights, so
    /// that the arms rise for whoever is let through, coming in or going out; on light 0
    /// alone the exit arm stayed down while the buses drove out through it.
    pub any_light: bool,
    /// (render instance, `[visible]` condition of that mesh)
    pub instances: Vec<(usize, Option<(String, f32)>)>,
    /// Per instance (parallel to `instances`) the material slots its lamp variables switch
    /// besides `[visible]`: see [`LampSlots`].
    pub slots: Vec<LampSlots>,
    /// Material switches kept with the lamp, updated alongside its visibility.
    pub variants: Vec<SceneryVariant>,
    pub pos: DVec3,
    /// The lamp's own script (`ampel1.osc` & co): it turns `TrafficLightPhase` into the
    /// `Red`/`Yellow`/`Green`/`Left`/`Light` variables its meshes and coronas show.
    /// Shared by the copies of the lamp list (the loaded tiles' lists are copied into
    /// `World::light_objects` whenever a tile comes or goes), so that it keeps its state.
    pub script: Option<Arc<Mutex<::simulation::scenery::SceneryInstance>>>,
    /// `[light_enh_2]` coronas switched by a lamp variable, and that variable.
    pub coronas: Vec<(::render::Corona, String)>,
    /// Per corona the mesh its light belongs to and the light's place and direction in the
    /// model: an animated lamp's lights move with their mesh (see `model_light_sources`).
    pub corona_mesh: Vec<(usize, glam::Vec3, glam::Vec3)>,
    /// Current brightness of each corona (set with the lamp state every frame).
    pub lit: Vec<f32>,
    /// The object's rotation, and whether its script moves meshes of it: a level
    /// crossing's barrier is a `[trafficlight]` object whose arm turns with its light
    /// (`bue_schranke.osc`), and stood as a static model across the road.
    pub xf: Mat4,
    pub animated: bool,
    /// `[sound]` of the lamp (a crossing's bell, `bue_anlage1`), resolved, and its sounds
    /// once the listener is near (shared by the copies of the list, like `script`).
    pub sound: Option<PathBuf>,
    pub sounds: Arc<Mutex<Option<::audio::SoundSet>>>,
}

pub struct SplineType {
    pub def: Spline,
    pub dir: PathBuf,
    /// `.surf` maps by the spline's material slots.
    pub surface_maps: Option<Arc<SurfFaces>>,
}

impl World {
    /// The map's indexed parking lists: index 0 is `parklist_p.txt`, and an
    /// editor caption of 1 selects `parklist_p_1.txt` for that parking space.
    pub fn parked_car_types(&self, index: usize) -> Vec<String> {
        let mut g = self.parklist.lock();
        if !g.contains_key(&index) {
            let filename = if index == 0 {
                "parklist_p.txt".to_string()
            } else {
                format!("parklist_p_{index}.txt")
            };
            let text = ::legacy_config::vfs::read(&::legacy_config::resolve_path(&self.map_dir, &filename))
                .ok()
                .map(|b| ::legacy_config::decode_text(&b))
                .unwrap_or_default();
            let list: Vec<String> = text
                .lines()
                .map(|l| l.trim().to_string())
                .filter(|l| !l.is_empty() && l.to_ascii_lowercase().ends_with(".sco"))
                .collect();
            log::info!("{filename}: {} parked car types", list.len());
            g.insert(index, list);
        }
        g.get(&index).cloned().unwrap_or_default()
    }

    /// Render texture of mirror `i` (created on first use, as large as the `mirror_size`
    /// setting says).
    pub fn mirror_texture(&self, renderer: &Renderer, scene: &mut Scene, i: usize) -> TextureId {
        let mut g = self.mirror_textures.lock();
        if g.len() <= i {
            g.resize(i + 1, None);
        }
        if let Some(t) = g[i] {
            return t;
        }
        let n = crate::MIRROR_SIZE
            .load(std::sync::atomic::Ordering::Relaxed)
            .clamp(64, 2048);
        let t = renderer.add_render_texture(scene, n, n);
        g[i] = Some(t);
        t
    }
}
