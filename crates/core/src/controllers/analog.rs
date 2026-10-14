use super::*;

/// The analog controls a controller gives this frame (None: that one is not on it).
#[derive(Debug, Clone, Copy, Default)]
pub struct Analog {
    pub steering: Option<f32>,
    /// The steering is a gamepad's stick (not a wheel): see `gamepad_steering`.
    pub stick: bool,
    pub throttle: Option<f32>,
    pub brake: Option<f32>,
    pub clutch: Option<f32>,
    /// How far the head is to turn this moment, right and down (-1 .. 1 each): a set-up
    /// axis that looks round, or a gamepad's right stick (#454).
    pub look: [f32; 2],
}

/// An axis that turns the head: nothing round its centre, then the rest of the way.
pub(crate) fn look_axis(v: f32) -> f32 {
    const DEAD: f32 = 0.12;
    if v.abs() <= DEAD {
        0.0
    } else {
        v.signum() * (v.abs() - DEAD) / (1.0 - DEAD)
    }
}

/// Where a gamepad's stick turns the wheel to (#200): a stick is no steering wheel - taken
/// as the wheel's place, the smallest movement turned the wheel a long way and a push to
/// the side was the full lock at any speed. As the bus games take it: a gentler curve
/// (squared), and less of the lock the faster the bus goes (the whole of it standing, a
/// third of it at 50 km/h, a fifth at 90 km/h). Full deflection stays full lock at every
/// `sens`, so parking still works.
pub fn gamepad_steering(x: f32, kmh: f32, sens: f32) -> f32 {
    let x = x.clamp(-1.0, 1.0);
    let curve = x.signum() * x.abs().powf(2.0 - sens.clamp(0.1, 2.0).log2());
    let reach = 1.0 / (1.0 + (kmh.abs() - 10.0).max(0.0) / 20.0);
    curve * reach
}

pub fn gamepad_steering_time(sens: f32) -> f32 {
    1.2 / sens.clamp(0.1, 2.0).sqrt()
}

pub(crate) const CENTRE_SNAP: f32 = 0.03;

pub(super) const STICK_DEADZONE: f32 = 0.08;

pub(super) fn default_stick_deadzone() -> f32 {
    global_deadzone().max(STICK_DEADZONE)
}

pub(super) fn stick_deadzone(x: f32, dz: f32) -> f32 {
    x.signum() * ((x.abs() - dz).max(0.0) / (1.0 - dz))
}

pub(crate) const ASSIGNABLE: [(&str, &str); 4] = [
    ("steering", "Steering"),
    ("throttle", "Throttle"),
    ("brake", "Brake"),
    ("clutch", "Clutch"),
];

pub(crate) fn parse_assign(text: &str) -> [Option<String>; 4] {
    let mut out: [Option<String>; 4] = Default::default();
    for part in text.split('|') {
        let Some((k, v)) = part.split_once('=') else {
            continue;
        };
        if let Some(i) = ASSIGNABLE
            .iter()
            .position(|(key, _)| key.eq_ignore_ascii_case(k.trim()))
        {
            out[i] = Some(v.trim().to_string()).filter(|v| !v.is_empty());
        }
    }
    out
}

