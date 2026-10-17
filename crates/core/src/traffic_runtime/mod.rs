//! Engine-integration facade for the headless `traffic` domain crate.
//!
//! It translates loaded content into typed domain values (see [`content`]) so the domain
//! crate never depends on `core`, `simulation`, rendering, maps, scripting or network.

pub(crate) mod content;
