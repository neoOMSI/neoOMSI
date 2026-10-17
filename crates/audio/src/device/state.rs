//! The output device's sample rate and channel count, shared between the device (which sets
//! them when it opens a stream) and the mixer (which reads them while rendering), and the
//! explicit state of the output: no device, opening, open, lost or reopening. The states are
//! pure transitions so they can be tested without a device.

use std::sync::atomic::{AtomicU32, AtomicU64, AtomicUsize, Ordering};

#[derive(Default)]
pub(crate) struct OutputErrors {
    pub(crate) xruns: AtomicU64,
    pub(crate) realtime_denials: AtomicU64,
}

pub struct OutputFormat {
    sample_rate: AtomicU32,
    channels: AtomicUsize,
}

impl OutputFormat {
    pub fn new(sample_rate: u32, channels: usize) -> OutputFormat {
        OutputFormat {
            sample_rate: AtomicU32::new(sample_rate.max(1)),
            channels: AtomicUsize::new(channels.max(1)),
        }
    }

    pub fn set(&self, sample_rate: u32, channels: usize) {
        self.sample_rate.store(sample_rate.max(1), Ordering::Relaxed);
        self.channels.store(channels.max(1), Ordering::Relaxed);
    }

    pub fn sample_rate(&self) -> u32 {
        self.sample_rate.load(Ordering::Relaxed).max(1)
    }

    pub fn channels(&self) -> usize {
        self.channels.load(Ordering::Relaxed).max(1)
    }
}

/// Where the output stream is. Every ending is defined: a lost device goes back to
/// `Reopening` and then `Open` (or `NoDevice`), so playback never sits in an undefined half
/// state.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DeviceState {
    /// There has never been a stream (no device at start-up).
    NoDevice,
    /// A stream is being built.
    Opening,
    /// A stream is playing.
    Open,
    /// The device played on went away; a reopen is due.
    Lost,
    /// The device's default changed; a reopen is due.
    Reopening,
}

impl DeviceState {
    /// The next attempt starts.
    pub fn opening(self) -> DeviceState {
        DeviceState::Opening
    }

    /// The attempt succeeded.
    pub fn opened(self) -> DeviceState {
        DeviceState::Open
    }

    /// The attempt failed: still no device if there never was one, else lost.
    pub fn open_failed(self) -> DeviceState {
        match self {
            DeviceState::NoDevice | DeviceState::Opening => DeviceState::NoDevice,
            _ => DeviceState::Lost,
        }
    }

    /// The device that was playing went away.
    pub fn lost(self) -> DeviceState {
        DeviceState::Lost
    }

    /// The system's default output changed.
    pub fn retrying(self) -> DeviceState {
        DeviceState::Reopening
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_lost_device_ends_up_open_again() {
        let s = DeviceState::Open;
        let s = s.lost();
        assert_eq!(s, DeviceState::Lost);
        let s = s.retrying();
        assert_eq!(s, DeviceState::Reopening);
        let s = s.opening();
        assert_eq!(s, DeviceState::Opening);
        let s = s.opened();
        assert_eq!(s, DeviceState::Open);
    }

    #[test]
    fn a_first_open_that_fails_stays_without_a_device() {
        assert_eq!(DeviceState::NoDevice.opening().open_failed(), DeviceState::NoDevice);
        assert_eq!(DeviceState::Opening.open_failed(), DeviceState::NoDevice);
        assert_eq!(DeviceState::Reopening.open_failed(), DeviceState::Lost);
    }
}
