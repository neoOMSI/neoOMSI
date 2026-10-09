//! Pages of rows for the options, vehicle and world windows.

use super::*;

mod keys;
mod options;
mod rows;
mod vehicle;
mod world;

pub(crate) use self::keys::*;
pub(crate) use self::options::*;
pub(crate) use self::rows::*;
pub(crate) use self::vehicle::*;
pub(crate) use self::world::*;

pub(crate) fn pages_of(app: &App, kind: &ListKind) -> Option<(Vec<Page>, usize)> {
    let (pages, tab) = match kind {
        ListKind::Options(t) if is_sub_tab(*t) => (options_pages(app), app.map_return_tab),
        ListKind::Options(t) => (options_pages(app), *t),
        ListKind::Vehicle(t) => (vehicle_pages(app), *t),
        ListKind::World(t) => (world_pages(app), *t),
        _ => return None,
    };
    let pages: Vec<Page> = pages.into_iter().filter(|p| !p.1.is_empty()).collect();
    let tab = tab.min(pages.len().saturating_sub(1));
    Some((pages, tab))
}

pub(super) type TitlesCache = Option<(ListKind, bool, std::time::Instant, (Vec<String>, usize))>;

thread_local! {
    static TITLES: std::cell::RefCell<TitlesCache> = const { std::cell::RefCell::new(None) };
}

pub(crate) fn forget_page_titles() {
    TITLES.with(|c| *c.borrow_mut() = None);
    *DRIVER_SCAN.lock().unwrap_or_else(|e| e.into_inner()) = None;
    *WEATHER_SCAN.lock().unwrap_or_else(|e| e.into_inner()) = None;
}

pub(crate) fn page_titles(app: &App, kind: &ListKind) -> Option<(Vec<String>, usize)> {
    let vr_nav_available = app.vr_active() && app.player.is_some();
    if let Some(hit) = TITLES.with(|c| {
        c.borrow()
            .as_ref()
            .filter(|(k, vr, t, _)| {
                k == kind && *vr == vr_nav_available && t.elapsed().as_millis() < 5000
            })
            .map(|(_, _, _, r)| r.clone())
    }) {
        return Some(hit);
    }
    let (pages, tab) = pages_of(app, kind)?;
    let r = (pages.iter().map(|p| p.0.clone()).collect::<Vec<_>>(), tab);
    TITLES.with(|c| {
        *c.borrow_mut() = Some((
            kind.clone(),
            vr_nav_available,
            std::time::Instant::now(),
            r.clone(),
        ))
    });
    Some(r)
}
