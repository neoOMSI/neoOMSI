//! Per-trip timetable observations, retained independently of the active duty.
//! Times use the duty clock (seconds, including its midnight offset), not wall time.

use crate::schedule::PlannedTrip;
use std::fmt::Write;

#[derive(Clone, Copy, Default)]
pub(crate) struct ActualStop {
    pub arrival: Option<f64>,
    pub departure: Option<f64>,
    pub skipped: bool,
}

#[derive(Default)]
pub(crate) struct TripLog {
    stops: Vec<ActualStop>,
}

impl TripLog {
    pub fn arrive(&mut self, index: usize, time: f64) {
        self.stops
            .resize(self.stops.len().max(index + 1), ActualStop::default());
        let stop = &mut self.stops[index];
        stop.arrival.get_or_insert(time);
        stop.skipped = false;
    }

    pub fn depart(&mut self, index: usize, time: f64) {
        if let Some(stop) = self.stops.get_mut(index).filter(|s| s.arrival.is_some()) {
            stop.departure.get_or_insert(time);
        }
    }

    pub fn skip(&mut self, from: usize, to: usize) {
        self.stops
            .resize(self.stops.len().max(to), ActualStop::default());
        for stop in &mut self.stops[from..to] {
            if stop.arrival.is_none() {
                stop.skipped = true;
            }
        }
    }

    pub fn report(&self, trip: &PlannedTrip, tour: &str, completed: bool) -> Report {
        let mut actual = self.stops.clone();
        actual.resize(trip.stops.len(), ActualStop::default());
        Report {
            trip: trip.clone(),
            tour: tour.into(),
            actual,
            completed,
            map: String::new(),
            date: 0,
        }
    }
}

#[derive(Clone)]
pub(crate) struct Report {
    pub trip: PlannedTrip,
    pub tour: String,
    pub actual: Vec<ActualStop>,
    pub completed: bool,
    pub map: String,
    pub date: i32,
}

/// Clock time to the second. Explicit day offsets keep a midnight crossing unambiguous.
pub(crate) fn clock_time(time: f64) -> String {
    let seconds = time.floor() as i64;
    let day = seconds.div_euclid(86400);
    let t = seconds.rem_euclid(86400);
    let clock = format!("{:02}:{:02}:{:02}", t / 3600, t / 60 % 60, t % 60);
    if day == 0 {
        clock
    } else {
        format!("{clock} ({day:+}d)")
    }
}

pub(crate) fn difference(actual: Option<f64>, planned: f64) -> String {
    actual
        .map(|a| format!("{:+.0} s", a.floor() - planned.floor()))
        .unwrap_or_else(|| "—".into())
}

/// The header date is the calendar day that the timetable's day-zero refers to.
fn report_date(clock: &::simulation::SimClock, trip: &PlannedTrip) -> i32 {
    let centre = (trip.departure + trip.end) * 0.5;
    let time = [clock.time - 86400.0, clock.time, clock.time + 86400.0]
        .into_iter()
        .min_by(|a, b| (a - centre).abs().total_cmp(&(b - centre).abs()))
        .unwrap_or(clock.time);
    let mut date = clock.clone();
    date.day_of_year -= (time / 86400.0).floor() as i32;
    while date.day_of_year <= 0 {
        date.year -= 1;
        date.day_of_year += ::simulation::clock::days_in_year(date.year);
    }
    while date.day_of_year > ::simulation::clock::days_in_year(date.year) {
        date.day_of_year -= ::simulation::clock::days_in_year(date.year);
        date.year += 1;
    }
    date.date_code()
}

