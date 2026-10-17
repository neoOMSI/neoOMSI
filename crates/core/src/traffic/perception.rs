//! Per-tick perception adapter: realized footprints, body/obstacle geometry and
//! route-relative observation helpers feeding the domain coordinators.

use super::*;

/// How much farther a vehicle on rails looks for the one ahead: a long train is found at
/// its front, that far beyond its tail.
const RAIL_CONSIST_REACH: f32 = 250.0;

fn body_part_from_obb(parent: &BodyFootprint, part: u16, box_: &Obb) -> BodyFootprint {
    // Obb::from_box has already converted the vehicle's degrees to radians.
    let heading = box_.heading;
    parent.part_of(
        part,
        box_.center,
        DVec2::new(heading.sin(), heading.cos()),
        box_.half.y,
        box_.half.x,
    )
}

impl Traffic {

    /// Nearest vehicle ahead of `s` on `lane` along `plan` (else the first `next`), up to
    /// `look` m: (distance to its rear, its speed, its index). One far enough beside the
    /// middle to be passed does not count; one coming the other way round an obstacle does.
    pub(crate) fn obstacle_from(
        &self,
        i: usize,
        lane: usize,
        s: f32,
        plan: Option<&[usize]>,
        look: f32,
        by_lane: &HashMap<usize, Vec<(usize, f32, f32, bool)>>,
    ) -> Option<(f32, f32, usize)> {
        let me = &self.cars[i];
        let mut best: Option<(f32, f32, usize)> = None;
        let mut lane = lane;
        let mut offset = 0.0f32;
        let mut s_from = s;
        let mut upcoming = plan.map(|p| p.iter().copied());
        // (as many lanes as fit into `look`: a junction is a string of short ones)
        for _ in 0..10 {
            if let Some(list) = by_lane.get(&lane) {
                for &(j, os, lat, foreign) in list {
                    if j == i || os <= s_from {
                        continue;
                    }
                    let o = &self.cars[j];
                    // Two overlapping cars each found the other ahead and waited for good: one
                    // that already follows this car and is behind it is not its lead.
                    if o.lead_car == Some(me.id) {
                        let h = me.vehicle.heading.to_radians();
                        let fwd = DVec2::new(h.sin(), h.cos());
                        if (o.vehicle.position - me.vehicle.position)
                            .truncate()
                            .dot(fwd)
                            < 0.0
                        {
                            continue;
                        }
                    }
                    // (this car's lateral place when it gets there, so pulling out clears it)
                    let mine = me.state.lateral_ahead(offset + (os - s_from));
                    if !foreign && (lat - mine).abs() > me.half_width + o.half_width + 0.1 {
                        continue;
                    }
                    let (d, v) = if foreign {
                        (offset + (os - s_from) - o.state.front, 0.0)
                    } else {
                        (offset + (os - s_from) - o.state.rear, o.state.speed)
                    };
                    if best.map(|b| d < b.0).unwrap_or(true) {
                        best = Some((d.max(0.0), v, j));
                    }
                }
            }
            let l = &self.net.lanes[lane];
            offset += l.length() - s_from;
            let reach = if l.kind == LaneKind::Rail {
                look + RAIL_CONSIST_REACH
            } else {
                look
            };
            if offset > reach || best.is_some() {
                break;
            }
            let next = match upcoming.as_mut() {
                Some(u) => u.next(),
                None => l.next.first().copied(),
            };
            match next {
                Some(n) => {
                    lane = n;
                    s_from = 0.0;
                }
                None => break,
            }
        }
        best.filter(|d| d.0 < look)
    }


