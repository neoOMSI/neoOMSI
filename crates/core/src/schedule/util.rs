//! Small helpers: times, tour keys, day bits, terminus texts.

use super::*;

/// "HH:MM" of a time of day in seconds.
pub(crate) fn hhmm(t: f64) -> String {
    format!(
        "{:02}:{:02}",
        (t / 3600.0) as i32,
        ((t % 3600.0) / 60.0) as i32
    )
}

/// The trip the player picked: "HH:MM" - the first trip leaving at that minute or later -
/// or its number in the tour (1 = the first).
pub(super) fn chosen_trip(trips: &[PlannedTrip], pick: &str) -> Option<usize> {
    if let Some((h, m)) = pick.split_once(':') {
        let (h, m) = (h.trim().parse::<f64>().ok()?, m.trim().parse::<f64>().ok()?);
        let at = h * 3600.0 + m * 60.0;
        return trips.iter().position(|t| t.departure >= at - 30.0);
    }
    let n = pick.parse::<usize>().ok()?;
    (n >= 1 && n <= trips.len()).then(|| n - 1)
}

/// The trip of a duty that fits the time of day: the one under way, else the next to leave
/// (the last one once all are over).
pub(super) fn starting_trip(trips: &[PlannedTrip], now: f64) -> usize {
    trips.iter().position(|t| t.end > now).unwrap_or_else(|| {
        // every trip over for today: a tour after midnight (the night line 13N's runs from
        // 0:49) picked in the evening is tonight's, and starts at its first trip
        match trips.first() {
            Some(first) if first.departure + DAY - now < DAY / 2.0 => 0,
            _ => trips.len().saturating_sub(1),
        }
    })
}

pub(super) const DAY: f64 = 86_400.0;

/// A 64-bit hash of a tour: its depot group (lower case), line and tour number.
pub(super) fn tour_key_of(group: &str, line: &str, tour: &str) -> u64 {
    let mut h: u64 = 0xcbf2_9ce4_8422_2325;
    for b in group
        .bytes()
        .chain([0])
        .chain(line.trim().bytes())
        .chain([0])
        .chain(tour.trim().bytes())
    {
        h = (h ^ b as u64).wrapping_mul(0x0000_0100_0000_01b3);
    }
    mix(h)
}

pub(super) fn mix(mut h: u64) -> u64 {
    h ^= h >> 33;
    h = h.wrapping_mul(0xff51_afd7_ed55_8ccd);
    h ^ (h >> 33)
}

/// A vehicle file path compared as `car_use` writes it (`vehicles\MAN_SD200\MAN_SD77.bus`):
/// lower case with forward slashes.
pub(super) fn norm_vehicle_path(p: &str) -> String {
    p.trim().replace('\\', "/").to_ascii_lowercase()
}

/// The tour mask bits `clock`'s date selects: (the weekday's or public holiday's, the school
/// holidays' or school days'). the original: bit 8 = runs in the school holidays,
/// bit 9 = runs on school days.
pub(super) fn day_bits(calendar: &::map::Calendar, clock: &::simulation::SimClock) -> (i32, i32) {
    let date = clock.date_code();
    let day_bit = if calendar.is_holiday(date) {
        1 << 7
    } else {
        1 << clock.weekday()
    };
    let school_bit = if calendar.in_holiday_range(date) {
        1 << 8
    } else {
        1 << 9
    };
    (day_bit, school_bit)
}

/// What a bus's displays call its terminus: the depot file's first string for it (what the
/// IBIS shows, in capitals - the stock departure display's font has no small letters, and
/// the trip's "Bauernhof" came out as a lone "B"), else the timetable's name in capitals
/// (a train has no depot file).
pub(super) fn terminus_text(hof: Option<&::legacy_vehicle::Hof>, terminus: &str) -> String {
    let name = terminus.trim();
    hof.and_then(|h| {
        h.termini.iter().find(|t| {
            t.texture_id.trim().eq_ignore_ascii_case(name)
                || t.strings
                .iter()
                .any(|s| s.trim().eq_ignore_ascii_case(name))
        })
    })
        .and_then(|t| t.strings.first())
        .map(|s| s.trim().to_string())
        .unwrap_or_else(|| name.to_uppercase())
}

/// GetTTTerminusIndex as Omsi.exe answers it: the first depot terminus whose name is the
/// trip's terminus (the second [trip] line), else -1.
pub(super) fn tt_terminus_index(hof: Option<&::legacy_vehicle::hof::Hof>, terminus: &str) -> i32 {
    hof.and_then(|h| h.termini.iter().position(|t| t.texture_id == terminus))
        .map_or(-1, |i| i as i32)
}
