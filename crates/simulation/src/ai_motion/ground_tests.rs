//! Real bicycle/body realization on synthetic drive surfaces, not a kinematic height stub.
use super::*;
use crate::rigid::GroundProbe;

#[test]
fn closed_loop_motion_feedback_does_not_amplify_the_commanded_speed() {
    let mut lane = ::traffic::LaneBuilder::polyline(
        vec![DVec3::ZERO, DVec3::new(0.0, 10000.0, 0.0)],
        ::traffic::LaneKind::Street,
        3.0,
    );
    lane.speed_limit_kmh = 50.0;
    let net = ::traffic::Network {
        lanes: vec![lane],
        ..Default::default()
    };
    let mut state = ::traffic::AiState::new(0, 10.0, 1);
    state.desire = 1.0;
    let mut body = AiBody::new(&super::tests::golf(), MotionKind::Road);
    body.place(&|d| state.way_point(&net, d), None, Some(&flat(0.0)), 0.0);
    for tick in 0..5000 {
        state.drive(&net, 0.02, None, None);
        body.step(
            0.02,
            state.speed,
            &|d| state.way_point(&net, d),
            None,
            Some(&flat(0.0)),
        );
        state.commit_feedback(
            &net,
            ::traffic::RealizedMotion {
                pose: body.position,
                heading_deg: body.heading as f32,
                speed: body.realized_speed(0.02),
                half_width: 0.9,
            },
        );
        assert!(
            state.speed <= 50.0 / 3.6 + 0.1,
            "tick {tick}: {} m/s",
            state.speed
        );
    }
}

fn flat(z: f64) -> impl crate::rigid::Ground {
    move |_: f64, _: f64, top: f64| GroundProbe {
        below: (z <= top).then_some(z),
        above: (z > top).then_some(z),
        normal: None,
    }
}

#[test]
fn road_height_is_corrected_both_above_and_below_the_authored_path() {
    for z in [-2.7, -0.8, -0.2, 0.0, 0.9, 1.3, 2.7] {
        let mut body = AiBody::new(&super::tests::golf(), MotionKind::Road);
        body.place(
            &|d| DVec3::new(0.0, d as f64, 0.0),
            None,
            Some(&flat(z)),
            0.0,
        );
        assert!(
            (body.position.z - z).abs() < 1e-6,
            "road {z}, body {}",
            body.position.z
        );
        assert!(body.ground_supported);
    }
}

#[test]
fn a_front_axle_beyond_the_asphalt_does_not_stall_traffic_at_a_dead_end() {
    let lane = ::traffic::LaneBuilder::polyline(
        vec![DVec3::new(0.0, 0.0, 13.3), DVec3::new(0.0, 30.0, 13.3)],
        ::traffic::LaneKind::Street,
        3.0,
    );
    let net = ::traffic::Network {
        lanes: vec![lane],
        ..Default::default()
    };
    let ground = |_: f64, y: f64, top: f64| {
        // The real-map failure: front tyres leave the street, revealing lower terrain.
        let h = if y <= 30.0 { 13.3 } else { 10.59 };
        GroundProbe {
            below: (h <= top).then_some(h),
            above: (h > top).then_some(h),
            normal: None,
        }
    };
    let mut state = ::traffic::AiState::new(0, 22.0, 1);
    state.speed = 5.0;
    let mut body = AiBody::new(&super::tests::golf(), MotionKind::Road);
    body.place(
        &|d| state.way_point(&net, d),
        None,
        Some(&ground),
        state.speed,
    );
    let mut ended = false;
    for _ in 0..500 {
        if !state.drive(&net, 0.02, None, None) {
            ended = true;
            break;
        }
        body.step(
            0.02,
            state.speed,
            &|d| state.way_point(&net, d),
            None,
            Some(&ground),
        );
        assert!(
            body.ground_supported,
            "partial support must not freeze road progress"
        );
        assert!(
            (body.position.z - 13.3).abs() < 1e-4,
            "front axle dipped onto terrain: {}",
            body.position.z
        );
        state.commit_feedback(
            &net,
            ::traffic::RealizedMotion {
                pose: body.position,
                heading_deg: body.heading as f32,
                speed: body.realized_speed(0.02),
                half_width: 0.9,
            },
        );
    }
    assert!(ended, "vehicle never reached its normal path-end removal");
}

#[test]
fn a_large_path_offset_does_not_stop_a_car_on_an_available_surface() {
    for road in [-2.7, 2.7] {
        let mut body = AiBody::new(&super::tests::golf(), MotionKind::Road);
        body.place(
            &|d| DVec3::new(0.0, d as f64, 0.0),
            None,
            Some(&flat(road)),
            0.0,
        );
        for tick in 0..250 {
            let distance = (tick + 1) as f64 * 0.1;
            body.step(
                0.02,
                5.0,
                &|d| DVec3::new(0.0, distance + d as f64, 0.0),
                None,
                Some(&flat(road)),
            );
            assert!(body.ground_supported);
            assert!((body.position.z - road).abs() < 1e-5);
            assert!((body.realized_speed(0.02) - 5.0).abs() < 1e-5);
        }
        assert!((body.position.y - 25.0).abs() < 0.01);
    }
}

