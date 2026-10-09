use crate::scene::{self, World};
use crate::{Args, Player};
use glam::DVec3;
use ::network::{Footprint, LanEvent, LanSession, PartPose, Pose, Role};
use ::render::{Renderer, Scene};
use ::legacy_script::VarId;
use ::simulation::traffic::{Lane, LaneKind, Network};
use std::net::SocketAddr;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::{Duration, Instant};
use winit::keyboard::KeyCode;

pub const WELCOME_WAIT: Duration = Duration::from_secs(3);
/// A host busy loading a heavy part of a big map can take longer than 3 s to answer.
pub const HOST_ANSWER_WAIT: Duration = Duration::from_secs(8);
const GAP_ALONG: f64 = 3.0;
const GAP_ACROSS: f64 = 1.0;
const MAX_VEHICLE_FILE: u64 = 16 << 20;
const HEAR_RANGE: f64 = 250.0;
const CLOCK_JUMP: f64 = 2.0;
const CHAT_LINES: usize = 6;

pub fn population_seed(session: &LanSession) -> u64 {
    session.session ^ 0x4F4D_5349_4C41_4E31
}

/// Each list is sorted by name so both games agree on the order; `hash` differs when the
/// vehicle files do.
pub struct SyncTable {
    pub lamps: Vec<(String, VarId)>,
    pub switches: Vec<(String, VarId)>,
    pub values: Vec<(String, VarId)>,
    pub doors: Vec<VarId>,
    /// Not in `hash`: a game without these still takes the rest of the table.
    ibis: Vec<Option<VarId>>,
    pub hash: u32,
    horn: Vec<VarId>,
    engine_n: Option<VarId>,
    ai_engine: Option<VarId>,
    ai_light: Option<VarId>,
    ai_interior: Option<VarId>,
    throttle: Option<VarId>,
    brake: Option<VarId>,
    sounds: Vec<(Arc<::legacy_vehicle::SoundCfg>, PathBuf)>,
    interior: Option<(Arc<::legacy_vehicle::SoundCfg>, PathBuf)>,
    part_sounds: Vec<(usize, Arc<::legacy_vehicle::SoundCfg>, PathBuf)>,
}

/// Indexed by `Pose::ibis`: append only.
const IBIS_VARS: [&str; 6] = [
    "IBIS_Linie_Complex",
    "IBIS_Linie_Suffix",
    "IBIS_LinieKurs",
    "IBIS_TerminusIndex",
    "IBIS_TerminusCode",
    "IBIS_RouteIndex",
];

/// Both games must agree on the depot file: the terminus indices of `IBIS_VARS` point into it.
fn hof_key(h: &::legacy_vehicle::Hof) -> u32 {
    let key = format!("{}|{}", h.name.trim().to_lowercase(), h.termini.len());
    fnv1a(key.as_bytes()).max(1)
}

fn engine_fed(name: &str) -> bool {
    const PREFIXES: [&str; 12] = [
        "wheel_",
        "axle_",
        "velocity",
        "ai_",
        "envir_",
        "dirt",
        "precip",
        "rain_",
        "streetcond",
        "door_",
        "pax_",
        "refresh_",
    ];
    let n = name.to_ascii_lowercase();
    PREFIXES.iter().any(|p| n.starts_with(p))
        || matches!(n.as_str(), "n_wheel" | "wetness" | "time" | "timegap")
}

fn outside_entry(e: &::legacy_vehicle::SoundEntry) -> bool {
    let hit = |s: &str| {
        let l = s.to_ascii_lowercase();
        ["hupe", "horn", "blinker", "kneel"]
            .iter()
            .any(|k| l.contains(k))
    };
    e.triggers.iter().any(|t| hit(t))
        || e.vol_curves.iter().any(|c| hit(&c.variable))
        || e.conditions.iter().any(|c| hit(&c.variable))
}

fn fnv1a(data: &[u8]) -> u32 {
    let mut h: u32 = 0x811c_9dc5;
    for b in data {
        h ^= *b as u32;
        h = h.wrapping_mul(0x0100_0193);
    }
    h
}

impl SyncTable {
    /// `parts`: rear sections, whose lamps, displays, moving parts and sounds run on the
    /// leading vehicle's variables.
    pub fn new(ty: &::simulation::VehicleType, parts: &[Arc<::simulation::VehicleType>]) -> SyncTable {
        let program = &ty.program;
        let types: Vec<&::simulation::VehicleType> = std::iter::once(ty)
            .chain(parts.iter().map(|p| p.as_ref()))
            .collect();
        let var = |n: &str| -> Option<VarId> {
            let n = n.trim();
            if n.is_empty() || n.parse::<f32>().is_ok() {
                return None;
            }
            program.var(n)
        };
        let collect = |names: &mut dyn Iterator<Item = String>,
                       skip: &dyn Fn(&str) -> bool,
                       cap: usize|
         -> Vec<(String, VarId)> {
            let mut v: Vec<(String, VarId)> = names
                .filter_map(|n| var(&n).map(|id| (program.var_names[id as usize].clone(), id)))
                .filter(|(n, _)| !skip(n))
                .collect();
            v.sort_by_key(|(n, _)| n.to_ascii_lowercase());
            v.dedup_by_key(|(n, _)| n.to_ascii_lowercase());
            v.truncate(cap);
            v
        };
        let paint_vars: Vec<String> = ty
            .paint_schemes
            .iter()
            .flat_map(|s| s.set_vars.iter().map(|(n, _)| n.to_ascii_lowercase()))
            .collect();
        let lamp_names = types
            .iter()
            .flat_map(|t| t.model.meshes.iter())
            .flat_map(|m| {
                m.materials
                    .iter()
                    .flat_map(|mat| {
                        mat.lightmap
                            .as_ref()
                            .map(|l| l.1.clone())
                            .into_iter()
                            .chain(mat.change.as_ref().map(|c| c.2.clone()))
                    })
                    .chain(m.light_enh.iter().map(|l| l.variable.clone()))
                    .chain(m.light_enh_2.iter().map(|l| l.variable.clone()))
            })
            .chain(
                types
                    .iter()
                    .flat_map(|t| t.model.interior_lights.iter())
                    .map(|l| l.variable.clone()),
            );
        let lamps = collect(
            &mut lamp_names.collect::<Vec<_>>().into_iter(),
            &|n| engine_fed(n) || paint_vars.contains(&n.to_ascii_lowercase()),
            ::network::wire::MAX_LAMPS,
        );
        let switch_names: Vec<String> = types
            .iter()
            .flat_map(|t| t.model.meshes.iter())
            .filter_map(|m| m.visible.as_ref().map(|v| v.0.clone()))
            .collect();
        let lamp_set: Vec<VarId> = lamps.iter().map(|l| l.1).collect();
        let switches = collect(
            &mut switch_names.into_iter(),
            &|n| {
                engine_fed(n)
                    || paint_vars.contains(&n.to_ascii_lowercase())
                    || var(n).map(|id| lamp_set.contains(&id)).unwrap_or(true)
            },
            ::network::wire::MAX_SWITCHES,
        );
        let def = &ty.def;
        let load = |rel: &str| -> Option<(Arc<::legacy_vehicle::SoundCfg>, PathBuf)> {
            let path = ::legacy_config::resolve_path(def.dir(), rel);
            match ::legacy_vehicle::SoundCfg::load(&path) {
                Ok(c) => Some((
                    Arc::new(c),
                    path.parent().map(|p| p.to_path_buf()).unwrap_or_default(),
                )),
                Err(e) => {
                    log::warn!("LAN: sounds of {}: {e}", def.path.display());
                    None
                }
            }
        };
        let mut sounds = Vec::new();
        let interior = def.sound.as_deref().and_then(|rel| load(rel));
        if let Some(rel) = def.sound_ai.as_deref().or(def.sound.as_deref()) {
            sounds.extend(load(rel));
        }
        if let (Some(_), Some(full)) = (def.sound_ai.as_deref(), def.sound.as_deref()) {
            if let Some((cfg, dir)) = load(full) {
                let entries: Vec<::legacy_vehicle::SoundEntry> = cfg
                    .sounds
                    .iter()
                    .filter(|e| outside_entry(e))
                    .cloned()
                    .collect();
                if !entries.is_empty() {
                    sounds.push((
                        Arc::new(::legacy_vehicle::SoundCfg {
                            sounds: entries,
                            unknown_keywords: Vec::new(),
                        }),
                        dir,
                    ));
                }
            }
        }
        let mut part_sounds = Vec::new();
        for (k, part) in parts.iter().enumerate() {
            let pdef = &part.def;
            if let Some(rel) = pdef.sound_ai.as_deref().or(pdef.sound.as_deref()) {
                let path = ::legacy_config::resolve_path(pdef.dir(), rel);
                match ::legacy_vehicle::SoundCfg::load(&path) {
                    Ok(c) => part_sounds.push((
                        k,
                        Arc::new(c),
                        path.parent().map(|p| p.to_path_buf()).unwrap_or_default(),
                    )),
                    Err(e) => log::warn!("LAN: sounds of {}: {e}", pdef.path.display()),
                }
            }
        }
        let sound_vars =
            |cfgs: &mut dyn Iterator<Item = &Arc<::legacy_vehicle::SoundCfg>>| -> Vec<String> {
                let mut out = Vec::new();
                for cfg in cfgs {
                    for e in &cfg.sounds {
                        out.extend(e.vol_curves.iter().map(|c| c.variable.clone()));
                        out.extend(e.conditions.iter().map(|c| c.variable.clone()));
                        if !e.pitch_variable.is_empty() {
                            out.push(e.pitch_variable.clone());
                        }
                    }
                }
                out
            };
        let sound_names = sound_vars(&mut sounds.iter().map(|s| &s.0));
        let part_sound_names = sound_vars(&mut part_sounds.iter().map(|s| &s.1));
        let mut value_names: Vec<String> = Vec::new();
        // "kryshka"/"dver": a Russian W906 mod's hand-opened doors (`cp_kryshka1_opn`).
        const DOORISH: [&str; 6] = ["door", "tuer", "tür", "ramp", "kryshka", "dver"];
        let doorish = |t: &str| {
            let t = t.to_ascii_lowercase();
            DOORISH.iter().any(|k| t.contains(k))
        };
        for (t, i, m) in types.iter().flat_map(|t| {
            t.model
                .meshes
                .iter()
                .enumerate()
                .map(move |(i, m)| (t, i, m))
        }) {
            if !t.meshes.iter().any(|vm| vm.def_index == i) {
                continue;
            }
            let file = m.file.to_ascii_lowercase();
            let wiper =
                ["wisch", "wiper"].iter().any(|k| file.contains(k)) && m.mouse_event.is_none();
            let door = doorish(&file)
                || m.mesh_ident.as_deref().is_some_and(|x| doorish(x))
                || m.mouse_event.as_deref().is_some_and(|x| doorish(x))
                || m.animations.iter().any(|a| doorish(&a.variable));
            if wiper || door {
                value_names.extend(m.animations.iter().map(|a| a.variable.clone()));
            }
        }
        // Window rain films are `rain_` (engine-fed) but follow the sender's wipers.
        let rain_film = |n: &str| n.trim().to_ascii_lowercase().starts_with("rain_window");
        for m in types.iter().flat_map(|t| t.model.meshes.iter()) {
            value_names.extend(
                m.materials
                    .iter()
                    .filter_map(|mat| mat.alphascale.clone())
                    .filter(|n| rain_film(n)),
            );
            value_names.extend(m.materials.iter().flat_map(|mat| {
                mat.texcoord_trans_x
                    .iter()
                    .chain(mat.texcoord_trans_y.iter())
                    .cloned()
            }));
        }
        let taken: Vec<VarId> = lamps.iter().chain(&switches).map(|l| l.1).collect();
        // Visible parts first, then sounds: the list is capped, and sorted as one list the
        // rear sections' sound variables pushed visible ones out.
        let skip = |n: &str| {
            (engine_fed(n) && !rain_film(n)) || var(n).map(|id| taken.contains(&id)).unwrap_or(true)
        };
        let mut values = collect(
            &mut value_names.into_iter(),
            &skip,
            ::network::wire::MAX_VALUES,
        );
        for names in [sound_names, part_sound_names] {
            let had: Vec<VarId> = values.iter().map(|v| v.1).collect();
            let more = collect(
                &mut names.into_iter(),
                &|n| skip(n) || var(n).is_some_and(|id| had.contains(&id)),
                ::network::wire::MAX_VALUES - values.len(),
            );
            values.extend(more);
        }
        let doors: Vec<VarId> = (0..::network::wire::MAX_DOORS)
            .map_while(|i| program.var(&format!("door_{i}")))
            .collect();
        let horn_sound_vars: Vec<VarId> = sounds
            .iter()
            .flat_map(|(c, _)| c.sounds.iter())
            .filter(|e| {
                e.triggers
                    .iter()
                    .chain(e.vol_curves.iter().map(|c| &c.variable))
                    .any(|t| {
                        t.to_ascii_lowercase().contains("hupe")
                            || t.to_ascii_lowercase().contains("horn")
                    })
            })
            .flat_map(|e| e.vol_curves.iter().filter_map(|c| var(&c.variable)))
            .collect();
        let horn: Vec<VarId> = ["cockpit_hupe", "cockpit_hupe_swheel", "horn"]
            .iter()
            .filter_map(|n| var(n))
            .chain(horn_sound_vars)
            .collect();
        let mut key = String::new();
        for (tag, list) in [
            ("lamps", &lamps),
            ("switches", &switches),
            ("values", &values),
        ] {
            key.push_str(tag);
            key.push(':');
            for (n, _) in list.iter() {
                key.push_str(&n.to_ascii_lowercase());
                key.push(',');
            }
            key.push(';');
        }
        key.push_str(&format!("doors:{}", doors.len()));
        SyncTable {
            hash: fnv1a(key.as_bytes()).max(1),
            lamps,
            switches,
            values,
            doors,
            ibis: IBIS_VARS.iter().map(|n| var(n)).collect(),
            horn,
            engine_n: var("engine_n"),
            ai_engine: var("AI_Engine"),
            ai_light: var("AI_Light"),
            ai_interior: var("AI_Interiorlight"),
            throttle: var("Throttle"),
            brake: var("Brake"),
            sounds,
            interior,
            part_sounds,
        }
    }

    pub fn describe(&self) -> String {
        let names =
            |l: &[(String, VarId)]| l.iter().map(|x| x.0.as_str()).collect::<Vec<_>>().join(" ");
        format!(
            "{} lamps, {} switches, {} doors, values [{}], {} sound set(s) and {} of rear sections, table {:08X}",
            self.lamps.len(),
            self.switches.len(),
            self.doors.len(),
            names(&self.values),
            self.sounds.len(),
            self.part_sounds.len(),
            self.hash
        )
    }
}

fn sync_table(game: &mut LanGame, v: &::simulation::VehicleInstance) -> Arc<SyncTable> {
    let ty = &v.ty;
    game.tables
        .entry(ty.def.path.clone())
        .or_insert_with(|| {
            let parts: Vec<Arc<::simulation::VehicleType>> =
                v.trailers.iter().map(|t| t.ty.clone()).collect();
            let t = Arc::new(SyncTable::new(ty, &parts));
            log::info!(
                "LAN: sync table of {}: {}",
                ty.def.path.display(),
                t.describe()
            );
            t
        })
        .clone()
}

