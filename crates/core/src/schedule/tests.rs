//! Tests of the schedule.

use super::*;

#[test]
fn direct_player_destination_runs_the_scheduled_target_trigger() {
    let dir = std::env::temp_dir().join(format!(
        "omsi-direct-destination-trigger-{}-{:?}",
        std::process::id(),
        std::thread::current().id()
    ));
    std::fs::create_dir_all(&dir).unwrap();
    std::fs::write(
        dir.join("test.bus"),
        "[model]\nmodel.cfg\n[varnamelist]\n1\nvars.txt\n[script]\n1\nmain.osc\n",
    )
    .unwrap();
    std::fs::write(dir.join("model.cfg"), "").unwrap();
    std::fs::write(
        dir.join("vars.txt"),
        "destination_triggered\nAI_target_index\nIBIS_LinieKurs\nIBIS_Linie_Complex\nIBIS_Linie_Suffix\nIBIS_TerminusIndex\nIBIS_TerminusCode\nIBIS_RouteIndex\nIBIS_mode\n",
    )
    .unwrap();
    std::fs::write(
        dir.join("main.osc"),
        "{trigger:ai_scheduled_settarget}\n(L.L.IBIS_TerminusCode) (S.L.destination_triggered)\n{end}\n",
    )
    .unwrap();
    let ty = std::sync::Arc::new(
        ::simulation::VehicleType::load(&dir, &dir.join("test.bus")).unwrap(),
    );
    let mut vehicle =
        ::simulation::VehicleInstance::new(ty, ::simulation::VehicleHost::new(Default::default()));
    let hof = ::legacy_vehicle::Hof {
        termini: vec![
            ::legacy_vehicle::hof::Terminus {
                code: 1,
                texture_id: "Other".into(),
                terminus_stop: Some("Other".into()),
                all_exit: false,
                strings: vec![],
            },
            ::legacy_vehicle::hof::Terminus {
                code: 2,
                texture_id: "Terminus".into(),
                terminus_stop: Some("Terminus".into()),
                all_exit: false,
                strings: vec![],
            },
        ],
        ..Default::default()
    };

    set_player_destination_directly(&mut vehicle, Some(&hof), "1", "Terminus", &[]);

    // The trigger sees the state the normal direct-destination path writes, rather than
    // replacing that path by returning early.
    assert_eq!(vehicle.var("destination_triggered"), Some(2.0));
    assert_eq!(vehicle.var("IBIS_TerminusIndex"), Some(1.0));
    assert_eq!(vehicle.var("IBIS_TerminusCode"), Some(2.0));
    assert_eq!(vehicle.var("IBIS_LinieKurs"), Some(1.0));
    std::fs::remove_dir_all(dir).ok();
}

/// The row OMSI's AI bus is given: the first whose ident is the destination, whatever
/// the codes' order; of equally loose matches the first as well.
#[test]
fn a_stop_is_no_target_of_itself() {
    // a circular line: from A round to A; B has two platforms of one name
    let names = |id: i64| match id {
        1 => "A".to_string(),
        2 | 3 => "B".to_string(),
        _ => "C".to_string(),
    };
    let t = station_targets([(vec![1, 2, 4, 3, 1], "A".to_string())].into_iter(), names);
    let of = |id: i64| t[&id].iter().map(|x| x.0.as_str()).collect::<Vec<_>>();
    assert_eq!(of(1), ["B", "C"]);
    assert_eq!(of(2), ["C", "A"]);
    assert_eq!(of(4), ["B", "A"]);
    assert_eq!(of(3), ["A"]);
    assert!(t[&1].iter().all(|x| x.1.contains("A")));
}

#[test]
fn a_terminus_is_the_first_row_of_its_name() {
    let t = |code: i32, id: &str, s: &[&str]| ::legacy_vehicle::hof::Terminus {
        code,
        texture_id: id.into(),
        terminus_stop: Some(id.into()),
        all_exit: false,
        strings: s.iter().map(|x| x.to_string()).collect(),
    };
    let hof = ::legacy_vehicle::Hof {
        termini: vec![
            t(0, "Depot", &[]),
            t(3, "61-Other", &["61-MaoFangChang"]),
            t(4, "61-Third", &[]),
            t(1, "61-MaoFangChang", &["61-MaoFangChang"]),
            t(2, "Wickenberg Nord", &[]),
        ],
        ..Default::default()
    };
    assert_eq!(super::find_terminus(&hof, "61-MaoFangChang"), Some(3));
    assert_eq!(super::find_terminus(&hof, "  61-MaoFangChang "), Some(3));
    let hof = ::legacy_vehicle::Hof {
        termini: vec![t(0, "A", &["Wickenberg"]), t(1, "B", &["Wickenberg"])],
        ..Default::default()
    };
    assert_eq!(super::find_terminus(&hof, "wickenberg"), Some(0));
}

#[test]
fn complex_line_keeps_letter_suffix() {
    assert_eq!(complex_line_text("5", 5.0), "005  ");
    assert_eq!(complex_line_text("5E", 5.0), "   5E");
    assert_eq!(line_suffix_from_text("5E"), 10);
    assert_eq!(line_suffix_from_text("5N"), 4);
    assert_eq!(line_suffix_from_text("5S"), 23);
    assert_eq!(line_code_from_text("5E", Some(505)), Some(510));
    assert_eq!(line_code_from_text("5", Some(505)), Some(500));
}

/// #546: a letter-first line had no number, and the DL05's matrix blanks line 0.
#[test]
fn line_with_letter_prefix_keeps_its_number() {
    assert_eq!(line_code_from_text("X10", None), Some(1036));
    assert_eq!(line_code_from_text("X10", Some(51001)), Some(51036));
    assert_eq!(line_code_from_text("M41", Some(4101)), Some(4128));
    assert_eq!(line_code_from_text("N9", None), Some(935));
    assert_eq!(line_code_from_text("TML", Some(7601)), Some(7601));
    assert_eq!(line_suffix_from_text("X10"), 36);
    assert_eq!(line_number_digits("X10"), "10");
    assert_eq!(line_number_digits("5E"), "5");
}

#[test]
fn berlin_5e_uses_its_real_terminus_when_no_hof_route_exists() {
    let path = std::path::Path::new("../../../OMSI 2 Original/Vehicles/MAN_SD202/Berlin.hof");
    let Ok(hof) = ::legacy_vehicle::Hof::load(path) else {
        return;
    };
    let target = ibis_target(&hof, "5E", "Fernbahnhof Spandau", &[], None).expect("5E target");
    assert_eq!(target.terminus_code, Some(232));
    assert_eq!(
        target.terminus_index,
        hof.termini.iter().position(|t| t.code == 232).unwrap() as i32
    );
    let target = ibis_target(&hof, "5E", "Spektefeld Schulzentrum", &[], None)
        .expect("5E shortened HOF target");
    assert_eq!(target.terminus_code, Some(233));
}

#[test]
fn berlin_5e_does_not_turn_hof_route_505_into_s5() {
    let path =
        std::path::Path::new("../../../OMSI 2 Original/Vehicles/MAN_NL_NG/Spandau 89-11.hof");
    let Ok(hof) = ::legacy_vehicle::Hof::load(path) else {
        return;
    };
    let target = ibis_target(&hof, "5E", "Nervenklinik", &["U Rathaus Spandau"], None)
        .expect("5E route target");
    assert_eq!(target.route, Some(3));
    assert_eq!(target.suffix, 10);
}

