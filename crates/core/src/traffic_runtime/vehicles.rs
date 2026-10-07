//! Vehicle motion realisation and script handshake adapter (Stage 1 batch C5).
//!
//! Owns the boundary where domain motion commands meet `VehicleInstance` and the vehicle
//! scripts. Populated as callers migrate off direct `Traffic`/`AiCar` access.
