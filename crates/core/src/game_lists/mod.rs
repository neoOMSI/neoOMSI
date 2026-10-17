//! The game menu's own windows besides the administration (see `admin`), as OMSI has them
//! in its menus: the options that can change while driving, the line and tour to drive,
//! the driver whose personnel file the run goes into, and the bus's fleet number. Each is a
//! list of (label, action) lines in the menu's chooser; choosing a line does it and shows
//! the list again (or the next one: a line's tours).

mod dropdown;
mod items;
mod options;
mod pages;
mod run;
mod settings;
mod tours;

#[allow(unused_imports)]
pub(crate) use self::dropdown::*;
#[allow(unused_imports)]
pub(crate) use self::items::*;
#[allow(unused_imports)]
pub(crate) use self::options::*;
#[allow(unused_imports)]
pub(crate) use self::pages::*;
#[allow(unused_imports)]
pub(crate) use self::run::*;
#[allow(unused_imports)]
pub(crate) use self::settings::*;
#[allow(unused_imports)]
pub(crate) use self::tours::*;

use crate::App;

#[derive(Debug, Clone, PartialEq)]
pub(crate) enum ListKind {
    Admin,
    Options(usize),
    Vehicle(usize),
    World(usize),
    Lines,
    /// A line's tours; the stop chosen in the timetable beside them to start from: (the
    /// tour's number, the stop as `Schedule::tour_stops` lists them), none: the default.
    Tours(String, Option<(String, usize, usize)>),
    Drivers,
    Numbers,
    Destinations,
    RouteNumbers,
    Hofs,
    Spots,
    PlaceMaker,
    PlaceType(String),
    PlaceLivery(String),
    PlaceHof(String, String),
}

/// A list line that heads the lines under it: not chosen, not run.
pub(crate) const HEADING: &str = "#";
/// The end of the action of a line whose value Left and Right step down and up (and the
/// arrows drawn round its value): the action is run with `-` or `+` in its place, and with
/// it as it is on Enter (see `App::chooser_adjust`).
pub(crate) const ADJUST: &str = " ±";

#[derive(Clone, Copy, Debug)]
pub(crate) enum Move {
    Next,
    Dec,
    Inc,
    To(f32),
}

pub(super) type Page = (String, Vec<(String, String)>);

/// Caches a value the lists request on every redraw, along with its creation time and key.
pub(crate) type Cache<K, V> = std::sync::Mutex<Option<(std::time::Instant, K, V)>>;

/// The value kept in `slot`, rebuilt by `build` when `key` changed or it is older than `ttl`.
pub(crate) fn cached<K: PartialEq, V: Clone>(
    slot: &Cache<K, V>,
    key: K,
    ttl: std::time::Duration,
    build: impl FnOnce() -> V,
) -> V {
    let mut entry = slot.lock().unwrap_or_else(|e| e.into_inner());
    if let Some((made, made_for, value)) = entry.as_ref()
        && *made_for == key
        && made.elapsed() < ttl
    {
        return value.clone();
    }
    let value = build();
    *entry = Some((std::time::Instant::now(), key, value.clone()));
    value
}

pub(super) type ScanCache = Cache<(), Vec<String>>;

/// Directory scans are asked for with every redraw: kept for 5 s, dropped when a list is opened.
pub(super) fn scan_cached(slot: &ScanCache, scan: impl FnOnce() -> Vec<String>) -> Vec<String> {
    cached(slot, (), std::time::Duration::from_secs(5), scan)
}

pub(super) static DRIVER_SCAN: ScanCache = std::sync::Mutex::new(None);
pub(super) static WEATHER_SCAN: ScanCache = std::sync::Mutex::new(None);

/// Set by `option_do` when a value really changed: the open list is out of date then.
pub(crate) static LIST_DIRTY: std::sync::atomic::AtomicBool =
    std::sync::atomic::AtomicBool::new(false);

#[cfg(test)]
mod tests {
    #[test]
    fn steps_wrap_round() {
        assert_eq!(super::next_step(&super::SPEEDS, 1.0), 2.0);
        assert_eq!(super::next_step(&super::SPEEDS, 15.0), 1.0);
        assert_eq!(super::next_step(&super::TRAFFIC, 35), 50);
    }

    #[test]
    fn lines_sort_as_numbers() {
        let mut v = vec!["13N", "5", "137", "N30", "92"];
        v.sort_by(|a, b| super::natural(a, b));
        assert_eq!(v, vec!["5", "13N", "92", "137", "N30"]);
    }
}