#[test]
fn stop_names_meet_in_any_order_and_spelling() {
    assert_eq!(stop_words("Nordstadt Bhf"), stop_words("Bhf. Nordstadt"));
    assert_eq!(stop_words("F_Kirchweg"), stop_words("Kirchweg"));
    assert_ne!(stop_words("Bhf Nordstadt"), stop_words("Nordstadt"));
    assert!(stop_words("").is_empty());
}

/// A made-up line 7 with six routes to Hafen, each telling one rule of `pick_route`.
fn hafen_depot() -> ::legacy_vehicle::Hof {
    let mut hof = ::legacy_vehicle::Hof {
        termini: vec![::legacy_vehicle::hof::Terminus {
            code: 100,
            strings: vec!["Hafen".into()],
            ..Default::default()
        }],
        ..Default::default()
    };
    let routes: [(&str, &[&str]); 6] = [
        ("707", &["Markt", "Schule", "Park", "Ufer", "Hafen"]),
        ("701", &["Markt", "Schule", "Park", "Hafen"]),
        (
            "702",
            &[
                "Bhf Nordstadt",
                "F_Kirchweg",
                "Markt",
                "Schule",
                "Park",
                "Hafen",
            ],
        ),
        ("703", &["Schule", "Park", "Hafen"]),
        (
            "705",
            &[
                "Bhf Nordstadt",
                "Markt",
                "Rathaus",
                "Schule",
                "Park",
                "Hafen",
            ],
        ),
        (
            "706",
            &["Am Wald", "Kirchweg", "Markt", "Schule", "Park", "Hafen"],
        ),
    ];
    for (code, stops) in routes {
        hof.info_trips.push(::legacy_vehicle::hof::InfoTrip {
            code: code.into(),
            route: "100".into(),
            line: "7".into(),
            ..Default::default()
        });
        hof.info_busstop_lists
            .push(stops.iter().map(|s| s.to_string()).collect());
    }
    hof
}

#[test]
fn the_route_follows_the_trips_stops() {
    let hof = hafen_depot();
    let route = |stops: &[&str]| {
        ibis_target(&hof, "7", "Hafen", stops, None)
            .expect("line 7 target")
            .route
    };
    // the depot file spells the first stop another way, and with a one-letter prefix
    assert_eq!(
        route(&[
            "Nordstadt Bhf",
            "Kirchweg",
            "Markt",
            "Schule",
            "Park",
            "Hafen"
        ]),
        Some(2)
    );
    // a short working gets its own route, not the long ones it is part of
    assert_eq!(route(&["Schule", "Park", "Hafen"]), Some(3));
    // of two routes from the trip's first stop, the one as long as the trip
    assert_eq!(route(&["Markt", "Schule", "Park", "Hafen"]), Some(1));
    // starting at the trip's first stop counts before one stop more of the trip: 705
    // has Rathaus too but begins at Bhf Nordstadt, so a route from Markt (707, as
    // long as the trip) is taken
    assert_eq!(
        route(&["Markt", "Rathaus", "Schule", "Park", "Hafen"]),
        Some(7)
    );
    // nothing known of the trip: the first route, as before
    assert_eq!(route(&[]), Some(7));
}

#[test]
fn the_ibis_stands_at_the_trips_first_stop_on_a_route_that_begins_before_it() {
    let hof = hafen_depot();
    // no route begins at Kirchweg; 702 and 706 have the trip's stops after one more
    let stops = ["Kirchweg", "Markt", "Schule", "Park", "Hafen"];
    let target =
        ibis_target(&hof, "7", "Hafen", &stops, Some((0, stops[0]))).expect("line 7 target");
    assert_eq!(target.route, Some(2));
    assert_eq!(target.stop, 1);
}

#[test]
fn where_a_bus_is_on_a_partly_loaded_route() {
    let key = |id: i64| {
        Some(LaneKey {
            tile: (0, 0),
            id,
            path: 0,
        })
    };
    let legs = [0, 0, 1, 1, 1, 2];
    let steps: Vec<Step> = legs
        .iter()
        .enumerate()
        .map(|(i, &leg)| Step {
            key: key(i as i64),
            leg,
            length: 0.0,
        })
        .collect();
    let slots = [
        Slot::Lane(10),
        Slot::Lane(11),
        Slot::Lane(12),
        Slot::Waiting,
        Slot::Lane(14),
        Slot::Absent,
    ];
    let est = [100.0, 100.0, 50.0, 70.0, 50.0, 0.0];
    // a layover bus stands at the start
    assert_eq!(step_at(&steps, &slots, &est, 0, 0.0), Some((0, 0.0)));
    // 17 m into leg 1: on its first lane, whose part of the route ends at the gap
    let (at, off) = step_at(&steps, &slots, &est, 1, 0.1).unwrap();
    assert!(at == 2 && (off - 17.0).abs() < 1e-9, "{at} {off}");
    assert_eq!(section_around(&slots, 2), (0, 3));
    // half way: on the step still to come - the bus has to wait
    assert_eq!(step_at(&steps, &slots, &est, 1, 0.5), Some((3, 35.0)));
    // near the end of the leg: after the gap
    let (at, off) = step_at(&steps, &slots, &est, 1, 0.9).unwrap();
    assert_eq!(at, 4);
    assert!((off - 33.0).abs() < 1e-9);
    assert_eq!(section_around(&slots, 4), (4, 6));
    // a leg of absent steps only, and a leg without steps: past the end
    assert_eq!(step_at(&steps, &slots, &est, 2, 0.5), None);
    assert_eq!(step_at(&steps, &slots, &est, 3, 0.5), None);
    // a leg without a station link: at the start of the next leg
    let steps2: Vec<Step> = [0, 2, 2]
        .iter()
        .enumerate()
        .map(|(i, &leg)| Step {
            key: key(i as i64),
            leg,
            length: 0.0,
        })
        .collect();
    let slots2 = [Slot::Lane(1), Slot::Absent, Slot::Lane(3)];
    assert_eq!(
        step_at(&steps2, &slots2, &[10.0, 0.0, 10.0], 1, 0.5),
        Some((2, 0.0))
    );
    assert_eq!(section_around(&slots2, 2), (0, 3));
}

fn link(a: i64, b: i64) -> Option<f64> {
    // 100 m between neighbours, 300 m from 3 to 4
    Some(if (a, b) == (3, 4) { 300.0 } else { 100.0 })
}

#[test]
fn trip_times_from_the_profile() {
    let stations = [1, 2, 3, 4, 5];
    // no manual times: the duration split by the link lengths
    let p = ::timetable::TripProfile {
        name: "p".into(),
        factor: 10.0,
        ..Default::default()
    };
    let t = TripTimes::new(&stations, Some(&p), &link);
    let arr: Vec<f64> = t.stations.iter().map(|s| s.0).collect();
    assert_eq!(arr, vec![0.0, 100.0, 200.0, 500.0, 600.0]);
    assert_eq!(t.duration, 600.0);
    // manual minutes win, the rest in between by length; a passed station stops nowhere
    let p = ::timetable::TripProfile {
        name: "p".into(),
        factor: 10.0,
        man_dep_time: vec![(0, 1.0), (1, 2.0)],
        man_arr_time: vec![(3, 6.0), (4, 9.0)],
        other_stopping: vec![(2, 2)],
    };
    let t = TripTimes::new(&stations, Some(&p), &link);
    assert_eq!(t.stations[0], (60.0, 60.0));
    assert_eq!(t.stations[1], (120.0, 120.0));
    // station 2 lies 100 m of the 400 m between the departure at 2 min and the arrival at 6
    assert!((t.stations[2].0 - 180.0).abs() < 1e-9, "{:?}", t.stations);
    assert_eq!(t.stations[3], (360.0, 360.0));
    assert_eq!(t.duration, 540.0);
    assert_eq!(t.stops, vec![true, true, false, true, true]);
    // a profile without stations keeps its duration (flights on a track)
    assert_eq!(
        TripTimes::new(
            &[],
            Some(&::timetable::TripProfile {
                factor: 25.0,
                ..Default::default()
            }),
            &link
        )
            .duration,
        1500.0
    );
}

