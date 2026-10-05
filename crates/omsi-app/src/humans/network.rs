use super::*;

/// Where one of the host's people is this frame, as a client draws them.
#[derive(Debug, Clone, Copy)]
pub struct MirrorPose {
    pub pos: DVec3,
    pub heading: f64,
    pub vel: DVec2,
    pub activity: Activity,
    /// Aboard a timetable bus: (its id, the point of its frame, heading in its frame, the
    /// seat or standing place, if known).
    pub aboard: Option<(u64, Vec3, f64, Option<usize>)>,
    /// Waiting at a stop: (the stop object, the waiting place).
    pub waiting: Option<(i64, usize)>,
}

/// The bus id (`BusId::Ai`) another LAN player's bus has among the buses here: far above
/// the traffic's car ids.
pub fn remote_bus_id(player: u32) -> u64 {
    (1 << 40) | player as u64
}

/// The bus id (`BusId::Ai`) of a vehicle the player placed (`Player::uid`).
pub fn placed_bus_id(uid: u64) -> u64 {
    (2 << 40) | uid
}

/// The player whose bus `remote_bus_id` gave this id (None for a traffic bus).
pub fn remote_bus_player(bus: u64) -> Option<u32> {
    (bus >> 40 == 1).then_some((bus & 0xFFFF_FFFF) as u32)
}

/// One of the host's people as it tells the clients.
pub struct LanPerson {
    pub id: u32,
    pub ty: Arc<HumanType>,
    pub pos: DVec3,
    pub heading: f64,
    pub speed: f64,
    pub activity: Activity,
    pub aboard: Option<(u64, Vec3, f64, Option<usize>)>,
    pub waiting: Option<(i64, usize)>,
}

impl Humans {
    /// Local identities may collide with a host that previously was a client.
    /// Rename only when needed, while updating every local id-based owner together.
    fn remap_person_id(&mut self, i: usize) {
        let old = self.people[i].id;
        let new = self.next_person_id();
        self.people[i].id = new;
        if self.desk.desk_busy == Some(old) {
            self.desk.desk_busy = Some(new);
        }
        for id in self.avatars.avatars.values_mut().filter(|id| **id == old) {
            *id = new;
        }
        if let Some(hidden) = self.avatars.avatar_hidden.remove(&old) {
            self.avatars.avatar_hidden.insert(new, hidden);
        }
    }
    /// Is `p` further than `r` from us and from every other LAN player?
    pub(super) fn far_from_players(&self, p: DVec3, r: f64) -> bool {
        (p - self.center).length() > r && self.lan_centers.iter().all(|c| (p - *c).length() > r)
    }

    /// The stops and pavements around the other players of a LAN session (host).
    pub(super) fn populate_lan_centers(
        &mut self,
        world: &World,
        net: &Network,
        renderer: &Renderer,
        scene: &mut Scene,
    ) {
        if self.lan_centers.is_empty() {
            return;
        }
        let mine = self.center;
        for c in self.lan_centers.clone() {
            if (c - mine).length() < 150.0 {
                continue;
            }
            self.populate_with(world, Some(net), renderer, scene, c);
            self.populate_on_foot(world, net, renderer, scene, 1.0);
        }
        self.center = mine;
    }

    /// Everybody within `radius` of `near` the clients may see (host): on foot, waiting at
    /// a stop, or aboard a timetable bus - not the riders of our own bus, which the others
    /// see from outside only.
    pub fn lan_people(&self, near: DVec3, radius: f64) -> Vec<LanPerson> {
        let r2 = radius * radius;
        self.people
            .iter()
            .filter(|p| p.puppet.is_none() && !p.remote)
            .filter(|p| (p.position - near).length_squared() < r2)
            .filter_map(|p| {
                let aboard = match p.place {
                    Place::Bus(BusId::Player, _) => return None,
                    Place::Bus(BusId::Ai(bus), l) => Some((
                        bus,
                        l,
                        p.lheading,
                        match &p.state {
                            State::Pax(x) if x.task == Task::SittingInBus => x.seat,
                            _ => None,
                        },
                    )),
                    Place::Ground => None,
                };
                let waiting = match &p.state {
                    State::Pax(x) if aboard.is_none() && x.task == Task::WaitingForBus => {
                        x.stop.zip(x.spot)
                    }
                    _ => None,
                };
                Some(LanPerson {
                    id: p.id,
                    ty: p.ty.clone(),
                    pos: p.position,
                    heading: p.heading,
                    speed: if aboard.is_some() {
                        0.0
                    } else {
                        p.vel.length()
                    },
                    activity: p.activity,
                    aboard,
                    waiting,
                })
            })
            .collect()
    }

