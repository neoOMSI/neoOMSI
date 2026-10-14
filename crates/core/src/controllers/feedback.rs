use super::*;

/// What the force feedback is made of this frame (OMSI's `FF_*` variables of the bus and
/// its speed).
#[derive(Debug, Clone, Copy, Default)]
pub struct FfInput {
    /// Driving (from the driver's seat): the forces are on; else the wheel is let go.
    pub on: bool,
    pub kmh: f32,
    /// Sideways acceleration in the bus frame (m/s², right positive).
    pub lateral_accel: f32,
    /// Short jolt from wheel suspension travel or an impact, 0..1.
    pub wheel_bump: f32,
    /// Time since the latest wheel jolt, for a repeatable initial kick.
    pub(crate) wheel_bump_age: f32,
    /// `FF_Vib_Amp` 0..1 and `FF_Vib_Period` (hundredths of a second) of the scripts.
    pub vib_amp: f32,
    pub vib_period: f32,
    pub dt: f32,
}

impl Controllers {
    /// On a force-feedback wheel, combine parking resistance, centring, the bus's
    /// lateral motion, front-wheel bumps and script-driven vibration. Other devices
    /// get vibration as rumble.
    pub fn feedback(&mut self, f: FfInput) {
        let on = self.enabled && f.on && self.ff_enabled && self.focused;
        let mut f = f;
        if on {
            // The bus body reacts to road and tyre forces every physics step. A short
            // filter keeps those impulses from becoming sharp forces at the wheel.
            let blend = (f.dt / 0.15).clamp(0.0, 1.0);
            self.ff_lateral += (f.lateral_accel.clamp(-6.0, 6.0) - self.ff_lateral) * blend;
        } else {
            self.ff_lateral = 0.0;
        }
        f.lateral_accel = self.ff_lateral;
        let incoming_bump = if on {
            f.wheel_bump.clamp(0.0, 1.0)
        } else {
            0.0
        };
        if incoming_bump > 0.05 && (self.ff_bump < 0.02 || incoming_bump > self.ff_bump + 0.12) {
            self.ff_bump_age = 0.0;
        } else {
            self.ff_bump_age += f.dt.max(0.0);
        }
        self.ff_bump = if on {
            (self.ff_bump - f.dt.max(0.0) * 6.0).max(incoming_bump)
        } else {
            0.0
        };
        f.wheel_bump = self.ff_bump;
        f.wheel_bump_age = self.ff_bump_age;
        if self.ff_source_logged.as_deref() != self.steer.as_ref().map(|s| s.0.as_str()) {
            self.ff_source_logged = self.steer.as_ref().map(|s| s.0.clone());
            if let Some((name, _, _, effect)) = self.steer.as_ref() {
                let cfg = find_device_cfg(&self.cfg, name);
                let (steering, vibration) = cfg.and_then(|d| d.ff_scale).unwrap_or((1.0, 1.0));
                #[cfg(windows)]
                let axis_reversed = self
                    .devices
                    .di
                    .as_ref()
                    .is_some_and(|di| force_axis_reversed(cfg, di.force_axis(name)));
                #[cfg(not(windows))]
                let axis_reversed = calibrated_steering_reversed(cfg);
                log::info!(
                    "force feedback: steering source {name}, effect available: {effect}, config: {}, steering force: {steering:.2}, vibration: {vibration:.2}, invert: {}",
                    cfg.map(|d| d.name.as_str()).unwrap_or("none"),
                    feedback_inverted(cfg, self.ff_invert, axis_reversed)
                );
            }
        }
        #[cfg(windows)]
        if let (Some((name, x, x0, true)), Some(di)) =
            (self.steer.clone(), self.devices.di.as_mut())
        {
            // (the file's [FFScale] of the device: steering forces, then vibration)
            let cfg = find_device_cfg(&self.cfg, &name);
            let (k_s, k_e) = cfg.and_then(|d| d.ff_scale).unwrap_or((1.0, 1.0));
            // The scripts' shaking is the wheel's own periodic effect where it has one
            // (a rattle of a few ms sampled once a frame comes out as a random wobble).
            let vib_amp = if on {
                f.vib_amp.clamp(0.0, 1.0) * VIB_SHARE * k_e.clamp(0.0, 2.0)
            } else {
                0.0
            };
            let f = if di.set_vibration(&name, vib_amp, f.vib_period) {
                FfInput { vib_amp: 0.0, ..f }
            } else {
                f
            };
            let force = if on {
                wheel_force(&f, x, x0, &mut self.ff_t, k_s, k_e)
            } else {
                0.0
            };
            // The wheel force is calculated from the steering axis after its configured
            // reversal, while DirectInput sends forces in the physical axis direction.
            let axis_reversed = force_axis_reversed(cfg, di.force_axis(&name));
            let force = if feedback_inverted(cfg, self.ff_invert, axis_reversed) {
                -force
            } else {
                force
            };
            if di.set_force(&name, force) {
                return;
            }
        }
        #[cfg(all(target_os = "linux", target_pointer_width = "64"))]
        if let Some((name, x, x0, true)) = self.steer.clone() {
            let other = self.wheel.as_ref().is_some_and(|w| w.name != name);
            let retry = self.wheel.is_none()
                && self.wheel_tried.as_ref().is_none_or(|(n, t)| {
                *n != name || t.elapsed() > std::time::Duration::from_secs(2)
            });
            if other || retry {
                self.wheel_tried = Some((name.clone(), std::time::Instant::now()));
                self.wheel = crate::evdev_ff::Wheel::open(&name);
            }
            if let Some(w) = self.wheel.as_mut() {
                let cfg = find_device_cfg(&self.cfg, &name);
                let (k_s, k_e) = cfg.and_then(|d| d.ff_scale).unwrap_or((1.0, 1.0));
                let force = if on {
                    wheel_force(&f, x, x0, &mut self.ff_t, k_s, k_e)
                } else {
                    0.0
                };
                let axis_reversed = calibrated_steering_reversed(cfg);
                let inverted = feedback_inverted(cfg, self.ff_invert, axis_reversed);
                if !w.set_force(if inverted { -force } else { force }) {
                    log::warn!("force feedback: {name} went away; looking for it again");
                    self.wheel = None;
                }
                return;
            }
        }
        let _ = (&self.steer, &self.ff_t, wheel_force);
        self.rumble_feedback(
            if on {
                f.vib_amp.max(f.wheel_bump * 0.75)
            } else {
                0.0
            },
            f.vib_period,
        );
    }