/// An X10 Berlin S-Bahn trip (S7_Tgn_Sav: from the map's edge, Zoo at 1.5 min) comes at
/// its departure time; a bus or a track beginning at a stop has its layover (#342).
#[test]
fn a_train_from_the_maps_edge_comes_at_its_departure() {
    let s7 = ::timetable::TripProfile {
        name: "hvz".into(),
        factor: 3.5,
        man_arr_time: vec![(0, 1.5)],
        man_dep_time: vec![(0, 2.0)],
        ..Default::default()
    };
    let t = TripTimes::new(&[10654593], Some(&s7), &link);
    assert_eq!(t.stations[0], (90.0, 120.0));
    assert!(spawn::comes_at_departure(true, &t));
    assert!(!spawn::comes_at_departure(false, &t));
    let plain = ::timetable::TripProfile {
        name: "p".into(),
        factor: 3.5,
        ..Default::default()
    };
    let from_stop = TripTimes::new(&[1, 2], Some(&plain), &link);
    assert_eq!(from_stop.stations[0].0, 0.0);
    assert!(!spawn::comes_at_departure(true, &from_stop));
}

fn planned(departure: f64, stops: &[(f64, f64, f64)]) -> PlannedTrip {
    let stops: Vec<PlannedStop> = stops
        .iter()
        .enumerate()
        .map(|(i, &(x, arr, dep))| PlannedStop {
            object_id: i as i64,
            name: format!("s{i}"),
            arr,
            dep,
            position: Some(glam::DVec3::new(x, 0.0, 0.0)),
            dir: StopDir::default(),
            stops: true,
        })
        .collect();
    PlannedTrip {
        name: format!("t{departure}"),
        line: "5".into(),
        terminus: "T".into(),
        departure,
        end: stops.last().unwrap().arr,
        stops,
    }
}

#[test]
fn terminus_index_is_the_depot_terminus_of_that_name() {
    let mut hof = ::legacy_vehicle::hof::Hof::default();
    for name in ["A", "B", "C"] {
        hof.termini.push(::legacy_vehicle::hof::Terminus {
            texture_id: name.into(),
            ..Default::default()
        });
    }
    assert_eq!(tt_terminus_index(Some(&hof), "B"), 1);
    assert_eq!(tt_terminus_index(Some(&hof), "b"), -1);
    assert_eq!(tt_terminus_index(None, "B"), -1);
}

#[test]
fn a_page_can_go_back_to_an_earlier_stop() {
    let trip = planned(
        0.0,
        &[
            (0.0, 0.0, 0.0),
            (100.0, 60.0, 60.0),
            (500.0, 120.0, 120.0),
            (1000.0, 200.0, 200.0),
        ],
    );
    let mut d = PlayerDuty {
        line: "5".into(),
        tour: "1".into(),
        trips: vec![trip],
        trip_index: 0,
        first_trip: 0,
        statistics: Default::default(),
        completed_report: None,
        next_stop: 0,
        at_stop: false,
        arrived_late: None,
        done: false,
        left_late: None,
        held_back: false,
        odo: 0.0,
        last_pos: None,
        arrival_odo: 0.0,
        left_odo: 0.0,
        placed: true,
        trip_changed: false,
        picked: true,
        first_update: None,
        heading: 90.0,
    };
    assert!(d.skip_to(2));
    assert_eq!(d.next_stop, 2);
    // back one stop: due again
    assert!(d.skip_to(1));
    assert_eq!(d.next_stop, 1);
    // the stop it is already heading for: nothing changes
    assert!(!d.skip_to(1));
    // the bus stands at stop 2: the duty does not jump forward again by itself
    d.advance(glam::DVec3::new(500.0, 0.0, 0.0), 100.0);
    assert_eq!(d.next_stop, 1);
    // from the last stop (done) back reopens the trip, forwards does not
    d.skip_to(3);
    d.advance(glam::DVec3::new(1000.0, 0.0, 0.0), 200.0);
    assert!(d.done);
    assert!(!d.skip_to(3));
    assert!(d.skip_to(2));
    assert!(!d.done);
    assert_eq!(d.next_stop, 2);
}

#[test]
fn the_players_trip_after_midnight_is_on_the_board() {
    // #299: trip ended at stop 11 at 24:12, the next leaves there at 24:22, clock 0:14:11
    let h = |m: f64| 24.0 * 3600.0 + m * 60.0;
    let mut ended = planned(
        h(-4.0),
        &[(0.0, h(-4.0), h(-4.0)), (500.0, h(12.0), h(12.0))],
    );
    let mut next = planned(
        h(22.0),
        &[(500.0, h(22.0), h(22.0)), (0.0, h(30.0), h(30.0))],
    );
    ended.stops[0].object_id = 10;
    ended.stops[1].object_id = 11;
    next.stops[0].object_id = 11;
    next.stops[1].object_id = 10;
    let d = PlayerDuty {
        line: "5".into(),
        tour: "1".into(),
        trips: vec![ended, next],
        trip_index: 0,
        first_trip: 0,
        statistics: Default::default(),
        completed_report: None,
        next_stop: 1,
        at_stop: true,
        arrived_late: None,
        done: true,
        left_late: None,
        held_back: false,
        odo: 0.0,
        last_pos: None,
        arrival_odo: 0.0,
        left_odo: 0.0,
        placed: true,
        trip_changed: false,
        picked: false,
        first_update: None,
        heading: 0.0,
    };
    let now = 14.0 * 60.0 + 11.0;
    let at_11: Vec<f64> = d.board_visits(11, now).into_iter().map(|v| v.0).collect();
    // standing there, and leaving at 0:22
    assert_eq!(at_11, vec![now, 22.0 * 60.0]);
    // late: due now
    let late = 25.0 * 60.0;
    assert_eq!(d.board_visits(11, late)[1].0, late);
}

#[test]
fn a_loop_does_not_jump_to_the_stop_over_the_road() {
    // out along y = 0 to x = 1000, back along y = 12: stop 1 at x = 100 going out, stop 5
    // at x = 100 coming back, 12 m apart (#254)
    let mut trip = planned(
        0.0,
        &[
            (0.0, 0.0, 0.0),
            (100.0, 60.0, 60.0),
            (500.0, 120.0, 120.0),
            (1000.0, 200.0, 200.0),
            (500.0, 280.0, 280.0),
            (100.0, 340.0, 340.0),
            (0.0, 400.0, 400.0),
        ],
    );
    for (i, s) in trip.stops.iter_mut().enumerate() {
        if i >= 4 {
            s.position.as_mut().unwrap().y = 12.0;
        }
    }
    trip.set_dirs();
    let mut d = PlayerDuty {
        line: "5".into(),
        tour: "1".into(),
        trips: vec![trip],
        trip_index: 0,
        first_trip: 0,
        statistics: Default::default(),
        completed_report: None,
        next_stop: 0,
        at_stop: false,
        arrived_late: None,
        done: false,
        left_late: None,
        held_back: false,
        odo: 0.0,
        last_pos: None,
        arrival_odo: 0.0,
        left_odo: 0.0,
        placed: true,
        trip_changed: false,
        picked: true,
        first_update: None,
        heading: 90.0,
    };
    // at stop 0, then leaving east
    d.advance(glam::DVec3::new(0.0, 0.0, 0.0), 0.0);
    d.advance(glam::DVec3::new(60.0, 0.0, 0.0), 30.0);
    assert_eq!(d.next_stop, 1);
    // at stop 1 heading east: stop 5 (12 m away, the other way round) is not taken
    d.advance(glam::DVec3::new(100.0, 0.0, 0.0), 60.0);
    d.advance(glam::DVec3::new(140.0, 0.0, 0.0), 70.0);
    assert_eq!(
        d.next_stop, 2,
        "the duty goes on to stop 2, not over the road to stop 5"
    );
}

