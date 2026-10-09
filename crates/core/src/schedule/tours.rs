//! Days, tours and their handover from trip to trip; the steps and slots of a trip.

use super::*;

impl Schedule {
    /// When departure `i` leaves on the traffic's clock.
    pub(super) fn dep_time(&self, i: usize) -> f64 {
        self.day_base + self.departures[i].time
    }

    /// Past midnight on the traffic's clock: the next day's timetable.
    pub(super) fn roll_day(&mut self, day_time: f64) {
        while day_time - self.day_base >= DAY {
            self.day_base += DAY;
            let mut c = self.date_clock.clone();
            c.paused = false;
            c.advance(DAY as f32);
            self.date_clock = c;
            let c = self.date_clock.clone();
            self.set_day(&c);
        }
    }

    /// Whether a tour is offered on the current day (the lists of lines and tours): its day
    /// mask has the day (and school day or holiday), or - for a night tour with trips after
    /// 24:00 - the next day's weekday, as the night belongs to both.
    pub(crate) fn tour_available(&self, tour: &::timetable::Tour) -> bool {
        let m = tour.extra.trim().parse::<i32>().unwrap_or(1023);
        if m & self.day_bits.0 != 0 && m & self.day_bits.1 != 0 {
            return true;
        }
        let night = tour.trips.iter().any(|t| t.departure >= 24.0 * 60.0);
        night && m & self.next_day_bit != 0 && m & self.day_bits.1 != 0
    }

    /// Whether departure `i`'s tour runs on the current day.
    pub(super) fn runs(&self, i: usize) -> bool {
        let m = self.departures[i].mask;
        m & self.day_bits.0 != 0 && m & self.day_bits.1 != 0
    }

    /// Move the timetable on to the clock's date when it has changed (midnight): the day's
    /// tours are chosen anew and every departure may run again. Before, the departures were
    /// made once for the start day and stayed spawned, so after the first midnight only the
    /// early-morning trips before the start time came, and on the old day's tours.
    /// Departures still queued or under way keep their state; the player's tour stays taken.
    pub(super) fn set_day(&mut self, clock: &::simulation::SimClock) {
        let date = clock.date_code();
        if date == self.day {
            return;
        }
        self.day = date;
        self.day_bits = day_bits(&self.calendar, clock);
        self.next_day_bit = 1 << ((clock.weekday() + 1) % 7);
        let busy: HashSet<usize> = self
            .pending
            .iter()
            .chain(self.waiting.iter())
            .chain(self.awaiting.iter())
            .chain(self.later_layover.iter())
            .chain(self.car_departure.values())
            .copied()
            .collect();
        let mut n = 0;
        for i in 0..self.departures.len() {
            if busy.contains(&i) {
                continue;
            }
            let mine = self.is_player_tour(i);
            let d = &mut self.departures[i];
            if d.spawned && !mine {
                d.spawned = false;
                n += 1;
            }
        }
        self.startup.clear();
        self.boards_made = f64::NEG_INFINITY;
        self.assign_car_use();
        let today = (0..self.departures.len()).filter(|&i| self.runs(i)).count();
        log::info!(
            "timetable: a new day ({date}): {today} departures today, {n} made ready to run again"
        );
    }

    /// Reset non-player timetable activity after the game clock is changed manually.
    pub fn refresh_time(&mut self, clock: &::simulation::SimClock, day_time: f64) {
        self.day = clock.date_code();
        self.day_bits = day_bits(&self.calendar, clock);
        self.next_day_bit = 1 << ((clock.weekday() + 1) % 7);
        self.day_base = day_time - clock.time;
        self.date_clock = clock.clone();
        self.last_tod = clock.time;
        for i in 0..self.departures.len() {
            if !self.is_player_tour(i) {
                self.departures[i].spawned = false;
            }
        }
        self.pending.clear();
        self.waiting.clear();
        self.running.clear();
        self.car_departure.clear();
        self.retry_at.clear();
        self.startup.clear();
        self.later_layover.clear();
        self.awaiting.clear();
        self.shared_stand.clear();
        self.purge_player_tour = false;
        self.seen_generation = 0;
        self.last_retry = f64::NEG_INFINITY;
        self.fleet_check = f64::NEG_INFINITY;
        self.boards_made = f64::NEG_INFINITY;
        self.next_number = 0;
        self.fleet_reading.clear();
        self.fleet_ready.lock().clear();
        self.assign_car_use();
    }

