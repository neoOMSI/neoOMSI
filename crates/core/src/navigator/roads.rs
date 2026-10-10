use super::*;

pub(super) fn visible_road_lanes(net: &Network) -> Vec<(usize, &::traffic::Lane)> {
    let mut seen = hashbrown::HashSet::<(LaneKey, u32)>::new();
    net.lanes
        .iter()
        .enumerate()
        .filter(|(_, l)| l.kind == LaneKind::Street && !l.invisible && l.points.len() >= 2)
        .filter(|(_, l)| {
            if let Some(key) = l.key {
                return seen.insert((key, l.source));
            }
            true
        })
        .collect()
}

pub(crate) fn confirm_road_surfaces(net: &mut Network, surfaces: &[(Vec<DVec3>, f32)]) {
    let mut segments = Vec::new();
    let mut grid: HashMap<(i32, i32), Vec<usize>> = HashMap::new();
    for (pts, width) in surfaces {
        for ab in pts.windows(2) {
            let (a, b) = (ab[0], ab[1]);
            let margin = *width as f64 * 0.5 + 0.5;
            let lo = a.truncate().min(b.truncate()) - DVec2::splat(margin);
            let hi = a.truncate().max(b.truncate()) + DVec2::splat(margin);
            let (x0, y0) = Network::grid_cell(lo.extend(0.0));
            let (x1, y1) = Network::grid_cell(hi.extend(0.0));
            let i = segments.len();
            segments.push((a, b, margin));
            for x in x0..=x1 {
                for y in y0..=y1 {
                    grid.entry((x, y)).or_default().push(i);
                }
            }
        }
    }
    for lane in net
        .lanes
        .iter_mut()
        .filter(|l| l.invisible && l.kind == LaneKind::Street)
    {
        let n = (lane.length() / 8.0).ceil().max(1.0) as usize;
        let mut covered = 0;
        for k in 0..n {
            let (p, _) = lane.at(lane.length() * (k as f32 + 0.5) / n as f32);
            let on_surface = grid
                .get(&Network::grid_cell(p))
                .map(|ids| {
                    ids.iter().any(|&i| {
                        let (a, b, margin) = segments[i];
                        let ab = (b - a).truncate();
                        let t = ((p - a).truncate().dot(ab) / ab.length_squared().max(1e-6))
                            .clamp(0.0, 1.0);
                        let q = a.lerp(b, t);
                        (p - q).truncate().length() <= margin && (p.z - q.z).abs() <= 2.0
                    })
                })
                .unwrap_or(false);
            if on_surface {
                covered += 1;
            }
        }
        if covered * 4 >= n * 3 {
            lane.invisible = false;
        }
    }
    let mut prev = vec![Vec::new(); net.lanes.len()];
    for (i, lane) in net
        .lanes
        .iter()
        .enumerate()
        .filter(|(_, l)| l.kind == LaneKind::Street)
    {
        for &j in &lane.next {
            if net
                .lanes
                .get(j)
                .map(|l| l.kind == LaneKind::Street)
                .unwrap_or(false)
            {
                prev[j].push(i);
            }
        }
    }
    let mut seen = vec![false; net.lanes.len()];
    let mut keep = Vec::new();
    for seed in 0..net.lanes.len() {
        if seen[seed] || !net.lanes[seed].invisible || net.lanes[seed].kind != LaneKind::Street {
            continue;
        }
        let mut queue = vec![seed];
        let mut component = Vec::new();
        let mut anchors = hashbrown::HashSet::new();
        while let Some(i) = queue.pop() {
            if seen[i] {
                continue;
            }
            seen[i] = true;
            component.push(i);
            for &j in net.lanes[i].next.iter().chain(prev[i].iter()) {
                let Some(l) = net.lanes.get(j).filter(|l| l.kind == LaneKind::Street) else {
                    continue;
                };
                if l.invisible {
                    if !seen[j] {
                        queue.push(j);
                    }
                } else {
                    anchors.insert((l.key, l.source, if l.key.is_none() { j } else { 0 }));
                }
            }
        }
        if anchors.len() >= 2 {
            keep.extend(component);
        }
    }
    for i in keep {
        net.lanes[i].invisible = false;
    }
}

