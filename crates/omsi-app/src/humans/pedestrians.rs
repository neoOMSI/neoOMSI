use super::*;

/// One piece of a walk along a pavement path: lane `lane` from distance `a` to `b`.
#[derive(Debug, Clone, Copy)]
pub(super) struct Leg {
    pub(super) lane: usize,
    pub(super) a: f32,
    pub(super) b: f32,
}

impl Leg {
    pub(super) fn len(&self) -> f32 {
        (self.b - self.a).abs()
    }
    pub(super) fn dist(&self, p: f32) -> f32 {
        if self.b >= self.a {
            self.a + p
        } else {
            self.a - p
        }
    }
    /// Point and walking heading `p` metres into the leg.
    pub(super) fn at(&self, net: &Network, p: f32) -> (DVec3, f64) {
        let (q, h) = net.lanes[self.lane].at(self.dist(p.clamp(0.0, self.len())));
        (
            q,
            if self.b >= self.a {
                h as f64
            } else {
                h as f64 + 180.0
            },
        )
    }
    /// How far into the leg the point nearest `pos` lies, looking around `hint`.
    pub(super) fn project(&self, net: &Network, pos: DVec3, hint: f32) -> f32 {
        let (lo, hi) = ((hint - 1.5).max(0.0), (hint + 3.0).min(self.len()));
        let mut best = (hint, f64::MAX);
        let mut p = lo;
        while p <= hi + 1e-3 {
            let d = (self.at(net, p).0 - pos).truncate().length_squared();
            if d < best.1 {
                best = (p, d);
            }
            p += 0.2;
        }
        best.0
    }
    /// Whether the leg starts at an end of its lane (at a kerb or a junction).
    pub(super) fn from_end(&self, net: &Network) -> bool {
        self.a < 0.05 || self.a > net.lanes[self.lane].length() - 0.05
    }
}

/// A walk along the pavement network.
#[derive(Debug, Clone)]
pub(super) struct PedWalk {
    pub(super) legs: Vec<Leg>,
    pub(super) leg: usize,
    /// Metres walked into the current leg.
    pub(super) s: f32,
    /// A stroll: goes on at random when the legs run out.
    pub(super) roam: bool,
    /// Keep-right offset (m).
    pub(super) side: f32,
    /// Seconds spent waiting at the kerb before the current leg.
    pub(super) held: f32,
}

impl PedWalk {
    pub(super) fn new(legs: Vec<Leg>, roam: bool, side: f32) -> PedWalk {
        PedWalk {
            legs,
            leg: 0,
            s: 0.0,
            roam,
            side,
            held: 0.0,
        }
    }
}

/// The pavement paths as a walking network: path ends closer than a metre are one
/// junction, whatever their heading (the road network joins lane ends only when they
/// continue in the same direction, which leaves every pavement corner open).
pub(super) struct PedNet {
    /// Per pavement lane: its start and end junction.
    pub(super) ends: HashMap<usize, (usize, usize)>,
    /// Per junction: (lane, walked forwards) leaving it.
    pub(super) out: Vec<Vec<(usize, bool)>>,
    /// Where each pavement lane crosses a carriageway (lazily), and the carriageway lanes
    /// it crosses.
    pub(super) crossings: HashMap<usize, Vec<DVec2>>,
    pub(super) crossed: HashMap<usize, Vec<usize>>,
    /// Pavement lanes by 50 m cell.
    pub(super) grid: HashMap<(i32, i32), Vec<usize>>,
    /// The junctions and a 1.5 m grid of them, for joining the paths of tiles loaded later.
    pub(super) nodes: Vec<DVec3>,
    pub(super) cells: HashMap<(i64, i64), Vec<usize>>,
    /// How many lanes of the traffic network are in (the network only grows: tiles bring
    /// their lanes and the indices stay).
    pub(super) built: usize,
    /// Length of each sidewalk lane in the network.
    pub(super) lengths: HashMap<usize, f32>,
}

impl PedNet {
    pub(super) fn build(net: &Network) -> PedNet {
        let mut p = PedNet {
            ends: HashMap::new(),
            out: Vec::new(),
            crossings: HashMap::new(),
            crossed: HashMap::new(),
            grid: HashMap::new(),
            nodes: Vec::new(),
            cells: HashMap::new(),
            built: 0,
            lengths: HashMap::new(),
        };
        p.extend(net);
        log::info!(
            "pavement network: {} paths, {} junctions",
            p.ends.len(),
            p.nodes.len()
        );
        p
    }

    /// Take in the lanes the network gained since the last call (tiles streamed in).
    pub(super) fn extend(&mut self, net: &Network) -> usize {
        let from = self.built.min(net.lanes.len());
        let before = self.ends.len();
        for i in from..net.lanes.len() {
            let l = &net.lanes[i];
            if l.kind == LaneKind::Street && from > 0 {
                // a new carriageway may cross pavement paths that are in already
                self.crossings.clear();
            }
            if l.kind != LaneKind::Sidewalk || l.points.len() < 2 || l.length() < 0.3 {
                continue;
            }
            let a = self.node_of(l.start());
            let b = self.node_of(l.end());
            if a == b && l.length() < 3.0 {
                continue;
            }
            self.ends.insert(i, (a, b));
            self.lengths.insert(i, l.length());
            self.out[a].push((i, true));
            self.out[b].push((i, false));
            let mut seen: Vec<(i32, i32)> = Vec::new();
            for p in &l.points {
                let c = ((p.x / 50.0).floor() as i32, (p.y / 50.0).floor() as i32);
                if !seen.contains(&c) {
                    seen.push(c);
                    self.grid.entry(c).or_default().push(i);
                }
            }
        }
        self.built = net.lanes.len();
        self.ends.len() - before
    }

