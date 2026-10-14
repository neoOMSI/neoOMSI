use super::*;

impl Player {
    pub(crate) fn tick_ibis(&mut self, dt: f32) {
        let Some((mut typist, line, terminus, stops)) = self.ibis_typist.take() else {
            return;
        };
        if typist.tick(&mut self.vehicle, dt) {
            self.ibis_typist = Some((typist, line, terminus, stops));
            return;
        }
        match typist.outcome() {
            Some(Ok(what)) => log::info!("IBIS set to line {line} terminus {terminus}: {what}"),
            other => {
                log::info!(
                    "IBIS: {}; line {line} terminus {terminus} set directly",
                    other
                        .and_then(|o| o.as_ref().err())
                        .cloned()
                        .unwrap_or_default()
                );
                let hof = self.vehicle.host.hof.clone();
                let stops: Vec<&str> = stops.iter().map(String::as_str).collect();
                schedule::set_player_destination_directly(
                    &mut self.vehicle,
                    hof.as_deref(),
                    &line,
                    &terminus,
                    &stops,
                );
            }
        }
    }

    pub(crate) fn ibis_to_stop(&mut self, trip: &schedule::PlannedTrip, stop: usize) {
        let Some(hof) = self.vehicle.host.hof.clone() else {
            return;
        };
        let Some(route) = self.vehicle.var("IBIS_RouteIndex").filter(|r| *r >= 0.0) else {
            return;
        };
        if self.vehicle.var("IBIS_busstop").is_none() {
            return;
        }
        let Some(name) = trip.stops.get(stop).map(|s| s.name.clone()) else {
            return;
        };
        if let Some(i) = schedule::ibis_stop_index(&hof, route.round() as usize, &name, stop) {
            self.vehicle.set_var("IBIS_busstop", i as f32);
        }
    }

    pub(crate) fn set_duty_destination(&mut self, trip: &schedule::PlannedTrip, stop: usize) {
        self.duty_typed = true;
        let stops: Vec<String> = trip.stops.iter().map(|s| s.name.clone()).collect();
        let name = trip
            .stops
            .get(stop)
            .map(|s| s.name.clone())
            .unwrap_or_default();
        if self.startup.is_some() {
            self.ibis_duty = Some((
                trip.line.clone(),
                trip.terminus.clone(),
                stops,
                (stop, name),
            ));
        } else {
            self.type_destination(&trip.line, &trip.terminus, &stops, (stop, &name));
        }
    }

    pub(crate) fn type_destination(
        &mut self,
        line: &str,
        terminus: &str,
        stops: &[String],
        stop: (usize, &str),
    ) {
        if let Some((mut old, ..)) = self.ibis_typist.take() {
            old.abandon(&mut self.vehicle);
        }
        let hof = self.vehicle.host.hof.clone();
        let mut keys: hashbrown::HashSet<String> = self
            .vehicle
            .ty
            .model
            .meshes
            .iter()
            .filter_map(|m| m.mouse_event.as_ref())
            .map(|e| e.trim().to_ascii_lowercase())
            .collect();
        keys.extend(
            self.bound_actions()
                .iter()
                .map(|a| a.trim().to_ascii_lowercase()),
        );
        let operable = |name: &str| keys.contains(&name.trim().to_ascii_lowercase());
        let names: Vec<&str> = stops.iter().map(String::as_str).collect();
        match schedule::player_ibis(
            &mut self.vehicle,
            hof.as_deref(),
            line,
            terminus,
            &names,
            Some(stop),
            &operable,
            self.ibis_background,
        ) {
            Some(typist) => {
                self.ibis_typist = Some((
                    typist,
                    line.to_string(),
                    terminus.to_string(),
                    stops.to_vec(),
                ))
            }
            None => log::info!("IBIS: nothing to type for line {line} terminus {terminus}"),
        }
    }

