use super::*;

fn field_width(inner: f32, create_w: f32, gap: f32, u: f32) -> f32 {
    ((inner - create_w - 6.0 * gap) / 4.0).max(40.0 * u)
}

fn field_rect(left: f32, top: f32, fw: f32, gap: f32, h: f32, i: usize) -> [f32; 4] {
    let x = left + (fw + gap) * i as f32;
    [x, top, x + fw, top + h]
}

impl Ui {

    pub(super) fn draw_place(&mut self, r: &Renderer, scene: &mut Scene, f: &Frame, m: Metrics, d: &Dialog, open: Option<usize>) {
        let Dialog::Place { title, preview, fields, create, can_create } = d else {
            return;
        };
        let Metrics { w, h, u, line, .. } = m;
        let pad = 24.0 * u;
        let gap = 10.0 * u;
        let nav_h = 56.0 * u;
        let btn_h = 60.0 * u;
        let bw = (820.0 * u).min(w - 2.0 * pad);
        let inner = bw - 2.0 * pad;
        let prev_h = (320.0 * u).min((h * 0.9 - nav_h - btn_h - pad * 2.0 - gap).max(80.0 * u));
        let bh = nav_h + pad + prev_h + gap + btn_h + pad;
        let (x, y) = ((w - bw) * 0.5, ((h - bh) * 0.5).max(0.0));
        self.text.rounded(r, scene, [x - line, y - line, x + bw + line, y + bh + line], 0.0, BORDER);
        self.text.rounded(r, scene, [x, y, x + bw, y + bh], 0.0, [14, 16, 20, 255]);
        self.text.rounded(r, scene, [x, y, x + bw, y + 3.0 * u], 0.0, ACCENT);
        self.place_box = [x, y, x + bw, y + bh];

        // navbar
        let tl = self.text.label(r, scene, title, (22.0 * u) as u32, WHITE);
        tl.place(scene, x + (bw - tl.w as f32) * 0.5, y + (nav_h - tl.h as f32) * 0.5);
        self.text.rounded(r, scene, [x, y + nav_h - line, x + bw, y + nav_h], 0.0, BORDER);

        // the picture of the vehicle
        let pr = [x + pad, y + nav_h + pad, x + bw - pad, y + nav_h + pad + prev_h];
        self.text.rounded(r, scene, [pr[0] - line, pr[1] - line, pr[2] + line, pr[3] + line], 0.0, BORDER);
        self.text.rounded(r, scene, pr, 0.0, [8, 10, 14, 255]);
        self.place_preview_size = ((pr[2] - pr[0]).round() as u32, (pr[3] - pr[1]).round() as u32);
        let first = preview.first().cloned().unwrap_or_default();
        if let Some(tex) = self.place_picture {
            scene.overlays.push((tex, pr));
            let name = clip_to(&self.text, &first, 18.0 * u, inner - 32.0 * u);
            let nw = self.text.width(&name, 18.0 * u);
            self.text.rounded(r, scene, [pr[0], pr[3] - 44.0 * u, pr[0] + nw + 32.0 * u, pr[3]], 0.0, [8, 10, 14, 215]);
            let l = self.text.label(r, scene, &name, (18.0 * u) as u32, WHITE);
            l.place(scene, pr[0] + 16.0 * u, pr[3] - 44.0 * u + (44.0 * u - l.h as f32) * 0.5);
        } else {
            let status = self.place_status.clone();
            let lines: Vec<(String, u32, [u8; 4])> = preview
                .iter()
                .chain(Some(&status).filter(|s| !s.is_empty()))
                .enumerate()
                .map(|(i, s)| {
                    let (px, col) = if i == 0 { ((28.0 * u) as u32, WHITE) } else { ((16.0 * u) as u32, SOFT) };
                    (clip_to(&self.text, s, px as f32, inner - 32.0 * u), px, col)
                })
                .collect();
            let total: f32 = lines.iter().map(|l| l.1 as f32 * 1.3 + 6.0 * u).sum();
            let mut ty = pr[1] + (prev_h - total) * 0.5;
            for (s, px, col) in &lines {
                let l = self.text.label(r, scene, s, *px, *col);
                l.place(scene, pr[0] + (pr[2] - pr[0] - l.w as f32) * 0.5, ty);
                ty += l.h as f32 + 6.0 * u;
            }
        }

        // buttons
        let by = pr[3] + gap;
        let create_w = 160.0 * u;
        let fw = field_width(inner, create_w, gap, u);
        self.place_rects.clear();
        for (i, (cap, val, on)) in fields.iter().enumerate().take(4) {
            let rc = field_rect(pr[0], by, fw, gap, btn_h, i);
            let hot = *on && inside(rc, f.cursor) && open.is_none();
            let hv = self.ease((205, "pf", i), if hot || open == Some(i) { 1.0 } else { 0.0 }, 8.0);
            let k = if *on { 1.0 } else { 0.4 };
            self.text.rounded(r, scene, [rc[0] - line, rc[1] - line, rc[2] + line, rc[3] + line], 0.0, fade(if open == Some(i) { ACCENT } else { BORDER }, k));
            self.text.rounded(r, scene, rc, 0.0, fade(mix([30, 32, 37, 255], [58, 62, 70, 255], hv), k));
            let cl = self.text.label(r, scene, cap, (12.0 * u) as u32, if *on { SOFT } else { MUTED });
            cl.place(scene, rc[0] + 12.0 * u, rc[1] + 9.0 * u);
            let vpx = (15.0 * u) as u32;
            let shown = clip_to(&self.text, val, vpx as f32, fw - 24.0 * u);
            let vl = self.text.label(r, scene, &shown, vpx, if *on { WHITE } else { MUTED });
            vl.place(scene, rc[0] + 12.0 * u, rc[3] - vl.h as f32 - 9.0 * u);
            self.place_rects.push(rc);
        }
        let cr = [pr[2] - create_w, by, pr[2], by + btn_h];
        if *can_create {
            self.dialog_button(r, scene, f, m, cr, create, 4, open.is_none());
        } else {
            self.text.rounded(r, scene, cr, 0.0, [30, 32, 37, 110]);
            let l = self.text.label(r, scene, create, (16.0 * u) as u32, MUTED);
            l.place(scene, cr[0] + (create_w - l.w as f32) * 0.5, cr[1] + (btn_h - l.h as f32) * 0.5);
        }
        self.place_rects.push(cr);
    }