    pub fn lane_length(&self, lane: usize) -> Option<f32> {
        self.lengths.get(&lane).copied()
    }

    /// Leave the stop along a finite, non-repeating route. A dead end is a place to
    /// stop, rather than an instruction to shuttle back through the arriving crowd.
    pub(super) fn departure_route(&self, net: &Network, first: Leg, mut pick: u64) -> Vec<Leg> {
        let mut legs = vec![first];
        let mut lanes: HashSet<usize> = [first.lane].into_iter().collect();
        let mut nodes = HashSet::new();
        if let Some(&(a, b)) = self.ends.get(&first.lane) {
            nodes.insert(if first.b > first.a { a } else { b });
        }
        let mut distance = first.len();
        while distance < 30.0 {
            let last = legs.last().unwrap();
            let Some(node) = self.end_node(net, last) else {
                break;
            };
            if !nodes.insert(node) {
                break;
            }
            let candidates: Vec<Leg> = self.out[node]
                .iter()
                .filter_map(|&(lane, forward)| {
                    if lanes.contains(&lane) {
                        return None;
                    }
                    let &(a, b) = self.ends.get(&lane)?;
                    if nodes.contains(&if forward { b } else { a }) {
                        return None;
                    }
                    let length = net.lanes[lane].length();
                    Some(Leg {
                        lane,
                        a: if forward { 0.0 } else { length },
                        b: if forward { length } else { 0.0 },
                    })
                })
                .collect();
            if candidates.is_empty() {
                break;
            }
            let next = candidates[pick as usize % candidates.len()];
            pick = pick.rotate_left(13);
            lanes.insert(next.lane);
            distance += next.len();
            legs.push(next);
        }
        legs
    }

    /// The junction at `p`, a new one when there is none within a metre.
    pub(super) fn node_of(&mut self, p: DVec3) -> usize {
        let (cx, cy) = ((p.x / 1.5).floor() as i64, (p.y / 1.5).floor() as i64);
        for dx in -1..=1 {
            for dy in -1..=1 {
                if let Some(list) = self.cells.get(&(cx + dx, cy + dy)) {
                    for &n in list {
                        if (self.nodes[n] - p).truncate().length() < 1.2
                            && (self.nodes[n].z - p.z).abs() < 2.5
                        {
                            return n;
                        }
                    }
                }
            }
        }
        self.nodes.push(p);
        self.out.push(Vec::new());
        self.cells
            .entry((cx, cy))
            .or_default()
            .push(self.nodes.len() - 1);
        self.nodes.len() - 1
    }

    /// The pavement lane nearest `p` within `reach` that can be reached without going over
    /// a carriageway: (lane, distance along it, distance to it). The plain nearest one was
    /// often the pavement across the road - a passenger off a bus then walked straight
    /// over the carriageway through the traffic to it, or joined a crossing in the middle.
    pub(super) fn nearest(&self, net: &Network, p: DVec3, reach: f64) -> Option<(usize, f32, f64)> {
        let (cx, cy) = ((p.x / 50.0).floor() as i32, (p.y / 50.0).floor() as i32);
        let mut cands: Vec<(usize, f32, f64)> = Vec::new();
        let mut seen = HashSet::new();
        for dx in -1..=1 {
            for dy in -1..=1 {
                for &i in self
                    .grid
                    .get(&(cx + dx, cy + dy))
                    .map(|v| v.as_slice())
                    .unwrap_or(&[])
                {
                    if !seen.insert(i) {
                        continue;
                    }
                    if let Some((s, d)) = net.lanes[i].nearest_point(p) {
                        if d < reach {
                            cands.push((i, s, d));
                        }
                    }
                }
            }
        }
        cands.sort_by(|a, b| a.2.total_cmp(&b.2));
        let first = cands.first().copied();
        cands
            .into_iter()
            .find(|&(i, s, _)| {
                let (q, _) = net.lanes[i].at(s);
                !crosses_street(net, p.truncate(), q.truncate())
                    && !self
                        .crossings
                        .get(&i)
                        .map(|x| !x.is_empty())
                        .unwrap_or(false)
            })
            // on an island between carriageways: the nearest after all
            .or(first)
    }

    /// The junction a leg ends at, when it ends at one.
    pub(super) fn end_node(&self, net: &Network, leg: &Leg) -> Option<usize> {
        let (a, b) = *self.ends.get(&leg.lane)?;
        let len = net.lanes[leg.lane].length();
        if leg.b < 0.05 {
            Some(a)
        } else if leg.b > len - 0.05 {
            Some(b)
        } else {
            None
        }
    }

