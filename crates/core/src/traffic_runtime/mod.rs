//! Engine-integration facade for the headless `traffic` domain crate.
//!
//! This module is the boundary between the traffic domain and the engine. It translates
//! loaded content into typed domain values so the domain crate never depends on `core`,
//! `simulation`, rendering, audio, map loading, scripting, or network.
//!
//! The Stage 9 cutover removed the migration scaffold: there is exactly one production
//! road-AI runtime (the `traffic` domain crate driven by `core::Traffic`), so the
//! session-start `RuntimeKind`/`OMSI_TRAFFIC_RUNTIME` selector and its empty adapter stubs
//! are gone. The remaining adapter is [`content`].

pub(crate) mod content;