    /// Freeze waiting people for a client's offer and return their journeys. Ownership
    /// stays here until acceptance; somebody already approaching another bus stays ours.
    pub fn hand_over(
        &mut self,
        ids: &[u32],
        bus: BusId,
    ) -> Vec<omsi_net::passenger::PassengerGrant> {
        let mut out = Vec::new();
        for id in ids {
            let Some(i) = self.people.iter().position(|p| p.id == *id) else {
                continue;
            };
            if !matches!(&self.people[i].state, State::Pax(x) if x.task == Task::WaitingForBus)
                || self.people[i].remote
            {
                continue;
            }
            let Some(p) = self.pax(i) else { continue };
            let Some(stop) = p.stop.filter(|s| self.in_stop_box(*s, bus)) else {
                continue;
            };
            let Some(spot) = p.spot else { continue };
            let grant = {
                let Some(stop_record) = self.stops.get(&stop) else {
                    continue;
                };
                omsi_net::passenger::PassengerGrant {
                    id: *id,
                    transfer: 0,
                    stop,
                    spot: spot as u32,
                    destination: p.journey.dest.clone(),
                    alternative: p.journey.alt.clone(),
                    alternative_m: p.journey.alt_m,
                    ride_km: p.journey.ride_km,
                    line_destination: p
                        .journey
                        .line
                        .and_then(|k| stop_record.lines.get(k))
                        .map(|l| l.0.clone()),
                    allowed_termini: p
                        .journey
                        .allowed_termini
                        .as_ref()
                        .or_else(|| {
                            p.journey
                                .line
                                .and_then(|k| stop_record.lines.get(k))
                                .map(|l| &l.1)
                        })
                        .map(|names| {
                            let mut names: Vec<String> = names.iter().cloned().collect();
                            names.sort();
                            names
                        }),
                }
            };
            if grant.encode().is_none() {
                log::warn!("passenger {id}: invalid or oversized journey; staying at stop {stop}");
                continue;
            }
            let p = self.pax_mut(i).unwrap();
            p.task = Task::AwaitingTransfer;
            p.movement = Movement::Standing;
            p.bus = None;
            p.handover_bus = Some(bus);
            out.push(grant);
        }
        out
    }

    /// Commit a confirmed transfer, or resume waiting if its client disconnected.
    pub fn finish_handover(&mut self, id: u32, accepted: bool) {
        let Some(i) = self.people.iter().position(|p| p.id == id && !p.remote) else {
            return;
        };
        if !self
            .pax(i)
            .is_some_and(|p| p.task == Task::AwaitingTransfer)
        {
            return;
        }
        if accepted {
            if let Some((stop, BusId::Ai(bus))) =
                self.pax(i).and_then(|p| p.stop.zip(p.handover_bus))
            {
                self.network.handed.push((stop, bus));
            }
            self.release(i);
            let person = self.people.swap_remove(i);
            self.retire(&person);
        } else {
            let p = self.pax_mut(i).unwrap();
            p.task = Task::WaitingForBus;
            p.handover_bus = None;
        }
    }

    /// Draw the host's people from now on (`on`), or simulate our own again. Everybody who
    /// is not getting on, riding or getting off our bus goes (the host's come instead; the
    /// host's copies cannot walk on by themselves).
    pub fn set_mirror(&mut self, on: bool) {
        if self.network.mirror == on {
            return;
        }
        self.network.mirror = on;
        let keep = |p: &Person| !p.remote && p.state.bus() == Some(BusId::Player);
        let mut i = 0;
        while i < self.people.len() {
            if keep(&self.people[i]) || self.people[i].puppet.is_some() {
                i += 1;
                continue;
            }
            self.release(i);
            let p = self.people.swap_remove(i);
            self.retire(&p);
        }
        // Reserve the usual local range. A host may itself have used this range in an
        // earlier client session, so incoming collisions still use the atomic remap.
        if on {
            self.next_id = self.next_id.max(1 << 30);
            for i in 0..self.people.len() {
                if self.people[i].remote || self.people[i].id >= 1 << 30 {
                    continue;
                }
                self.remap_person_id(i);
            }
        }
        for s in self.stops.values_mut() {
            for t in s.taken.iter_mut() {
                *t = false;
            }
        }
        self.network.claims_out.clear();
        self.network.claimed.clear();
        self.network.mirror_wait.clear();
        for person in &mut self.people {
            if let State::Pax(pax) = &mut person.state {
                pax.accepted_transfer = None;
            }
        }
    }

