use super::{BusAtStops, Movement, Pax, Posture, Task};
use crate::humans::{BusId, BusNow, Humans, LEFT_HAND, Place, STOP_RANGE, State, debug_pax};
use crate::scene::World;
use glam::{DVec2, DVec3};
use hashbrown::{HashMap, HashSet};
use ::render::{Renderer, Scene};
use ::traffic::Network;

/// A waiting place of a stop (a `[passpos]` of an object near it, sub_620c0c).
#[derive(Debug, Clone)]
pub(in crate::humans) struct WaitSpot {
    /// The `[passpos]` point (world): the feet, or a seated person's hip.
    pub pos: DVec3,
    /// Heading (degrees, the world's).
    pub face: f64,
    /// Seat height (+0x20); a seat when not 0.
    pub height: f32,
}

impl WaitSpot {
    /// Put the feet ahead of the seated hip, using the selected human's geometry.
    pub fn foot_root(&self, front: f32, floor: f64) -> DVec3 {
        let yaw = self.face.to_radians();
        DVec3::new(
            self.pos.x + yaw.sin() * front as f64,
            self.pos.y + yaw.cos() * front as f64,
            floor,
        )
    }
}

/// What Omsi.exe keeps of a bus stop for the people (the station record, sub_620058).
pub(in crate::humans) struct PaxStop {
    pub name: String,
    /// Its name in the timetable (empty without one), where the passengers' destinations
    /// come from: the object's label is the stop's name to Omsi.exe, but a map whose
    /// labels and `Busstops.cfg` disagree - a stop renamed, or the two files written in
    /// different code pages - had riders whose stop never came, and who rode on for good.
    pub alias: String,
    pub pos: DVec3,
    /// The object's heading (degrees).
    pub heading: f64,
    /// Where people gather when a bus comes (+0x48): a metre to the side and a metre
    /// along the stop.
    pub gather: DVec3,
    pub spots: Vec<WaitSpot>,
    /// Which places are taken (+0xa8).
    pub taken: Vec<bool>,
    /// pass_enter_max / _min (+0x70, +0x74) and the length (+0x7c, 30 by default).
    pub enter_max: f32,
    pub enter_min: f32,
    pub length: f32,
    /// The pavement next to it (where those getting off walk on).
    pub lane: Option<(usize, f32)>,
    /// In range of the player last time and now (+0x24, +0x25), the refill clock (+0x28,
    /// ms), people wanted and there (+0x30, +0x34), the stop's factor (+0x38), first fill
    /// done (+0xa5 clear).
    pub was_near: bool,
    pub near: bool,
    pub clock_ms: f32,
    pub want: usize,
    pub factor: f32,
    /// The buses listed here this frame (+0x80): (bus, standing in the stop's box).
    pub buses: Vec<(BusId, bool)>,
    /// The destinations (+0xb0): stop name, weight; and the line records (+0xac): the
    /// stop name and the termini of the buses that go there.
    pub dests: Vec<(String, f32)>,
    pub lines: Vec<(String, HashSet<String>)>,
}

impl PaxStop {
    /// Whether a destination or a terminus `name` is this stop: its label, or its name in
    /// the timetable.
    pub(in crate::humans) fn is_named(&self, name: &str) -> bool {
        let name = name.trim();
        name == self.name.trim() || (!self.alias.is_empty() && name == self.alias.trim())
    }
}

