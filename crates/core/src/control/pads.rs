use crate::controllers::{Connected, Devices};
use std::sync::{Condvar, Mutex, MutexGuard};
use std::time::{Duration, Instant};

/// gilrs and DirectInput list some devices only after their first polls.
const SETTLE: Duration = Duration::from_millis(500);
const IDLE: Duration = Duration::from_secs(30);

#[derive(Clone, Default)]
pub(super) struct Pads {
    pub connected: Vec<Connected>,
    pub pressed: Vec<(String, usize)>,
}

impl Pads {
    pub fn pressed(&self, name: &str) -> Vec<u32> {
        self.pressed
            .iter()
            .filter(|(d, _)| crate::controllers::names_match(d, name))
            .map(|(_, n)| *n as u32)
            .collect()
    }
}

struct Reader {
    pads: Option<Pads>,
    asked: Instant,
}

/// One thread owns the devices: they are not `Send`, and macOS's HID manager must stay on
/// the thread that polls it.
static READER: Mutex<Option<Reader>> = Mutex::new(None);
static READY: Condvar = Condvar::new();

fn lock() -> MutexGuard<'static, Option<Reader>> {
    READER.lock().unwrap_or_else(|e| e.into_inner())
}

pub(super) fn now() -> Pads {
    let mut r = lock();
    match r.as_mut() {
        Some(r) => r.asked = Instant::now(),
        None => {
            *r = Some(Reader {
                pads: None,
                asked: Instant::now(),
            });
            if let Err(e) = std::thread::Builder::new()
                .name("controller list".into())
                .spawn(read)
            {
                *r = None;
                log::warn!("controller list: {e}");
                return Pads::default();
            }
        }
    }
    let (r, _) = READY
        .wait_timeout_while(r, SETTLE * 4, |r| r.as_ref().is_some_and(|r| r.pads.is_none()))
        .unwrap_or_else(|e| e.into_inner());
    r.as_ref().and_then(|r| r.pads.clone()).unwrap_or_default()
}

#[cfg(windows)]
const XINPUT_BUTTONS: [(u16, usize); 14] = [
    (0x4000, 0),
    (0x1000, 1),
    (0x2000, 2),
    (0x8000, 3),
    (0x0100, 4),
    (0x0200, 5),
    (0x0020, 8),
    (0x0010, 9),
    (0x0040, 10),
    (0x0080, 11),
    (0x0001, crate::controllers::HAT_BUTTONS),
    (0x0008, crate::controllers::HAT_BUTTONS + 1),
    (0x0002, crate::controllers::HAT_BUTTONS + 2),
    (0x0004, crate::controllers::HAT_BUTTONS + 3),
];

#[cfg(windows)]
fn xinput(connected: &mut [Connected], pressed: &mut Vec<(String, usize)>) {
    use windows::Win32::UI::Input::XboxController::{XINPUT_STATE, XInputGetState};
    let mut pads = connected
        .iter_mut()
        .filter(|c| c.gamepad && crate::controllers::xinput_name(&c.name));
    for user in 0..4 {
        let mut s = XINPUT_STATE::default();
        if unsafe { XInputGetState(user, &mut s) } != 0 {
            continue;
        }
        let Some(pad) = pads.next() else { return };
        let g = s.Gamepad;
        let stick = |v: i16| if v < 0 { v as f32 / 32768.0 } else { v as f32 / 32767.0 };
        let trigger = |v: u8| v as f32 / 255.0 * 2.0 - 1.0;
        pad.axes = vec![
            (0, stick(g.sThumbLX)),
            (1, stick(g.sThumbLY)),
            (2, trigger(g.bLeftTrigger)),
            (3, stick(g.sThumbRX)),
            (4, stick(g.sThumbRY)),
            (5, trigger(g.bRightTrigger)),
        ];
        pressed.retain(|(d, _)| *d != pad.name);
        pressed.extend(
            XINPUT_BUTTONS
                .iter()
                .filter(|(bit, _)| g.wButtons.0 & bit != 0)
                .map(|(_, n)| (pad.name.clone(), *n)),
        );
    }
}

struct Stopped;

impl Drop for Stopped {
    fn drop(&mut self) {
        *lock() = None;
        READY.notify_all();
    }
}

fn read() {
    let _stopped = Stopped;
    #[cfg(windows)]
    let window = crate::dinput::helper_window();
    #[cfg(not(windows))]
    let window = None;
    let mut devices = Devices::new(window, false);
    let started = Instant::now();
    let mut pressed: Vec<(String, usize)> = Vec::new();
    loop {
        for (name, n, down) in devices.poll() {
            pressed.retain(|(d, b)| !(*b == n && *d == name));
            if down {
                pressed.push((name, n));
            }
        }
        #[allow(unused_mut)]
        let mut now = devices.connected();
        #[cfg(windows)]
        xinput(&mut now, &mut pressed);
        pressed.retain(|(d, _)| now.iter().any(|c| c.name == *d));
        {
            let mut r = lock();
            let Some(r) = r.as_mut() else { return };
            if r.asked.elapsed() > IDLE {
                return;
            }
            if r.pads.is_some() || started.elapsed() >= SETTLE {
                r.pads = Some(Pads {
                    connected: now,
                    pressed: pressed.clone(),
                });
                READY.notify_all();
            }
        }
        std::thread::sleep(Duration::from_millis(50));
    }
}