pub struct RemoteVehicle {
    vehicle: ::simulation::VehicleInstance,
    render: scene::VehicleRender,
    trailer_renders: Vec<scene::VehicleRender>,
    pub name: String,
    table: Arc<SyncTable>,
    sounds: Vec<::audio::SoundSet>,
    inside_sounds: Option<::audio::SoundSet>,
    target: (DVec3, f64),
    rear: Vec<(DVec3, f64)>,
    pose_seen: (DVec3, Instant),
    doors: Vec<f32>,
    suspension: Vec<f32>,
    values: Vec<f32>,
    odometer: f32,
    horn: bool,
    hof: Option<Arc<::legacy_vehicle::Hof>>,
    shown: (String, String),
    pub stand_in: bool,
    pub last: Pose,
    driver: Option<crate::driver::DriverFigure>,
    driver_tried: bool,
    /// Drawn a little in the past, between two states by their clock: snapping to the
    /// newest arrival made network jitter shake the bus.
    samples: std::collections::VecDeque<(f64, Pose)>,
    offset: Option<f64>,
    play: crate::lan_world::PlayClock,
}

impl RemoteVehicle {
    pub fn vehicle(&self) -> &::simulation::VehicleInstance {
        &self.vehicle
    }
}

// Chat keys are V and '/', not Y: stock keyboard.cfg binds Y's scan code to `scendes_set_z`
// and `view_toggle_informationdisplay`, while V (47) is free.

#[derive(Default)]
pub struct Chat {
    pub lines: Vec<String>,
    pub typing: Option<String>,
    pub hidden: bool,
    pub disabled: bool,
    draft: String,
    /// Keys pressed while typing: their release belongs to the chat too.
    swallow: hashbrown::HashSet<KeyCode>,
    error: Option<(String, Instant)>,
}

impl Chat {
    fn push(&mut self, line: String) {
        self.lines.push(line);
        let over = self.lines.len().saturating_sub(crate::ui::CHAT_KEEP);
        if over > 0 {
            self.lines.drain(..over);
        }
    }

    pub fn error(&self) -> Option<&str> {
        self.error
            .as_ref()
            .filter(|(_, at)| at.elapsed().as_secs_f32() < 6.0)
            .map(|(e, _)| e.as_str())
    }

    pub fn open(&mut self) {
        if !self.disabled && self.typing.is_none() {
            self.hidden = false;
            self.typing = Some(std::mem::take(&mut self.draft));
        }
    }

    pub fn blur(&mut self) {
        if let Some(t) = self.typing.take() {
            self.draft = t;
        }
    }
}

#[derive(Debug, Clone, PartialEq)]
pub enum WorldUpdate {
    Clock {
        year: i32,
        day_of_year: i32,
        time: f64,
    },
    Slew(f64),
    Weather(Option<String>),
    /// Host only: (line, tour), lower case.
    Tours(hashbrown::HashSet<(String, String)>),
}

#[derive(Default)]
pub struct LanGame {
    pub remotes: hashbrown::HashMap<u32, RemoteVehicle>,
    pub chat: Chat,
    pub world: crate::lan_world::LanWorld,
    tables: hashbrown::HashMap<PathBuf, Arc<SyncTable>>,
    adopted: u32,
    tours: hashbrown::HashSet<(String, String)>,
    slew: f64,
    status_t: f32,
    log_t: f32,
    clock: f32,
    last_gap: Option<f64>,
    last_sent: u64,
    last_received: u64,
    weather_seen: Option<String>,
    /// Retried only after a while: retrying every frame stalled a server on an unloadable bus.
    failed: hashbrown::HashMap<(u32, String), std::time::Instant>,
}

pub struct Frame<'a> {
    pub audio: Option<&'a ::audio::AudioEngine>,
    pub listener: Option<DVec3>,
    pub muffled: bool,
    pub riders: usize,
    /// None: an offscreen client, which keeps the clock it started with.
    pub clock: Option<&'a ::simulation::SimClock>,
    pub tour: Option<String>,
    pub walker: Option<::network::Walker>,
    pub inside_of: Option<u32>,
}

pub(crate) fn data_dir() -> Option<PathBuf> {
    let home = std::env::var_os("HOME").or_else(|| std::env::var_os("USERPROFILE"))?;
    Some(PathBuf::from(home).join(".neoomsi"))
}

fn status_path() -> Option<PathBuf> {
    let id = ::legacy_config::env::var("OMSI_INSTANCE")
        .ok()
        .filter(|s| {
            !s.is_empty()
                && s.len() <= 64
                && s.bytes()
                    .all(|b| b.is_ascii_alphanumeric() || b == b'-' || b == b'_')
        })
        .unwrap_or_else(|| std::process::id().to_string());
    Some(data_dir()?.join("lan").join(format!("{id}.json")))
}

pub struct StatusFileGuard;

impl Drop for StatusFileGuard {
    fn drop(&mut self) {
        if let Some(p) = status_path() {
            let _ = std::fs::remove_file(p);
        }
    }
}

fn date_of(clock: &::simulation::SimClock) -> String {
    let (d, m) = clock.day_month();
    format!("{:04}-{m:02}-{d:02}", clock.year)
}

pub fn player_name(args: &Args) -> String {
    let given = args.lan_name.trim();
    if !given.is_empty() && given != "Driver" {
        return given.to_string();
    }
    if let Some(stem) = args
        .driver
        .as_deref()
        .and_then(|d| Path::new(d).file_stem())
        .map(|s| s.to_string_lossy().to_string())
    {
        if !stem.trim().is_empty() && stem != "Driver" {
            return stem;
        }
    }
    #[cfg(target_os = "macos")]
    if let Ok(out) = std::process::Command::new("id").arg("-F").output() {
        let full = String::from_utf8_lossy(&out.stdout).trim().to_string();
        if !full.is_empty() {
            return full;
        }
    }
    std::env::var("USER")
        .or_else(|_| std::env::var("USERNAME"))
        .ok()
        .filter(|u| !u.trim().is_empty())
        .unwrap_or_else(|| "Driver".into())
}

pub fn world_info(args: &Args) -> ::network::WorldInfo {
    let clock = crate::start_clock(args);
    ::network::WorldInfo {
        map: args.map.replace('\\', "/"),
        date: date_of(&clock),
        time: clock.time,
        weather: args.weather.clone().unwrap_or_default().replace('\\', "/"),
        season: args.season.clone().unwrap_or_default(),
    }
}

struct WsPath {
    gateway: Option<::network::ws::WsGateway>,
    tunnel: Option<::network::tunnel::Tunnel>,
    /// Dropping it closes the link.
    _client: Option<::network::ws::WsClient>,
    url: Option<String>,
}

static WS_PATH: std::sync::Mutex<Option<WsPath>> = std::sync::Mutex::new(None);

pub fn close_public_gateway() {
    let path = WS_PATH.lock().ok().and_then(|mut w| w.take());
    drop(path);
}

pub fn tunnel_url() -> Option<String> {
    WS_PATH
        .lock()
        .ok()?
        .as_ref()?
        .tunnel
        .as_ref()?
        .url
        .lock()
        .ok()?
        .clone()
}

/// The tunnel is the way in for players whose router and ours cannot be hole-punched.
pub fn open_public_gateway(
    session: &LanSession,
    info: ::network::ws::ServerInfo,
    web_port: u16,
    want_tunnel: bool,
) {
    let Some(udp) = session.local_addr() else {
        return;
    };
    let target = SocketAddr::from(([127, 0, 0, 1], udp.port()));
    let port = if web_port == 0 {
        udp.port().saturating_add(10)
    } else {
        web_port
    };
    let gateway = match ::network::ws::WsGateway::start(
        SocketAddr::from(([0, 0, 0, 0], port)),
        target,
        info.clone(),
    )
    .or_else(|_| ::network::ws::WsGateway::start(SocketAddr::from(([0, 0, 0, 0], 0)), target, info))
    {
        Ok(g) => g,
        Err(e) => {
            log::warn!("LAN: no WebSocket gateway: {e}");
            return;
        }
    };
    if let Ok(mut w) = WS_PATH.lock() {
        *w = Some(WsPath {
            gateway: Some(gateway),
            tunnel: None,
            _client: None,
            url: None,
        });
    }
    if !want_tunnel
        || ::legacy_config::env::var_os("OMSI_NO_TUNNEL").is_some()
        || ::legacy_config::env::var_os("OMSI_NO_BRIDGE").is_some()
    {
        return;
    }
    // Threaded: cloudflared may have to be downloaded first.
    let sid = session.session;
    let gw_port = port_of_gateway();
    let _ = std::thread::Builder::new()
        .name("tunnel".into())
        .spawn(move || {
            let Some(port) = gw_port else { return };
            let Some(t) = ::network::tunnel::Tunnel::start(port) else {
                log::info!(
                    "LAN: no tunnel: players whose routers cannot be reached join by the code alone"
                );
                return;
            };
            let mut url = t.url.clone();
            if let Ok(mut w) = WS_PATH.lock() {
                if let Some(w) = w.as_mut() {
                    w.tunnel = Some(t);
                }
            }
            // Reposted every half hour: the relay keeps it for hours, joiners take the latest.
            let mut posted: Option<(String, Instant)> = None;
            let mut checked = Instant::now();
            // `OMSI_OFFICIAL_KEY`: path to the official server's signing key.
            let official = ::legacy_config::env::var_os("OMSI_OFFICIAL_KEY").and_then(|p| {
                std::fs::read(&p)
                    .map_err(|e| {
                        log::warn!("official key {}: {e}", std::path::Path::new(&p).display())
                    })
                    .ok()
            });
            let mut announced: Option<(String, Instant)> = None;
            loop {
                let now = url.lock().ok().and_then(|u| u.clone());
                if let (Some(key), Some(u)) = (official.as_ref(), now.as_ref()) {
                    let due = announced
                        .as_ref()
                        .is_none_or(|(a, t)| a != u || t.elapsed() > Duration::from_secs(300));
                    if due {
                        match ::network::official::announce(u, key) {
                            Ok(()) => log::info!("official server: announced at {u}"),
                            Err(e) => log::warn!("official server: not announced: {e}"),
                        }
                        announced = Some((u.clone(), Instant::now()));
                    }
                }
                match (now, &posted) {
                    (Some(u), None) => {
                        ::network::bridge::post_tunnel(sid, &u);
                        posted = Some((u, Instant::now()));
                    }
                    (Some(u), Some((p, t)))
                        if *p != u || t.elapsed() > Duration::from_secs(1800) =>
                    {
                        ::network::bridge::post_tunnel(sid, &u);
                        posted = Some((u, Instant::now()));
                    }
                    _ => {}
                }
                if Arc::strong_count(&url) == 1 {
                    return;
                }
                // Cloudflare drops quick tunnels now and then: start a new one and repost it.
                if checked.elapsed() > Duration::from_secs(15) {
                    checked = Instant::now();
                    let dead = WS_PATH
                        .lock()
                        .ok()
                        .and_then(|mut w| {
                            w.as_mut()
                                .and_then(|w| w.tunnel.as_mut().map(|t| !t.alive()))
                        })
                        .unwrap_or(false);
                    if dead {
                        log::warn!("LAN: the tunnel ended; starting a new one");
                        if let Some(t) = ::network::tunnel::Tunnel::start(port) {
                            url = t.url.clone();
                            posted = None;
                            if let Ok(mut w) = WS_PATH.lock() {
                                if let Some(w) = w.as_mut() {
                                    w.tunnel = Some(t);
                                }
                            }
                        }
                    }
                }
                std::thread::sleep(Duration::from_secs(2));
            }
        });
}

pub fn publish_vehicles(root: PathBuf, only: Vec<String>) {
    let _ = std::thread::Builder::new()
        .name("vehicle list".into())
        .spawn(move || {
            let list: Vec<String> = if only.is_empty() {
                crate::menu::Menu::new(&root, "")
                    .vehicles
                    .into_iter()
                    .map(|v| v.1)
                    .collect()
            } else {
                only
            };
            log::info!("LAN: {} buses offered to joining players", list.len());
            if let Ok(w) = WS_PATH.lock() {
                if let Some(g) = w.as_ref().and_then(|w| w.gateway.as_ref()) {
                    if let Ok(mut i) = g.info.lock() {
                        i.vehicles = list;
                    }
                }
            }
        });
}

fn port_of_gateway() -> Option<u16> {
    WS_PATH
        .lock()
        .ok()?
        .as_ref()?
        .gateway
        .as_ref()
        .map(|g| g.addr.port())
}

pub fn update_server_info(players: usize, time: &str, weather: &str) {
    if let Ok(w) = WS_PATH.lock() {
        if let Some(g) = w.as_ref().and_then(|w| w.gateway.as_ref()) {
            if let Ok(mut i) = g.info.lock() {
                i.players = players;
                i.time = time.to_string();
                if !weather.is_empty() {
                    i.weather = weather.to_string();
                }
            }
        }
    }
}

pub fn update_server_players(list: Vec<::network::ws::PlayerInfo>) {
    if let Ok(w) = WS_PATH.lock() {
        if let Some(g) = w.as_ref().and_then(|w| w.gateway.as_ref()) {
            if let Ok(mut i) = g.info.lock() {
                i.player_list = list;
            }
        }
    }
}

pub fn take_local_admin() -> Vec<String> {
    if let Ok(w) = WS_PATH.lock() {
        if let Some(g) = w.as_ref().and_then(|w| w.gateway.as_ref()) {
            if let Ok(mut i) = g.info.lock() {
                return std::mem::take(&mut i.local_admin_queue);
            }
        }
    }
    Vec::new()
}

fn ws_join_target(url: &str) -> Result<String, String> {
    let c = ::network::ws::WsClient::connect(url)?;
    let local = c.local;
    if let Ok(mut w) = WS_PATH.lock() {
        *w = Some(WsPath {
            gateway: None,
            tunnel: None,
            _client: Some(c),
            url: Some(url.to_string()),
        });
    }
    Ok(local.to_string())
}

