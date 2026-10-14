use super::*;

impl Ui {
    #[allow(clippy::too_many_arguments)]
    pub(super) fn start_overview(&mut self, r: &Renderer, scene: &mut Scene, f: &Frame, card: [f32; 4], v: &StartView, u: f32, hits: &mut Hits) {
        self.text.alpha = self.start.sect_e;
        let pad = (48.0 * u).round();
        let (x0, x1) = (card[0] + pad, card[2] - pad);
        let name = v.rows.first().map(|r| r.1.as_str()).unwrap_or("-");
        let tpx = (46.0 * u) as u32;

        let dock_h = (112.0 * u).round();
        let dock = [card[0], card[3] - dock_h, card[2], card[3]];
        let gw = (260.0 * u).round();
        let go_h = (60.0 * u).round();
        let go = [x1 - gw, dock[1] + (dock_h - go_h) * 0.5, x1, dock[1] + (dock_h + go_h) * 0.5];

        let title = clip_to(&self.text, name, tpx as f32, (x1 - x0) * 0.7);
        let tl = self.text.label(r, scene, &title, tpx, WHITE);
        let ty = card[1] + 26.0 * u;
        tl.place(scene, x0, ty);
        self.text.rounded(r, scene, [x0, ty + tl.h as f32 + 4.0 * u, x0 + 48.0 * u, ty + tl.h as f32 + 7.0 * u], 0.0, ACCENT);
        let sy = ty + tl.h as f32 + 14.0 * u;
        let bs = (32.0 * u).round();
        let csz = (22.0 * u).round();
        let sub = if v.vehicles > 0 {
            format!("{}  {} / {}", v.maker, v.vehicle + 1, v.vehicles)
        } else {
            "-".to_string()
        };
        let sl = self.text.label(r, scene, sub.trim(), (17.0 * u) as u32, SOFT);
        let prev = [x0, sy, x0 + bs, sy + bs];
        let next = [prev[2] + 12.0 * u + sl.w as f32, sy, prev[2] + 12.0 * u + sl.w as f32 + bs, sy + bs];
        for (k, (b, name)) in [(prev, "chevron_left"), (next, "chevron_right")].into_iter().enumerate() {
            let av = self.ease((218, name, 0), if inside(b, f.cursor) { 1.0 } else { 0.0 }, 12.0);
            if let Some(ic) = self.text.icon(r, scene, name, csz as u32, mix(MUTED, WHITE, av)) {
                ic.place(scene, b[0] + (bs - csz) * 0.5, b[1] + (bs - csz) * 0.5);
            }
            hits.1[k] = b;
        }
        sl.place(scene, prev[2] + 6.0 * u, sy + (bs - sl.h as f32) * 0.5);

        if v.can_continue {
            let cl = self.text.label(r, scene, "start.maps.cont", (16.0 * u) as u32, SOFT);
            let ci = (20.0 * u).round();
            let total = cl.w as f32 + ci + 8.0 * u;
            let cy = ty + 14.0 * u;
            let rc = [x1 - total - 16.0 * u, cy - 8.0 * u, x1 + 4.0 * u, cy + cl.h as f32 + 8.0 * u];
            let hv = self.ease((220, "cont", 0), if inside(rc, f.cursor) { 1.0 } else { 0.0 }, 10.0);
            self.text.rounded(r, scene, rc, ROW_R * u, mix(fade([255, 255, 255, 10], 1.0), [255, 255, 255, 24], hv));
            let cx = x1 - total - 4.0 * u;
            if let Some(ic) = self.text.icon(r, scene, "history", ci as u32, mix(MUTED, ACCENT, hv)) {
                ic.place(scene, cx, cy + (cl.h as f32 - ci) * 0.5);
            }
            cl.place(scene, cx + ci + 8.0 * u, cy);
            self.start.cont = rc;
        }

        let (ay0, ay1) = (sy + bs + 16.0 * u, dock[1]);
        if v.on_foot {
            self.start_foot_note(r, scene, [card[0], ay0, card[2], ay1], u);
        } else {
            self.start_bus_picture(r, scene, [card[0], ay0, card[2], ay1], v, u);
        }

        self.text.rounded(r, scene, dock, 0.0, SIDEBAR);
        self.text.rounded(r, scene, [dock[0], dock[1], dock[2], dock[1] + 1.0], 0.0, [255, 255, 255, 18]);
        let map = v.maps.get(v.map).map(String::as_str).unwrap_or("-");
        let row = |i: usize| v.rows.get(i).cloned().unwrap_or_default();
        let items: [(&str, &str, String, String); 3] = [
            (
                "route",
                TAB_KEYS[0],
                map.to_string(),
                if v.free { tr("start.maps.free") } else { row(7).1 },
            ),
            (
                if v.on_foot { "directions_walk" } else { "directions_bus" },
                TAB_KEYS[1],
                if v.on_foot { tr("start.maps.onfoot_short") } else { row(0).1 },
                if v.on_foot { String::new() } else { v.maker.to_string() },
            ),
            (
                "schedule",
                TAB_KEYS[2],
                format!("{}  -  {}", row(1).1, row(6).1),
                row(5).1,
            ),
        ];
        let tgap = 12.0 * u;
        let tw = ((go[0] - 24.0 * u - x0 - tgap * 2.0) / 3.0).floor();
        let isz = (28.0 * u).round();
        for (k, (icon, key, value, sub)) in items.iter().enumerate() {
            let x = x0 + k as f32 * (tw + tgap);
            let rc = [x, dock[1] + 14.0 * u, x + tw, dock[3] - 14.0 * u];
            let th = rc[3] - rc[1];
            let hot = inside(rc, f.cursor);
            let on = v.focus == k;
            let hv = self.ease((220, "sum", k), if hot || on { 1.0 } else { 0.0 }, 10.0);
            self.text.rounded(r, scene, rc, ROW_R * u, mix(OPT_ROW, OPT_ROW_HOT, hv));
            let ov = self.ease((220, "sumon", k), if on { 1.0 } else { 0.0 }, 10.0);
            if ov > 0.0 {
                self.accent_bar(r, scene, rc, ov, false, u);
            }
            let ix = rc[0] + 18.0 * u;
            if let Some(ic) = self.text.icon(r, scene, icon, isz as u32, ACCENT) {
                ic.place(scene, ix, rc[1] + (th - isz) * 0.5);
            }
            let tx = ix + isz + 16.0 * u;
            let room = rc[2] - tx - 14.0 * u;
            let cl = self.text.label(r, scene, key, (11.0 * u) as u32, if hot || on { SOFT } else { MUTED });
            let vt = clip_to(&self.text, value, 19.0 * u, room);
            let vl = self.text.label(r, scene, &vt, (19.0 * u) as u32, WHITE);
            let st = clip_to(&self.text, sub, 13.0 * u, room);
            let stl = self.text.label(r, scene, &st, (13.0 * u) as u32, MUTED);
            let total = (cl.h + vl.h) as f32 + if sub.is_empty() { 0.0 } else { stl.h as f32 };
            let mut y = rc[1] + (th - total) * 0.5;
            cl.place(scene, tx, y);
            y += cl.h as f32;
            vl.place(scene, tx, y);
            y += vl.h as f32;
            if !sub.is_empty() {
                stl.place(scene, tx, y);
            }
            self.start.summary.push(rc);
        }
        self.start_button(r, scene, f, go, "start.maps.drive", "play_arrow", 0, u);
        self.start.go = go;
    }

