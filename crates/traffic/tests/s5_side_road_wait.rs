//! Stage 5 — a side road's car at a main road with steady traffic.
//!
//! The south->north road has the right of way over the west approach (it comes from the
//! right). Its cars come every 3 s at 36 km/h: never a gap the waiting car takes, however
//! much shorter its patience makes the gap it wants. A driver who has waited long makes
//! himself seen; the main road's cars that can still stop comfortably let him in.

mod common;

use common::{four_way, Harness};
use traffic::{JunctionActor, JunctionCoordinator, Lead, VehicleId};

const DT: f32 = 0.05;
const HEADWAY: f32 = 3.0;
const MAIN_SPEED: f32 = 10.0;

#[derive(Clone)]
struct Car {
    a: JunctionActor,
    /// Along the main road: lane 4 (100 m) then lane 5 (20 m).
    x: f32,
}

fn main_car(id: u64) -> Car {
    let mut a = JunctionActor::new(VehicleId(id), 4, 0.0);
    a.speed = MAIN_SPEED;
    Car { a, x: 0.0 }
}

/// Speed after `DT`, stopping `stop` m ahead if given and keeping 2 m behind a car `gap` m
/// ahead at `lead` m/s.
fn step(a: &JunctionActor, stop: Option<f32>, ahead: Option<(f32, f32)>) -> f32 {
    let v = a.speed;
    let to_stop = stop.map(|d| v * v / (2.0 * (d - 0.5).max(0.05))).unwrap_or(0.0);
    let to_lead = ahead
        .map(|(gap, lead)| (v * v - lead * lead).max(0.0) / (2.0 * (gap - 2.0).max(0.05)))
        .unwrap_or(0.0);
    let brake = to_stop.max(to_lead);
    let acc = if brake > 0.5 { -brake.min(8.0) } else { a.accel };
    (v + acc * DT).clamp(0.0, MAIN_SPEED)
}

#[test]
fn a_side_road_car_gets_in_after_a_long_wait() {
    let mut side = JunctionActor::new(VehicleId(1000), 0, 100.0 - 2.25);
    side.speed = 0.0;
    // the stream is there already when the car comes to the line
    let mut mains: Vec<Car> = Vec::new();
    let mut next_id = 1;
    for x in [90.0, 60.0, 30.0] {
        let mut c = main_car(next_id);
        c.x = x;
        mains.push(c);
        next_id += 1;
    }
    let mut since_spawn = HEADWAY;
    let mut coord = JunctionCoordinator::new();
    let mut admitted = None;
    for tick in 0..(150.0 / DT) as u64 {
        let time = tick as f32 * DT;
        if since_spawn >= HEADWAY {
            mains.push(main_car(next_id));
            next_id += 1;
            since_spawn = 0.0;
        }
        since_spawn += DT;
        mains.retain(|c| c.x < 125.0);
        for c in &mut mains {
            (c.a.lane, c.a.s) = if c.x < 100.0 { (4, c.x) } else { (5, c.x - 100.0) };
        }
        // the frozen scene of this tick
        let mut h = Harness::new(four_way());
        h.time = time;
        h.tick = tick;
        let si = h.add(side.clone());
        h.place(si, 0, side.s);
        h.approach(si, 1, 100.0 - side.s);
        let mut leads = vec![None];
        for k in 0..mains.len() {
            let c = &mains[k];
            let mut a = c.a.clone();
            let lead = k.checked_sub(1).map(|j| {
                let o = &mains[j];
                (o.a.id, o.x - o.a.rear - c.x - c.a.front, o.a.speed)
            });
            a.lead_info = lead.map(|l| (l.0, l.1));
            let i = h.add(a);
            h.place(i, c.a.lane, c.a.s);
            if c.a.lane == 4 {
                h.approach(i, 5, 100.0 - c.a.s);
            }
            leads.push(lead.map(|l| Lead { gap: l.1, speed: l.2, acc: 0.0 }));
        }
        coord.begin_tick(tick);
        let mut decisions = Vec::new();
        for i in 0..h.actors.len() {
            decisions.push(h.plan(&mut coord, i, leads[i]));
        }
        if decisions[0].yield_at.is_none() {
            admitted = Some(time);
            // every main road car is through, or can still stop before the crossing
            for c in &mains {
                let to_line = 100.0 - c.x - c.a.front;
                let through = c.x - c.a.rear > 110.0 + 2.0;
                let can_stop = c.a.speed * c.a.speed / (2.0 * 3.0) <= to_line.max(0.0);
                assert!(
                    through || can_stop,
                    "the side car went in front of car {} ({:.1} m from its line at {:.1} m/s)",
                    c.a.id,
                    to_line,
                    c.a.speed
                );
            }
            break;
        }
        // move: the side car waits; the main road's cars keep their distance and stop where
        // the coordinator holds them
        side.stopped += DT;
        side.yield_time += DT;
        side.yielding = true;
        side.wait_at = decisions[0].yield_at;
        for (k, c) in mains.iter_mut().enumerate() {
            let d = &decisions[k + 1];
            let hold = d.yield_at.map(|y| y - c.a.front);
            c.a.speed = step(&c.a, hold, leads[k + 1].map(|l| (l.gap, l.speed)));
            c.x += c.a.speed * DT;
            c.a.yielding = d.yield_at.is_some();
            c.a.wait_at = d.yield_at;
            if c.a.speed < 0.3 && d.yield_at.is_some() {
                c.a.yield_time += DT;
            } else if d.yield_at.is_none() {
                c.a.yield_time = 0.0;
            }
            c.a.stopped = if c.a.speed < 0.1 { c.a.stopped + DT } else { 0.0 };
            c.a.crawl = if c.a.speed < 1.0 { c.a.crawl + DT } else { 0.0 };
        }
    }
    let t = admitted.expect("the side road's car waited for good");
    assert!(t >= 40.0, "it did not wait for a gap first: in after {t:.1} s");
}
