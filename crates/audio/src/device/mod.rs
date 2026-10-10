//! The CPAL output: opening a stream on the system's default device, following it when the
//! default changes, and the device's format as shared state.

pub mod mic;
pub mod output;
pub(crate) mod mix_output;
pub mod state;
pub mod watcher;

pub use output::DeviceOutput;
pub use state::{DeviceState, OutputFormat};
pub use watcher::watch_default_device;

pub mod convert;
