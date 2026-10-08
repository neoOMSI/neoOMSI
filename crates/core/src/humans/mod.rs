//! People on foot: passengers and pedestrians as agents with a goal.
//!
//! A passenger comes along the pavement (or already stands at the stop when the map
//! starts), waits at a free waiting place of the stop - the `[passpos]` points of the
//! map's `people_standing_*` markers and shelters, else places spread along the back of
//! the platform - and when a bus opens its doors there, queues at the nearest open
//! `[entry]` (a passenger who still has to buy a ticket only at one with a cash desk),
//! steps in when the doorway is free, pays or shows a pass at the desk, walks the cabin's
//! `paths.cfg` network to a free `[passpos]` (optionally preferring seated places),
//! rides, presses the stop button before their stop, walks to the nearest `[exit]` when
//! the bus stands there, steps out and walks away along the pavement - or waits at the
//! stop for another bus. Timetable (AI) buses carry their passengers the same way.
//! Nobody is taken away while the player can see them.
//!
//! An articulated bus is one cabin: the sections' path networks, seats and exits are put
//! together in the front section's frame with the sections straight behind each other, and
//! the front section's `[linkToPrevVeh]` point is joined to the rear section's
//! `[linkToNextVeh]` point, so people walk through the bellows to the seats and exits at the
//! back. Entries and exits are numbered front section first, which is how the stock door
//! scripts count them (the GN92's rear door is `PAX_Exit2`/`PAX_Exit3`). A point behind a
//! joint is carried by its own section, whatever the angle of the bend.
//!
//! Movement is a crowd: everybody on the same floor (the ground, or one bus) avoids
//! everybody else with the anticipatory model of `::simulation::crowd`, does not push into
//! somebody standing in front, speeds up, slows down and turns at a human pace, and keeps
//! to the aisle inside a bus. Doorways and the cash desk are taken one at a time, people
//! getting off go first, and somebody pressed against another for seconds slips past.
//! Every waiting state has a way out, and `OMSI_DEBUG_PAX=1` logs every change of state
//! and why somebody stands still.
//!
//! Pedestrians walk the map's pavement paths as one network (path ends that meet are
//! joined whatever their heading), wait at the kerb for a pedestrian light's green - and
//! only start across when it lasts long enough - and for approaching cars where there is
//! no light; nobody stops in the middle of the road.
//!
//! The map streams: stops, waiting places and pavements come with their tiles. The
//! pavement network grows as the traffic network does, a stop is set up again when its
//! neighbourhood changed and nobody uses it, a stop whose tile went takes its people with
//! it, and nobody stands or walks where the ground is not loaded.

use crate::ambience;
use crate::scene::World;
use crate::traffic::Traffic;
use glam::{DVec2, DVec3, Mat4, Vec3};
use hashbrown::{HashMap, HashSet};
use ::render::{AlphaMode, Camera, MaterialId, MeshId, Renderer, Scene};
use ::simulation::VehicleInstance;
use ::simulation::crowd::{self, Block, CrowdParams, PathGraph, Walker};
use ::simulation::human::{Activity, HumanType, skin};
use ::simulation::human_omsi::{AnimInput, OmsiAnim};
use ::simulation::traffic::{LaneKind, Network};
use ::legacy_vehicle::PassengerCabin;
use rayon::prelude::*;
use std::path::{Path, PathBuf};
use std::sync::Arc;

mod figures;
use figures::{load_figures, slot_key};
mod passengers;
use passengers as pax;
mod buses;
mod pedestrians;
mod person;
mod render;
use render::{PersonRender, RenderResources};
mod animation;
mod avatars;
mod network;
mod simulation;
pub use avatars::*;
pub use buses::BusId;
use buses::*;
pub use network::*;
use pax::*;
use pedestrians::*;
pub use person::Person;
use person::*;

/// Whether any entry or exit used by passengers is open on the vehicle.
pub(crate) fn any_passenger_door_open(vehicle: &VehicleInstance) -> bool {
    Humans::any_door_open(vehicle)
}

/// The map's traffic keeps left (its stops are on the left): see the doors of `Cabin`.
pub(crate) static LEFT_HAND: std::sync::atomic::AtomicBool =
    std::sync::atomic::AtomicBool::new(false);

/// How far outside the bus side somebody stands at a door (m).
const DOOR_OUT: f32 = 0.5;
/// Body radius for the crowd outside (m): shoulders and swinging arms. With the cabin's
/// radius people on the pavement came within 0.46 m, and two walking past each other or a
/// group crossing the road merged into one another in the picture.
const BODY_OUTSIDE: f64 = 0.28;
/// Stops within this distance of the player have their people (Omsi.exe: the stop's tile
/// and the eight round the camera's, sub_61bf94).
const STOP_RANGE: f64 = 450.0;
/// Pedestrians stroll within this distance of the player (m).
const STROLL_RADIUS: f64 = 200.0;
/// Over this distance on either side of a joint (m) a point of an articulated bus's cabin
/// moves from the frame of the section in front to the one behind.
const JOINT_BLEND: f32 = 0.5;

fn debug_pax() -> bool {
    static ON: std::sync::OnceLock<bool> = std::sync::OnceLock::new();
    *ON.get_or_init(|| {
        ::legacy_config::env::var_os("OMSI_DEBUG_PAX").is_some()
            || ::legacy_config::env::var_os("OMSI_DEBUG_HUMANS").is_some()
    })
}

/// Where the player looks from, for "nobody appears or vanishes in sight".
#[derive(Debug, Clone, Copy)]
pub struct Eye {
    pub pos: DVec3,
    pub fwd: DVec3,
    /// Cosine of half the diagonal field of view, with a margin.
    pub cos_half: f64,
    pub fov_y: f64,
}

impl Eye {
    pub fn of(cam: &Camera, aspect: f32) -> Eye {
        let half_v = (cam.fov_deg as f64 * 0.5).to_radians();
        let half_diag = (half_v.tan() * (1.0 + (aspect as f64).powi(2)).sqrt()).atan();
        Eye {
            pos: cam.position,
            fwd: cam.forward().as_dvec3().normalize_or_zero(),
            fov_y: (cam.fov_deg as f64).to_radians().max(1e-3),
            cos_half: (half_diag + 10f64.to_radians())
                .min(89f64.to_radians())
                .cos(),
        }
    }
}

/// What a person wants this frame.
struct Want {
    vel: DVec2,
    /// Heading to turn to when standing (world on the ground, bus frame inside).
    face: Option<f64>,
    give: f64,
    corridor: Option<(DVec2, DVec2, f64)>,
    /// What they do when not walking.
    idle: Activity,
}

impl Want {
    fn stand(face: Option<f64>, idle: Activity) -> Want {
        Want {
            vel: DVec2::ZERO,
            face,
            give: 0.35,
            corridor: None,
            idle,
        }
    }
}

