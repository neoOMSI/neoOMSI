//! Stage 9 accelerated soak: a dense, prolonged mixed churn over every owner, asserting that
//! each coordinator releases exactly what it hands out and that the bounded queues stay
//! bounded. No renderer and no OMSI assets; runs far faster than the wall-clock-locked
//! offscreen loop, so the full 60-minute simulation runs in seconds.
//!
//! The short test runs in the normal suite. The 60-minute run is `#[ignore]`d; run it with
//! `cargo test -p traffic --test s9_soak -- --ignored`.

mod common;

use common::maneuver::{Car, ManeuverWorld};
use common::population::{self, PopWorld};
use common::service::{berth, Bus, ServiceWorld};
use traffic::{
    JunctionActor, JunctionCoordinator, Reason, SpawnClass, VehicleId, QUEUE_MAX,
};

/// A short churn used by the always-on regression test (~100 s of simulation per owner).
const SHORT_TICKS: u64 = 5_000;
/// The release-gate soak: 60 minutes at the 50 Hz fixed clock.
const SOAK_TICKS: u64 = 180_000;

/// Rotate vehicles through a four-way junction, releasing claims as vehicles leave, and
/// invalidate the network periodically (a tile reload). No claim may outlive its vehicle.
fn junction_churn(ticks: u64) {
    let lanes = [0usize, 2, 4, 6];
    let mut h = common::Harness::new(common::four_way());
    let mut coord = JunctionCoordinator::new();
    let mut next_id = 1u64;
    let mut live: Vec<VehicleId> = Vec::new();
    for k in 0..32u64 {
        let id = VehicleId(next_id);
        next_id += 1;
        let lane = lanes[(k as usize) % 4];
        let s = 50.0 + (k as f32) * 3.0;
        let i = h.add(JunctionActor::new(id, lane, s));
        h.place(i, lane, s);
        live.push(id);
    }
    for t in 1..=ticks {
        for i in 0..h.actors.len() {
            let _ = h.plan(&mut coord, i, None);
        }
        assert!(
            coord.commitment_count() <= h.actors.len(),
            "more junction commitments than vehicles"
        );
        if t % 200 == 0 {
            for _ in 0..3 {
                if let Some(id) = live.pop() {
                    coord.release(id, Reason::Removed);
                }
            }
            for _ in 0..3 {
                let id = VehicleId(next_id);
                next_id += 1;
                let k = next_id as usize;
                let lane = lanes[k % 4];
                let s = 50.0 + ((k % 10) as f32) * 3.0;
                let i = h.add(JunctionActor::new(id, lane, s));
                h.place(i, lane, s);
                live.push(id);
            }
        }
        if t % 5_000 == 0 {
            // A streamed tile reload invalidates every commitment made against the old
            // network; the coordinator must drop them all and rebuild from nothing.
            coord.invalidate_network();
            assert_eq!(coord.commitment_count(), 0, "claims survived network invalidation");
        }
    }
    for id in &live {
        coord.release(*id, Reason::Removed);
    }
    coord.begin_tick(ticks);
    assert_eq!(coord.commitment_count(), 0, "junction claims leaked");
}

/// Keep demand saturated against the bounded admission queue, force retries, then stop the
/// demand and check the queue drains rather than growing without bound.
fn population_churn(ticks: u64) {
    let mut w = PopWorld::new(6);
    for t in 0..ticks {
        for k in 0..8 {
            let lane = (k as usize) % 6;
            let s = ((t as f32 * 0.1) % 350.0) + 10.0;
            let _ = w.coord.request(SpawnClass::Unscheduled, lane, s);
        }
        let visible = t % 3 != 0;
        let _ = w.step(false, population::demand(40, 0), visible);
        assert!(w.coord.queue_len() <= QUEUE_MAX, "admission queue grew past its bound");
    }
    // Demand stops: every pending request is denied for capacity and leaves the queue.
    let mut guard = 0u32;
    while w.coord.queue_len() > 0 {
        let _ = w.step(false, population::demand(0, 0), true);
        guard += 1;
        assert!(guard < 10_000, "pending queue never drained");
    }
    w.coord.invalidate_network();
    w.coord.clear();
    assert_eq!(w.coord.queue_len(), 0);
    assert_eq!(w.coord.dormant_len(), 0);
}

/// Share one berth with a bounded stream of buses; at most one berth owner, and every berth
/// is released when its bus leaves.
fn service_churn(ticks: u64) {
    let mut w = ServiceWorld::new();
    for t in 0..ticks {
        if t % 300 == 0 && w.buses.len() < 12 {
            let id = 1000 + t;
            w.add(Bus::new(id, 0.0, vec![berth(1, 100.0, 0.0)]));
        }
        w.step();
        assert!(w.coord.berth_count() <= 1, "more than one berth owner at a one-berth stop");
        let done: Vec<VehicleId> = w.buses.iter().filter(|b| b.s > 220.0).map(|b| b.id).collect();
        for id in done {
            w.coord.release(id);
            w.buses.retain(|b| b.id != id);
        }
    }
    for b in w.buses.iter() {
        w.coord.release(b.id);
    }
    w.coord.invalidate_network();
    assert_eq!(w.coord.berth_count(), 0, "berths leaked");
    assert_eq!(w.coord.queued_count(), 0, "service queue leaked");
}

/// Keep simultaneous lane-change wishes flowing; the approved set is bounded by the vehicle
/// count and is cleared by a network invalidation.
fn maneuver_churn(ticks: u64) {
    let mut w = ManeuverWorld::new(common::maneuver::two_lanes());
    for k in 0..40u64 {
        let mut c = Car::new(k + 1, 0, 20.0 + k as f32 * 6.0).speed(8.0);
        if k % 2 == 0 {
            c = c.change_to(1, 0);
        }
        w.add(c);
    }
    for _ in 0..ticks {
        w.step();
        assert!(w.coord.active_count() <= w.cars.len(), "maneuver set grew past its vehicles");
    }
    w.coord.invalidate_network();
    assert_eq!(w.coord.active_count(), 0, "maneuver commitments leaked");
}

#[test]
fn short_mixed_churn_soak_releases_everything() {
    junction_churn(SHORT_TICKS);
    population_churn(SHORT_TICKS);
    service_churn(SHORT_TICKS);
    maneuver_churn(SHORT_TICKS);
}

#[test]
#[ignore = "60-minute dense mixed soak (180k ticks); run with cargo test -- --ignored"]
fn sixty_minute_dense_mixed_soak_has_no_leak() {
    junction_churn(SOAK_TICKS);
    population_churn(SOAK_TICKS);
    service_churn(SOAK_TICKS);
    maneuver_churn(SOAK_TICKS);
}