#[test]
fn passing_a_later_overlapping_stop_does_not_skip_the_trip() {
    // Leaving a terminal stand can pass through the service circle of a later stop on the
    // same route.  It is not a missed-stop recovery unless the bus has actually opened its
    // doors there.
    let trip = planned(
        0.0,
        &[
            (0.0, 0.0, 0.0),
            (100.0, 60.0, 60.0),
            (200.0, 120.0, 120.0),
            (300.0, 180.0, 180.0),
            (400.0, 240.0, 240.0),
        ],
    );
    let mut d = PlayerDuty {
        line: "5".into(),
        tour: "1".into(),
        trips: vec![trip],
        trip_index: 0,
        first_trip: 0,
        statistics: Default::default(),
        completed_report: None,
        next_stop: 1,
        at_stop: false,
        arrived_late: None,
        done: false,
        left_late: Some(0.0),
        held_back: false,
        odo: 200.0,
        last_pos: Some(glam::DVec3::new(0.0, 0.0, 0.0)),
        arrival_odo: 0.0,
        left_odo: 0.0,
        placed: true,
        trip_changed: false,
        picked: true,
        first_update: None,
        heading: 90.0,
    };

    d.advance_while_serving(glam::DVec3::new(300.0, 0.0, 0.0), 180.0, false);
    assert_eq!(
        d.next_stop, 1,
        "passing the overlapping stop must not skip ahead"
    );

    d.advance_while_serving(glam::DVec3::new(300.0, 0.0, 0.0), 180.0, true);
    assert_eq!(
        d.next_stop, 3,
        "serving the later stop may recover missed progress"
    );
}

#[test]
fn a_duty_starts_with_the_trip_that_fits_the_time() {
    // Spandau line 5, tour "Mo-Fr 3": a depot run 14:44-15:01, then 15:07 and 16:01
    let trips = vec![
        planned(
            53040.0,
            &[(0.0, 53040.0, 53040.0), (500.0, 54060.0, 54060.0)],
        ),
        planned(
            54420.0,
            &[(500.0, 54420.0, 54420.0), (1000.0, 56400.0, 56400.0)],
        ),
        planned(
            57660.0,
            &[(1000.0, 57660.0, 57660.0), (500.0, 59400.0, 59400.0)],
        ),
    ];
    assert_eq!(
        starting_trip(&trips, 15.0 * 3600.0 + 300.0),
        1,
        "15:05: the 15:07"
    );
    assert_eq!(
        starting_trip(&trips, 14.0 * 3600.0 + 50.0 * 60.0),
        0,
        "14:50: the depot run under way"
    );
    assert_eq!(
        starting_trip(&trips, 15.0 * 3600.0 + 1800.0),
        1,
        "15:30: the 15:07 under way"
    );
    assert_eq!(
        starting_trip(&trips, 23.0 * 3600.0),
        2,
        "after the last: the last"
    );
    let now = 15.0 * 3600.0 + 300.0;
    let mut d = PlayerDuty {
        line: "5".into(),
        tour: "3".into(),
        trips,
        trip_index: 1,
        first_trip: 0,
        statistics: Default::default(),
        completed_report: None,
        next_stop: 0,
        at_stop: false,
        arrived_late: None,
        done: false,
        left_late: None,
        held_back: false,
        odo: 0.0,
        last_pos: None,
        arrival_odo: 0.0,
        left_odo: 0.0,
        placed: false,
        trip_changed: false,
        picked: false,
        first_update: None,
        heading: 0.0,
    };
    // 200 m from the first stop two minutes before the departure: early, next stop the first
    assert_eq!(d.advance(glam::DVec3::new(300.0, 0.0, 0.0), now), None);
    assert_eq!(d.next_stop, 0);
    assert!((d.delay(now) + 120.0).abs() < 1e-9);
    // at the first stop, leaving a minute late
    d.advance(glam::DVec3::new(500.0, 0.0, 0.0), now + 60.0);
    assert!(d.at_stop);
    assert_eq!(
        d.advance(glam::DVec3::new(560.0, 0.0, 0.0), 54480.0)
            .map(|(_, left)| left),
        Some(60.0)
    );
    assert_eq!(d.next_stop, 1);
    // on the way the delay is what it left with until the next stop is overdue
    assert!((d.delay(55000.0) - 60.0).abs() < 1e-9);
    assert!((d.delay(56600.0) - 200.0).abs() < 1e-9);
    // the next trip does not take over while this one is driven ...
    d.advance(glam::DVec3::new(800.0, 0.0, 0.0), 57620.0);
    assert_eq!(d.trip_index, 1);
    // ... but once its end is reached
    d.advance(glam::DVec3::new(1000.0, 0.0, 0.0), 57630.0);
    assert!(d.done && !d.take_trip_change());
    assert!(!d.duty_done(), "the following trip keeps the duty active");
    d.advance(glam::DVec3::new(1000.0, 0.0, 0.0), 57640.0);
    assert_eq!((d.trip_index, d.next_stop), (2, 0));
    assert!(d.take_trip_change());
    // The completed report survives the automatic change to the next trip.
    let report = d.take_completed_report().unwrap();
    assert_eq!(report.actual[0].arrival, Some(now + 60.0));
    assert_eq!(report.actual[0].departure, Some(54480.0));
    assert_eq!(report.actual[1].arrival, Some(57630.0));
    assert_eq!(report.actual[1].departure, None);
    assert!(d.take_completed_report().is_none());
    // The final trip, unlike the earlier layover, completes the duty when its last stop
    // is reached.
    d.advance(glam::DVec3::new(1000.0, 0.0, 0.0), 57660.0);
    d.advance(glam::DVec3::new(1050.0, 0.0, 0.0), 57670.0);
    d.advance(glam::DVec3::new(500.0, 0.0, 0.0), 59400.0);
    assert!(d.duty_done());
    let report = d.take_completed_report().unwrap();
    assert_eq!(report.actual[0].arrival, Some(57640.0));
    assert_eq!(report.actual[0].departure, Some(57670.0));
    assert_eq!(report.actual[1].arrival, Some(59400.0));
    d.advance(glam::DVec3::new(500.0, 0.0, 0.0), 59401.0);
    assert!(
        d.take_completed_report().is_none(),
        "completion is reported once"
    );
}

