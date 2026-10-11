use super::*;

/// Lanes this far apart do not join into one line (m): the route jumps there.
const ROUTE_BREAK: f64 = 12.0;
/// Half the stretch over which the line is smoothed (m): the step where the route changes
/// lanes melts into a gentle shift, a turn keeps its shape.
const ROUTE_SMOOTH: f64 = 8.0;
/// How far back along the line a lane may start beside it and still join it (m).
const ROUTE_REJOIN: f64 = 80.0;
/// Over how much of a lane the line drifts across onto it after a change of lanes (m).
const LANE_BLEND: f64 = 30.0;

/// Points of the route ahead along `lanes` (the first starting `start` m along the route),
/// as many lines as there are places where the lanes do not meet: each point with its
/// distance along the route and the lane it lies on. Ends after `max_len` m.
pub(super) fn route_lines(
    net: &Network,
    lanes: &[usize],
    start: f64,
    max_len: f64,
) -> Vec<Vec<(DVec3, f64, usize)>> {
    let mut lines: Vec<Vec<(DVec3, f64, usize)>> = Vec::new();
    let mut cur: Vec<(DVec3, f64, usize)> = Vec::new();
    let mut s0 = start;
    for &l in lanes {
        let Some(lane) = net.lanes.get(l) else { break };
        // a lane starting a little beside where the line is (a change of lanes): the line
        // drifts across over its first metres instead of stepping
        let step = match (cur.last(), lane.points.first()) {
            (Some(last), Some(q)) => {
                let d = last.0 - *q;
                (0.3..=ROUTE_BREAK).contains(&d.truncate().length()).then_some(d)
            }
            _ => None,
        };
        let blend = (lane.length() as f64 * 0.8).min(LANE_BLEND);
        for (q, d) in lane.points.iter().zip(&lane.dist) {
            let s = s0 + *d as f64;
            let q = &match step {
                Some(off) if (*d as f64) < blend => *q + off * (1.0 - *d as f64 / blend),
                _ => *q,
            };
            match cur.last() {
                Some(last) if (*q - last.0).truncate().length() > ROUTE_BREAK => {
                    // a lane that starts back beside the line (a stop's bay along the
                    // carriageway the trip has already covered): the line turns off into it
                    // where it begins instead of breaking into two strokes side by side
                    match rejoin(&cur, *q) {
                        Some((k, p, ps)) => {
                            cur.truncate(k + 1);
                            cur.push((p, ps, cur[k].2));
                            cur.push((*q, s, l));
                        }
                        None => {
                            lines.push(std::mem::take(&mut cur));
                            cur.push((*q, s, l));
                        }
                    }
                }
                Some(last) if (*q - last.0).truncate().length() < 0.3 => {}
                _ => cur.push((*q, s, l)),
            }
        }
        s0 += lane.length() as f64;
        if s0 - start > max_len {
            break;
        }
    }
    lines.push(cur);
    lines.retain(|l| l.len() >= 2);
    for line in lines.iter_mut() {
        let orig = line.clone();
        let n = orig.len();
        let mut lo = 0;
        for i in 1..n.saturating_sub(1) {
            let si = orig[i].1;
            while orig[lo].1 < si - ROUTE_SMOOTH {
                lo += 1;
            }
            let (mut sum, mut wsum) = (DVec3::ZERO, 0.0);
            let mut j = lo;
            while j < n && orig[j].1 <= si + ROUTE_SMOOTH {
                let w = 1.0 - (orig[j].1 - si).abs() / ROUTE_SMOOTH + 1e-3;
                sum += orig[j].0 * w;
                wsum += w;
                j += 1;
            }
            // (the ends of the line stay where they are: the smoothing fades in)
            let edge = ((si - orig[0].1).min(orig[n - 1].1 - si) / ROUTE_SMOOTH).clamp(0.0, 1.0);
            line[i].0 = orig[i].0.lerp(sum / wsum, edge);
        }
    }
    lines
}

