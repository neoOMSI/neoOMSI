use super::*;
use gilrs::{Axis, EventType, Gilrs};

/// A device connected now: its name, its axes (DirectInput slot, -1..1), whether the system
/// knows it as a gamepad (a known layout of sticks and triggers), and whether it can push
/// back (force feedback).
#[derive(Debug, Clone)]
pub(crate) struct Connected {
    pub name: String,
    pub hardware_id: Option<(u16, u16)>,
    pub axes: Vec<(usize, f32)>,
    pub gamepad: bool,
    #[cfg(target_os = "linux")]
    pub native_axes: bool,
    pub ff: bool,
    /// The hardware advertises FFB, even when this window has not created an effect.
    pub ff_capable: bool,
    /// How many buttons it has (0: the system does not say).
    pub buttons: usize,
}

impl Connected {
    pub(crate) fn gamepad_axes(&self) -> bool {
        #[cfg(target_os = "linux")]
        return self.gamepad && !self.native_axes;
        #[cfg(not(target_os = "linux"))]
        self.gamepad
    }
}

/// The first button number of the hat switches' directions (4 hats x up, right, down, left).
#[cfg_attr(not(windows), allow(dead_code))]
pub(crate) const HAT_BUTTONS: usize = 128;

/// The devices of every kind: gilrs (gamepads everywhere; on macOS and Linux every device),
/// and on Windows DirectInput for everything a gamepad is not (`crate::dinput`) - many wheels
/// never show up in the system's newer interface that gilrs uses there.
pub(crate) struct Devices {
    pub(super) gilrs: Option<Gilrs>,
    #[cfg(target_os = "linux")]
    pub(super) linux_axes: crate::evdev_axes::Devices,
    #[cfg(windows)]
    pub(super) di: Option<crate::dinput::DirectInput>,
    /// macOS: every axis element of every wheel and joystick, as last read (see `mac_hid`)
    #[cfg(target_os = "macos")]
    hid: Option<crate::mac_hid::MacHid>,
    #[cfg(target_os = "macos")]
    pub(super) hid_axes: Vec<(String, Vec<(u32, f32)>)>,
    #[cfg(target_os = "linux")]
    hats: Vec<(String, [i8; 8])>,
}

impl Devices {
    /// `hwnd`: the window (Windows: the devices belong to it); `ff`: take them for force
    /// feedback (the game, not the launcher).
    pub fn new(hwnd: Option<isize>, ff: bool) -> Devices {
        // without gilrs's default filters: its dead zone took 10 % of every axis - on a
        // wheel of 1800 degrees, 90 degrees either side of the middle did nothing - and its
        // jitter filter held back small movements; the settings' dead zone is the only one
        let gilrs = gilrs::GilrsBuilder::new()
            .with_default_filters(false)
            .build()
            .map_err(|e| log::info!("game controllers: {e}"))
            .ok();
        #[cfg(target_os = "linux")]
        let linux_axes = {
            let mut axes = crate::evdev_axes::Devices::new();
            axes.poll(gilrs.as_ref(), saved_axis_mode);
            axes
        };
        #[cfg(windows)]
        let di = hwnd.and_then(|h| crate::dinput::DirectInput::new(h, ff));
        #[cfg(not(windows))]
        let _ = (hwnd, ff);
        Devices {
            gilrs,
            #[cfg(target_os = "linux")]
            linux_axes,
            #[cfg(windows)]
            di,
            #[cfg(target_os = "macos")]
            hid: crate::mac_hid::MacHid::new(),
            #[cfg(target_os = "macos")]
            hid_axes: Vec::new(),
            #[cfg(target_os = "linux")]
            hats: Vec::new(),
        }
    }

    /// macOS: the HID device of this name has the axes of a wheel or pedals (a slider, a
    /// dial, or the simulation page's steering, accelerator, brake, clutch).
    #[cfg(target_os = "macos")]
    pub(crate) fn hid_wheel(&self, name: &str) -> bool {
        self.hid_axes.iter().any(|(n, axes)| {
            names_match(n, name)
                && axes
                .iter()
                .any(|(c, _)| matches!(*c, 0x10036 | 0x10037) || (*c >> 16) == 2)
        })
    }

    pub(super) fn direct_input(&self) -> bool {
        #[cfg(windows)]
        return self.di.is_some();
        #[cfg(not(windows))]
        false
    }

    #[cfg(target_os = "linux")]
    pub(crate) fn set_axis_mode(&mut self, name: &str, mode: crate::evdev_axes::AxisMode) {
        self.linux_axes.set_mode(name, mode);
    }

    /// A device was plugged in or removed; ask the worker to rescan without blocking a frame.
    pub(crate) fn refresh(&self) {
        #[cfg(windows)]
        if let Some(d) = self.di.as_ref() {
            d.refresh();
        }
    }

