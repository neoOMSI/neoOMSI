use super::*;

#[test]
fn map_humans_load_nested_paths_and_preserve_weights() {
    let root = std::env::temp_dir().join(format!("omsi-map-human-paths-{}", std::process::id()));
    let nested = root.join("Humans/JP_Test/Child_1");
    std::fs::create_dir_all(&nested).unwrap();
    // Synthetic definitions: no original passenger assets are required.
    std::fs::write(nested.join("Child_1.hum"), "[model]\nmodel.cfg\n").unwrap();
    std::fs::write(nested.join("model.cfg"), "").unwrap();
    let other = root.join("Humans/Other");
    std::fs::create_dir_all(&other).unwrap();
    std::fs::write(other.join("Man.hum"), "[model]\nmodel.cfg\n").unwrap();
    std::fs::write(other.join("model.cfg"), "").unwrap();
    let list = vec![
        "humans\\jp_test\\child_1\\child_1.hum".into(),
        "Humans/JP_Test/Child_1/Child_1.hum".into(),
        "JP_Test/Child_1/Child_1.hum".into(),
        "Humans/JP_Test/Missing.hum".into(),
    ];
    let picked = map_human_types(&root, &list);
    assert_eq!(picked.len(), 3);
    assert!(Arc::ptr_eq(&picked[0], &picked[1]));
    assert!(Arc::ptr_eq(&picked[1], &picked[2]));
    // (case-blind: a case-insensitive disk keeps the list's own spelling; and
    // separator-blind: on Windows `nested` keeps the slashes it was joined with, while
    // the resolved path is built with backslashes)
    let lower = |p: &Path| p.to_string_lossy().to_lowercase().replace('\\', "/");
    assert!(
        picked
            .iter()
            .all(|t| lower(&t.def.path).starts_with(&lower(&nested)))
    );
    assert!(map_human_types(&root, &["Humans/Missing/None.hum".into()]).is_empty());
    std::fs::remove_dir_all(root).unwrap();
}

/// Berlin 1991's pack: full fare, short haul, day ticket (adults), and two reduced
/// fares for 6..13.
fn berlin_91() -> omsi_content::tickets::TicketPack {
    let t = |name: &str, age: (i32, i32), day: bool, p: f32| omsi_content::tickets::Ticket {
        name: name.into(),
        age_min: age.0,
        age_max: age.1,
        day_ticket: day,
        probability: p,
        ..Default::default()
    };
    omsi_content::tickets::TicketPack {
        stamper_prop: 0.3,
        ticketbuy_prop: 0.2,
        tickets: vec![
            t("Fahrschein", (14, 200), false, 1.0),
            t("Kurzstrecke", (14, 200), false, 0.4),
            t("Tageskarte", (14, 200), true, 0.2),
            t("Ermaessigt", (6, 13), false, 1.0),
            t("Kurzstrecke Erm", (6, 13), false, 0.4),
        ],
        ..Default::default()
    }
}

#[test]
fn seats_counted_by_the_scripts_numbers() {
    let seat = |omsi_seat: usize| Seat {
        point: None,
        pos: Vec3::ZERO,
        floor: Vec3::ZERO,
        rot: 0.0,
        seated: true,
        height: 0.45,
        omsi_seat,
    };
    // the driver's place is seat 0, a second section's numbers follow the first's
    let seats = [seat(1), seat(2), seat(4), seat(6)];
    assert_eq!(
        seat_numbers(&seats, [0, 2, 2, 3].into_iter()),
        [0, 1, 0, 0, 2, 0, 1]
    );
    assert!(seat_numbers(&[], [0].into_iter()).is_empty());
}

