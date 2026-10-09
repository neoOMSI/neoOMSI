//! `PlayerDuty`: the progress of the player's bus through its trips and stops.

use super::*;

impl PlayerDuty {
    pub fn trip(&self) -> &PlannedTrip {
        &self.trips[self.trip_index]
    }

    pub(crate) fn take_completed_report(&mut self) -> Option<crate::run_statistics::Report> {
        self.completed_report.take()
    }

    pub fn trip_done(&self) -> bool {
        self.done
    }

    /// Whether the player has completed the final trip of the tour.  A completed trip with
    /// another one waiting is only a layover and must keep the duty active.
    pub fn duty_done(&self) -> bool {
        self.done && self.trip_index + 1 == self.trips.len()
    }

    /// Service/depot legs have no public line and use the HOF's
    /// `Betriebsfahrt` destination. They remain part of the duty, but the
    /// player's IBIS should use the next public leg while the bus is waiting.
    pub fn trip_for_ibis(&self) -> (&PlannedTrip, usize) {
        let current = self.trip();
        if !current.line.trim().is_empty() {
            return (
                current,
                self.next_stop.min(current.stops.len().saturating_sub(1)),
            );
        }
        let next = self
            .trips
            .iter()
            .skip(self.trip_index + 1)
            .find(|trip| !trip.line.trim().is_empty());
        match next {
            Some(trip) => (trip, 0),
            None => (
                current,
                self.next_stop.min(current.stops.len().saturating_sub(1)),
            ),
        }
    }

    /// Whether the current trip changed since the last call (the IBIS wants the new one).
    pub fn take_trip_change(&mut self) -> bool {
        std::mem::take(&mut self.trip_changed)
    }

    /// Places of stops the timetable did not know (their tiles were not loaded when the duty
    /// was made): the navigator reads the whole map.
    pub fn learn_places(&mut self, places: &HashMap<i64, glam::DVec3>) {
        for trip in &mut self.trips {
            for s in &mut trip.stops {
                if s.position.is_none() {
                    s.position = places.get(&s.object_id).copied();
                }
            }
            // (a stop that only now has a place gives its neighbours their direction)
            trip.set_dirs();
        }
    }

    /// Time to drive from `pos` to `to` (s), roughly: roads are longer than the straight
    /// line, a bus in town makes some 25 km/h, and it takes a minute or two to get going.
    pub(super) fn approach_time(pos: glam::DVec3, to: glam::DVec3) -> f64 {
        let d = (to - pos).truncate().length();
        if d < AT_STOP {
            return 0.0;
        }
        d * 1.35 / 7.0 + 60.0
    }

