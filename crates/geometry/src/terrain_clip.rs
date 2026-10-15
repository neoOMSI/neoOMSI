//! The ground cut along `[terrainhole]` rims cell by cell, as OMSI 2 cuts it for an object
//! laid with `[spline_terrain_align]` (see its `.map.terrain_0.rdy` cache): the part inside
//! the rim goes, and the new corners - the rim's own and where it crosses the grid lines -
//! stand at the rim's height, not the ground's. Cut by the mask alone, the ground in front
//! of Städtedreieck's Kaiserberg portal stood 6 m over the road (#241).

use crate::{MeshData, compute_normals};
use ::map::{Terrain, tile_size};
use glam::{DVec2, DVec3};
use std::collections::HashMap;

/// How near (m) a corner has to lie to a rim to stand on it.
const ON_RIM: f64 = 2e-3;
/// Corners this near (m) to a cutting line are on it.
const ON_LINE: f64 = 1e-5;
/// Pieces smaller than this (m²) are dropped.
const MIN_AREA: f64 = 1e-6;
/// How far inside the cutter's rim the ground ends (m), and how far under it, as in OMSI's
/// `.o3d.hole` files.
const RIM_INSET: f64 = 0.03;
const RIM_DROP: f64 = 0.003;

/// What is left of a tile's ground where `[terrainhole]` rims cut it.
#[derive(Debug, Clone, Default)]
pub struct TerrainCut {
    cells: usize,
    cell: f64,
    /// Per cut grid cell (`j * cells + i`): what is left of its two triangles, tile metres,
    /// counter-clockwise seen from above like [`crate::build_terrain_mesh`]'s.
    pieces: HashMap<usize, Vec<[DVec3; 3]>>,
}

impl TerrainCut {
    /// Cut `t` along `rims`: closed rings in tile metres, each corner at the height of the
    /// cutter's rim there. None when nothing of the tile is cut.
    pub fn new(t: &Terrain, rims: &[Vec<glam::Vec3>]) -> Option<TerrainCut> {
        Self::with_spline_outlines(t, rims, &[])
    }

    /// Combine object rims (inset here) with spline troughs (already inset by their
    /// profiles), so overlapping cuts share the same cell boundary and triangulation.
    pub fn with_spline_outlines(
        t: &Terrain,
        rims: &[Vec<glam::Vec3>],
        splines: &[&[glam::Vec3]],
    ) -> Option<TerrainCut> {
        let n = t.cells;
        if n == 0 {
            return None;
        }
        let cell = tile_size() as f64 / n as f64;
        // Intersections stay in f64 until mesh upload: f32 split endpoints can land
        // on different weld keys and leave artificial vertices in a cell.
        // Per cell: what is left of its two triangles, as convex pieces
        let mut polys: HashMap<usize, Vec<Vec<DVec2>>> = HashMap::new();
        let mut segments: Vec<(DVec3, DVec3)> = Vec::new();
        for (rim, object) in rims
            .iter()
            .map(|r| (r.as_slice(), true))
            .chain(splines.iter().map(|r| (*r, false)))
        {
            let mut ring: Vec<DVec3> = Vec::with_capacity(rim.len());
            for p in rim {
                if ring
                    .last()
                    .is_none_or(|q: &DVec3| q.truncate() != p.truncate().as_dvec2())
                {
                    ring.push(p.as_dvec3());
                }
            }
            while ring.len() > 1 && ring[0].truncate() == ring[ring.len() - 1].truncate() {
                ring.pop();
            }
            if ring.len() < 3 {
                continue;
            }
            let ring = if object { inset(&ring) } else { ring };
            let flat: Vec<DVec2> = ring.iter().map(|p| p.truncate()).collect();
            let (lo, hi) = flat.iter().fold(
                (DVec2::splat(f64::MAX), DVec2::splat(f64::MIN)),
                |(lo, hi), p| (lo.min(*p), hi.max(*p)),
            );
            if hi.x < 0.0 || hi.y < 0.0 || lo.x > tile_size() as f64 || lo.y > tile_size() as f64 {
                continue;
            }
            let parts = ear_clip(&flat);
            if parts.is_empty() {
                continue;
            }
            for k in 0..ring.len() {
                segments.push((ring[k], ring[(k + 1) % ring.len()]));
            }
            // (one cell more all round: the cell beside shares the corners put on its edge)
            let span = |a: f64, b: f64| {
                let i0 = ((a / cell).floor() as i64 - 1).clamp(0, n as i64 - 1) as usize;
                let i1 = ((b / cell).floor() as i64 + 1).clamp(0, n as i64 - 1) as usize;
                i0..=i1
            };
            for j in span(lo.y, hi.y) {
                for i in span(lo.x, hi.x) {
                    let cut = polys
                        .entry(j * n + i)
                        .or_insert_with(|| grid_cell(i, j, cell));
                    for c in &parts {
                        let mut rest = Vec::with_capacity(cut.len());
                        for q in cut.drain(..) {
                            subtract(q, c, &mut rest);
                        }
                        *cut = rest;
                    }
                }
            }
        }
        if polys.is_empty() {
            return None;
        }
        // every corner of the pieces by cell, to close the T-junctions with
        let key = |p: DVec2| ((p.x * 1e4).round() as i64, (p.y * 1e4).round() as i64);
        let mut corners: HashMap<(i64, i64), Vec<DVec2>> = HashMap::new();
        let mut seen = std::collections::HashSet::new();
        // (and the rim's: one in line seen from above can still bend in height)
        let rim_corners: Vec<DVec2> = segments.iter().map(|(a, _)| a.truncate()).collect();
        for p in polys.values().flatten().flatten().chain(&rim_corners) {
            if seen.insert(key(*p)) {
                let at = ((p.x / cell).floor() as i64, (p.y / cell).floor() as i64);
                corners.entry(at).or_default().push(*p);
            }
        }
        let height = |p: DVec2| -> f64 {
            rim_height(&segments, p).unwrap_or_else(|| t.sample(p.x as f32, p.y as f32) as f64)
        };
        // OMSI's corners: the grid's, the rim's, and the rim's crossings with grid lines
        let on_grid = |v: f64| (v / cell - (v / cell).round()).abs() * cell < 1e-3;
        let is_corner = |p: DVec2| {
            (on_grid(p.x) && on_grid(p.y))
                || rim_corners.iter().any(|c| c.distance(p) < 1e-3)
                || ((on_grid(p.x) || on_grid(p.y)) && rim_height(&segments, p).is_some())
        };
        let mut pieces = HashMap::new();
        for (k, cut) in polys {
            let (i, j) = ((k % n) as i64, (k / n) as i64);
            let mut tris = Vec::new();
            let q: Vec<Vec<DVec3>> = cut
                .iter()
                .map(|q| {
                    with_corners_on_edges(q, |f: &mut dyn FnMut(DVec2)| {
                        for dj in -1..=1 {
                            for di in -1..=1 {
                                for p in corners.get(&(i + di, j + dj)).into_iter().flatten() {
                                    f(*p);
                                }
                            }
                        }
                    })
                    .iter()
                    .map(|p| p.extend(height(*p)))
                    .collect()
                })
                .collect();
            // (a cell the cut does not reach keeps its triangles, unless a corner of the
            // cell beside lies on their edges)
            let left: f64 = cut.iter().map(|q| area(q)).sum();
            if (left - cell * cell).abs() < 1e-3
                && q.iter()
                    .flatten()
                    .all(|p| (p.z - t.sample(p.x as f32, p.y as f32) as f64).abs() < 1e-4)
            {
                continue;
            }
            {
                // the cell as a whole (OMSI drops a cut cell's diagonal), from OMSI's corners
                match retriangulate(&q, is_corner, |p: DVec2| on_grid(p.x) && on_grid(p.y)) {
                    Some(t) => tris.extend(t),
                    None => tris.extend(q.iter().flat_map(|q| fan_convex(q))),
                }
            }
            let (i, j) = (i as usize, j as usize);
            if !same_as_grid(&tris, t, i, j, cell) {
                pieces.insert(k, tris);
            }
        }
        if pieces.is_empty() {
            return None;
        }
        Some(TerrainCut {
            cells: n,
            cell,
            pieces,
        })
    }

