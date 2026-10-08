//! AI road traffic: vehicles from `ailists.cfg` moving on the map's path network, the
//! traffic light programs of the junctions, and the population of cars around the player.
//!
//! This is the L6 engine adapter for the headless [`::traffic`] domain crate. Stage 9b split
//! the formerly single-file orchestrator into owned submodules; `Traffic` itself and its
//! shared types/free helpers stay here, and the `impl Traffic`/`impl AiCar`/`impl Viewer`
//! method groups live in the submodules (`car`, `viewer`, `loading`, `population`,
//! `perception`, `presentation`, `network`, `lan`, `lifecycle`, `diagnostics`, `tick`).
//! Types are defined here so every submodule can read their private fields; methods are
//! `pub(crate)` where a sibling submodule calls them.

use crate::bus_service::BusService;
use crate::scene::{VehicleRender, World};
use anyhow::Result;
use glam::{DVec2, DVec3};
use hashbrown::HashMap;
use ::render::{Renderer, Scene};
use ::simulation::ai_motion::{AiBody, MotionKind};
use ::simulation::collision::Obb;
pub(crate) use ::traffic::{
    AiState, Aspect, BodyFootprint, Capture, CaptureTrigger, ChangeInfo, ChangeKind,
    JunctionActor, JunctionCoordinator, JunctionDecision, JunctionScene, JunctionState, LaneId,
    LaneKind, Lead, ManeuverActor, ManeuverCoordinator, ManeuverInputs,
    ManeuverIntent, ManeuverPhase, ManeuverScene, ManeuverState, Network, NetworkVersion,
    Occupancy, ParkPlan, Placement, RealizedMotion, Reason, StopTarget, SweepSample,
    TickSnapshot, TraceHeader, TrafficLightController, VehicleCapabilities, VehicleClass,
    VehicleId, VehicleSnapshot, TRACE_VERSION, junction_ahead,
    DormantView, Lifecycle, PopulationCoordinator, PopulationDemand, PopulationScene,
    RemovalCause, SpawnClass, SpawnFacts, SpawnOutcome, SpawnRequest, SpawnRequestId,
    TraceEvent, DORMANT_CAP_FACTOR,
};
use ::traffic::{
    BerthIntent, ServiceActor, ServiceCoordinator, ServiceInputs, ServicePhase, ServiceScene,
    StopDemand, STOP_REACH,
};
use ::simulation::vehicle::AiFrame;
use ::simulation::{VehicleInstance, VehicleType};
use std::path::Path;
use std::sync::Arc;


// ---- Stage 9b decomposition: the adapter is split into owned submodules ----
mod car;
pub(crate) use car::emergency_drive;
mod viewer;
mod loading;
mod population;
mod perception;
mod presentation;
mod network;
mod lan;
mod lifecycle;
mod diagnostics;
mod tick;
mod safety;
#[cfg(all(feature = "devtools", debug_assertions))]
mod debug;
#[cfg(all(feature = "devtools", debug_assertions))]
pub(crate) use debug::TrafficDebugFrame;



/// How many of a type's paint schemes the AI uses: every scheme is a full upload of the
/// bus's textures the first time it appears, which used to cost a frame of 100-200 ms
/// each and a minute of stutter after loading Spandau.
pub const AI_SCHEMES: usize = 4;
const SCRIPT_UPLOAD_BUDGET: usize = 4 << 20;

/// Start a vehicle of type `ty` once and throw it away: its `{init}` and its displays read
/// the files they need (depot data, fonts) into the caches before the first real one of the
/// type comes along in the middle of a drive.
pub fn warm_up(world: &World, ty: &Arc<VehicleType>, hof: Option<Arc<::legacy_vehicle::Hof>>) {
    let t = std::time::Instant::now();
    let mut host = ::simulation::VehicleHost::new(::simulation::SimClock::default());
    host.hof = hof;
    host.font_lib = Some(world.fonts.clone());
    let mut vehicle = VehicleInstance::new(ty.clone(), host);
    vehicle.init_text_textures(&mut world.fonts.lock(), &|p| {
        ::texture::decode_file(p)
            .ok()
            .map(|i| (i.width, i.height, i.rgba))
    });
    if ::legacy_config::env::var_os("OMSI_PROFILE").is_some() {
        log::info!(
            "  first start of {}: {:.1} ms",
            ty.def.path.display(),
            t.elapsed().as_secs_f64() * 1000.0
        );
    }
}

/// A car edging out round something standing keeps to `PULL_OUT_ACCEL` until its front is
/// this far past the obstacle's rear (m).
const PRIORITY_WARN_GAP: f32 = 60.0;

/// Seconds a car may stand held at low speed behind a non-moving obstruction before its
/// driver sounds the horn (s). This is a documented provisional neoOMSI trigger: the
/// reference establishes only that `ev_AI_Horn` exists, not its original trigger. It is
/// presentation feedback and never grants entry or releases a claim.
const HORN_HOLD: f32 = 3.0;
/// Minimum seconds between two horns from the same car (s), so a queue does not beep.
const HORN_COOLDOWN: f32 = 8.0;
/// Above this speed (m/s) a car is moving, not held, and does not sound the horn.
const HORN_SPEED: f32 = 1.0;

/// Room an oncoming vehicle needs beside a car (m from the car's side to the middle of the
/// oncoming lane): its half width and a margin.
const PLAYER_BOX_MARGIN: f32 = 0.5;

/// What makes a new car a timetable bus (`Traffic::create_car`).
pub struct BusSetup {
    /// The trip's lanes, as far as the loaded tiles have them.
    pub route: Vec<usize>,
    pub stops: Vec<StopTarget>,
    /// Fleet number and registration (`number`, `ident` string variables).
    pub number: Option<(String, String)>,
    pub hof: Option<Arc<::legacy_vehicle::Hof>>,
}

