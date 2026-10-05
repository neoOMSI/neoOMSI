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
        self.text.shadow(
            r,
            scene,
            [x, y, x + w, y + h],
            CARD_R * s,
            28.0 * s,
            10.0 * s,
            110,
        );
        self.text.rounded(
            r,
            scene,
            [x - 1.0, y - 1.0, x + w + 1.0, y + h + 1.0],
            CARD_R * s + 1.0,
            BORDER,
        );
        self.text
            .rounded(r, scene, [x, y, x + w, y + h], CARD_R * s, PANEL);
        let left = x + 20.0 * s;
        let right = x + w - 24.0 * s;
        let available = right - left;
        let compact = w < 1000.0 * s;
        let title = if report.completed {
            "Trip complete"
        } else {
            "Current trip"
        };
        self.report_label(
            r,
            scene,
            title,
            [left, y + 12.0 * s, right, y + 30.0 * s],
            (11.0 * s) as u32,
            AMBER,
        );
        self.report_label(
            r,
            scene,
            "Trip evaluation",
            [left, y + 29.0 * s, right, y + 56.0 * s],
            (25.0 * s) as u32,
            WHITE,
        );
        self.report_label(
            r,
            scene,
            &report.caption,
            [left, y + 61.0 * s, right, y + 82.0 * s],
            (14.0 * s) as u32,
            SOFT,
        );
        self.report_label(
            r,
            scene,
            &report.context,
            [left, y + 83.0 * s, right, y + 100.0 * s],
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
        self.text.rounded(
            r,
            scene,
            [left, header - 4.0 * s, right, body - 3.0 * s],
            ROW_R * s,
            PANEL_ALT,
        );
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
                self.report_label(
                    r,
                    scene,
                    label,
                    [cx, header + 3.0 * s, cx + narrow_step - 4.0 * s, body],
                    (11.0 * s) as u32,
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
                self.report_label(
                    r,
                    scene,
                    label,
                    [
                        columns[start] + 6.0 * s,
                        header,
                        columns[end] - 6.0 * s,
                        body,
                    ],
                    (12.0 * s) as u32,
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
                self.report_label(
                    r,
                    scene,
                    label,
                    [
                        columns[i + 1] + 6.0 * s,
                        header + 22.0 * s,
                        columns[i + 2] - 6.0 * s,
                        body,
                    ],
                    (10.0 * s) as u32,
                    MUTED,
                );
            }
        }
        for index in first..(first + rows).min(count) {
            let top = body + (index - first) as f32 * row_h;
            if (index - first) % 2 == 0 {
                self.text.rounded(
                    r,
                    scene,
                    [left, top, right, top + row_h - 2.0 * s],
                    ROW_R * s,
                    [255, 255, 255, 5],
                );
            }
            let cells = report.rows[index].cells.clone();
            let status = report.rows[index].status;
            let status_color = match status {
                "Late" | "Late / too early" => txt(DANGER),
                "Too early" => AMBER,
                "On time" => [148, 210, 164, 0],
                _ => MUTED,
            };
            if compact {
                self.report_label(
                    r,
                    scene,
                    &cells[0],
                    [
                        left + 6.0 * s,
                        top + 5.0 * s,
                        right - 126.0 * s,
                        top + 25.0 * s,
                    ],
                    (14.0 * s) as u32,
                    WHITE,
                );
                self.report_label(
                    r,
                    scene,
                    &cells[7],
                    [
                        right - 122.0 * s,
                        top + 7.0 * s,
                        right - 5.0 * s,
                        top + 25.0 * s,
                    ],
                    (11.0 * s) as u32,
                    status_color,
                );
                for (label, base, dy) in [("Arrival", 1, 32.0), ("Departure", 4, 68.0)] {
                    self.report_label(
                        r,
                        scene,
                        label,
                        [
                            left + 6.0 * s,
                            top + dy * s,
                            narrow_name - 4.0 * s,
                            top + row_h,
                        ],
                        (11.0 * s) as u32,
                        MUTED,
                    );
                    for i in 0..3 {
                        let cx = narrow_name + i as f32 * narrow_step;
                        self.report_time(
                            r,
                            scene,
                            &cells[base + i],
                            [cx, top + dy * s, cx + narrow_step - 4.0 * s, top + row_h],
                            s,
                            SOFT,
                        );
                    }
                }
            } else {
                for i in 0..8 {
                    let rect = [
                        columns[i] + 6.0 * s,
                        top + 7.0 * s,
                        columns[i + 1] - 5.0 * s,
                        top + row_h,
                    ];
                    let ink = if i == 7 {
                        status_color
                    } else if i == 0 {
                        WHITE
                    } else {
                        SOFT
                    };
                    if matches!(i, 1 | 2 | 4 | 5) {
                        self.report_time(r, scene, &cells[i], rect, s, ink);
                    } else {
                        self.report_label(
                            r,
                            scene,
                            &cells[i],
                            rect,
                            (if i == 7 { 12.0 } else { 14.0 } * s) as u32,
                            ink,
                        );
                    }
                }
            }
        }
        if count > rows {
            let track = [
                x + w - 13.0 * s,
                body,
                x + w - 9.0 * s,
                body + rows as f32 * row_h,
            ];
            self.menu_scroll_track = Some(track);
            self.text.rounded(r, scene, track, 2.0 * s, CHIP);
            let th = track[3] - track[1];
            let thumb = [
                track[0],
                track[1] + th * first as f32 / count as f32,
                track[2],
                track[1] + th * (first + rows) as f32 / count as f32,
            ];
            self.text.rounded(r, scene, thumb, 2.0 * s, ACCENT);
            self.menu_scroll_thumb =
                Some([thumb[0] - 5.0 * s, thumb[1], thumb[2] + 5.0 * s, thumb[3]]);
        }
        self.text.rounded(
            r,
            scene,
            [left, footer - 2.0 * s, right, footer - 1.0 * s],
            0.0,
            BORDER,
        );
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
            let fill = if i == 1 {
                if focus { ACCENT_HOT } else { ACCENT }
            } else if focus {
                SELECTED
            } else {
                PANEL_ALT
            };
            self.text.rounded(r, scene, rect, ROW_R * s, fill);
            let px = (14.0 * s) as u32;
            let label = clip_to(&self.text, &crate::tr(label), px as f32, bw - 16.0 * s);
            let l = self
                .text
                .label(r, scene, &label, px, if i == 1 { ON_ACCENT } else { WHITE });
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
