use crate::navigator::{confirm_road_surfaces, road_geometry, simplify};
use crate::scene::World;
use anyhow::{Context, Result};
use glam::{DVec3, Vec3};
use omsi_launcher_lib as lib;
use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};
use std::sync::Mutex;

#[derive(Clone, Debug, Default, serde::Serialize, serde::Deserialize)]
pub struct Point {
    pub x: f64,
    pub y: f64,
}

#[derive(Clone, Debug, Default, serde::Serialize, serde::Deserialize)]
pub struct MinimapRoad {
    pub main: bool,
    pub width: f64,
    pub points: Vec<Point>,
}

#[derive(Clone, Debug, Default, serde::Serialize, serde::Deserialize)]
pub struct MinimapStop {
    pub id: i64,
    pub name: String,
    pub x: f64,
    pub y: f64,
    pub spawn: String,
}

#[derive(Clone, Debug, Default, serde::Serialize, serde::Deserialize)]
pub struct MinimapEntry {
    pub index: u32,
    pub name: String,
    pub x: f64,
    pub y: f64,
    pub spawn: String,
}

#[derive(Clone, Debug, Default, serde::Serialize, serde::Deserialize)]
pub struct MinimapLane {
    pub points: Vec<Point>,
}

#[derive(Clone, Debug, Default, serde::Serialize, serde::Deserialize)]
pub struct MinimapTrip {
    pub lanes: Vec<u32>,
}

#[derive(Clone, Debug, Default, serde::Serialize, serde::Deserialize)]
pub struct Minimap {
    pub map: String,
    pub roads: Vec<MinimapRoad>,
    pub stops: Vec<MinimapStop>,
    pub entries: Vec<MinimapEntry>,
    pub lanes: Vec<MinimapLane>,
    pub trips: HashMap<String, MinimapTrip>,
}

const DEFAULT_DATE: &str = "2026-10-01";
const TOLERANCE: f32 = 0.75;
const BEFORE_STOP: f64 = 12.0;
const KEPT: usize = 4;
const CACHE_VERSION: u32 = 2;
const CACHE_FILES: usize = 32;

static BUILT: Mutex<Vec<(String, Vec<Dep>, Minimap)>> = Mutex::new(Vec::new());
static BUILDING: Mutex<()> = Mutex::new(());

#[derive(Clone, Debug, PartialEq, serde::Serialize, serde::Deserialize)]
struct Dep {
    name: String,
    path: PathBuf,
    len: u64,
    modified: Option<std::time::SystemTime>,
}

impl Dep {
    fn of(root: &Path, name: &str) -> Dep {
        let path = ::mods::resolve_path(root, name);
        let meta = std::fs::metadata(&path).ok();
        Dep {
            name: name.to_string(),
            len: meta.as_ref().map_or(0, |m| m.len()),
            modified: meta.and_then(|m| m.modified().ok()),
            path,
        }
    }
}

fn deps_fresh(root: &Path, deps: &[Dep]) -> bool {
    use rayon::prelude::*;
    deps.par_iter().all(|d| Dep::of(root, &d.name) == *d)
}

#[derive(serde::Serialize, serde::Deserialize)]
struct Cached {
    deps: Vec<Dep>,
    minimap: Minimap,
}

pub(crate) fn minimap(map: &str, date: &str) -> Result<Minimap> {
    let date = if date.trim().is_empty() { DEFAULT_DATE } else { date.trim() };
    let root = PathBuf::from(lib::load_config().root);
    let _ = lib::register_content();
    let cfg = ::mods::resolve_path(&root, map);
    let code = ::map::date_code(date).with_context(|| format!("'{date}' is not a date (YYYY-MM-DD)"))?;
    let map_dir = cfg.parent().map(Path::to_path_buf).unwrap_or_default();
    let _one = BUILDING.lock().unwrap_or_else(|e| e.into_inner());
    // the date only changes the preview through the chrono folders active on it
    let chrono = ::map::active_chrono_dirs(&map_dir, code);
    let file = cache_file(map, &map_dir, &chrono);
    let key = match &file {
        Some(f) => f.to_string_lossy().into_owned(),
        None => format!("{map}|{chrono:?}"),
    };
    let kept = {
        let built = BUILT.lock().unwrap_or_else(|e| e.into_inner());
        built.iter().find(|b| b.0 == key).map(|b| (b.1.clone(), b.2.clone()))
    };
    if let Some((_, v)) = kept.filter(|(deps, _)| deps_fresh(&root, deps)) {
        return Ok(v);
    }
    let cached = file
        .as_deref()
        .and_then(read_cached)
        .filter(|c| deps_fresh(&root, &c.deps));
    let (deps, v) = match cached {
        Some(c) => (c.deps, c.minimap),
        None => {
            let (v, deps) = build(map, date, &root, &cfg, code)?;
            if let Some(f) = &file {
                write_cached(f, &deps, &v);
            }
            (deps, v)
        }
    };
    let mut built = BUILT.lock().unwrap_or_else(|e| e.into_inner());
    built.retain(|b| b.0 != key);
    if built.len() >= KEPT {
        built.remove(0);
    }
    built.push((key, deps, v.clone()));
    Ok(v)
}

