mod common;
use common::maneuver::two_way;
use traffic::*;

#[test]
fn cars_on_the_emergency_route_react_but_parallel_roads_do_not() {
    let net = two_way();
    let drive = EmergencyDrive {
        vehicle: VehicleId(9),
        way: vec![(0, -20.0)],
        front: 2.5,
        speed: 12.0,
    };
    let own = AiState::new(0, 60.0, 1);
    assert!(approaching_emergency(&net, VehicleId(1), &own, 2.0, &[drive.clone()]).is_some());
    let other = AiState::new(1, 60.0, 1);
    assert!(approaching_emergency(&net, VehicleId(1), &other, 2.0, &[drive.clone()]).is_none());
    assert!(approaching_emergency(&net, VehicleId(9), &own, 2.0, &[drive]).is_none());
}

#[test]
fn courtesy_moves_within_lane_bounds_and_mirrors_on_left_hand_maps() {
    for left_hand in [false, true] {
        let mut net = two_way();
        net.left_hand = left_hand;
        let mut actor = ManeuverActor::new(VehicleId(1), 0, 60.0);
        actor.half_width = 0.9;
        actor.speed = 10.0;
        let occupancy = Occupancy::build(net.version(), 1, vec![]);
        let scene = ManeuverScene {
            static_clearance: None,
            net: &net,
            occupancy: &occupancy,
            actors: &[actor],
            people: &[],
            time: 1.0,
            dt: 0.02,
            tick: 1,
        };
        let mut inputs = ManeuverInputs::new(0);
        inputs.emergency = Some(EmergencyApproach {
            vehicle: VehicleId(9),
            gap: 30.0,
        });
        let mut c = ManeuverCoordinator::new();
        let mut state = ManeuverState::default();
        let decision = c.plan(&scene, &mut state, &inputs);
        let lat = decision.lateral_target.unwrap();
        assert!(lat.abs() <= 0.6);
        assert_eq!(lat < 0.0, left_hand);
        assert!(decision.accel_cap.unwrap() < 0.0);
        assert_eq!(decision.binding, Some(Reason::EmergencyYield));
        inputs.emergency = None;
        assert_eq!(
            c.plan(&scene, &mut state, &inputs).lateral_target,
            Some(0.0)
        );
    }
}

#[test]
fn scenery_can_veto_courtesy_without_erasing_ordinary_stop_constraints() {
    let net = two_way();
    let actors = [ManeuverActor::new(VehicleId(1), 0, 60.0)];
    let occupancy = Occupancy::build(net.version(), 1, vec![]);
    let scene = ManeuverScene {
        static_clearance: Some(&|_, _| false),
        net: &net,
        occupancy: &occupancy,
        actors: &actors,
        people: &[],
        time: 1.0,
        dt: 0.02,
        tick: 1,
    };
    let mut inputs = ManeuverInputs::new(0);
    inputs.emergency = Some(EmergencyApproach {
        vehicle: VehicleId(9),
        gap: 30.0,
    });
    let decision = ManeuverCoordinator::new().plan(&scene, &mut ManeuverState::default(), &inputs);
    assert!(decision.lateral_target.is_none());
    assert!(decision.change.is_none());
}
