use super::*;

fn route_image(mini: &minimap::Minimap, trips: &[String], entry: Option<u32>, w: u32, h: u32, zoom: f32, pan: (f32, f32)) -> (Vec<u8>, Vec<(String, f32, f32)>, Vec<(u32, f32, f32)>) {
    const BG: [u8; 3] = [24, 24, 26];
    let (wu, hu) = (w.max(2) as usize, h.max(2) as usize);
    let mut img = vec![255u8; wu * hu * 4];
    for px in img.chunks_exact_mut(4) {
        px[..3].copy_from_slice(&BG);
    }
    let mut route: Vec<Vec<(f64, f64)>> = Vec::new();
    let mut seen = std::collections::HashSet::new();
    for t in trips {
        let trip = mini
            .trips
            .get(t)
            .or_else(|| mini.trips.iter().find(|(k, _)| k.eq_ignore_ascii_case(t)).map(|x| x.1));
        for &l in trip.map(|x| x.lanes.as_slice()).unwrap_or(&[]) {
            if seen.insert(l) {
                if let Some(lane) = mini.lanes.get(l as usize) {
                    route.push(lane.points.iter().map(|p| (p.x, p.y)).collect());
                }
            }
        }
    }
    let mut b = (f64::MAX, f64::MAX, f64::MIN, f64::MIN);
    let mut grow = |x: f64, y: f64| {
        b = (b.0.min(x), b.1.min(y), b.2.max(x), b.3.max(y));
    };
    if route.is_empty() {
        for r in &mini.roads {
            for p in &r.points {
                grow(p.x, p.y);
            }
        }
    } else {
        for l in &route {
            for p in l {
                grow(p.0, p.1);
            }
        }
    }
    if b.0 > b.2 {
        return (img, Vec::new(), Vec::new());
    }
    let (min_ext, pad) = if route.is_empty() { (100.0, 0.04) } else { (500.0, 0.2) };
    let bw = (b.2 - b.0).max(min_ext);
    let bh = (b.3 - b.1).max(min_ext);
    let (cx, cy) = ((b.0 + b.2) * 0.5, (b.1 + b.3) * 0.5);
    let k = (wu as f64 / (bw * (1.0 + 2.0 * pad))).min(hu as f64 / (bh * (1.0 + 2.0 * pad))) * zoom as f64;
    let to = |x: f64, y: f64| -> (f32, f32) {
        (
            (wu as f64 * 0.5 + (x - cx) * k) as f32 + pan.0,
            (hu as f64 * 0.5 - (y - cy) * k) as f32 + pan.1,
        )
    };
    for main in [false, true] {
        let (lo, hi, col) = if main { (2.0, 6.0, [74, 74, 80]) } else { (1.2, 3.5, [54, 54, 59]) };
        for road in mini.roads.iter().filter(|r| r.main == main) {
            let width = (road.width * k).clamp(lo, hi) as f32;
            let pts: Vec<(f32, f32)> = road.points.iter().map(|p| to(p.x, p.y)).collect();
            for s in pts.windows(2) {
                paint_seg(&mut img, wu, hu, s[0], s[1], width, col);
            }
        }
    }
    let line_px: Vec<Vec<(f32, f32)>> = route.iter().map(|l| l.iter().map(|p| to(p.0, p.1)).collect()).collect();
    for l in &line_px {
        for s in l.windows(2) {
            paint_seg(&mut img, wu, hu, s[0], s[1], 6.0, [255, 106, 0]);
        }
    }
    let mut named: Vec<(usize, String, (f32, f32))> = Vec::new();
    if !route.is_empty() {
        let flat: Vec<(f64, f64)> = route.iter().flatten().copied().collect();
        for stop in &mini.stops {
            let mut best = (f64::MAX, 0usize);
            for (i, s) in flat.windows(2).enumerate() {
                let d = seg_distance((stop.x, stop.y), s[0], s[1]);
                if d < best.0 {
                    best = (d, i);
                }
            }
            if best.0 < 35.0 {
                named.push((best.1, stop.name.trim().to_string(), to(stop.x, stop.y)));
            }
        }
        named.sort_by_key(|n| n.0);
        named.dedup_by(|a, b| a.1 == b.1);
    }
    for n in &named {
        paint_disc(&mut img, wu, hu, n.2, 6.0, [255, 255, 255]);
        paint_disc(&mut img, wu, hu, n.2, 3.5, BG);
    }
    let mut hits: Vec<(u32, f32, f32)> = Vec::new();
    let mut shown: Vec<u32> = Vec::new();
    for e in mini.entries.iter().filter(|e| Some(e.index) != entry) {
        if !shown.contains(&e.index) {
            shown.push(e.index);
            let c = to(e.x, e.y);
            paint_disc(&mut img, wu, hu, c, 6.5, [255, 255, 255]);
            paint_disc(&mut img, wu, hu, c, 4.0, BG);
            hits.push((e.index, c.0 / wu as f32, c.1 / hu as f32));
        }
    }
    if let Some(chosen) = entry {
        if let Some(e) = mini.entries.iter().find(|e| e.index == chosen) {
            let c = to(e.x, e.y);
            paint_disc(&mut img, wu, hu, c, 9.0, [255, 255, 255]);
            paint_disc(&mut img, wu, hu, c, 6.5, [255, 106, 0]);
            hits.push((e.index, c.0 / wu as f32, c.1 / hu as f32));
        }
    }
    hits.retain(|l| (0.0..=1.0).contains(&l.1) && (0.0..=1.0).contains(&l.2));
    let step = (named.len() as f32 / (10.0 * zoom.max(1.0))).max(1.0);
    let mut labels = Vec::new();
    let mut at = 0.0f32;
    while (at as usize) < named.len() {
        let n = &named[at as usize];
        labels.push((n.1.clone(), n.2 .0 / wu as f32, n.2 .1 / hu as f32));
        at += step;
    }
    if let Some(last) = named.last() {
        if labels.last().is_none_or(|l| l.0 != last.1) {
            labels.push((last.1.clone(), last.2 .0 / wu as f32, last.2 .1 / hu as f32));
        }
    }
    labels.retain(|l| (0.0..=1.0).contains(&l.1) && (0.0..=1.0).contains(&l.2));
    (img, labels, hits)
}

