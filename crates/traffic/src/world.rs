//! Immutable per-tick snapshot, committed state, and deterministic arbitration.
//!
//! Planning reads one [`Snapshot`]; it never mutates another vehicle. Candidate decisions
//! that need a shared resource (a junction lane, a merge order, downstream exit space) go
//! through [`Arbiter`], which grants deterministically by stable [`VehicleId`]. The result
//! is written back as a [`Commit`] at the end of the tick and read as the previous commit
//! on the next, so previous-frame blocker/leader relations are keyed by id rather than by
//! container position.

use crate::ids::{LaneId, NetworkVersion, VehicleId};
use crate::perception::Occupancy;
use hashbrown::HashMap;

/// What was committed last tick: which vehicle each one kept behind, and the lane claims it
/// held. Keyed by stable id, so reordering the entity container changes nothing.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Commit {
    pub blocked_by: HashMap<VehicleId, VehicleId>,
    pub claims: HashMap<LaneId, Vec<VehicleId>>,
}

impl Commit {
    /// Who this vehicle waited behind last tick, if anyone.
    pub fn blocker_of(&self, id: VehicleId) -> Option<VehicleId> {
        self.blocked_by.get(&id).copied()
    }

    pub fn holders(&self, lane: LaneId) -> &[VehicleId] {
        self.claims.get(&lane).map(|v| v.as_slice()).unwrap_or(&[])
    }

    pub fn holds(&self, lane: LaneId, id: VehicleId) -> bool {
        self.holders(lane).contains(&id)
    }
}

/// One frozen tick that all planners read: the realized occupancy, the previous commit, and
/// the network version it was built against.
#[derive(Debug, Clone, Default)]
pub struct Snapshot {
    pub tick: u64,
    pub network_version: NetworkVersion,
    pub occupancy: Occupancy,
    pub previous: Commit,
}

impl Snapshot {
    pub fn new(tick: u64, network_version: NetworkVersion, occupancy: Occupancy, previous: Commit) -> Snapshot {
        Snapshot {
            tick,
            network_version,
            occupancy,
            previous,
        }
    }
}

/// Exit storage reserved at one lane: the free distance to the first obstacle is shared by
/// every admitted vehicle.
#[derive(Debug, Clone, Default, PartialEq)]
struct Storage {
    capacity: f32,
    slots: Vec<(VehicleId, f32)>,
}

/// The deterministic arbiter for lane reservations, simultaneous merges, and downstream
/// exit storage. Grants are keyed by stable id; ties go to the lower id, never to container
/// order, so the same inputs decide the same way whatever the storage order.
#[derive(Debug, Clone, Default)]
pub struct Arbiter {
    claims: HashMap<LaneId, Vec<VehicleId>>,
    storage: HashMap<LaneId, Storage>,
}

impl Arbiter {
    pub fn new() -> Arbiter {
        Arbiter::default()
    }

    /// Vehicles holding a claim on `lane`, in id order.
    pub fn holders(&self, lane: LaneId) -> &[VehicleId] {
        self.claims.get(&lane).map(|v| v.as_slice()).unwrap_or(&[])
    }

    pub fn holds(&self, lane: LaneId, id: VehicleId) -> bool {
        self.holders(lane).contains(&id)
    }

    /// Grant `id` a claim on `lane` (idempotent). Returns whether it was newly granted.
    pub fn grant(&mut self, lane: LaneId, id: VehicleId) -> bool {
        let list = self.claims.entry(lane).or_default();
        if list.contains(&id) {
            return false;
        }
        list.push(id);
        list.sort_unstable();
        true
    }

    pub fn release(&mut self, lane: LaneId, id: VehicleId) {
        if let Some(list) = self.claims.get_mut(&lane) {
            list.retain(|&x| x != id);
        }
    }

    pub fn release_all(&mut self, id: VehicleId, lanes: &[LaneId]) {
        for &l in lanes {
            self.release(l, id);
        }
    }