    /// The shaking as a rumble (`FF_Vib_Amp`, `FF_Vib_Period`: OMSI hands DirectInput
    /// Round(period × 10000) µs, so a hundredth of a second per unit - the shaking comes in
    /// pulses that long, on for half of it; 0 is a steady rumble).
    fn rumble_feedback(&mut self, amp: f32, period: f32) {
        let amp = amp.clamp(0.0, 1.0);
        let period = if period.is_finite() {
            period.clamp(0.0, 100.0)
        } else {
            0.0
        };
        let Some(g) = self.devices.gilrs.as_mut() else {
            return;
        };
        if let Some((_, was, was_period)) = &self.rumble {
            if (was - amp).abs() < 0.02 && (was_period - period).abs() < 0.05 {
                return;
            }
        }
        self.rumble = None;
        if amp < 0.01 {
            return;
        }
        let pads: Vec<gilrs::GamepadId> = g
            .gamepads()
            .filter(|(_, p)| p.is_ff_supported())
            .map(|(id, _)| id)
            .collect();
        if pads.is_empty() {
            return;
        }
        // (the file's [FFScale] of a set-up device scales it)
        let scale = self
            .cfg
            .iter()
            .find_map(|d| d.ff_scale)
            .map(|s| s.1)
            .unwrap_or(1.0)
            .clamp(0.0, 2.0);
        let m = ((amp * scale).min(1.0) * u16::MAX as f32) as u16;
        let ms = (period * 10.0).round() as u32;
        let scheduling = if ms >= 20 {
            gilrs::ff::Replay {
                after: gilrs::ff::Ticks::from_ms(0),
                play_for: gilrs::ff::Ticks::from_ms(ms / 2),
                with_delay: gilrs::ff::Ticks::from_ms(ms - ms / 2),
            }
        } else {
            Default::default()
        };
        let effect = gilrs::ff::EffectBuilder::new()
            .add_effect(gilrs::ff::BaseEffect {
                kind: gilrs::ff::BaseEffectType::Strong { magnitude: m },
                scheduling,
                ..Default::default()
            })
            .add_effect(gilrs::ff::BaseEffect {
                kind: gilrs::ff::BaseEffectType::Weak { magnitude: m / 2 },
                scheduling,
                ..Default::default()
            })
            .repeat(gilrs::ff::Repeat::Infinitely)
            .gamepads(&pads)
            .finish(g);
        if let Ok(e) = effect {
            let _ = e.play();
            self.rumble = Some((e, amp, period));
        }
    }
}

