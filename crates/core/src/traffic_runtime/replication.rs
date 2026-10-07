//! Host snapshots and client presentation adapter (Stage 1 batch C5).
//!
//! Owns the LAN replication boundary. Clients display replicated committed state; they do
//! not make independent traffic decisions. Populated as callers migrate.
