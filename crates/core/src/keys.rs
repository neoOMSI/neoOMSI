//! DirectInput scan codes (as used by `Inputs/keyboard.cfg`) ↔ winit key codes.

use winit::keyboard::KeyCode;

/// DIK_* scan code for a winit key, if known.
pub fn dik_code(key: KeyCode) -> Option<i32> {
    use KeyCode::*;
    Some(match key {
        Escape => 1,
        Digit1 => 2,
        Digit2 => 3,
        Digit3 => 4,
        Digit4 => 5,
        Digit5 => 6,
        Digit6 => 7,
        Digit7 => 8,
        Digit8 => 9,
        Digit9 => 10,
        Digit0 => 11,
        Minus => 12,
        Equal => 13,
        Backspace => 14,
        Tab => 15,
        KeyQ => 16,
        KeyW => 17,
        KeyE => 18,
        KeyR => 19,
        KeyT => 20,
        KeyY => 21,
        KeyU => 22,
        KeyI => 23,
        KeyO => 24,
        KeyP => 25,
        BracketLeft => 26,
        BracketRight => 27,
        Enter => 28,
        ControlLeft => 29,
        KeyA => 30,
        KeyS => 31,
        KeyD => 32,
        KeyF => 33,
        KeyG => 34,
        KeyH => 35,
        KeyJ => 36,
        KeyK => 37,
        KeyL => 38,
        Semicolon => 39,
        Quote => 40,
        Backquote => 41,
        ShiftLeft => 42,
        Backslash => 43,
        KeyZ => 44,
        KeyX => 45,
        KeyC => 46,
        KeyV => 47,
        KeyB => 48,
        KeyN => 49,
        KeyM => 50,
        Comma => 51,
        Period => 52,
        Slash => 53,
        ShiftRight => 54,
        NumpadMultiply => 55,
        AltLeft => 56,
        Space => 57,
        CapsLock => 58,
        F1 => 59,
        F2 => 60,
        F3 => 61,
        F4 => 62,
        F5 => 63,
        F6 => 64,
        F7 => 65,
        F8 => 66,
        F9 => 67,
        F10 => 68,
        NumLock => 69,
        ScrollLock => 70,
        Numpad7 => 71,
        Numpad8 => 72,
        Numpad9 => 73,
        NumpadSubtract => 74,
        Numpad4 => 75,
        Numpad5 => 76,
        Numpad6 => 77,
        NumpadAdd => 78,
        Numpad1 => 79,
        Numpad2 => 80,
        Numpad3 => 81,
        Numpad0 => 82,
        NumpadDecimal => 83,
        IntlBackslash => 86,
        F11 => 87,
        F12 => 88,
        F13 => 100,
        F14 => 101,
        F15 => 102,
        NumpadEqual => 141,
        NumpadEnter => 156,
        ControlRight => 157,
        NumpadDivide => 181,
        PrintScreen => 183,
        AltRight => 184,
        Pause => 197,
        Home => 199,
        ArrowUp => 200,
        PageUp => 201,
        ArrowLeft => 203,
        ArrowRight => 205,
        End => 207,
        ArrowDown => 208,
        PageDown => 209,
        Insert => 210,
        Delete => 211,
        SuperLeft => 219,
        SuperRight => 220,
        ContextMenu => 221,
        _ => return None,
    })
}