/// Velocity towards `to`, easing into the stop over the last metre.
fn arrive(from: DVec2, to: DVec2, pace: f64) -> DVec2 {
    let d = to - from;
    let dist = d.length();
    if dist < 0.1 {
        return DVec2::ZERO;
    }
    let speed = (pace * dist.min(1.0)).max(if dist > 0.3 { 0.25 } else { 0.0 });
    d / dist * speed
}

/// Seconds after one passenger's greeting or complaint before anybody says another.
const CHAT_PAUSE: f64 = 12.0;

pub struct Humans {
    avatars: Avatars,
    walking: PedestrianState,
    desk: FareDesk,
    buses: PassengerBuses,
    voice: PassengerVoices,
    network: PassengerNetwork,
    render: RenderResources,
    /// Append-only registry; indices are shared with LAN and avatars.
    types: Vec<Arc<HumanType>>,
    /// Weighted local spawn slots into `types`; duplicate indices represent map weights.
    population: Vec<usize>,
    next_variant: Option<usize>,
    alternates: HashMap<String, Vec<Arc<HumanType>>>,
    pub people: Vec<Person>,
    rng: u64,
    next_id: u32,
    /// Seconds since the start.
    time: f64,
    /// The bus stops as Omsi.exe keeps them for the people (see `passengers::stops`).
    stops: HashMap<i64, PaxStop>,
    pub tickets: Option<Arc<::content::tickets::TicketPack>>,
    /// Current ticket request at the player's cash desk: (ticket name, value).
    pub request: Option<(String, f32)>,
    /// Payment on the desk: (paid, ticket value), and the change still owed after the ticket.
    pub paid: Option<(f32, f32)>,
    pub change_due: Option<f32>,
    pub money: Option<crate::money::Money>,
    /// A rider pressed the stop button for the next stop (the app fires the vehicle trigger `int_haltewunsch`).
    pub stop_request: bool,
    /// Tickets sold at the cash desk this session and what they were worth.
    pub tickets_sold: u32,
    pub ticket_cash: f32,
    /// Passengers that reached the cash desk, and those the driver served there.
    pub boarded: u32,
    pub served: u32,
    /// OMSI's rating counters: people who stepped into the player's bus
    /// and of those who had nothing to complain about (comfort = content / stepped in);
    /// tickets asked for and the points for selling them, two for the right change, one
    /// for the wrong (ticket selling = points / 2 × asked).
    pub stepped_in: u32,
    pub content: u32,
    pub ticket_requests: u32,
    pub ticket_points: u32,
    /// `PAX_Entry<i>_Req`: somebody at the kerb wants in through entry `i`.
    pub entry_req: Vec<bool>,
    /// `PAX_Exit<i>_Req`: somebody inside wants out through exit `i`.
    pub exit_req: Vec<bool>,
    /// Procedural inverse kinematics animation enabled.
    pub ik: bool,
    /// Natural movement and gait, independent of the skeletal pose used to draw people.
    pub natural: bool,
    /// Feet put down since the app last collected them (see [`Humans::take_footfalls`]).
    footfalls: Vec<ambience::Footfall>,
    /// `[trafficdensity_passenger]` factor for the current hour (set by the app).
    pub density: f32,
    /// The clock's time of day in seconds (set by the app): day tickets sell by it.
    pub time_of_day: f64,
    /// How late the player's bus is on its duty (s; set by the app): over five minutes,
    /// boarding passengers may say so.
    pub delay: f64,
    /// The game's folder (the ticket pack's voices are found from it).
    root: std::path::PathBuf,
    /// What passengers may say (the `pax_voices` setting): 0 everything, 1 only the
    /// ticket they ask for, 2 nothing.
    pub voices: u8,
    /// Only avatars: nobody else is put on the map (the passengers are off).
    pub avatar_only: bool,
    /// The player has got up and left the wheel: a standing bus with a door open is left
    /// by its riders as at a terminus (see `ALL_OUT_STOP`).
    pub driver_away: bool,
    /// Per bus stop, Omsi.exe's station targets (0x61cb18, `Schedule::stop_targets`): the
    /// stops the trips go on to, each with the termini of those trips. A person waiting there
    /// wants one of them and boards only a bus showing one of its termini; at a stop no trip
    /// goes on from, anybody takes the first bus (0x61c33c).
    pub stop_targets: Option<HashMap<i64, Vec<(String, HashSet<String>)>>>,
    /// The timetable's name of each stop object (`Schedule::stop_names`), the names the
    /// targets above are made of.
    pub stop_names: Option<HashMap<i64, String>>,
    /// Buses whose validator somebody used since the app last looked (`take_stamped`).
    stamped: Vec<BusId>,
    /// Pedestrians to keep strolling near the player (scaled by `density`).
    pub pedestrians: usize,
    /// Passengers pay the exact fare: no change is ever due.
    pub exact_fare: bool,
    /// How passengers board (`boarding` in the settings): `auto` - pay and take the
    /// ticket by themselves after a moment; `pay` - wait at the desk for the driver to
    /// sell it (and show a pass after `PAY_PATIENCE`); `walk` - no cash desk at all.
    pub boarding: String,
    /// Prefer free seated places when reserving a place; off preserves OMSI's random
    /// choice among seated and standing places.
    pub prefer_seats: bool,
    pub rear_entry: bool,
    /// The driver pressed the ticket key (`ticket_give`): sell the requested ticket.
    pub give_ticket: bool,
    /// The driver pressed `change_give`: all the change owed goes on the tray at once.
    pub give_change_all: bool,
    /// The key that sells a ticket, as the HUD names it.
    pub ticket_key: String,
    /// Where the player looks from (set by the app every frame).
    pub eye: Option<Eye>,
    /// Around where people are kept (the player's bus, else the camera).
    center: DVec3,
    /// A line for the HUD about something that just happened.
    message: Option<String>,
    /// Frames ticked, total and longest tick (ms).
    tick_stats: (u32, f64, f64),
    /// Where the time of this tick went (stage, ms since the one before), for the slow
    /// ticks OMSI_PROFILE reports.
    tick_stages: Vec<(&'static str, f64)>,
    /// The map's weighted population has been set up from `humans.txt`.
    map_humans_done: bool,
    /// Simulation time of the last `sync`.
    last_sync: f64,
    /// Frames synced, people posed and skinned, the time that took and the part of it spent
    /// uploading (ms), in total.
    pose_stats: (u32, usize, f64, f64),
    /// `OMSI_TRACE_PAX=<csv>`: every person near the eye, every frame (see `sync`).
    trace: Option<std::io::BufWriter<std::fs::File>>,
    /// `World::tiles_generation` the stops were last checked against.
    tiles_seen: u64,
    /// LAN play: where the other players are (host): people are kept around them too.
    pub lan_centers: Vec<DVec3>,
    /// How the player's bus is driven, for its riders' complaints.
    comfort: RideComfort,
}

/// Resolve each map entry directly, including human packs with nested folders.
/// Keep duplicate entries as spawn weights, but load each definition only once.
fn map_human_types(root: &Path, list: &[String]) -> Vec<Arc<HumanType>> {
    // (keyed case-blind: OMSI paths are, and the lists spell one file several ways)
    let mut loaded: HashMap<String, Option<Arc<HumanType>>> = HashMap::new();
    let mut picked = Vec::new();
    for line in list {
        let rel = line.trim().replace('\\', "/");
        // Lists normally include Humans/, but also accept paths relative to that folder.
        let rel = if rel.to_ascii_lowercase().starts_with("humans/") {
            rel
        } else {
            format!("Humans/{rel}")
        };
        let path = ::legacy_config::resolve_path(root, &rel);
        let ty = loaded
            .entry(path.to_string_lossy().to_lowercase())
            .or_insert_with(|| match HumanType::load(&path) {
                Ok(t) => Some(Arc::new(t)),
                Err(e) => {
                    log::warn!("map human {}: {e:#}", path.display());
                    None
                }
            });
        if let Some(t) = ty {
            picked.push(t.clone());
        }
    }
    picked
}

fn human_file_key(path: &str) -> String {
    let path = path.replace('\\', "/").to_ascii_lowercase();
    match path.rfind("humans/") {
        Some(k) => path[k + 7..].to_string(),
        None => path,
    }
}

impl Humans {
    fn next_person_id(&mut self) -> u32 {
        while self.people.iter().any(|person| person.id == self.next_id) {
            self.next_id += 1;
        }
        let id = self.next_id;
        self.next_id += 1;
        id
    }
    /// LAN uses the room id as the shared source of randomness.  This keeps the
    /// initial pedestrian selection and their generated identities identical on
    /// the host and clients; subsequent movement remains simulation-local.
    pub fn set_lan_seed(&mut self, seed: u64) {
        self.rng = (seed ^ 0xA5A5_5A5A_1F2E_3D4C) | 1;
    }

