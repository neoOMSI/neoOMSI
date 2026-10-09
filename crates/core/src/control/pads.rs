use crate::controllers::{Connected, Devices};
use std::sync::{Condvar, Mutex, MutexGuard};
use std::time::{Duration, Instant};

/// gilrs and DirectInput list some devices only after their first polls.
const SETTLE: Duration = Duration::from_millis(500);
const IDLE: Duration = Duration::from_secs(30);

struct Reader {
    connected: Option<Vec<Connected>>,
    asked: Instant,
}

/// One thread owns the devices: they are not `Send`, and macOS's HID manager must stay on
/// the thread that polls it.
static READER: Mutex<Option<Reader>> = Mutex::new(None);
static READY: Condvar = Condvar::new();

fn lock() -> MutexGuard<'static, Option<Reader>> {
    READER.lock().unwrap_or_else(|e| e.into_inner())
}

pub(super) fn connected() -> Vec<Connected> {
    let mut r = lock();
    match r.as_mut() {
        Some(r) => r.asked = Instant::now(),
        None => {
            *r = Some(Reader {
                connected: None,
                asked: Instant::now(),
            });
            if let Err(e) = std::thread::Builder::new()
                .name("controller list".into())
                .spawn(read)
            {
                *r = None;
                log::warn!("controller list: {e}");
                return Vec::new();
            }
        }
    }
    let (r, _) = READY
        .wait_timeout_while(r, SETTLE * 4, |r| r.as_ref().is_some_and(|r| r.connected.is_none()))
        .unwrap_or_else(|e| e.into_inner());
    r.as_ref().and_then(|r| r.connected.clone()).unwrap_or_default()
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
    loop {
        devices.poll();
        let now = devices.connected();
        {
            let mut r = lock();
            let Some(r) = r.as_mut() else { return };
            if r.asked.elapsed() > IDLE {
                return;
            }
            if r.connected.is_some() || started.elapsed() >= SETTLE {
                r.connected = Some(now);
                READY.notify_all();
            }
        }
        std::thread::sleep(Duration::from_millis(50));
    }
}
