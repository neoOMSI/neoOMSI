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

#[test]
fn emergency_crawls_only_when_crossing_against_the_signal() {
    for (aspect, crawl) in [(Aspect::Green, false), (Aspect::Dark, false), (Aspect::Red, true)] {
        let mut net = four_way();
        net.lanes[1].traffic_light = Some((0, 0));
        let mut w = Harness::new(net);
        w.aspects.insert((0, 0), aspect);
        let i = w.add(emergency(1, 0));
        let mut c = JunctionCoordinator::new();
        prepare(&w, &mut c);
        assert!(c.emergency_owns(VehicleId(1), 1));
        w.plan(&mut c, i, None);
        assert_eq!(c.emergency_speed_cap(VehicleId(1)).is_some(), crawl, "{aspect:?}");
    }
}

#[test]
fn an_unsignalled_reserved_junction_is_crossed_at_normal_speed() {
    let mut w = Harness::new(four_way());
    let i = w.add(emergency(1, 0));
    let mut c = JunctionCoordinator::new();
    prepare(&w, &mut c);
    assert!(c.emergency_owns(VehicleId(1), 1));
    w.plan(&mut c, i, None);
    assert!(c.emergency_speed_cap(VehicleId(1)).is_none());
}

#[test]
fn a_car_in_front_of_the_emergency_on_its_way_leads_it_through_the_junction() {
    let mut w = Harness::new(four_way());
    let mut amb = emergency(1, 0);
    amb.s = 70.0;
    let a = w.add(amb);
    w.place(a, 0, 70.0);
    let ahead = w.add(JunctionActor::new(VehicleId(2), 0, 90.0));
    w.place(ahead, 0, 90.0);
    let mut c = JunctionCoordinator::new();
    prepare(&w, &mut c);
    assert!(c.emergency_owns(VehicleId(1), 1));
    assert_ne!(
        w.plan(&mut c, ahead, None).binding,
        Some(Reason::EmergencyYield),
        "the car in front waited for the ambulance behind it"
    );
    // crossing traffic still waits
    let cross = w.add(JunctionActor::new(VehicleId(3), 4, 94.0));
    w.place(cross, 4, 94.0);
    prepare(&w, &mut c);
    assert_eq!(w.plan(&mut c, cross, None).binding, Some(Reason::EmergencyYield));
}

/// #462: admitted cross traffic stops for the ambulance while it still can.
#[test]
fn admitted_cross_traffic_still_stops_for_an_emergency_while_it_can() {
    let mut w = Harness::new(four_way());
    let mut cross = JunctionActor::new(VehicleId(2), 4, 85.0);
    cross.speed = 8.0;
    let cross = w.add(cross);
    w.place(cross, 4, 85.0);
    let mut c = JunctionCoordinator::new();
    prepare(&w, &mut c);
    w.plan(&mut c, cross, None);
    assert!(
        !c.claims_of(VehicleId(2)).is_empty(),
        "the cross traffic was not admitted"
    );
    let amb = w.add(emergency(1, 0));
    w.place(amb, 0, 94.0);
    prepare(&w, &mut c);
    assert_eq!(
        w.plan(&mut c, cross, None).binding,
        Some(Reason::EmergencyYield)
    );
    assert!(
        c.claims_of(VehicleId(2)).is_empty(),
        "it kept its claim on the junction"
    );
    // admitted right at the line at a speed it cannot stop from: it goes, and the
    // ambulance waits for it
    let mut w = Harness::new(four_way());
    let mut late = JunctionActor::new(VehicleId(2), 4, 97.5);
    late.speed = 12.0;
    let late = w.add(late);
    w.place(late, 4, 97.5);
    let mut c = JunctionCoordinator::new();
    prepare(&w, &mut c);
    w.plan(&mut c, late, None);
    assert!(!c.claims_of(VehicleId(2)).is_empty());
    let amb = w.add(emergency(1, 0));
    w.place(amb, 0, 94.0);
    prepare(&w, &mut c);
    assert_ne!(
        w.plan(&mut c, late, None).binding,
        Some(Reason::EmergencyYield)
    );
}

