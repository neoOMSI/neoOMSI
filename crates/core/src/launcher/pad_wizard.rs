use crate::controllers::{self, AxisCal, Controllers, DeviceCfg, Func};
use crate::ffb_calibration::{self as ffb, Calibration};
use crate::game_lists::{HEADING, row};
use std::time::Instant;

type Rows = Vec<(String, String)>;
type Axes = [Option<f32>; 8];

pub struct Wizard {
    device: usize,
    step: Step,
    error: Option<String>,
}

enum Step {
    Calibrate {
        lo: Axes,
        hi: Axes,
        centre: Axes,
        cleared: [bool; 8],
    },
    Assist {
        at: usize,
        rest: Axes,
        held: Vec<Axes>,
    },
    Feedback {
        axes: [Option<(Func, bool)>; 8],
        strength: f32,
        test: Option<(Instant, Calibration)>,
        invert: Option<bool>,
    },
}

const ASSIST: [(&str, &str); 5] = [
    (
        "Let go of everything",
        "Take your hands off the wheel and your feet off the pedals (the wheel in the middle), then press Next.",
    ),
    (
        "Steering",
        "Turn the wheel (or move the stick) all the way to the LEFT and hold it there, then press Next.",
    ),
    (
        "Throttle",
        "Press the throttle pedal all the way down and hold it, then press Next. No pedals: Skip.",
    ),
    (
        "Brake",
        "Press the brake pedal all the way down and hold it, then press Next. No brake pedal: Skip.",
    ),
    (
        "Clutch",
        "Press the clutch pedal all the way down and hold it, then press Next. No clutch: Skip.",
    ),
];

pub fn entry_rows() -> Rows {
    vec![
        (row("Assistants", 'h', "", "", None), HEADING.to_string()),
        (
            row(
                "Set-up assistant",
                'a',
                "Start",
                "Finds the steering axis, the pedals and the force feedback's direction step by step",
                None,
            ),
            "wiz_assist".to_string(),
        ),
        (
            row(
                "Calibrate the axes",
                'a',
                "Start",
                "Teaches the game how far each axis really goes and where its middle is",
                None,
            ),
            "wiz_cal".to_string(),
        ),
    ]
}

fn live_of(pads: Option<&Controllers>, d: &DeviceCfg) -> Option<controllers::Connected> {
    pads?
        .connected_devices()
        .into_iter()
        .find(|c| controllers::names_match(&d.name, &c.name))
}

fn axes_now(live: &[(usize, f32)]) -> Axes {
    let mut a = [None; 8];
    for (k, v) in live {
        if *k < 8 {
            a[*k] = Some(*v);
        }
    }
    a
}

fn info(text: &str) -> (String, String) {
    (row("", 'i', "", text, None), HEADING.to_string())
}

fn button(name: &str, label: &str, desc: &str, id: &str) -> (String, String) {
    (row(name, 'a', label, desc, None), id.to_string())
}

fn with_meter(r: String, v: f32, one_sided: bool) -> String {
    format!(
        "{r}\u{1f}\u{1f}{v:.3}{}",
        if one_sided { "\u{1f}u" } else { "" }
    )
}

impl Wizard {
    fn merged(&self, k: usize, old: Option<AxisCal>) -> Option<AxisCal> {
        let Step::Calibrate {
            lo,
            hi,
            centre,
            cleared,
        } = &self.step
        else {
            return old;
        };
        let base = if cleared[k] { None } else { old };
        let (min, max) = match (lo[k], hi[k]) {
            (Some(lo), Some(hi)) if hi - lo >= AxisCal::MIN_SPAN => (lo, hi),
            _ => base.map_or((-1.0, 1.0), |b| (b.min, b.max)),
        };
        let centre = centre[k].or(base.and_then(|b| b.centre));
        let deadzone = base.and_then(|b| b.deadzone);
        (min != -1.0 || max != 1.0 || centre.is_some() || deadzone.is_some()).then_some(AxisCal {
            min,
            centre,
            max,
            deadzone,
        })
    }