pub struct AiCar {
    /// Stable id for references from other systems (passengers).
    pub(crate) id: VehicleId,
    /// Validated, immutable physical capabilities (extents, class, brake configuration).
    pub(crate) caps: VehicleCapabilities,
    pub(crate) motion_fault: Option<Reason>,
    /// The random seed it was made with and its paint scheme: a car that goes out of range
    /// and comes back is the same car (`DormantCar`).
    pub(crate) seed: u64,
    pub(crate) scheme: Option<usize>,
    pub(crate) state: AiState,
    pub(crate) vehicle: VehicleInstance,
    pub(crate) render: VehicleRender,
    pub(crate) trailer_renders: Vec<VehicleRender>,
    /// The body following the way `state` lays out.
    pub(crate) body: AiBody,
    /// Seconds this car has been standing still without a stop of its own: a red light or
    /// a queue is seconds, a jam that never clears grows without bound.
    pub(crate) stopped: f32,
    /// The car it follows now (its id), when one is close ahead.
    pub(crate) lead_car: Option<VehicleId>,
    /// A car it does not take for its lead until the time given: two that had each other
    /// for their lead (see `Traffic::break_lead_pairs`).
    pub(crate) ignore_lead: Option<(VehicleId, f64)>,
    /// Seconds it has crept along below 1 m/s (a claim of one that crawls in a jam of its
    /// own is no car about to come either).
    pub(crate) crawl: f32,
    /// A timetable bus: its trip's stops, the doors, the layover, the people aboard (see
    /// `bus_service`). Everything else about it is this car's.
    pub(crate) bus: Option<Box<BusService>>,
    /// `[sound_ai]` set, created when the car comes near the listener.
    pub(crate) sounds: Option<::audio::SoundSet>,
    /// Half the vehicle's width (m).
    pub(crate) half_width: f32,
    /// Waiting at a junction for someone with the right of way this frame.
    pub(crate) yielding: bool,
    /// Stopped by a red light this frame.
    pub(crate) light_hold: bool,
    /// Where the car is in its junction movement (owned by `traffic::junctions`).
    pub(crate) junction_state: JunctionState,
    /// The car's lateral maneuver memory and commitments (owned by `traffic::maneuvers`).
    pub(crate) maneuver: ManeuverState,
    /// Finished (a dead end, the end of a timetable trip, given up): taken off the road as
    /// soon as nobody can see it.
    pub(crate) gone: bool,
    /// Seconds since it was put on the road are fewer than this: its speed was a guess.
    pub(crate) fresh: f32,
    /// The car it lets go first at the next merge (by id).
    pub(crate) merge_after: Option<VehicleId>,
    /// What holds it (`OMSI_DEBUG_TRAFFIC`, for cars standing for long).
    pub(crate) holding: Option<String>,
    /// What held the car back this frame (for OMSI_TRACE_AI): the typed constraint nearest
    /// ahead and its distance ahead of the front (m). [`Reason::NONE`] means nothing did.
    pub(crate) why: (Reason, f32),
    /// Something made it wait this frame: a stop point, or a car or an obstacle close ahead.
    pub(crate) held: bool,
    /// The vehicle (by id) whose body stands in this car's way off its lanes this frame
    /// (`Traffic::body_in_way`).
    pub(crate) geo_block: Option<VehicleId>,
    /// What it keeps behind (by id) and the gap to it, as of its last step.
    pub(crate) lead_info: Option<(VehicleId, f32)>,
    /// What it waited for at its last junction (`OMSI_DEBUG_STUCK` only).
    pub(crate) junction_why: String,
    /// Giving way: where it waits (distance from its origin to the line).
    pub(crate) wait_at: Option<f32>,
    /// The vehicle (by id) standing half out of the lane that this car is squeezing past.
    pub(crate) squeeze: Option<VehicleId>,
    /// How far behind something standing (a bus at its stop, the player's bus) this car
    /// stops, so that it can steer out round it later (m, front bumper to the other's body;
    /// from its own steering, `pull_out_room`).
    pub(crate) pass_room: f32,
    /// Seconds until this car may sound its horn (`ev_AI_Horn`) again (s); see `HORN_HOLD`.
    pub(crate) horn_cooldown: f32,
    /// The traffic light it waited for in the last frame: distance from its origin.
    pub(crate) light_at: Option<f32>,
    /// A rail vehicle: the track it has come along, (odometer, point), oldest first -
    /// where its rear bogie and its coupled cars and sections run (see `rail_behind`).
    pub(crate) rail_trail: std::collections::VecDeque<(f64, DVec3)>,
    /// A train turned round as a whole (its last car leads now): what a trip's
    /// `[trainreverse]` is compared with (Omsi.exe's vehicle +0x4e1).
    pub(crate) consist_reversed: bool,
}

/// Where a vehicle's body stands, for the checks that go by geometry rather than by lanes:
/// the car it belongs to (a trailer or rear section counts as its own footprint), centre,
/// forward and right unit vectors, half length, half width and speed along its heading.
#[derive(Debug, Clone, Copy)]
pub(crate) struct Footprint {
    car: usize,
    center: DVec2,
    fwd: DVec2,
    right: DVec2,
    half_len: f64,
    half_w: f64,
    speed: f32,
    /// Height of the vehicle's origin (an aircraft overhead is not in a car's way).
    z: f64,
}

/// The player's vehicle as the traffic sees it: centre, heading (deg), half length, half
/// width, speed along the heading (m/s, negative when reversing).
pub type PlayerBox = (DVec3, f64, f32, f32, f32);

/// Where the player looks from, for putting cars on the road and taking them off only
/// where nobody sees it happen.
#[derive(Debug, Clone, Copy)]
pub struct Viewer {
    pub pos: DVec3,
    pub forward: DVec3,
    /// Tangents of half the horizontal and vertical field of view.
    pub tan_x: f64,
    pub tan_y: f64,
    /// Beyond this distance nothing shows (fog, or a car smaller than a pixel) (m).
    pub range: f64,
    /// What the renderer leaves out (see `::render::RenderOptions`): objects smaller on
    /// the screen than `min_size` (the original's measure), and farther than `max_dist`
    /// (0 = no limit); `fov` is the vertical field of view (radians).
    pub min_size: f64,
    pub max_dist: f64,
    pub fov: f64,
}

/// A car further away than this is below two pixels on a 900-line screen (m).
const VISIBLE_RANGE: f64 = 900.0;

/// Within this distance (m) of the camera a timetable bus has its driver at the wheel.
const DRIVER_NEAR: f64 = 70.0;

/// Within this distance of the camera no car appears or vanishes, seen or not: the mirrors
/// and a turn of the head see what is near (see [`Traffic::may_appear`]).
const NEVER_VANISH_WITHIN: f64 = 150.0;
/// Within this distance a vehicle appears or vanishes only behind something, wherever the
/// player looks (see `Traffic::hidden`).
const NEAR_HIDE: f64 = 350.0;

/// Within this distance of the camera an AI vehicle is animated and drawn even out of the
/// view (m): the mirrors look behind, and a car beside the view throws its shadow into it.
const UNSEEN_NEAR: f64 = 80.0;

