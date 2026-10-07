use glam::DVec3;
use crate::following::smooth01;

    #[test]
    fn a_stop_is_matched_to_the_lane_it_stands_beside() {
        // out along y = 0 (east), back along y = 6 (west); the stop stands north of the
        // way back: on its right, across the road from the way out
        let out = LaneBuilder::polyline(
            vec![DVec3::new(0.0, 0.0, 0.0), DVec3::new(100.0, 0.0, 0.0)],
            LaneKind::Street,
            3.0,
        );
        let back = LaneBuilder::polyline(
            vec![DVec3::new(100.0, 6.0, 0.0), DVec3::new(0.0, 6.0, 0.0)],
            LaneKind::Street,
            3.0,
        );
        let net = Network {
            lanes: vec![out, back],
            ..Default::default()
        };
        let stop = DVec3::new(50.0, 9.0, 0.0);
        // nearer to the way back anyway: matched there
        assert_eq!(
            net.project_stop_on_route(&[0, 1], stop, Some(25.0), 0)
                .unwrap()
                .0,
            1
        );
        // a stop on the right of the way out, nearer the middle of the road
        let stop2 = DVec3::new(50.0, -2.0, 0.0);
        assert_eq!(
            net.project_stop_on_route(&[0, 1], stop2, Some(25.0), 0)
                .unwrap()
                .0,
            0
        );
        // the route out, back and out again: a stop on the way out, once the trip is past
        // its first leg, is the one on the second way out
        assert_eq!(
            net.project_stop_on_route(&[0, 1, 0], stop2, Some(25.0), 1)
                .unwrap()
                .0,
            2
        );
    }

    use super::*;

    /// A straight lane of 60 m running north, then a right-hand bend of radius 14 m over
    /// 60°, then straight on again.
    fn junction() -> Network {
        let a = LaneBuilder::arc(DVec3::ZERO, 0.0, 60.0, 0.0, 0.0, LaneKind::Street, 3.0);
        let bend = LaneBuilder::arc(
            a.end(),
            0.0,
            14.0 * 60f64.to_radians(),
            14.0,
            0.0,
            LaneKind::Street,
            3.0,
        );
        let c = LaneBuilder::arc(bend.end(), 60.0, 80.0, 0.0, 0.0, LaneKind::Street, 3.0);
        let mut net = Network {
            lanes: vec![a, bend, c],
            ..Default::default()
        };
        net.link(1.5);
        net
    }

    #[test]
    fn path_rules_open_lanes_by_vehicle_type() {
        let mut l = LaneBuilder::polyline(
            vec![DVec3::ZERO, DVec3::new(0.0, 50.0, 0.0)],
            LaneKind::Street,
            3.0,
        );
        // no rules: cars and taxis, no AI buses or trucks (Omsi.exe 0x71d714)
        assert!(l.allows(0) && l.allows(1) && !l.allows(2) && !l.allows(3) && l.allows(-1));
        l.rule_trucks = true;
        assert!(l.allows(0) && l.allows(3) && !l.allows(2));
        l.no_cars = true;
        assert!(!l.allows(0) && l.allows(1) && l.allows(3));
        l.rule_trucks = false;
        assert!(!l.allows(0) && !l.allows(1));
        l.rule_bus = true;
        assert!(!l.allows(0) && l.allows(1) && l.allows(2) && !l.allows(3));
    }

    #[test]
    fn slows_down_for_a_bend() {
        let net = junction();
        assert_eq!(net.lanes[0].next, vec![1]);
        let mut car = AiState::new(0, 0.0, 7);
        car.speed = 13.9;
        car.plan_next(&net);
        assert_eq!(car.upcoming().collect::<Vec<_>>(), vec![1, 2]);
        // the bend allows sqrt(2.8 × 14) ≈ 6.3 m/s; 60 m before it the car may still go fast
        let far = car.curve_speed(&net);
        assert!(far > 13.0, "60 m before the bend: {far}");
        let dt = 1.0 / 30.0;
        let mut entered = None;
        for _ in 0..600 {
            if !car.advance(&net, dt, None, None) {
                break; // the end of the test road
            }
            if car.lane == 1 && entered.is_none() {
                entered = Some(car.speed);
            }
        }
        let v = entered.expect("reached the bend");
        assert!(v < 7.5, "entered the bend at {v} m/s");
    }

    #[test]
    fn a_turning_lane_is_signalled_in_advance() {
        let mut net = junction();
        net.lanes[1].turn = 2;
        let mut car = AiState::new(0, 0.0, 7);
        car.speed = 10.0;
        car.plan_next(&net);
        let dt = 1.0 / 30.0;
        let mut first_on = None;
        for _ in 0..900 {
            car.advance(&net, dt, None, None);
            car.update_blinker(&net);
            if car.blinker == 2 && first_on.is_none() {
                first_on = Some((car.lane, net.lanes[0].length() - car.s));
            }
            if car.lane == 2 && car.s > 10.0 {
                assert_eq!(car.blinker, 0, "indicator off after the turn");
                break;
            }
        }
        let (lane, before) = first_on.expect("indicator on");
        assert_eq!(lane, 0);
        assert!(
            before >= 24.0,
            "indicator on only {before} m before the turn"
        );
    }

    /// Einm_erzgebirgs.sco's "Main" light: red and yellow 2 s, green 11 s, yellow 3 s,
    /// red for the rest of the 38 s cycle.
    fn erzgebirgs() -> TrafficLightController {
        TrafficLightController::from_program(
            vec![(vec![(3, 2.0), (6, 11.0), (9, 3.0), (0, 0.0)], None)],
            Some(38.0),
            &[],
            &[],
        )
    }

    #[test]
    fn a_light_shows_every_aspect_for_its_time_at_any_frame_rate() {
        for dt in [1.0 / 144.0, 1.0 / 30.0, 0.1, 0.25] {
            let mut c = erzgebirgs();
            c.start(0.0);
            // (aspect, seconds) as the lamps show it over two cycles
            let mut seen: Vec<(Aspect, f32)> = Vec::new();
            let mut t = 0.0f32;
            while t < 76.0 {
                let a = TrafficLightController::aspect(c.state(0));
                match seen.last_mut() {
                    Some(last) if last.0 == a => last.1 += dt,
                    _ => seen.push((a, dt)),
                }
                c.advance(dt);
                t += dt;
            }
            let order: Vec<Aspect> = seen.iter().map(|s| s.0).collect();
            assert_eq!(
                &order[..5],
                &[
                    Aspect::RedYellow,
                    Aspect::Green,
                    Aspect::Yellow,
                    Aspect::Red,
                    Aspect::RedYellow
                ],
                "dt {dt}"
            );
            let tol = dt + 1e-3;
            assert!(
                (seen[0].1 - 2.0).abs() <= tol,
                "red-yellow {} at dt {dt}",
                seen[0].1
            );
            assert!(
                (seen[1].1 - 11.0).abs() <= tol,
                "green {} at dt {dt}",
                seen[1].1
            );
            assert!(
                (seen[2].1 - 3.0).abs() <= tol,
                "yellow {} at dt {dt}",
                seen[2].1
            );
            assert!(
                (seen[3].1 - 22.0).abs() <= tol,
                "red {} at dt {dt}",
                seen[3].1
            );
        }
    }

    #[test]
    fn stock_state_codes_mean_what_the_lamp_scripts_show() {
        assert_eq!(TrafficLightController::lamps(0), (true, false, false));
        assert_eq!(TrafficLightController::lamps(3), (true, true, false));
        assert_eq!(TrafficLightController::lamps(6), (false, false, true));
        assert_eq!(TrafficLightController::lamps(9), (false, true, false));
        assert_eq!(TrafficLightController::lamps(12), (false, false, false));
        assert!(
            TrafficLightController::allows_go(6)
                && !TrafficLightController::allows_go(9)
                && !TrafficLightController::allows_go(3)
        );
        // the clock starts from the time of day: two crossings with one cycle run in step
        let (mut a, mut b) = (erzgebirgs(), erzgebirgs());
        a.start(8.0 * 3600.0 + 10.0);
        b.start(8.0 * 3600.0 + 10.0);
        assert_eq!(a.time, b.time);
        // 28 810 s = 758 cycles and 6 s: green
        assert!((a.time - 6.0).abs() < 1e-6);
        assert_eq!(a.state(0), 6);
    }

    #[test]
    fn a_level_crossing_waits_for_its_train() {
        // bue_falks_ohe.sco: road green until a train asks at light 0 (stop at 1 s), barrier
        // down while it is still there (stop at 16 s)
        let mut c = TrafficLightController::from_program(
            vec![
                (vec![(0, 15.0), (6, 4.0), (0, 1.0)], None),
                (vec![(6, 2.0), (9, 12.0), (0, 5.0), (3, 1.0)], None),
            ],
            Some(22.0),
            &[[0.0, 1.0, 1.0], [0.0, 16.0, 0.0]],
            &[],
        );
        c.start(0.0);
        for _ in 0..600 {
            c.advance(0.1);
        }
        assert!(
            (c.time - 1.0).abs() < 1e-6 && c.held,
            "holding at {}",
            c.time
        );
        assert_eq!(c.state(1), 6, "road green while no train comes");
        c.request[0] = true;
        let mut t = 0.0;
        while t < 20.0 {
            c.advance(0.1);
            t += 0.1;
        }
        assert!(
            (c.time - 16.0).abs() < 1e-6 && c.held,
            "the train is still there: holding at {}",
            c.time
        );
        assert_eq!(c.state(0), 6);
        assert_eq!(c.state(1), 0, "road red while the train passes");
        c.request[0] = false;
        c.advance(1.0);
        assert!((c.time - 17.0).abs() < 1e-3, "{}", c.time);
    }

    #[test]
    fn a_bus_phase_is_skipped_when_no_bus_comes() {
        // Kreuz_Heerstr_Pillnitzer_Reimer.sco: the bus light 3 jumps from 51.5 to 61.5 s
        let mut c = TrafficLightController::from_program(
            vec![(
                vec![(0, 52.0), (3, 2.0), (6, 4.0), (9, 3.0), (0, 0.0)],
                Some(10.0),
            )],
            Some(64.0),
            &[],
            &[[0.0, 51.5, 1.0, 61.5]],
        );
        c.start(50.0);
        c.advance(2.0);
        assert!((c.time - 62.0).abs() < 1e-3, "jumped: {}", c.time);
        c.start(0.0);
        c.time = 50.0;
        c.request[0] = true;
        c.advance(3.0);
        assert!((c.time - 53.0).abs() < 1e-3);
        assert_eq!(c.state(0), 3, "the bus gets its phase");
    }

    #[test]
    fn a_backwards_jump_extends_green_once_per_cycle() {
        // Bowdenham crossings use a rewind to time zero to extend a green. It must not
        // restart the phase again when the replay reaches that source time.
        let mut c = TrafficLightController::from_program(
            vec![
                (vec![(6, 15.0), (9, 3.0), (0, 8.0), (8, 5.0)], None),
                (vec![(0, 18.0), (6, 8.0), (12, 5.0)], Some(3.0)),
            ],
            Some(31.0),
            &[],
            &[[1.0, 15.0, 1.0, 0.0]],
        );
        c.time = 14.9;
        let mut left_green = false;
        for _ in 0..500 {
            c.advance(0.1);
            left_green |= matches!(
                TrafficLightController::aspect(c.state(0)),
                Aspect::Yellow | Aspect::Red
            );
        }
        assert!(
            left_green,
            "the extended green still reached yellow and red"
        );
    }

    #[test]
    fn a_car_stops_at_the_line_without_braking_hard() {
        let net = junction();
        let mut car = AiState::new(0, 0.0, 3);
        car.speed = 13.9;
        car.accel = 1.5;
        car.decel = 2.2;
        car.plan_next(&net);
        let dt = 1.0 / 60.0;
        let line = 55.0;
        let mut hardest = 0.0f32;
        for _ in 0..1200 {
            let stop = line - car.s;
            car.drive(&net, dt, None, Some(stop));
            hardest = hardest.min(car.acc);
        }
        let front = car.s + car.front;
        assert!(car.speed < 0.01, "stopped: {}", car.speed);
        assert!(
            front <= line && front > line - 1.5,
            "front at {front}, line at {line}"
        );
        assert!(hardest > -3.5, "braked at {hardest} m/s²");
    }

    #[test]
    fn a_follower_keeps_its_distance_when_the_leader_brakes() {
        let net = junction();
        let mut car = AiState::new(0, 0.0, 5);
        car.speed = 12.0;
        car.plan_next(&net);
        let (mut lead_s, mut lead_v) = (30.0f32, 12.0f32);
        let dt = 1.0 / 60.0;
        let mut closest = f32::MAX;
        for k in 0..900 {
            // the leader brakes hard after a second and stays standing
            if k > 60 {
                lead_v = (lead_v - 6.0 * dt).max(0.0);
            }
            lead_s += lead_v * dt;
            let gap = lead_s - 2.5 - (car.s + car.front);
            closest = closest.min(gap);
            car.drive(
                &net,
                dt,
                Some(Lead {
                    gap,
                    speed: lead_v,
                    acc: if k > 60 && lead_v > 0.0 { -6.0 } else { 0.0 },
                }),
                None,
            );
        }
        assert!(closest > 0.8, "came within {closest} m");
        assert!(car.speed < 0.05);
    }

    #[test]
    fn right_of_way_between_paths() {
        // a crossing: a from the south going north, b from the east going west, c from the
        // north going south and turning left (east)
        let a = LaneBuilder::arc(
            DVec3::new(0.0, -10.0, 0.0),
            0.0,
            20.0,
            0.0,
            0.0,
            LaneKind::Street,
            3.0,
        );
        let b = LaneBuilder::arc(
            DVec3::new(10.0, 0.0, 0.0),
            270.0,
            20.0,
            0.0,
            0.0,
            LaneKind::Street,
            3.0,
        );
        let mut c = LaneBuilder::arc(
            DVec3::new(-1.5, 10.0, 0.0),
            180.0,
            10.0 * std::f64::consts::FRAC_PI_2,
            -10.0,
            0.0,
            LaneKind::Street,
            3.0,
        );
        c.turn = 1;
        let mut net = Network {
            lanes: vec![a, b, c],
            ..Default::default()
        };
        net.link(1.5);
        // equal priority: b comes from a's right
        assert!(net.must_yield(0, 1));
        assert!(!net.must_yield(1, 0));
        // the left turn waits for the oncoming car
        assert!(net.must_yield(2, 0));
        assert!(!net.must_yield(0, 2));
        // a [rule] priority beats the geometry
        net.lanes[0].priority = 192.0;
        net.lanes[1].priority = 64.0;
        assert!(!net.must_yield(0, 1));
        assert!(net.must_yield(1, 0));
    }

    #[test]
    fn right_of_way_on_the_left() {
        // the same crossing on a left-hand-traffic map: b (from a's right) now waits for a
        // (from b's left), and a right turn across the oncoming traffic waits, a left one not
        let a = LaneBuilder::arc(
            DVec3::new(0.0, -10.0, 0.0),
            0.0,
            20.0,
            0.0,
            0.0,
            LaneKind::Street,
            3.0,
        );
        let b = LaneBuilder::arc(
            DVec3::new(10.0, 0.0, 0.0),
            270.0,
            20.0,
            0.0,
            0.0,
            LaneKind::Street,
            3.0,
        );
        let mut c = LaneBuilder::arc(
            DVec3::new(-1.5, 10.0, 0.0),
            180.0,
            10.0 * std::f64::consts::FRAC_PI_2,
            -10.0,
            0.0,
            LaneKind::Street,
            3.0,
        );
        c.turn = 2;
        let mut net = Network {
            lanes: vec![a, b, c],
            left_hand: true,
            ..Default::default()
        };
        net.link(1.5);
        assert!(!net.must_yield(0, 1));
        assert!(net.must_yield(1, 0));
        assert!(net.must_yield(2, 0));
        net.lanes[2].turn = 1;
        assert!(!net.must_yield(2, 0));
        assert_eq!(net.oncoming_sign(), 1.0);
    }

    #[test]
    fn a_shallow_crossing_is_a_long_meeting_place() {
        // one junction object (source 2, same key): a straight lane and two lanes crossing
        // it, one square, one at 20°
        let key = |path: u16| {
            Some(LaneKey {
                tile: (0, 0),
                id: 1,
                path,
            })
        };
        let mut a = LaneBuilder::arc(
            DVec3::new(0.0, -20.0, 0.0),
            0.0,
            40.0,
            0.0,
            0.0,
            LaneKind::Street,
            3.0,
        );
        let mut b = LaneBuilder::arc(
            DVec3::new(20.0, 0.0, 0.0),
            270.0,
            40.0,
            0.0,
            0.0,
            LaneKind::Street,
            3.0,
        );
        let h = 20f64.to_radians();
        let mut c = LaneBuilder::arc(
            DVec3::new(-20.0 * h.sin(), -20.0 * h.cos(), 0.0),
            20.0,
            40.0,
            0.0,
            0.0,
            LaneKind::Street,
            3.0,
        );
        for (l, k) in [(&mut a, 0), (&mut b, 1), (&mut c, 2)] {
            l.source = 2;
            l.key = key(k);
        }
        let mut net = Network {
            lanes: vec![a, b, c],
            ..Default::default()
        };
        net.link(1.5);
        let square = net.crossings[0]
            .iter()
            .find(|x| x.other == 1)
            .expect("square crossing");
        let shallow = net.crossings[0]
            .iter()
            .find(|x| x.other == 2)
            .expect("shallow crossing");
        assert!((square.at - 20.0).abs() < 0.5 && (shallow.at - 20.0).abs() < 0.5);
        // square: the bodies touch within a car's width or so of the point
        assert!(square.before <= 3.0 && square.after <= 3.0, "{square:?}");
        // at 20° the centre lines stay within 2.6 m for 2.6 / sin 20° ≈ 7.6 m either side
        assert!(shallow.before >= 7.0 && shallow.after >= 7.0, "{shallow:?}");
    }

    #[test]
    fn a_driver_with_a_choice_keeps_out_of_a_dead_end() {
        // a lane that forks: one way ends after 50 m, the other runs round a long loop
        let a = LaneBuilder::arc(DVec3::ZERO, 0.0, 30.0, 0.0, 0.0, LaneKind::Street, 3.0);
        let dead = LaneBuilder::arc(
            DVec3::new(0.0, 30.0, 0.0),
            10.0,
            50.0,
            0.0,
            0.0,
            LaneKind::Street,
            3.0,
        );
        let on = LaneBuilder::arc(
            DVec3::new(0.0, 30.0, 0.0),
            350.0,
            700.0,
            0.0,
            0.0,
            LaneKind::Street,
            3.0,
        );
        let mut net = Network {
            lanes: vec![a, dead, on],
            ..Default::default()
        };
        net.link(1.5);
        assert_eq!(net.lanes[0].next.len(), 2);
        assert!(net.reach[1] < DEAD_END && net.reach[0] >= DEAD_END && net.reach[2] >= DEAD_END);
        for seed in 1..40 {
            let mut car = AiState::new(0, 0.0, seed);
            car.plan_next(&net);
            assert_eq!(car.planned_next, Some(2), "seed {seed}");
        }
    }

    #[test]
    fn the_way_has_no_steps_at_a_lane_joint() {
        // two lanes that meet 1.2 m apart: the way bends over the joint instead of jumping
        let a = LaneBuilder::arc(DVec3::ZERO, 0.0, 30.0, 0.0, 0.0, LaneKind::Street, 3.0);
        let b = LaneBuilder::arc(
            DVec3::new(1.2, 30.0, 0.0),
            0.0,
            30.0,
            0.0,
            0.0,
            LaneKind::Street,
            3.0,
        );
        let mut net = Network {
            lanes: vec![a, b],
            ..Default::default()
        };
        net.link(1.5);
        let mut car = AiState::new(0, 20.0, 1);
        car.plan_next(&net);
        let mut last = car.way_point(&net, -10.0);
        let mut d = -10.0;
        while d < 25.0 {
            d += 0.25;
            let p = car.way_point(&net, d);
            assert!(
                (p - last).length() < 0.3,
                "step of {} m at {d}",
                (p - last).length()
            );
            last = p;
        }
    }

    /// The southbound half of a road through a junction: a lane B (60 m) into the junction's
    /// straight path J1 (10 m), a left turn J2 from the east into the same exit, and the lane
    /// A (50 m) after the junction.
    fn oncoming_road() -> Network {
        let a = LaneBuilder::arc(
            DVec3::new(-3.0, 150.0, 0.0),
            180.0,
            50.0,
            0.0,
            0.0,
            LaneKind::Street,
            3.0,
        );
        let j1 = LaneBuilder::arc(
            DVec3::new(-3.0, 160.0, 0.0),
            180.0,
            10.0,
            0.0,
            0.0,
            LaneKind::Street,
            3.0,
        );
        let j2 = LaneBuilder::arc(
            DVec3::new(5.0, 158.0, 0.0),
            270.0,
            8.0 * std::f64::consts::FRAC_PI_2,
            -8.0,
            0.0,
            LaneKind::Street,
            3.0,
        );
        let b = LaneBuilder::arc(
            DVec3::new(-3.0, 220.0, 0.0),
            180.0,
            60.0,
            0.0,
            0.0,
            LaneKind::Street,
            3.0,
        );
        assert!(
            (j2.end() - DVec3::new(-3.0, 150.0, 0.0)).length() < 0.05,
            "{:?}",
            j2.end()
        );
        let mut net = Network {
            lanes: vec![a, j1, j2, b],
            ..Default::default()
        };
        net.link(1.5);
        net
    }

    #[test]
    fn upstream_walks_back_through_the_junction() {
        let net = oncoming_road();
        assert_eq!(net.prev[0].len(), 2, "{:?}", net.prev);
        // 20 m into A, looking 100 m back: A itself, both junction paths and the lane before
        let up = net.upstream(0, 20.0, 100.0, 16);
        let find = |l: usize| up.iter().find(|e| e.0 == l).copied();
        assert_eq!(up[0], (0, 0.0, None));
        let (_, off1, into1) = find(1).expect("J1");
        assert!((off1 + 10.0).abs() < 0.01 && into1 == Some(0), "{up:?}");
        let (_, off2, into2) = find(2).expect("J2");
        assert!(
            (off2 + 8.0 * std::f32::consts::FRAC_PI_2).abs() < 0.05 && into2 == Some(0),
            "{up:?}"
        );
        let (_, off3, into3) = find(3).expect("B");
        assert!((off3 + 70.0).abs() < 0.01 && into3 == Some(1), "{up:?}");
        // a car 15 m into B is at 15 - 70 = -55 in A's distances: 75 m before the place
        // looking only 25 m back, the lanes that end within reach are there, B is not
        let near = net.upstream(0, 20.0, 25.0, 16);
        assert!(
            near.iter().any(|e| e.0 == 1) && !near.iter().any(|e| e.0 == 3),
            "{near:?}"
        );
        // and the list is capped
        assert_eq!(net.upstream(0, 20.0, 100.0, 2).len(), 2);
    }

    #[test]
    fn an_acceleration_cap_holds_a_car_back() {
        let net = junction();
        let mut car = AiState::new(0, 0.0, 7);
        car.accel = 2.5;
        car.plan_next(&net);
        car.accel_cap = Some(1.0);
        for _ in 0..30 {
            car.drive(&net, 1.0 / 30.0, None, None);
        }
        assert!(car.speed > 0.9 && car.speed < 1.0 + 1e-3, "{}", car.speed);
        car.accel_cap = None;
        for _ in 0..30 {
            car.drive(&net, 1.0 / 30.0, None, None);
        }
        assert!(car.speed > 3.0, "{}", car.speed);
    }

    #[test]
    fn arrival_times() {
        assert_eq!(arrival_time(-1.0, 5.0, 1.0, 10.0), 0.0);
        // at a steady 10 m/s
        assert!((arrival_time(100.0, 10.0, 1.0, 10.0) - 10.0).abs() < 1e-4);
        // from a standstill at 2 m/s² without reaching the limit: sqrt(2 d / a)
        assert!((arrival_time(50.0, 0.0, 2.0, 20.0) - 50f32.sqrt()).abs() < 1e-3);
        // 5 s up to 10 m/s over 25 m, then 125 m at 10 m/s
        assert!((arrival_time(150.0, 0.0, 2.0, 10.0) - 17.5).abs() < 1e-3);
        // a car faster than the limit keeps its speed
        assert!((arrival_time(60.0, 15.0, 2.0, 10.0) - 4.0).abs() < 1e-3);
    }

    #[test]
    fn moving_back_in_clears_the_oncoming_lane_part_way() {
        // 3.3 m over, 2.05 m needed to the oncoming lane's middle: 62 % of the offset
        let t = ramp_progress_for(3.3, 2.05);
        assert!((smooth01(t) - 2.05 / 3.3).abs() < 1e-4, "{t}");
        assert!(t > 0.5 && t < 0.7, "{t}");
        assert_eq!(ramp_progress_for(3.3, 0.0), 0.0);
        assert_eq!(ramp_progress_for(2.0, 2.5), 1.0);
        assert!(ramp_progress_for(3.3, 1.5) < t);
    }
