//! Doing a chosen line of a list and changing settings from the options windows.

use super::*;

pub(crate) fn run(app: &mut App, kind: &ListKind, action: &str) -> Option<ListKind> {
    run_move(app, kind, action, Move::Next)
}

pub(crate) fn run_move(app: &mut App, kind: &ListKind, action: &str, mv: Move) -> Option<ListKind> {
    if action == "back" {
        return match kind {
            ListKind::Tours(..) => Some(ListKind::Lines),
            ListKind::PlaceType(_) | ListKind::PlaceLivery(_) | ListKind::PlaceHof(..) => {
                Some(ListKind::PlaceMaker)
            }
            _ => None,
        };
    }
    let (verb, arg) = action.split_once(' ').unwrap_or((action, ""));
    match kind {
        ListKind::Admin => {
            crate::admin::run(app, action);
            Some(ListKind::Admin)
        }
        ListKind::Options(_) | ListKind::World(_) => {
            if verb == "noop" || option_do(app, verb, arg, mv) {
                return Some(kind.clone());
            }
            let step = matches!(mv, Move::Next);
            match verb {
                "vr_nav_edit" if step && app.vr_active() && app.player.is_some() => {
                    app.start_vr_nav_edit();
                    return None;
                }
                "vr_nav_reset" if step && app.vr_active() && app.player.is_some() => {
                    app.vr_nav_adjust("reset", 1.0);
                    LIST_DIRTY.store(true, std::sync::atomic::Ordering::Relaxed);
                }
                // (the preset, the clouds and the precipitation are picked from a drop-down: `App::chooser_pick`)
                "weather" | "cloudkind" | "precipkind" | "metar_src" | "sel" | "preset"
                | "gfxprofile" | "reset" => {}
                "mapopts" => {
                    if step {
                        return Some(ListKind::Options(MAP_TAB));
                    }
                    let m = if let Move::To(_) = mv { Move::Next } else { mv };
                    option_do(app, "navigator", "", m);
                }
                "lookopts" => {
                    if step {
                        return Some(ListKind::Options(LOOK_TAB));
                    }
                    let m = if let Move::To(_) = mv { Move::Next } else { mv };
                    option_do(app, "free_look", "", m);
                }
                v if v.starts_with("pad_") => crate::lab_pads::click(app, v, arg),
                "keysearch" => {}
                "keybind" if matches!(mv, Move::Dec | Move::Inc) => {
                    let mut it = arg.splitn(3, ' ');
                    if let (Some(sec), Some(idx), Some(name)) = (
                        it.next().and_then(|x| x.parse::<usize>().ok()),
                        it.next().and_then(|x| x.parse::<usize>().ok()),
                        it.next(),
                    ) {
                        let name = name.to_string();
                        app.keybind_edit(sec, idx, &name, crate::game_menu::KeyEdit::Clear);
                    }
                }
                "keysopts" if step => return Some(ListKind::Options(KEYS_TAB)),
                "mapback" if step => return Some(ListKind::Options(0)),
                "metar_icao_edit" if step => {
                    if app.menu_edit_icao {
                        app.apply_icao_edit();
                    } else {
                        app.start_icao_edit();
                    }
                }
                "time_edit" if step => {
                    if app.menu_edit.is_some() {
                        app.apply_time_edit();
                    } else if app
                        .lan
                        .as_ref()
                        .is_some_and(|l| l.role == ::network::Role::Client)
                    {
                        app.service_msg =
                            Some(("In a LAN session the host sets the clock".into(), 3.0));
                    } else if app.real_time_locked() {
                        app.service_msg = Some((
                            "The time cannot be changed while the real-time sync is on".into(),
                            3.0,
                        ));
                    } else {
                        app.menu_edit = Some(String::new());
                    }
                }
                "keybind" if step => {
                    let mut it = arg.split(' ');
                    if let (Some(sec), Some(idx)) = (
                        it.next().and_then(|x| x.parse::<usize>().ok()),
                        it.next().and_then(|x| x.parse::<usize>().ok()),
                    ) {
                        app.key_capture = Some((sec, idx));
                    }
                }
                "seat_reset" if step => {
                    for k in ["seat_x", "seat_y", "seat_z"] {
                        ::config::set_setting("camera", k, 0.0_f64);
                        let _ = ::config::save();
                    }
                }
                "clock_ontime" if step => {
                    if app
                        .lan
                        .as_ref()
                        .is_some_and(|l| l.role == ::network::Role::Client)
                    {
                        app.service_msg =
                            Some(("In a LAN session the host sets the clock".into(), 3.0));
                    } else if let Some(d) = app
                        .player
                        .as_ref()
                        .map(|p| p.vehicle.host.tt_delay as f64)
                        .filter(|d| d.abs() >= 1.0)
                    {
                        // (the delay as it is now, not as the button was drawn: a second click
                        // would otherwise move the clock by the old amount again)
                        app.shift_clock(-d);
                        if let Some(p) = app.player.as_mut() {
                            p.vehicle.host.tt_delay = 0.0;
                        }
                    }
                }
                "clock_set" | "clock_shift" if step => {
                    if app
                        .lan
                        .as_ref()
                        .is_some_and(|l| l.role == ::network::Role::Client)
                    {
                        app.service_msg =
                            Some(("In a LAN session the host sets the clock".into(), 3.0));
                    } else if let Ok(secs) = arg.trim().parse::<f64>() {
                        let by = if verb == "clock_set" {
                            secs - app.clock.time
                        } else {
                            secs
                        };
                        app.shift_clock(by);
                    }
                }
                other if step => {
                    app.page_action(other);
                    return None;
                }
                _ => {}
            }
            Some(kind.clone())
        }
        ListKind::Vehicle(_) => {
            if matches!(mv, Move::Next) {
                app.page_action(verb);
                return None;
            }
            Some(kind.clone())
        }
        ListKind::Lines => match verb {
            "line" => Some(ListKind::Tours(arg.to_string(), None)),
            "free" => {
                app.duty = None;
                if let Some(p) = app.player.as_mut() {
                    crate::schedule_paper::clear_vehicle(&mut p.vehicle);
                }
                app.service_msg = Some((::i18n::translate("pause.msg.free_drive", &[]), 4.0));
                None
            }
            _ => None,
        },
        ListKind::Tours(_, pick) => {
            if let Some((line, tour)) = arg.split_once('\u{1}') {
                let chosen = pick
                    .as_ref()
                    .filter(|p| p.0 == tour)
                    .map(|p| p.1)
                    .unwrap_or(0);
                let trip = pick
                    .as_ref()
                    .filter(|p| p.0 == tour)
                    .map(|p| p.2)
                    .unwrap_or_else(|| {
                        app.schedule
                            .as_ref()
                            .map(|s| s.tour_trip_now(line, tour, app.clock.time))
                            .unwrap_or(0)
                    });
                start_duty_at(app, line, tour, trip, chosen);
            }
            None
        }
        ListKind::Drivers => {
            switch_driver(app, arg);
            Some(ListKind::Drivers)
        }
        ListKind::Hofs => {
            if let Some(p) = app.player.as_mut() {
                match ::legacy_vehicle::Hof::load(std::path::Path::new(arg)) {
                    Ok(h) => {
                        let name = h.name.clone();
                        p.vehicle.host.hof = Some(std::sync::Arc::new(h));
                        app.service_msg = Some((::i18n::translate("pause.msg.depot_file", &[("name", &name.trim())]), 3.0));
                    }
                    Err(e) => app.service_msg = Some((::i18n::translate("pause.msg.depot_file", &[("name", &e)]), 4.0)),
                }
            }
            None
        }
        ListKind::Spots => {
            if app
                .lan
                .as_ref()
                .is_some_and(|l| l.role == ::network::Role::Client)
            {
                app.service_msg = Some((
                    "In a LAN session only the host moves vehicles on the map".into(),
                    4.0,
                ));
                return None;
            }
            let found = app.world.clone().and_then(|w| {
                let ep = arg
                    .trim()
                    .parse::<usize>()
                    .ok()
                    .and_then(|i| w.global.entry_points.get(i))?;
                // (the entry points of tiles that are not loaded come from the map index)
                w.index();
                w.entry_point_place(ep).map(|(pos, rot)| (pos, rot[0]))
            });
            match found {
                Some((pos, heading)) => {
                    crate::admin::teleport(app, pos, heading);
                    app.service_msg = Some((::i18n::translate("pause.msg.start_point_reached", &[]), 3.0));
                }
                None => app.service_msg = Some((::i18n::translate("pause.msg.start_point_missing", &[]), 3.0)),
            }
            None
        }
        ListKind::PlaceMaker | ListKind::PlaceType(_) if verb == "maker" => {
            Some(ListKind::PlaceType(arg.to_string()))
        }
        ListKind::PlaceMaker | ListKind::PlaceType(_) => {
            Some(ListKind::PlaceLivery(arg.to_string()))
        }
        ListKind::PlaceLivery(bus) => Some(ListKind::PlaceHof(bus.clone(), arg.to_string())),
        ListKind::PlaceHof(bus, paint) => {
            let (bus, paint, hof) = (bus.clone(), paint.clone(), arg.trim().to_string());
            app.close_game_menu();
            app.place_vehicle(
                &bus,
                Some(paint).filter(|p| !p.is_empty()),
                Some(hof).filter(|h| !h.is_empty()),
            );
            None
        }
        ListKind::Destinations if verb == "routes" => Some(ListKind::RouteNumbers),
        ListKind::RouteNumbers if verb == "route_type" => match app.menu_edit.take() {
            Some(t) => {
                set_route_by_hand(app, &t);
                None
            }
            None => {
                app.menu_edit = Some(String::new());
                Some(ListKind::RouteNumbers)
            }
        },
        ListKind::RouteNumbers => {
            set_route_by_hand(app, arg);
            None
        }
        ListKind::Destinations => {
            if let Some(p) = app.player.as_mut() {
                let hof = p.vehicle.host.hof.clone();
                let code: i32 = arg.trim().parse().unwrap_or(-1);
                if let Some(t) = hof
                    .as_ref()
                    .and_then(|h| h.termini.iter().find(|t| t.code == code))
                {
                    let line = p
                        .vehicle
                        .var("IBIS_LinieKurs")
                        .filter(|l| *l > 0.0)
                        .map(|l| format!("{}", l as i64))
                        .unwrap_or_default();
                    let name = t.strings.first().cloned().unwrap_or_default();
                    crate::schedule::set_player_destination_directly(
                        &mut p.vehicle,
                        hof.as_deref(),
                        &line,
                        &name,
                        &[],
                    );
                    log::info!(
                        "destination display set by hand: {code} {} (terminus code now {:?})",
                        name.trim(),
                        p.vehicle.var("IBIS_TerminusCode")
                    );
                    app.service_msg = Some((::i18n::translate("pause.msg.destination", &[("name", &name.trim())]), 3.0));
                }
            }
            None
        }
        ListKind::Numbers => {
            if let (Some((n, reg)), Some(p)) = (arg.split_once('\u{1}'), app.player.as_mut()) {
                let v = &mut p.vehicle;
                if let Some(i) = v.ty.program.str_var("number") {
                    v.state.str_vars[i as usize] = n.to_string();
                }
                if !reg.is_empty() {
                    if let Some(i) = v.ty.program.str_var("ident") {
                        v.state.str_vars[i as usize] = reg.to_string();
                    }
                }
                app.service_msg = Some((::i18n::translate("pause.msg.fleet_number", &[("number", &n)]), 3.0));
            }
            None
        }
    }
}

