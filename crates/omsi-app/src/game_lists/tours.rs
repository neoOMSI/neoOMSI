//! Line and tour lists and starting a duty from them.

use super::*;

pub(crate) fn tour_start(tour: &omsi_timetable::Tour) -> Option<f64> {
    tour.trips
        .iter()
        .map(|t| t.departure as f64 * 60.0)
        .fold(None, |a: Option<f64>, d| Some(a.map_or(d, |x| x.min(d))))
}

/// Whether a tour is listed now: it runs on this day and is current at `now` (seconds of the
/// day) - under way, or leaving within half an hour; one that has finished is not.
pub(super) fn tour_listed(
    sch: &crate::schedule::Schedule,
    line: &str,
    tour: &omsi_timetable::Tour,
    now: f64,
) -> bool {
    if !sch.tour_available(tour) {
        return false;
    }
    let start = tour_start(tour).unwrap_or(0.0);
    let end = sch
        .tour_stops(line, &tour.number)
        .iter()
        .map(|s| s.3)
        .fold(start, f64::max);
    // (a night tour's times go on past 24:00: the early hours of the next day count too)
    [now, now + 86400.0]
        .iter()
        .any(|n| *n >= start - 1800.0 && *n <= end + 60.0)
}

pub(super) fn line_sign(
    schedule: Option<&crate::schedule::Schedule>,
    line: &omsi_timetable::Line,
) -> String {
    let sign = schedule.and_then(|sch| {
        line.tours
            .iter()
            .flat_map(|t| t.trips.iter())
            .find_map(|tt| {
                let t = sch
                    .data
                    .trips
                    .iter()
                    .find(|x| x.name.eq_ignore_ascii_case(&tt.trip))?;
                Some(t.line.trim().to_string()).filter(|n| !n.is_empty())
            })
    });
    sign.unwrap_or_else(|| line.name.clone())
}

/// A line's tours in alphabetical order of their numbers (numbers inside them as numbers:
/// "2" before "10"; equal numbers by the time they start).
pub(super) fn sorted_tours(line: &omsi_timetable::Line) -> Vec<&omsi_timetable::Tour> {
    let mut tours: Vec<&omsi_timetable::Tour> = line.tours.iter().collect();
    tours.sort_by(|a, b| {
        bus_cmp(a.number.trim(), b.number.trim()).then_with(|| {
            let (ta, tb) = (
                tour_start(a).unwrap_or(f64::MAX),
                tour_start(b).unwrap_or(f64::MAX),
            );
            ta.partial_cmp(&tb).unwrap_or(std::cmp::Ordering::Equal)
        })
    });
    tours
}

pub(super) fn tour_trip_name(
    sch: &crate::schedule::Schedule,
    tour: &omsi_timetable::Tour,
    k: usize,
) -> Option<String> {
    tour.trips
        .iter()
        .filter(|tt| {
            sch.data
                .trips
                .iter()
                .any(|x| x.name.eq_ignore_ascii_case(&tt.trip))
        })
        .nth(k)
        .map(|tt| tt.trip.clone())
}

/// Numbers compared as numbers where they are ("5" before "13", "N30" after "M49").
pub(super) fn natural_key(s: &str) -> (u64, String) {
    let digits: String = s.chars().take_while(|c| c.is_ascii_digit()).collect();
    (
        digits.parse::<u64>().unwrap_or(u64::MAX),
        s.to_ascii_lowercase(),
    )
}

pub(super) fn natural(a: &str, b: &str) -> std::cmp::Ordering {
    natural_key(a).cmp(&natural_key(b))
}

pub(crate) fn tour_at(app: &App, k: usize) -> Option<(String, String)> {
    let action = app.admin_list.as_ref()?.get(k)?.1.strip_prefix("tour ")?;
    let (line, tour) = action.split_once('\u{1}')?;
    Some((line.to_string(), tour.to_string()))
}