/// A joiner waits for the host's welcome so its map is loaded with the host's world.
pub fn start(args: &Args) -> Option<LanSession> {
    let world = world_info(args);
    let session = match (&args.lan_host, &args.lan_join) {
        (Some(port), _) => {
            let (p, try_next) = if *port == 0 {
                (::network::DEFAULT_PORT, true)
            } else {
                (*port, false)
            };
            match LanSession::host(p, &player_name(args), world, try_next) {
                Ok(s) => Some(s),
                Err(e) => {
                    log::warn!("LAN: cannot host on port {p}: {e}");
                    write_failure(&format!("cannot host on port {p}: {e}"));
                    None
                }
            }
        }
        (None, Some(target)) => {
            let direct = match ::network::ws::ws_url(target) {
                Some(url) => match ws_join_target(&url) {
                    Ok(local) => local,
                    Err(e) => {
                        log::warn!("LAN: cannot reach '{target}': {e}");
                        write_failure(&format!("cannot reach '{target}': {e}"));
                        return None;
                    }
                },
                None => target.clone(),
            };
            match LanSession::join(&direct, &player_name(args), world, Duration::from_secs(3)) {
                Ok(s) => Some(s),
                Err(e) => {
                    log::warn!("LAN: cannot join '{target}': {e}");
                    write_failure(&format!("cannot join '{target}': {e}"));
                    None
                }
            }
        }
        _ => None,
    };
    let mut session = session?;
    if let Some(c) = session.code() {
        log::info!(
            "LAN: other players join with the code {}  (or by an address: {})",
            c.encode(),
            host_addresses(c.port)
                .iter()
                .map(|(label, addr, _)| format!("{label} {addr}"))
                .collect::<Vec<_>>()
                .join(", ")
        );
        for a in ::network::addrs::local_addresses() {
            log::info!(
                "LAN: address {} on {} ({}{})",
                a.ip,
                a.interface,
                a.label(),
                if a.kind.reachable() {
                    ""
                } else {
                    ", not offered: nobody else reaches it"
                }
            );
        }
    }
    if session.role == Role::Client {
        let planned = Pose {
            bus: args.bus.clone().unwrap_or_default().replace('\\', "/"),
            ..Default::default()
        };
        let t0 = Instant::now();
        while t0.elapsed() < HOST_ANSWER_WAIT && !session.connected && session.rejected.is_none() {
            session.tick(0.02, &planned);
            std::thread::sleep(Duration::from_millis(20));
        }
        if session.welcome.is_none()
            && session.rejected.is_none()
            && args
                .lan_join
                .as_deref()
                .map(|t| ::network::ws::ws_url(t).is_none())
                .unwrap_or(false)
        {
            if let Some(url) = ::network::SessionCode::decode(args.lan_join.as_deref().unwrap_or(""))
                .ok()
                .and_then(|c| ::network::bridge::lookup_tunnel(c.session))
            {
                log::info!(
                    "LAN: no answer at the code's addresses; through the host's tunnel {url}"
                );
                if let Some(ws) = ::network::ws::ws_url(&url) {
                    match ws_join_target(&ws).and_then(|local| {
                        LanSession::join(
                            &local,
                            &player_name(args),
                            world_info(args),
                            Duration::from_secs(3),
                        )
                    }) {
                        Ok(mut s2) => {
                            let t1 = Instant::now();
                            while t1.elapsed() < WELCOME_WAIT * 2
                                && !s2.connected
                                && s2.rejected.is_none()
                            {
                                s2.tick(0.02, &planned);
                                std::thread::sleep(Duration::from_millis(20));
                            }
                            session = s2;
                        }
                        Err(e) => log::warn!("LAN: the tunnel {url} did not answer: {e}"),
                    }
                }
            }
        }
        match (&session.welcome, &session.rejected) {
            (Some(_), _) => log::info!("LAN: welcome after {:.2} s", t0.elapsed().as_secs_f32()),
            (None, Some(why)) => log::warn!("LAN: turned away: {why}"),
            (None, None) => log::info!(
                "LAN: no welcome within {:.0} s; the host's world is taken over when it comes",
                HOST_ANSWER_WAIT.as_secs_f32()
            ),
        }
    }
    if session.role == Role::Host && args.server.is_none() {
        let info = ::network::ws::ServerInfo {
            name: format!("{}'s game", player_name(args)),
            map: args.map.clone(),
            max_players: 16,
            version: env!("CARGO_PKG_VERSION").into(),
            ..Default::default()
        };
        open_public_gateway(&session, info, 0, true);
        publish_vehicles(args.root.clone(), Vec::new());
    }
    write_status(&session, &Default::default(), None);
    Some(session)
}

pub fn share_mods(args: &mut Args, lan: &mut LanSession) {
    match lan.role {
        Role::Host => {
            if let Some(port) = lan.local_addr().map(|a| a.port()) {
                crate::lan_mods::serve(port, lan.session, args);
            }
        }
        Role::Client => {
            let Some(mut host) = lan.host.filter(|_| lan.welcome.is_some()) else {
                log::info!("LAN map resources: no host to ask yet");
                return;
            };
            let note = |lan: &mut LanSession, text: String| {
                lan.warnings.retain(|w| !w.starts_with("Host's map"));
                lan.warnings.push(text);
                write_status(lan, &Default::default(), None);
            };
            note(lan, "Host's map: checking resources…".into());
            let session = lan.session;
            // the host's TCP port may be unreachable (UDP-only forward, or joined over a
            // WebSocket): the files go through its tunnel then
            let joined_url = WS_PATH
                .lock()
                .ok()
                .and_then(|w| w.as_ref().and_then(|w| w.url.clone()));
            let direct = joined_url.is_none()
                && std::net::TcpStream::connect_timeout(&host, Duration::from_secs(3)).is_ok();
            if !direct {
                let base = joined_url.or_else(|| {
                    ::network::bridge::lookup_tunnel(session).and_then(|u| ::network::ws::ws_url(&u))
                });
                match base.map(|b| format!("{}/tcp", b.strip_suffix("/ws").unwrap_or(&b))) {
                    Some(tcp_url) => match ::network::ws::tcp_forward(&tcp_url) {
                        Ok(local) => {
                            log::info!(
                                "LAN map resources: the host's TCP port is out of reach; through {tcp_url}"
                            );
                            host = local;
                        }
                        Err(e) => log::warn!("LAN map resources: {e}"),
                    },
                    None => {
                        note(lan, "Host's map resources could not be downloaded; playing with what is installed here".into());
                        return;
                    }
                }
            }
            // in the background with the session kept alive: a join held for minutes was
            // dropped by the host and came back without the mods
            let (tx, rx) = std::sync::mpsc::channel::<String>();
            let mut fargs = args.clone();
            let worker = std::thread::spawn(move || {
                let mut last = Instant::now() - Duration::from_secs(1);
                let r =
                    crate::lan_mods::fetch(&mut fargs, host, session, &mut |done, total, file| {
                        if last.elapsed() > Duration::from_millis(500) {
                            last = Instant::now();
                            let pct = if total > 0 { done * 100 / total } else { 100 };
                            let _ = tx.send(format!(
                                "Host's map: {pct}% ({:.0} of {:.0} MB) {file}",
                                done as f64 / 1e6,
                                total as f64 / 1e6
                            ));
                        }
                    });
                (r, fargs.map)
            });
            let planned = Pose {
                bus: args.bus.clone().unwrap_or_default().replace('\\', "/"),
                ..Default::default()
            };
            while !worker.is_finished() {
                lan.keepalive(0.05, &planned);
                if let Some(line) = rx.try_iter().last() {
                    log::info!("LAN map resources: {line}");
                    note(lan, line);
                }
                std::thread::sleep(Duration::from_millis(50));
            }
            let (result, map) = worker
                .join()
                .unwrap_or_else(|_| (Err("the download stopped".into()), args.map.clone()));
            args.map = map;
            match result {
                Ok(r) => {
                    let text = if r.fetched > 0 {
                        format!(
                            "Host's map: {} files ({:.0} MB) fetched (kept for the next join)",
                            r.fetched,
                            r.bytes as f64 / 1e6
                        )
                    } else {
                        "Host's map: everything needed is installed here".to_string()
                    };
                    note(lan, text);
                    let mine = world_info(args);
                    lan.set_world(mine);
                }
                Err(e) => {
                    log::warn!("LAN map resources: {e}");
                    note(lan, format!("Host's map resources could not be fetched: {e}"));
                }
            }
        }
    }
}

/// Loading a big map takes minutes; without the session kept alive meanwhile nobody can
/// join a host, and a joining game is dropped by its host.
pub fn answering_while<T>(
    lan: &mut Option<LanSession>,
    bus: Option<&str>,
    work: impl FnOnce() -> T,
) -> T {
    let Some(session) = lan.take() else {
        return work();
    };
    let planned = Pose {
        bus: bus.unwrap_or_default().replace('\\', "/"),
        ..Default::default()
    };
    let stop = std::sync::atomic::AtomicBool::new(false);
    std::thread::scope(|s| {
        let keeper = s.spawn(|| {
            let mut session = session;
            while !stop.load(std::sync::atomic::Ordering::Relaxed) {
                session.keepalive(0.05, &planned);
                std::thread::sleep(Duration::from_millis(50));
            }
            session
        });
        let out = work();
        stop.store(true, std::sync::atomic::Ordering::Relaxed);
        match keeper.join() {
            Ok(session) => *lan = Some(session),
            Err(_) => log::warn!("LAN: the session stopped while the world loaded"),
        }
        out
    })
}

/// Not left to the host's mod list: listing a big add-on map can outlast the joiner's
/// wait, leaving the player alone on its own map.
pub fn take_host_map(args: &mut Args, lan: &mut LanSession) {
    if lan.role != Role::Client {
        return;
    }
    let Some(theirs) = lan
        .welcome
        .as_ref()
        .map(|w| w.world.map.trim().replace('\\', "/"))
    else {
        return;
    };
    // compared from `maps/` on: a whole path or an archive path is still the host's map,
    // and mistaking it for another map dropped the chosen line and tour
    let norm = |s: &str| {
        let s = s.trim().replace('\\', "/").to_ascii_lowercase();
        match s.rfind("maps/") {
            Some(k) => s[k..].to_string(),
            None => s,
        }
    };
    if theirs.is_empty() || norm(&theirs) == norm(&args.map) {
        return;
    }
    if crate::lan_mods::refuse_path(&theirs).is_some() || !norm(&theirs).starts_with("maps/") {
        return;
    }
    match ::legacy_config::find_in_roots(&theirs) {
        Some(_) => {
            log::info!(
                "LAN: the session is on the host's map {theirs} (not {})",
                args.map
            );
            args.map = theirs;
            // the entry point, depot file and tour chosen belonged to the other map
            args.entry = 0;
            args.spawn = None;
            args.hof = None;
            args.line = None;
            args.tour = None;
            args.trip = None;
            let mine = world_info(args);
            lan.set_world(mine);
        }
        None => {
            let line = format!(
                "the host plays on {theirs}, which is not installed here - install that map to meet the others"
            );
            log::warn!("LAN: {line}");
            if !lan.warnings.contains(&line) {
                lan.warnings.push(line);
            }
        }
    }
}

fn parse_date(s: &str) -> Option<(i32, i32)> {
    let v: Vec<i32> = s.split('-').filter_map(|x| x.trim().parse().ok()).collect();
    (v.len() == 3).then(|| {
        let mut c = ::simulation::SimClock::default();
        c.set_date(v[0], v[1], v[2]);
        (c.year, c.day_of_year)
    })
}

fn host_clock_now(h: &::network::HostClock) -> Option<::simulation::SimClock> {
    let (year, day_of_year) = parse_date(&h.world.date)?;
    let mut c = ::simulation::SimClock {
        year,
        day_of_year,
        time: h.world.time,
        ..Default::default()
    };
    c.advance(h.at.elapsed().as_secs_f32() * h.speed as f32);
    Some(c)
}

pub fn host_time_now(lan: &LanSession) -> Option<String> {
    let w = lan.welcome.as_ref()?;
    let c = host_clock_now(&::network::HostClock {
        world: w.world.clone(),
        at: w.at,
        speed: 1.0,
    })?;
    let t = c.time.rem_euclid(86400.0) as u32;
    Some(format!("{:02}:{:02}:{:02}", t / 3600, t / 60 % 60, t % 60))
}

fn host_weather(args: &Args, weather: &str) -> Result<Option<String>, String> {
    let w = weather.trim();
    if w.is_empty() {
        return Ok(None);
    }
    if crate::weather_setup::custom_weather(Some(w)).is_some() {
        return Ok(Some(w.to_string()));
    }
    if w.starts_with(crate::weather_setup::REPORT) {
        return if crate::weather_setup::from_report(w).is_some() {
            Ok(Some(w.to_string()))
        } else {
            Err(format!("the host's weather {w} cannot be read here"))
        };
    }
    let path = ::legacy_config::resolve_path(&args.root, w);
    let inside = !w.contains("..") && !w.starts_with('/') && !w.contains(':');
    if inside && ::legacy_config::vfs::is_file(&path) {
        Ok(Some(w.to_string()))
    } else {
        Err(format!("the host's weather {w} is not installed here"))
    }
}

pub fn adopt_host_world(args: &mut Args, lan: &mut LanSession, game: &mut LanGame) {
    if lan.role != Role::Client {
        return;
    }
    let Some(w) = lan.welcome.clone() else { return };
    game.adopted = lan.welcomes;
    let hw = &w.world;
    if let Some(c) = host_clock_now(&::network::HostClock {
        world: hw.clone(),
        at: w.at,
        speed: lan.clock_speed,
    }) {
        args.date = Some(date_of(&c));
        args.day_of_year = None;
        args.time = format!(
            "{:02}:{:02}:{:05.2}",
            (c.time / 3600.0) as i32,
            ((c.time % 3600.0) / 60.0) as i32,
            c.time % 60.0
        );
    }
    let mut warnings = Vec::new();
    match host_weather(args, &hw.weather) {
        Ok(wt) => args.weather = wt,
        Err(e) => warnings.push(e),
    }
    args.season = (!hw.season.trim().is_empty()).then(|| hw.season.trim().to_string());
    log::info!(
        "LAN: taking the host's world: {} {} weather {} season {}",
        args.date.as_deref().unwrap_or(""),
        args.time,
        args.weather.as_deref().unwrap_or("(the map's)"),
        args.season.as_deref().unwrap_or("(by date)")
    );
    let mut mine = world_info(args);
    if warnings.is_empty() {
        mine.weather = hw.weather.clone();
    }
    lan.set_world(mine);
    for line in warnings {
        log::warn!("LAN: {line}");
        lan.warnings.push(line);
    }
}

fn adopt_at_runtime(
    args: &Args,
    lan: &mut LanSession,
    game: &mut LanGame,
    world: Option<&World>,
    clock: &::simulation::SimClock,
) -> Vec<WorldUpdate> {
    let mut out = Vec::new();
    let Some(w) = lan.welcome.clone() else {
        return out;
    };
    game.adopted = lan.welcomes;
    let hw = w.world.clone();
    if let Some(c) = host_clock_now(&::network::HostClock {
        world: hw.clone(),
        at: w.at,
        speed: lan.clock_speed,
    }) {
        log::info!(
            "LAN: the host's clock: {} {:02}:{:02} (ours was {} {:02}:{:02})",
            date_of(&c),
            (c.time / 3600.0) as i32,
            ((c.time % 3600.0) / 60.0) as i32,
            date_of(clock),
            (clock.time / 3600.0) as i32,
            ((clock.time % 3600.0) / 60.0) as i32
        );
        out.push(WorldUpdate::Clock {
            year: c.year,
            day_of_year: c.day_of_year,
            time: c.time,
        });
    }
    let mut warnings = Vec::new();
    let mut weather = args.weather.clone();
    match host_weather(args, &hw.weather) {
        Ok(wt) => {
            let norm = |s: &Option<String>| {
                s.as_deref()
                    .unwrap_or("")
                    .trim()
                    .replace('\\', "/")
                    .to_ascii_lowercase()
            };
            if norm(&wt) != norm(&args.weather) {
                out.push(WorldUpdate::Weather(wt.clone()));
            }
            weather = wt;
        }
        Err(e) => warnings.push(e),
    }
    // the season is baked into the loaded textures, so a differing one is only warned about
    if let Some(world) = world {
        let mut theirs = args.clone();
        theirs.date = Some(hw.date.clone());
        theirs.day_of_year = None;
        theirs.season = (!hw.season.trim().is_empty()).then(|| hw.season.trim().to_string());
        theirs.weather = weather.clone();
        let want = crate::season_folder(&theirs, &world.global).1;
        if want != ::texture::season_folder() {
            warnings.push(format!("the host's season ({}) differs from the one loaded here ({}) - start the game again to see it", want.as_deref().unwrap_or("summer"), ::texture::season_folder().as_deref().unwrap_or("summer")));
        }
    }
    let mut mine = world_info(args);
    mine.weather = if warnings.iter().any(|w| w.contains("weather")) {
        mine.weather
    } else {
        hw.weather.clone()
    };
    lan.set_world(mine);
    for line in warnings {
        log::warn!("LAN: {line}");
        lan.warnings.push(line);
    }
    out
}

fn day_number(year: i32, day_of_year: i32) -> i64 {
    let y = year as i64 - 1;
    let before = if year > 0 {
        365 * y + y / 4 - y / 100 + y / 400 + 366
    } else {
        0
    };
    before + day_of_year as i64
}

/// Seconds our clock is behind the host's (negative: ahead).
fn clock_gap(host: &::simulation::SimClock, mine: &::simulation::SimClock) -> f64 {
    (day_number(host.year, host.day_of_year) - day_number(mine.year, mine.day_of_year)) as f64
        * 86400.0
        + host.time
        - mine.time
}