impl Humans {
    /// A stop as Omsi.exe sets it up (sub_620058, sub_620c0c, sub_61c604): its waiting
    /// places are the `[passpos]` of every object near it - within 10 m to the platform's
    /// side and from 10 m behind to the stop's length ahead of it -, the gather point a
    /// metre to the kerb and a metre ahead, the destinations of the trips leaving it.
    pub(in crate::humans) fn build_pax_stop(
        &mut self,
        world: &World,
        net: Option<&Network>,
        id: i64,
        pos: DVec3,
        heading: f64,
        name: &str,
    ) -> PaxStop {
        let length = world.stop_length(id);
        let side = world.stop_side(id).round().clamp(0.0, 255.0) as u8;
        let left = LEFT_HAND.load(std::sync::atomic::Ordering::Relaxed);
        let (xmax, xmin) = (
            if (side == 1) != left { 0.0 } else { 10.0 },
            if (side == 0) != left { 0.0 } else { -10.0 },
        );
        let h = heading.to_radians();
        let objects = world.object_positions.lock();
        let mut spots: Vec<WaitSpot> = Vec::new();
        for (obj, p, face, height) in world.waiting_places.lock().iter() {
            let Some((opos, _)) = objects.get(obj) else {
                if debug_pax() && (*p - pos).length() < 30.0 {
                    log::info!(
                        "stop {id}: waiting place of object {obj} at {p:?}: object position unknown"
                    );
                }
                continue;
            };
            // (sub_7f0db8 / sub_7f0d3c: the stop less the object)
            let v = pos - *opos;
            // (Omsi.exe looks at every object of the stop's tile and the ones round it; the
            // region below reaches the stop's length ahead, which a long bus station stop
            // takes past 40 m)
            if v.length() > 40.0_f64.max(length as f64 + 15.0) {
                continue;
            }
            // (0x7efb08 with -heading: in the stop's frame)
            let lat = h.cos() * v.x - h.sin() * v.y;
            let along = h.cos() * v.y + h.sin() * v.x;
            if !(-along < 10.0 && -along > -(length as f64).max(10.0) && -lat < xmax && -lat > xmin)
            {
                if debug_pax() && v.length() < 30.0 {
                    log::info!(
                        "stop {id}: object {obj} (waiting place {p:?}) not the stop's: across {:.1}, along {:.1}",
                        -lat,
                        -along
                    );
                }
                continue;
            }
            spots.push(WaitSpot {
                pos: *p,
                face: *face,
                height: *height,
            });
        }
        drop(objects);
        // Map-authored standing markers can intersect a neighbouring shelter wall.
        // Seated hip markers intentionally lie inside the shelter's geometry.
        spots.retain(|sp| {
            if sp.height != 0.0 {
                return true;
            }
            let floor = world
                .walk_height_near(sp.pos.x, sp.pos.y, sp.pos.z)
                .unwrap_or(sp.pos.z);
            !crate::camera_arm::standing_space_blocked(world, DVec3::new(sp.pos.x, sp.pos.y, floor))
        });
        // the gather point (+0x48): (1, 0, 1) or (-1, 0, 1) through the stop's turn
        let x = if (side == 1) == left { 1.0 } else { -1.0 };
        let (fwd, right) = (DVec2::new(h.sin(), h.cos()), DVec2::new(h.cos(), -h.sin()));
        let g = pos.truncate() + right * x + fwd * 1.0;
        let gather = DVec3::new(g.x, g.y, pos.z);
        let lane = net
            .and_then(|n| {
                self.walking
                    .ped
                    .as_ref()
                    .and_then(|pn| pn.nearest(n, pos, 12.0))
            })
            .map(|(l, s, _)| (l, s));
        let (enter_max, enter_min) = world.stop_enter(id);
        // the destinations: the stops the trips from here go on to, as likely as people
        // get off there; each with the termini of the buses that go there
        let lines: Vec<(String, HashSet<String>)> = self
            .stop_targets
            .as_ref()
            .and_then(|m| m.get(&id))
            .cloned()
            .unwrap_or_default();
        let weights: Vec<f32> = lines
            .iter()
            .map(|(n, _)| {
                world
                    .bus_stops
                    .lock()
                    .iter()
                    .find(|s| s.3.trim() == n.trim())
                    .map(|s| world.stop_exit_weight(s.0))
                    .unwrap_or(0.5)
            })
            .collect();
        let total: f32 = weights.iter().sum();
        let dests: Vec<(String, f32)> = if total > 0.0 {
            lines
                .iter()
                .zip(&weights)
                .map(|((n, _), w)| (n.clone(), w / total))
                .collect()
        } else {
            Vec::new()
        };
        if debug_pax() {
            log::info!(
                "stop {id} '{name}' at ({:.1}, {:.1}, {:.2}) heading {heading:.0}: {} waiting places, length {length}, side {side}, {} destinations",
                pos.x,
                pos.y,
                pos.z,
                spots.len(),
                dests.len()
            );
        }
        let n = spots.len();
        // what the timetable calls it - its id when the timetable does not know it, as the
        // targets then do
        let alias = match &self.stop_names {
            Some(n) => n.get(&id).cloned().unwrap_or_else(|| id.to_string()),
            None => String::new(),
        };
        PaxStop {
            name: name.to_string(),
            alias,
            pos,
            heading,
            gather,
            spots,
            taken: vec![false; n],
            enter_max,
            enter_min,
            length,
            lane,
            was_near: false,
            near: false,
            clock_ms: 0.0,
            want: 0,
            factor: 1.0,
            buses: Vec::new(),
            dests,
            lines,
        }
    }