    /// The human type of a file relative to a content root (`Humans/…/x.hum`).
    pub fn type_by_file(&self, file: &str) -> Option<usize> {
        let want = file.replace('\\', "/").to_ascii_lowercase();
        self.types.iter().position(|t| {
            t.def
                .path
                .to_string_lossy()
                .replace('\\', "/")
                .to_ascii_lowercase()
                .ends_with(&want)
        })
    }

    /// The file of a human type relative to its content root (`Humans/…/x.hum`).
    pub fn type_file(ty: &HumanType) -> String {
        let p = ty.def.path.to_string_lossy().replace('\\', "/");
        match p.to_ascii_lowercase().rfind("/humans/") {
            Some(k) => p[k + 1..].to_string(),
            None => p,
        }
    }

    /// One of the host's people appears here (client).
    pub fn mirror_add(
        &mut self,
        world: &World,
        renderer: &Renderer,
        scene: &mut Scene,
        id: u32,
        ty: usize,
        pose: &MirrorPose,
    ) -> bool {
        if let Some(i) = self.people.iter().position(|p| p.id == id) {
            if self.people[i].remote {
                return false;
            }
            self.remap_person_id(i);
        }
        let state = State::Idle;
        let Some(i) = self.spawn_as(
            world,
            renderer,
            scene,
            pose.pos,
            pose.heading,
            state,
            Some(ty),
        ) else {
            return false;
        };
        self.next_id -= 1;
        let p = &mut self.people[i];
        p.id = id;
        p.anim = OmsiAnim::default();
        p.pose = omsi_sim::human::Pose::new(id);
        p.remote = true;
        self.mirror_set(id, pose);
        true
    }

    /// Where one of the host's people is this frame (client).
    pub fn mirror_set(&mut self, id: u32, pose: &MirrorPose) {
        let Some(p) = self.people.iter_mut().find(|p| p.id == id && p.remote) else {
            return;
        };
        p.position = pose.pos;
        p.heading = pose.heading;
        p.vel = pose.vel;
        p.activity = pose.activity;
        p.render.mirror_seat = pose.aboard.and_then(|(_, _, _, seat)| seat);
        match pose.waiting {
            Some(w) => {
                self.network.mirror_wait.insert(id, w);
            }
            None => {
                self.network.mirror_wait.remove(&id);
            }
        }
        match pose.aboard {
            Some((bus, local, lheading, _)) => {
                p.place = Place::Bus(BusId::Ai(bus), local);
                p.lheading = lheading;
                p.vel = DVec2::ZERO;
            }
            None => p.place = Place::Ground,
        }
    }

    /// One of the host's people has gone (client).
    pub fn mirror_remove(&mut self, id: u32) {
        if let Some(i) = self.people.iter().position(|p| p.id == id && p.remote) {
            let p = self.people.swap_remove(i);
            self.retire(&p);
        }
        self.network.claimed.remove(&id);
        self.network.mirror_wait.remove(&id);
    }

    /// A remote person this frame: they stand where the host put them.
    pub(super) fn mirror_want(&mut self, i: usize, _buses: &[BusNow]) -> Want {
        Want::stand(None, self.people[i].activity)
    }

    /// The type of one of our people (host).
    pub fn lan_people_by_id(&self, id: u32) -> Option<Arc<HumanType>> {
        self.people
            .iter()
            .find(|p| p.id == id && !p.remote)
            .map(|p| p.ty.clone())
    }

    /// Where the host's people are drawn (client; `OMSI_LAN_TRACE`).
    pub fn mirror_positions(&self) -> Vec<(u32, DVec3)> {
        self.people
            .iter()
            .filter(|p| p.remote && p.place == Place::Ground)
            .map(|p| (p.id, p.position))
            .collect()
    }

    /// Waiting people to ask the host for (client).
    pub fn take_claims(&mut self) -> Vec<u32> {
        std::mem::take(&mut self.network.claims_out)
    }