pub(super) fn may_give(sources: &[Option<String>; 4], present: &[String], i: usize, name: &str) -> bool {
    match &sources[i] {
        // "Controller" is also contained in "Controller (Xbox One)".
        Some(s) if present.iter().any(|p| names_match(s, p)) => {
            let exact = |n: &str| normalized_device_name(n) == normalized_device_name(s);
            if present.iter().any(|p| exact(p)) {
                exact(name)
            } else {
                names_match(s, name)
            }
        }
        _ => true,
    }
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub(super) struct PadDefaults {
    pub(super) steering: bool,
    pub(super) throttle: bool,
    pub(super) brake: bool,
    pub(super) look: bool,
}

impl PadDefaults {
    pub(super) fn of(cfg: Option<&DeviceCfg>) -> PadDefaults {
        let Some(d) = cfg else {
            return PadDefaults {
                steering: true,
                throttle: true,
                brake: true,
                look: true,
            };
        };
        let has = |f: &[Func]| d.axes.iter().flatten().any(|(g, _)| f.contains(g));
        let free = |k: usize| d.axes[k].is_none();
        PadDefaults {
            steering: !has(&[Func::Steering]) && free(0),
            throttle: !has(&[Func::Throttle, Func::ThrottleBrake]) && free(5),
            brake: !has(&[Func::Brake, Func::ThrottleBrake]) && free(2),
            look: !has(&[Func::LookX, Func::LookY]) && free(3) && free(4),
        }
    }
}

pub(super) fn stick_steers(current: Option<f32>, set_up: bool, x: f32) -> bool {
    match current {
        None => true,
        Some(s) => !set_up && (x.abs() > s.abs() || s.abs() < CENTRE_SNAP),
    }
}

/// Bus steering follows its characteristic, dead zone and range; feedback follows the physical
/// wheel position, so it can keep returning even inside the input dead zone.
pub(super) fn wheel_steering(axis: f32, reversed: bool, flags: i32, deadzone: f32, gain: f32) -> (f32, f32) {
    let position = if reversed { -axis } else { axis };
    let shaped = axis_shape((position + 1.0) * 0.5, flags) * 2.0 - 1.0;
    let deadzone = deadzone.clamp(0.0, 0.3);
    let steering = shaped.signum() * ((shaped.abs() - deadzone).max(0.0) / (1.0 - deadzone)) * gain;
    (steering.clamp(-1.0, 1.0), position.clamp(-1.0, 1.0))
}

pub(super) fn configured_steering(
    c: &Connected,
    d: &DeviceCfg,
    k: usize,
    value: f32,
    gain: f32,
) -> (f32, Option<f32>) {
    let reversed = d.axes[k].is_some_and(|(_, reversed)| reversed);
    if c.gamepad {
        (
            stick_deadzone(if reversed { -value } else { value }, d.stick_deadzone(k)),
            None,
        )
    } else {
        let (steering, position) =
            wheel_steering(value, reversed, d.axis_flags[k], d.deadzone(k), gain);
        (steering, Some(position))
    }
}

/// A control several set-up devices give: the first one set wins, unless a later one is
/// moved further (two wheels, or pedals on their own device).
pub(super) fn set(slot: &mut Option<f32>, v: f32) {
    match slot {
        Some(old) if old.abs() >= v.abs() => {}
        _ => *slot = Some(v),
    }
}

/// An axis' characteristic from its gamectrler.cfg flags, on 0..1 (after the inversion),
/// as Omsi.exe applies it (poll sub_6466b8): 2 extends the range by a quarter, 4 is
/// degressive, 8 progressive, 0x10 makes either of them symmetric round the centre.
pub(crate) fn axis_shape(v: f32, flags: i32) -> f32 {
    use std::f32::consts::PI;
    let mut v = v;
    if flags & 2 != 0 {
        v = v * 1.25 - 0.125;
    }
    let bi = flags & 0x10 != 0;
    if flags & 4 != 0 {
        v = if bi {
            ((v - 0.5) * PI).sin() * 0.5 + 0.5
        } else {
            (v * PI * 0.5).sin()
        };
    } else if flags & 8 != 0 {
        v = if bi {
            let d = (v - 0.5) * 2.0;
            0.5 + d.signum() * d * d * 0.5
        } else {
            v * v
        };
    }
    v.clamp(0.0, 1.0)
}

/// A pedal's last few per cent of travel are its end: a wheel's pedal on the floor reads
/// 0.93..0.99, and the scripts ask for the ends exactly - the LiAZ/PAZ gearboxes put a gear
/// in only at `(L.L.clutch) 1 =` and part the engine from the wheels only above 0.95, so a
/// clutch held down to the floor still dragged and the engine died at every stop.
pub(super) fn pedal_ends(v: f32) -> f32 {
    if v >= 0.96 {
        1.0
    } else if v <= 0.02 {
        0.0
    } else {
        v
    }
}

/// A pedal as the settings shape it: `v` 0..1 through the response curve of `strength`.
pub(super) fn pedal_curve(v: f32, strength: f32) -> f32 {
    v.clamp(0.0, 1.0).powf(1.0 / strength.clamp(0.25, 4.0))
}
