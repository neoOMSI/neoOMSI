//! Headless domain benchmark: the per-tick cost of the traffic domain at 100, 500 and 1000
//! active vehicles under an ordinary and a congested junction load.
//!
//! This target is `harness = false` and uses only `std`, so it adds no dependency and keeps
//! `cargo tree -p traffic` at `glam`, `hashbrown`, `log` (+ leaves). It measures the work core
//! does **before** motion realization: build the frozen `Occupancy` (the local spatial index),
//! then plan each vehicle's junction decision against that one snapshot. Rendering, asset
//! loading and vehicle scripts are deliberately excluded (the plan measures those separately).
//!
//! Run with `cargo bench -p traffic --bench domain`.
//!
//! Reported per case: p50/p95/p99 per-tick cost, allocations per tick and bytes per tick
//! (counting global allocator), and peak live heap. A streaming case measures
//! `Network::extend` + relinking.

use std::alloc::{GlobalAlloc, Layout, System};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::time::Instant;

use glam::{DVec2, DVec3};
use hashbrown::HashMap;
use traffic::perception::{BodyFootprint, Occupancy, Placement};
use traffic::{
    junction_ahead, JunctionActor, JunctionCoordinator, JunctionScene, LaneBuilder, LaneId,
    LaneKey, LaneKind, Lead, ManeuverActor, ManeuverCoordinator, ManeuverInputs, ManeuverScene,
    ManeuverState, Network, VehicleId,
};

// ---------------------------------------------------------------------------------------
// Counting allocator
// ---------------------------------------------------------------------------------------

static ALLOCS: AtomicUsize = AtomicUsize::new(0);
static BYTES: AtomicUsize = AtomicUsize::new(0);
static LIVE: AtomicUsize = AtomicUsize::new(0);
static PEAK: AtomicUsize = AtomicUsize::new(0);

struct Counting;

unsafe impl GlobalAlloc for Counting {
    unsafe fn alloc(&self, layout: Layout) -> *mut u8 {
        ALLOCS.fetch_add(1, Ordering::Relaxed);
        BYTES.fetch_add(layout.size(), Ordering::Relaxed);
        let live = LIVE.fetch_add(layout.size(), Ordering::Relaxed) + layout.size();
        PEAK.fetch_max(live, Ordering::Relaxed);
        unsafe { System.alloc(layout) }
    }

    unsafe fn dealloc(&self, ptr: *mut u8, layout: Layout) {
        LIVE.fetch_sub(layout.size(), Ordering::Relaxed);
        unsafe { System.dealloc(ptr, layout) }
    }

    unsafe fn realloc(&self, ptr: *mut u8, layout: Layout, new_size: usize) -> *mut u8 {
        ALLOCS.fetch_add(1, Ordering::Relaxed);
        BYTES.fetch_add(new_size, Ordering::Relaxed);
        if new_size >= layout.size() {
            let grow = new_size - layout.size();
            let live = LIVE.fetch_add(grow, Ordering::Relaxed) + grow;
            PEAK.fetch_max(live, Ordering::Relaxed);
        } else {
            LIVE.fetch_sub(layout.size() - new_size, Ordering::Relaxed);
        }
        unsafe { System.realloc(ptr, layout, new_size) }
    }
}

#[global_allocator]
static GLOBAL: Counting = Counting;

// ---------------------------------------------------------------------------------------
// Synthetic world
// ---------------------------------------------------------------------------------------

const APPROACH_LEN: f64 = 2000.0;
const HALF_LEN: f64 = 2.25;
const HALF_W: f64 = 1.25;

