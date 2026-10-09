use super::*;

/// Where a staged object will stand once the ground under it is final.
#[derive(Clone)]
pub(super) enum Placement {
    /// An `[object]` record: world x, y, the height above the terrain and its rotation.
    Ground {
        x: f64,
        y: f64,
        z: f64,
        rot: [f64; 3],
    },
    /// A pose known from the start: `[absheight]` objects, objects joined to splines by
    /// `[splinehelper]` (crossings, switches) and spline attachment rows.
    Pose(Pose),
    /// `[attachObj]`: attachment point `index` of object `parent`, turned by `rot`.
    Attached {
        parent: i64,
        index: usize,
        rot: [f64; 3],
    },
}

/// An object of a tile with its type, before it stands on the final ground.
pub(super) struct StagedObject {
    pub(super) ot: Arc<ObjectType>,
    pub(super) id: i64,
    pub(super) place: Placement,
    pub(super) rules: Vec<::map::MapRule>,
    /// The record's trailing lines: text strings, tree parameters, a lamp's light index,
    /// a bus stop's name.
    pub(super) extra: Vec<String>,
    /// The crossing a traffic light lamp belongs to (`[varparent]`, else what it hangs on).
    pub(super) lamp_parent: Option<i64>,
    /// A car put on a parking space (the traffic steers round it).
    pub(super) parked: bool,
    /// The record is an `[object]` (its position goes into `object_positions`).
    pub(super) map_object: bool,
    /// The instance of a spline-attachment row this object represents.
    pub(super) instance: usize,
    /// What the collisions call this object: its map id, or for an object of a spline
    /// attachment row (which all share the row's id) a key of its own, so that one post of
    /// a row of `[crashmode_pole]` bollards falls alone.
    pub(super) key: i64,
}

/// The collision key of object `index` of spline attachment row `row` in tile (tx, ty):
/// above every map id (2^53 and up), and the same on every load.
pub(super) fn row_object_key(tx: i32, ty: i32, row: i64, index: usize) -> i64 {
    let mut h: u64 = 0xcbf2_9ce4_8422_2325;
    for v in [tx as i64 as u64, ty as i64 as u64, row as u64, index as u64] {
        h = (h ^ v).wrapping_mul(0x0000_0100_0000_01b3);
        h ^= h >> 29;
    }
    ((1u64 << 53) | (h & ((1u64 << 53) - 1))) as i64
}

/// A spline of a staged tile as the rasters see it.
pub(super) struct StagedSpline {
    /// Positions (relative to the tile origin) and triangles only.
    pub(super) shape: MeshData,
    pub(super) ty: Arc<SplineType>,
    /// World bounds (x0, y0, x1, y1).
    pub(super) bounds: [f64; 4],
    /// It carries a road or a footway (a railway embankment or a bridge deck does not).
    pub(super) drivable: bool,
    /// Every profile of it is blended (`[matl_alpha] 2`): a layer laid over the ground or a
    /// road, not a surface of its own (see `prepare_surfaces`).
    pub(super) overlay: bool,
    /// It is ground (see `SPLINE_OVERHEAD`): it goes into the surface raster, cutting the
    /// terrain where that comes up through it. Overhead wires do not.
    pub(super) cuts_terrain: bool,
    /// It stands clear of the ground all along (see `SPLINE_SHADOW_CLEARANCE`): it casts a
    /// sun shadow.
    pub(super) casts_shadow: bool,
    /// Start point used by OMSI's far-to-near blend sort for spline surfaces.
    pub(super) sort_origin: DVec3,
}

/// How far a spline has to stand clear of the ground under it, everywhere, before it casts
/// a sun shadow: a bridge deck, a viaduct or an elevated railway does, a road lying on the
/// terrain does not - a caster in one plane with what it falls on paints dark patches into
/// it (the sun shadow's bias is 6 cm). Splines are surfaces and cast nothing otherwise.
pub(super) const SPLINE_SHADOW_CLEARANCE: f32 = 0.75;

/// OMSI's metric separation for a spline or `[surface]` object's vertices.
pub(super) const OMSI_SURFACE_LIFT: f32 = 0.08;

pub(super) fn scenery_draw_position(authored: DVec3, surface: bool) -> DVec3 {
    authored
        + if surface {
        DVec3::Z * OMSI_SURFACE_LIFT as f64
    } else {
        DVec3::ZERO
    }
}

/// Whether a scenery object is drawn with the roads' `OMSI_SURFACE_LIFT`: a `[surface]`
/// object, and whatever is drawn in the surfaces' phases on them - a `[rendertype] surface`
/// plate and an `on_surface` marking. A road arrow or a zebra laid a few centimetres over
/// the authored road went under the road drawn 8 cm higher (every turn arrow of Spandau's
/// Falkenseer Chaussee, the zebra crossings of many maps, #871).
pub(super) fn drawn_on_surfaces(sco: &SceneryObject) -> bool {
    use ::scenery::sco::RenderType;
    sco.surface || matches!(sco.render_type, RenderType::Surface | RenderType::OnSurface)
}

/// A spline whose profiles all hang this far (m) over its line - wires, catenaries, a
/// canopy - is no ground surface: it neither cuts the terrain nor carries anything.
pub(super) const SPLINE_OVERHEAD: f32 = 2.0;