fn write_failure(msg: &str) {
    let Some(p) = status_path() else { return };
    let _ = std::fs::create_dir_all(p.parent().unwrap());
    let v = serde_json::json!({ "pid": std::process::id(), "role": "none", "error": msg, "updated": now_secs() });
    let _ = std::fs::write(p, serde_json::to_vec_pretty(&v).unwrap_or_default());
}

fn now_secs() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0)
}

/// Best first, a VPN's before the LAN's: (label, `ip:port`, kind key).
pub fn host_addresses(port: u16) -> Vec<(&'static str, String, &'static str)> {
    ::network::addrs::joinable_addresses()
        .into_iter()
        .map(|a| (a.label(), format!("{}:{port}", a.ip), a.kind.key()))
        .collect()
}

fn write_status(lan: &LanSession, game: &LanGame, player: Option<&Player>) {
    let Some(p) = status_path() else { return };
    let players: Vec<serde_json::Value> = lan
        .peers()
        .map(|peer| {
            let d = player.filter(|_| peer.pose.has_vehicle()).map(|pl| relative_position(&pl.vehicle, &peer.pose));
            serde_json::json!({ "id": peer.pose.id, "name": peer.pose.name, "bus": peer.pose.bus, "line": peer.pose.line, "destination": peer.pose.destination, "passengers": peer.pose.passengers, "where": d, "drawn": game.remotes.contains_key(&peer.pose.id), "generic_bus": game.remotes.get(&peer.pose.id).map(|v| v.stand_in).unwrap_or(false) })
        })
        .collect();
    let code = lan.code();
    let v = serde_json::json!({
        "pid": std::process::id(),
        "role": if lan.role == Role::Host { "host" } else { "client" },
        "name": lan.my_name,
        "code": code.as_ref().map(|c| c.encode()),
        "address": code.as_ref().map(|c| format!("{}:{}", c.ip(), c.port)).or_else(|| lan.host.map(|h| h.to_string())),
        "addresses": code.as_ref().map(|c| host_addresses(c.port).into_iter().map(|(label, addr, kind)| serde_json::json!({ "label": label, "address": addr, "kind": kind })).collect::<Vec<_>>()).unwrap_or_default(),
        "trying": (lan.role == Role::Client && lan.host.is_none()).then(|| lan.candidates.iter().map(|a| a.to_string()).collect::<Vec<_>>()),
        "port": lan.local_addr().map(|a| a.port()),
        "session": ::network::session_hex(lan.session),
        "tunnel": tunnel_url(),
        "connected": lan.connected,
        "rejected": lan.rejected,
        "warnings": lan.warnings,
        "host_name": lan.welcome.as_ref().map(|w| w.host_name.clone()),
        "map": lan.world.map,
        "players": players,
        "generic_vehicles": game.remotes.values().filter(|vehicle| vehicle.stand_in).count(),
        "chat": game.chat.lines.iter().rev().take(CHAT_LINES).rev().cloned().collect::<Vec<_>>(),
        "updated": now_secs(),
    });
    // on its own thread: a slow disk or a virus scanner stalled the frame every two seconds
    static WRITER: std::sync::OnceLock<Option<std::sync::mpsc::Sender<(PathBuf, Vec<u8>)>>> =
        std::sync::OnceLock::new();
    let writer = WRITER.get_or_init(|| {
        let (tx, rx) = std::sync::mpsc::channel::<(PathBuf, Vec<u8>)>();
        std::thread::Builder::new()
            .name("lan status".into())
            .spawn(move || {
                while let Ok(mut job) = rx.recv() {
                    while let Ok(newer) = rx.try_recv() {
                        job = newer;
                    }
                    let (p, bytes) = job;
                    if let Some(dir) = p.parent() {
                        let _ = std::fs::create_dir_all(dir);
                    }
                    let tmp = p.with_extension("json.tmp");
                    if std::fs::write(&tmp, bytes).is_ok() {
                        let _ = std::fs::rename(&tmp, &p);
                    }
                }
            })
            .ok()
            .map(|_| tx)
    });
    if let Some(tx) = writer {
        let _ = tx.send((p, serde_json::to_vec_pretty(&v).unwrap_or_default()));
    }
}

fn relative_position(me: &::simulation::VehicleInstance, pose: &Pose) -> String {
    let d = DVec3::new(pose.x, pose.y, pose.z) - me.position;
    let dist = d.truncate().length();
    let h = me.heading.to_radians();
    let (fwd, right) = (d.x * h.sin() + d.y * h.cos(), d.x * h.cos() - d.y * h.sin());
    let side = if fwd.abs() >= right.abs() {
        if fwd >= 0.0 { "ahead" } else { "behind" }
    } else if right >= 0.0 {
        "to the right"
    } else {
        "to the left"
    };
    if dist >= 1000.0 {
        format!("{:.1} km {side}", dist / 1000.0)
    } else {
        format!("{dist:.0} m {side}")
    }
}

/// Screen positions are in physical pixels of a `width` x `height` picture.
pub fn name_tags(
    game: &LanGame,
    cam: &::render::Camera,
    width: f32,
    height: f32,
) -> Vec<((f32, f32), String, String, f32)> {
    let vp = cam.view_proj(width / height.max(1.0), cam.position);
    let mut tags = Vec::new();
    for r in game.remotes.values() {
        let v = &r.vehicle;
        let top =
            v.ty.def
                .bounding_box
                .map(|b| (b[5] + b[2] * 0.5) as f64)
                .filter(|t| *t > 1.0)
                .unwrap_or(3.4);
        let p = match r.last.walker {
            Some(w) => DVec3::new(w.x, w.y, w.z + 2.15),
            None => v.position + DVec3::new(0.0, 0.0, top + 0.6),
        };
        let d = (p - cam.position).length();
        if d > 450.0 {
            continue;
        }
        let c = vp * (p - cam.position).as_vec3().extend(1.0);
        if c.w <= 0.1 {
            continue;
        }
        let (x, y) = (c.x / c.w, c.y / c.w);
        if x.abs() > 1.2 || y.abs() > 1.2 {
            continue;
        }
        let name = if r.name.trim().is_empty() {
            format!("player {}", r.last.id)
        } else {
            r.name.trim().to_string()
        };
        let pose = &r.last;
        let mut sub = match (
            pose.line.trim().is_empty(),
            pose.destination.trim().is_empty(),
        ) {
            (false, false) => format!("{} {}", pose.line.trim(), pose.destination.trim()),
            (true, false) => pose.destination.trim().to_string(),
            (false, true) => format!("line {}", pose.line.trim()),
            _ => String::new(),
        };
        if d > 25.0 {
            let dist = if d >= 1000.0 {
                format!("{:.1} km", d / 1000.0)
            } else {
                format!("{:.0} m", d)
            };
            sub = if sub.is_empty() {
                dist
            } else {
                format!("{sub} · {dist}")
            };
        }
        let alpha = (1.0 - ((d as f32 - 300.0) / 150.0)).clamp(0.0, 1.0);
        tags.push((
            ((x + 1.0) * 0.5 * width, (1.0 - y) * 0.5 * height),
            name,
            sub,
            alpha,
        ));
    }
    tags
}

fn content_relative(path: &Path, root: &Path) -> String {
    let mut roots = ::legacy_config::content_roots();
    roots.push(root.to_path_buf());
    for r in roots {
        if let Ok(rel) = path.strip_prefix(&r) {
            return rel.to_string_lossy().replace('\\', "/");
        }
    }
    String::new()
}

pub fn paint_name(args: &Args, ty: &::simulation::VehicleType) -> String {
    let Some(p) = args.paint.as_deref() else {
        return String::new();
    };
    ty.paint_schemes
        .iter()
        .find(|s| s.name.eq_ignore_ascii_case(p))
        .or_else(|| {
            p.parse::<usize>()
                .ok()
                .and_then(|i| ty.paint_schemes.get(i))
        })
        .map(|s| s.name.clone())
        .unwrap_or_default()
}

pub fn footprint_of(v: &::simulation::VehicleInstance, fallback: [f32; 6]) -> Footprint {
    let bb = v.ty.def.bounding_box.unwrap_or(fallback);
    let h = v.heading.to_radians();
    let (cx, cy) = (bb[3] as f64, bb[4] as f64);
    let c = v.position
        + DVec3::new(
            cx * h.cos() + cy * h.sin(),
            -cx * h.sin() + cy * h.cos(),
            0.0,
        );
    let mut length = bb[1];
    let mut centre = c;
    for t in &v.trailers {
        if let Some(tb) = t.ty.def.bounding_box {
            let back = (t.position - v.position).truncate().length() as f32 + tb[1] * 0.5;
            let extra = (back - (bb[1] * 0.5 - bb[4])).max(0.0);
            length += extra;
            centre -= DVec3::new(h.sin(), h.cos(), 0.0) * (extra as f64 * 0.5);
        }
    }
    Footprint {
        x: centre.x,
        y: centre.y,
        z: v.position.z,
        heading: v.heading as f32,
        length,
        width: bb[0],
    }
}

const BUS_BOX: [f32; 6] = [2.5, 11.5, 3.0, 0.0, 0.0, 1.5];
const CAR_BOX: [f32; 6] = [2.0, 4.5, 1.6, 0.0, 0.0, 0.8];

fn line_and_destination(p: &Player, duty: Option<(&str, &str)>) -> (String, String) {
    if let Some((l, d)) = duty {
        return (l.to_string(), d.to_string());
    }
    let v = &p.vehicle;
    let line = v
        .var("IBIS_Linie_Complex")
        .filter(|x| *x >= 100.0)
        .map(|x| ((x / 100.0) as i32).to_string())
        .unwrap_or_default();
    let dest = match (v.var("IBIS_TerminusIndex"), v.host.hof.as_ref()) {
        // terminus code 0 is the blank display ("Leerfeld")
        (Some(i), Some(h)) if i >= 0.0 => h
            .termini
            .get(i as usize)
            .filter(|t| t.code != 0)
            .map(|t| {
                t.strings
                    .first()
                    .cloned()
                    .unwrap_or_else(|| t.texture_id.clone())
            })
            .unwrap_or_default(),
        _ => String::new(),
    };
    (line, dest.trim().to_string())
}

pub fn my_pose(
    game: &mut LanGame,
    p: Option<&Player>,
    args: &Args,
    duty: Option<(&str, &str)>,
    riders: usize,
) -> Pose {
    let Some(p) = p else { return Pose::default() };
    let v = &p.vehicle;
    let table = sync_table(game, v);
    let val = |n: &str| v.var(n).unwrap_or(0.0);
    let on = |n: &str| val(n) > 0.5;
    let get = |id: VarId| v.state.vars.get(id as usize).copied().unwrap_or(0.0);
    let rpm = table.engine_n.map(get).unwrap_or(0.0);
    let mut flags = ::network::FLAG_VEHICLE;
    for (bit, set) in [
        (::network::FLAG_ENGINE, on("engine_on") || rpm > 100.0),
        (
            ::network::FLAG_ELECTRICS,
            on("elec_busbar_main") || on("elec_busbar_avail"),
        ),
        (
            ::network::FLAG_HORN,
            table.horn.iter().any(|id| get(*id) > 0.5),
        ),
        (::network::FLAG_BRAKE, on("lights_brems")),
        (::network::FLAG_REVERSE, on("lights_rueckfahr")),
        (::network::FLAG_FOG, on("lights_nebelschluss")),
        (
            ::network::FLAG_KNEELING,
            ["bremse_kneeling", "vdv_kneel", "ecas_kneel", "kneeling"]
                .iter()
                .any(|n| on(n)),
        ),
        (
            ::network::FLAG_WIPERS,
            on("wiperrunning") || on("wiper_running"),
        ),
        (::network::FLAG_STOP_BRAKE, on("bremse_halte")),
    ] {
        if set {
            flags |= bit;
        }
    }
    let head = if on("lights_fern") {
        3
    } else if on("lights_abbl")
        || on("lights_main")
        || v.var("Spot_Select").is_some_and(|s| s >= 0.0)
    {
        // mod buses name their lamps their own way; the selected spotlight is the dipped
        // beam every script sets for the renderer
        2
    } else if on("lights_stand") {
        1
    } else {
        0
    };
    // 0 off, 1 left, 2 right, 3 hazard: the script's switch if it has one, else the lamps
    let blinker = if on("lights_sw_warnblinker") {
        3
    } else {
        match v.var("lights_sw_blinker") {
            Some(s) if (0.5..2.5).contains(&s) => s.round() as u8,
            _ => match (on("lights_blinker_l"), on("lights_blinker_r")) {
                (true, true) => 3,
                (true, false) => 1,
                (false, true) => 2,
                _ => 0,
            },
        }
    };
    let fp = footprint_of(v, BUS_BOX);
    let bb = v.ty.def.bounding_box.unwrap_or(BUS_BOX);
    let h = v.heading.to_radians();
    let box_offset = ((fp.x - v.position.x) * h.sin() + (fp.y - v.position.y) * h.cos()) as f32;
    let (line, destination) = line_and_destination(p, duty);
    let texts = display_texts(v);
    let freetex = freetex_values(v);
    let (hof, ibis) = match v.host.hof.as_deref() {
        Some(h) => (
            hof_key(h),
            table.ibis.iter().map(|id| id.map(get)).collect(),
        ),
        None => (0, Vec::new()),
    };
    Pose {
        id: 0,
        name: String::new(),
        bus: content_relative(&v.ty.def.path, &args.root),
        paint: paint_name(args, &v.ty),
        line,
        destination,
        tour: String::new(),
        texts,
        freetex,
        hof,
        ibis,
        figure: p
            .driver
            .as_ref()
            .map(|d| content_relative(&d.human_type().def.path, &args.root))
            .unwrap_or_default(),
        length: fp.length.max(bb[1]),
        width: bb[0],
        box_offset,
        table: table.hash,
        x: v.position.x,
        y: v.position.y,
        z: v.position.z,
        heading: v.heading as f32,
        pitch: v.pitch,
        bank: v.bank,
        speed_kmh: v.physics.velocity_kmh(),
        steer_deg: v.physics.steer_deg,
        flags,
        head,
        interior: (v.interior_light() * 3.0).round() as u8,
        blinker,
        rpm,
        throttle: v.physics.controls.throttle,
        brake: v.physics.controls.brake,
        passengers: riders as u32,
        doors: table.doors.iter().map(|id| get(*id)).collect(),
        suspension: v
            .physics
            .wheels
            .iter()
            .flat_map(|a| a.iter().map(|w| w.suspension))
            .collect(),
        rear: v
            .trailers
            .iter()
            .map(|t| PartPose {
                x: t.position.x,
                y: t.position.y,
                z: t.position.z,
                heading: t.heading as f32,
            })
            .collect(),
        lamps: table.lamps.iter().map(|(_, id)| get(*id)).collect(),
        switches: table.switches.iter().map(|(_, id)| get(*id)).collect(),
        values: table.values.iter().map(|(_, id)| get(*id)).collect(),
        walker: None,
        sent_ms: 0,
    }
}

/// Includes rear sections' display variables the model lacks, else their signs stay blank.
fn display_vars(v: &::simulation::VehicleInstance) -> Vec<String> {
    let mut names: Vec<String> = Vec::new();
    let models = std::iter::once(&v.ty.model).chain(v.trailers.iter().map(|t| &t.ty.model));
    for t in models.flat_map(|m| m.text_textures.iter()) {
        let n = t.variable.trim();
        if !names.iter().any(|x| x.eq_ignore_ascii_case(n)) {
            names.push(n.to_string());
        }
    }
    names.truncate(::network::MAX_TEXTS);
    names
}

