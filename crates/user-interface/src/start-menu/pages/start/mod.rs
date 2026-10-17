mod editor;
mod overview;
mod tab_bus;
mod tab_time;
mod widgets;

use super::*;

const ROW_H: f32 = 36.0;
const TAB_KEYS: [&str; 3] = ["start.maps.t1", "start.maps.t2", "start.maps.t3"];
const TAB_ICONS: [&str; 3] = ["route", "directions_bus", "schedule"];
const HEADS: [&str; 3] = ["start.maps.h1", "start.maps.h2", "start.maps.h3"];

type Hits = ([[f32; 4]; 16], [[f32; 4]; 32]);

fn row_seen(top: f32, bottom: f32, y: f32, row_h: f32) -> Option<(f32, f32, f32)> {
    let (y0, y1) = (y.max(top), (y + row_h).min(bottom));
    let seen = ((y1 - y0) / row_h).clamp(0.0, 1.0);
    (seen > 0.0).then_some((y0, y1, seen))
}

fn tr(key: &str) -> String {
    (&*crate::tr(key)).to_string()
}

impl Ui {
    pub(in super::super) fn draw_start_maps(&mut self, r: &Renderer, scene: &mut Scene, f: &Frame, m: Metrics, v: &StartView) {
        let Metrics { w, h, u, .. } = m;
        self.start.view_h = h;
        self.text.rounded(r, scene, [0.0, 0.0, w, h], 0.0, BG);
        let bar_h = self.draw_start_bar(r, scene, f, m, 0);
        if self.start.last_sect != Some(v.sect) {
            self.start.prev_sect = self.start.last_sect.unwrap_or(v.sect);
            self.start.last_sect = Some(v.sect);
            self.start.sect_t = if self.start.prev_sect != 0 && v.sect != 0 { 1.0 } else { 0.0 };
        }
        self.start.sect_t = (self.start.sect_t + self.anim_dt / 0.2).min(1.0);
        let ep = out(self.start.page_t);
        let es = out(self.start.sect_t).min(ep);
        self.start.page_e = ep;
        self.start.sect_e = es;
        self.start.enter = es;
        let rise = 14.0 * u * (1.0 - ep);
        let card = [0.0, bar_h + rise, w, h + rise];
        self.text.alpha = ep;
        self.text.rounded(r, scene, card, 0.0, OPT_CARD);

        self.text.rounded(r, scene, [0.0, bar_h, w, bar_h + 1.0], 0.0, DIVIDER);
        let mut hits: Hits = ([[0.0; 4]; 16], [[0.0; 4]; 32]);
        if v.sect == 0 {
            self.start_overview(r, scene, f, card, v, u, &mut hits);
        } else {
            self.start_editor(r, scene, f, card, v, u, &mut hits);
        }
        self.text.alpha = 1.0;
        self.start.rows = hits.0.to_vec();
        self.start.arrows = hits.1.to_vec();
    }
}

const WHITE_FILL: [u8; 4] = [236, 236, 236, 255];
