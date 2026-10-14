use super::*;

impl Ui {
    pub(super) fn start_section_head(&mut self, r: &Renderer, scene: &mut Scene, key: &str, rc: [f32; 4], u: f32) {
        let l = self.text.label(r, scene, key, (17.0 * u) as u32, WHITE);
        l.place(scene, rc[0] + 4.0 * u, rc[1] + (rc[3] - rc[1] - l.h as f32) * 0.5);
    }

    pub(super) fn start_toggle(&mut self, r: &Renderer, scene: &mut Scene, f: &Frame, rc: [f32; 4], key: &str, on: bool, u: f32) {
        let h = rc[3] - rc[1];
        let hv = self.ease((234, "tog", 0), if inside(rc, f.cursor) { 1.0 } else { 0.0 }, 10.0);
        let ov = self.ease((234, "togon", 0), if on { 1.0 } else { 0.0 }, 12.0);
        self.text.rounded(r, scene, rc, ROW_R * u, mix(OPT_ROW, OPT_ROW_HOT, hv));
        let l = self.text.label(r, scene, key, (16.0 * u) as u32, if hv > 0.0 { WHITE } else { SOFT });
        l.place(scene, rc[0] + TEXT_IN * u, rc[1] + (h - l.h as f32) * 0.5);
        let (sw, sh) = ((50.0 * u).round(), (28.0 * u).round());
        let sx = rc[2] - 14.0 * u - sw;
        let sy = rc[1] + (h - sh) * 0.5;
        self.text.rounded(r, scene, [sx, sy, sx + sw, sy + sh], sh * 0.5, mix(OPT_GROUP, ACCENT, ov));
        let k = sh - 8.0 * u;
        let kx = sx + 4.0 * u + (sw - sh) * ov;
        self.text.rounded(r, scene, [kx, sy + 4.0 * u, kx + k, sy + 4.0 * u + k], k * 0.5, WHITE_FILL);
    }

    #[allow(clippy::too_many_arguments)]
    pub(super) fn start_pick_list(
        &mut self,
        r: &Renderer,
        scene: &mut Scene,
        f: &Frame,
        a: [f32; 4],
        items: &[(String, String)],
        badge: bool,
        sel: Option<usize>,
        top: usize,
        id: u8,
        u: f32,
        rects: &mut Vec<[f32; 4]>,
        focus: bool,
        empty: &str,
    ) -> usize {
        let row_h = (ROW_H * u).round();
        let vis = (((a[3] - a[1]) / row_h).floor() as usize).max(1);
        let n = items.len();
        if n == 0 {
            let l = self.text.label(r, scene, empty, (15.0 * u) as u32, MUTED);
            l.place(scene, a[0] + 4.0 * u, a[1] + 6.0 * u);
            return vis;
        }
        let first = top.min(n.saturating_sub(vis));
        let bar_room = if n > vis { 14.0 * u } else { 0.0 };
        let px = (15.0 * u) as u32;
        let (i0, frac) = self.glide_rows(id, first);
        let bottom = a[1] + vis as f32 * row_h;
        let slots = vis.min(n - first);
        let mut slot_rects = vec![[0.0f32; 4]; slots];
        let alpha0 = self.text.alpha;
        for i in i0..n.min(i0 + vis + 1) {
            let y = a[1] + ((i - i0) as f32 - frac) * row_h;
            let Some((vy0, vy1, seen)) = row_seen(a[1], bottom, y, row_h) else {
                continue;
            };
            let rc = [a[0], vy0, a[2] - bar_room, vy1];
            self.text.alpha = alpha0 * seen.powi(3);
            let on = sel == Some(i);
            let hot = inside(rc, f.cursor);
            // a row partly under the open dropdown keeps its highlight from showing past its edge
            let d = self.start.drop_last;
            let hidden = rc[0] < d[2] && rc[2] > d[0] && rc[1] < d[3] && rc[3] > d[1];
            let hv = self.ease((id, "item", i), if (hot || on) && !hidden { 1.0 } else { 0.0 }, 10.0);
            if hv > 0.0 {
                let base = if on { OPT_GROUP_ON } else { OPT_GROUP_HOT };
                self.text.rounded(r, scene, rc, ROW_R * u, fade(base, if on { 1.0 } else { hv }));
            }
            let ov = self.ease((id, "itemon", i), if on && focus && !hidden { 1.0 } else { 0.0 }, 10.0);
            if ov > 0.0 {
                self.accent_bar(r, scene, rc, ov, false, u);
            }
            let col = if on || hot { WHITE } else { SOFT };
            let (left, right) = &items[i];
            if badge {
                let bl = self.text.label(r, scene, left, px, ACCENT_TEXT);
                let bw = (bl.w as f32 + 16.0 * u).max(40.0 * u);
                let bh = row_h - 10.0 * u;
                let br = [rc[0] + TEXT_IN * u * 0.5, y + 5.0 * u, rc[0] + TEXT_IN * u * 0.5 + bw, y + 5.0 * u + bh];
                self.text.rounded(r, scene, br, 6.0 * u, OPT_ROW);
                bl.place(scene, br[0] + (bw - bl.w as f32) * 0.5, y + (row_h - bl.h as f32) * 0.5);
                let tx = br[2] + 12.0 * u;
                let name = clip_to(&self.text, right, px as f32, rc[2] - tx - TEXT_IN * u);
                let l = self.text.label(r, scene, &name, px, col);
                l.place(scene, tx, y + (row_h - l.h as f32) * 0.5);
            } else {
                let tl = self.text.label(r, scene, right, px, MUTED);
                let tx = rc[2] - TEXT_IN * u - tl.w as f32;
                tl.place(scene, tx, y + (row_h - tl.h as f32) * 0.5);
                let name = clip_to(&self.text, left, px as f32, tx - rc[0] - TEXT_IN * u - 12.0 * u);
                let l = self.text.label(r, scene, &name, px, col);
                l.place(scene, rc[0] + TEXT_IN * u, y + (row_h - l.h as f32) * 0.5);
            }
            if i >= first && i - first < slots {
                slot_rects[i - first] = rc;
            }
        }
        self.text.alpha = alpha0;
        rects.extend(slot_rects);
        if n > vis {
            let track = [a[2] - 6.0 * u, a[1], a[2], a[1] + vis as f32 * row_h];
            self.text.rounded(r, scene, track, 3.0 * u, OPT_TRACK);
            let th = ((vis as f32 / n as f32) * (track[3] - track[1])).max(24.0 * u);
            let ty = track[1] + (track[3] - track[1] - th) * (i0 as f32 + frac) / (n - vis) as f32;
            self.text.rounded(r, scene, [track[0], ty, track[2], ty + th], 3.0 * u, [255, 255, 255, 60]);
        }
        vis
    }

