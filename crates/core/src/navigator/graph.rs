use super::*;

/// Size of a cell of [`RoadGraph`]'s grid (m).
const CELL: f64 = 100.0;
/// Road ends closer than this to another road are joined to it (m).
const JOIN_GAP: f64 = 3.0;
/// Ends this far from any road do not count as a gap in the network (m).
const OPEN_END: f64 = 12.0;

/// What the navigator draws as the street network, built once per map: every carriageway a
/// spline draws (with or without a path for cars), the lanes of the junction objects and the
/// surface between them, and short pieces that close the gaps between ends that almost meet.
#[derive(Default)]
pub(crate) struct RoadGraph {
    pub(crate) roads: Vec<MapRoad>,
    /// Junction surfaces: convex outlines on the ground.
    pub(crate) areas: Vec<Vec<DVec3>>,
    /// Road ends no other road comes near: where the network has a hole (or ends).
    pub(crate) open_ends: Vec<DVec3>,
    grid: HashMap<(i32, i32), Vec<u32>>,
    area_grid: HashMap<(i32, i32), Vec<u32>>,
}

fn cell(p: DVec2) -> (i32, i32) {
    ((p.x / CELL).floor() as i32, (p.y / CELL).floor() as i32)
}

fn cells_of(pts: &[DVec3], margin: f64, mut add: impl FnMut((i32, i32))) {
    let mut seen = hashbrown::HashSet::new();
    for ab in pts.windows(2).chain(pts.get(..1).filter(|_| pts.len() == 1)) {
        let (a, b) = (ab[0].truncate(), ab[ab.len() - 1].truncate());
        let (x0, y0) = cell(a.min(b) - DVec2::splat(margin));
        let (x1, y1) = cell(a.max(b) + DVec2::splat(margin));
        for x in x0..=x1 {
            for y in y0..=y1 {
                if seen.insert((x, y)) {
                    add((x, y));
                }
            }
        }
    }
}

/// The nearest point of the polyline to `p` within `dz` in height: (distance, point).
fn nearest_on(pts: &[DVec3], p: DVec3, dz: f64) -> Option<(f64, DVec3)> {
    let mut best: Option<(f64, DVec3)> = None;
    for ab in pts.windows(2) {
        let (a, b) = (ab[0], ab[1]);
        let d = (b - a).truncate();
        let t = ((p - a).truncate().dot(d) / d.length_squared().max(1e-9)).clamp(0.0, 1.0);
        let q = a.lerp(b, t);
        if (q.z - p.z).abs() > dz {
            continue;
        }
        let dist = (q - p).truncate().length();
        if best.map(|b| dist < b.0).unwrap_or(true) {
            best = Some((dist, q));
        }
    }
    best
}

/// Convex hull of points in the plane (counter-clockwise), heights kept.
pub(super) fn convex_hull(mut pts: Vec<DVec3>) -> Vec<DVec3> {
    pts.sort_by(|a, b| a.x.total_cmp(&b.x).then(a.y.total_cmp(&b.y)));
    pts.dedup_by(|a, b| (*a - *b).truncate().length_squared() < 1e-6);
    if pts.len() < 3 {
        return pts;
    }
    let cross = |o: DVec3, a: DVec3, b: DVec3| (a - o).truncate().perp_dot((b - o).truncate());
    let mut hull: Vec<DVec3> = Vec::with_capacity(pts.len() * 2);
    for pass in 0..2 {
        let start = hull.len();
        let iter: Box<dyn Iterator<Item = &DVec3>> = if pass == 0 {
            Box::new(pts.iter())
        } else {
            Box::new(pts.iter().rev())
        };
        for &p in iter {
            while hull.len() >= start + 2 && cross(hull[hull.len() - 2], hull[hull.len() - 1], p) <= 0.0
            {
                hull.pop();
            }
            hull.push(p);
        }
        hull.pop();
    }
    hull
}

fn area_of(poly: &[DVec3]) -> f64 {
    let n = poly.len();
    (0..n)
        .map(|i| poly[i].truncate().perp_dot(poly[(i + 1) % n].truncate()))
        .sum::<f64>()
        .abs()
        * 0.5
}