    /// A leg leaving junction `node`, not back along `came` (unless it is a dead end).
    pub(super) fn next_leg(
        &self,
        net: &Network,
        node: usize,
        came: usize,
        pick: u64,
    ) -> Option<Leg> {
        let back = self.ends.get(&came).copied();
        let twin = |l: usize| -> bool {
            l == came
                || matches!((self.ends.get(&l), back), (Some(&(a, b)), Some((c, d))) if a == d && b == c && (net.lanes[l].length() - net.lanes[came].length()).abs() < 1.0)
        };
        let list: Vec<(usize, bool)> = self
            .out
            .get(node)?
            .iter()
            .copied()
            .filter(|(l, _)| !twin(*l))
            .collect();
        // the way on rather than back: a path leaving the junction within 110° of the way
        // the walker came (there usually is one - a pavement goes on past a side street),
        // else any. Picked from all, a stroller would turn round at every corner and walk
        // back the way they came, which looked like a change of mind for no reason.
        let heading_in = {
            let l = &net.lanes[came];
            let (a, _) = back.unwrap_or((usize::MAX, usize::MAX));
            // arriving at `node` along `came`: forwards if its end is the node
            if a == node {
                wrap_heading(l.start_heading() as f64 + 180.0)
            } else {
                l.end_heading() as f64
            }
        };
        let leaving = |&(l, fwd): &(usize, bool)| -> f64 {
            let lane = &net.lanes[l];
            if fwd {
                lane.start_heading() as f64
            } else {
                wrap_heading(lane.end_heading() as f64 + 180.0)
            }
        };
        let onward: Vec<(usize, bool)> = list
            .iter()
            .copied()
            .filter(|o| angle_between(heading_in, leaving(o)) <= 110.0)
            .collect();
        let list = if onward.is_empty() { list } else { onward };
        let (lane, fwd) = if list.is_empty() {
            // a dead end: turn round
            let (a, _) = back?;
            (came, a == node)
        } else {
            list[(pick as usize) % list.len()]
        };
        let len = net.lanes[lane].length();
        Some(if fwd {
            Leg {
                lane,
                a: 0.0,
                b: len,
            }
        } else {
            Leg {
                lane,
                a: len,
                b: 0.0,
            }
        })
    }

    /// Where pavement lane `lane` crosses a carriageway.
    pub(super) fn crossings(&mut self, net: &Network, lane: usize) -> &[DVec2] {
        if !self.crossings.contains_key(&lane) {
            let l = &net.lanes[lane];
            let mut cand: Vec<usize> = Vec::new();
            for p in &l.points {
                if let Some(list) = net
                    .grid
                    .get(&((p.x / 50.0).floor() as i32, (p.y / 50.0).floor() as i32))
                {
                    for &i in list {
                        if net.lanes[i].kind == LaneKind::Street && !cand.contains(&i) {
                            cand.push(i);
                        }
                    }
                }
            }
            let mut out = Vec::new();
            let mut crossed = Vec::new();
            for i in cand {
                let o = &net.lanes[i];
                for w in l.points.windows(2) {
                    for v in o.points.windows(2) {
                        if (w[0].z - v[0].z).abs() > 3.0 {
                            continue;
                        }
                        if let Some(x) = seg_cross(
                            w[0].truncate(),
                            w[1].truncate(),
                            v[0].truncate(),
                            v[1].truncate(),
                        ) {
                            if !crossed.contains(&i) {
                                crossed.push(i);
                            }
                            if !out.iter().any(|q: &DVec2| (*q - x).length() < 1.5) {
                                out.push(x);
                            }
                        }
                    }
                }
            }
            self.crossings.insert(lane, out);
            self.crossed.insert(lane, crossed);
        }
        &self.crossings[&lane]
    }

    /// The carriageway lanes a pavement lane crosses.
    pub(super) fn crossed_lanes(&mut self, net: &Network, lane: usize) -> &[usize] {
        self.crossings(net, lane);
        &self.crossed[&lane]
    }
}

/// Seconds a pedestrian starting across `path` now has before a vehicle may drive over it:
/// until the first light of a carriageway lane it crosses (or of a lane leading into one)
/// turns green once the pedestrian green is over. Lanes that have green now, or get it
/// while the pedestrians still have theirs, are turning traffic that gives way. Without
/// such a light, the pedestrian green `green_left` and two seconds.
pub(super) fn pedestrian_window(
    ped: Option<&mut PedNet>,
    net: &Network,
    traffic: &Traffic,
    path: usize,
    green_left: f32,
) -> f32 {
    let mut window = f32::MAX;
    if let Some(ped) = ped {
        for &s in ped.crossed_lanes(net, path) {
            let feeding = net.prev.get(s).map(|p| p.as_slice()).unwrap_or(&[]);
            for &l in std::iter::once(&s).chain(feeding) {
                let Some((c, li)) = net.lanes[l].traffic_light else {
                    continue;
                };
                if let Some(g) = traffic
                    .light_until_go(c, li)
                    .filter(|g| *g > 0.0 && *g >= green_left)
                {
                    window = window.min(g);
                }
            }
        }
    }
    if window == f32::MAX {
        green_left + 2.0
    } else {
        window
    }
}

pub(super) fn seg_cross(a: DVec2, b: DVec2, c: DVec2, d: DVec2) -> Option<DVec2> {
    let r = b - a;
    let s = d - c;
    let den = r.perp_dot(s);
    if den.abs() < 1e-9 {
        return None;
    }
    let t = (c - a).perp_dot(s) / den;
    let u = (c - a).perp_dot(r) / den;
    (t >= 0.0 && t <= 1.0 && u >= 0.0 && u <= 1.0).then(|| a + r * t)
}

impl Humans {
    /// Hand motion to the pavement network; without a route, remain at the exit point.
    pub(super) fn walk_street(
        &mut self,
        i: usize,
        at: DVec3,
        heading: f64,
        stop: Option<i64>,
        net: Option<&Network>,
    ) {
        self.cancel_fare(self.people[i].id);
        let stop_lane = stop
            .and_then(|s| self.stops.get(&s))
            .and_then(|s| s.lane)
            .and_then(|(l, _)| {
                net.and_then(|n| n.lanes.get(l))
                    .and_then(|lane| lane.nearest_point(at))
                    .map(|(s, _)| (l, s))
            });
        let lane = stop_lane.or_else(|| {
            net.and_then(|n| {
                self.walking
                    .ped
                    .as_ref()
                    .and_then(|pn| pn.nearest(n, at, 25.0))
            })
            .map(|(l, s, _)| (l, s))
        });
        let r1 = self.rand();
        let r2 = self.rand();
        let p = &mut self.people[i];
        p.position = at;
        p.heading = heading;
        p.place = Place::Ground;
        p.interior = 0.0;
        p.vel = DVec2::ZERO;
        match (lane, self.walking.ped.as_ref(), net) {
            (Some((l, s)), Some(pn), Some(net)) if pn.lane_length(l).is_some() => {
                let len = pn.lane_length(l).unwrap();
                let fwd = if s <= 1.0 {
                    true
                } else if s >= len - 1.0 {
                    false
                } else {
                    (r1 & 1) == 0
                };
                let target_b = if fwd { len } else { 0.0 };
                let side = ((r2 % 7) as f32 - 3.0) * 0.12;
                let leg = Leg {
                    lane: l,
                    a: s,
                    b: target_b,
                };
                p.state =
                    State::Strolling(PedWalk::new(pn.departure_route(net, leg, r1), false, side));
                p.activity = Activity::Walk;
            }
            _ => {
                p.state = State::Standing;
                p.activity = Activity::Stand;
            }
        }
    }
}