/// Front-wheel contact reaches the steering linkage directly; rear-wheel contact
/// reaches it through the bus body at a lower strength.
impl Controllers {
    /// The wheels' jolt this frame (None: not driving, the travel is settled anew on return).
    pub(crate) fn wheel_bump(
        &mut self,
        body: Option<&::simulation::rigid::RigidBody>,
        kmh: f32,
        dt: f32,
    ) -> f32 {
        let Some(body) = body else {
            self.settled.clear();
            return 0.0;
        };
        let travel: Vec<f32> = body.wheels.iter().map(|w| w.compression).collect();
        let ripples = settle(&mut self.settled, &travel, dt);
        body.wheels
            .iter()
            .zip(ripples)
            .enumerate()
            .map(|(i, (w, ripple))| {
                let impact_speed = body
                    .wheel_impacts
                    .iter()
                    .filter(|hit| hit.obstacle == i)
                    .map(|hit| hit.speed)
                    .fold(0.0, f32::max);
                bump_strength(ripple, impact_speed, kmh) * if w.steered { 1.0 } else { 0.55 }
            })
            .fold(0.0, f32::max)
    }
}

/// How far each wheel's travel leaves where it settled (a 1-2 cm road seam is no jolt).
pub(super) fn settle(settled: &mut Vec<f32>, travel: &[f32], dt: f32) -> Vec<f32> {
    if settled.len() != travel.len() {
        *settled = travel.to_vec();
    }
    let k = 1.0 - (-dt.max(0.0) / 0.1).exp();
    travel
        .iter()
        .zip(settled.iter_mut())
        .map(|(t, s)| {
            let ripple = t - *s;
            *s += ripple * k;
            ripple
        })
        .collect()
}

pub(super) fn bump_strength(ripple: f32, impact_speed: f32, kmh: f32) -> f32 {
    let suspension = ((ripple.abs() - 0.012) / 0.09).clamp(0.0, 1.0);
    let impact = ((impact_speed - 0.12) / 1.1).clamp(0.0, 1.0);
    suspension.max(impact) * (kmh.abs() / 4.0).clamp(0.0, 1.0)
}

/// How much of the full force `FF_Vib_Amp` 1 shakes the wheel with.
pub(super) const VIB_SHARE: f32 = 0.25;

