//! Engine-integration facade for the headless `traffic` domain crate.
//!
//! This module is the boundary between the traffic domain and the engine. It translates
//! loaded content into typed domain values and, in later Stage 1 batches, owns the
//! presentation, population, passenger, and LAN adapters so the domain crate never
//! depends on `core`, `simulation`, rendering, audio, map loading, scripting, or network.

pub(crate) mod content;