    pub fn rows(&mut self, mut pads: Option<&mut Controllers>) -> Rows {
        let devices = controllers::read_cfg();
        let Some(d) = devices.get(self.device) else {
            return vec![info("The device is not set up any more.")];
        };
        let dev = live_of(pads.as_deref(), d);
        let live = dev.as_ref().map(|c| c.axes.clone()).unwrap_or_default();
        let names = crate::lab_pads::axis_names(dev.as_ref().is_some_and(|c| c.gamepad));
        let name_of = |k: usize| {
            if names[k].is_empty() {
                format!("Axis {}", k + 1)
            } else {
                names[k].clone()
            }
        };
        let mut out = Vec::new();
        if let Step::Calibrate { lo, hi, .. } = &mut self.step {
            for (k, v) in &live {
                lo[*k] = Some(lo[*k].map_or(*v, |x| x.min(*v)));
                hi[*k] = Some(hi[*k].map_or(*v, |x| x.max(*v)));
            }
        }
        if let Step::Feedback {
            axes,
            test: Some((started, t)),
            invert,
            ..
        } = &mut self.step
        {
            let axis = axes
                .iter()
                .position(|a| matches!(a, Some((Func::Steering, _))));
            let position = axis.and_then(|a| live.iter().find(|(k, _)| *k == a).map(|(_, x)| *x));
            if let Some(force) = t.update(started.elapsed().as_secs_f32(), position) {
                let sent = axis
                    .zip(pads.as_deref_mut())
                    .is_some_and(|(a, p)| p.calibration_pulse(&d.name, a, force));
                if !sent {
                    t.fail("Force feedback is unavailable. Choose the direction by hand.");
                }
            }
            if let Some(Ok(inv)) = t.result {
                *invert = Some(inv);
            }
        }
        match &self.step {
            Step::Calibrate { lo, hi, centre, .. } => {
                out.push((
                    row("Calibrate the axes", 'h', "", "", None),
                    HEADING.to_string(),
                ));
                out.push(info("Move every axis all the way to both ends a few times: the wheel from lock to lock, each pedal down and back up. Then let go of everything (the wheel in the middle), press Set centre, and Apply. An axis that did not move keeps its calibration."));
                if dev.is_none() {
                    out.push(info("The device is not connected: plug it in."));
                }
                let mut ks: Vec<usize> = live.iter().map(|(k, _)| *k).collect();
                ks.sort();
                for k in ks {
                    let raw = live.iter().find(|(x, _)| *x == k).map_or(0.0, |(_, v)| *v);
                    let cal = self.merged(k, d.calibration[k]);
                    let shown = cal.map_or(raw, |c| c.apply(raw));
                    let seen = match (lo[k], hi[k]) {
                        (Some(a), Some(b)) => {
                            format!("Seen {:+.0} % to {:+.0} %", a * 100.0, b * 100.0)
                        }
                        _ => "Not moved yet".to_string(),
                    };
                    let mid = centre[k]
                        .map_or(String::new(), |c| format!(", centre {:+.0} %", c * 100.0));
                    let r = row(
                        &name_of(k),
                        'a',
                        "As the system reports it",
                        &format!("{seen}{mid}"),
                        None,
                    );
                    out.push((with_meter(r, shown, false), format!("wiz_cal_clear {k}")));
                }
                out.push(button(
                    "",
                    "Set centre",
                    "Let go of everything first",
                    "wiz_cal_centre",
                ));
                out.push(button("", "Apply", "", "wiz_cal_apply"));
                out.push(button("", "Cancel", "", "wiz_cancel"));
            }
            Step::Assist { at, .. } => {
                let (title, text) = ASSIST[*at];
                out.push((
                    row(
                        &format!("Step {} of {}: {title}", at + 1, ASSIST.len()),
                        'h',
                        "",
                        "",
                        None,
                    ),
                    HEADING.to_string(),
                ));
                out.push(info(text));
                if dev.is_none() {
                    out.push(info("The device is not connected: plug it in."));
                }
                for (k, v) in &live {
                    out.push((
                        with_meter(row(&name_of(*k), 'i', "", "", None), *v, false),
                        HEADING.to_string(),
                    ));
                }
                let last = *at + 1 == ASSIST.len();
                out.push(button(
                    "",
                    if last { "Finish" } else { "Next" },
                    "",
                    "wiz_next",
                ));
                if *at >= 2 {
                    out.push(button("", "Skip", "", "wiz_skip"));
                }
                out.push(button("", "Cancel", "", "wiz_cancel"));
            }
            Step::Feedback {
                strength,
                test,
                invert,
                ..
            } => {
                out.push((
                    row("Force feedback direction", 'h', "", "", None),
                    HEADING.to_string(),
                ));
                out.push(info("INJURY RISK: TAKE YOUR HANDS OFF THE WHEEL. Keep hands and fingers clear before starting and throughout the test."));
                out.push(info("The test applies two short forces in opposite directions and finds out which way the wheel turns."));
                let running = test.as_ref().is_some_and(|(_, t)| t.result.is_none());
                if let Some((_, t)) = test {
                    out.push(info(match t.result {
                        Some(Ok(false)) => "Direction detected: normal",
                        Some(Ok(true)) => "Direction detected: inverted",
                        Some(Err(m)) => m,
                        None => "Testing: keep your hands off the wheel…",
                    }));
                }
                if !running {
                    out.push(button(
                        "Test strength",
                        &format!("{:.0} %", strength * 100.0),
                        "If the wheel barely moves, make the test stronger and try again",
                        "wiz_ff_strength",
                    ));
                    out.push(button("", "Start the test", "", "wiz_ff_test"));
                    let inv = invert
                        .or(d.ff_invert)
                        .unwrap_or_else(controllers::global_ff_invert);
                    out.push((
                        row(
                            "Invert force feedback",
                            's',
                            if inv { "on" } else { "off" },
                            "Set by the test, or by hand when it found nothing",
                            None,
                        ),
                        "wiz_ff_flip".to_string(),
                    ));
                    out.push(button("", "Finish", "", "wiz_ff_finish"));
                }
                out.push(button("", "Cancel", "", "wiz_cancel"));
            }
        }
        if let Some(e) = &self.error {
            out.insert(2.min(out.len()), info(e));
        }
        out
    }
}