    /// The ground's height at tile metres (x, y): None where the cut leaves the cell as the
    /// grid has it, Some(None) where it took the ground away.
    pub fn height(&self, x: f32, y: f32) -> Option<Option<f32>> {
        let (x, y) = (x as f64, y as f64);
        let tris = self.cell_at(x, y)?;
        Some(
            tris.iter()
                .find_map(|t| on_triangle(t, DVec2::new(x, y)))
                .map(|h| h as f32),
        )
    }

    /// Upward normal of the cut ground at (x, y), if there is cut ground there.
    pub fn normal(&self, x: f32, y: f32) -> Option<glam::Vec3> {
        let (x, y) = (x as f64, y as f64);
        let p = DVec2::new(x, y);
        let t = self
            .cell_at(x, y)?
            .iter()
            .find(|t| on_triangle(t, p).is_some())?;
        Some(
            (t[1] - t[0])
                .cross(t[2] - t[0])
                .normalize_or_zero()
                .as_vec3(),
        )
    }

    fn cell_at(&self, x: f64, y: f64) -> Option<&Vec<[DVec3; 3]>> {
        let n = self.cells as f64;
        let i = (x / self.cell).clamp(0.0, n - 1e-4) as usize;
        let j = (y / self.cell).clamp(0.0, n - 1e-4) as usize;
        self.pieces.get(&(j * self.cells + i))
    }

    /// Approximate heap storage for the cut cells and their triangles.
    pub fn heap_bytes(&self) -> usize {
        self.pieces.capacity() * std::mem::size_of::<(usize, Vec<[DVec3; 3]>)>()
            + self
                .pieces
                .values()
                .map(|t| t.capacity() * std::mem::size_of::<[DVec3; 3]>())
                .sum::<usize>()
    }

    /// How many grid cells the cut changed.
    pub fn cells_cut(&self) -> usize {
        self.pieces.len()
    }

    /// The tile's terrain mesh with the cut cells drawn as the cut leaves them (see
    /// [`crate::build_terrain_mesh`]).
    pub fn mesh(&self, t: &Terrain) -> MeshData {
        let n = t.samples();
        let mut mesh = MeshData::default();
        let push = |mesh: &mut MeshData, p: DVec3| {
            mesh.positions.push(p.as_vec3());
            mesh.uvs.push(glam::Vec2::new(
                (p.x / tile_size() as f64) as f32,
                (p.y / tile_size() as f64) as f32,
            ));
            mesh.normals.push(glam::Vec3::Z);
            mesh.positions.len() as u32 - 1
        };
        for j in 0..n {
            for i in 0..n {
                let p = DVec3::new(
                    i as f64 * self.cell,
                    j as f64 * self.cell,
                    t.height_at(i, j) as f64,
                );
                push(&mut mesh, p);
            }
        }
        // a corner on a grid point is that point (one normal for the ground on both sides)
        let mut extra: HashMap<(i64, i64), u32> = HashMap::new();
        let mut index = |mesh: &mut MeshData, p: DVec3| -> u32 {
            let (fi, fj) = (p.x / self.cell, p.y / self.cell);
            let (gi, gj) = (fi.round(), fj.round());
            if (fi - gi).abs() * self.cell < 1e-4
                && (fj - gj).abs() * self.cell < 1e-4
                && gi >= 0.0
                && gj >= 0.0
                && (gi as usize) < n
                && (gj as usize) < n
                && (t.height_at(gi as usize, gj as usize) as f64 - p.z).abs() < 1e-4
            {
                return (gj as usize * n + gi as usize) as u32;
            }
            let k = ((p.x * 1e4).round() as i64, (p.y * 1e4).round() as i64);
            *extra.entry(k).or_insert_with(|| push(mesh, p))
        };
        for j in 0..n - 1 {
            for i in 0..n - 1 {
                if let Some(tris) = self.pieces.get(&(j * self.cells + i)) {
                    for t in tris {
                        let k = t.map(|p| index(&mut mesh, p));
                        mesh.indices.extend_from_slice(&k);
                    }
                    continue;
                }
                let a = (j * n + i) as u32;
                let b = a + 1;
                let c = a + n as u32;
                let d = c + 1;
                mesh.indices.extend_from_slice(&[a, d, c, a, b, d]);
            }
        }
        mesh.ranges.push((0, mesh.indices.len() as u32, 0));
        compute_normals(&mut mesh);
        mesh
    }
}

