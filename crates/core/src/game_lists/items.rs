//! Lines of the vehicle, route, driver, fleet number and place lists.

use super::*;

pub(super) fn bus_def(app: &App, bus: &str) -> Option<::legacy_vehicle::Vehicle> {
    let path = crate::spawn::player_bus_path(&app.args.root, bus).ok()?;
    ::legacy_vehicle::Vehicle::load(&path).ok()
}

pub(super) fn route_numbers(app: &App) -> Vec<String> {
    let mut out: Vec<String> = Vec::new();
    if let Some(hof) = app.player.as_ref().and_then(|p| p.vehicle.host.hof.clone()) {
        for t in &hof.info_trips {
            let code = t.code.trim();
            let l = if !t.line.trim().is_empty() {
                t.line.trim().to_string()
            } else if code.len() > 2 && code.chars().all(|c| c.is_ascii_digit()) {
                code[..code.len() - 2].trim_start_matches('0').to_string()
            } else {
                String::new()
            };
            let l = l.trim_matches(|c: char| !c.is_alphanumeric()).to_string();
            if !l.is_empty() && !out.contains(&l) {
                out.push(l);
            }
        }
    }
    if let Some(sch) = app.schedule.as_ref() {
        for l in &sch.data.lines {
            let n = l.name.trim().to_string();
            if !n.is_empty() && !out.contains(&n) {
                out.push(n);
            }
        }
    }
    out.sort_by_cached_key(|s| natural_key(s));
    out
}

pub(crate) fn set_route_by_hand(app: &mut App, line: &str) {
    let line = line.trim();
    if line.is_empty() {
        return;
    }
    if let Some(p) = app.player.as_mut() {
        let hof = p.vehicle.host.hof.clone();
        let code = p.vehicle.var("IBIS_TerminusCode").unwrap_or(-1.0) as i32;
        let named = |t: &&::legacy_vehicle::hof::Terminus| {
            t.strings.first().is_some_and(|s| !s.trim().is_empty())
        };
        let term = hof.as_ref().and_then(|h| {
            h.termini
                .iter()
                .filter(named)
                .find(|t| t.code == code)
                .or_else(|| h.termini.iter().find(named))
        });
        let name = term
            .and_then(|t| t.strings.first().cloned())
            .unwrap_or_default();
        crate::schedule::set_player_destination_directly(
            &mut p.vehicle,
            hof.as_deref(),
            line,
            &name,
            &[],
        );
        log::info!(
            "route number set by hand: {line} (IBIS_LinieKurs {:?})",
            p.vehicle.var("IBIS_LinieKurs")
        );
        app.service_msg = Some((::i18n::translate("pause.list.route_set", &[("line", &line)]), 3.0));
    }
}

/// Names in older packs often use underscores as spaces.
pub(super) fn bus_label(name: &str) -> String {
    name.replace('_', " ")
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
}

/// Numbers inside names sort as numbers (DL9 before DL10), case does not matter.
pub(super) fn bus_cmp(a: &str, b: &str) -> std::cmp::Ordering {
    use std::cmp::Ordering;
    let (mut a, mut b) = (
        a.chars().flat_map(char::to_lowercase).peekable(),
        b.chars().flat_map(char::to_lowercase).peekable(),
    );
    loop {
        match (a.peek().copied(), b.peek().copied()) {
            (None, None) => return Ordering::Equal,
            (None, _) => return Ordering::Less,
            (_, None) => return Ordering::Greater,
            (Some(x), Some(y)) if x.is_ascii_digit() && y.is_ascii_digit() => {
                let x: String = std::iter::from_fn(|| a.next_if(|c| c.is_ascii_digit())).collect();
                let y: String = std::iter::from_fn(|| b.next_if(|c| c.is_ascii_digit())).collect();
                let (x, y) = (x.trim_start_matches('0'), y.trim_start_matches('0'));
                let order = x.len().cmp(&y.len()).then_with(|| x.cmp(y));
                if order != Ordering::Equal {
                    return order;
                }
            }
            (Some(x), Some(y)) => {
                let order = x.cmp(&y);
                if order != Ordering::Equal {
                    return order;
                }
                a.next();
                b.next();
            }
        }
    }
}

