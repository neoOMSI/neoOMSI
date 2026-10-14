#[test]
fn a_wheel_with_buttons_past_the_table_counts_them_in_order() {
    let moza = super::key_bitmap_buttons(
        "ffffffff ffffffffffffffff ffff000000000000 0 0 0 0 ffff00000000 0 0 0 0",
    );
    assert_eq!(moza.len(), 128);
    if cfg!(target_os = "linux") {
        assert_eq!(super::button_index(&moza, 0x120), Some(0));
        assert_eq!(super::button_index(&moza, 0x12f), Some(15));
        assert_eq!(super::button_index(&moza, 0x270), Some(16));
        assert_eq!(super::button_index(&moza, 0x2c0), Some(96));
        let pad: Vec<u32> = vec![0x130, 0x131, 0x133, 0x134];
        assert_eq!(super::button_index(&pad, 0x133), None);
    }
}

#[test]
fn a_device_lists_as_many_buttons_as_its_highest_number() {
    let moza = super::key_bitmap_buttons(
        "ffffffff ffffffffffffffff ffff000000000000 0 0 0 0 ffff00000000 0 0 0 0",
    );
    assert_eq!(super::button_count(&moza), 128);
    if cfg!(target_os = "linux") {
        assert_eq!(super::button_count(&[0x130, 0x131, 0x133, 0x134]), 5);
    }
    assert_eq!(super::button_count(&[]), 0);
}

#[test]
fn buttons_count_as_directinput_does() {
    if cfg!(target_os = "macos") {
        assert_eq!(super::code_button(0x9_0001), Some(0));
        assert_eq!(super::code_button(0x9_0010), Some(15));
    }
    if cfg!(target_os = "linux") {
        assert_eq!(super::code_button(0x1_0120), Some(0));
        assert_eq!(super::code_button(0x1_02c0), Some(16));
    }
    if cfg!(windows) {
        assert_eq!(super::code_button(3), Some(3));
        // an analog trigger (WGI axis code) is no numbered button
        assert_eq!(super::code_button(0x1_0004), None);
        // the D-pad is the first hat, not dropped
        assert_eq!(super::code_button(u32::MAX - 3), Some(super::HAT_BUTTONS));
        assert_eq!(super::code_button(u32::MAX), Some(super::HAT_BUTTONS + 3));
    }
}

#[test]
fn the_wheel_is_pulled_to_the_middle_harder_at_speed() {
    let mut t = 0.0;
    let f = |kmh| super::FfInput {
        on: true,
        kmh,
        dt: 0.016,
        ..Default::default()
    };
    let slow = super::wheel_force(&f(10.0), 0.5, 0.5, &mut t, 1.0, 1.0);
    let fast = super::wheel_force(&f(60.0), 0.5, 0.5, &mut t, 1.0, 1.0);
    assert!(slow < 0.0 && fast < slow, "{slow} {fast}");
    // Moving to the right at a standstill, it is held back by tyre scrub.
    let turning = super::wheel_force(&f(0.0), 0.0, -0.05, &mut t, 1.0, 1.0);
    assert!(turning < 0.0);
}

#[test]
fn a_parked_wheel_is_not_pulled_to_the_middle() {
    let mut t = 0.0;
    for kmh in [-0.5, -0.1, 0.0, 0.1, 0.5] {
        // Include residual lateral acceleration from the body's suspension.
        let f = super::FfInput {
            on: true,
            kmh,
            lateral_accel: 3.0,
            dt: 0.016,
            ..Default::default()
        };
        for x in [-1.0, -0.5, 0.0, 0.5, 1.0] {
            let force = super::wheel_force(&f, x, x, &mut t, 1.0, 0.0);
            assert_eq!(force, 0.0, "kmh={kmh}, steering={x}");
        }
    }
}

#[test]
fn parking_drag_opposes_turning_in_both_directions() {
    let mut t = 0.0;
    let f = super::FfInput {
        on: true,
        dt: 0.016,
        ..Default::default()
    };
    for x in [-0.5, 0.5] {
        let right = super::wheel_force(&f, x, x - 0.01, &mut t, 1.0, 0.0);
        let left = super::wheel_force(&f, x, x + 0.01, &mut t, 1.0, 0.0);
        assert!(right < 0.0 && left > 0.0, "{right} {left}");
        assert!((right + left).abs() < 1e-6, "{right} {left}");
    }
}

