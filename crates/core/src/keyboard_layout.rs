//! What a key types on the player's keyboard layout, asked of the system: the character
//! the key at a physical position gives without modifiers, on a Latin layout where the
//! system has one (see [`crate::keys::layout_label`]). `None` where it cannot be asked.

use winit::keyboard::KeyCode;

/// The key's character on the active layout (Windows: its Latin legend, as `MapVirtualKey`
/// gives it for Cyrillic, Greek, ... layouts too).
#[cfg(windows)]
pub(crate) fn legend(key: KeyCode) -> Option<char> {
    use windows::Win32::UI::Input::KeyboardAndMouse::{
        GetKeyboardLayout, MapVirtualKeyExW, MAPVK_VK_TO_CHAR, MAPVK_VSC_TO_VK_EX,
    };
    // DIK codes below 0x80 are the set-1 scan codes Windows maps
    let scan = crate::keys::dik_code(key).filter(|s| *s < 0x80)? as u32;
    // SAFETY: plain lookups in the layout of the calling (window) thread
    let ch = unsafe {
        let hkl = GetKeyboardLayout(0);
        let vk = MapVirtualKeyExW(scan, MAPVK_VSC_TO_VK_EX, Some(hkl));
        if vk == 0 {
            return None;
        }
        MapVirtualKeyExW(vk, MAPVK_VK_TO_CHAR, Some(hkl))
    };
    // the top bit marks a dead key; its character is still the legend
    char::from_u32(ch & 0x7fff_ffff).filter(|c| *c != '\0')
}

/// The key's character on the current ASCII-capable input source (the Latin layout the
/// system keeps next to a Cyrillic, Greek, ... one), through `UCKeyTranslate`.
#[cfg(target_os = "macos")]
pub(crate) fn legend(key: KeyCode) -> Option<char> {
    use std::ffi::c_void;

    #[link(name = "Carbon", kind = "framework")]
    unsafe extern "C" {
        fn TISCopyCurrentASCIICapableKeyboardLayoutInputSource() -> *mut c_void;
        fn TISGetInputSourceProperty(source: *mut c_void, key: *const c_void) -> *mut c_void;
        static kTISPropertyUnicodeKeyLayoutData: *const c_void;
        fn LMGetKbdType() -> u8;
        fn UCKeyTranslate(
            layout: *const u8,
            key: u16,
            action: u16,
            modifiers: u32,
            kbd_type: u32,
            options: u32,
            dead: *mut u32,
            max: usize,
            len: *mut usize,
            chars: *mut u16,
        ) -> i32;
    }
    #[link(name = "CoreFoundation", kind = "framework")]
    unsafe extern "C" {
        fn CFDataGetBytePtr(data: *const c_void) -> *const u8;
        fn CFRelease(object: *const c_void);
    }
    const ACTION_DISPLAY: u16 = 3;
    const NO_DEAD_KEYS: u32 = 1;

    let code = mac_code(key)?;
    // SAFETY: the input source is released after use; the layout data belongs to it
    unsafe {
        let source = TISCopyCurrentASCIICapableKeyboardLayoutInputSource();
        if source.is_null() {
            return None;
        }
        let data = TISGetInputSourceProperty(source, kTISPropertyUnicodeKeyLayoutData);
        let mut out = [0u16; 4];
        let mut len = 0usize;
        let mut dead = 0u32;
        let ok = !data.is_null()
            && UCKeyTranslate(
                CFDataGetBytePtr(data),
                code,
                ACTION_DISPLAY,
                0,
                LMGetKbdType() as u32,
                NO_DEAD_KEYS,
                &mut dead,
                out.len(),
                &mut len,
                out.as_mut_ptr(),
            ) == 0;
        CFRelease(source);
        if !ok || len == 0 {
            return None;
        }
        char::decode_utf16(out[..len].iter().copied()).next()?.ok()
    }
}

