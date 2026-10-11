//! Triangulation of a polygon with holes by ear clipping (after mapbox/earcut): the
//! navigator fills the outlines the surface raster gives, which are rarely convex.

/// Triangles (indices into `verts`) of the polygon whose outer ring is `verts[..holes[0]]`
/// (any orientation) and whose holes start at the indices in `holes`.
pub(crate) fn earcut(verts: &[[f32; 2]], holes: &[usize]) -> Vec<u32> {
    let mut e = Earcut {
        nodes: Vec::with_capacity(verts.len() * 2),
        verts,
        tris: Vec::new(),
    };
    let outer_end = holes.first().copied().unwrap_or(verts.len());
    let Some(mut outer) = e.linked_list(0, outer_end, true) else {
        return e.tris;
    };
    if e.nodes[outer].next == e.nodes[outer].prev {
        return e.tris;
    }
    if !holes.is_empty() {
        outer = e.eliminate_holes(holes, outer);
    }
    e.earcut_linked(Some(outer), 0);
    e.tris
}

#[derive(Clone, Copy)]
struct Node {
    i: u32,
    x: f32,
    y: f32,
    prev: usize,
    next: usize,
    steiner: bool,
}

struct Earcut<'a> {
    nodes: Vec<Node>,
    verts: &'a [[f32; 2]],
    tris: Vec<u32>,
}

fn signed_area(verts: &[[f32; 2]], start: usize, end: usize) -> f64 {
    let mut sum = 0.0f64;
    let mut j = end - 1;
    for i in start..end {
        sum += (verts[j][0] as f64 - verts[i][0] as f64) * (verts[i][1] as f64 + verts[j][1] as f64);
        j = i;
    }
    sum
}

fn tri_area(p: &Node, q: &Node, r: &Node) -> f32 {
    (q.y - p.y) * (r.x - q.x) - (q.x - p.x) * (r.y - q.y)
}

fn point_in_triangle(ax: f32, ay: f32, bx: f32, by: f32, cx: f32, cy: f32, px: f32, py: f32) -> bool {
    (cx - px) * (ay - py) >= (ax - px) * (cy - py)
        && (ax - px) * (by - py) >= (bx - px) * (ay - py)
        && (bx - px) * (cy - py) >= (cx - px) * (by - py)
}

fn equals(a: &Node, b: &Node) -> bool {
    a.x == b.x && a.y == b.y
}

fn sign(v: f32) -> i32 {
    if v > 0.0 {
        1
    } else if v < 0.0 {
        -1
    } else {
        0
    }
}

fn on_segment(p: &Node, q: &Node, r: &Node) -> bool {
    q.x <= p.x.max(r.x) && q.x >= p.x.min(r.x) && q.y <= p.y.max(r.y) && q.y >= p.y.min(r.y)
}

fn intersects(p1: &Node, q1: &Node, p2: &Node, q2: &Node) -> bool {
    let o1 = sign(tri_area(p1, q1, p2));
    let o2 = sign(tri_area(p1, q1, q2));
    let o3 = sign(tri_area(p2, q2, p1));
    let o4 = sign(tri_area(p2, q2, q1));
    (o1 != o2 && o3 != o4)
        || (o1 == 0 && on_segment(p1, p2, q1))
        || (o2 == 0 && on_segment(p1, q2, q1))
        || (o3 == 0 && on_segment(p2, p1, q2))
        || (o4 == 0 && on_segment(p2, q1, q2))
}

