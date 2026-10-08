use super::*;

struct QuickLayout {
    cols: usize,
    tile_w: f32,
    tile_h: f32,
    gap: f32,
    pad: f32,
    header_h: f32,
    panel: [f32; 4],
}

struct QuickConfirmLayout {
    scale: f32,
    panel: [f32; 4],
    yes: [f32; 4],
    no: [f32; 4],
}

fn quick_layout(width: f32, height: f32, scale: f32, count: usize) -> QuickLayout {
    let width = width.max(1.0);
    let height = height.max(1.0);
    let scale = scale.max(0.5);
    let margin_x = (16.0 * scale).min(width * 0.08);
    let margin_y = (16.0 * scale).min(height * 0.08);
    let max_w = (width - margin_x * 2.0).max(1.0);
    let max_h = (height - margin_y * 2.0).max(1.0);
    let gap = (6.0 * scale).min(max_w / 10.0).min(max_h / 10.0);
    let pad = (12.0 * scale).min(max_w * 0.08);
    let header_h = (30.0 * scale).min(max_h * 0.2);
    let desired_w = 88.0 * scale;
    let desired_h = 55.0 * scale;
    let mut best = (0.0, 1, 1.0, 1.0);

    for cols in 1..=count.clamp(1, 4) {
        let rows = count.max(1).div_ceil(cols);
        let tile_w = (max_w - gap * (cols + 1) as f32) / cols as f32;
        let tile_h = (max_h - header_h - gap * (rows + 1) as f32) / rows as f32;
        let tile_w = desired_w.min(tile_w).max(1.0);
        let tile_h = desired_h.min(tile_h).max(1.0);
        let fit = quick_label_size(scale, tile_w, tile_h) / (12.5 * scale);
        if fit >= best.0 {
            best = (fit, cols, tile_w, tile_h);
        }
    }

    let (_, cols, tile_w, tile_h) = best;
    let rows = count.max(1).div_ceil(cols);
    let panel_w = cols as f32 * tile_w + (cols + 1) as f32 * gap;
    let panel_h = header_h + rows as f32 * tile_h + (rows + 1) as f32 * gap;
    let left = (width - margin_x - panel_w).max(0.0);
    let top = (height - margin_y - panel_h).max(0.0);
    QuickLayout {
        cols,
        tile_w,
        tile_h,
        gap,
        pad,
        header_h,
        panel: [left, top, left + panel_w, top + panel_h],
    }
}

fn quick_confirm_layout(width: f32, height: f32, scale: f32) -> QuickConfirmLayout {
    let width = width.max(1.0);
    let height = height.max(1.0);
    let scale = scale
        .max(0.0)
        .min((width - 24.0).max(1.0) / 440.0)
        .min((height - 24.0).max(1.0) / 168.0);
    let panel_w = 440.0 * scale;
    let panel_h = 168.0 * scale;
    let left = (width - panel_w) * 0.5;
    let top = (height - panel_h) * 0.5;
    let bw = 112.0 * scale;
    let bh = 34.0 * scale;
    let yes = [
        left + panel_w * 0.5 - bw - 6.0 * scale,
        top + panel_h - bh - 16.0 * scale,
        left + panel_w * 0.5 - 6.0 * scale,
        top + panel_h - 16.0 * scale,
    ];
    let no = [
        left + panel_w * 0.5 + 6.0 * scale,
        top + panel_h - bh - 16.0 * scale,
        left + panel_w * 0.5 + bw + 6.0 * scale,
        top + panel_h - 16.0 * scale,
    ];
    QuickConfirmLayout {
        scale,
        panel: [left, top, left + panel_w, top + panel_h],
        yes,
        no,
    }
}

fn quick_label_size(scale: f32, tile_w: f32, tile_h: f32) -> f32 {
    let text_w = (tile_w - 10.0 * scale).max(1.0);
    (12.5 * scale).min(tile_h * 0.42).min(text_w * 0.2).max(1.0)
}

impl Ui {
    pub(super) fn draw_quick_menu(&mut self, r: &Renderer, scene: &mut Scene, f: &Frame) {
        self.quick_rects.clear();
        self.quick_confirm_rects.clear();
        let Some(menu) = f.quick_menu.as_ref() else {
            return;
        };

        let s = f.scale.max(0.5) * f.ui_scale;
        let layout = quick_layout(f.width, f.height, s, menu.items.len());
        let QuickLayout {
            cols,
            tile_w,
            tile_h,
            gap,
            pad,
            header_h,
            panel,
        } = layout;
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
            &crate::i18n::translate("quick_menu.title", &[]),
            (13.0 * s) as u32,
            [245, 245, 245, 235],
        );
        title.place(
            scene,
            panel[0] + pad,
            panel[1] + (header_h - title.h as f32) * 0.5,
        );