    /// The vehicle car `i` follows: (gap from its front bumper, its speed, its index) along
    /// its lane chain (up to `look` m); during a lane change the target lane counts too.
    pub(crate) fn obstacle_ahead(
        &self,
        i: usize,
        look: f32,
        by_lane: &HashMap<usize, Vec<(usize, f32, f32, bool)>>,
    ) -> Option<(Lead, usize)> {
        let me = &self.cars[i].state;
        // once well over into the new lane, what stands in the old one no longer matters
        let committed = me
            .change
            .map(|c| c.t > 0.4 || (c.bypass && c.wait <= 0.0))
            .unwrap_or(false);
        let plan: Vec<usize> = me.upcoming().collect();
        let mut best = if committed {
            None
        } else {
            self.obstacle_from(i, me.lane, me.s, Some(&plan), look, by_lane)
        };
        if let Some(c) = me.change {
            // along the way it has chosen from the new lane
            if let Some(o) =
                self.obstacle_from(i, c.to, c.s_to, Some(&me.change_plan), look, by_lane)
            {
                if best.map(|b| o.0 < b.0).unwrap_or(true) {
                    best = Some(o);
                }
            }
        }
        // Merging: a car on another lane leading into our next lane that reaches the joint
        // first goes first; this car keeps behind it as if it were ahead in its own lane.
        if !committed {
            let mut before = self.net.lanes[me.lane].length() - me.s;
            let mut from = me.lane;
            for next in me.upcoming().take(2) {
                if before > look {
                    break;
                }
                // only near the joint (far upstream a bus on the right slowed the free lane)
                let merge_reach = me.speed * me.speed / (2.0 * me.decel.max(0.5))
                    + me.speed * me.reaction + me.min_gap + 5.0;
                if before - me.front > merge_reach {
                    before += self.net.lanes[next].length();
                    from = next;
                    continue;
                }
                for &f in self.net.prev.get(next).map(|v| v.as_slice()).unwrap_or(&[]) {
                    // (two paths of one junction meeting: `junction_stop` sorts that out)
                    if f == from
                        || self.net.crossings[from]
                        .iter()
                        .any(|c| c.other == f && c.merge)
                    {
                        continue;
                    }
                    if before - me.front < 0.5 {
                        continue; // this car is at the joint already
                    }
                    for &(j, os, _, foreign) in by_lane.get(&f).map(|v| v.as_slice()).unwrap_or(&[])
                    {
                        let other = &self.cars[j];
                        if j == i
                            || foreign
                            || other.state.lane != f
                            || other.state.planned_next != Some(next)
                        {
                            continue;
                        }
                        let theirs = self.net.lanes[f].length() - os;
                        if theirs < -2.0 {
                            continue;
                        }
                        // a near tie goes to the one already let in (no flipping per frame)
                        let t_me = (before - me.front).max(0.0) / me.speed.max(1.0);
                        let dist_them = (theirs - other.state.front).max(0.0);
                        let t_them = if other.yielding || other.light_hold
                            || (other.at_stop() && other.state.speed < 0.3) {
                            f32::MAX
                        } else if other.state.speed < 0.5 {
                            time_to(dist_them, 0.0, other.state.accel) + other.state.reaction
                        } else {
                            dist_them / other.state.speed
                        };
                        let kept = self.cars[i].merge_after == Some(other.id);
                        let first = t_them < t_me - 0.4
                            || (kept && t_them < t_me + 1.0)
                            || ((t_them - t_me).abs() <= 0.4
                            && !kept
                            && other.merge_after != Some(self.cars[i].id)
                            && other.id < self.cars[i].id);
                        if first {
                            // behind it at the joint; until it is past, wait at the joint
                            let d = (before - theirs) - other.state.rear;
                            let (d, v) = if d >= 0.0 {
                                (d, other.state.speed)
                            } else {
                                ((before - 1.0).max(0.0), 0.0)
                            };
                            if best.map(|b| d < b.0).unwrap_or(true) {
                                best = Some((d, v, j));
                            }
                        }
                    }
                }
                before += self.net.lanes[next].length();
                from = next;
            }
        }
        best.map(|(d, v, j)| {
            (
                Lead {
                    gap: d - me.front,
                    speed: v,
                    acc: if v > 0.1 { self.cars[j].state.acc } else { 0.0 },
                },
                j,
            )
        })
    }


    /// Is car `i`'s lead an obstacle that stays put (a bus serving its stop, a broken-down
    /// car, the player's bus waiting)?
    pub(crate) fn standing_obstacle(
        &self,
        i: usize,
        lead: Option<(Lead, Option<usize>)>,
        parked_ahead: bool,
        player_standing: f32,
    ) -> bool {
        let Some((l, who)) = lead else { return false };
        if l.speed.abs() > 0.3 {
            return false;
        }
        match who {
            Some(j) if j < self.cars.len() => {
                let o = &self.cars[j];
                // a bus at its stop, a car long stood with nothing holding it, or the end of
                // a queue behind a bus at its stop
                o.standing_for(self.day_time) > 4.0
                    || (o.stopped > 25.0 && !o.held && self.cars[i].stopped > 6.0)
                    || (o.stopped > 3.0 && self.standing_queue(j).1)
            }
            Some(_) => player_standing > 10.0,
            None => parked_ahead,
        }
    }


    /// The queue standing nose to tail from car `j` on: its length (m), and whether a bus
    /// serving its stop heads it (then the cars behind may pass it as a whole).
    pub(crate) fn standing_queue(&self, j: usize) -> (f32, bool) {
        let mut k = j;
        let mut len = self.cars[j].state.front + self.cars[j].state.rear;
        let mut long = self.cars[j].standing_for(self.day_time) > 4.0;
        for _ in 0..8 {
            if long {
                break;
            }
            let Some((id, gap)) = self.cars[k].lead_info else {
                break;
            };
            let Some(&n) = self.index_of.get(&id) else {
                break;
            };
            let o = &self.cars[n];
            if gap > 8.0 || o.state.speed > 0.3 || o.light_hold || o.yielding {
                break;
            }
            len += gap.max(0.0) + o.state.front + o.state.rear;
            long = o.standing_for(self.day_time) > 4.0;
            k = n;
        }
        (len, long)
    }


