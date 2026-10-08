//! Passing a stopped bus remains available, but scenery, islands and passengers veto it.
mod common;
use common::maneuver::two_way;
use glam::DVec2;
use traffic::perception::SweepSample;
use traffic::*;

fn plan(
    net: &Network,
    people: &[DVec2],
    clear: &dyn Fn(&[SweepSample], &ManeuverActor) -> bool,
    queue: bool,
) -> ManeuverDecision {
    let mut actor = ManeuverActor::new(VehicleId(1), 0, 60.0);
    actor.speed = 6.0;
    actor.stopped = 5.0;
    let actors = [actor];
    let occupancy = Occupancy::build(net.version(), 1, vec![]);
    let scene = ManeuverScene {
        static_clearance: Some(clear),
        net,
        occupancy: &occupancy,
        actors: &actors,
        people,
        time: 1.0,
        dt: 0.02,
        tick: 1,
    };
    let mut inputs = ManeuverInputs::new(0);
    inputs.lead_standing = true;
    inputs.lead_gap = Some(6.0);
    inputs.obstacle_len = 12.0;
    inputs.queue_for_stop = queue;
    ManeuverCoordinator::new().plan(&scene, &mut ManeuverState::default(), &inputs)
}

#[test]
fn a_car_still_passes_a_bus_when_the_entire_way_is_clear() {
    assert_eq!(
        plan(&two_way(), &[], &|_, _| true, false).phase,
        ManeuverPhase::Passing
    );
}
#[test]
fn scenery_vetoes_the_pass_even_after_a_long_wait() {
    assert_ne!(
        plan(&two_way(), &[], &|_, _| false, false).phase,
        ManeuverPhase::Passing
    );
}
#[test]
fn a_divided_carriageway_is_not_an_oncoming_passing_lane_issue_126() {
    let mut net = two_way();
    for p in &mut net.lanes[1].points {
        p.x -= 1.5;
    }
    net.lanes[1].refresh();
    net.link(1.5);
    assert_ne!(
        plan(&net, &[], &|_, _| true, false).phase,
        ManeuverPhase::Passing
    );
}
#[test]
fn passengers_crossing_the_passing_path_are_protected() {
    let people = [DVec2::new(-3.5, 80.0)];
    assert_ne!(
        plan(&two_way(), &people, &|_, _| true, false).phase,
        ManeuverPhase::Passing
    );
}
#[test]
fn another_bus_waiting_for_its_own_berth_does_not_pass_the_queue() {
    assert_ne!(
        plan(&two_way(), &[], &|_, _| true, true).phase,
        ManeuverPhase::Passing
    );
}