/// #462: an ambulance at the end of a queue up to a junction reserves it; a gap ends that.
#[test]
fn an_emergency_at_the_end_of_a_queue_to_the_junction_reserves_it() {
    let queue = |gap: f32| {
        let mut w = Harness::new(four_way());
        let mut amb = emergency(1, 0);
        amb.s = 10.0;
        let a = w.add(amb);
        w.place(a, 0, 10.0);
        let mut s = 98.0;
        let mut id = 10;
        while s > 20.0 {
            let car = w.add(JunctionActor::new(VehicleId(id), 0, s));
            w.place(car, 0, s);
            id += 1;
            s -= if s < 60.0 { gap } else { 7.0 };
        }
        let mut c = JunctionCoordinator::new();
        prepare(&w, &mut c);
        c.emergency_owns(VehicleId(1), 1)
    };
    assert!(
        queue(7.0),
        "no reservation from the end of a standing queue 90 m long"
    );
    assert!(!queue(25.0), "a reservation across a gap in the queue");
}

/// #462: a reservation made from the end of a queue stays while the queue moves on.
#[test]
fn a_reservation_from_the_end_of_a_queue_stays_while_the_queue_moves_on() {
    let mut w = Harness::new(four_way());
    let mut amb = emergency(1, 0);
    amb.s = 10.0;
    let a = w.add(amb);
    w.place(a, 0, 10.0);
    let mut s = 98.0;
    let mut id = 10;
    while s > 20.0 {
        let car = w.add(JunctionActor::new(VehicleId(id), 0, s));
        w.place(car, 0, s);
        id += 1;
        s -= 7.0;
    }
    let mut c = JunctionCoordinator::new();
    prepare(&w, &mut c);
    assert!(c.emergency_owns(VehicleId(1), 1));
    for car in w.actors.iter_mut().skip(1) {
        car.speed = 4.0;
    }
    prepare(&w, &mut c);
    assert!(c.emergency_owns(VehicleId(1), 1), "dropped as the queue moved on");
    // without one it does not make it from so far back
    let mut fresh = JunctionCoordinator::new();
    prepare(&w, &mut fresh);
    assert!(!fresh.emergency_owns(VehicleId(1), 1));
}

/// #462: at a red light the head of the queue rolls carefully over the line for the
/// ambulance.
#[test]
fn the_head_of_the_queue_crosses_the_red_line_for_the_emergency() {
    let mut net = four_way();
    net.lanes[1].traffic_light = Some((0, 0));
    let mut w = Harness::new(net);
    w.aspects.insert((0, 0), Aspect::Red);
    let mut amb = emergency(1, 0);
    amb.s = 85.0;
    let a = w.add(amb);
    w.place(a, 0, 85.0);
    let head = w.add(JunctionActor::new(VehicleId(2), 0, 96.0));
    w.place(head, 0, 96.0);
    let mut c = JunctionCoordinator::new();
    prepare(&w, &mut c);
    assert!(c.emergency_owns(VehicleId(1), 1));
    w.plan(&mut c, a, None);
    let d = w.plan(&mut c, head, None);
    assert!(
        d.light.is_none(),
        "the head of the queue kept its red for the ambulance"
    );
    assert!(
        c.emergency_speed_cap(VehicleId(2)).is_some(),
        "it did not cross at a crawl"
    );
    // without the ambulance it keeps its red
    let mut c = JunctionCoordinator::new();
    w.actors[a].emergency = false;
    prepare(&w, &mut c);
    assert!(w.plan(&mut c, head, None).light.is_some());
}

/// #462: one junction object can hold a whole square; traffic on a way through it that
/// never meets the ambulance's does not stop for it.
#[test]
fn traffic_whose_way_never_meets_the_emergency_does_not_stop_for_it() {
    let mut net = four_way();
    // a path of the same object 60 m east of the crossing, with its own approach
    net.lanes.push(common::object_lane(
        glam::DVec3::new(60.0, -10.0, 0.0),
        glam::DVec3::new(60.0, 10.0, 0.0),
        4,
    ));
    net.lanes.push(LaneBuilder::polyline(
        vec![
            glam::DVec3::new(60.0, -110.0, 0.0),
            glam::DVec3::new(60.0, -10.0, 0.0),
        ],
        LaneKind::Street,
        3.0,
    ));
    net.lanes[9].next = vec![8];
    // ... crossed there by another path of the object: a junction of its own
    net.lanes.push(common::object_lane(
        glam::DVec3::new(50.0, 0.0, 0.0),
        glam::DVec3::new(70.0, 0.0, 0.0),
        5,
    ));
    net.compute_conflicts();
    let mut w = Harness::new(net);
    let amb = w.add(emergency(1, 0));
    w.place(amb, 0, 94.0);
    let far = w.add(JunctionActor::new(VehicleId(2), 9, 94.0));
    w.place(far, 9, 94.0);
    let cross = w.add(JunctionActor::new(VehicleId(3), 4, 94.0));
    w.place(cross, 4, 94.0);
    let mut c = JunctionCoordinator::new();
    prepare(&w, &mut c);
    assert_ne!(
        w.plan(&mut c, far, None).binding,
        Some(Reason::EmergencyYield)
    );
    assert_eq!(
        w.plan(&mut c, cross, None).binding,
        Some(Reason::EmergencyYield)
    );
}