#[test]
fn a_duty_starts_with_a_trip_the_bus_can_reach() {
    let trips = vec![
        planned(
            54420.0,
            &[(500.0, 54420.0, 54420.0), (1000.0, 56400.0, 56400.0)],
        ),
        planned(
            57660.0,
            &[(1000.0, 57660.0, 57660.0), (500.0, 59400.0, 59400.0)],
        ),
    ];
    let duty = |trips: Vec<PlannedTrip>| PlayerDuty {
        line: "5".into(),
        tour: "3".into(),
        trips,
        trip_index: 0,
        first_trip: 0,
        statistics: Default::default(),
        completed_report: None,
        next_stop: 0,
        at_stop: false,
        arrived_late: None,
        done: false,
        left_late: None,
        held_back: false,
        odo: 0.0,
        last_pos: None,
        arrival_odo: 0.0,
        left_odo: 0.0,
        placed: false,
        trip_changed: false,
        picked: false,
        first_update: None,
        heading: 0.0,
    };
    // 4 km away two minutes before the 15:07 leaves: the duty begins with the 16:01
    let mut d = duty(trips.clone());
    d.advance(glam::DVec3::new(-3500.0, 0.0, 0.0), 54300.0);
    assert_eq!((d.trip_index, d.next_stop), (1, 0));
    assert!(d.delay(54300.0) < -3000.0, "early for the 16:01");
    // under way already and nothing later: the last trip, from its first stop the bus
    // can still make on time
    let mut d = duty(trips[1..].to_vec());
    d.advance(glam::DVec3::new(-3500.0, 0.0, 0.0), 58000.0);
    assert_eq!((d.trip_index, d.next_stop), (0, 1));
    // ... or from its first when none can be
    let mut d = duty(trips[1..].to_vec());
    d.advance(glam::DVec3::new(-3500.0, 0.0, 0.0), 59000.0);
    assert_eq!((d.trip_index, d.next_stop), (0, 0));
}

#[test]
fn ibis_skips_a_service_leg_for_the_player_display() {
    let mut service = planned(100.0, &[(0.0, 100.0, 100.0), (100.0, 200.0, 200.0)]);
    service.line.clear();
    service.terminus = "Betriebsfahrt".into();
    let mut passenger = planned(300.0, &[(100.0, 300.0, 300.0), (200.0, 400.0, 400.0)]);
    passenger.line = "5E".into();
    let d = PlayerDuty {
        line: "5E".into(),
        tour: "1".into(),
        trips: vec![service, passenger],
        trip_index: 0,
        first_trip: 0,
        statistics: Default::default(),
        completed_report: None,
        next_stop: 1,
        at_stop: false,
        arrived_late: None,
        done: false,
        left_late: None,
        held_back: false,
        odo: 0.0,
        last_pos: None,
        arrival_odo: 0.0,
        left_odo: 0.0,
        placed: false,
        trip_changed: false,
        picked: false,
        first_update: None,
        heading: 0.0,
    };
    let (trip, stop) = d.trip_for_ibis();
    assert_eq!(trip.line, "5E");
    assert_eq!(trip.terminus, "T");
    assert_eq!(stop, 0);
}

#[test]
fn the_player_picks_the_trip_to_start_with() {
    let trips = vec![
        planned(4.0 * 3600.0 + 7.0 * 60.0, &[(0.0, 0.0, 0.0)]),
        planned(4.0 * 3600.0 + 22.0 * 60.0, &[(0.0, 0.0, 0.0)]),
        planned(4.0 * 3600.0 + 37.0 * 60.0, &[(0.0, 0.0, 0.0)]),
    ];
    assert_eq!(chosen_trip(&trips, "04:22"), Some(1));
    assert_eq!(chosen_trip(&trips, "4:30"), Some(2), "the next one leaving");
    assert_eq!(chosen_trip(&trips, "05:00"), None);
    assert_eq!(chosen_trip(&trips, "1"), Some(0));
    assert_eq!(chosen_trip(&trips, "3"), Some(2));
    assert_eq!(chosen_trip(&trips, "4"), None);
}

#[test]
fn bays() {
    // the stop's box offset is kept as it is until the vehicle is known
    for lat in [0.0, 2.0, -4.0] {
        assert_eq!(bay_offset(lat), lat);
    }
}

#[test]
fn a_service_trip_without_a_line_shows_none() {
    // two trips of Gerolstein's `Diensttafeln.ttl`: OMSI 2 shows no line for the first,
    // not "Diensttafeln" (#185)
    let trip = |name: &str, text: &str| {
        ::timetable::Trip::parse(&::legacy_config::CfgFile::from_str(
            format!("TTData/{name}.ttp"),
            text,
        ))
    };
    let service = trip(
        "BTF_Bth-Spur1_Schulz",
        "[trip]\nBTF_Bth-Spur1_Schulz\nBetriebsfahrt\n\n",
    );
    assert_eq!(bus_line(&service), "");
    let swim = trip(
        "S_Schulz_Hallenbad",
        "[trip]\nS_Schulz_Hallenbad\nSchwimmkinder\n&\n",
    );
    assert_eq!(bus_line(&swim), "&");
}

#[test]
fn school_days_select_bit_8_and_school_holidays_bit_9() {
    let calendar = ::map::Calendar {
        ranges: vec![::map::HolidayRange {
            start: 20230622,
            end: 20230804,
            name: "Sommerferien".into(),
        }],
        holidays: vec![],
    };
    // Tuesdays 2023-06-20 and 2023-06-27.
    let school_day = ::simulation::SimClock {
        year: 2023,
        day_of_year: 171,
        ..Default::default()
    };
    let in_holidays = ::simulation::SimClock {
        day_of_year: 178,
        ..school_day.clone()
    };
    assert_eq!(day_bits(&calendar, &school_day), (1 << 1, 1 << 8));
    assert_eq!(day_bits(&calendar, &in_holidays), (1 << 1, 1 << 9));
}

#[test]
fn a_tour_runs_on_its_days_and_a_night_tour_on_the_eve() {
    let tour = |mask: i32, departure: f32| ::timetable::Tour {
        extra: mask.to_string(),
        trips: vec![::timetable::TourTrip { departure, ..Default::default() }],
        ..Default::default()
    };
    let weekdays = (0..5).fold(0, |m, d| m | 1 << d) | 3 << 8;
    let saturday = 1 << 5 | 3 << 8;
    // a Tuesday, a school day
    let tuesday = ((1 << 1, 1 << 8), 1 << 2);
    assert!(tour_runs(&tour(weekdays, 6.0 * 60.0), tuesday.0, tuesday.1));
    assert!(!tour_runs(&tour(saturday, 6.0 * 60.0), tuesday.0, tuesday.1));
    // a Friday night: the Saturday tour's trips after 24:00 belong to it
    let friday = ((1 << 4, 1 << 8), 1 << 5);
    assert!(tour_runs(&tour(saturday, 24.0 * 60.0 + 49.0), friday.0, friday.1));
    assert!(!tour_runs(&tour(saturday, 23.0 * 60.0), friday.0, friday.1));
    // school days only, in the holidays
    assert!(!tour_runs(&tour(1 << 1 | 1 << 8, 6.0 * 60.0), (1 << 1, 1 << 9), 1 << 2));
}

/// A Saturday after one midnight on the traffic's clock: departures of one trip over stops
/// 1, 2, 3 (ten minutes apart), as (time, tour mask).
fn saturday_after_midnight(departures: &[(f64, i32)]) -> Schedule {
    let trip = ::timetable::Trip {
        line: "N1".into(),
        terminus: "T".into(),
        stations: vec![1, 2, 3],
        ..Default::default()
    };
    let times = TripTimes {
        stations: vec![(0.0, 0.0), (600.0, 600.0), (1200.0, 1200.0)],
        stops: vec![true; 3],
        kinds: vec![0; 3],
        duration: 1200.0,
    };
    let departures: Vec<Departure> = departures
        .iter()
        .map(|&(time, mask)| Departure {
            time,
            trip: 0,
            profile: 0,
            line: "N1".into(),
            ai_group: String::new(),
            tour: "1".into(),
            mask,
            spawned: false,
            base: DAY,
        })
        .collect();
    Schedule {
        data: ::timetable::TimetableData {
            trips: vec![trip],
            ..Default::default()
        },
        trip_departures: vec![(0..departures.len()).collect()],
        departures,
        times: vec![vec![times]],
        visits: HashMap::from([(1, vec![(0, 0)]), (2, vec![(0, 1)]), (3, vec![(0, 2)])]),
        day_bits: (1 << 5, 1 << 8),
        eve_bits: (1 << 4, 1 << 8),
        day_base: DAY,
        ..Default::default()
    }
}

