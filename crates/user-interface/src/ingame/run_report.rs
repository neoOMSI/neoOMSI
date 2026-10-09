//! The timetable evaluation card uses the game's menu input, scrolling and VR surface.

use super::*;

impl Ui {
    fn report_label(
        &mut self,
        r: &Renderer,
        scene: &mut Scene,
        text: &str,
        rect: [f32; 4],
        px: u32,
        color: [u8; 4],
    ) {
        let text = clip_to(&self.text, &crate::tr(text), px as f32, rect[2] - rect[0]);
        let l = self.text.label(r, scene, &text, px, color);
        scene.overlays.push((
            l.tex,
            [rect[0], rect[1], rect[0] + l.w as f32, rect[1] + l.h as f32],
        ));
    }

    fn report_time(
        &mut self,
        r: &Renderer,
        scene: &mut Scene,
        text: &str,
        rect: [f32; 4],
        s: f32,
        color: [u8; 4],
    ) {
        let (clock, day) = text.split_once(" (").unwrap_or((text, ""));
        self.report_label(r, scene, clock, rect, (14.0 * s) as u32, color);
        if !day.is_empty() {
            self.report_label(
                r,
                scene,
                &format!("({day}"),
                [rect[0], rect[1] + 18.0 * s, rect[2], rect[3]],
                (10.0 * s) as u32,
                MUTED,
            );
        }
    }

    fn report_caps(
        &mut self,
        r: &Renderer,
        scene: &mut Scene,
        text: &str,
        rect: [f32; 4],
        px: u32,
        color: [u8; 4],
    ) {
        let up = crate::tr(text).to_uppercase();
        let up = clip_to(&self.text, &up, px as f32, rect[2] - rect[0]);
        let l = self.text.label(r, scene, &up, px, color);
        scene.overlays.push((
            l.tex,
            [rect[0], rect[1], rect[0] + l.w as f32, rect[1] + l.h as f32],
        ));
    }

    fn report_chip(
        &mut self,
        r: &Renderer,
        scene: &mut Scene,
        text: &str,
        status: &str,
        x: f32,
        right_aligned: bool,
        cy: f32,
        s: f32,
    ) {
        let (ink, fill) = match status {
            "Late" | "Late / too early" => (mix(txt(DANGER), [255, 176, 166, 0], 0.4), fade(DANGER, 0.20)),
            "Too early" => (AMBER, ACCENT_SOFT),
            "On time" => ([148, 210, 164, 0], [90, 190, 120, 40]),
            _ => (MUTED, CHIP),
        };
        let px = (12.0 * s) as u32;
        let text = clip_to(&self.text, text, px as f32, 150.0 * s);
        let tw = self.text.width(&text, px as f32);
        let (cw, ch) = (tw + 20.0 * s, 24.0 * s);
        let x0 = if right_aligned { x - cw } else { x };
        self.text.rounded(
            r,
            scene,
            [x0, cy - ch * 0.5, x0 + cw, cy + ch * 0.5],
            ch * 0.5,
            fill,
        );
        self.put(r, scene, &text, px, ink, x0 + 10.0 * s, cy);
    }

    pub(super) fn draw_report_screen(&mut self, r: &Renderer, scene: &mut Scene, f: &Frame) {
        self.menu_rects.clear();
        self.menu_ctl.clear();
        self.menu_side.clear();
        self.menu_pane.clear();
        self.menu_pane_start = 0;
        self.menu_pane_go = None;
        self.menu_pane_box = None;
        self.menu_time.clear();
        self.menu_scroll_thumb = None;
        self.menu_scroll_track = None;
        self.dd_rects.clear();
        let overlay_start = scene.overlays.len();
        match (f.menu, f.report) {
            (Some((sel, _)), Some(report)) => self.draw_run_report(r, scene, f, report, sel),
            _ => {
                if f.lab.is_none() {
                    self.anim.clear();
                }
            }
        }
        self.menu_overlay_range = overlay_start..scene.overlays.len();
    }

