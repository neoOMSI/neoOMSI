//! Dialogs

use super::*;

const MAX_HEIGHT: f32 = 560.0;

#[derive(Clone)]
pub enum Dialog {
    Confirm { title: String, text: String, yes: String, no: String },
    Select { title: String, options: Vec<String>, sel: usize, scroll: usize, search: Option<String>, drop: Option<usize> },
    Loading { title: String },
    Editor { title: String, value: String, ok: String, cancel: String },
    Place { title: String, preview: Vec<String>, fields: Vec<(String, String, bool)>, create: String, can_create: bool },
}

impl Ui {
    pub(super) fn dialog_button(&mut self, r: &Renderer, scene: &mut Scene, f: &Frame, m: Metrics, rc: [f32; 4], text: &str, i: usize, main: bool) {
        let hot = inside(rc, f.cursor);
        let hv = self.ease((205, "btn", i), if hot { 1.0 } else { 0.0 }, 8.0);
        let base = if main { [58, 62, 70, 255] } else { [30, 32, 37, 255] };
        self.text.rounded(r, scene, rc, 0.0, mix(base, [74, 78, 88, 255], hv));
        if main {
            self.text.rounded(r, scene, [rc[0], rc[3] - 3.0 * m.u, rc[2], rc[3]], 0.0, ACCENT);
        }
        let px = (16.0 * m.u) as u32;
        let l = self.text.label(r, scene, text, px, if hot || main { WHITE } else { SOFT });
        l.place(scene, rc[0] + (rc[2] - rc[0] - l.w as f32) * 0.5, rc[1] + (rc[3] - rc[1] - l.h as f32) * 0.5);
    }