/// How near an articulated AI bus has to be for its bellows to be reshaped as its joint
/// turns (m): the fold of the bend is a few centimetres, which is a screen pixel or more
/// within this range - farther out it is not worth a mesh update every frame it steers.
const SKIN_DISTANCE: f64 = 200.0;

/// How far from the player cars are kept (m) and how far out of sight one may be before it
/// is taken off (m); in plain view a car stays until it is too small to see.
const DESPAWN_FACTOR: f64 = 1.6;

/// A random car out of the player's range: still on the map and still driving, but without
/// a body, a script or a picture - a few numbers. It comes back as the same car (type,
/// paint, id) where it has got to when the player comes near, and it only ever leaves the
/// map at the end of the road network. Before, every car out of range was simply taken
/// away and new ones made up around the player: the traffic followed the player about, and
/// a car driven past was never seen again.
pub struct DormantCar {
    pub id: VehicleId,
    pub ty: Arc<VehicleType>,
    pub kind: LaneKind,
    pub lane: usize,
    pub s: f32,
    pub speed: f32,
    pub seed: u64,
    pub scheme: Option<usize>,
    /// Its own dice for the turns it takes.
    pub walk: u64,
}

/// How many cars a whole map keeps at most, as a multiple of the number asked for around the
/// player (memory: a dormant car is a few dozen bytes, but each one woken is a full vehicle).
const MAP_POPULATION_FACTOR: f32 = 8.0;

fn street_lane_weight(l: &::traffic::Lane) -> Option<f64> {
    (l.kind == LaneKind::Street && !l.no_cars && l.density > 0.0 && l.length() >= 8.0)
        .then(|| l.length() as f64 * l.density.clamp(0.05, 4.0) as f64)
}