fn display_texts(v: &::simulation::VehicleInstance) -> Vec<String> {
    display_vars(v)
        .iter()
        .map(|n| {
            v.ty.program
                .str_var(n)
                .and_then(|i| v.state.str_vars.get(i as usize))
                .cloned()
                .unwrap_or_default()
        })
        .collect()
}

/// Sorted by name: both games must agree on the order on the wire.
fn freetex_names(ty: &::simulation::VehicleType) -> Vec<String> {
    let mut names: Vec<String> = ty
        .model
        .meshes
        .iter()
        .flat_map(|m| {
            m.materials
                .iter()
                .filter_map(|mat| mat.freetex.as_ref().map(|f| f.1.trim().to_string()))
        })
        .filter(|n| ty.program.str_var(n).is_some())
        .collect();
    names.sort_by_key(|n| n.to_ascii_lowercase());
    names.dedup_by_key(|n| n.to_ascii_lowercase());
    names.truncate(::network::MAX_FREETEX);
    names
}

fn freetex_values(v: &::simulation::VehicleInstance) -> Vec<String> {
    freetex_names(&v.ty)
        .iter()
        .map(|n| {
            v.ty.program
                .str_var(n)
                .and_then(|i| v.state.str_vars.get(i as usize))
                .cloned()
                .unwrap_or_default()
        })
        .collect()
}

/// The remote copy's scripts don't drive the roller blind, so it would stay blank.
fn show_freetex(v: &mut ::simulation::VehicleInstance, values: &[String]) {
    if values.is_empty() {
        return;
    }
    for (name, value) in freetex_names(&v.ty).iter().zip(values) {
        if let Some(i) = v.ty.program.str_var(name) {
            if let Some(s) = v.state.str_vars.get_mut(i as usize) {
                if s != value {
                    *s = value.clone();
                }
            }
        }
    }
}

fn show_display_texts(v: &mut ::simulation::VehicleInstance, texts: &[String]) {
    if texts.is_empty() {
        return;
    }
    for (name, text) in display_vars(v).iter().zip(texts) {
        if let Some(i) = v.ty.program.str_var(name) {
            if let Some(s) = v.state.str_vars.get_mut(i as usize) {
                if s != text {
                    *s = text.clone();
                }
            }
        }
    }
}

fn host_footprints(
    p: Option<&Player>,
    traffic: Option<&crate::traffic::Traffic>,
) -> Vec<Footprint> {
    let mut out = Vec::new();
    if let Some(p) = p {
        out.push(footprint_of(&p.vehicle, BUS_BOX));
    }
    if let Some(t) = traffic {
        out.extend(t.cars.iter().map(|c| footprint_of(&c.vehicle, CAR_BOX)));
    }
    out
}

fn obb(f: &Footprint, grow: bool) -> ::simulation::collision::Obb {
    let (ga, gc) = if grow {
        (GAP_ALONG * 0.5, GAP_ACROSS * 0.5)
    } else {
        (0.0, 0.0)
    };
    ::simulation::collision::Obb {
        center: glam::DVec2::new(f.x, f.y),
        half: glam::DVec2::new(f.width as f64 * 0.5 + gc, f.length as f64 * 0.5 + ga),
        heading: (f.heading as f64).to_radians(),
        z0: f.z - 3.0,
        z1: f.z + 4.0,
        velocity: glam::DVec2::ZERO,
        mass: 0.0,
        pole: None,
        id: -1,
    }
}

fn blocked(me: &Footprint, occupied: &[Footprint]) -> bool {
    let a = obb(me, true);
    occupied.iter().any(|o| {
        let b = obb(o, true);
        (a.center - b.center).length() <= a.radius() + b.radius() && a.overlaps(&b)
    })
}

fn along(lanes: &[Lane], prev: &[Vec<usize>], lane: usize, s: f32, d: f32) -> Option<(DVec3, f32)> {
    let mut li = lane;
    let mut s = s + d;
    for _ in 0..16 {
        let l = &lanes[li];
        let len = l.length();
        if s < 0.0 {
            match prev.get(li).and_then(|p| {
                p.iter()
                    .copied()
                    .filter(|&i| lanes[i].kind == l.kind)
                    .min_by(|&a, &b| {
                        heading_gap(lanes[a].end_heading(), l.start_heading())
                            .total_cmp(&heading_gap(lanes[b].end_heading(), l.start_heading()))
                    })
            }) {
                Some(p) => {
                    s += lanes[p].length();
                    li = p;
                    continue;
                }
                None => {
                    let h = (l.start_heading() as f64).to_radians();
                    return Some((
                        l.start() + DVec3::new(h.sin(), h.cos(), 0.0) * s as f64,
                        l.start_heading(),
                    ));
                }
            }
        }
        if s > len {
            match l
                .next
                .iter()
                .copied()
                .filter(|&i| lanes[i].kind == l.kind)
                .min_by(|&a, &b| {
                    heading_gap(lanes[a].start_heading(), l.end_heading())
                        .total_cmp(&heading_gap(lanes[b].start_heading(), l.end_heading()))
                }) {
                Some(n) => {
                    s -= len;
                    li = n;
                    continue;
                }
                None => {
                    let h = (l.end_heading() as f64).to_radians();
                    return Some((
                        l.end() + DVec3::new(h.sin(), h.cos(), 0.0) * (s - len) as f64,
                        l.end_heading(),
                    ));
                }
            }
        }
        return Some(l.at(s));
    }
    None
}

fn heading_gap(a: f32, b: f32) -> f32 {
    ((a - b + 540.0).rem_euclid(360.0) - 180.0).abs()
}

/// Returns the distance moved: 0 if the spawn was free, None if no free place was found.
pub fn clear_spawn(
    p: &mut Player,
    occupied: &[Footprint],
    world: &World,
    net: Option<&Network>,
) -> Option<f64> {
    let me = footprint_of(&p.vehicle, BUS_BOX);
    if !blocked(&me, occupied) {
        log::info!(
            "LAN: our spawn at ({:.1}, {:.1}) is free ({} vehicle(s) nearby)",
            p.vehicle.position.x,
            p.vehicle.position.y,
            occupied.len()
        );
        return Some(0.0);
    }
    let v = &p.vehicle;
    let h0 = v.heading.to_radians();
    let dc = DVec3::new(me.x, me.y, me.z) - v.position;
    let (c_along, c_across) = (
        dc.x * h0.sin() + dc.y * h0.cos(),
        dc.x * h0.cos() - dc.y * h0.sin(),
    );
    let statics = v.collision.clone();
    let free_at = |centre: DVec3, heading: f64| -> Option<DVec3> {
        let fp = Footprint {
            x: centre.x,
            y: centre.y,
            z: centre.z,
            heading: heading as f32,
            length: me.length,
            width: me.width,
        };
        if blocked(&fp, occupied) {
            return None;
        }
        let z = world.ground_height(centre.x, centre.y)?;
        if let Some(cw) = statics.as_ref() {
            let mut b = obb(&fp, false);
            b.z0 = z + 0.3;
            b.z1 = z + 3.0;
            if cw.hit(&b).is_some() {
                return None;
            }
        }
        let h = heading.to_radians();
        Some(DVec3::new(
            centre.x - c_along * h.sin() - c_across * h.cos(),
            centre.y - c_along * h.cos() + c_across * h.sin(),
            z,
        ))
    };
    let world_lanes;
    let (lanes, prev): (&[Lane], &[Vec<usize>]) = match net {
        Some(n) if !n.lanes.is_empty() => (&n.lanes, &n.prev),
        _ => {
            world_lanes = world.lanes.lock().clone();
            (&world_lanes, &[])
        }
    };
    let centre = DVec3::new(me.x, me.y, me.z);
    let mut best_lane: Option<(usize, f32, f64, bool)> = None;
    for (i, l) in lanes.iter().enumerate() {
        if l.kind != LaneKind::Street || l.points.len() < 2 {
            continue;
        }
        if (l.points[0] - centre).truncate().length() > l.length() as f64 + 10.0 {
            continue;
        }
        if let Some((s, d)) = l.nearest_point(centre) {
            if d > 6.0 {
                continue;
            }
            let same_way = heading_gap(l.at(s).1, v.heading as f32) < 60.0;
            let score = d + if same_way { 0.0 } else { 4.0 };
            if best_lane.map(|b| score < b.2).unwrap_or(true) {
                best_lane = Some((i, s, score, same_way));
            }
        }
    }
    let mut placed: Option<(DVec3, f64, f64)> = None;
    let mut on_lane: Option<(usize, f32, f32)> = None;
    if let Some((li, s, _, same_way)) = best_lane {
        let sign = if same_way { 1.0 } else { -1.0 };
        // A straight box on a bend leaves its ends (or rear section) off the road, so prefer
        // a straight stretch a little further away.
        let reach = me.length * 0.5 + 1.0;
        let bend = |c: f32| -> f32 {
            match (
                along(lanes, prev, li, s, c + reach),
                along(lanes, prev, li, s, c - reach),
            ) {
                (Some(a), Some(b)) => heading_gap(a.1, b.1),
                _ => 90.0,
            }
        };
        let mut fallback: Option<(DVec3, f64, f64, f32)> = None;
        'search: for k in 1..=90 {
            let d = k as f32 * 1.5;
            for dd in [-d, d] {
                if let Some((pos, lh)) = along(lanes, prev, li, s, dd * sign) {
                    let heading = if same_way {
                        lh as f64
                    } else {
                        lh as f64 + 180.0
                    }
                    .rem_euclid(360.0);
                    if let Some(origin) = free_at(pos, heading) {
                        if bend(dd * sign) < 12.0 {
                            placed = Some((origin, heading, dd as f64));
                            on_lane = Some((li, s, dd * sign));
                            break 'search;
                        }
                        if fallback.is_none() {
                            fallback = Some((origin, heading, dd as f64, dd * sign));
                        }
                    }
                }
            }
            if d > 60.0 && fallback.is_some() {
                break;
            }
        }
        if placed.is_none() {
            if let Some((o, h, m, at)) = fallback {
                placed = Some((o, h, m));
                on_lane = Some((li, s, at));
            }
        }
    }
    if placed.is_none() {
        let (fwd, right) = (
            DVec3::new(h0.sin(), h0.cos(), 0.0),
            DVec3::new(h0.cos(), -h0.sin(), 0.0),
        );
        'rows: for row in [0.0, 1.0, -1.0, 2.0, -2.0] {
            let side = right * row * (me.width as f64 + GAP_ACROSS + 0.5);
            for k in 0..=40 {
                let d = k as f64 * 1.5;
                for dd in [-d, d] {
                    if let Some(origin) = free_at(centre + side + fwd * dd, v.heading) {
                        placed = Some((origin, v.heading, dd));
                        break 'rows;
                    }
                }
            }
        }
    }
    let Some((origin, heading, moved)) = placed else {
        log::warn!(
            "LAN: no free place found near our spawn at ({:.1}, {:.1}); staying there",
            v.position.x,
            v.position.y
        );
        return None;
    };
    let from = p.vehicle.position;
    p.vehicle.position = origin;
    p.vehicle.heading = heading;
    if let Some(rb) = p.vehicle.rigid.as_mut() {
        rb.place(origin, heading);
    }
    for t in p.vehicle.trailers.iter_mut() {
        t.realign();
    }
    // Put the rear section's axle on the lane so it follows the bend, not the verge.
    if let (Some((li, s, at)), true) = (on_lane, !p.vehicle.trailers.is_empty()) {
        let back = if heading_gap(lanes[li].at(s).1, heading as f32) < 90.0 {
            -1.0
        } else {
            1.0
        };
        let (lead_pos, lead_rot) = (p.vehicle.position, p.vehicle.body_rotation());
        let fwd = DVec3::new(heading.to_radians().sin(), heading.to_radians().cos(), 0.0);
        // Only the first: later sections line up behind it as it moves.
        if let Some(t) = p.vehicle.trailers.first_mut() {
            let c = t.coupling_point(lead_pos, lead_rot);
            let len = t.pivot_length() as f64;
            let mut k = 0.0f32;
            while k < 60.0 {
                if let Some((q, _)) = along(lanes, prev, li, s, at + back * k) {
                    let q = DVec3::new(q.x, q.y, c.z);
                    if (q - c).truncate().length() >= len && (q - c).dot(fwd) < 0.0 {
                        let dir = (q - c).truncate().normalize_or_zero();
                        t.place_pivot(c + DVec3::new(dir.x, dir.y, 0.0) * len);
                        break;
                    }
                }
                k += 0.25;
            }
        }
    }
    for _ in 0..3 {
        p.vehicle.update(1.0 / 30.0);
    }
    log::info!(
        "LAN: {} vehicle(s) stand at our spawn; moved our bus {:.1} m {} to ({:.1}, {:.1}, {:.1}) heading {:.0} (from ({:.1}, {:.1}))",
        occupied
            .iter()
            .filter(|o| (DVec3::new(o.x, o.y, o.z) - from).truncate().length() < 30.0)
            .count(),
        moved.abs(),
        if moved < 0.0 { "back" } else { "forward" },
        origin.x,
        origin.y,
        origin.z,
        heading,
        from.x,
        from.y
    );
    Some(moved)
}

/// With a zero `wait` the frame loop moves the bus once the host answers (see `frame`);
/// offscreen runs have no frames to spare and wait here.
pub fn settle_spawn(
    lan: &mut LanSession,
    game: &mut LanGame,
    p: &mut Player,
    args: &Args,
    world: &World,
    net: Option<&Network>,
    wait: Duration,
) {
    if lan.role != Role::Client || lan.spawn_settled {
        return;
    }
    lan.request_near(footprint_of(&p.vehicle, BUS_BOX));
    if wait.is_zero() {
        return;
    }
    let mine = my_pose(game, Some(p), args, None, 0);
    let t0 = Instant::now();
    while t0.elapsed() < wait && lan.near.is_none() && lan.rejected.is_none() {
        lan.tick(0.02, &mine);
        std::thread::sleep(Duration::from_millis(20));
    }
    match lan.near.clone() {
        Some(near) => {
            log::info!(
                "LAN: the host's list of what stands at our spawn after {:.2} s",
                t0.elapsed().as_secs_f32()
            );
            clear_spawn(p, &near, world, net);
            lan.spawn_settled = true;
        }
        None => log::info!(
            "LAN: the host did not say within {:.0} s what stands at our spawn; the bus is moved when it does (if it has not been driven by then)",
            wait.as_secs_f32()
        ),
    }
}

/// The path comes off the network: only a relative `.bus`/`.ovh` path inside a content
/// root, and a regular file of sane size (never a device such as `/dev/zero`).
fn remote_bus_file(args: &Args, bus: &str) -> Result<PathBuf, String> {
    let rel = ::network::vehicle_path(bus)
        .ok_or_else(|| "not a vehicle file inside a content folder".to_string())?;
    let mut path = ::legacy_config::resolve_path(&args.root, &rel);
    if !::legacy_config::vfs::is_file(&path) {
        // The sender may keep it elsewhere (an archive, its own content folder): look up the
        // `vehicles/...` tail under our content roots.
        let lower = rel.to_ascii_lowercase().replace('\\', "/");
        if let Some(k) = lower.find("vehicles/") {
            if let Some((_, p)) = ::legacy_config::find_in_roots(&rel.replace('\\', "/")[k..]) {
                path = p;
            }
        }
    }
    let mut roots = ::legacy_config::content_roots();
    roots.push(args.root.clone());
    if !roots.iter().any(|r| path.starts_with(r)) {
        return Err("not inside a content folder".into());
    }
    let md = std::fs::metadata(&path).map_err(|e| e.to_string())?;
    if !md.is_file() {
        return Err("not a file".into());
    }
    if md.len() > MAX_VEHICLE_FILE {
        return Err(format!("{} bytes is too much for a vehicle file", md.len()));
    }
    Ok(path)
}

