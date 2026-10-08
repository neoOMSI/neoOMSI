//! Short, exclusive junction reservations for active emergency drives. Existing occupants
//! and committed movements drain normally; no timer removes physical occupancy.
use super::*;

#[derive(Debug, Clone)]
pub(super) struct EmergencyReservation {
    pub owner: VehicleId,
    pub lanes: Vec<usize>,
    exit: Option<usize>,
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
                        || (a.emergency
                            && junction_ahead(net, w).is_some_and(|m| {
                                m.lanes[0].1 - a.front < 45.0 && r.lanes.contains(&m.lanes[0].0)
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
                (m.inside || m.lanes[0].1 - a.front < 45.0).then_some((a, m))
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
            self.emergency_reservations.push(EmergencyReservation {
                owner: actor.id,
                lanes,
                exit: movement.exit.map(|x| x.0),
            });
        }
    }

    pub fn emergency_owns(&self, id: VehicleId, lane: usize) -> bool {
        self.emergency_reservations
            .iter()
            .any(|r| r.owner == id && r.lanes.contains(&lane))
    }

    pub fn emergency_speed_cap(&self, id: VehicleId) -> Option<f32> {
        self.emergency_reservations
            .iter()
            .any(|r| r.owner == id)
            .then_some(5.0)
    }
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