    #[allow(clippy::too_many_arguments)]

    /// Where car `i` has to stop for somebody on foot (its front's distance from its origin):
    /// anybody in the strip it is about to sweep, or stepping into it by the time it gets
    /// there. A timetable bus ignores people waiting at the kerb unless well inside its path.
    /// `past_standing`: the car edges past standing passengers who step aside
    /// (`PEOPLE_PATIENCE`); the third value says whether the person stopping it is one.
    pub(crate) fn people_stop(
        &self,
        i: usize,
        way: &[(usize, f32)],
        past_standing: bool,
    ) -> Option<(f32, DVec2, bool)> {
        if self.people.is_empty() {
            return None;
        }
        let car = &self.cars[i];
        let st = &car.state;
        let v = st.speed.max(0.0);
        // as far as the car needs to stop without a jolt, and never less than a car length
        let reach = (v * v / 5.0 + v + 6.0).clamp(8.0, 45.0);
        let origin = car.vehicle.position.truncate();
        let max_dist = (st.front + reach) as f64 + 6.0;
        let max_dist_sq = max_dist * max_dist;
        let (min_gx, min_gy) = people_cell(origin - DVec2::splat(max_dist));
        let (max_gx, max_gy) = people_cell(origin + DVec2::splat(max_dist));
        let mut near_buf = [0usize; 32];
        let mut near_overflow = Vec::new();
        let mut near_len = 0;
        for gy in min_gy..=max_gy {
            for gx in min_gx..=max_gx {
                let Some(list) = self.people_grid.get(&(gx, gy)) else {
                    continue;
                };
                for &idx in list {
                    if (self.people[idx].0 - origin).length_squared() >= max_dist_sq {
                        continue;
                    }
                    if near_len < near_buf.len() {
                        near_buf[near_len] = idx;
                    } else {
                        if near_overflow.is_empty() {
                            near_overflow.extend_from_slice(&near_buf);
                        }
                        near_overflow.push(idx);
                    }
                    near_len += 1;
                }
            }
        }
        if near_len == 0 {
            return None;
        }
        // in `people` order, as the full scan visited them
        let near: &[usize] = if near_len <= near_buf.len() {
            near_buf[..near_len].sort_unstable();
            &near_buf[..near_len]
        } else {
            near_overflow.sort_unstable();
            &near_overflow
        };
        let from = st.front - 1.0;
        let mut first = true;
        for &(l, dl) in way {
            let lane = &self.net.lanes[l];
            let len = lane.length();
            let mut s = (from - dl).max(0.0);
            while s <= len {
                let d = dl + s;
                if d > st.front + reach {
                    return None;
                }
                let (q, h) = lane.at(s);
                let hr = (h as f64).to_radians();
                let right = DVec2::new(hr.cos(), -hr.sin());
                let fwd = DVec2::new(hr.sin(), hr.cos());
                // the car's own offset from the lane counts on the lane it is on
                let lat = if first { st.lateral as f64 } else { 0.0 };
                let c = q.truncate() + right * lat;
                // when the car's front gets here, at most two seconds on
                let t = (((d - st.front).max(0.0)) / v.max(1.0)).min(2.0) as f64;
                for &idx in near {
                    let (p, pv, waiting, steps_aside) = &self.people[idx];
                    let standing = *steps_aside && pv.length() < PEOPLE_STANDING_SPEED;
                    if past_standing && standing {
                        continue;
                    }
                    let half = if *waiting && car.is_bus() {
                        car.half_width as f64 - 0.3
                    } else {
                        car.half_width as f64 + 0.3
                    };
                    for at in [*p, *p + *pv * t] {
                        let rel = at - c;
                        if rel.dot(fwd).abs() <= 0.55 && rel.dot(right).abs() < half {
                            return Some((d - 1.5, *p, standing));
                        }
                    }
                }
                s += 1.0;
            }
            first = false;
        }
        None
    }


    pub(crate) fn way_lanes_into(&self, st: &AiState, within: f32, out: &mut Vec<(usize, f32)>) {
        out.clear();
        if let Some(c) = st.change {
            // over on the new lane: its distances count from the same place
            out.push((c.to, -c.s_to));
            let mut d = self.net.lanes[c.to].length() - c.s_to;
            for &l in &st.change_plan {
                if d > within {
                    break;
                }
                out.push((l, d));
                d += self.net.lanes[l].length();
            }
            return;
        }
        out.push((st.lane, -st.s));
        let mut d = self.net.lanes[st.lane].length() - st.s;
        for l in st.upcoming() {
            if d > within {
                break;
            }
            out.push((l, d));
            d += self.net.lanes[l].length();
        }
    }

