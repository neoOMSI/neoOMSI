use std::path::Path;

mod analog;
mod buttons;
mod device_cfg;
mod devices;
mod feedback;
mod poll;
mod settings;

pub use analog::*;
pub use buttons::*;
pub use device_cfg::*;
pub use devices::*;
pub use feedback::*;
pub use settings::*;

#[cfg(test)]
mod axis_shape_tests;
#[cfg(test)]
mod button_tests;
#[cfg(test)]
mod cfg_tests;
#[cfg(test)]
mod slot_tests;
#[cfg(test)]
mod tests;

pub struct Controllers {
    /// Each wheel's suspension travel, settled over a tenth of a second (see `wheel_bump`).
    settled: Vec<f32>,
    devices: Devices,
    focused: bool,
    cfg: Vec<DeviceCfg>,
    pub enabled: bool,
    /// The pedals' response curves (Settings → pedal strength; 1 = as the pedal reads).
    pub pedal_throttle: f32,
    pub pedal_brake: f32,
    pub sources: [Option<String>; 4],
    pub centre: bool,
    /// Force feedback the other way round (`[controller] ff_invert`).
    pub ff_invert: bool,
    /// Force feedback and rumble switched on (`[controller] ff_enabled`).
    pub ff_enabled: bool,
    /// The wheel's rotation over the rotation that is the bus's full lock (Settings:
    /// `wheel_range` / `wheel_lock`; 1 = the whole wheel is the full lock, as OMSI).
    pub steer_gain: f32,
    /// Key actions of buttons pressed (true) and released (false) since the last poll.
    pub actions: Vec<(String, bool)>,
    /// Devices told about in the log (and on the screen) as not set up.
    announced: Vec<String>,
    /// A message for the screen: a wheel that is not set up.
    pub notice: Option<String>,
    /// The steering device: its name, physical position (-1..1, before dead zone
    /// and steering gain) now and previously, and whether it pushes back.
    steer: Option<(String, f32, f32, bool)>,
    ff_t: f32,
    ff_lateral: f32,
    ff_bump: f32,
    ff_bump_age: f32,
    ff_source_logged: Option<String>,
    /// The rumble playing (`FF_Vib_Amp` and `FF_Vib_Period` of the bus), rebuilt when
    /// either changes.
    rumble: Option<(gilrs::ff::Effect, f32, f32)>,
    #[cfg(all(target_os = "linux", target_pointer_width = "64"))]
    wheel: Option<crate::evdev_ff::Wheel>,
    #[cfg(all(target_os = "linux", target_pointer_width = "64"))]
    wheel_tried: Option<(String, std::time::Instant)>,
}

impl Controllers {
    pub(crate) fn connected_devices(&self) -> Vec<Connected> {
        self.devices.connected()
    }

    pub(crate) fn reload_cfg(&mut self) {
        self.cfg = read_cfg();
        #[cfg(target_os = "linux")]
        for c in self.devices.connected() {
            self.devices
                .set_axis_mode(&c.name, saved_axis_mode(&c.name));
        }
    }

    pub(crate) fn refresh_devices(&self) {
        self.devices.refresh();
    }

    pub(crate) fn set_focus(&mut self, focused: bool) {
        if self.focused == focused {
            return;
        }
        self.focused = focused;
        self.devices.set_focus(focused);
        if !focused {
            self.steer = None;
            self.actions.clear();
            self.ff_source_logged = None;
        }
    }

    pub fn new(_root: &Path, hwnd: Option<isize>) -> Controllers {
        let devices = Devices::new(hwnd, true);
        let cfg = read_cfg();
        for c in devices.connected() {
            log::info!(
                "game controller: {} ({})",
                c.name,
                if cfg.iter().any(|d| names_match(&d.name, &c.name)) {
                    "set up in the settings"
                } else if c.gamepad {
                    "as a gamepad"
                } else {
                    "not set up: its X axis steers"
                }
            );
        }
        Controllers {
            settled: Vec::new(),
            devices,
            focused: true,
            cfg,
            pedal_throttle: 1.0,
            pedal_brake: 1.0,
            sources: Default::default(),
            centre: true,
            ff_invert: false,
            ff_enabled: true,
            steer_gain: 1.0,
            enabled: true,
            actions: Vec::new(),
            announced: Vec::new(),
            notice: None,
            steer: None,
            ff_t: 0.0,
            ff_lateral: 0.0,
            ff_bump: 0.0,
            ff_bump_age: 0.0,
            ff_source_logged: None,
            rumble: None,
            #[cfg(all(target_os = "linux", target_pointer_width = "64"))]
            wheel: None,
            #[cfg(all(target_os = "linux", target_pointer_width = "64"))]
            wheel_tried: None,
        }
    }

    fn off(&self, name: &str) -> bool {
        find_device_cfg(&self.cfg, name).is_some_and(|d| !d.enabled)
    }

    /// Any controller there at all.
    pub fn any(&self) -> bool {
        !self.devices.connected().is_empty()
    }
}