#[test]
fn seat_preference_uses_available_seats_then_standing_places() {
    let places: Vec<Seat> = [false, true, false, true]
        .into_iter()
        .enumerate()
        .map(|(omsi_seat, seated)| Seat {
            point: None,
            pos: Vec3::ZERO,
            floor: Vec3::ZERO,
            rot: 0.0,
            seated,
            height: if seated { 0.45 } else { 0.0 },
            omsi_seat,
        })
        .collect();
    // The already reserved seat is removed before applying the preference.
    let mut taken = [false, true, false, false];
    let mut free: Vec<_> = (0..places.len()).filter(|&k| !taken[k]).collect();
    prefer_seated_places(&mut free, &places, true);
    assert_eq!(free, vec![3]);

    // When every seat is reserved, standing places remain available.
    taken[3] = true;
    let mut free: Vec<_> = (0..places.len()).filter(|&k| !taken[k]).collect();
    prefer_seated_places(&mut free, &places, true);
    assert_eq!(free, vec![0, 2]);

    taken[0] = true;
    taken[2] = true;
    let mut free: Vec<_> = (0..places.len()).filter(|&k| !taken[k]).collect();
    prefer_seated_places(&mut free, &places, true);
    assert!(free.is_empty());
}

#[test]
fn seat_preference_off_preserves_random_place_selection() {
    let places = [false, true].map(|seated| Seat {
        point: Some(0),
        pos: Vec3::ZERO,
        floor: Vec3::ZERO,
        rot: 0.0,
        seated,
        height: if seated { 0.45 } else { 0.0 },
        omsi_seat: 0,
    });
    let mut cabin = passenger_compat_tests::cabin();
    Arc::get_mut(&mut cabin).unwrap().seats = places.to_vec();
    let mut h = Humans::new(Path::new("/nonexistent"));
    assert!(!h.prefer_seats);
    let mut reference = Humans::new(Path::new("/nonexistent"));
    let mut seen = [false; 2];
    for _ in 0..32 {
        let expected = (reference.rand() as usize) % places.len();
        let k = h.reserve_place(BusId::Player, &cabin, None, false).unwrap();
        assert_eq!(k, expected);
        seen[k] = true;
        h.free_seat(BusId::Player, k);
    }
    assert_eq!(seen, [true, true]);
}

#[test]
fn tickets_by_age_and_time() {
    let mut h = Humans::new(Path::new("/nonexistent"));
    h.tickets = Some(Arc::new(berlin_91()));
    let count = |h: &mut Humans, age: f32| {
        let mut n = [0usize; 5];
        for _ in 0..4000 {
            n[h.pick_ticket(age).unwrap()] += 1;
        }
        n
    };
    // an adult (OMSI's default age of 40) never gets a reduced fare, a child only those
    h.time_of_day = 9.0 * 3600.0;
    let adult = count(&mut h, 40.0);
    assert_eq!(adult[3] + adult[4], 0);
    assert!(adult[2] > 300, "{adult:?}");
    let child = count(&mut h, 10.0);
    assert_eq!(child[0] + child[1] + child[2], 0);
    // day tickets sell best at 9:00, little early in the morning and late at night
    h.time_of_day = 1.0 * 3600.0;
    let early = count(&mut h, 40.0);
    assert!(early[2] * 4 < adult[2], "{early:?} vs {adult:?}");
    assert!(day_ticket_factor(9.0 * 3600.0) > 0.99);
    assert!(day_ticket_factor(0.0) < 0.01);
    assert!((day_ticket_factor(20.0 * 3600.0) - (1.0 - 39_600.0 / 56_376.0) as f32).abs() < 1e-3);
    // nobody in the age range: no ticket
    assert_eq!(h.pick_ticket(3.0), None);
}

fn lane(points: Vec<DVec3>, kind: LaneKind) -> omsi_sim::traffic::Lane {
    omsi_sim::traffic::LaneBuilder::polyline(points, kind, 2.5)
}

