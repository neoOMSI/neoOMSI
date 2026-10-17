use super::*;

const WHITE_FILL: [u8; 4] = [236, 236, 236, 255];
const OK: [u8; 4] = [96, 200, 120, 255];
const BAD: [u8; 4] = [236, 92, 82, 255];

fn state_ink(state: u8) -> [u8; 4] {
    match state {
        2 => OK,
        3 => BAD,
        _ => MUTED,
    }
}

impl Ui {
    pub(in super::super) fn draw_start_join(&mut self, r: &Renderer, scene: &mut Scene, f: &Frame, m: Metrics, v: &StartView) {
        let Metrics { w, h, u, .. } = m;
        self.text.rounded(r, scene, [0.0, 0.0, w, h], 0.0, BG);
        let bar_h = self.draw_start_bar(r, scene, f, m, 1);
        let ep = out(self.start.page_t);
        let rise = 14.0 * u * (1.0 - ep);
        let card = [0.0, bar_h + rise, w, h + rise];
        self.text.alpha = ep;
        self.text.rounded(r, scene, card, 0.0, OPT_CARD);
        self.text.rounded(r, scene, [0.0, bar_h, w, bar_h + 1.0], 0.0, DIVIDER);

        let pad = (48.0 * u).round();
        let (x0, x1) = (card[0] + pad, card[2] - pad);

        let tl = self.text.label(r, scene, "start.join.nav", (46.0 * u) as u32, WHITE);
        let ty = card[1] + 26.0 * u;
        tl.place(scene, x0, ty);
        self.text.rounded(r, scene, [x0, ty + tl.h as f32 + 4.0 * u, x0 + 48.0 * u, ty + tl.h as f32 + 7.0 * u], 0.0, ACCENT);
        let sl = self.text.label(r, scene, "start.join.note", (17.0 * u) as u32, SOFT);
        let sy = ty + tl.h as f32 + 14.0 * u;
        sl.place(scene, x0, sy);

        let body_x1 = x1.min(x0 + 900.0 * u);
        let body_y = sy + sl.h as f32 + 40.0 * u;
        self.draw_join_tab(r, scene, f, u, v, [x0, body_y, body_x1, card[3] - 16.0 * u]);
        self.text.alpha = 1.0;
    }