pub(super) fn place_vehicles(app: &App, unknown: &str) -> Vec<(String, String, String, String)> {
    app.vehicle_list
        .iter()
        .map(|(name, path)| {
            let (maker, ty) = app.vehicle_meta.get(path).cloned().unwrap_or_default();
            let maker = bus_label(&maker);
            let ty = if ty.trim().is_empty() {
                bus_label(name)
            } else {
                bus_label(&ty)
            };
            let shown = if maker.is_empty() {
                unknown.to_string()
            } else {
                maker.clone()
            };
            (maker.to_lowercase(), shown, ty, path.clone())
        })
        .collect()
}

pub(crate) fn place_makers(app: &App) -> Vec<(String, String, usize)> {
    let all = place_vehicles(app, &::i18n::translate("pause.list.unknown_maker", &[]));
    let mut groups: Vec<(String, String, usize)> = Vec::new();
    for v in &all {
        match groups.iter_mut().find(|g| g.0 == v.0) {
            Some(g) => g.2 += 1,
            None => groups.push((v.0.clone(), v.1.clone(), 1)),
        }
    }
    groups.sort_by(|a, b| bus_cmp(&a.1, &b.1).then_with(|| a.0.cmp(&b.0)));
    groups
}

pub(super) fn liveries(def: &::legacy_vehicle::Vehicle) -> Vec<String> {
    let Some(m) = def.model.as_ref() else {
        return Vec::new();
    };
    let mp = ::legacy_config::resolve_path(def.dir(), m);
    let Ok(model) = ::model::Model::load(&mp) else {
        return Vec::new();
    };
    let mut names: Vec<String> = model
        .ctc
        .iter()
        .flat_map(|c| {
            ::simulation::vehicle::load_paint_schemes(&::legacy_config::resolve_path(def.dir(), &c.path))
        })
        .map(|s| s.name)
        .collect();
    names.dedup();
    names
}

pub(super) fn hof_label(p: &std::path::Path) -> String {
    let file = p
        .file_name()
        .map(|f| f.to_string_lossy().into_owned())
        .unwrap_or_default();
    match ::legacy_vehicle::Hof::load(p)
        .ok()
        .map(|h| h.name)
        .filter(|n| !n.trim().is_empty())
    {
        Some(n)
        if !file
            .to_ascii_lowercase()
            .starts_with(&n.trim().to_ascii_lowercase()) =>
            {
                format!("{}  ({file})", n.trim())
            }
        _ => file,
    }
}