/// A `+` crossing with four 2 km approaches (lanes 0/2/4/6) into four crossing-object lanes
/// (1/3/5/7). Mirrors `tests/common::four_way` but long enough to hold 1000 vehicles.
fn four_way() -> Network {
    let object = |from: DVec3, to: DVec3, path: u16| {
        let mut l = LaneBuilder::polyline(vec![from, to], LaneKind::Street, 3.0);
        l.source = 2;
        l.key = Some(LaneKey {
            tile: (0, 0),
            id: 1,
            path,
        });
        l
    };
    let far = APPROACH_LEN;
    let mut net = Network {
        lanes: vec![
            LaneBuilder::polyline(
                vec![DVec3::new(-far, 0.0, 0.0), DVec3::new(-10.0, 0.0, 0.0)],
                LaneKind::Street,
                3.0,
            ),
            object(DVec3::new(-10.0, 0.0, 0.0), DVec3::new(10.0, 0.0, 0.0), 0),
            LaneBuilder::polyline(
                vec![DVec3::new(far, 0.0, 0.0), DVec3::new(10.0, 0.0, 0.0)],
                LaneKind::Street,
                3.0,
            ),
            object(DVec3::new(10.0, 0.0, 0.0), DVec3::new(-10.0, 0.0, 0.0), 1),
            LaneBuilder::polyline(
                vec![DVec3::new(0.0, -far, 0.0), DVec3::new(0.0, -10.0, 0.0)],
                LaneKind::Street,
                3.0,
            ),
            object(DVec3::new(0.0, -10.0, 0.0), DVec3::new(0.0, 10.0, 0.0), 2),
            LaneBuilder::polyline(
                vec![DVec3::new(0.0, far, 0.0), DVec3::new(0.0, 10.0, 0.0)],
                LaneKind::Street,
                3.0,
            ),
            object(DVec3::new(0.0, 10.0, 0.0), DVec3::new(0.0, -10.0, 0.0), 3),
        ],
        ..Default::default()
    };
    for (approach, object) in [(0, 1), (2, 3), (4, 5), (6, 7)] {
        net.lanes[approach].next = vec![object];
    }
    net.compute_conflicts();
    net
}

/// The four approach lanes, their world start and unit direction.
fn approaches() -> [(usize, DVec2, DVec2); 4] {
    [
        (0, DVec2::new(-APPROACH_LEN, 0.0), DVec2::new(1.0, 0.0)),
        (2, DVec2::new(APPROACH_LEN, 0.0), DVec2::new(-1.0, 0.0)),
        (4, DVec2::new(0.0, -APPROACH_LEN), DVec2::new(0.0, 1.0)),
        (6, DVec2::new(0.0, APPROACH_LEN), DVec2::new(0.0, -1.0)),
    ]
}

/// `n` vehicles on the four approaches. Ordinary spreads them over the whole approach;
/// congested packs them bumper-to-bumper against the stop line.
fn place(n: usize, congested: bool) -> Vec<BodyFootprint> {
    let app = approaches();
    let per = n.div_ceil(app.len());
    let mut feet = Vec::with_capacity(n);
    let mut id = 1u64;
    for &(lane, start, dir) in &app {
        for k in 0..per {
            if feet.len() == n {
                break;
            }
            let s = if congested {
                (APPROACH_LEN - 4.0 - (k as f64) * 6.0).max(0.5)
            } else {
                (k as f64 + 0.5) / (per as f64) * (APPROACH_LEN - 20.0) + 10.0
            };
            let center = start + dir * s;
            let mut f = BodyFootprint::new(
                VehicleId(id),
                center,
                dir,
                HALF_LEN,
                HALF_W,
                0.0,
                3.0,
                0.0,
            );
            f.current = Some(Placement {
                lane: LaneId(lane),
                s: s as f32,
                lateral: 0.0,
                foreign: false,
            });
            feet.push(f);
            id += 1;
        }
    }
    feet
}

/// Everything a tick's junction planning reads, rebuilt from the realized bodies each tick,
/// exactly as `core::Traffic::tick` does.
struct Scene {
    actors: Vec<JunctionActor>,
    index_of: HashMap<VehicleId, usize>,
    on_lane: HashMap<usize, Vec<(usize, f32, f32, bool)>>,
    coming: HashMap<usize, Vec<(usize, f32)>>,
    geo_prev: HashMap<VehicleId, Option<VehicleId>>,
    aspects: HashMap<(usize, usize), traffic::Aspect>,
    ways: Vec<Vec<(usize, f32)>>,
    movements: Vec<Option<traffic::Movement>>,
    leads: Vec<Option<Lead>>,
}

impl Scene {
    fn build(net: &Network, feet: &[BodyFootprint]) -> Scene {
        let mut actors = Vec::with_capacity(feet.len());
        let mut index_of = HashMap::with_capacity(feet.len());
        let mut on_lane: HashMap<usize, Vec<(usize, f32, f32, bool)>> = HashMap::new();
        let mut ways = Vec::with_capacity(feet.len());
        for (i, f) in feet.iter().enumerate() {
            let place = f.current.expect("current placement");
            index_of.insert(f.owner, i);
            on_lane
                .entry(place.lane.index())
                .or_default()
                .push((i, place.s, place.lateral, place.foreign));
            let mut a = JunctionActor::new(f.owner, place.lane.index(), place.s);
            a.front = f.front;
            a.rear = f.rear;
            a.length = f.front + f.rear;
            a.speed = f.speed;
            actors.push(a);
            // The planned way: own lane then up to three `next` lanes.
            let mut way = vec![(place.lane.index(), -place.s)];
            let mut d = net.lanes[place.lane.index()].length() - place.s;
            let mut cur = place.lane.index();
            for _ in 0..3 {
                let Some(&n) = net.lanes[cur].next.first() else {
                    break;
                };
                way.push((n, d));
                d += net.lanes[n].length();
                cur = n;
            }
            ways.push(way);
        }
        let movements = ways.iter().map(|w| junction_ahead(net, w)).collect();
        let leads = vec![None; actors.len()];
        Scene {
            actors,
            index_of,
            on_lane,
            coming: HashMap::new(),
            geo_prev: HashMap::new(),
            aspects: HashMap::new(),
            ways,
            movements,
            leads,
        }
    }

