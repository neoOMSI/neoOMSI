//! The surface raster of one chunk and the outlines traced from it.

/// Raster cell (m): fine enough for kerbs and traffic islands, coarse enough to close the
/// hairline joints between splines and junction meshes.
pub(crate) const CELL: f64 = 0.2;
/// Side of a chunk (m).
pub(crate) const CHUNK: f64 = 64.0;
/// Cells of a chunk's side.
pub(crate) const N: usize = (CHUNK / CELL) as usize;
/// Cells rasterised around a chunk, so closing and outlines agree across its edges.
pub(crate) const MARGIN: usize = 4;
/// Cells of a raster's side, margin included.
pub(crate) const W: usize = N + 2 * MARGIN;

/// The cells a chunk is rasterised in: their size, how many across the chunk and how many
/// around it.
#[derive(Debug, Clone, Copy)]
pub(crate) struct Grid {
    pub(crate) cell: f64,
    pub(crate) n: usize,
    pub(crate) margin: usize,
}

impl Grid {
    /// Cells of the raster's side, margin included.
    pub(crate) const fn w(self) -> usize {
        self.n + 2 * self.margin
    }
}

/// The surfaces' grid.
pub(crate) const COARSE: Grid = Grid {
    cell: CELL,
    n: N,
    margin: MARGIN,
};
/// The paint's grid: four by four cells to a surface cell, fine enough for a lane line.
pub(crate) const FINE: Grid = Grid {
    cell: CELL / 4.0,
    n: N * 4,
    margin: MARGIN * 4,
};

/// Two surfaces closer in height than this are one level: the class decides which shows.
const SAME_LEVEL: f32 = 0.05;

pub(crate) struct Raster {
    pub(crate) g: Grid,
    pub(crate) class: Vec<u8>,
    pub(crate) z: Vec<f32>,
    /// Paint on the topmost surface: lines, arrows, stop lines, crossings.
    pub(crate) mark: Vec<bool>,
}

/// Where a texture shows road paint: its pixels lighter than the surface it is painted on
/// (white) or yellow, rows from the top as the file stores them.
pub(crate) struct PaintMask {
    pub(crate) w: usize,
    pub(crate) h: usize,
    pub(crate) bits: Vec<bool>,
}

impl PaintMask {
    pub(crate) fn from_rgba(w: usize, h: usize, rgba: &[u8], alpha: bool) -> Option<PaintMask> {
        if w == 0 || h == 0 || rgba.len() < w * h * 4 {
            return None;
        }
        let px = |k: usize| {
            let p = &rgba[k * 4..k * 4 + 4];
            (
                p[0] as f32 / 255.0,
                p[1] as f32 / 255.0,
                p[2] as f32 / 255.0,
                if alpha { p[3] as f32 / 255.0 } else { 1.0 },
            )
        };
        let lum = |(r, g, b, _): (f32, f32, f32, f32)| 0.299 * r + 0.587 * g + 0.114 * b;
        let mut lums: Vec<f32> = (0..w * h)
            .map(px)
            .filter(|p| p.3 >= 0.5)
            .map(lum)
            .collect();
        if lums.is_empty() {
            return None;
        }
        let mid = lums.len() / 2;
        let median = *lums.select_nth_unstable_by(mid, f32::total_cmp).1;
        // a texture that is paint all over (a line on a thin spline, a decal) is light
        // throughout; on asphalt or cobbles the paint stands out of the surface
        let light = if median > 0.62 {
            0.6
        } else {
            (median + 0.28).max(0.5)
        };
        let raw: Vec<bool> = (0..w * h)
            .map(|k| {
                let p = px(k);
                let (r, g, b, a) = p;
                let yellow = r > 0.55 && g > 0.42 && b < 0.4 && r - b > 0.3;
                let grey = (r - g).abs() < 0.18 && (g - b).abs() < 0.18;
                a >= 0.5 && ((lum(p) >= light && grey) || yellow)
            })
            .collect();
        // single specks of light gravel are not paint
        let bits: Vec<bool> = (0..w * h)
            .map(|k| {
                if !raw[k] {
                    return false;
                }
                let (i, j) = (k % w, k / w);
                let n = [
                    raw[j * w + (i + 1) % w],
                    raw[j * w + (i + w - 1) % w],
                    raw[((j + 1) % h) * w + i],
                    raw[((j + h - 1) % h) * w + i],
                ];
                n.iter().filter(|&&b| b).count() >= 2
            })
            .collect();
        bits.iter().any(|&b| b).then_some(PaintMask { w, h, bits })
    }

