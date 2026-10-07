//! Validated, immutable physical capabilities of a vehicle.
//!
//! Capabilities are derived once when a vehicle enters the traffic world and are not
//! mutated afterwards. They separate what a vehicle physically is from what a driver
//! chooses to do (speed, gaps, reactions), and from asset/presentation state. The
//! reported fallback provenance is kept so a later stage can calibrate the guesses.

/// The physical class a vehicle drives as (`[ai_veh_type]`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum VehicleClass {
    Car,
    Taxi,
    Bus,
    Truck,
    Other(i32),
}

impl VehicleClass {
    pub fn from_ai_veh_type(value: i32) -> VehicleClass {
        match value {
            0 => VehicleClass::Car,
            1 => VehicleClass::Taxi,
            2 => VehicleClass::Bus,
            3 => VehicleClass::Truck,
            other => VehicleClass::Other(other),
        }
    }

    pub fn ai_veh_type(self) -> i32 {
        match self {
            VehicleClass::Car => 0,
            VehicleClass::Taxi => 1,
            VehicleClass::Bus => 2,
            VehicleClass::Truck => 3,
            VehicleClass::Other(other) => other,
        }
    }
}

/// Where the physical extents came from, so a guessed fallback stays visible.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CapabilitySource {
    /// The vehicle's `[boundingbox]`.
    BoundingBox,
    /// The loaded model's own bounding box, as Omsi.exe takes it.
    ModelBox,
    /// No usable box: a length-based guess.
    LengthFallback,
}

/// Immutable physical capabilities of one vehicle.
#[derive(Debug, Clone, PartialEq)]
pub struct VehicleCapabilities {
    /// Total length from rear to front bumper (m).
    pub length: f32,
    /// Origin (the lane position `s`) to the front bumper (m).
    pub front: f32,
    /// Origin to the rear bumper (m).
    pub rear: f32,
    /// Half the body width (m).
    pub half_width: f32,
    /// `[ai_veh_type]` as stored on the path rules (see `Lane::allows`).
    pub veh_type: i32,
    pub class: VehicleClass,
    /// The vehicle's own top speed (km/h), before driver desire.
    pub max_speed_kmh: f32,
    /// The five `[ai_brakeperformance]` values, unresolved semantics kept as data.
    pub brake_performance: Option<[f32; 5]>,
    pub source: CapabilitySource,
}

/// A capability that could not be validated; the value is clamped instead of failing.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CapabilityDefect {
    NonPositiveLength,
    NonPositiveHalfWidth,
}

impl VehicleCapabilities {
    /// Build from measured extents. Values are clamped to safe minimums; any clamp is
    /// reported through [`VehicleCapabilities::defects`] rather than silently hidden.
    #[allow(clippy::too_many_arguments)]
    pub fn from_extents(
        front: f32,
        rear: f32,
        half_width: f32,
        source: CapabilitySource,
        veh_type: i32,
        max_speed_kmh: f32,
        brake_performance: Option<[f32; 5]>,
    ) -> VehicleCapabilities {
        let mut caps = VehicleCapabilities {
            length: (front + rear).max(0.0),
            front: front.max(0.0),
            rear: rear.max(0.0),
            half_width: half_width.max(0.5),
            veh_type,
            class: VehicleClass::from_ai_veh_type(veh_type),
            max_speed_kmh,
            brake_performance,
            source,
        };
        caps.clamp();
        caps
    }

    fn clamp(&mut self) {
        if self.length <= 0.0 {
            self.length = 1.0;
            self.front = 0.5;
            self.rear = 0.5;
        }
        if self.half_width < 0.5 {
            self.half_width = 0.5;
        }
    }

    /// Physical problems found in this capability, if any.
    pub fn defects(&self) -> Vec<CapabilityDefect> {
        let mut out = Vec::new();
        if self.length <= 0.0 || self.front < 0.0 || self.rear < 0.0 {
            out.push(CapabilityDefect::NonPositiveLength);
        }
        if self.half_width <= 0.0 {
            out.push(CapabilityDefect::NonPositiveHalfWidth);
        }
        out
    }

    /// True when the extents were guessed rather than read from content.
    pub fn is_fallback(&self) -> bool {
        self.source == CapabilitySource::LengthFallback
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn extents_are_kept_and_classified() {
        let caps = VehicleCapabilities::from_extents(
            1.2,
            0.8,
            1.0,
            CapabilitySource::BoundingBox,
            2,
            50.0,
            None,
        );
        assert_eq!(caps.length, 2.0);
        assert_eq!(caps.class, VehicleClass::Bus);
        assert!(!caps.is_fallback());
        assert!(caps.defects().is_empty());
    }

    #[test]
    fn a_degenerate_vehicle_is_clamped_not_rejected() {
        let caps =
            VehicleCapabilities::from_extents(0.0, 0.0, 0.0, CapabilitySource::LengthFallback, 0, 40.0, None);
        assert!(caps.length > 0.0);
        assert!(caps.half_width >= 0.5);
        assert!(caps.is_fallback());
    }

    #[test]
    fn all_five_brake_values_are_kept_even_though_only_one_is_used() {
        let values = [1.0, 2.0, 3.0, 4.0, 5.0];
        let caps = VehicleCapabilities::from_extents(
            1.0,
            1.0,
            1.0,
            CapabilitySource::BoundingBox,
            2,
            50.0,
            Some(values),
        );
        assert_eq!(caps.brake_performance, Some(values));
        assert_eq!(caps.brake_performance.unwrap()[4], 5.0);
    }

    #[test]
    fn class_round_trips_through_ai_veh_type() {
        for v in 0..=4 {
            assert_eq!(VehicleClass::from_ai_veh_type(v).ai_veh_type(), v);
        }
    }
}
