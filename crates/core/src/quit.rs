//! Ending the game from outside: the launcher's Stop button sends SIGTERM, a terminal
//! Ctrl+C sends SIGINT, a closed terminal SIGHUP. Left to their default these end the
//! process on the spot, so no session summary and no personnel file were written and the
//! LAN peers only noticed the player was gone when he timed out. Here the signal only
//! marks the request; a watcher thread wakes the event loop, which then ends the session
//! the way Escape and closing the window do (summary, personnel file, LAN goodbye). A
//! second signal ends the process at once, for a game that does not react. The launcher's
//! quit over the game link (`request`) takes the same way, on every platform.

use std::sync::Mutex;
use std::sync::atomic::{AtomicI32, Ordering};

/// The signal that asked the game to end (0 = none yet).
static REQUESTED: AtomicI32 = AtomicI32::new(0);

type Wake = Box<dyn FnOnce(i32) + Send>;

static WAKE: Mutex<Option<Wake>> = Mutex::new(None);

/// Not a signal: the engine asked over the game link.
pub const LAUNCHER: i32 = -1;

/// Why the game was asked to end, for the log.
pub fn signal_name(sig: i32) -> &'static str {
    match sig {
        SIGTERM => "SIGTERM",
        SIGINT => "SIGINT",
        SIGHUP => "SIGHUP",
        LAUNCHER => "the launcher's quit request",
        _ => "a signal",
    }
}

const SIGHUP: i32 = 1;
const SIGINT: i32 = 2;
const SIGTERM: i32 = 15;

#[cfg(unix)]
mod sys {
    pub type Handler = usize;
    pub const SIG_DFL: Handler = 0;
    pub const SIG_IGN: Handler = 1;
    pub const SIG_ERR: Handler = usize::MAX;
    unsafe extern "C" {
        pub fn signal(sig: i32, handler: Handler) -> Handler;
        pub fn raise(sig: i32) -> i32;
    }
}

/// The handler: only atomics and async-signal-safe calls (`signal`, `raise`) in here.
#[cfg(unix)]
extern "C" fn on_signal(sig: i32) {
    if REQUESTED.swap(sig, Ordering::SeqCst) != 0 {
        // asked a second time: the default action (the process ends now)
        // SAFETY: both calls are async-signal-safe
        unsafe {
            sys::signal(sig, sys::SIG_DFL);
            sys::raise(sig);
        }
    }
}

fn fire(sig: i32) {
    let wake = WAKE.lock().unwrap_or_else(|e| e.into_inner()).take();
    if let Some(wake) = wake {
        wake(sig);
    }
}

/// Take SIGTERM, SIGINT and SIGHUP over for the window's event loop: `wake` is called once,
/// from a watcher thread, when one of them arrived (a signal the parent set to be ignored,
/// as `nohup` does, stays ignored), or by `request`.
pub fn install(wake: impl FnOnce(i32) + Send + 'static) {
    *WAKE.lock().unwrap_or_else(|e| e.into_inner()) = Some(Box::new(wake));
    if let Some(sig) = requested() {
        fire(sig);
    }
    #[cfg(unix)]
    {
        for sig in [SIGTERM, SIGINT, SIGHUP] {
            // SAFETY: `on_signal` has the C signature and does only async-signal-safe work
            unsafe {
                let old = sys::signal(sig, on_signal as extern "C" fn(i32) as sys::Handler);
                if old == sys::SIG_ERR {
                    log::warn!(
                        "cannot handle {}: the game ends at once when it arrives",
                        signal_name(sig)
                    );
                } else if old == sys::SIG_IGN {
                    sys::signal(sig, sys::SIG_IGN);
                }
            }
        }
        let spawned = std::thread::Builder::new()
            .name("quit signal".into())
            .spawn(move || {
                loop {
                    let sig = REQUESTED.load(Ordering::SeqCst);
                    if sig != 0 {
                        fire(sig);
                        return;
                    }
                    std::thread::sleep(std::time::Duration::from_millis(100));
                }
            });
        if let Err(e) = spawned {
            log::warn!("no quit signal watcher ({e}): SIGTERM ends the game without saving");
            // SAFETY: back to the default actions
            unsafe {
                for sig in [SIGTERM, SIGINT, SIGHUP] {
                    let old = sys::signal(sig, sys::SIG_DFL);
                    if old == sys::SIG_IGN {
                        sys::signal(sig, sys::SIG_IGN);
                    }
                }
            }
        }
    }
}

pub fn request() {
    if REQUESTED
        .compare_exchange(0, LAUNCHER, Ordering::SeqCst, Ordering::SeqCst)
        .is_ok()
    {
        fire(LAUNCHER);
    }
}

/// The signal that asked the game to end, if one did.
pub fn requested() -> Option<i32> {
    Some(REQUESTED.load(Ordering::SeqCst)).filter(|s| *s != 0)
}

#[cfg(all(test, not(unix)))]
mod tests {
    use super::*;

    #[test]
    fn a_request_before_install_is_not_lost() {
        request();
        assert_eq!(requested(), Some(LAUNCHER));
        let (tx, rx) = std::sync::mpsc::channel();
        install(move |sig| tx.send(sig).unwrap());
        assert_eq!(
            rx.recv_timeout(std::time::Duration::from_secs(5)).unwrap(),
            LAUNCHER
        );
        request();
        assert!(
            rx.recv_timeout(std::time::Duration::from_millis(200)).is_err(),
            "woken once"
        );
        REQUESTED.store(0, Ordering::SeqCst);
    }
}

#[cfg(all(test, unix))]
mod tests {
    use super::*;

    #[test]
    fn a_signal_wakes_the_loop_once() {
        let (tx, rx) = std::sync::mpsc::channel();
        install(move |sig| tx.send(sig).unwrap());
        assert!(requested().is_none());
        // SAFETY: raising a handled signal in our own process
        unsafe { sys::raise(SIGTERM) };
        assert_eq!(
            rx.recv_timeout(std::time::Duration::from_secs(5)).unwrap(),
            SIGTERM
        );
        assert_eq!(requested(), Some(SIGTERM));
        // woken once: the watcher is done (and has dropped its sender)
        assert_eq!(
            rx.recv_timeout(std::time::Duration::from_millis(300)),
            Err(std::sync::mpsc::RecvTimeoutError::Disconnected)
        );
        // (a second signal would end the test process: not raised here)
    }
}