    fn at(&self, u: f32, v: f32) -> bool {
        if !u.is_finite() || !v.is_finite() {
            return false;
        }
        let i = ((u - u.floor()) * self.w as f32) as usize;
        let j = ((v - v.floor()) * self.h as f32) as usize;
        self.bits[j.min(self.h - 1) * self.w + i.min(self.w - 1)]
    }
}

impl Raster {
    pub(crate) fn new(g: Grid) -> Raster {
        let n = g.w() * g.w();
        Raster {
            g,
            class: vec![0; n],
            z: vec![f32::NEG_INFINITY; n],
            mark: vec![false; n],
        }
    }

    /// Paint a triangle given in cell units (cell (i, j) has its centre at (i + 0.5, j + 0.5))
    /// with class `c` (higher wins on the same level; 0 is nothing) wherever it is the
    /// topmost surface, and the road paint its texture shows (`paint`: the texture's paint
    /// and the corners' texture coordinates).
    pub(crate) fn fill_tri(
        &mut self,
        t: [[f32; 3]; 3],
        c: u8,
        paint: Option<(&PaintMask, [[f32; 2]; 3])>,
    ) {
        let [a, b, d] = t;
        let det = (b[0] - a[0]) * (d[1] - a[1]) - (d[0] - a[0]) * (b[1] - a[1]);
        if det.abs() < 1e-6 {
            return;
        }
        // a value given at the corners as a plane: v = v(a) + gx (x - a.x) + gy (y - a.y)
        let grad = |va: f32, vb: f32, vd: f32| {
            (
                ((vb - va) * (d[1] - a[1]) - (vd - va) * (b[1] - a[1])) / det,
                ((vd - va) * (b[0] - a[0]) - (vb - va) * (d[0] - a[0])) / det,
            )
        };
        let (gx, gy) = grad(a[2], b[2], d[2]);
        let uv = paint.map(|(m, uv)| {
            (
                m,
                uv[0],
                grad(uv[0][0], uv[1][0], uv[2][0]),
                grad(uv[0][1], uv[1][1], uv[2][1]),
            )
        });
        let w = self.g.w();
        let y0 = a[1].min(b[1]).min(d[1]);
        let y1 = a[1].max(b[1]).max(d[1]);
        let j0 = ((y0 - 0.5).ceil().max(0.0)) as usize;
        let j1 = ((y1 - 0.5).floor()).min(w as f32 - 1.0);
        if j1 < 0.0 {
            return;
        }
        let edges = [(a, b), (b, d), (d, a)];
        for j in j0..=j1 as usize {
            let y = j as f32 + 0.5;
            let (mut lo, mut hi) = (f32::INFINITY, f32::NEG_INFINITY);
            for (p, q) in edges {
                if (p[1] <= y && q[1] >= y) || (q[1] <= y && p[1] >= y) {
                    if (q[1] - p[1]).abs() < 1e-9 {
                        lo = lo.min(p[0].min(q[0]));
                        hi = hi.max(p[0].max(q[0]));
                    } else {
                        let x = p[0] + (y - p[1]) * (q[0] - p[0]) / (q[1] - p[1]);
                        lo = lo.min(x);
                        hi = hi.max(x);
                    }
                }
            }
            if lo > hi {
                continue;
            }
            let i0 = ((lo - 0.5).ceil().max(0.0)) as usize;
            let i1 = (hi - 0.5).floor().min(w as f32 - 1.0);
            if i1 < 0.0 {
                continue;
            }
            for i in i0..=i1 as usize {
                let x = i as f32 + 0.5;
                let z = a[2] + gx * (x - a[0]) + gy * (y - a[1]);
                let painted = uv.is_some_and(|(m, uv0, gu, gv)| {
                    let (dx, dy) = (x - a[0], y - a[1]);
                    m.at(uv0[0] + gu.0 * dx + gu.1 * dy, uv0[1] + gv.0 * dx + gv.1 * dy)
                });
                self.put_painted(j * w + i, c, z, painted);
            }
        }
    }

    /// Put class `c` at height `z` into cell `k` where it is the topmost surface.
    pub(crate) fn put(&mut self, k: usize, c: u8, z: f32) {
        self.put_painted(k, c, z, false);
    }