impl Report {
    pub fn status(&self, index: usize) -> &'static str {
        let actual = self.actual[index];
        let planned = &self.trip.stops[index];
        if actual.skipped {
            return "Skipped";
        }
        if actual.arrival.is_none() {
            return "Not recorded";
        }
        if !planned.stops {
            return "Passing stop";
        }
        let late = actual.arrival.is_some_and(|a| a - planned.arr > 180.0);
        let early = actual.departure.is_some_and(|d| d - planned.dep < -120.0);
        match (late, early) {
            (true, true) => "Late / too early",
            (true, false) => "Late",
            (false, true) => "Too early",
            _ if actual.departure.is_none() && index + 1 < self.trip.stops.len() => {
                if self.completed || self.actual[index + 1..].iter().any(|s| s.arrival.is_some()) {
                    "Incomplete"
                } else {
                    "At stop"
                }
            }
            _ => "On time",
        }
    }

    pub fn cells(&self, index: usize) -> [String; 8] {
        let stop = &self.trip.stops[index];
        let actual = self.actual[index];
        let observed = |t: Option<f64>| t.map(clock_time).unwrap_or_else(|| "—".into());
        [
            stop.name.replace(['\t', '\n', '\r'], " "),
            clock_time(stop.arr),
            observed(actual.arrival),
            difference(actual.arrival, stop.arr),
            clock_time(stop.dep),
            observed(actual.departure),
            difference(actual.departure, stop.dep),
            ::user_interface::tr(self.status(index)).into_owned(),
        ]
    }

    pub fn view(&self) -> ::user_interface::ingame::RunReportView {
        ::user_interface::ingame::RunReportView {
            completed: self.completed,
            caption: self.caption(),
            context: self.context(),
            rows: (0..self.trip.stops.len())
                .map(|i| ::user_interface::ingame::RunReportRow {
                    cells: self.cells(i),
                    status: self.status(i),
                })
                .collect(),
        }
    }

    pub fn caption(&self) -> String {
        format!(
            "{} {} · {} {} · {}",
            ::user_interface::tr("Line"),
            self.trip.line,
            ::user_interface::tr("Tour"),
            self.tour,
            self.trip.terminus
        )
    }

    pub fn context(&self) -> String {
        format!(
            "{} · {:04}-{:02}-{:02} · {}",
            self.map,
            self.date / 10000,
            self.date / 100 % 100,
            self.date % 100,
            self.trip.name
        )
    }

    pub fn text(&self) -> String {
        let mut text = format!(
            "{}\n{}\n{}\n\n{}\n\n",
            ::user_interface::tr("Trip evaluation"),
            self.caption(),
            self.context(),
            ::user_interface::tr("Times: HH:MM:SS · differences in seconds")
        );
        let headings = [
            "Bus stop",
            "Arrival planned",
            "Arrival actual",
            "Arrival difference",
            "Departure planned",
            "Departure actual",
            "Departure difference",
            "Status",
        ];
        let mut rows = vec![headings.map(|h| ::user_interface::tr(h).into_owned())];
        rows.extend((0..self.trip.stops.len()).map(|i| self.cells(i)));
        let widths: [usize; 8] =
            std::array::from_fn(|i| rows.iter().map(|r| r[i].chars().count()).max().unwrap_or(0));
        for row in rows {
            for (i, cell) in row.iter().enumerate() {
                let _ = write!(
                    text,
                    "{cell:<width$}{}",
                    if i == 7 { "\n" } else { "  " },
                    width = widths[i]
                );
            }
        }
        text.push_str("\n");
        text.push_str(&::user_interface::tr("Positive difference = late; negative difference = early. Missing observations are shown as —."));
        text.push('\n');
        text
    }

    pub fn filename(&self) -> String {
        let name = format!(
            "{}-{}-{}-{}",
            self.map, self.trip.line, self.date, self.trip.name
        );
        let safe: String = name
            .chars()
            .map(|c| {
                if c.is_control() || "<>:\"/\\|?*".contains(c) {
                    '_'
                } else {
                    c
                }
            })
            .collect();
        format!("{}.txt", safe.trim_end_matches(['.', ' ']))
    }
}

impl crate::App {
    pub(crate) fn open_run_report(&mut self, current: bool) {
        let report = if current {
            self.duty
                .as_ref()
                .map(|d| d.statistics.report(d.trip(), &d.tour, d.trip_done()))
        } else {
            self.last_report.clone()
        };
        let Some(mut report) = report else { return };
        if current {
            self.report_context(&mut report);
        }
        if report.completed {
            self.report_pending = false;
        }
        if self.game_menu.is_none() {
            self.open_game_menu();
        }
        self.close_list();
        self.report_view = Some(report);
        self.report_status.clear();
        self.menu_top = Some(0.0);
        self.game_menu = Some(1);
        self.mouse_look = false;
        self.both_drag = None;
    }

    pub(crate) fn report_context(&self, report: &mut Report) {
        report.date = report_date(&self.clock, &report.trip);
        report.map = self
            .world
            .as_ref()
            .and_then(|w| w.map_dir.file_name())
            .map(|n| n.to_string_lossy().into_owned())
            .unwrap_or_default();
    }