fn seg_distance(p: (f64, f64), a: (f64, f64), b: (f64, f64)) -> f64 {
    let (dx, dy) = (b.0 - a.0, b.1 - a.1);
    let len2 = dx * dx + dy * dy;
    let t = if len2 > 0.0 { (((p.0 - a.0) * dx + (p.1 - a.1) * dy) / len2).clamp(0.0, 1.0) } else { 0.0 };
    (p.0 - (a.0 + dx * t)).hypot(p.1 - (a.1 + dy * t))
}

fn paint_disc(img: &mut [u8], w: usize, h: usize, c: (f32, f32), r: f32, col: [u8; 3]) {
    let x0 = ((c.0 - r - 1.0).floor() as i32).max(0);
    let x1 = ((c.0 + r + 1.0).ceil() as i32).min(w as i32 - 1);
    let y0 = ((c.1 - r - 1.0).floor() as i32).max(0);
    let y1 = ((c.1 + r + 1.0).ceil() as i32).min(h as i32 - 1);
    for y in y0..=y1 {
        for x in x0..=x1 {
            let d = (x as f32 + 0.5 - c.0).hypot(y as f32 + 0.5 - c.1);
            let a = (r - d + 0.5).clamp(0.0, 1.0);
            if a <= 0.0 {
                continue;
            }
            let o = (y as usize * w + x as usize) * 4;
            for i in 0..3 {
                img[o + i] = (col[i] as f32 * a + img[o + i] as f32 * (1.0 - a)).round() as u8;
            }
        }
    }
}

fn paint_seg(img: &mut [u8], w: usize, h: usize, a: (f32, f32), b: (f32, f32), width: f32, col: [u8; 3]) {
    let m = width + 2.0;
    if (a.0 < -m && b.0 < -m) || (a.0 > w as f32 + m && b.0 > w as f32 + m) {
        return;
    }
    if (a.1 < -m && b.1 < -m) || (a.1 > h as f32 + m && b.1 > h as f32 + m) {
        return;
    }
    let len = (b.0 - a.0).hypot(b.1 - a.1);
    let n = ((len / (width * 0.3).max(0.5)).ceil() as usize).clamp(1, 40_000);
    for i in 0..=n {
        let t = i as f32 / n as f32;
        paint_disc(img, w, h, (a.0 + (b.0 - a.0) * t, a.1 + (b.1 - a.1) * t), width * 0.5, col);
    }
}