    /// Release foreground wheel effects when the game loses focus.
    pub(crate) fn set_focus(&mut self, focused: bool) {
        #[cfg(windows)]
        if let Some(d) = self.di.as_mut() {
            d.set_focus(focused);
        }
        #[cfg(not(windows))]
        let _ = focused;
    }

    /// Read the devices; the buttons pressed (true) and let go since the last call:
    /// (device, button number from 0, as DirectInput and `gamectrler.cfg` count them).
    pub fn poll(&mut self) -> Vec<(String, usize, bool)> {
        let mut out = Vec::new();
        let di = self.direct_input();
        #[cfg(windows)]
        let xinput_pads = self
            .gilrs
            .as_ref()
            .is_some_and(|g| g.gamepads().any(|(_, p)| xinput_name(p.name())));
        #[cfg(windows)]
        let is_di = |pad: &gilrs::Gamepad<'_>| -> bool {
            let id = pad.vendor_id().zip(pad.product_id());
            self.di.as_ref().is_some_and(|d| {
                d.devices.iter().any(|dev| {
                    include_direct_input_device(&dev.name, dev.ff_capable(), xinput_pads)
                        && (names_match(&dev.name, pad.name())
                        || id.is_some_and(|id| dev.hardware_id == Some(id)))
                })
            })
        };
        #[cfg(not(windows))]
        let is_di = |_pad: &gilrs::Gamepad<'_>| -> bool { false };

