//! Temporary re-export of the headless [`traffic`] domain crate.
//!
//! The implementation moved to `crates/traffic` in Stage 1 (PR batch B); this shim keeps the
//! `simulation::traffic::*` paths working until callers migrate to the new crate directly.

pub use ::traffic::*;
