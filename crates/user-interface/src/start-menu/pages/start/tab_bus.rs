use super::*;

impl Ui {
    #[allow(clippy::too_many_arguments)]
    pub(super) fn start_tab_bus(&mut self, r: &Renderer, scene: &mut Scene, f: &Frame, a: [f32; 4], v: &StartView, u: f32, hits: &mut Hits) {
        let th = (54.0 * u).round();
        let g = 6.0 * u;
        let mut y = a[1];

        let foot = [a[0], y, a[2], y + th];
        self.start_toggle(r, scene, f, foot, "start.maps.onfoot", v.on_foot, u);
        self.start.foot = foot;
        y += th + g + 6.0 * u;
        if v.on_foot {
            return;
        }

        let lab = tr("start.maps.livery");
        self.start_dropdown(r, scene, f, [a[0], y, a[2], y + th], 0, &lab, v.livery, v.drop == 8, u);
        y += th + g;
        if v.drop == 8 {
            y = self.start_drop_list(r, scene, f, a, y, v, u) + g;
        }

        let dh = (32.0 * u).round();
        let dr = [a[0], y, a[2], y + dh];
        let hv = self.ease((225, "details", 0), if inside(dr, f.cursor) { 1.0 } else { 0.0 }, 10.0);
        let isz = (20.0 * u).round();
        let icon = if v.details { "expand_more" } else { "chevron_right" };
        if let Some(ic) = self.text.icon(r, scene, icon, isz as u32, mix(MUTED, WHITE, hv)) {
            ic.place(scene, dr[0] + 2.0 * u, y + (dh - isz) * 0.5);
        }
        let dl = self.text.label(r, scene, "start.maps.details", (15.0 * u) as u32, mix(SOFT, WHITE, hv));
        dl.place(scene, dr[0] + isz + 8.0 * u, y + (dh - dl.h as f32) * 0.5);
        self.start.details = dr;
        y += dh + g;
        if v.details {
            for (k, key, val) in [(1usize, "start.maps.depot", v.depot), (2, "start.maps.fleet", v.fleet)] {
                let lab = tr(key);
                self.start_dropdown(r, scene, f, [a[0], y, a[2], y + th], k, &lab, val, v.drop == 8 + k, u);
                y += th + g;
                if v.drop == 8 + k {
                    y = self.start_drop_list(r, scene, f, a, y, v, u) + g;
                }
            }
            let rc = [a[0], y, a[2], y + th];
            let lab = tr("start.maps.plate");
            let auto = tr("start.maps.auto");
            self.start_field(r, scene, f, rc, Some(lab.as_str()), None, v.plate, &auto, v.edit == 2, v.caret, 1, u);
            self.start.plate = rc;
            y += th + g;
        }

        let ch = self.start_caption(r, scene, "start.maps.buses", a[0] + 4.0 * u, y + 6.0 * u, u);
        y += ch + 14.0 * u;

        let sh = (42.0 * u).round();
        let rc = [a[0], y, a[2], y + sh];
        let ph = tr("start.maps.search");
        self.start_field(r, scene, f, rc, None, Some("search"), v.query, &ph, v.edit == 1, v.caret, 0, u);
        self.start.search = rc;
        y += sh + g + 2.0 * u;

        let list = [a[0], y, a[2], a[3].max(y + ROW_H * u * 3.0)];
        let vis = self.start_veh_list(r, scene, f, list, v, u);
        self.start.veh_vis = vis;
    }