/// A wizard's button; `w` is the wizard shown (none: the device's own rows).
pub fn press(w: &mut Option<Wizard>, mut pads: Option<&mut Controllers>, verb: &str, arg: &str) {
    let mut devices = controllers::read_cfg();
    let start = |step: Step| {
        crate::lab_pads::selected(&devices).map(|device| Wizard {
            device,
            step,
            error: None,
        })
    };
    match verb {
        "wiz_cal" => {
            *w = start(Step::Calibrate {
                lo: [None; 8],
                hi: [None; 8],
                centre: [None; 8],
                cleared: [false; 8],
            });
            return;
        }
        "wiz_assist" => {
            *w = start(Step::Assist {
                at: 0,
                rest: [None; 8],
                held: Vec::new(),
            });
            return;
        }
        "wiz_cancel" => {
            *w = None;
            return;
        }
        _ => {}
    }
    let Some(wz) = w.as_mut() else {
        return;
    };
    let Some(d) = devices.get(wz.device).cloned() else {
        *w = None;
        return;
    };
    let live = live_of(pads.as_deref(), &d)
        .map(|c| c.axes)
        .unwrap_or_default();
    let ff_capable = live_of(pads.as_deref(), &d).is_some_and(|c| c.ff_capable && !c.gamepad);
    wz.error = None;
    let mut done: Option<DeviceCfg> = None;
    let mut next: Option<Step> = None;
    if verb == "wiz_cal_apply" && matches!(wz.step, Step::Calibrate { .. }) {
        let mut d = d.clone();
        for k in 0..8 {
            d.calibration[k] = wz.merged(k, d.calibration[k]);
        }
        done = Some(d);
    }
    match (&mut wz.step, verb) {
        (Step::Calibrate { centre, .. }, "wiz_cal_centre") => {
            for (k, v) in &live {
                centre[*k] = Some(*v);
            }
        }
        (
            Step::Calibrate {
                lo,
                hi,
                centre,
                cleared,
            },
            "wiz_cal_clear",
        ) => {
            if let Some(k) = arg.parse::<usize>().ok().filter(|k| *k < 8) {
                (lo[k], hi[k], centre[k], cleared[k]) = (None, None, None, true);
            }
        }
        (Step::Assist { at, rest, held }, "wiz_next" | "wiz_skip") => {
            let now = axes_now(&live);
            if *at == 0 {
                if live.is_empty() {
                    wz.error = Some("The device shows no axis yet: move the wheel and the pedals a little, let go, and press Next again.".into());
                    return;
                }
                *rest = now;
            } else if verb == "wiz_skip" {
                held.push([None; 8]);
            } else {
                // (an axis taken before is not taken again - but the throttle's may be the brake's too)
                let used: Vec<usize> = held
                    .iter()
                    .filter_map(|a| moved_most(rest, a, &[]).map(|m| m.0))
                    .collect();
                let exclude: Vec<usize> = if *at == 3 {
                    used.iter().copied().take(1).collect()
                } else {
                    used
                };
                if moved_most(rest, &now, &exclude).is_none() {
                    wz.error = Some(
                        "Nothing moved far enough. Hold it all the way, then press Next (or Skip)."
                            .into(),
                    );
                    return;
                }
                held.push(now);
            }
            *at += 1;
            if *at == ASSIST.len() {
                let axes = assist_result(rest, held);
                let steering = axes.iter().any(|a| matches!(a, Some((Func::Steering, _))));
                if ff_capable && steering {
                    next = Some(Step::Feedback {
                        axes,
                        strength: ffb::PULSE_FORCE,
                        test: None,
                        invert: None,
                    });
                } else {
                    let mut d = d.clone();
                    d.axes = axes;
                    done = Some(d);
                }
            }
        }
        (Step::Feedback { strength, .. }, "wiz_ff_strength") => {
            *strength = if *strength + 0.1 > ffb::MAX_PULSE_FORCE + 1e-3 {
                ffb::PULSE_FORCE
            } else {
                *strength + 0.1
            };
        }
        (Step::Feedback { strength, test, .. }, "wiz_ff_test") => {
            if let Some(p) = pads.as_deref_mut() {
                p.set_focus(true);
            }
            *test = Some((Instant::now(), Calibration::new(*strength)));
        }
        (Step::Feedback { invert, .. }, "wiz_ff_flip") => {
            let now = invert
                .or(d.ff_invert)
                .unwrap_or_else(controllers::global_ff_invert);
            *invert = Some(!now);
        }
        (Step::Feedback { axes, invert, .. }, "wiz_ff_finish") => {
            let mut d = d.clone();
            d.axes = *axes;
            d.ff_invert = Some(
                invert
                    .or(d.ff_invert)
                    .unwrap_or_else(controllers::global_ff_invert),
            );
            done = Some(d);
        }
        _ => {}
    }
    if let Some(s) = next {
        wz.step = s;
    }
    if let Some(d) = done {
        let i = wz.device;
        devices[i] = d;
        crate::lab_pads::save(pads, &devices);
        *w = None;
    }
}