    pub(super) fn glide_rows(&mut self, id: u8, target: usize) -> (usize, f32) {
        use std::hash::{Hash, Hasher};
        let mut h = std::collections::hash_map::DefaultHasher::new();
        (id, "glide", 0usize).hash(&mut h);
        let k = 1.0 - (-self.anim_dt * 20.0).exp();
        let t = target as f32;
        let v = self.anim.entry(h.finish()).or_insert(t);
        *v += (t - *v) * k;
        if (t - *v).abs() < 0.01 {
            *v = t;
        }
        let s = v.max(0.0);
        let i0 = s.floor() as usize;
        (i0, s - i0 as f32)
    }

    pub(super) fn start_art(&mut self, r: &Renderer, scene: &mut Scene, a: [f32; 4], icon: &str) {
        let d = ((a[2] - a[0]).min(a[3] - a[1]) * 0.9).floor();
        if d < 60.0 {
            return;
        }
        let (cx, cy) = ((a[0] + a[2]) * 0.5, (a[1] + a[3]) * 0.5);
        for (k, al) in [(1.0f32, 8u8), (0.78, 12), (0.56, 22)] {
            let s = d * k * 0.5;
            self.text.rounded(r, scene, [cx - s, cy - s, cx + s, cy + s], s, [232, 160, 48, al]);
        }
        let isz = (d * 0.3).round();
        if let Some(ic) = self.text.icon(r, scene, icon, isz as u32, ACCENT) {
            ic.place(scene, cx - isz * 0.5, cy - isz * 0.5);
        }
    }

    pub(super) fn start_caption(&mut self, r: &Renderer, scene: &mut Scene, key: &str, x: f32, y: f32, u: f32) -> f32 {
        let l = self.text.label(r, scene, key, (12.0 * u) as u32, MUTED);
        l.place(scene, x, y);
        l.h as f32
    }

