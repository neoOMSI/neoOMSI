use super::*;

/// The tiles of a map and what each depends on.
pub struct TileLayout {
    pub paths: HashMap<(i32, i32), PathBuf>,
    /// Tile → the tiles whose splines, crossings or ground its own final ground, crossings
    /// and cut are made from: its neighbours, and tiles with splines reaching it from
    /// further away. The same for a whole-map load as for streaming.
    pub(super) sources: HashMap<(i32, i32), Vec<(i32, i32)>>,
}

impl TileLayout {
    pub fn sources_of(&self, key: (i32, i32)) -> &[(i32, i32)] {
        self.sources.get(&key).map(|v| v.as_slice()).unwrap_or(&[])
    }

    /// The tile and its neighbours that exist.
    pub fn ring(&self, key: (i32, i32)) -> impl Iterator<Item = (i32, i32)> + '_ {
        NEIGHBOURHOOD
            .iter()
            .map(move |(dx, dy)| (key.0 + dx, key.1 + dy))
            .filter(|k| self.paths.contains_key(k))
    }
}

/// A placed scenery object, ready for the GPU.
pub struct PlacedObject {
    pub(super) ot: Arc<ObjectType>,
    pub(super) pos: DVec3,
    pub(super) xf: Mat4,
    /// Traffic light lamp: (crossing id, light index).
    pub(super) lamp: Option<(i64, usize, bool)>,
    pub(super) map_id: i64,
    /// Collision key (see [`StagedObject::key`]).
    pub(super) key: i64,
    pub(super) controller: Option<usize>,
    pub(super) strings: Vec<String>,
    /// `[varparent]` of the record.
    pub(super) var_parent: Option<i64>,
    /// A car on a `[carpark_p]` space.
    pub(super) parked: bool,
    /// A tile's own `[object]` record standing on the ground: the object editor may move it.
    pub(super) editable: bool,
    pub(super) script: Option<::simulation::scenery::SceneryInstance>,
}

/// A scenery object the object editor can take hold of (see [`World::edit_objects`]).
#[derive(Clone)]
pub struct EditObject {
    pub tile: (i32, i32),
    pub pos: DVec3,
    pub xf: Mat4,
    /// Collision key (its boxes carry it).
    pub key: i64,
    pub instances: Vec<usize>,
    /// Its `.sco`, for the editor's display.
    pub sco: std::path::PathBuf,
}

/// What the object editor did to one object: moved by `moved` (m), turned by `turned`
/// (degrees clockwise, as a map object's heading), or taken away.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct ObjectEdit {
    pub moved: DVec3,
    pub turned: f64,
    pub deleted: bool,
}

/// A parked car of a loaded tile (see [`World::parked_objects`]).
#[derive(Clone)]
pub struct ParkedObject {
    pub tile: (i32, i32),
    pub pos: DVec3,
    pub heading: f64,
    /// Its `.sco` (the AI car of the same folder is what drives off in its place).
    pub sco: std::path::PathBuf,
    /// Its instances (every LOD).
    pub instances: Vec<usize>,
}

/// A tile ready for upload: everything computed, nothing on the GPU yet.
pub struct Prepared {
    pub tx: i32,
    pub ty: i32,
    pub(super) terrain: Option<MeshData>,
    /// Ground painting: for every `[groundtex]` layer above the first that is painted on
    /// this tile, its index and the alpha mask the editor's brush left behind (as read;
    /// [`World::cut_terrain`] turns them into `paint`).
    pub(super) paint_masks: Vec<(usize, Image)>,
    /// The painted layers ready for the GPU: index, mask (with the roads' cut taken out)
    /// and the painted fraction of the tile.
    pub(super) paint: Vec<(usize, TextureData, f32)>,
    /// `tile.map.water`: the height of the tile's water surface at its four corners.
    pub(super) water: Option<[f32; 4]>,
    /// Spline meshes are local to the tile origin.
    /// Spline meshes (local to the tile origin), their type and whether they cast a shadow.
    pub(super) splines: Vec<(Arc<MeshData>, Arc<SplineType>, bool, DVec3)>,
    /// Terrain-mapped spline faces pooled across types within spatial cells.
    pub(super) ground_splines: Vec<Arc<MeshData>>,
    pub(super) objects: Vec<PlacedObject>,
    /// (type, texture, position, height, width, heading)
    pub(super) trees: Vec<(Arc<ObjectType>, String, DVec3, f64, f64, f64)>,
    pub(super) origin: DVec3,
    pub(super) light_map: Option<TextureData>,
    /// The cut the roads make into the ground (alpha 0 = cut), in tile space.
    pub(super) cut: Option<TextureData>,
    /// Textures prepared on the worker, by file (shared by the tiles of a batch).
    pub(super) images: Arc<HashMap<PathBuf, Arc<TextureData>>>,
}

