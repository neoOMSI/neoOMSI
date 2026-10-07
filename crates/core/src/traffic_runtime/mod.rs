//! Engine-integration facade for the headless `traffic` domain crate.
//!
//! This module is the boundary between the traffic domain and the engine. It translates
//! loaded content into typed domain values and owns the presentation, population,
//! passenger, and LAN adapters so the domain crate never depends on `core`, `simulation`,
//! rendering, audio, map loading, scripting, or network.
//!
//! Migration is caller-group by caller-group. `Traffic` exposes query/command methods
//! instead of public fields; the submodules below collect the remaining adapters as their
//! callers move.

pub(crate) mod content;
pub(crate) mod passengers;
pub(crate) mod presentation;
pub(crate) mod replication;
pub(crate) mod vehicles;

/// Which traffic runtime a session uses.
///
/// During migration the only production runtime is [`RuntimeKind::Current`] (the extracted
/// domain crate). The selector exists so a later stage can add the replacement runtime and
/// choose it at session start. A live vehicle is never switched between incompatible state
/// models: the choice is made once, before any vehicle exists.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum RuntimeKind {
    Current,
}

/// The runtime selected for this session, from `OMSI_TRAFFIC_RUNTIME` (default `current`).
///
/// Unknown values fall back to the current runtime with a warning. Read once at session
/// start.
pub(crate) fn selected() -> RuntimeKind {
    match ::legacy_config::env::var("OMSI_TRAFFIC_RUNTIME").ok().as_deref() {
        None | Some("current") => RuntimeKind::Current,
        Some(other) => {
            log::warn!("unknown OMSI_TRAFFIC_RUNTIME={other:?}; using the current runtime");
            RuntimeKind::Current
        }
    }
}