    #[allow(clippy::too_many_arguments)]
    pub(super) fn start_button(&mut self, r: &Renderer, scene: &mut Scene, f: &Frame, rc: [f32; 4], label: &str, icon: &str, id: usize, u: f32) {
        let bh = rc[3] - rc[1];
        let hv = self.ease((215, "go", id), if inside(rc, f.cursor) { 1.0 } else { 0.0 }, 10.0);
        self.text.rounded(r, scene, rc, bh * 0.5, mix(ACCENT, ACCENT_HOT, hv));
        let gl = self.text.label(r, scene, label, (18.0 * u) as u32, ON_ACCENT);
        let gi = (22.0 * u).round();
        let total = gl.w as f32 + gi + 8.0 * u;
        let gx = rc[0] + ((rc[2] - rc[0]) - total) * 0.5;
        gl.place(scene, gx, rc[1] + (bh - gl.h as f32) * 0.5);
        if let Some(ic) = self.text.icon(r, scene, icon, gi as u32, ON_ACCENT) {
            ic.place(scene, gx + gl.w as f32 + 8.0 * u, rc[1] + (bh - gi) * 0.5);
        }
    }

    #[allow(clippy::too_many_arguments)]
    pub(super) fn start_stepper(
        &mut self,
        r: &Renderer,
        scene: &mut Scene,
        f: &Frame,
        rc: [f32; 4],
        i: usize,
        label: &str,
        value: &str,
        v: &StartView,
        u: f32,
        wide: bool,
        bar: Option<f32>,
        out: &mut Hits,
    ) {
        let h = rc[3] - rc[1];
        let focus = v.zone == 1 && v.row == i;
        let hot = inside(rc, f.cursor);
        let hv = self.ease((213, "row", i), if focus || hot { 1.0 } else { 0.0 }, 10.0);
        self.text.rounded(r, scene, rc, ROW_R * u, mix(OPT_ROW, OPT_ROW_HOT, hv));
        let fv = self.ease((213, "rowon", i), if focus { 1.0 } else { 0.0 }, 10.0);
        if fv > 0.0 {
            self.accent_bar(r, scene, rc, fv, false, u);
        }
        let th = if bar.is_some() { h - 26.0 * u } else { h };
        let aw = (30.0 * u).round();
        let isz = (20.0 * u).round();
        let ra = [rc[2] - 8.0 * u - aw, rc[1], rc[2] - 8.0 * u, rc[1] + th];
        let la = [ra[0] - aw, rc[1], ra[0], rc[1] + th];
        for (k, (rect, name)) in [(la, "chevron_left"), (ra, "chevron_right")].into_iter().enumerate() {
            let av = self.ease((214, name, i), if inside(rect, f.cursor) { 1.0 } else { 0.0 }, 12.0);
            if let Some(ic) = self.text.icon(r, scene, name, isz as u32, mix(MUTED, WHITE, av)) {
                ic.place(scene, rect[0] + (aw - isz) * 0.5, rc[1] + (th - isz) * 0.5);
            }
            out.1[i * 2 + k] = rect;
        }
        out.0[i] = rc;
        let tx = rc[0] + TEXT_IN * u;
        let room = (la[0] - 8.0 * u - tx).max(30.0 * u);
        let lpx = ((if wide { 15.0 } else { 11.0 }) * u) as u32;
        let vpx = (16.0 * u) as u32;
        let ll = self.text.label(r, scene, label, lpx, if focus || hot { SOFT } else { MUTED });
        let text = clip_to(&self.text, value, vpx as f32, if wide { room - ll.w as f32 - 12.0 * u } else { room });
        let vl = self.text.label(r, scene, &text, vpx, if focus { ACCENT_TEXT } else { WHITE });
        if wide {
            ll.place(scene, tx, rc[1] + (h - ll.h as f32) * 0.5);
            let vx = la[0] - 8.0 * u - vl.w as f32;
            vl.place(scene, vx.max(tx + ll.w as f32 + 12.0 * u), rc[1] + (h - vl.h as f32) * 0.5);
        } else {
            let total = (ll.h + vl.h) as f32;
            let y = rc[1] + (th - total) * 0.5;
            ll.place(scene, tx, y);
            vl.place(scene, tx, y + ll.h as f32);
        }
        if let Some(frac) = bar {
            let (x0, x1) = (rc[0] + TEXT_IN * u, rc[2] - TEXT_IN * u);
            let ty = rc[3] - 16.0 * u;
            let t = 5.0 * u;
            self.text.rounded(r, scene, [x0, ty, x1, ty + t], t * 0.5, OPT_LINE);
            let fx = x0 + (x1 - x0) * frac;
            if frac > 0.0 {
                self.text.rounded(r, scene, [x0, ty, fx.max(x0 + t), ty + t], t * 0.5, ACCENT);
            }
            let k = 8.0 * u;
            self.text.rounded(r, scene, [fx - k, ty + t * 0.5 - k, fx + k, ty + t * 0.5 + k], k, WHITE_FILL);
        }
    }
}
