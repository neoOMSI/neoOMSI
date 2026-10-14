use super::*;

impl Ui {
    #[allow(clippy::too_many_arguments)]
    pub(super) fn start_editor(&mut self, r: &Renderer, scene: &mut Scene, f: &Frame, card: [f32; 4], v: &StartView, u: f32, hits: &mut Hits) {
        let tab_a = if self.start.prev_sect == 0 { self.start.sect_e } else { self.start.page_e };
        self.text.alpha = self.start.page_e;
        let side = (36.0 * u).round();
        let y0 = card[1] + 12.0 * u;
        let tab_h = (44.0 * u).round();
        let band_b = y0 + tab_h + 12.0 * u;
        self.text.rounded(r, scene, [card[0], card[1], card[2], band_b], 0.0, SIDEBAR);
        self.text.rounded(r, scene, [card[0], band_b, card[2], band_b + 1.0], 0.0, [255, 255, 255, 14]);
        self.text.alpha = tab_a;
        let px = (16.0 * u) as u32;
        let isz = (20.0 * u).round();
        let mut x = card[0] + side;
        for k in 0..3 {
            let label = tr(TAB_KEYS[k]);
            let lw = self.text.width(&label, px as f32) + isz + 8.0 * u + 36.0 * u;
            let rc = [x, y0, x + lw, y0 + tab_h];
            let on = v.sect == k + 1;
            let hot = inside(rc, f.cursor);
            let ov = self.ease((221, "tab", k), if on { 1.0 } else { 0.0 }, 12.0);
            let hv = self.ease((221, "tabhot", k), if hot && !on { 1.0 } else { 0.0 }, 12.0);
            self.text.rounded(r, scene, rc, tab_h * 0.5, mix(fade(LIT, hv), OPT_GROUP_ON, ov));
            let l = self.text.label(r, scene, &label, px, if on { WHITE } else { mix(SOFT, WHITE, hv) });
            let gx = rc[0] + 18.0 * u;
            if let Some(ic) = self.text.icon(r, scene, TAB_ICONS[k], isz as u32, if on { ACCENT } else { MUTED }) {
                ic.place(scene, gx, y0 + (tab_h - isz) * 0.5);
            }
            l.place(scene, gx + isz + 8.0 * u, y0 + (tab_h - l.h as f32) * 0.5);
            self.start.sect_tabs.push(rc);
            x += lw + 6.0 * u;
        }
        let bs = (40.0 * u).round();
        let close = [card[2] - side - bs, y0 + (tab_h - bs) * 0.5, card[2] - side, y0 + (tab_h + bs) * 0.5];
        let cv = self.ease((221, "close", 0), if inside(close, f.cursor) { 1.0 } else { 0.0 }, 12.0);
        self.text.rounded(r, scene, close, bs * 0.5, fade(LIT, cv));
        let csz = (22.0 * u).round();
        if let Some(ic) = self.text.icon(r, scene, "close", csz as u32, mix(MUTED, WHITE, cv)) {
            ic.place(scene, close[0] + (bs - csz) * 0.5, close[1] + (bs - csz) * 0.5);
        }
        self.start.close = close;

        let pw = ((card[2] - card[0]) * 0.34).clamp(380.0 * u, 540.0 * u).round();
        let left = [card[0] + side, band_b + 24.0 * u, card[0] + pw, card[3] - 28.0 * u];
        let right = [card[0] + pw + side, band_b + 1.0, card[2], card[3]];
        self.text.alpha = self.start.page_e;
        self.text.rounded(r, scene, right, 0.0, OPT_ROW);
        self.text.rounded(r, scene, [right[0], right[1], right[0] + 1.0, right[3]], 0.0, [255, 255, 255, 14]);
        self.text.alpha = self.start.sect_e;

        let s = v.sect;
        let hl = self.text.label(r, scene, HEADS[s - 1], (28.0 * u) as u32, WHITE);
        hl.place(scene, left[0] + 4.0 * u, left[1]);
        let btn_h = (54.0 * u).round();
        let btn = [left[0], left[3] - btn_h, left[2], left[3]];
        let area = [left[0], left[1] + hl.h as f32 + 14.0 * u, left[2], btn[1] - 14.0 * u];
        match s {
            1 => self.start_tab_map(r, scene, f, area, v, u, hits),
            2 => self.start_tab_bus(r, scene, f, area, v, u, hits),
            _ => self.start_tab_time(r, scene, f, area, v, u, hits),
        }
        self.start_preview(r, scene, f, right, v, u, hits);

        if s < 3 {
            let label = format!("{} {}", tr("start.maps.next"), tr(TAB_KEYS[s]));
            self.start_button(r, scene, f, btn, &label, "chevron_right", 1, u);
            self.start.next = btn;
        } else {
            self.start_button(r, scene, f, btn, "start.maps.drive", "play_arrow", 0, u);
            self.start.go = btn;
        }
    }

