//! Audio.
//!
//! The crate is layered, each layer only depending on the ones below it:
//! `runtime` (OMSI rules) -> `engine` (façade, command/feedback channels) -> `voice` (state
//! and per-block rendering) -> `dsp` (resampling, filters, limiter, reverb), with `assets`,
//! `device` and `spatial` at the leaves. The runtime decides *what* is heard; the engine
//! decides *how* it sounds; `device` keeps the cpal output apart from both. The engine sends
//! only bounded commands to the audio thread, which owns the voices and DSP state alone.

pub mod assets;
pub mod clock;
pub mod device;
pub mod dsp;
pub mod engine;
mod runtime;
pub mod spatial;
pub mod voice;

// Façade aliases: the module paths the rest of the workspace uses.
pub use assets::{radio, stream, talk, wav};
pub use device::mic;
pub use engine::mixer;
pub use runtime::soundset;

pub use assets::{Clip, Source};
pub use clock::Clock;
pub use engine::{AudioEngine, Playback};
pub use runtime::event::{ordered, EventSource, SoundEvent};
pub use runtime::sound::SoundState;
pub use runtime::soundset::SoundSet;
pub use voice::{DOPPLER, Listener, VoiceId, VoiceParams};

pub use engine::bus::Bus;