const FRIDAY: i32 = 1 << 4 | 1 << 8;
const SATURDAY: i32 = 1 << 5 | 1 << 8;

fn passenger_service(
    s: &Schedule,
    departure: usize,
    from: usize,
) -> crate::bus_service::BusService {
    let entry = &s.departures[departure];
    let times = s.times_of(departure);
    crate::bus_service::BusService::new(
        trip_stations(&s.data.trips[entry.trip])
            .into_iter()
            .enumerate()
            .skip(from)
            .filter(|(index, _)| times.stops[*index])
            .map(|(index, id)| {
                ::traffic::StopTarget::from_tuple((
                    index,
                    0.0,
                    0.0,
                    entry.base + entry.time + times.stations[index].1,
                    id,
                    0.0,
                ))
            })
            .collect(),
    )
}

fn passenger_duty(s: &Schedule, departure: usize, next_stop: usize) -> PlayerDuty {
    let entry = &s.departures[departure];
    let times = s.times_of(departure);
    let trip = &s.data.trips[entry.trip];
    let stops = trip_stations(trip)
        .into_iter()
        .enumerate()
        .map(|(index, id)| PlannedStop {
            object_id: id,
            name: id.to_string(),
            arr: entry.time + times.stations[index].0,
            dep: entry.time + times.stations[index].1,
            position: None,
            dir: StopDir::default(),
            stops: times.stops[index],
        })
        .collect();
    PlayerDuty {
        line: entry.line.clone(),
        tour: entry.tour.clone(),
        trips: vec![PlannedTrip {
            name: trip.name.clone(),
            line: trip.line.clone(),
            terminus: trip.terminus.clone(),
            departure: entry.time,
            end: entry.time + times.duration,
            stops,
        }],
        trip_index: 0,
        first_trip: 0,
        next_stop,
        statistics: Default::default(),
        completed_report: None,
        at_stop: false,
        arrived_late: None,
        done: false,
        left_late: None,
        held_back: false,
        odo: 0.0,
        last_pos: None,
        arrival_odo: 0.0,
        left_odo: 0.0,
        placed: true,
        trip_changed: false,
        picked: true,
        first_update: None,
        heading: 0.0,
    }
}

#[test]
fn passenger_targets_keep_a_pre_midnight_trip_on_the_previous_days_mask() {
    let s = saturday_after_midnight(&[
        (23.0 * 3600.0 + 55.0 * 60.0, FRIDAY),
        (23.0 * 3600.0 + 55.0 * 60.0, SATURDAY),
    ]);
    let targets = s.stop_targets_within(DAY + 4.0 * 60.0, 300.0, None, None);
    let target = &targets.get(&2).expect("Friday's bus calls at 00:05")[0];
    assert_eq!(target.destination, "3");
    assert!(target.termini.contains("T"));
    assert!(target.deadline > DAY + 5.0 * 60.0);
    assert!(
        targets.get(&3).is_none_or(Vec::is_empty),
        "no boarding at the terminus"
    );

    let saturday_only = saturday_after_midnight(&[(23.0 * 3600.0 + 55.0 * 60.0, SATURDAY)]);
    assert!(
        saturday_only
            .stop_targets_within(DAY + 4.0 * 60.0, 300.0, None, None)
            .is_empty()
    );
}

#[test]
fn passenger_targets_keep_previous_days_departures_after_24_hours() {
    let s = saturday_after_midnight(&[(DAY + 22.0 * 60.0, FRIDAY)]);
    let targets = s.stop_targets_within(DAY + 31.0 * 60.0, 300.0, None, None);
    assert_eq!(targets[&2][0].destination, "3");
}

#[test]
fn passenger_targets_remain_available_at_later_stops_of_a_delayed_trip() {
    let mut s = saturday_after_midnight(&[(8.0 * 3600.0, SATURDAY)]);
    s.data.trips[0].stations = (1..=10).collect();
    s.times[0][0] = TripTimes {
        stations: (0..10)
            .map(|i| (i as f64 * 600.0, i as f64 * 600.0))
            .collect(),
        stops: vec![true; 10],
        kinds: vec![0; 10],
        duration: 5400.0,
    };
    for stop in 1..10 {
        let call = DAY + 8.0 * 3600.0 + (stop - 1) as f64 * 600.0;
        // The bus reaches each new streaming region five minutes behind schedule.
        let service = passenger_service(&s, 0, (stop - 1) as usize);
        let active = HashMap::from([(0, s.passenger_run(0, Some(&service), None))]);
        let targets = s.stop_targets_for_runs(call + 300.0, 300.0, &active);
        let target = &targets
            .get(&stop)
            .expect("late service still has waiting demand")[0];
        assert!(target.termini.contains("T"));
        assert_eq!(target.deadline, call + 3600.0);
        let expired = s.stop_targets_for_runs(call + 3600.0, 300.0, &active);
        assert!(expired.get(&stop).is_none_or(Vec::is_empty));
    }
    s.times[0][0].stops[4] = false;
    let service = passenger_service(&s, 0, 0);
    let active = HashMap::from([(0, s.passenger_run(0, Some(&service), None))]);
    let targets = s.stop_targets_for_runs(DAY + 8.0 * 3600.0 + 2400.0, 300.0, &active);
    assert!(targets.get(&5).is_none_or(Vec::is_empty));
    assert!(
        targets
            .values()
            .flatten()
            .all(|target| target.destination != "5")
    );
}

#[test]
fn passenger_targets_use_the_next_call_deadline_for_overdue_demand() {
    let s = saturday_after_midnight(&[(8.0 * 3600.0, SATURDAY), (8.0 * 3600.0 + 1200.0, SATURDAY)]);
    let call = DAY + 8.0 * 3600.0;
    let service = passenger_service(&s, 0, 0);
    let active = HashMap::from([(0, s.passenger_run(0, Some(&service), None))]);
    let targets = s.stop_targets_for_runs(call + 300.0, 300.0, &active);
    assert_eq!(targets[&1][0].deadline, call + 1500.0);
    assert!(
        s.stop_targets_within(call - 301.0, 300.0, None, None)
            .is_empty()
    );
}

#[test]
fn passenger_targets_do_not_invent_an_overdue_bus() {
    let s = saturday_after_midnight(&[(8.0 * 3600.0, SATURDAY)]);
    let call = DAY + 8.0 * 3600.0;
    assert!(!s.stop_targets_within(call - 300.0, 300.0, None, None)[&1].is_empty());
    assert!(
        s.stop_targets_within(call - 301.0, 300.0, None, None)
            .is_empty()
    );
    assert!(
        s.stop_targets_within(call + 1.0, 300.0, None, None)
            .get(&1)
            .is_none_or(Vec::is_empty)
    );
    assert!(
        s.stop_targets_within(call + 1800.0, 300.0, None, None)
            .is_empty()
    );
}

#[test]
fn passenger_targets_stop_refilling_after_the_actual_bus_passes() {
    let s = saturday_after_midnight(&[(8.0 * 3600.0, SATURDAY)]);
    let now = DAY + 8.0 * 3600.0 + 900.0;
    let mut service = passenger_service(&s, 0, 0);
    let active = |service: &crate::bus_service::BusService| {
        HashMap::from([(0, s.passenger_run(0, Some(service), None))])
    };
    assert!(!s.stop_targets_for_runs(now, 300.0, &active(&service))[&1].is_empty());
    service.stops.pop_front();
    let targets = s.stop_targets_for_runs(now, 300.0, &active(&service));
    assert!(targets.get(&1).is_none_or(Vec::is_empty));
    assert!(
        !targets[&2].is_empty(),
        "the later stop still needs the late bus"
    );
    service.stops.clear();
    assert!(
        s.stop_targets_for_runs(now, 300.0, &active(&service))
            .is_empty()
    );
}

