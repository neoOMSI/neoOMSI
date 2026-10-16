//! Stage 8 exit gate — overload, bounded admission and backpressure.
//!
//! Excess demand must become *diagnosed capacity-limited* demand: the queue is bounded, a
//! busy entrance retries after a growing backoff instead of stacking vehicles, and admitted
//! placements never overlap.

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
fn a_refused_entrance_is_not_asked_again_before_its_backoff() {
    let mut w = PopWorld::new(1);
    w.occupy(1000, 0, 10.0);
    let req = w
        .coord
        .request(SpawnClass::Unscheduled, 0, 10.0)
        .expect("queued");
    // (time, the retry delay a refusal gives then): doubling up to RETRY_MAX
    let mut refusals: Vec<(f32, f32)> = Vec::new();
    while w.time < 20.0 {
        let time = w.time;
        for d in w.step(false, demand(5, 0), true) {
            assert_eq!(d.request.id, req.id);
            match d.outcome {
                SpawnOutcome::Retry { after } => refusals.push((time, after)),
                other => panic!("the blocked entrance gave {other:?}"),
            }
        }
    }
    let delays: Vec<f32> = refusals.iter().map(|r| r.1).collect();
    assert_eq!(
        &delays[..5],
        &[0.5, 1.0, 2.0, 4.0, 6.0],
        "refusals {refusals:?}"
    );
    assert!(
        delays[5..].iter().all(|&d| d == RETRY_MAX),
        "refusals {refusals:?}"
    );
    for pair in refusals.windows(2) {
        let waited = pair[1].0 - pair[0].0;
        assert!(
            waited >= pair[0].1 - 0.001 && waited < pair[0].1 + 0.05,
            "asked again after {waited} s instead of {} s: {refusals:?}",
            pair[0].1
        );
    }
    assert_eq!(w.coord.queue_len(), 1, "the refused request lost its place");

    // the way clears: the request is admitted once its backoff is over, not before
    w.bodies.clear();
    let last = *refusals.last().unwrap();
    let mut admitted_at = None;
    while admitted_at.is_none() && w.time < 40.0 {
        let time = w.time;
        if PopWorld::admitted(&w.step(false, demand(5, 0), true)) == 1 {
            admitted_at = Some(time);
        }
    }
    let at = admitted_at.expect("never admitted after the way cleared");
    assert!(
        at >= last.0 + last.1 - 0.001,
        "admitted at {at} s inside the backoff {last:?}"
    );
}

#[test]
fn a_blocked_spot_does_not_hold_back_the_rest_of_its_lane() {
    let mut w = PopWorld::new(1);
    w.occupy(1000, 0, 10.0);
    w.coord.request(SpawnClass::Unscheduled, 0, 10.0);
    let first = w.step(false, demand(5, 0), true);
    assert!(matches!(first[0].outcome, SpawnOutcome::Retry { .. }));
    // further along the same lane, while the blocked spot waits out its backoff
    w.coord.request(SpawnClass::Unscheduled, 0, 200.0);
    let next = w.step(false, demand(5, 0), true);
    assert_eq!(
        next.len(),
        1,
        "the backing-off spot was examined again: {next:?}"
    );
    assert_eq!(next[0].request.s, 200.0);
    assert_eq!(next[0].outcome, SpawnOutcome::Admit);
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