/// The force on a wheel standing at `x` (-1 full left .. 1), `x0` the frame before: -1..1.
/// Tyre scrub resists turning the wheel at a standstill and falls away once the bus rolls.
/// Self-aligning torque then returns the wheel to centre, with a softer response near full
/// lock and feedback from the bus's lateral acceleration. Front-wheel jolts and the
/// scripts' shaking come on top.
pub(super) fn wheel_force(f: &FfInput, x: f32, x0: f32, t: &mut f32, k_springs: f32, k_effects: f32) -> f32 {
    let dt = f.dt.max(1e-3);
    let v = f.kmh.abs();
    let x = x.clamp(-1.0, 1.0);
    // A parked bus has no rolling self-aligning torque. Ignore tiny physics
    // speed fluctuations, then smoothly restore the usual forces by 5 km/h.
    let rolling = ((v - 0.5) / 4.5).clamp(0.0, 1.0);
    let rolling = rolling * rolling * (3.0 - 2.0 * rolling);
    // At road speed, power steering gives the driver a firmer sense of direction.
    // Keep parking and town-speed forces familiar while separating 70 km/h from 10 km/h.
    let road_speed = ((v - 20.0) / 50.0).clamp(0.0, 1.0);
    let spring_strength = (0.22 + 0.28 * v / (v + 10.0)) * (1.0 + 0.5 * road_speed);
    let moving_steering_gain = 1.0 + 0.18 * v / (v + 8.0);
    // More return torque at small and medium angles, where wheel friction can
    // otherwise stop the return. Compensate wheel friction near the centre with
    // a smooth extra torque, tapered away at larger angles. It crosses zero
    // continuously so there is no fixed kick when the wheel passes the centre.
    let centre_return = 0.025 * x / (x * x + 0.004 * 0.004).sqrt() / (1.0 + (x / 0.12).powi(4));
    let spring = -(spring_strength * x / (0.5 + 1.15 * x.abs()) + centre_return) * rolling;
    let road_align =
        -(f.lateral_accel / 9.81).clamp(-0.45, 0.45) * 0.25 * (v / 5.0).clamp(0.0, 1.0) * rolling;
    // Assisted steering should not demand ever more hand force near full lock.
    let lock_assist = 1.0 / (1.0 + 0.55 * x * x);
    let turning_speed = ((x - x0) / dt).clamp(-4.0, 4.0);
    // Without a steering-column torque sensor, motion away from the centre is our
    // indication that the driver is actively turning. Assist that motion, but keep
    // the full self-aligning torque when the wheel is held or let go.
    let turning_out = (x * turning_speed * 2.0).clamp(0.0, 1.0);
    let assist = 1.0 - (0.4 - 0.16 * (v / 80.0).min(1.0)) * turning_out;
    let parking_drag = 0.018 + 0.12 / (1.0 + (v / 6.0).powi(2));
    // Once rolling, reduce drag on return so it does not cancel the centring
    // force. At a standstill, resist motion equally in either direction.
    let returning = x * turning_speed < 0.0;
    let drag = -turning_speed * parking_drag * if returning { 1.0 - 0.8 * rolling } else { 1.0 };
    // Keep some damping on return as well as when turning out. Reducing all
    // resistance on return lets a quick wheel overshoot and oscillate around
    // the centre, especially when force commands arrive at a low frame rate.
    // This only resists motion: it adds no holding force or parked centring.
    let damping = -0.03 * turning_speed * rolling;
    *t += dt;
    let period = (f.vib_period * 0.01).max(0.02);
    let shake = f.vib_amp.clamp(0.0, 1.0) * VIB_SHARE * (std::f32::consts::TAU * *t / period).sin();
    // Preserve small road details while softening kerb-sized peaks. One short
    // kick and rebound feels less like a continuously shaking wheel mount.
    let bump = f.wheel_bump.clamp(0.0, 1.0).sqrt()
        * 0.46
        * (std::f32::consts::TAU * f.wheel_bump_age * 6.5).cos();
    let steering =
        ((spring + road_align) * lock_assist * assist + drag) * moving_steering_gain + damping;
    (steering * k_springs.clamp(0.0, 2.0) + (shake + bump) * k_effects.clamp(0.0, 2.0))
        .clamp(-1.0, 1.0)
}

#[cfg_attr(not(windows), allow(dead_code))]
pub(super) fn force_axis_reversed(cfg: Option<&DeviceCfg>, axis: Option<usize>) -> bool {
    axis.and_then(|axis| cfg.and_then(|d| d.axes.get(axis).copied().flatten()))
        .is_some_and(|(function, reversed)| function == Func::Steering && reversed)
}

pub(super) fn feedback_inverted(cfg: Option<&DeviceCfg>, global: bool, axis_reversed: bool) -> bool {
    cfg.and_then(|d| d.ff_invert).unwrap_or(global) ^ axis_reversed
}

#[cfg_attr(windows, allow(dead_code))]
pub(super) fn calibrated_steering_reversed(cfg: Option<&DeviceCfg>) -> bool {
    // Linux previously applied the global sign directly; preserve it for uncalibrated wheels.
    cfg.filter(|d| d.ff_invert.is_some())
        .and_then(|d| {
            d.axes
                .iter()
                .flatten()
                .find(|(function, _)| *function == Func::Steering)
        })
        .is_some_and(|(_, reversed)| *reversed)
}