/// The two triangles of grid cell (i, j), as [`crate::build_terrain_mesh`] splits it.
fn grid_cell(i: usize, j: usize, cell: f64) -> Vec<Vec<DVec2>> {
    let at = |di: usize, dj: usize| DVec2::new((i + di) as f64 * cell, (j + dj) as f64 * cell);
    let (a, b, c, d) = (at(0, 0), at(1, 0), at(0, 1), at(1, 1));
    vec![vec![a, d, c], vec![a, b, d]]
}

fn same_as_grid(tris: &[[DVec3; 3]], t: &Terrain, i: usize, j: usize, cell: f64) -> bool {
    if tris.len() != 2 {
        return false;
    }
    let grid = grid_cell(i, j, cell);
    tris.iter().zip(&grid).all(|(a, b)| {
        a.iter().zip(b).all(|(p, q)| {
            p.truncate().distance(*q) < 1e-5
                && (p.z - t.sample(q.x as f32, q.y as f32) as f64).abs() < 1e-4
        })
    })
}

/// Triangles of the region the convex counter-clockwise `pieces` cover together, without
/// the corners in line with their neighbours that are not `corner`s. None when its outline
/// cannot be followed.
fn retriangulate(
    pieces: &[Vec<DVec3>],
    corner: impl Fn(DVec2) -> bool,
    grid: impl Fn(DVec2) -> bool,
) -> Option<Vec<[DVec3; 3]>> {
    if pieces.is_empty() {
        return Some(Vec::new());
    }
    // Weld by distance rather than rounded coordinate keys: two endpoints on
    // opposite sides of a rounding boundary must still cancel their shared edge.
    let mut vertices: Vec<DVec3> = Vec::new();
    for p in pieces.iter().flatten() {
        if !vertices
            .iter()
            .any(|q| q.truncate().distance_squared(p.truncate()) < 1e-8)
        {
            vertices.push(*p);
        }
    }
    let key = |p: DVec3| {
        vertices
            .iter()
            .position(|q| q.truncate().distance_squared(p.truncate()) < 1e-8)
            .unwrap()
    };
    // the outline: the pieces' edges that no other piece runs back along
    let mut edges: Vec<(DVec3, DVec3)> = Vec::new();
    for q in pieces {
        for k in 0..q.len() {
            let (a, b) = (q[k], q[(k + 1) % q.len()]);
            if key(a) != key(b) {
                edges.push((vertices[key(a)], vertices[key(b)]));
            }
        }
    }
    // (an edge by its ends, to 0.1 mm)
    type Edge = (usize, usize);
    let mut back: HashMap<Edge, usize> = HashMap::new();
    for (a, b) in &edges {
        *back.entry((key(*a), key(*b))).or_default() += 1;
    }
    let mut outline: Vec<(DVec3, DVec3)> = Vec::new();
    for (a, b) in &edges {
        let (own, rev) = ((key(*a), key(*b)), (key(*b), key(*a)));
        if back[&own] > 0 && back.get(&rev).is_some_and(|n| *n > 0) {
            *back.get_mut(&own).unwrap() -= 1;
            *back.get_mut(&rev).unwrap() -= 1;
        }
    }
    for (a, b) in &edges {
        let k = (key(*a), key(*b));
        if let Some(n) = back.get_mut(&k)
            && *n > 0
        {
            *n -= 1;
            outline.push((*a, *b));
        }
    }
    // chained into loops
    let mut from: HashMap<usize, Vec<usize>> = HashMap::new();
    for (k, (a, _)) in outline.iter().enumerate() {
        from.entry(key(*a)).or_default().push(k);
    }
    let mut used = vec![false; outline.len()];
    let mut loops: Vec<Vec<DVec3>> = Vec::new();
    for s in 0..outline.len() {
        if used[s] {
            continue;
        }
        let mut lp = Vec::new();
        let mut e = s;
        loop {
            used[e] = true;
            let (a, b) = outline[e];
            lp.push(a);
            if key(b) == key(outline[s].0) {
                break;
            }
            // (where two loops touch: the turn furthest to the right keeps them apart)
            let d = (b - a).truncate();
            let next = from
                .get(&key(b))?
                .iter()
                .copied()
                .filter(|&k| !used[k])
                .min_by(|&x, &y| {
                    let turn = |k: usize| {
                        let o = (outline[k].1 - outline[k].0).truncate();
                        d.perp_dot(o).atan2(d.dot(o))
                    };
                    turn(x).total_cmp(&turn(y))
                })?;
            e = next;
            if lp.len() > outline.len() {
                return None;
            }
        }
        // (a split line's end on the cell's edge keeps the old ground's height there: kept,
        // it stood up as a wall beside a portal)
        let mut changed = true;
        while changed && lp.len() > 3 {
            changed = false;
            for k in 0..lp.len() {
                let m = lp.len();
                let (a, b, c) = (lp[(k + m - 1) % m], lp[k], lp[(k + 1) % m]);
                let (ab, bc) = ((b - a).truncate(), (c - b).truncate());
                let ac = ab + bc;
                if ab.perp_dot(bc).abs() > 1e-4 * ac.length() || ab.dot(bc) <= 0.0 {
                    continue;
                }
                let s = ab.length() / ac.length().max(1e-9);
                if !corner(b.truncate()) || (a.z + (c.z - a.z) * s - b.z).abs() < 1e-3 {
                    lp.remove(k);
                    changed = true;
                    break;
                }
            }
        }
        loops.push(lp);
    }
    // holes (a cut wholly inside the cell) joined to the loop round them
    let flat = |l: &[DVec3]| l.iter().map(|p| p.truncate()).collect::<Vec<_>>();
    let (mut outer, holes): (Vec<Vec<DVec3>>, Vec<Vec<DVec3>>) =
        loops.into_iter().partition(|l| area(&flat(l)) > 0.0);
    if holes.is_empty() {
        let mut out = Vec::new();
        for o in &outer {
            match hub_fan(o, &grid) {
                Some(t) => out.extend(t),
                None => out.extend(ear_clip_3d(o)?),
            }
        }
        return Some(out);
    }
    for h in holes {
        let hp = flat(&h);
        let c = hp[0];
        let host = outer.iter_mut().find(|o| point_in(&flat(o), c))?;
        *host = bridge(host, &h)?;
    }
    let mut out = Vec::new();
    for o in &outer {
        out.extend(ear_clip_3d(o)?);
    }
    Some(out)
}

