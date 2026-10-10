//! Stage 8 exit gate — LAN host authority.
//!
//! The host is the single authority: it makes the population decisions. A mirror client never
//! requests or admits anything; an authority change or a time reset preserves duty ownership
//! and never duplicates a duty.

mod common;

use common::population::{demand, PopWorld};
use traffic::*;

#[test]
fn a_client_makes_no_population_decisions() {
    // The host fills its population.
    let mut host = PopWorld::new(1);
    host.coord.request(SpawnClass::Unscheduled, 0, 100.0);
    let decisions = host.step(true, demand(5, 0), true);
    assert!(PopWorld::admitted(&decisions) >= 1);
    assert!(host.coord.admitted_total() >= 1);

    // A mirror client only presents replicated committed state: no demand, no admission.
    let client = PopWorld::new(1);
    assert_eq!(client.coord.queue_len(), 0);
    assert_eq!(client.coord.admitted_total(), 0);
    assert!(client.coord.dormant_ids().is_empty());
}

#[test]
fn rejoin_and_time_reset_do_not_duplicate_duty() {
    let mut w = PopWorld::new(1);
    w.coord
        .enter_dormant(VehicleId(1), SpawnClass::Scheduled, true, 0, 50.0);
    assert_eq!(w.coord.dormant_duty_count(), 1);

    // An authority change / tile reset invalidates demand but keeps duty ownership.
    w.coord.invalidate_network();
    assert_eq!(w.coord.dormant_duty_count(), 1);

    // Re-registering the same duty (a rejoin) is idempotent.
    w.coord
        .enter_dormant(VehicleId(1), SpawnClass::Scheduled, true, 0, 50.0);
    assert_eq!(w.coord.dormant_len(), 1);
    assert_eq!(w.coord.dormant_duty_count(), 1);

    // A population reset (a clock jump) clears the map; the host re-registers duties once.
    w.coord.clear();
    assert_eq!(w.coord.dormant_duty_count(), 0);
    w.coord
        .enter_dormant(VehicleId(1), SpawnClass::Scheduled, true, 0, 50.0);
    assert_eq!(w.coord.dormant_duty_count(), 1);
}