pub struct Traffic {
    net: Network,
    /// Sum of the spawn weights of every street lane, updated only as tiles add lanes.
    street_weight: f64,
    /// Parked cars standing in or beside a lane: per lane, (distance along it, signed
    /// lateral offset of the car's centre, + = right). A car in the lane's middle is an
    /// obstacle to stop behind; one over the kerb side is passed with a swerve to the left.
    parked: HashMap<usize, Vec<(f32, f32)>>,
    /// Parked cars no lane has been found beside yet: the lane may come with a tile that is
    /// not loaded yet (a road spline often starts in the next tile). Most stand in car parks
    /// and stay here.
    parked_waiting: Vec<DVec3>,
    /// Tiles whose lanes the network has (lanes stay once they are in).
    lane_tiles: hashbrown::HashSet<(i32, i32)>,
    /// Counts the times tiles brought their lanes: whoever resolved something against the
    /// network and missed a part of it looks again when this changes.
    lanes_generation: u64,
    /// AI vehicle types with weight, the lane kind they run on (`[type]` 2 rail, 3 air)
    /// and their group in `groups`.
    types: Vec<(Arc<VehicleType>, f32, LaneKind, usize)>,
    /// The random traffic groups: name, `unsched_trafficdens.txt` factor and day curves.
    groups: Vec<::map::ailists::UnschedGroup>,
    /// The map has an `unsched_trafficdens.txt` (else the global.cfg curve applies).
    group_curves: bool,
    /// Each group's place in `unsched_vehgroups.txt`, the number a path's `[rule]
    /// trafficdensity` names it by (None: the map has no such file, and every group drives
    /// wherever the lane's density lets traffic).
    group_uvg: Vec<Option<usize>>,
    /// The default density of every `unsched_vehgroups.txt` entry, in file order: 0 none, 1
    /// for the first entry its medium density, for any other that of the first entry, 2 of
    /// the second, and so on. It applies on the paths without a rule for the group.
    uvg_defaults: Arc<Vec<i32>>,
    cars: Vec<AiCar>,
    /// The random cars out of range (see `DormantCar`).
    dormant: Vec<DormantCar>,
    /// `time` when the dormant cars last moved on.
    dormant_time: f32,
    rng: u64,
    /// Target number of cars around the camera.
    target: usize,
    /// Made only so that the light programs run (no traffic, no timetable): nobody is put
    /// on the roads - no aircraft, no parked car pulling out - while `target` is 0.
    lights_only: bool,
    spawn_radius: f64,
    time: f32,
    /// Renders of cars that have gone, given back at the next `sync`.
    released: Vec<VehicleRender>,
    /// Where the camera is (the window sets it before `sync`): far cars show their script
    /// textures as stand-ins.
    camera: Option<DVec3>,
    /// Sound sets of despawned cars, stopped at the next audio update.
    orphan_sounds: Vec<::audio::SoundSet>,
    lights: Vec<TrafficLightController>,
    controller_of_object: HashMap<i64, usize>,
    /// Coupled vehicle types by file.
    trailer_types: HashMap<std::path::PathBuf, Option<Arc<VehicleType>>>,
    /// `[sound_ai]` configurations by file.
    sound_cfgs: HashMap<std::path::PathBuf, Option<Arc<::legacy_vehicle::SoundCfg>>>,
    root: std::path::PathBuf,
    /// Car-frames spent waiting for a red light (statistics).
    held_at_red: usize,
    /// Who wants a timetable bus to stop (`Humans::stop_wishes`): the buses somebody
    /// aboard wants to get off, the stops where somebody waits. None without passengers:
    /// every bus then serves every stop.
    stop_wishes: Option<(hashbrown::HashSet<VehicleId>, hashbrown::HashSet<i64>)>,
    /// Seconds the player's vehicle has been standing.
    player_still: f32,
    /// Time of day (seconds since midnight); light cycles and timetables run on it.
    day_time: f64,
    /// How fast the clock runs (the time speed): the timetable keeps to it.
    time_scale: f64,
    /// Day of the week (0 Monday … 6 Sunday) for the traffic density curves.
    weekday: i32,
    /// Street lights on → AI vehicles switch their lights on.
    night: bool,
    /// The light of the day, for the cars' `Envir_Brightness` (see `sync`).
    daylight: Option<::simulation::Daylight>,
    next_id: u64,
    /// The last car that started an overtake and when (for chase-camera debugging).
    last_overtaker: Option<(VehicleId, f32)>,
    /// The first car that entered a turning lane and when (`--follow turn`).
    first_turner: Option<(VehicleId, f32)>,
    /// The first car that stopped at a red light (`--follow red`), the first that gave way
    /// at a junction (`--follow yield`), the first that pulled out onto the other side of
    /// the road round an obstacle or squeezed past a bus at its stop (`--follow pass`).
    first_red: Option<(VehicleId, f32)>,
    first_yield: Option<(VehicleId, f32)>,
    first_passer: Option<(VehicleId, f32)>,
    /// `[trafficdensity_road]` curve of the map: (hour, factor).
    density_curve: Vec<(f32, f32)>,
    /// The options' `[AIUnschedFactor]`: the share of the random traffic.
    unsched_factor: f32,
    /// The options' `[AIMaxCountScheduled]` (0 = no limit).
    max_scheduled: u32,
    /// Where the player looks from (set every frame).
    viewer: Option<Viewer>,
    /// Buildings that hide what is behind them (the player's collision world).
    occluders: Option<Arc<::simulation::collision::CollisionWorld>>,
    /// Current streamed scenery collision, independent of the player's visibility inputs.
    road_collision: Arc<::simulation::collision::CollisionWorld>,
    /// Pedestrians on the footpaths: (lane, distance along it), for giving way at crossings
    /// and for the pedestrian lights' request buttons.
    walkers: Vec<(usize, f32)>,
    /// Everybody on foot on the ground: position, velocity and whether they are waiting
    /// at a stop (set every frame) - the cars stop for anybody in their way, not only on
    /// a crossing.
    people: Vec<(DVec2, DVec2, bool)>,
    /// No car has been placed yet: the first population may fill the view.
    initial: bool,
    /// Seconds of the last tick (the lamp scripts run in `sync`).
    last_dt: f32,
    /// Game time since the lamps' scripts last ran (see `sync`).
    lamp_dt: f32,
    /// `OMSI_TRACE_AI=<file.csv>`: every car's pose, steering and speed, every frame.
    trace: Option<std::io::BufWriter<std::fs::File>>,
    /// Cars already reported for a hard bend (`OMSI_DEBUG_TRAFFIC`).
    logged_hard: hashbrown::HashSet<VehicleId>,
    /// `OMSI_DEBUG_LIGHTS=all|near|<controller>,…`: which programs log their changes, and
    /// the states they showed last.
    light_log: Option<String>,
    light_prev: Vec<Vec<i32>>,
    /// `OMSI_DEBUG_POPULATION`: log where cars appear and vanish relative to the view.
    debug_population: bool,
    /// Cars placed since the last look inside the view frustum (hidden behind something):
    /// (id, position). `OMSI_POPULATION_SHOTS` photographs them to check.
    framed_spawns: Vec<(VehicleId, DVec3)>,
    /// The player's vehicle as of the last tick (nothing is put on the road on top of it).
    player: Option<PlayerBox>,
    /// The player's bus has right of way over the traffic (its script's `TrafficPriority`,
    /// OMSI: priority 1000 over the types' own): cars keep out of the way it is about
    /// to take for longer.
    player_priority: bool,
    player_emergency: bool,
    /// The LAN players' vehicles (their session ids and boxes as for the player), set
    /// before each `tick`: the cars stop behind them and go round them as round the
    /// player's bus.
    others: Vec<(u32, PlayerBox)>,
    /// The drivers at the wheel of the timetable buses near the camera, by car id (see
    /// `driver.rs`; made within `DRIVER_NEAR` m of the camera, let go beyond twice that).
    drivers: HashMap<VehicleId, crate::driver::DriverFigure>,
    /// Figures let go by their bus, hidden, for the next one (their GPU meshes stay).
    driver_pool: Vec<crate::driver::DriverFigure>,
    /// Where the last `tick` spent its time (s, OMSI_PROFILE): who is on which lane and the
    /// light programs, every car's plan, the bodies and scripts on the workers.
    tick_split: [f64; 3],
    /// Seconds each of them has stood still.
    others_still: HashMap<u32, f32>,
    /// Per car: `AiCar::geo_block` of the frame before (who waits for whom by geometry),
    /// keyed by stable id so a container reorder changes nothing.
    geo_prev: HashMap<VehicleId, Option<VehicleId>>,
    /// Car index by id (as of the start of the tick).
    index_of: HashMap<VehicleId, usize>,
    /// The junction coordinator: the single owner of admission, claims and the wait-for graph.
    junctions: JunctionCoordinator,
    /// The service coordinator: the single owner of berth capacity and service transitions.
    services: ServiceCoordinator,
    /// The maneuver coordinator: the single owner of every lateral maneuver on the road.
    maneuvers: ManeuverCoordinator,
    /// The population coordinator: the single owner of demand, admission, the dormant
    /// lifecycle and topology demand (`traffic::population`).
    population: PopulationCoordinator,
    /// `pull_out_room` by vehicle file.
    pull_out_rooms: HashMap<std::path::PathBuf, f32>,
    /// Timetable buses taken off the road because the tile under them was unloaded (their
    /// ids), for the timetable to put them back when the tiles come again.
    removed_scheduled: Vec<VehicleId>,
    /// One-way lanes that have had their reverse twin added (`add_reverse_twins`).
    twinned: hashbrown::HashSet<usize>,
    /// Bodies besides the AI vehicles' that no timetable vehicle may be put into: the
    /// player's vehicle and the LAN players' (set before each `Schedule::tick`).
    keep_clear: Vec<::simulation::collision::Obb>,
    /// LAN play: this game draws the host's traffic instead of its own (`lan_world`).
    mirror: bool,
    /// Count only the cars within this distance of this point when filling up (the
    /// population around a LAN player, `populate_lan_centers`).
    count_near: Option<(DVec3, f64)>,
    /// LAN play: where the other players are (host): the traffic is kept around them too.
    lan_centers: Vec<DVec3>,
    /// Optional automatic failure capture: where to persist it and the rolling trace.
    capture: Option<(std::path::PathBuf, Capture)>,
    /// The capture has already been persisted (persist once per run).
    capture_written: bool,
}

/// `[boundingbox]` of a vehicle that gives none.
const DEFAULT_BOX: [f32; 6] = [2.5, 12.0, 3.0, 0.0, 0.0, 1.5];

/// The bodies of a vehicle and of the parts coupled to it, where they stand.
pub fn vehicle_bodies(v: &VehicleInstance) -> Vec<::simulation::collision::Obb> {
    let mut out = vec![::simulation::collision::Obb::from_box(
        v.ty.def.bounding_box.unwrap_or(DEFAULT_BOX),
        v.position,
        v.heading,
    )];
    for t in &v.trailers {
        let heading = if t.reversed {
            t.heading + 180.0
        } else {
            t.heading
        };
        out.push(::simulation::collision::Obb::from_box(
            t.ty.def.bounding_box.unwrap_or(DEFAULT_BOX),
            t.position,
            heading,
        ));
    }
    out
}