pub(in crate::humans) struct PedestrianState {
    pub(in crate::humans) wall_cells: HashMap<(i32, i32, i32), Vec<(Block, f64, f64)>>,
    pub(in crate::humans) wall_key: (usize, usize, usize, f64),
    pub(in crate::humans) ped: Option<PedNet>,
    /// The first populate put people at the stops; later stops fill on foot when in sight.
    pub(in crate::humans) started: bool,
    pub(in crate::humans) stroll_timer: f32,
}

impl Humans {
    /// What pedestrian `i` wants this frame (task 8, `WalkStreet`).
    #[allow(clippy::too_many_arguments)]
    pub(super) fn decide(
        &mut self,
        i: usize,
        dt: f32,
        world: &World,
        net: Option<&Network>,
        traffic: Option<&Traffic>,
        cars: &[(DVec2, DVec2, f64)],
        remove: &mut Vec<usize>,
    ) -> Want {
        let state = self.people[i].state.clone();
        let pos2 = self.people[i].position.truncate();
        // somebody walking on towards ground that is not loaded goes
        if self.people[i].place == Place::Ground
            && self.people[i].vel.length_squared() > 1e-4
            && !world.has_ground(pos2.x, pos2.y)
        {
            remove.push(i);
            return Want::stand(None, Activity::Stand);
        }
        match state {
            State::Strolling(mut walk) => {
                let seen = self.seen(self.people[i].position);
                // (a stroller goes only once well out of everybody's range and out of sight)
                let far = self.far_from_players(self.people[i].position, STROLL_RADIUS * 2.0);
                let Some(net) = net else {
                    remove.push(i);
                    return Want::stand(None, Activity::Stand);
                };
                if far && !seen {
                    remove.push(i);
                    return Want::stand(None, Activity::Stand);
                }
                let w = self.walk_want(i, &mut walk, net, traffic, cars, dt);
                self.people[i].state = State::Strolling(walk);
                w
            }
            State::Standing => {
                if !self.seen(self.people[i].position)
                    && self.far_from_players(self.people[i].position, STROLL_RADIUS)
                {
                    remove.push(i);
                }
                Want::stand(None, Activity::Stand)
            }
            _ => Want::stand(None, Activity::Stand),
        }
    }

    /// Where a walk along the pavement takes somebody next.
    pub(super) fn walk_want(
        &mut self,
        i: usize,
        walk: &mut PedWalk,
        net: &Network,
        traffic: Option<&Traffic>,
        cars: &[(DVec2, DVec2, f64)],
        dt: f32,
    ) -> Want {
        let mut ped = self.walking.ped.take();
        let w = self.walk_want_with(ped.as_mut(), i, walk, net, traffic, cars, dt);
        self.walking.ped = ped;
        w
    }