    /// [`Raster::put`] with paint on it or not. Paint on the same level adds to what is
    /// there: the clear parts of a decal leave the lines under them.
    fn put_painted(&mut self, k: usize, c: u8, z: f32, painted: bool) {
        let (cur, cz) = (self.class[k], self.z[k]);
        if cur == 0 || z > cz + SAME_LEVEL {
            self.class[k] = c;
            self.z[k] = z;
            self.mark[k] = painted;
        } else if z > cz - SAME_LEVEL {
            if c > cur {
                self.class[k] = c;
            }
            self.z[k] = cz.max(z);
            self.mark[k] |= painted;
        }
    }

    /// The cells of class `c`.
    #[cfg(test)]
    pub(crate) fn mask(&self, c: u8) -> Vec<bool> {
        self.class.iter().map(|&k| k == c).collect()
    }
}

/// Grow (`grow`) or shrink a mask by `r` cells (a square), separably.
fn morph(m: &[bool], g: Grid, r: usize, grow: bool) -> Vec<bool> {
    let w = g.w();
    let pass = |src: &[bool], horizontal: bool| -> Vec<bool> {
        let mut out = vec![false; w * w];
        for a in 0..w {
            // running count of set cells in the window
            let at = |b: usize| if horizontal { a * w + b } else { b * w + a };
            let mut count = 0usize;
            for b in 0..r.min(w) {
                count += src[at(b)] as usize;
            }
            for b in 0..w {
                if b + r < w {
                    count += src[at(b + r)] as usize;
                }
                if b > r {
                    count -= src[at(b - r - 1)] as usize;
                }
                let span = (b + r).min(w - 1) + 1 - b.saturating_sub(r);
                out[at(b)] = if grow { count > 0 } else { count == span };
            }
        }
        out
    };
    let h = pass(m, true);
    pass(&h, false)
}

/// Close cracks narrower than about `2 r` cells: grow, then shrink.
pub(crate) fn close(m: &[bool], g: Grid, r: usize) -> Vec<bool> {
    if r == 0 {
        return m.to_vec();
    }
    morph(&morph(m, g, r, true), g, r, false)
}

/// A closed outline in metres from the chunk's corner; `outer` rings run counter-clockwise
/// around their surface, holes clockwise.
#[derive(Debug, Clone)]
pub(crate) struct Ring {
    pub(crate) pts: Vec<[f32; 2]>,
    pub(crate) outer: bool,
    pub(crate) area: f32,
}

/// Where an outline point really lies: the nearest edge of the geometry the surface was
/// rasterised from, if one is near (metres from the chunk's corner).
pub(crate) type Snap<'a> = &'a dyn Fn([f32; 2]) -> Option<[f32; 2]>;

