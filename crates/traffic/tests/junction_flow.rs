//! Signalled junctions in a jam (BRT Berlin, Potsdamer Straße / Warthestraße): a queue does
//! not creep across the line at red, and nobody follows a standing queue into the junction.
mod common;
use common::{Harness, four_way};
use glam::DVec3;
use traffic::*;

fn signalled() -> Harness {
    let mut net = four_way();
    net.lanes[1].traffic_light = Some((0, 0));
    Harness::new(net)
}

/// A car on the west approach `gap` m before the line, at `speed`.
fn approach(w: &mut Harness, gap: f32, speed: f32) -> usize {
    let s = 100.0 - 2.25 - gap;
    let mut a = JunctionActor::new(VehicleId(1), 0, s);
    a.speed = speed;
    let i = w.add(a);
    w.place(i, 0, s);
    i
}

#[test]
fn a_car_that_crept_up_to_the_line_on_green_stops_there_at_red() {
    let mut w = signalled();
    let i = approach(&mut w, 1.2, 2.0);
    let mut c = JunctionCoordinator::new();
    w.aspects.insert((0, 0), Aspect::Green);
    assert!(w.plan(&mut c, i, None).light.is_none());
    w.aspects.insert((0, 0), Aspect::Red);
    assert!(w.plan(&mut c, i, None).light.is_some(), "rolled on across the line at red");
}

#[test]
fn a_car_moving_on_at_the_line_still_clears_on_amber_and_red() {
    let mut w = signalled();
    let i = approach(&mut w, 1.2, 8.0);
    let mut c = JunctionCoordinator::new();
    w.aspects.insert((0, 0), Aspect::Green);
    w.plan(&mut c, i, None);
    w.aspects.insert((0, 0), Aspect::Red);
    assert!(w.plan(&mut c, i, None).light.is_none(), "braked hard on the line");
}

#[test]
fn nobody_follows_a_standing_queue_into_the_junction() {
    let mut w = Harness::new(four_way());
    let i = approach(&mut w, 8.0, 4.0);
    // the car ahead stands with its rear 3 m into the junction (its middle crossing at 10 m)
    let lead = Lead { gap: 8.0 + 3.0, speed: 0.3, acc: 0.0 };
    let d = w.plan(&mut JunctionCoordinator::new(), i, Some(lead));
    assert!(d.yield_at.is_some(), "entered behind a standing queue: {d:?}");
    assert!(d.reasons.contains(&Reason::OccupiedExit));
}

#[test]
fn a_queue_with_room_beyond_the_junction_or_moving_on_is_followed() {
    for lead in [
        // standing, but far enough past the crossing for the whole car
        Lead { gap: 8.0 + 30.0, speed: 0.0, acc: 0.0 },
        // inside the junction, but driving off
        Lead { gap: 8.0 + 3.0, speed: 5.0, acc: 0.0 },
        // waiting before the junction itself: an ordinary queue at the line
        Lead { gap: 4.0, speed: 0.0, acc: 0.0 },
    ] {
        let mut w = Harness::new(four_way());
        let i = approach(&mut w, 8.0, 4.0);
        let d = w.plan(&mut JunctionCoordinator::new(), i, Some(lead));
        assert!(!d.reasons.contains(&Reason::OccupiedExit), "{lead:?}: {d:?}");
    }
}

/// Lane 1 (light (0, 0)) -> lane 8 -> lane 9 (light `second`), all of one crossing object;
/// a car through the first light, then on lane 8.
fn second_light(second: (usize, usize)) -> (Harness, usize, JunctionCoordinator) {
    let mut net = four_way();
    let key = net.lanes[1].key;
    net.lanes[1].traffic_light = Some((0, 0));
    net.lanes[1].next = vec![8];
    for (k, (a, b)) in [(4, (10.0, 20.0)), (5, (20.0, 30.0))] {
        let mut l = common::object_lane(DVec3::new(a, 0.0, 0.0), DVec3::new(b, 0.0, 0.0), k);
        l.key = key.map(|x| LaneKey { path: k as u16, ..x });
        net.lanes.push(l);
    }
    net.lanes[8].next = vec![9];
    net.lanes[9].traffic_light = Some(second);
    net.compute_conflicts();
    let mut w = Harness::new(net);
    w.aspects.insert((0, 0), Aspect::Green);
    w.aspects.insert((0, 1), Aspect::Red);
    let mut a = JunctionActor::new(VehicleId(1), 1, 15.0);
    a.speed = 4.0;
    let i = w.add(a);
    w.place(i, 1, 15.0);
    let mut c = JunctionCoordinator::new();
    w.plan(&mut c, i, None);
    w.actors[i].lane = 8;
    w.actors[i].s = 4.0;
    w.place(i, 8, 4.0);
    (w, i, c)
}

#[test]
fn another_light_of_the_same_junction_holds_who_came_through_the_first() {
    // BRT Berlin: a signal after the pedestrian crossing's, a second stop line for the turn
    let (w, i, mut c) = second_light((0, 1));
    assert!(w.plan(&mut c, i, None).light.is_some(), "ran the junction's second light at red");
}

#[test]
fn the_light_a_car_went_through_does_not_stop_it_again_further_in() {
    // the paths after the stop line carry its light on; it turned red behind the car
    let (mut w, i, mut c) = second_light((0, 0));
    w.aspects.insert((0, 0), Aspect::Red);
    assert!(w.plan(&mut c, i, None).light.is_none(), "stopped inside the junction");
}

#[test]
fn a_car_standing_with_its_nose_over_the_line_does_not_set_off_at_red() {
    let mut w = signalled();
    let i = approach(&mut w, 1.0, 1.5);
    let mut c = JunctionCoordinator::new();
    w.aspects.insert((0, 0), Aspect::Green);
    w.plan(&mut c, i, None);
    // it stopped 0.3 m over the line; the light is red now
    let s = 100.0 - 2.25 + 0.3;
    w.actors[i].s = s;
    w.actors[i].speed = 0.0;
    w.place(i, 0, s);
    w.aspects.insert((0, 0), Aspect::Red);
    assert!(w.plan(&mut c, i, None).light.is_some(), "set off across the line at red");
}