    /// Where the bus starts: at the stop of the trip under way it stands at; else with the
    /// first trip of the tour whose first stop it can reach before that trip leaves (a
    /// duty picked for 08:00 with the bus in the depot used to start with the trip under
    /// way at 08:00, led the driver to whatever stop that trip was due at next - halfway
    /// along the line - and ran late from the first second). When no trip of the tour
    /// can be reached in time any more, the last one is driven from its first stop that
    /// can (else its first stop), late as that is.
    pub(super) fn place(&mut self, pos: glam::DVec3, now: f64) {
        let trip = &self.trips[self.trip_index];
        // of two stops within AT_STOP of the bus - the two sides of a street on a circular
        // route - the one the bus drives the way the trip runs through it, else the nearer
        let fwd = forward_of(self.heading);
        let near: Vec<(usize, f64)> = trip
            .stops
            .iter()
            .enumerate()
            .filter_map(|(k, s)| s.position.map(|p| (k, (p - pos).length())))
            .filter(|(_, d)| *d < AT_STOP)
            .collect();
        let nearest = |v: Vec<(usize, f64)>| v.into_iter().min_by(|a, b| a.1.total_cmp(&b.1));
        let near = nearest(
            near.iter()
                .copied()
                .filter(|&(k, _)| trip.stops[k].dir.takes(fwd))
                .collect(),
        )
            .or_else(|| nearest(near));
        if near.is_none() && !self.picked {
            let reachable = (self.trip_index..self.trips.len()).find(|&k| {
                let t = &self.trips[k];
                let first = t.stops.first().and_then(|s| s.position);
                // (a first stop nobody knows the place of: ten minutes)
                let need = first.map(|p| Self::approach_time(pos, p)).unwrap_or(600.0);
                t.departure >= now + need
            });
            match reachable {
                Some(k) if k != self.trip_index => {
                    log::info!(
                        "duty: trip {} ({}) cannot be reached in time from here; starting with trip {} ({}) at {}",
                        self.trip_index + 1,
                        trip.name,
                        k + 1,
                        self.trips[k].name,
                        hhmm(self.trips[k].departure)
                    );
                    self.set_trip(k);
                    self.trip_changed = true;
                }
                Some(_) => {}
                None => {
                    let t = &self.trips[self.trip_index];
                    self.next_stop = t
                        .stops
                        .iter()
                        .position(|s| {
                            s.stops
                                && s.position
                                .map(|p| s.arr >= now + Self::approach_time(pos, p))
                                .unwrap_or(false)
                        })
                        .unwrap_or(0);
                    log::info!(
                        "duty: no trip of the tour can be reached in time; trip {} ({}) from stop {} '{}'",
                        self.trip_index + 1,
                        t.name,
                        self.next_stop,
                        t.stops
                            .get(self.next_stop)
                            .map(|s| s.name.as_str())
                            .unwrap_or("")
                    );
                    return;
                }
            }
        }
        let trip = &self.trips[self.trip_index];
        if trip.departure >= now {
            log::info!(
                "duty: trip {} ({}) leaves {} at {:.0} s",
                self.trip_index + 1,
                trip.name,
                trip.stops.first().map(|s| s.name.as_str()).unwrap_or(""),
                trip.departure
            );
            return;
        }
        self.next_stop = match near {
            Some((k, _)) => k,
            // a trip the player picked is driven from its first stop, late as it may be
            None if self.picked => self.next_stop,
            None => trip
                .stops
                .iter()
                .position(|s| s.arr >= now)
                .unwrap_or(trip.stops.len().saturating_sub(1)),
        };
        // standing at a stop of it, the bus is on its way (as if it had left the stop before
        // on time); elsewhere the duty goes on with the next trip when that is due
        self.left_late = near.map(|_| 0.0);
        log::info!(
            "duty: the bus starts {} trip {} ({}) under way, next stop {} '{}'",
            if near.is_some() {
                "at a stop of"
            } else {
                "away from the stops of"
            },
            self.trip_index + 1,
            trip.name,
            self.next_stop,
            trip.stops
                .get(self.next_stop)
                .map(|s| s.name.as_str())
                .unwrap_or("")
        );
    }

    /// The trip a bus placed at its first stop starts with: the one under way or picked,
    /// else the first to leave from now on (the last when all have left).
    pub fn start_trip(&self, now: f64) -> usize {
        if self.picked {
            return self.trip_index;
        }
        (self.trip_index..self.trips.len())
            .find(|&k| self.trips[k].departure >= now)
            .unwrap_or(self.trip_index)
    }

    /// The duty starts with trip `k` at its stop `stop` (`duty_start`: the stops before it
    /// cannot be reached by road): that trip is the duty's first, driven from there.
    pub fn start_at(&mut self, k: usize, stop: usize) {
        if k < self.trips.len() {
            self.set_trip(k);
            self.picked = true;
            self.trip_changed = true;
            self.next_stop = stop.min(self.trips[k].stops.len().saturating_sub(1));
        }
    }

    /// Like `start_at`, for a bus that stays where it is (the stop was chosen in the menu,
    /// the bus is not put there): the first update does not look where the bus stands and
    /// does not move the chosen stop to one it happens to be near.
    pub fn start_at_here(&mut self, k: usize, stop: usize) {
        self.start_at(k, stop);
        self.placed = true;
    }

    /// A page sets the stop the duty goes on with (`omsi.setNextStop`), forwards or
    /// backwards: skipped stops count as not served, and going back makes the stops from
    /// `stop` on due again. Not once the trip's last stop is reached (`done`), unless the
    /// page goes back, which reopens the trip.
    pub fn skip_to(&mut self, stop: usize) -> bool {
        let last = self.trip().stops.len().saturating_sub(1);
        let stop = stop.min(last);
        log::debug!(
            "duty: page asks for stop {stop} (next {}, at_stop {}, done {})",
            self.next_stop,
            self.at_stop,
            self.done
        );
        if stop == self.next_stop && !self.done {
            return false;
        }
        if self.done && stop >= self.next_stop {
            return false;
        }
        let back = stop < self.next_stop;
        if !back {
            self.statistics.skip(self.next_stop, stop);
        }
        self.next_stop = stop;
        self.at_stop = false;
        self.arrived_late = None;
        if back {
            self.done = false;
            self.held_back = true;
        }
        true
    }

