fn round(d: &super::DeviceCfg) -> super::DeviceCfg {
    super::write_device(d);
    super::read_device(&d.name)
}

#[cfg(target_os = "linux")]
#[test]
fn omsi_import_keeps_inversion_on_an_unassigned_axis() {
    let lines: Vec<String> = "[ctrl]\nOMSI unassigned Z regression\n0\n-1 0 -1 0 -1 9 -1 0 -1 0 -1 0 -1 0 -1 0\n[buttons]\n0"
        .lines().map(String::from).collect();
    let devices = super::parse_omsi_cfg(&lines);
    assert_eq!(devices.len(), 1);
    let mut device = round(&devices[0]);
    assert!(device.axis_reversed(2));
    assert_eq!(device.axes[2], None);
    assert_eq!(device.axis_flags[2], 9);
    device.set_axis_function(2, Some(super::Func::Brake));
    assert_eq!(device.axes[2], Some((super::Func::Brake, true)));
}

#[cfg(target_os = "linux")]
#[test]
fn importing_omsi_assignments_keeps_the_native_mode_and_calibration() {
    use super::{AxisCal, DeviceCfg, Func, preserve_device_settings};
    use crate::evdev_axes::AxisMode;
    let mut existing = DeviceCfg {
        axis_mode: AxisMode::Native,
        enabled: false,
        ff_scale: Some((0.5, 0.4)),
        ff_invert: Some(true),
        deadzone: Some(0.1),
        ..Default::default()
    };
    existing.calibration[2] = Some(AxisCal {
        min: -0.9,
        max: 0.8,
        centre: None,
        deadzone: Some(0.02),
    });
    let mut imported = DeviceCfg {
        name: "OMSI Native import regression".into(),
        ..Default::default()
    };
    imported.axes[2] = Some((Func::Brake, true));
    preserve_device_settings(&mut imported, &existing);
    assert_eq!(imported.axis_mode, AxisMode::Native);
    assert_eq!(imported.calibration, existing.calibration);
    assert_eq!(imported.axes[2], Some((Func::Brake, true)));
    assert!(!imported.enabled);
    assert_eq!(imported.ff_scale, existing.ff_scale);
    assert_eq!(imported.ff_invert, existing.ff_invert);
    assert_eq!(imported.deadzone, existing.deadzone);
    let saved = round(&imported);
    assert_eq!(saved.axis_mode, AxisMode::Native);
    assert_eq!(saved.axes[2], imported.axes[2]);
}

#[cfg(target_os = "linux")]
#[test]
fn axis_modes_round_trip_without_changing_axis_configuration() {
    use crate::evdev_axes::AxisMode;
    let mut device = super::DeviceCfg {
        name: "Linux axis mode round trip".into(),
        second: "0".into(),
        ..Default::default()
    };
    device.axes[2] = Some((super::Func::Throttle, true));
    device.axis_flags[2] = 1 | 8;
    device.calibration[2] = Some(super::AxisCal {
        min: -0.75,
        centre: None,
        max: 0.75,
        deadzone: Some(0.05),
    });
    device.ff_scale = Some((0.5, 1.0));
    device.ff_invert = Some(true);
    for mode in [AxisMode::Auto, AxisMode::Gamepad, AxisMode::Native] {
        device.axis_mode = mode;
        let loaded = round(&device);
        assert_eq!(loaded, device);
    }
}

#[cfg(target_os = "linux")]
#[test]
fn old_or_unknown_axis_modes_use_auto_and_preserve_inversion() {
    use crate::evdev_axes::AxisMode;
    let name = "Linux legacy axis mode";
    let mut device = super::DeviceCfg {
        name: name.into(),
        ..Default::default()
    };
    device.set_axis_reversed(2, true);
    super::write_device(&device);
    for mode in [
        None,
        Some(::config::Value::from("future-mode")),
        Some(::config::Value::from(7)),
    ] {
        super::dev_put(name, "axis_mode", mode);
        let loaded = super::read_device(name);
        assert_eq!(loaded.axis_mode, AxisMode::Auto);
        assert!(loaded.axis_reversed(2));
    }
}