    /// Refresh the free storage at `lane` (distance to the first obstacle, m) this tick.
    pub fn set_storage_capacity(&mut self, lane: LaneId, capacity: f32) {
        self.storage.entry(lane).or_default().capacity = capacity.max(0.0);
    }

    /// Try to reserve `need` metres of `lane`'s exit storage for `id`. Every admitted slot
    /// counts against the same capacity, so the exit is not promised twice.
    pub fn reserve_storage(&mut self, lane: LaneId, id: VehicleId, need: f32) -> bool {
        let s = self.storage.entry(lane).or_default();
        if let Some(slot) = s.slots.iter_mut().find(|(x, _)| *x == id) {
            slot.1 = need.max(0.0);
            return true;
        }
        let used: f32 = s.slots.iter().map(|(_, n)| *n).sum();
        if used + need.max(0.0) > s.capacity {
            return false;
        }
        s.slots.push((id, need.max(0.0)));
        s.slots.sort_by_key(|(x, _)| *x);
        true
    }

    pub fn storage_reserved(&self, lane: LaneId, id: VehicleId) -> bool {
        self.storage
            .get(&lane)
            .map(|s| s.slots.iter().any(|(x, _)| *x == id))
            .unwrap_or(false)
    }

    pub fn release_storage(&mut self, lane: LaneId, id: VehicleId) {
        if let Some(s) = self.storage.get_mut(&lane) {
            s.slots.retain(|(x, _)| *x != id);
        }
    }

    /// The deterministic winner of a simultaneous merge for `lane`: the lowest id among the
    /// candidates that already hold a claim, else the lowest candidate id.
    pub fn merge_winner(&self, lane: LaneId, candidates: &[VehicleId]) -> Option<VehicleId> {
        let claimed = candidates.iter().copied().filter(|id| self.holds(lane, *id));
        claimed.min().or_else(|| candidates.iter().copied().min())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_commit_is_read_back_by_id_not_position() {
        let mut c = Commit::default();
        c.blocked_by.insert(VehicleId(3), VehicleId(9));
        c.claims.entry(LaneId(4)).or_default().push(VehicleId(3));
        assert_eq!(c.blocker_of(VehicleId(3)), Some(VehicleId(9)));
        assert!(c.holds(LaneId(4), VehicleId(3)));
        assert!(!c.holds(LaneId(4), VehicleId(9)));
    }

    #[test]
    fn two_vehicles_cannot_reserve_the_same_empty_exit() {
        let lane = LaneId(0);
        let mut a = Arbiter::new();
        a.set_storage_capacity(lane, 15.0);
        // A 10 m bus reserves the exit; a second cannot also take the same 15 m.
        assert!(a.reserve_storage(lane, VehicleId(1), 10.0));
        assert!(!a.reserve_storage(lane, VehicleId(2), 10.0));
        // Releasing the bus frees the space for the second.
        a.release_storage(lane, VehicleId(1));
        assert!(a.reserve_storage(lane, VehicleId(2), 10.0));
    }

    #[test]
    fn a_simultaneous_merge_goes_to_the_lowest_id() {
        let lane = LaneId(2);
        let mut a = Arbiter::new();
        let candidates = [VehicleId(7), VehicleId(2), VehicleId(5)];
        // No claims yet: the lowest id wins.
        assert_eq!(a.merge_winner(lane, &candidates), Some(VehicleId(2)));
        // With a claim from 5 it wins even though 2 is lower.
        a.grant(lane, VehicleId(5));
        assert_eq!(a.merge_winner(lane, &candidates), Some(VehicleId(5)));
    }

    #[test]
    fn grants_are_idempotent_and_sorted() {
        let lane = LaneId(1);
        let mut a = Arbiter::new();
        assert!(a.grant(lane, VehicleId(9)));
        assert!(a.grant(lane, VehicleId(4)));
        assert!(!a.grant(lane, VehicleId(9)));
        assert_eq!(a.holders(lane), &[VehicleId(4), VehicleId(9)]);
    }
}
