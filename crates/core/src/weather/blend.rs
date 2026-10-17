use super::model::*;
use super::template::{self, Template};

pub struct Blend {
    from: Weather,
    to: Weather,
    k: f32,
    secs: f32,
}

impl Blend {
    pub fn new(from: Weather, to: Weather, secs: f32) -> Blend {
        Blend {
            from,
            to,
            k: 0.0,
            secs: secs.max(0.5),
        }
    }

    pub fn progress(&self) -> f32 {
        self.k
    }

    pub fn target(&self) -> &Weather {
        &self.to
    }

    pub fn step(&mut self, dt: f32) -> (Weather, bool) {
        self.k = (self.k + dt / self.secs).min(1.0);
        (Weather::lerp(&self.from, &self.to, self.k), self.k >= 1.0)
    }
}

pub struct Cycle {
    pub next_in: f64,
    pub current: String,
    rng: u64,
}

fn plausible(w: &Weather, month: u32) -> bool {
    let winter = matches!(month, 11 | 12 | 1 | 2 | 3);
    let summer = matches!(month, 5..=9);
    if (w.is_snowy() || w.snow_cover) && !matches!(month, 10..=12 | 1..=4) {
        return false;
    }
    !(summer && w.temp_c < 3.0 || winter && w.temp_c > 16.0)
}

fn distance(a: &Weather, b: &Weather) -> f32 {
    let vis = |w: &Weather| (w.visibility_m.clamp(100.0, 60_000.0).ln() - 100f32.ln()) / (60_000f32.ln() - 100f32.ln());
    let wet = |w: &Weather| if w.precip.is_falling() { w.precip.intensity } else { 0.0 };
    (a.cover() - b.cover()).abs() * 1.5
        + (wet(a) - wet(b)).abs() * 2.5
        + (vis(a) - vis(b)).abs() * 1.5
        + (a.temp_c - b.temp_c).abs() / 20.0
}

impl Cycle {
    pub fn new(seed: u64) -> Cycle {
        let mut c = Cycle {
            next_in: 0.0,
            current: String::new(),
            rng: seed | 1,
        };
        c.next_in = c.interval();
        c
    }

    pub fn rand(&mut self) -> f32 {
        let mut x = self.rng;
        x ^= x << 13;
        x ^= x >> 7;
        x ^= x << 17;
        self.rng = x;
        (x >> 40) as f32 / (1u64 << 24) as f32
    }

    pub fn interval(&mut self) -> f64 {
        (20.0 + 30.0 * self.rand() as f64) * 60.0
    }

    pub fn pick(&mut self, month: u32, now: &Weather) -> Option<Template> {
        let library = template::library();
        let all: Vec<&Template> = library.iter().filter(|t| t.cycle && t.id != self.current).collect();
        let seasonal: Vec<&Template> = all
            .iter()
            .copied()
            .filter(|t| t.in_season(month) && plausible(&t.weather(), month))
            .collect();
        let pool = if seasonal.is_empty() { all } else { seasonal };
        let linked: Vec<&Template> = library
            .iter()
            .find(|t| t.id == self.current)
            .map(|c| pool.iter().copied().filter(|t| c.next.contains(&t.id)).collect())
            .unwrap_or_default();
        let pool = if linked.is_empty() { pool } else { linked };
        let weights: Vec<f32> = pool
            .iter()
            .map(|t| {
                let d = distance(&t.weather(), now);
                1.0 / (0.25 + d * d)
            })
            .collect();
        let total: f32 = weights.iter().sum();
        let mut x = self.rand().clamp(0.0, 0.9999) * total;
        for (t, w) in pool.iter().zip(&weights) {
            if x < *w {
                self.current = t.id.clone();
                return Some((*t).clone());
            }
            x -= w;
        }
        let last = (*pool.last()?).clone();
        self.current = last.id.clone();
        Some(last)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_blend_runs_from_one_weather_to_the_other() {
        let a = Weather::default();
        let mut b = Weather::default();
        b.temp_c = 5.0;
        b.visibility_m = 2000.0;
        b.precip = Precip { kind: PrecipKind::Rain, intensity: 0.8, ..Default::default() };
        let mut bl = Blend::new(a, b, 100.0);
        let (mid, done) = bl.step(50.0);
        assert!(!done && (mid.temp_c - 10.0).abs() < 1e-3 && mid.precip.intensity > 0.3);
        let (end, done) = bl.step(60.0);
        assert!(done && end.precip.kind == PrecipKind::Rain && (end.visibility_m - 2000.0).abs() < 1.0);
    }

    #[test]
    fn the_cycle_never_snows_in_july() {
        let mut c = Cycle::new(7);
        for _ in 0..40 {
            let t = c.pick(7, &Weather::default()).expect("a template");
            assert!(!t.weather().is_snowy(), "{}", t.id);
        }
    }
}