/// A prepared tile on its way to the GPU (see [`World::begin_upload`]).
pub struct PendingUpload {
    pub prepared: Prepared,
    pub(super) textures: Vec<PathBuf>,
    pub(super) types: Vec<Arc<ObjectType>>,
    pub(super) tg: TileGpu,
    pub(super) placing: Placing,
}

/// How far [`World::place_step`] got with a tile, and what it made so far.
#[derive(Default)]
pub(super) struct Placing {
    /// 0: the ground, 1: splines, 2: trees, 3: objects, 4: done.
    pub(super) phase: u8,
    /// The next spline or tree of the phase.
    pub(super) next: usize,
    pub(super) ground_next: usize,
    pub(super) night_slots: Vec<(usize, usize, MaterialId, MaterialId)>,
    pub(super) night_modes: Vec<NightMode>,
    pub(super) light_objects: Vec<LightObject>,
    pub(super) poles: Vec<i64>,
    pub(super) splines: usize,
    pub(super) trees: usize,
    pub(super) objects: usize,
    /// Seconds per phase (OMSI_PROFILE).
    pub(super) secs: [f64; 4],
    /// `[terrainmapping]` uses the first [groundtex], without the roads' cut (which
    /// would punch holes into a traffic island). Painted terrain layers belong to the
    /// ground itself and must not be projected onto a spline verge or object.
    pub(super) terrain_mapping_mat: Option<MaterialId>,
}

impl PendingUpload {
    pub fn key(&self) -> (i32, i32) {
        (self.prepared.tx, self.prepared.ty)
    }
}

/// What a loaded tile added to the world, so that unloading it can take it away again.
#[derive(Default)]
pub struct TileState {
    pub bus_stops: Vec<(i64, DVec3, f64, String)>,
    /// The tile's waiting places (see `World::waiting_places`).
    pub waiting_places: Vec<(i64, DVec3, f64, f32)>,
    pub obstacles: Vec<::simulation::collision::Obb>,
    /// The boxes of the tile's parked cars (also among `obstacles`), for the pedestrians.
    pub parked_boxes: Vec<::simulation::collision::Obb>,
    /// Objects that collide with their `[collision_mesh]`.
    pub mesh_obstacles: Vec<::simulation::collision::MeshObstacle>,
    /// The visible meshes as shadow casters of the point lights.
    pub light_obstacles: Vec<::simulation::collision::MeshObstacle>,
    pub coronas: Vec<StaticCorona>,
    pub lights: Vec<::render::PointLight>,
    pub light_objects: Vec<LightObject>,
    pub night_slots: Vec<(usize, usize, MaterialId, MaterialId)>,
    /// The tile's objects whose night textures follow a `[NightMapMode]` timetable.
    pub night_modes: Vec<NightMode>,
    /// Collision keys of the tile's `[crashmode_pole]` posts (in `World::poles`).
    pub poles: Vec<i64>,
    /// The tile's objects that stop the outside camera.
    pub blockers: Vec<crate::camera_arm::Blocker>,
    /// The boxes of the tile's `[petrolstation]` objects (see `World::petrol_stations`).
    pub petrol_stations: Vec<::simulation::collision::Obb>,
    /// Parked cars the tile placed (counted in `World::parked_live`).
    pub parked_count: usize,
    /// The tile's echoing places (see `World::reverb_zones`).
    pub reverb_zones: Vec<(::simulation::collision::Obb, f32, f32)>,
    pub gpu: TileGpu,
}
