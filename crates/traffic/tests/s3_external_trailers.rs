//! Stage 3 exit gate — player/LAN trailers are perceived, and height separates bridges.
//!
//! External road users (the player and LAN remotes) are fed to perception with stable ids;
//! their trailers are separate footprints sharing the towing id. A body on a bridge above a
//! road does not occupy that road, while a body at the same level does.

use glam::{DVec2, DVec3};
use traffic::perception::{BodyFootprint, Occupancy, SweepSample};
use traffic::{NetworkVersion, VehicleId};

/// Emulate core's external-actor id scheme: a base id per vehicle, trailers at
/// `base + 1 + part`.
fn external_bus_with_trailer(base: u64, z0: f64, z1: f64) -> Vec<BodyFootprint> {
    let mut tow = BodyFootprint::new(
        VehicleId(base),
        DVec2::new(0.0, 20.0),
        DVec2::new(0.0, 1.0),
        5.0,
        1.25,
        z0,
        z1,
        6.0,
    );
    tow.front = 5.0;
    tow.rear = 5.0;
    let trailer = tow.part_of(
        1,
        DVec2::new(0.0, 11.0),
        DVec2::new(0.0, 1.0),
        3.0,
        1.25,
    );
    vec![tow, trailer]
}

fn sample(y: f64) -> [SweepSample; 1] {
    [SweepSample {
        p: DVec3::new(0.0, y, 0.0),
        d: 1.0,
        dir: DVec2::new(0.0, 1.0),
    }]
}

#[test]
fn a_lan_trailer_is_perceived_as_its_own_body() {
    // Base id 0x0000_1000 as a LAN session; the trailer is base + 2.
    let occ = Occupancy::build(NetworkVersion(1), 0, external_bus_with_trailer(0x1000, 0.0, 3.0));
    let hit = occ.swept_clearance(&sample(11.0), 1.25, &[]).unwrap();
    assert_eq!(hit.owner, VehicleId(0x1000), "the trailer's owner was lost");
    assert_eq!(hit.part, 1, "the trailer is not reported as a separate part");
    // The towing vehicle is perceived where it stands.
    assert_eq!(
        occ.swept_clearance(&sample(20.0), 1.25, &[]).map(|s| s.owner),
        Some(VehicleId(0x1000))
    );
}

#[test]
fn a_body_on_a_bridge_above_the_road_does_not_occupy_it() {
    // A LAN vehicle on an overpass: same plan view, 7 m up.
    let mut above = external_bus_with_trailer(0x2000, 0.0, 3.0);
    for f in &mut above {
        f.z0 += 7.0;
        f.z1 += 7.0;
    }
    let occ = Occupancy::build(NetworkVersion(1), 0, above);
    assert!(
        occ.swept_clearance(&sample(20.0), 1.25, &[]).is_none(),
        "a bridge body occupied the road below"
    );
}

#[test]
fn a_body_at_the_same_level_conflicts() {
    let occ = Occupancy::build(NetworkVersion(1), 0, external_bus_with_trailer(0x3000, 0.0, 3.0));
    assert!(
        occ.swept_clearance(&sample(20.0), 1.25, &[]).is_some(),
        "a same-level body was not perceived"
    );
}

#[test]
fn a_trailer_inside_the_corridor_stops_a_car_but_a_far_one_does_not() {
    let occ = Occupancy::build(NetworkVersion(1), 0, external_bus_with_trailer(0x4000, 0.0, 3.0));
    // The corridor is the car's half width round the sample: 10 m to the side is clear.
    let side = [SweepSample {
        p: DVec3::new(10.0, 20.0, 0.0),
        d: 1.0,
        dir: DVec2::new(0.0, 1.0),
    }];
    assert!(occ.swept_clearance(&side, 1.25, &[]).is_none());
}