#[test]
fn a_short_surface_query_gap_does_not_drop_a_car_onto_buried_terrain() {
    let ground = |_: f64, y: f64, top: f64| {
        let z = if (10.0..14.0).contains(&y) { -2.7 } else { 0.0 };
        GroundProbe {
            below: (z <= top).then_some(z),
            above: (z > top).then_some(z),
            normal: None,
        }
    };
    let mut body = AiBody::new(&super::tests::golf(), MotionKind::Road);
    body.place(
        &|d| DVec3::new(0.0, d as f64, 0.0),
        None,
        Some(&ground),
        0.0,
    );
    for tick in 0..250 {
        let distance = (tick + 1) as f64 * 0.1;
        body.step(
            0.02,
            5.0,
            &|d| DVec3::new(0.0, distance + d as f64, 0.0),
            None,
            Some(&ground),
        );
        assert!(body.ground_supported);
        assert!(
            body.position.z.abs() < 0.01,
            "tyres dropped through continuous road at y={}: {}",
            body.position.y,
            body.position.z
        );
    }
    assert!((body.position.y - 25.0).abs() < 0.01);
}

#[test]
fn a_continuous_road_with_a_wrong_path_grade_keeps_its_physical_height() {
    for direction in [-1.0, 1.0] {
        let ground = |_: f64, y: f64, top: f64| {
            let z = direction * 2.7 * (y / 50.0).clamp(0.0, 1.0);
            GroundProbe {
                below: (z <= top).then_some(z),
                above: (z > top).then_some(z),
                normal: None,
            }
        };
        let mut body = AiBody::new(&super::tests::golf(), MotionKind::Road);
        body.place(
            &|d| DVec3::new(0.0, d as f64, 0.0),
            None,
            Some(&ground),
            0.0,
        );
        for tick in 0..750 {
            let distance = (tick + 1) as f64 * 0.1;
            body.step(
                0.02,
                5.0,
                &|d| DVec3::new(0.0, distance + d as f64, 0.0),
                None,
                Some(&ground),
            );
            let road = direction * 2.7 * (body.position.y / 50.0).clamp(0.0, 1.0);
            assert!(body.ground_supported);
            assert!(
                (body.position.z - road).abs() < 0.16,
                "body {} road {road}",
                body.position.z
            );
            assert!(
                body.pitch_deg.abs() < 8.0,
                "unexpected pitch {}",
                body.pitch_deg
            );
            assert!(
                body.bank_deg.abs() < 1.0,
                "unexpected bank {}",
                body.bank_deg
            );
        }
        assert!((body.position.y - 75.0).abs() < 0.01);
    }
}

#[test]
fn one_wheel_query_on_buried_terrain_does_not_rotate_the_whole_car() {
    let ground = |x: f64, y: f64, top: f64| {
        let z = if x < 0.0 && (10.0..15.0).contains(&y) {
            0.0
        } else {
            0.9
        };
        GroundProbe {
            below: (z <= top).then_some(z),
            above: (z > top).then_some(z),
            normal: None,
        }
    };
    let mut body = AiBody::new(&super::tests::golf(), MotionKind::Road);
    body.place(
        &|d| DVec3::new(0.0, d as f64, 0.9),
        None,
        Some(&ground),
        0.0,
    );
    for tick in 0..250 {
        let distance = (tick + 1) as f64 * 0.1;
        body.step(
            0.02,
            5.0,
            &|d| DVec3::new(0.0, distance + d as f64, 0.9),
            None,
            Some(&ground),
        );
        assert!(body.ground_supported);
        assert!(
            (body.position.z - 0.9).abs() < 0.01,
            "road edge sank body {}",
            body.position.z
        );
        assert!(
            body.bank_deg.abs() < 1.0,
            "road edge rotated body {}",
            body.bank_deg
        );
    }
}

#[test]
fn a_bridge_does_not_pull_a_car_off_the_road_below_it() {
    let ground = |_: f64, _: f64, top: f64| GroundProbe {
        below: Some(if top >= 5.0 { 5.0 } else { 0.0 }),
        above: (top < 5.0).then_some(5.0),
        normal: None,
    };
    for road in [0.0, 5.0] {
        let mut body = AiBody::new(&super::tests::golf(), MotionKind::Road);
        body.place(
            &|d| DVec3::new(0.0, d as f64, road),
            None,
            Some(&ground),
            0.0,
        );
        assert!((body.position.z - road).abs() < 1e-6);
    }
}