    #[allow(clippy::too_many_arguments)]
    pub(super) fn walk_want_with(
        &mut self,
        mut ped: Option<&mut PedNet>,
        i: usize,
        walk: &mut PedWalk,
        net: &Network,
        traffic: Option<&Traffic>,
        cars: &[(DVec2, DVec2, f64)],
        dt: f32,
    ) -> Want {
        let pos2 = self.people[i].position.truncate();
        let pace = self.people[i].pace;
        if walk.leg >= walk.legs.len() {
            return Want::stand(None, Activity::Stand);
        }
        let leg = walk.legs[walk.leg];
        if walk.s >= leg.len() - 0.35 || walk.held > 0.0 {
            // at the end of the leg: which way on
            if walk.leg + 1 >= walk.legs.len() {
                if !walk.roam {
                    walk.leg += 1;
                    return Want::stand(None, Activity::Stand);
                }
                let pick = self.rand();
                let next = ped
                    .as_ref()
                    .and_then(|p| {
                        p.end_node(net, &leg)
                            .and_then(|n| p.next_leg(net, n, leg.lane, pick))
                    })
                    .unwrap_or(Leg {
                        lane: leg.lane,
                        a: leg.b,
                        b: leg.a,
                    });
                walk.legs.push(next);
                if walk.leg > 6 {
                    walk.legs.drain(..walk.leg);
                    walk.leg = 0;
                }
            }
            let next = walk.legs[walk.leg + 1];
            match self.may_cross(
                ped.as_deref_mut(),
                net,
                &next,
                traffic,
                cars,
                pace,
                walk.held,
            ) {
                Ok(()) => {
                    if walk.held > 0.0 && debug_pax() {
                        let light = net.lanes[next.lane].traffic_light.and_then(|(c, li)| {
                            traffic
                                .and_then(|t| t.light_state(c, li))
                                .map(|(st, left)| {
                                    format!(", light {c}.{li} state {st} for {left:.1} s more")
                                })
                        });
                        log::info!(
                            "t={:.1} pax {} crosses path {} after waiting {:.0} s{}",
                            self.time,
                            self.people[i].label(),
                            next.lane,
                            walk.held,
                            light.unwrap_or_default()
                        );
                    }
                    walk.s = (walk.s - leg.len()).max(0.0);
                    walk.leg += 1;
                    walk.held = 0.0;
                }
                Err(why) => {
                    walk.held += dt;
                    self.people[i].why = why;
                    // a light that stays red (nobody crosses on red any more): a stroller
                    // gives up after three minutes and walks back the way they came
                    if walk.roam && why == "red light" && walk.held > 180.0 {
                        if debug_pax() {
                            log::info!(
                                "t={:.1} pax {} gives up waiting at the red light and turns back",
                                self.time,
                                self.people[i].label()
                            );
                        }
                        walk.legs.truncate(walk.leg + 1);
                        walk.legs.push(Leg {
                            lane: leg.lane,
                            a: leg.b,
                            b: leg.a,
                        });
                        walk.held = 0.0;
                        return Want::stand(None, Activity::Stand);
                    }
                    // at the kerb, facing the way across, spread along it and a step back
                    let (end, _) = leg.at(net, leg.len());
                    let (_, h) = next.at(net, 0.3);
                    let hr = h.to_radians();
                    let (fwd, right) = (
                        DVec2::new(hr.sin(), hr.cos()),
                        DVec2::new(hr.cos(), -hr.sin()),
                    );
                    let id = self.people[i].id;
                    let spread = ((id % 5) as f64 - 2.0) * 0.45;
                    let back = 0.25 + (id % 3) as f64 * 0.45;
                    let spot = end.truncate() + right * spread - fwd * back;
                    return Want {
                        vel: arrive(pos2, spot, pace * 0.6),
                        face: Some(h),
                        give: 0.5,
                        corridor: None,
                        idle: Activity::Stand,
                    };
                }
            }
        }
        let leg = walk.legs[walk.leg];
        let len = leg.len();
        let (p, h) = leg.at(net, (walk.s + 1.3).min(len));
        let lane = &net.lanes[leg.lane];
        let width = (lane.width as f64).max(1.0);
        let crossing = lane.traffic_light.is_some()
            || ped
                .as_ref()
                .map(|p| {
                    p.crossings
                        .get(&leg.lane)
                        .map(|x| !x.is_empty())
                        .unwrap_or(false)
                })
                .unwrap_or(false);
        // keep to the right of the pavement (less so on a crossing) - the left where the
        // traffic drives on the left
        let side = if crossing {
            (walk.side.abs() as f64).min(0.3)
        } else {
            (walk.side.abs() as f64).min(width * 0.5 - 0.3).max(0.0)
        };
        let side = if net.left_hand { -side } else { side };
        let hr = h.to_radians();
        let right = DVec2::new(hr.cos(), -hr.sin());
        let target = p.truncate() + right * side;
        let vel = (target - pos2).normalize_or_zero() * pace;
        // stay on the path: the lane locally, as wide as it is
        let (a, _) = leg.at(net, (walk.s - 2.0).max(0.0));
        let (b, _) = leg.at(net, (walk.s + 2.5).min(len));
        let (m, _) = leg.at(net, (walk.s + 0.25).min(len));
        let bow = crowd::project_on_segment(m.truncate(), a.truncate(), b.truncate())
            .0
            .distance(m.truncate());
        let corridor = ((b - a).truncate().length() > 0.5).then(|| {
            (
                a.truncate() + right * side,
                b.truncate() + right * side,
                (width * 0.5 - side).max(0.35) + bow,
            )
        });
        if self.people[i].why != "queueing behind somebody" {
            self.people[i].why = "";
        }
        Want {
            vel,
            face: None,
            give: 1.0,
            corridor,
            idle: Activity::Stand,
        }
    }