    /// The timetable bus on the road that runs departure `k` (not one that has been let go).
    pub(super) fn tour_bus(&self, k: usize, traffic: &Traffic) -> Option<usize> {
        traffic
            .cars
            .iter()
            .position(|c| c.is_bus() && !c.gone && self.car_departure.get(&c.id) == Some(&k))
    }

    /// The timetable buses at the end of their trip: each takes its tour's next trip on
    /// where it stands, or goes.
    pub(super) fn tour_handover(
        &mut self,
        world: &World,
        traffic: &mut Traffic,
        renderer: &Renderer,
        scene: &mut Scene,
        day_time: f64,
    ) {
        let done: Vec<u64> = traffic
            .cars
            .iter()
            .filter(|c| c.trip_done())
            .map(|c| c.id)
            .collect();
        for id in done {
            let Some(ci) = traffic.cars.iter().position(|c| c.id == id) else {
                continue;
            };
            let next = self.car_departure.get(&id).and_then(|&k| self.tour_next[k]);
            let mut taken = false;
            if let Some(j) = next {
                let d = &self.departures[j];
                let open = self.awaiting.contains(&j) || (!d.spawned && self.runs(j));
                if open
                    && !self.is_player_tour(j)
                    && self.day_base + d.time - day_time < TOUR_LAYOVER_MAX
                {
                    if let Placed::Spawned =
                        self.spawn_departure(j, world, traffic, renderer, scene, day_time, Some(ci))
                    {
                        taken = true;
                        self.departures[j].spawned = true;
                        self.awaiting.remove(&j);
                        self.pending.retain(|x| *x != j);
                        self.waiting.retain(|x| *x != j);
                        self.retry_at.remove(&j);
                        self.later_layover.remove(&j);
                    }
                }
                // its bus is not coming: the trip gets a bus of its own
                if !taken && self.awaiting.remove(&j) {
                    self.pending.push_back(j);
                }
            }
            if !taken && next.is_none() {
                // the tour's last trip is over: Omsi takes the bus (and what is coupled to
                // it) off the road at once rather than letting it drive on
                traffic.remove_car(world, renderer, scene, id);
                self.car_departure.remove(&id);
                if ::legacy_config::env::var_os("OMSI_DEBUG_TRAFFIC").is_some() {
                    log::info!("scheduled bus {id}: the last trip of its tour is over: removed");
                }
            } else if !taken {
                traffic.release(ci);
                if ::legacy_config::env::var_os("OMSI_DEBUG_TRAFFIC").is_some() {
                    log::info!(
                        "scheduled bus {id}: trip over, no next trip of its tour to take on here: it drives off"
                    );
                }
            }
        }
        // a trip whose tour's bus has gone off the road meanwhile
        if !self.awaiting.is_empty() {
            let orphans: Vec<usize> = self
                .awaiting
                .iter()
                .copied()
                .filter(|&j| {
                    self.tour_prev[j]
                        .and_then(|k| self.tour_bus(k, traffic))
                        .is_none()
                })
                .collect();
            for j in orphans {
                self.awaiting.remove(&j);
                self.pending.push_back(j);
            }
        }
    }

