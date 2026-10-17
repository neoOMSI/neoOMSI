//! Stage 5 — exit storage is held by exactly the admitted vehicles: each takes its length
//! plus gap behind the exit queue's tail, keeps it while driving through the junction and
//! gives it up once its body is on the exit.

mod common;

use common::{blocked_exit, Harness};
use traffic::{JunctionActor, JunctionCoordinator, Lead, Reason, VehicleId};

/// The exit (lane 2) of `blocked_exit` with a standing car whose rear is `rear` metres in.
fn with_tail(rear: f32) -> Harness {
    let mut h = Harness::new(blocked_exit());
    let mut tail = JunctionActor::new(VehicleId(99), 2, rear + 2.25);
    tail.speed = 0.0;
    let t = h.add(tail);
    h.place(t, 2, rear + 2.25);
    h
}

fn car(h: &mut Harness, id: u64, lane: usize, s: f32) -> usize {
    let mut a = JunctionActor::new(VehicleId(id), lane, s);
    a.speed = 0.0;
    h.add(a)
}

#[test]
fn a_car_not_admitted_leaves_the_exit_room_to_the_next() {
    // 8 m behind the tail: room for one car (4.5 m + 2 m gap)
    let mut h = with_tail(8.0);
    let queued = car(&mut h, 1, 0, 95.0);
    let next = car(&mut h, 2, 0, 90.0);
    let mut coord = JunctionCoordinator::new();
    coord.begin_tick(0);
    // the first one queues behind a car standing right in front of it: not admitted
    let stand = Lead { gap: 2.0, speed: 0.0, acc: 0.0 };
    let d = h.plan(&mut coord, queued, Some(stand));
    assert!(!coord.holds(VehicleId(1), 1), "a queued car claimed the junction: {d:?}");
    let d = h.plan(&mut coord, next, None);
    assert!(
        d.yield_at.is_none() && !d.reasons.contains(&Reason::OccupiedExit),
        "the exit room went to a car that was not admitted: {d:?}"
    );
}

#[test]
fn an_admitted_cars_exit_room_is_held_until_it_is_on_the_exit() {
    // 12.5 m behind the tail: one car fits, two (13 m) do not
    let mut h = with_tail(12.5);
    let first = car(&mut h, 1, 0, 95.0);
    let mut coord = JunctionCoordinator::new();
    coord.begin_tick(0);
    let d = h.plan(&mut coord, first, None);
    assert!(d.yield_at.is_none(), "the first car was not admitted: {d:?}");

    // next tick it drives through the junction; a second car comes up to the line
    h.actors[first].lane = 1;
    h.actors[first].s = 5.0;
    h.actors[first].speed = 5.0;
    h.place(first, 1, 5.0);
    let second = car(&mut h, 2, 0, 95.0);
    for order in [[first, second], [second, first]] {
        coord.begin_tick(1);
        for &i in &order {
            let d = h.plan(&mut coord, i, None);
            if i == second {
                assert!(
                    d.yield_at.is_some() && d.reasons.contains(&Reason::OccupiedExit),
                    "the exit room was promised twice: {d:?}"
                );
            }
        }
    }

    // then it stands on the exit, 1 m behind the tail: 7 m are left, enough for the second
    h.on_lane.get_mut(&1).unwrap().clear();
    h.actors[first].lane = 2;
    h.actors[first].s = 12.5 - 1.0 - 2.25;
    h.actors[first].speed = 0.0;
    h.place(first, 2, 12.5 - 1.0 - 2.25);
    coord.begin_tick(2);
    let d = h.plan(&mut coord, second, None);
    assert!(
        d.yield_at.is_none(),
        "the slot of a car on the exit still counted against it: {d:?}"
    );
}
