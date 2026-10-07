//! Engine-integration facade for the headless `traffic` domain crate.
//!
//! This module is the boundary between the traffic domain and the engine. It translates
//! loaded content into typed domain values and owns the presentation, population,
//! passenger, and LAN adapters so the domain crate never depends on `core`, `simulation`,
//! rendering, audio, map loading, scripting, or network.
//!
//! Migration is caller-group by caller-group. Query methods on `Traffic` (for example
//! `car_count`, `target`) are the first step; the submodules below collect the remaining
//! adapters as their callers move.

pub(crate) mod content;
pub(crate) mod passengers;
pub(crate) mod presentation;
pub(crate) mod replication;
pub(crate) mod vehicles;