    /// Accept the host's journey once the corresponding person and stop are loaded.
    /// Missing streamed metadata leaves the mirrored passenger intact for the retry.
    pub fn grant(&mut self, grant: &omsi_net::passenger::PassengerGrant) -> bool {
        let id = grant.id;
        if self.people.iter().any(|p| {
            p.id == id
                && !p.remote
                && matches!(&p.state, State::Pax(x) if x.accepted_transfer == Some(grant.transfer))
        }) {
            return true;
        }
        let (stop, spot) = (grant.stop, grant.spot as usize);
        let Some(i) = self.people.iter().position(|p| p.id == id && p.remote) else {
            return false;
        };
        if !self.stops.contains_key(&stop) {
            return false;
        }
        let line = grant
            .line_destination
            .as_ref()
            .and_then(|name| self.stops[&stop].lines.iter().position(|l| l.0 == *name));
        // ours from now on: waiting at that place, for the bus that stands there (the first
        // listed, as Omsi.exe takes it without a line record)
        let Some(sp) = self.stops[&stop].spots.get(spot).cloned() else {
            return false;
        };
        if self.stops[&stop].taken.get(spot).is_none() {
            return false;
        }
        let seatheight = self.people[i].ty.def.seat_height;
        let walk = 1.1 + (self.rand_f() as f32 * 2.0 - 1.0) * 0.2;
        let mut pax = Pax::new(walk);
        pax.accepted_transfer = Some(grant.transfer);
        pax.journey.dest = grant.destination.clone();
        pax.journey.line = line;
        pax.journey.allowed_termini = grant
            .allowed_termini
            .as_ref()
            .map(|names| names.iter().cloned().collect());
        pax.journey.alt = grant.alternative.clone();
        pax.journey.alt_m = grant.alternative_m;
        // The decoded grant already validated the distance. An absent destination is
        // also host-owned journey data, not permission to consult a different timetable.
        pax.journey.ride_km = grant.ride_km;
        pax.task = Task::WaitingForBus;
        pax.stop = Some(stop);
        pax.pos = self.people[i].position;
        pax.yaw = self.people[i].heading.to_radians();
        {
            pax.spot = Some(spot);
            if let Some(t) = self.stops.get_mut(&stop).unwrap().taken.get_mut(spot) {
                *t = true;
            }
            if sp.height != 0.0 {
                pax.seat_h = sp.height;
                pax.pos = if self.ik {
                    sp.foot_root(
                        self.people[i].ty.rig.seat_front(),
                        self.people[i].position.z,
                    )
                } else {
                    sp.pos - DVec3::Z * seatheight as f64
                };
                pax.posture = Posture::Sitting;
            }
            pax.yaw = sp.face.to_radians();
        }
        let p = &mut self.people[i];
        p.remote = false;
        p.state = State::Pax(Box::new(pax));
        self.network.claimed.remove(&id);
        self.network.mirror_wait.remove(&id);
        true
    }

    /// A grant is for our bus, not for a rider waiting for another line at the same stop.
    pub fn grant_eligible(&self, grant: &omsi_net::passenger::PassengerGrant) -> Option<bool> {
        let stop = self.stops.get(&grant.stop)?;
        let bus = self
            .buses
            .last_buses
            .iter()
            .find(|b| b.id == BusId::Player)?;
        Some(
            !bus.out_of_service
                && stop.buses.iter().any(|b| b.0 == BusId::Player && b.1)
                && grant.allowed_termini.as_ref().is_none_or(|names| {
                    bus.terminus
                        .as_ref()
                        .is_some_and(|t| names.iter().any(|n| n.trim() == t.trim()))
                }),
        )
    }

    /// The host's people waiting at the stop our bus is listed at (client): ask for them.
    pub(super) fn claim_waiting(&mut self) {
        if !self.network.mirror {
            return;
        }
        let now = self.time;
        let Some(bus) = self.buses.last_buses.iter().find(|b| b.id == BusId::Player) else {
            return;
        };
        let target = bus.terminus.clone();
        self.network
            .claimed
            .retain(|_, (t, name)| now - *t < 10.0 && *name == target);
        if bus.out_of_service || !bus.entry_open.iter().any(|o| *o) {
            return;
        }
        let at: Vec<i64> = self
            .stops
            .iter()
            .filter(|(_, s)| s.buses.iter().any(|b| b.0 == BusId::Player && b.1))
            .map(|(id, _)| *id)
            .collect();
        if at.is_empty() {
            return;
        }
        for (id, (stop, _)) in &self.network.mirror_wait {
            if at.contains(stop) && !self.network.claimed.contains_key(id) {
                self.network.claimed.insert(*id, (now, target.clone()));
                self.network.claims_out.push(*id);
            }
        }
    }
}

