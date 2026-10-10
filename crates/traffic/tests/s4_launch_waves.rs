//! Stage 4 exit gate — red-light queues start and discharge with credible per-driver
//! variation, and a flickering constraint does not reset the launch timer.
//!
//! Headless: a synthetic lane and the real longitudinal controller. No renderer or assets.

use glam::DVec3;
use traffic::{AiState, LaneBuilder, LaneKind, Lead, Network};

const DT: f32 = 1.0 / 50.0;
const SPACING: f32 = 6.0;
const STOP_GAP: f32 = 0.6;

fn straight() -> Network {
    let lane = LaneBuilder::polyline(
        vec![DVec3::new(0.0, 0.0, 0.0), DVec3::new(0.0, 400.0, 0.0)],
        LaneKind::Street,
        3.0,
    );
    let mut net = Network {
        lanes: vec![lane],
        ..Default::default()
    };
    net.link(1.5);
    net
}

fn car(s: f32, seed: u64, reaction: f32) -> AiState {
    let mut c = AiState::new(0, s, seed);
    c.front = 2.25;
    c.rear = 2.25;
    c.length = 4.5;
    c.max_speed_kmh = 50.0;
    c.reaction = reaction;
    c
}

#[test]
fn a_red_queue_discharges_with_varied_launch_delays() {
    let net = straight();
    let line = net.lanes[0].length();
    let reactions = [0.35f32, 0.55, 0.7, 0.85, 1.0, 1.15];
    let mut cars: Vec<AiState> = reactions
        .iter()
        .enumerate()
        .map(|(i, &r)| {
            let s = line - STOP_GAP - i as f32 * SPACING - 2.25;
            car(s, (i as u64 + 1) * 7919, r)
        })
        .collect();
    for c in cars.iter_mut() {
        c.plan_next(&net);
    }

    let mut launched: Vec<Option<f32>> = vec![None; cars.len()];
    let mut t = 0.0f32;
    let green = 10.0f32;
    for _ in 0..2500 {
        let red = t < green;
        let order = {
            let mut o: Vec<usize> = (0..cars.len()).collect();
            o.sort_by(|&a, &b| cars[b].s.partial_cmp(&cars[a].s).unwrap());
            o
        };
        for rank in 0..order.len() {
            let i = order[rank];
            let obstacle = if rank == 0 {
                None
            } else {
                let lead = &cars[order[rank - 1]];
                Some((lead.s - lead.rear) - cars[i].s)
            };
            let stop_at = if red { Some(line - cars[i].s) } else { None };
            cars[i].advance(&net, DT, obstacle, stop_at);
            if !red && launched[i].is_none() && cars[i].speed > 0.05 {
                launched[i] = Some(t - green);
            }
        }
        t += DT;
    }

    let delays: Vec<f32> = launched.iter().map(|d| d.expect("a car never launched")).collect();
    let min = delays.iter().cloned().fold(f32::MAX, f32::min);
    let max = delays.iter().cloned().fold(f32::MIN, f32::max);
    assert!(
        max - min > 0.15,
        "the queue launched as one block: delays {delays:?}"
    );
    // The front car waits out its own reaction before moving.
    assert!(
        (delays[0] - reactions[0]).abs() < 0.25,
        "the front car launched after {:.2}s, its reaction {:.2}",
        delays[0],
        reactions[0]
    );
    // And the queue actually discharges.
    let front = cars.iter().map(|c| c.s).fold(f32::MIN, f32::max);
    assert!(front > line + 40.0, "the queue did not discharge: front at {front:.1}");
}

#[test]
fn a_flickering_constraint_does_not_reset_the_launch_timer() {
    let net = straight();
    let mut c = car(0.0, 3, 0.7);
    c.plan_next(&net);
    // A leader at exactly the standstill gap holds the car with no braking.
    let hold = Lead {
        gap: c.min_gap,
        speed: 0.0,
        acc: 0.0,
    };

    // Establish the hold.
    for _ in 0..20 {
        c.drive(&net, DT, Some(hold), None);
    }
    assert!(c.held);
    assert!((c.start_timer - c.reaction).abs() < 1e-3);

    // Clear the constraint for long enough to count the timer down a little.
    for _ in 0..8 {
        c.drive(&net, DT, None, None);
        c.speed = 0.0; // keep the launch gate in play while we probe the timer
        c.s = 0.0;
    }
    let before = c.start_timer;
    assert!(before < c.reaction - 0.05, "the timer did not count down");

    // The constraint flickers back; the timer must not jump back up.
    for _ in 0..8 {
        c.drive(&net, DT, None, None);
        c.speed = 0.0;
        c.s = 0.0;
        c.drive(&net, DT, Some(hold), None);
        c.speed = 0.0;
        c.s = 0.0;
        assert!(
            c.start_timer <= before + 1e-4,
            "the launch timer was reset by a flicker: {before} -> {}",
            c.start_timer
        );
    }
}
