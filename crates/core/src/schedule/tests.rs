//! Tests of the schedule.

use super::*;

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