/// Trace the outlines of the mask's cells inside the chunk (the margin left out): along the
/// cell edges with the surface on the left, then through the middles of those edges (a
/// marching-squares outline), each point moved onto the true edge `snap` knows of, then
/// simplified to `tol` metres: a straight kerb comes out as one straight line.
pub(crate) fn outlines(m: &[bool], g: Grid, tol: f32, snap: Option<Snap>) -> Vec<Ring> {
    let (w, n, margin) = (g.w(), g.n, g.margin);
    let inside = |i: isize, j: isize| -> bool {
        let lo = margin as isize;
        let hi = (margin + n) as isize;
        i >= lo && i < hi && j >= lo && j < hi && m[j as usize * w + i as usize]
    };
    #[allow(non_snake_case)]
    let V: usize = w + 1;
    // up to two edges leave a corner (where two cells touch diagonally)
    let mut out: Vec<[u32; 2]> = vec![[u32::MAX; 2]; V * V];
    let mut edges: Vec<(u32, u8)> = Vec::new();
    let push = |out: &mut Vec<[u32; 2]>, edges: &mut Vec<(u32, u8)>, from: usize, dir: u8| {
        let e = edges.len() as u32;
        edges.push((from as u32, dir));
        let slot = &mut out[from];
        if slot[0] == u32::MAX {
            slot[0] = e;
        } else {
            slot[1] = e;
        }
    };
    // directions: 0 +x, 1 +y, 2 -x, 3 -y
    for j in margin..margin + n {
        for i in margin..margin + n {
            if !m[j * w + i] {
                continue;
            }
            let (ii, jj) = (i as isize, j as isize);
            if !inside(ii, jj - 1) {
                push(&mut out, &mut edges, j * V + i, 0);
            }
            if !inside(ii + 1, jj) {
                push(&mut out, &mut edges, j * V + i + 1, 1);
            }
            if !inside(ii, jj + 1) {
                push(&mut out, &mut edges, (j + 1) * V + i + 1, 2);
            }
            if !inside(ii - 1, jj) {
                push(&mut out, &mut edges, (j + 1) * V + i, 3);
            }
        }
    }
    let step = |v: usize, d: u8| -> usize {
        match d {
            0 => v + 1,
            1 => v + V,
            2 => v - 1,
            _ => v - V,
        }
    };
    let mut used = vec![false; edges.len()];
    let mut rings = Vec::new();
    let origin = margin as f32;
    for start in 0..edges.len() {
        if used[start] {
            continue;
        }
        let mut corners: Vec<[f32; 2]> = Vec::new();
        let mut e = start;
        loop {
            used[e] = true;
            let (from, d) = edges[e];
            let from = from as usize;
            let (fx, fy) = ((from % V) as f32, (from / V) as f32);
            let to = step(from, d);
            let (tx, ty) = ((to % V) as f32, (to / V) as f32);
            corners.push([(fx + tx) * 0.5 - origin, (fy + ty) * 0.5 - origin]);
            // turn left where two ways leave a corner: cells touching at a corner stay apart
            let slot = out[to];
            let left = (d + 1) % 4;
            let next = if slot[1] != u32::MAX {
                if edges[slot[0] as usize].1 == left {
                    slot[0]
                } else {
                    slot[1]
                }
            } else {
                slot[0]
            };
            if next == u32::MAX || used[next as usize] {
                break;
            }
            e = next as usize;
        }
        if corners.len() < 4 {
            continue;
        }
        let cell = g.cell as f32;
        if let Some(snap) = snap {
            let edge = n as f32;
            let mut moved = vec![false; corners.len()];
            for (k, p) in corners.iter_mut().enumerate() {
                // (the points on the chunk's border stay on it)
                if p[0] == 0.0 || p[1] == 0.0 || p[0] == edge || p[1] == edge {
                    continue;
                }
                if let Some(q) = snap([p[0] * cell, p[1] * cell]) {
                    *p = [q[0] / cell, q[1] / cell];
                    moved[k] = true;
                }
            }
            smooth_loose(&mut corners, &moved, edge);
        }
        let pts: Vec<[f32; 2]> = simplify_ring(&corners, n as f32, tol / cell)
            .into_iter()
            .map(|p| [p[0] * cell, p[1] * cell])
            .collect();
        // (paint is thin: only outlines of surfaces lose their teeth)
        let pts = if snap.is_some() {
            without_spikes(pts, n as f32 * cell)
        } else {
            pts
        };
        if pts.len() < 3 {
            continue;
        }
        let area = ring_area(&pts);
        rings.push(Ring {
            outer: area > 0.0,
            area: area.abs(),
            pts,
        });
    }
    rings
}

/// Even out the steps of the points no geometry edge holds (painted ground, closed joints):
/// each one moves halfway to the middle of its neighbours, a few times over.
fn smooth_loose(p: &mut [[f32; 2]], held: &[bool], edge: f32) {
    let n = p.len();
    if n < 5 {
        return;
    }
    let on_border = |q: [f32; 2]| q[0] == 0.0 || q[1] == 0.0 || q[0] == edge || q[1] == edge;
    for _ in 0..3 {
        let prev = p.to_vec();
        for k in 0..n {
            if held[k] || on_border(prev[k]) {
                continue;
            }
            let (a, b) = (prev[(k + n - 1) % n], prev[(k + 1) % n]);
            let mid = [(a[0] + b[0]) * 0.5, (a[1] + b[1]) * 0.5];
            p[k] = [(prev[k][0] + mid[0]) * 0.5, (prev[k][1] + mid[1]) * 0.5];
        }
    }
}

