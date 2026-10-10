//! Stage 8 exit gate — overload, bounded admission and backpressure.
//!
//! Excess demand must become *diagnosed capacity-limited* demand: the queue is bounded, a
//! busy entrance retries instead of stacking vehicles, and admitted placements never overlap.

mod common;

use common::population::{body, demand, PopWorld};
use traffic::*;

#[test]
fn overload_is_capacity_limited_and_the_queue_stays_bounded() {
    let mut w = PopWorld::new(1);
    for i in 0..300 {
        w.coord
            .request(SpawnClass::Unscheduled, 0, (i % 380) as f32 + 5.0);
    }
    assert!(
        w.coord.queue_len() <= QUEUE_MAX,
        "queue {} over bound",
        w.coord.queue_len()
    );
    let decisions = w.step(true, demand(3, 0), true);
    let admits = PopWorld::admitted(&decisions);
    assert!(admits <= 3, "admitted {admits} beyond the target");
    assert!(
        w.coord.capacity_denied() >= 1,
        "overload was not diagnosed as capacity-limited"
    );
}

#[test]
fn a_busy_entrance_is_backpressured_not_stacked() {
    let mut w = PopWorld::new(1);
    // Fill the lane densely: no gap is open anywhere near the requests.
    for i in 0..100 {
        w.occupy(1000 + i, 0, i as f32 * 2.0);
    }
    for i in 0..20 {
        w.coord
            .request(SpawnClass::Unscheduled, 0, 5.0 + i as f32);
    }
    let decisions = w.step(false, demand(50, 0), true);
    assert_eq!(PopWorld::admitted(&decisions), 0);
    assert!(
        decisions
            .iter()
            .all(|d| matches!(d.outcome, SpawnOutcome::Retry { .. })),
        "a blocked entrance must retry, not deny or stack"
    );
    assert!(w.coord.queue_len() <= 20);
}

#[test]
fn admitted_placements_never_overlap() {
    let mut w = PopWorld::new(1);
    let target = 40usize;
    let mut placed: Vec<(usize, f32)> = Vec::new();
    for pass in 0..40 {
        let have = placed.len();
        for k in 0..target.saturating_sub(have) {
            if !w.coord.has_room() {
                break;
            }
            let s = (((pass * 37 + k * 53 + have * 11) % 380) as f32) + 5.0;
            w.coord.request(SpawnClass::Unscheduled, 0, s);
        }
        let decisions = w.step(true, demand(target, placed.len()), true);
        for d in decisions {
            if d.outcome == SpawnOutcome::Admit {
                let (lane, s) = (d.request.lane, d.request.s);
                w.bodies.push(body(9000 + placed.len() as u64, lane, s));
                placed.push((lane, s));
            }
        }
    }
    assert!(!placed.is_empty(), "nothing was ever admitted");
    for i in 0..placed.len() {
        for j in (i + 1)..placed.len() {
            let (l1, s1) = placed[i];
            let (l2, s2) = placed[j];
            let dx = (l1 as f64 - l2 as f64) * 4.0;
            let dy = (s1 - s2) as f64;
            assert!(
                (dx * dx + dy * dy).sqrt() >= GAP_MARGIN - 0.001,
                "placements {i} and {j} overlap"
            );
        }
    }
    assert!(w.coord.queue_len() <= QUEUE_MAX);
}