#[test]
fn passenger_targets_do_not_refill_a_stop_served_early() {
    let s = saturday_after_midnight(&[(8.0 * 3600.0, SATURDAY)]);
    let service = passenger_service(&s, 0, 1);
    let active = HashMap::from([(0, s.passenger_run(0, Some(&service), None))]);
    let targets = s.stop_targets_for_runs(DAY + 8.0 * 3600.0 - 60.0, 300.0, &active);
    assert!(targets.get(&1).is_none_or(Vec::is_empty));
}

#[test]
fn passenger_targets_distinguish_equal_time_stops_and_unloaded_calls() {
    let mut s = saturday_after_midnight(&[(8.0 * 3600.0, SATURDAY)]);
    s.times[0][0].stations[1] = (0.0, 0.0);
    let mut service = passenger_service(&s, 0, 1);
    let tail = service.stops.pop_back().unwrap();
    let unloaded = RunningTrip {
        car: 10,
        steps: vec![],
        next: 0,
        stations: vec![(tail.stop.get(), tail.depart)],
        served: vec![false],
    };
    let run = s.passenger_run(0, Some(&service), Some(&unloaded));
    assert_eq!(run.pending, HashSet::from_iter([1, 2]));
    let targets =
        s.stop_targets_for_runs(DAY + 8.0 * 3600.0 + 60.0, 300.0, &HashMap::from([(0, run)]));
    assert!(targets.get(&1).is_none_or(Vec::is_empty));
    assert_eq!(targets[&2][0].destination, "3");
}

#[test]
fn passenger_targets_do_not_apply_yesterdays_progress_to_todays_run() {
    let mut s = saturday_after_midnight(&[(8.0 * 3600.0, FRIDAY | SATURDAY)]);
    s.departures[0].base = 0.0;
    let finished = passenger_service(&s, 0, 3);
    let active = HashMap::from([(0, s.passenger_run(0, Some(&finished), None))]);
    let targets = s.stop_targets_for_runs(DAY + 8.0 * 3600.0 - 60.0, 300.0, &active);
    assert!(!targets[&1].is_empty());
}

#[test]
fn passenger_targets_follow_an_actual_bus_delayed_across_midnight() {
    let mut s = saturday_after_midnight(&[(23.0 * 3600.0 + 30.0 * 60.0, FRIDAY)]);
    s.departures[0].base = 0.0; // Scheduled end 23:50, still at stop 2 after midnight.
    let service = passenger_service(&s, 0, 1);
    let active = HashMap::from([(0, s.passenger_run(0, Some(&service), None))]);
    let targets = s.stop_targets_for_runs(DAY + 60.0, 300.0, &active);
    assert_eq!(targets[&2][0].destination, "3");
    assert!(targets.get(&1).is_none_or(Vec::is_empty));
}

#[test]
fn passenger_targets_follow_player_progress_and_stop_after_trip_completion() {
    let s = saturday_after_midnight(&[(8.0 * 3600.0, SATURDAY)]);
    let now = DAY + 8.0 * 3600.0 + 900.0;
    let mut duty = passenger_duty(&s, 0, 1);
    let targets = s.stop_targets_within(now, 300.0, None, Some(&duty));
    assert!(targets.get(&1).is_none_or(Vec::is_empty));
    assert_eq!(targets[&2][0].destination, "3");
    duty.done = true;
    assert!(
        s.stop_targets_within(now, 300.0, None, Some(&duty))
            .is_empty()
    );
}

#[test]
fn passenger_targets_keep_the_next_departure_after_the_player_passes() {
    let s = saturday_after_midnight(&[(8.0 * 3600.0, SATURDAY), (8.0 * 3600.0 + 1200.0, SATURDAY)]);
    let duty = passenger_duty(&s, 0, 2);
    let call = DAY + 8.0 * 3600.0;
    assert!(
        s.stop_targets_within(call + 300.0, 300.0, None, Some(&duty))
            .get(&1)
            .is_none_or(Vec::is_empty)
    );
    let targets = s.stop_targets_within(call + 900.0, 300.0, None, Some(&duty));
    assert_eq!(
        targets[&1].len(),
        2,
        "one target per destination, not per bus"
    );
    assert_eq!(targets[&1][0].deadline, call + 1200.0 + 3600.0);
}

#[test]
fn passenger_targets_follow_the_players_previous_day_trip_after_midnight() {
    let s = saturday_after_midnight(&[(23.0 * 3600.0 + 55.0 * 60.0, FRIDAY)]);
    let mut duty = passenger_duty(&s, 0, 1);
    let now = DAY + 10.0 * 60.0;
    let targets = s.stop_targets_within(now, 300.0, None, Some(&duty));
    assert_eq!(targets[&2][0].destination, "3");
    duty.next_stop = 2;
    assert!(
        s.stop_targets_within(now, 300.0, None, Some(&duty))
            .get(&2)
            .is_none_or(Vec::is_empty)
    );
}

#[test]
#[ignore = "requires OMSI_ROOT with Tettau installed; reads timetable data only"]
fn installed_tettau_keeps_passenger_targets_along_a_delayed_countryside_trip() {
    let root = std::path::PathBuf::from(std::env::var_os("OMSI_ROOT").expect("OMSI_ROOT"));
    let data = TimetableData::load(&root.join("maps/Tettau"));
    assert!(data.errors.is_empty(), "{:?}", data.errors);
    let trip = data
        .trips
        .iter()
        .find(|trip| trip.name == "8342_Kronach")
        .unwrap()
        .clone();
    let stations = trip_stations(&trip);
    assert!(stations.len() > 20);
    let times = TripTimes::new(&stations, trip.profiles.first(), &|from, to| {
        data.stn_links
            .iter()
            .find(|link| link.from_id == from && link.to_id == to)
            .map(|link| link.length)
    });
    let mut s = saturday_after_midnight(&[(8.0 * 3600.0, SATURDAY)]);
    s.data.trips = vec![trip];
    s.data.bus_stops = data.bus_stops;
    s.times = vec![vec![times]];
    let names = s.stop_names();
    for (index, stop) in stations.iter().enumerate().take(stations.len() - 1) {
        if !s.times[0][0].stops[index] {
            continue;
        }
        let now = DAY + 8.0 * 3600.0 + s.times[0][0].stations[index].1 + 300.0;
        let service = passenger_service(&s, 0, index);
        let active = HashMap::from([(0, s.passenger_run(0, Some(&service), None))]);
        let targets = s.stop_targets_for_runs(now, 300.0, &active);
        assert!(
            targets.get(stop).is_some_and(|targets| !targets.is_empty()),
            "no demand at stop {index}: {:?}",
            names.get(stop)
        );
    }
}

#[test]
fn the_day_befores_night_trips_run_after_midnight() {
    let s = saturday_after_midnight(&[
        (10.0 * 60.0, SATURDAY),
        (DAY + 22.0 * 60.0, FRIDAY),
        (DAY + 22.0 * 60.0, SATURDAY),
    ]);
    // at 0:25: Saturday's 0:10, and Friday's tour's 24:22 - Saturday's 24:22 runs tonight
    assert_eq!(s.due(25.0 * 60.0, 20.0 * 60.0), vec![(0, DAY), (1, 0.0)]);
}

