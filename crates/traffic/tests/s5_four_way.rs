//! Stage 5 exit gate — four-way / conflicting claims.
//!
//! Two approaches meet at a `+` junction. The rule-first coordinator admits the movement
//! with the right of way and holds the other at its line; no two conflicting movements are
//! granted at once, and the decision does not depend on the container order.

mod common;

use common::{four_way, Harness};
use traffic::{JunctionActor, JunctionCoordinator, VehicleId};

fn car(id: u64, lane: usize, s: f32, speed: f32) -> JunctionActor {
    let mut a = JunctionActor::new(VehicleId(id), lane, s);
    a.speed = speed;
    a.accel = 2.0;
    a.decel = 2.5;
    a.accept_gap = 5.0;
    a
}

/// West->east yields to south->north (it comes from the right): one goes, the other waits.
#[test]
fn conflicting_claims_admit_exactly_one_movement() {
    let mut h = Harness::new(four_way());
    let w = h.add(car(10, 0, 95.0, 0.0)); // west approach, 5 m from its object lane
    let s = h.add(car(20, 4, 95.0, 0.0)); // south approach
    // both are physically 5 m before their object lane, so each is 15 m from the crossing
    h.approach(w, 1, 5.0);
    h.approach(s, 5, 5.0);

    let mut coord = JunctionCoordinator::new();
    coord.begin_tick(0);

    let dec_s = h.plan(&mut coord, s, None);
    let dec_w = h.plan(&mut coord, w, None);

    assert!(
        dec_s.yield_at.is_none(),
        "the right-of-way movement had to wait: {dec_s:?}"
    );
    assert!(
        coord.holds(VehicleId(20), 5),
        "the admitted movement did not claim its junction lane"
    );
    assert!(
        dec_w.yield_at.is_some(),
        "the yielding movement went anyway: {dec_w:?}"
    );
    assert!(
        !coord.holds(VehicleId(10), 1),
        "the waiting movement reserved the junction anyway"
    );
}

/// Reordering the actors must not change who is admitted: the outcome follows the rule and
/// the stable ids, not the storage order.
#[test]
fn the_admission_does_not_depend_on_container_order() {
    let mut first_order = Harness::new(four_way());
    let w = first_order.add(car(10, 0, 95.0, 0.0));
    let s = first_order.add(car(20, 4, 95.0, 0.0));
    first_order.approach(w, 1, 5.0);
    first_order.approach(s, 5, 5.0);

    let mut reversed = Harness::new(four_way());
    let s2 = reversed.add(car(20, 4, 95.0, 0.0));
    let w2 = reversed.add(car(10, 0, 95.0, 0.0));
    reversed.approach(w2, 1, 5.0);
    reversed.approach(s2, 5, 5.0);

    let mut a = JunctionCoordinator::new();
    a.begin_tick(0);
    let a_s = first_order.plan(&mut a, s, None);
    let a_w = first_order.plan(&mut a, w, None);

    let mut b = JunctionCoordinator::new();
    b.begin_tick(0);
    let b_w = reversed.plan(&mut b, w2, None);
    let b_s = reversed.plan(&mut b, s2, None);

    assert_eq!(a_s.yield_at.is_none(), b_s.yield_at.is_none());
    assert_eq!(a_w.yield_at.is_none(), b_w.yield_at.is_none());
    assert_eq!(a.holds(VehicleId(20), 5), b.holds(VehicleId(20), 5));
    assert_eq!(a.holds(VehicleId(10), 1), b.holds(VehicleId(10), 1));
}