/// The axes found: `rest` where everything rested, `held` with the wheel turned left and the
/// throttle, the brake and the clutch pressed (all None: that step skipped).
fn assist_result(rest: &Axes, held: &[Axes]) -> [Option<(Func, bool)>; 8] {
    let mut axes: [Option<(Func, bool)>; 8] = [None; 8];
    let steer = held.first().and_then(|a| moved_most(rest, a, &[]));
    if let Some((k, delta)) = steer {
        // turned left the value falls: else the axis runs the other way
        axes[k] = Some((Func::Steering, delta > 0.0));
    }
    let taken: Vec<usize> = steer.map(|s| vec![s.0]).unwrap_or_default();
    let pedal = |i: usize, ex: &[usize]| held.get(i).and_then(|a| moved_most(rest, a, ex));
    let throttle = pedal(1, &taken);
    let brake = pedal(2, &taken);
    match (throttle, brake) {
        // both pedals on one axis: the throttle towards the raw maximum (as Omsi.exe), else reversed
        (Some((kt, dt)), Some((kb, db))) if kt == kb && dt * db < 0.0 => {
            axes[kt] = Some((Func::ThrottleBrake, dt < 0.0))
        }
        _ => {
            if let Some((k, dl)) = throttle {
                axes[k] = Some((Func::Throttle, dl < 0.0));
            }
            if let Some((k, dl)) = brake.filter(|b| Some(b.0) != throttle.map(|t| t.0)) {
                axes[k] = Some((Func::Brake, dl < 0.0));
            }
        }
    }
    let mut ex = taken;
    ex.extend(throttle.map(|t| t.0));
    ex.extend(brake.map(|t| t.0));
    if let Some((k, dl)) = pedal(3, &ex) {
        axes[k] = Some((Func::Clutch, dl < 0.0));
    }
    axes
}

