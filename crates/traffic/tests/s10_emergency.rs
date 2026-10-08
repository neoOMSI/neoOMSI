mod common;
use common::{Harness, four_way};
use traffic::*;

fn emergency(id: u64, lane: usize) -> JunctionActor {
    let mut a = JunctionActor::new(VehicleId(id), lane, 94.0);
    a.emergency = true;
    a.priority = true;
    a
}
fn prepare(w: &Harness, c: &mut JunctionCoordinator) {
    let ways: Vec<_> = (0..w.actors.len()).map(|i| w.way(i)).collect();
    c.prepare_emergencies(&w.net, &w.actors, &ways, &w.on_lane);
}

#[test]
fn emergency_gets_reserved_red_entry_while_normal_traffic_keeps_its_red() {
    let mut net = four_way();
    net.lanes[1].traffic_light = Some((0, 0));
    let mut w = Harness::new(net);
    w.aspects.insert((0, 0), Aspect::Red);
    let i = w.add(emergency(1, 0));
    let mut c = JunctionCoordinator::new();
    prepare(&w, &mut c);
    let d = w.plan(&mut c, i, None);
    assert!(d.light.is_none());
    assert!(d.yield_at.is_none());
    w.actors[i].emergency = false;
    w.actors[i].priority = true; // ordinary priority is insufficient
    c.release(VehicleId(1), Reason::NONE);
    prepare(&w, &mut c);
    assert!(w.plan(&mut c, i, None).light.is_some());
}

#[test]
fn crossing_traffic_waits_and_restarts_after_emergency_leaves() {
    let mut w = Harness::new(four_way());
    w.add(emergency(1, 0));
    let other = w.add(JunctionActor::new(VehicleId(2), 4, 94.0));
    let mut c = JunctionCoordinator::new();
    prepare(&w, &mut c);
    assert_eq!(
        w.plan(&mut c, other, None).binding,
        Some(Reason::EmergencyYield)
    );
    w.actors[0].emergency = false;
    prepare(&w, &mut c);
    assert_ne!(
        w.plan(&mut c, other, None).binding,
        Some(Reason::EmergencyYield)
    );
}

#[test]
fn existing_occupant_clears_before_the_emergency_is_admitted() {
    let mut w = Harness::new(four_way());
    let i = w.add(emergency(1, 0));
    let j = w.add(JunctionActor::new(VehicleId(2), 5, 10.0));
    w.place(j, 5, 10.0);
    let mut c = JunctionCoordinator::new();
    prepare(&w, &mut c);
    assert!(w.plan(&mut c, i, None).yield_at.is_some());
    assert_ne!(
        w.plan(&mut c, j, None).binding,
        Some(Reason::EmergencyYield)
    );
}

#[test]
fn two_emergency_requests_have_a_stable_winner() {
    for reverse in [false, true] {
        let mut w = Harness::new(four_way());
        let mut actors = vec![emergency(10, 0), emergency(5, 4)];
        if reverse {
            actors.reverse();
        }
        for a in actors {
            w.add(a);
        }
        let mut c = JunctionCoordinator::new();
        prepare(&w, &mut c);
        assert!(c.emergency_owns(VehicleId(5), 5));
        assert!(!c.emergency_owns(VehicleId(10), 1));
        c.invalidate_network();
        assert!(c.emergency_speed_cap(VehicleId(5)).is_none());
    }
}

#[test]
fn script_contract_distinguishes_scheduled_priority_and_explicit_override() {
    assert!(emergency_active(true, false, None));
    assert!(!emergency_active(true, true, None));
    assert!(!emergency_active(true, false, Some(false)));
    assert!(emergency_active(false, true, Some(true)));
}

#[test]
fn an_emergency_cannot_force_a_full_exit_even_after_minutes() {
    let mut w = Harness::new(common::blocked_exit());
    let mut a = emergency(1, 0);
    a.yield_time = 300.0;
    let i = w.add(a);
    let j = w.add(JunctionActor::new(VehicleId(2), 2, 1.0));
    w.place(j, 2, 1.0);
    let mut c = JunctionCoordinator::new();
    prepare(&w, &mut c);
    let d = w.plan(&mut c, i, None);
    assert!(d.yield_at.is_some());
    assert!(d.reasons.contains(&Reason::OccupiedExit));
}

#[test]
fn reservation_survives_until_the_rear_clears_even_if_warning_is_switched_off() {
    let mut w = Harness::new(common::blocked_exit());
    let i = w.add(emergency(1, 0));
    let mut c = JunctionCoordinator::new();
    prepare(&w, &mut c);
    w.actors[i].lane = 2;
    w.actors[i].s = 1.0;
    w.actors[i].emergency = false;
    prepare(&w, &mut c);
    assert!(c.emergency_owns(VehicleId(1), 1));
    w.actors[i].s = 8.0;
    prepare(&w, &mut c);
    assert!(!c.emergency_owns(VehicleId(1), 1));
}

#[test]
fn red_override_never_overrides_an_occupied_pedestrian_crossing() {
    let mut net = four_way();
    net.walks[1] = vec![(99, 10.0, 5.0)];
    net.lanes[1].traffic_light = Some((0, 0));
    let mut w = Harness::new(net);
    let i = w.add(emergency(1, 0));
    w.walkers.insert(99, vec![5.0]);
    w.aspects.insert((0, 0), Aspect::Red);
    let mut c = JunctionCoordinator::new();
    prepare(&w, &mut c);
    let d = w.plan(&mut c, i, None);
    assert!(d.light.is_none());
    assert!(d.yield_at.is_some());
    assert!(d.reasons.contains(&Reason::Pedestrian));
}

#[test]
fn articulated_emergency_keeps_reservation_until_its_last_body_clears() {
    let mut w = Harness::new(common::blocked_exit());
    let i = w.add(emergency(1, 0));
    let mut c = JunctionCoordinator::new();
    prepare(&w, &mut c);
    w.actors[i].lane = 2;
    w.actors[i].s = 20.0;
    w.actors[i].emergency = false;
    w.place(i, 1, 18.0); // rear section still on the junction
    prepare(&w, &mut c);
    assert!(c.emergency_owns(VehicleId(1), 1));
    w.on_lane.clear();
    prepare(&w, &mut c);
    assert!(!c.emergency_owns(VehicleId(1), 1));
}

#[test]
fn repeated_warning_route_and_network_changes_do_not_leak_reservations() {
    let mut c = JunctionCoordinator::new();
    for cycle in 0..5000 {
        let mut w = Harness::new(common::blocked_exit());
        let i = w.add(emergency(cycle + 1, 0));
        prepare(&w, &mut c);
        assert!(c.emergency_owns(w.actors[i].id, 1));
        if cycle % 3 == 0 {
            c.invalidate_network();
        } else if cycle % 3 == 1 {
            c.release(w.actors[i].id, Reason::Removed);
        } else {
            w.actors[i].lane = 2;
            w.actors[i].s = 20.0;
            w.actors[i].emergency = false;
            prepare(&w, &mut c);
        }
        assert!(c.emergency_speed_cap(w.actors[i].id).is_none());
    }
}