pub(crate) fn items(app: &App, kind: &ListKind) -> Vec<(String, String)> {
    let mut out: Vec<(String, String)> = Vec::new();
    match kind {
        ListKind::Admin => return crate::admin::items(app),
        ListKind::Options(_) | ListKind::Vehicle(_) | ListKind::World(_) => {
            if matches!(kind, ListKind::Options(t) if *t == MAP_TAB) {
                return map_options_page(app).1;
            }
            if matches!(kind, ListKind::Options(t) if *t == KEYS_TAB) {
                return key_rows(&app.args.root, &KeyView::of(app));
            }
            if matches!(kind, ListKind::Options(t) if *t == LOOK_TAB) {
                return look_options_page(app).1;
            }
            let Some((mut pages, tab)) = pages_of(app, kind) else {
                return out;
            };
            if pages.is_empty() {
                return vec![(
                    row(&::i18n::translate("pause.list.nothing_to_set", &[]), 'i', "", "", None),
                    "noop".to_string(),
                )];
            }
            return pages.swap_remove(tab).1;
        }
        ListKind::Lines => {
            if let Some(sch) = app.schedule.as_ref() {
                let mut lines: Vec<&::timetable::Line> = sch
                    .data
                    .lines
                    .iter()
                    .filter(|l| {
                        l.user_allowed
                            && l.tours
                            .iter()
                            .any(|t| tour_listed(sch, &l.name, t, app.clock.time))
                    })
                    .collect();
                lines.sort_by(|a, b| natural(&a.name, &b.name));
                for l in lines {
                    let count = l
                        .tours
                        .iter()
                        .filter(|t| tour_listed(sch, &l.name, t, app.clock.time))
                        .count();
                    let key = if count == 1 { "pause.list.line_one" } else { "pause.list.line_many" };
                    out.push((
                        ::i18n::translate(key, &[("name", &l.name), ("count", &count)]),
                        format!("line {}", l.name),
                    ));
                }
            }
            if out.is_empty() {
                out.push((::i18n::translate("pause.list.no_timetable", &[]), "back".into()));
            }
        }
        ListKind::Tours(line, _) => {
            if let Some(l) = app
                .schedule
                .as_ref()
                .and_then(|s| s.data.lines.iter().find(|l| l.name == *line))
            {
                for t in sorted_tours(l).into_iter().filter(|t| {
                    app.schedule
                        .as_ref()
                        .is_some_and(|s| tour_listed(s, line, t, app.clock.time))
                }) {
                    out.push((
                        ::i18n::translate("pause.list.tour", &[("number", &t.number.trim())]),
                        format!("tour {}\u{1}{}", line, t.number),
                    ));
                }
            }
        }
        ListKind::Drivers => {
            for name in driver_names(app) {
                let mark = if app
                    .career
                    .path
                    .as_ref()
                    .and_then(|p| p.file_stem())
                    .is_some_and(|s| s.to_string_lossy().eq_ignore_ascii_case(&name))
                {
                    format!("  {}", ::i18n::translate("pause.list.now", &[]))
                } else {
                    String::new()
                };
                out.push((format!("{name}{mark}"), format!("driver {name}")));
            }
        }
        ListKind::Destinations => {
            if let Some(p) = app.player.as_ref().filter(|p| p.vehicle.host.hof.is_some()) {
                let now = p
                    .vehicle
                    .var("IBIS_LinieKurs")
                    .filter(|l| *l > 0.0)
                    .map(|l| format!("{}", l as i64))
                    .unwrap_or_else(|| "-".into());
                out.push((::i18n::translate("pause.list.route_now", &[("value", &now)]), "routes".into()));
            }
            if let Some(hof) = app.player.as_ref().and_then(|p| p.vehicle.host.hof.clone()) {
                let mut termini: Vec<(String, String)> = hof
                    .termini
                    .iter()
                    .map(|t| {
                        (
                            t.strings
                                .iter()
                                .find(|s| !s.trim().is_empty())
                                .cloned()
                                .unwrap_or_else(|| t.code.to_string()),
                            t.code.to_string(),
                        )
                    })
                    .collect();
                termini.sort_by_key(|(name, _)| name.trim().to_lowercase());
                for (name, code) in termini {
                    out.push((
                        format!("{:>3}  {}", code, name.trim()),
                        format!("dest {code}"),
                    ));
                }
            }
            if out.is_empty() {
                out.push((
                    ::i18n::translate("pause.list.no_destinations", &[]),
                    "back".into(),
                ));
            }
        }
        ListKind::RouteNumbers => {
            // any route number, typed as on OMSI's own field (#836): the scripts that read
            // it (a bus that switches its functions by route number) take what is typed
            match app.menu_edit.as_ref() {
                Some(t) => out.push((
                    ::i18n::translate("pause.list.route_typing", &[("text", t)]),
                    "route_type".into(),
                )),
                None => out.push((::i18n::translate("pause.list.route_type", &[]), "route_type".into())),
            }
            for l in route_numbers(app) {
                out.push((::i18n::translate("pause.list.route", &[("line", &l)]), format!("route {l}")));
            }
            if out.len() == 1 {
                out.push((
                    ::i18n::translate("pause.list.no_routes", &[]),
                    "back".into(),
                ));
            }
        }
        ListKind::Hofs => {
            if let Some(p) = app.player.as_ref() {
                let now = p.vehicle.host.hof.as_ref().map(|h| h.path.clone());
                let mut files: Vec<(String, std::path::PathBuf)> =
                    ::legacy_vehicle::hof::depot_files(p.vehicle.ty.def.dir())
                        .into_iter()
                        .map(|f| (hof_label(&f), f))
                        .collect();
                files.sort_by_key(|(label, _)| label.to_lowercase());
                for (label, f) in files {
                    let mark = if now.as_ref() == Some(&f) {
                        format!("  {}", ::i18n::translate("pause.list.now", &[]))
                    } else {
                        String::new()
                    };
                    out.push((
                        format!("{label}{mark}"),
                        format!("hof {}", f.to_string_lossy()),
                    ));
                }
            }
            if out.is_empty() {
                out.push((::i18n::translate("pause.list.no_hofs", &[]), "back".into()));
            }
        }
        ListKind::Spots => {
            if let Some(w) = app.world.as_ref() {
                // (the entry points of tiles that are not loaded come from the map index)
                w.index();
                // our bus where it stands, to put it at each place: one where it would touch
                // a vehicle - an AI one, or our bus itself - is taken
                let ours = app.player.as_ref().map(|p| {
                    let v = &p.vehicle;
                    (crate::traffic::vehicle_bodies(v), v.position, v.heading)
                });
                let taken = |i: usize| {
                    let (Some((bodies, from, from_heading)), Some((pos, rot))) =
                        (ours.as_ref(), w.entry_point_place(&w.global.entry_points[i]))
                    else {
                        return false;
                    };
                    let mut there =
                        crate::traffic::bodies_moved(bodies, *from, *from_heading, pos, rot[0]);
                    for b in &mut there {
                        b.half += glam::DVec2::splat(0.5);
                    }
                    bodies.iter().any(|o| there.iter().any(|b| b.overlaps(o)))
                        || app.traffic.as_ref().is_some_and(|t| t.occupied(&there, pos))
                };
                // one line per name, its first free place; a name with none is left out
                for (name, i) in w.global.free_entry_points(taken) {
                    let label = if name.is_empty() {
                        let e = &w.global.entry_points[i];
                        ::i18n::translate("pause.list.entry", &[("number", &(e.index + 1))])
                    } else {
                        name.to_string()
                    };
                    out.push((label, format!("spot {i}")));
                }
            }
            if out.is_empty() {
                out.push((::i18n::translate("pause.list.no_entries", &[]), "back".into()));
            }
        }
        ListKind::PlaceMaker => {
            let all = place_vehicles(app, &::i18n::translate("pause.list.unknown_maker", &[]));
            let mut groups: Vec<(String, String, Vec<&(String, String, String, String)>)> =
                Vec::new();
            for v in &all {
                match groups.iter_mut().find(|g| g.0 == v.0) {
                    Some(g) => g.2.push(v),
                    None => groups.push((v.0.clone(), v.1.clone(), vec![v])),
                }
            }
            groups.sort_by(|a, b| bus_cmp(&a.1, &b.1).then_with(|| a.0.cmp(&b.0)));
            for (key, name, vs) in groups {
                if vs.len() == 1 {
                    out.push((
                        format!("{name}  ·  {}", vs[0].2),
                        format!("bus {}", vs[0].3),
                    ));
                } else {
                    out.push((
                        ::i18n::translate("pause.list.maker_models", &[("name", &name), ("count", &vs.len())]),
                        format!("maker {key}"),
                    ));
                }
            }
        }
        ListKind::PlaceType(key) => {
            let all = place_vehicles(app, &::i18n::translate("pause.list.unknown_maker", &[]));
            let mut types: Vec<(String, String)> = all
                .iter()
                .filter(|v| v.0 == *key)
                .map(|v| (v.2.clone(), v.3.clone()))
                .collect();
            types.sort_by(|a, b| bus_cmp(&a.0, &b.0).then_with(|| a.1.cmp(&b.1)));
            // (a type name used twice: with its pack's folder, then with its file)
            let same = |t: &[(String, String)], n: &str| {
                t.iter()
                    .filter(|x| x.0.to_lowercase() == n.to_lowercase())
                    .count()
            };
            let counts: Vec<usize> = types.iter().map(|t| same(&types, &t.0)).collect();
            for (t, n) in types.iter().zip(counts) {
                let mut label = t.0.clone();
                if n > 1 {
                    let parts: Vec<&str> = t.1.split('/').collect();
                    let folder = parts.get(1).copied().unwrap_or_default();
                    let file = parts
                        .last()
                        .copied()
                        .unwrap_or_default()
                        .rsplit_once('.')
                        .map(|x| x.0)
                        .unwrap_or_default();
                    label = format!("{label}  ·  {}  ·  {}", bus_label(folder), bus_label(file));
                }
                out.push((label, format!("bus {}", t.1)));
            }
        }
        ListKind::PlaceLivery(bus) => {
            out.push((::i18n::translate("pause.dialog.place.random_livery", &[]), "livery ".into()));
            let mut names = bus_def(app, bus).map(|d| liveries(&d)).unwrap_or_default();
            names.sort_by_key(|n| n.to_lowercase());
            names.dedup();
            for n in names {
                out.push((n.clone(), format!("livery {n}")));
            }
        }
        ListKind::PlaceHof(bus, _) => {
            out.push((::i18n::translate("pause.dialog.place.depot_file", &[]), "placehof ".into()));
            if let Some(d) = bus_def(app, bus) {
                let mut files: Vec<(String, String)> = ::legacy_vehicle::hof::depot_files(d.dir())
                    .into_iter()
                    .map(|f| {
                        (
                            hof_label(&f),
                            f.file_name()
                                .map(|x| x.to_string_lossy().into_owned())
                                .unwrap_or_default(),
                        )
                    })
                    .collect();
                files.sort_by_key(|(label, _)| label.to_lowercase());
                for (label, name) in files {
                    out.push((label, format!("placehof {name}")));
                }
            }
        }
        ListKind::Numbers => {
            if let Some(p) = app.player.as_ref() {
                let mut numbers = fleet_numbers(&p.vehicle);
                numbers.sort_by(|a, b| natural(&a.0, &b.0));
                for (n, reg) in numbers {
                    out.push((
                        if reg.is_empty() {
                            n.clone()
                        } else {
                            format!("{n}  ({reg})")
                        },
                        format!("number {n}\u{1}{reg}"),
                    ));
                }
            }
            if out.is_empty() {
                out.push((::i18n::translate("pause.list.no_numbers", &[]), "back".into()));
            }
        }
    }
    out.push((::i18n::translate("pause.dialog.back", &[]), "back".into()));
    out
}