/// Drop the teeth an outline keeps where its points were held by different edges: a point
/// standing out of the line of its neighbours by little, or by a short sharp spike.
fn without_spikes(mut p: Vec<[f32; 2]>, size: f32) -> Vec<[f32; 2]> {
    let border = |q: [f32; 2]| q[0] == 0.0 || q[1] == 0.0 || q[0] == size || q[1] == size;
    loop {
        let n = p.len();
        if n <= 4 {
            return p;
        }
        let mut drop = None;
        for k in 0..n {
            let (a, q, b) = (p[(k + n - 1) % n], p[k], p[(k + 1) % n]);
            if border(q) {
                continue;
            }
            let (ax, ay, bx, by) = (q[0] - a[0], q[1] - a[1], b[0] - q[0], b[1] - q[1]);
            let (la, lb) = (ax.hypot(ay), bx.hypot(by));
            let (cx, cy) = (b[0] - a[0], b[1] - a[1]);
            let lc = cx.hypot(cy).max(1e-6);
            // how far q stands off the line from a to b
            let height = ((q[0] - a[0]) * cy - (q[1] - a[1]) * cx).abs() / lc;
            // the turn at q: a spike folds back on itself
            let cos = (ax * bx + ay * by) / (la * lb).max(1e-9);
            if height < 0.06 || (height < 0.35 && la.min(lb) < 0.7 && cos < -0.2) {
                drop = Some(k);
                break;
            }
        }
        match drop {
            Some(k) => {
                p.remove(k);
            }
            None => return p,
        }
    }
}

/// Signed area (counter-clockwise positive).
pub(crate) fn ring_area(p: &[[f32; 2]]) -> f32 {
    let n = p.len();
    (0..n)
        .map(|i| {
            let (a, b) = (p[i], p[(i + 1) % n]);
            a[0] * b[1] - b[0] * a[1]
        })
        .sum::<f32>()
        * 0.5
}

/// Douglas-Peucker on a closed ring (cell units); points on the chunk's border stay, so a
/// surface that goes on in the next chunk meets it along the border.
fn simplify_ring(p: &[[f32; 2]], edge: f32, tol: f32) -> Vec<[f32; 2]> {
    let n = p.len();
    // the border lines a point lies on (bits: x = 0, x = edge, y = 0, y = edge)
    let lines = |q: [f32; 2]| {
        (q[0] == 0.0) as u8
            | ((q[0] == edge) as u8) << 1
            | ((q[1] == 0.0) as u8) << 2
            | ((q[1] == edge) as u8) << 3
    };
    // where the outline comes to the border or leaves it (the points between run straight)
    let fixed = |k: usize| {
        let here = lines(p[k]);
        here != 0
            && (here & lines(p[(k + n - 1) % n]) == 0 || here & lines(p[(k + 1) % n]) == 0)
    };
    // split at the point farthest from the first
    let far = (1..n)
        .max_by(|&a, &b| {
            let da = (p[a][0] - p[0][0]).powi(2) + (p[a][1] - p[0][1]).powi(2);
            let db = (p[b][0] - p[0][0]).powi(2) + (p[b][1] - p[0][1]).powi(2);
            da.total_cmp(&db)
        })
        .unwrap_or(0);
    let mut keep = vec![false; n + 1];
    keep[0] = true;
    keep[far] = true;
    keep[n] = true;
    for k in 0..n {
        keep[k] |= fixed(k);
    }
    let at = |k: usize| p[k % n];
    let anchors: Vec<usize> = (0..=n).filter(|&k| keep[k]).collect();
    let mut stack: Vec<(usize, usize)> = anchors.windows(2).map(|w| (w[0], w[1])).collect();
    while let Some((a, b)) = stack.pop() {
        if b <= a + 1 {
            continue;
        }
        let (pa, pb) = (at(a), at(b));
        let (dx, dy) = (pb[0] - pa[0], pb[1] - pa[1]);
        let len = (dx * dx + dy * dy).sqrt().max(1e-6);
        let mut worst = (0.0f32, 0usize);
        for k in a + 1..b {
            let q = at(k);
            let d = ((q[0] - pa[0]) * dy - (q[1] - pa[1]) * dx).abs() / len;
            if d > worst.0 {
                worst = (d, k);
            }
        }
        if worst.0 > tol {
            keep[worst.1] = true;
            stack.push((a, worst.1));
            stack.push((worst.1, b));
        }
    }
    (0..n).filter(|&k| keep[k]).map(|k| p[k]).collect()
}