#[test]
fn self_aligning_torque_builds_up_as_the_bus_rolls_in_either_direction() {
    let mut t = 0.0;
    let force = |kmh, t: &mut f32| {
        let f = super::FfInput {
            on: true,
            kmh,
            dt: 0.016,
            ..Default::default()
        };
        super::wheel_force(&f, 0.5, 0.5, t, 1.0, 0.0)
    };
    let mut previous = 0.0;
    for kmh in [0.51, 1.0, 2.0, 3.0, 4.0, 5.0] {
        let forward = force(kmh, &mut t);
        let reverse = force(-kmh, &mut t);
        assert!(forward < previous, "kmh={kmh}: {forward} >= {previous}");
        assert_eq!(forward, reverse);
        previous = forward;
    }
}

#[test]
fn steering_resistance_drops_as_the_bus_starts_rolling() {
    let mut t = 0.0;
    let f = |kmh| super::FfInput {
        on: true,
        kmh,
        dt: 0.016,
        ..Default::default()
    };
    let parked = super::wheel_force(&f(0.0), 0.2, 0.15, &mut t, 1.0, 0.0);
    let moving = super::wheel_force(&f(30.0), 0.2, 0.15, &mut t, 1.0, 0.0);
    assert!(parked < moving && moving < 0.0, "{parked} {moving}");
}

#[test]
fn a_returning_wheel_is_not_stopped_by_parking_drag() {
    let mut t = 0.0;
    let f = super::FfInput {
        on: true,
        kmh: 20.0,
        dt: 0.016,
        ..Default::default()
    };
    let right = super::wheel_force(&f, 0.5, 0.55, &mut t, 1.0, 0.0);
    let left = super::wheel_force(&f, -0.5, -0.55, &mut t, 1.0, 0.0);
    assert!(right < 0.0 && left > 0.0, "{right} {left}");
}

#[test]
fn a_ninety_degree_turn_has_return_torque_across_wheel_ranges() {
    let mut t = 0.0;
    let f = super::FfInput {
        on: true,
        kmh: 30.0,
        dt: 0.016,
        ..Default::default()
    };
    for range in [900.0, 1080.0, 1800.0, 2880.0] {
        let x = 90.0 / (range * 0.5);
        let right = super::wheel_force(&f, x, x, &mut t, 1.0, 0.0);
        let left = super::wheel_force(&f, -x, -x, &mut t, 1.0, 0.0);
        assert!(right < -0.05, "range={range}: {right}");
        assert_eq!(right, -left);
    }
}

#[test]
fn small_remaining_angles_keep_returning_while_the_bus_rolls() {
    let mut t = 0.0;
    for kmh in [5.0, 20.0, 70.0] {
        let f = super::FfInput {
            on: true,
            kmh,
            dt: 1.0 / 60.0,
            ..Default::default()
        };
        for x in [0.005, 0.01, 0.05] {
            // Already returning slowly: do not let steering drag cancel
            // the last few degrees of return on a wheel with friction.
            let right = super::wheel_force(&f, x, x + 0.001, &mut t, 1.0, 0.0);
            let left = super::wheel_force(&f, -x, -x - 0.001, &mut t, 1.0, 0.0);
            assert!(
                right < -0.02 && left > 0.02,
                "kmh={kmh}, x={x}: {right} {left}"
            );
            assert!((right + left).abs() < 1e-6);
        }
    }
}

#[test]
fn return_torque_fades_smoothly_at_the_physical_centre() {
    let mut t = 0.0;
    let f = super::FfInput {
        on: true,
        kmh: 30.0,
        dt: 0.016,
        ..Default::default()
    };
    let mut previous: f32 = 1.0;
    for x in [0.2, 0.1, 0.01, 0.001, 0.0001, 0.00001, 0.0] {
        let force = super::wheel_force(&f, x, x, &mut t, 1.0, 0.0);
        assert!(force.abs() < previous, "x={x}: {force}");
        if x == 0.0 {
            assert_eq!(force, 0.0);
        } else {
            assert!(force < 0.0);
        }
        if x <= 0.0001 {
            assert!(force.abs() < 0.002, "x={x}: {force}");
        }
        previous = force.abs();
    }
}