    pub fn new(root: &Path) -> Humans {
        let (types, alternates) = load_figures(root);
        let population = (0..types.len()).collect();
        if ::legacy_config::env::var_os("OMSI_DEBUG_HUMANS").is_some() {
            for t in &types {
                let (mut lo, mut hi) = (f32::MAX, f32::MIN);
                for m in &t.meshes {
                    for v in &m.data.positions {
                        lo = lo.min(v.z);
                        hi = hi.max(v.z);
                    }
                }
                log::info!(
                    "  {} z {lo:.2}..{hi:.2}",
                    t.def.path.file_name().unwrap_or_default().to_string_lossy()
                );
            }
        }
        Humans {
            alternates,
            avatars: Avatars {
                avatars: HashMap::new(),
                avatar_cmds: HashMap::new(),
                settle: HashSet::new(),
                avatar_hidden: HashMap::new(),
            },
            walking: PedestrianState {
                wall_cells: HashMap::new(),
                wall_key: (0, 0, 0, 0.0),
                ped: None,
                started: false,
                stroll_timer: 0.0,
            },
            desk: FareDesk {
                desk_busy: None,
                pardons: 0,
                pardon_max: 0,
            },
            buses: PassengerBuses {
                cabins: HashMap::new(),
                player_cabin: None,
                seats: HashMap::new(),
                odometer: HashMap::new(),
                pax_req: HashMap::new(),
                served_stop: None,
                ai_visits: HashMap::new(),
                last_door_open: HashMap::new(),
                holds: Vec::new(),
                ai_requests: Vec::new(),
                last_buses: Vec::new(),
                placed_now: Vec::new(),
                bus_motion: HashMap::new(),
            },
            voice: PassengerVoices {
                voice_lines: Vec::new(),
                voice_said: HashMap::new(),
                last_chat: -1e9,
            },
            network: PassengerNetwork {
                mirror: false,
                remote_now: Vec::new(),
                claims_out: Vec::new(),
                claimed: HashMap::new(),
                mirror_wait: HashMap::new(),
                handed: Vec::new(),
            },
            render: RenderResources {
                blob: None,
                spare_blobs: Vec::new(),
                hidden: Vec::new(),
                gpu_textures: HashMap::new(),
                gpu_materials: HashMap::new(),
                spare: HashMap::new(),
                sync_frame: 0,
            },
            types,
            population,
            next_variant: None,
            people: Vec::new(),
            rng: 0x1234_5678_9ABC_DEF1,
            next_id: 1,
            time: 0.0,
            stops: HashMap::new(),
            tickets: None,
            request: None,
            paid: None,
            change_due: None,
            money: None,
            stop_request: false,
            tickets_sold: 0,
            ticket_cash: 0.0,
            boarded: 0,
            served: 0,
            stepped_in: 0,
            content: 0,
            ticket_requests: 0,
            ticket_points: 0,
            entry_req: Vec::new(),
            exit_req: Vec::new(),
            ik: ::legacy_config::env::var("OMSI_PAX_IK")
                .map(|v| v != "0" && !v.eq_ignore_ascii_case("false"))
                .unwrap_or(true),
            natural: true,
            footfalls: Vec::new(),
            density: 1.0,
            time_of_day: 12.0 * 3600.0,
            delay: 0.0,
            root: root.to_path_buf(),
            voices: 0,
            avatar_only: false,
            driver_away: false,
            stop_targets: None,
            stop_names: None,
            stamped: Vec::new(),
            pedestrians: 14,
            exact_fare: true,
            boarding: "auto".into(),
            prefer_seats: false,
            rear_entry: true,
            give_ticket: false,
            give_change_all: false,
            ticket_key: "T".into(),
            eye: None,
            center: DVec3::ZERO,
            message: None,
            tick_stats: (0, 0.0, 0.0),
            tick_stages: Vec::new(),
            map_humans_done: false,
            last_sync: 0.0,
            pose_stats: (0, 0, 0.0, 0.0),
            trace: ::legacy_config::env::var("OMSI_TRACE_PAX").ok().and_then(|f| std::fs::File::create(f).ok()).map(|f| {
                use std::io::Write;
                let mut w = std::io::BufWriter::new(f);
                let _ = writeln!(w, "t,id,state,ground,posed,x,y,z,heading,lx,ly,lz,rx,ry,rz,vx,vy,task,movement,stop,door,point,target,why,bx,by");
                w
            }),
            tiles_seen: 0,
            lan_centers: Vec::new(),
            comfort: RideComfort::default(),
        }
    }

    pub fn set_ik(&mut self, ik: bool) {
        if self.ik != ik {
            for person in &mut self.people {
                person.render.active_bones = None;
            }
        }
        self.ik = ik;
    }

    pub fn set_natural(&mut self, natural: bool) {
        self.natural = natural;
    }

