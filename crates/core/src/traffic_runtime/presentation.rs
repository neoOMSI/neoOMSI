//! Rendering and audio synchronisation adapter (Stage 1 batch C5).
//!
//! Owns the presentation-side traffic boundary (`sync`, `update_audio`, viewer and camera
//! inputs) so presentation reads committed state through typed calls. Populated as callers
//! migrate.
