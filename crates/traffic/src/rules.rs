//! Content-derived traffic rule primitives: path priority and per-group traffic density.

/// Priority of a path without a `[rule] priority`.
pub const DEFAULT_PRIORITY: f32 = 128.0;

/// How much of `unsched_vehgroups.txt` group `pool`'s traffic a path carries: its `[rule]
/// trafficdensity` for the group (`rules`, see `Lane::group_density`: the last rule of a
/// group counts), else the group's default there (`defaults`): 0 none, for the first group 1
/// its medium density, for any other k that of the k-th group on the same path.
pub fn pool_density(rules: &[(u16, f32)], defaults: &[i32], pool: usize) -> f32 {
    let mut u = pool;
    // (a default naming another group that names this one again would go round for ever)
    for _ in 0..=defaults.len() {
        if let Some(&(_, v)) = rules.iter().rev().find(|(k, _)| *k as usize == u) {
            return v;
        }
        match defaults.get(u).copied().unwrap_or(0) {
            d if d <= 0 => return 0.0,
            _ if u == 0 => return 1.0,
            d => u = d as usize - 1,
        }
    }
    0.0
}

/// A lane's spawn density: each group's density there (its own rule or the one it inherits,
/// see `pool_density`) times its `uvg_share` (at most 1; 0 for a group not loaded).
/// Unweighed, BRT Berlin's Kurfürstendamm bus lanes (Ambulance 3.0, factor 0.0002) spawned
/// queues of ambulances (#411). Open lanes stay above 0.
pub fn weigh_lane_density(
    density: f32,
    rules: &[(u16, f32)],
    defaults: &[i32],
    uvg_share: &[f32],
) -> f32 {
    if uvg_share.is_empty() || rules.is_empty() || density <= 0.0 {
        return density;
    }
    uvg_share
        .iter()
        .enumerate()
        .map(|(u, share)| pool_density(rules, defaults, u) * share.min(1.0))
        .fold(0.0, f32::max)
        .max(1e-4)
}

#[cfg(test)]
mod lane_density_tests {
    use super::weigh_lane_density;

    fn weighed(
        density: f32,
        group_density: Vec<(u16, f32)>,
        defaults: &[i32],
        share: &[f32],
    ) -> f32 {
        weigh_lane_density(density, &group_density, defaults, share)
    }

    /// BRT Berlin's Kurfürstendamm bus lanes (#411) make next to no traffic, but stay open.
    #[test]
    fn a_rare_group_brings_its_rarity_to_the_lane() {
        let defaults = [1, 0, 1, 1, 0, 0, 1];
        let share = [1.0, 0.05, 0.1, 0.0001, 1.0, 1.0, 0.05];
        let d = weighed(3.0, vec![(0, 0.0), (3, 3.0)], &defaults, &share);
        assert!(d > 0.0 && d < 0.001, "{d}");
        // as busy as the first group: unchanged; busier: not raised
        assert_eq!(
            weighed(2.0, vec![(0, 0.0), (4, 2.0)], &defaults, &share),
            2.0
        );
        assert_eq!(
            weighed(1.0, vec![(0, 0.0), (1, 1.0)], &[1, 0], &[1.0, 4.2]),
            1.0
        );
        assert_eq!(
            weighed(1.0, vec![(0, 1.0), (3, 1.0)], &defaults, &share),
            1.0
        );
        // no rule for the first group: its medium density
        assert_eq!(weighed(3.0, vec![(3, 3.0)], &defaults, &share), 1.0);
        // closed and rule-less lanes, and maps without groups, unchanged
        assert_eq!(weighed(0.0, vec![(0, 0.0)], &defaults, &share), 0.0);
        assert_eq!(weighed(0.5, Vec::new(), &defaults, &share), 0.5);
        assert_eq!(weighed(3.0, vec![(0, 0.0), (3, 3.0)], &[], &[]), 3.0);
    }

    /// A busy group inheriting a rare group's rule keeps the lane busy, unless it is not
    /// loaded.
    #[test]
    fn an_inherited_density_counts_with_its_own_group() {
        let rules = vec![(0, 0.0), (1, 3.0)];
        assert_eq!(
            weighed(3.0, rules.clone(), &[1, 0, 2], &[1.0, 0.001, 1.0]),
            3.0
        );
        let d = weighed(3.0, rules, &[1, 0, 2], &[1.0, 0.001, 0.0]);
        assert!((d - 0.003).abs() < 1e-6, "{d}");
    }
}