    pub(super) fn start_preview(&mut self, r: &Renderer, scene: &mut Scene, f: &Frame, rc: [f32; 4], v: &StartView, u: f32, hits: &mut Hits) {
        let pad = (48.0 * u).round();
        let (x0, x1) = (rc[0] + pad, rc[2] - pad);
        let row = |i: usize| v.rows.get(i).cloned().unwrap_or_default();
        if v.sect == 3 {
            let cpx = ((rc[3] - rc[1]) * 0.30).min((rc[2] - rc[0]) * 0.22).max(40.0 * u) as u32;
            let cl = self.text.label(r, scene, &row(1).1, cpx, WHITE);
            let dpx = (26.0 * u) as u32;
            let dl = self.text.label(r, scene, &row(6).1, dpx, SOFT);
            let wl = self.text.label(r, scene, &format!("{}  -  {}", row(8).1, row(5).1), (20.0 * u) as u32, MUTED);
            let wi = (28.0 * u).round();
            let total = (cl.h + dl.h) as f32 + wi + 40.0 * u;
            let mut y = (rc[1] + rc[3] - total) * 0.5;
            let cx = (rc[0] + rc[2]) * 0.5;
            cl.place(scene, cx - cl.w as f32 * 0.5, y);
            y += cl.h as f32 + 6.0 * u;
            dl.place(scene, cx - dl.w as f32 * 0.5, y);
            y += dl.h as f32 + 24.0 * u;
            let tw = wl.w as f32 + wi + 10.0 * u;
            if let Some(ic) = self.text.icon(r, scene, v.weathers.get(v.weather).map_or("wb_sunny", |w| w.0.as_str()), wi as u32, ACCENT) {
                ic.place(scene, cx - tw * 0.5, y + (wl.h as f32 - wi) * 0.5);
            }
            wl.place(scene, cx - tw * 0.5 + wi + 10.0 * u, y);
            return;
        }
        if v.sect == 2 && v.on_foot {
            self.start_foot_note(r, scene, rc, u);
            return;
        }
        let map = v.maps.get(v.map).cloned().unwrap_or_else(|| "-".to_string());
        let (title, sub, icon) = match v.sect {
            1 => (map, if v.free { tr("start.maps.free") } else { v.route_sub.to_string() }, "map".to_string()),
            2 => (
                row(0).1,
                format!("{}  {} / {}", v.maker, v.vehicle + 1, v.vehicles),
                "directions_bus".to_string(),
            ),
            _ => (
                row(5).1,
                String::new(),
                v.weathers.get(v.weather).map(|w| w.0.clone()).unwrap_or_else(|| "wb_sunny".to_string()),
            ),
        };
        let tpx = (48.0 * u) as u32;
        let t = clip_to(&self.text, &title, tpx as f32, x1 - x0);
        let tl = self.text.label(r, scene, &t, tpx, WHITE);
        tl.place(scene, x0, rc[1] + pad);
        let mut top = rc[1] + pad + tl.h as f32;
        if v.sect == 2 {
            let bs = (30.0 * u).round();
            let csz = (22.0 * u).round();
            let sy = top + 4.0 * u;
            let name = clip_to(&self.text, v.livery, 18.0 * u, (x1 - x0 - bs * 2.0 - 24.0 * u).max(40.0 * u));
            let sl = self.text.label(r, scene, &name, (18.0 * u) as u32, SOFT);
            let prev = [x0, sy, x0 + bs, sy + bs];
            let nx = prev[2] + 8.0 * u + sl.w as f32 + 8.0 * u;
            let next = [nx, sy, nx + bs, sy + bs];
            for (k, (b, ic)) in [(prev, "chevron_left"), (next, "chevron_right")].into_iter().enumerate() {
                let av = self.ease((218, ic, 1), if inside(b, f.cursor) { 1.0 } else { 0.0 }, 12.0);
                if let Some(i) = self.text.icon(r, scene, ic, csz as u32, mix(MUTED, WHITE, av)) {
                    i.place(scene, b[0] + (bs - csz) * 0.5, b[1] + (bs - csz) * 0.5);
                }
                hits.1[22 + k] = b;
            }
            sl.place(scene, prev[2] + 8.0 * u, sy + (bs - sl.h as f32) * 0.5);
            top = sy + bs + 4.0 * u;
        } else {
            let s = clip_to(&self.text, sub.trim(), 18.0 * u, x1 - x0);
            let sl = self.text.label(r, scene, &s, (18.0 * u) as u32, SOFT);
            sl.place(scene, x0, top + 6.0 * u);
            top += sl.h as f32 + 6.0 * u;
        }
        let area = [rc[0] + pad, top + 18.0 * u, rc[2] - pad, rc[3] - pad];
        if v.sect == 2 {
            self.start_bus_picture(r, scene, area, v, u);
        } else if v.sect == 1 {
            self.start_route_picture(r, scene, area, u);
        } else {
            self.start_art(r, scene, area, &icon);
        }
    }

