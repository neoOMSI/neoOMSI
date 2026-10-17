//! Short, exclusive junction reservations for active emergency drives. Existing occupants
//! and committed movements drain normally; no timer removes physical occupancy.
use super::*;

#[derive(Debug, Clone)]
pub(super) struct EmergencyReservation {
    pub owner: VehicleId,
    pub lanes: Vec<usize>,
    /// The owner's own way through the junction and the lane after it: what has to stay
    /// clear of the traffic that makes way for it.
    pub movement: Vec<usize>,
    exit: Option<usize>,
    /// The reserved movement shows the owner a stop aspect: it crosses against it, slowly.
    /// Sticky for the reservation's life, so the owner does not speed up once past the line.
    pub against_signal: bool,
    /// The vehicles on the owner's way between it and the junction: they lead it through
    /// and are not held back by its reservation.
    pub ahead: Vec<VehicleId>,
}

impl JunctionCoordinator {
    pub fn has_emergency_reservations(&self) -> bool {
        !self.emergency_reservations.is_empty()
    }
    /// Freeze requests once per tick. Stable IDs choose among simultaneous requests;
    /// vehicles already inside retain their reservation until their rear has cleared.
    pub fn prepare_emergencies(
        &mut self,
        net: &Network,
        actors: &[JunctionActor],
        ways: &[Vec<(usize, f32)>],
        on_lane: &HashMap<usize, Vec<(usize, f32, f32, bool)>>,
    ) {
        self.emergency_reservations.retain(|r| {
            actors
                .iter()
                .zip(ways)
                .find(|(a, _)| a.id == r.owner)
                .is_some_and(|(a, w)| {
                    r.lanes.iter().any(|lane| {
                        on_lane.get(lane).is_some_and(|bodies| {
                            bodies.iter().any(|&(i, _, _, foreign)| {
                                !foreign && actors.get(i).is_some_and(|part| part.id == a.id)
                            })
                        })
                    }) || r.lanes.contains(&a.lane)
                        || (r.exit == Some(a.lane) && a.s < a.rear + 1.0)
                        // (it stays while the queue moves on, not flickering with its speed)
                        || (a.emergency
                            && junction_ahead(net, w).is_some_and(|m| {
                                m.lanes[0].1 - a.front <= EMERGENCY_QUEUE_REACH
                                    && r.lanes.contains(&m.lanes[0].0)
                            }))
                })
        });
        let mut requests: Vec<_> = actors
            .iter()
            .zip(ways)
            .filter_map(|(a, w)| {
                if !a.emergency {
                    return None;
                }
                let m = junction_ahead(net, w)?;
                (m.inside || in_reach(a, w, &m, actors, on_lane)).then_some((a, m))
            })
            .collect();
        requests.sort_by_key(|(a, m)| (!m.inside, a.id));
        for (actor, movement) in requests {
            let lanes = junction_group(net, movement.lanes[0].0);
            if self
                .emergency_reservations
                .iter()
                .any(|r| r.lanes.iter().any(|l| lanes.contains(l)))
            {
                continue;
            }
            let way_through = movement
                .lanes
                .iter()
                .map(|&(l, _)| l)
                .chain(movement.exit.map(|x| x.0))
                .collect();
            self.emergency_reservations.push(EmergencyReservation {
                owner: actor.id,
                lanes,
                movement: way_through,
                exit: movement.exit.map(|x| x.0),
                against_signal: false,
                ahead: Vec::new(),
            });
        }
        // who drives in front of each owner on its way, up to and through the junction
        for r in &mut self.emergency_reservations {
            let Some((owner, way)) = actors.iter().zip(ways).find(|(a, _)| a.id == r.owner) else {
                r.ahead.clear();
                continue;
            };
            let until = junction_ahead(net, way).map(|m| {
                m.lanes.iter().map(|&(l, d)| d + net.lanes[l].length()).fold(0.0, f32::max)
            });
            r.ahead = way
                .iter()
                .filter(|&&(_, d)| until.is_some_and(|u| d <= u))
                .flat_map(|&(lane, d)| {
                    on_lane.get(&lane).into_iter().flatten().filter_map(move |&(j, s, _, foreign)| {
                        let along = d + s;
                        (!foreign && along > owner.front && j < actors.len()).then(|| actors[j].id)
                    })
                })
                .filter(|&id| id != owner.id)
                .collect();
        }
    }

    pub fn emergency_owns(&self, id: VehicleId, lane: usize) -> bool {
        self.emergency_reservations
            .iter()
            .any(|r| r.owner == id && r.lanes.contains(&lane))
    }

    /// Does `id` hold a reservation for an emergency drive anywhere?
    pub fn owns_emergency_reservation(&self, id: VehicleId) -> bool {
        self.emergency_reservations.iter().any(|r| r.owner == id)
    }