pub(in crate::humans) struct PassengerNetwork {
    /// LAN play: this game draws the host's people instead of its own (`lan_world`).
    pub(in crate::humans) mirror: bool,
    /// LAN play: the other players' buses this frame (`set_remote_buses`), for their riders
    /// to sit in. Nobody of ours boards them: their doors count as shut.
    pub(in crate::humans) remote_now: Vec<BusNow>,
    /// LAN play (client): waiting people our bus could take, to ask the host for, and when
    /// each was last asked for.
    pub(in crate::humans) claims_out: Vec<u32>,
    pub(in crate::humans) claimed: HashMap<u32, (f64, Option<String>)>,
    /// The host's people waiting at a stop (client): (stop, waiting place).
    pub(in crate::humans) mirror_wait: HashMap<u32, (i64, usize)>,
    /// LAN play (host): the waiting people handed over to another player's bus, by stop and
    /// that bus. They count among the people of the stop while the bus stands there, as
    /// the people who board a bus of ours keep their stop until it has left: without them
    /// the stop filled up again at once - one more person a frame once its 10..15 s were
    /// up - and the client's bus took them all, one stream of passengers that never ended
    /// (#842, #840, #830).
    pub(in crate::humans) handed: Vec<(i64, u64)>,
}

impl Humans {
    /// LAN play: the other players' buses this frame, by player id (their riders are drawn
    /// in them, see `remote_bus_id`).
    pub fn set_remote_buses<'a>(
        &mut self,
        buses: impl Iterator<Item = (u32, &'a VehicleInstance)>,
    ) {
        let mut out = Vec::new();
        for (player, v) in buses {
            let Some(cabin) = self.cabin_for(v) else {
                continue;
            };
            let bb =
                v.ty.def
                    .bounding_box
                    .unwrap_or([2.5, 11.0, 3.0, 0.0, 0.0, 1.5]);
            let trailers = part_frames(v, &cabin);
            // (their doors as their game has them: a walker gets in only where one is open;
            // the passengers here never board it - that bus's own game boards them)
            let walk_open = Self::doors_open(v, cabin.entries.len(), cabin.exits.len());
            out.push(BusNow {
                terminus: None,
                out_of_service: false,
                id: BusId::Ai(remote_bus_id(player)),
                entry_open: vec![false; cabin.entries.len()],
                exit_open: vec![false; cabin.exits.len()],
                walk_open: Some(walk_open),
                cabin,
                pos: v.position,
                rot: v.body_rotation(),
                heading: v.heading,
                speed: v.physics.velocity_kmh() as f64 / 3.6,
                interior: v.interior_light(),
                air: CabinAir::of(v),
                half: DVec2::new(bb[0] as f64 * 0.5, bb[1] as f64 * 0.5),
                centre: DVec2::new(bb[3] as f64, bb[4] as f64),
                accel: DVec2::ZERO,
                trailers,
            });
        }
        self.network.remote_now = out;
    }

    /// Is `bus` among the buses people can be in this frame (an AI bus, or another player's)?
    pub fn knows_bus(&self, bus: u64) -> bool {
        self.network
            .remote_now
            .iter()
            .any(|b| b.id == BusId::Ai(bus))
            || self.buses.seats.contains_key(&BusId::Ai(bus))
    }

    /// Is person `id` (one drawn for another game) here?
    pub fn has_mirror(&self, id: u32) -> bool {
        self.people.iter().any(|p| p.id == id && p.remote)
    }

    /// The riders of our own bus, for the other LAN players to see: (id, type, place in the
    /// bus frame, heading there, seat, activity).
    pub fn lan_riders(&self) -> Vec<LanPerson> {
        self.people
            .iter()
            .filter(|p| p.puppet.is_none() && !p.remote)
            .filter_map(|p| match p.place {
                Place::Bus(BusId::Player, l) => Some(LanPerson {
                    id: p.id,
                    ty: p.ty.clone(),
                    pos: p.position,
                    heading: p.heading,
                    speed: 0.0,
                    activity: p.activity,
                    aboard: Some((
                        0,
                        l,
                        p.lheading,
                        match &p.state {
                            State::Pax(x) if x.task == Task::SittingInBus => x.seat,
                            _ => None,
                        },
                    )),
                    waiting: None,
                }),
                _ => None,
            })
            .collect()
    }
}
