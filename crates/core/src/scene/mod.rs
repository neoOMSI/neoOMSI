//! Builds a renderable scene from a map: terrain, splines and scenery objects.
//!
//! Loading is parallel: every tile is parsed and tessellated on the rayon pool, scenery
//! object types are loaded once and shared, then everything is uploaded to the GPU on the
//! calling thread.

// Shared imports: the submodules and the test modules pull these in with `use super::*;`.
#![allow(unused_imports)]

use crate::tiles::{MapIndex, Pose};
use anyhow::{Context, Result};
use glam::{DVec2, DVec3, Mat4};
use hashbrown::HashMap;
use ::geometry::{
    HeightMap, MeshData, SplineCurve, SurfFaces, TileSurface, build_spline_mesh,
    build_terrain_mesh, mesh_from_o3d, object_rotation,
};
use ::map::{GlobalCfg, Terrain, tile_size};
use ::model::{MaterialDef, MeshDef, Model};
use ::render::{
    AlphaMode, MaterialExtra, MaterialId, MeshId, RenderPhase, Renderer, Scene, TextureId,
};
use ::scenery::{SceneryObject, Spline};
use ::traffic::{BlockRule, Lane, LaneBuilder, LaneKey, LaneKind, TrafficLightController};
use ::texture::{Image, TextureCache, TextureData};
use parking_lot::{Mutex, RwLock};
use rayon::prelude::*;
use std::path::{Path, PathBuf};
use std::sync::Arc;

mod batching;
mod crossing;
mod gpu;
mod ground;
mod lamps;
mod light_map;
mod materials;
mod navigation;
mod object_type;
mod object_types;
mod open;
mod paint;
mod parked;
mod place;
mod place_step;
mod scripted;
mod stage;
mod staged;
mod stats;
mod stops;
mod summary;
mod surface;
mod terrain_cut;
mod textures;
mod tile_types;
mod traffic;
mod upload;
mod variants;
mod vehicle_instance;
mod vehicle_sync;
mod vehicles;

use self::batching::*;
use self::crossing::*;
pub use self::gpu::*;
pub use self::ground::*;
pub use self::lamps::*;
use self::light_map::*;
pub use self::materials::*;
pub use self::navigation::*;
pub use self::object_type::*;
use self::object_types::*;
pub use self::open::*;
use self::paint::*;
use self::parked::*;
use self::place::*;
use self::place_step::*;
pub use self::scripted::*;
use self::stage::*;
pub use self::staged::*;
pub use self::stats::*;
use self::stops::*;
use self::summary::*;
pub use self::surface::*;
use self::terrain_cut::*;
use self::textures::*;
pub use self::tile_types::*;
use self::traffic::*;
use self::upload::*;
pub use self::variants::*;
use self::vehicle_instance::*;
pub use self::vehicle_sync::*;
pub use self::vehicles::*;

#[cfg(test)]
mod material_tests;
#[cfg(test)]
mod navigation_road_tests;
#[cfg(test)]
mod tests;
#[cfg(test)]
mod crossing_tests;
#[cfg(test)]
mod detail_loading_tests;
#[cfg(test)]
mod parity_acceptance_tests;
#[cfg(test)]
mod render_queue_tests;
#[cfg(test)]
mod terrain_mapping_tests;