    /// May a pedestrian at the kerb start along `next`? A pedestrian light must show green,
    /// and the time left to get across - the green and then the clearance until a light of
    /// the carriageway turns green - must do; without a light no car may be about to pass
    /// the crossing. Somebody who has waited very long takes any green (never a red).
    #[allow(clippy::too_many_arguments)]
    pub(super) fn may_cross(
        &self,
        mut ped: Option<&mut PedNet>,
        net: &Network,
        next: &Leg,
        traffic: Option<&Traffic>,
        cars: &[(DVec2, DVec2, f64)],
        pace: f64,
        held: f32,
    ) -> Result<(), &'static str> {
        if !next.from_end(net) {
            return Ok(());
        }
        let lane = &net.lanes[next.lane];
        let t_cross = next.len() as f64 / pace.max(0.5) + 1.0;
        if let (Some((c, li)), Some(t)) = (lane.traffic_light, traffic) {
            if let Some((state, left)) = t.light_state(c, li) {
                if !omsi_sim::traffic::TrafficLightController::allows_go(state) {
                    return Err("red light");
                }
                if held > 150.0 {
                    return Ok(());
                }
                // A pedestrian green is short (8 s at Grundorf for an 11.6 m crossing that
                // takes 10.7 s): who starts on green crosses in the clearance time after
                // it, until the cars get their green. Only the green alone was counted, so
                // nobody ever started on green and everybody went across on red after
                // 150 s, in front of moving cars.
                let window = pedestrian_window(ped.as_deref_mut(), net, t, next.lane, left);
                if (window as f64) < t_cross {
                    return Err("the green ends before they would be across");
                }
                return Ok(());
            }
        }
        let Some(ped) = ped else { return Ok(()) };
        // somebody who has waited long accepts a shorter gap (down to the time the crossing
        // takes, never less): a steady stream does not hold them for ever, but nobody walks
        // out in front of a car that is about to be there (after 45 s they used to ignore
        // the cars altogether)
        let margin = if held > 45.0 {
            0.0
        } else if held > 20.0 {
            1.0
        } else {
            2.5
        };
        for x in ped.crossings(net, next.lane) {
            for (p, v, half) in cars {
                let rel = *x - *p;
                let dist = rel.length();
                if dist > 90.0 {
                    continue;
                }
                if dist < half + 1.5 {
                    return Err("a vehicle stands on the crossing");
                }
                let speed = v.length();
                if speed < 0.5 {
                    continue;
                }
                let dir = *v / speed;
                let along = rel.dot(dir);
                let lateral = rel.perp_dot(dir).abs();
                if along > -half && lateral < 3.5 && along / speed < t_cross + margin {
                    return Err("waits for a car to pass");
                }
            }
        }
        Ok(())
    }

    /// Take over where the crowd moved person `i`.
    #[allow(clippy::too_many_arguments)]
    pub(super) fn apply(
        &mut self,
        i: usize,
        w: &Walker,
        want: &Want,
        dt: f32,
        world: &World,
        net: Option<&Network>,
        buses: &[BusNow],
        bus_ix: &HashMap<BusId, usize>,
    ) {
        let dt64 = dt as f64;
        let time = self.time;
        let p = &mut self.people[i];
        let speed = w.vel.length();
        // somebody pressed against somebody else for seconds slips past them
        if want.vel.length() > 0.2 && speed < 0.08 {
            p.stuck += dt;
        } else if speed > 0.2 {
            p.stuck = 0.0;
        }
        p.detour = (p.detour - dt).max(0.0);
        if p.ghost > 0.0 {
            p.ghost -= dt;
        } else if p.stuck > 2.5 {
            p.ghost = 1.5;
            p.stuck = 0.0;
            if debug_pax() {
                log::info!(
                    "t={time:.1} pax {} ({}) is stuck and slips past",
                    p.label(),
                    p.state.name()
                );
            }
        }
        p.vel = w.vel;
        match p.place {
            Place::Ground => {
                p.position.x = w.pos.x;
                p.position.y = w.pos.y;
                let spline_z = match (&mut p.state, net) {
                    (State::Strolling(walk), Some(net)) => {
                        if let Some(leg) = walk.legs.get(walk.leg) {
                            walk.s = leg.project(net, p.position, walk.s).max(walk.s - 0.3);
                            Some(leg.at(net, walk.s).0.z)
                        } else {
                            None
                        }
                    }
                    _ => None,
                };
                let target_z = match (
                    spline_z,
                    world.walk_height_near(p.position.x, p.position.y, p.position.z),
                ) {
                    (Some(sz), Some(wz)) if (wz - sz).abs() <= 0.15 => wz,
                    (Some(sz), Some(wz)) if wz < sz - 0.05 => sz,
                    (Some(sz), Some(wz)) if wz > sz + 0.35 => sz,
                    (Some(_), Some(wz)) => wz,
                    (Some(sz), None) => sz,
                    (None, Some(wz)) => wz,
                    (None, None) => p.position.z,
                };
                p.position.z = if target_z - p.position.z > 0.35 || speed < 0.05 {
                    target_z
                } else if target_z > p.position.z {
                    target_z.min(p.position.z + 1.5 * dt64)
                } else {
                    target_z.max(p.position.z - 2.0 * dt64)
                };
            }
            Place::Bus(b, l) => {
                let here = Vec3::new(w.pos.x as f32, w.pos.y as f32, l.z);
                let z = l.z;
                let local = Vec3::new(here.x, here.y, z);
                p.place = Place::Bus(b, local);
                if let Some(bn) = bus_ix.get(&b).map(|k| &buses[*k]) {
                    p.position = bn.world(local);
                    p.interior = bn.interior;
                    p.tilt = bn.tilt_at(local);
                }
            }
        }
        let walking = if p.activity == Activity::Walk {
            speed > 0.12
        } else {
            speed > 0.3
        };
        let activity = if walking { Activity::Walk } else { want.idle };
        let inside = matches!(p.place, Place::Bus(..));
        let bus_heading = match p.place {
            Place::Bus(b, l) => bus_ix
                .get(&b)
                .map(|k| buses[*k].heading_at(l))
                .unwrap_or(0.0),
            Place::Ground => 0.0,
        };
        let current = if inside { p.lheading } else { p.heading };
        let target = if speed > 0.25 {
            Some(crowd::heading_of(w.vel))
        } else {
            want.face
        };
        // turning eases in and out (a constant rate started and stopped with a jerk): the
        // rate follows the angle still to go, up to the most a walker or a stander turns
        let turned = match target {
            Some(t) => {
                let left = crowd::angle_diff(current, t);
                let max_rate = if walking { 260.0 } else { 140.0 };
                let rate = (left.abs() * 5.0).min(max_rate).max(12.0);
                crowd::turn_towards(current, t, rate, dt64)
            }
            None => current,
        };
        if inside {
            p.lheading = turned;
            p.heading = bus_heading + turned;
        } else {
            p.heading = turned;
        }
        p.activity = activity;
    }

    /// People carried by a bus in their seat.
    pub(super) fn carry(
        &mut self,
        i: usize,
        dt: f32,
        buses: &[BusNow],
        bus_ix: &HashMap<BusId, usize>,
        face: Option<f64>,
    ) {
        let p = &mut self.people[i];
        let Place::Bus(b, l) = p.place else { return };
        let Some(bn) = bus_ix.get(&b).map(|k| &buses[*k]) else {
            return;
        };
        p.position = bn.world(l);
        p.tilt = bn.tilt_at(l);
        p.vel = DVec2::ZERO;
        if let Some(f) = face {
            p.lheading = crowd::turn_towards(p.lheading, f, 150.0, dt as f64);
        }
        p.heading = bn.heading_at(l) + p.lheading;
        p.interior = bn.interior;
    }

    /// Nobody on foot walks into a wall: the scenery's collision boxes and meshes (shelters,
    /// fences, walls, buildings with a collision mesh) between knee and head height stop a
    /// step that would enter one, keeping the part of it along the wall. Somebody already
    /// inside one (a waiting place the map put in a shelter's box) is left alone - pushed
    /// out, they jumped. People used to walk through everything but the vehicles.
    pub(super) fn keep_out_of_walls(
        &mut self,
        world: &World,
        who: &[usize],
        ground: &mut [(usize, Walker)],
    ) {
        const R: f64 = 0.22;
        const CELL: f64 = 12.0;
        let collision = world.collision.lock();
        let places: Vec<DVec2> = world
            .waiting_places
            .lock()
            .iter()
            .map(|w| w.1.truncate())
            .collect();
        let (boxes, meshes, since) = (
            collision.boxes.len(),
            collision.meshes.len(),
            self.walking.wall_key.3,
        );
        if (boxes, meshes, places.len())
            != (
                self.walking.wall_key.0,
                self.walking.wall_key.1,
                self.walking.wall_key.2,
            )
            || self.time - since > 2.0
            || self.time < since
        {
            self.walking.wall_cells.clear();
            self.walking.wall_key = (boxes, meshes, places.len(), self.time);
        }
        let mut cells = std::mem::take(&mut self.walking.wall_cells);
        for (k, w) in ground.iter_mut() {
            let i = who[*k];
            if w.fixed || self.people[i].place != Place::Ground {
                continue;
            }
            let p0 = self.people[i].position.truncate();
            if (w.pos - p0).length_squared() < 1e-8 {
                continue;
            }
            let z = self.people[i].position.z;
            let key = (
                (p0.x / CELL).floor() as i32,
                (p0.y / CELL).floor() as i32,
                z.floor() as i32,
            );
            let walls = cells.entry(key).or_insert_with(|| {
                let c = DVec2::new((key.0 as f64 + 0.5) * CELL, (key.1 as f64 + 0.5) * CELL);
                let probe = omsi_sim::collision::Obb {
                    center: c,
                    half: DVec2::splat(CELL * 0.5 + 2.0),
                    heading: 0.0,
                    z0: key.2 as f64 - 1.0,
                    z1: key.2 as f64 + 3.5,
                    velocity: DVec2::ZERO,
                    mass: 0.0,
                    pole: None,
                    id: -1,
                };
                let near = collision.obstacles_near(&probe);
                let reach = near
                    .iter()
                    .map(|o| (o.center - c).length() + o.half.length() + 1.0)
                    .fold(0.0, f64::max);
                let local: Vec<DVec2> = places
                    .iter()
                    .copied()
                    .filter(|q| (*q - c).length() < reach)
                    .collect();
                near.into_iter()
                    .filter(|o| {
                        // a shelter given as one solid box has its waiting places inside:
                        // people go in there
                        let b = Block {
                            center: o.center,
                            half: o.half,
                            heading: o.heading,
                            vel: DVec2::ZERO,
                        };
                        !local.iter().any(|q| {
                            (*q - o.center).length() < o.half.length() + 1.0 && b.near(*q, 0.3)
                        })
                    })
                    .map(|o| {
                        (
                            Block {
                                center: o.center,
                                half: o.half + DVec2::splat(R),
                                heading: o.heading,
                                vel: DVec2::ZERO,
                            },
                            o.z0,
                            o.z1,
                        )
                    })
                    .collect()
            });
            for (b, z0, z1) in walls.iter() {
                // between the knees and the head of somebody standing here
                if *z0 > z + 1.6 || *z1 < z + 0.5 {
                    continue;
                }
                if (w.pos - b.center).length_squared() >= b.half.length_squared()
                    || !b.near(w.pos, 0.0)
                    || b.near(p0, -0.01)
                {
                    continue;
                }
                let (q, inside) = b.closest(w.pos);
                if !inside {
                    continue;
                }
                // onto the wall's face, keeping the step along it
                if omsi_cfg::env::var_os("OMSI_DEBUG_WALLS").is_some() {
                    log::info!(
                        "t={:.1} pax {} ({}) kept out of a wall ({:.1} x {:.1} m, heights {:.1}..{:.1}) at ({:.2}, {:.2}), its centre ({:.2}, {:.2}), want ({:.2}, {:.2}) vel ({:.2}, {:.2})",
                        self.time,
                        self.people[i].label(),
                        self.people[i].state.name(),
                        b.half.x * 2.0,
                        b.half.y * 2.0,
                        z0 - z,
                        z1 - z,
                        w.pos.x,
                        w.pos.y,
                        b.center.x,
                        b.center.y,
                        w.want.x,
                        w.want.y,
                        w.vel.x,
                        w.vel.y
                    );
                }
                let n = (q - w.pos).try_normalize().unwrap_or(DVec2::ZERO);
                w.pos = q + n * 0.005;
                let vn = w.vel.dot(n);
                if vn < 0.0 {
                    w.vel -= n * vn;
                }
                let fresh = self.people[i].detour <= 0.0;
                self.people[i].detour = 2.0;
                w.corridor = None;
                // walking straight at it: round it, the way that turns least from where they
                // want to go (a lamp post or a pillar stopped people dead)
                let speed = w.want.length();
                let t = DVec2::new(-n.y, n.x);
                if fresh || self.people[i].detour_side == 0.0 {
                    let along = w.want.dot(t);
                    self.people[i].detour_side = if along.abs() > 0.05 * speed {
                        along.signum()
                    } else if i % 2 == 0 {
                        1.0
                    } else {
                        -1.0
                    };
                }
                if speed > 0.2 && w.vel.dot(t) * self.people[i].detour_side < 0.4 * speed {
                    w.vel = t * self.people[i].detour_side * speed * 0.8;
                }
            }
        }
        self.walking.wall_cells = cells;
    }

    /// Keep strollers on the pavements near the player, and people walking up to the stops.
    pub(super) fn populate_on_foot(
        &mut self,
        world: &World,
        net: &Network,
        renderer: &Renderer,
        scene: &mut Scene,
        dt: f32,
    ) {
        let Some(ped) = self.walking.ped.take() else {
            return;
        };
        self.populate_on_foot_with(&ped, world, net, renderer, scene, dt);
        self.walking.ped = Some(ped);
    }

    pub(super) fn populate_on_foot_with(
        &mut self,
        ped: &PedNet,
        world: &World,
        net: &Network,
        renderer: &Renderer,
        scene: &mut Scene,
        _dt: f32,
    ) {
        let center = self.center;
        // strollers: as many as the pavement around carries
        let lanes: Vec<usize> = ped
            .ends
            .keys()
            .copied()
            .filter(|&l| {
                (net.lanes[l].start() - center).truncate().length() < STROLL_RADIUS * 0.9
                    && net.lanes[l].length() > 4.0
            })
            .collect();
        let crowd = (lanes.len() as f32 / 120.0).clamp(0.6, 3.0);
        let target =
            (self.pedestrians as f32 * crowd * self.density.clamp(0.0, 3.0)).round() as usize;
        let have = self
            .people
            .iter()
            .filter(|p| matches!(p.state, State::Strolling(_)))
            // (around this player only, when a LAN host keeps people around several)
            .filter(|p| {
                self.lan_centers.is_empty() || (p.position - center).length() < STROLL_RADIUS
            })
            .count();
        if have < target && !lanes.is_empty() {
            for _ in 0..(target - have).min(4) {
                let lane = lanes[(self.rand() as usize) % lanes.len()];
                let len = net.lanes[lane].length();
                let s = (self.rand_f() as f32 * (len - 1.0)).max(0.5);
                let (p, h) = net.lanes[lane].at(s);
                if self.seen(p)
                    || (p - center).length() < 20.0
                    || (p - center).length() > STROLL_RADIUS * 0.9
                    || !world.has_ground(p.x, p.y)
                {
                    continue;
                }
                let fwd = self.rand_f() < 0.5;
                let leg = if fwd {
                    Leg { lane, a: s, b: len }
                } else {
                    Leg { lane, a: s, b: 0.0 }
                };
                let side = 0.3 + self.rand_f() as f32 * 0.4;
                let heading = if fwd { h as f64 } else { h as f64 + 180.0 };
                if let Some(i) = self.spawn(
                    world,
                    renderer,
                    scene,
                    p,
                    heading,
                    State::Strolling(PedWalk::new(vec![leg], true, side)),
                ) {
                    self.people[i].activity = Activity::Walk;
                }
            }
        }
        // OMSI_PAX_CROSS=x,y: a few pedestrians sent across the signalised crossing nearest that point
        if let Some((x, y)) = omsi_cfg::env::var("OMSI_PAX_CROSS").ok().and_then(|v| {
            let mut it = v.split(',').filter_map(|t| t.trim().parse::<f64>().ok());
            Some((it.next()?, it.next()?))
        }) {
            let want = DVec3::new(x, y, center.z);
            let placed = self
                .people
                .iter()
                .filter(|p| matches!(p.state, State::Strolling(ref w) if !w.roam || w.side < 0.0))
                .count();
            let lane = ped
                .ends
                .keys()
                .copied()
                .filter(|&l| net.lanes[l].traffic_light.is_some())
                .min_by(|a, b| {
                    (net.lanes[*a].start() - want)
                        .truncate()
                        .length()
                        .total_cmp(&(net.lanes[*b].start() - want).truncate().length())
                });
            if let (Some(cross), 0) = (lane, placed) {
                let (start_node, _) = ped.ends[&cross];
                let feeders: Vec<(usize, bool)> = ped.out[start_node]
                    .iter()
                    .copied()
                    .filter(|(l, _)| *l != cross)
                    .collect();
                log::info!(
                    "OMSI_PAX_CROSS: crossing path {cross} light {:?}, {} paths lead to it",
                    net.lanes[cross].traffic_light,
                    feeders.len()
                );
                for k in 0..6 {
                    let Some(&(lane, fwd)) = feeders.get(k % feeders.len().max(1)) else {
                        break;
                    };
                    let len = net.lanes[lane].length();
                    let back = (3.0 + k as f32 * 1.6).min(len);
                    let first = if fwd {
                        Leg {
                            lane,
                            a: back,
                            b: 0.0,
                        }
                    } else {
                        Leg {
                            lane,
                            a: len - back,
                            b: len,
                        }
                    };
                    let over = Leg {
                        lane: cross,
                        a: 0.0,
                        b: net.lanes[cross].length(),
                    };
                    let (p, h) = first.at(net, 0.0);
                    let mut walk = PedWalk::new(vec![first, over], true, 0.4);
                    // marked so that the knob spawns them once
                    walk.side = -0.4;
                    if let Some(i) =
                        self.spawn(world, renderer, scene, p, h, State::Strolling(walk))
                    {
                        self.people[i].activity = Activity::Walk;
                    }
                }
            }
        }
    }
}