#[test]
fn missing_streamed_contact_holds_the_previous_plane_and_recovers() {
    let mut body = AiBody::new(&super::tests::golf(), MotionKind::Road);
    let way = |d| DVec3::new(0.0, d as f64, 0.0);
    body.place(&way, None, Some(&flat(0.8)), 0.0);
    let absent = |_: f64, _: f64, _: f64| GroundProbe::default();
    for _ in 0..100 {
        body.step(0.02, 0.0, &way, None, Some(&absent));
        assert!((body.position.z - 0.8).abs() < 1e-5);
        assert!(!body.ground_supported);
    }
    body.step(0.02, 0.0, &way, None, Some(&flat(0.8)));
    assert!(body.ground_supported);
}

#[test]
fn unavailable_ground_stops_motion_without_dropping_the_body() {
    let mut body = AiBody::new(&super::tests::golf(), MotionKind::Road);
    let way = |d| DVec3::new(0.0, d as f64, 0.0);
    body.place(&way, None, Some(&flat(0.8)), 0.0);
    let absent = |_: f64, _: f64, _: f64| GroundProbe::default();
    body.step(0.02, 3.0, &way, None, Some(&absent));
    let pose = body.position;
    for _ in 0..100 {
        body.step(0.02, 10.0, &way, None, Some(&absent));
    }
    assert_eq!(body.position, pose);
    assert_eq!(body.realized_speed(0.02), 0.0);
}

#[test]
#[ignore = "requires installed OMSI content through OMSI_ROOT"]
fn installed_car_and_bus_apply_their_static_model_offset_exactly_once() {
    let root = std::path::PathBuf::from(std::env::var_os("OMSI_ROOT").expect("OMSI_ROOT"));
    for file in [
        "Vehicles/VW_Golf_2/ai_vw_golf_2.bus",
        "Vehicles/MAN_SD200/MAN_SD80.bus",
    ] {
        let ty = std::sync::Arc::new(crate::VehicleType::load_ai(&root, &root.join(file)).unwrap());
        let mut vehicle =
            crate::VehicleInstance::new(ty, crate::VehicleHost::new(crate::SimClock::default()));
        let lift = vehicle.ai_rest_offset().0 as f64;
        for road in [-2.7, 0.9, 2.7] {
            let ground = |x: f64, y: f64, top: f64| {
                let z = if x < 0.0 && (10.0..15.0).contains(&y) {
                    road - 0.9
                } else {
                    road
                };
                GroundProbe {
                    below: (z <= top).then_some(z),
                    above: (z > top).then_some(z),
                    normal: None,
                }
            };
            let mut body = AiBody::new(&vehicle.ty.def, MotionKind::Road);
            body.place(
                &|d| DVec3::new(0.0, d as f64, 0.0),
                None,
                Some(&ground),
                0.0,
            );
            for tick in 0..250 {
                let distance = (tick + 1) as f64 * 0.1;
                body.step(
                    0.02,
                    5.0,
                    &|d| DVec3::new(0.0, distance + d as f64, 0.0),
                    None,
                    Some(&ground),
                );
                body.apply(&mut vehicle);
                assert!(
                    (vehicle.position.z - (road + lift)).abs() < 1e-5,
                    "{file} at y={}: {}",
                    vehicle.position.y,
                    vehicle.position.z
                );
                body.apply(&mut vehicle);
                assert!(
                    (vehicle.position.z - (road + lift)).abs() < 1e-5,
                    "offset accumulated: {file}"
                );
                assert!(body.ground_supported);
                assert!(
                    body.bank_deg.abs() < 1.0,
                    "road edge rotated {file}: {}",
                    body.bank_deg
                );
            }
            assert!((body.position.y - 25.0).abs() < 0.01);
        }
    }
}

#[test]
fn a_missing_wheel_does_not_sink_a_supported_axle_back_to_path_height() {
    let ground = |x: f64, _: f64, _: f64| GroundProbe {
        below: (x > 0.0).then_some(0.8),
        ..Default::default()
    };
    let mut body = AiBody::new(&super::tests::golf(), MotionKind::Road);
    body.place(
        &|d| DVec3::new(0.0, d as f64, 0.0),
        None,
        Some(&ground),
        0.0,
    );
    assert!((body.position.z - 0.8).abs() < 1e-6);
    assert!(body.bank_deg.abs() < 0.01);
}

#[test]
fn sloping_and_banked_road_uses_actual_wheel_contacts() {
    let ground = |x: f64, y: f64, top: f64| {
        let z = 0.6 + y * 0.05 + x * 0.03;
        GroundProbe {
            below: (z <= top).then_some(z),
            above: (z > top).then_some(z),
            normal: None,
        }
    };
    let mut body = AiBody::new(&super::tests::golf(), MotionKind::Road);
    body.place(
        &|d| DVec3::new(0.0, d as f64, d as f64 * 0.05),
        None,
        Some(&ground),
        0.0,
    );
    assert!((body.position.z - 0.6).abs() < 1e-5);
    assert!((body.pitch_deg - 0.05f32.atan().to_degrees()).abs() < 0.01);
    assert!((body.bank_deg + 0.03f32.atan().to_degrees()).abs() < 0.01);
}