pub(super) fn scenery_render_phase(kind: ::scenery::sco::RenderType) -> RenderPhase {
    use ::scenery::sco::RenderType as ScoPhase;
    match kind {
        ScoPhase::PreSurface => RenderPhase::PreSurface,
        ScoPhase::Surface => RenderPhase::Surface,
        ScoPhase::OnSurface => RenderPhase::OnSurface,
        ScoPhase::BeforeNormal => RenderPhase::BeforeNormal,
        ScoPhase::AfterNormal => RenderPhase::AfterNormal,
        ScoPhase::AfterVehicles => RenderPhase::AfterVehicles,
        ScoPhase::Normal => RenderPhase::Normal,
    }
}

/// Does every profile of the spline hang `SPLINE_OVERHEAD` or more over its line?
pub(super) fn overhead_only(def: &::scenery::sli::Spline) -> bool {
    !def.profiles.is_empty()
        && def
        .profiles
        .iter()
        .all(|p| !p.points.is_empty() && p.points.iter().all(|q| q.z >= SPLINE_OVERHEAD))
}

/// Which `parklist_p` a car park draws from: its first map string, as a number (Omsi.exe
/// sub_79c8b8 - `StrToInt`, 0 when that fails or there is none). 0 is `parklist_p.txt`,
/// n is `parklist_p_n.txt`.
pub(super) fn parklist_index(strings: &[String]) -> usize {
    strings
        .first()
        .and_then(|s| s.trim().parse::<usize>().ok())
        .unwrap_or(0)
}

/// A tile read and tessellated, its objects typed but not yet standing on the ground. Kept
/// (by [`World::prepare_tiles`]) while a loaded tile or one on its way depends on it.
pub struct StagedTile {
    pub(super) tx: i32,
    pub(super) ty: i32,
    pub(super) origin: DVec3,
    pub(super) path: PathBuf,
    /// The terrain as the tile file has it, before roads and crossings pulled it about.
    pub(super) base_terrain: Terrain,
    /// `[spline_terrain_align]` splines: (index into `splines`, reach in metres).
    pub(super) align: Vec<(usize, f32)>,
    /// The outlines (world x, y) those splines cut out of the ground (see
    /// `::geometry::spline_hole_outlines`).
    pub(super) hole_outlines: Vec<Vec<DVec2>>,
    pub(super) water: Option<[f32; 4]>,
    pub(super) splines: Vec<StagedSpline>,
    /// The whole spline meshes, in the order of `splines`, until the tile is placed.
    pub(super) meshes: Mutex<Option<Vec<Arc<MeshData>>>>,
    /// The splines' wheel-contact meshes (local to `origin`) with their world bounds.
    pub(super) drive: Vec<StagedDrive>,
    /// Lanes, taken when the tile is loaded for the first time.
    pub(super) lanes: Mutex<Vec<Lane>>,
    /// The street lanes' points of the tile's splines, kept for good (what an object's box
    /// is checked against: a road through it makes it no wall).
    pub(super) street_points: Vec<DVec3>,
    pub(super) objects: Vec<StagedObject>,
    /// The spline attachment rows `[attachObj]` records can hang on: (row id, where its first
    /// object stands, the row's own type - a car park row's, not its car's).
    pub(super) anchors: Vec<(i64, Pose, Arc<ObjectType>)>,
    /// What reading the tile counted (missing types, empty car parks, rows, attachments).
    pub(super) counts: LoadStats,
    pub(super) resolved: std::sync::OnceLock<Arc<Resolved>>,
}

pub(super) enum StagedDrive {
    HeightProfiles(MeshData, [f64; 4]),
    DrawnMesh {
        mesh: MeshData,
        bounds: [f64; 4],
        surface_maps: Option<Arc<SurfFaces>>,
    },
}

/// A staged tile's final ground and where its objects finally stand.
pub(super) struct Resolved {
    pub(super) terrain: Arc<Terrain>,
    /// By object index; None for an attachment without its parent or attachment point.
    pub(super) poses: Vec<Option<Pose>>,
    pub(super) unattached: usize,
    pub(super) aligned_points: usize,
    pub(super) biggest: Option<(f32, f64, f64)>,
    pub(super) deformed: bool,
}

/// The tile and its eight neighbours.
pub(super) const NEIGHBOURHOOD: [(i32, i32); 9] = [
    (-1, -1),
    (0, -1),
    (1, -1),
    (-1, 0),
    (0, 0),
    (1, 0),
    (-1, 1),
    (0, 1),
    (1, 1),
];

/// How far outside a tile a spline still counts for it: the terrain alignment reaches 20 m
/// beyond, and a crossing plate standing on the tile's edge looks for the roads that run
/// into it well past that.
pub(super) const SOURCE_MARGIN: f64 = 150.0;

/// OMSI_CHECK_SPLINES: every spline's two ends, its neighbours in the chain and its file.
pub(crate) static SPLINE_ENDS: std::sync::LazyLock<
    Mutex<HashMap<i64, (DVec3, DVec3, i64, i64, String)>>,
> = std::sync::LazyLock::new(Default::default);