/// A readable name for a DIK scan code ("W", "Num8", "Up", "F5"), from the same table.
pub fn scan_name(scan: i32) -> Option<String> {
    use KeyCode::*;
    const ALL: &[KeyCode] = &[
        Escape,
        Digit1,
        Digit2,
        Digit3,
        Digit4,
        Digit5,
        Digit6,
        Digit7,
        Digit8,
        Digit9,
        Digit0,
        Minus,
        Equal,
        Backspace,
        Tab,
        KeyQ,
        KeyW,
        KeyE,
        KeyR,
        KeyT,
        KeyY,
        KeyU,
        KeyI,
        KeyO,
        KeyP,
        BracketLeft,
        BracketRight,
        Enter,
        ControlLeft,
        KeyA,
        KeyS,
        KeyD,
        KeyF,
        KeyG,
        KeyH,
        KeyJ,
        KeyK,
        KeyL,
        Semicolon,
        Quote,
        Backquote,
        ShiftLeft,
        Backslash,
        KeyZ,
        KeyX,
        KeyC,
        KeyV,
        KeyB,
        KeyN,
        KeyM,
        Comma,
        Period,
        Slash,
        ShiftRight,
        NumpadMultiply,
        AltLeft,
        Space,
        CapsLock,
        F1,
        F2,
        F3,
        F4,
        F5,
        F6,
        F7,
        F8,
        F9,
        F10,
        NumLock,
        ScrollLock,
        Numpad7,
        Numpad8,
        Numpad9,
        NumpadSubtract,
        Numpad4,
        Numpad5,
        Numpad6,
        NumpadAdd,
        Numpad1,
        Numpad2,
        Numpad3,
        Numpad0,
        NumpadDecimal,
        IntlBackslash,
        F11,
        F12,
        F13,
        F14,
        F15,
        NumpadEqual,
        NumpadEnter,
        ControlRight,
        NumpadDivide,
        PrintScreen,
        AltRight,
        Pause,
        Home,
        ArrowUp,
        PageUp,
        ArrowLeft,
        ArrowRight,
        End,
        ArrowDown,
        PageDown,
        Insert,
        Delete,
        SuperLeft,
        SuperRight,
        ContextMenu,
    ];
    let k = ALL.iter().find(|k| dik_code(**k) == Some(scan))?;
    let raw = format!("{k:?}");
    let named = match raw.as_str() {
        "NumpadAdd" => "Numpad +",
        "NumpadSubtract" => "Numpad -",
        "NumpadMultiply" => "Numpad *",
        "NumpadDivide" => "Numpad /",
        "NumpadDecimal" => "Numpad .",
        "NumpadEnter" => "Numpad Enter",
        "NumpadEqual" => "Numpad =",
        "NumLock" => "Num Lock",
        "ScrollLock" => "Scroll Lock",
        "CapsLock" => "Caps Lock",
        "PrintScreen" => "Print Screen",
        "PageUp" => "Page Up",
        "PageDown" => "Page Down",
        "ArrowUp" => "Up",
        "ArrowDown" => "Down",
        "ArrowLeft" => "Left",
        "ArrowRight" => "Right",
        "ControlLeft" => "Left Ctrl",
        "ControlRight" => "Right Ctrl",
        "ShiftLeft" => "Left Shift",
        "ShiftRight" => "Right Shift",
        "AltLeft" => "Left Alt",
        "AltRight" => "Right Alt",
        "SuperLeft" => "Left Win",
        "SuperRight" => "Right Win",
        "ContextMenu" => "Menu",
        "Escape" => "Esc",
        "Delete" => "Del",
        "Period" => ".",
        "Comma" => ",",
        "Minus" => "-",
        "Equal" => "=",
        "Slash" => "/",
        "Semicolon" => ";",
        "Quote" => "'",
        "Backquote" => "`",
        "BracketLeft" => "[",
        "BracketRight" => "]",
        "Backslash" => "\\",
        "IntlBackslash" => "<",
        _ => "",
    };
    if !named.is_empty() {
        return Some(named.to_string());
    }
    if let Some(d) = raw.strip_prefix("Numpad") {
        return Some(format!("Numpad {d}"));
    }
    Some(
        raw.strip_prefix("Key")
            .or_else(|| raw.strip_prefix("Digit"))
            .map(String::from)
            .unwrap_or(raw.clone()),
    )
}

#[cfg(test)]
mod scan_name_tests {
    #[test]
    fn names() {
        assert_eq!(super::scan_name(17).as_deref(), Some("W"));
        assert_eq!(super::scan_name(72).as_deref(), Some("Numpad 8"));
        assert_eq!(super::scan_name(200).as_deref(), Some("Up"));
        assert_eq!(super::scan_name(52).as_deref(), Some("."));
    }
}

pub(crate) fn layout_label(key: KeyCode) -> String {
    crate::keyboard_layout::legend(key)
        .and_then(latin_label)
        .or_else(|| typed(key))
        .unwrap_or_else(|| qwerty_label(key))
}

fn qwerty_label(key: KeyCode) -> String {
    dik_code(key)
        .and_then(scan_name)
        .unwrap_or_else(|| format!("{key:?}"))
}

/// The tab keys' labels, left and right. Asked at most twice a second (a switch of the
/// layout shows that soon; the X server is a round trip away).
pub(crate) fn tab_keys() -> [String; 2] {
    static LAST: std::sync::Mutex<Option<(std::time::Instant, [String; 2])>> = std::sync::Mutex::new(None);
    let mut g = LAST.lock().unwrap_or_else(|e| e.into_inner());
    if let Some((t, keys)) = g.as_ref() {
        if t.elapsed() < std::time::Duration::from_millis(500) {
            return keys.clone();
        }
    }
    let keys = [layout_label(KeyCode::KeyQ), layout_label(KeyCode::KeyE)];
    *g = Some((std::time::Instant::now(), keys.clone()));
    keys
}

