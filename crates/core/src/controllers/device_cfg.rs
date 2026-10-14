use super::*;

/// What one axis of a device does.
#[derive(Debug, Clone, Copy, PartialEq)]
pub(crate) enum Func {
    Steering,
    Throttle,
    Brake,
    Clutch,
    ThrottleBrake,
    /// The driver's head turned left and right, up and down (#454; neoOMSI's own: OMSI's
    /// file has the five above, numbered 0 to 4).
    LookX,
    LookY,
}

impl Func {
    /// The file's number of the function (-1 none).
    pub(crate) fn code(f: Option<Func>) -> i32 {
        match f {
            None => -1,
            Some(Func::Steering) => 0,
            Some(Func::Throttle) => 1,
            Some(Func::Brake) => 2,
            Some(Func::Clutch) => 3,
            Some(Func::ThrottleBrake) => 4,
            Some(Func::LookX) => 5,
            Some(Func::LookY) => 6,
        }
    }

    pub(crate) fn from_code(c: i32) -> Option<Func> {
        match c {
            0 => Some(Func::Steering),
            1 => Some(Func::Throttle),
            2 => Some(Func::Brake),
            3 => Some(Func::Clutch),
            4 => Some(Func::ThrottleBrake),
            5 => Some(Func::LookX),
            6 => Some(Func::LookY),
            _ => None,
        }
    }
}

#[derive(Debug, Clone, PartialEq)]
pub(crate) struct DeviceCfg {
    pub(crate) name: String,
    /// A device that is off is not read at all.
    pub(crate) enabled: bool,
    /// The line after the name (kept as the file has it).
    pub(crate) second: String,
    /// Per DirectInput axis: the function and whether it runs the other way.
    pub(crate) axes: [Option<(Func, bool)>; 8],
    pub(crate) axis_flags: [i32; 8],
    /// Per button: the key action (empty: none) and the number after it.
    pub(crate) buttons: Vec<(String, String)>,
    /// `[FFScale]`: steering forces (centering and drag), then vibration strength.
    pub(crate) ff_scale: Option<(f32, f32)>,
    /// Motor polarity for this device; None uses the existing global setting.
    pub(crate) ff_invert: Option<bool>,
    pub(crate) calibration: [Option<AxisCal>; 8],
    pub(crate) deadzone: Option<f32>,
    #[cfg(target_os = "linux")]
    pub(crate) axis_mode: crate::evdev_axes::AxisMode,
}

impl Default for DeviceCfg {
    fn default() -> Self {
        DeviceCfg {
            name: String::new(),
            enabled: true,
            second: String::new(),
            axes: [None; 8],
            axis_flags: [0; 8],
            buttons: Vec::new(),
            ff_scale: None,
            ff_invert: None,
            calibration: [None; 8],
            deadzone: None,
            #[cfg(target_os = "linux")]
            axis_mode: Default::default(),
        }
    }
}

pub(crate) fn global_deadzone() -> f32 {
    ::config::get_float("controller", "deadzone")
        .filter(|v| v.is_finite())
        .map_or(0.0, |v| (v as f32).clamp(0.0, 0.3))
}

impl DeviceCfg {
    #[cfg(target_os = "linux")]
    pub(crate) fn axis_reversed(&self, k: usize) -> bool {
        self.axes[k].map_or(self.axis_flags[k] & 1 != 0, |(_, reversed)| reversed)
    }

    #[cfg(target_os = "linux")]
    pub(crate) fn set_axis_reversed(&mut self, k: usize, reversed: bool) {
        self.axis_flags[k] = (self.axis_flags[k] & !1) | i32::from(reversed);
        if let Some((_, value)) = self.axes[k].as_mut() {
            *value = reversed;
        }
    }

    #[cfg(target_os = "linux")]
    pub(crate) fn set_axis_function(&mut self, k: usize, function: Option<Func>) {
        let reversed = self.axis_reversed(k);
        self.axes[k] = function.map(|function| (function, reversed));
        self.set_axis_reversed(k, reversed);
    }

    #[cfg(target_os = "linux")]
    pub(crate) fn axis_preview(&self, k: usize, value: f32) -> f32 {
        let value = self.calibrated(k, value);
        if self.axis_reversed(k) { -value } else { value }
    }

