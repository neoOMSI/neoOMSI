use super::*;

const TABS: [&str; 3] = ["start.maps.nav", "start.join.nav", "pause.page.options.nav"];

impl Ui {
    pub(crate) fn draw_start_bar(
        &mut self,
        r: &Renderer,
        scene: &mut Scene,
        f: &Frame,
        m: Metrics,
        tab: usize,
    ) -> f32 {
        let Metrics { w, u, mx, .. } = m;
        let bar_h = (56.0 * u).round();
        self.text.rounded(r, scene, [0.0, 0.0, w, bar_h], 0.0, SIDEBAR);
        let qpx = (15.0 * u) as u32;
        let ql = self.text.label(r, scene, "pause.entry.quit", qpx, WHITE);
        let (qw, qh) = (ql.w as f32 + 28.0 * u, ql.h as f32 + 14.0 * u);
        let qx = (w - mx - qw).round();
        let qy = ((bar_h - qh) * 0.5).round();
        let qrc = [qx, qy, qx + qw, qy + qh];
        let qv = self.ease((217, "quit", 0), if inside(qrc, f.cursor) { 1.0 } else { 0.0 }, 8.0);
        let lerp = |a: u8, b: u8| (a as f32 + (b as f32 - a as f32) * qv).round() as u8;
        let qbg = [lerp(190, 236), lerp(54, 92), lerp(48, 82), 255];
        self.text.rounded(r, scene, qrc, 6.0 * u, qbg);
        ql.place(scene, qrc[0] + (qw - ql.w as f32) * 0.5, qrc[1] + (qh - ql.h as f32) * 0.5);
        self.start.quit = qrc;
        self.start_update_chip(r, scene, f, qrc[0] - 14.0 * u, bar_h * 0.5, u);
        self.ensure_logo();
        if let Some((tex, iw, ih)) = self.logo_at(r, scene, (bar_h * 0.46).round()) {
            let top = ((bar_h - ih as f32) * 0.5).round();
            scene.overlays.push((tex, [mx, top, mx + iw as f32, top + ih as f32]));
        } else {
            let brand = self.text.label(r, scene, "neoOMSI", (15.0 * u) as u32, MUTED);
            brand.place(scene, mx, (bar_h - brand.h as f32) * 0.5);
        }
        let tpx = (17.0 * u) as u32;
        let gap = 6.0 * u;
        let widths: Vec<f32> = TABS.iter().map(|t| self.text.width(t, tpx as f32) + 40.0 * u).collect();
        let total = widths.iter().sum::<f32>() + gap * (widths.len() - 1) as f32;
        let x = ((w - total) * 0.5).round();
        for (label, bx) in [("Q", x - 40.0 * u), ("E", x + total + 12.0 * u)] {
            let l = self.text.label(r, scene, label, (13.0 * u) as u32, WHITE);
            let (bw, bh) = (28.0 * u, 24.0 * u);
            let by = (bar_h - bh) * 0.5;
            self.text.rounded(r, scene, [bx, by, bx + bw, by + bh], 4.0 * u, [255, 255, 255, 56]);
            l.place(scene, bx + (bw - l.w as f32) * 0.5, by + (bh - l.h as f32) * 0.5);
        }
        let mut lefts = Vec::new();
        let mut lx = x;
        for wd in &widths {
            lefts.push(lx);
            lx += wd + gap;
        }
        let pos = self.ease((217, "pos", 0), tab as f32, 9.0);
        for (i, key) in TABS.iter().enumerate() {
            let rc = [lefts[i], 0.0, lefts[i] + widths[i], bar_h];
            let hot = inside(rc, f.cursor);
            let hv = self.ease((217, "hover", i), if hot && i != tab { 1.0 } else { 0.0 }, 8.0);
            let tv = self.easeq((217, "tab", i), if i == tab { 1.0 } else { 0.0 }, 7.0);
            if hv > 0.0 {
                self.text.rounded(r, scene, rc, 0.0, fade([255, 255, 255, 8], hv));
            }
            let l = self.text.label(r, scene, key, tpx, mix(if hot { WHITE } else { SOFT }, WHITE, tv));
            l.place(scene, rc[0] + (widths[i] - l.w as f32) * 0.5, (bar_h - l.h as f32) * 0.5);
            self.start.tabs.push(rc);
        }
        let last = TABS.len() - 1;
        let p = pos.clamp(0.0, last as f32);
        let i0 = (p.floor() as usize).min(last);
        let i1 = (i0 + 1).min(last);
        let fr = p - i0 as f32;
        let sm = fr * fr * (3.0 - 2.0 * fr);
        let ul = lefts[i0] + (lefts[i1] - lefts[i0]) * sm;
        let uw = widths[i0] + (widths[i1] - widths[i0]) * sm;
        self.text.rounded(r, scene, [ul, bar_h - 3.0 * u, ul + uw, bar_h], 0.0, ACCENT);
        bar_h
    }

