//! Stage 5 exit gate — blocked downstream exit and recovery.
//!
//! A short exit lane is fully occupied by a stopped vehicle. The entering vehicle must wait
//! outside the conflict for as long as the blocker stands: waiting duration never grants
//! entry (there is no `GRIDLOCK_WAIT` timer any more). When the blocker is removed, the
//! admitted flow resumes within a bounded number of ticks, and it then holds the claim.

mod common;

use common::{blocked_exit, Harness};
use traffic::{JunctionActor, JunctionCoordinator, VehicleId};

const DT: f32 = 0.02;

fn stopper(id: u64, lane: usize, s: f32) -> JunctionActor {
    let mut a = JunctionActor::new(VehicleId(id), lane, s);
    a.speed = 0.0;
    a
}

#[test]
fn waiting_never_grants_entry_and_removal_resumes_flow() {
    let mut h = Harness::new(blocked_exit());
    let ego = h.add(stopper(1, 0, 95.0)); // approach, 5 m from the object lane
    let blocker = h.add(stopper(99, 2, 0.0)); // stopped on the exit lane
    h.place(blocker, 2, 0.0);

    let mut coord = JunctionCoordinator::new();

    // The blocker stands for a full minute: the entry is never granted, however long the wait.
    for tick in 0..3000u64 {
        h.tick = tick;
        h.time = tick as f32 * DT;
        h.actors[ego].yield_time = h.time;
        coord.begin_tick(tick);
        let d = h.plan(&mut coord, ego, None);
        assert!(
            d.yield_at.is_some(),
            "the vehicle entered a full exit at t={:.1}: {d:?}",
            h.time
        );
        assert!(
            !coord.holds(VehicleId(1), 1),
            "the vehicle reserved the junction despite the full exit at t={:.1}",
            h.time
        );
    }

    // Remove the blocker: the same vehicle is admitted and claims its way through.
    h.on_lane.remove(&2);
    h.tick = 3000;
    h.time = 60.0;
    coord.begin_tick(3000);
    let d = h.plan(&mut coord, ego, None);
    assert!(
        d.yield_at.is_none(),
        "the vehicle did not resume once the exit cleared: {d:?}"
    );
    assert!(
        coord.holds(VehicleId(1), 1),
        "the admitted vehicle did not claim its junction lane"
    );
}