#[cfg(target_os = "linux")]
#[test]
fn z_inversion_is_saved_before_assignment_and_survives_function_changes() {
    let mut device = super::DeviceCfg {
        name: "Z inversion before assignment".into(),
        ..Default::default()
    };
    device.axis_flags[2] = 2 | 8 | 0x10;
    device.set_axis_reversed(2, true);
    let mut device = round(&device);
    assert_eq!(device.axes[2], None);
    assert!(device.axis_reversed(2));
    device.set_axis_function(2, Some(super::Func::Throttle));
    assert_eq!(device.axes[2], Some((super::Func::Throttle, true)));
    device.set_axis_function(2, None);
    let mut device = round(&device);
    assert!(device.axis_reversed(2));
    device.set_axis_function(2, Some(super::Func::Brake));
    assert_eq!(device.axes[2], Some((super::Func::Brake, true)));
    device.set_axis_reversed(2, false);
    let device = round(&device);
    assert_eq!(device.axes[2], Some((super::Func::Brake, false)));
    assert!(!device.axis_reversed(2));
    assert_eq!(device.axis_flags[2], 2 | 8 | 0x10);
}

#[cfg(target_os = "linux")]
#[test]
fn an_assigned_axis_can_clear_a_previous_inversion_flag() {
    let mut device = super::DeviceCfg {
        name: "Z inversion stale flag".into(),
        ..Default::default()
    };
    device.axis_flags[2] = 1 | 4;
    device.axes[2] = Some((super::Func::Brake, false));
    let device = round(&device);
    assert!(!device.axis_reversed(2));
    assert_eq!(device.axes[2], Some((super::Func::Brake, false)));
    assert_eq!(device.axis_flags[2], 4);
}

#[cfg(target_os = "linux")]
#[test]
fn z_preview_reverses_the_calibrated_range_without_reversing_other_axes() {
    let mut device = super::DeviceCfg::default();
    device.calibration[2] = Some(super::AxisCal {
        min: -0.5,
        centre: None,
        max: 0.5,
        deadzone: None,
    });
    assert_eq!(device.axis_preview(2, 0.5), 1.0);
    device.set_axis_reversed(2, true);
    assert_eq!(device.axis_preview(2, 0.5), -1.0);
    assert_eq!(device.axis_preview(2, -0.5), 1.0);
    assert_eq!(device.axis_preview(2, 0.0), 0.0);
    assert_eq!(device.axis_preview(0, 0.5), 0.5);
    assert_eq!(device.calibrated(2, 0.5), 1.0);
}

#[test]
fn feedback_keeps_the_physical_position_inside_the_steering_deadzone() {
    for (axis, deadzone) in [(0.2, 0.3), (0.01, 0.02)] {
        let (steering, position) = super::wheel_steering(axis, false, 0, deadzone, 1.0);
        assert_eq!(steering, 0.0);
        assert_eq!(position, axis);
        let f = super::FfInput {
            on: true,
            kmh: 30.0,
            dt: 0.016,
            ..Default::default()
        };
        let mut t = 0.0;
        let force = super::wheel_force(&f, position, position, &mut t, 1.0, 0.0);
        assert!(force < 0.0, "axis={axis}: {force}");
    }
}

#[test]
fn steering_range_and_reversal_preserve_the_feedback_position() {
    let (steering, position) = super::wheel_steering(0.5, false, 0, 0.1, 2.0);
    assert!((steering - 8.0 / 9.0).abs() < 1e-6);
    assert_eq!(position, 0.5);
    let (steering, position) = super::wheel_steering(0.5, true, 0, 0.1, 2.0);
    assert!((steering + 8.0 / 9.0).abs() < 1e-6);
    assert_eq!(position, -0.5);
    let (steering, position) = super::wheel_steering(0.5, false, 0, 0.0, 0.25);
    assert_eq!(steering, 0.125);
    assert_eq!(position, 0.5);
}