fn point_in(poly: &[DVec2], p: DVec2) -> bool {
    let mut inside = false;
    for k in 0..poly.len() {
        let (a, b) = (poly[k], poly[(k + 1) % poly.len()]);
        if (a.y > p.y) != (b.y > p.y) && p.x < a.x + (p.y - a.y) / (b.y - a.y) * (b.x - a.x) {
            inside = !inside;
        }
    }
    inside
}

fn crosses(a: DVec2, b: DVec2, c: DVec2, d: DVec2) -> bool {
    let o = |p: DVec2, q: DVec2, r: DVec2| (q - p).perp_dot(r - p);
    let (d1, d2, d3, d4) = (o(a, b, c), o(a, b, d), o(c, d, a), o(c, d, b));
    d1 * d2 < 0.0 && d3 * d4 < 0.0
}

/// The clockwise `hole` spliced into the counter-clockwise `outer` along a cut from the
/// hole's corner furthest east to the nearest corner of `outer` it sees.
fn bridge(outer: &[DVec3], hole: &[DVec3]) -> Option<Vec<DVec3>> {
    let ih = (0..hole.len()).max_by(|&a, &b| hole[a].x.total_cmp(&hole[b].x))?;
    let h = hole[ih].truncate();
    let walls: Vec<(DVec2, DVec2)> = [outer, hole]
        .iter()
        .flat_map(|l| (0..l.len()).map(move |k| (l[k].truncate(), l[(k + 1) % l.len()].truncate())))
        .collect();
    let io = (0..outer.len())
        .filter(|&k| {
            let o = outer[k].truncate();
            walls.iter().all(|(a, b)| !crosses(h, o, *a, *b))
        })
        .min_by(|&a, &b| {
            outer[a]
                .truncate()
                .distance(h)
                .total_cmp(&outer[b].truncate().distance(h))
        })?;
    let mut out: Vec<DVec3> = outer[..=io].to_vec();
    out.extend(hole[ih..].iter().chain(&hole[..=ih]).copied());
    out.extend(outer[io..].iter().copied());
    Some(out)
}

/// Triangles of the counter-clockwise polygon `poly` as OMSI lays them in a cut cell: going
/// round clockwise, fanned from a hub - first the rim's last corner before the `grid`
/// corners - while each triangle is an ear, else the hub moves one corner back. None when
/// it runs out of ears.
fn hub_fan(poly: &[DVec3], grid: impl Fn(DVec2) -> bool) -> Option<Vec<[DVec3; 3]>> {
    // clockwise, as OMSI goes round
    let mut v: Vec<DVec3> = poly.iter().rev().copied().collect();
    let m = v.len();
    let mut h = (0..m)
        .find(|&k| !grid(v[k].truncate()) && grid(v[(k + 1) % m].truncate()))
        .unwrap_or(m - 1);
    let ear = |v: &[DVec3], t: usize| {
        let m = v.len();
        let (a, b, c) = (
            v[(t + m - 1) % m].truncate(),
            v[t].truncate(),
            v[(t + 1) % m].truncate(),
        );
        // (under 1e-4 rad is no turn: rounding)
        if (b - a).perp_dot(c - b) >= -1e-4 * (b - a).length() * (c - b).length() {
            return false;
        }
        v.iter().all(|p| {
            let p = p.truncate();
            p == a || p == b || p == c || !in_triangle(p, a, b, c, -1.0)
        })
    };
    let mut out = Vec::new();
    let mut misses = 0;
    while v.len() > 3 {
        let m = v.len();
        let t = (h + 1) % m;
        if ear(&v, t) {
            // (counter-clockwise again for the mesh)
            out.push([v[h], v[(t + 1) % m], v[t]]);
            v.remove(t);
            if t < h {
                h -= 1;
            }
            misses = 0;
        } else {
            misses += 1;
            if misses > m {
                return None;
            }
            h = (h + m - 1) % m;
        }
    }
    let m = v.len();
    if m == 3 && area(&v.iter().map(|p| p.truncate()).collect::<Vec<_>>()).abs() > MIN_AREA {
        out.push([v[h % m], v[(h + 2) % m], v[(h + 1) % m]]);
    }
    Some(out)
}

