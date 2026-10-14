use super::*;
use gilrs::Axis;

impl Controllers {
    /// Read the devices: the analog controls, and the button actions into `actions`.
    pub fn poll(&mut self) -> Analog {
        let mut out = Analog::default();
        for (name, n, down) in self.devices.poll() {
            if let Ok(mut p) = PRESSED.lock() {
                p.retain(|(d, b)| !(*b == n && d == &name));
                if down {
                    p.push((name.clone(), n));
                }
            }
            if self.off(&name) {
                continue;
            }
            if let Some(action) = find_device_cfg(&self.cfg, &name)
                .and_then(|d| d.buttons.get(n))
                .filter(|a| !a.0.is_empty())
            {
                self.actions.push((action.0.clone(), down));
            }
        }
        if !self.enabled {
            return out;
        }
        // the devices set up in gamectrler.cfg first; a device the file does not know only
        // gives what none of them does - a pad lying beside a set-up wheel held the steering
        // at its own centre, whichever the system listed first
        let mut pads: Vec<(Option<&DeviceCfg>, Connected)> = self
            .devices
            .connected()
            .into_iter()
            .filter(|c| !self.off(&c.name))
            .map(|c| (find_device_cfg(&self.cfg, &c.name), c))
            .collect();
        pads.sort_by_key(|(cfg, _)| cfg.is_none());
        let present: Vec<String> = pads.iter().map(|(_, c)| c.name.clone()).collect();
        let sources = self.sources.clone();
        let gives = |i: usize, name: &str| may_give(&sources, &present, i, name);
        let mut steer: Option<(String, f32, bool)> = None;
        let mut steering_set_up = false;
        for (cfg, c) in pads {
            if let Some((k, v)) = c.axes.iter().find(|(_, v)| v.abs() > 0.5) {
                let key = format!("axis:{}", c.name);
                if !self.announced.contains(&key) {
                    log::info!(
                        "game controller {}: axis {k} at {v:.2} (set up in gamectrler.cfg: {}, gamepad: {})",
                        c.name,
                        cfg.is_some(),
                        c.gamepad
                    );
                    self.announced.push(key);
                }
            }
            match cfg {
                Some(d) => {
                    for (k, v) in c.axes.iter().copied() {
                        let Some((f, inverted)) = d.axes[k] else {
                            continue;
                        };
                        let v = d.calibrated(k, v);
                        let dz = d.deadzone(k);
                        if matches!(f, Func::Steering) {
                            if !gives(0, &c.name) {
                                continue;
                            }
                            steering_set_up = true;
                            let (steering, position) =
                                configured_steering(&c, d, k, v, self.steer_gain);
                            if out.steering.map_or(true, |s| s.abs() < steering.abs()) {
                                out.stick = position.is_none();
                            }
                            set(&mut out.steering, steering);
                            if let Some(position) = position.filter(|_| steer.is_none()) {
                                steer = Some((c.name.clone(), position, c.ff));
                            }
                            continue;
                        }
                        let v = if inverted { -v } else { v };
                        if let Func::LookX | Func::LookY = f {
                            let i = (f == Func::LookY) as usize;
                            if out.look[i] == 0.0 {
                                out.look[i] = look_axis(v);
                            }
                            continue;
                        }
                        // the characteristic set up for the axis (gamectrler.cfg flags)
                        let v = axis_shape((v + 1.0) * 0.5, d.axis_flags[k]) * 2.0 - 1.0;
                        // the dead zone: round the wheel's centre, or at a pedal's rest
                        let v = match f {
                            Func::ThrottleBrake => {
                                v.signum() * ((v.abs() - dz).max(0.0) / (1.0 - dz))
                            }
                            _ => ((v + 1.0 - 2.0 * dz).max(0.0) / (1.0 - dz)) - 1.0,
                        };
                        // a pedal travels the whole range, -1 up to 1 down
                        let pedal = pedal_ends(((v + 1.0) * 0.5).clamp(0.0, 1.0));
                        match f {
                            Func::Steering | Func::LookX | Func::LookY => {
                                unreachable!("steering and looking handled before pedal mapping")
                            }
                            Func::Throttle if gives(1, &c.name) => {
                                set(&mut out.throttle, pedal_curve(pedal, self.pedal_throttle))
                            }
                            Func::Brake if gives(2, &c.name) => {
                                set(&mut out.brake, pedal_curve(pedal, self.pedal_brake))
                            }
                            Func::Clutch if gives(3, &c.name) => set(&mut out.clutch, pedal),
                            Func::ThrottleBrake => {
                                // Omsi.exe: throttle = 2v-1 and brake = 1-2v (v = 0..1), so the
                                // raw-maximum half is the throttle
                                if gives(1, &c.name) {
                                    set(
                                        &mut out.throttle,
                                        pedal_curve(v.max(0.0), self.pedal_throttle),
                                    );
                                }
                                if gives(2, &c.name) {
                                    set(
                                        &mut out.brake,
                                        pedal_curve((-v).max(0.0), self.pedal_brake),
                                    );
                                }
                            }
                            Func::Throttle | Func::Brake | Func::Clutch => {}
                        }
                    }
                }
                None if c.gamepad => {}
                None => {
                    // a wheel or joystick nobody has set up yet: its X axis steers (as on
                    // nearly every wheel), the pedals wait for the set-up (Launcher →
                    // Controls → Game controllers), said once on the screen
                    if !self.announced.contains(&c.name) {
                        self.announced.push(c.name.clone());
                        log::info!(
                            "game controller {} is not set up: its X axis steers",
                            c.name
                        );
                        self.notice = Some(format!(
                            "{} is not set up: it steers; set up its pedals and buttons in the launcher (Controls → Game controllers)",
                            c.name
                        ));
                    }
                    if let Some((_, v)) = c
                        .axes
                        .iter()
                        .find(|(k, _)| *k == 0)
                        .filter(|_| gives(0, &c.name))
                    {
                        // (a joystick's centre is slack, so it gets a little dead zone; a
                        // force-feedback wheel's is not: 2 % of it held a 1080° wheel's
                        // picture 11° behind the rim, #866)
                        let dz_free = 0.0;
                        let (steering, position) =
                            wheel_steering(*v, false, 0, dz_free, self.steer_gain);
                        out.steering.get_or_insert(steering);
                        if steer.is_none() {
                            steer = Some((c.name.clone(), position, c.ff));
                        }
                    }
                }
            }
        }
        // gamepads: the left stick steers, the triggers are the pedals
        let di = self.devices.direct_input();
        if let Some(g) = self.devices.gilrs.as_ref() {
            for (_pad_id, pad) in g.gamepads() {
                // An Xbox-type pad's DirectInput twin is left out on Windows, so the pad is
                // read here even when gamectrler.cfg names it (#171).
                let xinput = cfg!(windows) && xinput_name(pad.name());
                #[cfg(target_os = "linux")]
                let forced_gamepad =
                    self.devices.linux_axes.mode(_pad_id) == crate::evdev_axes::AxisMode::Gamepad;
                #[cfg(not(target_os = "linux"))]
                let forced_gamepad = false;
                if pad.mapping_source() == gilrs::MappingSource::None && !forced_gamepad {
                    continue;
                }
                #[cfg(target_os = "linux")]
                if self.devices.linux_axes.is_native(_pad_id) {
                    continue;
                }
                #[cfg(target_os = "macos")]
                if self.devices.hid_wheel(pad.name()) {
                    continue;
                }
                if (di && !xinput) || self.off(pad.name()) {
                    continue;
                }
                let cfg = find_device_cfg(&self.cfg, pad.name());
                let free = PadDefaults::of(cfg);
                let x = stick_deadzone(
                    pad.value(Axis::LeftStickX),
                    cfg.map_or_else(default_stick_deadzone, |d| d.stick_deadzone(0)),
                );
                // A pad set up without a steering axis would otherwise not steer at all.
                let steers = free.steering
                    && gives(0, pad.name())
                    && stick_steers(out.steering, steering_set_up, x);
                // (said once per pad: the stick moved, and whether it steers - a report of
                // "the sticks do nothing" then says which way the pad came in)
                if x.abs() > 0.5
                    && !self
                    .announced
                    .iter()
                    .any(|n| n == &format!("stick:{}", pad.name()))
                {
                    self.announced.push(format!("stick:{}", pad.name()));
                    log::info!(
                        "game controller {}: left stick {x:.2}, steers: {steers} (layout {:?})",
                        pad.name(),
                        pad.mapping_source()
                    );
                }
                let rt = pad
                    .button_data(gilrs::Button::RightTrigger2)
                    .map(|d| d.value())
                    .unwrap_or(0.0);
                let lt = pad
                    .button_data(gilrs::Button::LeftTrigger2)
                    .map(|d| d.value())
                    .unwrap_or(0.0);
                if steers {
                    out.steering = Some(x);
                    out.stick = true;
                }
                if free.throttle && gives(1, pad.name()) {
                    out.throttle
                        .get_or_insert(pedal_curve(rt, self.pedal_throttle));
                }
                if free.brake && gives(2, pad.name()) {
                    out.brake.get_or_insert(pedal_curve(lt, self.pedal_brake));
                }
                // the right stick looks round, as the truck games have it (#454)
                if free.look && out.look == [0.0, 0.0] {
                    out.look = [
                        look_axis(pad.value(Axis::RightStickX)),
                        look_axis(-pad.value(Axis::RightStickY)),
                    ];
                }
            }
        }
        if self.centre && out.stick && out.steering.is_some_and(|s| s.abs() < CENTRE_SNAP) {
            out.steering = Some(0.0);
        }
        let before = self
            .steer
            .as_ref()
            .filter(|s| steer.as_ref().is_some_and(|n| n.0 == s.0))
            .map(|s| s.1);
        self.steer = steer.map(|(name, v, ff)| (name, v, before.unwrap_or(v), ff));
        out
    }
}