        if let Some(g) = self.gilrs.as_mut() {
            while let Some(ev) = g.next_event() {
                #[cfg(target_os = "linux")]
                if matches!(ev.event, EventType::Connected | EventType::Disconnected) {
                    self.linux_axes.forget(ev.id);
                }
                let pad = g.gamepad(ev.id);
                match ev.event {
                    EventType::Connected => log::info!(
                        "game controller connected: {} (layout {:?}, DirectInput {})",
                        pad.name(),
                        pad.mapping_source(),
                        di
                    ),
                    // DirectInput handles wheels on Windows; system-mapped gamepads
                    // such as Xbox controllers are listed through gilrs.
                    EventType::ButtonPressed(_, code) | EventType::ButtonReleased(_, code)
                        if use_gilrs_buttons(di, is_system_gamepad(pad.name(), is_di(&pad))) =>
                    {
                        if let Some(n) = button_number(&pad, code) {
                            out.push((
                                pad.name().to_string(),
                                n,
                                matches!(ev.event, EventType::ButtonPressed(..)),
                            ));
                        }
                    }
                    #[cfg(target_os = "linux")]
                    EventType::AxisChanged(_, value, code)
                        if code.into_u32() >> 16 == 3
                            && (0x10..0x18).contains(&(code.into_u32() & 0xFFFF)) =>
                    {
                        let axis = (code.into_u32() & 0xFFFF) as usize - 0x10;
                        let name = pad.name().to_string();
                        let k = match self.hats.iter().position(|(n, _)| *n == name) {
                            Some(k) => k,
                            None => {
                                self.hats.push((name.clone(), [0; 8]));
                                self.hats.len() - 1
                            }
                        };
                        let now = if value > 0.5 {
                            1
                        } else if value < -0.5 {
                            -1
                        } else {
                            0
                        };
                        let was = std::mem::replace(&mut self.hats[k].1[axis], now);
                        let (hat, y) = (axis / 2, axis % 2 == 1);
                        let dir = |v: i8| match (y, v) {
                            (true, -1) => Some(0),
                            (false, 1) => Some(1),
                            (true, 1) => Some(2),
                            (false, -1) => Some(3),
                            _ => None,
                        };
                        if was != now {
                            if let Some(d) = dir(was) {
                                out.push((name.clone(), HAT_BUTTONS + hat * 4 + d, false));
                            }
                            if let Some(d) = dir(now) {
                                out.push((name, HAT_BUTTONS + hat * 4 + d, true));
                            }
                        }
                    }
                    _ => {}
                }
            }
        }
        #[cfg(windows)]
        if let Some(d) = self.di.as_mut() {
            d.poll();
            out.extend(d.events.drain(..).filter(|(name, ..)| {
                d.devices
                    .iter()
                    .find(|dev| &dev.name == name)
                    .map_or(true, |dev| {
                        include_direct_input_device(&dev.name, dev.ff_capable(), xinput_pads)
                    })
            }));
        }
        #[cfg(target_os = "macos")]
        if let Some(h) = self.hid.as_mut() {
            self.hid_axes = h.read();
        }
        #[cfg(target_os = "linux")]
        {
            let cfg = read_cfg();
            self.linux_axes.poll(self.gilrs.as_ref(), |name| {
                find_device_cfg(&cfg, name)
                    .map_or_else(|| read_device(name).axis_mode, |d| d.axis_mode)
            });
        }
        out
    }

    /// The devices connected, with their axes as last read.
    pub fn connected(&self) -> Vec<Connected> {
        let mut v = Vec::new();
        // (Windows: an Xbox-type pad is gilrs's - the system's own layout -, everything else
        // DirectInput's; a wheel that a community mapping makes a "gamepad" in gilrs was
        // listed twice, "Logitech G29" beside "G29 Driving Force Racing Wheel")
        let xinput_pads = self
            .gilrs
            .as_ref()
            .is_some_and(|g| g.gamepads().any(|(_, p)| xinput_name(p.name())));
        #[cfg(windows)]
        if let Some(d) = self.di.as_ref() {
            v.extend(
                d.devices
                    .iter()
                    // A G920's DirectInput name contains "Xbox One", but it is the
                    // force-feedback wheel. Keep it even when gilrs also lists a pad.
                    .filter(|d| include_direct_input_device(&d.name, d.ff_capable(), xinput_pads))
                    .map(|d| Connected {
                        name: d.name.clone(),
                        hardware_id: d.hardware_id,
                        axes: d.axes(),
                        gamepad: false,
                        ff: d.has_ff(),
                        ff_capable: d.ff_capable(),
                        buttons: d.buttons.min(128),
                    }),
            );
        }
        let _ = xinput_pads;
        if let Some(g) = self.gilrs.as_ref() {
            for (_pad_id, pad) in g.gamepads() {
                #[cfg(not(target_os = "linux"))]
                #[allow(unused_mut)]
                let mut gamepad = pad.mapping_source() != gilrs::MappingSource::None;
                #[cfg(target_os = "linux")]
                let native_axes = self.linux_axes.is_native(_pad_id);
                #[cfg(target_os = "linux")]
                let gamepad = self.linux_axes.is_gamepad(_pad_id);
                // (macOS: a device with sliders or the simulation page's axes is a wheel or
                // pedals, whatever SDL's list calls it - the HORI Truck Control System was
                // taken as a gamepad: its left stick steered, with a gamepad's dead zone)
                #[cfg(target_os = "macos")]
                if self.hid_wheel(pad.name()) {
                    gamepad = false;
                }
                let id = pad.vendor_id().zip(pad.product_id());
                let is_di = v.iter().any(|c: &Connected| {
                    names_match(&c.name, pad.name()) || (id.is_some() && c.hardware_id == id)
                });
                if self.direct_input() && (is_di || !xinput_name(pad.name())) {
                    continue;
                }
                if v.iter()
                    .any(|c: &Connected| names_match(&c.name, pad.name()))
                {
                    continue;
                }
                #[allow(unused_mut)]
                let mut axes: Vec<(u32, f32)> = pad
                    .state()
                    .axes()
                    .map(|(c, d)| (c.into_u32(), d.value()))
                    .collect();
                #[cfg(target_os = "linux")]
                if native_axes {
                    axes = self.linux_axes.axes(_pad_id).unwrap_or_default().to_vec();
                }
                // (macOS: the device's own axis elements where it is found among them - two
                // of one usage stay two)
                #[cfg(target_os = "macos")]
                if !gamepad {
                    if let Some((_, a)) = self
                        .hid_axes
                        .iter()
                        .find(|(n, _)| names_match(n, pad.name()))
                    {
                        axes = a.clone();
                    }
                }
                #[cfg(target_os = "linux")]
                let buttons = declared_button_count(pad.name());
                #[cfg(not(target_os = "linux"))]
                let buttons = 0;
                #[cfg(target_os = "linux")]
                let mapped_axes = gamepad && !native_axes;
                #[cfg(not(target_os = "linux"))]
                let mapped_axes = gamepad;
                let axes = if mapped_axes {
                    gamepad_axes(&pad)
                } else {
                    di_slots(&axes)
                };
                #[allow(unused_mut)]
                let mut ff = pad.is_ff_supported();
                #[cfg(target_os = "linux")]
                {
                    ff |= self.linux_axes.ff_capable(_pad_id).unwrap_or(false);
                }
                v.push(Connected {
                    name: pad.name().to_string(),
                    hardware_id: id,
                    axes,
                    gamepad,
                    #[cfg(target_os = "linux")]
                    native_axes,
                    ff,
                    ff_capable: ff,
                    buttons,
                });
            }
        }
        // (and a wheel gilrs does not list at all: one whose only axes are the simulation
        // page's steering and pedals)
        #[cfg(target_os = "macos")]
        for (name, axes) in &self.hid_axes {
            if !v.iter().any(|c| names_match(&c.name, name)) {
                v.push(Connected {
                    name: name.clone(),
                    hardware_id: None,
                    axes: di_slots(axes),
                    gamepad: false,
                    ff: false,
                    ff_capable: false,
                    buttons: 0,
                });
            }
        }
        v
    }
}