    pub(crate) fn way_lanes(&self, st: &AiState, within: f32) -> Vec<(usize, f32)> {
        let mut out = Vec::new();
        self.way_lanes_into(st, within, &mut out);
        out
    }

    /// Prefix of a precomputed route lookahead that falls within `within` metres.
    pub(crate) fn way_prefix(way: &[(usize, f32)], within: f32) -> &[(usize, f32)] {
        if way.is_empty() {
            return way;
        }
        let count = 1 + way[1..].iter().take_while(|&&(_, d)| d <= within).count();
        &way[..count]
    }


    /// Build the frozen junction view for car `i` and let the coordinator decide (it stays
    /// the sole writer of junction claims).
    #[allow(clippy::too_many_arguments)]
    pub(crate) fn junction_plan(
        &mut self,
        i: usize,
        way: &[(usize, f32)],
        lead: Option<Lead>,
        by_lane: &HashMap<usize, Vec<(usize, f32, f32, bool)>>,
        coming: &HashMap<usize, Vec<(usize, f32)>>,
        walkers: &HashMap<usize, Vec<f32>>,
        actors: &[JunctionActor],
        aspects: &HashMap<(usize, usize), Aspect>,
    ) -> JunctionDecision {
        let movement = if self.net.lanes[self.cars[i].state.lane].kind == LaneKind::Air {
            None
        } else {
            junction_ahead(&self.net, way)
        };
        let scene = JunctionScene {
            net: &self.net,
            actors,
            index_of: &self.index_of,
            on_lane: by_lane,
            coming,
            walkers,
            geo_prev: &self.geo_prev,
            aspects,
            time: self.time,
            tick: (self.time * 1000.0).max(0.0) as u64,
        };
        self.junctions.plan(&scene, i, way, lead, movement)
    }


    /// Two cars that have each other for their lead would wait for good: the one further
    /// along (same lane; else the lower id) ignores the other for a few seconds.
    pub(crate) fn break_lead_pairs(&mut self) {
        let index: HashMap<VehicleId, usize> = self
            .cars
            .iter()
            .enumerate()
            .map(|(k, c)| (c.id, k))
            .collect();
        let mut pairs: Vec<(usize, VehicleId)> = Vec::new();
        for c in &self.cars {
            let Some(bid) = c.lead_car else { continue };
            // each pair once, by stable id so a reorder cannot change the outcome
            if c.id > bid {
                continue;
            }
            let Some(&b) = index.get(&bid) else { continue };
            if self.cars[b].lead_car != Some(c.id) {
                continue;
            }
            let a = index[&c.id];
            let (sa, sb) = (&self.cars[a].state, &self.cars[b].state);
            let a_goes = if sa.lane == sb.lane {
                sa.s > sb.s
            } else {
                true
            };
            let (go, other) = if a_goes { (a, bid) } else { (b, c.id) };
            pairs.push((go, other));
        }
        for (go, other) in pairs {
            if ::legacy_config::env::var_os("OMSI_DEBUG_STUCK").is_some() {
                log::info!(
                    "t={:.1}: cars {} and {} each waited for the other: {} drives off",
                    self.time,
                    self.cars[go].id,
                    other,
                    self.cars[go].id
                );
            }
            self.cars[go].ignore_lead = Some((other, self.time as f64 + 5.0));
        }
    }


    /// The footprints of all AI vehicles, rear sections and trailers included.
    pub(crate) fn footprints(&self) -> Vec<Footprint> {
        let mut out = Vec::with_capacity(self.cars.len() + 8);
        for (i, c) in self.cars.iter().enumerate() {
            let st = &c.state;
            let h = c.vehicle.heading.to_radians();
            let (fwd, right) = (DVec2::new(h.sin(), h.cos()), DVec2::new(h.cos(), -h.sin()));
            let center = c.vehicle.position.truncate() + fwd * ((st.front - st.rear) * 0.5) as f64;
            out.push(Footprint {
                car: i,
                center,
                fwd,
                right,
                half_len: ((st.front + st.rear) * 0.5) as f64,
                half_w: c.half_width as f64,
                speed: st.speed,
                z: c.vehicle.position.z,
            });
            for t in &c.vehicle.trailers {
                if let Some(bb) = t.ty.def.bounding_box {
                    out.push(Footprint::from_obb(
                        i,
                        &::simulation::collision::Obb::from_box(bb, t.position, t.heading),
                        st.speed,
                    ));
                }
            }
        }
        out
    }