/// The axis that moved most from `rest` to `now` (at least a sixth of its travel), not one of
/// `exclude`: (slot, how far, signed).
fn moved_most(rest: &Axes, now: &Axes, exclude: &[usize]) -> Option<(usize, f32)> {
    (0..8)
        .filter(|k| !exclude.contains(k))
        .filter_map(|k| Some((k, now[k]? - rest[k].unwrap_or(0.0))))
        .filter(|(_, d)| d.abs() > 0.33)
        .max_by(|a, b| a.1.abs().total_cmp(&b.1.abs()))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_assistant_finds_the_wheel_and_pedals_it_was_shown() {
        let rest: Axes = [
            Some(0.0),
            Some(-1.0),
            Some(-1.0),
            Some(1.0),
            None,
            None,
            None,
            None,
        ];
        let held = [
            [
                Some(-0.9),
                Some(-1.0),
                Some(-1.0),
                Some(1.0),
                None,
                None,
                None,
                None,
            ],
            [
                Some(0.0),
                Some(1.0),
                Some(-1.0),
                Some(1.0),
                None,
                None,
                None,
                None,
            ],
            [
                Some(0.0),
                Some(-1.0),
                Some(1.0),
                Some(1.0),
                None,
                None,
                None,
                None,
            ],
            [None; 8],
        ];
        let axes = assist_result(&rest, &held);
        assert_eq!(axes[0], Some((Func::Steering, false)));
        assert_eq!(axes[1], Some((Func::Throttle, false)));
        assert_eq!(axes[2], Some((Func::Brake, false)));
        assert_eq!(axes[3], None, "the clutch was skipped");
    }

    #[test]
    fn pedals_on_one_axis_are_throttle_and_brake() {
        let rest: Axes = [Some(0.0), Some(0.0), None, None, None, None, None, None];
        let held = [
            [Some(-1.0), Some(0.0), None, None, None, None, None, None],
            [Some(0.0), Some(1.0), None, None, None, None, None, None],
            [Some(0.0), Some(-1.0), None, None, None, None, None, None],
            [None; 8],
        ];
        let axes = assist_result(&rest, &held);
        assert_eq!(axes[1], Some((Func::ThrottleBrake, false)));
        assert_eq!(moved_most(&rest, &held[1], &[1]), None);
    }
}