pub(super) fn use_gilrs_buttons(direct_input: bool, system_gamepad: bool) -> bool {
    !direct_input || system_gamepad
}

pub(crate) fn is_system_gamepad(name: &str, is_di_device: bool) -> bool {
    !is_di_device && xinput_name(name)
}

/// A DirectInput name of an Xbox-type pad (which gilrs lists with the system's layout).
#[cfg_attr(not(windows), allow(dead_code))]
pub(crate) fn xinput_name(name: &str) -> bool {
    let n = name.to_ascii_lowercase();
    n.contains("xbox") || n.contains("xinput") || n.starts_with("controller (")
}

#[cfg_attr(not(windows), allow(dead_code))]
pub(super) fn include_direct_input_device(name: &str, ff_capable: bool, xinput_pads: bool) -> bool {
    !xinput_pads || !xinput_name(name) || ff_capable
}

/// The handle of `window` for DirectInput (Windows; elsewhere nothing is needed).
pub(crate) fn window_handle(window: &winit::window::Window) -> Option<isize> {
    #[cfg(windows)]
    {
        use winit::raw_window_handle::{HasWindowHandle, RawWindowHandle};
        if let Ok(h) = window.window_handle() {
            if let RawWindowHandle::Win32(w) = h.as_raw() {
                return Some(w.hwnd.get());
            }
        }
    }
    let _ = window;
    None
}

/// The DirectInput axis (0-5 X, Y, Z, Rx, Ry, Rz; 6, 7 the sliders) each of a device's axes
/// is, from the system's code for it: OMSI's gamectrler.cfg numbers them so. The HID usages
/// on macOS (generic desktop page 1: 0x30-0x35, slider 0x36, dial 0x37; the simulation page
/// 2: steering, accelerator, brake, clutch as Windows' HID driver places them), the evdev
/// ABS codes on Linux (ABS_X..ABS_RZ, then THROTTLE and RUDDER as the sliders), the axis
/// index on Windows. Axes of no known slot take the free ones in their order.
pub(crate) fn di_slots(axes: &[(u32, f32)]) -> Vec<(usize, f32)> {
    let known = |code: u32| -> Option<usize> {
        let (hi, lo) = (code >> 16, code & 0xFFFF);
        if cfg!(target_os = "macos") {
            match (hi, lo) {
                (1, 0x30..=0x35) => Some((lo - 0x30) as usize),
                (1, 0x36) => Some(6),
                (1, 0x37) => Some(7),
                (2, 0xC8) => Some(0), // steering
                (2, 0xC4) => Some(1), // accelerator
                (2, 0xC5) => Some(5), // brake
                (2, 0xC6) => Some(6), // clutch
                (2, 0xBB) => Some(2), // throttle
                (2, 0xBA) => Some(5), // rudder
                _ => None,
            }
        } else if cfg!(target_os = "linux") {
            match lo {
                0..=5 => Some(lo as usize),
                6 | 9 => Some(6),  // ABS_THROTTLE, ABS_GAS
                7 | 10 => Some(7), // ABS_RUDDER, ABS_BRAKE
                _ => None,
            }
        } else {
            (lo < 8).then_some(lo as usize)
        }
    };
    let mut sorted = axes.to_vec();
    sorted.sort_by_key(|(c, _)| *c);
    let mut used = [false; 8];
    let mut out: Vec<(usize, f32)> = Vec::new();
    let mut rest = Vec::new();
    for (c, v) in sorted {
        // (a second slider of the same usage is DirectInput's slider 1)
        let k = known(c).map(|k| {
            if used[k] && k == 6 && !used[7] && cfg!(target_os = "macos") && c == 0x10036 {
                7
            } else {
                k
            }
        });
        match k.filter(|k| !used[*k]) {
            Some(k) => {
                used[k] = true;
                out.push((k, v));
            }
            None => rest.push(v),
        }
    }
    for v in rest {
        if let Some(k) = used.iter().position(|u| !u) {
            used[k] = true;
            out.push((k, v));
        }
    }
    out
}

/// Triggers run -1 released to 1 pressed, like a pedal.
pub(super) fn gamepad_axes(pad: &gilrs::Gamepad) -> Vec<(usize, f32)> {
    let trigger =
        |b: gilrs::Button| pad.button_data(b).map(|d| d.value()).unwrap_or(0.0) * 2.0 - 1.0;
    vec![
        (0, pad.value(Axis::LeftStickX)),
        (1, pad.value(Axis::LeftStickY)),
        (2, trigger(gilrs::Button::LeftTrigger2)),
        (3, pad.value(Axis::RightStickX)),
        (4, pad.value(Axis::RightStickY)),
        (5, trigger(gilrs::Button::RightTrigger2)),
    ]
}