    /// The stops near the player fill with people (sub_61bf94, every frame): a stop coming
    /// into range gets its people at once, one in range another one every 10..15 s while
    /// it has fewer than it should - its pass_enter mean times its own random factor
    /// times the passenger density, at most one per waiting place. A stop going out of
    /// range loses the people waiting there.
    pub(in crate::humans) fn stops_tick(
        &mut self,
        dt: f32,
        world: &World,
        renderer: &Renderer,
        scene: &mut Scene,
    ) {
        if self.network.mirror || self.avatar_only {
            return;
        }
        let mut ids: Vec<i64> = self.stops.keys().copied().collect();
        ids.sort_unstable();
        let forced = ::legacy_config::env::var("OMSI_PAX_WAITING")
            .ok()
            .and_then(|v| v.parse::<usize>().ok());
        // the people handed over to another player's bus stop counting once it has left
        // their stop (or the session)
        if !self.network.handed.is_empty() {
            let (stops, remote) = (&self.stops, &self.network.remote_now);
            self.network.handed.retain(|(stop, bus)| {
                let Some(s) = stops.get(stop) else {
                    return false;
                };
                remote
                    .iter()
                    .any(|b| b.id == BusId::Ai(*bus) && (b.pos - s.pos).length() < 60.0)
            });
        }
        for id in ids {
            let center = self.center;
            let near = {
                let s = &self.stops[&id];
                (s.pos - center).length() < STOP_RANGE
                    || self
                        .lan_centers
                        .iter()
                        .any(|c| (s.pos - *c).length() < STOP_RANGE)
            };
            let changed = {
                let s = self.stops.get_mut(&id).unwrap();
                s.was_near = s.near;
                s.near = near;
                s.was_near != s.near
            };
            if !near {
                if changed {
                    // (sub_61be80) the people waiting there go
                    for i in (0..self.people.len()).rev() {
                        let here = matches!(&self.people[i].state, State::Pax(p) if p.stop == Some(id) && p.inside.is_none() && matches!(p.task, Task::WaitingForBus | Task::WalkingToBusstop));
                        if here {
                            self.release(i);
                            let p = self.people.swap_remove(i);
                            self.retire(&p);
                        }
                    }
                    for t in self.stops.get_mut(&id).unwrap().taken.iter_mut() {
                        *t = false;
                    }
                    for person in &self.people {
                        if let State::Pax(p) = &person.state {
                            if p.stop == Some(id) && p.task == Task::AwaitingTransfer {
                                if let Some(k) = p
                                    .spot
                                    .and_then(|k| self.stops.get_mut(&id).unwrap().taken.get_mut(k))
                                {
                                    *k = true;
                                }
                            }
                        }
                    }
                }
                continue;
            }
            if changed {
                let r = self.rand_f() as f32;
                let s = self.stops.get_mut(&id).unwrap();
                let mean = (s.enter_max + s.enter_min) / 2.0;
                let k = if s.enter_max == 0.0 {
                    0.0
                } else if mean == 0.0 {
                    (s.enter_max - s.enter_min) / (s.enter_max * 2.0)
                } else {
                    (s.enter_max - s.enter_min) / (mean * 2.0)
                };
                s.factor = (r * 2.0 - 1.0) * k + 1.0;
            }
            let count = self
                .people
                .iter()
                .filter(|p| matches!(&p.state, State::Pax(x) if x.stop == Some(id)))
                .count()
                + self.network.handed.iter().filter(|h| h.0 == id).count();
            let want = {
                let s = &self.stops[&id];
                let mean = (s.enter_max + s.enter_min) / 2.0;
                let w = (self.density.max(0.0) * mean * s.factor).round().max(0.0) as usize;
                forced.unwrap_or(w).min(s.spots.len())
            };
            let s = self.stops.get_mut(&id).unwrap();
            s.want = want;
            s.clock_ms += dt * 1000.0;
            if !changed {
                let r = self.rand_f() as f32;
                if self.stops[&id].clock_ms <= r * 5000.0 + 10000.0 {
                    continue;
                }
            }
            let mut count = count;
            while count < want {
                if self.spawn_waiting(world, renderer, scene, id).is_none() {
                    break;
                }
                count += 1;
                if !changed {
                    break;
                }
            }
            if count >= want {
                self.stops.get_mut(&id).unwrap().clock_ms = 0.0;
            }
        }
    }