pub(crate) fn menu_extras(
    kind: Option<&ListKind>,
    list: Option<&[(String, String)]>,
    sel: Option<usize>,
    schedule: Option<&crate::schedule::Schedule>,
    now: f64,
) -> (
    crate::ui::MenuKind,
    Option<(String, String)>,
    Option<crate::ui::Preview>,
) {
    use crate::ui::{MenuKind, Preview};
    let Some(sel) = sel else {
        return (MenuKind::Game, None, None);
    };
    let head = |key: &str| Some((::i18n::translate(key, &[]), String::new()));
    let hm = |m: f32| format!("{:02}:{:02}", (m / 60.0) as i32 % 24, (m % 60.0) as i32);
    let trip_of = |name: &str| -> (String, String) {
        schedule
            .and_then(|s| {
                s.data
                    .trips
                    .iter()
                    .find(|x| x.name.eq_ignore_ascii_case(name))
            })
            .map(|x| (x.line.trim().to_string(), x.terminus.trim().to_string()))
            .unwrap_or_default()
    };
    let action = list
        .and_then(|l| l.get(sel))
        .map(|x| x.1.as_str())
        .unwrap_or("");
    let Some(kind) = kind else {
        return (MenuKind::List, head("pause.dialog.title.place"), None);
    };
    match kind {
        ListKind::Options(t) if *t == KEYS_TAB => {
            (MenuKind::Options, head("pause.dialog.title.keys"), None)
        }
        ListKind::Options(_) => (MenuKind::Options, head("pause.dialog.title.options"), None),
        ListKind::Vehicle(_) => (MenuKind::Options, head("pause.dialog.title.vehicle"), None),
        ListKind::World(_) => (MenuKind::Options, head("pause.dialog.title.world"), None),
        ListKind::Lines => {
            let preview = action.strip_prefix("line ").and_then(|name| {
                let line = schedule?.data.lines.iter().find(|l| l.name == name)?;
                let rows = sorted_tours(line)
                    .into_iter()
                    .filter(|t| schedule.is_some_and(|s| tour_listed(s, &line.name, t, now)))
                    .map(|t| {
                        let next = schedule.and_then(|s| {
                            s.tour_stops_from(&line.name, &t.number, now)
                                .first()
                                .cloned()
                        });
                        let end = match (schedule, next.as_ref()) {
                            (Some(s), Some(n)) => tour_trip_name(s, t, n.0)
                                .map(|name| trip_of(&name).1)
                                .unwrap_or_default(),
                            _ => t
                                .trips
                                .first()
                                .map(|tt| trip_of(&tt.trip).1)
                                .unwrap_or_default(),
                        };
                        let what = if end.is_empty() {
                            ::i18n::translate("pause.list.tour", &[("number", &t.number.trim())])
                        } else {
                            ::i18n::translate("pause.list.tour_to", &[("number", &t.number.trim()), ("end", &end)])
                        };
                        let when = match next {
                            Some(n) => hm((n.3 / 60.0) as f32),
                            None => t
                                .trips
                                .first()
                                .map(|tt| hm(tt.departure))
                                .unwrap_or_default(),
                        };
                        (what, when)
                    })
                    .collect();
                Some(Preview {
                    title: ::i18n::translate("pause.list.line_title", &[("name", &line.name)]),
                    meta: {
                        let count = line
                            .tours
                            .iter()
                            .filter(|t| schedule.is_some_and(|s| tour_listed(s, &line.name, t, now)))
                            .count();
                        let key = if count == 1 { "pause.list.tours_one" } else { "pause.list.tours_many" };
                        ::i18n::translate(key, &[("count", &count)])
                    },
                    rows,
                    chosen: None,
                    button: None,
                    time: None,
                })
            });
            (MenuKind::Lines, head("pause.dialog.title.line_tour"), preview)
        }
        ListKind::Tours(line_name, pick) => {
            // (the line number of the trip of a tour shown: a tour may run trips of several lines)
            let tour_line = |ln: &str, num: &str| -> Option<String> {
                let sch = schedule?;
                let line = sch.data.lines.iter().find(|l| l.name == ln)?;
                let tour = line.tours.iter().find(|t| t.number == num)?;
                let n_trips = sch.tour_trip_count(ln, num);
                let trip = pick
                    .as_ref()
                    .filter(|p| p.0 == num)
                    .map(|p| p.2)
                    .unwrap_or_else(|| sch.tour_trip_now(ln, num, now))
                    .min(n_trips.saturating_sub(1));
                let stops = sch.tour_trip_stops(ln, num, trip);
                stops
                    .first()
                    .and_then(|s| tour_trip_name(sch, tour, s.0))
                    .map(|n| trip_of(&n).0)
                    .filter(|l| !l.is_empty())
            };
            let preview = action
                .strip_prefix("tour ")
                .and_then(|rest| rest.split_once('\u{1}'))
                .and_then(|(ln, num)| {
                    let sch = schedule?;
                    let line = sch.data.lines.iter().find(|l| l.name == ln)?;
                    let tour = line.tours.iter().find(|t| t.number == num)?;
                    let n_trips = sch.tour_trip_count(ln, num);
                    let trip = pick
                        .as_ref()
                        .filter(|p| p.0 == num)
                        .map(|p| p.2)
                        .unwrap_or_else(|| sch.tour_trip_now(ln, num, now))
                        .min(n_trips.saturating_sub(1));
                    let stops = sch.tour_trip_stops(ln, num, trip);
                    let at = stops
                        .first()
                        .map(|s| s.3)
                        .unwrap_or_else(|| tour_start(tour).unwrap_or(0.0));
                    let chosen = pick
                        .as_ref()
                        .filter(|p| p.0 == num)
                        .map(|p| p.1)
                        .unwrap_or(0)
                        .min(stops.len().saturating_sub(1));
                    let rows = stops
                        .iter()
                        .map(|s| (s.2.trim().to_string(), hm((s.3 / 60.0) as f32)))
                        .collect();
                    let trip_line = tour_line(ln, num).unwrap_or_else(|| line_sign(schedule, line));
                    Some(Preview {
                        title: ::i18n::translate("pause.list.tour", &[("number", &num.trim())]),
                        meta: ::i18n::translate(
                            "pause.list.preview_meta",
                            &[("line", &trip_line), ("trip", &(trip + 1)), ("trips", &n_trips.max(1))],
                        ),
                        rows,
                        chosen: Some(chosen),
                        button: Some(::i18n::translate("pause.list.start_trip", &[])),
                        time: Some(hm((at / 60.0) as f32)),
                    })
                });
            let chosen_line = action
                .strip_prefix("tour ")
                .and_then(|rest| rest.split_once('\u{1}'))
                .and_then(|(ln, num)| tour_line(ln, num));
            let sign = chosen_line
                .or_else(|| {
                    schedule
                        .and_then(|s| s.data.lines.iter().find(|l| l.name == *line_name))
                        .map(|l| line_sign(schedule, l))
                })
                .unwrap_or_else(|| line_name.clone());
            (
                MenuKind::Tours,
                Some((
                    ::i18n::translate("pause.dialog.title.line_tour", &[]),
                    ::i18n::translate("pause.list.line_title", &[("name", &sign)]),
                )),
                preview,
            )
        }
        ListKind::Drivers => (MenuKind::List, head("pause.dialog.title.driver"), None),
        ListKind::Numbers => (MenuKind::List, head("pause.dialog.title.fleet_number"), None),
        ListKind::Destinations => (MenuKind::List, head("pause.dialog.title.destination"), None),
        ListKind::RouteNumbers => (
            MenuKind::List,
            Some((::i18n::translate("pause.list.route_number", &[]), String::new())),
            None,
        ),
        ListKind::Hofs => (MenuKind::List, head("pause.dialog.title.hof"), None),
        ListKind::Spots => (MenuKind::List, head("pause.dialog.title.spot"), None),
        ListKind::PlaceMaker
        | ListKind::PlaceType(_)
        | ListKind::PlaceLivery(_)
        | ListKind::PlaceHof(..) => (MenuKind::List, head("pause.dialog.title.place"), None),
        ListKind::Admin => (
            MenuKind::List,
            head("pause.dialog.title.admin"),
            None,
        ),
    }
}

