use gilrs::{GamepadId, Gilrs, LinuxGamepadExt};
use std::fs::File;
use std::io;
use std::os::fd::AsRawFd;
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

const EV_KEY: u32 = 1;
const EV_ABS: u32 = 3;
const EV_FF: u32 = 0x15;
const FF_CONSTANT: usize = 0x52;
const BTN_JOYSTICK: usize = 0x120;
const BTN_GAMEPAD: usize = 0x130;
const BTN_GEAR_DOWN: usize = 0x150;
const BTN_GEAR_UP: usize = 0x151;
const ABS_X: u32 = 0;
const ABS_Y: u32 = 1;
const ABS_Z: u32 = 2;
const ABS_RX: u32 = 3;
const ABS_RY: u32 = 4;
const ABS_RZ: u32 = 5;
const ABS_WHEEL: u32 = 8;
const ABS_GAS: u32 = 9;
const ABS_BRAKE: u32 = 10;
const RETRY_DELAY: Duration = Duration::from_millis(500);
const MAX_ATTEMPTS: u8 = 3;

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub(crate) enum AxisMode {
    #[default]
    Auto,
    Gamepad,
    Native,
}

impl AxisMode {
    pub(crate) fn from_str(value: &str) -> Self {
        match value {
            "gamepad" => Self::Gamepad,
            "native" => Self::Native,
            _ => Self::Auto,
        }
    }

    pub(crate) fn as_str(self) -> &'static str {
        match self {
            Self::Auto => "auto",
            Self::Gamepad => "gamepad",
            Self::Native => "native",
        }
    }

    pub(crate) fn index(self) -> usize {
        match self {
            Self::Auto => 0,
            Self::Gamepad => 1,
            Self::Native => 2,
        }
    }

    fn native(self, automatic: bool) -> bool {
        match self {
            Self::Auto => automatic,
            Self::Gamepad => false,
            Self::Native => true,
        }
    }
}

#[repr(C)]
#[derive(Default)]
struct AbsInfo {
    value: i32,
    minimum: i32,
    maximum: i32,
    fuzz: i32,
    flat: i32,
    resolution: i32,
}

impl AbsInfo {
    fn normalized(&self) -> f32 {
        let span = i64::from(self.maximum) - i64::from(self.minimum);
        if span <= 0 {
            return 0.0;
        }
        let offset = i64::from(self.value) - i64::from(self.minimum);
        (offset as f64 / span as f64 * 2.0 - 1.0).clamp(-1.0, 1.0) as f32
    }
}

const fn eviocgabs(code: u32) -> libc::c_ulong {
    libc::_IOR::<AbsInfo>(b'E' as u32, 0x40 + code) as _
}

const fn eviocgbit<const N: usize>(kind: u32) -> libc::c_ulong {
    libc::_IOR::<[u8; N]>(b'E' as u32, 0x20 + kind) as _
}

fn read_axis(file: &File, code: u32) -> io::Result<AbsInfo> {
    let mut info = AbsInfo::default();
    let result = unsafe { libc::ioctl(file.as_raw_fd(), eviocgabs(code) as _, &mut info) };
    if result < 0 {
        Err(io::Error::last_os_error())
    } else {
        Ok(info)
    }
}

fn capabilities<const N: usize>(file: &File, kind: u32) -> io::Result<[u8; N]> {
    let mut bits = [0; N];
    let result = unsafe {
        libc::ioctl(
            file.as_raw_fd(),
            eviocgbit::<N>(kind) as _,
            bits.as_mut_ptr(),
        )
    };
    if result < 0 {
        Err(io::Error::last_os_error())
    } else {
        Ok(bits)
    }
}

fn has_bit(bits: &[u8], bit: usize) -> bool {
    bits.get(bit / 8)
        .is_some_and(|byte| byte & (1 << (bit % 8)) != 0)
}

fn axis_codes(bits: &[u8]) -> impl Iterator<Item = u32> + '_ {
    (0..=10).filter(|code| has_bit(bits, *code as usize))
}

