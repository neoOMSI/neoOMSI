//! Stable identifiers for the traffic domain.
//!
//! These survive container reordering: a `VehicleId` stays valid while vehicles are
//! inserted, removed, or sorted, and a `NetworkVersion` invalidates handles when a
//! streamed network change makes them meaningless. Vector positions are internal
//! implementation details and must not leak across the domain boundary as identities.

use std::fmt;

macro_rules! id_type {
    ($name:ident, $inner:ty, $doc:literal) => {
        #[doc = $doc]
        #[repr(transparent)]
        #[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
        pub struct $name(pub $inner);

        impl $name {
            #[inline]
            pub const fn new(value: $inner) -> Self {
                Self(value)
            }

            #[inline]
            pub const fn get(self) -> $inner {
                self.0
            }
        }

        impl From<$inner> for $name {
            #[inline]
            fn from(value: $inner) -> Self {
                Self(value)
            }
        }

        impl From<$name> for $inner {
            #[inline]
            fn from(value: $name) -> Self {
                value.0
            }
        }

        impl fmt::Display for $name {
            fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
                fmt::Display::fmt(&self.0, f)
            }
        }
    };
}

id_type!(VehicleId, u64, "Identity of a vehicle in the traffic world.");
id_type!(LaneId, usize, "Index of a lane in the current network version.");
id_type!(StopId, i64, "Identity of a stop as its map object id.");
id_type!(TripId, u64, "Identity of a timetable trip.");
id_type!(DutyId, u64, "Identity of a scheduled duty/tour.");
id_type!(NetworkVersion, u64, "Version of the loaded network; invalidates lane handles when it changes.");

impl LaneId {
    /// Wrap a raw lane index.
    #[inline]
    pub const fn index(self) -> usize {
        self.0
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ids_round_trip_through_their_raw_value() {
        let v = VehicleId::from(7u64);
        assert_eq!(v.get(), 7);
        assert_eq!(u64::from(v), 7);
        assert_eq!(VehicleId::new(9).get(), 9);
        assert_eq!(format!("{v}"), "7");
    }

    #[test]
    fn ids_compare_by_value_and_order() {
        assert!(VehicleId(2) > VehicleId(1));
        assert_eq!(VehicleId(3), 3u64.into());
        assert_ne!(LaneId(1), LaneId(2));
    }

    #[test]
    fn hashing_matches_the_raw_value() {
        use std::collections::hash_map::DefaultHasher;
        use std::hash::{Hash, Hasher};
        let mut a = DefaultHasher::new();
        VehicleId(42).hash(&mut a);
        let mut b = DefaultHasher::new();
        42u64.hash(&mut b);
        assert_eq!(a.finish(), b.finish());
    }
}