#[test]
fn a_moving_wheel_is_damped_when_it_crosses_the_centre() {
    let mut t = 0.0;
    for kmh in [5.0, 20.0, 70.0] {
        let f = super::FfInput {
            on: true,
            kmh,
            dt: 0.02,
            ..Default::default()
        };
        // At the centre there is no spring torque. The remaining force must
        // slow a crossing wheel in either direction rather than accelerate it.
        let right = super::wheel_force(&f, 0.0, -0.01, &mut t, 1.0, 0.0);
        let left = super::wheel_force(&f, 0.0, 0.01, &mut t, 1.0, 0.0);
        assert!(right < -0.015 && left > 0.015, "kmh={kmh}: {right} {left}");
        assert!((right + left).abs() < 1e-6);
        assert_eq!(super::wheel_force(&f, 0.0, 0.0, &mut t, 1.0, 0.0), 0.0);
        // Steering strength controls damping too, independently of vibration.
        assert_eq!(super::wheel_force(&f, 0.0, -0.01, &mut t, 0.0, 0.0), 0.0);
    }
}

#[test]
fn the_same_wheel_velocity_gives_the_same_force_at_different_frame_rates() {
    let mut t = 0.0;
    for velocity in [-0.5, 0.5] {
        let mut reference: Option<f32> = None;
        for fps in [20.0, 30.0, 60.0, 144.0] {
            let dt = 1.0 / fps;
            let f = super::FfInput {
                on: true,
                kmh: 30.0,
                dt,
                ..Default::default()
            };
            let force = super::wheel_force(&f, 0.05, 0.05 - velocity * dt, &mut t, 1.0, 0.0);
            if let Some(previous) = reference {
                assert!(
                    (force - previous).abs() < 1e-6,
                    "fps={fps}: {force} {previous}"
                );
            } else {
                reference = Some(force);
            }
        }
    }
}

#[test]
fn a_real_turn_adds_aligning_torque_but_a_parked_bus_does_not() {
    let mut t = 0.0;
    let mut f = super::FfInput {
        on: true,
        kmh: 30.0,
        dt: 0.016,
        ..Default::default()
    };
    let straight = super::wheel_force(&f, 0.2, 0.2, &mut t, 1.0, 0.0);
    f.lateral_accel = 3.0;
    let right_turn = super::wheel_force(&f, 0.2, 0.2, &mut t, 1.0, 0.0);
    assert!(right_turn < straight, "{straight} {right_turn}");
    f.kmh = 0.0;
    let parked = super::wheel_force(&f, 0.2, 0.2, &mut t, 1.0, 0.0);
    f.lateral_accel = 0.0;
    let parked_without_accel = super::wheel_force(&f, 0.2, 0.2, &mut t, 1.0, 0.0);
    assert_eq!(parked, parked_without_accel);
}

#[test]
fn steering_assist_lightens_turning_out() {
    let mut t = 0.0;
    let f = super::FfInput {
        on: true,
        kmh: 25.0,
        lateral_accel: 2.0,
        dt: 0.016,
        ..Default::default()
    };
    let held = super::wheel_force(&f, 0.5, 0.5, &mut t, 1.0, 0.0);
    let turning_out = super::wheel_force(&f, 0.5, 0.48, &mut t, 1.0, 0.0);
    let returning = super::wheel_force(&f, 0.5, 0.52, &mut t, 1.0, 0.0);
    // Assistance still lightens turning out. A moving return also includes
    // damping, so its net torque need not exceed the outward-turning torque.
    assert!(
        held < turning_out && turning_out < 0.0,
        "{held} {turning_out}"
    );
    assert!(returning < 0.0, "{returning}");
}

#[test]
fn centering_does_not_grow_linearly_to_full_lock() {
    let mut t = 0.0;
    let f = super::FfInput {
        on: true,
        kmh: 30.0,
        dt: 0.016,
        ..Default::default()
    };
    let quarter = super::wheel_force(&f, 0.25, 0.25, &mut t, 1.0, 0.0);
    let full = super::wheel_force(&f, 1.0, 1.0, &mut t, 1.0, 0.0);
    assert!(full < quarter && full > 2.5 * quarter, "{quarter} {full}");
}

#[test]
fn wheel_bumps_need_motion_and_a_suspension_or_impact_event() {
    assert_eq!(super::bump_strength(0.0, 0.0, 20.0), 0.0);
    assert_eq!(super::bump_strength(1.0, 0.0, 0.0), 0.0);
    assert_eq!(super::bump_strength(0.008, 0.08, 20.0), 0.0);
    assert!(super::bump_strength(0.015, 0.0, 20.0) < 0.05);
    assert!(super::bump_strength(0.05, 0.0, 20.0) > 0.4);
    assert!(super::bump_strength(0.1, 0.0, 20.0) > 0.9);
    assert!(super::bump_strength(0.0, 1.0, 20.0) > 0.5);
}