    pub(super) fn set_trip(&mut self, index: usize) {
        self.trip_index = index;
        self.statistics = Default::default();
        self.next_stop = 0;
        self.at_stop = false;
        self.done = false;
        self.left_late = None;
        self.held_back = false;
        self.trip_changed = true;
        self.picked = false;
    }

    /// How late the bus is (s; negative = early), as the IBIS shows it: at a stop against
    /// its departure there, on the way at least as late as it left the last stop and later
    /// once the next one is overdue, and at the end of a trip against the next trip's start.
    pub fn delay(&self, now: f64) -> f64 {
        let now = self.duty_time(now);
        let trip = self.trip();
        if self.done {
            if let Some(next) = self.trips.get(self.trip_index + 1) {
                return now - next.departure;
            }
        }
        let Some(stop) = trip.stops.get(self.next_stop) else {
            return 0.0;
        };
        if self.at_stop {
            return now - stop.dep;
        }
        let due = now - stop.arr;
        self.left_late.map(|l| l.max(due)).unwrap_or(due)
    }

    /// The clock's time of day as the duty counts it: the day before or after when that is
    /// nearer the trip under way, so a duty across midnight (picked at 23:00 for trips from
    /// 0:49, or running from 23:40 into the night) is neither 22 hours late nor early.
    pub(super) fn duty_time(&self, day_time: f64) -> f64 {
        let t = self.trip();
        let centre = (t.departure + t.end) / 2.0;
        [day_time - DAY, day_time, day_time + DAY]
            .into_iter()
            .min_by(|a, b| (a - centre).abs().total_cmp(&(b - centre).abs()))
            .unwrap_or(day_time)
    }

    /// Advance the duty and feed the vehicle host's timetable callbacks. Returns how late
    /// the bus left a stop, at the moment it leaves it (negative = early), which is what
    /// the personnel file counts.
    /// Returns, when the bus has just left a stop it had to serve, how late it arrived
    /// there and how late it left (seconds; negative: early).
    pub fn update(
        &mut self,
        bus: &mut ::simulation::VehicleInstance,
        day_time: f64,
    ) -> Option<(f64, f64)> {
        let day_time = self.duty_time(day_time);
        self.heading = bus.heading;
        let served = self.advance(bus.position, day_time);
        let delay = self.delay(day_time);
        let trip = &self.trips[self.trip_index];
        let host = &mut bus.host;
        host.tt_line = trip.line.clone();
        host.tt_stops = trip
            .stops
            .iter()
            .map(|s| (s.name.clone(), s.arr as f32, s.dep as f32))
            .collect();
        host.tt_stop_ids = trip.stops.iter().map(|s| s.object_id).collect();
        host.tt_busstop_index = self.next_stop as i32;
        host.tt_terminus_index = tt_terminus_index(host.hof.as_deref(), &trip.terminus);
        host.tt_delay = delay as f32;
        served
    }

    /// The bus came to a later stop of the trip than the one it is due at (it drove past
    /// some). Which stop that is cannot be told by the distance alone: a circular route, or
    /// one that turns back, calls at the same place twice, and its two stops there stand a
    /// few metres apart, so a bus at one is within [`AT_STOP`] of the other as well - the
    /// duty jumped from stop 2 to stop 18 and 3-17 were never served (#254). Three things
    /// have to agree: the trip runs through the stop the way the bus heads ([`StopDir`]);
    /// the bus stands nearer to it than to the stop it is due at; and it has driven away
    /// from the stop it served last.
    pub(super) fn catch_up(&mut self, pos: glam::DVec3, fwd: glam::DVec2) {
        if self.held_back {
            return;
        }
        let trip = &self.trips[self.trip_index];
        let last = trip.stops.len().saturating_sub(1);
        let upto = if self.left_late.is_some() {
            trip.stops.len()
        } else {
            last
        };
        if self.next_stop + 1 >= upto {
            return;
        }
        let of = |k: usize| -> Option<f64> {
            trip.stops
                .get(k)
                .and_then(|s| s.position)
                .map(|p| (p - pos).length())
        };
        // still at the stop it served: too early to look for a later one
        if let Some(k) = self.next_stop.checked_sub(1) {
            if self.odo - self.left_odo < LEFT_STOP && of(k).is_some_and(|d| d <= AT_STOP) {
                return;
            }
        }
        let here = of(self.next_stop);
        let mut best: Option<(usize, f64)> = None;
        for k in self.next_stop + 1..upto {
            let Some(d) = of(k) else { continue };
            if d >= AT_STOP || here.is_some_and(|h| d >= h) || !trip.stops[k].dir.takes(fwd) {
                continue;
            }
            if best.is_none_or(|(_, b)| d < b) {
                best = Some((k, d));
            }
        }
        let Some((k, _)) = best else { return };
        log::info!(
            "duty: trip {}: {} stop(s) passed without stopping, the bus at stop {} '{}' (it was due at {})",
            trip.name,
            k - self.next_stop,
            k + 1,
            trip.stops[k].name.trim(),
            self.next_stop + 1
        );
        self.statistics.skip(self.next_stop, k);
        self.next_stop = k;
    }