    pub(crate) fn apply_html_requests(&mut self) {
        let requests = self.vehicle.take_html_requests();
        if requests.is_empty() {
            return;
        }
        let (stops, requests): (Vec<_>, Vec<_>) = requests
            .into_iter()
            .partition(|r| matches!(r, ::simulation::htmltex::HtmlRequest::SetNextStop(_)));
        let (direct, requests): (Vec<_>, Vec<_>) = requests.into_iter().partition(|r| {
            matches!(
                r,
                ::simulation::htmltex::HtmlRequest::PlaySound { .. }
                    | ::simulation::htmltex::HtmlRequest::FireEvent(_)
            )
        });
        for r in direct {
            match r {
                ::simulation::htmltex::HtmlRequest::PlaySound { file, volume } => {
                    log::info!("HTML page: playSound {file} ({volume})");
                    self.vehicle.host.html_sounds.push((file, volume));
                }
                ::simulation::htmltex::HtmlRequest::FireEvent(name) => {
                    log::info!("HTML page: fireEvent {name}");
                    self.vehicle
                        .host
                        .fired_sounds
                        .push(::simulation::host::FiredSound::Trigger { name });
                }
                _ => {}
            }
        }
        if let Some(::simulation::htmltex::HtmlRequest::SetNextStop(i)) = stops.last() {
            log::info!("HTML page: setNextStop({i}) received");
            self.html_next_stop = Some(*i);
        }
        if requests.is_empty() {
            return;
        }
        let Some(hof) = self.vehicle.host.hof.clone() else {
            log::info!("HTML page: the bus has no depot file, {requests:?} ignored");
            return;
        };
        for req in requests {
            match req {
                ::simulation::htmltex::HtmlRequest::SetRoute(i) => self.set_route_from_page(&hof, i),
                ::simulation::htmltex::HtmlRequest::SetLine(line) => {
                    let wanted = line.trim();
                    let digits: String =
                        wanted.chars().take_while(|c| c.is_ascii_digit()).collect();
                    let found = hof
                        .info_trips
                        .iter()
                        .position(|t| route_line(t).eq_ignore_ascii_case(wanted))
                        .or_else(|| {
                            hof.info_trips
                                .iter()
                                .position(|t| !digits.is_empty() && route_line(t) == digits)
                        });
                    match found {
                        Some(i) => self.set_route_from_page(&hof, i),
                        None => {
                            log::info!("HTML page: depot file {} has no line '{wanted}'", hof.name)
                        }
                    }
                }
                ::simulation::htmltex::HtmlRequest::SetNextStop(_) => {}
                ::simulation::htmltex::HtmlRequest::PlaySound { .. }
                | ::simulation::htmltex::HtmlRequest::FireEvent(_) => {}
                ::simulation::htmltex::HtmlRequest::PlayAnnouncement {
                    route,
                    stop,
                    terminus,
                } => {
                    let Some(ident) = hof
                        .info_busstop_lists
                        .get(route)
                        .and_then(|l| l.get(stop))
                        .map(|s| s.split('#').next().unwrap_or("").trim().to_string())
                        .filter(|s| !s.is_empty())
                    else {
                        log::info!(
                            "HTML page: depot file {} has no stop {stop} on route {route}",
                            hof.name
                        );
                        continue;
                    };
                    let folder = hof
                        .global_strings
                        .first()
                        .map(|s| s.trim().to_string())
                        .unwrap_or_default();
                    let file = format!(
                        "..\\..\\Announcements\\{folder}\\{ident}{}.wav",
                        if terminus { "_#terminus" } else { "" }
                    );
                    log::info!("HTML page: announcement {file}");
                    self.vehicle
                        .host
                        .fired_sounds
                        .push(::simulation::host::FiredSound::File {
                            name: "ev_IBIS_Ansagen".to_string(),
                            file,
                        });
                }
                ::simulation::htmltex::HtmlRequest::ClearLine => {
                    if let Some((mut old, ..)) = self.ibis_typist.take() {
                        old.abandon(&mut self.vehicle);
                    }
                    for n in ["IBIS_LinieKurs", "IBIS_Linie_Complex", "IBIS_Linie_Suffix"] {
                        self.vehicle.set_var(n, 0.0);
                    }
                    self.vehicle.set_var("IBIS_RouteIndex", -1.0);
                    if let Some(i) = self.vehicle.ty.program.str_var("IBIS_Complex_Line") {
                        self.vehicle.state.str_vars[i as usize] = "     ".into();
                    }
                    if let Some(i) = self.vehicle.ty.program.str_var("SetLineTo") {
                        self.vehicle.state.str_vars[i as usize] = String::new();
                    }
                    log::info!("HTML page: line cleared");
                }
                ::simulation::htmltex::HtmlRequest::SetDestination(ti) => {
                    let Some(term) = hof.termini.get(ti) else {
                        log::info!("HTML page: depot file {} has no destination {ti}", hof.name);
                        continue;
                    };
                    let line = {
                        let l = self.vehicle.host.tt_line.trim().to_string();
                        if !l.is_empty() {
                            l
                        } else {
                            let c = self.vehicle.str_var("IBIS_Complex_Line").trim().to_string();
                            if !c.is_empty() {
                                c
                            } else {
                                self.vehicle
                                    .var("IBIS_LinieKurs")
                                    .filter(|n| *n > 0.5)
                                    .map(|n| format!("{}", n.round() as i64))
                                    .unwrap_or_default()
                            }
                        }
                    };
                    let wanted = if term.texture_id.trim().is_empty() {
                        term.strings.first().cloned().unwrap_or_default()
                    } else {
                        term.texture_id.clone()
                    };
                    log::info!("HTML page: destination {ti} '{wanted}' on line '{line}'");
                    if let Some((mut old, ..)) = self.ibis_typist.take() {
                        old.abandon(&mut self.vehicle);
                    }
                    schedule::set_player_destination_directly(
                        &mut self.vehicle,
                        Some(&hof),
                        &line,
                        &wanted,
                        &[],
                    );
                }
            }
        }
    }

    pub(super) fn set_route_from_page(&mut self, hof: &::legacy_vehicle::hof::Hof, i: usize) {
        let Some(t) = hof.info_trips.get(i) else {
            log::info!("HTML page: depot file {} has no route {i}", hof.name);
            return;
        };
        let code = ::legacy_config::parse_i32(&t.route);
        let Some(term) = hof.termini.iter().find(|x| x.code == code) else {
            log::info!(
                "HTML page: route {i} ({}) leads to destination code {code}, which the depot file lacks",
                t.name.trim()
            );
            return;
        };
        let stops: Vec<String> = hof
            .info_busstop_lists
            .get(i)
            .map(|l| l.iter().map(|s| s.trim().to_string()).collect())
            .unwrap_or_default();
        let name = stops.first().cloned().unwrap_or_default();
        let wanted = if term.texture_id.trim().is_empty() {
            term.strings.first().cloned().unwrap_or_default()
        } else {
            term.texture_id.clone()
        };
        let line = route_line(t);
        log::info!(
            "HTML page: route {i} '{}' (line '{line}' (file: '{}') to '{wanted}')",
            t.name.trim(),
            t.line.trim()
        );
        self.type_destination(&line, &wanted, &stops, (0, &name));
    }
}

pub(crate) fn route_line(t: &::legacy_vehicle::hof::InfoTrip) -> String {
    let raw = t.line.trim();
    let code = ::legacy_config::parse_i32(&t.code);
    if (raw.is_empty() || raw.chars().all(|c| c == 'x' || c == 'X')) && code >= 100 {
        (code / 100).to_string()
    } else {
        raw.to_string()
    }
}
