use super::model::*;
use super::sky::{self, Field};

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Local {
    pub thick: f32,
    pub precip_kind: i32,
    pub precip_rate: f32,
    pub visibility_m: f32,
    pub storm: f32,
}

impl Local {
    pub fn precip(&self) -> (i32, f32) {
        (self.precip_kind, self.precip_rate)
    }

    pub fn snowing(&self) -> bool {
        self.precip_kind == 2 && self.precip_rate > 0.0
    }

    pub fn uniform(w: &Weather) -> Local {
        let rate = if w.falling() { w.precip.intensity.clamp(0.0, 1.0) } else { 0.0 };
        Local {
            thick: sky::thickness(w.cover(), 0.5, true),
            precip_kind: if rate > 0.0 { w.precip.sim_kind() } else { 0 },
            precip_rate: rate,
            visibility_m: w.visibility_m,
            storm: if w.precip.thunder { rate } else { 0.0 },
        }
    }
}

impl Default for Local {
    fn default() -> Self {
        Local::uniform(&Weather::default())
    }
}

pub fn areal_fraction(w: &Weather) -> f32 {
    if !w.precip.is_falling() {
        return 0.0;
    }
    let c = w.cover();
    if w.precip.thunder {
        0.12 + 0.2 * c
    } else if w.precip.showers || w.precip.vicinity {
        0.18 + 0.4 * c
    } else if c >= 0.85 {
        0.97
    } else {
        0.5 + 0.45 * c
    }
}

#[derive(Debug, Clone, Copy)]
struct Slide {
    delta: [f32; 2],
    secs: f32,
    t: f32,
    applied: [f32; 2],
}

#[derive(Debug, Clone)]
pub struct Regional {
    origin: [f32; 2],
    drift: [f32; 2],
    slide: Option<Slide>,
    enhanced: bool,
    seed: u32,
}

fn hash(mut h: u32) -> u32 {
    h ^= h >> 16;
    h = h.wrapping_mul(0x85eb_ca6b);
    h ^= h >> 13;
    h = h.wrapping_mul(0xc2b2_ae35);
    h ^ (h >> 16)
}

fn wrap_to(v: f32, period: f32) -> f32 {
    (v + period * 0.5).rem_euclid(period) - period * 0.5
}