    /// A destination drawn from stop `id`'s (sub_61baa8): by weight; none when the weights
    /// leave the draw over. Also the stop's line record it matched.
    pub(in crate::humans) fn draw_dest(&mut self, id: i64) -> (Option<String>, Option<usize>) {
        let mut r = self.rand_f() as f32;
        let mut dest: Option<String> = None;
        let Some(stop) = self.stops.get(&id) else {
            return (None, None);
        };
        for (n, w) in &stop.dests {
            if r <= 0.0 {
                break;
            }
            r -= w;
            if r <= 0.0 {
                dest = Some(n.clone());
            }
        }
        let line = dest
            .as_ref()
            .and_then(|d| stop.lines.iter().position(|(n, _)| n.trim() == d.trim()));
        (dest, line)
    }

    /// A person put at a free waiting place of stop `id` (sub_626044) with a destination
    /// drawn from the stop's (sub_61baa8); they settle there as task 6 does.
    pub(in crate::humans) fn spawn_waiting(
        &mut self,
        world: &World,
        renderer: &Renderer,
        scene: &mut Scene,
        id: i64,
    ) -> Option<usize> {
        let k = self.take_spot(id, None)?;
        let sp = self.stops[&id].spots[k].clone();
        let (dest, line) = self.draw_dest(id);
        let walk = 1.1 + (self.rand_f() as f32 * 2.0 - 1.0) * 0.2;
        let mut pax = Pax::new(walk);
        pax.stop = Some(id);
        pax.spot = Some(k);
        pax.pos = sp.pos;
        pax.yaw = sp.face.to_radians();
        pax.journey.dest = dest;
        pax.journey.line = line;
        pax.movement = Movement::Standing;
        if self.ik {
            let near = sp.pos.z - sp.height as f64;
            pax.pos.z = world
                .walk_height_near(sp.pos.x, sp.pos.y, near)
                .unwrap_or(near);
            pax.task = Task::WaitingForBus;
            if sp.height != 0.0 {
                pax.seat_h = sp.height;
                pax.posture = Posture::Sitting;
            }
        }
        let position = pax.pos;
        let Some(i) = self.spawn(
            world,
            renderer,
            scene,
            position,
            sp.face,
            State::Pax(Box::new(pax)),
        ) else {
            self.free_spot(id, k);
            return None;
        };
        let dummy_b: Vec<BusNow> = Vec::new();
        let dummy_ix: HashMap<BusId, usize> = HashMap::new();
        if self.ik {
            let distance = self.rand_f() as f32 * 19.0 + 1.0;
            self.pax_mut(i).unwrap().journey.ride_km = distance;
        } else {
            self.set_task(i, Task::WalkingToBusstop, &dummy_b, &dummy_ix, world);
        }
        if debug_pax() {
            let d = self.pax(i).and_then(|p| p.journey.dest.clone());
            log::info!(
                "t={:.1} pax {} waits at stop {id} place {k}, for {:?}",
                self.time,
                self.people[i].label(),
                d
            );
        }
        Some(i)
    }
}

impl Humans {
    /// The stops as the buses see them this frame (sub_61f93c / sub_61f238), and the
    /// odometers of the buses.
    pub(in crate::humans) fn register_buses(
        &mut self,
        buses: &[BusNow],
        dt: f32,
    ) -> HashMap<BusId, BusAtStops> {
        let mut out: HashMap<BusId, BusAtStops> = HashMap::new();
        for s in self.stops.values_mut() {
            s.buses.clear();
        }
        let left = LEFT_HAND.load(std::sync::atomic::Ordering::Relaxed);
        let _ = left;
        let mut ids: Vec<i64> = self.stops.keys().copied().collect();
        ids.sort_unstable();
        for bn in buses {
            let km = self.buses.odometer.entry(bn.id).or_insert(0.0);
            *km += bn.speed.abs() * dt as f64 / 1000.0;
            let mut reg = BusAtStops::default();
            let mut nearest = f64::INFINITY;
            let mut nearest_at = f64::INFINITY;
            reg.all_exit = bn.out_of_service;
            for id in &ids {
                let s = &self.stops[id];
                let d = bn.pos - s.pos;
                let dist = d.length();
                if !(dist < 60.0) {
                    continue;
                }
                let sh = s.heading.to_radians();
                let (s_fwd, s_right) = (
                    DVec2::new(sh.sin(), sh.cos()),
                    DVec2::new(sh.cos(), -sh.sin()),
                );
                let same_way = bn.fwd().dot(s_fwd) > 0.0;
                reg.near.push(*id);
                if same_way && dist < nearest {
                    reg.next = Some(*id);
                    nearest = dist;
                }
                // a bus not in service, or at its own terminus, empties and takes nobody
                // (0x61f3e3)
                if same_way {
                    let lateral = d.truncate().dot(s_right);
                    let along = d.truncate().dot(s_fwd);
                    let in_box =
                        lateral.abs() < 2.0 && along.abs() < (s.length as f64 - 5.0).max(0.0);
                    if in_box && dist < nearest_at {
                        nearest_at = dist;
                        reg.at = Some(*id);
                    }
                    if !bn.out_of_service && !bn.terminus.as_ref().is_some_and(|t| s.is_named(t)) {
                        self.stops.get_mut(id).unwrap().buses.push((bn.id, in_box));
                    }
                }
            }
            if reg.at.is_some_and(|s| {
                bn.terminus
                    .as_ref()
                    .is_some_and(|t| self.stops[&s].is_named(t))
            }) {
                reg.all_exit = true;
            }
            out.insert(bn.id, reg);
        }
        out
    }