/// The macOS virtual key code (`kVK_ANSI_*`) of a key of the typing block.
#[cfg(any(target_os = "macos", test))]
fn mac_code(key: KeyCode) -> Option<u16> {
    use KeyCode::*;
    Some(match key {
        KeyA => 0x00,
        KeyS => 0x01,
        KeyD => 0x02,
        KeyF => 0x03,
        KeyH => 0x04,
        KeyG => 0x05,
        KeyZ => 0x06,
        KeyX => 0x07,
        KeyC => 0x08,
        KeyV => 0x09,
        IntlBackslash => 0x0A,
        KeyB => 0x0B,
        KeyQ => 0x0C,
        KeyW => 0x0D,
        KeyE => 0x0E,
        KeyR => 0x0F,
        KeyY => 0x10,
        KeyT => 0x11,
        Digit1 => 0x12,
        Digit2 => 0x13,
        Digit3 => 0x14,
        Digit4 => 0x15,
        Digit6 => 0x16,
        Digit5 => 0x17,
        Equal => 0x18,
        Digit9 => 0x19,
        Digit7 => 0x1A,
        Minus => 0x1B,
        Digit8 => 0x1C,
        Digit0 => 0x1D,
        BracketRight => 0x1E,
        KeyO => 0x1F,
        KeyU => 0x20,
        BracketLeft => 0x21,
        KeyI => 0x22,
        KeyP => 0x23,
        KeyL => 0x25,
        KeyJ => 0x26,
        Quote => 0x27,
        KeyK => 0x28,
        Semicolon => 0x29,
        Backslash => 0x2A,
        Comma => 0x2B,
        Slash => 0x2C,
        KeyN => 0x2D,
        KeyM => 0x2E,
        Period => 0x2F,
        Backquote => 0x32,
        _ => return None,
    })
}

/// The key's character in the X server's keymap (X11, and XWayland under Wayland): the
/// active group's, else the first group with a Latin one (a "us,ru" setup on Cyrillic).
#[cfg(all(unix, not(target_os = "macos"), not(target_os = "android")))]
pub(crate) fn legend(key: KeyCode) -> Option<char> {
    // X keycodes are the evdev ones + 8; evdev's match the set-1 scan codes (DIK) up to F12
    let code = crate::keys::dik_code(key).filter(|s| (1..=88).contains(s))? as u8 + 8;
    x11::legend(code)
}

#[cfg(target_os = "android")]
pub(crate) fn legend(_: KeyCode) -> Option<char> {
    None
}

#[cfg(all(unix, not(target_os = "macos"), not(target_os = "android")))]
mod x11 {
    use std::ffi::{c_char, c_int, c_long, c_uint, c_ulong, c_void, CStr};
    use std::sync::{Mutex, OnceLock};

    type OpenDisplay =
        unsafe extern "C" fn(*const c_char, *mut c_int, *mut c_int, *mut c_int, *mut c_int, *mut c_int) -> *mut c_void;
    type GetState = unsafe extern "C" fn(*mut c_void, c_uint, *mut XkbState) -> c_int;
    type KeycodeToKeysym = unsafe extern "C" fn(*mut c_void, u8, c_int, c_int) -> c_ulong;
    type Pending = unsafe extern "C" fn(*mut c_void) -> c_int;
    type NextEvent = unsafe extern "C" fn(*mut c_void, *mut [c_long; 24]) -> c_int;
    type ToUtf32 = unsafe extern "C" fn(u32) -> u32;

    /// `XkbStateRec`
    #[repr(C)]
    #[derive(Default)]
    struct XkbState {
        group: u8,
        locked_group: u8,
        base_group: u16,
        latched_group: u16,
        mods: u8,
        base_mods: u8,
        latched_mods: u8,
        locked_mods: u8,
        compat_state: u8,
        grab_mods: u8,
        compat_grab_mods: u8,
        lookup_mods: u8,
        compat_lookup_mods: u8,
        ptr_buttons: u16,
    }

    const USE_CORE_KBD: c_uint = 0x100;

    /// An own connection to the X server, only for reading its keymap.
    struct X {
        display: *mut c_void,
        get_state: GetState,
        to_keysym: KeycodeToKeysym,
        pending: Pending,
        next_event: NextEvent,
        to_utf32: Option<ToUtf32>,
    }

