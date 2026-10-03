//! Particle systems: `[smoke]` and `[particle_emitter]` of vehicles and scenery objects, as
//! OMSI runs them (TRauch / TRauchInst: the original emits, the original sets a particle
//! off, the original moves it). An emitter keeps at most 100 particles. A particle leaves along
//! the emitter's direction at its speed plus a random spread; every frame its velocity is
//! multiplied by the brake factor (per frame, not per second: taken at OMSI's default 30 fps
//! here) and gravity pulls it down (a negative factor makes it rise); it grows from its start
//! size by `size_grow` a second, and its alpha goes linearly from the initial value at
//! birth to the final one at the end of its life.

use glam::{DVec3, Mat4, Vec3};
use omsi_model::{ParticleSystemDef, PsRange, PsValue};
use std::sync::RwLock;

/// Particles an emitter keeps at most (OMSI's 100).
pub const MAX_PER_EMITTER: usize = 100;
/// The frame rate a brake factor is written for.
const FRAME_RATE: f32 = 30.0;

/// Where the camera is: emitters farther than their `calc_dist` send no new particles.
static EYE: RwLock<Option<DVec3>> = RwLock::new(None);

pub fn set_eye(p: DVec3) {
    *EYE.write().unwrap_or_else(|e| e.into_inner()) = Some(p);
}

fn eye() -> Option<DVec3> {
    *EYE.read().unwrap_or_else(|e| e.into_inner())
}

#[derive(Debug, Clone)]
pub struct Particle {
    pub pos: DVec3,
    pub vel: Vec3,
    pub age: f32,
    pub life: f32,
    pub size0: f32,
    pub grow: f32,
    pub alpha0: f32,
    pub alpha1: f32,
    pub color: [f32; 3],
    pub brake: f32,
    pub gravity: f32,
}

impl Particle {
    /// Its width (m): the start size plus the growth over its age.
    pub fn size(&self) -> f32 {
        (self.size0 + self.grow * self.age).max(0.0)
    }

    pub fn alpha(&self) -> f32 {
        let t = (self.age / self.life.max(1e-3)).clamp(0.0, 1.0);
        (self.alpha0 + (self.alpha1 - self.alpha0) * t).clamp(0.0, 1.0)
    }
}

#[derive(Debug, Clone)]
pub struct Emitter {
    pub def: ParticleSystemDef,
    pub particles: Vec<Particle>,
    /// Particles owed from fractions of frames.
    carry: f32,
    /// The burst of a free emitter has gone off.
    burst_done: bool,
    /// Where and how fast its particles were going when they ended this frame (for an
    /// emitter attached in burst mode).
    ended: Vec<(DVec3, Vec3)>,
}

/// The particle systems of one vehicle part or scenery object.
#[derive(Debug, Clone, Default)]
pub struct ParticleSet {
    pub emitters: Vec<Emitter>,
    rng: u64,
}

fn eval(v: &PsValue, value: &dyn Fn(&str) -> f32) -> f32 {
    match v {
        PsValue::Const(x) => *x,
        PsValue::Var(n) => value(n),
    }
}

impl ParticleSet {
    pub fn new(defs: Vec<ParticleSystemDef>, seed: u64) -> ParticleSet {
        ParticleSet {
            emitters: defs
                .into_iter()
                .map(|def| Emitter {
                    def,
                    particles: Vec::new(),
                    carry: 0.0,
                    burst_done: false,
                    ended: Vec::new(),
                })
                .collect(),
            rng: seed | 1,
        }
    }

    pub fn is_empty(&self) -> bool {
        self.emitters.is_empty()
    }

    /// Live particles of every emitter, and whether each glows (`--PS_emissive--`) and its
    /// picture.
    pub fn particles(&self) -> impl Iterator<Item = (&Particle, &ParticleSystemDef)> {
        self.emitters
            .iter()
            .flat_map(|e| e.particles.iter().map(move |p| (p, &e.def)))
    }

    /// -1..1
    fn rand(&mut self) -> f32 {
        self.rng ^= self.rng << 13;
        self.rng ^= self.rng >> 7;
        self.rng ^= self.rng << 17;
        ((self.rng >> 40) as f32 / (1u64 << 24) as f32) * 2.0 - 1.0
    }

    fn draw(&mut self, r: &PsRange, value: &dyn Fn(&str) -> f32) -> f32 {
        let (base, spread) = (eval(&r.0, value), eval(&r.1, value));
        base + spread * self.rand()
    }

