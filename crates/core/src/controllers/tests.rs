#[test]
fn an_idle_device_nobody_set_up_does_not_hold_the_sticks_steering() {
    assert!(super::stick_steers(None, false, 0.0));
    assert!(super::stick_steers(Some(0.0), false, -0.6));
    assert!(super::stick_steers(Some(0.0), false, 0.0));
    assert!(super::stick_steers(Some(0.02), false, 0.0));
    assert!(!super::stick_steers(Some(0.8), false, 0.3));
    assert!(!super::stick_steers(Some(0.0), true, 1.0));
}

#[test]
fn a_gentler_stick_turns_less_for_a_small_push_but_still_reaches_full_lock() {
    use super::{gamepad_steering, gamepad_steering_time};
    assert!((gamepad_steering(0.5, 0.0, 1.0) - 0.25).abs() < 1e-6);
    let half = gamepad_steering(0.5, 0.0, 0.25);
    assert!((half - 0.0625).abs() < 1e-6, "{half}");
    assert_eq!(gamepad_steering(-0.5, 0.0, 0.25), -half);
    for sens in [0.1, 0.25, 0.7, 1.0, 2.0] {
        assert!((gamepad_steering(1.0, 0.0, sens) - 1.0).abs() < 1e-6);
        assert!((gamepad_steering(-1.0, 0.0, sens) + 1.0).abs() < 1e-6);
    }
    assert!((gamepad_steering_time(1.0) - 1.2).abs() < 1e-6);
    assert!((gamepad_steering_time(0.25) - 2.4).abs() < 1e-6);
}

#[test]
fn a_stick_dead_zone_still_reaches_the_whole_lock() {
    use super::{STICK_DEADZONE, stick_deadzone};
    assert_eq!(stick_deadzone(0.05, STICK_DEADZONE), 0.0);
    assert!(stick_deadzone(0.09, STICK_DEADZONE) < 0.02);
    assert!((stick_deadzone(-1.0, STICK_DEADZONE) + 1.0).abs() < 1e-6);
    let mut d = super::DeviceCfg::default();
    assert_eq!(
        d.stick_deadzone(0),
        super::global_deadzone().max(STICK_DEADZONE)
    );
    d.deadzone = Some(0.02);
    assert_eq!(d.stick_deadzone(0), 0.02);
}

#[test]
fn calibration_stretches_the_travel_and_finds_the_middle() {
    use super::AxisCal;
    let pedal = AxisCal {
        min: -0.9,
        centre: Some(-0.9),
        max: 0.8,
        deadzone: None,
    };
    assert_eq!(pedal.apply(-0.9), -1.0);
    assert_eq!(pedal.apply(0.8), 1.0);
    assert!((pedal.apply(-0.05) - 0.0).abs() < 1e-6);
    assert_eq!(pedal.apply(-1.0), -1.0);
    let wheel = AxisCal {
        min: -0.95,
        centre: Some(0.1),
        max: 1.0,
        deadzone: None,
    };
    assert_eq!(wheel.apply(0.1), 0.0);
    assert!((wheel.apply(0.55) - 0.5).abs() < 1e-6);
    assert!((wheel.apply(-0.425) + 0.5).abs() < 1e-6);
    let still = AxisCal {
        min: 0.1,
        centre: None,
        max: 0.15,
        deadzone: None,
    };
    assert_eq!(still.apply(0.3), 0.3);
}

fn round(d: &super::DeviceCfg) -> super::DeviceCfg {
    super::write_device(d);
    super::read_device(&d.name)
}

#[test]
fn calibration_and_dead_zone_go_through_the_file() {
    let mut d = super::DeviceCfg {
        name: "Cal Wheel".into(),
        second: "0".into(),
        ..Default::default()
    };
    d.axes[0] = Some((super::Func::Steering, false));
    d.calibration[0] = Some(super::AxisCal {
        min: -0.95,
        centre: Some(0.02),
        max: 0.97,
        deadzone: Some(0.05),
    });
    d.calibration[5] = Some(super::AxisCal {
        min: -1.0,
        centre: None,
        max: 0.6,
        deadzone: None,
    });
    let back = round(&d);
    assert_eq!(back.calibration[0], d.calibration[0]);
    assert_eq!(back.calibration[5], d.calibration[5]);
    assert_eq!(back.calibration[1], None);
    assert_eq!(back.deadzone(0), 0.05);
    assert_eq!(back.deadzone(1), super::global_deadzone());
    let mut own = d.clone();
    own.deadzone = Some(0.08);
    let own = round(&own);
    assert_eq!(own.deadzone, Some(0.08));
    assert_eq!(own.deadzone(0), 0.05);
    assert_eq!(own.deadzone(1), 0.08);
    let mut plain = d.clone();
    plain.calibration = Default::default();
    assert_eq!(round(&plain).calibrated(0, 0.3), 0.3);
}