#[test]
fn pavement_corners_are_joined_and_routed() {
    // an L of pavement: the two paths meet at a right angle, which the road network's
    // heading rule leaves unlinked
    let mut net = Network::default();
    net.lanes.push(lane(
        vec![DVec3::new(0.0, 0.0, 0.0), DVec3::new(0.0, 20.0, 0.0)],
        LaneKind::Sidewalk,
    ));
    net.lanes.push(lane(
        vec![DVec3::new(0.5, 20.3, 0.0), DVec3::new(30.0, 20.3, 0.0)],
        LaneKind::Sidewalk,
    ));
    net.lanes.push(lane(
        vec![DVec3::new(-5.0, 10.0, 0.0), DVec3::new(5.0, 10.0, 0.0)],
        LaneKind::Street,
    ));
    net.link(1.5);
    assert!(
        net.lanes[0].next.is_empty(),
        "the road rule does not join the corner"
    );
    let ped = PedNet::build(&net);
    // the corner is one junction: walking up the first path goes on round it
    let up = Leg {
        lane: 0,
        a: 5.0,
        b: 20.0,
    };
    let corner = ped.end_node(&net, &up).unwrap();
    assert!(
        ped.out[corner].iter().any(|&(l, fwd)| l == 1 && fwd),
        "{:?}",
        ped.out[corner]
    );
    // a dead end turns round
    let n = ped
        .end_node(
            &net,
            &Leg {
                lane: 1,
                a: 0.0,
                b: net.lanes[1].length(),
            },
        )
        .unwrap();
    let turn = ped.next_leg(&net, n, 1, 7).unwrap();
    assert_eq!(turn.lane, 1);
    assert!(turn.a > turn.b);
}

#[test]
fn crossings_of_a_pavement_path_are_found() {
    let mut net = Network::default();
    net.lanes.push(lane(
        vec![DVec3::new(0.0, 0.0, 0.0), DVec3::new(0.0, 8.0, 0.0)],
        LaneKind::Sidewalk,
    ));
    net.lanes.push(lane(
        vec![DVec3::new(-30.0, 4.0, 0.0), DVec3::new(30.0, 4.0, 0.0)],
        LaneKind::Street,
    ));
    net.link(1.5);
    let mut ped = PedNet::build(&net);
    let x = ped.crossings(&net, 0).to_vec();
    assert_eq!(x.len(), 1);
    assert!((x[0] - DVec2::new(0.0, 4.0)).length() < 1e-6);
}