impl Earcut<'_> {
    fn n(&self, i: usize) -> &Node {
        &self.nodes[i]
    }

    fn insert(&mut self, i: usize, last: Option<usize>) -> usize {
        let idx = self.nodes.len();
        let [x, y] = self.verts[i];
        let mut node = Node {
            i: i as u32,
            x,
            y,
            prev: idx,
            next: idx,
            steiner: false,
        };
        if let Some(l) = last {
            node.next = self.nodes[l].next;
            node.prev = l;
            let ln = self.nodes[l].next;
            self.nodes.push(node);
            self.nodes[ln].prev = idx;
            self.nodes[l].next = idx;
        } else {
            self.nodes.push(node);
        }
        idx
    }

    fn remove(&mut self, p: usize) {
        let (prev, next) = (self.nodes[p].prev, self.nodes[p].next);
        self.nodes[next].prev = prev;
        self.nodes[prev].next = next;
    }

    /// A circular list of the ring's vertices, wound as `clockwise` asks.
    fn linked_list(&mut self, start: usize, end: usize, clockwise: bool) -> Option<usize> {
        if end <= start {
            return None;
        }
        let mut last = None;
        if clockwise == (signed_area(self.verts, start, end) > 0.0) {
            for i in start..end {
                last = Some(self.insert(i, last));
            }
        } else {
            for i in (start..end).rev() {
                last = Some(self.insert(i, last));
            }
        }
        let l = last?;
        let ln = self.nodes[l].next;
        if equals(self.n(l), self.n(ln)) {
            self.remove(l);
            return Some(ln);
        }
        Some(l)
    }

    /// Drop duplicate and collinear points.
    fn filter_points(&mut self, start: usize, end: Option<usize>) -> usize {
        let mut end = end.unwrap_or(start);
        let mut p = start;
        loop {
            let mut again = false;
            let (pp, pn) = (self.nodes[p].prev, self.nodes[p].next);
            if !self.nodes[p].steiner
                && (equals(self.n(p), self.n(pn)) || tri_area(self.n(pp), self.n(p), self.n(pn)) == 0.0)
            {
                self.remove(p);
                p = pp;
                end = pp;
                if p == self.nodes[p].next {
                    break;
                }
                again = true;
            } else {
                p = pn;
            }
            if !again && p == end {
                break;
            }
        }
        end
    }

    fn earcut_linked(&mut self, ear: Option<usize>, pass: u8) {
        let Some(mut ear) = ear else { return };
        let mut stop = ear;
        while self.nodes[ear].prev != self.nodes[ear].next {
            let (prev, next) = (self.nodes[ear].prev, self.nodes[ear].next);
            if self.is_ear(ear) {
                self.tris
                    .extend([self.nodes[prev].i, self.nodes[ear].i, self.nodes[next].i]);
                self.remove(ear);
                ear = self.nodes[next].next;
                stop = self.nodes[next].next;
                continue;
            }
            ear = next;
            if ear == stop {
                match pass {
                    0 => {
                        let f = self.filter_points(ear, None);
                        self.earcut_linked(Some(f), 1);
                    }
                    1 => {
                        let f = self.filter_points(ear, None);
                        let e = self.cure_local_intersections(f);
                        self.earcut_linked(Some(e), 2);
                    }
                    _ => self.split_earcut(ear),
                }
                break;
            }
        }
    }

    fn is_ear(&self, ear: usize) -> bool {
        let (a, b, c) = (
            self.n(self.nodes[ear].prev),
            self.n(ear),
            self.n(self.nodes[ear].next),
        );
        if tri_area(a, b, c) >= 0.0 {
            return false;
        }
        let (x0, x1) = (a.x.min(b.x).min(c.x), a.x.max(b.x).max(c.x));
        let (y0, y1) = (a.y.min(b.y).min(c.y), a.y.max(b.y).max(c.y));
        let mut p = c.next;
        let stop = self.nodes[ear].prev;
        while p != stop {
            let q = self.n(p);
            if q.x >= x0
                && q.x <= x1
                && q.y >= y0
                && q.y <= y1
                && point_in_triangle(a.x, a.y, b.x, b.y, c.x, c.y, q.x, q.y)
                && tri_area(self.n(q.prev), q, self.n(q.next)) >= 0.0
            {
                return false;
            }
            p = q.next;
        }
        true
    }

    fn cure_local_intersections(&mut self, start: usize) -> usize {
        let mut start = start;
        let mut p = start;
        loop {
            let a = self.nodes[p].prev;
            let pn = self.nodes[p].next;
            let b = self.nodes[pn].next;
            if !equals(self.n(a), self.n(b))
                && intersects(self.n(a), self.n(p), self.n(pn), self.n(b))
                && self.locally_inside(a, b)
                && self.locally_inside(b, a)
            {
                self.tris
                    .extend([self.nodes[a].i, self.nodes[p].i, self.nodes[b].i]);
                self.remove(p);
                self.remove(pn);
                p = b;
                start = b;
            }
            p = self.nodes[p].next;
            if p == start {
                break;
            }
        }
        self.filter_points(p, None)
    }

    fn split_earcut(&mut self, start: usize) {
        let mut a = start;
        loop {
            let mut b = self.nodes[self.nodes[a].next].next;
            while b != self.nodes[a].prev {
                if self.nodes[a].i != self.nodes[b].i && self.is_valid_diagonal(a, b) {
                    let mut c = self.split_polygon(a, b);
                    let an = self.nodes[a].next;
                    a = self.filter_points(a, Some(an));
                    let cn = self.nodes[c].next;
                    c = self.filter_points(c, Some(cn));
                    self.earcut_linked(Some(a), 0);
                    self.earcut_linked(Some(c), 0);
                    return;
                }
                b = self.nodes[b].next;
            }
            a = self.nodes[a].next;
            if a == start {
                break;
            }
        }
    }

    fn eliminate_holes(&mut self, holes: &[usize], mut outer: usize) -> usize {
        let mut queue = Vec::new();
        for (k, &start) in holes.iter().enumerate() {
            let end = holes.get(k + 1).copied().unwrap_or(self.verts.len());
            if let Some(list) = self.linked_list(start, end, false) {
                if list == self.nodes[list].next {
                    self.nodes[list].steiner = true;
                }
                queue.push(self.leftmost(list));
            }
        }
        queue.sort_by(|&a, &b| {
            self.nodes[a]
                .x
                .total_cmp(&self.nodes[b].x)
                .then(self.nodes[a].y.total_cmp(&self.nodes[b].y))
        });
        for h in queue {
            outer = self.eliminate_hole(h, outer);
        }
        outer
    }

    fn eliminate_hole(&mut self, hole: usize, outer: usize) -> usize {
        let Some(bridge) = self.find_hole_bridge(hole, outer) else {
            return outer;
        };
        let reverse = self.split_polygon(bridge, hole);
        let rn = self.nodes[reverse].next;
        self.filter_points(reverse, Some(rn));
        let bn = self.nodes[bridge].next;
        self.filter_points(bridge, Some(bn))
    }

    fn find_hole_bridge(&self, hole: usize, outer: usize) -> Option<usize> {
        let mut p = outer;
        let (hx, hy) = (self.n(hole).x, self.n(hole).y);
        let mut qx = f32::NEG_INFINITY;
        let mut m: Option<usize> = None;
        loop {
            let (a, b) = (self.n(p), self.n(self.nodes[p].next));
            if hy <= a.y && hy >= b.y && b.y != a.y {
                let x = a.x + (hy - a.y) * (b.x - a.x) / (b.y - a.y);
                if x <= hx && x > qx {
                    qx = x;
                    m = Some(if a.x < b.x { p } else { self.nodes[p].next });
                    if x == hx {
                        return m;
                    }
                }
            }
            p = self.nodes[p].next;
            if p == outer {
                break;
            }
        }
        let mut m = m?;
        let stop = m;
        let (mx, my) = (self.n(m).x, self.n(m).y);
        let mut tan_min = f32::INFINITY;
        p = m;
        loop {
            let q = self.n(p);
            if hx >= q.x
                && q.x >= mx
                && hx != q.x
                && point_in_triangle(
                    if hy < my { hx } else { qx },
                    hy,
                    mx,
                    my,
                    if hy < my { qx } else { hx },
                    hy,
                    q.x,
                    q.y,
                )
            {
                let tan = (hy - q.y).abs() / (hx - q.x);
                if self.locally_inside(p, hole)
                    && (tan < tan_min
                        || (tan == tan_min
                            && (q.x > self.n(m).x
                                || (q.x == self.n(m).x && self.sector_contains(m, p)))))
                {
                    m = p;
                    tan_min = tan;
                }
            }
            p = q.next;
            if p == stop {
                break;
            }
        }
        Some(m)
    }

    fn sector_contains(&self, m: usize, p: usize) -> bool {
        let (mp, mn) = (self.n(self.nodes[m].prev), self.n(self.nodes[m].next));
        tri_area(mp, self.n(m), self.n(p)) < 0.0 && tri_area(self.n(p), mn, self.n(m)) < 0.0
    }

    fn leftmost(&self, start: usize) -> usize {
        let mut p = start;
        let mut left = start;
        loop {
            let (q, l) = (self.n(p), self.n(left));
            if q.x < l.x || (q.x == l.x && q.y < l.y) {
                left = p;
            }
            p = q.next;
            if p == start {
                break;
            }
        }
        left
    }

    fn is_valid_diagonal(&self, a: usize, b: usize) -> bool {
        let (na, nb) = (self.n(a), self.n(b));
        self.n(na.next).i != nb.i
            && self.n(na.prev).i != nb.i
            && !self.intersects_polygon(a, b)
            && ((self.locally_inside(a, b)
                && self.locally_inside(b, a)
                && self.middle_inside(a, b)
                && (tri_area(self.n(na.prev), na, self.n(nb.prev)) != 0.0
                    || tri_area(na, self.n(nb.prev), nb) != 0.0))
                || (equals(na, nb)
                    && tri_area(self.n(na.prev), na, self.n(na.next)) > 0.0
                    && tri_area(self.n(nb.prev), nb, self.n(nb.next)) > 0.0))
    }

    fn intersects_polygon(&self, a: usize, b: usize) -> bool {
        let (na, nb) = (self.n(a), self.n(b));
        let mut p = a;
        loop {
            let q = self.n(p);
            let qn = self.n(q.next);
            if q.i != na.i && qn.i != na.i && q.i != nb.i && qn.i != nb.i && intersects(q, qn, na, nb) {
                return true;
            }
            p = q.next;
            if p == a {
                return false;
            }
        }
    }

    fn locally_inside(&self, a: usize, b: usize) -> bool {
        let (na, nb) = (self.n(a), self.n(b));
        let (prev, next) = (self.n(na.prev), self.n(na.next));
        if tri_area(prev, na, next) < 0.0 {
            tri_area(na, nb, next) >= 0.0 && tri_area(na, prev, nb) >= 0.0
        } else {
            tri_area(na, nb, prev) < 0.0 || tri_area(na, next, nb) < 0.0
        }
    }

    fn middle_inside(&self, a: usize, b: usize) -> bool {
        let (na, nb) = (self.n(a), self.n(b));
        let (px, py) = ((na.x + nb.x) * 0.5, (na.y + nb.y) * 0.5);
        let mut inside = false;
        let mut p = a;
        loop {
            let q = self.n(p);
            let qn = self.n(q.next);
            if ((q.y > py) != (qn.y > py))
                && qn.y != q.y
                && px < (qn.x - q.x) * (py - q.y) / (qn.y - q.y) + q.x
            {
                inside = !inside;
            }
            p = q.next;
            if p == a {
                return inside;
            }
        }
    }

    /// Link `a` and `b` with a bridge, splitting the ring in two; returns the new copy of `b`
    /// on the other side.
    fn split_polygon(&mut self, a: usize, b: usize) -> usize {
        let a2 = self.nodes.len();
        let b2 = a2 + 1;
        let (an, bp) = (self.nodes[a].next, self.nodes[b].prev);
        let mut na = self.nodes[a];
        let mut nb = self.nodes[b];
        self.nodes[a].next = b;
        self.nodes[b].prev = a;
        na.next = an;
        na.prev = b2;
        nb.next = a2;
        nb.prev = bp;
        self.nodes.push(na);
        self.nodes.push(nb);
        self.nodes[an].prev = a2;
        self.nodes[bp].next = b2;
        b2
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn area(verts: &[[f32; 2]], tris: &[u32]) -> f32 {
        tris.chunks_exact(3)
            .map(|t| {
                let (a, b, c) = (verts[t[0] as usize], verts[t[1] as usize], verts[t[2] as usize]);
                ((b[0] - a[0]) * (c[1] - a[1]) - (c[0] - a[0]) * (b[1] - a[1])).abs() * 0.5
            })
            .sum()
    }

    #[test]
    fn square_with_a_hole_is_covered_once() {
        let v = [
            [0.0, 0.0],
            [10.0, 0.0],
            [10.0, 10.0],
            [0.0, 10.0],
            [4.0, 4.0],
            [4.0, 6.0],
            [6.0, 6.0],
            [6.0, 4.0],
        ];
        let t = earcut(&v, &[4]);
        assert!((area(&v, &t) - 96.0).abs() < 1e-3, "{}", area(&v, &t));
    }

    #[test]
    fn concave_l_shape() {
        let v = [[0.0, 0.0], [6.0, 0.0], [6.0, 2.0], [2.0, 2.0], [2.0, 6.0], [0.0, 6.0]];
        let t = earcut(&v, &[]);
        assert_eq!(t.len(), 12);
        assert!((area(&v, &t) - 20.0).abs() < 1e-3);
    }
}