fn automatic_native(name: &str, mapped: bool, absolute: &[u8], keys: &[u8]) -> bool {
    let abs = |code| has_bit(absolute, code as usize);
    if axis_codes(absolute).next().is_none() {
        return false;
    }
    if !mapped {
        return true;
    }
    let gamepad = has_bit(keys, BTN_GAMEPAD);
    let dual_stick = gamepad && [ABS_X, ABS_Y, ABS_RX, ABS_RY].into_iter().all(abs);
    if has_bit(keys, BTN_GEAR_DOWN)
        || has_bit(keys, BTN_GEAR_UP)
        || abs(ABS_WHEEL)
        || (!dual_stick && (abs(ABS_GAS) || abs(ABS_BRAKE)))
        || (has_bit(keys, BTN_JOYSTICK) && !gamepad)
    {
        return true;
    }
    let name = name.to_ascii_lowercase();
    let wheel_name = name.contains("racing wheel")
        || name.contains("steering wheel")
        || name.contains("driving force")
        || name.contains("thrustmaster t128")
        || name.contains("thrustmaster t248");
    let pedal_name = name.contains("pedal");
    (wheel_name
        && abs(ABS_X)
        && [ABS_Y, ABS_Z, ABS_RZ, ABS_GAS, ABS_BRAKE]
            .into_iter()
            .any(abs))
        || (pedal_name
            && [ABS_Y, ABS_Z, ABS_RZ, ABS_GAS, ABS_BRAKE]
                .into_iter()
                .any(abs))
}

fn gamepad_controls(name: &str, absolute: &[u8], keys: &[u8]) -> bool {
    has_bit(keys, BTN_GAMEPAD)
        && [ABS_X, ABS_Y]
            .into_iter()
            .all(|code| has_bit(absolute, code as usize))
        && !automatic_native(name, true, absolute, keys)
}

struct Attempts {
    remaining: u8,
    next: Option<Instant>,
}

impl Default for Attempts {
    fn default() -> Self {
        Self {
            remaining: MAX_ATTEMPTS,
            next: None,
        }
    }
}

impl Attempts {
    fn due(&self, now: Instant) -> bool {
        self.remaining > 0 && self.next.is_none_or(|at| now >= at)
    }

    fn failed(&mut self, now: Instant, error: &io::Error) {
        let temporary = matches!(
            error.raw_os_error(),
            Some(
                libc::EACCES
                    | libc::EPERM
                    | libc::EINTR
                    | libc::EAGAIN
                    | libc::EBUSY
                    | libc::EIO
                    | libc::ENOENT
            )
        );
        self.remaining = if temporary {
            self.remaining.saturating_sub(1)
        } else {
            0
        };
        self.next = Some(now + RETRY_DELAY);
    }
}

#[derive(Default)]
struct Cache<T> {
    value: Option<T>,
    attempts: Attempts,
}

impl<T> Cache<T> {
    fn probe(&mut self, now: Instant, open: impl FnOnce() -> io::Result<T>) -> bool {
        if self.value.is_some() || !self.attempts.due(now) {
            return false;
        }
        match open() {
            Ok(value) => {
                self.value = Some(value);
                true
            }
            Err(error) => {
                self.attempts.failed(now, &error);
                false
            }
        }
    }
}

struct Reader {
    file: File,
    absolute: [u8; 8],
    keys: [u8; 96],
    ff_constant: Cache<bool>,
    info: Vec<(u32, AbsInfo)>,
    values: Vec<(u32, f32)>,
    attempts: Attempts,
}