/// An articulated bus: the front section's cabin and the rear section's (which only has
/// exits and a seat) become one network through the joint, numbered front first, and a
/// walk through the bent joint moves on without a jump.
#[test]
fn articulated_cabins_are_joined_through_the_bellows() {
    let dir = std::env::temp_dir().join(format!("omsi-humans-test-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let write = |name: &str, text: &str| std::fs::write(dir.join(name), text).unwrap();
    // front: door 0 at the front right, exit 1 in the middle, link to the rear at 3
    write(
        "paths_a.cfg",
        "[pathpnt]\n1.2\n4\n0.4\n[pathpnt]\n0\n4\n0.5\n[pathpnt]\n0\n0\n0.5\n[pathpnt]\n0\n-4.2\n0.6\n[pathpnt]\n1.2\n0\n0.4\n[pathlink]\n0\n1\n[pathlink]\n1\n2\n[pathlink]\n2\n3\n[pathlink]\n2\n4\n",
    );
    write(
        "cabin_a.cfg",
        "[entry]\n0\n[exit]\n4\n[linkToPrevVeh]\n3\n[passpos]\n-0.5\n2\n1.0\n0.45\n0\n",
    );
    // rear: only exits, a seat and the link to the front at point 0
    write(
        "paths_b.cfg",
        "[pathpnt]\n0\n3.6\n0.6\n[pathpnt]\n0\n0\n0.6\n[pathpnt]\n1.2\n0\n0.4\n[pathpnt]\n0\n-2\n0.6\n[pathlink]\n0\n1\n[pathlink]\n1\n2\n[pathlink]\n1\n3\n",
    );
    write(
        "cabin_b.cfg",
        "[exit]\n2\n[linkToNextVeh]\n0\n[passpos]\n-0.5\n-2\n1.1\n0.45\n0\n",
    );
    let def = |cabin: &str, paths: &str| omsi_vehicle::Vehicle {
        path: dir.join("bus.bus"),
        passenger_cabin: Some(cabin.into()),
        paths: Some(paths.into()),
        bounding_box: Some([2.5, 9.0, 3.0, 0.0, 0.0, 1.5]),
        ..Default::default()
    };
    let (front, rear) = (
        def("cabin_a.cfg", "paths_a.cfg"),
        def("cabin_b.cfg", "paths_b.cfg"),
    );
    // couplings: the front's at y -4.3, the rear's own at y 4.0
    let (back, own) = (Vec3::new(0.0, -4.3, 0.3), Vec3::new(0.0, 4.0, 0.3));
    let offset = back - own;
    let cabin = Cabin::load_train(&[(&front, Vec3::ZERO, f32::INFINITY), (&rear, offset, back.y)])
        .expect("cabin");
    std::fs::remove_dir_all(&dir).ok();
    assert_eq!(cabin.parts.len(), 2);
    assert_eq!(cabin.graph.points.len(), 9);
    assert_eq!(
        (cabin.entries.len(), cabin.exits.len(), cabin.seats.len()),
        (1, 2, 2)
    );
    // exit 1 is the rear section's door, where the rear file puts it
    assert!(
        (cabin.exits[1].inside - Vec3::new(1.2, -8.3, 0.4)).length() < 1e-4,
        "{:?}",
        cabin.exits[1].inside
    );
    // the seat in the rear is reached from the front door through the joint, along the
    // routing tables Omsi.exe builds over the joined network
    let seat = &cabin.seats[1];
    assert!((seat.pos.y + 10.3).abs() < 1e-4);
    let all: Vec<Option<usize>> = (0..cabin.graph.points.len()).map(Some).collect();
    let to = cabin
        .omsi_nearest(seat.floor, &all, false, false, None, None)
        .unwrap();
    let mut at = cabin.entries[0].point.unwrap();
    let mut route = vec![cabin.graph.points[at]];
    while at != to {
        at = cabin.route_next(at, to).expect("a way on").0;
        route.push(cabin.graph.points[at]);
        assert!(route.len() < 20, "{route:?}");
    }
    assert!(
        route.iter().any(|p| (p.y + 4.2).abs() < 1e-4)
            && route.iter().any(|p| (p.y + 4.7).abs() < 1e-4),
        "{route:?}"
    );
    // and the nearest exit from there is the rear one
    let exit = cabin.omsi_nearest(seat.floor, &cabin.exit_points(), false, false, None, None);
    assert_eq!(exit, cabin.exits[1].point);
    // the rear section bent 30 degrees about the coupling: walking down the aisle moves on
    // smoothly, and the frames agree with the sections away from the joint
    let lead_rot = Mat4::IDENTITY;
    let bent = 30.0f64;
    let rot = Mat4::from_rotation_z((-bent).to_radians() as f32);
    let pos = back.as_dvec3() - rot.transform_point3(own).as_dvec3();
    let frames = [PartFrame {
        pos,
        rot,
        heading: bent,
        offset,
        joint_y: back.y,
        half: DVec2::new(1.25, 4.5),
        centre: DVec2::ZERO,
    }];
    // beside the aisle the two frames disagree by 0.31 m at the joint itself
    let at_joint = Vec3::new(0.6, back.y, 0.5);
    let rear_frame = pos + rot.transform_point3(at_joint - offset).as_dvec3();
    assert!((rear_frame - at_joint.as_dvec3()).length() > 0.3);
    let mut last = train_point(DVec3::ZERO, &lead_rot, &frames, Vec3::new(0.6, 0.0, 0.5));
    for k in 1..=100 {
        let y = -(k as f32) * 0.1;
        let p = train_point(DVec3::ZERO, &lead_rot, &frames, Vec3::new(0.6, y, 0.5));
        assert!(
            (p - last).length() < 0.14,
            "a jump of {:.3} m at y {y}",
            (p - last).length()
        );
        last = p;
    }
    let ahead = train_point(DVec3::ZERO, &lead_rot, &frames, Vec3::new(1.0, -1.0, 0.5));
    assert!((ahead - DVec3::new(1.0, -1.0, 0.5)).length() < 1e-4);
    let behind_joint = Vec3::new(1.0, -9.0, 0.5);
    let p = train_point(DVec3::ZERO, &lead_rot, &frames, behind_joint);
    assert!((p - (pos + rot.transform_point3(behind_joint - offset).as_dvec3())).length() < 1e-4);
    assert!((train_heading(0.0, &frames, behind_joint) - bent).abs() < 1e-9);
    assert!((train_heading(0.0, &frames, Vec3::new(0.0, back.y, 0.5)) - bent * 0.5).abs() < 1e-9);
}

/// The SD200's footsteps as its paths.cfg gives them to the links (#311): the stairs
/// sound as stairs, the front of the upper deck as its own floor, the aisle below as
/// the plain floor.
#[test]
fn footsteps_come_from_the_links_step_sound_pack() {
    let root = omsi_cfg::env::var_os("OMSI_ROOT")
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from("../../../OMSI 2 Original"));
    let bus = root.join("Vehicles/MAN_SD200/MAN_SD80.bus");
    if !bus.exists() {
        eprintln!("skipped: no {}", bus.display());
        return;
    }
    let def = omsi_vehicle::Vehicle::load(&bus).expect("SD200");
    let cabin = Cabin::load_train(&[(&def, Vec3::ZERO, f32::INFINITY)]).expect("cabin");
    // the link nearest the point, and its pack
    let first = |p: Vec3| {
        let pts = &cabin.graph.points;
        let d = |l: &(i32, i32, bool)| {
            let (a, b) = (pts[l.0 as usize], pts[l.1 as usize]);
            let ab = b - a;
            let t = ((p - a).dot(ab) / ab.length_squared().max(1e-6)).clamp(0.0, 1.0);
            (a + ab * t - p).length()
        };
        let l = (0..cabin.links.len())
            .min_by(|&x, &y| d(&cabin.links[x]).total_cmp(&d(&cabin.links[y])))?;
        cabin.link_pack[l].map(|k| cabin.step_packs[k][0].to_ascii_lowercase())
    };
    assert_eq!(
        first(Vec3::new(-0.89, -1.61, 1.63)).as_deref(),
        Some("step_st_01.wav"),
        "the rear stairs"
    );
    assert_eq!(
        first(Vec3::new(0.0, 4.35, 2.5)).as_deref(),
        Some("step_ov_01.wav"),
        "the upper deck's front"
    );
    assert_eq!(
        first(Vec3::new(0.0, 0.84, 0.57)).as_deref(),
        Some("step_01.wav"),
        "the aisle below"
    );
}

/// The SD202's cabin: the stairs down from the upper deck end beside the rear exits, and
/// the walk from up there to an exit goes down the stairs.
#[test]
fn double_decker_exits_are_reached_down_the_stairs() {
    let root = omsi_cfg::env::var_os("OMSI_ROOT")
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from("../../../OMSI 2 Original"));
    let bus = root.join("Vehicles/MAN_SD202/MAN_D92.bus");
    if !bus.exists() {
        eprintln!("skipped: no {}", bus.display());
        return;
    }
    let def = omsi_vehicle::Vehicle::load(&bus).expect("SD202");
    let cabin = Cabin::load_train(&[(&def, Vec3::ZERO, f32::INFINITY)]).expect("cabin");
    assert_eq!(cabin.exits.len(), 2);
    let exit = &cabin.exits[1];
    assert!(
        (exit.wait - Vec3::new(0.806, -1.26, 0.505)).length() < 1e-3,
        "{:?}",
        exit.wait
    );
    // from the upper deck the routing tables lead down both flights to the exit
    let all: Vec<Option<usize>> = (0..cabin.graph.points.len()).map(Some).collect();
    let upstairs = cabin
        .omsi_nearest(Vec3::new(0.0, -1.8, 2.46), &all, false, false, None, None)
        .unwrap();
    assert!((cabin.graph.points[upstairs].z - 2.46).abs() < 0.1);
    let to = exit.point.unwrap();
    let mut at = upstairs;
    let mut route = vec![cabin.graph.points[at]];
    while at != to {
        at = cabin.route_next(at, to).expect("a way down").0;
        route.push(cabin.graph.points[at]);
        assert!(route.len() < 60, "{route:?}");
    }
    assert!(
        route.iter().any(|p| (p.z - 1.82).abs() < 0.01)
            && route.iter().any(|p| (p.z - 1.205).abs() < 0.01),
        "{route:?}"
    );
}