/// Log the content defects of a freshly compiled network (`OMSI_DEBUG_NETWORK`). The
/// network is not changed: wrong or ambiguous content is diagnosed for its owner.
fn report_network_defects(net: &Network) {
    if ::legacy_config::env::var_os("OMSI_DEBUG_NETWORK").is_none() {
        return;
    }
    let d = net.validate();
    if d.is_clean() {
        return;
    }
    log::warn!("traffic network: {} content defects", d.len());
    for defect in d.defects.iter().take(40) {
        log::warn!("  {defect:?}");
    }
}

/// Persist an automatic failure capture as a self-contained text trace.
fn write_capture(path: &std::path::Path, cap: &Capture) {
    use std::io::Write;
    let Ok(mut f) = std::fs::File::create(path) else {
        log::warn!("traffic capture: cannot write {}", path.display());
        return;
    };
    let _ = writeln!(
        f,
        "trace_version={} seed={} tick_hz={} network_version={}",
        cap.header.trace_version, cap.header.seed, cap.header.tick_hz, cap.header.network_version
    );
    for t in &cap.ticks {
        for v in &t.vehicles {
            let _ = writeln!(
                f,
                "tick={} time={:.3} id={} lane={} s={:.2} speed={:.2} binding={:?}",
                t.tick, t.sim_time, v.id, v.lane, v.s, v.speed, v.binding
            );
        }
    }
    for e in &cap.events {
        let _ = writeln!(f, "event {e:?}");
    }
    let _ = writeln!(f, "decision_hash={:016X}", cap.decision_hash());
    let _ = writeln!(f, "trigger={:?}", cap.trigger);
    log::warn!("traffic capture written to {}", path.display());
}

/// Cruising speed of an AI aircraft where its flight path sets no limit (km/h): an
/// airliner on its final approach.
const AIRCRAFT_KMH: f32 = 280.0;
/// How far ahead a car looks for other vehicles at least (m).
const LOOK_AHEAD: f32 = 70.0;
/// ... and at most, when it is fast.
const LOOK_AHEAD_MAX: f32 = 150.0;

/// How far ahead a driver at `speed` watches for something standing in the way: far enough
/// to slow down gently for it. With a fixed 70 m a car at 50-65 km/h first saw the player's
/// bus standing (or a bus at its stop) so late that the following model braked at 4-5 m/s².
/// A timetable bus's IBIS moves on to its next stop as the driver would press it on: the
/// stock scripts' interior displays, announcements and side displays read `IBIS_busstop`
/// (an index into the depot file's stop list of the route), which nothing moved on an AI
/// bus - its saloon display stood on the first stop for the whole trip. `remaining` is the
/// number of stops still to come.
pub(crate) fn ibis_to_next_stop(v: &mut VehicleInstance, remaining: usize) {
    let Some(ri) = v.var("IBIS_RouteIndex").filter(|r| *r >= 0.0) else {
        return;
    };
    let Some(n) = v
        .host
        .hof
        .as_ref()
        .and_then(|h| h.info_busstop_lists.get(ri as usize))
        .map(|l| l.len())
    else {
        return;
    };
    if n == 0 || v.var("IBIS_busstop").is_none() {
        return;
    }
    let idx = n.saturating_sub(remaining.max(1)).min(n - 1);
    v.set_var("IBIS_busstop", idx as f32);
}

fn look_ahead(speed: f32) -> f32 {
    (speed * speed / 3.0 + speed * 2.0 + 20.0).clamp(LOOK_AHEAD, LOOK_AHEAD_MAX)
}

/// Ground height for an AI vehicle's wheels: the road surface (blended between raster
/// texels), else any surface, else the terrain.
fn ai_ground(world: &World) -> Arc<dyn Fn(f64, f64) -> Option<f64> + Send + Sync> {
    let terrains = world.terrains.clone();
    let surfaces = world.surfaces.clone();
    Arc::new(move |x, y| {
        let tx = (x / ::map::tile_size()).floor() as i32;
        let ty = (y / ::map::tile_size()).floor() as i32;
        let lx = (x - tx as f64 * ::map::tile_size()) as f32;
        let ly = (y - ty as f64 * ::map::tile_size()) as f32;
        let surface = surfaces.read().get(&(tx, ty)).cloned();
        if let Some(h) = surface.and_then(|s| s.sample_road_smooth(lx, ly)) {
            return Some(h as f64);
        }
        let t = terrains.read();
        Some(t.get(&(tx, ty))?.sample(lx, ly) as f64)
    })
}

/// The bare ground at a point (no roads, decks or platforms on it).
fn terrain_height(world: &World, x: f64, y: f64) -> Option<f64> {
    let tx = (x / ::map::tile_size()).floor() as i32;
    let ty = (y / ::map::tile_size()).floor() as i32;
    let t = world.terrains.read();
    let terrain = t.get(&(tx, ty))?;
    Some(terrain.sample(
        (x - tx as f64 * ::map::tile_size()) as f32,
        (y - ty as f64 * ::map::tile_size()) as f32,
    ) as f64)
}

/// How a vehicle on lanes of `kind` moves.
fn motion_kind(kind: LaneKind) -> MotionKind {
    match kind {
        LaneKind::Air => MotionKind::Air,
        LaneKind::Rail => MotionKind::Rail,
        _ => MotionKind::Road,
    }
}

/// The maneuver phase a service phase is doing its lateral half in (docking or merge-out).
fn service_maneuver_phase(phase: ServicePhase) -> ManeuverPhase {
    match phase {
        ServicePhase::Departing => ManeuverPhase::Departing,
        _ => ManeuverPhase::Docking,
    }
}

/// How far ahead of the player's bus centre a car looks for it (m): the bus's half length,
/// and where it will be in `horizon` seconds for a car whose way crosses the bus's. Not
/// for one going the same way (`way_dir` within 60 degrees of the bus's heading): with the
/// bus behind it, that stretch ahead of the bus reached over the car itself and it braked
/// for a bus that was only following it (#139).
fn player_reach_ahead(half_len: f32, speed: f32, horizon: f32, fwd: DVec2, way_dir: DVec2) -> f64 {
    let same_way = way_dir.length() > 0.5 && way_dir.normalize().dot(fwd) > 0.5;
    half_len as f64
        + if same_way {
        0.0
    } else {
        (speed.max(0.0) * horizon) as f64
    }
}

/// How much track an AI rail vehicle keeps behind it (m): a long train's length.
const RAIL_TRAIL: f64 = 400.0;