    /// sub_61c33c: the bus at stop `stop` person `i` gets into: with a line record, the
    /// nearest of the buses listed whose terminus goes there; without, the first listed.
    pub(in crate::humans) fn bus_for(
        &self,
        i: usize,
        stop: i64,
        buses: &[BusNow],
        bus_ix: &HashMap<BusId, usize>,
    ) -> Option<BusId> {
        let s = self.stops.get(&stop)?;
        let p = self.pax(i)?;
        match p
            .journey
            .allowed_termini
            .as_ref()
            .or_else(|| p.journey.line.and_then(|k| s.lines.get(k)).map(|l| &l.1))
        {
            None => s.buses.first().map(|b| b.0),
            Some(termini) => {
                let mut best: Option<(f64, BusId)> = None;
                for (id, _) in &s.buses {
                    let Some(bn) = bus_ix.get(id).map(|k| &buses[*k]) else {
                        continue;
                    };
                    if bn.cabin.entries.is_empty() {
                        continue;
                    }
                    let Some(t) = &bn.terminus else { continue };
                    if !termini.contains(t.trim()) {
                        continue;
                    }
                    let d = (bn.pos - self.people[i].position).length();
                    if best.is_none_or(|b| d < b.0) {
                        best = Some((d, *id));
                    }
                }
                best.map(|b| b.1)
            }
        }
    }

    /// Whether the bus stands in the stop's box (the flag of its entry, sub_61ee18).
    pub(in crate::humans) fn in_stop_box(&self, stop: i64, bus: BusId) -> bool {
        self.stops
            .get(&stop)
            .is_some_and(|s| s.buses.iter().any(|b| b.0 == bus && b.1))
    }

    pub(in crate::humans) fn listed_at(&self, stop: i64, bus: BusId) -> bool {
        self.stops
            .get(&stop)
            .is_some_and(|s| s.buses.iter().any(|b| b.0 == bus))
    }

    pub(in crate::humans) fn free_spot(&mut self, stop: i64, k: usize) {
        if let Some(t) = self.stops.get_mut(&stop).and_then(|s| s.taken.get_mut(k)) {
            *t = false;
        }
    }

    /// sub_61c8d8: a free waiting place of the stop, at random.
    pub(in crate::humans) fn take_spot(&mut self, stop: i64, except: Option<u32>) -> Option<usize> {
        let free: Vec<usize> = self
            .stops
            .get(&stop)?
            .taken
            .iter()
            .enumerate()
            .filter(|(_, t)| !**t)
            .filter(|(k, _)| {
                let sp = &self.stops[&stop].spots[*k];
                let floor = sp.pos.z - sp.height as f64;
                !self.people.iter().any(|person| {
                    if Some(person.id) == except || person.place != Place::Ground {
                        return false;
                    }
                    let claim = match &person.state {
                        State::Pax(p) => p.stop.zip(p.spot),
                        _ => self.network.mirror_wait.get(&person.id).copied(),
                    };
                    let at = claim
                        .and_then(|(s, k)| self.stops.get(&s).and_then(|s| s.spots.get(k)))
                        .map(|sp| sp.pos - DVec3::Z * sp.height as f64)
                        .unwrap_or(person.position);
                    // Stops can share authored markers. A reservation in another
                    // stop, or somebody standing here, still occupies physical space.
                    (at.z - floor).abs() < 0.75
                        && (at - sp.pos).truncate().length_squared() < 0.5 * 0.5
                })
            })
            .map(|(k, _)| k)
            .collect();
        if free.is_empty() {
            return None;
        }
        let k = free[(self.rand() as usize) % free.len()];
        self.stops.get_mut(&stop).unwrap().taken[k] = true;
        Some(k)
    }
}