impl App {
    pub(super) fn start_entry_click(&mut self, cursor: (f32, f32)) {
        let Some(u) = self.ui.as_ref() else {
            return;
        };
        let a = u.start.route_rect;
        if a[2] <= a[0] || a[3] <= a[1] {
            return;
        }
        let (w, h) = (a[2] - a[0], a[3] - a[1]);
        let near = u
            .start
            .entry_hits
            .iter()
            .map(|&(i, nx, ny)| (i, (a[0] + nx * w - cursor.0).hypot(a[1] + ny * h - cursor.1)))
            .filter(|d| d.1 < 16.0)
            .min_by(|x, y| x.1.total_cmp(&y.1));
        let (Some((index, _)), Some(m)) = (near, self.menu.as_mut()) else {
            return;
        };
        if m.route && m.entry_chosen() == Some(index) {
            m.entry = None;
        } else {
            m.entry_pick(index);
        }
    }

    pub(super) fn start_route_preview(&mut self) {
        if let Some(m) = self.menu.as_mut() {
            m.want_icao();
            m.poll_icao();
        }
        let in_tab = self.menu.as_ref().is_some_and(|m| m.page == StartPage::Start && m.sect == 1);
        if !in_tab {
            if let Some(m) = self.menu.as_mut() {
                m.pic_key.clear();
            }
            if let Some(u) = self.ui.as_mut() {
                u.start.route_pic = None;
                u.start.stops.clear();
                u.start.entry_hits.clear();
            }
            return;
        }
        let cursor = self.ui_cursor();
        if let Some(m) = self.menu.as_mut() {
            m.want_mini();
            m.poll_mini();
            let base = format!(
                "{}|{}",
                m.maps.get(m.map).map_or("", |x| x.1.as_str()),
                match m.chosen_tour() {
                    Some((l, t)) if m.route => format!("{}/{}/{}", l.name, t.number, m.trip),
                    _ => String::new(),
                }
            );
            if m.view_for != base {
                m.view_for = base;
                m.zoom = 1.0;
                m.pan = (0.0, 0.0);
            }
            if let Some((from, pan0)) = m.drag {
                m.pan = (pan0.0 + cursor.0 - from.0, pan0.1 + cursor.1 - from.1);
            }
        }
        let cur_tex = self.menu.as_ref().and_then(|m| m.route_tex);
        let size = self.ui.as_ref().map(|u| u.start.route_size).filter(|s| s.0 >= 40 && s.1 >= 40);
        let Some((w, h)) = size else {
            return;
        };
        let (Some(m), Some(r), Some(scene), Some(ui)) =
            (self.menu.as_ref(), self.renderer.as_mut(), self.scene.as_mut(), self.ui.as_mut())
        else {
            return;
        };
        let map = m.mini_key().map(|(n, d)| format!("{n}|{d}")).unwrap_or_default();
        let trips: Vec<String> = match m.chosen_tour() {
            Some((_, t)) if m.route => match t.runs.get(m.trip) {
                Some(run) => vec![run.2.clone()],
                None => t.trips.iter().take(1).cloned().collect(),
            },
            _ => Vec::new(),
        };
        let have = m.mini.as_ref().is_some_and(|x| x.0 == map);
        ui.start.route_status = if have {
            String::new()
        } else if m.mini_failed.as_deref() == Some(map.as_str()) {
            ::i18n::translate("start.maps.nopreview", &[])
        } else {
            ::i18n::translate("start.maps.mappreview", &[])
        };
        let Some((_, mini)) = m.mini.as_ref().filter(|_| have) else {
            ui.start.route_pic = None;
            ui.start.stops.clear();
            ui.start.entry_hits.clear();
            return;
        };
        let entry = if m.route { m.entry_chosen() } else { Some(m.entry_shown()) };
        let key = format!("{map}|{}|{w}x{h}|{:.3}|{:.0}|{:.0}|{:?}", trips.join(","), m.zoom, m.pan.0, m.pan.1, entry);
        if m.pic_key == key && ui.start.route_pic.is_some() {
            return;
        }
        let (rgba, stops, entry_hits) = route_image(mini, &trips, entry, w, h, m.zoom, m.pan);
        let id = match cur_tex {
            Some((id, tw, th)) if (tw, th) == (w, h) => id,
            old => {
                if let Some((id, _, _)) = old {
                    r.free_texture(scene, id);
                }
                r.add_blank_texture(scene, w, h)
            }
        };
        r.update_texture(scene, id, &::texture::Image { width: w, height: h, rgba, has_alpha: true });
        ui.start.route_pic = Some(id);
        ui.start.stops = stops;
        ui.start.entry_hits = entry_hits;
        if let Some(m) = self.menu.as_mut() {
            m.route_tex = Some((id, w, h));
            m.pic_key = key;
        }
    }
}