/// Note where an AI rail vehicle is: `odometer` (m) and the point of its way there. A jump
/// (put somewhere else, turned round at a terminus) starts the trail afresh.
fn record_rail_trail(
    trail: &mut std::collections::VecDeque<(f64, DVec3)>,
    odometer: f64,
    here: DVec3,
) {
    if let Some(&(u, p)) = trail.back() {
        if (here - p).truncate().length() > (odometer - u).abs() + 2.0 {
            trail.clear();
        } else if (odometer - u).abs() <= 0.5 {
            return;
        }
    }
    // (backing up takes the trail back with it)
    while trail.back().is_some_and(|b| b.0 > odometer) {
        trail.pop_back();
    }
    trail.push_back((odometer, here));
    while trail.front().is_some_and(|f| odometer - f.0 > RAIL_TRAIL) {
        trail.pop_front();
    }
}

/// The point of an AI rail vehicle's track `d` metres behind its origin: on the trail it
/// came along. (Its way knows only the lane it came off; farther back it runs straight on,
/// and a train's last cars stood beside the track after a pair of points.) Where the trail
/// does not reach - the last half metre, a vehicle just put there - the way.
fn rail_behind(
    trail: &std::collections::VecDeque<(f64, DVec3)>,
    state: &AiState,
    net: &Network,
    d: f64,
) -> DVec3 {
    let u = state.odometer as f64 - d;
    let newest = trail.back().map_or(f64::MIN, |b| b.0);
    if u >= newest {
        return state.way_point(net, -d as f32);
    }
    crate::rail_drive::point_at(trail, u).unwrap_or_else(|| state.way_point(net, -d as f32))
}

/// A body for a vehicle that has just been put on the way `state` describes, with the
/// vehicle posed on it.
fn place_body(
    net: &Network,
    state: &AiState,
    vehicle: &mut VehicleInstance,
    kind: MotionKind,
) -> AiBody {
    let mut body = AiBody::new(&vehicle.ty.def, kind);
    let ground = vehicle.ground.clone();
    let contact = vehicle.contact.clone();
    body.place(
        &|d| state.way_point(net, d),
        ground
            .as_ref()
            .map(|g| g.as_ref() as &dyn Fn(f64, f64) -> Option<f64>),
        contact.as_deref(),
        state.speed,
    );
    body.apply(vehicle);
    body
}

/// The `OMSI_TRACE_AI` file, with its header written.
fn open_trace() -> Option<std::io::BufWriter<std::fs::File>> {
    use std::io::Write;
    let path = ::legacy_config::env::var_os("OMSI_TRACE_AI")?;
    let mut f = std::io::BufWriter::new(
        std::fs::File::create(&path)
            .map_err(|e| log::warn!("OMSI_TRACE_AI: {e}"))
            .ok()?,
    );
    writeln!(f, "t,id,type,x,y,z,heading,pitch,bank,steer,speed,lane,s,blinker,turn,lane_heading,lateral,at_station,acc,yielding,light_hold,passing,front,rear,half_width,scheduled,why,why_gap,phase,lane_z").ok()?;
    Some(f)
}

/// The vehicle's extent from its origin: (to the front bumper, to the rear bumper, half
/// the width) from its `[boundingbox]`.
pub(crate) fn extents(ty: &VehicleType, length: f32) -> (f32, f32, f32) {
    match ty.def.bounding_box {
        Some(bb) if bb[1] > 1.0 => (
            bb[1] * 0.5 + bb[4],
            bb[1] * 0.5 - bb[4],
            (bb[0] * 0.5).max(0.5),
        ),
        // without a `[boundingbox]` the model's own box, as Omsi.exe takes it (0x7b5da4):
        // the Berlin S-Bahn's cars, 18 m long, counted as 12 m ones
        _ => match ty.model_box() {
            Some((lo, hi)) if hi.y - lo.y > 1.0 => {
                (hi.y.max(0.5), (-lo.y).max(0.5), (hi.x.max(-lo.x)).max(0.5))
            }
            _ => (length * 0.5, length * 0.5, 0.9),
        },
    }
}

/// The driver of a random car: how fast, how close, how patient (see `AiState`).
fn personality(state: &mut AiState, seed: u64, heavy: bool) {
    let r = |k: u32| ((seed >> k) & 0xff) as f32 / 255.0;
    state.desire = if heavy {
        0.88 + 0.1 * r(3)
    } else {
        0.9 + 0.22 * r(3)
    };
    state.headway = 1.0 + 0.8 * r(11);
    state.min_gap = 1.6 + 1.4 * r(19);
    state.accel = if heavy {
        0.8 + 0.4 * r(27)
    } else {
        1.3 + 1.0 * r(27)
    };
    state.decel = if heavy { 1.6 } else { 2.0 + 0.8 * r(35) };
    state.accept_gap = 3.0 + 2.5 * r(43);
    state.reaction = 0.4 + 0.8 * r(51);
}

/// Where a vehicle meets a crossing lane on its way: its lane in the sequence, the distance
/// from its origin to that lane's start.
/// Seconds until a vehicle `dist` metres from a point gets its front there, from speed `v`
/// with acceleration `a`.
fn time_to(dist: f32, v: f32, a: f32) -> f32 {
    if dist <= 0.0 {
        return 0.0;
    }
    let a = a.max(0.3);
    // v t + a t² / 2 = dist
    (-v + (v * v + 2.0 * a * dist).sqrt()) / a
}

impl Traffic {
    /// Number of active AI cars (read-only query; prefer this over the `cars` field).
    pub fn car_count(&self) -> usize {
        self.cars.len()
    }

    /// The active AI cars.
    pub fn cars(&self) -> &[AiCar] {
        &self.cars
    }

    /// The active AI cars, mutably. The engine adapter (population/schedule/LAN) owns broad
    /// mutation of the fleet; prefer [`Traffic::car_mut_by_id`] or a narrower command when one
    /// vehicle is meant.
    pub(crate) fn cars_mut(&mut self) -> &mut Vec<AiCar> {
        &mut self.cars
    }

    /// One active AI car.
    pub fn car(&self, ci: usize) -> &AiCar {
        &self.cars[ci]
    }

    /// One active AI car, mutably.
    pub(crate) fn car_mut(&mut self, ci: usize) -> &mut AiCar {
        &mut self.cars[ci]
    }

    /// The active AI car with this stable id, mutably. Targeted alternative to scanning
    /// [`Traffic::cars_mut`] for an event or display update aimed at one vehicle.
    pub(crate) fn car_mut_by_id(&mut self, id: VehicleId) -> Option<&mut AiCar> {
        self.cars.iter_mut().find(|c| c.id == id)
    }