    /// Is `id` in front of an emergency vehicle on its way through the reserved junction of
    /// `lane`? It leads it through, at a red light carefully over the line.
    pub fn emergency_leads(&self, id: VehicleId, lane: usize) -> bool {
        self.emergency_reservations
            .iter()
            .any(|r| r.ahead.contains(&id) && r.lanes.contains(&lane))
    }

    /// The crawl speed for an emergency crossing its reserved junction against the signal.
    /// A green or unsignalled reserved movement is driven at the normal speed.
    pub fn emergency_speed_cap(&self, id: VehicleId) -> Option<f32> {
        self.emergency_reservations
            .iter()
            .any(|r| (r.owner == id || r.ahead.contains(&id)) && r.against_signal)
            .then_some(EMERGENCY_CROSSING_SPEED)
    }

    /// Does a movement through `lanes` cross, join or share the way an emergency vehicle
    /// reserved (`r.movement`)? Only that traffic stops for it: one junction object can
    /// hold a whole square (#462).
    pub(super) fn meets_emergency(
        net: &Network,
        r: &EmergencyReservation,
        lanes: &[usize],
    ) -> bool {
        lanes.iter().any(|&l| {
            r.movement.contains(&l)
                || net.crossings[l].iter().any(|c| r.movement.contains(&c.other))
                || net.lanes[l].next.iter().any(|n| r.movement.contains(n))
        })
    }

    pub(super) fn mark_against_signal(&mut self, id: VehicleId, lane: usize) {
        for r in &mut self.emergency_reservations {
            if r.owner == id && r.lanes.contains(&lane) {
                r.against_signal = true;
            }
        }
    }
}

/// How fast (m/s) an emergency crosses its reserved junction against a red light.
pub const EMERGENCY_CROSSING_SPEED: f32 = 5.0;
/// An emergency vehicle reserves the junction ahead this far (m) before it ...
const EMERGENCY_RESERVE_REACH: f32 = 45.0;
/// ... or at the end of a queue standing up to it, at most this long (m) and with no gap
/// wider than `EMERGENCY_QUEUE_GAP` (m), so the queue drains through in front of it (#462).
const EMERGENCY_QUEUE_REACH: f32 = 150.0;
const EMERGENCY_QUEUE_GAP: f32 = 15.0;

/// Is the junction `m` on `a`'s way `way` close enough for `a` to reserve it: near, or at
/// the head of the queue `a` stands at the end of?
fn in_reach(
    a: &JunctionActor,
    way: &[(usize, f32)],
    m: &Movement,
    actors: &[JunctionActor],
    on_lane: &HashMap<usize, Vec<(usize, f32, f32, bool)>>,
) -> bool {
    let entry = m.lanes[0].1;
    if entry - a.front < EMERGENCY_RESERVE_REACH {
        return true;
    }
    if entry - a.front > EMERGENCY_QUEUE_REACH {
        return false;
    }
    // (distance along the way to its origin, its front and rear extents, its speed)
    let mut queue: Vec<(f32, f32, f32, f32)> = way
        .iter()
        .filter(|&&(_, d)| d < entry)
        .flat_map(|&(lane, d)| {
            on_lane.get(&lane).into_iter().flatten().filter_map(move |&(j, s, _, foreign)| {
                let o = actors.get(j)?;
                let along = d + s;
                (!foreign && o.id != a.id && along > a.front && along - o.rear < entry)
                    .then_some((along, o.front, o.rear, o.speed))
            })
        })
        .collect();
    if queue.is_empty() {
        return false;
    }
    queue.sort_by(|x, y| x.0.total_cmp(&y.0));
    let mut reached = a.front;
    for (along, front, rear, speed) in queue {
        if along - rear - reached > EMERGENCY_QUEUE_GAP || speed > 2.0 {
            return false;
        }
        reached = reached.max(along + front);
    }
    entry - reached <= EMERGENCY_QUEUE_GAP
}

fn junction_group(net: &Network, first: usize) -> Vec<usize> {
    let mut group = vec![first];
    let mut i = 0;
    while i < group.len() {
        let lane = group[i];
        let object = net.lanes[lane].key.filter(|_| net.lanes[lane].source == 2);
        for other in net.crossings[lane].iter().map(|c| c.other).chain(
            net.lanes.iter().enumerate().filter_map(|(n, l)| {
                object
                    .filter(|k| {
                        l.source == 2 && l.key.is_some_and(|o| o.tile == k.tile && o.id == k.id)
                    })
                    .map(|_| n)
            }),
        ) {
            if !group.contains(&other) {
                group.push(other);
            }
        }
        i += 1;
    }
    group.sort_unstable();
    group
}