pub(super) fn driver_names(_app: &App) -> Vec<String> {
    scan_cached(&DRIVER_SCAN, || {
        let mut names: Vec<String> = ::legacy_config::read_dir_merged("Drivers")
            .into_iter()
            .filter(|p| p.extension().is_some_and(|e| e.eq_ignore_ascii_case("odr")))
            .filter_map(|p| p.file_stem().map(|s| s.to_string_lossy().to_string()))
            .collect();
        names.sort_by_cached_key(|n| n.to_ascii_lowercase());
        names.dedup_by(|a, b| a.eq_ignore_ascii_case(b));
        names
    })
}

pub(super) fn switch_driver(app: &mut App, name: &str) {
    if app.career.path.is_some() {
        if let Err(e) = app.career.save() {
            log::warn!("writing the personnel file: {e}");
        }
    }
    let rel = format!("Drivers/{name}.odr");
    let mut next = crate::career::Career::load(&app.args.root, &rel);
    // (the distance and the clock of the run go on; the counters start with the new file)
    next.seconds = app.career.seconds;
    app.career = next;
    app.args.driver = Some(rel);
    app.service_msg = Some((::i18n::translate("pause.list.driver_set", &[("name", &name)]), 3.0));
}

pub(super) fn fleet_numbers(v: &::simulation::VehicleInstance) -> Vec<(String, String)> {
    let def = &v.ty.def;
    def.numbers_with_plates()
        .into_iter()
        .map(|(n, _)| {
            let reg = if def.registration_mode == 1 {
                String::new()
            } else {
                def.chosen_plate_of_number(&n)
            };
            (n, reg)
        })
        .collect()
}