    #[allow(clippy::too_many_arguments)]
    pub(super) fn start_tab_map(&mut self, r: &Renderer, scene: &mut Scene, f: &Frame, a: [f32; 4], v: &StartView, u: f32, hits: &mut Hits) {
        let _ = hits;
        let row_h = (ROW_H * u).round();
        let field_h = (46.0 * u).round();
        let cap = (18.0 * u).round();
        let gap = 16.0 * u;
        let filter_h = (38.0 * u).round();
        let route = !v.free;
        let open = v.drop == 11 || v.drop == 12 || v.drop == 16;
        let open_h = if open { v.drop_items.len().clamp(1, 6) as f32 * row_h + 14.0 * u } else { 0.0 };
        let head = filter_h + 6.0 * u;
        let fixed = (cap + 6.0 * u + field_h) * 2.0
            + field_h
            + open_h
            + if route { gap * 4.0 + head * 2.0 } else { gap * 2.0 };
        let avail = (a[3] - a[1] - fixed).max(row_h * 4.0);
        let lh = (avail * 0.4).max(row_h * 2.0);
        let th = (avail * 0.6).max(row_h * 2.0);
        let mut y = a[1];

        self.start_caption(r, scene, "start.maps.map", a[0] + 4.0 * u, y, u);
        y += cap + 6.0 * u;
        let map = v.maps.get(v.map).map(String::as_str).unwrap_or("-");
        self.start_dropdown(r, scene, f, [a[0], y, a[2], y + field_h], 0, "", map, v.drop == 11, u);
        y += field_h;
        if v.drop == 11 {
            y = self.start_drop_list(r, scene, f, a, y + 6.0 * u, v, u);
        }
        y += gap;

        if !route {
            self.start_caption(r, scene, "start.maps.entry", a[0] + 4.0 * u, y, u);
            y += cap + 6.0 * u;
            self.start_dropdown(r, scene, f, [a[0], y, a[2], y + field_h], 1, "", v.entry_text, v.drop == 12, u);
            y += field_h;
            if v.drop == 12 {
                y = self.start_drop_list(r, scene, f, a, y + 6.0 * u, v, u);
            }
            y += gap;
        }

        let tog = [a[0], y, a[2], y + field_h];
        self.start_toggle(r, scene, f, tog, "start.maps.free", v.free, u);
        self.start.mode.push(tog);
        y += field_h;
        if !route {
            return;
        }
        y += gap;

        let ph = tr("start.maps.filter");
        let fw = ((a[2] - a[0]) * 0.58).round();
        self.start_section_head(r, scene, "start.maps.line", [a[0], y, a[2], y + filter_h], u);
        let fr = [a[2] - fw, y, a[2], y + filter_h];
        self.start_field(r, scene, f, fr, None, Some("search"), v.line_query, &ph, v.edit == 3, v.caret, 2, u);
        self.start.line_filter = fr;
        y += head;
        let lb = [a[0], y, a[2], y + lh];
        let rows: Vec<(String, String)> = v.lines.iter().map(|l| (l.badge.clone(), l.label.clone())).collect();
        let mut rects = Vec::new();
        self.start.line_vis = self.start_pick_list(r, scene, f, lb, &rows, true, v.line_sel, v.line_top, 216, u, &mut rects, v.zone == 2, "start.maps.nolines");
        self.start.lines = rects;
        self.start.line_box = lb;
        y += lh + gap;

        self.start_section_head(r, scene, "start.maps.tour", [a[0], y, a[2], y + filter_h], u);
        let fr = [a[2] - fw, y, a[2], y + filter_h];
        self.start_field(r, scene, f, fr, None, Some("search"), v.tour_query, &ph, v.edit == 4, v.caret, 3, u);
        self.start.tour_filter = fr;
        y += head;
        let tb = [a[0], y, a[2], y + th];
        let rows: Vec<(String, String)> = v.tours.iter().map(|t| (t.label.clone(), t.span.clone())).collect();
        let mut rects = Vec::new();
        self.start.tour_vis = self.start_pick_list(r, scene, f, tb, &rows, false, v.tour_sel, v.tour_top, 217, u, &mut rects, v.zone == 3, "start.maps.notours");
        self.start.tours = rects;
        self.start.tour_box = tb;
        y += th + gap;

        self.start_caption(r, scene, "start.maps.trip", a[0] + 4.0 * u, y, u);
        y += cap + 6.0 * u;
        self.start_dropdown(r, scene, f, [a[0], y, a[2], y + field_h], 1, "", v.trip_text, v.drop == 16, u);
        y += field_h;
        if v.drop == 16 {
            self.start_drop_list(r, scene, f, a, y + 6.0 * u, v, u);
        }
    }
}