/// The bool is true for a stand-in (ours, or the server's first allowed bus).
fn remote_type(
    args: &Args,
    pose: &Pose,
    player: Option<&Player>,
) -> Option<(Arc<::simulation::VehicleType>, bool)> {
    let allowed = crate::server::SERVER_VEHICLES
        .get()
        .filter(|l| !l.is_empty());
    let norm = |s: &str| s.trim().replace('\\', "/").to_ascii_lowercase();
    let listed = allowed
        .map(|l| {
            l.iter()
                .any(|v| norm(v) == norm(&pose.bus) || norm(&pose.bus).ends_with(&norm(v)))
        })
        .unwrap_or(true);
    let loaded = if listed {
        remote_bus_file(args, &pose.bus).and_then(|path| {
            ::simulation::VehicleType::load(&args.root, &path).map_err(|e| e.to_string())
        })
    } else {
        Err("the server does not offer it".to_string())
    };
    match loaded {
        Ok(t) => Some((Arc::new(t), false)),
        Err(e) => {
            log::warn!(
                "LAN: player {} drives {:?}, which cannot be loaded here ({e}); showing a stand-in",
                pose.id,
                pose.bus
            );
            if let Some(p) = player {
                return Some((p.vehicle.ty.clone(), true));
            }
            let first = allowed.and_then(|l| l.first())?;
            let path = remote_bus_file(args, first).ok()?;
            ::simulation::VehicleType::load(&args.root, &path)
                .ok()
                .map(|t| (Arc::new(t), true))
        }
    }
}

#[allow(clippy::too_many_arguments)]
fn new_remote(
    game: &mut LanGame,
    args: &Args,
    pose: &Pose,
    player: Option<&Player>,
    world: &World,
    r: &Renderer,
    scene: &mut Scene,
    clock: Option<&::simulation::SimClock>,
) -> Option<RemoteVehicle> {
    let (ty, stand_in) = remote_type(args, pose, player)?;
    let mut host =
        ::simulation::VehicleHost::new(clock.cloned().unwrap_or_else(|| crate::start_clock(args)));
    host.font_lib = Some(world.fonts.clone());
    let hof = crate::find_hof(args, world, &ty);
    host.hof = hof.clone();
    let scheme = if pose.paint.is_empty() {
        None
    } else {
        ty.paint_schemes
            .iter()
            .position(|s| s.name.eq_ignore_ascii_case(&pose.paint))
    };
    host.paint_scheme = Some(scheme);
    let mut vehicle = ::simulation::VehicleInstance::new(ty.clone(), host);
    vehicle.ground = None;
    if !ty.model.text_textures.is_empty() {
        vehicle.init_text_textures(&mut world.fonts.lock(), &|p| {
            ::texture::decode_file(p)
                .ok()
                .map(|i| (i.width, i.height, i.rgba))
        });
    }
    vehicle.apply_paint_vars(scheme);
    let render = world.add_vehicle_shared(r, scene, &ty, scheme, None);
    let mut trailer_renders = Vec::new();
    let mut lead = ty.clone();
    let mut lead_rev = false;
    for _ in 0..8 {
        let Some((path, rev)) = crate::spawn::next_coupled(&lead.def, lead_rev, true) else {
            break;
        };
        match ::simulation::VehicleType::load(&args.root, &path) {
            Ok(t) => {
                let t = Arc::new(t);
                trailer_renders.push(world.add_vehicle_shared(
                    r,
                    scene,
                    &t,
                    scheme.filter(|i| *i < t.paint_schemes.len()),
                    Some(&render),
                ));
                vehicle.attach_trailer_ex(t.clone(), rev);
                lead = t;
                lead_rev = rev;
            }
            Err(e) => {
                log::warn!("LAN: rear section {}: {e}", path.display());
                break;
            }
        }
    }
    // After attaching the rear sections: their variables are part of the table.
    let table = sync_table(game, &vehicle);
    vehicle.position = DVec3::new(pose.x, pose.y, pose.z);
    vehicle.heading = pose.heading as f64;
    let rear: Vec<(DVec3, f64)> = pose
        .rear
        .iter()
        .map(|q| (DVec3::new(q.x, q.y, q.z), q.heading as f64))
        .collect();
    let matched = pose.table == table.hash;
    log::info!(
        "LAN: drawing player {} '{}' in {}{} at ({:.1}, {:.1}) heading {:.0}{}; {}",
        pose.id,
        pose.name,
        pose.bus,
        if pose.paint.is_empty() {
            String::new()
        } else {
            format!(" ({})", pose.paint)
        },
        pose.x,
        pose.y,
        pose.heading,
        if trailer_renders.is_empty() {
            String::new()
        } else {
            format!(", {} rear section(s)", trailer_renders.len())
        },
        if matched {
            "lamps, switches and sound values follow theirs".to_string()
        } else {
            format!(
                "their vehicle files differ from ours (table {:08X}, ours {:08X}): lights and doors only",
                pose.table, table.hash
            )
        }
    );
    Some(RemoteVehicle {
        vehicle,
        render,
        trailer_renders,
        name: pose.name.clone(),
        table,
        sounds: Vec::new(),
        inside_sounds: None,
        target: (DVec3::new(pose.x, pose.y, pose.z), pose.heading as f64),
        rear,
        pose_seen: (DVec3::new(pose.x, pose.y, pose.z), Instant::now()),
        doors: pose.doors.clone(),
        suspension: pose.suspension.clone(),
        values: pose.values.clone(),
        odometer: 0.0,
        horn: false,
        hof,
        shown: (String::new(), String::new()),
        stand_in,
        last: pose.clone(),
        driver: None,
        driver_tried: false,
        samples: std::collections::VecDeque::new(),
        offset: None,
        play: Default::default(),
    })
}

fn lan_now() -> f64 {
    static T0: std::sync::OnceLock<Instant> = std::sync::OnceLock::new();
    T0.get_or_init(Instant::now).elapsed().as_secs_f64()
}

const INTERP_DELAY: f64 = 0.12;

impl RemoteVehicle {
    /// Only when both games draw from the same depot file; otherwise the line and
    /// destination are looked up by name.
    fn ibis_pins(&self, pose: &Pose) -> Vec<(VarId, f32)> {
        if self.stand_in || pose.hof == 0 || self.hof.as_deref().map(hof_key) != Some(pose.hof) {
            return Vec::new();
        }
        self.table
            .ibis
            .iter()
            .zip(&pose.ibis)
            .filter_map(|(id, v)| Some(((*id)?, (*v)?)))
            .collect()
    }

    fn take_samples(&mut self, history: &std::collections::VecDeque<(Instant, Pose)>) {
        let now_i = Instant::now();
        let now = lan_now();
        for (at, p) in history {
            if p.sent_ms == 0 {
                continue;
            }
            let sent = p.sent_ms as f64 / 1000.0;
            if self.samples.back().map(|b| sent <= b.0).unwrap_or(false) {
                if self
                    .samples
                    .back()
                    .map(|b| b.0 - sent > 30.0)
                    .unwrap_or(false)
                {
                    self.samples.clear();
                    self.offset = None;
                    self.play = Default::default();
                } else {
                    continue;
                }
            }
            let arrived = now - now_i.saturating_duration_since(*at).as_secs_f64();
            let o = arrived - sent;
            // The fastest delivery sets the offset; it may creep up slowly as clocks drift.
            self.offset = Some(match self.offset {
                Some(off) if o >= off => off + (o - off) * 0.01,
                _ => o,
            });
            self.samples.push_back((sent, p.clone()));
            while self.samples.len() > 40 {
                self.samples.pop_front();
            }
        }
    }

    fn interpolated(&mut self) -> Option<Pose> {
        static OFF: std::sync::OnceLock<bool> = std::sync::OnceLock::new();
        if *OFF.get_or_init(|| ::legacy_config::env::var_os("OMSI_NO_INTERP").is_some()) {
            return None;
        }
        let off = self.offset?;
        let (last_t, last) = self.samples.back()?;
        // Stay two send intervals behind: a parked bus is sent at only 5 Hz, and a fixed
        // 0.12 s ran past the newest state so doors and wheels jumped.
        let n = self.samples.len();
        let gap = if n >= 2 {
            self.samples
                .iter()
                .skip(n.saturating_sub(4))
                .zip(self.samples.iter().skip(n.saturating_sub(4) + 1))
                .map(|(a, b)| b.0 - a.0)
                .fold(0.0f64, f64::max)
        } else {
            0.05
        };
        // Smoothed: recomputed per frame, the playback time stepped back after one slow
        // frame of theirs and the bus jumped.
        let now = lan_now();
        let t = self.play.step(
            now,
            now - off - (gap * 2.0 + 0.02).clamp(INTERP_DELAY, 0.45),
            0.5,
        );
        let k = self.samples.iter().rposition(|(st, _)| *st <= t);
        let Some(k) = k else {
            return self.samples.front().map(|x| x.1.clone());
        };
        if k + 1 >= self.samples.len() {
            let ahead = (t - last_t).clamp(0.0, 0.3);
            let mut p = last.clone();
            let h = (p.heading as f64).to_radians();
            let d = (p.speed_kmh as f64 / 3.6) * ahead;
            p.x += h.sin() * d;
            p.y += h.cos() * d;
            for r in p.rear.iter_mut() {
                r.x += h.sin() * d;
                r.y += h.cos() * d;
            }
            // Extrapolate the walker too: held back, it stalls, jumps each state, drags its feet.
            if let Some(w) = p
                .walker
                .as_mut()
                .filter(|w| w.aboard.is_none() && !w.seated)
            {
                let c = if w.course.is_finite() {
                    w.course
                } else {
                    w.heading
                } as f64;
                let wd = w.speed as f64 * ahead;
                w.x += c.to_radians().sin() * wd;
                w.y += c.to_radians().cos() * wd;
            }
            return Some(p);
        }
        let (ta, a) = &self.samples[k];
        let (tb, b) = &self.samples[k + 1];
        let f = if tb > ta {
            ((t - ta) / (tb - ta)).clamp(0.0, 1.0)
        } else {
            1.0
        };
        if ((b.x - a.x).powi(2) + (b.y - a.y).powi(2)).sqrt() > 40.0 {
            return Some(if f < 0.5 { a.clone() } else { b.clone() });
        }
        let l = |x: f64, y: f64| x + (y - x) * f;
        let lf = |x: f32, y: f32| x + (y - x) * f as f32;
        let mut p = b.clone();
        p.x = l(a.x, b.x);
        p.y = l(a.y, b.y);
        p.z = l(a.z, b.z);
        p.heading = lerp_angle(a.heading as f64, b.heading as f64, f) as f32;
        p.pitch = lf(a.pitch, b.pitch);
        p.bank = lf(a.bank, b.bank);
        p.speed_kmh = lf(a.speed_kmh, b.speed_kmh);
        p.steer_deg = lf(a.steer_deg, b.steer_deg);
        p.rpm = lf(a.rpm, b.rpm);
        let mix = |x: &[f32], y: &[f32]| -> Vec<f32> {
            if x.len() == y.len() {
                x.iter()
                    .zip(y)
                    .map(|(u, v)| u + (v - u) * f as f32)
                    .collect()
            } else {
                y.to_vec()
            }
        };
        p.doors = mix(&a.doors, &b.doors);
        p.suspension = mix(&a.suspension, &b.suspension);
        p.values = mix(&a.values, &b.values);
        if a.rear.len() == b.rear.len() {
            for (r, (ra, rb)) in p.rear.iter_mut().zip(a.rear.iter().zip(&b.rear)) {
                r.x = l(ra.x, rb.x);
                r.y = l(ra.y, rb.y);
                r.z = l(ra.z, rb.z);
                r.heading = lerp_angle(ra.heading as f64, rb.heading as f64, f) as f32;
            }
        }
        if let (Some(wa), Some(wb)) = (a.walker, p.walker.as_mut()) {
            wb.x = l(wa.x, wb.x);
            wb.y = l(wa.y, wb.y);
            wb.z = l(wa.z, wb.z);
            wb.heading = lerp_angle(wa.heading as f64, wb.heading as f64, f) as f32;
            if wa.course.is_finite() && wb.course.is_finite() {
                wb.course = lerp_angle(wa.course as f64, wb.course as f64, f) as f32;
            }
            wb.speed = lf(wa.speed, wb.speed);
            if let (Some(aa), Some(ab)) = (wa.aboard, wb.aboard.as_mut()) {
                if aa.owner == ab.owner {
                    for i in 0..3 {
                        ab.local[i] = lf(aa.local[i], ab.local[i]);
                    }
                }
            }
        }
        Some(p)
    }
}

fn lerp_angle(a: f64, b: f64, t: f64) -> f64 {
    let d = (b - a + 540.0).rem_euclid(360.0) - 180.0;
    (a + d * t).rem_euclid(360.0)
}

fn release(
    r: &Renderer,
    scene: &mut Scene,
    world: &World,
    audio: Option<&::audio::AudioEngine>,
    rv: RemoteVehicle,
) {
    if let Some(a) = audio {
        for mut s in rv.sounds {
            s.stop_all(a);
        }
    }
    if let Some(mut d) = rv.driver {
        d.hide(r, scene);
    }
    world.release_vehicle(r, scene, rv.render);
    for t in rv.trailer_renders {
        world.release_vehicle(r, scene, t);
    }
}

fn ease_heading(from: f64, to: f64, k: f64) -> f64 {
    let dh = (to - from + 540.0).rem_euclid(360.0) - 180.0;
    if dh.abs() > 90.0 { to } else { from + dh * k }
}

fn glide(cur: &mut Vec<f32>, want: &[f32], k: f32) {
    if cur.len() != want.len() {
        *cur = want.to_vec();
        return;
    }
    for (c, w) in cur.iter_mut().zip(want) {
        *c += (w - *c) * k;
    }
}