/// Whether `p` lies in or on (within 0.1 mm) the triangle a, b, c, counter-clockwise for
/// `sign` 1, clockwise for -1.
fn in_triangle(p: DVec2, a: DVec2, b: DVec2, c: DVec2, sign: f64) -> bool {
    let side = |a: DVec2, b: DVec2| sign * (b - a).perp_dot(p - a) >= -1e-4 * (b - a).length();
    side(a, b) && side(b, c) && side(c, a)
}

/// Triangles of a counter-clockwise polygon, every corner kept. None when no ear is left.
fn ear_clip_3d(poly: &[DVec3]) -> Option<Vec<[DVec3; 3]>> {
    let mut v = poly.to_vec();
    let mut out = Vec::new();
    while v.len() > 3 {
        let m = v.len();
        let ear = (0..m).find(|&k| {
            let (a, b, c) = (
                v[(k + m - 1) % m].truncate(),
                v[k].truncate(),
                v[(k + 1) % m].truncate(),
            );
            if (b - a).perp_dot(c - b) <= 1e-4 * (b - a).length() * (c - b).length() {
                return false;
            }
            v.iter().all(|p| {
                let p = p.truncate();
                p == a || p == b || p == c || !in_triangle(p, a, b, c, 1.0)
            })
        })?;
        out.push([v[(ear + m - 1) % m], v[ear], v[(ear + 1) % m]]);
        v.remove(ear);
    }
    if v.len() == 3 && area(&v.iter().map(|p| p.truncate()).collect::<Vec<_>>()) > MIN_AREA {
        out.push([v[0], v[1], v[2]]);
    }
    Some(out)
}

/// The height of the rim where `p` lies on it.
fn rim_height(segments: &[(DVec3, DVec3)], p: DVec2) -> Option<f64> {
    let mut best: Option<(f64, f64)> = None;
    for (a, b) in segments {
        let (a2, b2) = (a.truncate(), b.truncate());
        let d = b2 - a2;
        let len2 = d.length_squared();
        let s = if len2 > 0.0 {
            ((p - a2).dot(d) / len2).clamp(0.0, 1.0)
        } else {
            0.0
        };
        let dist = (a2 + d * s).distance(p);
        if dist < ON_RIM && best.is_none_or(|(bd, _)| dist < bd) {
            best = Some((dist, a.z + (b.z - a.z) * s));
        }
    }
    best.map(|(_, z)| z)
}

/// `ring`'s edges moved [`RIM_INSET`] inwards and [`RIM_DROP`] down.
fn inset(ring: &[DVec3]) -> Vec<DVec3> {
    let flat: Vec<DVec2> = ring.iter().map(|p| p.truncate()).collect();
    let sign = if area(&flat) < 0.0 { -1.0 } else { 1.0 };
    let m = ring.len();
    (0..m)
        .map(|k| {
            let (a, b, c) = (flat[(k + m - 1) % m], flat[k], flat[(k + 1) % m]);
            let n1 = (b - a).normalize_or_zero().perp();
            let n2 = (c - b).normalize_or_zero().perp();
            let bis = (n1 + n2).normalize_or_zero();
            // (a needle-sharp corner: at most four times the inset)
            let cos = bis.dot(n1).max(0.25);
            let p = b + bis * (sign * RIM_INSET / cos);
            p.extend(ring[k].z - RIM_DROP)
        })
        .collect()
}

fn area(poly: &[DVec2]) -> f64 {
    let mut a = 0.0;
    for k in 0..poly.len() {
        a += poly[k].perp_dot(poly[(k + 1) % poly.len()]);
    }
    a * 0.5
}

/// `poly` split by the line through `a` and `b`: (the part left of it, the part right).
fn split(poly: &[DVec2], a: DVec2, b: DVec2) -> (Vec<DVec2>, Vec<DVec2>) {
    let d = (b - a).normalize_or_zero();
    let side = |p: DVec2| d.perp_dot(p - a);
    let (mut left, mut right) = (Vec::new(), Vec::new());
    for k in 0..poly.len() {
        let (p, q) = (poly[k], poly[(k + 1) % poly.len()]);
        let (sp, sq) = (side(p), side(q));
        if sp >= -ON_LINE {
            left.push(p);
        }
        if sp <= ON_LINE {
            right.push(p);
        }
        if (sp > ON_LINE && sq < -ON_LINE) || (sp < -ON_LINE && sq > ON_LINE) {
            let x = p + (q - p) * (sp / (sp - sq));
            left.push(x);
            right.push(x);
        }
    }
    (left, right)
}

/// Add what is left of the convex polygon `q` without the counter-clockwise triangle `c` to
/// `out`, as convex pieces.
fn subtract(q: Vec<DVec2>, c: &[DVec2; 3], out: &mut Vec<Vec<DVec2>>) {
    let d = |k: usize| (c[(k + 1) % 3] - c[k]).normalize_or_zero();
    // apart (an edge of either has the other wholly outside): untouched, not split
    let m = q.len();
    let apart = (0..3).any(|k| q.iter().all(|p| d(k).perp_dot(*p - c[k]) <= ON_LINE))
        || (0..m).any(|k| {
            let e = (q[(k + 1) % m] - q[k]).normalize_or_zero();
            c.iter().all(|p| e.perp_dot(*p - q[k]) <= ON_LINE)
        });
    if apart {
        out.push(q);
        return;
    }
    let mut rest = q;
    for k in 0..3 {
        let (inside, outside) = split(&rest, c[k], c[(k + 1) % 3]);
        if outside.len() >= 3 && area(&outside) > MIN_AREA {
            out.push(outside);
        }
        rest = inside;
        if rest.len() < 3 || area(&rest) <= MIN_AREA {
            return;
        }
    }
}

