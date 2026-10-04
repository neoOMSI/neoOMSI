//! The timetable evaluation card uses the game's menu input, scrolling and VR surface.

use super::*;
use crate::run_statistics::Report;

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
        let text = clip_to(&self.text, &omsi_ui::tr(text), px as f32, rect[2] - rect[0]);
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
        report: &Report,
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
            &report.caption(),
            [left, y + 61.0 * s, right, y + 82.0 * s],
            (14.0 * s) as u32,
            SOFT,
        );
        self.report_label(
            r,
            scene,
            &report.context(),
            [left, y + 83.0 * s, right, y + 100.0 * s],
            (11.0 * s) as u32,
            MUTED,
        );
        let header = y + 111.0 * s;
        let body = header + if compact { 31.0 * s } else { 48.0 * s };
        let footer = y + h - 88.0 * s;
        let row_h = if compact { 109.0 * s } else { 46.0 * s };
        let rows = (((footer - body) / row_h).floor() as usize).max(1);
        let count = report.trip.stops.len();
        let first =
            (f.menu_top.unwrap_or(0.0).round().max(0.0) as usize).min(count.saturating_sub(rows));
        self.menu_start = 0; // the two fixed footer buttons use indices 0 and 1
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
            let cells = report.cells(index);
            let status = report.status(index);
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
            let label = clip_to(&self.text, &omsi_ui::tr(label), px as f32, bw - 16.0 * s);
            let l = self
                .text
                .label(r, scene, &label, px, if i == 1 { ON_ACCENT } else { WHITE });
            let tx = bx + (bw - l.w as f32) * 0.5;
            let ty = rect[1] + (rect[3] - rect[1] - l.h as f32) * 0.5;
            scene
                .overlays
                .push((l.tex, [tx, ty, tx + l.w as f32, ty + l.h as f32]));
        }
        if !f.vr && !crate::platform::touch_controls() {
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::schedule::{PlannedStop, PlannedTrip, StopDir};

    #[test]
    #[ignore = "requires a graphics adapter and OMSI_REPORT_PREVIEW output directory"]
    fn render_report_preview() {
        let out = std::path::PathBuf::from(std::env::var("OMSI_REPORT_PREVIEW").unwrap());
        std::fs::create_dir_all(&out).unwrap();
        let instance = wgpu::Instance::new(wgpu::InstanceDescriptor::new_without_display_handle());
        let mut renderer = pollster::block_on(Renderer::new(
            &instance,
            None,
            Some(wgpu::TextureFormat::Rgba8UnormSrgb),
        ))
        .unwrap();
        let camera = omsi_render::Camera {
            position: glam::DVec3::new(0.0, 0.0, 1.0),
            yaw: 0.0,
            pitch: 0.0,
            roll: 0.0,
            fov_deg: 60.0,
            near: 0.1,
            far: 1000.0,
        };
        let trip = PlannedTrip {
            name: "76_North-Central".into(),
            line: "76".into(),
            terminus: "Central station".into(),
            departure: 8.0 * 3600.0,
            end: 9.0 * 3600.0,
            stops: (0..25)
                .map(|i| PlannedStop {
                    object_id: i,
                    name: match i % 5 {
                        0 => "Northern depot",
                        1 => "Market square",
                        2 => "University / Botanical garden",
                        3 => "West park",
                        _ => "Central station",
                    }
                    .into(),
                    arr: 28800.0 + i as f64 * 150.0,
                    dep: 28830.0 + i as f64 * 150.0,
                    position: None,
                    dir: StopDir::default(),
                    stops: true,
                })
                .collect(),
        };
        let mut log = crate::run_statistics::TripLog::default();
        for i in 0..24 {
            if i == 4 {
                continue;
            }
            let offset = match i % 5 {
                0 => 12.0,
                1 => 210.0,
                2 => -160.0,
                _ => 0.0,
            };
            log.arrive(i, trip.stops[i].arr + offset);
            log.depart(i, trip.stops[i].dep + offset);
        }
        log.skip(4, 5);
        log.arrive(24, trip.stops[24].arr);
        let mut report = log.report(&trip, "1", true);
        report.map = "Demo city".into();
        report.date = 20261004;
        let items = [("report_save", "Save as text…"), ("resume", "Continue")];
        for (name, width, height, language, top) in [
            ("desktop", 1600, 900, "de", 0.0),
            ("desktop-end", 1600, 900, "de", 25.0),
            ("compact", 800, 600, "de", 0.0),
            ("phone", 390, 844, "de", 0.0),
        ] {
            crate::ui_language(language);
            let mut scene = renderer.new_scene();
            let mut ui = Ui::new().unwrap();
            let frame = Frame {
                scale: 1.0,
                ui_scale: 1.0,
                opacity: 1.0,
                width: width as f32,
                height: height as f32,
                cursor: (-100.0, -100.0),
                vr: false,
                tooltip: None,
                chat: None,
                notes: &[],
                fps: None,
                paused: true,
                menu: Some((1, &items)),
                menu_top: Some(top),
                menu_disabled: &[],
                timetable: None,
                info: None,
                tutorial: None,
                tags: vec![],
                menu_kind: MenuKind::Game,
                report: Some(&report),
                report_status: "",
                menu_head: None,
                menu_preview: None,
                pane_first: None,
                menu_tabs: None,
                menu_kbd: true,
                dropdown: None,
            };
            ui.draw(&renderer, &mut scene, &frame, 0.016);
            assert_eq!(ui.menu_rects.len(), 2);
            assert!(ui.menu_scroll_thumb.is_some());
            for rect in &ui.menu_rects {
                assert!(rect[0] >= 0.0 && rect[2] <= width as f32);
                assert!(rect[1] >= 0.0 && rect[3] <= height as f32);
            }
            let rgba = renderer
                .render_to_image(
                    &mut scene,
                    width,
                    height,
                    &camera,
                    &omsi_render::Lighting::default(),
                )
                .unwrap();
            image::save_buffer(
                out.join(format!("{name}.png")),
                &rgba,
                width,
                height,
                image::ColorType::Rgba8,
            )
            .unwrap();
        }
        std::fs::write(out.join("evaluation.txt"), report.text()).unwrap();
    }
}
