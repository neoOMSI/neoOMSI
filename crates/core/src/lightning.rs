use glam::DVec3;
use ::render::{Corona, Scene};

const LIFE: f32 = 0.6;

struct Bolt {
    main: Vec<DVec3>,
    branches: Vec<Vec<DVec3>>,
    age: f32,
    closeness: f32,
    view: f32,
}

pub struct Lightning {
    rng: u64,
    next: f32,
    bolt: Option<Bolt>,
    pub flash: f32,
}

impl Default for Lightning {
    fn default() -> Self {
        Lightning::new()
    }
}

fn envelope(a: f32) -> f32 {
    if a < 0.06 {
        1.0
    } else if a < 0.1 {
        0.25
    } else if a < 0.17 {
        0.9
    } else {
        (-(a - 0.17) * 6.0).exp() * 0.6
    }
}

impl Lightning {
    pub fn new() -> Lightning {
        Lightning { rng: 0x1234_5678_9ABC_DEF1, next: 6.0, bolt: None, flash: 0.0 }
    }

    fn rand(&mut self) -> f32 {
        self.rng ^= self.rng << 13;
        self.rng ^= self.rng >> 7;
        self.rng ^= self.rng << 17;
        (self.rng >> 40) as f32 / (1u64 << 24) as f32
    }

    fn path(&mut self, top: DVec3, bottom: DVec3, steps: usize, spread: f64) -> Vec<DVec3> {
        let mut pts = Vec::with_capacity(steps + 1);
        let (mut jx, mut jy) = (0.0f64, 0.0f64);
        for i in 0..=steps {
            let t = i as f64 / steps as f64;
            jx += (self.rand() as f64 - 0.5) * spread;
            jy += (self.rand() as f64 - 0.5) * spread;
            let k = (std::f64::consts::PI * t).sin();
            let mut p = top + (bottom - top) * t;
            if i != 0 && i != steps {
                p.x += jx * k;
                p.y += jy * k;
            }
            pts.push(p);
        }
        pts
    }

    fn strike(&mut self, camera: DVec3, ground: &impl Fn(f64, f64) -> Option<f64>, visibility: f32) {
        let r = self.rand();
        let dist = 60.0 + (r * r) as f64 * 5400.0;
        let bearing = self.rand() as f64 * std::f64::consts::TAU;
        let (x, y) = (camera.x + bearing.cos() * dist, camera.y + bearing.sin() * dist);
        let gz = ground(x, y).unwrap_or(camera.z - 2.0);
        let bottom = DVec3::new(x, y, gz);
        let height = 1000.0 + self.rand() as f64 * 600.0;
        let top = bottom + DVec3::new((self.rand() as f64 - 0.5) * 400.0, (self.rand() as f64 - 0.5) * 400.0, height);
        let main = self.path(top, bottom, 28, 60.0);
        let mut branches = Vec::new();
        for _ in 0..(2 + (self.rand() * 3.0) as usize) {
            let i = 4 + (self.rand() * 16.0) as usize;
            let from = main[i.min(main.len() - 1)];
            let a = self.rand() as f64 * std::f64::consts::TAU;
            let len = 150.0 + self.rand() as f64 * 350.0;
            let end = from + DVec3::new(a.cos() * len, a.sin() * len, -len * (0.6 + self.rand() as f64));
            branches.push(self.path(from, end, 8, 40.0));
        }
        let closeness = (2500.0 / (dist as f32 + 500.0)).clamp(0.0, 1.0);
        let view = 0.35 + 0.65 * (visibility * 2.5 / dist as f32).clamp(0.0, 1.0);
        self.bolt = Some(Bolt { main, branches, age: 0.0, closeness, view });
    }

    pub fn tick(
        &mut self,
        dt: f32,
        storm: f32,
        visibility: f32,
        camera: DVec3,
        ground: impl Fn(f64, f64) -> Option<f64>,
        scene: &mut Scene,
    ) {
        if storm > 0.05 && self.bolt.is_none() {
            self.next -= dt;
            if self.next <= 0.0 {
                self.strike(camera, &ground, visibility);
                self.next = (3.0 + self.rand() * 14.0) / (0.3 + storm);
            }
        }
        self.flash = 0.0;
        let Some(b) = self.bolt.as_mut() else {
            return;
        };
        b.age += dt;
        if b.age >= LIFE {
            self.bolt = None;
            return;
        }
        let env = envelope(b.age);
        self.flash = env * (0.3 + 0.7 * b.closeness) * (0.5 + 0.5 * storm.max(0.3));
        if b.age > 0.3 {
            return;
        }
        let mut push = |pts: &[DVec3], strength: f32| {
            for s in pts.windows(2) {
                let d = s[1] - s[0];
                let len = d.length();
                if len < 1e-3 {
                    continue;
                }
                scene.coronas.push(Corona {
                    position: (s[0] + s[1]) * 0.5,
                    size: (len / 16.0) as f32,
                    color: [0.8, 0.85, 1.0],
                    brightness: strength * env * b.view,
                    direction: (d / len).as_vec3(),
                    cone_cos: -2.0,
                    ..Default::default()
                });
            }
        };
        push(&b.main, 6.0);
        for br in &b.branches {
            push(br, 2.5);
        }
    }
}
