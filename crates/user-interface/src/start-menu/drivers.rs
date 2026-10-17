use super::*;

const MAX_ROWS: usize = 6;

fn drv_w(btn: [f32; 4], u: f32) -> f32 {
    ((btn[2] - btn[0]).max(190.0 * u)).min(230.0 * u)
}
const DANGER_T: [u8; 4] = [222, 78, 68, 0];

impl Ui {
    pub(super) fn draw_driver_menu(&mut self, r: &Renderer, scene: &mut Scene, f: &Frame, m: Metrics, v: &StartView) {
        let d = v.drivers;
        let btn = self.start.drv_btn;
        if !d.open || btn[2] <= btn[0] {
            return;
        }
        let u = m.u;
        let (bw, rh, pad) = ((drv_w(btn, u)).round(), (34.0 * u).round(), (5.0 * u).round());
        let sep = (7.0 * u).round();
        let isz = (18.0 * u).round() as u32;
        let n = d.names.len();
        let shown = n.min(MAX_ROWS);
        let first = d.current.map_or(0, |c| (c + 1).saturating_sub(MAX_ROWS)).min(n.saturating_sub(MAX_ROWS));
        let note_h = if d.note.is_empty() { 0.0 } else { (20.0 * u).round() };
        let bh = pad + shown as f32 * rh + sep + rh + note_h + pad;
        let x0 = (btn[2] - bw).max(8.0 * u).round();
        let y0 = (btn[3] + 8.0 * u).round();
        let bx = [x0, y0, x0 + bw, y0 + bh];
        self.text.rounded(r, scene, [bx[0] - 1.0, bx[1] - 1.0, bx[2] + 1.0, bx[3] + 1.0], 11.0 * u, [255, 255, 255, 26]);
        self.text.rounded(r, scene, bx, 10.0 * u, [26, 26, 29, 255]);
        self.start.drv_box = bx;
        self.start.drv_first = first;
        let px = (14.0 * u) as u32;
        let (l, rgt) = (x0 + pad, x0 + bw - pad);
        let mut y = y0 + pad;

        for k in 0..shown {
            let i = first + k;
            let confirming = d.confirm == Some(i);
            let rc = [l, y, rgt, y + rh];
            let cur = d.current == Some(i);
            let row_hv = self.ease((236, "row", i), if inside(rc, f.cursor) { 1.0 } else { 0.0 }, 10.0);
            let dw = if confirming { (84.0 * u).round() } else { (28.0 * u).round() };
            let del = [rc[2] - dw - 3.0 * u, y + 4.0 * u, rc[2] - 3.0 * u, y + rh - 4.0 * u];
            let item = [rc[0], y, del[0] - 2.0 * u, y + rh];
            let bg = row_hv.max(if cur { 0.6 } else { 0.0 });
            if bg > 0.0 {
                self.text.rounded(r, scene, rc, 6.0 * u, fade([255, 255, 255, 14], bg));
            }
            if cur {
                let bar = [rc[0] + 3.0 * u, y + 9.0 * u, rc[0] + 6.0 * u, y + rh - 9.0 * u];
                self.text.rounded(r, scene, bar, 1.5 * u, ACCENT);
            }
            let tx = rc[0] + 14.0 * u;
            let name = clip_to(&self.text, &d.names[i], px as f32, item[2] - tx - 6.0 * u);
            let nl = self.text.label(r, scene, &name, px, if cur { WHITE } else { mix(SOFT, WHITE, row_hv) });
            nl.place(scene, tx, y + (rh - nl.h as f32) * 0.5);
            let dv = self.ease((236, "del", i), if inside(del, f.cursor) { 1.0 } else { 0.0 }, 10.0);
            if confirming {
                self.text.rounded(r, scene, del, 5.0 * u, mix([190, 54, 48, 255], [236, 92, 82, 255], dv));
                let cl = self.text.label(r, scene, "start.profile.delete_confirm", (12.0 * u) as u32, WHITE);
                cl.place(scene, del[0] + (dw - cl.w as f32) * 0.5, del[1] + (del[3] - del[1] - cl.h as f32) * 0.5);
            } else if row_hv > 0.0 {
                if dv > 0.0 {
                    self.text.rounded(r, scene, del, 5.0 * u, fade(DANGER, 0.18 * dv));
                }
                let col = fade(mix(MUTED, DANGER_T, dv), row_hv);
                if let Some(ic) = self.text.icon(r, scene, "delete", isz, col) {
                    ic.place(scene, del[0] + (dw - isz as f32) * 0.5, y + (rh - isz as f32) * 0.5);
                }
            }
            self.start.drv_items.push(item);
            self.start.drv_dels.push(del);
            y += rh;
        }
        self.text.rounded(r, scene, [l + 6.0 * u, y + sep * 0.5, rgt - 6.0 * u, y + sep * 0.5 + 1.0], 0.0, [255, 255, 255, 20]);
        y += sep;

        match &d.new {
            None => {
                let rc = [l, y, rgt, y + rh];
                let hv = self.ease((236, "new", 0), if inside(rc, f.cursor) { 1.0 } else { 0.0 }, 10.0);
                if hv > 0.0 {
                    self.text.rounded(r, scene, rc, 6.0 * u, fade([255, 255, 255, 14], hv));
                }
                if let Some(ic) = self.text.icon(r, scene, "add", isz, ACCENT) {
                    ic.place(scene, rc[0] + 10.0 * u, y + (rh - isz as f32) * 0.5);
                }
                let nl = self.text.label(r, scene, "start.profile.new", px, mix(SOFT, WHITE, hv));
                nl.place(scene, rc[0] + 10.0 * u + isz as f32 + 10.0 * u, y + (rh - nl.h as f32) * 0.5);
                self.start.drv_new_row = rc;
            }
            Some(text) => {
                let field = [l, y, rgt - rh - 4.0 * u, y + rh];
                let line = (1.5 * u).round().max(1.0);
                self.text.rounded(r, scene, field, ROW_R * u, ACCENT);
                self.text.rounded(r, scene, [field[0] + line, field[1] + line, field[2] - line, field[3] - line], (ROW_R * u - line).max(1.0), OPT_FIELD);
                let tx = field[0] + 10.0 * u;
                let room = field[2] - tx - 10.0 * u;
                if text.is_empty() {
                    let hl = self.text.label(r, scene, "start.profile.new_hint", px, MUTED);
                    hl.place(scene, tx, y + (rh - hl.h as f32) * 0.5);
                    if v.caret {
                        let c = self.text.label(r, scene, "|", px, WHITE);
                        c.place(scene, tx - c.w as f32 * 0.5, y + (rh - c.h as f32) * 0.5);
                    }
                } else {
                    let shown = format!("{}{}", text, if v.caret { "|" } else { "" });
                    let cl = self.text.label(r, scene, &clip_left(&self.text, &shown, px as f32, room), px, WHITE);
                    cl.place(scene, tx, y + (rh - cl.h as f32) * 0.5);
                }
                let cb = [field[2] + 4.0 * u, y, rgt, y + rh];
                let ok = !text.trim().is_empty();
                let hv = self.ease((236, "create", 0), if ok && inside(cb, f.cursor) { 1.0 } else { 0.0 }, 10.0);
                self.text.rounded(r, scene, cb, ROW_R * u, if ok { mix(ACCENT, ACCENT_HOT, hv) } else { OPT_ROW });
                if let Some(ic) = self.text.icon(r, scene, "check", isz, if ok { ON_ACCENT } else { MUTED }) {
                    ic.place(scene, cb[0] + (cb[2] - cb[0] - isz as f32) * 0.5, y + (rh - isz as f32) * 0.5);
                }
                if ok {
                    self.start.drv_create = cb;
                }
            }
        }
        y += rh;
        if !d.note.is_empty() {
            let nl = self.text.label(r, scene, &clip_to(&self.text, &d.note, 13.0 * u, bw - 2.0 * pad - 12.0 * u), (13.0 * u) as u32, DANGER_T);
            nl.place(scene, l + 6.0 * u, y + (note_h - nl.h as f32) * 0.5);
        }
    }
}