        for (i, item) in menu.items.iter().enumerate() {
            let id = item.id;
            let row = i / cols;
            let col = i % cols;
            let x = panel[0] + gap + col as f32 * (tile_w + gap);
            let y = panel[1] + header_h + row as f32 * (tile_h + gap);
            let rect = [x, y, x + tile_w, y + tile_h];
            self.quick_rects.push((id, rect));
            let disabled = item.disabled;
            let hit = f.cursor.0 >= x
                && f.cursor.0 <= rect[2]
                && f.cursor.1 >= y
                && f.cursor.1 <= rect[3];
            let ending = menu.end_duty && id == "duty";
            let active = id == "arrows" && menu.arrows_on;
            let selected = menu.selected == Some(id);
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
            } else if selected {
                [67, 91, 112, 250]
            } else if hit {
                [78, 84, 92, 250]
            } else {
                [40, 44, 50, 240]
            };
            self.text.rounded(r, scene, rect, 5.0 * s, fill);
            let key = if ending {
                "quick_menu.end_timetable"
            } else {
                item.label
            };
            let color = if disabled {
                [160, 164, 170, 120]
            } else if active {
                [24, 20, 16, 255]
            } else {
                [245, 245, 245, 245]
            };
            let max_px = (tile_w - 10.0 * s).max(1.0);
            let font_size = quick_label_size(s, tile_w, tile_h);
            let label = crate::i18n::translate(key, &[]);
            let label = clip_to(&self.text, &label, font_size, max_px);
            let line = self.text.label(r, scene, &label, font_size as u32, color);
            line.place(
                scene,
                rect[0] + (tile_w - line.w as f32) * 0.5,
                rect[1] + (tile_h - line.h as f32) * 0.5,
            );
        }

        if menu.confirm_end_duty {
            let veil = self.text.plate(r, scene, 6);
            scene.overlays.push((veil, [0.0, 0.0, f.width, f.height]));
            let QuickConfirmLayout {
                scale: s,
                panel: rect,
                yes,
                no,
            } = quick_confirm_layout(f.width, f.height, s);
            let [x, y, _, _] = rect;
            let w = rect[2] - rect[0];
            let h = rect[3] - rect[1];
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
                &crate::i18n::translate("quick_menu.confirm_title", &[]),
                (17.0 * s) as u32,
                [255, 255, 255, 255],
            );
            title.place(scene, x + (w - title.w as f32) * 0.5, y + 24.0 * s);
            let body = self.text.label(
                r,
                scene,
                &crate::i18n::translate("quick_menu.confirm_body", &[]),
                (13.0 * s) as u32,
                [200, 205, 212, 230],
            );
            body.place(scene, x + (w - body.w as f32) * 0.5, y + 57.0 * s);
            let bw = yes[2] - yes[0];
            let bh = yes[3] - yes[1];
            self.quick_confirm_rects
                .push((QuickConfirmAction::EndDuty, yes));
            self.quick_confirm_rects
                .push((QuickConfirmAction::KeepDriving, no));
            self.text
                .rounded(r, scene, yes, 5.0 * s, [168, 50, 44, 250]);
            self.text.rounded(r, scene, no, 5.0 * s, [57, 62, 70, 250]);
            let yes_label = self.text.label(
                r,
                scene,
                &crate::i18n::translate("quick_menu.end_timetable", &[]),
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
                &crate::i18n::translate("quick_menu.keep_driving", &[]),
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn quick_menu_layout_fits_small_windows_and_ui_scaling() {
        let small = quick_layout(320.0, 240.0, 2.0, 12);
        assert!(small.panel[0] >= 0.0 && small.panel[1] >= 0.0);
        assert!(small.panel[2] <= 320.0 && small.panel[3] <= 240.0);
        let label_size = quick_label_size(2.0, small.tile_w, small.tile_h);
        assert!((10.0..=12.5).contains(&label_size));

        let base = quick_layout(1200.0, 800.0, 1.0, 12);
        let scaled = quick_layout(1200.0, 800.0, 1.5, 12);
        assert!(scaled.tile_w > base.tile_w);
        assert!(scaled.tile_h > base.tile_h);
    }

    #[test]
    fn quick_confirmation_fits_a_scaled_small_window() {
        let dialog = quick_confirm_layout(320.0, 240.0, 2.0);
        assert!(dialog.panel[0] >= 0.0 && dialog.panel[1] >= 0.0);
        assert!(dialog.panel[2] <= 320.0 && dialog.panel[3] <= 240.0);
        assert!(dialog.scale < 2.0);
        for button in [dialog.yes, dialog.no] {
            assert!(button[0] >= dialog.panel[0]);
            assert!(button[1] >= dialog.panel[1]);
            assert!(button[2] <= dialog.panel[2]);
            assert!(button[3] <= dialog.panel[3]);
        }
    }
}
