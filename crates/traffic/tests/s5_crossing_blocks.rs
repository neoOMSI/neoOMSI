//! Stage 5 exit gate — content rules: `[crossingproblem]` and typed `[blockpath]` modes.
//!
//! Provenance: the reference parser only proves that `[crossingproblem]` is a boolean and
//! that `[blockpath] <path> <mode>` keeps a path plus a byte mode. The decision meanings
//! tested here are the documented neoOMSI Stage 5 interpretation: keep-clear entry refusal,
//! reservation refusal (`Reserve`), and oncoming commitment (`Oncoming`).

mod common;

use common::{four_way, oncoming_pair, Harness};
use traffic::{BlockRule, JunctionActor, JunctionCoordinator, VehicleId};

fn stopper(id: u64, lane: usize, s: f32) -> JunctionActor {
    let mut a = JunctionActor::new(VehicleId(id), lane, s);
    a.speed = 0.0;
    a
}

#[test]
fn a_crossing_problem_path_refuses_entry() {
    let mut h = Harness::new(four_way());
    let ego = h.add(stopper(10, 0, 95.0));
    let occupant = h.add(stopper(30, 1, 2.0));
    // a body already on the keep-clear movement lane
    h.place(occupant, 1, 2.0);

    let mut plain = JunctionCoordinator::new();
    plain.begin_tick(0);
    let d_plain = h.plan(&mut plain, ego, None);
    assert!(
        d_plain.yield_at.is_none(),
        "a body on the movement lane acted as a crossing conflict: {d_plain:?}"
    );

    // now flag the movement lane `[crossingproblem]`: the vehicle must not enter it
    h.net.lanes[1].crossing_problem = true;
    let mut flagged = JunctionCoordinator::new();
    flagged.begin_tick(0);
    let d = h.plan(&mut flagged, ego, None);
    assert!(
        d.yield_at.is_some(),
        "a keep-clear path was entered with a body on it: {d:?}"
    );
    assert!(!flagged.holds(VehicleId(10), 1));
}

#[test]
fn reserve_mode_refuses_a_reservation_that_occupy_mode_ignores() {
    // The ego has priority, so only the block rule can hold it back.
    let with_mode = |mode: u16| {
        let mut net = four_way();
        net.lanes[1].priority = 192.0;
        net.lanes[5].priority = 64.0;
        let mut h = Harness::new(net);
        // lane 1 has object path 0; the south->north lane (index 5) has path 2
        h.net.lanes[1].blocks = vec![BlockRule { path: 2, mode }];
        let ego = h.add(stopper(10, 0, 95.0));
        let other = h.add(stopper(20, 4, 95.0));
        h.approach(ego, 1, 5.0);
        h.approach(other, 5, 5.0);
        let mut coord = JunctionCoordinator::new();
        coord.begin_tick(0);
        // the other road holds a speculative reservation
        coord.restore_claim(VehicleId(20), &[5]);
        let d = h.plan(&mut coord, ego, None);
        (d, coord.holds(VehicleId(10), 1))
    };

    let (occupy, occupy_claimed) = with_mode(0);
    assert!(
        occupy.yield_at.is_none() && occupy_claimed,
        "Occupy mode should ignore a speculative claim: {occupy:?}"
    );

    let (reserve, reserve_claimed) = with_mode(1);
    assert!(
        reserve.yield_at.is_some() && !reserve_claimed,
        "Reserve mode should refuse its own reservation: {reserve:?}"
    );
}

#[test]
fn oncoming_mode_waits_for_the_other_side_to_commit() {
    let mut net = oncoming_pair();
    net.lanes[1].priority = 192.0; // the ego would otherwise have priority
    let mut h = Harness::new(net);
    h.net.lanes[1].blocks = vec![BlockRule { path: 1, mode: 2 }];
    let mut ego = stopper(10, 0, 95.0);
    ego.priority = true; // an emergency vehicle: still cannot force a committed oncoming path
    let ego = h.add(ego);
    let other = h.add(stopper(20, 2, 95.0));
    h.approach(ego, 1, 5.0);
    h.approach(other, 3, 5.0);

    let mut coord = JunctionCoordinator::new();
    coord.begin_tick(0);
    coord.restore_claim(VehicleId(20), &[3]);
    let d = h.plan(&mut coord, ego, None);
    assert!(
        d.yield_at.is_some(),
        "the oncoming commitment was ignored: {d:?}"
    );
    assert!(!coord.holds(VehicleId(10), 1));
}
