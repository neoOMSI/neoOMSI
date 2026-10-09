//! The full-screen pause screen

use super::*;

/// A divider goes above entry `i` (resume/save | menus | quit).
fn break_before(i: usize, n: usize) -> bool {
    n >= 4 && i > 0 && (i == 2 || i == n - 1)
}

/// Font size of the entries for the room `avail` leaves.
fn entry_px(avail: f32, n: usize, gap: f32, u: f32) -> f32 {
    (((avail / n as f32) - gap - 10.0 * u) / 1.3).clamp(13.0 * u, 21.0 * u)
}

impl Ui {
    pub(super) fn draw_pause_screen(
        &mut self,
        r: &Renderer,
        scene: &mut Scene,
        m: Metrics,
        sel: usize,
    ) {
        let Metrics { w, h, u, mx, line } = m;
        self.pause_items.clear();
        let o = out(self.pause_open);
        self.text.alpha = o;
        let entries = self.pause_entries.clone();
        let n = entries.len().max(1);

        // the picture dims, a panel on the left carries the menu
        self.text.rounded(r, scene, [0.0, 0.0, w, h], 0.0, [6, 8, 12, 150]);
        let mx = (mx * 0.7).round();
        let row_w = 300.0 * u;
        let pw = mx + row_w + 8.0 * u;
        // it slides in from the left (and out again)
        let dx = -pw * (1.0 - o);
        self.text.rounded(r, scene, [dx, 0.0, pw + dx, h], 0.0, [8, 10, 14, 238]);
        self.text.rounded(r, scene, [pw - line + dx, 0.0, pw + dx, h], 0.0, BORDER);

        // the logo, then a short accent rule
        let mut y = (h * 0.06).round();
        let slide = dx;
        self.ensure_logo();
        if let Some((tex, iw, ih)) = self.logo_at(r, scene, (38.0 * u).round()) {
            if o > 0.05 {
                let x = (mx + slide).round();
                scene.overlays.push((tex, [x, y, x + iw as f32, y + ih as f32]));
            }
            y += ih as f32 + 12.0 * u;
        } else {
            let title = self.text.label(r, scene, "neoOMSI", (30.0 * u) as u32, WHITE);
            title.place(scene, mx + slide, y);
            y += title.h as f32 + 6.0 * u;
        }
        self.text.rounded(
            r,
            scene,
            [mx + dx, y, mx + 48.0 * u + dx, y + 3.0 * u],
            0.0,
            ACCENT,
        );
        y += 22.0 * u;

        // the entries: as large as the screen leaves room for
        let gap = 2.0 * u;
        // (groups: resume/save | menus | quit, a divider between them)
        let brk = |i: usize| break_before(i, n);
        let breaks = (0..n).filter(|i| brk(*i)).count() as f32;
        let sep_h = 20.0 * u;
        let avail = (h - y - 28.0 * u - breaks * sep_h).max(100.0 * u);
        let px = entry_px(avail, n, gap, u);
        for (i, name) in entries.iter().enumerate() {
            let e = 1.0;
            let danger = n >= 2 && i == n - 1;
            if brk(i) {
                let ly = y + 8.0 * u;
                let lx = mx - 24.0 * u + dx;
                self.text.rounded(
                    r,
                    scene,
                    [lx, ly, lx + row_w - 24.0 * u, ly + line],
                    0.0,
                    [255, 255, 255, 34],
                );
                y += sep_h;
            }
            let on = i == sel;
            let col = if danger {
                [222, 78, 68, 0]
            } else if on {
                WHITE
            } else {
                SOFT
            };
            let l = self.text.label(r, scene, name, px as u32, col);
            let row_h = l.h as f32 + 10.0 * u;
            let rc = [mx - 24.0 * u, y, mx - 24.0 * u + row_w, y + row_h];
            let hv = self.ease((200, "hover", i), if on { 1.0 } else { 0.0 }, 8.0);
            let ox = dx * e;
            if hv > 0.0 {
                let rr = [rc[0] + ox, rc[1], rc[2] + ox, rc[3]];
                let hc = if danger { fade(DANGER, 0.16) } else { LIT };
                self.text.rounded(r, scene, rr, 6.0 * u, fade(hc, hv * e));
            }
            if hv > 0.0 {
                let bar = [rc[0] + ox, rc[1] + 4.0 * u, rc[0] + ox + 3.0 * u, rc[3] - 4.0 * u];
                let bc = if danger { DANGER } else { ACCENT };
                self.text.rounded(r, scene, bar, 0.0, fade(bc, hv * e));
            }
            l.place(scene, mx + ox, y + 5.0 * u);
            self.pause_items.push(rc);
            y += row_h + gap;
        }
        self.text.alpha = 1.0;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn no_dividers_below_four_entries() {
        for n in 0..4 {
            assert!((0..n.max(1)).all(|i| !break_before(i, n)));
        }
    }

    #[test]
    fn dividers_for_default_entries() {
        let n = PAUSE_ENTRIES.len();
        let at: Vec<usize> = (0..n).filter(|i| break_before(*i, n)).collect();
        assert_eq!(at, vec![2, n - 1]);
    }

    #[test]
    fn first_entry_never_has_divider() {
        for n in 0..20 {
            assert!(!break_before(0, n));
        }
    }

    #[test]
    fn entry_px_is_clamped() {
        assert_eq!(entry_px(10_000.0, 6, 2.0, 1.0), 21.0);
        assert_eq!(entry_px(1.0, 6, 2.0, 1.0), 13.0);
        assert_eq!(entry_px(10_000.0, 6, 2.0, 2.0), 42.0);
        let mid = entry_px(6.0 * 30.0, 6, 2.0, 1.0);
        assert!((13.0..=21.0).contains(&mid));
    }
}
