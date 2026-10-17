//! Traffic path network and AI road vehicles (original units `mc_path`, `mc_pathrule`,
//! the AI part of `mc_roadvehicle`).
//!
//! Lanes come from the `[path]` entries of splines and scenery objects; lane ends are linked
//! by proximity, which reproduces the spline chains and `[splinehelper]` connectors.
//!
//! Modules: [`network`] (topology, geometry), [`rules`] (permissions), [`signals`] (light
//! programs), [`following`] (longitudinal control); contract types in [`ids`],
//! [`capabilities`], [`routing`], [`service`] and [`diagnostics`].

pub mod capabilities;
pub mod diagnostics;
pub mod emergency;
pub mod following;
pub mod ids;
pub mod junctions;
pub mod maneuvers;
pub mod network;
pub mod perception;
pub mod population;
pub mod routing;
pub mod rules;
pub mod scenario;
pub mod service;
pub mod signals;
pub mod validation;
pub mod world;

pub use capabilities::*;
pub use diagnostics::*;
pub use emergency::*;
pub use following::*;
pub use ids::*;
pub use junctions::*;
pub use maneuvers::*;
pub use network::*;
pub use perception::*;
pub use population::*;
pub use routing::*;
pub use rules::*;
pub use scenario::*;
pub use service::*;
pub use signals::*;
pub use validation::*;
pub use world::*;

#[cfg(test)]
mod tests;