    /// Realized bodies for perception: AI bodies with their lane placements, trailers and rear
    /// sections under the owner id, and external road users. Placements only speed up queries.
    pub(crate) fn body_feet(&self, player: Option<PlayerBox>, others: &[(u32, PlayerBox)]) -> Vec<BodyFootprint> {
        let mut out: Vec<BodyFootprint> = Vec::with_capacity(self.cars.len() * 2 + others.len() + 2);
        for c in &self.cars {
            let st = &c.state;
            let h = c.vehicle.heading.to_radians();
            let fwd = DVec2::new(h.sin(), h.cos());
            let center = c.vehicle.position.truncate() + fwd * ((st.front - st.rear) * 0.5) as f64;
            let z = c.vehicle.position.z;
            let mut f = BodyFootprint::new(
                c.id,
                center,
                fwd,
                ((st.front + st.rear) * 0.5) as f64,
                c.half_width as f64,
                z,
                z + 3.0,
                st.speed,
            )
            .with_acc(st.acc);
            f.front = st.front;
            f.rear = st.rear;
            f.current = Some(f.placement_at(&self.net, LaneId(st.lane), st.s));
            if let Some(p) = st.prev_lane {
                if st.s < st.rear + 1.0 && st.change.is_none() {
                    f.prev = Some(f.placement_at(&self.net, LaneId(p), self.net.lanes[p].length() + st.s));
                }
            }
            if let Some(ch) = st.change {
                f.crossing = Some(f.placement_at(&self.net, LaneId(ch.to), ch.s_to));
            }
            if let Some(p) = c.maneuver.passing {
                let deep = st.lateral * self.net.oncoming_sign();
                let out_p = if p.aborted {
                    deep > p.side - c.half_width - 1.45
                } else {
                    deep > 1.2
                };
                if out_p {
                    if let Some((l, s, _)) = self
                        .net
                        .opposite(st.lane, st.s)
                        .filter(|o| o.0 == p.lane || (o.2 - p.side).abs() < 1.5)
                    {
                        let r = if p.aborted {
                            0.0
                        } else {
                            (p.clear_at(c.half_width) - st.odometer).max(0.0)
                        };
                        let at = s - r;
                        if at >= 0.0 {
                            f.passing = Some(Placement {
                                lane: LaneId(l),
                                s: at,
                                lateral: 0.0,
                                foreign: true,
                            });
                        } else {
                            for (ul, off, _) in self.net.upstream(l, at, 1.0, 12).into_iter().skip(1) {
                                let x = at - off;
                                if x >= 0.0 && x <= self.net.lanes[ul].length() {
                                    f.passing = Some(Placement {
                                        lane: LaneId(ul),
                                        s: x,
                                        lateral: 0.0,
                                        foreign: true,
                                    });
                                }
                            }
                        }
                    }
                }
            }
            out.push(f);
            for (k, t) in c.vehicle.trailers.iter().enumerate() {
                if let Some(bb) = t.ty.def.bounding_box {
                    let obb = ::simulation::collision::Obb::from_box(bb, t.position, t.heading);
                    out.push(body_part_from_obb(&f, (k + 1) as u16, &obb));
                }
            }
        }
        let mut external = |id: u64, b: PlayerBox| {
            let (c, heading, hl, hw, speed) = b;
            let h = heading.to_radians();
            let fwd = DVec2::new(h.sin(), h.cos());
            let mut f = BodyFootprint::new(
                VehicleId(id),
                c.truncate(),
                fwd,
                hl as f64,
                hw as f64,
                c.z,
                c.z + 3.0,
                speed,
            );
            // (on the lane it drives along, so lane changers keep their gap; not when standing
            // off the lane, which the cars on it would queue behind)
            if let Some((lane, s, _)) = self.net.lane_along(c, heading, LaneKind::Street, EXTERNAL_ON_LANE, 45.0) {
                f.current = Some(f.placement_at(&self.net, LaneId(lane), s));
            }
            out.push(f);
        };
        if let Some(p) = player {
            external(u64::MAX, p);
        }
        for (id, b) in others {
            external(u64::MAX - 1 - *id as u64, *b);
        }
        out
    }


    /// Another AI vehicle's body in each car's way where the lanes do not show it (a bus in
    /// its bay, a swinging rear section, a car cutting in), by sweeping the way with the
    /// car's width. Mutual blocks are sorted out by `geo_prev`: the higher id goes.
    ///
    /// Per car (by index): the body met and the way's direction there. It reads only frozen
    /// tick state, so it runs for all cars in parallel before the planning loop.
    pub(crate) fn bodies_in_way(
        &self,
        occupancy: &Occupancy,
        ignore_external: &[VehicleId],
    ) -> Vec<Option<(::traffic::Sweep, DVec2)>> {
        use rayon::prelude::*;
        let waiting = waiting_feet(occupancy.feet(), &self.geo_prev);
        let net = &self.net;
        let probes: Vec<WayProbe> = self
            .cars
            .iter()
            .map(|c| WayProbe {
                id: c.id,
                state: &c.state,
                position: c.vehicle.position,
                heading: c.vehicle.heading,
                half_width: c.half_width,
                going_round: c.maneuver.passing.map(|p| !p.aborted).unwrap_or(false)
                    || c.state.change.map(|c| c.bypass).unwrap_or(false),
            })
            .collect();
        probes
            .par_iter()
            .with_min_len(4)
            .map(|p| swept_body(net, occupancy, &waiting, ignore_external, p))
            .collect()
    }