    pub(super) fn draw_dialog(&mut self, r: &Renderer, scene: &mut Scene, f: &Frame, m: Metrics, d: &Dialog) {
        let Metrics { w, h, u, line, .. } = m;
        self.dialog_rects.clear();
        self.dialog_back_rc = [0.0; 4];
        self.dialog_pane_rc = [0.0; 4];
        self.dialog_pane_max = 0;
        self.dialog_bar = None;
        self.dialog_pane_bar = None;
        self.menu_pane.clear();
        self.menu_pane_go = None;
        self.menu_time.clear();
        self.menu_pane_start = 0;
        self.text.rounded(r, scene, [0.0, 0.0, w, h], 0.0, [0, 0, 0, 160]);
        if let Dialog::Place { .. } = d {
            self.draw_place(r, scene, f, m, d, None);
            return;
        }
        if let (Dialog::Select { drop: Some(i), .. }, Some(under)) = (d, self.dialog_under.clone()) {
            self.draw_place(r, scene, f, m, &under, Some(*i));
            self.draw_dropdown(r, scene, f, m, d, *i);
            return;
        }

        let pad = 24.0 * u;
        let want_pane = if matches!(d, Dialog::Select { drop: None, .. }) && (self.dialog_preview.is_some() || self.dialog_tall) { 320.0 * u } else { 0.0 };
        let bw = (if matches!(d, Dialog::Loading { .. }) { 260.0 * u } else { 460.0 * u + want_pane }).min(w - 2.0 * pad);
        // (too narrow for the timetable: it is left out)
        let pane = if bw - want_pane >= 340.0 * u { want_pane } else { 0.0 };
        let inner = bw - pane - 2.0 * pad;
        let btn_h = 44.0 * u;
        let row_h = 44.0 * u;
        let tpx = (22.0 * u) as u32;
        let bpx = 16.0 * u;

        let tpx_h = tpx as f32 * 1.3;
        // no dialog is taller than this (and none taller than the screen): a select scrolls
        let max_h = (h * 0.9).min(MAX_HEIGHT * u);
        let search_h = if matches!(d, Dialog::Select { search: Some(_), .. }) { 48.0 * u } else { 0.0 };
        let tall = self.dialog_tall && matches!(d, Dialog::Select { drop: None, .. });
        let back = self.dialog_back && matches!(d, Dialog::Select { drop: None, .. });
        let back_h = if back { btn_h + 12.0 * u } else { 0.0 };
        let cap = ((max_h - pad * 2.0 - tpx_h - 16.0 * u - search_h - back_h) / (row_h + 4.0 * u)) as usize;
        // (the lines / tours window always has the same height)
        let vis = |n: usize| if tall { cap.max(1) } else { cap.clamp(1, n.max(1)) };
        let (title, body_h) = match d {
            Dialog::Loading { title } => (title, 64.0 * u),
            Dialog::Confirm { title, text, .. } => {
                (title, wrap(&self.text, text, bpx, inner).len() as f32 * (bpx * 1.3 + 3.0 * u) + btn_h + pad)
            }
            Dialog::Select { title, options, .. } => {
                self.dialog_vis = vis(options.len());
                (title, self.dialog_vis as f32 * (row_h + 4.0 * u) + search_h)
            }
            Dialog::Editor { title, .. } => (title, 48.0 * u + pad + btn_h),
            Dialog::Place { title, .. } => (title, 0.0),
        };
        let bh = if tall { max_h } else { pad * 2.0 + tpx as f32 * 1.3 + 16.0 * u + body_h };
        let (x, y) = ((w - bw) * 0.5, ((h - bh) * 0.5).max(0.0));
        self.dialog_box = [x, y, x + bw, y + bh];
        self.text.rounded(r, scene, [x - line, y - line, x + bw + line, y + bh + line], 0.0, BORDER);
        self.text.rounded(r, scene, [x, y, x + bw, y + bh], 0.0, [14, 16, 20, 255]);
        self.text.rounded(r, scene, [x, y, x + bw, y + 3.0 * u], 0.0, ACCENT);

        let tl = self.text.label(r, scene, title, tpx, WHITE);
        tl.place(scene, x + pad, y + pad);
        let mut cy = y + pad + tl.h as f32 + 16.0 * u;
        let (bx, bw2) = (x + pad, (inner - 12.0 * u) * 0.5);

        match d {
            Dialog::Confirm { text, yes, no, .. } => {
                for ln in wrap(&self.text, text, bpx, inner) {
                    let l = self.text.label(r, scene, &ln, bpx as u32, SOFT);
                    l.place(scene, bx, cy);
                    cy += l.h as f32 + 3.0 * u;
                }
                cy += pad;
                let a = [bx, cy, bx + bw2, cy + btn_h];
                let b = [bx + bw2 + 12.0 * u, cy, bx + inner, cy + btn_h];
                self.dialog_button(r, scene, f, m, a, yes, 0, true);
                self.dialog_button(r, scene, f, m, b, no, 1, false);
                self.dialog_rects.extend([a, b]);
            }
            Dialog::Loading { .. } => {
                let (sx, sy) = (x + bw * 0.5, cy + 32.0 * u);
                let step = (self.text.frame / 4) as usize;
                for i in 0..8usize {
                    let a = std::f32::consts::TAU * i as f32 / 8.0;
                    let (dx, dy) = (a.sin() * 20.0 * u, -a.cos() * 20.0 * u);
                    let alpha = 255 - ((step + 8 - i) % 8) as u32 * 28;
                    let s = 4.0 * u;
                    self.text.rounded(r, scene, [sx + dx - s, sy + dy - s, sx + dx + s, sy + dy + s], s, [255, 255, 255, alpha as u8]);
                }
            }
            Dialog::Select { options, sel, scroll, search, .. } => {
                if let Some(q) = search {
                    let rc = [bx, cy, bx + inner, cy + 40.0 * u];
                    self.text.rounded(r, scene, [rc[0] - line, rc[1] - line, rc[2] + line, rc[3] + line], 0.0, ACCENT);
                    self.text.rounded(r, scene, rc, 0.0, [8, 10, 14, 255]);
                    let (txt, col) = if q.is_empty() { (t("pause.dialog.search"), SOFT) } else { (clip_left(&self.text, q, bpx, inner - 32.0 * u), WHITE) };
                    let l = self.text.label(r, scene, &txt, bpx as u32, col);
                    l.place(scene, rc[0] + 16.0 * u, rc[1] + (40.0 * u - l.h as f32) * 0.5);
                    if (self.text.frame / 30) % 2 == 0 {
                        let cx = rc[0] + 16.0 * u + if q.is_empty() { 0.0 } else { l.w as f32 } + 2.0 * u;
                        self.text.rounded(r, scene, [cx, rc[1] + 8.0 * u, cx + 2.0 * u, rc[3] - 8.0 * u], 0.0, WHITE);
                    }
                    cy += 48.0 * u;
                    if options.is_empty() {
                        let l = self.text.label(r, scene, &t("pause.dialog.no_results"), bpx as u32, SOFT);
                        l.place(scene, bx, cy + 8.0 * u);
                    }
                }
                let vis = self.dialog_vis;
                let step = row_h + 4.0 * u;
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
                let (list_top, view_end) = (cy, cy + vis as f32 * step - 4.0 * u);
                let a0 = self.text.alpha;
                if options.len() > vis {
                    let th = vis as f32 * step;
                    let (tx, n) = (bx + inner + 10.0 * u, options.len() as f32);
                    self.text.rounded(r, scene, [tx, cy, tx + 4.0 * u, cy + th], 0.0, [34, 36, 42, 255]);
                    self.dialog_bar = Some([tx - 8.0 * u, cy, tx + 12.0 * u, cy + th]);
                    let ty = cy + th * pos / n;
                    self.text.rounded(r, scene, [tx, ty, tx + 4.0 * u, ty + th * vis as f32 / n], 0.0, ACCENT);
                }
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
                    let rc = [bx, ry.max(list_top), bx + inner, (ry + row_h).min(view_end)];
                    let hot = inside(rc, f.cursor);
                    let hv = self.ease((205, "opt", i), if hot { 1.0 } else { 0.0 }, 8.0);
                    let on = i == *sel;
                    let bg = mix(if on { [46, 49, 57, 255] } else { [28, 30, 35, 255] }, [46, 49, 57, 255], hv);
                    self.text.rounded(r, scene, rc, 0.0, fade(bg, edge));
                    if on {
                        self.text.rounded(r, scene, [rc[0], rc[1], rc[0] + 4.0 * u, rc[3]], 0.0, fade(ACCENT, edge));
                    }
                    self.text.alpha = a0 * edge;
                    let name = clip_to(&self.text, o, bpx, inner - 40.0 * u);
                    let l = self.text.label(r, scene, &name, bpx as u32, if on || hot { WHITE } else { SOFT });
                    l.place(scene, rc[0] + 18.0 * u, ry + (row_h - l.h as f32) * 0.5);
                    self.text.alpha = a0;
                    self.dialog_rects.push(rc);
                }
            }
            Dialog::Editor { value, ok, cancel, .. } => {
                let rc = [bx, cy, bx + inner, cy + 48.0 * u];
                self.text.rounded(r, scene, [rc[0] - line, rc[1] - line, rc[2] + line, rc[3] + line], 0.0, ACCENT);
                self.text.rounded(r, scene, rc, 0.0, [8, 10, 14, 255]);
                let shown = clip_left(&self.text, value, bpx, inner - 32.0 * u);
                let sw = if shown.is_empty() { 0.0 } else { self.text.width(&shown, bpx) };
                if !shown.is_empty() {
                    let l = self.text.label(r, scene, &shown, bpx as u32, WHITE);
                    l.place(scene, rc[0] + 16.0 * u, rc[1] + (48.0 * u - l.h as f32) * 0.5);
                }
                if (self.text.frame / 30) % 2 == 0 {
                    let cx = rc[0] + 16.0 * u + sw + 2.0 * u;
                    self.text.rounded(r, scene, [cx, rc[1] + 12.0 * u, cx + 2.0 * u, rc[3] - 12.0 * u], 0.0, WHITE);
                }
                cy = rc[3] + pad;
                let a = [bx, cy, bx + bw2, cy + btn_h];
                let b = [bx + bw2 + 12.0 * u, cy, bx + inner, cy + btn_h];
                self.dialog_button(r, scene, f, m, a, ok, 0, true);
                self.dialog_button(r, scene, f, m, b, cancel, 1, false);
                self.dialog_rects.extend([a, b]);
            }
            Dialog::Place { .. } => {}
        }