impl Reader {
    fn open(path: &Path) -> io::Result<Self> {
        let file = File::open(path)?;
        let events: [u8; 4] = capabilities(&file, 0)?;
        let absolute = if has_bit(&events, EV_ABS as usize) {
            capabilities(&file, EV_ABS)?
        } else {
            [0; 8]
        };
        let keys = if has_bit(&events, EV_KEY as usize) {
            capabilities(&file, EV_KEY)?
        } else {
            [0; 96]
        };
        let ff_constant = Cache {
            value: (!has_bit(&events, EV_FF as usize)).then_some(false),
            attempts: Attempts::default(),
        };
        let info: Vec<_> = axis_codes(&absolute)
            .map(|code| read_axis(&file, code).map(|info| (code, info)))
            .collect::<io::Result<_>>()?;
        let values = info
            .iter()
            .map(|(code, info)| (0x3_0000 | code, info.normalized()))
            .collect();
        Ok(Self {
            file,
            absolute,
            keys,
            ff_constant,
            info,
            values,
            attempts: Attempts::default(),
        })
    }

    fn poll(&mut self, now: Instant) {
        refresh_axes(
            &mut self.info,
            &mut self.values,
            &mut self.attempts,
            now,
            |code| read_axis(&self.file, code),
        );
    }
}

fn refresh_axes(
    info: &mut [(u32, AbsInfo)],
    values: &mut Vec<(u32, f32)>,
    attempts: &mut Attempts,
    now: Instant,
    mut read: impl FnMut(u32) -> io::Result<AbsInfo>,
) {
    if !attempts.due(now) {
        return;
    }
    values.clear();
    for (code, info) in info.iter_mut() {
        match read(*code) {
            Ok(current) => *info = current,
            Err(error) => {
                attempts.failed(now, &error);
                return;
            }
        }
    }
    values.extend(
        info.iter()
            .map(|(code, info)| (0x3_0000 | code, info.normalized())),
    );
    *attempts = Attempts::default();
}

struct Entry {
    name: String,
    os_name: String,
    path: PathBuf,
    mapped: bool,
    mode: AxisMode,
    automatic: bool,
    gamepad: bool,
    reader: Cache<Reader>,
}

impl Entry {
    fn poll(&mut self, now: Instant) {
        let opened = self.reader.probe(now, || Reader::open(&self.path));
        if opened {
            let reader = self.reader.value.as_ref().unwrap();
            self.automatic =
                automatic_native(&self.os_name, self.mapped, &reader.absolute, &reader.keys);
            self.gamepad = gamepad_controls(&self.os_name, &reader.absolute, &reader.keys);
        }
        if let Some(reader) = self.reader.value.as_mut() {
            reader.ff_constant.probe(now, || {
                capabilities::<16>(&reader.file, EV_FF).map(|bits| has_bit(&bits, FF_CONSTANT))
            });
        }
        if self.mode.native(self.automatic) && !opened {
            if let Some(reader) = self.reader.value.as_mut() {
                reader.poll(now);
            }
        }
    }

    fn is_gamepad(&self) -> bool {
        match self.mode {
            AxisMode::Auto => self.mapped && !self.automatic,
            AxisMode::Gamepad => true,
            AxisMode::Native => self.gamepad,
        }
    }
}

pub(crate) struct Devices {
    devices: Vec<(GamepadId, Entry)>,
}

impl Devices {
    pub(crate) fn new() -> Self {
        Self {
            devices: Vec::new(),
        }
    }

    pub(crate) fn forget(&mut self, id: GamepadId) {
        self.devices.retain(|(known, _)| *known != id);
    }

    pub(crate) fn poll(
        &mut self,
        gilrs: Option<&Gilrs>,
        mut mode_for: impl FnMut(&str) -> AxisMode,
    ) {
        let Some(gilrs) = gilrs else {
            self.devices.clear();
            return;
        };
        self.devices.retain(|(id, entry)| {
            let pad = gilrs.gamepad(*id);
            pad.is_connected() && entry.path == pad.devpath()
        });
        let now = Instant::now();
        for (id, pad) in gilrs.gamepads() {
            if !self.devices.iter().any(|(known, _)| *known == id) {
                self.devices.push((
                    id,
                    Entry {
                        name: pad.name().to_string(),
                        os_name: pad.os_name().to_string(),
                        path: pad.devpath().to_path_buf(),
                        mapped: pad.mapping_source() != gilrs::MappingSource::None,
                        mode: AxisMode::Auto,
                        automatic: false,
                        gamepad: false,
                        reader: Cache {
                            value: None,
                            attempts: Attempts::default(),
                        },
                    },
                ));
            }
        }
        for (_, entry) in &mut self.devices {
            entry.mode = mode_for(&entry.name);
            entry.poll(now);
        }
    }