#[test]
fn a_device_chosen_for_a_control_gives_it_alone_while_it_is_connected() {
    let sources = super::parse_assign(
        "steering=G29 Driving Force Racing Wheel|brake=Heusinkveld Sprint|nonsense=x",
    );
    assert_eq!(
        sources[0].as_deref(),
        Some("G29 Driving Force Racing Wheel")
    );
    assert_eq!(sources[1], None);
    assert_eq!(sources[2].as_deref(), Some("Heusinkveld Sprint"));
    let present = vec![
        "G29 Driving Force Racing Wheel".to_string(),
        "Controller (Xbox One For Windows)".to_string(),
    ];
    assert!(super::may_give(
        &sources,
        &present,
        0,
        "G29 Driving Force Racing Wheel"
    ));
    assert!(!super::may_give(
        &sources,
        &present,
        0,
        "Controller (Xbox One For Windows)"
    ));
    assert!(super::may_give(
        &sources,
        &present,
        1,
        "Controller (Xbox One For Windows)"
    ));
    assert!(super::may_give(
        &sources,
        &present,
        2,
        "G29 Driving Force Racing Wheel"
    ));
    let pads = vec![
        "Controller".to_string(),
        "Controller (Xbox One For Windows)".to_string(),
    ];
    let only = super::parse_assign("steering=Controller");
    assert!(super::may_give(&only, &pads, 0, "Controller"));
    assert!(!super::may_give(
        &only,
        &pads,
        0,
        "Controller (Xbox One For Windows)"
    ));
}

#[test]
fn a_set_up_pad_keeps_the_defaults_its_file_leaves_free() {
    use super::{DeviceCfg, Func, PadDefaults};
    assert_eq!(
        PadDefaults::of(None),
        PadDefaults {
            steering: true,
            throttle: true,
            brake: true,
            look: true
        }
    );
    let mut d = DeviceCfg::default();
    d.axes[5] = Some((Func::Throttle, false));
    d.axes[2] = Some((Func::Brake, false));
    assert_eq!(
        PadDefaults::of(Some(&d)),
        PadDefaults {
            steering: true,
            throttle: false,
            brake: false,
            look: true
        }
    );
    d.axes[0] = Some((Func::LookX, false));
    let free = PadDefaults::of(Some(&d));
    assert!(!free.steering && !free.look);
    let mut e = DeviceCfg::default();
    e.axes[3] = Some((Func::Steering, false));
    assert!(!PadDefaults::of(Some(&e)).steering);
}

#[test]
fn names() {
    assert!(super::names_match(
        "Logitech G25 Racing Wheel USB",
        "Logitech G25 Racing Wheel"
    ));
    assert!(!super::names_match("", "x"));
    assert!(super::names_match("Кнопочная панель", "кнопочная  панель"));
    assert!(!super::names_match("Кнопочная панель", "Руль"));
}

#[test]
fn an_xbox_named_ff_wheel_stays_visible_in_the_launcher() {
    assert!(super::include_direct_input_device(
        "G920 Driving Force Racing Wheel for Xbox One",
        true,
        true
    ));
    assert!(!super::include_direct_input_device(
        "Controller (Xbox One)",
        false,
        true
    ));
}

#[test]
fn xbox_gamepad_with_direct_input_twin_is_not_claimed_by_direct_input() {
    let xinput_pads = true;
    let is_claimed_by_di = |name: &str, ff_capable: bool| {
        super::include_direct_input_device(name, ff_capable, xinput_pads)
    };
    assert!(!is_claimed_by_di(
        "Controller (XBOX 360 For Windows)",
        false
    ));
    assert!(!is_claimed_by_di("Xbox Wireless Controller", false));
    assert!(is_claimed_by_di(
        "Logitech G29 Driving Force Racing Wheel",
        true
    ));
    assert!(is_claimed_by_di("HORI Racing Wheel APEX", false));
    assert!(is_claimed_by_di(
        "G920 Driving Force Racing Wheel for Xbox One",
        true
    ));
}

#[test]
fn system_gamepad_buttons_work_alongside_direct_input_wheels() {
    assert!(super::use_gilrs_buttons(true, true));
    assert!(!super::use_gilrs_buttons(true, false));
    assert!(super::use_gilrs_buttons(false, false));
}

#[test]
fn wheels_are_not_treated_as_system_gamepads() {
    assert!(!super::is_system_gamepad(
        "Logitech G29 Driving Force Racing Wheel",
        true
    ));
    assert!(!super::is_system_gamepad("Logitech Driving Force GT", true));
    assert!(!super::is_system_gamepad(
        "Logitech Driving Force GT",
        false
    ));
    assert!(!super::is_system_gamepad("Thrustmaster T300RS", true));
    assert!(!super::is_system_gamepad("Thrustmaster T300RS", false));

    assert!(super::is_system_gamepad("Xbox 360 Controller", false));
    assert!(super::is_system_gamepad("Controller (Xbox One)", false));
    assert!(super::is_system_gamepad("Xbox Series X Controller", false));
    assert!(!super::is_system_gamepad(
        "G920 Driving Force Racing Wheel for Xbox One",
        true
    ));
}