pub(crate) fn tour_start_of(app: &App, line: &str, tour: &str) -> f64 {
    app.schedule
        .as_ref()
        .and_then(|s| s.data.lines.iter().find(|l| l.name == line))
        .and_then(|l| l.tours.iter().find(|t| t.number == tour))
        .and_then(tour_start)
        .unwrap_or(0.0)
}

pub(crate) fn tour_choice(app: &App, k: usize) -> Option<(usize, usize, usize, usize)> {
    let (line, tour) = tour_at(app, k)?;
    let sch = app.schedule.as_ref()?;
    let trips = sch.tour_trip_count(&line, &tour);
    let (stop, trip) = match app.list_kind.as_ref() {
        Some(ListKind::Tours(_, Some(p))) if p.0 == tour => (p.1, p.2),
        _ => (0, sch.tour_trip_now(&line, &tour, app.clock.time)),
    };
    let trip = trip.min(trips.saturating_sub(1));
    let n = sch.tour_trip_stops(&line, &tour, trip).len();
    (n > 0).then(|| (n, stop.min(n - 1), trip, trips))
}

pub(crate) fn start_duty_at(app: &mut App, line: &str, tour: &str, trip: usize, chosen: usize) {
    let now = app.clock.time;
    let at = tour_start_of(app, line, tour);
    let Some((k, j)) = app.schedule.as_ref().and_then(|s| {
        s.tour_trip_stops(line, tour, trip)
            .get(chosen)
            .map(|x| (x.0, x.1))
    }) else {
        return start_duty(app, line, tour);
    };
    let (Some(w), Some(sch)) = (app.world.clone(), app.schedule.as_mut()) else {
        return;
    };
    let mut d = match sch.player_duty(&w, line, tour, at, None, false) {
        Ok(d) => d,
        Err(e) => {
            app.service_msg = Some((format!("No duty: {e}"), 8.0));
            return;
        }
    };
    // (no teleport: the stop chosen is the one the bus drives to next)
    d.start_at_here(k, j);
    if let Some(p) = app.player.as_mut() {
        d.update(&mut p.vehicle, now);
        let (trip, stop) = d.trip_for_ibis();
        if p.auto_ibis {
            p.set_duty_destination(trip, stop);
        }
        if let Some(w) = app.world.as_ref() {
            let mut fonts = w.fonts.lock();
            if let Err(e) = crate::schedule_paper::update_vehicle(&mut p.vehicle, &d, &mut fonts) {
                log::warn!("driver timetable paper: {e:#}");
            }
        }
    }
    app.args.line = Some(line.to_string());
    app.args.tour = Some(tour.to_string());
    app.duty = Some(d);
    app.service_msg = Some((format!("Line {line}, tour {}", tour.trim()), 4.0));
}

pub(super) fn start_duty(app: &mut App, line: &str, tour: &str) {
    let (Some(w), Some(sch)) = (app.world.clone(), app.schedule.as_mut()) else {
        return;
    };
    let now = app.clock.time;
    match sch.player_duty(&w, line, tour, now, None, false) {
        Ok(mut d) => {
            if let Some(p) = app.player.as_mut() {
                d.update(&mut p.vehicle, now);
                let (trip, stop) = d.trip_for_ibis();
                if p.auto_ibis {
                    p.set_duty_destination(trip, stop);
                }
                let mut fonts = w.fonts.lock();
                if let Err(e) =
                    crate::schedule_paper::update_vehicle(&mut p.vehicle, &d, &mut fonts)
                {
                    log::warn!("driver timetable paper: {e:#}");
                }
            }
            app.args.line = Some(line.to_string());
            app.args.tour = Some(tour.to_string());
            app.duty = Some(d);
            app.service_msg = Some((format!("Line {line}, tour {}", tour.trim()), 4.0));
        }
        Err(e) => app.service_msg = Some((format!("No duty: {e}"), 8.0)),
    }
}
