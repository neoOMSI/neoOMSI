use super::*;

pub(super) const SIGN_ALONG: f64 = 90.0;

pub(super) fn build_streets(net: &Network, signs: &[(DVec3, f64, String)]) -> Streets {
    let t0 = std::time::Instant::now();
    let n = net.lanes.len();
    let mut names: Vec<String> = Vec::new();
    let mut index: HashMap<String, u32> = HashMap::new();
    let mut of_lane = vec![u32::MAX; n];
    let mut prev: Vec<Vec<usize>> = vec![Vec::new(); n];
    for (i, l) in net.lanes.iter().enumerate() {
        for &j in &l.next {
            if j < n {
                prev[j].push(i);
            }
        }
    }
    let straight = |l: &::traffic::Lane| {
        l.kind == LaneKind::Street
            && ::traffic::wrap_deg(l.end_heading() - l.start_heading()).abs() < 30.0
    };
    let debug = ::legacy_config::env::var_os("OMSI_DEBUG_NAV").is_some();
    let mut hist = [0u32; 12];
    let mut seeds: Vec<(usize, u32)> = Vec::new();
    for (pos, rot, name) in signs {
        let id = *index.entry(name.clone()).or_insert_with(|| {
            names.push(name.clone());
            (names.len() - 1) as u32
        });
        let mut best: Option<(usize, f64)> = None;
        for i in lanes_near(net, pos.truncate(), 30.0) {
            let l = &net.lanes[i];
            if !straight(l) || l.length() < 12.0 {
                continue;
            }
            let Some((s, d)) = l.nearest_point(DVec3::new(pos.x, pos.y, l.start().z)) else {
                continue;
            };
            if d > 22.0 {
                continue;
            }
            let (_, h) = l.at(s);
            let off = (angle_diff(*rot, h as f64).abs() - SIGN_ALONG).abs();
            let off = off.min(180.0 - off);
            if debug && d < 12.0 {
                let raw = angle_diff(*rot, h as f64).rem_euclid(180.0);
                hist[((raw / 15.0) as usize).min(11)] += 1;
            }
            let score = d + off * 0.5;
            if off < 35.0 && best.map(|b| score < b.1).unwrap_or(true) {
                best = Some((i, score));
            }
        }
        if let Some((i, _)) = best {
            seeds.push((i, id));
        }
    }
    if debug {
        if let Some((pos, rot, name)) = signs.first() {
            let near = lanes_near(net, pos.truncate(), 30.0);
            let best = near
                .iter()
                .filter_map(|&i| {
                    net.lanes[i]
                        .nearest_point(*pos)
                        .map(|p| (i, p.1, net.lanes[i].kind, net.lanes[i].length()))
                })
                .min_by(|a, b| a.1.total_cmp(&b.1));
            log::info!(
                "navigator: first sign '{name}' at {pos:?} heading {rot}: {} lanes near, nearest {best:?}",
                near.len()
            );
        }
        let mut along = [0u32; 12];
        for (i, (p, r, n)) in signs.iter().enumerate() {
            for (q, _, m) in &signs[i + 1..] {
                let d = (*q - *p).truncate();
                if n == m && d.length() > 150.0 && d.length() < 700.0 {
                    let h = d.x.atan2(d.y).to_degrees();
                    let raw = angle_diff(*r, h).rem_euclid(180.0);
                    along[((raw / 15.0) as usize).min(11)] += 1;
                }
            }
        }
        log::info!(
            "navigator: sign heading minus the line to another sign of its name (mod 180): {along:?}"
        );
        log::info!(
            "navigator: sign heading minus road heading (mod 180, 15-degree bins): {hist:?}"
        );
    }
    for &(seed, id) in &seeds {
        if of_lane[seed] != u32::MAX {
            continue;
        }
        let mut queue = vec![(seed, 0.0f32)];
        while let Some((i, far)) = queue.pop() {
            if of_lane[i] != u32::MAX && i != seed {
                continue;
            }
            of_lane[i] = id;
            let l = &net.lanes[i];
            if let Some(k) = l.key {
                for &j in net.by_key.get(&k).map(|v| v.as_slice()).unwrap_or(&[]) {
                    if of_lane[j] == u32::MAX {
                        of_lane[j] = id;
                    }
                }
            }
            if far > 1500.0 {
                continue;
            }
            for &j in l.next.iter() {
                let m = &net.lanes[j];
                if of_lane[j] == u32::MAX
                    && straight(m)
                    && ::traffic::wrap_deg(m.start_heading() - l.end_heading()).abs() < 20.0
                {
                    queue.push((j, far + m.length()));
                }
            }
            for &j in &prev[i] {
                let m = &net.lanes[j];
                if of_lane[j] == u32::MAX
                    && straight(m)
                    && ::traffic::wrap_deg(l.start_heading() - m.end_heading()).abs() < 20.0
                {
                    queue.push((j, far + m.length()));
                }
            }
        }
    }
    let mut order: Vec<usize> = (0..n)
        .filter(|&i| {
            of_lane[i] != u32::MAX && !net.lanes[i].reversed && net.lanes[i].length() > 30.0
        })
        .collect();
    order.sort_by(|a, b| net.lanes[*b].length().total_cmp(&net.lanes[*a].length()));
    let mut labels: Vec<(DVec2, f32, u32)> = Vec::new();
    for i in order {
        let l = &net.lanes[i];
        let (q, h) = l.at(l.length() * 0.5);
        let q = q.truncate();
        if labels
            .iter()
            .any(|(p, _, id)| *id == of_lane[i] && (*p - q).length() < 350.0)
        {
            continue;
        }
        let hr = (h as f64).to_radians();
        labels.push((q, (hr.cos()).atan2(hr.sin()) as f32, of_lane[i]));
    }
    let named = of_lane.iter().filter(|&&x| x != u32::MAX).count();
    log::info!(
        "navigator: {} street name signs, {} names, {} of {} lanes named, {} labels, {:.0} ms",
        signs.len(),
        names.len(),
        named,
        n,
        labels.len(),
        t0.elapsed().as_secs_f64() * 1000.0
    );
    Streets {
        names,
        of_lane,
        labels,
    }
}

