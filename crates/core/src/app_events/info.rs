//! Texts and rows of the timetable and the info line.

use super::*;

/// OMSI's timetable window: the current trip's stops with their times, the ones served
/// greyed, the next one marked.
pub(super) fn timetable_rows(
    duty: Option<&schedule::PlayerDuty>,
    delay: Option<f64>,
) -> Option<(String, Vec<(String, String, u8)>)> {
    let d = duty?;
    let trip = d.trips.get(d.trip_index)?;
    let hm = |t: f64| {
        format!(
            "{:02}:{:02}",
            ((t / 3600.0) as i64).rem_euclid(24),
            ((t % 3600.0) / 60.0) as i64
        )
    };
    let delay = delay.unwrap_or(0.0);
    let title = format!(
        "{} › {}   {}{}:{:02}   ({}/{})",
        if trip.line.trim().is_empty() {
            d.line.trim()
        } else {
            trip.line.trim()
        },
        trip.terminus.trim(),
        if delay < 0.0 { "−" } else { "+" },
        (delay.abs() / 60.0) as i64,
        (delay.abs() % 60.0) as i64,
        d.trip_index + 1,
        d.trips.len()
    );
    let last = trip.stops.iter().rposition(|s| s.stops);
    let mut rows: Vec<(String, String, u8)> = trip
        .stops
        .iter()
        .enumerate()
        .filter(|(_, s)| s.stops)
        .map(|(k, s)| {
            let time = if Some(k) == last {
                hm(s.arr)
            } else if s.dep - s.arr >= 60.0 {
                format!("{}-{}", hm(s.arr), &hm(s.dep)[3..])
            } else {
                hm(s.dep)
            };
            (
                s.name.trim().to_string(),
                time,
                if k < d.next_stop {
                    0
                } else if k == d.next_stop {
                    1
                } else {
                    2
                },
            )
        })
        .collect();
    if let Some(next) = d.trips.get(d.trip_index + 1) {
        let name = format!(
            "› {} {}",
            if next.line.trim().is_empty() {
                d.line.trim()
            } else {
                next.line.trim()
            },
            next.terminus.trim()
        );
        rows.push((name, hm(next.departure), 0));
    }
    Some((title, rows))
}

/// The outside air from the weather and the cabin air the vehicle scripts/engine maintain.
/// OMSI exposes both to every bus as Weather_Temperature and Cabinair_Temp.
pub(crate) fn vehicle_temperatures(p: &Player) -> (f32, f32) {
    let outside = p.vehicle.host.temperature;
    let inside = p
        .vehicle
        .var("Cabinair_Temp")
        .filter(|v| v.is_finite())
        .unwrap_or_else(|| outside.clamp(18.0, 25.0));
    (outside, inside)
}

pub(super) fn info_line(
    clock: &::simulation::SimClock,
    player: Option<&Player>,
    duty: Option<&schedule::PlayerDuty>,
    passengers: Option<usize>,
) -> String {
    let on = |k: &str| ::config::get_bool("ui", k).unwrap_or(true);
    let t = clock.time;
    let mut parts: Vec<String> = Vec::new();
    if on("info_time") {
        parts.push(format!(
            "{:02}:{:02}:{:02}",
            ((t / 3600.0) as i64).rem_euclid(24),
            ((t % 3600.0) / 60.0) as i64,
            (t % 60.0) as i64
        ));
    }
    if let Some(p) = player {
        if on("info_speed") {
            parts.push(format!(
                "{:.0} km/h",
                p.vehicle.physics.velocity_kmh().abs()
            ));
        }
        if on("info_temp") {
            let (outside, inside) = vehicle_temperatures(p);
            parts.push(format!(
                "{} {:.0} °C / {} {:.0} °C",
                ::user_interface::tr("ingame.infobar.outside"),
                outside,
                ::user_interface::tr("ingame.infobar.inside"),
                inside
            ));
        }
        if on("info_fuel") {
            if let Some(tank) = p.vehicle.var("tank_percent").filter(|v| v.is_finite()) {
                parts.push(format!(
                    "{} {:.0} %",
                    ::user_interface::tr("ingame.infobar.fuel"),
                    (tank * 100.0).round()
                ));
            }
        }
        if on("info_pax") {
            if let Some(n) = passengers {
                parts.push(passengers_aboard(n));
            }
        }
        if let Some(d) = duty {
            if let Some(trip) = d.trips.get(d.trip_index) {
                if on("info_line") {
                    let line = if trip.line.trim().is_empty() {
                        d.line.trim()
                    } else {
                        trip.line.trim()
                    };
                    parts.push(format!("{line} › {}", trip.terminus.trim()));
                }
                if on("info_next") {
                    if let Some(s) = trip.stops.get(d.next_stop) {
                        parts.push(format!(
                            "{}: {}",
                            ::user_interface::tr("ingame.infobar.next_stop"),
                            s.name.trim()
                        ));
                    }
                }
                if on("info_delay") {
                    let delay = p.vehicle.host.tt_delay;
                    // the interface colours the cell by its first char: L late, E early, O on time
                    let tone = if delay > 59.0 {
                        'L'
                    } else if delay < -59.0 {
                        'E'
                    } else {
                        'O'
                    };
                    parts.push(format!(
                        "\u{1e}{tone}{}{}:{:02}",
                        if delay < 0.0 { "−" } else { "+" },
                        (delay.abs() / 60.0) as i64,
                        (delay.abs() % 60.0) as i64
                    ));
                }
            }
        }
    }
    // the parts are told apart by the interface, which draws each as its own cell
    parts.join("\u{1f}")
}

/// `n` with the word for a passenger in the interface's language (singular for one; both
/// words are keys of the tables - the whole line is too much of a sentence to translate).
pub(super) fn passengers_aboard(n: usize) -> String {
    format!(
        "{n} {}",
        ::user_interface::tr(if n == 1 {
            "ingame.infobar.passenger"
        } else {
            "ingame.infobar.passengers"
        })
    )
}

#[cfg(test)]
mod info_tests {
    use super::passengers_aboard;

    /// The count stands before the word, which is singular for one passenger (in the
    /// tables' language; without a lookup the English key is drawn as it is).
    #[test]
    fn one_passenger_is_written_in_the_singular() {
        assert_eq!(passengers_aboard(0), "0 Passengers");
        assert_eq!(passengers_aboard(1), "1 Passenger");
        assert_eq!(passengers_aboard(23), "23 Passengers");
    }
}