pub(super) fn option_do(app: &mut App, verb: &str, arg: &str, mv: Move) -> bool {
    // (the weather is the METAR report's while the sync is on)
    if app.metar_locked()
        && matches!(
            verb,
            "visibility"
                | "rain_amt"
                | "wet"
                | "brightness"
                | "humidity"
                | "temp"
                | "wind_speed"
                | "wind_dir"
                | "snow_cover"
                | "snow_road"
        )
    {
        app.service_msg = Some((
            "The weather cannot be changed while the METAR sync is on".into(),
            3.0,
        ));
        return true;
    }
    if let Some(cur) = toggle_now(app, verb) {
        let on = match mv {
            Move::Next => !cur,
            Move::Inc => true,
            Move::Dec => false,
            Move::To(f) => f >= 0.5,
        };
        if on != cur {
            if let Some((k, v)) = toggle_set(app, verb, on) {
                remember_setting(k, &v);
            }
            sync_live(app);
            LIST_DIRTY.store(true, std::sync::atomic::Ordering::Relaxed);
        }
        return true;
    }
    if let Some(steps) = steps_of(verb) {
        if let Some(now) = option_now(app, verb, arg) {
            let to = step_move(&steps, now, mv);
            // (a slider dragged sends the same value many times over)
            if (to - now).abs() > 1e-6 {
                if let Some((k, v)) = option_set(app, verb, arg, to) {
                    remember_setting(k, &v);
                }
                sync_live(app);
                LIST_DIRTY.store(true, std::sync::atomic::Ordering::Relaxed);
            }
        }
        return true;
    }
    false
}