    fn draw_join_tab(&mut self, r: &Renderer, scene: &mut Scene, f: &Frame, u: f32, v: &StartView, a: [f32; 4]) {
        let cap = self.text.label(r, scene, "start.join.label", (12.0 * u) as u32, MUTED);
        cap.place(scene, a[0], a[1]);
        let fh = (56.0 * u).round();
        let fy = a[1] + cap.h as f32 + 8.0 * u;
        let field = [a[0], fy, a[2], fy + fh];
        let hv = self.ease((216, "field", 0), if inside(field, f.cursor) { 1.0 } else { 0.0 }, 10.0);
        let fv = self.ease((216, "fieldon", 0), if v.addr_focus { 1.0 } else { 0.0 }, 12.0);
        self.text.rounded(r, scene, field, ROW_R * u, mix(OPT_LINE, ACCENT, fv));
        let line = (1.5 * u).round().max(1.0);
        let inner = [field[0] + line, field[1] + line, field[2] - line, field[3] - line];
        self.text.rounded(r, scene, inner, (ROW_R * u - line).max(1.0), mix(OPT_FIELD, OPT_FIELD_HOT, hv));
        let isz = (24.0 * u).round();
        if let Some(ic) = self.text.icon(r, scene, "link", isz as u32, MUTED) {
            ic.place(scene, field[0] + 16.0 * u, fy + (fh - isz) * 0.5);
        }
        let px = (18.0 * u) as u32;
        let tx = field[0] + 16.0 * u + isz + 12.0 * u;
        let room = field[2] - tx - 16.0 * u;
        if v.address.is_empty() {
            let l = self.text.label(r, scene, "start.join.placeholder", px, MUTED);
            l.place(scene, tx, fy + (fh - l.h as f32) * 0.5);
            if v.caret && v.addr_focus {
                let c = self.text.label(r, scene, "|", px, WHITE);
                c.place(scene, tx - c.w as f32 * 0.5, fy + (fh - c.h as f32) * 0.5);
            }
        } else {
            let shown = format!("{}{}", v.address, if v.caret && v.addr_focus { "|" } else { "" });
            let text = clip_left(&self.text, &shown, px as f32, room);
            let l = self.text.label(r, scene, &text, px, WHITE);
            if v.addr_sel {
                let pad = 4.0 * u;
                self.text.rounded(r, scene, [tx - pad, fy + (fh - l.h as f32) * 0.5 - pad * 0.5, tx + l.w as f32 + pad, fy + (fh + l.h as f32) * 0.5 + pad * 0.5], 4.0 * u, [232, 160, 48, 90]);
            }
            l.place(scene, tx, fy + (fh - l.h as f32) * 0.5);
        }

        let ny = field[3] + 10.0 * u;
        let nh = if v.addr_state == 0 {
            let l = self.text.label(r, scene, "start.join.hint", (13.0 * u) as u32, MUTED);
            l.place(scene, field[0] + 4.0 * u, ny);
            l.h as f32
        } else {
            let ink = state_ink(v.addr_state);
            let dot = (10.0 * u).round();
            let l = self.text.label(r, scene, &clip_to(&self.text, v.addr_note, 14.0 * u, field[2] - field[0] - 40.0 * u), (14.0 * u) as u32, ink);
            self.text.rounded(r, scene, [field[0] + 6.0 * u, ny + (l.h as f32 - dot) * 0.5, field[0] + 6.0 * u + dot, ny + (l.h as f32 + dot) * 0.5], dot * 0.5, ink);
            l.place(scene, field[0] + 6.0 * u + dot + 8.0 * u, ny);
            l.h as f32
        };

        let gy = ny + nh + 36.0 * u;
        let gl = self.text.label(r, scene, "start.join.game", (12.0 * u) as u32, MUTED);
        gl.place(scene, a[0], gy);
        let rh = (76.0 * u).round();
        let row = [a[0], gy + gl.h as f32 + 8.0 * u, a[2], gy + gl.h as f32 + 8.0 * u + rh];
        let hot = inside(row, f.cursor);
        let rv = self.ease((216, "host", 0), if hot { 1.0 } else { 0.0 }, 10.0);
        let on = self.ease((216, "hoston", 0), if v.host { 1.0 } else { 0.0 }, 12.0);
        self.text.rounded(r, scene, row, ROW_R * u, mix(OPT_ROW, OPT_ROW_HOT, rv));
        let ov = self.ease((216, "hostbar", 0), if v.host { 1.0 } else { 0.0 }, 10.0);
        if ov > 0.0 {
            self.accent_bar(r, scene, row, ov, false, u);
        }
        let ix = row[0] + 18.0 * u;
        if let Some(ic) = self.text.icon(r, scene, "wifi_tethering", isz as u32, ACCENT) {
            ic.place(scene, ix, row[1] + (rh - isz) * 0.5);
        }
        let t = self.text.label(r, scene, "start.join.host", (19.0 * u) as u32, WHITE);
        let n = self.text.label(r, scene, "start.join.host_note", (13.0 * u) as u32, MUTED);
        let tx = ix + isz + 16.0 * u;
        let ty = row[1] + (rh - t.h as f32 - n.h as f32) * 0.5;
        t.place(scene, tx, ty);
        n.place(scene, tx, ty + t.h as f32);
        let (sw, sh) = ((50.0 * u).round(), (28.0 * u).round());
        let (sx, sy) = (row[2] - 18.0 * u - sw, row[1] + (rh - sh) * 0.5);
        self.text.rounded(r, scene, [sx, sy, sx + sw, sy + sh], sh * 0.5, mix(OPT_GROUP, ACCENT, on));
        let k = sh - 8.0 * u;
        let kx = sx + 4.0 * u + (sw - sh) * on;
        self.text.rounded(r, scene, [kx, sy + 4.0 * u, kx + k, sy + 4.0 * u + k], k * 0.5, WHITE_FILL);

        self.start.field = field;
        self.start.host_row = row;
    }
}
