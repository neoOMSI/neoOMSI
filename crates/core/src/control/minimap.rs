use crate::navigator::{confirm_road_surfaces, road_geometry, simplify};
use crate::scene::World;
use anyhow::{Context, Result};
use glam::{DVec3, Vec3};
use omsi_launcher_lib as lib;
use serde_json::{Value, json};
use std::collections::{HashMap, HashSet};
use std::path::PathBuf;
use std::sync::Mutex;
use std::sync::atomic::{AtomicU64, Ordering};

const DEFAULT_DATE: &str = "1989-05-30";
/// The navigator samples every 3 m: unsimplified, a big map is tens of MB.
const TOLERANCE: f32 = 0.75;
const BEFORE_STOP: f64 = 12.0;
const KEPT: usize = 4;

static BUILT: Mutex<Vec<(String, Value)>> = Mutex::new(Vec::new());
/// One at a time: opening a map sets the tile size for the whole process.
static BUILDING: Mutex<()> = Mutex::new(());
static GENERATION: AtomicU64 = AtomicU64::new(0);

pub(super) fn forget() {
    GENERATION.fetch_add(1, Ordering::SeqCst);
    BUILT.lock().unwrap_or_else(|e| e.into_inner()).clear();
}

fn kept(key: &str) -> Option<Value> {
    let built = BUILT.lock().unwrap_or_else(|e| e.into_inner());
    built.iter().find(|(k, _)| k == key).map(|(_, v)| v.clone())
}

pub(super) fn minimap(map: &str, date: &str) -> Result<Value> {
    let date = if date.trim().is_empty() { DEFAULT_DATE } else { date.trim() };
    let key = format!("{map}|{date}");
    if let Some(v) = kept(&key) {
        return Ok(v);
    }
    let _one = BUILDING.lock().unwrap_or_else(|e| e.into_inner());
    if let Some(v) = kept(&key) {
        return Ok(v);
    }
    let generation = GENERATION.load(Ordering::SeqCst);
    let v = build(map, date)?;
    let mut built = BUILT.lock().unwrap_or_else(|e| e.into_inner());
    if generation == GENERATION.load(Ordering::SeqCst) {
        if built.len() >= KEPT {
            built.remove(0);
        }
        built.push((key, v.clone()));
    }
    Ok(v)
}

fn round(v: f64) -> f64 {
    (v * 10.0).round() / 10.0
}

fn spawn(p: DVec3, heading: f64) -> String {
    format!("{},{},{}", round(p.x), round(p.y), round(heading.rem_euclid(360.0)))
}

fn build(map: &str, date: &str) -> Result<Value> {
    let t0 = std::time::Instant::now();
    let root = PathBuf::from(lib::load_config().root);
    // registers the content folder's roots with the OMSI readers
    let _ = lib::content_dir();
    let cfg = ::legacy_config::resolve_path(&root, map);
    let code = ::map::date_code(date).with_context(|| format!("'{date}' is not a date (YYYY-MM-DD)"))?;
    let world = World::open(&root, &cfg, code)?;
    world.index();

    let nav = world.navigation_map();
    let mut net = ::simulation::traffic::Network {
        lanes: nav.lanes,
        ..Default::default()
    };
    net.link(1.5);
    confirm_road_surfaces(&mut net, &nav.road_surfaces);
    let roads: Vec<Value> = road_geometry(&net)
        .into_iter()
        .map(|r| {
            // some maps' coordinates run into millions, where an f32 is off by half a metre
            let origin = r.points.first().copied().unwrap_or_default();
            let points: Vec<Vec3> = r.points.iter().map(|p| (*p - origin).as_vec3()).collect();
            let points: Vec<[f64; 2]> = simplify(&points, TOLERANCE)
                .iter()
                .map(|p| [round(origin.x + p.x as f64), round(origin.y + p.y as f64)])
                .collect();
            json!({ "main": r.main, "width": round(r.width as f64), "points": points })
        })
        .collect();

    let positions = world.object_positions.lock().clone();
    let chrono = world.chrono_dirs.read().clone();
    let off = ::map::chrono_deactivated_lines(&chrono);
    let tt = ::timetable::TimetableData::load_with_chrono(&world.map_dir, &chrono, &off);
    let mut seen = HashSet::new();
    let stops: Vec<Value> = tt
        .bus_stops
        .iter()
        .filter(|b| seen.insert(b.object_id))
        .filter_map(|b| {
            let (p, rot) = positions.get(&b.object_id)?;
            let h = rot[0].to_radians();
            let back = *p - DVec3::new(h.sin(), h.cos(), 0.0) * BEFORE_STOP;
            Some(json!({
                "id": b.object_id,
                "name": b.name.trim(),
                "x": round(p.x),
                "y": round(p.y),
                "spawn": spawn(back, rot[0]),
            }))
        })
        .collect();

    // numbered as `list_maps` does
    let eps = &world.global.entry_points;
    let mut total: HashMap<&str, usize> = HashMap::new();
    for e in eps {
        *total.entry(e.name.trim()).or_default() += 1;
    }
    let mut counted: HashMap<&str, usize> = HashMap::new();
    let entries: Vec<Value> = eps
        .iter()
        .enumerate()
        .filter_map(|(k, e)| {
            let name = e.name.trim();
            let n = counted.entry(name).or_default();
            *n += 1;
            let name = if total[name] > 1 { format!("{name} ({n})") } else { name.to_string() };
            let (p, rot) = world.entry_point_place(e)?;
            Some(json!({
                "index": k,
                "name": name,
                "x": round(p.x),
                "y": round(p.y),
                "spawn": spawn(p, rot[0]),
            }))
        })
        .collect();

    log::info!(
        "minimap of {map}: {} roads, {} stops, {} entry points in {:.1} s",
        roads.len(),
        stops.len(),
        entries.len(),
        t0.elapsed().as_secs_f64()
    );
    Ok(json!({ "map": map, "roads": roads, "stops": stops, "entries": entries }))
}
