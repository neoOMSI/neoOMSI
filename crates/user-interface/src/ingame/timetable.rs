//! Timetable window
use super::*;

impl Ui {
    pub(super) fn draw_timetable(
        &mut self,
        r: &Renderer,
        scene: &mut Scene,
        f: &Frame,
        s: f32,
        top: f32,
        tutorial_w: f32,
    ) {
        let Some((title, rows)) = f.timetable.as_ref() else {
            return;
        };
        let px = (15.0 * s) as u32;
        let rh = (32.0 * s).round();
        let head_h = (46.0 * s).round();
        let w = (360.0 * s).min(f.width * 0.42);
        let shown = rows
            .len()
            .min((((f.height * 0.7) - head_h) / rh) as usize)
            .max(1);
        let next = rows.iter().position(|r| r.2 == 1).unwrap_or(0);
        let first = next.saturating_sub(1).min(rows.len().saturating_sub(shown));
        let h = head_h + rh * shown as f32 + 8.0 * s;
        let beside = if f.tutorial.is_some() {
            tutorial_w + 12.0 * s
        } else {
            0.0
        };
        let x = (f.width - w - 16.0 * s - beside).max(16.0 * s);
        let y = top;
        self.text.flat = true;
        let radius = CARD_R * s;
        self.text.rounded(
            r,
            scene,
            [x - 1.0, y - 1.0, x + w + 1.0, y + h + 1.0],
            radius + 1.0,
            OPT_LINE,
        );
        self.text
            .rounded(r, scene, [x, y, x + w, y + h], radius, OPT_CARD);
        let tpx = (17.0 * s) as u32;
        let t = self.text.label(
            r,
            scene,
            &clip_to(&self.text, title, tpx as f32, w - 32.0 * s),
            tpx,
            WHITE,
        );
        t.place(scene, x + 16.0 * s, y + (head_h - t.h as f32) * 0.5);
        let hair = (y + head_h).round();
        self.text.rounded(
            r,
            scene,
            [x, hair, x + w, hair + 1.0_f32.max(s).round()],
            0.0,
            OPT_LINE,
        );
        let time_w = rows
            .iter()
            .map(|r| self.text.width(&r.1, px as f32))
            .fold(0.0f32, f32::max)
            .max(40.0 * s);
        let name_x = x + 16.0 * s + time_w + 14.0 * s;
        let line = 1.0_f32.max(s).round();
        for (k, (name, time, state)) in rows.iter().skip(first).take(shown).enumerate() {
            let ry = (hair + 4.0 * s + rh * k as f32).round();
            let rc = [x, ry, x + w, ry + rh];
            if *state == 1 {
                self.text.rounded(r, scene, rc, 0.0, OPT_GROUP_ON);
                self.text
                    .rounded(r, scene, [x, ry, x + 4.0 * s, ry + rh], 0.0, ACCENT);
            } else if k % 2 == 1 {
                self.text.rounded(r, scene, rc, 0.0, OPT_ROW);
            }
            let (tc, nc) = match state {
                0 => (MUTED, MUTED),
                1 => (txt(ACCENT), WHITE),
                _ => (SOFT, WHITE),
            };
            let tl = self.text.label(r, scene, time, px, tc);
            tl.place(scene, x + 16.0 * s, ry + (rh - tl.h as f32) * 0.5);
            let nl = self.text.label(
                r,
                scene,
                &clip_to(&self.text, name, px as f32, x + w - name_x - 14.0 * s),
                px,
                nc,
            );
            nl.place(scene, name_x, ry + (rh - nl.h as f32) * 0.5);
            if k + 1 < shown && *state != 1 {
                self.text.rounded(
                    r,
                    scene,
                    [x + 16.0 * s, ry + rh - line, x + w - 16.0 * s, ry + rh],
                    0.0,
                    OPT_LINE,
                );
            }
        }
        self.text.flat = false;
    }
}