    #[allow(clippy::too_many_arguments)]
    pub(super) fn start_dropdown(&mut self, r: &Renderer, scene: &mut Scene, f: &Frame, rc: [f32; 4], k: usize, label: &str, value: &str, open: bool, u: f32) {
        let h = rc[3] - rc[1];
        let hot = inside(rc, f.cursor);
        let hv = self.ease((231, "drop", k), if open || hot { 1.0 } else { 0.0 }, 10.0);
        self.text.rounded(r, scene, rc, ROW_R * u, mix(OPT_ROW, OPT_ROW_HOT, hv));
        let fv = self.ease((231, "dropon", k), if open { 1.0 } else { 0.0 }, 10.0);
        if fv > 0.0 {
            self.accent_bar(r, scene, rc, fv, false, u);
        }
        let isz = (22.0 * u).round();
        let icon = if open { "expand_less" } else { "expand_more" };
        if let Some(ic) = self.text.icon(r, scene, icon, isz as u32, mix(MUTED, WHITE, hv)) {
            ic.place(scene, rc[2] - 12.0 * u - isz, rc[1] + (h - isz) * 0.5);
        }
        let tx = rc[0] + TEXT_IN * u;
        let room = (rc[2] - 12.0 * u - isz - 8.0 * u - tx).max(30.0 * u);
        let text = clip_to(&self.text, value, 16.0 * u, room);
        let vl = self.text.label(r, scene, &text, (16.0 * u) as u32, if open { ACCENT_TEXT } else { WHITE });
        if label.is_empty() {
            vl.place(scene, tx, rc[1] + (h - vl.h as f32) * 0.5);
        } else {
            let ll = self.text.label(r, scene, label, (11.0 * u) as u32, if open || hot { SOFT } else { MUTED });
            let y = rc[1] + (h - (ll.h + vl.h) as f32) * 0.5;
            ll.place(scene, tx, y);
            vl.place(scene, tx, y + ll.h as f32);
        }
        self.start.drops.push(rc);
    }

    #[allow(clippy::too_many_arguments)]
    pub(super) fn start_drop_list(&mut self, r: &Renderer, scene: &mut Scene, f: &Frame, a: [f32; 4], y: f32, v: &StartView, u: f32) -> f32 {
        let row_h = (ROW_H * u).round();
        let n = v.drop_items.len();
        let vis = n.clamp(1, 6);
        let pad = 4.0 * u;
        let bx = [a[0], y, a[2], y + vis as f32 * row_h + pad * 2.0];
        self.text.rounded(r, scene, bx, ROW_R * u, OPT_ROW);
        let first = v.drop_top.min(n.saturating_sub(vis));
        let bar_room = if n > vis { 12.0 * u } else { 0.0 };
        let px = (15.0 * u) as u32;
        for (k, i) in (first..n.min(first + vis)).enumerate() {
            let ry = y + pad + k as f32 * row_h;
            let rc = [a[0] + pad, ry, a[2] - pad - bar_room, ry + row_h];
            let on = v.drop_sel == i;
            let hot = inside(rc, f.cursor);
            let hv = self.ease((232, "item", i), if hot || on { 1.0 } else { 0.0 }, 10.0);
            if hv > 0.0 {
                let base = if on { OPT_GROUP_ON } else { OPT_GROUP_HOT };
                self.text.rounded(r, scene, rc, ROW_R * u, fade(base, if on { 1.0 } else { hv }));
            }
            let name = clip_to(&self.text, &v.drop_items[i], px as f32, rc[2] - rc[0] - TEXT_IN * u * 2.0);
            let l = self.text.label(r, scene, &name, px, if on || hot { WHITE } else { SOFT });
            l.place(scene, rc[0] + TEXT_IN * u, ry + (row_h - l.h as f32) * 0.5);
            self.start.drop_items.push(rc);
        }
        if n > vis {
            let track = [a[2] - pad - 6.0 * u, y + pad, a[2] - pad, y + pad + vis as f32 * row_h];
            self.text.rounded(r, scene, track, 3.0 * u, OPT_TRACK);
            let th = ((vis as f32 / n as f32) * (track[3] - track[1])).max(24.0 * u);
            let ty = track[1] + (track[3] - track[1] - th) * first as f32 / (n - vis) as f32;
            self.text.rounded(r, scene, [track[0], ty, track[2], ty + th], 3.0 * u, [255, 255, 255, 60]);
        }
        self.start.drop_box = bx;
        self.start.drop_first = first;
        bx[3]
    }