/// The cached preview's file, named by the map, its active chrono folders and the size and
/// time of every file in its folder: a changed or added tile or timetable builds it anew.
fn cache_file(map: &str, map_dir: &Path, chrono: &[PathBuf]) -> Option<PathBuf> {
    use std::hash::{Hash, Hasher};
    let mut h = std::collections::hash_map::DefaultHasher::new();
    (CACHE_VERSION, map, chrono).hash(&mut h);
    let mut dirs = vec![map_dir.to_path_buf()];
    let mut any = false;
    while let Some(d) = dirs.pop() {
        let Ok(rd) = std::fs::read_dir(&d) else {
            continue;
        };
        let mut entries: Vec<(PathBuf, std::fs::Metadata)> =
            rd.flatten().filter_map(|e| Some((e.path(), e.metadata().ok()?))).collect();
        entries.sort_by(|a, b| a.0.cmp(&b.0));
        for (p, m) in entries {
            if m.is_dir() {
                dirs.push(p);
            } else {
                (&p, m.len(), m.modified().ok()).hash(&mut h);
                any = true;
            }
        }
    }
    any.then(|| lib::data_dir().join("cache").join("minimap").join(format!("{:016x}.json", h.finish())))
}

fn read_cached(path: &Path) -> Option<Cached> {
    let t0 = std::time::Instant::now();
    let c: Cached = serde_json::from_slice(&std::fs::read(path).ok()?).ok()?;
    log::info!("minimap of {} from the cache in {:.2} s", c.minimap.map, t0.elapsed().as_secs_f64());
    Some(c)
}

fn write_cached(path: &Path, deps: &[Dep], v: &Minimap) {
    let Some(dir) = path.parent() else {
        return;
    };
    let tmp = path.with_extension("tmp");
    let saved = std::fs::create_dir_all(dir)
        .and_then(|_| std::fs::write(&tmp, serde_json::to_vec(&Cached { deps: deps.to_vec(), minimap: v.clone() }).unwrap_or_default()))
        .and_then(|_| std::fs::rename(&tmp, path));
    if let Err(e) = saved {
        log::warn!("minimap cache {}: {e}", path.display());
        let _ = std::fs::remove_file(&tmp);
        return;
    }
    let Ok(rd) = std::fs::read_dir(dir) else {
        return;
    };
    let mut old: Vec<(std::time::SystemTime, PathBuf)> = rd
        .flatten()
        .filter_map(|e| Some((e.metadata().ok()?.modified().ok()?, e.path())))
        .collect();
    old.sort_by(|a, b| b.0.cmp(&a.0));
    for (_, p) in old.into_iter().skip(CACHE_FILES) {
        let _ = std::fs::remove_file(p);
    }
}

fn round(v: f64) -> f64 {
    (v * 10.0).round() / 10.0
}

fn spawn(p: DVec3, heading: f64) -> String {
    format!(
        "{},{},{},{}",
        round(p.x),
        round(p.y),
        round(heading.rem_euclid(360.0)),
        round(p.z)
    )
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
    let schedule = crate::schedule::Schedule::timetable_only(root, world, &clock);
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

fn build(map: &str, date: &str, root: &Path, cfg: &Path, code: i32) -> Result<(Minimap, Vec<Dep>)> {
    let t0 = std::time::Instant::now();
    let world = World::open(root, cfg, code)?;
    let deps: Vec<Dep> = {
        use rayon::prelude::*;
        let ix = world.index();
        let names: Vec<&String> = ix.files.keys().collect();
        names.into_par_iter().map(|n| Dep::of(root, n)).collect()
    };

    let nav = world.navigation_map();
    let mut net = ::simulation::traffic::Network {
        lanes: nav.lanes,
        ..Default::default()
    };
    net.link(1.5);
    confirm_road_surfaces(&mut net, &nav.road_surfaces);
    let (lanes, trips, driven, trains) = trip_routes(root, &world, &net, date);
    for l in driven {
        net.lanes[l].invisible = false;
    }
    let roads: Vec<MinimapRoad> = road_geometry(&net)
        .into_iter()
        .map(|r| MinimapRoad {
            main: r.main,
            width: round(r.width as f64),
            points: line(&r.points),
        })
        .collect();

    let chrono = world.chrono_dirs.read().clone();
    let off = ::map::chrono_deactivated_lines(&chrono);
    let tt = ::timetable::TimetableData::load_with_chrono(&world.map_dir, &chrono, &off);
    let mut seen = HashSet::new();
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
    let v = Minimap {
        map: map.to_string(),
        roads,
        stops,
        entries,
        lanes,
        trips,
    };
    Ok((v, deps))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_changed_dependency_makes_the_preview_stale() {
        let dir = std::env::temp_dir().join(format!("minimap-deps-{}", std::process::id()));
        std::fs::create_dir_all(dir.join("Splines")).unwrap();
        let file = dir.join("Splines").join("road.sli");
        std::fs::write(&file, "[spline]\n").unwrap();
        let deps = vec![Dep::of(&dir, "Splines/road.sli"), Dep::of(&dir, "Sceneryobjects/gone.sco")];
        assert!(deps_fresh(&dir, &deps));
        std::fs::write(&file, "[spline]\n[path]\n").unwrap();
        assert!(!deps_fresh(&dir, &deps));
        let _ = std::fs::remove_dir_all(&dir);
    }
}