    /// The current random-traffic population target.
    pub fn target(&self) -> usize {
        self.target
    }

    /// Set the random-traffic population target.
    pub fn set_target(&mut self, target: usize) {
        self.target = target;
    }

    /// The per-frame view inputs the population and lighting use.
    pub fn set_viewer(&mut self, viewer: Option<Viewer>) {
        self.viewer = viewer;
    }

    /// The per-frame world inputs: day of week, footpath walkers, pedestrians, occluders.
    pub fn set_world_inputs(
        &mut self,
        weekday: i32,
        walkers: Vec<(usize, f32)>,
        people: Vec<(DVec2, DVec2, bool)>,
        occluders: Option<Arc<::simulation::collision::CollisionWorld>>,
    ) {
        self.weekday = weekday;
        self.walkers = walkers;
        self.people = people;
        self.occluders = occluders;
    }

    /// Replace the player/LAN bodies traffic must keep clear of.
    pub fn set_keep_clear(&mut self, boxes: Vec<Obb>) {
        self.keep_clear = boxes;
    }

    /// Replace the external road users (player/remote outlines) traffic sees this tick.
    pub fn set_external_actors(&mut self, others: Vec<(u32, PlayerBox)>) {
        self.others = others;
    }

    /// Whether the player's vehicle has traffic priority (opens depot gates on request).
    pub fn set_player_priority(&mut self, priority: bool) {
        self.player_priority = priority;
    }

    pub fn set_player_emergency(&mut self, active: bool) {
        self.player_emergency = active;
    }

    /// Where the camera is, for population visibility.
    pub fn set_camera(&mut self, camera: Option<DVec3>) {
        self.camera = camera;
    }

    /// A car's realized world position (for the LAN mirror's odometer).
    pub fn car_vehicle_pos(&self, ci: usize) -> DVec3 {
        self.cars[ci].vehicle.position
    }

    /// The current network generation (bumped when streamed lanes are added).
    pub fn lanes_generation(&self) -> u64 {
        self.lanes_generation
    }

    /// The traffic network.
    pub fn net(&self) -> &Network {
        &self.net
    }

    /// Where the last tick spent its time.
    pub fn tick_split(&self) -> [f64; 3] {
        self.tick_split
    }

    /// The last car that started an overtake and when (chase-camera debugging).
    pub fn last_overtaker(&self) -> Option<(VehicleId, f32)> {
        self.last_overtaker
    }

    pub fn first_turner(&self) -> Option<(VehicleId, f32)> {
        self.first_turner
    }

    pub fn first_red(&self) -> Option<(VehicleId, f32)> {
        self.first_red
    }

    pub fn first_yield(&self) -> Option<(VehicleId, f32)> {
        self.first_yield
    }

    pub fn first_passer(&self) -> Option<(VehicleId, f32)> {
        self.first_passer
    }

    /// The engine's time-speed factor.
    pub fn set_time_scale(&mut self, scale: f64) {
        self.time_scale = scale;
    }

    /// The calendar/service time schedules use.
    pub fn day_time(&self) -> f64 {
        self.day_time
    }

    /// Set the calendar/service time.
    pub fn set_day_time(&mut self, day_time: f64) {
        self.day_time = day_time;
    }

    /// Advance the calendar/service time.
    pub fn advance_day_time(&mut self, dt: f64) {
        self.day_time += dt;
    }

    /// Take the recorded in-frame spawns (for the population-shots diagnostic).
    pub fn take_framed_spawns(&mut self) -> Vec<(VehicleId, DVec3)> {
        std::mem::take(&mut self.framed_spawns)
    }

    fn rand(&mut self) -> u64 {
        let mut x = self.rng;
        x ^= x >> 12;
        x ^= x << 25;
        x ^= x >> 27;
        self.rng = x;
        x.wrapping_mul(0x2545_F491_4F6C_DD1D)
    }

    fn rand_f(&mut self) -> f64 {
        (self.rand() >> 11) as f64 / (1u64 << 53) as f64
    }
}

/// What the tiles placed since the last call hand to the traffic: their lanes, their parked
/// cars and which tiles they were (taken together, see `World::lane_tiles`).
fn take_from_tiles(
    world: &World,
) -> (
    Vec<::traffic::Lane>,
    Vec<(DVec3, f64)>,
    Vec<(i32, i32)>,
) {
    let mut lanes = world.lanes.lock();
    let parked = std::mem::take(&mut *world.parked_cars.lock());
    let tiles = std::mem::take(&mut *world.lane_tiles.lock());
    let mut new = std::mem::take(&mut *lanes);
    // The tiles are read in parallel and hand in their lanes in the order they finish:
    // sorted by their map identity, the lanes a set of tiles brings are numbered alike in
    // every run, and so is the random traffic drawn from them (a run can be repeated to
    // look at what a car did).
    new.sort_by(|a, b| {
        let first = |l: &::traffic::Lane| {
            l.points
                .first()
                .map(|p| (p.x.to_bits(), p.y.to_bits()))
                .unwrap_or((0, 0))
        };
        (
            a.key.map(|k| (k.tile, k.id, k.path)),
            a.reversed,
            a.source,
            first(a),
        )
            .cmp(&(
                b.key.map(|k| (k.tile, k.id, k.path)),
                b.reversed,
                b.source,
                first(b),
            ))
    });
    (new, parked, tiles)
}

/// How many times the base street target the neighbourhood asks for, from the trafficdensity
/// of each street lane starting near the player (0 for a no_cars lane): the count of lanes
/// per about 250 (1 to 4) times their mean density (0 to 2).
fn road_scale(near_density: &[f32]) -> f32 {
    let road = (near_density.len() as f32 / 250.0).clamp(1.0, 4.0);
    if near_density.is_empty() {
        return road;
    }
    let mean = near_density.iter().sum::<f32>() / near_density.len() as f32;
    road * mean.clamp(0.0, 2.0)
}

/// Whether the point `p` of a car's way lies in the player's box round `centre` (`wide` to
/// either side, `ahead` in front and `behind` behind it) - on the same level only: a bus
/// under a bridge held up the traffic on the bridge above it (#753). 4 m, as for the other
/// vehicles' bodies.
fn in_player_box(
    p: DVec3,
    centre: DVec3,
    fwd: DVec2,
    right: DVec2,
    wide: f64,
    ahead: f64,
    behind: f64,
) -> bool {
    let rel = p.truncate() - centre.truncate();
    let (x, y) = (rel.dot(right), rel.dot(fwd));
    x.abs() <= wide && y <= ahead && y >= -behind && (p.z - centre.z).abs() < 4.0
}