    pub(crate) fn maybe_open_run_report(&mut self) {
        if !self.report_pending || self.game_menu.is_some() || self.paused {
            return;
        }
        let (Some(report), Some(player)) = (&self.last_report, &self.player) else {
            return;
        };
        if report
            .trip
            .stops
            .last()
            .and_then(|s| s.position)
            .is_some_and(|p| (p - player.vehicle.position).length() > 45.0)
        {
            self.report_pending = false;
            return;
        }
        let bus = &player.vehicle;
        let ready = bus.physics.velocity_kmh().abs() < 1.0
            && (report.trip.line.trim().is_empty()
                || bus.ty.def.passenger_cabin.is_none()
                || crate::humans::Humans::any_door_open(bus));
        if ready {
            self.report_pending = false;
            self.open_run_report(false);
        }
    }

    pub(crate) fn save_run_report(&mut self) {
        if self.report_save_rx.is_some() {
            return;
        }
        let Some(report) = self.report_view.as_ref() else {
            return;
        };
        let filename = report.filename();
        let text = report.text();
        #[cfg(not(target_os = "android"))]
        let title = ::user_interface::tr("Save trip evaluation").into_owned();
        #[cfg(target_os = "android")]
        let content_dir = crate::startup::content_dir();
        let (tx, rx) = std::sync::mpsc::channel();
        self.report_save_rx = Some(rx);
        self.report_status = ::user_interface::tr("Saving trip evaluation…").into_owned();
        // The dialog and file write must not suspend the simulation/network event loop.
        std::thread::spawn(move || {
            #[cfg(not(target_os = "android"))]
            let path = pollster::block_on(
                rfd::AsyncFileDialog::new()
                    .set_title(title)
                    .add_filter("Text", &["txt"])
                    .set_file_name(filename)
                    .save_file(),
            )
            .map(|file| file.path().to_path_buf());
            #[cfg(target_os = "android")]
            let path = content_dir.map(|p| p.join("Reports").join(filename));
            let result = (|| -> std::io::Result<Option<std::path::PathBuf>> {
                let Some(path) = path else { return Ok(None) };
                if let Some(parent) = path.parent() {
                    std::fs::create_dir_all(parent)?;
                }
                std::fs::write(&path, text)?;
                Ok(Some(path))
            })();
            let _ = tx.send(result);
        });
    }

    pub(crate) fn tick_run_report_save(&mut self) {
        use std::sync::mpsc::TryRecvError;
        let Some(rx) = self.report_save_rx.as_ref() else {
            return;
        };
        let result = match rx.try_recv() {
            Ok(result) => result,
            Err(TryRecvError::Empty) => return,
            Err(TryRecvError::Disconnected) => Err(std::io::Error::other("Export worker stopped")),
        };
        self.report_save_rx = None;
        self.report_status = match result {
            Ok(Some(path)) => format!("{}: {}", ::user_interface::tr("Saved"), path.display()),
            Ok(None) => String::new(),
            Err(e) => format!("{}: {e}", ::user_interface::tr("Could not save trip evaluation")),
        };
    }