    pub(super) fn start_route_picture(&mut self, r: &Renderer, scene: &mut Scene, a: [f32; 4], u: f32) {
        let (w, h) = ((a[2] - a[0]).round(), (a[3] - a[1]).round());
        if w < 40.0 || h < 40.0 {
            return;
        }
        self.start.route_size = (w as u32, h as u32);
        self.start.route_rect = a;
        if let Some(tex) = self.start.route_pic {
            if self.start.enter > 0.7 {
                scene.overlays.push((tex, a));
                let px = (13.0 * u) as u32;
                for (name, nx, ny) in self.start.stops.clone() {
                    let (dx, dy) = (a[0] + nx * w, a[1] + ny * h);
                    let l = self.text.label(r, scene, &name, px, WHITE);
                    let mut x = dx + 10.0 * u;
                    if x + l.w as f32 > a[2] - 4.0 * u {
                        x = dx - 10.0 * u - l.w as f32;
                    }
                    l.place(scene, x.max(a[0] + 4.0 * u), (dy - l.h as f32 * 0.5).clamp(a[1], a[3] - l.h as f32));
                }
            }
            return;
        }
        let text = self.start.route_status.clone();
        if !text.is_empty() {
            let l = self.text.label(r, scene, &clip_to(&self.text, &text, 15.0 * u, w - 24.0 * u), (15.0 * u) as u32, MUTED);
            l.place(scene, a[0] + (w - l.w as f32) * 0.5, (a[1] + a[3] - l.h as f32) * 0.5);
        }
    }
}