    /// Clear time-dependent people while keeping player-bus riders, avatars, and pending
    /// LAN handovers with their stop claims until they are acknowledged or rejected.
    pub fn reset_population(&mut self) {
        let avatars: HashSet<u32> = self.avatars.avatars.values().copied().collect();
        let transfer_stops: HashSet<i64> = self
            .people
            .iter()
            .filter_map(|person| match &person.state {
                State::Pax(pax) if pax.task == Task::AwaitingTransfer => pax.stop,
                _ => None,
            })
            .collect();
        for i in (0..self.people.len()).rev() {
            let keep = matches!(self.people[i].place, Place::Bus(BusId::Player, _))
                || avatars.contains(&self.people[i].id)
                || matches!(&self.people[i].state, State::Pax(pax) if pax.task == Task::AwaitingTransfer);
            if !keep {
                self.release(i);
                let p = self.people.swap_remove(i);
                self.retire(&p);
            }
        }
        self.buses.seats.retain(|bus, _| *bus == BusId::Player);
        self.buses.odometer.retain(|bus, _| *bus == BusId::Player);
        self.buses.pax_req.retain(|bus, _| *bus == BusId::Player);
        self.buses
            .last_door_open
            .retain(|bus, _| *bus == BusId::Player);
        self.buses.bus_motion.retain(|bus, _| *bus == BusId::Player);
        self.stops.retain(|stop, _| transfer_stops.contains(stop));
        self.buses.ai_visits.clear();
        self.buses.holds.clear();
        self.buses.ai_requests.clear();
        self.buses.served_stop = None;
        self.network.handed.clear();
        self.stop_targets = None;
        self.stop_names = None;
        self.walking.started = false;
        self.walking.stroll_timer = 0.0;
    }

    /// The map's tiles changed: a stop gone with its tile takes the people waiting there
    /// with it; a stop nobody waits at is set up again with what its tiles hold now (its
    /// waiting places come with the objects round it, sub_620c0c).
    fn tiles_changed(&mut self, world: &World) {
        let present: HashSet<i64> = world.bus_stops.lock().iter().map(|s| s.0).collect();
        let bound = |st: &State| -> Option<i64> {
            match st {
                State::Pax(p) if p.inside.is_none() => p.stop,
                _ => None,
            }
        };
        let used: HashSet<i64> = self.people.iter().filter_map(|p| bound(&p.state)).collect();
        let gone: Vec<i64> = self
            .stops
            .keys()
            .copied()
            .filter(|id| !present.contains(id))
            .collect();
        let mut removed = 0usize;
        for i in (0..self.people.len()).rev() {
            let p = &self.people[i];
            if matches!(&p.state, State::Pax(x) if x.task == Task::AwaitingTransfer) {
                continue;
            }
            let lost_stop = bound(&p.state).is_some_and(|s| gone.contains(&s));
            let lost_ground = p.place == Place::Ground
                && p.puppet.is_none()
                && !world.has_ground(p.position.x, p.position.y);
            if lost_stop || lost_ground {
                self.release(i);
                let p = self.people.swap_remove(i);
                if debug_pax() {
                    log::info!(
                        "t={:.1} pax {} taken away with its tile ({})",
                        self.time,
                        p.label(),
                        p.state.name()
                    );
                }
                self.retire(&p);
                removed += 1;
            }
        }
        for id in &gone {
            self.stops.remove(id);
        }
        let idle: Vec<i64> = self
            .stops
            .keys()
            .copied()
            .filter(|id| !used.contains(id))
            .collect();
        let rebuilt = idle.len();
        for id in idle {
            self.stops.remove(&id);
        }
        if debug_pax()
            || (::legacy_config::env::var_os("OMSI_PROFILE").is_some()
                && (removed > 0 || !gone.is_empty()))
        {
            log::info!(
                "people: tiles changed: {} stops gone, {rebuilt} set up again, {removed} people taken away",
                gone.len()
            );
        }
    }