    #[allow(clippy::too_many_arguments)]
    pub(super) fn start_field(
        &mut self,
        r: &Renderer,
        scene: &mut Scene,
        f: &Frame,
        rc: [f32; 4],
        label: Option<&str>,
        icon: Option<&str>,
        value: &str,
        placeholder: &str,
        focus: bool,
        caret: bool,
        id: usize,
        u: f32,
    ) {
        let h = rc[3] - rc[1];
        let hv = self.ease((226, "field", id), if focus || inside(rc, f.cursor) { 1.0 } else { 0.0 }, 10.0);
        self.text.rounded(r, scene, rc, ROW_R * u, mix(OPT_ROW, OPT_ROW_HOT, hv));
        let fv = self.ease((226, "fieldon", id), if focus { 1.0 } else { 0.0 }, 10.0);
        if fv > 0.0 {
            self.accent_bar(r, scene, rc, fv, false, u);
        }
        let mut tx = rc[0] + TEXT_IN * u;
        if let Some(name) = icon {
            let isz = (20.0 * u).round();
            if let Some(ic) = self.text.icon(r, scene, name, isz as u32, MUTED) {
                ic.place(scene, tx - 4.0 * u, rc[1] + (h - isz) * 0.5);
            }
            tx += isz + 6.0 * u;
        }
        let vpx = (16.0 * u) as u32;
        let room = rc[2] - tx - TEXT_IN * u;
        let shown = if value.is_empty() { placeholder.to_string() } else { value.to_string() };

        let mut text = shown.clone();
        while !value.is_empty() && self.text.width(&text, vpx as f32) > room && text.chars().count() > 1 {
            text.remove(0);
        }
        let col = if value.is_empty() { MUTED } else { WHITE };
        let vl = self.text.label(r, scene, &text, vpx, col);
        let (ly, vy) = match label {
            Some(l) => {
                let ll = self.text.label(r, scene, l, (11.0 * u) as u32, if focus { SOFT } else { MUTED });
                let total = (ll.h + vl.h) as f32;
                let y = rc[1] + (h - total) * 0.5;
                ll.place(scene, tx, y);
                (y, y + ll.h as f32)
            }
            None => (rc[1], rc[1] + (h - vl.h as f32) * 0.5),
        };
        let _ = ly;
        vl.place(scene, tx, vy);
        if focus && caret {
            let cx = tx + if value.is_empty() { 0.0 } else { vl.w as f32 } + 2.0 * u;
            self.text.rounded(r, scene, [cx, vy + 2.0 * u, cx + 2.0 * u, vy + vl.h as f32 - 2.0 * u], 0.0, ACCENT);
        }
    }

