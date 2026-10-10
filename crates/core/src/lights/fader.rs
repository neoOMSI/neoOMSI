use std::collections::HashMap;

struct Entry {
    level: f32,
    frame: u64,
}

#[derive(Default)]
pub(super) struct Faders {
    levels: HashMap<(usize, u32), Entry>,
    frame: u64,
    dt: f32,
}

impl Faders {
    pub(super) fn begin_frame(&mut self, dt: f32) {
        self.frame += 1;
        self.dt = dt.clamp(0.0, 0.25);
        if self.frame % 256 == 0 {
            let frame = self.frame;
            self.levels.retain(|_, e| frame - e.frame < 512);
        }
    }

    pub(super) fn level(&mut self, owner: usize, slot: u32, target: f32, rise: f32, fall: f32) -> f32 {
        let (frame, dt) = (self.frame, self.dt);
        let e = self.levels.entry((owner, slot)).or_insert(Entry {
            level: target,
            frame: 0,
        });
        if e.frame == frame {
            return e.level;
        }
        e.frame = frame;
        let tau = if target > e.level { rise } else { fall };
        e.level = if tau <= 1e-4 {
            target
        } else {
            target + (e.level - target) * (-dt / tau).exp()
        };
        if (e.level - target).abs() < 0.004 {
            e.level = target;
        }
        e.level
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn eases_and_settles() {
        let mut f = Faders::default();
        f.begin_frame(0.016);
        assert_eq!(f.level(1, 0, 0.0, 0.05, 0.12), 0.0);
        f.begin_frame(0.016);
        let a = f.level(1, 0, 1.0, 0.05, 0.12);
        assert!(a > 0.0 && a < 1.0);
        assert_eq!(f.level(1, 0, 1.0, 0.05, 0.12), a);
        for _ in 0..60 {
            f.begin_frame(0.05);
            f.level(1, 0, 1.0, 0.05, 0.12);
        }
        f.begin_frame(0.05);
        assert_eq!(f.level(1, 0, 1.0, 0.05, 0.12), 1.0);
    }
}