    fn junction<'a>(&'a self, net: &'a Network) -> JunctionScene<'a> {
        JunctionScene {
            net,
            actors: &self.actors,
            index_of: &self.index_of,
            on_lane: &self.on_lane,
            coming: &self.coming,
            walkers: empty_walkers(),
            geo_prev: &self.geo_prev,
            aspects: &self.aspects,
            time: 0.0,
            tick: 0,
        }
    }
}

/// A process-wide empty pedestrian map, so the scene can borrow it for its whole life.
fn empty_walkers() -> &'static HashMap<usize, Vec<f32>> {
    static EMPTY: std::sync::LazyLock<HashMap<usize, Vec<f32>>> =
        std::sync::LazyLock::new(HashMap::new);
    &EMPTY
}

// ---------------------------------------------------------------------------------------
// Measurement
// ---------------------------------------------------------------------------------------

struct Report {
    ticks: usize,
    p50: f64,
    p95: f64,
    p99: f64,
    allocs_per_tick: f64,
    bytes_per_tick: f64,
    peak_kib: f64,
}

fn percentile(sorted: &[f64], p: f64) -> f64 {
    if sorted.is_empty() {
        return 0.0;
    }
    let idx = ((p / 100.0) * (sorted.len() - 1) as f64).round() as usize;
    sorted[idx.min(sorted.len() - 1)]
}

/// One measured run: build the scene and plan every vehicle's junction admission per tick.
fn run_junction(net: &Network, feet: &[BodyFootprint], ticks: usize) -> Report {
    let mut coord = JunctionCoordinator::new();
    // Warm up (also lets the arbiter/claims reach a steady state).
    for t in 0..8 {
        coord.begin_tick(t);
        let scene = Scene::build(net, feet);
        let js = scene.junction(net);
        for (i, way) in scene.ways.iter().enumerate() {
            coord.plan(&js, i, way, scene.leads[i], scene.movements[i].clone());
        }
    }
    let mut times = Vec::with_capacity(ticks);
    let allocs0 = ALLOCS.load(Ordering::Relaxed);
    let bytes0 = BYTES.load(Ordering::Relaxed);
    for t in 0..ticks as u64 {
        let a0 = ALLOCS.load(Ordering::Relaxed);
        let b0 = BYTES.load(Ordering::Relaxed);
        let start = Instant::now();
        coord.begin_tick(t);
        let scene = Scene::build(net, feet);
        let js = scene.junction(net);
        for (i, way) in scene.ways.iter().enumerate() {
            coord.plan(&js, i, way, scene.leads[i], scene.movements[i].clone());
        }
        times.push(start.elapsed().as_secs_f64() * 1000.0);
        let _ = (a0, b0);
    }
    let allocs = ALLOCS.load(Ordering::Relaxed) - allocs0;
    let bytes = BYTES.load(Ordering::Relaxed) - bytes0;
    times.sort_by(|a, b| a.partial_cmp(b).unwrap());
    Report {
        ticks,
        p50: percentile(&times, 50.0),
        p95: percentile(&times, 95.0),
        p99: percentile(&times, 99.0),
        allocs_per_tick: allocs as f64 / ticks as f64,
        bytes_per_tick: bytes as f64 / ticks as f64,
        peak_kib: PEAK.load(Ordering::Relaxed) as f64 / 1024.0,
    }
}

