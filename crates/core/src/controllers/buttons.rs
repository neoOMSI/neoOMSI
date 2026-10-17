use super::*;
use std::sync::Mutex;

pub(super) static PRESSED: Mutex<Vec<(String, usize)>> = Mutex::new(Vec::new());

pub(crate) fn is_pressed(device: &str, n: usize) -> bool {
    PRESSED
        .lock()
        .map(|p| p.iter().any(|(d, b)| *b == n && names_match(d, device)))
        .unwrap_or(false)
}

/// The button's number on its device as DirectInput counts them (and `gamectrler.cfg` with
/// it), from the system's code for it: the HID button usage on macOS (page 9, from 1), the
/// evdev key code on Linux (BTN_JOYSTICK.. and BTN_TRIGGER_HAPPY.. for a wheel's or
/// joystick's buttons, BTN_GAMEPAD.. for a pad's), the button index on Windows. It used to be
/// the place among the buttons pressed so far - the first button ever pressed was "button 1"
/// whichever it was. None on Windows for a code that is no button (gilrs turns the analog
/// triggers, axis codes, into button events too: they are pedals, not numbered buttons).
pub(crate) fn button_number(pad: &gilrs::Gamepad, code: gilrs::ev::Code) -> Option<usize> {
    #[cfg(target_os = "linux")]
    if let Some(n) = declared_button_index(pad.name(), code.into_u32()) {
        return Some(n);
    }
    if cfg!(windows) {
        return code_button(code.into_u32());
    }
    Some(code_button(code.into_u32()).unwrap_or_else(|| {
        let mut codes: Vec<u32> = pad.state().buttons().map(|(c, _)| c.into_u32()).collect();
        codes.sort_unstable();
        codes
            .iter()
            .position(|c| *c == code.into_u32())
            .unwrap_or(usize::MAX)
    }))
}

#[cfg(target_os = "linux")]
pub(super) fn with_declared<R>(name: &str, f: impl FnOnce(&[u32]) -> R) -> Option<R> {
    static DECLARED: std::sync::Mutex<Vec<(String, Option<Vec<u32>>)>> =
        std::sync::Mutex::new(Vec::new());
    let mut cache = DECLARED.lock().unwrap_or_else(|e| e.into_inner());
    if !cache.iter().any(|(n, _)| n == name) {
        cache.push((name.to_string(), declared_buttons(name)));
    }
    cache.iter().find(|(n, _)| n == name)?.1.as_deref().map(f)
}

#[cfg(target_os = "linux")]
pub(super) fn declared_button_index(name: &str, code: u32) -> Option<usize> {
    with_declared(name, |codes| button_index(codes, code & 0xFFFF)).flatten()
}

#[cfg(target_os = "linux")]
pub(super) fn declared_button_count(name: &str) -> usize {
    with_declared(name, button_count).unwrap_or(0)
}

#[cfg(any(target_os = "linux", test))]
pub(super) fn button_count(declared: &[u32]) -> usize {
    declared
        .iter()
        .filter_map(|c| button_index(declared, *c).or_else(|| code_button(*c)))
        .map(|n| n + 1)
        .max()
        .unwrap_or(0)
        .min(128)
}

#[cfg(target_os = "linux")]
pub(super) fn declared_buttons(name: &str) -> Option<Vec<u32>> {
    let mut nodes: Vec<std::path::PathBuf> = std::fs::read_dir("/sys/class/input")
        .ok()?
        .filter_map(|e| e.ok())
        .map(|e| e.path())
        .filter(|p| {
            p.file_name()
                .and_then(|n| n.to_str())
                .is_some_and(|n| n.starts_with("event"))
        })
        .collect();
    nodes.sort();
    let named: Vec<(String, Vec<u32>)> = nodes
        .iter()
        .filter_map(|p| {
            let dev_name = std::fs::read_to_string(p.join("device/name"))
                .ok()?
                .trim()
                .to_string();
            let bitmap = std::fs::read_to_string(p.join("device/capabilities/key")).ok()?;
            let codes = key_bitmap_buttons(&bitmap);
            (names_match(&dev_name, name) && !codes.is_empty()).then_some((dev_name, codes))
        })
        .collect();
    named
        .iter()
        .find(|(n, _)| n == name)
        .or(named.first())
        .map(|(_, c)| c.clone())
}

#[cfg(any(target_os = "linux", test))]
pub(super) fn key_bitmap_buttons(bitmap: &str) -> Vec<u32> {
    let words: Vec<u64> = bitmap
        .split_whitespace()
        .rev()
        .filter_map(|w| u64::from_str_radix(w, 16).ok())
        .collect();
    (0x100..words.len() as u32 * 64)
        .filter(|b| words[*b as usize / 64] >> (b % 64) & 1 != 0)
        .collect()
}

#[cfg(any(target_os = "linux", test))]
pub(super) fn button_index(declared: &[u32], code: u32) -> Option<usize> {
    if declared.iter().all(|c| code_button(*c).is_some()) {
        return None;
    }
    declared.iter().position(|c| *c == code)
}

pub(super) fn code_button(code: u32) -> Option<usize> {
    let (hi, lo) = (code >> 16, (code & 0xFFFF) as usize);
    if cfg!(target_os = "macos") {
        (hi == 9 && lo >= 1).then(|| lo - 1)
    } else if cfg!(target_os = "linux") || cfg!(target_os = "android") {
        match lo {
            0x120..=0x12f => Some(lo - 0x120),
            0x2c0..=0x2e7 => Some(16 + lo - 0x2c0),
            0x130..=0x13e => Some(lo - 0x130),
            0x100..=0x109 => Some(lo - 0x100),
            _ => None,
        }
    } else if cfg!(windows) {
        // gilrs' D-pad (Button codes u32::MAX-3.. up, right, down, left) is the pad's hat,
        // numbered as DirectInput's first hat
        if code >= u32::MAX - 3 {
            return Some(HAT_BUTTONS + (code - (u32::MAX - 3)) as usize);
        }
        (hi == 0).then_some(lo)
    } else {
        None
    }
}