    /// The steps of a trip's route: from the trip's own track when it has one (trains,
    /// ferries, planes), else from the station links between its stops. The flag says it is
    /// a track.
    ///
    /// The track is the one the trip's `[trip]` block names on its first line (Novi Sad's
    /// trip "1 Klisa-Liman I" runs track "1_Klisa-Liman1"; the stock trains name tracks of
    /// their own name), else the one named like the trip.
    pub(super) fn steps_of(&self, track_name: &str, stations: &[i64]) -> (Vec<Step>, bool) {
        let track_name = self
            .data
            .trip(track_name)
            .map(|t| t.display_name.trim())
            .filter(|n| !n.is_empty())
            .unwrap_or(track_name);
        let key = |id: f64, path: f64, tile_index: f64| {
            self.tile_coords
                .get(tile_index as usize)
                .map(|&tile| LaneKey {
                    tile,
                    id: id as i64,
                    path: path as u16,
                })
        };
        let mut steps: Vec<Step> = Vec::new();
        if let Some(track) = self.data.tracks.iter().find(|t| {
            t.path
                .file_stem()
                .map(|s| s.to_string_lossy().eq_ignore_ascii_case(track_name))
                .unwrap_or(false)
        }) {
            steps.extend(
                track
                    .entries
                    .iter()
                    .filter(|e| e.values.len() >= 5)
                    .map(|e| Step {
                        key: key(e.values[0], e.values[1], e.values[2]),
                        leg: 0,
                        length: e.values[4],
                    }),
            );
            return (steps, true);
        }
        for (leg, w) in stations.windows(2).enumerate() {
            match self
                .data
                .stn_links
                .iter()
                .find(|l| l.from_id == w[0] && l.to_id == w[1])
            {
                Some(link) => {
                    for e in &link.entries {
                        let k = key(e.values[0], e.values[1], e.values[2]);
                        // consecutive links repeat the shared lane
                        if steps.last().map(|s| s.key == k).unwrap_or(false) {
                            continue;
                        }
                        steps.push(Step {
                            key: k,
                            leg,
                            length: e.values[3],
                        });
                    }
                }
                None => log::debug!("trip {track_name}: no station link {} -> {}", w[0], w[1]),
            }
        }
        (steps, false)
    }

    /// One-way paths that a route drives the other way - their end lies where the path
    /// before it ends, their start where the next one begins - get a lane that way
    /// (`Traffic::add_reverse_twins`). OMSI's timetable buses follow their station links
    /// and tracks whichever way a path runs: Spandau's line to Kladow and a dozen Novi Sad
    /// tracks run over invisible one-way helper streets backwards, and the bus drove them
    /// forwards, against its route, and jumped back at their end.
    pub(super) fn add_twins(traffic: &mut Traffic, steps: &[Step]) {
        let net = &traffic.net;
        let cands: Vec<Option<&Vec<usize>>> = steps
            .iter()
            .map(|st| {
                st.key
                    .and_then(|k| net.by_key.get(&k))
                    .filter(|c| !c.is_empty())
            })
            .collect();
        let ends = |c: &Vec<usize>| -> Vec<glam::DVec3> {
            c.iter()
                .flat_map(|&l| [net.lanes[l].start(), net.lanes[l].end()])
                .collect()
        };
        let near = |p: glam::DVec3, pts: &[glam::DVec3]| {
            pts.iter()
                .map(|q| (*q - p).truncate().length())
                .fold(f64::MAX, f64::min)
        };
        let mut want = Vec::new();
        for (i, c) in cands.iter().enumerate() {
            // a path that runs both ways has its lanes already
            let Some(c) = c.filter(|c| c.len() == 1) else {
                continue;
            };
            let l = &net.lanes[c[0]];
            let prev = i.checked_sub(1).and_then(|k| cands[k]).map(ends);
            let next = cands.get(i + 1).copied().flatten().map(ends);
            if prev.is_none() && next.is_none() {
                continue;
            }
            let score = |a: glam::DVec3, b: glam::DVec3| {
                prev.as_ref().map(|p| near(a, p)).unwrap_or(0.0)
                    + next.as_ref().map(|n| near(b, n)).unwrap_or(0.0)
            };
            let (fwd, bwd) = (score(l.start(), l.end()), score(l.end(), l.start()));
            if bwd + 3.0 < fwd && bwd < 6.0 {
                want.push(c[0]);
            }
        }
        if !want.is_empty() {
            traffic.add_reverse_twins(&want);
        }
    }

    /// Where a route's lanes do not join and the network has no way between them either,
    /// a connector lane across the gap (`Traffic::add_connector`), so that `bridge_gaps`
    /// finds a way to drive.
    pub(super) fn add_connectors(traffic: &mut Traffic, lanes: &[usize]) {
        let net = &traffic.net;
        let holes: Vec<(usize, usize)> = lanes
            .windows(2)
            .filter(|w| !joins(net, w[0], w[1]))
            .filter(|w| {
                let gap = (net.lanes[w[1]].start() - net.lanes[w[0]].end())
                    .truncate()
                    .length();
                way_between(net, w[0], w[1], (gap * 2.5 + 60.0) as f32).is_none()
            })
            .map(|w| (w[0], w[1]))
            .collect();
        for (a, b) in holes {
            traffic.add_connector(a, b);
        }
    }
}