pub(crate) fn road_geometry(net: &Network) -> Vec<MapRoad> {
    let mut roads = Vec::new();
    let mut splines =
        std::collections::BTreeMap::<((i32, i32), i64), Vec<&::traffic::Lane>>::new();
    for (_, lane) in visible_road_lanes(net) {
        if let Some(key) = lane.key.filter(|_| lane.source == 1) {
            splines.entry((key.tile, key.id)).or_default().push(lane);
        } else {
            roads.push(MapRoad {
                points: lane.points.clone(),
                width: lane.width.max(2.6),
                main: lane.speed_limit_kmh >= 55.0,
            });
        }
    }
    for mut lanes in splines.into_values() {
        lanes.sort_by(|a, b| a.offset.total_cmp(&b.offset));
        let mut start = 0;
        while start < lanes.len() {
            let a = lanes[start];
            let lo = a.offset - a.width.max(2.6) * 0.5;
            let mut hi = a.offset + a.width.max(2.6) * 0.5;
            let mut end = start + 1;
            while end < lanes.len() {
                let b = lanes[end];
                if b.points.len() != a.points.len() || b.offset - b.width.max(2.6) * 0.5 > hi + 0.65
                {
                    break;
                }
                let mid = a.points.len() / 2;
                let za = a.points[if a.reversed {
                    a.points.len() - 1 - mid
                } else {
                    mid
                }]
                    .z;
                let zb = b.points[if b.reversed {
                    b.points.len() - 1 - mid
                } else {
                    mid
                }]
                    .z;
                if (za - zb).abs() > 1.5 {
                    break;
                }
                hi = hi.max(b.offset + b.width.max(2.6) * 0.5);
                end += 1;
            }
            let b = lanes[end - 1];
            let span = b.offset - a.offset;
            let t = if span.abs() > 0.01 {
                (((lo + hi) * 0.5 - a.offset) / span) as f64
            } else {
                0.0
            };
            let point = |l: &::traffic::Lane, i: usize| {
                l.points[if l.reversed {
                    l.points.len() - 1 - i
                } else {
                    i
                }]
            };
            let points = (0..a.points.len())
                .map(|i| point(a, i).lerp(point(b, i), t))
                .collect();
            roads.push(MapRoad {
                points,
                width: hi - lo,
                main: lanes[start..end].iter().any(|l| l.speed_limit_kmh >= 55.0),
            });
            start = end;
        }
    }
    for lane in net
        .lanes
        .iter()
        .filter(|l| l.kind == LaneKind::Street && !l.invisible && l.points.len() >= 2)
    {
        for &j in &lane.next {
            let Some(next) = net
                .lanes
                .get(j)
                .filter(|l| l.kind == LaneKind::Street && !l.invisible && l.points.len() >= 2)
            else {
                continue;
            };
            let distance = lane.end().distance(next.start());
            if distance > 0.05 && distance <= 2.0 {
                roads.push(MapRoad {
                    points: vec![lane.end(), next.start()],
                    width: lane.width.min(next.width).max(2.6),
                    main: lane.speed_limit_kmh >= 55.0 && next.speed_limit_kmh >= 55.0,
                });
            }
        }
    }
    roads
}

pub(super) fn build_roads(p: &mut Painter, net: &Network, anchor: DVec2) {
    let rel = |q: DVec3| Vec3::new((q.x - anchor.x) as f32, (q.y - anchor.y) as f32, 0.0);
    let lanes: Vec<(MapRoad, Vec<Vec3>)> = road_geometry(net)
        .into_iter()
        .filter(|l| {
            l.points
                .iter()
                .any(|q| (q.truncate() - anchor).length() < ROAD_RADIUS)
        })
        .map(|l| {
            let pts = simplify(&l.points.iter().map(|q| rel(*q)).collect::<Vec<_>>(), 0.12);
            (l, pts)
        })
        .collect();
    for (l, pts) in &lanes {
        p.ribbon(pts, l.width + 1.6, 3.6, ROAD_CASING, true);
    }
    for (l, pts) in &lanes {
        p.ribbon(
            pts,
            l.width + 0.2,
            2.4,
            if l.main { ROAD_MAIN } else { ROAD },
            true,
        );
    }
}

pub(crate) fn simplify(pts: &[Vec3], tol: f32) -> Vec<Vec3> {
    if pts.len() < 3 {
        return pts.to_vec();
    }
    let mut keep = vec![false; pts.len()];
    keep[0] = true;
    keep[pts.len() - 1] = true;
    let mut stack = vec![(0usize, pts.len() - 1)];
    while let Some((a, b)) = stack.pop() {
        let (pa, pb) = (pts[a].truncate(), pts[b].truncate());
        let ab = pb - pa;
        let len = ab.length().max(1e-6);
        let mut worst = (0.0f32, 0usize);
        for k in a + 1..b {
            let d = (pts[k].truncate() - pa).perp_dot(ab).abs() / len;
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
    pts.iter()
        .zip(keep)
        .filter(|(_, k)| *k)
        .map(|(p, _)| *p)
        .collect()
}

pub(super) fn lanes_near(net: &Network, c: DVec2, radius: f64) -> Vec<usize> {
    let mut seen = hashbrown::HashSet::new();
    let (cx, cy) = Network::grid_cell(c.extend(0.0));
    let r = (radius / 50.0).ceil() as i32;
    if net.grid.is_empty() {
        return (0..net.lanes.len())
            .filter(|&i| {
                net.lanes[i]
                    .points
                    .iter()
                    .any(|q| (q.truncate() - c).length() < radius)
            })
            .collect();
    }
    for gx in cx - r..=cx + r {
        for gy in cy - r..=cy + r {
            if let Some(v) = net.grid.get(&(gx, gy)) {
                seen.extend(v.iter().copied());
            }
        }
    }
    let mut v: Vec<usize> = seen.into_iter().collect();
    v.sort_unstable();
    v
}

pub(super) fn probe_lanes(net: &Network) {
    let Ok(v) = ::legacy_config::env::var("OMSI_NAV_PROBE") else {
        return;
    };
    let f: Vec<f64> = v.split(',').filter_map(|x| x.trim().parse().ok()).collect();
    if f.len() < 2 {
        return;
    }
    let c = DVec2::new(f[0], f[1]);
    let r = f.get(2).copied().unwrap_or(25.0);
    for (i, l) in net.lanes.iter().enumerate() {
        let (s, e) = (l.start(), l.end());
        if (s.truncate() - c).length() > r
            && (e.truncate() - c).length() > r
            && l.nearest_point(DVec3::new(c.x, c.y, s.z))
            .map(|p| p.1 > r)
            .unwrap_or(true)
        {
            continue;
        }
        let prev = net.prev.get(i).cloned().unwrap_or_default();
        log::info!(
            "nav probe: lane {i} {:?} {:?} key {:?} rev {} start ({:.1}, {:.1}, {:.1}) h {:.0} end ({:.1}, {:.1}, {:.1}) h {:.0} len {:.1} next {:?} prev {:?}",
            l.kind,
            l.name,
            l.key,
            l.reversed,
            s.x,
            s.y,
            s.z,
            l.start_heading(),
            e.x,
            e.y,
            e.z,
            l.end_heading(),
            l.length(),
            l.next,
            prev
        );
    }
}