impl Regional {
    pub fn new() -> Regional {
        let seed = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.subsec_nanos() ^ d.as_secs() as u32)
            .unwrap_or(1);
        let h = hash(seed);
        Regional {
            origin: [(h & 0xffff) as f32 / 65536.0, (h >> 16) as f32 / 65536.0],
            drift: [0.0; 2],
            slide: None,
            enhanced: true,
            seed: h,
        }
    }

    pub fn offset(&self) -> [f32; 2] {
        [
            (self.origin[0] + self.drift[0]).rem_euclid(1.0),
            (self.origin[1] + self.drift[1]).rem_euclid(1.0),
        ]
    }

    pub fn advance(&mut self, w: &Weather, secs: f32) {
        let (dir, speed) = (w.wind_dir.to_radians() as f64, w.wind_speed as f64);
        let s = secs as f64 * speed / sky::ENHANCED_TILE_M;
        self.drift[0] = (self.drift[0] as f64 + dir.sin() * s).rem_euclid(1.0) as f32;
        self.drift[1] = (self.drift[1] as f64 + dir.cos() * s).rem_euclid(1.0) as f32;
        if let Some(s) = self.slide.as_mut() {
            s.t = (s.t + secs / s.secs).min(1.0);
            let e = s.t * s.t * (3.0 - 2.0 * s.t);
            let now = [s.delta[0] * e, s.delta[1] * e];
            self.origin[0] = (self.origin[0] + now[0] - s.applied[0]).rem_euclid(1.0);
            self.origin[1] = (self.origin[1] + now[1] - s.applied[1]).rem_euclid(1.0);
            s.applied = now;
            if s.t >= 1.0 {
                self.slide = None;
            }
        }
    }

    pub fn anchor(&mut self, w: &Weather, observer: [f64; 2], enhanced: bool) {
        self.enhanced = enhanced;
        self.slide = None;
        if let Some(target) = self.solve(w, observer, enhanced, None) {
            self.origin = [
                (target[0] - self.drift[0]).rem_euclid(1.0),
                (target[1] - self.drift[1]).rem_euclid(1.0),
            ];
        }
    }

    pub fn retarget(&mut self, to: &Weather, observer: [f64; 2], enhanced: bool, secs: f32) {
        if enhanced != self.enhanced {
            self.anchor(to, observer, enhanced);
            return;
        }
        let now = self.offset();
        let Some(target) = self.solve(to, observer, enhanced, Some(now)) else {
            return;
        };
        let period = 1.0 / Field::offset_scale(enhanced);
        let delta = [
            wrap_to(target[0] - now[0], period),
            wrap_to(target[1] - now[1], period),
        ];
        if delta[0].abs() + delta[1].abs() > 1e-4 {
            self.slide = Some(Slide {
                delta,
                secs: secs.max(1.0),
                t: 0.0,
                applied: [0.0; 2],
            });
        }
    }

    fn solve(&self, w: &Weather, observer: [f64; 2], enhanced: bool, near: Option<[f32; 2]>) -> Option<[f32; 2]> {
        let f = areal_fraction(w);
        if f <= 0.0 {
            return None;
        }
        let field = sky::field();
        let n = sky::COARSE;
        let k = Field::offset_scale(enhanced);
        let period = 1.0 / k;
        let base = Field::base_uv(observer, enhanced);
        let tile = Field::tile_m(enhanced) as f32;
        let here = !w.precip.vicinity;
        let (sin, cos) = (w.wind_dir.to_radians().sin(), w.wind_dir.to_radians().cos());
        let reach = (w.wind_speed * 600.0).clamp(4000.0, 12000.0);
        let (du, dv) = (sin * reach / tile, cos * reach / tile);
        let edge = 1.0 - f;
        let offset_of = |idx: usize| -> [f32; 2] {
            let (u, v) = (((idx % n) as f32 + 0.5) / n as f32, ((idx / n) as f32 + 0.5) / n as f32);
            [(u - base[0]).rem_euclid(1.0) / k, (v - base[1]).rem_euclid(1.0) / k]
        };
        let mut best: Option<(f32, [f32; 2])> = None;
        let start = (self.seed as usize) % (n * n);
        for step in 0..n * n {
            let idx = (start + step * 2749) % (n * n);
            let cell = field.grid(enhanced)[idx];
            let fits = if here {
                let core = smoothstep(edge, 1.0, cell);
                cell >= edge + 0.08 && (0.25..=0.8).contains(&core)
            } else {
                let (u, v) = (((idx % n) as f32 + 0.5) / n as f32, ((idx / n) as f32 + 0.5) / n as f32);
                let upwind = field.at(enhanced, (u + du).rem_euclid(1.0), (v + dv).rem_euclid(1.0));
                cell <= edge - 0.06 && upwind >= edge + 0.1
            };
            if !fits {
                continue;
            }
            let offset = offset_of(idx);
            let Some(now) = near else {
                return Some(offset);
            };
            let score = -(wrap_to(offset[0] - now[0], period).abs() + wrap_to(offset[1] - now[1], period).abs());
            if best.is_none_or(|b| score > b.0) {
                best = Some((score, offset));
            }
        }
        if let Some((_, o)) = best {
            return Some(o);
        }
        let wanted = |idx: usize| {
            let cell = field.grid(enhanced)[idx];
            if here { cell } else { -cell }
        };
        let idx = (0..n * n).max_by(|a, b| wanted(*a).total_cmp(&wanted(*b)))?;
        Some(offset_of(idx))
    }

    pub fn sample(&self, w: &Weather, observer: [f64; 2], enhanced: bool) -> Local {
        let cell = sky::field().cell(observer, self.offset(), enhanced);
        let cover = w.cover();
        let thick = sky::thickness(cover, cell, enhanced);
        let f = areal_fraction(w);
        let (gate, core) = if f > 0.0 {
            (smoothstep(1.0 - f - 0.06, 1.0 - f + 0.06, cell), smoothstep(1.0 - f, 1.0, cell))
        } else {
            (0.0, 0.0)
        };
        let rate = (w.precip.intensity * gate * (0.7 + 0.6 * core)).clamp(0.0, 1.0);
        let clear_air = (w.visibility_m * 3.0).min(30_000.0).max(w.visibility_m);
        let visibility = if f > 0.0 {
            clear_air + (w.visibility_m - clear_air) * gate
        } else {
            w.visibility_m
        };
        Local {
            thick: thick.max(0.9 * gate),
            precip_kind: if rate > 0.003 { w.precip.sim_kind() } else { 0 },
            precip_rate: rate,
            visibility_m: visibility,
            storm: if w.precip.thunder { gate } else { 0.0 },
        }
    }
}

impl Default for Regional {
    fn default() -> Self {
        Regional::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::weather::metar;

    #[test]
    fn a_reported_shower_falls_where_the_player_stands_but_not_everywhere() {
        let w = metar::parse("ZZZZ 011200Z 26012KT 9000 SHRA SCT025 BKN040CB 14/08 Q1005");
        let mut r = Regional::new();
        r.anchor(&w, [1234.0, 5678.0], true);
        let here = r.sample(&w, [1234.0, 5678.0], true);
        assert!(here.precip_rate > 0.1, "{here:?}");
        let dry = (0..200)
            .filter(|i| r.sample(&w, [*i as f64 * 1500.0, 0.0], true).precip_rate < 0.01)
            .count();
        assert!(dry > 60, "{dry}");
    }

    #[test]
    fn rain_in_the_vicinity_is_not_over_the_player() {
        let w = metar::parse("ZZZZ 011200Z 27010KT 9999 VCSH SCT028 BKN045CB 16/09 Q1008");
        let mut r = Regional::new();
        r.anchor(&w, [0.0, 0.0], true);
        assert!(r.sample(&w, [0.0, 0.0], true).precip_rate < 0.01);
    }

    #[test]
    fn steady_rain_falls_nearly_everywhere() {
        let w = metar::parse("ZZZZ 011200Z 23012KT 4000 RA OVC012 12/11 Q0998");
        let r = Regional::new();
        let wet = (0..200)
            .filter(|i| r.sample(&w, [*i as f64 * 1500.0, 900.0], true).precip_rate > 0.05)
            .count();
        assert!(wet > 170, "{wet}");
    }
}
