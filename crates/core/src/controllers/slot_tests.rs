use super::*;

#[cfg(target_os = "linux")]
#[test]
fn thrustmaster_pedals_keep_z_and_rz_without_rx_or_ry() {
    let axes = [
        (0x3_0000, 0.0),
        (0x3_0001, 1.0),
        (0x3_0002, 1.0),
        (0x3_0005, 1.0),
    ];
    assert_eq!(di_slots(&axes), [(0, 0.0), (1, 1.0), (2, 1.0), (5, 1.0)]);
}

#[cfg(target_os = "linux")]
#[test]
fn native_layout_keeps_gamepad_and_wheel_steering_separate() {
    use super::{
        Connected, DeviceCfg, Func, STICK_DEADZONE, configured_steering, di_slots,
        stick_deadzone,
    };
    let mut connected = Connected {
        name: "Native controller".into(),
        hardware_id: None,
        axes: di_slots(&[(0x3_0000, 0.04), (0x3_0002, 1.0), (0x3_0005, -1.0)]),
        gamepad: false,
        native_axes: true,
        ff: false,
        ff_capable: false,
        buttons: 0,
    };
    let mut device = DeviceCfg {
        deadzone: Some(0.0),
        ..Default::default()
    };
    device.axes[0] = Some((Func::Steering, false));
    assert_eq!(connected.axes, [(0, 0.04), (2, 1.0), (5, -1.0)]);
    assert!(!connected.gamepad_axes());
    let (steering, position) = configured_steering(&connected, &device, 0, 0.04, 1.0);
    assert!((steering - 0.04).abs() < 1e-6);
    assert_eq!(position, Some(0.04));
    connected.gamepad = true;
    device.deadzone = Some(STICK_DEADZONE);
    assert!(!connected.gamepad_axes());
    assert_eq!(
        configured_steering(&connected, &device, 0, 0.04, 1.0),
        (0.0, None)
    );
    let (steering, position) = configured_steering(&connected, &device, 0, 0.5, 0.25);
    assert!((steering - stick_deadzone(0.5, STICK_DEADZONE)).abs() < 1e-6);
    assert_eq!(position, None);
    device.set_axis_reversed(0, true);
    assert_eq!(
        configured_steering(&connected, &device, 0, 0.5, 0.25).0,
        -steering
    );
}

#[test]
fn a_missing_axis_does_not_shift_the_rest() {
    // X and Rz only (a wheel with one pedal axis): Rz stays slot 5
    let axes = if cfg!(target_os = "macos") {
        vec![(0x1_0030, 0.5), (0x1_0035, -1.0)]
    } else {
        vec![(0x3_0000, 0.5), (0x3_0005, -1.0)]
    };
    let got = di_slots(&axes);
    assert!(got.contains(&(0, 0.5)), "{got:?}");
    if !cfg!(windows) {
        assert!(got.contains(&(5, -1.0)), "{got:?}");
    }
}

#[test]
fn two_devices_merge_by_the_larger_movement() {
    let mut s = None;
    set(&mut s, 0.0);
    set(&mut s, 0.4);
    set(&mut s, -0.1);
    assert_eq!(s, Some(0.4));
}