pub struct World {
    pub root: PathBuf,
    pub global: GlobalCfg,
    pub map_dir: PathBuf,
    /// Detail snapshot for this map session; raw editor tiles remain unfiltered.
    map_detail: u8,
    /// Indexed parked car lists of the map, loaded when a parking space uses one.
    parklist: Mutex<HashMap<usize, Vec<String>>>,
    /// Render textures of the player's mirrors (`reflexionN.bmp`), by camera index.
    pub mirror_textures: Mutex<Vec<Option<TextureId>>>,
    object_types: Mutex<HashMap<String, Option<Arc<ObjectType>>>>,
    spline_types: Mutex<HashMap<String, Option<Arc<SplineType>>>>,
    pub textures: Arc<TextureCache>,
    /// World position (with terrain height) and rotation of every loaded map object by id.
    pub object_positions: Mutex<HashMap<i64, (DVec3, [f64; 3])>>,
    /// The objects whose id is used on more than one tile, by (tile, id) (see
    /// `MapIndex::duplicates`, `World::entry_point_place`).
    pub object_dups: Mutex<HashMap<((i32, i32), i64), (DVec3, [f64; 3])>>,
    /// Loaded terrains by tile coordinate.
    pub terrains: Arc<RwLock<HashMap<(i32, i32), Arc<Terrain>>>>,
    /// Surface rasters (roads) by tile coordinate.
    pub surfaces: Arc<RwLock<HashMap<(i32, i32), Arc<TileSurface>>>>,
    /// GPU resources per vehicle type (by .bus path) and paint scheme, shared by AI vehicles.
    vehicle_gpu: Mutex<HashMap<VehicleKey, VehicleSet>>,
    /// GPU textures of vehicles by file: one bus spawned in twenty adverts used to upload
    /// its whole texture set twenty times (200 ms a spawn, 1.5 fps on Spandau).
    /// (With the number of sets holding each.)
    vehicle_textures: Arc<Mutex<HashMap<PathBuf, (TextureId, usize)>>>,
    /// GPU meshes of vehicles by (bus file, mesh index), shared across paint schemes.
    vehicle_meshes: Arc<Mutex<HashMap<(PathBuf, usize), (MeshId, usize)>>>,
    /// Vehicle meshes and textures made on a worker, until their set is uploaded.
    vehicle_ready: Arc<Mutex<PreparedVehicles>>,
    /// Textures uploaded as RGBA to spare a frame, being compressed on the workers, and the
    /// compressed ones waiting to be swapped in.
    upgrades_pending: Mutex<hashbrown::HashSet<PathBuf>>,
    upgrades_done: Arc<Mutex<Vec<(PathBuf, Arc<TextureData>)>>>,
    /// Roller-blind pictures (`[matl_freetex]`) uploaded as RGBA, to be compressed.
    freetex_upgrades: Arc<Mutex<Vec<PathBuf>>>,
    /// OMSI's `[texmemlimit]`: bytes the scenery and vehicle textures may take on the GPU
    /// (0 = no limit), and when the budget was last looked at.
    texture_limit: std::sync::atomic::AtomicU64,
    budget_checked: Mutex<Option<std::time::Instant>>,
    /// Traffic-path lanes collected while building tiles.
    pub lanes: Mutex<Vec<::traffic::Lane>>,
    /// The tiles whose lanes and parked cars have been put into `lanes` and `parked_cars`.
    /// These two and `lanes` are only filled, and should only be taken, while the `lanes`
    /// lock is held: whoever takes them then has every parked car together with the lanes
    /// it stands beside, and knows which tiles those came from.
    pub lane_tiles: Mutex<Vec<(i32, i32)>>,
    /// Traffic light programs of placed crossings, and which map object owns each.
    pub traffic_lights: Mutex<Vec<TrafficLightController>>,
    pub controller_of_object: Mutex<HashMap<i64, usize>>,
    /// Placed traffic light lamps.
    pub light_objects: Mutex<Vec<LightObject>>,
    /// Placed objects with scripts / animations.
    pub scripted: Mutex<Vec<ScriptedObject>>,
    /// The clock and the departure boards the scenery scripts read.
    pub timetable_boards: Mutex<StopBoards>,
    /// The map's `Holidays.txt`, read when first asked.
    pub calendar: std::sync::OnceLock<::map::Calendar>,
    /// The number plates of `registrations.txt` (the active chrono scenarios' first, the
    /// latest before, then the map's own: the original), read when first asked.
    pub registrations: std::sync::OnceLock<Vec<String>>,
    /// The clock the run starts at: a scenery object placed before the simulation's clock
    /// reaches the boards runs its `{init}` on it (it ran on 09:00 of 1989).
    pub start_clock: Mutex<::simulation::SimClock>,
    /// Obstacles for vehicle collisions (of the loaded tiles; replaced when they change).
    pub collision: Mutex<Arc<::simulation::collision::CollisionWorld>>,
    /// The visible meshes as boxes: what the point lights' shadows are cast by.
    pub light_occluders: Mutex<Arc<::simulation::collision::CollisionWorld>>,
    /// `[crashmode_pole]` objects of the loaded tiles by collision key: where they stand and
    /// their instances, so that a post a vehicle knocked over can be laid on the ground
    /// (see [`World::lay_down_pole`]). A tile's posts leave with it: its instances are
    /// handed to other objects.
    pub poles: Mutex<HashMap<i64, (DVec3, Mat4, Vec<usize>)>>,
    /// Posts knocked over in this run and the way they fell: a tile loaded again lays them
    /// down again.
    fallen_poles: Mutex<HashMap<i64, DVec3>>,
    /// The parked cars of the loaded tiles by collision key, so that one can pull out into
    /// the traffic (see [`World::depart_parked`]). They leave with their tile.
    pub parked_objects: Mutex<HashMap<i64, ParkedObject>>,
    /// Parked cars that drove off in this run: their space stays empty when the tile comes
    /// back.
    departed: Mutex<std::collections::HashSet<i64>>,
    /// What a parked car that drove off left behind: its object and its boxes, for an AI
    /// car of the same kind that parks in the space again (`return_parked`).
    departed_objects: Mutex<
        std::collections::HashMap<
            i64,
            (
                ParkedObject,
                Vec<::simulation::collision::Obb>,
                Vec<::simulation::collision::Obb>,
            ),
        >,
    >,
    /// The loaded tiles' own objects by map id, for the object editor (`crate::editor`).
    pub edit_objects: Mutex<HashMap<i64, EditObject>>,
    /// What the object editor did this run, by map id: kept over tile reloads until saved.
    pub object_edits: Mutex<HashMap<i64, ObjectEdit>>,
    /// The ground as the editor's brush has left it, by tile (read instead of the file).
    pub terrain_edits: Mutex<HashMap<(i32, i32), Terrain>>,
    /// Placed `[busstop]` objects: (map id, world position, heading, name).
    pub bus_stops: Mutex<Vec<(i64, DVec3, f64, String)>>,
    /// Where people wait at the stops: the `[passpos]` points of placed objects with a
    /// `[passengercabin]` (the maps' `people_standing_*` markers and bus shelters) as
    /// (object id, world position, heading in degrees, seat height - 0 for a standing place).
    pub waiting_places: Mutex<Vec<(i64, DVec3, f64, f32)>>,
    /// Passenger cabins of waiting objects by file, read once.
    waiting_cabins: Mutex<HashMap<PathBuf, Option<Arc<::legacy_vehicle::PassengerCabin>>>>,
    /// Counts the changes of the loaded tiles (see [`World::refresh_tile_lists`]): whoever
    /// keeps what it derived from the stops, the waiting places or the ground looks again.
    pub tiles_generation: std::sync::atomic::AtomicU64,
    /// Parked cars placed on `[carpark_p]` spaces: world position and heading (deg), for
    /// the traffic to steer round (see `lane_tiles`).
    pub parked_cars: Mutex<Vec<(DVec3, f64)>>,
    /// The boxes of the parked cars of the loaded tiles, which people walk round.
    pub parked_boxes: Mutex<Arc<Vec<::simulation::collision::Obb>>>,
    /// Placed objects with particle systems (chimney smoke, the fireworks, a memorial's
    /// flame) by tile.
    pub particle_objects: Mutex<HashMap<(i32, i32), Vec<ParticleObject>>>,
    /// The boxes of the loaded `[petrolstation]` objects (the depots' fuel and wash yards):
    /// OMSI lets the pump and the wash run only while the bus's box
    /// overlaps one, and sends the workshop's team out when the bus stands in none.
    pub petrol_stations: Mutex<Vec<::simulation::collision::Obb>>,
    /// Parked cars standing in the loaded tiles, and the options' `[AIMaxCountParked]`
    /// (0 = every space the map fills, -1 = none): past it the spaces stay empty.
    pub parked_live: std::sync::atomic::AtomicUsize,
    pub parked_max: i64,
    /// Places that echo (`[triggerbox_new]` + `[triggerbox_setreverb]`: the railway bridges'
    /// underpasses): the box, the reverberation time (s) and the distance (m) over which it
    /// fades in at the box's sides.
    pub reverb_zones: Mutex<Vec<(::simulation::collision::Obb, f32, f32)>>,
    /// The loaded tiles' night light maps (`.map.LM.bmp`), for the light map atlas.
    pub light_maps: Mutex<HashMap<(i32, i32), Arc<::texture::Image>>>,
    light_maps_generation: std::sync::atomic::AtomicU64,
    /// The atlas as last filled: centre tile and the generation of `light_maps`.
    light_map_atlas: Mutex<Option<((i32, i32), u64)>>,
    /// The map's `signalroutes.cfg` (unit `mc_fahrstrasse`): which track pieces each railway
    /// signal protects, its distant signal, the next signal and a speed limit.
    pub signal_routes: Vec<::map::ailists::SignalRoute>,
    /// Chrono folders active on the sim date, in order, and the merged AI lists / date.
    /// The chrono scenarios in force on the sim date (changed at midnight: `set_date`).
    pub chrono_dirs: parking_lot::RwLock<Vec<PathBuf>>,
    pub ailists: ::map::AiLists,
    pub date: i32,
    /// Ticket pack (chrono folders may override the map's).
    pub ticket_pack: String,
    /// Fonts for text and script textures, shared by all vehicles.
    pub fonts: Arc<Mutex<::simulation::texttex::FontLibrary>>,
    /// Scenery material variants switched by `NightlightA`: (instance, slot, material on, off).
    pub night_slots: Mutex<Vec<(usize, usize, MaterialId, MaterialId)>>,
    /// Objects whose night textures follow a `[NightMapMode]` timetable (see `update_night_modes`).
    pub night_modes: Mutex<Vec<NightMode>>,
    /// Light coronas and point lights of placed scenery objects.
    pub static_coronas: Mutex<Vec<StaticCorona>>,
    pub static_lights: Mutex<Vec<::render::PointLight>>,
    /// Every tile file of the map read once: splines for the spline attachment rows,
    /// objects for entry points and stops of tiles that are not loaded.
    index: Mutex<Option<Arc<MapIndex>>>,
    /// What each loaded tile added (see [`TileState`]).
    pub tile_state: Mutex<HashMap<(i32, i32), TileState>>,
    /// Tiles that have been loaded at least once: their lanes, light programs and parked
    /// cars are in the lists for good.
    seeded: Mutex<hashbrown::HashSet<(i32, i32)>>,
    /// The GPU side of the loaded tiles.
    gpu: Mutex<GpuCache>,
    /// Types this installation lacks, logged once each (file, what it is).
    missing: Mutex<hashbrown::HashMap<String, &'static str>>,
    /// Tiles read and typed that loaded tiles (or tiles on their way) depend on.
    staged: Mutex<HashMap<(i32, i32), Arc<StagedTile>>>,
    layout: Mutex<Option<Arc<TileLayout>>>,
    /// Scenery objects' sound configurations, read once per file.
    sound_cfgs: Mutex<HashMap<PathBuf, Option<Arc<::legacy_vehicle::SoundCfg>>>>,
}