    pub(crate) fn calibrated(&self, k: usize, v: f32) -> f32 {
        self.calibration
            .get(k)
            .copied()
            .flatten()
            .map_or(v, |c| c.apply(v))
    }

    pub(crate) fn deadzone(&self, k: usize) -> f32 {
        self.calibration
            .get(k)
            .copied()
            .flatten()
            .and_then(|c| c.deadzone)
            .or(self.deadzone)
            .unwrap_or_else(global_deadzone)
            .clamp(0.0, 0.3)
    }

    pub(crate) fn stick_deadzone(&self, k: usize) -> f32 {
        let own = self.deadzone.is_some()
            || self
            .calibration
            .get(k)
            .copied()
            .flatten()
            .is_some_and(|c| c.deadzone.is_some());
        if own {
            self.deadzone(k)
        } else {
            default_stick_deadzone()
        }
    }
}

/// A `centre` near either end (a pedal at rest) is ignored, so a pedal maps linearly.
#[derive(Debug, Clone, Copy, PartialEq)]
pub(crate) struct AxisCal {
    pub(crate) min: f32,
    pub(crate) centre: Option<f32>,
    pub(crate) max: f32,
    pub(crate) deadzone: Option<f32>,
}

impl AxisCal {
    /// Less travel than this means the axis wasn't moved during calibration.
    pub(crate) const MIN_SPAN: f32 = 0.2;

    pub(crate) fn apply(&self, v: f32) -> f32 {
        let (lo, hi) = (self.min.min(self.max), self.min.max(self.max));
        if hi - lo < Self::MIN_SPAN {
            return v;
        }
        let out = match self
            .centre
            .filter(|c| *c > lo + (hi - lo) * 0.1 && *c < hi - (hi - lo) * 0.1)
        {
            Some(c) if v < c => (v - c) / (c - lo),
            Some(c) => (v - c) / (hi - c),
            None => (v - lo) / (hi - lo) * 2.0 - 1.0,
        };
        out.clamp(-1.0, 1.0)
    }

    pub(super) fn line(&self) -> String {
        let opt = |v: Option<f32>| v.map_or("-".to_string(), |v| format!("{v:.4}"));
        format!(
            "{:.4} {} {:.4} {}",
            self.min,
            opt(self.centre),
            self.max,
            opt(self.deadzone)
        )
    }

    pub(super) fn parse(line: &str) -> Option<AxisCal> {
        let mut it = line.split_whitespace();
        let num = |s: Option<&str>| -> Option<Option<f32>> {
            match s? {
                "-" => Some(None),
                s => s
                    .parse::<f32>()
                    .ok()
                    .filter(|v| v.is_finite())
                    .map(|v| Some(v.clamp(-1.0, 1.0))),
            }
        };
        let min = num(it.next())??;
        let centre = num(it.next())?;
        let max = num(it.next())??;
        let deadzone = num(it.next()).flatten().map(|d| d.clamp(0.0, 0.3));
        Some(AxisCal {
            min,
            centre,
            max,
            deadzone,
        })
    }
}

/// OMSI stores DirectInput's product name; the system's may differ in spacing and case.
pub(super) fn normalized_device_name(s: &str) -> String {
    s.to_lowercase()
        .chars()
        .filter(|c| c.is_alphanumeric())
        .collect()
}

pub(crate) fn names_match(a: &str, b: &str) -> bool {
    // (letters of any script: a name of Cyrillic or Chinese letters only was empty here and
    // matched nothing)
    let (a, b) = (normalized_device_name(a), normalized_device_name(b));
    !a.is_empty() && (a == b || a.contains(&b) || b.contains(&a))
}

/// An exact device name wins over a shorter alias elsewhere in the same OMSI file.
pub(super) fn find_device_cfg<'a>(cfg: &'a [DeviceCfg], name: &str) -> Option<&'a DeviceCfg> {
    let exact = normalized_device_name(name);
    cfg.iter()
        .find(|d| !exact.is_empty() && normalized_device_name(&d.name) == exact)
        .or_else(|| cfg.iter().find(|d| names_match(&d.name, name)))
}