/// A two-way road east-west through a crossing object, right-hand traffic: 0 eastbound
/// approach, 1 eastbound object lane, 2 eastbound exit; 3 westbound approach, 4 westbound
/// object lane, 5 westbound exit; 6 a side road from the north, 7 its right turn into the
/// westbound exit, 8 its way straight across; 9 a left turn from the eastbound approach
/// into the north.
fn two_way_crossing() -> Network {
    use glam::DVec3;
    let street = |a: DVec3, b: DVec3| LaneBuilder::polyline(vec![a, b], LaneKind::Street, 3.0);
    let mut net = Network {
        lanes: vec![
            street(DVec3::new(-110.0, -1.75, 0.0), DVec3::new(-10.0, -1.75, 0.0)),
            common::object_lane(DVec3::new(-10.0, -1.75, 0.0), DVec3::new(10.0, -1.75, 0.0), 0),
            street(DVec3::new(10.0, -1.75, 0.0), DVec3::new(110.0, -1.75, 0.0)),
            street(DVec3::new(110.0, 1.75, 0.0), DVec3::new(10.0, 1.75, 0.0)),
            common::object_lane(DVec3::new(10.0, 1.75, 0.0), DVec3::new(-10.0, 1.75, 0.0), 1),
            street(DVec3::new(-10.0, 1.75, 0.0), DVec3::new(-110.0, 1.75, 0.0)),
            street(DVec3::new(-1.75, 110.0, 0.0), DVec3::new(-1.75, 10.0, 0.0)),
            common::object_lane(DVec3::new(-1.75, 10.0, 0.0), DVec3::new(-10.0, 1.75, 0.0), 2),
            common::object_lane(DVec3::new(-1.75, 10.0, 0.0), DVec3::new(-1.75, -10.0, 0.0), 3),
            common::object_lane(DVec3::new(-10.0, -1.75, 0.0), DVec3::new(1.75, 10.0, 0.0), 4),
        ],
        ..Default::default()
    };
    for (from, to) in [(0, vec![1, 9]), (1, vec![2]), (3, vec![4]), (4, vec![5]), (6, vec![7]), (7, vec![5])] {
        net.lanes[from].next = to;
    }
    net.build_grid();
    net.compute_conflicts();
    net
}

/// An ambulance passing a queue across its junction drives on the oncoming side: traffic
/// turning into that side waits for it too, not only traffic meeting its own lane.
#[test]
fn traffic_turning_into_the_oncoming_side_waits_for_an_ambulance_passing_its_queue() {
    for queue in [true, false] {
        let mut w = Harness::new(two_way_crossing());
        let mut amb = emergency(1, 0);
        amb.s = 80.0;
        let a = w.add(amb);
        w.place(a, 0, 80.0);
        if queue {
            let head = w.add(JunctionActor::new(VehicleId(2), 0, 96.0));
            w.place(head, 0, 96.0);
        }
        let turn = w.add(JunctionActor::new(VehicleId(3), 6, 94.0));
        w.place(turn, 6, 94.0);
        let mut c = JunctionCoordinator::new();
        prepare(&w, &mut c);
        assert!(c.emergency_owns(VehicleId(1), 1));
        let held = w.plan(&mut c, turn, None).binding == Some(Reason::EmergencyYield);
        assert_eq!(held, queue, "a queue in front of the ambulance: {queue}");
    }
}

/// The car in front of an ambulance crosses the red line for it only on the ambulance's own
/// way through the junction: turning off, it would cross traffic that is not held.
#[test]
fn only_a_car_taking_the_emergencys_way_crosses_the_red_line_for_it() {
    let mut w = Harness::new(two_way_crossing());
    let mut amb = emergency(1, 0);
    amb.s = 85.0;
    let a = w.add(amb);
    w.place(a, 0, 85.0);
    let head = w.add(JunctionActor::new(VehicleId(2), 0, 96.0));
    w.place(head, 0, 96.0);
    let mut c = JunctionCoordinator::new();
    prepare(&w, &mut c);
    assert!(c.emergency_owns(VehicleId(1), 1));
    assert!(c.emergency_leads(VehicleId(2), &[(1, 4.0), (2, 24.0)]));
    assert!(!c.emergency_leads(VehicleId(2), &[(9, 4.0)]));
}