    /// The buses somebody stamped a ticket in since the last call (the app fires their
    /// `ev_Stamper` sound trigger): `None` the player's, else the AI car's id.
    pub fn take_stamped(&mut self) -> Vec<Option<u64>> {
        std::mem::take(&mut self.stamped)
            .into_iter()
            .map(|b| match b {
                BusId::Ai(id) => Some(id),
                _ => None,
            })
            .collect()
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

    /// Whether the player could see somebody standing at `p`.
    fn seen(&self, p: DVec3) -> bool {
        match self.eye {
            None => (p - self.center).length() < 150.0,
            Some(e) => {
                let d = p + DVec3::Z * 0.9 - e.pos;
                let dist = d.length();
                if dist > 230.0 {
                    return false;
                }
                dist < 3.0 || d.dot(e.fwd) / dist > e.cos_half
            }
        }
    }

    /// The footsteps taken since the last call, for the environment sounds. They pile up
    /// only between two frames; a run without audio never looks at them, so the list is
    /// dropped once it grows past a crowd's worth of steps.
    /// `OMSI_TRACE_PAX` is writing a trace.
    pub fn tracing(&self) -> bool {
        self.trace.is_some()
    }

    pub fn take_footfalls(&mut self) -> Vec<ambience::Footfall> {
        if self.footfalls.len() > 256 {
            self.footfalls.clear();
        }
        std::mem::take(&mut self.footfalls)
    }

    /// People currently in the player's bus.
    pub fn riding(&self) -> usize {
        self.people
            .iter()
            .filter(|p| p.inside(BusId::Player))
            .count()
    }

    /// People walking the footpaths of the traffic network: (lane, distance along it). The
    /// traffic gives way to them at crossings and presses the pedestrian lights' buttons
    /// for them.
    /// Everybody on foot on the ground, for the traffic to stop for: position, velocity
    /// and whether they wait at a stop (a bus pulls up right beside those).
    pub fn on_foot(&self) -> Vec<(DVec2, DVec2, bool)> {
        self.people
            .iter()
            .filter(|p| p.place == Place::Ground)
            .map(|p| {
                let waiting = matches!(&p.state, State::Pax(x) if x.inside.is_none());
                (p.position.truncate(), p.vel, waiting)
            })
            .collect()
    }

    pub fn strollers(&self) -> Vec<(usize, f32)> {
        self.people
            .iter()
            .filter_map(|p| match &p.state {
                State::Strolling(walk) => walk
                    .legs
                    .get(walk.leg)
                    .map(|leg| (leg.lane, leg.dist(walk.s))),
                _ => None,
            })
            .collect()
    }

    fn spawn(
        &mut self,
        world: &World,
        renderer: &Renderer,
        scene: &mut Scene,
        position: DVec3,
        heading: f64,
        state: State,
    ) -> Option<usize> {
        self.spawn_as(world, renderer, scene, position, heading, state, None)
    }

    #[allow(clippy::too_many_arguments)]
    fn spawn_as(
        &mut self,
        world: &World,
        renderer: &Renderer,
        scene: &mut Scene,
        position: DVec3,
        heading: f64,
        mut state: State,
        kind: Option<usize>,
    ) -> Option<usize> {
        self.use_map_humans(world);
        if self.types.is_empty()
            || kind.is_some_and(|index| index >= self.types.len())
            || (kind.is_none() && self.population.is_empty())
        {
            return None;
        }
        // on the surface they will walk on, not on the bare terrain under a pavement
        // (they stood in the asphalt and climbed out of it when they started walking)
        let mut position = position;
        if let Some(z) = world.walk_height_near(position.x, position.y, position.z) {
            if (z - position.z).abs() < 3.0 {
                position.z = z;
            }
        }
        if let State::Pax(p) = &mut state {
            if p.inside.is_none() && p.posture != Posture::Sitting {
                p.pos = position;
            }
        }
        let (ty, variant) = self.pick_figure(position, kind);
        let variant = self.next_variant.take().unwrap_or(variant);
        let mut initial_seat = None;
        if self.ik {
            if let State::Pax(p) = &mut state {
                if p.task == Task::WaitingForBus && p.posture == Posture::Sitting {
                    if let Some(sp) = p
                        .stop
                        .zip(p.spot)
                        .and_then(|(s, k)| self.stops.get(&s).and_then(|s| s.spots.get(k)))
                    {
                        position = sp.foot_root(ty.rig.seat_front(), position.z);
                        p.pos = position;
                        initial_seat = Some(model_point(position, heading, sp.pos));
                    }
                }
            }
        }
        let tkey = Arc::as_ptr(&ty) as usize;
        let mut meshes = Vec::new();
        for mi in 0..ty.mesh_count() {
            let (level, hm) = ty.mesh_at(mi);
            let key = (tkey, variant, mi);
            // somebody of this type has gone: their mesh and instance
            if let Some((id, inst)) = self.render.spare.get_mut(&key).and_then(|v| v.pop()) {
                self.render.hidden.retain(|h| *h != inst);
                renderer.set_transform(scene, inst, position, Mat4::IDENTITY);
                renderer.set_params(scene, inst, &[], level == 0, &[]);
                meshes.push((id, inst));
                continue;
            }
            if !self.render.gpu_materials.contains_key(&key) {
                let dirs = ty.texture_dirs(&world.root);
                let mut mats = Vec::new();
                for (k, m) in hm.materials.iter().enumerate() {
                    // the variant's texture from its own folder first, else the default
                    let (name, first) = ty.variant_texture(&m.texture, variant);
                    let mut look: Vec<&Path> = first.into_iter().collect();
                    look.extend(dirs.iter().map(|p| p.as_path()));
                    let found = ::texture::find_texture(name, &look)
                        .or_else(|| ::texture::find_texture(&m.texture, &look));
                    if found.is_none() && !m.texture.trim().is_empty() {
                        log::warn!(
                            "human {}: texture {} not found",
                            ty.def.path.display(),
                            m.texture
                        );
                    }
                    let tex = match found {
                        Some(path) => match self.render.gpu_textures.get(&path) {
                            Some(t) => *t,
                            None => {
                                let t = world
                                    .textures
                                    .get_gpu_fast(&path)
                                    .map(|(img, _)| renderer.add_texture_data(scene, &img));
                                world.textures.release(&path);
                                self.render.gpu_textures.insert(path, t);
                                t
                            }
                        },
                        None => None,
                    };
                    let alpha = match hm.alpha.get(k).copied().unwrap_or(0) {
                        1 => AlphaMode::Test,
                        2 => AlphaMode::Blend,
                        _ => AlphaMode::Opaque,
                    };
                    mats.push(renderer.add_material(scene, tex, alpha, [1.0; 4], false));
                }
                self.render.gpu_materials.insert(key, mats);
            }
            let mats = self.render.gpu_materials[&key].clone();
            let id = renderer.add_mesh(scene, &hm.data);
            let inst = renderer.add_instance(scene, id, position, Mat4::IDENTITY, mats);
            renderer.set_omsi_caster(scene, inst, true);
            if level > 0 {
                renderer.set_params(scene, inst, &[], false, &[]);
            }
            meshes.push((id, inst));
        }
        // walking pace 1.1 m/s +- 0.2, as Omsi.exe draws it for everybody (0x625758:
        // sub_7f08b0(0.2, 1.1)); `[walk_param]` holds the stride, not a speed
        let pace = 1.1 + (self.rand_f() * 2.0 - 1.0) * 0.2;
        let age = ty.def.age.map(|a| a as f32).unwrap_or(40.0);
        let id = self.next_person_id();
        if debug_pax() {
            log::info!(
                "pax #{id} ({}) appears at ({:.1}, {:.1}, {:.1}): {}{}",
                ty.def
                    .path
                    .file_name()
                    .unwrap_or_default()
                    .to_string_lossy(),
                position.x,
                position.y,
                position.z,
                state.name(),
                if self.seen(position) { " IN SIGHT" } else { "" }
            );
        }
        self.people.push(Person {
            render: PersonRender {
                level: 0,
                blob: None,
                blob_shown: false,
                mirror_seat: None,
                active_bones: None,
                meshes,
                skins: Vec::new(),
                skin_bones: None,
                pose_changed: false,
                lit: 0.0,
                skinned: false,
                since_posed: 0,
                posed_at: (position, heading),
                ankles: [Vec3::ZERO; 2],
            },
            id,
            ty,
            variant,
            position,
            heading,
            lheading: 0.0,
            place: Place::Ground,
            vel: DVec2::ZERO,
            pace,
            activity: Activity::Stand,
            anim: OmsiAnim::default(),
            pose: ::simulation::human::Pose::new(id),
            state,
            t_state: 0.0,
            interior: 0.0,
            tilt: Mat4::IDENTITY,

            age,
            stuck: 0.0,
            ghost: 0.0,
            car_wait: 0.0,
            detour: 0.0,
            detour_side: 0.0,
            why: "",
            puppet: None,
            remote: false,
        });
        let person = self.people.last_mut().unwrap();
        person.pose.advance(
            &person.ty.rig,
            &::simulation::human::PoseInput {
                origin: position,
                heading,
                activity: if initial_seat.is_some() {
                    Activity::Sit
                } else {
                    Activity::Stand
                },
                seat: initial_seat,
                ..Default::default()
            },
            0.0,
        );
        Some(self.people.len() - 1)
    }

    fn free_seat(&mut self, bus: BusId, seat: usize) {
        if let Some(t) = self.buses.seats.get_mut(&bus).and_then(|v| v.get_mut(seat)) {
            *t = false;
        }
    }

    /// Put people at the bus stops near `center`: at the start everywhere, later only at
    /// stops out of sight (the others fill with people walking up).
    pub fn populate(
        &mut self,
        world: &World,
        renderer: &Renderer,
        scene: &mut Scene,
        center: DVec3,
    ) {
        if self.avatar_only {
            return;
        }
        // whoever stands under the surface there (its tile's pavements and roads came
        // after them), or anyone standing still off it: onto it
        for p in self.people.iter_mut() {
            if matches!(p.place, Place::Ground) {
                if let Some(z) = world.walk_height_near(p.position.x, p.position.y, p.position.z) {
                    let d = z - p.position.z;
                    let still = p.vel.length() < 0.05;
                    if d.abs() < 3.0 && (d > 0.02 || (still && d.abs() > 0.02)) {
                        p.set_ground_height(z, self.ik);
                    }
                }
            }
        }
        self.populate_with(world, None, renderer, scene, center);
    }

    fn populate_with(
        &mut self,
        world: &World,
        net: Option<&Network>,
        _renderer: &Renderer,
        _scene: &mut Scene,
        center: DVec3,
    ) {
        self.center = center;
        let list: Vec<(i64, DVec3, f64, String)> = world
            .bus_stops
            .lock()
            .iter()
            .filter(|s| (s.1 - center).length() < STOP_RANGE + 100.0)
            .map(|s| (s.0, s.1, s.2, s.3.clone()))
            .collect();
        for (id, pos, rot, name) in list {
            // only once the ground under the stop is there
            if world.walk_height(pos.x, pos.y).is_none() || self.stops.contains_key(&id) {
                continue;
            }
            let st = self.build_pax_stop(world, net, id, pos, rot, &name);
            self.stops.insert(id, st);
        }
        self.walking.started = true;
    }

    /// Keep only the people the map's `humans.txt` names, an entry listed twice counting
    /// twice, as OMSI draws a map's pedestrians and passengers from that list alone. A map
    /// without the file, or whose list names nobody to be found, keeps everybody.
    fn use_map_humans(&mut self, world: &World) {
        if self.map_humans_done {
            return;
        }
        self.map_humans_done = true;
        let path = ::legacy_config::resolve_path(&world.map_dir, "humans.txt");
        let list = ::map::ailists::load_list(&path);
        if list.is_empty() {
            return;
        }
        // Match installed paths first, then load entries from packs outside the initial scan.
        let mut population = Vec::new();
        for line in &list {
            let want = human_file_key(line.trim());
            let exact = self
                .types
                .iter()
                .position(|ty| human_file_key(&ty.def.path.to_string_lossy()) == want);
            let index = exact.or_else(|| {
                if figures::is_alternate(Path::new(&want)) {
                    let slot = slot_key(Path::new(&want));
                    let alternate = self
                        .alternates
                        .get(&slot)
                        .and_then(|types| {
                            types
                                .iter()
                                .find(|ty| human_file_key(&ty.def.path.to_string_lossy()) == want)
                        })
                        .cloned();
                    alternate.map(|ty| self.type_index(ty))
                } else {
                    None
                }
            });
            if let Some(index) = index {
                population.push(index);
            } else {
                population.extend(
                    map_human_types(&world.root, std::slice::from_ref(line))
                        .into_iter()
                        .map(|ty| self.type_index(ty)),
                );
            }
        }
        // (a list that names nobody to be found keeps everybody: a map without people
        // looked broken)
        if population.is_empty() {
            log::warn!("humans.txt of the map names nobody installed: keeping all people");
            return;
        }
        log::info!(
            "humans: {} of {} map entries loaded from {}",
            population.len(),
            list.len(),
            path.display()
        );
        let mut slots = HashSet::new();
        for &index in &population {
            let ty = self.types[index].clone();
            let key = slot_key(&ty.def.path);
            if !slots.insert(key.clone()) {
                continue;
            }
            let Some(parent) = ty.def.path.parent() else {
                continue;
            };
            for (name, is_dir) in ::legacy_config::vfs::list_dir(parent).unwrap_or_default() {
                let path = parent.join(name);
                if is_dir || !figures::is_alternate(&path) || slot_key(&path) != key {
                    continue;
                }
                let candidates = self.alternates.entry(key.clone()).or_default();
                if candidates.iter().any(|t| t.def.path == path) {
                    continue;
                }
                match HumanType::load(&path) {
                    Ok(ty) => candidates.push(Arc::new(ty)),
                    Err(error) => log::warn!("human {}: {error:#}", path.display()),
                }
            }
        }
        self.population = population;
    }

    /// People the moving bus has just knocked down. OMSI counts them in the driver's
    /// personnel file; they are only counted once and then walk away.
    pub fn run_over(&mut self, bus: &VehicleInstance) -> u32 {
        if bus.physics.velocity_kmh().abs() < 5.0 {
            return 0;
        }
        let Some(bb) = bus.ty.def.bounding_box else {
            return 0;
        };
        let (half_x, half_y) = ((bb[0] - bb[3]).abs() / 2.0, (bb[1] - bb[4]).abs() / 2.0);
        let inv = bus.body_rotation().transpose();
        let mut knocked = Vec::new();
        for (i, p) in self.people.iter().enumerate() {
            // (sub_62a6a0 at 0x62dc6c: the people waiting at a stop and those on the pavements)
            let counts = match &p.state {
                State::Pax(x) => x.task == Task::WaitingForBus,
                State::Strolling(_) | State::Standing => true,
                State::Idle => false,
            };
            if p.place != Place::Ground || !counts {
                continue;
            }
            let local = inv.transform_vector3((p.position - bus.position).as_vec3());
            if local.x.abs() < half_x + 0.2 && local.y.abs() < half_y + 0.2 && local.z.abs() < 3.0 {
                knocked.push(i);
            }
        }
        for &i in knocked.iter().rev() {
            // a waiting passenger knocked down leaves the stop and walks off (sub_626818)
            if let State::Pax(x) = &self.people[i].state {
                let (at, h, stop) = (x.pos, x.yaw.to_degrees(), x.stop);
                self.release(i);
                self.walk_street(i, at, h, stop, None);
            }
        }
        knocked.len() as u32
    }

    /// `--riders n`: n passengers already in their places in the player's bus (a test
    /// start; OMSI's buses start empty), without a destination - they ride 1..20 km.
    pub fn seed_riders(
        &mut self,
        n: usize,
        bus: &VehicleInstance,
        world: &World,
        renderer: &Renderer,
        scene: &mut Scene,
    ) {
        let Some(cabin) = self.cabin_for(bus) else {
            return;
        };
        let trailers = part_frames(bus, &cabin);
        let rot = bus.body_rotation();
        for _ in 0..n {
            let Some(k) = self.reserve_place(BusId::Player, &cabin, None, false) else {
                break;
            };
            let walk = 1.1 + (self.rand_f() as f32 * 2.0 - 1.0) * 0.2;
            let r = self.rand_f() as f32;
            let mut pax = Pax::new(walk);
            pax.bus = Some(BusId::Player);
            pax.inside = Some(BusId::Player);
            pax.seat = Some(k);
            pax.journey.ride_km = r * 19.0 + 1.0;
            pax.task = Task::InBusToPlace;
            let at = train_point(bus.position, &rot, &trailers, cabin.seats[k].pos);
            let Some(i) = self.spawn(
                world,
                renderer,
                scene,
                at,
                bus.heading,
                State::Pax(Box::new(pax)),
            ) else {
                self.free_seat(BusId::Player, k);
                break;
            };
            let s = cabin.seats[k].clone();
            let seatheight = self.people[i].ty.def.seat_height;
            let ik = self.ik;
            let seat_front = self.people[i].ty.rig.seat_front() as f64;
            let mut sit_activity = false;
            if let Some(p) = self.pax_mut(i) {
                p.task = Task::SittingInBus;
                p.movement = Movement::Standing;
                if s.seated {
                    p.seat_h = s.height;
                    if ik {
                        let r = (s.rot as f64).to_radians();
                        let floor = DVec3::new(
                            s.pos.x as f64 + r.sin() * seat_front,
                            s.pos.y as f64 + r.cos() * seat_front,
                            (s.pos.z - s.height) as f64,
                        );
                        p.pos = floor;
                        sit_activity = true;
                    } else {
                        p.pos = (s.pos - Vec3::Z * seatheight).as_dvec3();
                    }
                    p.posture = Posture::Sitting;
                } else {
                    p.pos = s.pos.as_dvec3();
                }
                p.yaw = (s.rot as f64).to_radians();
            }
            if sit_activity {
                self.people[i].activity = Activity::Sit;
            }
            let place_pos = self.pax(i).map(|p| p.pos.as_vec3()).unwrap_or(s.pos);
            self.people[i].place = Place::Bus(BusId::Player, place_pos);
            let person = &mut self.people[i];
            person.position = train_point(bus.position, &rot, &trailers, place_pos);
            person.heading = train_heading(bus.heading, &trailers, place_pos) + s.rot as f64;
            person.lheading = s.rot as f64;
            person.pose.advance(
                &person.ty.rig,
                &::simulation::human::PoseInput {
                    origin: place_pos.as_dvec3(),
                    heading: person.lheading,
                    frame: BusId::Player.space(),
                    activity: person.activity,
                    seat: s.seated.then(|| {
                        model_point(place_pos.as_dvec3(), person.lheading, s.pos.as_dvec3())
                    }),
                    ..Default::default()
                },
                0.0,
            );
        }
    }

    /// Give back what a person holds (a waiting place, a seat) before they change plans.
    fn release(&mut self, i: usize) {
        let id = self.people[i].id;
        self.cancel_fare(id);
        let State::Pax(x) = &mut self.people[i].state else {
            return;
        };
        let (stop, spot, bus) = (x.stop, x.spot.take(), x.bus.or(x.inside));
        let seats = [x.seat.take(), x.vacating.take()];
        if let (Some(s), Some(k)) = (stop, spot) {
            self.free_spot(s, k);
        }
        for k in seats.into_iter().flatten() {
            if let Some(b) = bus {
                self.free_seat(b, k);
            }
        }
    }

    /// OMSI_CHECK_WALLS: everybody inside a bus who stands away from its walkways (more
    /// than 0.45 m from every path link, not on a seat): through a seat back or a wall.
    fn check_walls(&self) {
        for p in &self.people {
            let Place::Bus(bus, local) = p.place else {
                continue;
            };
            if matches!(&p.state, State::Pax(x) if x.task == Task::SittingInBus || x.movement == Movement::Turning)
            {
                continue;
            }
            let Some(bn) = self.buses.last_buses.iter().find(|b| b.id == bus) else {
                continue;
            };
            let pts = &bn.cabin.graph.points;
            let mut best = f32::INFINITY;
            for &(a, b, _) in &bn.cabin.links {
                let (Some(pa), Some(pb)) = (pts.get(a.max(0) as usize), pts.get(b.max(0) as usize))
                else {
                    continue;
                };
                let ab = *pb - *pa;
                let t = if ab.length_squared() > 1e-6 {
                    ((local - *pa).dot(ab) / ab.length_squared()).clamp(0.0, 1.0)
                } else {
                    0.0
                };
                let q = *pa + ab * t;
                best = best.min((q.truncate() - local.truncate()).length() + (q.z - local.z).abs());
            }
            let near_seat = bn.cabin.seats.iter().any(|s| {
                (s.floor - local).truncate().length() < 0.35
                    || (s.pos - local).truncate().length() < 0.35
            });
            if best > 0.45 && !near_seat && !bn.cabin.links.is_empty() {
                log::warn!(
                    "t={:.1} person {} in bus {:?} off the walkways by {best:.2} m at ({:.2}, {:.2}, {:.2}), state {}",
                    self.time,
                    p.id,
                    bus,
                    local.x,
                    local.y,
                    local.z,
                    p.state.name()
                );
            }
        }
    }

    /// Report the waiting and alighting passengers to the bus script, the way OMSI does.
    pub fn write_pax_vars(&self, b: &mut VehicleInstance) {
        for (i, r) in self.entry_req.iter().enumerate() {
            b.set_var(&format!("PAX_Entry{i}_Req"), if *r { 1.0 } else { 0.0 });
        }
        for (i, r) in self.exit_req.iter().enumerate() {
            b.set_var(&format!("PAX_Exit{i}_Req"), if *r { 1.0 } else { 0.0 });
        }
    }

    /// Who wants a timetable bus to stop, as Omsi.exe asks before it lets one pull in
    /// (0x7da91f): the AI buses with somebody aboard on the way to a door to get off
    /// (task 5), and the stops where somebody is waiting for a bus or walking to one
    /// (tasks 1 to 3).
    pub fn stop_wishes(&self) -> (HashSet<u64>, HashSet<i64>) {
        let (mut alighting, mut waiting) = (HashSet::new(), HashSet::new());
        for p in &self.people {
            let State::Pax(x) = &p.state else { continue };
            match x.task {
                Task::InBusToExit => {
                    if let Some(BusId::Ai(id)) = x.inside {
                        alighting.insert(id);
                    }
                }
                Task::WaitingForBus | Task::ToBus | Task::WalkingToBus => {
                    if let Some(s) = x.stop {
                        waiting.insert(s);
                    }
                }
                _ => {}
            }
        }
        (alighting, waiting)
    }

    /// Timetable buses to hold at their stop, for the traffic.
    pub fn take_holds(&mut self) -> Vec<(u64, f32, bool)> {
        std::mem::take(&mut self.buses.holds)
    }

    /// Door requests for the timetable buses, for the traffic to hand to their scripts.
    pub fn take_ai_requests(&mut self) -> Vec<(u64, Vec<bool>, Vec<bool>)> {
        std::mem::take(&mut self.buses.ai_requests)
    }

    /// A line for the HUD about something that just happened.
    pub fn take_message(&mut self) -> Option<String> {
        self.message.take()
    }

    /// What the driver should do now, for the HUD: a passenger waiting at the cash desk
    /// for the ticket (only when the driver has to sell it).
    pub fn hint(&self) -> Option<String> {
        if !self.boarding.eq_ignore_ascii_case("pay") {
            return None;
        }
        self.people.iter().find(|p| matches!(&p.state, State::Pax(x) if x.inside == Some(BusId::Player) && x.ticket == TicketAction::Buy && x.fare_phase == FarePhase::AwaitTicket))?;
        let (name, value) = self.request.clone()?;
        Some(format!(
            "Passenger waiting for a ticket: {name} {value:.2} - press {}",
            self.ticket_key
        ))
    }

    pub fn driver_cue(&self) -> crate::driver::Cue {
        let Some(cabin) = self.buses.player_cabin.as_ref() else {
            return Default::default();
        };
        let door = cabin
            .entries
            .iter()
            .filter(|d| d.sells)
            .max_by(|a, b| a.inside.y.total_cmp(&b.inside.y))
            .or(cabin.entries.first())
            .map(|d| d.inside + Vec3::Z * 1.4);
        let mut cue = crate::driver::Cue {
            door,
            ..Default::default()
        };
        let sale = cabin.sale.map(|s| s.1);
        for p in &self.people {
            let State::Pax(x) = &p.state else {
                continue;
            };
            if x.inside != Some(BusId::Player) || x.fare_phase < FarePhase::RequestTicket {
                continue;
            }
            cue.customer = Some(x.pos.as_vec3() + Vec3::Z * (p.ty.rig.neck.z + 0.08));
            cue.desk = match x.fare_phase {
                FarePhase::Paying | FarePhase::AwaitTicket => cabin.money_point.or(sale),
                FarePhase::TakingTicket => sale.or(cabin.money_point),
                FarePhase::AwaitChange => cabin.change_point.or(cabin.money_point),
                _ => None,
            };
            break;
        }
        cue
    }

    /// People sitting on each `[passpos]` of the player's bus, for `GetHumanCountOnSeat`
    /// (the BVG Citaro folds its tip-up seats down when somebody sits on them).
    pub fn seat_counts(&self) -> Vec<u32> {
        let Some(cabin) = self.buses.player_cabin.as_ref() else {
            return Vec::new();
        };
        let sitting = self.people.iter().filter_map(|p| match &p.state {
            State::Pax(x) if x.inside == Some(BusId::Player) => x
                .seat
                .filter(|_| x.task == Task::SittingInBus)
                .or(x.vacating),
            _ => None,
        });
        seat_numbers(&cabin.seats, sitting)
    }

    /// How many people stand on each `paths.cfg` link inside the player's bus, for the
    /// scripts' `GetHumanCountOnPathLink` (the NL/NG uses it for the fare gate).
    pub fn path_link_counts(&self) -> Vec<u32> {
        let Some(cabin) = self.buses.player_cabin.as_ref() else {
            return Vec::new();
        };
        // (the link each walker is on, +0x698)
        let mut out = vec![0u32; cabin.links.len()];
        for p in &self.people {
            if let State::Pax(x) = &p.state {
                if x.inside == Some(BusId::Player)
                    && (x.movement == Movement::ToTarget
                        || x.movement == Movement::AlongPath
                        || x.movement == Movement::Turning)
                {
                    if let Some(c) = x.link.and_then(|l| out.get_mut(l)) {
                        *c += 1;
                    }
                }
            }
        }
        out
    }

    /// Where everybody is, for logs.
    pub fn positions(&self) -> Vec<(String, DVec3)> {
        self.people
            .iter()
            .map(|p| (p.state.name().to_string(), p.position))
            .collect()
    }

    /// Count of people per state, for logs.
    pub fn summary(&self) -> String {
        let mut counts: std::collections::BTreeMap<&'static str, usize> = Default::default();
        for p in &self.people {
            *counts.entry(p.state.name()).or_default() += 1;
        }
        let mut out = counts
            .iter()
            .map(|(k, v)| format!("{v} {k}"))
            .collect::<Vec<_>>()
            .join(", ");
        let (n, total, worst) = self.tick_stats;
        if n > 0 {
            out.push_str(&format!(
                "; {:.2} ms a frame, longest {worst:.1} ms",
                total / n as f64
            ));
        }
        let (frames, posed, ms, up) = self.pose_stats;
        if frames > 0 {
            out.push_str(&format!(
                "; posing {:.2} ms a frame ({:.1} people, {:.2} ms of it uploading and placing)",
                ms / frames as f64,
                posed as f64 / frames as f64,
                up / frames as f64
            ));
        }
        out
    }
}

#[cfg(test)]
#[path = "../passenger_compat_tests.rs"]
mod passenger_compat_tests;

#[cfg(test)]
mod tests;

/// A heading in 0..360 degrees.
/// A line a passenger says: the sample and where they stand.
pub struct VoiceLine {
    pub position: DVec3,
    pub path: std::path::PathBuf,
}

/// How much of its probability a day ticket keeps at a time of day (seconds): rising from
/// nothing at midnight to all of it at 9:00, as the ticket packs describe it, then falling
/// on OMSI's line.
fn day_ticket_factor(t: f64) -> f32 {
    let t = t.rem_euclid(86_400.0);
    let rise = t / 32_400.0;
    let fall = 1.0 - (t - 32_400.0) / (88_776.0 - 32_400.0);
    rise.min(fall).clamp(0.0, 1.0) as f32
}

fn wrap_heading(h: f64) -> f64 {
    h.rem_euclid(360.0)
}

/// The angle between two headings (degrees, 0..180).
fn angle_between(a: f64, b: f64) -> f64 {
    ((b - a + 540.0).rem_euclid(360.0) - 180.0).abs()
}

pub(super) fn person_hash(id: u32, n: u32) -> f64 {
    let mut x = (id as u64) << 32 | n as u64;
    x = (x ^ (x >> 30)).wrapping_mul(0xbf58_476d_1ce4_e5b9);
    x = (x ^ (x >> 27)).wrapping_mul(0x94d0_49bb_1331_11eb);
    x ^= x >> 31;
    (x >> 11) as f64 / (1u64 << 53) as f64
}

/// Walking speed by age and height, `omsi` (Omsi.exe's 0.9..1.3 m/s draw) setting where in
/// the range a person is: about 1.35 m/s for an adult, 1.0 at 75, 1.1 for a young child.
pub(super) fn natural_pace(def: &::content::Human, omsi: f64) -> f64 {
    let age = def.age.map_or(40.0, |a| a as f64);
    let by_age = match age {
        a if a < 8.0 => 1.0,
        a if a < 13.0 => 1.0 + (a - 8.0) * 0.06,
        a if a < 60.0 => 1.35,
        a => (1.35 - (a - 60.0) * 0.022).max(0.8),
    };
    let tall = if age >= 13.0 && def.height > 1.0 {
        (def.height as f64 / 1.75).sqrt().clamp(0.9, 1.07)
    } else {
        1.0
    };
    by_age * tall * (1.0 + 0.5 * (omsi - 1.1))
}