    /// One frame: age and move the particles and send new ones off. `origin` and `rot` place
    /// the owner (a vehicle's frame: x right, y forward, z up), `value` reads its variables.
    pub fn update(&mut self, dt: f32, origin: DVec3, rot: Mat4, value: &dyn Fn(&str) -> f32) {
        if self.emitters.is_empty() || dt <= 0.0 {
            return;
        }
        let eye = eye();
        for i in 0..self.emitters.len() {
            // move what is there
            let mut ended = Vec::new();
            {
                let e = &mut self.emitters[i];
                e.particles.retain_mut(|p| {
                    p.age += dt;
                    if p.age >= p.life {
                        ended.push((p.pos, p.vel));
                        return false;
                    }
                    p.vel *= p.brake.clamp(0.0, 1.5).powf(dt * FRAME_RATE);
                    p.vel.z -= 9.81 * p.gravity * dt;
                    p.pos += p.vel.as_dvec3() * dt as f64;
                    true
                });
                e.ended = ended;
            }
            let def = self.emitters[i].def.clone();
            let own = origin + rot.transform_vector3(Vec3::from(def.pos)).as_dvec3();
            if let Some(eye) = eye {
                if (own - eye).length() > def.calc_dist.max(50.0) as f64 {
                    continue;
                }
            }
            let dir = rot
                .transform_vector3(Vec3::from(def.dir))
                .normalize_or_zero();
            // where new particles start: the emitter itself, or the particles of the one it
            // is attached to
            let mut sources: Vec<(DVec3, Vec3)> = Vec::new();
            let mut burst_sources: Vec<(DVec3, Vec3)> = Vec::new();
            match def.attach {
                Some((parent, mode)) if parent < i => {
                    let pe = &self.emitters[parent];
                    match mode {
                        1 => burst_sources = pe.ended.clone(),
                        _ => {
                            sources = pe
                                .particles
                                .iter()
                                .map(|p| (p.pos, if mode == 2 { -p.vel } else { dir }))
                                .collect()
                        }
                    }
                }
                Some(_) => {}
                None => {
                    sources.push((own, dir));
                    if !self.emitters[i].burst_done && def.burst.is_some() {
                        burst_sources.push((own, dir));
                        self.emitters[i].burst_done = true;
                    }
                }
            }
            // continuous emission
            let freq = eval(&def.freq.0, value).max(0.0);
            let mut n = 0usize;
            if freq > 0.0 && !sources.is_empty() {
                let e = &mut self.emitters[i];
                e.carry += freq * dt;
                n = e.carry.floor() as usize;
                e.carry -= n as f32;
            }
            let mut spawn: Vec<(DVec3, Vec3)> = Vec::new();
            for _ in 0..n {
                spawn.extend(sources.iter().copied());
            }
            if let Some(b) = &def.burst {
                for s in &burst_sources {
                    let count = self.draw(b, value).round().max(0.0) as usize;
                    spawn.extend(std::iter::repeat(*s).take(count));
                }
            }
            for (at, d) in spawn {
                if self.emitters[i].particles.len() >= MAX_PER_EMITTER {
                    break;
                }
                let p = self.new_particle(&def, at, d.normalize_or_zero(), value);
                self.emitters[i].particles.push(p);
            }
        }
    }

    fn new_particle(
        &mut self,
        def: &ParticleSystemDef,
        at: DVec3,
        dir: Vec3,
        value: &dyn Fn(&str) -> f32,
    ) -> Particle {
        let speed = eval(&def.velocity.0, value);
        let spread = eval(&def.velocity.1, value);
        let vel = if def.velocity_all_round {
            let v = Vec3::new(self.rand(), self.rand(), self.rand()).normalize_or(Vec3::Z);
            v * (speed + spread * self.rand())
        } else {
            dir * speed + Vec3::new(self.rand(), self.rand(), self.rand()) * spread
        };
        let color = [
            self.draw(&def.rgb[0], value).clamp(0.0, 1.0),
            self.draw(&def.rgb[1], value).clamp(0.0, 1.0),
            self.draw(&def.rgb[2], value).clamp(0.0, 1.0),
        ];
        Particle {
            pos: at,
            vel,
            age: 0.0,
            life: self.draw(&def.life, value).max(0.05),
            size0: self.draw(&def.size_start, value),
            grow: self.draw(&def.size_grow, value),
            alpha0: self.draw(&def.alpha_initial, value),
            alpha1: self.draw(&def.alpha_final, value),
            color,
            brake: self.draw(&def.brake, value),
            gravity: self.draw(&def.gravity, value),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn smoke(freq: f32) -> ParticleSystemDef {
        let p: Vec<String> = [
            "0",
            "-5",
            "0.4",
            "0",
            "-1",
            "0",
            "2",
            "0.2",
            &freq.to_string(),
            "2",
            "0.95",
            "-0.2",
            "0.5",
            "3",
            "0.8",
            "0",
            "0.6",
            "0.6",
            "0.6",
        ]
        .iter()
        .map(|s| s.to_string())
        .collect();
        ParticleSystemDef::from_smoke(&p)
    }

    #[test]
    fn exhaust_puffs_rise_grow_and_fade() {
        let mut s = ParticleSet::new(vec![smoke(20.0)], 7);
        for _ in 0..30 {
            s.update(
                1.0 / 30.0,
                DVec3::new(100.0, 200.0, 10.0),
                Mat4::IDENTITY,
                &|_| 0.0,
            );
        }
        let ps: Vec<&Particle> = s.particles().map(|(p, _)| p).collect();
        assert!(
            ps.len() >= 18 && ps.len() <= 21,
            "{} particles after a second at 20/s",
            ps.len()
        );
        let oldest = ps.iter().max_by(|a, b| a.age.total_cmp(&b.age)).unwrap();
        assert!(
            oldest.pos.y < 195.0,
            "blown out backwards: {:?}",
            oldest.pos
        );
        assert!(oldest.size() > 2.5, "grown to {}", oldest.size());
        assert!(oldest.alpha() < 0.5, "faded to {}", oldest.alpha());
        assert!(
            oldest.vel.length() < 2.0,
            "slowed to {}",
            oldest.vel.length()
        );
    }

    #[test]
    fn a_variable_frequency_of_zero_sends_nothing_and_the_cap_holds() {
        let mut p = smoke(0.0);
        p.freq.0 = PsValue::Var("auspuff_freq".into());
        let mut s = ParticleSet::new(vec![p], 3);
        for _ in 0..30 {
            s.update(1.0 / 30.0, DVec3::ZERO, Mat4::IDENTITY, &|n| {
                if n == "auspuff_freq" { 0.0 } else { 1.0 }
            });
        }
        assert_eq!(s.particles().count(), 0);
        for _ in 0..60 {
            s.update(1.0 / 30.0, DVec3::ZERO, Mat4::IDENTITY, &|_| 500.0);
        }
        assert!(s.particles().count() <= MAX_PER_EMITTER);
    }
}