/// The surface of each junction object: the outline around where its street paths begin and
/// end, as wide as the paths. Skipped where its middle is on none of the paths (a roundabout's
/// island, a depot's yard) or it would cover far more than the paths do.
fn junction_areas(net: &Network) -> Vec<Vec<DVec3>> {
    let mut objects: std::collections::BTreeMap<((i32, i32), i64), Vec<&::simulation::traffic::Lane>> =
        Default::default();
    for l in net.lanes.iter().filter(|l| {
        l.kind == LaneKind::Street && l.source == 2 && !l.invisible && l.points.len() >= 2
    }) {
        if let Some(k) = l.key {
            objects.entry((k.tile, k.id)).or_default().push(l);
        }
    }
    let mut out = Vec::new();
    for lanes in objects.into_values() {
        if lanes.len() < 2 {
            continue;
        }
        let mut pts = Vec::new();
        let mut covered = 0.0;
        for l in &lanes {
            let w = l.width.max(2.6) as f64 * 0.5;
            covered += l.length() as f64 * w * 2.0;
            for (p, h) in [(l.start(), l.start_heading()), (l.end(), l.end_heading())] {
                let hr = (h as f64).to_radians();
                let side = DVec3::new(hr.cos(), -hr.sin(), 0.0) * w;
                pts.push(p + side);
                pts.push(p - side);
            }
        }
        let hull = convex_hull(pts);
        if hull.len() < 3 {
            continue;
        }
        let span = hull
            .iter()
            .flat_map(|a| hull.iter().map(move |b| (*a - *b).truncate().length()))
            .fold(0.0, f64::max);
        // a roundabout's island or a yard: the middle of the outline is on none of the paths
        let mid = hull.iter().copied().sum::<DVec3>() / hull.len() as f64;
        let on_path = lanes.iter().any(|l| {
            l.nearest_point(DVec3::new(mid.x, mid.y, l.start().z))
                .is_some_and(|(_, d)| d <= l.width.max(2.6) as f64 * 0.5 + 2.0)
        });
        if span <= 70.0 && on_path && area_of(&hull) <= covered * 2.5 {
            out.push(hull);
        }
    }
    out
}

impl RoadGraph {
    /// The map's streets from its lanes and the carriageways its splines draw. The lanes of a
    /// spline that has its carriageway here are not drawn again.
    pub(crate) fn build(net: &Network, ways: &[crate::scene::NavCarriageway]) -> RoadGraph {
        let t0 = std::time::Instant::now();
        let with_way: hashbrown::HashSet<((i32, i32), i64)> =
            ways.iter().filter(|w| w.cars).map(|w| (w.tile, w.id)).collect();
        let mut fast: hashbrown::HashSet<((i32, i32), i64)> = hashbrown::HashSet::new();
        for l in net.lanes.iter().filter(|l| l.kind == LaneKind::Street) {
            if let Some(k) = l.key.filter(|_| l.source == 1 && l.speed_limit_kmh >= 55.0) {
                fast.insert((k.tile, k.id));
            }
        }
        let mut roads: Vec<MapRoad> = road_geometry(net)
            .into_iter()
            .filter(|r| r.spline.map(|k| !with_way.contains(&k)).unwrap_or(true))
            .collect();
        let from_lanes = roads.len();
        roads.extend(ways.iter().filter(|w| w.points.len() >= 2).map(|w| MapRoad {
            points: w.points.clone(),
            width: w.width,
            main: fast.contains(&(w.tile, w.id)),
            spline: Some((w.tile, w.id)),
        }));
        if ::legacy_config::env::var_os("OMSI_DEBUG_NAV").is_some() {
            let mut by: HashMap<(u32, String), (usize, f32)> = HashMap::new();
            for l in net.lanes.iter().filter(|l| l.kind == LaneKind::Street && !l.invisible) {
                let e = by.entry((l.source, l.name.clone())).or_default();
                e.0 += 1;
                e.1 += l.length();
            }
            let mut v: Vec<_> = by.into_iter().collect();
            v.sort_by(|a, b| b.1 .1.total_cmp(&a.1 .1));
            for ((src, name), (n, len)) in v.iter().take(25) {
                log::info!("navigator: street lanes of source {src} '{name}': {n}, {len:.0} m");
            }
        }
        let areas = junction_areas(net);
        let mut g = RoadGraph {
            roads,
            areas,
            ..Default::default()
        };
        g.index();
        let joined = g.close_gaps();
        g.index();
        log::info!(
            "navigator: road graph of {} roads ({} from lanes, {} carriageways, {} joins), {} junction surfaces, {} open ends, {:.0} ms",
            g.roads.len(),
            from_lanes,
            ways.len(),
            joined,
            g.areas.len(),
            g.open_ends.len(),
            t0.elapsed().as_secs_f64() * 1000.0
        );
        g
    }