    pub(super) fn start_veh_list(&mut self, r: &Renderer, scene: &mut Scene, f: &Frame, a: [f32; 4], v: &StartView, u: f32) -> usize {
        let row_h = (ROW_H * u).round();
        let vis = (((a[3] - a[1]) / row_h).floor() as usize).max(1);
        let rows = v.veh_rows;
        let n = rows.len();
        if n == 0 {
            let l = self.text.label(r, scene, "start.maps.nobuses", (15.0 * u) as u32, MUTED);
            l.place(scene, a[0] + 4.0 * u, a[1] + 6.0 * u);
            return vis;
        }
        let first = v.veh_top.min(n.saturating_sub(vis));
        let bar_room = if n > vis { 14.0 * u } else { 0.0 };
        let px = (15.0 * u) as u32;
        for (k, i) in (first..n.min(first + vis)).enumerate() {
            let row = &rows[i];
            let y = a[1] + k as f32 * row_h;
            let rc = [a[0], y, a[2] - bar_room, y + row_h];
            let hot = inside(rc, f.cursor);
            let header = row.vehicle.is_none();
            let hv = self.ease((222, "item", i), if hot || row.on { 1.0 } else { 0.0 }, 10.0);
            if hv > 0.0 {
                let base = if row.on { OPT_GROUP_ON } else { OPT_GROUP_HOT };
                self.text.rounded(r, scene, rc, ROW_R * u, fade(base, if row.on { 1.0 } else { hv }));
            }
            let ov = self.ease((222, "itemon", i), if row.on { 1.0 } else { 0.0 }, 10.0);
            if ov > 0.0 {
                self.accent_bar(r, scene, rc, ov, false, u);
            }
            if header {
                let isz = (22.0 * u).round();
                let ix = rc[2] - TEXT_IN * u * 0.5 - isz;
                if let Some(ic) = self.text.icon(r, scene, if row.open { "expand_less" } else { "expand_more" }, isz as u32, mix(MUTED, WHITE, hv)) {
                    ic.place(scene, ix, y + (row_h - isz) * 0.5);
                }
                let cl = self.text.label(r, scene, &row.count.to_string(), px, MUTED);
                let cx = ix - 10.0 * u - cl.w as f32;
                cl.place(scene, cx, y + (row_h - cl.h as f32) * 0.5);
                let name = clip_to(&self.text, &row.label, px as f32, cx - rc[0] - TEXT_IN * u - 8.0 * u);
                let l = self.text.label(r, scene, &name, px, WHITE);
                l.place(scene, rc[0] + TEXT_IN * u, y + (row_h - l.h as f32) * 0.5);
            } else {
                let ix = rc[0] + TEXT_IN * u + 18.0 * u;
                let name = clip_to(&self.text, &row.label, px as f32, rc[2] - ix - TEXT_IN * u);
                let l = self.text.label(r, scene, &name, px, if row.on || hot { WHITE } else { SOFT });
                l.place(scene, ix, y + (row_h - l.h as f32) * 0.5);
            }
            if header && k > 0 {
                self.text.rounded(r, scene, [rc[0] + 4.0 * u, y, rc[2] - 4.0 * u, y + 1.0], 0.0, [255, 255, 255, 14]);
            }
            self.start.veh.push(rc);
        }
        if n > vis {
            let track = [a[2] - 6.0 * u, a[1], a[2], a[1] + vis as f32 * row_h];
            self.text.rounded(r, scene, track, 3.0 * u, OPT_TRACK);
            let th = ((vis as f32 / n as f32) * (track[3] - track[1])).max(24.0 * u);
            let ty = track[1] + (track[3] - track[1] - th) * first as f32 / (n - vis) as f32;
            self.text.rounded(r, scene, [track[0], ty, track[2], ty + th], 3.0 * u, [255, 255, 255, 60]);
        }
        vis
    }

    pub(super) fn start_foot_note(&mut self, r: &Renderer, scene: &mut Scene, a: [f32; 4], u: f32) {
        let l = self.text.label(r, scene, "start.maps.onfoot_short", (15.0 * u) as u32, MUTED);
        l.place(scene, (a[0] + a[2] - l.w as f32) * 0.5, (a[1] + a[3] - l.h as f32) * 0.5);
    }

    pub(super) fn start_bus_picture(&mut self, r: &Renderer, scene: &mut Scene, a: [f32; 4], v: &StartView, u: f32) {
        let (w, h) = ((a[2] - a[0]).round(), (a[3] - a[1]).round());
        if w < 40.0 || h < 40.0 {
            return;
        }
        self.start.pic_size = (w as u32, h as u32);
        self.start.pic_rect = a;
        if let Some(tex) = self.start.picture {
            if self.start.enter > 0.7 {
                scene.overlays.push((tex, a));
            }
            return;
        }
        let text = if self.start.status.is_empty() { tr("start.maps.loading") } else { self.start.status.clone() };
        let _ = v;
        let l = self.text.label(r, scene, &clip_to(&self.text, &text, 15.0 * u, w - 24.0 * u), (15.0 * u) as u32, MUTED);
        l.place(scene, a[0] + (w - l.w as f32) * 0.5, (a[1] + a[3] - l.h as f32) * 0.5);
    }
}