    /// Car `i`'s lead from the body its way meets (`bodies_in_way`): the gap, and how fast
    /// that body moves along the way and speeds up, as of now.
    pub(crate) fn body_lead(&self, i: usize, hit: Option<(::traffic::Sweep, DVec2)>) -> Option<(Lead, usize)> {
        let (hit, dir) = hit?;
        let st = &self.cars[i].state;
        let j = *self.index_of.get(&hit.owner)?;
        let oh = self.cars[j].vehicle.heading.to_radians();
        let ofwd = DVec2::new(oh.sin(), oh.cos());
        let along = (ofwd.dot(dir) as f32 * hit.speed).max(0.0);
        let acc = if along > 0.1 { self.cars[j].state.acc } else { 0.0 };
        Some((
            Lead {
                gap: (hit.d - st.front).max(0.0),
                speed: along,
                acc,
            },
            j,
        ))
    }


    /// Where the player's vehicle is in car `i`'s way: the gap and its speed along the way.
    /// Its box is stretched along its motion (1.5 s) so it is seen before it is in the lane.
    pub(crate) fn player_in_way(&self, i: usize, player: &PlayerBox) -> Option<Lead> {
        let car = &self.cars[i];
        if (car.vehicle.position - player.0).length() > LOOK_AHEAD_MAX as f64 + 30.0 {
            return None;
        }
        self.player_on_way(&car.state, car.half_width, player)
    }


    /// `player_in_way` for a car of half width `half_width` on the way `st` lays out (also a
    /// way it only considers taking).
    pub(crate) fn player_on_way(&self, st: &AiState, half_width: f32, player: &PlayerBox) -> Option<Lead> {
        let (centre, heading, half_len, half_w, speed) = *player;
        let h = heading.to_radians();
        let fwd = DVec2::new(h.sin(), h.cos());
        let right = DVec2::new(h.cos(), -h.sin());
        let horizon = if self.player_priority { 5.0 } else { 1.5 };
        let way_dir = (st.way_point(&self.net, 3.0) - st.way_point(&self.net, 0.0)).truncate();
        let ahead = player_reach_ahead(half_len, speed, horizon, fwd, way_dir);
        let behind = half_len as f64 + ((-speed).max(0.0) * horizon) as f64;
        let wide = (half_w + half_width + 0.35) as f64;
        let margin = PLAYER_BOX_MARGIN as f64;
        let inside =
            |p: DVec3| in_player_box(p, centre, fwd, right, wide, ahead + margin, behind + margin);
        let look = (st.speed * st.speed / (2.0 * st.decel) + st.speed * 2.0 + 15.0)
            .clamp(15.0, look_ahead(st.speed));
        let mut d = 0.0f32;
        let mut step = 1.5f32;
        while d <= st.front + look {
            let p = st.way_point(&self.net, d.max(0.0));
            if inside(p) {
                // bisect where between the samples the way enters the box (exact gap)
                let (mut lo, mut hi) = ((d - step).max(0.0), d);
                if d > 0.0 {
                    for _ in 0..5 {
                        let mid = 0.5 * (lo + hi);
                        if inside(st.way_point(&self.net, mid)) {
                            hi = mid;
                        } else {
                            lo = mid;
                        }
                    }
                }
                let q = st.way_point(&self.net, hi + 1.0);
                let dir = (q - st.way_point(&self.net, hi))
                    .truncate()
                    .normalize_or_zero();
                let along = (fwd.dot(dir) as f32 * speed).max(0.0);
                return Some(Lead {
                    gap: (hi - st.front).max(0.0),
                    speed: along,
                    acc: 0.0,
                });
            }
            // (coarser far off: the entry is found by halving anyway)
            step = if d < st.front + 30.0 { 1.5 } else { 3.0 };
            d += step;
        }
        None
    }

}


/// What `bodies_in_way` needs of a car, gathered on the main thread.
struct WayProbe<'a> {
    id: VehicleId,
    state: &'a AiState,
    position: DVec3,
    heading: f64,
    half_width: f32,
    /// Pulling out round something: what stands on its lane does not stop it.
    going_round: bool,
}

/// The footprints grouped by the car they wait for (`geo_prev`).
fn waiting_feet(
    feet: &[BodyFootprint],
    geo_prev: &HashMap<VehicleId, Option<VehicleId>>,
) -> HashMap<VehicleId, Vec<usize>> {
    let mut waiting: HashMap<VehicleId, Vec<usize>> = HashMap::new();
    for (k, f) in feet.iter().enumerate() {
        if let Some(&Some(target)) = geo_prev.get(&f.owner) {
            waiting.entry(target).or_default().push(k);
        }
    }
    waiting
}