/// One measured run of the lateral owner (maneuver planning), the other per-vehicle cost.
fn run_maneuver(net: &Network, feet: &[BodyFootprint], ticks: usize) -> Report {
    let mut coord = ManeuverCoordinator::new();
    let mut states: Vec<ManeuverState> = feet.iter().map(|_| ManeuverState::default()).collect();
    let actors: Vec<ManeuverActor> = feet
        .iter()
        .map(|f| {
            let p = f.current.unwrap();
            ManeuverActor::new(f.owner, p.lane.index(), p.s)
        })
        .collect();
    let mut times = Vec::with_capacity(ticks);
    let allocs0 = ALLOCS.load(Ordering::Relaxed);
    let bytes0 = BYTES.load(Ordering::Relaxed);
    for t in 0..ticks as u64 {
        let start = Instant::now();
        coord.begin_tick(&[], t);
        let occ = Occupancy::build(net.version(), t, feet.to_vec());
        let scene = ManeuverScene {
            net,
            occupancy: &occ,
            actors: &actors,
            people: &[],
            time: t as f32 * 0.02,
            dt: 0.02,
            tick: t,
        };
        for k in 0..actors.len() {
            let inputs = ManeuverInputs::new(k);
            coord.plan(&scene, &mut states[k], &inputs);
        }
        times.push(start.elapsed().as_secs_f64() * 1000.0);
    }
    let allocs = ALLOCS.load(Ordering::Relaxed) - allocs0;
    let bytes = BYTES.load(Ordering::Relaxed) - bytes0;
    times.sort_by(|a, b| a.partial_cmp(b).unwrap());
    Report {
        ticks,
        p50: percentile(&times, 50.0),
        p95: percentile(&times, 95.0),
        p99: percentile(&times, 99.0),
        allocs_per_tick: allocs as f64 / ticks as f64,
        bytes_per_tick: bytes as f64 / ticks as f64,
        peak_kib: PEAK.load(Ordering::Relaxed) as f64 / 1024.0,
    }
}

fn print_report(label: &str, r: &Report) {
    println!(
        "{label:<34} ticks={:<5} p50={:>7.3}ms p95={:>7.3}ms p99={:>7.3}ms  allocs/tick={:>8.1}  \
         bytes/tick={:>10.0}  peak={:>8.1}KiB",
        r.ticks, r.p50, r.p95, r.p99, r.allocs_per_tick, r.bytes_per_tick, r.peak_kib
    );
}

/// Streaming update cost: extend the network and relink, the per-tile work the population
/// owner requests ahead of a route frontier.
fn run_streaming() {
    let ticks = 40;
    let mut times = Vec::with_capacity(ticks);
    for round in 0..ticks {
        let mut net = four_way();
        let base = net.lanes.len();
        let fresh: Vec<traffic::Lane> = (0..50)
            .map(|i| {
                LaneBuilder::polyline(
                    vec![
                        DVec3::new(-APPROACH_LEN - 200.0, i as f64 * 4.0, 0.0),
                        DVec3::new(-APPROACH_LEN - 10.0, i as f64 * 4.0, 0.0),
                    ],
                    LaneKind::Street,
                    3.0,
                )
            })
            .collect();
        let start = Instant::now();
        let added = net.extend(fresh, 1.5);
        assert_eq!(added.len(), 50);
        let _ = net.version();
        times.push(start.elapsed().as_secs_f64() * 1000.0);
        let _ = (round, base);
    }
    times.sort_by(|a, b| a.partial_cmp(b).unwrap());
    println!(
        "{:<34} extend 50 lanes + relink: p50={:.3}ms p95={:.3}ms p99={:.3}ms",
        "streaming",
        percentile(&times, 50.0),
        percentile(&times, 95.0),
        percentile(&times, 99.0)
    );
}

fn main() {
    println!("traffic domain benchmark (headless, no renderer/assets)");
    println!(
        "vehicle: {} | human: traffic domain tick = Occupancy::build + per-vehicle junction \
         admission (core also realizes bodies/scripts separately)",
        env!("CARGO_PKG_VERSION")
    );
    let ticks: usize = std::env::var("BENCH_TICKS")
        .ok()
        .and_then(|v| v.parse().ok())
        .unwrap_or(60);
    for &n in &[100usize, 500, 1000] {
        let net = four_way();
        for &congested in &[false, true] {
            let feet = place(n, congested);
            let label = format!(
                "junction n={n} {}",
                if congested { "congested" } else { "ordinary" }
            );
            let r = run_junction(&net, &feet, ticks);
            print_report(&label, &r);
        }
    }
    for &n in &[100usize, 500, 1000] {
        let net = four_way();
        let feet = place(n, true);
        let label = format!("maneuver n={n} congested");
        let r = run_maneuver(&net, &feet, ticks.min(30));
        print_report(&label, &r);
    }
    run_streaming();
}