    // (the display is only used behind the mutex)
    unsafe impl Send for X {}

    unsafe fn sym<T: Copy>(lib: *mut c_void, name: &CStr) -> Option<T> {
        let p = unsafe { libc::dlsym(lib, name.as_ptr()) };
        // SAFETY: T is the symbol's function pointer type
        (!p.is_null()).then(|| unsafe { std::mem::transmute_copy(&p) })
    }

    fn open() -> Option<Mutex<X>> {
        std::env::var_os("DISPLAY").filter(|d| !d.is_empty())?;
        // SAFETY: libX11 and libxkbcommon by their stable C interfaces
        unsafe {
            let x11 = libc::dlopen(c"libX11.so.6".as_ptr(), libc::RTLD_NOW | libc::RTLD_LOCAL);
            if x11.is_null() {
                return None;
            }
            let open: OpenDisplay = sym(x11, c"XkbOpenDisplay")?;
            let xkb = libc::dlopen(c"libxkbcommon.so.0".as_ptr(), libc::RTLD_NOW | libc::RTLD_LOCAL);
            let to_utf32 = if xkb.is_null() { None } else { sym(xkb, c"xkb_keysym_to_utf32") };
            let (mut event, mut error, mut major, mut minor, mut reason) = (0, 0, 1, 0, 0);
            let display = open(std::ptr::null(), &mut event, &mut error, &mut major, &mut minor, &mut reason);
            if display.is_null() {
                log::info!("keyboard layout: no Xkb display ({reason}), key hints follow typed keys");
                return None;
            }
            Some(Mutex::new(X {
                display,
                get_state: sym(x11, c"XkbGetState")?,
                to_keysym: sym(x11, c"XkbKeycodeToKeysym")?,
                pending: sym(x11, c"XPending")?,
                next_event: sym(x11, c"XNextEvent")?,
                to_utf32,
            }))
        }
    }

    impl X {
        fn char_of(&self, keysym: c_ulong) -> Option<char> {
            let cp = match self.to_utf32 {
                // SAFETY: a pure conversion
                Some(f) => unsafe { f(keysym as u32) },
                None if (0x20..=0x7e).contains(&keysym) || (0xa0..=0xff).contains(&keysym) => keysym as u32,
                None if keysym & 0xff00_0000 == 0x0100_0000 => (keysym & 0x00ff_ffff) as u32,
                None => 0,
            };
            char::from_u32(cp).filter(|c| *c != '\0')
        }
    }

    pub(super) fn legend(code: u8) -> Option<char> {
        static X: OnceLock<Option<Mutex<X>>> = OnceLock::new();
        let x = X.get_or_init(open).as_ref()?.lock().ok()?;
        // SAFETY: the display is ours and used under the lock
        unsafe {
            // read the server's events: Xlib reloads its copy of the keymap on a MapNotify
            let mut event = [0 as c_long; 24];
            while (x.pending)(x.display) > 0 {
                (x.next_event)(x.display, &mut event);
            }
            let mut state = XkbState::default();
            let group = if (x.get_state)(x.display, USE_CORE_KBD, &mut state) == 0 { state.group as c_int } else { 0 };
            let at = |g: c_int| x.char_of((x.to_keysym)(x.display, code, g, 0));
            at(group).filter(|c| crate::keys::is_latin(*c)).or_else(|| {
                (0..4).filter_map(at).find(|c| crate::keys::is_latin(*c))
            })
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn mac_codes_are_the_ansi_positions() {
        assert_eq!(mac_code(KeyCode::KeyQ), Some(0x0C));
        assert_eq!(mac_code(KeyCode::KeyE), Some(0x0E));
        assert_eq!(mac_code(KeyCode::KeyA), Some(0x00));
        assert_eq!(mac_code(KeyCode::F1), None);
    }

    #[cfg(windows)]
    #[test]
    fn the_windows_layout_names_the_letter_keys() {
        assert!(legend(KeyCode::KeyQ).is_some_and(|c| !c.is_control()));
    }
}
