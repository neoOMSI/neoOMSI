use super::*;

impl Ui {
    pub(super) fn draw_quick_menu(&mut self, r: &Renderer, scene: &mut Scene, f: &Frame) {
        self.quick_rects.clear();
        self.quick_confirm_rects = [[0.0; 4]; 2];
        let Some(menu) = f.quick_menu.as_ref() else {
            return;
        };

        let s = f.scale.max(0.5) * f.ui_scale;
        let (cols, tile_w, tile_h, gap, pad, header_h) =
            (4usize, 88.0 * s, 55.0 * s, 6.0 * s, 12.0 * s, 30.0 * s);
        let rows = menu.items.len().div_ceil(cols);
        let panel_w = cols as f32 * tile_w + (cols + 1) as f32 * gap;
        let panel_h = header_h + rows as f32 * tile_h + (rows + 1) as f32 * gap;
        let panel = [
            f.width - panel_w - 16.0 * s,
            f.height - panel_h - 32.0 * s,
            f.width - 16.0 * s,
            f.height - 32.0 * s,
        ];
        self.text
            .rounded(r, scene, panel, 9.0 * s, [255, 255, 255, 40]);
        self.text.rounded(
            r,
            scene,
            [panel[0] + s, panel[1] + s, panel[2] - s, panel[3] - s],
            8.0 * s,
            [14, 17, 22, 232],
        );
        let title = self.text.label(
            r,
            scene,
            "Quick menu  ·  Alt to close",
            (13.0 * s) as u32,
            [245, 245, 245, 235],
        );
        title.place(scene, panel[0] + pad, panel[1] + (header_h - title.h as f32) * 0.5);

        for (i, &(id, label)) in menu.items.iter().enumerate() {
            let row = i / cols;
            let col = i % cols;
            let x = panel[0] + gap + col as f32 * (tile_w + gap);
            let y = panel[1] + header_h + row as f32 * (tile_h + gap);
            let rect = [x, y, x + tile_w, y + tile_h];
            self.quick_rects.push(rect);
            let disabled = menu.disabled.get(i).copied().unwrap_or(false);
            let hit = f.cursor.0 >= x
                && f.cursor.0 <= rect[2]
                && f.cursor.1 >= y
                && f.cursor.1 <= rect[3];
            let ending = menu.end_duty && i == 4;
            let active = id == "arrows" && menu.arrows_on;
            let fill = if disabled {
                [44, 48, 55, 115]
            } else if ending {
                if hit {
                    [145, 55, 50, 250]
                } else {
                    [91, 42, 39, 242]
                }
            } else if active {
                [244, 127, 48, 245]
            } else if hit {
                [78, 84, 92, 250]
            } else {
                [40, 44, 50, 240]
            };
            self.text.rounded(r, scene, rect, 5.0 * s, fill);
            let text = if ending { "End timetable" } else { label };
            let color = if disabled {
                [160, 164, 170, 120]
            } else if active {
                [24, 20, 16, 255]
            } else {
                [245, 245, 245, 245]
            };
            let label = crate::tr(text);
            let max_px = (tile_w - 10.0 * s).max(1.0);
            let label = clip_to(&self.text, &label, (12.5 * s) as f32, max_px);
            let line = self.text.label(r, scene, &label, (12.5 * s) as u32, color);
            line.place(
                scene,
                rect[0] + (tile_w - line.w as f32) * 0.5,
                rect[1] + (tile_h - line.h as f32) * 0.5,
            );
        }

        if menu.confirm_end_duty {
            let veil = self.text.plate(r, scene, 6);
            scene.overlays.push((veil, [0.0, 0.0, f.width, f.height]));
            let w = (f.width - 48.0 * s).min(440.0 * s);
            let h = 168.0 * s;
            let x = (f.width - w) * 0.5;
            let y = (f.height - h) * 0.5;
            let rect = [x, y, x + w, y + h];
            self.text
                .rounded(r, scene, rect, 9.0 * s, [255, 255, 255, 50]);
            self.text.rounded(
                r,
                scene,
                [rect[0] + s, rect[1] + s, rect[2] - s, rect[3] - s],
                8.0 * s,
                [24, 27, 32, 253],
            );
            let title = self.text.label(
                r,
                scene,
                "End the active timetable?",
                (17.0 * s) as u32,
                [255, 255, 255, 255],
            );
            title.place(scene, x + (w - title.w as f32) * 0.5, y + 24.0 * s);
            let body = self.text.label(
                r,
                scene,
                "The game will continue in free drive.",
                (13.0 * s) as u32,
                [200, 205, 212, 230],
            );
            body.place(scene, x + (w - body.w as f32) * 0.5, y + 57.0 * s);
            let bw = 112.0 * s;
            let bh = 34.0 * s;
            let yes = [
                x + w * 0.5 - bw - 6.0 * s,
                y + h - bh - 16.0 * s,
                x + w * 0.5 - 6.0 * s,
                y + h - 16.0 * s,
            ];
            let no = [
                x + w * 0.5 + 6.0 * s,
                y + h - bh - 16.0 * s,
                x + w * 0.5 + bw + 6.0 * s,
                y + h - 16.0 * s,
            ];
            self.quick_confirm_rects = [yes, no];
            self.text.rounded(r, scene, yes, 5.0 * s, [168, 50, 44, 250]);
            self.text.rounded(r, scene, no, 5.0 * s, [57, 62, 70, 250]);
            let yes_label = self.text.label(
                r,
                scene,
                "End timetable",
                (12.5 * s) as u32,
                [255, 255, 255, 255],
            );
            yes_label.place(
                scene,
                yes[0] + (bw - yes_label.w as f32) * 0.5,
                yes[1] + (bh - yes_label.h as f32) * 0.5,
            );
            let no_label = self.text.label(
                r,
                scene,
                "Keep driving",
                (12.5 * s) as u32,
                [245, 245, 245, 255],
            );
            no_label.place(
                scene,
                no[0] + (bw - no_label.w as f32) * 0.5,
                no[1] + (bh - no_label.h as f32) * 0.5,
            );
        }
    }
}