#[test]
fn legs_run_both_ways() {
    let mut net = Network::default();
    net.lanes.push(lane(
        vec![DVec3::new(0.0, 0.0, 0.0), DVec3::new(0.0, 10.0, 0.0)],
        LaneKind::Sidewalk,
    ));
    let back = Leg {
        lane: 0,
        a: 8.0,
        b: 2.0,
    };
    let (p, h) = back.at(&net, 1.0);
    assert!((p.y - 7.0).abs() < 1e-6);
    assert!((h - 180.0).abs() < 1e-6);
    assert!((back.project(&net, DVec3::new(0.3, 5.0, 0.0), 2.5) - 3.0).abs() < 0.11);
}

#[test]
fn doors_open_falls_back_when_exit_vars_are_undeclared() {
    let dir = std::env::temp_dir().join(format!("omsi-doors-open-test-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    std::fs::write(
        dir.join("test.bus"),
        "[model]\nmodel.cfg\n[varnamelist]\n1\nvars.txt\n[script]\n1\nmain.osc\n",
    )
    .unwrap();
    std::fs::write(dir.join("model.cfg"), "").unwrap();
    // Front door leaf 0 uses PAX_Entry0_Open. Rear door (door_2) has no PAX_Exit0_Open in varlist.
    std::fs::write(
        dir.join("vars.txt"),
        "door_0\ndoor_1\ndoor_2\nPAX_Entry0_Open\n",
    )
    .unwrap();
    std::fs::write(dir.join("main.osc"), "{init}\n{end}\n").unwrap();

    let ty = std::sync::Arc::new(omsi_sim::VehicleType::load(&dir, &dir.join("test.bus")).unwrap());
    let mut v = VehicleInstance::new(ty, omsi_sim::VehicleHost::new(Default::default()));

    // Initially both entries and exit closed
    let (e, x) = Humans::doors_open(&v, 2, 1);
    assert_eq!(e, vec![false, false]);
    assert_eq!(x, vec![false]);

    // Front door leaf 0 opens via PAX_Entry0_Open
    v.set_var("PAX_Entry0_Open", 1.0);
    let (e, x) = Humans::doors_open(&v, 2, 1);
    assert_eq!(e, vec![true, false]);
    assert_eq!(x, vec![false]);

    // Rear door leaf 2 opens (falls back to door_2 since PAX_Exit0_Open is not in varlist)
    v.set_var("door_2", 1.0);
    let (e, x) = Humans::doors_open(&v, 2, 1);
    assert_eq!(e, vec![true, false]);
    assert_eq!(x, vec![true]);

    // Front door leaf 1 opens via door_1 fallback
    v.set_var("door_1", 1.0);
    let (e, x) = Humans::doors_open(&v, 2, 1);
    assert_eq!(e, vec![true, true]);
    assert_eq!(x, vec![true]);

    std::fs::remove_dir_all(&dir).ok();
}

#[test]
fn doors_open_reads_pax_vars_the_script_writes_without_declaring() {
    let dir =
        std::env::temp_dir().join(format!("omsi-doors-open-undeclared-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    std::fs::write(
        dir.join("test.bus"),
        "[model]\nmodel.cfg\n[varnamelist]\n1\nvars.txt\n[script]\n1\nmain.osc\n",
    )
    .unwrap();
    std::fs::write(dir.join("model.cfg"), "").unwrap();
    std::fs::write(dir.join("vars.txt"), "door_0\n").unwrap();
    std::fs::write(
        dir.join("main.osc"),
        "{frame}\n1 (S.L.PAX_Entry0_Open)\n{end}\n",
    )
    .unwrap();

    let ty = std::sync::Arc::new(omsi_sim::VehicleType::load(&dir, &dir.join("test.bus")).unwrap());
    let mut v = VehicleInstance::new(ty, omsi_sim::VehicleHost::new(Default::default()));
    v.set_var("door_0", 0.0);
    v.set_var("PAX_Entry0_Open", 1.0);
    let (e, _) = Humans::doors_open(&v, 1, 0);
    assert_eq!(e, vec![true]);

    std::fs::remove_dir_all(&dir).ok();
}

#[test]
fn doors_open_3door_bus_handles_middle_and_rear_exits() {
    let dir = std::env::temp_dir().join(format!("omsi-doors-3door-test-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    std::fs::write(
        dir.join("test.bus"),
        "[model]\nmodel.cfg\n[varnamelist]\n1\nvars.txt\n[script]\n1\nmain.osc\n",
    )
    .unwrap();
    std::fs::write(dir.join("model.cfg"), "").unwrap();
    std::fs::write(
        dir.join("vars.txt"),
        "door_0\ndoor_1\ndoor_2\ndoor_3\ndoor_4\ndoor_5\n",
    )
    .unwrap();
    std::fs::write(dir.join("main.osc"), "{init}\n{end}\n").unwrap();

    let ty = std::sync::Arc::new(omsi_sim::VehicleType::load(&dir, &dir.join("test.bus")).unwrap());
    let mut v = VehicleInstance::new(ty, omsi_sim::VehicleHost::new(Default::default()));

    // 3-door bus: 6 entries (all 3 doors), 4 exits (middle door leaves 2,3; rear door leaves 4,5)
    let (e, x) = Humans::doors_open(&v, 6, 4);
    assert_eq!(e, vec![false; 6]);
    assert_eq!(x, vec![false; 4]);

    // Middle doors (door_2 and door_3) open
    v.set_var("door_2", 1.0);
    v.set_var("door_3", 1.0);
    let (e, x) = Humans::doors_open(&v, 6, 4);
    assert_eq!(e, vec![false, false, true, true, false, false]);
    assert_eq!(x, vec![true, true, false, false]);

    // Rear doors (door_4 and door_5) open
    v.set_var("door_4", 1.0);
    v.set_var("door_5", 1.0);
    let (e, x) = Humans::doors_open(&v, 6, 4);
    assert_eq!(e, vec![false, false, true, true, true, true]);
    assert_eq!(x, vec![true, true, true, true]);

    std::fs::remove_dir_all(&dir).ok();
}

#[test]
fn seat_preference_respects_reservations_capacity_and_released_seats() {
    let places: Vec<Seat> = [false, true, false, true]
        .into_iter()
        .enumerate()
        .map(|(omsi_seat, seated)| Seat {
            point: Some(0),
            pos: Vec3::ZERO,
            floor: Vec3::ZERO,
            rot: 0.0,
            seated,
            height: if seated { 0.45 } else { 0.0 },
            omsi_seat,
        })
        .collect();
    let mut cabin = passenger_compat_tests::cabin();
    Arc::get_mut(&mut cabin).unwrap().seats = places.to_vec();
    let mut h = Humans::new(Path::new("/nonexistent"));
    h.prefer_seats = true;
    for bus in [BusId::Player, BusId::Ai(42)] {
        // One seated place is already reserved, including a walker or an avatar.
        h.buses.seats.insert(bus, vec![false, true, false, false]);
        assert_eq!(h.reserve_place(bus, &cabin, None, false), Some(3));
        let a = h.reserve_place(bus, &cabin, None, false).unwrap();
        let b = h.reserve_place(bus, &cabin, None, false).unwrap();
        assert!(!places[a].seated && !places[b].seated && a != b);
        assert_eq!(h.reserve_place(bus, &cabin, None, false), None);
        h.free_seat(bus, 1);
        assert_eq!(h.reserve_place(bus, &cabin, None, false), Some(1));
    }
    Arc::get_mut(&mut cabin).unwrap().seats.clear();
    assert_eq!(h.reserve_place(BusId::Player, &cabin, None, false), None);
}