/// A letter of the Latin script or a printable sign: what a key's legend may be.
pub(crate) fn is_latin(c: char) -> bool {
    c.is_ascii_graphic()
        || (('\u{a1}'..='\u{24f}').contains(&c) && c != '\u{ad}')
        || ('\u{1e00}'..='\u{1eff}').contains(&c)
}

/// A character as a key's label, if it is a Latin one.
fn latin_label(c: char) -> Option<String> {
    is_latin(c).then(|| c.to_uppercase().collect())
}

static TYPED: std::sync::Mutex<Vec<(KeyCode, String)>> = std::sync::Mutex::new(Vec::new());

/// Remembers what a key typed, for [`layout_label`] where the layout cannot be asked
/// (Wayland without X, Android).
pub(crate) fn note_typed(key: KeyCode, text: &str) {
    let mut chars = text.chars();
    let (Some(c), None) = (chars.next(), chars.next()) else {
        return;
    };
    let Some(label) = latin_label(c) else {
        return;
    };
    if let Ok(mut t) = TYPED.lock() {
        match t.iter_mut().find(|e| e.0 == key) {
            Some(e) => e.1 = label,
            None => t.push((key, label)),
        }
    }
}

fn typed(key: KeyCode) -> Option<String> {
    TYPED.lock().ok()?.iter().find(|e| e.0 == key).map(|e| e.1.clone())
}

#[cfg(test)]
mod layout_tests {
    use super::*;

    #[test]
    fn labels_are_the_latin_legend() {
        assert_eq!(latin_label('a').as_deref(), Some("A"));
        assert_eq!(latin_label('ğ').as_deref(), Some("Ğ"));
        assert_eq!(latin_label('\'').as_deref(), Some("'"));
        assert_eq!(latin_label('.').as_deref(), Some("."));
        for other in ['й', 'ε', 'ض', 'ק', 'ๆ', 'ㅂ', '\r', ' ', '\u{ad}'] {
            assert_eq!(latin_label(other), None, "{other:?}");
        }
    }

    #[test]
    fn typed_letters_are_remembered_unless_they_are_no_label() {
        note_typed(KeyCode::KeyZ, "w");
        assert_eq!(typed(KeyCode::KeyZ).as_deref(), Some("W"));
        note_typed(KeyCode::KeyZ, "я");
        note_typed(KeyCode::KeyZ, "ab");
        assert_eq!(typed(KeyCode::KeyZ).as_deref(), Some("W"));
        // (not `layout_label`: macOS answers layout questions on the main thread only)
        assert_eq!(qwerty_label(KeyCode::KeyQ), "Q");
    }
}

/// A key and its modifiers as the player reads them ("Ctrl+D"); `(unbound)` for scan 0.
pub(crate) fn key_name(scan: i64, modifier: i64) -> String {
    if scan == 0 {
        return "(unbound)".into();
    }
    let k = scan_name(scan as i32).unwrap_or_else(|| format!("scan {scan}"));
    let mut mods = Vec::new();
    if modifier & ::content::input::KEY_SHIFT as i64 != 0 {
        mods.push("Shift");
    }
    if modifier & ::content::input::KEY_CTRL as i64 != 0 {
        mods.push("Ctrl");
    }
    if modifier & ::content::input::KEY_ALT as i64 != 0 {
        mods.push("Alt");
    }
    if mods.is_empty() {
        k
    } else {
        format!("{}+{k}", mods.join("+"))
    }
}

static BINDINGS: std::sync::Mutex<Option<(std::time::Instant, std::sync::Arc<serde_json::Value>)>> = std::sync::Mutex::new(None);

pub(crate) fn keybindings() -> Option<std::sync::Arc<serde_json::Value>> {
    let mut g = BINDINGS.lock().ok()?;
    if let Some((t, v)) = g.as_ref() {
        if t.elapsed() < std::time::Duration::from_secs(2) {
            return Some(v.clone());
        }
    }
    let v = std::sync::Arc::new(omsi_launcher_lib::get_keybindings().ok()?);
    *g = Some((std::time::Instant::now(), v.clone()));
    Some(v)
}

pub(crate) fn keybindings_changed() {
    if let Ok(mut g) = BINDINGS.lock() {
        *g = None;
    }
}