/// `q` with the corners lying on its edges (between their ends) put in.
fn with_corners_on_edges(q: &[DVec2], each: impl Fn(&mut dyn FnMut(DVec2))) -> Vec<DVec2> {
    let mut out = Vec::with_capacity(q.len());
    for k in 0..q.len() {
        let (a, b) = (q[k], q[(k + 1) % q.len()]);
        out.push(a);
        let d = b - a;
        let len = d.length();
        if len <= 0.0 {
            continue;
        }
        let dir = d / len;
        let mut on: Vec<(f64, DVec2)> = Vec::new();
        each(&mut |p: DVec2| {
            let s = (p - a).dot(dir);
            if s > 1e-4 && s < len - 1e-4 && dir.perp_dot(p - a).abs() < 1e-4 {
                on.push((s, p));
            }
        });
        on.sort_by(|x, y| x.0.total_cmp(&y.0));
        on.dedup_by(|x, y| (x.0 - y.0).abs() < 1e-4);
        out.extend(on.into_iter().map(|(_, p)| p));
    }
    out
}

/// Triangles of a convex counter-clockwise polygon, every corner kept: with corners in line
/// on its edges, fanned from its middle at their mean height.
fn fan_convex(q: &[DVec3]) -> Vec<[DVec3; 3]> {
    let m = q.len();
    if m < 3 {
        return Vec::new();
    }
    let flat = |k: usize| q[k % m].truncate();
    let in_line = (0..m).any(|k| {
        let (a, b, c) = (flat(k + m - 1), flat(k), flat(k + 1));
        (b - a).perp_dot(c - b).abs() <= 1e-4 * (b - a).length() * (c - b).length()
    });
    let mut out = Vec::new();
    if !in_line {
        for k in 1..m - 1 {
            out.push([q[0], q[k], q[k + 1]]);
        }
        return out;
    }
    let mid = q.iter().copied().sum::<DVec3>() / m as f64;
    for k in 0..m {
        let t = [mid, q[k], q[(k + 1) % m]];
        if area(&t.map(|p| p.truncate())) > MIN_AREA {
            out.push(t);
        }
    }
    out
}

/// Triangles of the simple polygon `ring` (either way round), counter-clockwise; empty when
/// the ring is not simple.
fn ear_clip(ring: &[DVec2]) -> Vec<[DVec2; 3]> {
    let mut v: Vec<DVec2> = ring.to_vec();
    if area(&v) < 0.0 {
        v.reverse();
    }
    let mut out = Vec::new();
    while v.len() > 3 {
        let m = v.len();
        let mut clipped = false;
        for k in 0..m {
            let (a, b, c) = (v[(k + m - 1) % m], v[k], v[(k + 1) % m]);
            let turn = (b - a).perp_dot(c - b);
            if turn.abs() <= 1e-4 * (b - a).length() * (c - b).length() {
                // (in line: the outline is the same without it)
                v.remove(k);
                clipped = true;
                break;
            }
            if turn < 0.0 {
                continue;
            }
            let blocked = v.iter().any(|&p| {
                p != a
                    && p != b
                    && p != c
                    && (b - a).perp_dot(p - a) > -ON_LINE
                    && (c - b).perp_dot(p - b) > -ON_LINE
                    && (a - c).perp_dot(p - c) > -ON_LINE
            });
            if blocked {
                continue;
            }
            out.push([a, b, c]);
            v.remove(k);
            clipped = true;
            break;
        }
        if !clipped {
            return Vec::new();
        }
    }
    if v.len() == 3 && area(&v) > MIN_AREA {
        out.push([v[0], v[1], v[2]]);
    }
    out
}