/// The first body the car's swept way meets (see `Traffic::bodies_in_way`).
fn swept_body(
    net: &Network,
    occupancy: &Occupancy,
    waiting: &HashMap<VehicleId, Vec<usize>>,
    ignore_external: &[VehicleId],
    car: &WayProbe,
) -> Option<(::traffic::Sweep, DVec2)> {
    let st = car.state;
    if net.lanes[st.lane].kind == LaneKind::Air {
        return None;
    }
    let z = car.position.z;
    let look = (st.speed * st.speed / (2.0 * st.decel.max(1.0)) + st.speed * 2.0 + 12.0)
        .clamp(12.0, LOOK_AHEAD);
    let reach = st.front + look;
    let me = car.id;
    // What the car is pulling out round does not stop it.
    let mut ignore: Vec<VehicleId> = ignore_external.to_vec();
    // Own primary body and articulated sections are never a leader/obstacle.
    ignore.push(me);
    if car.going_round {
        for iv in occupancy.intervals(LaneId(st.lane)) {
            if iv.speed < 0.5 {
                ignore.push(iv.owner);
            }
        }
    }
    // A body that already waits for this car is not in its way, unless at the bumper.
    let h = car.heading.to_radians();
    let fwd = DVec2::new(h.sin(), h.cos());
    let center = car.position.truncate() + fwd * ((st.front - st.rear) * 0.5) as f64;
    let mut probe = BodyFootprint::new(
        me,
        center,
        fwd,
        ((st.front + st.rear) * 0.5) as f64,
        car.half_width as f64,
        z,
        z + 3.0,
        st.speed,
    );
    probe.front = st.front;
    probe.rear = st.rear;
    for &k in waiting.get(&me).map(|v| v.as_slice()).unwrap_or(&[]) {
        let f = &occupancy.feet()[k];
        if f.owner == me {
            continue;
        }
        if me > f.owner && !f.overlaps(&probe, 1.5) {
            ignore.push(f.owner);
        }
    }
    let mut samples: Vec<SweepSample> = Vec::new();
    let way = st.way_fn(net);
    let mut d = st.front + 0.2;
    let mut p = way(d);
    while d <= reach {
        let step = if d < st.front + 20.0 { 0.75 } else { 1.5 };
        let q = way(d + step);
        let dir = (q - p).truncate().normalize_or_zero();
        samples.push(SweepSample { p, d, dir });
        p = q;
        d += step;
    }
    let hit = occupancy.swept_clearance(&samples, car.half_width as f64, &ignore)?;
    let dir = samples
        .iter()
        .find(|s| s.d == hit.d)
        .map(|s| s.dir)
        .unwrap_or(DVec2::ZERO);
    Some((hit, dir))
}

/// How far (m) the middle of a player's or LAN vehicle may be from a lane to count as on it.
const EXTERNAL_ON_LANE: f64 = 2.0;

#[cfg(test)]
mod tests {
    use super::*;

    /// The player's bus stands on the lane it drives along, so lane changers keep their gap.
    #[test]
    fn the_players_bus_is_counted_on_its_lane() {
        let lane = |x: f64| {
            ::traffic::LaneBuilder::polyline(
                vec![DVec3::new(x, 0.0, 0.0), DVec3::new(x, 400.0, 0.0)],
                LaneKind::Street,
                3.5,
            )
        };
        let mut net = Network { lanes: vec![lane(0.0), lane(-3.5)], ..Default::default() };
        net.link(1.5);
        let t = Traffic::for_tests(net);
        let player = (DVec3::new(0.3, 100.0, 0.0), 0.0, 6.0, 1.25, 22.0);
        let occupancy = Occupancy::build(t.net.version(), 0, t.body_feet(Some(player), &[]));
        let on = |l: usize| occupancy.intervals(LaneId(l)).iter().find(|iv| iv.owner == VehicleId(u64::MAX)).copied();
        let iv = on(0).expect("the player is on no lane");
        assert!((iv.s - 100.0).abs() < 0.1 && (iv.front - 106.0).abs() < 0.1 && (iv.rear - 94.0).abs() < 0.1, "{iv:?}");
        assert_eq!(iv.speed, 22.0);
        assert!(on(1).is_none(), "on the lane beside it too");
        // standing 3 m off the lane (a shoulder): the cars on it do not queue behind it
        let aside = (DVec3::new(3.0, 100.0, 0.0), 0.0, 6.0, 1.25, 0.0);
        let occupancy = Occupancy::build(t.net.version(), 0, t.body_feet(Some(aside), &[]));
        assert!(occupancy.intervals(LaneId(0)).iter().all(|iv| iv.owner != VehicleId(u64::MAX)));
    }