#[test]
fn axis_characteristics_change_bus_steering_but_not_feedback_position() {
    let (steering, position) = super::wheel_steering(0.5, false, 24, 0.0, 1.0);
    assert_eq!(steering, 0.25);
    assert_eq!(position, 0.5);
    let (steering, position) = super::wheel_steering(0.5, true, 24, 0.0, 1.0);
    assert_eq!(steering, -0.25);
    assert_eq!(position, -0.5);
    let (steering, position) = super::wheel_steering(0.4, false, 2, 0.0, 1.0);
    assert!((steering - 0.5).abs() < 1e-6);
    assert_eq!(position, 0.4);
}

#[test]
fn force_feedback_scales_are_saved_per_controller() {
    let devices = vec![
        super::DeviceCfg {
            name: "Wheel A".into(),
            ff_scale: Some((2.0, 0.5)),
            ..Default::default()
        },
        super::DeviceCfg {
            name: "Wheel B".into(),
            ff_scale: Some((0.75, 1.25)),
            ..Default::default()
        },
    ];
    let saved: Vec<_> = devices.iter().map(round).collect();
    assert_eq!(saved[0].ff_scale, Some((2.0, 0.5)));
    assert_eq!(saved[1].ff_scale, Some((0.75, 1.25)));
}

#[test]
fn force_feedback_direction_is_saved_and_applied_per_wheel() {
    let devices = vec![
        super::DeviceCfg {
            name: "Invert A".into(),
            ff_invert: Some(true),
            ..Default::default()
        },
        super::DeviceCfg {
            name: "Invert B".into(),
            ff_invert: Some(false),
            ..Default::default()
        },
        super::DeviceCfg {
            name: "Invert None".into(),
            ..Default::default()
        },
    ];
    let saved: Vec<_> = devices.iter().map(round).collect();
    assert_eq!(saved[0].ff_invert, Some(true));
    assert_eq!(saved[1].ff_invert, Some(false));
    assert_eq!(saved[2].ff_invert, None);
    assert!(super::feedback_inverted(Some(&saved[0]), false, false));
    assert!(!super::feedback_inverted(Some(&saved[1]), true, false));
    assert!(super::feedback_inverted(Some(&saved[2]), true, false));
    assert!(!super::feedback_inverted(Some(&saved[0]), false, true));
    assert!(super::feedback_inverted(Some(&saved[1]), true, true));
    let mut reversed = super::DeviceCfg::default();
    reversed.axes[0] = Some((super::Func::Steering, true));
    assert!(!super::calibrated_steering_reversed(Some(&reversed)));
    reversed.ff_invert = Some(false);
    assert!(super::calibrated_steering_reversed(Some(&reversed)));
}

#[test]
fn exact_wheel_configuration_beats_a_shorter_alias() {
    let devices = vec![
        super::DeviceCfg {
            name: "G920".into(),
            ff_scale: Some((0.0, 0.0)),
            ..Default::default()
        },
        super::DeviceCfg {
            name: "G920 Driving Force Racing Wheel for Xbox One".into(),
            ff_scale: Some((1.2, 0.6)),
            ..Default::default()
        },
    ];
    let matched =
        super::find_device_cfg(&devices, "G920 Driving Force Racing Wheel for Xbox One")
            .unwrap();
    assert_eq!(matched.ff_scale, Some((1.2, 0.6)));
}

#[test]
fn a_reversed_steering_axis_reverses_its_motor_force_too() {
    let mut wheel = super::DeviceCfg::default();
    wheel.axes[0] = Some((super::Func::Steering, true));
    wheel.axes[1] = Some((super::Func::Throttle, true));
    assert!(super::force_axis_reversed(Some(&wheel), Some(0)));
    assert!(!super::force_axis_reversed(Some(&wheel), Some(1)));
    assert!(!super::force_axis_reversed(Some(&wheel), None));
}