/// The height of triangle `t` over `p`, if `p` lies on it (seen from above).
fn on_triangle(t: &[DVec3; 3], p: DVec2) -> Option<f64> {
    let (a, b, c) = (t[0].truncate(), t[1].truncate(), t[2].truncate());
    let det = (b - a).perp_dot(c - a);
    if det.abs() < 1e-9 {
        return None;
    }
    let u = (p - a).perp_dot(c - a) / det;
    let v = (b - a).perp_dot(p - a) / det;
    const E: f64 = 1e-5;
    (u >= -E && v >= -E && u + v <= 1.0 + E)
        .then(|| t[0].z + (t[1].z - t[0].z) * u + (t[2].z - t[0].z) * v)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn clip(t: &Terrain, rims: &[Vec<DVec3>]) -> Option<TerrainCut> {
        let rims: Vec<_> = rims
            .iter()
            .map(|r| r.iter().map(|p| p.as_vec3()).collect())
            .collect();
        TerrainCut::new(t, &rims)
    }
    fn clip_splines(t: &Terrain, rims: &[Vec<DVec3>], splines: &[&[DVec3]]) -> Option<TerrainCut> {
        let rims: Vec<_> = rims
            .iter()
            .map(|r| r.iter().map(|p| p.as_vec3()).collect())
            .collect();
        let splines: Vec<Vec<_>> = splines
            .iter()
            .map(|r| r.iter().map(|p| p.as_vec3()).collect())
            .collect();
        TerrainCut::with_spline_outlines(
            t,
            &rims,
            &splines.iter().map(|r| r.as_slice()).collect::<Vec<_>>(),
        )
    }
    fn flat(cells: usize, h: f64) -> Terrain {
        Terrain {
            cells,
            heights: vec![h as f32; (cells + 1) * (cells + 1)],
        }
    }

    /// Area of the ground left in the cut cells.
    fn ground_area(cut: &TerrainCut) -> f64 {
        cut.pieces
            .values()
            .flatten()
            .map(|t| area(&t.map(|p| p.truncate())))
            .sum()
    }

    /// The part inside the rim goes, and the ground meets the (inset) rim at its height.
    #[test]
    fn the_ground_ends_on_the_rim() {
        // 5 m cells, ground at 10 m; a 4 x 3 m cutter whose rim lies at 7 m
        let t = flat(60, 10.0);
        let rim: Vec<DVec3> = [
            (101.0, 101.0),
            (105.0, 101.0),
            (105.0, 104.0),
            (101.0, 104.0),
        ]
        .iter()
        .map(|&(x, y)| DVec3::new(x, y, 7.0))
        .collect();
        let cut = clip(&t, &[rim]).unwrap();
        // inside: no ground; on the rim: the rim's height; a grid point: its own
        assert_eq!(cut.height(103.0, 102.5), Some(None));
        let h = cut.height((105.0 - RIM_INSET) as f32, 102.0).unwrap().unwrap();
        assert!((h as f64 - (7.0 - RIM_DROP)).abs() < 1e-3, "{h}");
        assert!((cut.height(100.0, 100.0).unwrap().unwrap() - 10.0).abs() < 1e-3);
        // halfway from the rim to the grid point at (100, 100) beyond it
        let h = cut.height(100.5, 102.0).unwrap().unwrap();
        assert!(h > 7.0 && h < 10.0, "{h}");
        // the cells it changed are whole but for the hole
        let hole = (4.0 - 2.0 * RIM_INSET) * (3.0 - 2.0 * RIM_INSET);
        let cells = cut.pieces.len() as f64 * 25.0;
        assert!(
            (ground_area(&cut) - (cells - hole)).abs() < 1e-2,
            "{}",
            ground_area(&cut)
        );
        // far away: the grid's own
        assert_eq!(cut.height(200.0, 200.0), None);
    }

    /// A concave rim (an L) cuts exactly its shape, without cracks.
    #[test]
    fn a_concave_rim_cuts_its_shape_without_cracks() {
        let mut t = flat(60, 0.0);
        for (k, h) in t.heights.iter_mut().enumerate() {
            *h = (k % 61) as f32 * 0.3 + (k / 61) as f32 * 0.1;
        }
        let rim: Vec<DVec3> = [
            (52.0, 52.0, 1.0),
            (63.0, 52.0, 2.0),
            (63.0, 56.0, 2.0),
            (56.0, 56.0, 1.5),
            (56.0, 63.0, 3.0),
            (52.0, 63.0, 1.0),
        ]
        .iter()
        .map(|&(x, y, z)| DVec3::new(x, y, z))
        .collect();
        let cut = clip(&t, &[rim.clone()]).unwrap();
        let rim = inset(&rim);
        let hole = area(&rim.iter().map(|p| p.truncate()).collect::<Vec<_>>());
        assert!((hole - (11.0 * 4.0 + 4.0 * 7.0)).abs() < 1.5);
        let cells = cut.pieces.len() as f64 * 25.0;
        assert!((ground_area(&cut) - (cells - hole)).abs() < 1e-2);
        assert_eq!(cut.height(54.0, 60.0), Some(None));
        assert_eq!(cut.height(60.0, 54.0), Some(None));
        assert!(cut.height(57.0, 57.0).unwrap().is_some());
        assert_eq!(cut.height(62.0, 62.0), None);
        // every edge used by one triangle only lies on the rim or on the cut cells' border
        let mesh = cut.mesh(&t);
        let mut edges: HashMap<(u32, u32), u32> = HashMap::new();
        for tri in mesh.indices.chunks_exact(3) {
            for k in 0..3 {
                let (a, b) = (tri[k], tri[(k + 1) % 3]);
                *edges.entry((a.min(b), a.max(b))).or_default() += 1;
            }
        }
        let on_rim = |p: DVec3| {
            (0..6).any(|k| {
                let (a, b) = (rim[k].truncate(), rim[(k + 1) % 6].truncate());
                let d = b - a;
                let s = ((p.truncate() - a).dot(d) / d.length_squared()).clamp(0.0, 1.0);
                (a + d * s).distance(p.truncate()) < 1e-3
            })
        };
        for ((a, b), n) in edges {
            if n == 1 {
                let (p, q) = (
                    mesh.positions[a as usize].as_dvec3(),
                    mesh.positions[b as usize].as_dvec3(),
                );
                let border = |v: f64| v < 1e-3 || v > 300.0 - 1e-3;
                assert!(
                    (on_rim(p) && on_rim(q))
                        || (border(p.x) && border(q.x))
                        || (border(p.y) && border(q.y)),
                    "open edge {p} - {q}"
                );
            }
        }
    }

    /// The cut ground has OMSI's corners only: the grid's, the rim's (in-line ones too) and
    /// the rim's crossings with grid lines - not with diagonals or split lines.
    #[test]
    fn the_cut_adds_only_the_rims_corners_and_crossings() {
        let mut t = flat(60, 0.0);
        for (k, h) in t.heights.iter_mut().enumerate() {
            *h = 8.0 + (k % 61) as f32 * 0.2;
        }
        // a portal's mouth, turned off the grid: a ramp down to a flat strip along each side
        for (origin, angle) in [
            (DVec2::new(152.3, 151.1), 0.6435_f64),
            (DVec2::new(274.4, 187.9), 1.149_f64),
        ] {
            for reversed in [false, true] {
                for start in 0..6 {
                    let o = origin;
                    let r = DVec2::new(angle.cos(), angle.sin());
                    let f = r.perp();
                    let at = |x: f64, y: f64, z: f64| (o + r * x + f * y).extend(z);
                    let mut rim = vec![
                        at(-7.5, 5.6, 7.2),
                        at(7.5, 5.6, 7.2),
                        at(7.5, 9.1, 0.1),
                        at(7.5, 10.1, 0.1),
                        at(-7.5, 10.1, 0.1),
                        at(-7.5, 9.1, 0.1),
                    ];
                    rim.rotate_left(start);
                    if reversed {
                        rim.reverse();
                    }
                    let cut = clip(&t, &[rim.clone()]).unwrap();
                    let corners: Vec<DVec2> = inset(&rim).iter().map(|p| p.truncate()).collect();
                    let on_grid = |v: f64| (v / 5.0 - (v / 5.0).round()).abs() < 1e-4;
                    let on_rim = |p: DVec2| {
                        (0..6).any(|k| {
                            let (a, b) = (corners[k], corners[(k + 1) % 6]);
                            let d = b - a;
                            let s = ((p - a).dot(d) / d.length_squared()).clamp(0.0, 1.0);
                            (a + d * s).distance(p) < 1e-3
                        })
                    };
                    for p in cut.pieces.values().flatten().flatten() {
                        let q = p.truncate();
                        assert!(
                            (on_grid(q.x) && on_grid(q.y))
                                || corners.iter().any(|c| c.distance(q) < 1e-3)
                                || ((on_grid(q.x) || on_grid(q.y)) && on_rim(q)),
                            "corner {p} is none of OMSI's"
                        );
                    }
                    // Every bend in height survives, even when collinear from above.
                    for c in &inset(&rim) {
                        assert!(
                            cut.pieces
                                .values()
                                .flatten()
                                .flatten()
                                .any(|p| p.distance(*c) < 1e-3)
                        );
                    }
                }
            }
        }
    }

    #[test]
    fn spline_cuts_keep_a_narrow_strip_without_a_second_inset() {
        let t = flat(60, 10.0);
        let rect = |x0, x1| {
            vec![
                DVec3::new(x0, 99.0, 7.0),
                DVec3::new(x1, 99.0, 7.0),
                DVec3::new(x1, 111.0, 7.0),
                DVec3::new(x0, 111.0, 7.0),
            ]
        };
        let (road, kerb) = (rect(101.0, 104.2), rect(104.4, 109.0));
        for rings in [
            [road.as_slice(), kerb.as_slice()],
            [kerb.as_slice(), road.as_slice()],
        ] {
            let cut = clip_splines(&t, &[], &rings).unwrap();
            assert_eq!(cut.height(103.0, 103.0), Some(None));
            assert_eq!(cut.height(107.0, 103.0), Some(None));
            let h = cut.height(104.3, 103.0).unwrap().unwrap();
            assert!((h - 7.0).abs() < 1e-4, "strip: {h}");
            // A second 3 cm inset would incorrectly keep these points.
            assert_eq!(cut.height(101.01, 103.0), Some(None));
            assert_eq!(cut.height(108.99, 103.0), Some(None));
            let mesh = cut.mesh(&t);
            let ray = crate::ray_mesh(
                glam::Vec3::new(104.3, 103.0, 20.0),
                -glam::Vec3::Z,
                &mesh,
                &glam::Mat4::IDENTITY,
            )
            .unwrap();
            assert!((20.0 - ray - h).abs() < 1e-4);
        }
    }

    #[test]
    fn overlapping_object_and_spline_cuts_form_one_hole() {
        let t = flat(60, 10.0);
        let rect = |x0, x1| {
            vec![
                DVec3::new(x0, 101.0, 7.0),
                DVec3::new(x1, 101.0, 7.0),
                DVec3::new(x1, 104.0, 7.0),
                DVec3::new(x0, 104.0, 7.0),
            ]
        };
        let object = rect(101.0, 103.0);
        let spline = rect(102.0, 104.0);
        let cut = clip_splines(&t, &[object], &[&spline]).unwrap();
        for x in [101.5, 102.5, 103.5] {
            assert_eq!(cut.height(x, 102.0), Some(None));
        }
        assert!(cut.height(100.5, 102.0).unwrap().is_some());
        let mesh = cut.mesh(&t);
        for x in [101.5, 102.5, 103.5] {
            assert!(
                crate::ray_mesh(
                    glam::Vec3::new(x as f32, 102.0, 20.0),
                    -glam::Vec3::Z,
                    &mesh,
                    &glam::Mat4::IDENTITY
                )
                .is_none()
            );
        }
    }

    #[test]
    fn split_endpoints_across_a_rounding_boundary_still_share_an_edge() {
        let a = 5.000049;
        let b = 5.000051;
        let point = |x, y| DVec3::new(x, y, 1.0);
        let pieces = [
            vec![
                point(0.0, 0.0),
                point(a, 0.0),
                point(a, 5.0),
                point(0.0, 5.0),
            ],
            vec![
                point(b, 0.0),
                point(10.0, 0.0),
                point(10.0, 5.0),
                point(b, 5.0),
            ],
        ];
        let triangles =
            retriangulate(&pieces, |_| true, |_| true).expect("shared edge must cancel");
        let actual: f64 = triangles
            .iter()
            .map(|t| area(&t.map(|p| p.truncate())))
            .sum();
        assert!((actual - 50.0).abs() < 1e-6);
    }

    #[test]
    fn ear_clip_keeps_the_area() {
        let l = [
            DVec2::new(0.0, 0.0),
            DVec2::new(0.0, 4.0),
            DVec2::new(1.0, 4.0),
            DVec2::new(1.0, 1.0),
            DVec2::new(2.0, 1.0),
            DVec2::new(3.0, 1.0),
            DVec2::new(3.0, 0.0),
        ];
        let tris = ear_clip(&l);
        let a: f64 = tris.iter().map(|t| area(t)).sum();
        assert!((a - 6.0).abs() < 1e-5, "{a}");
        assert!(tris.iter().all(|t| area(t) > 0.0));
    }
}