fn drive_remote(rv: &mut RemoteVehicle, pose: &Pose, dt: f32, exact: bool) {
    rv.name = pose.name.clone();
    rv.vehicle.pitch = pose.pitch;
    rv.vehicle.bank = pose.bank;
    rv.odometer += pose.speed_kmh / 3.6 * dt;
    if exact {
        rv.vehicle.position = DVec3::new(pose.x, pose.y, pose.z);
        rv.vehicle.heading = pose.heading as f64;
        rv.target = (rv.vehicle.position, rv.vehicle.heading);
        rv.pose_seen = (rv.vehicle.position, Instant::now());
        rv.rear = pose
            .rear
            .iter()
            .map(|q| (DVec3::new(q.x, q.y, q.z), q.heading as f64))
            .collect();
        for (i, cur) in rv.rear.iter().enumerate() {
            if let Some(t) = rv.vehicle.trailers.get_mut(i) {
                t.set_pose(cur.0, cur.1);
            }
        }
        rv.doors = pose.doors.clone();
        rv.suspension = pose.suspension.clone();
        rv.values = pose.values.clone();
    } else {
        // Older games send no timestamps: extrapolate by the pose's age plus ~40 ms latency.
        let at = DVec3::new(pose.x, pose.y, pose.z);
        if at != rv.pose_seen.0 {
            rv.pose_seen = (at, Instant::now());
        }
        let age = (rv.pose_seen.1.elapsed().as_secs_f64() + 0.04).min(0.4);
        let h = (pose.heading as f64).to_radians();
        let ahead = DVec3::new(h.sin(), h.cos(), 0.0) * (pose.speed_kmh as f64 / 3.6) * age;
        rv.target = (at + ahead, pose.heading as f64);
        let k = (dt * 20.0).min(1.0);
        let kd = k as f64;
        let d = rv.target.0 - rv.vehicle.position;
        rv.vehicle.position += if d.length() > 25.0 { d } else { d * kd };
        rv.vehicle.heading = ease_heading(rv.vehicle.heading, rv.target.1, kd);
        rv.rear.resize(pose.rear.len(), (DVec3::ZERO, 0.0));
        for (i, (cur, q)) in rv.rear.iter_mut().zip(pose.rear.iter()).enumerate() {
            let tgt = (DVec3::new(q.x, q.y, q.z) + ahead, q.heading as f64);
            let jump = cur.0 == DVec3::ZERO || (tgt.0 - cur.0).length() > 25.0;
            cur.0 = if jump {
                tgt.0
            } else {
                cur.0 + (tgt.0 - cur.0) * kd
            };
            cur.1 = if jump {
                tgt.1
            } else {
                ease_heading(cur.1, tgt.1, kd)
            };
            if let Some(t) = rv.vehicle.trailers.get_mut(i) {
                t.set_pose(cur.0, cur.1);
            }
        }
        glide(&mut rv.doors, &pose.doors, k);
        glide(&mut rv.suspension, &pose.suspension, k);
        glide(&mut rv.values, &pose.values, k);
    }
    let mut it = rv.suspension.iter();
    for axle in rv.vehicle.physics.wheels.iter_mut() {
        for w in axle.iter_mut() {
            w.suspension = it.next().copied().unwrap_or(0.0);
        }
    }
    // Trigger rather than pin: the scripts fire the horn's sound events.
    let horn = pose.flags & ::network::FLAG_HORN != 0;
    if horn != rv.horn {
        rv.horn = horn;
        rv.vehicle.trigger(if horn { "horn" } else { "horn_off" });
    }
    let t = rv.table.clone();
    let matched = pose.table == t.hash && !rv.stand_in;
    let engine = pose.flags & ::network::FLAG_ENGINE != 0;
    let electrics = pose.flags & ::network::FLAG_ELECTRICS != 0;
    // AI light levels: 0.5 parking, 1 dipped, 2 main beam.
    let mut inputs: Vec<(VarId, f32)> = Vec::with_capacity(8);
    let active = if engine || (electrics && t.engine_n.is_some()) {
        1.0
    } else {
        -1.0
    };
    inputs.extend(t.ai_engine.map(|id| (id, active)));
    inputs.extend(
        t.ai_light
            .map(|id| (id, [0.0, 0.5, 1.0, 2.0][pose.head.min(3) as usize])),
    );
    inputs.extend(
        t.ai_interior
            .map(|id| (id, (pose.interior > 0) as i32 as f32)),
    );
    inputs.extend(t.throttle.map(|id| (id, pose.throttle)));
    inputs.extend(t.brake.map(|id| (id, pose.brake)));
    let mut pinned: Vec<(VarId, f32)> =
        Vec::with_capacity(8 + t.lamps.len() + t.switches.len() + t.values.len());
    if !t.values.iter().any(|v| Some(v.1) == t.engine_n) {
        pinned.extend(t.engine_n.map(|id| (id, pose.rpm)));
    }
    pinned.extend(t.doors.iter().zip(&rv.doors).map(|(id, v)| (*id, *v)));
    pinned.extend(rv.ibis_pins(pose));
    if matched
        && pose.lamps.len() == t.lamps.len()
        && pose.switches.len() == t.switches.len()
        && rv.values.len() == t.values.len()
    {
        pinned.extend(
            t.lamps
                .iter()
                .zip(&pose.lamps)
                .map(|((_, id), v)| (*id, *v)),
        );
        pinned.extend(
            t.switches
                .iter()
                .zip(&pose.switches)
                .map(|((_, id), v)| (*id, *v)),
        );
        pinned.extend(
            t.values
                .iter()
                .zip(&rv.values)
                .map(|((_, id), v)| (*id, *v)),
        );
    }
    let doors_open = rv.doors.iter().any(|d| *d > 0.2);
    let frame = ::simulation::vehicle::AiFrame {
        speed: pose.speed_kmh / 3.6,
        odometer: rv.odometer,
        steer_deg: pose.steer_deg,
        blinker: pose.blinker as i32,
        brake: pose.flags & ::network::FLAG_BRAKE != 0 || pose.brake > 0.1,
        lights: pose.head >= 2,
        at_station: if doors_open { 1 } else { -1 },
        // Their stop side isn't sent: the doors are pinned to their values, and only the AI
        // half of the script runs.
        at_station_side: 0.0,
        priority_warning: false,
    };
    rv.vehicle.update_ai_with(dt, &frame, &inputs, &pinned);
    rv.last = pose.clone();
}

fn sound_remote(
    rv: &mut RemoteVehicle,
    audio: Option<&::audio::AudioEngine>,
    listener: Option<DVec3>,
    muffled: bool,
    inside: bool,
) {
    let fired: Vec<::simulation::host::FiredSound> =
        std::mem::take(&mut rv.vehicle.host.fired_sounds);
    let (Some(audio), Some(at)) = (audio, listener) else {
        return;
    };
    let events = crate::sound_events::events_from(::audio::EventSource::Lan, &fired, &[]);
    // A rider hears the bus's interior `[sound]`, not its exterior sounds muffled.
    let interior = inside.then_some(()).and(rv.table.interior.clone());
    if let Some((cfg, dir)) = interior {
        for mut s in rv.sounds.drain(..) {
            s.stop_all(audio);
        }
        if rv.inside_sounds.is_none()
            && audio.clips_ready(&::audio::SoundSet::clip_paths(&cfg, &dir))
        {
            let number = rv.vehicle.number();
            rv.inside_sounds = Some(::audio::SoundSet::new(
                audio,
                &cfg.chosen_for(&number),
                &dir,
            ));
        }
        if let Some(ss) = rv.inside_sounds.as_mut() {
            let xf = rv.vehicle.world_transform();
            let v = &rv.vehicle;
            ss.set_inside(true);
            ss.set_muffled(true);
            ss.set_listener_vehicle(true);
            ss.update_events(audio, &|n| v.var(n), &xf, &events, &|n| v.var_slot(n));
        }
        return;
    }
    if let Some(mut s) = rv.inside_sounds.take() {
        s.stop_all(audio);
    }
    let d = (rv.vehicle.position - at).length();
    if d > HEAR_RANGE * 1.2 {
        for mut s in rv.sounds.drain(..) {
            s.stop_all(audio);
        }
        return;
    }
    if rv.sounds.is_empty() && d < HEAR_RANGE && !rv.table.sounds.is_empty() {
        let ready = rv
            .table
            .sounds
            .iter()
            .map(|(cfg, dir)| (cfg, dir))
            .chain(rv.table.part_sounds.iter().map(|(_, cfg, dir)| (cfg, dir)))
            .all(|(cfg, dir)| audio.clips_ready(&::audio::SoundSet::clip_paths(cfg, dir)));
        if ready {
            let number = rv.vehicle.number();
            rv.sounds = rv
                .table
                .sounds
                .iter()
                .map(|(cfg, dir)| {
                    ::audio::SoundSet::new_exterior(audio, &cfg.chosen_for(&number), dir)
                })
                .collect();
            if let Some(first) = rv.sounds.first_mut() {
                for (k, cfg, dir) in &rv.table.part_sounds {
                    first.add_part(
                        *k,
                        ::audio::SoundSet::new_exterior(audio, &cfg.chosen_for(&number), dir),
                    );
                }
            }
        }
    }
    let xf = rv.vehicle.world_transform();
    let v = &rv.vehicle;
    for ss in rv.sounds.iter_mut() {
        ss.set_muffled(muffled);
        ss.update_events(audio, &|n| v.var(n), &xf, &events, &|n| v.var_slot(n));
        ss.update_parts_events(
            audio,
            &|n| v.var(n),
            &|i| v.trailers.get(i).map(|t| t.world_transform()),
            &events,
            &|n| v.var_slot(n),
        );
    }
}

#[allow(clippy::too_many_arguments)]
pub fn tick(
    lan: &mut LanSession,
    game: &mut LanGame,
    dt: f32,
    args: &Args,
    mut player: Option<&mut Player>,
    world: Option<&World>,
    renderer: Option<&Renderer>,
    mut scene: Option<&mut Scene>,
    mut traffic: Option<&mut crate::traffic::Traffic>,
    mut humans: Option<&mut crate::humans::Humans>,
    duty: Option<(&str, &str)>,
    frame: &Frame,
) -> Vec<WorldUpdate> {
    let mut updates = Vec::new();
    let mut mine = my_pose(game, player.as_deref(), args, duty, frame.riders);
    mine.tour = frame.tour.clone().unwrap_or_default();
    mine.walker = frame.walker;
    if lan.role == Role::Host {
        // tours other players drive are theirs, not the timetable's
        let tours: hashbrown::HashSet<(String, String)> = lan
            .peers()
            .filter_map(|p| p.pose.tour.split_once('/'))
            .map(|(l, t)| (l.trim().to_lowercase(), t.trim().to_lowercase()))
            .filter(|(l, t)| !l.is_empty() && !t.is_empty())
            .collect();
        if tours != game.tours {
            game.tours = tours.clone();
            updates.push(WorldUpdate::Tours(tours));
        }
        if let Some(c) = frame.clock {
            lan.set_clock(&date_of(c), c.time);
        }
        if !lan.pending_joins().is_empty() {
            lan.set_local_footprints(host_footprints(player.as_deref(), traffic.as_deref()));
            lan.answer_joins();
        }
    }
    let gone = lan.tick(dt, &mine);
    game.world.tick(
        lan,
        dt,
        args,
        world,
        renderer,
        scene.as_deref_mut(),
        traffic.as_deref_mut(),
        humans.as_deref_mut(),
        player.as_deref().map(|p| p.vehicle.position),
    );
    if let (Role::Client, Some(clock)) = (lan.role, frame.clock) {
        if lan.welcomes != game.adopted && lan.welcome.is_some() {
            updates.extend(adopt_at_runtime(args, lan, game, world, clock));
            game.slew = 0.0;
            lan.take_host_clock();
        } else if let Some(h) = lan.take_host_clock() {
            let norm = |s: &str| s.trim().replace('\\', "/").to_ascii_lowercase();
            if !h.world.weather.is_empty()
                && norm(&h.world.weather) != norm(args.weather.as_deref().unwrap_or(""))
                && game.weather_seen.as_deref() != Some(h.world.weather.as_str())
            {
                game.weather_seen = Some(h.world.weather.clone());
                if let Ok(wt) = host_weather(args, &h.world.weather) {
                    log::info!("LAN: the host's weather is now {}", h.world.weather);
                    updates.push(WorldUpdate::Weather(wt));
                }
            }
            if let Some(hc) = host_clock_now(&h) {
                let gap = clock_gap(&hc, clock);
                game.last_gap = Some(gap);
                if gap.abs() > CLOCK_JUMP {
                    log::info!(
                        "LAN: our clock is {gap:+.1} s off the host's; set to {} {:02}:{:02}:{:02}",
                        date_of(&hc),
                        (hc.time / 3600.0) as i32,
                        ((hc.time % 3600.0) / 60.0) as i32,
                        (hc.time % 60.0) as i32
                    );
                    updates.push(WorldUpdate::Clock {
                        year: hc.year,
                        day_of_year: hc.day_of_year,
                        time: hc.time,
                    });
                    game.slew = 0.0;
                } else {
                    game.slew = gap;
                }
            }
        }
        if game.slew.abs() > 1.0e-3 {
            let step = game.slew * (dt as f64 * 0.5).min(1.0);
            game.slew -= step;
            updates.push(WorldUpdate::Slew(step));
        }
    }
    if lan.role == Role::Client && !lan.spawn_settled {
        if let (Some(near), Some(p), Some(world)) = (lan.near.clone(), player.as_deref_mut(), world)
        {
            if p.vehicle.physics.velocity_kmh().abs() < 1.0 {
                clear_spawn(p, &near, world, traffic.map(|t| &t.net));
            } else {
                log::info!(
                    "LAN: the host's list came after we drove off; our bus stays where it is"
                );
            }
            lan.spawn_settled = true;
        }
    }
    // OMSI_LAN_SAY="30=Hallo;45=Tschüss": chat lines said at these session seconds (offscreen)
    game.clock += dt;
    if let Ok(script) = ::legacy_config::env::var("OMSI_LAN_SAY") {
        for item in script.split(';') {
            if let Some((at, text)) = item.split_once('=') {
                if at
                    .trim()
                    .parse::<f32>()
                    .map(|t| t <= game.clock && t > game.clock - dt)
                    .unwrap_or(false)
                {
                    chat_send(lan, game, text);
                }
            }
        }
    }
    for e in lan.take_events() {
        game.chat.push(match e {
            LanEvent::Chat { name, text, .. } => {
                format!("{name}: {}", crate::ui::filter_chat(&text))
            }
            LanEvent::Notice(n) => format!("* {n}"),
        });
    }
    game.status_t -= dt;
    if game.status_t <= 0.0 {
        write_status(lan, game, player.as_deref());
        game.status_t = 2.0;
    }
    let (Some(w), Some(r), Some(scene)) = (world, renderer, scene) else {
        return updates;
    };
    for id in gone {
        if let Some(rv) = game.remotes.remove(&id) {
            log::info!("LAN: no longer drawing player {id} '{}'", rv.name);
            release(r, scene, w, frame.audio, rv);
        }
    }
    let known: Vec<u32> = lan
        .peers()
        .filter(|p| !(p.has_pose && !p.pose.has_vehicle()))
        .map(|p| p.pose.id)
        .collect();
    let stale: Vec<u32> = game
        .remotes
        .keys()
        .copied()
        .filter(|id| !known.contains(id))
        .collect();
    for id in stale {
        if let Some(rv) = game.remotes.remove(&id) {
            release(r, scene, w, frame.audio, rv);
        }
    }
    let poses: Vec<Pose> = lan
        .peers()
        .filter(|p| p.has_pose && p.has_info && p.pose.has_vehicle())
        .map(|p| p.pose.clone())
        .collect();
    for pose in poses {
        if game
            .remotes
            .get(&pose.id)
            .map(|rv| rv.last.bus != pose.bus)
            .unwrap_or(false)
        {
            if let Some(rv) = game.remotes.remove(&pose.id) {
                release(r, scene, w, frame.audio, rv);
            }
        }
        if !game.remotes.contains_key(&pose.id) {
            let key = (pose.id, pose.bus.clone());
            if game
                .failed
                .get(&key)
                .is_some_and(|t| t.elapsed().as_secs_f32() < 30.0)
            {
                continue;
            }
            let Some(rv) = new_remote(
                game,
                args,
                &pose,
                player.as_deref(),
                w,
                r,
                scene,
                frame.clock,
            ) else {
                game.failed.insert(key, std::time::Instant::now());
                continue;
            };
            game.failed.remove(&key);
            game.remotes.insert(pose.id, rv);
        }
        let Some(rv) = game.remotes.get_mut(&pose.id) else {
            continue;
        };
        // not when IBIS values are pinned in `drive_remote`: the AI trigger would overwrite them
        let want = (pose.line.clone(), pose.destination.clone());
        if want != rv.shown && !want.1.is_empty() && rv.ibis_pins(&pose).is_empty() {
            if let Some(i) = rv.vehicle.ty.program.str_var("Linie") {
                rv.vehicle.state.str_vars[i as usize] = want.0.clone();
            }
            crate::schedule::set_ai_destination(
                &mut rv.vehicle,
                rv.hof.as_deref(),
                &want.0,
                &want.1,
                &[],
            );
            log::info!(
                "LAN: player {} '{}' shows {}{}",
                pose.id,
                pose.name,
                if want.0.is_empty() {
                    String::new()
                } else {
                    format!("line {} ", want.0)
                },
                want.1
            );
            rv.shown = want;
        }
        // older games send no clock: glide towards the newest pose
        if let Some(peer) = lan.peers().find(|p| p.pose.id == pose.id) {
            rv.take_samples(&peer.history);
        }
        match rv.interpolated() {
            Some(mut ip) => {
                ip.name = pose.name.clone();
                ip.bus = pose.bus.clone();
                ip.table = pose.table;
                ip.line = pose.line.clone();
                ip.destination = pose.destination.clone();
                ip.texts = pose.texts.clone();
                ip.freetex = pose.freetex.clone();
                ip.hof = pose.hof;
                ip.ibis = pose.ibis.clone();
                drive_remote(rv, &ip, dt, true);
            }
            None => drive_remote(rv, &pose, dt, false),
        }
        show_display_texts(&mut rv.vehicle, &pose.texts);
        show_freetex(&mut rv.vehicle, &pose.freetex);
        let inside = frame.inside_of == Some(pose.id);
        sound_remote(rv, frame.audio, frame.listener, frame.muffled, inside);
    }
    for (id, rv) in game.remotes.iter_mut() {
        if !rv.driver_tried && !rv.stand_in {
            rv.driver_tried = true;
            rv.driver = crate::driver::DriverFigure::new_named(
                w,
                r,
                scene,
                &rv.vehicle,
                &rv.last.figure,
                1000 + *id as u64,
            );
        }
        if let Some(d) = rv.driver.as_mut() {
            d.update(
                r,
                scene,
                &rv.vehicle,
                &rv.render,
                dt.max(1.0 / 120.0),
                rv.last.walker.is_none(),
                false,
            );
        }
        // drawn like our own bus, not as AI: outside and AI meshes together fought over the
        // same surfaces (flicker, black patches in the saloon)
        let inside = frame.inside_of == Some(*id);
        // OMSI_TRACE_REMOTE=<file.csv>: logs where each remote bus is drawn, every frame
        if let Ok(path) = ::legacy_config::env::var("OMSI_TRACE_REMOTE") {
            use std::io::Write;
            if let Ok(mut f) = std::fs::OpenOptions::new()
                .create(true)
                .append(true)
                .open(&path)
            {
                let _ = writeln!(
                    f,
                    "{:.4},{id},{:.3},{:.3},{:.3},{:.2},{}",
                    lan_now(),
                    rv.vehicle.position.x,
                    rv.vehicle.position.y,
                    rv.vehicle.position.z,
                    rv.vehicle.heading,
                    rv.offset.is_some()
                );
            }
        }
        crate::player::sync_vehicle_transforms(
            r,
            scene,
            &mut rv.vehicle,
            &mut rv.render,
            &mut rv.trailer_renders,
            inside,
        );
    }
    debug_log(lan, game, dt, frame);
    updates
}