        if back {
            let rc = [bx, y + bh - pad - btn_h, bx + inner, y + bh - pad];
            self.dialog_button(r, scene, f, m, rc, &t("pause.dialog.back"), 23, false);
            self.dialog_back_rc = rc;
        }

        if pane > 0.0 {
            let px0 = x + bw - pane;
            self.text.rounded(r, scene, [px0, y + 3.0 * u, px0 + line, y + bh], 0.0, BORDER);
            self.dialog_pane_rc = [px0, y, x + bw, y + bh];
            if let Some(p) = self.dialog_preview.clone() {
                let (lx, rx) = (px0 + 18.0 * u, x + bw - pad);
                let mut ty = y + pad;
                let hl = self.text.label(r, scene, &clip_to(&self.text, &p.title, tpx as f32, rx - lx), tpx, WHITE);
                hl.place(scene, lx, ty);
                ty += hl.h as f32 + 4.0 * u;
                let ml = self.text.label(r, scene, &clip_to(&self.text, &p.meta, 14.0 * u, rx - lx), (14.0 * u) as u32, MUTED);
                ml.place(scene, lx, ty);
                ty += ml.h as f32 + 14.0 * u;
                let pick = p.chosen.is_some();
                // the time of the trip, with the arrows to the trip before / the next
                if let (true, Some(time)) = (pick, p.time.as_ref()) {
                    let (ah, aw) = (34.0 * u, 46.0 * u);
                    let (a, b) = ([lx, ty, lx + aw, ty + ah], [rx - aw, ty, rx, ty + ah]);
                    self.dialog_button(r, scene, f, m, a, "\u{2039}", 20, false);
                    self.dialog_button(r, scene, f, m, b, "\u{203a}", 21, false);
                    self.menu_time.extend([a, b]);
                    let tl = self.text.label(r, scene, time, (20.0 * u) as u32, ACCENT);
                    tl.place(scene, (lx + rx - tl.w as f32) * 0.5, ty + (ah - tl.h as f32) * 0.5);
                    ty += ah + 10.0 * u;
                }
                let go_h = 40.0 * u;
                let has_go = pick && p.button.is_some();
                let bottom = if has_go { y + bh - pad - go_h - 10.0 * u } else { y + bh - pad };
                let rh = 30.0 * u;
                let total = p.rows.len();
                let fit = (((bottom - ty) / rh) as usize).max(1);
                let n = fit.min(total);
                if self.dialog_pane_key != p.title {
                    self.dialog_pane_key = p.title.clone();
                    self.dialog_pane_top = None;
                    self.dialog_pane_pos = 0.0;
                }
                let max_first = total.saturating_sub(fit);
                let target = self.dialog_pane_top.unwrap_or(0).min(max_first);
                let mut pos = self.dialog_pane_pos.clamp(0.0, max_first as f32);
                if (target as f32 - pos).abs() < 0.01 {
                    pos = target as f32;
                } else {
                    pos += (target as f32 - pos) * (1.0 - (-18.0 * self.anim_dt).exp());
                }
                self.dialog_pane_pos = pos;
                self.dialog_pane_max = max_first;
                let first = (pos.floor() as usize).min(max_first);
                let frac = pos - first as f32;
                self.menu_pane_start = first;
                let (list_top, view_end) = (ty, ty + rh * n as f32);
                if max_first > 0 {
                    let tx = x + bw - 10.0 * u;
                    let th = view_end - list_top;
                    self.text.rounded(r, scene, [tx, list_top, tx + 4.0 * u, view_end], 0.0, [34, 36, 42, 255]);
                    self.dialog_pane_bar = Some([tx - 8.0 * u, list_top, tx + 12.0 * u, view_end]);
                    let by = list_top + th * pos / total as f32;
                    self.text.rounded(r, scene, [tx, by, tx + 4.0 * u, by + th * fit as f32 / total as f32], 0.0, ACCENT);
                }
                for (i, (what, when)) in p.rows.iter().enumerate().skip(first).take(n + 1) {
                    let ry = list_top + rh * (i - first) as f32 - rh * frac;
                    // rows leaving at the top or the bottom fade out
                    let edge = ((ry + rh - list_top) / rh).min((view_end - ry) / rh).clamp(0.0, 1.0);
                    if edge <= 0.03 {
                        if pick {
                            self.menu_pane.push([0.0; 4]);
                        }
                        continue;
                    }
                    let rc = [lx - 8.0 * u, ry.max(list_top), rx + 4.0 * u, (ry + rh).min(view_end)];
                    if pick {
                        let on = p.chosen == Some(i);
                        let hot = inside(rc, f.cursor);
                        if on || hot {
                            self.text.rounded(r, scene, rc, 0.0, fade(if on { [46, 49, 57, 255] } else { [34, 36, 42, 255] }, edge));
                        }
                        if on {
                            self.text.rounded(r, scene, [rc[0], rc[1], rc[0] + 3.0 * u, rc[3]], 0.0, fade(ACCENT, edge));
                        }
                        self.menu_pane.push(rc);
                    }
                    self.text.alpha = edge;
                    let tw = self.text.width(when, 14.0 * u);
                    let name = clip_to(&self.text, what, 14.0 * u, (rx - lx - tw - 14.0 * u).max(20.0 * u));
                    let l = self.text.label(r, scene, &name, (14.0 * u) as u32, if p.chosen == Some(i) { WHITE } else { SOFT });
                    l.place(scene, lx, ry + (rh - l.h as f32) * 0.5);
                    let tl = self.text.label(r, scene, when, (14.0 * u) as u32, WHITE);
                    tl.place(scene, rx - tl.w as f32, ry + (rh - tl.h as f32) * 0.5);
                    self.text.alpha = 1.0;
                    self.text.rounded(r, scene, [lx, ry + rh - line, rx, ry + rh], 0.0, fade([34, 36, 42, 255], edge));
                }
                ty = view_end;
                if !pick && total > n {
                    let more = format!("+ {}", total - n);
                    let l = self.text.label(r, scene, &more, (13.0 * u) as u32, MUTED);
                    l.place(scene, lx, ty + 4.0 * u);
                }
                if let (true, Some(label)) = (pick, p.button.as_ref()) {
                    let go = [lx, y + bh - pad - go_h, rx, y + bh - pad];
                    self.dialog_button(r, scene, f, m, go, label, 22, true);
                    self.menu_pane_go = Some(go);
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn dialog_clone_keeps_fields() {
        let d = Dialog::Select {
            title: "t".into(),
            options: vec!["a".into(), "b".into()],
            sel: 1,
            scroll: 0,
            search: Some("x".into()),
            drop: Some(2),
        };
        let Dialog::Select { options, sel, search, drop, .. } = d.clone() else {
            panic!("variant changed");
        };
        assert_eq!(options, ["a", "b"]);
        assert_eq!(sel, 1);
        assert_eq!(search.as_deref(), Some("x"));
        assert_eq!(drop, Some(2));
    }

    #[test]
    fn max_height_is_positive() {
        assert!(MAX_HEIGHT > 0.0);
    }
}