    pub(super) fn draw_dropdown(&mut self, r: &Renderer, scene: &mut Scene, f: &Frame, m: Metrics, d: &Dialog, field: usize) {
        let Dialog::Select { options, sel, scroll, search, .. } = d else {
            return;
        };
        let Metrics { w, u, line, .. } = m;
        let Some(&anchor) = self.place_rects.get(field) else {
            return;
        };
        let row_h = 36.0 * u;
        let step = row_h + 2.0 * u;
        let pad = 8.0 * u;
        let bpx = 15.0 * u;
        let search_h = if search.is_some() { 44.0 * u } else { 0.0 };
        let avail = anchor[1] - 8.0 * u - 2.0 * pad - search_h - 8.0 * u;
        let vis = ((avail / step) as usize).clamp(1, 8).min(options.len().max(1));
        self.dialog_vis = vis;
        let pw = (anchor[2] - anchor[0]).max(300.0 * u).min(w - 16.0 * u);
        let px0 = anchor[0].min(w - 8.0 * u - pw).max(8.0 * u);
        let ph = pad * 2.0 + search_h + vis as f32 * step;
        let py1 = anchor[1] - 4.0 * u;
        let py0 = py1 - ph;
        let panel = [px0, py0, px0 + pw, py1];
        self.text.rounded(r, scene, [panel[0] - line, panel[1] - line, panel[2] + line, panel[3] + line], 0.0, ACCENT);
        self.text.rounded(r, scene, panel, 0.0, [20, 22, 27, 255]);
        let (bx, inner) = (panel[0] + pad, pw - 2.0 * pad);
        let mut cy = panel[1] + pad;
        if let Some(q) = search {
            let rc = [bx, cy, bx + inner, cy + 40.0 * u];
            self.text.rounded(r, scene, rc, 0.0, [8, 10, 14, 255]);
            let (txt, col) = if q.is_empty() { (t("pause.dialog.search"), SOFT) } else { (clip_left(&self.text, q, bpx, inner - 32.0 * u), WHITE) };
            let l = self.text.label(r, scene, &txt, bpx as u32, col);
            l.place(scene, rc[0] + 12.0 * u, rc[1] + (40.0 * u - l.h as f32) * 0.5);
            if (self.text.frame / 30) % 2 == 0 {
                let cx = rc[0] + 12.0 * u + if q.is_empty() { 0.0 } else { l.w as f32 } + 2.0 * u;
                self.text.rounded(r, scene, [cx, rc[1] + 8.0 * u, cx + 2.0 * u, rc[3] - 8.0 * u], 0.0, WHITE);
            }
            cy += search_h;
        }
        let max_first = options.len().saturating_sub(vis);
        let target = (*scroll).min(max_first);
        let mut pos = self.dialog_list_pos.clamp(0.0, max_first as f32);
        if self.dialog_t < 0.2 || (target as f32 - pos).abs() < 0.01 {
            pos = target as f32;
        } else {
            pos += (target as f32 - pos) * (1.0 - (-18.0 * self.anim_dt).exp());
        }
        self.dialog_list_pos = pos;
        let first = (pos.floor() as usize).min(max_first);
        let (list_top, view_end) = (cy, cy + vis as f32 * step - 2.0 * u);
        let a0 = self.text.alpha;
        if options.is_empty() {
            let l = self.text.label(r, scene, &t("pause.dialog.no_results"), bpx as u32, SOFT);
            l.place(scene, bx + 8.0 * u, cy + 8.0 * u);
        }
        if options.len() > vis {
            let th = vis as f32 * step;
            let (tx, n) = (panel[2] - pad * 0.5 - 4.0 * u, options.len() as f32);
            self.text.rounded(r, scene, [tx, cy, tx + 4.0 * u, cy + th], 0.0, [34, 36, 42, 255]);
            self.dialog_bar = Some([tx - 8.0 * u, cy, tx + 12.0 * u, cy + th]);
            let ty = cy + th * pos / n;
            self.text.rounded(r, scene, [tx, ty, tx + 4.0 * u, ty + th * vis as f32 / n], 0.0, ACCENT);
        }
        self.dialog_rects.clear();
        let row_w = inner - if options.len() > vis { 10.0 * u } else { 0.0 };
        for (i, o) in options.iter().enumerate() {
            if i < first || i > first + vis {
                self.dialog_rects.push([0.0; 4]);
                continue;
            }
            let ry = list_top + step * (i as f32 - pos);

            let edge = ((ry + step - list_top) / step).min((view_end - ry) / step).clamp(0.0, 1.0);
            if edge <= 0.03 {
                self.dialog_rects.push([0.0; 4]);
                continue;
            }
            let rc = [bx, ry.max(list_top), bx + row_w, (ry + row_h).min(view_end)];
            let hot = inside(rc, f.cursor);
            let hv = self.ease((205, "dd", i), if hot { 1.0 } else { 0.0 }, 8.0);
            let on = i == *sel;
            let bg = mix(if on { [46, 49, 57, 255] } else { [28, 30, 35, 255] }, [46, 49, 57, 255], hv);
            self.text.rounded(r, scene, rc, 0.0, fade(bg, edge));
            if on {
                self.text.rounded(r, scene, [rc[0], rc[1], rc[0] + 4.0 * u, rc[3]], 0.0, fade(ACCENT, edge));
            }
            self.text.alpha = a0 * edge;
            let name = clip_to(&self.text, o, bpx, row_w - 32.0 * u);
            let l = self.text.label(r, scene, &name, bpx as u32, if on || hot { WHITE } else { SOFT });
            l.place(scene, rc[0] + 14.0 * u, ry + (row_h - l.h as f32) * 0.5);
            self.text.alpha = a0;
            self.dialog_rects.push(rc);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn field_width_splits_remaining_room() {
        assert_eq!(field_width(800.0, 160.0, 10.0, 1.0), 145.0);
    }

    #[test]
    fn field_width_has_a_minimum() {
        assert_eq!(field_width(10.0, 160.0, 10.0, 1.0), 40.0);
        assert_eq!(field_width(10.0, 160.0, 10.0, 2.0), 80.0);
    }

    #[test]
    fn field_rects_are_adjacent_with_gap() {
        let (fw, gap) = (100.0, 10.0);
        let rs: Vec<[f32; 4]> = (0..4).map(|i| field_rect(20.0, 5.0, fw, gap, 60.0, i)).collect();
        assert_eq!(rs[0], [20.0, 5.0, 120.0, 65.0]);
        for p in rs.windows(2) {
            assert_eq!(p[1][0] - p[0][2], gap);
            assert_eq!((p[0][1], p[0][3]), (p[1][1], p[1][3]));
        }
    }

    #[test]
    fn fields_and_create_button_fit_in_inner_width() {
        let (inner, create_w, gap) = (700.0, 160.0, 10.0);
        let fw = field_width(inner, create_w, gap, 1.0);
        let last = field_rect(0.0, 0.0, fw, gap, 1.0, 3);
        assert!(last[2] + gap + create_w <= inner + 0.001);
    }
}