/// `OMSI_DEBUG_LAN`: logs every player's state, sounds and network traffic every 4 s.
fn debug_log(lan: &LanSession, game: &mut LanGame, dt: f32, frame: &Frame) {
    if ::legacy_config::env::var_os("OMSI_DEBUG_LAN").is_none() {
        return;
    }
    game.log_t -= dt;
    if game.log_t > 0.0 {
        return;
    }
    game.log_t = 4.0;
    let (sent, received) = (lan.sent(), lan.received);
    log::info!(
        "LAN traffic: {:.0} B/s out, {:.0} B/s in{}",
        (sent - game.last_sent) as f32 / 4.0,
        (received - game.last_received) as f32 / 4.0,
        game.last_gap
            .take()
            .map(|g| format!(
                "; our clock was {g:+.2} s off the host's, {:+.2} s still to catch up",
                game.slew
            ))
            .unwrap_or_default()
    );
    game.last_sent = sent;
    game.last_received = received;
    for peer in lan.peers().filter(|p| p.has_pose) {
        let q = &peer.pose;
        log::info!(
            "LAN state of player {} '{}': {} paint '{}' at ({:.2}, {:.2}, {:.3}) pitch {:.2} bank {:.2} wheels {:?} {:.1} km/h steer {:.1} flags {:010b} head {} interior {} blinker {} rpm {:.0} throttle {:.2} brake {:.2} doors {:?} passengers {} line '{}' destination '{}' {} lamps lit of {}, {} rear section(s), table {:08X}",
            q.id,
            q.name,
            q.bus,
            q.paint,
            q.x,
            q.y,
            q.z,
            q.pitch,
            q.bank,
            q.suspension
                .iter()
                .map(|s| (s * 1000.0).round() / 1000.0)
                .collect::<Vec<_>>(),
            q.speed_kmh,
            q.steer_deg,
            q.flags,
            q.head,
            q.interior,
            q.blinker,
            q.rpm,
            q.throttle,
            q.brake,
            q.doors
                .iter()
                .map(|d| (d * 10.0).round() / 10.0)
                .collect::<Vec<_>>(),
            q.passengers,
            q.line,
            q.destination,
            q.lamps.iter().filter(|l| **l > 0.5).count(),
            q.lamps.len(),
            q.rear.len(),
            q.table
        );
        if let Some(rv) = game.remotes.get(&q.id) {
            let v = &rv.vehicle;
            let lit: Vec<&str> = rv
                .table
                .lamps
                .iter()
                .filter(|(n, id)| {
                    !n.starts_with("cockpit")
                        && !n.starts_with("cp_")
                        && v.state.vars.get(*id as usize).copied().unwrap_or(0.0) > 0.5
                })
                .map(|(n, _)| n.as_str())
                .take(16)
                .collect();
            let steer = v.var("Axle_Steering_0_L").unwrap_or(0.0).to_degrees();
            let values: Vec<String> = rv
                .table
                .values
                .iter()
                .map(|(n, id)| {
                    format!(
                        "{n}={:.2}",
                        v.state.vars.get(*id as usize).copied().unwrap_or(0.0)
                    )
                })
                .collect();
            log::info!(
                "LAN copy of player {}: {} lamps lit {:?}; values {}; door_0 {:.2} engine_n {:.0} wheel {:.0} deg steer {:.1} deg AI_Light {:?} lights_sw_blinker {:?}",
                q.id,
                rv.table
                    .lamps
                    .iter()
                    .filter(|(_, id)| v.state.vars.get(*id as usize).copied().unwrap_or(0.0) > 0.5)
                    .count(),
                lit,
                values.join(" "),
                rv.table
                    .doors
                    .first()
                    .and_then(|id| v.state.vars.get(*id as usize))
                    .copied()
                    .unwrap_or(0.0),
                v.var("engine_n").unwrap_or(0.0),
                v.var("Wheel_Rotation_1_L")
                    .unwrap_or(0.0)
                    .to_degrees()
                    .rem_euclid(360.0),
                steer,
                v.var("AI_Light"),
                v.var("lights_sw_blinker")
            );
            if let Some(a) = frame.audio {
                let playing: Vec<String> = rv
                    .sounds
                    .iter()
                    .flat_map(|s| s.playing(a))
                    .filter(|p| p.1 > 1.0e-4)
                    .map(|(f, g, pitch)| {
                        format!(
                            "{} {:.3}x{:.2}",
                            f.rsplit(['\\', '/']).next().unwrap_or(&f),
                            g,
                            pitch
                        )
                    })
                    .take(10)
                    .collect();
                log::info!(
                    "LAN sounds of player {} ({} set(s)): {:?}",
                    q.id,
                    rv.sounds.len(),
                    playing
                );
            }
        }
    }
}

/// Returns whether the chat took the key; an open chat line takes every key.
pub fn chat_key(
    lan: &mut LanSession,
    game: &mut LanGame,
    code: KeyCode,
    pressed: bool,
    repeat: bool,
    bound: Option<&str>,
) -> bool {
    let chat = &mut game.chat;
    if chat.disabled {
        return false;
    }
    if chat.typing.is_none() {
        let toggle = bound.is_some_and(|a| a.eq_ignore_ascii_case("chat_toggle"));
        let open = bound.is_some_and(|a| a.eq_ignore_ascii_case("chat_open"));
        if pressed && !repeat && (toggle || open) {
            if toggle {
                chat.hidden = !chat.hidden;
            } else {
                chat.open();
            }
            chat.swallow.insert(code);
            return true;
        }
        return !pressed && chat.swallow.remove(&code);
    }
    if !pressed {
        return chat.swallow.remove(&code);
    }
    chat.swallow.insert(code);
    match code {
        KeyCode::Enter | KeyCode::NumpadEnter => {
            let text = chat.typing.take().unwrap_or_default();
            chat_send(lan, game, &text);
        }
        KeyCode::Escape => chat.typing = None,
        KeyCode::Backspace => {
            if let Some(t) = chat.typing.as_mut() {
                t.pop();
            }
        }
        _ => {}
    }
    true
}

pub fn chat_swallow(game: &mut LanGame, code: KeyCode) {
    game.chat.swallow.insert(code);
}

pub fn chat_type(game: &mut LanGame, text: &str) {
    if let Some(t) = game.chat.typing.as_mut() {
        for c in text.chars().filter(|c| !c.is_control()) {
            if t.chars().count() < ::network::MAX_CHAT {
                t.push(c);
            }
        }
    }
}

pub fn chat_open(game: &LanGame) -> bool {
    game.chat.typing.is_some()
}

pub fn chat_send(lan: &mut LanSession, game: &mut LanGame, text: &str) {
    if text.trim().is_empty() {
        return;
    }
    if let Some(pw) = text.trim().strip_prefix("/admin ") {
        crate::admin::request(lan, pw.trim());
        game.chat
            .push("* asked the server for its administration".into());
        return;
    }
    let text = crate::ui::filter_chat(text.trim());
    match lan.say(&text) {
        Ok(()) => game.chat.error = None,
        Err(e) => {
            log::info!("LAN chat not sent: {e}");
            game.chat.error = Some((e, Instant::now()));
        }
    }
}

pub fn hud_lines(lan: &LanSession, _game: &LanGame, _player: Option<&Player>) -> Vec<String> {
    // Kept to one line: names are drawn above the buses (`name_tags`), talk is in the chat.
    let mut lines = Vec::new();
    let players = lan.peer_count();
    let others = match players {
        0 => "nobody else yet".to_string(),
        1 => "1 other player".to_string(),
        n => format!("{n} other players"),
    };
    match lan.role {
        Role::Host => {
            let c = lan.code();
            lines.push(format!(
                "{}   {}",
                ::user_interface::tr(&format!("Online: {others}")),
                c.as_ref().map(|c| c.encode()).unwrap_or_default(),
            ));
        }
        Role::Client => {
            if let Some(why) = lan.rejected.as_ref() {
                lines.push(format!("Online: not connected: {why}"));
            } else if lan.connected {
                let name = lan
                    .welcome
                    .as_ref()
                    .map(|w| w.host_name.clone())
                    .unwrap_or_default();
                lines.push(format!("Online: in {name}'s game, {others}"));
            } else {
                lines.push("Online: connecting ...".to_string());
            }
        }
    }
    if let Some(w) = lan.warnings.first() {
        lines.push(format!("Online: {w}"));
    }
    if ::legacy_config::env::var_os("OMSI_DEBUG_LAN").is_some() {
        HUD_LOG.with(|t| {
            let now = Instant::now();
            if t.get()
                .map(|last| now.duration_since(last) > Duration::from_secs(4))
                .unwrap_or(true)
            {
                t.set(Some(now));
                log::info!("LAN HUD:\n    {}", lines.join("\n    "));
            }
        });
    }
    lines
}

thread_local! {
    static HUD_LOG: std::cell::Cell<Option<Instant>> = const { std::cell::Cell::new(None) };
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Visual values outrank sound ones in the capped list: the Agora L's sounds crowded out
    /// its roller blind's scroll.
    #[test]
    fn the_roller_blind_scroll_is_in_the_sync_table() {
        let root = ::legacy_config::env::var_os("OMSI_ROOT")
            .map(PathBuf::from)
            .unwrap_or_else(|| PathBuf::from("../../../OMSI 2 Original"));
        let bus = root.join("Vehicles/AA-FR_BusBundle/2002_Agora_L_4d_main.bus");
        if !bus.exists() {
            eprintln!("skipped: no {}", bus.display());
            return;
        }
        let ty = ::simulation::VehicleType::load(&root, &bus).expect("Agora L");
        let t = SyncTable::new(&ty, &[]);
        assert!(t.values.len() <= ::network::wire::MAX_VALUES);
        for want in ["Rollband_Linie_Trans", "Rollband_Linie_Trans_2"] {
            assert!(
                t.values.iter().any(|v| v.0.eq_ignore_ascii_case(want)),
                "no {want}: {}",
                t.describe()
            );
        }
    }

    #[test]
    fn the_rear_section_is_in_the_sync_table() {
        let root = ::legacy_config::env::var_os("OMSI_ROOT")
            .map(PathBuf::from)
            .unwrap_or_else(|| PathBuf::from("../../../OMSI 2 Original"));
        let bus = root.join("Vehicles/AA-FR_BusBundle/2002_Agora_L_3d_main.bus");
        let trail = root.join("Vehicles/AA-FR_BusBundle/2002_Agora_L_3d_trail.bus");
        if !bus.exists() || !trail.exists() {
            eprintln!("skipped: no {}", bus.display());
            return;
        }
        let ty = ::simulation::VehicleType::load(&root, &bus).expect("Agora L");
        let part = Arc::new(::simulation::VehicleType::load(&root, &trail).expect("Agora L trail"));
        let alone = SyncTable::new(&ty, &[]);
        let whole = SyncTable::new(&ty, &[part]);
        let count = |t: &SyncTable| t.lamps.len() + t.switches.len();
        assert!(
            count(&whole) > count(&alone),
            "alone {}, whole {}",
            alone.describe(),
            whole.describe()
        );
        assert!(
            !whole.part_sounds.is_empty(),
            "no sounds for the rear section: {}",
            whole.describe()
        );
        assert_ne!(whole.hash, alone.hash);
    }

    #[test]
    fn day_numbers_and_clock_gaps() {
        assert_eq!(day_number(1990, 1) - day_number(1989, 365), 1);
        assert_eq!(day_number(1989, 1) - day_number(1988, 366), 1);
        let a = ::simulation::SimClock {
            year: 1990,
            day_of_year: 1,
            time: 10.0,
            ..Default::default()
        };
        let b = ::simulation::SimClock {
            year: 1989,
            day_of_year: 365,
            time: 86390.0,
            ..Default::default()
        };
        assert!((clock_gap(&a, &b) - 20.0).abs() < 1e-9);
        assert!((clock_gap(&b, &a) + 20.0).abs() < 1e-9);
        assert_eq!(parse_date("1989-05-30"), Some((1989, 150)));
        assert_eq!(parse_date("x"), None);
    }

    #[test]
    fn the_hosts_clock_moves_on_past_midnight() {
        let h = ::network::HostClock {
            world: ::network::WorldInfo {
                date: "1989-12-31".into(),
                time: 86399.5,
                ..Default::default()
            },
            at: Instant::now() - Duration::from_secs(2),
            speed: 1.0,
        };
        let c = host_clock_now(&h).unwrap();
        assert_eq!((c.year, c.day_of_year), (1990, 1));
        assert!((c.time - 1.5).abs() < 0.1, "{}", c.time);
    }

    #[test]
    fn engine_fed_names() {
        for n in [
            "Wheel_RotationSpeed_1_R",
            "Velocity",
            "AI_Light",
            "door_0",
            "StreetCond",
            "Timegap",
        ] {
            assert!(engine_fed(n), "{n}");
        }
        for n in [
            "engine_n",
            "engine_throttle_injection",
            "M_Wheel",
            "doorSpeed_0",
            "wiperpos",
            "cockpit_hupe_volume",
            "lights_stand",
        ] {
            assert!(!engine_fed(n), "{n}");
        }
    }
}
