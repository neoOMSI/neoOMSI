//! Stage 3 exit gate — an articulated bus rear still blocks a junction until it is clear.
//!
//! The realized geometry is the occupancy truth. While the rear section of an articulated
//! bus still projects onto the junction lane, that lane is occupied; once the rear has
//! moved onto the exit lane, the junction is clear even though the bus is still on the
//! road.

use glam::DVec2;
use traffic::perception::{BodyFootprint, Occupancy, Placement, PART_BODY};
use traffic::{LaneId, NetworkVersion, VehicleId};

const JUNCTION: LaneId = LaneId(0);
const EXIT: LaneId = LaneId(1);
/// Part index of an articulated bus's rear section.
const PART_REAR: u16 = 1;

/// An articulated bus, front section on `front_lane`, rear section on `rear_lane`
/// (or fully on the exit once `rear_on_exit`).
fn articulated_bus(rear_on_exit: bool) -> Vec<BodyFootprint> {
    let bus = VehicleId(42);
    let mut front = BodyFootprint::new(
        bus,
        DVec2::new(0.0, 10.0),
        DVec2::new(0.0, 1.0),
        3.0,
        1.25,
        0.0,
        3.0,
        4.0,
    );
    front.current = Some(Placement {
        lane: EXIT,
        s: 8.0,
        lateral: 0.0,
        foreign: false,
    });
    assert_eq!(front.part, PART_BODY);

    let mut rear = front.part_of(
        PART_REAR,
        DVec2::new(0.0, 3.0),
        DVec2::new(0.0, 1.0),
        2.5,
        1.25,
    );
    // Before the rear clears: it still projects onto the junction. After: the exit lane.
    rear.current = Some(Placement {
        lane: if rear_on_exit { EXIT } else { JUNCTION },
        s: if rear_on_exit { 3.0 } else { 25.0 },
        lateral: 0.0,
        foreign: false,
    });
    vec![front, rear]
}

#[test]
fn the_bus_rear_blocks_the_junction_until_it_clears() {
    // A car approaching the junction at s = 5 with its front bumper there.
    let probe_front = 5.0f32;

    let blocked = Occupancy::build(NetworkVersion(1), 0, articulated_bus(false));
    let seen = blocked.nearest_ahead(JUNCTION, probe_front, 0.0, 3.0, false);
    assert_eq!(
        seen.map(|s| s.owner),
        Some(VehicleId(42)),
        "the bus rear did not block the junction"
    );
    assert_eq!(seen.unwrap().part, PART_REAR, "the blocking part is not the rear section");

    let cleared = Occupancy::build(NetworkVersion(1), 1, articulated_bus(true));
    assert!(
        cleared.nearest_ahead(JUNCTION, probe_front, 0.0, 3.0, false).is_none(),
        "the junction stayed blocked after the rear cleared it"
    );
    // The bus is still on the road, now on the exit lane.
    assert!(
        cleared
            .nearest_ahead(EXIT, 2.0, 0.0, 3.0, false)
            .is_some(),
        "the bus vanished instead of moving onto the exit"
    );
}

#[test]
fn the_bus_rear_keeps_its_owner_visible_even_when_only_the_rear_remains() {
    let occ = Occupancy::build(NetworkVersion(1), 0, articulated_bus(false));
    // Only the rear part is on the junction: berth occupancy still names the bus.
    assert_eq!(
        occ.berth_occupancy(JUNCTION, 25.0, 3.0, &[]),
        Some(VehicleId(42))
    );
}
