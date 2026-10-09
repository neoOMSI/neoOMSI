use std::collections::HashMap;
use std::sync::{LazyLock, Mutex};
use std::time::Instant;

pub(super) const LAMP_RISE: f32 = 0.05;
pub(super) const LAMP_FALL: f32 = 0.12;

pub(super) const SALOON_RISE: f32 = 0.08;
pub(super) const SALOON_FALL: f32 = 0.10;

pub(super) const SLOT_SPOT2: u32 = 1000;
pub(super) const SLOT_SALOON: u32 = 2000;

struct Slot {
    level: f32,
    seen: Instant,
}

static LEVELS: LazyLock<Mutex<HashMap<(usize, u32), Slot>>> = LazyLock::new(Default::default);

pub(super) fn key_of<T>(t: &T) -> usize {
    t as *const T as usize
}

pub(super) fn lamp_level(owner: usize, slot: u32, target: f32, rise: f32, fall: f32) -> f32 {
    let now = Instant::now();
    let mut map = LEVELS.lock().unwrap_or_else(|e| e.into_inner());
    if map.len() > 2048 {
        map.retain(|_, s| now.duration_since(s.seen).as_secs_f32() < 5.0);
        if map.len() > 2048 {
            map.clear();
        }
    }
    let s = map
        .entry((owner, slot))
        .or_insert(Slot { level: target, seen: now });
    let dt = now.duration_since(s.seen).as_secs_f32().min(0.25);
    s.seen = now;
    let tau = if target > s.level { rise } else { fall };
    s.level = if tau <= 1e-4 {
        target
    } else {
        target + (s.level - target) * (-dt / tau).exp()
    };
    if (s.level - target).abs() < 0.004 {
        s.level = target;
    }
    s.level
}