    pub(crate) fn run_report_key(
        &mut self,
        event_loop: &winit::event_loop::ActiveEventLoop,
        code: winit::keyboard::KeyCode,
    ) {
        use winit::keyboard::KeyCode::*;
        let ctrl = self.keys.contains(&ControlLeft) || self.keys.contains(&ControlRight);
        match code {
            Escape => self.close_game_menu(),
            KeyS if ctrl => self.save_run_report(),
            ArrowUp => self.menu_wheel(1.0),
            ArrowDown => self.menu_wheel(-1.0),
            PageUp => self.menu_wheel(self.ui.as_ref().map_or(8, |u| u.menu_rows) as f32),
            PageDown => self.menu_wheel(-(self.ui.as_ref().map_or(8, |u| u.menu_rows) as f32)),
            Home => self.menu_top = Some(0.0),
            End => {
                self.menu_top = Some(
                    self.menu_len()
                        .saturating_sub(self.ui.as_ref().map_or(0, |u| u.menu_rows))
                        as f32,
                )
            }
            Tab | ArrowLeft | ArrowRight => {
                self.game_menu = Some(1 - self.game_menu.unwrap_or(1).min(1))
            }
            Enter | NumpadEnter | Space => {
                self.menu_choose(event_loop, self.game_menu.unwrap_or(1))
            }
            _ => {}
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::schedule::{PlannedStop, StopDir};
    fn trip() -> PlannedTrip {
        PlannedTrip {
            name: "night".into(),
            line: "76".into(),
            terminus: "End".into(),
            departure: 86300.0,
            end: 86500.0,
            stops: (0..3)
                .map(|i| PlannedStop {
                    name: format!("Stop {i}"),
                    object_id: i,
                    arr: 86300.0 + i as f64 * 100.0,
                    dep: 86310.0 + i as f64 * 100.0,
                    position: None,
                    dir: StopDir::default(),
                    stops: true,
                })
                .collect(),
        }
    }
    #[test]
    fn repeated_observations_keep_first_times_and_snapshot_is_independent() {
        let mut log = TripLog::default();
        log.arrive(0, 86302.0);
        log.arrive(0, 86304.0);
        log.depart(0, 86313.0);
        log.depart(0, 86320.0);
        log.skip(1, 2);
        log.arrive(2, 86504.0);
        let report = log.report(&trip(), "1", true);
        log.arrive(1, 86400.0);
        assert_eq!(report.actual[0].arrival, Some(86302.0));
        assert_eq!(report.actual[0].departure, Some(86313.0));
        assert_eq!(report.status(1), "Skipped");
        assert_eq!(report.actual[2].departure, None);
        assert!(report.text().contains("00:01:44 (+1d)"));
        assert!(report.text().contains("+4 s"));
    }
    #[test]
    fn status_uses_arrival_and_departure_thresholds() {
        let mut report = TripLog::default().report(&trip(), "1", false);
        report.actual[0] = ActualStop {
            arrival: Some(86480.0),
            departure: Some(86190.0),
            skipped: false,
        };
        assert_eq!(report.status(0), "On time");
        report.actual[0].arrival = Some(86480.1);
        assert_eq!(report.status(0), "Late");
        report.actual[0].departure = Some(86189.9);
        assert_eq!(report.status(0), "Late / too early");
    }
    #[test]
    fn a_missing_departure_is_only_at_stop_while_the_stop_is_current() {
        let mut log = TripLog::default();
        log.arrive(0, 86300.0);
        assert_eq!(log.report(&trip(), "1", false).status(0), "At stop");
        log.arrive(2, 86500.0);
        let report = log.report(&trip(), "1", true);
        assert_eq!(report.status(0), "Incomplete");
        assert_eq!(report.status(2), "On time");
    }

    #[test]
    fn midnight_report_date_matches_the_day_offsets() {
        let mut clock = ::simulation::SimClock::default();
        clock.set_date(2026, 1, 1);
        clock.time = 104.0;
        assert_eq!(report_date(&clock, &trip()), 20251231);
        clock.set_date(2024, 3, 1);
        assert_eq!(report_date(&clock, &trip()), 20240229);
        let mut after_midnight = trip();
        after_midnight.departure = 60.0;
        after_midnight.end = 180.0;
        clock.set_date(2025, 12, 31);
        clock.time = 86380.0;
        assert_eq!(report_date(&clock, &after_midnight), 20260101);
    }

    #[test]
    fn clock_preserves_midnight_offsets_and_missing_data() {
        assert_eq!(clock_time(86400.0), "00:00:00 (+1d)");
        assert_eq!(clock_time(-1.0), "23:59:59 (-1d)");
        assert_eq!(difference(None, 60.0), "—");
        assert_eq!(difference(Some(60.9), 60.1), "+0 s");
        assert_eq!(difference(Some(59.0), 60.0), "-1 s");
    }
}

#[cfg(test)]
mod preview {
    use crate::schedule::{PlannedStop, PlannedTrip, StopDir};
    use ::render::Renderer;
    use ::user_interface::ingame::{Frame, MenuKind, Ui};

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
        let camera = ::render::Camera {
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
            let view = report.view();
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
                crosshair: false,
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
                report: Some(&view),
                touch: false,
                build: "",
                report_status: "",
                menu_head: None,
                menu_preview: None,
                pane_first: None,
                menu_tabs: None,
                menu_kbd: true,
                dropdown: None,
                quick_menu: None,
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
                    &::render::Lighting::default(),
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