    fn index(&mut self) {
        self.grid.clear();
        self.area_grid.clear();
        for (i, r) in self.roads.iter().enumerate() {
            let grid = &mut self.grid;
            cells_of(&r.points, r.width as f64 * 0.5, |c| {
                grid.entry(c).or_default().push(i as u32)
            });
        }
        for (i, a) in self.areas.iter().enumerate() {
            let grid = &mut self.area_grid;
            let mut ring = a.clone();
            ring.extend(a.first().copied());
            cells_of(&ring, 0.0, |c| grid.entry(c).or_default().push(i as u32));
        }
    }

    fn roads_in(&self, lo: DVec2, hi: DVec2) -> Vec<usize> {
        let (x0, y0) = cell(lo);
        let (x1, y1) = cell(hi);
        let mut seen = hashbrown::HashSet::new();
        for x in x0..=x1 {
            for y in y0..=y1 {
                if let Some(v) = self.grid.get(&(x, y)) {
                    seen.extend(v.iter().map(|&i| i as usize));
                }
            }
        }
        let mut v: Vec<usize> = seen.into_iter().collect();
        v.sort_unstable();
        v
    }

    /// Roads and junction surfaces with a part within `radius` of `c`.
    pub(crate) fn near(&self, c: DVec2, radius: f64) -> (Vec<usize>, Vec<usize>) {
        let r = DVec2::splat(radius);
        let roads = self.roads_in(c - r, c + r);
        let (x0, y0) = cell(c - r);
        let (x1, y1) = cell(c + r);
        let mut areas = hashbrown::HashSet::new();
        for x in x0..=x1 {
            for y in y0..=y1 {
                if let Some(v) = self.area_grid.get(&(x, y)) {
                    areas.extend(v.iter().map(|&i| i as usize));
                }
            }
        }
        let mut areas: Vec<usize> = areas.into_iter().collect();
        areas.sort_unstable();
        (roads, areas)
    }

    /// Join every road end that stops just short of another road to it; note the ends no
    /// road comes near. Returns how many joins were made.
    fn close_gaps(&mut self) -> usize {
        let mut joins = Vec::new();
        let mut open = Vec::new();
        for (i, r) in self.roads.iter().enumerate() {
            let n = r.points.len();
            for (end, inward) in [(r.points[0], r.points[1.min(n - 1)]), (r.points[n - 1], r.points[n.saturating_sub(2)])] {
                let reach = DVec2::splat(OPEN_END + r.width as f64);
                let mut best: Option<(f64, DVec3, f32)> = None;
                for j in self.roads_in(end.truncate() - reach, end.truncate() + reach) {
                    if j == i {
                        continue;
                    }
                    let o = &self.roads[j];
                    if let Some((d, q)) = nearest_on(&o.points, end, 3.0) {
                        // the edge of the other road, not its centre line
                        let gap = d - o.width as f64 * 0.5;
                        if best.map(|b| gap < b.0).unwrap_or(true) {
                            best = Some((gap, q, o.width));
                        }
                    }
                }
                let inside_area = self.areas.iter().any(|a| {
                    a.len() >= 3
                        && (0..a.len()).all(|k| {
                            let (p, q) = (a[k].truncate(), a[(k + 1) % a.len()].truncate());
                            (q - p).perp_dot(end.truncate() - p) >= -1.0
                        })
                });
                match best {
                    _ if inside_area => {}
                    Some((gap, _, _)) if gap <= 0.0 => {}
                    Some((gap, q, w)) if gap <= JOIN_GAP => {
                        // carry on in the direction the road ran, onto the other one
                        let dir = (end - inward).truncate().normalize_or_zero();
                        let to = (q - end).truncate();
                        let ahead = dir.dot(to).max(0.0);
                        let tip = end + (dir * ahead).extend(q.z - end.z);
                        joins.push(MapRoad {
                            points: if (tip - end).truncate().length() > 0.3 {
                                vec![end, tip, q]
                            } else {
                                vec![end, q]
                            },
                            width: r.width.min(w.max(2.6)),
                            main: r.main,
                            spline: None,
                        });
                    }
                    Some((gap, _, _)) if gap <= OPEN_END => open.push(end),
                    _ => {}
                }
            }
        }
        let n = joins.len();
        self.roads.extend(joins);
        self.open_ends = open;
        n
    }
}