    pub(super) fn draw_run_report(
        &mut self,
        r: &Renderer,
        scene: &mut Scene,
        f: &Frame,
        report: &RunReportView,
        selected: usize,
    ) {
        let s = (f.scale.max(0.5) * f.ui_scale)
            .min(f.width / 360.0)
            .min(f.height / 400.0)
            .max(0.5);
        let w = (1140.0 * s).min(f.width - 24.0 * s);
        let h = (760.0 * s).min(f.height - 24.0 * s);
        let x = ((f.width - w) * 0.5).round();
        let y = ((f.height - h) * 0.5).round();
        let dim = self.text.plate(r, scene, 6);
        scene.overlays.push((dim, [0.0, 0.0, f.width, f.height]));
        let radius = CARD_R * s;
        self.text.shadow(
            r,
            scene,
            [x, y, x + w, y + h],
            radius,
            28.0 * s,
            10.0 * s,
            110,
        );
        self.text.rounded(
            r,
            scene,
            [x - 1.0, y - 1.0, x + w + 1.0, y + h + 1.0],
            radius + 1.0,
            BORDER,
        );
        self.text
            .rounded(r, scene, [x, y, x + w, y + h], radius, PANEL);
        let pad = PAD * s;
        let tin = TEXT_IN * s;
        let left = x + pad + tin;
        let right = x + w - pad - tin;
        let lrow = x + pad;
        let available = right - left;
        let compact = w < 1000.0 * s;
        let eyebrow = if report.completed {
            "Trip complete"
        } else {
            "Current trip"
        };
        self.report_caps(
            r,
            scene,
            eyebrow,
            [left, y + 14.0 * s, right, y + 30.0 * s],
            (12.0 * s) as u32,
            txt(ACCENT),
        );
        self.report_label(
            r,
            scene,
            "Trip evaluation",
            [left, y + 29.0 * s, right, y + 58.0 * s],
            (24.0 * s) as u32,
            WHITE,
        );
        self.report_label(
            r,
            scene,
            &report.caption,
            [left, y + 64.0 * s, right, y + 84.0 * s],
            (14.0 * s) as u32,
            SOFT,
        );
        self.report_label(
            r,
            scene,
            &report.context,
            [left, y + 85.0 * s, right, y + 101.0 * s],
            (11.0 * s) as u32,
            MUTED,
        );
        let header = y + 111.0 * s;
        let body = header + if compact { 31.0 * s } else { 48.0 * s };
        let footer = y + h - 88.0 * s;
        let row_h = if compact { 109.0 * s } else { 46.0 * s };
        let rows = (((footer - body) / row_h).floor() as usize).max(1);
        let count = report.rows.len();
        let first =
            (f.menu_top.unwrap_or(0.0).round().max(0.0) as usize).min(count.saturating_sub(rows));
        self.menu_start = 0;
        self.menu_rows = rows;
        self.menu_row_h = row_h;
        let scrolls = count > rows;
        let right_row = x + w - pad - if scrolls { 12.0 * s } else { 0.0 };
        let widths = [0.28, 0.09, 0.09, 0.09, 0.09, 0.09, 0.09, 0.18];
        let mut columns = [left; 9];
        for i in 0..8 {
            columns[i + 1] = columns[i] + available * widths[i];
        }
        let narrow_name = left + 78.0 * s;
        let narrow_step = (available - 78.0 * s) / 3.0;
        if compact {
            for (i, label) in ["Planned", "Actual", "Difference"].iter().enumerate() {
                let cx = narrow_name + i as f32 * narrow_step;
                self.report_caps(
                    r,
                    scene,
                    label,
                    [cx, header + 8.0 * s, cx + narrow_step - 4.0 * s, body],
                    (10.0 * s) as u32,
                    MUTED,
                );
            }
        } else {
            for (label, start, end) in [
                ("Bus stop", 0, 1),
                ("Arrival", 1, 4),
                ("Departure", 4, 7),
                ("Status", 7, 8),
            ] {
                self.report_caps(
                    r,
                    scene,
                    label,
                    [
                        columns[start],
                        header,
                        columns[end] - 6.0 * s,
                        body,
                    ],
                    (11.0 * s) as u32,
                    SOFT,
                );
            }
            for (i, label) in [
                "Planned",
                "Actual",
                "Difference",
                "Planned",
                "Actual",
                "Difference",
            ]
                .iter()
                .enumerate()
            {
                self.report_caps(
                    r,
                    scene,
                    label,
                    [
                        columns[i + 1],
                        header + 20.0 * s,
                        columns[i + 2] - 6.0 * s,
                        body,
                    ],
                    (9.0 * s) as u32,
                    MUTED,
                );
            }
        }
        let sep = self.text.plate(r, scene, 9);
        let sy = (body - 4.0 * s).round();
        scene.overlays.push((sep, [left, sy, right, sy + 1.0]));
        for index in first..(first + rows).min(count) {
            let top = body + (index - first) as f32 * row_h;
            let rect = [lrow, top, right_row, top + row_h - 4.0 * s];
            let cy = (rect[1] + rect[3]) * 0.5;
            let hover = f.cursor.0 >= rect[0]
                && f.cursor.0 <= rect[2]
                && f.cursor.1 >= rect[1]
                && f.cursor.1 <= rect[3];
            let glow = self.easeq(
                (21, "report", index),
                if hover { 1.0 } else { 0.0 },
                1.0 / FADE_SECS,
            );
            self.text.rounded(r, scene, rect, ROW_R * s, PANEL_ALT);
            if glow > 0.0 {
                self.text
                    .rounded(r, scene, rect, ROW_R * s, fade(LIT, glow));
                self.accent_bar(r, scene, rect, glow, false, s);
            }
            let cells = report.rows[index].cells.clone();
            let status = report.rows[index].status;
            let diff_ink = |c: &str| {
                if c.starts_with('+') {
                    [232, 138, 128, 0]
                } else if c.starts_with('-') || c.starts_with('\u{2212}') {
                    AMBER
                } else {
                    SOFT
                }
            };
            if compact {
                self.report_label(
                    r,
                    scene,
                    &cells[0],
                    [
                        left,
                        top + 5.0 * s,
                        right_row - 140.0 * s,
                        top + 25.0 * s,
                    ],
                    (14.0 * s) as u32,
                    WHITE,
                );
                self.report_chip(
                    r,
                    scene,
                    &cells[7],
                    status,
                    right_row - tin,
                    true,
                    top + 16.0 * s,
                    s,
                );
                for (label, base, dy) in [("Arrival", 1, 32.0), ("Departure", 4, 68.0)] {
                    self.report_label(
                        r,
                        scene,
                        label,
                        [left, top + dy * s, narrow_name - 4.0 * s, top + row_h],
                        (11.0 * s) as u32,
                        MUTED,
                    );
                    for i in 0..3 {
                        let cx = narrow_name + i as f32 * narrow_step;
                        let ink = if i == 2 { diff_ink(&cells[base + i]) } else { SOFT };
                        self.report_time(
                            r,
                            scene,
                            &cells[base + i],
                            [cx, top + dy * s, cx + narrow_step - 4.0 * s, top + row_h],
                            s,
                            ink,
                        );
                    }
                }
            } else {
                for i in 0..7 {
                    let rect = [
                        columns[i],
                        top + 7.0 * s,
                        columns[i + 1] - 5.0 * s,
                        top + row_h,
                    ];
                    if i == 0 {
                        self.report_label(r, scene, &cells[0], rect, (14.0 * s) as u32, WHITE);
                    } else {
                        let ink = if matches!(i, 3 | 6) { diff_ink(&cells[i]) } else { SOFT };
                        self.report_time(r, scene, &cells[i], rect, s, ink);
                    }
                }
                self.report_chip(r, scene, &cells[7], status, columns[7], false, cy, s);
            }
        }
        if scrolls {
            let track = [
                x + w - 13.0 * s,
                body,
                x + w - 10.0 * s,
                body + rows as f32 * row_h - 4.0 * s,
            ];
            self.menu_scroll_track = Some(track);
            self.text
                .rounded(r, scene, track, 1.5 * s, [255, 255, 255, 22]);
            let th = track[3] - track[1];
            let thumb = [
                track[0],
                track[1] + th * first as f32 / count as f32,
                track[2],
                track[1] + th * (first + rows) as f32 / count as f32,
            ];
            self.text.rounded(r, scene, thumb, 1.5 * s, ACCENT);
            self.menu_scroll_thumb =
                Some([thumb[0] - 6.0 * s, thumb[1], thumb[2] + 6.0 * s, thumb[3]]);
        }
        let sep = self.text.plate(r, scene, 9);
        scene
            .overlays
            .push((sep, [left, footer - 2.0 * s, right, footer - 1.0 * s]));
        let message = if f.report_status.is_empty() {
            "Times: HH:MM:SS · differences in seconds"
        } else {
            f.report_status
        };
        self.report_label(
            r,
            scene,
            message,
            [left, footer + 5.0 * s, right, footer + 21.0 * s],
            (11.0 * s) as u32,
            MUTED,
        );
        let gap = 12.0 * s;
        let bw = (available - gap) * 0.5;
        for (i, label) in ["Save as text…", "Continue"].iter().enumerate() {
            let bx = left + i as f32 * (bw + gap);
            let rect = [bx, footer + 27.0 * s, bx + bw, footer + 66.0 * s];
            self.menu_rects.push(rect);
            let hover = f.cursor.0 >= rect[0]
                && f.cursor.0 <= rect[2]
                && f.cursor.1 >= rect[1]
                && f.cursor.1 <= rect[3];
            let focus = hover || (f.menu_kbd && selected == i);
            let glow = self.easeq(
                (22, "report_btn", i),
                if focus { 1.0 } else { 0.0 },
                1.0 / FADE_SECS,
            );
            let fill = if i == 1 {
                mix(ACCENT, ACCENT_HOT, glow)
            } else {
                mix(PANEL_ALT, [58, 58, 58, 255], glow)
            };
            self.text.rounded(r, scene, rect, ROW_R * s, fill);
            let px = (14.0 * s) as u32;
            let text: &str = label;
            let label = clip_to(&self.text, &crate::tr(text), px as f32, bw - 16.0 * s);
            let ink = if i == 1 { ON_ACCENT } else { mix(SOFT, WHITE, glow) };
            let l = self.text.label(r, scene, &label, px, ink);
            let tx = bx + (bw - l.w as f32) * 0.5;
            let ty = rect[1] + (rect[3] - rect[1] - l.h as f32) * 0.5;
            l.place(scene, tx, ty);
        }
        if !f.vr && !f.touch {
            self.report_label(
                r,
                scene,
                "↑ ↓ / Page Up / Page Down scroll · Ctrl+S save · Esc continue",
                [left, footer + 70.0 * s, right, y + h],
                (10.0 * s) as u32,
                MUTED,
            );
        }
    }
}