    /// The duty's progress with the bus at `pos` (see [`PlayerDuty::update`]).
    pub(super) fn advance(&mut self, pos: glam::DVec3, day_time: f64) -> Option<(f64, f64)> {
        // the path the bus drove (a jump, e.g. a teleport, does not count)
        if let Some(last) = self.last_pos {
            let step = (pos - last).length();
            if step < 100.0 {
                self.odo += step;
            }
        }
        self.last_pos = Some(pos);
        if !self.placed {
            // (stops beyond the loaded tiles have no place yet: a few seconds for the
            // navigator's map, unless the bus stands at a stop of its trip)
            let first = *self.first_update.get_or_insert(day_time);
            let at_a_stop = self.trip().stops.iter().any(|s| {
                s.position
                    .map(|p| (p - pos).length() < AT_STOP)
                    .unwrap_or(false)
            });
            let known = self.trips[self.trip_index..].iter().all(|t| {
                t.stops
                    .first()
                    .map(|s| s.position.is_some())
                    .unwrap_or(true)
            });
            if !(at_a_stop || known || (day_time - first).abs() > 12.0) {
                return None;
            }
            self.placed = true;
            self.place(pos, day_time);
        }
        // on to the next trip a minute before it leaves, once this one is over, was never
        // begun, or was given up half an hour ago
        while self.trip_index + 1 < self.trips.len()
            && self.trips[self.trip_index + 1].departure - 60.0 <= day_time
        {
            let given_up = day_time > self.trip().end + 1800.0;
            let unbegun = self.left_late.is_none() && !self.picked;
            if !(self.done || unbegun || given_up) {
                break;
            }
            self.set_trip(self.trip_index + 1);
            log::info!(
                "duty: trip {} {} to {}",
                self.trip_index + 1,
                self.trip().name,
                self.trip().terminus
            );
        }
        let mut served = None;
        let trip = &self.trips[self.trip_index];
        let last = trip.stops.len().saturating_sub(1);
        // the bus reached a later stop of the trip (skipped stops); before it has left a
        // stop of the trip, not its last one (where the tour's previous trip may end)
        if !self.at_stop {
            self.catch_up(pos, forward_of(self.heading));
        }
        let trip = &self.trips[self.trip_index];
        // stop progress by proximity
        if let Some(stop) = trip.stops.get(self.next_stop) {
            if let Some(p) = stop.position {
                let d = (p - pos).length();
                if d < AT_STOP {
                    self.held_back = false;
                    if !self.at_stop {
                        self.arrived_late = Some(day_time - stop.arr);
                        self.arrival_odo = self.odo;
                        self.statistics.arrive(self.next_stop, day_time);
                    }
                    self.at_stop = true;
                    if self.next_stop == last && !self.done {
                        self.completed_report =
                            Some(self.statistics.report(trip, &self.tour, true));
                        self.done = true;
                    }
                } else if self.at_stop && self.odo - self.arrival_odo > LEFT_STOP {
                    // (driven on past the stop along the road, whatever the straight line says)
                    self.left_odo = self.odo;
                    self.at_stop = false;
                    let late = day_time - stop.dep;
                    self.statistics.depart(self.next_stop, day_time);
                    self.left_late = Some(late);
                    // (OMSI counts a stop only with its arrival: the original)
                    if let (true, Some(arrived)) = (stop.stops, self.arrived_late.take()) {
                        served = Some((arrived, late));
                    }
                    self.next_stop = (self.next_stop + 1).min(last);
                    log::debug!("duty: left stop, next stop now {}", self.next_stop);
                }
            }
        }
        served
    }
}