    #[test]
    fn the_footprints_are_found_by_the_car_they_wait_for() {
        let foot = |owner: u64| BodyFootprint::new(VehicleId(owner), DVec2::ZERO, DVec2::Y, 2.0, 1.0, 0.0, 3.0, 0.0);
        // car 1 has two parts; 1 and 3 wait for 5, 2 for nobody, 4 (no footprint) for 1
        let feet = [foot(1), foot(1), foot(2), foot(3)];
        let geo_prev: HashMap<VehicleId, Option<VehicleId>> = [
            (VehicleId(1), Some(VehicleId(5))),
            (VehicleId(2), None),
            (VehicleId(3), Some(VehicleId(5))),
            (VehicleId(4), Some(VehicleId(1))),
        ]
        .into_iter()
        .collect();
        let waiting = waiting_feet(&feet, &geo_prev);
        assert_eq!(waiting.get(&VehicleId(5)), Some(&vec![0, 1, 3]));
        assert_eq!(waiting.len(), 1, "{waiting:?}");
    }

    /// A body standing in the car's way stops it - unless it waits for this car itself (by
    /// `geo_prev`) and has the lower id, and is not right at the bumper: then this car goes.
    #[test]
    fn a_body_in_the_way_that_waits_for_the_car_lets_it_go() {
        let lane = ::traffic::LaneBuilder::polyline(
            vec![DVec3::new(0.0, 0.0, 0.0), DVec3::new(0.0, 200.0, 0.0)],
            LaneKind::Street,
            3.0,
        );
        let mut net = Network { lanes: vec![lane], ..Default::default() };
        net.link(1.5);
        let mut state = AiState::new(0, 20.0, 1);
        (state.front, state.rear, state.length, state.speed) = (2.25, 2.25, 4.5, 8.0);
        state.plan_next(&net);
        let car = WayProbe {
            id: VehicleId(10),
            state: &state,
            position: DVec3::new(0.0, 20.0, 0.0),
            heading: 0.0,
            half_width: 1.0,
            going_round: false,
        };
        let other = BodyFootprint::new(VehicleId(3), DVec2::new(0.0, 40.0), DVec2::Y, 2.25, 1.0, 0.0, 3.0, 0.0);
        let occupancy = Occupancy::build(net.version(), 0, vec![other]);
        let none = HashMap::new();
        let (hit, _) = swept_body(&net, &occupancy, &none, &[], &car).expect("the body is in the way");
        assert_eq!(hit.owner, VehicleId(3));
        assert!((hit.d - 17.75).abs() < 0.8, "met at {}", hit.d);
        let geo_prev: HashMap<VehicleId, Option<VehicleId>> =
            [(VehicleId(3), Some(VehicleId(10)))].into_iter().collect();
        let waiting = waiting_feet(occupancy.feet(), &geo_prev);
        assert_eq!(swept_body(&net, &occupancy, &waiting, &[], &car), None, "it waits for this car");
    }

    #[test]
    fn articulated_bus_boxes_follow_the_real_heading_and_leave_the_adjacent_lane_free() {
        for degrees in [30.0f64, 90.0, 149.6, 270.0] {
            let heading = degrees.to_radians();
            let fwd = DVec2::new(heading.sin(), heading.cos());
            let right = DVec2::new(fwd.y, -fwd.x);
            let owner = VehicleId(93);
            let main = BodyFootprint::new(owner, fwd * 50.0, fwd, 5.0, 1.25, 0.0, 3.0, 0.0);
            let rear_box = Obb::vehicle(fwd * 40.0, degrees, 3.6, 3.6, 1.25);
            let rear = body_part_from_obb(&main, 1, &rear_box);
            assert!((rear.fwd - fwd).length() < 1e-6, "rear footprint rotated at {degrees} degrees");
            assert_eq!(rear.owner, owner);
            assert_eq!(rear.part, 1);
            let occupancy = Occupancy::build(NetworkVersion(1), 0, vec![main, rear]);
            let samples: Vec<_> = (25..70).map(|s| {
                let p = fwd * s as f64 - right * 3.0;
                SweepSample { p: DVec3::new(p.x, p.y, 0.0), d: (s - 25) as f32, dir: fwd }
            }).collect();
            assert!(occupancy.swept_clearance(&samples, 0.9, &[]).is_none(),
                "right-lane articulated bus blocked the left lane at {degrees} degrees");
            // A rear section that really swings across the lane must remain an obstacle.
            let swung_box = Obb::vehicle(fwd * 40.0, degrees + 40.0, 3.6, 3.6, 1.25);
            let swung_rear = body_part_from_obb(&main, 1, &swung_box);
            let swung = Occupancy::build(NetworkVersion(1), 0, vec![main, swung_rear]);
            let hit = swung.swept_clearance(&samples, 0.9, &[]).expect("real rear overlap must block");
            assert_eq!(hit.owner, owner);
            assert_eq!(hit.part, 1);
            assert!(swung.swept_clearance(&samples, 0.9, &[owner]).is_none());
        }
    }
}