#[test]
fn a_run_of_the_day_before_does_not_take_todays() {
    let mut s = saturday_after_midnight(&[
        (23.0 * 3600.0 + 50.0 * 60.0, FRIDAY | SATURDAY),
        (DAY + 5.0 * 60.0, FRIDAY | SATURDAY),
    ]);
    // Friday's 23:50 is still on the road at midnight; Friday's 24:05 ran this morning
    for d in &mut s.departures {
        d.spawned = true;
        d.base = 0.0;
    }
    // Saturday's 23:50 runs all the same, Friday's 24:05 not twice
    assert_eq!(s.due(23.0 * 3600.0 + 55.0 * 60.0, 600.0), vec![(0, DAY)]);
    assert!(s.due(10.0 * 60.0, 600.0).is_empty());
    // the player's tour is no one else's, whatever day it was taken on
    s.player_tour = Some(("N1".into(), "1".into()));
    assert!(s.due(23.0 * 3600.0 + 55.0 * 60.0, 600.0).is_empty());
}

#[test]
fn the_boards_show_the_buses_of_the_night_after_midnight() {
    let mut s = saturday_after_midnight(&[
        (23.0 * 3600.0 + 50.0 * 60.0, FRIDAY),
        (DAY + 22.0 * 60.0, FRIDAY),
        (DAY + 22.0 * 60.0, SATURDAY),
    ]);
    // Friday's 23:50 is on the road, due at stop 3 at 0:10
    s.departures[0].spawned = true;
    s.departures[0].base = 0.0;
    let on_road = HashMap::from([(
        0,
        OnRoad {
            next: Some(10.0 * 60.0),
            dwelling: false,
            late: 0.0,
        },
    )]);
    let now = 14.0 * 60.0;
    let at = |stop: i64| -> Vec<f64> {
        let mut l: Vec<f64> = s
            .stop_list(stop, now, &on_road, None)
            .iter()
            .map(|v| v.0)
            .collect();
        l.sort_by(f64::total_cmp);
        l
    };
    // the 23:50 is past stop 2 and overdue at stop 3; Friday's 24:22 comes at 0:32
    assert_eq!(at(2), vec![32.0 * 60.0]);
    assert_eq!(at(3), vec![now, 42.0 * 60.0]);
}

/// #393: the boards show when a bus leaves, not when it arrives.
#[test]
fn the_boards_show_when_a_bus_leaves() {
    let mut s = saturday_after_midnight(&[(10.0 * 60.0, SATURDAY)]);
    // at stop 2 from 0:20:00 to 0:20:30
    s.times[0][0].stations[1] = (600.0, 630.0);
    let leaves = |s: &Schedule, now: f64, on_road: &HashMap<usize, OnRoad>| -> Vec<f64> {
        s.stop_list(2, now, on_road, None)
            .iter()
            .map(|v| v.0)
            .collect()
    };
    let at_20_30 = 20.0 * 60.0 + 30.0;
    // not on the road yet
    assert_eq!(leaves(&s, 12.0 * 60.0, &HashMap::new()), vec![at_20_30]);
    // on its way, two minutes late
    s.departures[0].spawned = true;
    let late = HashMap::from([(
        0,
        OnRoad {
            next: Some(at_20_30),
            dwelling: false,
            late: 120.0,
        },
    )]);
    assert_eq!(leaves(&s, 12.0 * 60.0, &late), vec![at_20_30 + 120.0]);
    // standing there
    let standing = HashMap::from([(
        0,
        OnRoad {
            next: Some(at_20_30),
            dwelling: true,
            late: 0.0,
        },
    )]);
    assert_eq!(leaves(&s, 20.0 * 60.0 + 10.0, &standing), vec![at_20_30]);
    assert_eq!(leaves(&s, 21.0 * 60.0, &standing), vec![21.0 * 60.0]);
}

/// #393: the boards' two hours count to when a bus leaves.
#[test]
fn the_boards_look_two_hours_ahead_by_departure() {
    let mut s = saturday_after_midnight(&[(3.0 * 3600.0, SATURDAY)]);
    // at stop 2 from 3:10:00 to 3:10:30
    s.times[0][0].stations[1] = (600.0, 630.0);
    let leaves = |now: f64| -> Vec<f64> {
        s.stop_list(2, now, &HashMap::new(), None)
            .iter()
            .map(|v| v.0)
            .collect()
    };
    let at_3_10_30 = 3.0 * 3600.0 + 10.0 * 60.0 + 30.0;
    // it arrives within two hours, but leaves after
    assert!(leaves(at_3_10_30 - BOARD_AHEAD - 15.0).is_empty());
    assert_eq!(leaves(at_3_10_30 - BOARD_AHEAD), vec![at_3_10_30]);
}

#[test]
fn the_day_before_new_year() {
    let mut c = ::simulation::SimClock {
        year: 2027,
        day_of_year: 1,
        ..Default::default()
    };
    let d = day_before(&c);
    assert_eq!((d.year, d.day_of_year), (2026, 365));
    c.day_of_year = 60;
    assert_eq!(day_before(&c).day_of_year, 59);
}

/// #101: a detector bus standing at its stop from 20:00 to 24:00 is due at a 23:30 load.
#[test]
fn a_long_trip_under_way_is_put_on_the_road_when_the_map_loads() {
    let mut s = saturday_after_midnight(&[(20.0 * 3600.0, SATURDAY)]);
    s.times[0][0] = TripTimes {
        stations: vec![(0.0, 4.0 * 3600.0)],
        stops: vec![true],
        kinds: vec![0],
        duration: 4.0 * 3600.0,
    };
    let tod = 23.5 * 3600.0;
    assert_eq!(s.due(tod, 20.0 * 60.0), vec![(0, DAY)]);
    assert!(s.due(tod, 2.5).is_empty());
    assert!(s.due(24.5 * 3600.0, 20.0 * 60.0).is_empty());
}

#[test]
fn a_track_trips_bus_does_not_move_while_it_stands_at_a_stop() {
    let detector = TripTimes {
        stations: vec![(0.0, 4.0 * 3600.0)],
        stops: vec![true],
        kinds: vec![0],
        duration: 4.0 * 3600.0,
    };
    assert_eq!(spawn::track_progress(&detector, 3.5 * 3600.0), 0.0);
    // 10 min driving, 2 min at a stop, 10 min driving
    let train = TripTimes {
        stations: vec![(0.0, 0.0), (600.0, 720.0), (1320.0, 1320.0)],
        stops: vec![true; 3],
        kinds: vec![0; 3],
        duration: 1320.0,
    };
    assert!((spawn::track_progress(&train, 300.0) - 0.25).abs() < 1e-9);
    assert!((spawn::track_progress(&train, 660.0) - 0.5).abs() < 1e-9);
    assert!((spawn::track_progress(&train, 1020.0) - 0.75).abs() < 1e-9);
    assert_eq!(spawn::track_progress(&train, 2000.0), 1.0);
}

#[test]
fn a_trip_of_the_day_before_still_under_way_is_put_on_the_road_when_the_map_loads() {
    let mut s = saturday_after_midnight(&[(23.0 * 3600.0, FRIDAY), (23.0 * 3600.0, SATURDAY)]);
    for t in &mut s.times[0] {
        t.duration = 5400.0;
    }
    // at 0:10 Friday's 23:00 (90 min) still runs; Saturday's tour did not run Friday
    assert_eq!(s.due(10.0 * 60.0, 20.0 * 60.0), vec![(0, 0.0)]);
    assert!(s.due(10.0 * 60.0, 2.5).is_empty());
    assert!(s.due(40.0 * 60.0, 20.0 * 60.0).is_empty());
}
