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
