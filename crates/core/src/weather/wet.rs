use super::model::Weather;
use super::region::Regional;

pub const SIDE: usize = 16;
pub const CELL_M: f64 = 160.0;
const SLOTS: usize = 32;
const STEP_S: f64 = 0.5;
const WET_PER_S: f32 = 1.0 / 180.0;
const DRY_PER_S: f32 = 1.0 / 1200.0;

#[derive(Clone, Copy)]
struct Slot {
    key: (i32, i32),
    wet: f32,
    at: f64,
    set: bool,
}

pub struct WetGrid {
    slots: Vec<Slot>,
    clock: f64,
    last: f64,
    base: f32,
}

fn advance(wet: f32, rate: f32, secs: f64) -> f32 {
    let secs = secs as f32;
    if rate > 0.0 {
        (wet + secs * rate * WET_PER_S).clamp(0.0, 1.0)
    } else {
        (wet - secs * DRY_PER_S).clamp(0.0, 1.0)
    }
}

impl WetGrid {
    pub fn new() -> WetGrid {
        WetGrid {
            slots: vec![
                Slot { key: (0, 0), wet: 0.0, at: 0.0, set: false };
                SLOTS * SLOTS
            ],
            clock: 0.0,
            last: -1.0,
            base: 0.0,
        }
    }

    pub fn set_all(&mut self, wet: f32) {
        self.base = wet.clamp(0.0, 1.0);
        for s in self.slots.iter_mut() {
            s.set = false;
        }
    }

    fn index(key: (i32, i32)) -> usize {
        key.1.rem_euclid(SLOTS as i32) as usize * SLOTS + key.0.rem_euclid(SLOTS as i32) as usize
    }

    fn cell(&self, key: (i32, i32)) -> f32 {
        let s = &self.slots[Self::index(key)];
        if s.set && s.key == key { s.wet } else { self.base }
    }

    fn corner(observer: [f64; 2]) -> (i32, i32) {
        (
            (observer[0] / CELL_M).floor() as i32 - SIDE as i32 / 2,
            (observer[1] / CELL_M).floor() as i32 - SIDE as i32 / 2,
        )
    }

    pub fn update(&mut self, dt: f64, w: &Weather, region: &Regional, observer: [f64; 2], enhanced: bool) {
        self.clock += dt.max(0.0);
        if self.clock - self.last < STEP_S {
            return;
        }
        self.last = self.clock;
        let (cx0, cy0) = Self::corner(observer);
        for j in 0..SIDE as i32 {
            for i in 0..SIDE as i32 {
                let key = (cx0 + i, cy0 + j);
                let at = [(key.0 as f64 + 0.5) * CELL_M, (key.1 as f64 + 0.5) * CELL_M];
                let s = region.sample(w, at, enhanced);
                let rate = if s.precip_kind == 1 { s.precip_rate } else { 0.0 };
                let base = self.base;
                let clock = self.clock;
                let slot = &mut self.slots[Self::index(key)];
                if !slot.set || slot.key != key {
                    *slot = Slot { key, wet: base, at: clock, set: true };
                }
                let elapsed = (clock - slot.at).min(3600.0);
                slot.wet = advance(slot.wet, rate, elapsed);
                slot.at = clock;
            }
        }
    }

    pub fn at(&self, x: f64, y: f64) -> f32 {
        let (gx, gy) = (x / CELL_M - 0.5, y / CELL_M - 0.5);
        let (fx, fy) = (gx.floor(), gy.floor());
        let (tx, ty) = ((gx - fx) as f32, (gy - fy) as f32);
        let (ix, iy) = (fx as i32, fy as i32);
        let a = self.cell((ix, iy)) * (1.0 - tx) + self.cell((ix + 1, iy)) * tx;
        let b = self.cell((ix, iy + 1)) * (1.0 - tx) + self.cell((ix + 1, iy + 1)) * tx;
        (a * (1.0 - ty) + b * ty).clamp(0.0, 1.0)
    }

    pub fn fill(&self, l: &mut ::render::Lighting, observer: [f64; 2]) {
        let (cx0, cy0) = Self::corner(observer);
        for j in 0..SIDE as i32 {
            for i in 0..SIDE as i32 {
                l.wet_grid[j as usize * SIDE + i as usize] = self.cell((cx0 + i, cy0 + j));
            }
        }
        l.wet_origin = [
            (cx0 as f64 * CELL_M - observer[0]) as f32,
            (cy0 as f64 * CELL_M - observer[1]) as f32,
        ];
        l.wet_cell = CELL_M as f32;
    }
}

impl Default for WetGrid {
    fn default() -> Self {
        WetGrid::new()
    }
}