#[cfg(test)]
mod road_scale_tests {
    use super::road_scale;

    #[test]
    fn path_density_scales_the_street_target() {
        assert!((road_scale(&[1.0; 100]) - 1.0).abs() < 1e-6);
        assert!((road_scale(&[0.2; 100]) - 0.2).abs() < 1e-6);
        assert_eq!(road_scale(&[0.0; 100]), 0.0);
        assert!((road_scale(&[1.0; 500]) - 2.0).abs() < 1e-6);
    }
}

#[cfg(test)]
mod junction_arrival_tests {
    use ::traffic::{crossing_arrival, AiState};

    #[test]
    fn stopped_queue_does_not_predict_a_restart() {
        let st = AiState::new(0, 0.0, 1);
        assert_eq!(crossing_arrival(&st, 10.0, false, false, true), f32::MAX);
    }

    #[test]
    fn crawling_queue_is_measured_at_its_actual_speed() {
        let mut st = AiState::new(0, 0.0, 1);
        for speed in [0.2, 0.3, 0.4, 0.5, 0.8, 1.4] {
            st.speed = speed;
            assert!((crossing_arrival(&st, 10.0, false, false, true) - 10.0 / speed).abs() < 1e-5);
        }
    }

    #[test]
    fn crawling_queue_allows_a_gap_but_nearby_traffic_still_counts() {
        let mut st = AiState::new(0, 0.0, 1);
        st.speed = 0.4;
        let gap = 7.5;
        assert!(crossing_arrival(&st, 10.0, false, false, true) > gap);
        assert!(crossing_arrival(&st, 1.0, false, false, true) < gap);
        // Once it can move freely again, account for it accelerating towards the crossing.
        assert!(crossing_arrival(&st, 10.0, true, false, false) < gap);
    }

    #[test]
    fn queue_already_at_the_conflict_still_blocks() {
        let st = AiState::new(0, 0.0, 1);
        for distance in [-1.0, 0.0, 0.3] {
            assert_eq!(crossing_arrival(&st, distance, false, true, true), 0.0);
        }
    }

    #[test]
    fn freely_starting_car_keeps_its_accelerating_prediction() {
        let st = AiState::new(0, 0.0, 1);
        let expected = (20.0 / st.accel).sqrt() + st.reaction;
        assert!((crossing_arrival(&st, 10.0, false, false, false) - expected).abs() < 1e-5);
        assert!((crossing_arrival(&st, 10.0, true, false, false) - expected).abs() < 1e-5);
    }

    #[test]
    fn car_waiting_before_the_conflict_is_not_approaching() {
        let mut st = AiState::new(0, 0.0, 1);
        st.speed = 2.0;
        assert_eq!(crossing_arrival(&st, 10.0, false, true, false), f32::MAX);
        assert!(crossing_arrival(&st, 10.0, true, false, false) < 5.0);
        assert_eq!(crossing_arrival(&st, 10.0, false, false, false), 5.0);
    }
}

#[cfg(test)]
mod group_density_tests {
    use super::player_reach_ahead;
    use glam::DVec2;
    use ::traffic::pool_density as uvg_density;

    /// Berlin-Spandau's `unsched_vehgroups.txt`: NormalCars 1, Trucks 0, Commercials 1,
    /// Ambulance 1, GDRCars 0.
    const SPANDAU: [i32; 5] = [1, 0, 1, 1, 0];

    #[test]
    fn a_bus_under_a_bridge_is_not_in_the_way_on_it() {
        use super::in_player_box;
        use glam::DVec3;
        let (c, f, r) = (
            DVec3::new(0.0, 0.0, 32.0),
            DVec2::new(0.0, 1.0),
            DVec2::new(1.0, 0.0),
        );
        // the road through the bus's box, on its level and on a bridge 5.4 m above it
        assert!(in_player_box(
            DVec3::new(0.5, 3.0, 32.3),
            c,
            f,
            r,
            2.5,
            6.0,
            6.0
        ));
        assert!(!in_player_box(
            DVec3::new(0.5, 3.0, 37.4),
            c,
            f,
            r,
            2.5,
            6.0,
            6.0
        ));
        assert!(!in_player_box(
            DVec3::new(0.5, 3.0, 26.0),
            c,
            f,
            r,
            2.5,
            6.0,
            6.0
        ));
        // beside it
        assert!(!in_player_box(
            DVec3::new(3.5, 3.0, 32.0),
            c,
            f,
            r,
            2.5,
            6.0,
            6.0
        ));
    }

    #[test]
    fn a_following_bus_is_no_bus_in_the_way() {
        let north = DVec2::new(0.0, 1.0);
        // a car ahead going the same way: only the bus itself counts
        assert_eq!(
            player_reach_ahead(6.0, 14.0, 1.5, north, DVec2::new(0.1, 3.0)),
            6.0
        );
        // a car crossing its way (or coming towards it): where the bus will be counts too
        assert_eq!(
            player_reach_ahead(6.0, 14.0, 1.5, north, DVec2::new(3.0, 0.0)),
            27.0
        );
        assert_eq!(
            player_reach_ahead(6.0, 14.0, 1.5, north, DVec2::new(0.0, -3.0)),
            27.0
        );
    }

    #[test]
    fn a_group_off_by_default_drives_where_a_path_asks_for_it() {
        // a Falkensee path: no rule for the normal cars, the GDR cars asked for
        let rules = [(4u16, 1.0f32)];
        assert_eq!(uvg_density(&rules, &SPANDAU, 4), 1.0);
        assert_eq!(uvg_density(&rules, &SPANDAU, 0), 1.0);
        // and nowhere else
        assert_eq!(uvg_density(&[], &SPANDAU, 4), 0.0);
        assert_eq!(uvg_density(&[(0, 0.5)], &SPANDAU, 1), 0.0);
    }

    #[test]
    fn a_default_follows_the_first_group_on_the_path() {
        // commercials (default 1) take the normal cars' density of the path
        assert_eq!(uvg_density(&[(0, 0.4)], &SPANDAU, 2), 0.4);
        assert_eq!(uvg_density(&[(0, 0.0)], &SPANDAU, 2), 0.0);
        assert_eq!(uvg_density(&[], &SPANDAU, 2), 1.0);
        // an own rule wins
        assert_eq!(uvg_density(&[(0, 0.4), (2, 2.0)], &SPANDAU, 2), 2.0);
    }

    #[test]
    fn defaults_naming_each_other_end() {
        assert_eq!(uvg_density(&[], &[1, 3, 2], 1), 0.0);
    }
}
