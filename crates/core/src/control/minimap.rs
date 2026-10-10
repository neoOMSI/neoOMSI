use crate::navigator::{confirm_road_surfaces, road_geometry, simplify};
use crate::scene::World;
use anyhow::{Context, Result};
use glam::{DVec3, Vec3};
use omsi_launcher_lib as lib;
use launcher_protocol::api::{
    Minimap, MinimapEntry, MinimapLane, MinimapRoad, MinimapStop, MinimapTrip, Point,
};
use std::collections::{HashMap, HashSet};
use std::path::PathBuf;
use std::sync::Mutex;
use std::sync::atomic::{AtomicU64, Ordering};

const DEFAULT_DATE: &str = "1989-05-30";
/// The navigator samples every 3 m: unsimplified, a big map is tens of MB.
const TOLERANCE: f32 = 0.75;
const BEFORE_STOP: f64 = 12.0;
const KEPT: usize = 4;

static BUILT: Mutex<Vec<(String, Minimap)>> = Mutex::new(Vec::new());
/// One at a time: opening a map sets the tile size for the whole process.
static BUILDING: Mutex<()> = Mutex::new(());
static GENERATION: AtomicU64 = AtomicU64::new(0);

pub(super) fn forget() {
    GENERATION.fetch_add(1, Ordering::SeqCst);
    BUILT.lock().unwrap_or_else(|e| e.into_inner()).clear();
}

fn kept(key: &str) -> Option<Minimap> {
    let built = BUILT.lock().unwrap_or_else(|e| e.into_inner());
    built.iter().find(|(k, _)| k == key).map(|(_, v)| v.clone())
}

pub(super) fn minimap(map: &str, date: &str) -> Result<Minimap> {
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

fn line(points: &[DVec3]) -> Vec<Point> {
    let origin = points.first().copied().unwrap_or_default();
    let local: Vec<Vec3> = points.iter().map(|p| (*p - origin).as_vec3()).collect();
    simplify(&local, TOLERANCE)
        .iter()
        .map(|p| Point {
            x: round(origin.x + p.x as f64),
            y: round(origin.y + p.y as f64),
        })
        .collect()
}

fn trip_routes(
    root: &std::path::Path,
    world: &World,
    net: &::simulation::traffic::Network,
    date: &str,
) -> (Vec<MinimapLane>, HashMap<String, MinimapTrip>, Vec<usize>, HashSet<String>) {
    let mut clock = ::simulation::SimClock::default();
    let ymd: Vec<i32> = date.split('-').filter_map(|x| x.trim().parse().ok()).collect();
    if let [y, m, d] = ymd[..] {
        clock.set_date(y, m, d);
    }
    let schedule = crate::schedule::Schedule::new(root, world, &clock);
    let mut lanes = Vec::new();
    let mut placed: HashMap<usize, u32> = HashMap::new();
    let mut trips = HashMap::new();
    let mut trains = HashSet::new();
    for trip in &schedule.data.trips {
        let route = schedule.trip_route_in(net, &trip.name);
        if route.is_empty() {
            continue;
        }
        if route.iter().all(|&l| net.lanes[l].kind == ::simulation::traffic::LaneKind::Rail) {
            trains.insert(trip.name.clone());
        }
        let ids = route
            .into_iter()
            .map(|l| {
                *placed.entry(l).or_insert_with(|| {
                    lanes.push(MinimapLane {
                        points: line(&net.lanes[l].points),
                    });
                    (lanes.len() - 1) as u32
                })
            })
            .collect();
        trips.insert(trip.name.clone(), MinimapTrip { lanes: ids });
    }
    (lanes, trips, placed.into_keys().collect(), trains)
}

fn build(map: &str, date: &str) -> Result<Minimap> {
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
    let (lanes, trips, driven, trains) = trip_routes(&root, &world, &net, date);
    for l in driven {
        net.lanes[l].invisible = false;
    }
    let roads: Vec<MinimapRoad> = road_geometry(&net)
        .into_iter()
        .map(|r| MinimapRoad {
            main: r.main,
            width: round(r.width as f64),
            // some maps' coordinates run into millions, where an f32 is off by half a metre
            points: line(&r.points),
        })
        .collect();

    let chrono = world.chrono_dirs.read().clone();
    let off = ::map::chrono_deactivated_lines(&chrono);
    let tt = ::timetable::TimetableData::load_with_chrono(&world.map_dir, &chrono, &off);
    let mut seen = HashSet::new();
    // Busstops.cfg is the editor's list and may hold a few of them only: the trips name every
    // stop they serve, by object id, tile and name
    let named = tt
        .bus_stops
        .iter()
        .map(|b| (b.object_id, b.group, b.name.trim()))
        .chain(tt.trips.iter().flat_map(|t| &t.stations_legacy).filter_map(|s| {
            Some((s.first()?.trim().parse().ok()?, s.get(3)?.trim().parse().ok()?, s.get(2)?.trim()))
        }));
    let (mut by_bus, mut by_train) = (HashSet::new(), HashSet::new());
    for t in &tt.trips {
        let ids = t
            .stations
            .iter()
            .copied()
            .chain(t.stations_legacy.iter().filter_map(|s| s.first()?.trim().parse().ok()));
        if trains.contains(&t.name) { by_train.extend(ids) } else { by_bus.extend(ids) }
    }
    let stops: Vec<MinimapStop> = named
        .filter(|(id, _, _)| by_bus.contains(id) || !by_train.contains(id))
        .filter(|(id, tile, _)| seen.insert((*id, *tile)))
        .filter_map(|(id, tile, name)| {
            let (p, rot) = world.object_on_tile(tile, id)?;
            let h = rot[0].to_radians();
            let back = p - DVec3::new(h.sin(), h.cos(), 0.0) * BEFORE_STOP;
            Some(MinimapStop {
                id,
                name: name.to_string(),
                x: round(p.x),
                y: round(p.y),
                spawn: spawn(back, rot[0]),
            })
        })
        .collect();

    // as `list_maps` lists them: every entry point of a name picks that name's line
    let eps = &world.global.entry_points;
    let mut line: HashMap<&str, usize> = HashMap::new();
    for (name, places) in world.global.entry_point_groups() {
        line.insert(name, places[0]);
    }
    let entries: Vec<MinimapEntry> = eps
        .iter()
        .filter_map(|e| {
            let name = e.name.trim();
            let (p, rot) = world.entry_point_place(e)?;
            Some(MinimapEntry {
                index: line[name] as u32,
                name: name.to_string(),
                x: round(p.x),
                y: round(p.y),
                spawn: spawn(p, rot[0]),
            })
        })
        .collect();

    log::info!(
        "minimap of {map}: {} roads, {} stops, {} entry points, {} trips on {} lanes in {:.1} s",
        roads.len(),
        stops.len(),
        entries.len(),
        trips.len(),
        lanes.len(),
        t0.elapsed().as_secs_f64()
    );
    Ok(Minimap {
        map: map.to_string(),
        roads,
        stops,
        entries,
        lanes,
        trips,
    })
}
