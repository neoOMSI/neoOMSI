//! Traffic path network and AI road vehicles (original units `mc_path`, `mc_pathrule`,
//! the AI part of `mc_roadvehicle`).
//!
//! Lanes come from `[path]` entries of splines (running along the spline at a lateral
//! offset) and of scenery objects (arcs in the object's frame). Lane ends are linked by
//! proximity, which reproduces the original's spline `prev`/`next` chains and the object
//! `[splinehelper]` connectors without needing their ids.
//!
//! The domain is split by responsibility: [`network`] (topology and geometry), [`rules`]
//! (content-derived permissions), [`signals`] (light programs), and [`following`]
//! (longitudinal control and per-vehicle state). The Stage 1 contract types live in
//! [`ids`], [`capabilities`], [`routing`], [`service`], and [`diagnostics`].

pub mod capabilities;
pub mod diagnostics;
pub mod following;
pub mod ids;
pub mod network;
pub mod routing;
pub mod rules;
pub mod scenario;
pub mod service;
pub mod signals;

pub use capabilities::*;
pub use diagnostics::*;
pub use following::*;
pub use ids::*;
pub use network::*;
pub use routing::*;
pub use rules::*;
pub use scenario::*;
pub use service::*;
pub use signals::*;

#[cfg(test)]
mod tests;
