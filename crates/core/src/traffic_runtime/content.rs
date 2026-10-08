//! Translation from loaded vehicle content to validated domain capabilities.

use ::simulation::VehicleType;
use ::traffic::{CapabilitySource, VehicleCapabilities};

/// Build a vehicle's immutable capabilities from its loaded type and AI class.
pub(crate) fn capabilities(
    ty: &VehicleType,
    veh_type: i32,
    max_speed_kmh: f32,
    fallback_length: f32,
) -> VehicleCapabilities {
    let (front, rear, half_width) = crate::traffic::extents(ty, fallback_length);
    let source = if matches!(ty.def.bounding_box, Some(bb) if bb[1] > 1.0) {
        CapabilitySource::BoundingBox
    } else if ty.model_box().map(|(lo, hi)| hi.y - lo.y > 1.0).unwrap_or(false) {
        CapabilitySource::ModelBox
    } else {
        CapabilitySource::LengthFallback
    };
    VehicleCapabilities::from_extents(
        front,
        rear,
        half_width,
        source,
        veh_type,
        max_speed_kmh,
        ty.def.ai_brake_performance,
    )
}