/// Where `q` lies beside the last `ROUTE_REJOIN` m of `line`, if within `ROUTE_BREAK` of it:
/// the segment it falls on (its first point's index), the nearest point there and its
/// distance along the route.
fn rejoin(line: &[(DVec3, f64, usize)], q: DVec3) -> Option<(usize, DVec3, f64)> {
    let end = line.last()?.1;
    let mut best: Option<(f64, usize, DVec3, f64)> = None;
    for k in (0..line.len().saturating_sub(1)).rev() {
        let (a, b) = (line[k], line[k + 1]);
        if end - b.1 > ROUTE_REJOIN {
            break;
        }
        let ab = (b.0 - a.0).truncate();
        let t = if ab.length_squared() > 1e-6 {
            ((q - a.0).truncate().dot(ab) / ab.length_squared()).clamp(0.0, 1.0)
        } else {
            0.0
        };
        let p = a.0.lerp(b.0, t);
        let d = (q - p).truncate().length();
        if d <= ROUTE_BREAK && best.map(|x| d < x.0).unwrap_or(true) {
            best = Some((d, k, p, a.1 + (b.1 - a.1) * t));
        }
    }
    best.map(|(_, k, p, s)| (k, p, s))
}

/// The route ahead as one line of `w_px` pixels with a dark edge, coloured by the traffic
/// on it, each point carrying its distance along the route for the layer's `route` cut.
#[allow(clippy::too_many_arguments)]
pub(super) fn build_route_line(
    p: &mut Painter,
    net: &Network,
    lanes: &[usize],
    start: f64,
    anchor: DVec2,
    jam: &HashMap<usize, f32>,
    max_len: f64,
    w_px: f32,
) {
    let rel = |q: DVec3| Vec3::new((q.x - anchor.x) as f32, (q.y - anchor.y) as f32, 0.0);
    let lines = route_lines(net, lanes, start, max_len);
    let simple: Vec<Vec<(Vec3, f32, usize)>> = lines
        .iter()
        .map(|line| {
            let pts: Vec<Vec3> = line.iter().map(|(q, _, _)| rel(*q)).collect();
            let kept = simplify(&pts, 0.05);
            let mut k = 0;
            kept.into_iter()
                .map(|q| {
                    while pts[k] != q {
                        k += 1;
                    }
                    (q, line[k].1 as f32, line[k].2)
                })
                .collect()
        })
        .collect();
    for line in &simple {
        let pts: Vec<Vec3> = line.iter().map(|x| x.0).collect();
        let along: Vec<f32> = line.iter().map(|x| x.1).collect();
        p.ribbon_along(&pts, &along, 0.0, w_px + 3.0, ROUTE_EDGE, true);
    }
    for line in &simple {
        let mut run: Vec<(Vec3, f32)> = Vec::new();
        let mut lv = usize::MAX;
        for &(q, s, l) in line {
            let v = level(jam.get(&l).copied().unwrap_or(0.0));
            if v != lv && !run.is_empty() {
                let (pts, along): (Vec<Vec3>, Vec<f32>) = run.iter().copied().unzip();
                p.ribbon_along(&pts, &along, 0.0, w_px, LEVEL[lv], true);
                run = vec![*run.last().unwrap()];
            }
            lv = v;
            run.push((q, s));
        }
        if run.len() >= 2 {
            let (pts, along): (Vec<Vec3>, Vec<f32>) = run.into_iter().unzip();
            p.ribbon_along(&pts, &along, 0.0, w_px, LEVEL[lv], true);
        }
    }
}

/// How far beside a lane's start another lane may start and be changed onto there (m).
const LANE_CHANGE: f64 = 6.0;
/// What a change of lanes costs the way search, as if that many metres more were driven.
const LANE_CHANGE_COST: f32 = 40.0;

/// Street lanes running the same way as lane `n` that start beside its start, up to
/// `LANE_CHANGE` across and a little ahead or behind: the lanes a bus can change onto there.
fn lanes_beside_start(net: &Network, n: usize) -> impl Iterator<Item = usize> + '_ {
    let l = &net.lanes[n];
    let (p, h) = (l.start(), l.start_heading() as f64);
    let fwd = DVec2::new(h.to_radians().sin(), h.to_radians().cos());
    lanes_near(net, p.truncate(), LANE_CHANGE).into_iter().filter(move |&j| {
        let o = &net.lanes[j];
        let d = o.start() - p;
        let along = d.truncate().dot(fwd);
        let across = (d.truncate() - fwd * along).length();
        j != n
            && o.kind == LaneKind::Street
            && (1.0..=LANE_CHANGE).contains(&across)
            && along.abs() < 4.0
            && d.z.abs() < 1.5
            && angle_diff(h, o.start_heading() as f64).abs() < 20.0
    })
}

