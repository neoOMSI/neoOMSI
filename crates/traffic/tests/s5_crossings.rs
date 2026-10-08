//! Stage 5 exit gate — pedestrian protection and rail-crossing restrictions.
//!
//! A pedestrian on a crossing and a train on its rail lane both hold the street movement
//! outside the conflict; neither is driven through, and the movement resumes once the
//! crossing clears.

mod common;

use common::{four_way, rail_crossing, Harness};
use traffic::{JunctionActor, JunctionCoordinator, VehicleId};

fn stopper(id: u64, lane: usize, s: f32) -> JunctionActor {
    let mut a = JunctionActor::new(VehicleId(id), lane, s);
    a.speed = 0.0;
    a
}

#[test]
fn a_pedestrian_on_the_crossing_holds_the_street() {
    let mut net = four_way();
    // a footpath crossing the west->east object lane at s = 10
    net.walks[1] = vec![(99, 10.0, 5.0)];
    let mut h = Harness::new(net);
    let ego = h.add(stopper(10, 0, 95.0));
    h.approach(ego, 1, 5.0);
    h.walkers.insert(99, vec![5.0]);

    let mut coord = JunctionCoordinator::new();
    coord.begin_tick(0);
    let d = h.plan(&mut coord, ego, None);
    assert!(
        d.yield_at.is_some() && d.reasons.contains(&traffic::Reason::Pedestrian),
        "the street drove over a pedestrian: {d:?}"
    );

    // the crossing clears: the movement resumes
    h.walkers.clear();
    coord.begin_tick(1);
    let d = h.plan(&mut coord, ego, None);
    assert!(d.yield_at.is_none(), "the street did not resume: {d:?}");
}

#[test]
fn a_train_holds_the_level_crossing_until_it_clears() {
    let mut h = Harness::new(rail_crossing());
    let ego = h.add(stopper(10, 0, 95.0));
    let train = h.add(stopper(90, 3, 10.0));
    h.approach(ego, 1, 5.0);
    h.place(train, 3, 10.0);

    let mut coord = JunctionCoordinator::new();
    coord.begin_tick(0);
    let d = h.plan(&mut coord, ego, None);
    assert!(d.yield_at.is_some(), "the street crossed in front of the train: {d:?}");
    assert!(!coord.holds(VehicleId(10), 1));

    // the train clears: the street goes
    h.on_lane.remove(&3);
    coord.begin_tick(1);
    let d = h.plan(&mut coord, ego, None);
    assert!(d.yield_at.is_none(), "the street did not go after the train: {d:?}");
}
