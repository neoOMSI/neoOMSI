use super::*;

const ROW_PAD: f32 = 20.0;

fn entry_icon(id: &str) -> &'static str {
    match id {
        "start" => "play_arrow",
        "continue" => "history",
        "join" => "groups",
        "options" => "settings",
        _ => "logout",
    }
}

const OFF: [u8; 4] = [92, 92, 92, 0];

impl Ui {
    pub(super) fn draw_start_screen(&mut self, r: &Renderer, scene: &mut Scene, f: &Frame, m: Metrics, v: &StartView) {
        let Metrics { w, h, u, mx, line } = m;
        let o = out(self.start.t);
        self.text.alpha = o;
        self.text.rounded(r, scene, [0.0, 0.0, w, h], 0.0, BG);
        let mx = (mx * 0.7).round();
        let row_w = 320.0 * u;
        let pw = mx + row_w + 8.0 * u;
        let dx = -pw * (1.0 - o);
        self.text.rounded(r, scene, [dx, 0.0, pw + dx, h], 0.0, SIDEBAR);

        let mut y = (h * 0.06).round();
        self.ensure_logo();
        if let Some((tex, iw, ih)) = self.logo_at(r, scene, (38.0 * u).round()) {
            if o > 0.05 {
                let x = (mx + dx).round();
                scene.overlays.push((tex, [x, y, x + iw as f32, y + ih as f32]));
            }
            y += ih as f32 + 12.0 * u;
        } else {
            let title = self.text.label(r, scene, "neoOMSI", (30.0 * u) as u32, WHITE);
            title.place(scene, mx + dx, y);
            y += title.h as f32 + 6.0 * u;
        }
        self.text
            .rounded(r, scene, [mx + dx, y, mx + 48.0 * u + dx, y + 3.0 * u], 0.0, ACCENT);
        y += 22.0 * u;

        let n = START_ENTRIES.len();
        let gap = 4.0 * u;
        let sep_h = 36.0 * u;
        let px = (22.0 * u) as u32;
        for (i, id) in START_ENTRIES.iter().enumerate() {
            let danger = i == n - 1;
            if i == 3 || danger {
                let ly = (y + (sep_h - gap - line) * 0.5).round();
                let lx = mx - 24.0 * u + dx;
                self.text
                    .rounded(r, scene, [lx, ly, lx + row_w - 24.0 * u, ly + line], 0.0, [255, 255, 255, 34]);
                y += sep_h;
            }
            let enabled = *id != "continue" || v.can_continue;
            let on = enabled && i == v.sel;
            let col = if !enabled {
                OFF
            } else if danger {
                [222, 78, 68, 0]
            } else if on {
                WHITE
            } else {
                SOFT
            };
            let key = format!("start.entry.{id}");
            let l = self.text.label(r, scene, &key, px, col);
            let row_h = l.h as f32 + ROW_PAD * u;
            let rc = [mx - 24.0 * u, y, mx - 24.0 * u + row_w, y + row_h];
            let hv = self.ease((211, "hover", i), if on { 1.0 } else { 0.0 }, 8.0);
            if hv > 0.0 {
                let rr = [rc[0] + dx, rc[1], rc[2] + dx, rc[3]];
                let hc = if danger { fade(DANGER, 0.1) } else { [255, 255, 255, 8] };
                self.text.rounded(r, scene, rr, 6.0 * u, fade(hc, hv));
                let bar = [rc[0] + dx, rc[1] + 4.0 * u, rc[0] + dx + 3.0 * u, rc[3] - 4.0 * u];
                self.text.rounded(r, scene, bar, 0.0, fade(if danger { DANGER } else { ACCENT }, hv));
            }
            let isz = (px as f32 * 1.1).round();
            let icol = if !enabled {
                OFF
            } else if danger {
                col
            } else if on {
                ACCENT
            } else {
                MUTED
            };
            let ty = y + ROW_PAD * 0.5 * u;
            let tx = match self.text.icon(r, scene, entry_icon(id), isz as u32, icol) {
                Some(ic) => {
                    ic.place(scene, mx + dx, (ty + (l.h as f32 - isz) * 0.5).round());
                    mx + isz + 16.0 * u
                }
                None => mx,
            };
            l.place(scene, tx + dx, ty);
            self.start.items.push(rc);
            y += row_h + gap;
        }
        self.start_update_chip(r, scene, f, pw + dx - 16.0 * u, h - 36.0 * u, u);
        self.text.alpha = 1.0;
    }
}