    pub(crate) fn set_mode(&mut self, name: &str, mode: AxisMode) {
        for (_, entry) in &mut self.devices {
            if crate::controllers::names_match(&entry.name, name) {
                entry.mode = mode;
            }
        }
    }

    fn entry(&self, id: GamepadId) -> Option<&Entry> {
        self.devices
            .iter()
            .find(|(known, _)| *known == id)
            .map(|(_, entry)| entry)
    }

    pub(crate) fn mode(&self, id: GamepadId) -> AxisMode {
        self.entry(id).map_or(AxisMode::Auto, |entry| entry.mode)
    }

    pub(crate) fn is_native(&self, id: GamepadId) -> bool {
        self.entry(id)
            .is_some_and(|entry| entry.mode.native(entry.automatic))
    }

    pub(crate) fn is_gamepad(&self, id: GamepadId) -> bool {
        self.entry(id).is_some_and(Entry::is_gamepad)
    }

    pub(crate) fn ff_capable(&self, id: GamepadId) -> Option<bool> {
        self.entry(id)?.reader.value.as_ref()?.ff_constant.value
    }

    pub(crate) fn axes(&self, id: GamepadId) -> Option<&[(u32, f32)]> {
        self.is_native(id).then_some(())?;
        Some(self.entry(id)?.reader.value.as_ref()?.values.as_slice())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn bits<const N: usize>(codes: &[usize]) -> [u8; N] {
        let mut bits = [0; N];
        for &code in codes {
            bits[code / 8] |= 1 << (code % 8);
        }
        bits
    }

    #[test]
    fn auto_uses_wheel_evidence_and_leaves_unknown_mapped_controllers_alone() {
        let pedals = bits::<8>(&[0, 1, 2, 5]);
        let no_keys = [0; 96];
        assert!(automatic_native(
            "Thrustmaster Thrustmaster Racing Wheel FFB",
            true,
            &pedals,
            &no_keys
        ));
        assert!(automatic_native(
            "Thrustmaster T128",
            true,
            &pedals,
            &no_keys
        ));
        assert!(!automatic_native(
            "Unknown controller",
            true,
            &pedals,
            &no_keys
        ));
        assert!(automatic_native(
            "Unknown controller",
            false,
            &pedals,
            &no_keys
        ));
        assert!(!automatic_native(
            "Thrustmaster T128",
            true,
            &[0; 8],
            &no_keys
        ));
        assert!(automatic_native(
            "Unknown controller",
            true,
            &pedals,
            &bits::<96>(&[BTN_GEAR_DOWN])
        ));
        assert!(automatic_native(
            "Generic pedals",
            true,
            &bits::<8>(&[2, 5]),
            &no_keys
        ));
    }

    #[test]
    fn a_gamepad_keeps_its_classification_when_native_is_forced() {
        let axes = bits::<8>(&[0, 1, 2, 3, 4, 5, ABS_GAS as usize]);
        let keys = bits::<96>(&[BTN_GAMEPAD]);
        let automatic = automatic_native("Generic gamepad", true, &axes, &keys);
        assert!(!automatic);
        assert!(!AxisMode::Auto.native(automatic));
        assert!(AxisMode::Native.native(automatic));
        assert!(!AxisMode::Gamepad.native(true));
    }

    #[test]
    fn native_axes_do_not_determine_the_device_type() {
        let keys = bits::<96>(&[BTN_GAMEPAD]);
        for codes in [vec![0, 1], vec![0, 1, 2, 5], vec![0, 1, 2, 3, 4, 5, 9]] {
            let axes = bits::<8>(&codes);
            assert!(gamepad_controls("Generic gamepad", &axes, &keys));
            let mut entry = Entry {
                name: "Generic gamepad".into(),
                os_name: "Generic gamepad".into(),
                path: PathBuf::new(),
                mapped: true,
                mode: AxisMode::Native,
                automatic: automatic_native("Generic gamepad", true, &axes, &keys),
                gamepad: gamepad_controls("Generic gamepad", &axes, &keys),
                reader: Cache {
                    value: None,
                    attempts: Attempts::default(),
                },
            };
            assert!(entry.mode.native(entry.automatic));
            assert!(entry.is_gamepad());
            entry.os_name = "Unknown mapped wheel".into();
            let wheel_axes = bits::<8>(&[0, 2, 5]);
            entry.gamepad = gamepad_controls(&entry.os_name, &wheel_axes, &[0; 96]);
            entry.automatic = automatic_native(&entry.os_name, true, &wheel_axes, &[0; 96]);
            assert!(!entry.is_gamepad());
            entry.mode = AxisMode::Auto;
            assert!(entry.is_gamepad());
            entry.mode = AxisMode::Gamepad;
            assert!(entry.is_gamepad());
            assert!(!entry.mode.native(entry.automatic));
            entry.automatic = true;
            assert!(entry.is_gamepad());
            assert!(!entry.mode.native(entry.automatic));
        }
    }

    #[test]
    fn wheel_evidence_overrides_gamepad_buttons_and_extra_axes() {
        let axes = bits::<8>(&[0, 1, 2, 3, 4, 5]);
        let keys = bits::<96>(&[BTN_GAMEPAD]);
        assert!(automatic_native("Thrustmaster T128", true, &axes, &keys));
        assert!(!gamepad_controls("Thrustmaster T128", &axes, &keys));
        assert!(!gamepad_controls(
            "Generic controller",
            &axes,
            &bits::<96>(&[BTN_GAMEPAD, BTN_GEAR_UP])
        ));
        assert!(!gamepad_controls(
            "Generic controller",
            &bits::<8>(&[ABS_Z as usize]),
            &keys
        ));
    }

    #[test]
    fn unsupported_capabilities_are_not_retried_until_a_new_connection() {
        let mut cache = Cache::<bool>::default();
        let now = Instant::now();
        let mut calls = 0;
        for seconds in [0, 1, 3, 30, 300] {
            cache.probe(now + Duration::from_secs(seconds), || {
                calls += 1;
                Err(io::Error::from_raw_os_error(libc::ENOTTY))
            });
        }
        assert_eq!(calls, 1);
        let mut reconnected = Cache::<bool>::default();
        assert!(reconnected.probe(now, || {
            calls += 1;
            Ok(true)
        }));
        assert_eq!(calls, 2);
    }

    #[test]
    fn cached_capabilities_are_not_requeried_even_when_unsupported() {
        let mut cache = Cache::<bool>::default();
        let now = Instant::now();
        assert!(cache.probe(now, || Ok(false)));
        for seconds in [0, 1, 3, 30] {
            assert!(!cache.probe(now + Duration::from_secs(seconds), || panic!(
                "capability queried again"
            )));
        }
        assert_eq!(cache.value, Some(false));
    }

    #[test]
    fn transient_access_errors_have_delayed_and_bounded_retries() {
        let mut cache = Cache::<bool>::default();
        let now = Instant::now();
        let mut calls = 0;
        for millis in [0, 1, 499, 500, 501, 999, 1000, 1500, 30000] {
            cache.probe(now + Duration::from_millis(millis), || {
                calls += 1;
                Err(io::Error::from_raw_os_error(libc::EACCES))
            });
        }
        assert_eq!(calls, MAX_ATTEMPTS);
        assert!(AxisMode::Native.native(false));
        let mut cache = Cache::<bool>::default();
        cache.probe(now, || Err(io::Error::from_raw_os_error(libc::EACCES)));
        assert!(cache.probe(now + RETRY_DELAY, || Ok(true)));
        assert_eq!(cache.value, Some(true));
    }

    #[test]
    fn snapshots_update_unmoved_pedals_and_driver_ranges_without_capability_queries() {
        let mut info = vec![(2, AbsInfo::default()), (5, AbsInfo::default())];
        let mut values = Vec::new();
        let mut attempts = Attempts::default();
        let now = Instant::now();
        let mut calls = Vec::new();
        for maximum in [65535, 1023] {
            refresh_axes(&mut info, &mut values, &mut attempts, now, |code| {
                calls.push(code);
                Ok(AbsInfo {
                    value: maximum,
                    maximum,
                    ..Default::default()
                })
            });
            assert_eq!(values, [(0x3_0002, 1.0), (0x3_0005, 1.0)]);
            assert_eq!(info[0].1.maximum, maximum);
        }
        assert_eq!(calls, [2, 5, 2, 5]);
    }

    #[test]
    fn failed_snapshots_clear_stale_values_and_retry_without_reprobing_capabilities() {
        let mut info = vec![(2, AbsInfo::default()), (5, AbsInfo::default())];
        let mut values = vec![(0x3_0002, 1.0), (0x3_0005, 1.0)];
        let mut attempts = Attempts::default();
        let now = Instant::now();
        refresh_axes(&mut info, &mut values, &mut attempts, now, |_| {
            Err(io::Error::from_raw_os_error(libc::EINTR))
        });
        assert!(values.is_empty());
        refresh_axes(&mut info, &mut values, &mut attempts, now, |_| {
            panic!("retry too early")
        });
        refresh_axes(
            &mut info,
            &mut values,
            &mut attempts,
            now + RETRY_DELAY,
            |_| {
                Ok(AbsInfo {
                    value: 1023,
                    maximum: 1023,
                    ..Default::default()
                })
            },
        );
        assert_eq!(values, [(0x3_0002, 1.0), (0x3_0005, 1.0)]);
    }

    #[test]
    fn wheel_capabilities_include_unmoved_z_but_exclude_absent_axes_hats_and_misc() {
        let capabilities = [0x27, 0, 0x03, 0, 0, 0x01, 0, 0];
        assert_eq!(
            super::axis_codes(&capabilities).collect::<Vec<_>>(),
            [0, 1, 2, 5]
        );
        assert!(super::axis_codes(&[0; 8]).next().is_none());
    }

    #[test]
    fn unsigned_wheel_and_pedal_ranges_keep_their_native_direction() {
        let value = |value| {
            AbsInfo {
                value,
                minimum: 0,
                maximum: 65535,
                ..Default::default()
            }
            .normalized()
        };
        assert_eq!(value(0), -1.0);
        assert!(value(32768).abs() < 0.0001);
        assert_eq!(value(65535), 1.0);
    }

    #[test]
    fn signed_and_short_pedal_ranges_use_the_reported_limits() {
        for (minimum, maximum) in [(-32768, 32767), (0, 1023), (10, 110)] {
            for (value, expected) in [(minimum, -1.0), (maximum, 1.0)] {
                assert_eq!(
                    AbsInfo {
                        value,
                        minimum,
                        maximum,
                        ..Default::default()
                    }
                    .normalized(),
                    expected
                );
            }
        }
    }

    #[test]
    fn invalid_ranges_and_out_of_range_values_are_bounded() {
        assert_eq!(AbsInfo::default().normalized(), 0.0);
        assert_eq!(
            AbsInfo {
                value: i32::MAX,
                minimum: i32::MIN,
                maximum: i32::MAX,
                ..Default::default()
            }
            .normalized(),
            1.0
        );
        assert_eq!(
            AbsInfo {
                value: -10,
                minimum: 0,
                maximum: 100,
                ..Default::default()
            }
            .normalized(),
            -1.0
        );
    }

    #[test]
    fn abs_info_matches_the_kernel_layout() {
        assert_eq!(std::mem::size_of::<AbsInfo>(), 24);
    }
}
