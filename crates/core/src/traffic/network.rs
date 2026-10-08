//! Network editing: adding streamed tiles, reverse twins and connectors.

use super::*;

impl Traffic {

    /// Whether a tile's lanes have been loaded into the network.
    pub fn has_lane_tile(&self, tile: (i32, i32)) -> bool {
        self.lane_tiles.contains(&tile)
    }


    /// The lanes the other way along one-way paths `lanes` (see `Schedule`'s route
    /// building: OMSI's timetable buses drive a path against its direction where the trip's
    /// station links or track say so). Only timetable routes use them: they carry no traffic
    /// and no light. Returns how many were added.
    pub fn add_reverse_twins(&mut self, lanes: &[usize]) -> usize {
        let mut new = Vec::new();
        for &l in lanes {
            if !self.twinned.insert(l) {
                continue;
            }
            let o = &self.net.lanes[l];
            let pts: Vec<DVec3> = o.points.iter().rev().copied().collect();
            let mut t = ::traffic::LaneBuilder::polyline(pts, o.kind, o.width);
            t.key = o.key;
            t.reversed = !o.reversed;
            t.speed_limit_kmh = o.speed_limit_kmh;
            t.source = o.source;
            t.offset = o.offset;
            t.name = o.name.clone();
            t.invisible = o.invisible;
            t.priority = o.priority;
            t.density = 0.0;
            t.no_cars = true;
            new.push(t);
        }
        let n = new.len();
        if n > 0 {
            let added = self.net.extend(new, 1.5);
            log::debug!(
                "traffic: {n} lanes added for timetable routes that drive a one-way path the other way ({:?})",
                added
            );
        }
        n
    }


    /// A lane from the end of `a` to the start of `b`, for a timetable route that jumps a
    /// gap in the map (a road piece deleted after the timetable's tracks were recorded - a
    /// dozen such holes of 10 to 150 m on Novi Sad): a smooth curve with the two lanes'
    /// headings at its ends, used by the timetable only. None when the two do not line up.
    pub fn add_connector(&mut self, a: usize, b: usize) -> Option<usize> {
        let (la, lb) = (&self.net.lanes[a], &self.net.lanes[b]);
        let (p0, p1) = (la.end(), lb.start());
        let d = (p1 - p0).truncate();
        let len = d.length();
        if !(2.0..=150.0).contains(&len) || la.kind != lb.kind {
            return None;
        }
        let dir = |h: f32| {
            let r = (h as f64).to_radians();
            glam::DVec2::new(r.sin(), r.cos())
        };
        let (t0, t1) = (dir(la.end_heading()), dir(lb.start_heading()));
        let chord = d / len;
        // the lanes point along the gap or turn across it (a junction whose turning path
        // the map lost, 66 of them on Novi Sad), never a reversal
        if t0.dot(chord) < 0.3 || t1.dot(chord) < 0.3 || t0.dot(t1) < -0.2 {
            return None;
        }
        // (tangents as long as the gap make a straight gap a smooth S; across a turn they
        // would swing out past the corner)
        let len_t = len * if t0.dot(t1) > 0.9 { 1.0 } else { 0.6 };
        let n = ((len / 3.0).ceil() as usize).max(2);
        let pts: Vec<DVec3> = (0..=n)
            .map(|i| {
                let t = i as f64 / n as f64;
                let (h00, h10, h01, h11) = (
                    2.0 * t * t * t - 3.0 * t * t + 1.0,
                    t * t * t - 2.0 * t * t + t,
                    -2.0 * t * t * t + 3.0 * t * t,
                    t * t * t - t * t,
                );
                let xy =
                    p0.truncate() * h00 + t0 * len_t * h10 + p1.truncate() * h01 + t1 * len_t * h11;
                DVec3::new(xy.x, xy.y, p0.z + (p1.z - p0.z) * t)
            })
            .collect();
        let mut l = ::traffic::LaneBuilder::polyline(pts, la.kind, la.width);
        l.speed_limit_kmh = la.speed_limit_kmh.min(lb.speed_limit_kmh);
        l.name = "(timetable connector)".into();
        l.density = 0.0;
        l.no_cars = true;
        let added = self.net.extend(vec![l], 1.5);
        log::debug!(
            "traffic: a {len:.0} m connector lane {} from lane {a} to lane {b} for a timetable route",
            added.start
        );
        Some(added.start)
    }

    /// Take in what the tiles loaded since the last call brought: their lanes (linked into
    /// the network, whose existing indices stay valid), their parked cars (sorted onto the
    /// lanes once those are in) and the light programs of their crossings.
    pub fn add_tiles(&mut self, world: &World) -> usize {
        let (new, parked_cars, tiles) = take_from_tiles(world);
        let n = new.len();
        let mut added = self.net.lanes.len()..self.net.lanes.len();
        if n > 0 {
            added = self.net.extend(new, 1.5);
            report_network_defects(&self.net);
            // a grown/changed network invalidates every junction claim made against the old one
            self.junctions.invalidate_network();
            self.maneuvers.invalidate_network();
            // queued demand and entrance backpressure are stale; the dormant registry (identity
            // and duty) survives because lane indices are stable
            self.population.invalidate_network();
            self.street_weight += self.net.lanes[added.clone()]
                .iter()
                .filter_map(street_lane_weight)
                .sum::<f64>();
            log::debug!(
                "traffic: {} lanes added ({} in all)",
                added.len(),
                self.net.lanes.len()
            );
        }
        if !tiles.is_empty() {
            self.lane_tiles.extend(tiles);
            self.lanes_generation += 1;
        }
        self.sort_parked(parked_cars, added);
        // new crossings bring their light programs; the running ones keep their clocks
        // (a program is never taken away again: the world's list only grows)
        let lights = world.traffic_lights.lock();
        if lights.len() > self.lights.len() {
            let from = self.lights.len();
            self.lights.extend(lights[from..].iter().cloned());
            self.light_prev
                .extend(lights[from..].iter().map(|c| vec![-100; c.lights.len()]));
            self.controller_of_object = world.controller_of_object.lock().clone();
        }
        n
    }

}