    pub(super) fn start_update_chip(&mut self, r: &Renderer, scene: &mut Scene, f: &Frame, right: f32, cy: f32, u: f32) {
        let up = self.start.update.clone();
        self.start.update_btn = [0.0; 4];
        if up.kind == 0 {
            return;
        }
        let px = (14.0 * u) as u32;
        let isz = (18.0 * u).round();
        let busy = up.kind == 2 || up.kind == 3;
        let l = self.text.label(r, scene, &up.label, px, if busy { WHITE } else { ON_ACCENT });
        let (bw, bh) = (l.w as f32 + isz + 34.0 * u, l.h as f32 + 14.0 * u);
        let rc = [right - bw, cy - bh * 0.5, right, cy + bh * 0.5];
        let hv = self.ease((235, "update", 0), if !busy && inside(rc, f.cursor) { 1.0 } else { 0.0 }, 10.0);
        if busy {
            self.text.rounded(r, scene, rc, bh * 0.5, OPT_ROW);
            let fill = (bw * up.progress.clamp(0.0, 1.0)).max(bh);
            self.text.rounded(r, scene, [rc[0], rc[1], rc[0] + fill.min(bw), rc[3]], bh * 0.5, fade(ACCENT, 0.55));
        } else if up.kind == 4 {
            self.text.rounded(r, scene, rc, bh * 0.5, mix([190, 54, 48, 255], [236, 92, 82, 255], hv));
        } else {
            self.text.rounded(r, scene, rc, bh * 0.5, mix(ACCENT, ACCENT_HOT, hv));
        }
        let ink = if busy { WHITE } else { ON_ACCENT };
        if let Some(ic) = self.text.icon(r, scene, "download", isz as u32, ink) {
            ic.place(scene, rc[0] + 12.0 * u, cy - isz * 0.5);
        }
        l.place(scene, rc[0] + 12.0 * u + isz + 8.0 * u, cy - l.h as f32 * 0.5);
        if !busy {
            self.start.update_btn = rc;
        }
    }

    pub(super) fn start_head(
        &mut self,
        r: &Renderer,
        scene: &mut Scene,
        m: Metrics,
        head: &str,
        note: &str,
        y: f32,
        pt: f32,
    ) -> f32 {
        let Metrics { u, mx, .. } = m;
        let hs = head_alpha(pt);
        self.text.alpha = hs;
        let hl = self.text.label(r, scene, head, (34.0 * u) as u32, WHITE);
        hl.place(scene, mx - 40.0 * u * (1.0 - hs), y);
        let ns = out((pt - 0.08) / 0.6);
        self.text.alpha = ns;
        let nl = self.text.label(r, scene, note, (14.0 * u) as u32, MUTED);
        nl.place(scene, mx - 40.0 * u * (1.0 - ns), y + hl.h as f32);
        self.text.alpha = 1.0;
        y + hl.h as f32 + nl.h as f32 + 18.0 * u
    }
}