/// Whether the edge from `a` to `b` runs along the chunk's border (the outline of a surface
/// that goes on in the next chunk, not a kerb).
pub(crate) fn on_border(a: [f32; 2], b: [f32; 2]) -> bool {
    let size = CHUNK as f32;
    let eps = 1e-3;
    let on = |v: f32, w: f32, edge: f32| (v - edge).abs() < eps && (w - edge).abs() < eps;
    on(a[0], b[0], 0.0) || on(a[0], b[0], size) || on(a[1], b[1], 0.0) || on(a[1], b[1], size)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn square(r: &mut Raster, x0: f32, y0: f32, x1: f32, y1: f32, z: f32, c: u8) {
        r.fill_tri([[x0, y0, z], [x1, y0, z], [x1, y1, z]], c, None);
        r.fill_tri([[x0, y0, z], [x1, y1, z], [x0, y1, z]], c, None);
    }

    #[test]
    fn paint_is_read_through_the_texture() {
        // a 4x4 texture, white in its left column, mapped once onto a 4 m square: a stripe
        let mut rgba = vec![60u8; 4 * 4 * 4];
        for j in 0..4 {
            rgba[(j * 4) * 4..(j * 4) * 4 + 3].copy_from_slice(&[240, 240, 240]);
        }
        let m = PaintMask::from_rgba(4, 4, &rgba, false).unwrap();
        let mut r = Raster::new(COARSE);
        let o = MARGIN as f32;
        let (s, z) = (20.0f32, 0.0);
        let uv = |p: [f32; 3]| [(p[0] - o) / s, (p[1] - o) / s];
        let t1 = [[o, o, z], [o + s, o, z], [o + s, o + s, z]];
        let t2 = [[o, o, z], [o + s, o + s, z], [o, o + s, z]];
        r.fill_tri(t1, 5, Some((&m, t1.map(uv))));
        r.fill_tri(t2, 5, Some((&m, t2.map(uv))));
        let painted = r.mark.iter().filter(|&&b| b).count();
        // a quarter of the 20x20 cells, give or take the stripe's edge
        assert!((80..=140).contains(&painted), "{painted}");
    }

    #[test]
    fn a_ring_around_a_hole() {
        let mut r = Raster::new(COARSE);
        let m = MARGIN as f32;
        square(&mut r, m + 10.0, m + 10.0, m + 60.0, m + 60.0, 0.0, 2);
        square(&mut r, m + 30.0, m + 30.0, m + 40.0, m + 40.0, 0.3, 1);
        let rings = outlines(&r.mask(2), COARSE, 0.05, None);
        assert_eq!(rings.len(), 2, "{rings:?}");
        let outer = rings.iter().find(|r| r.outer).unwrap();
        let hole = rings.iter().find(|r| !r.outer).unwrap();
        // 50 cells of 0.2 m: 10 m, less the corners cut through the edge middles
        assert!((outer.area - 100.0).abs() < 0.2, "{}", outer.area);
        assert!((hole.area - 4.0).abs() < 0.2, "{}", hole.area);
    }

    #[test]
    fn closing_mends_a_crack() {
        let mut r = Raster::new(COARSE);
        let m = MARGIN as f32;
        square(&mut r, m + 10.0, m + 10.0, m + 30.0, m + 20.0, 0.0, 2);
        square(&mut r, m + 31.0, m + 10.0, m + 50.0, m + 20.0, 0.0, 2);
        assert_eq!(outlines(&r.mask(2), COARSE, 0.05, None).len(), 2);
        assert_eq!(outlines(&close(&r.mask(2), COARSE, 1), COARSE, 0.05, None).len(), 1);
    }

    #[test]
    fn the_higher_surface_shows() {
        let mut r = Raster::new(COARSE);
        let m = MARGIN as f32;
        square(&mut r, m, m, m + 20.0, m + 20.0, 0.0, 3);
        square(&mut r, m, m, m + 20.0, m + 20.0, 0.15, 1);
        assert!(r.mask(1).iter().filter(|&&b| b).count() > 300);
        assert_eq!(r.mask(3).iter().filter(|&&b| b).count(), 0);
    }
}

#[cfg(test)]
mod border_tests {
    use super::*;

    #[test]
    fn a_surface_running_on_has_its_border_marked() {
        let mut r = Raster::new(COARSE);
        let m = MARGIN as f32;
        let z = 0.0;
        let (x0, y0, x1, y1) = (m + 100.0, m + 100.0, W as f32, m + 150.0);
        r.fill_tri([[x0, y0, z], [x1, y0, z], [x1, y1, z]], 2, None);
        r.fill_tri([[x0, y0, z], [x1, y1, z], [x0, y1, z]], 2, None);
        let rings = outlines(&close(&r.mask(2), COARSE, 2), COARSE, 0.1, None);
        assert_eq!(rings.len(), 1);
        let p = &rings[0].pts;
        let n = p.len();
        let border = (0..n).filter(|&i| on_border(p[i], p[(i + 1) % n])).count();
        assert!(border >= 1, "{p:?}");
    }
}