pub(crate) fn way_back(
    net: &Network,
    bus: DVec3,
    heading: f64,
    ahead: &[usize],
    max_cost: f32,
) -> Option<(Vec<usize>, usize)> {
    use std::cmp::Ordering;
    use std::collections::BinaryHeap;
    let cands: Vec<(usize, f64, f64)> = lanes_near(net, bus.truncate(), 90.0)
        .into_iter()
        .filter_map(|i| {
            let l = net.lanes.get(i)?;
            if l.kind != LaneKind::Street {
                return None;
            }
            let (s, d) = l.nearest_point(bus)?;
            let (_, h) = l.at(s);
            Some((i, d, angle_diff(heading, h as f64).abs()))
        })
        .collect();
    let pick = |max_d: f64, max_a: f64| {
        cands
            .iter()
            .filter(|c| c.1 < max_d && c.2 < max_a)
            .min_by(|a, b| (a.1 + a.2 * 0.1).total_cmp(&(b.1 + b.2 * 0.1)))
            .map(|c| c.0)
    };
    let start = pick(14.0, 70.0)
        .or_else(|| pick(80.0, 100.0))
        .or_else(|| pick(80.0, 181.0));
    if ::legacy_config::env::var_os("OMSI_DEBUG_NAV").is_some() {
        log::info!(
            "navigator: way search from {:?} ({} street lanes within 90 m) to {} route lanes",
            start,
            cands.len(),
            ahead.len()
        );
    }
    let start = start?;
    let targets: HashMap<usize, usize> = ahead
        .iter()
        .take(120)
        .enumerate()
        .map(|(k, &l)| (l, k))
        .collect();
    #[derive(PartialEq)]
    struct Node(f32, usize);
    impl Eq for Node {}
    impl Ord for Node {
        fn cmp(&self, o: &Self) -> Ordering {
            o.0.total_cmp(&self.0)
        }
    }
    impl PartialOrd for Node {
        fn partial_cmp(&self, o: &Self) -> Option<Ordering> {
            Some(self.cmp(o))
        }
    }
    let mut dist: HashMap<usize, f32> = HashMap::new();
    let mut parent: HashMap<usize, usize> = HashMap::new();
    let mut heap = BinaryHeap::new();
    dist.insert(start, 0.0);
    heap.push(Node(0.0, start));
    while let Some(Node(cost, lane)) = heap.pop() {
        if cost > dist.get(&lane).copied().unwrap_or(f32::INFINITY) {
            continue;
        }
        if lane != start {
            if let Some(&k) = targets.get(&lane) {
                let mut path = vec![lane];
                let mut c = lane;
                while let Some(&p) = parent.get(&c) {
                    path.push(p);
                    c = p;
                }
                path.reverse();
                path.pop();
                return Some((path, k));
            }
        }
        if cost > max_cost {
            break;
        }
        let Some(l) = net.lanes.get(lane) else {
            continue;
        };
        for &n in &l.next {
            let Some(nl) = net.lanes.get(n) else { continue };
            if nl.kind != LaneKind::Street {
                continue;
            }
            let u_turn =
                angle_diff(l.end_heading() as f64, nl.start_heading() as f64).abs() > 150.0;
            let c = cost + if u_turn { 400.0 } else { 0.0 };
            // the lane itself, or one beside it the same way: where the splines do not link
            // the lanes of a road, the bus still changes lanes rather than drive a loop
            let changes = lanes_beside_start(net, n).map(|j| (j, LANE_CHANGE_COST));
            for (m, extra) in std::iter::once((n, 0.0)).chain(changes) {
                let c = c + net.lanes[m].length() + extra;
                if c < dist.get(&m).copied().unwrap_or(f32::INFINITY) {
                    dist.insert(m, c);
                    parent.insert(m, lane);
                    heap.push(Node(c, m));
                }
            }
        }
    }
    None
}