#[test]
fn settling_ignores_seams_and_keeps_slow_motion_as_it_was() {
    let dt = 1.0 / 60.0;
    let mut settled = Vec::new();
    // the first frame (or the first back in the bus) starts from the travel as it is
    assert_eq!(super::settle(&mut settled, &[0.09], dt), vec![0.0]);
    // a 1.5 cm seam for one step
    let seam = super::settle(&mut settled, &[0.105], dt)[0];
    assert!(super::bump_strength(seam, 0.0, 20.0) < 0.05);
    // a 6 cm step (a pothole's edge)
    let mut settled = vec![0.0];
    let step = super::settle(&mut settled, &[0.06], dt)[0];
    assert!(super::bump_strength(step, 0.0, 20.0) > 0.45);
    // brake dive at 0.3 m/s: about as strong as the old one-step rate made it
    let mut settled = vec![0.0];
    let mut travel = 0.0;
    let mut ripple = 0.0;
    for _ in 0..60 {
        travel += 0.3 * dt;
        ripple = super::settle(&mut settled, &[travel], dt)[0];
    }
    let old = (0.3 - 0.12) / 0.9;
    assert!(
        (super::bump_strength(ripple, 0.0, 20.0) - old).abs() < 0.05,
        "{ripple}"
    );
}

/// Where the wheel plays the shaking as its own periodic effect, the force set each
/// frame carries none of it.
#[test]
fn the_shaking_leaves_the_force_when_the_wheel_plays_it() {
    let f = super::FfInput {
        on: true,
        kmh: 0.0,
        vib_amp: 1.0,
        vib_period: 2.5,
        dt: 0.016,
        ..Default::default()
    };
    let mut t = 0.0;
    let soft = super::wheel_force(&f, 0.0, 0.0, &mut t, 0.0, 1.0);
    assert!(soft.abs() > 0.01, "{soft}");
    t = 0.0;
    let periodic = super::wheel_force(
        &super::FfInput { vib_amp: 0.0, ..f },
        0.0,
        0.0,
        &mut t,
        0.0,
        1.0,
    );
    assert_eq!(periodic, 0.0);
}

#[test]
fn a_wheel_bump_uses_the_device_vibration_strength() {
    let mut t = 0.0;
    let f = super::FfInput {
        on: true,
        kmh: 20.0,
        wheel_bump: 1.0,
        dt: 0.016,
        ..Default::default()
    };
    let off = super::wheel_force(&f, 0.0, 0.0, &mut t, 0.0, 0.0);
    t = 0.0;
    let on = super::wheel_force(&f, 0.0, 0.0, &mut t, 0.0, 1.0);
    assert_eq!(off, 0.0);
    assert!(on.abs() > 0.15, "{on}");
    assert!(on.abs() < 0.5, "{on}");
    let lighter = super::wheel_force(
        &super::FfInput {
            wheel_bump: 0.25,
            ..f
        },
        0.0,
        0.0,
        &mut t,
        0.0,
        1.0,
    );
    assert!(lighter.abs() > on.abs() * 0.45, "{lighter} {on}");
    t = 0.37;
    let at_impact = super::wheel_force(&f, 0.0, 0.0, &mut t, 0.0, 1.0);
    assert!((on - at_impact).abs() < 0.001, "{on} {at_impact}");
}

#[test]
fn active_turning_feels_firmer_at_road_speed_than_in_town() {
    let mut t = 0.0;
    let f = |kmh| super::FfInput {
        on: true,
        kmh,
        dt: 0.016,
        ..Default::default()
    };
    let town = super::wheel_force(&f(10.0), 0.5, 0.48, &mut t, 1.0, 0.0);
    let road = super::wheel_force(&f(70.0), 0.5, 0.48, &mut t, 1.0, 0.0);
    assert!(town < 0.0 && road < town * 1.4, "{town} {road}");
}

#[test]
fn steering_force_scale_changes_the_constant_force() {
    let mut t = 0.0;
    let f = super::FfInput {
        on: true,
        kmh: 30.0,
        dt: 0.016,
        ..Default::default()
    };
    let zero = super::wheel_force(&f, 0.4, 0.4, &mut t, 0.0, 0.0);
    let normal = super::wheel_force(&f, 0.4, 0.4, &mut t, 1.0, 0.0);
    assert_eq!(zero, 0.0);
    assert!(normal.abs() > 0.05, "{normal}");
}
