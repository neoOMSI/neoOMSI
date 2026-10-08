//! Per-tick perception adapter: realized footprints, body/obstacle geometry and
//! route-relative observation helpers feeding the domain coordinators.

use super::*;

impl Traffic {

    /// Nearest vehicle ahead of position `s` on `lane` (following the lanes `plan` has
    /// chosen after it, else the first `next`, for up to `look` m): (distance from `s` to
    /// its rear, its speed along the lane, its index). A vehicle beside the lane's middle
    /// far enough to be passed (a bus in its bay) does not count; one coming the other way
    /// round an obstacle does, standing.
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
        // (as many lanes as fit into `look`: a junction is a string of short ones, and four
        // of them hid a bus standing just behind it)
        for _ in 0..10 {
            if let Some(list) = by_lane.get(&lane) {
                for &(j, os, lat, foreign) in list {
                    if j == i || os <= s_from {
                        continue;
                    }
                    let o = &self.cars[j];
                    // Two that overlap (a car that ended up beside or in a bus) each found the
                    // other ahead - one's front past the other's rear - and each waited for the
                    // other for good. One that follows this car already and whose middle is
                    // behind this one's is not its lead: the front one drives off.
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
                    // (the lateral place this car will have when it gets there: pulling out
                    // round a standing bus, it is clear of it before it arrives)
                    let mine = me.state.lateral_ahead(offset + (os - s_from));
                    if !foreign && (lat - mine).abs() > me.half_width + o.half_width + 0.3 {
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
            if offset > look || best.is_some() {
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
        // (that is the whole point of pulling out round it)
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
        // Merging: a car on another lane that leads into the same lane as the next one of
        // ours, and is nearer to that joint, goes first; this car keeps behind it as if it
        // were already ahead in its own lane. (A left turn and the straight lane beside it
        // end in the same exit; taking the turn at a sensible speed, a car used to be run
        // through by the one going straight.)
        if !committed {
            let mut before = self.net.lanes[me.lane].length() - me.s;
            let mut from = me.lane;
            for next in me.upcoming().take(2) {
                if before > look {
                    break;
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
                        // who reaches the joint first goes first; a near tie goes to the one
                        // already let in (the order is kept, it does not flip frame by frame)
                        let t_me = (before - me.front).max(0.0) / me.speed.max(1.0);
                        let dist_them = (theirs - other.state.front).max(0.0);
                        let t_them = if other.yielding || other.light_hold {
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
                            // behind it at the joint; while it is not past yet, wait at the
                            // joint itself rather than behind a car that is still beside
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


    /// Is the stretch `s - back .. s + ahead` of `lane` free of cars (other than `i`)?
    /// bus serving its stop, a broken-down or abandoned car, the player's bus waiting)?
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
                // a bus at its stop, or a car that has stood for long with nothing holding
                // it (not the head of a queue that waits for a light, a junction or a car),
                // or the end of a queue standing behind a bus at its stop
                o.standing_for(self.day_time) > 4.0
                    || (o.stopped > 25.0 && !o.held && self.cars[i].stopped > 6.0)
                    || (o.stopped > 3.0 && self.standing_queue(j).1)
            }
            Some(_) => player_standing > 10.0,
            None => parked_ahead,
        }
    }


    /// The vehicles standing nose to tail from car `j` on (as far as their last steps
    /// show): the length of road they fill (m), and whether a bus serving its stop heads
    /// it - a queue that will not move for a while, which the cars behind may pass as a
    /// whole. (Behind a bus on its layover the whole street
    /// used to wait, five buses and a dozen cars for a quarter of an hour.)
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


    /// Pull out round something that has stopped in front (a bus at its stop, a car that
    /// gave up, the player standing in the lane): a car held for a few seconds behind a
    /// standing obstacle within 25 m moves to a free neighbouring lane - left first, then
    /// right. With nowhere to go it waits, like everybody else in a jam.
    #[allow(clippy::too_many_arguments)]

    /// The lanes of a car's way with their distance from its origin: the current lane (at
    /// minus `s`) and the plan, up to `within` metres.
    /// Where car `i` has to stop for somebody on foot (the distance of its front from its
    /// origin, as the other stops): anybody standing in the strip it is about to sweep, or
    /// stepping into it by the time the car gets there. Only the zebras and signalled
    /// crossings used to count, and only for people strolling the footpaths, so a car
    /// drove at full speed through a passenger walking off a bus across the road, through
    /// somebody leaving a stop, or through anybody standing in the carriageway. A
    /// timetable bus ignores the people waiting at the kerb for it unless they stand well
    /// inside its path (it pulls up right beside them).
    pub(crate) fn people_stop(&self, i: usize, way: &[(usize, f32)]) -> Option<(f32, DVec2)> {
        if self.people.is_empty() {
            return None;
        }
        let car = &self.cars[i];
        let st = &car.state;
        let v = st.speed.max(0.0);
        // as far as the car needs to stop without a jolt, and never less than a car length
        let reach = (v * v / 5.0 + v + 6.0).clamp(8.0, 45.0);
        let origin = car.vehicle.position.truncate();
        let near: Vec<&(DVec2, DVec2, bool)> = self
            .people
            .iter()
            .filter(|(p, _, _)| (*p - origin).length() < (st.front + reach) as f64 + 6.0)
            .collect();
        if near.is_empty() {
            return None;
        }
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
                for (p, pv, waiting) in &near {
                    let half = if *waiting && car.is_bus() {
                        car.half_width as f64 - 0.3
                    } else {
                        car.half_width as f64 + 0.3
                    };
                    for at in [*p, *p + *pv * t] {
                        let rel = at - c;
                        if rel.dot(fwd).abs() <= 0.55 && rel.dot(right).abs() < half {
                            return Some((d - 1.5, *p));
                        }
                    }
                }
                s += 1.0;
            }
            first = false;
        }
        None
    }


    pub(crate) fn way_lanes(&self, st: &AiState, within: f32) -> Vec<(usize, f32)> {
        let mut out = vec![(st.lane, -st.s)];
        let mut d = self.net.lanes[st.lane].length() - st.s;
        let plan: Vec<usize> = match st.change {
            Some(c) => std::iter::once(c.to)
                .chain(st.change_plan.iter().copied())
                .collect(),
            None => st.upcoming().collect(),
        };
        if let Some(c) = st.change {
            // over on the new lane: its distances count from the same place
            out.clear();
            out.push((c.to, -c.s_to));
            d = self.net.lanes[c.to].length() - c.s_to;
            for &l in plan.iter().skip(1) {
                if d > within {
                    break;
                }
                out.push((l, d));
                d += self.net.lanes[l].length();
            }
            return out;
        }
        for l in plan {
            if d > within {
                break;
            }
            out.push((l, d));
            d += self.net.lanes[l].length();
        }
        out
    }


    /// Build the frozen junction view for car `i` and let the coordinator decide. The scene
    /// borrows only immutable fields (network, index, previous blockers) and the caller's
    /// view maps, so the coordinator is the sole writer of junction claims.
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


    /// Two cars that have each other for their lead - a car that ended up in a bus's body,
    /// each finding the other in its way - wait for each other for good. The one further
    /// along its lane (on the same lane; else the lower number) stops taking the other for
    /// its lead for a few seconds and drives off.
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
            // Each unordered pair is handled once, chosen by stable id rather than by
            // container position, so a reorder cannot pick a different partner.
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


    /// Realized bodies for the perception layer: primary bodies with their lane placements
    /// (current lane, the lane a rear still stands on, a lane-change target, a passing
    /// offset), trailers/rear sections sharing the owner id, and the external road users.
    /// The geometry is the truth; the placements only accelerate queries.
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
            f.current = Some(Placement {
                lane: LaneId(st.lane),
                s: st.s,
                lateral: st.lateral,
                foreign: false,
            });
            if let Some(p) = st.prev_lane {
                if st.s < st.rear + 1.0 && st.change.is_none() {
                    f.prev = Some(Placement {
                        lane: LaneId(p),
                        s: self.net.lanes[p].length() + st.s,
                        lateral: st.lateral,
                        foreign: false,
                    });
                }
            }
            if let Some(ch) = st.change {
                f.crossing = Some(Placement {
                    lane: LaneId(ch.to),
                    s: ch.s_to,
                    lateral: 0.0,
                    foreign: false,
                });
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
                    let th = obb.heading.to_radians();
                    let tfwd = DVec2::new(th.sin(), th.cos());
                    out.push(f.part_of(
                        (k + 1) as u16,
                        obb.center,
                        tfwd,
                        obb.half.y,
                        obb.half.x,
                    ));
                }
            }
        }
        let mut external = |id: u64, b: PlayerBox| {
            let (c, heading, hl, hw, speed) = b;
            let h = heading.to_radians();
            let fwd = DVec2::new(h.sin(), h.cos());
            out.push(BodyFootprint::new(
                VehicleId(id),
                c.truncate(),
                fwd,
                hl as f64,
                hw as f64,
                c.z,
                c.z + 3.0,
                speed,
            ));
        };
        if let Some(p) = player {
            external(u64::MAX, p);
        }
        for (id, b) in others {
            external(u64::MAX - 1 - *id as u64, *b);
        }
        out
    }


    /// Another AI vehicle's body in car `i`'s way where the lanes do not show it: a bus
    /// standing in its bay across a turning path, a car stopped half inside a junction, the
    /// rear section of an articulated bus still swinging round, a car cutting in. The way
    /// ahead is swept with the car's width against every footprint near it (the lanes alone
    /// let a car turn right into the side of a bus that stood 1.7 m out in its bay).
    /// Two vehicles that each stand in the other's way are sorted out by `geo_prev`: the one
    /// with the higher id goes, the other waits.
    pub(crate) fn body_in_way(
        &self,
        i: usize,
        occupancy: &Occupancy,
        ignore_external: &[VehicleId],
    ) -> Option<(Lead, usize)> {
        let car = &self.cars[i];
        let st = &car.state;
        if self.net.lanes[st.lane].kind == LaneKind::Air {
            return None;
        }
        let z = car.vehicle.position.z;
        let look = (st.speed * st.speed / (2.0 * st.decel.max(1.0)) + st.speed * 2.0 + 12.0)
            .clamp(12.0, LOOK_AHEAD);
        let reach = st.front + look;
        let me = car.id;
        // What the car is pulling out round does not stop it.
        let mut ignore: Vec<VehicleId> = ignore_external.to_vec();
        if car.maneuver.passing.map(|p| !p.aborted).unwrap_or(false)
            || st.change.map(|c| c.bypass).unwrap_or(false)
        {
            for iv in occupancy.intervals(LaneId(st.lane)) {
                if iv.speed < 0.5 {
                    ignore.push(iv.owner);
                }
            }
        }
        // A body that already waits for this car is not in its way - unless it is within
        // reach of the bumper, where the car would be driven into it.
        let h = car.vehicle.heading.to_radians();
        let fwd = DVec2::new(h.sin(), h.cos());
        let center = car.vehicle.position.truncate() + fwd * ((st.front - st.rear) * 0.5) as f64;
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
        for f in occupancy.feet() {
            if f.owner == me || self.geo_prev.get(&f.owner).copied().flatten() != Some(me) {
                continue;
            }
            if me > f.owner && !f.overlaps(&probe, 1.5) {
                ignore.push(f.owner);
            }
        }
        // Sweep the corridor along the car's own way with its width.
        let mut samples: Vec<SweepSample> = Vec::new();
        let mut d = st.front + 0.2;
        let mut p = st.way_point(&self.net, d);
        while d <= reach {
            let step = if d < st.front + 20.0 { 0.75 } else { 1.5 };
            let q = st.way_point(&self.net, d + step);
            let dir = (q - p).truncate().normalize_or_zero();
            samples.push(SweepSample { p, d, dir });
            p = q;
            d += step;
        }
        let hit = occupancy.swept_clearance(&samples, car.half_width as f64, &ignore)?;
        let j = self.cars.iter().position(|c| c.id == hit.owner)?;
        let dir = samples
            .iter()
            .find(|s| s.d == hit.d)
            .map(|s| s.dir)
            .unwrap_or(DVec2::ZERO);
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


    /// Where the player's vehicle is in car `i`'s way: the gap to it and how fast it moves
    /// along that way. The bus's box is stretched along its motion for the next second and
    /// a half, so a bus pulling out of a stop, turning across or reversing is seen before
    /// it is in the lane - the lanes alone saw it only once it stood in them.
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
                // where between the samples the way enters the box: the gap to a standing bus
                // used to come in steps of a metre and a half (and up to that much too long,
                // so that cars stopped closer than they meant to)
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


    /// The street lanes a vehicle standing at `pos` facing `heading` is on and will drive
    /// onto within `reach` metres, each with the distance from the vehicle to its start
    /// (0 for the lane it is on): the way straight on and the gentle turns (within 60° of
    /// the lane before), not every branch of a junction.
    pub(crate) fn lanes_ahead_of(&self, pos: DVec3, heading: f64, reach: f32) -> Vec<(usize, f32)> {
        let Some((lane, s, _)) = self
            .net
            .lane_along(pos, heading, LaneKind::Street, 4.0, 45.0)
        else {
            return Vec::new();
        };
        let mut out = vec![(lane, 0.0f32)];
        let mut open = vec![(lane, self.net.lanes[lane].length() - s)];
        while let Some((l, to_end)) = open.pop() {
            if to_end > reach || out.len() > 64 {
                continue;
            }
            let end_heading = self.net.lanes[l].headings.last().copied().unwrap_or(0.0);
            for &n in &self.net.lanes[l].next {
                let nl = &self.net.lanes[n];
                if nl.kind != LaneKind::Street || out.iter().any(|o| o.0 == n) {
                    continue;
                }
                let start = nl.headings.first().copied().unwrap_or(end_heading);
                let turn = ((start - end_heading) as f64 + 540.0).rem_euclid(360.0) - 180.0;
                if turn.abs() > 60.0 {
                    continue;
                }
                out.push((n, to_end));
                open.push((n, to_end + nl.length()));
            }
        }
        out
    }

}
