use glam::Vec3;
use ::render::{Corona, SmokeParticle};
use ::simulation::particles::{Particle, ParticleSet};

const PLUME_SIZE: f32 = 0.3;
const PLUME_FADE_IN: f32 = 0.06;
const PLUME_PULL_MAX: f32 = 0.3;

fn smoothstep(a: f32, b: f32, x: f32) -> f32 {
    let t = ((x - a) / (b - a)).clamp(0.0, 1.0);
    t * t * (3.0 - 2.0 * t)
}

fn is_plume(p: &Particle, def: &::model::ParticleSystemDef, vehicle: bool, classic: bool) -> bool {
    vehicle && !classic && !def.emissive && p.gravity < 0.0
}

fn plume_diameter(p: &Particle) -> f32 {
    p.size() * PLUME_SIZE
}

fn plume_alpha(p: &Particle) -> f32 {
    let age01 = p.age / p.life.max(1e-3);
    let d = plume_diameter(p).max(0.05);
    let start = p.size0.max(p.grow * PLUME_FADE_IN).max(0.0);
    let depth = -(1.0 - p.alpha().min(0.99)).ln() * start * start / (d * d);
    (1.0 - (-depth).exp()) * smoothstep(0.0, PLUME_FADE_IN, p.age) * (1.0 - smoothstep(0.4, 1.0, age01))
}

fn plume_turn_and_pull(p: &Particle, radius: f32) -> (f32, f32) {
    let turn = p.seed * std::f32::consts::TAU + (p.seed - 0.5) * 0.8 * p.age;
    (turn, (radius * 0.5).min(PLUME_PULL_MAX))
}

pub fn particle_sprites(
    set: &ParticleSet,
    vehicle: bool,
    smoke: &mut Vec<SmokeParticle>,
    coronas: &mut Vec<Corona>,
) {
    let classic = crate::startup::CLASSIC.load(std::sync::atomic::Ordering::Relaxed);
    for (p, def) in set.particles() {
        if def.emissive {
            let alpha = p.alpha();
            if alpha > 0.002 {
                coronas.push(Corona {
                    position: p.pos,
                    size: (p.size() * 0.5).max(0.02),
                    color: p.color,
                    brightness: alpha,
                    direction: Vec3::ZERO,
                    cone_cos: -1.0,
                    z_offset: 0.0,
                    ..Default::default()
                });
            }
            continue;
        }
        let puff = if is_plume(p, def, vehicle, classic) {
            let radius = plume_diameter(p) * 0.5;
            let (angle, pull) = plume_turn_and_pull(p, radius);
            SmokeParticle {
                position: p.pos,
                size: radius,
                color: p.color,
                alpha: plume_alpha(p),
                angle,
                pull,
            }
        } else {
            SmokeParticle {
                position: p.pos,
                size: p.size() * 0.5,
                color: p.color,
                alpha: p.alpha(),
                angle: 0.0,
                pull: 0.0,
            }
        };
        if puff.alpha > 0.002 {
            smoke.push(puff);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn puff(age: f32, seed: f32) -> Particle {
        Particle {
            pos: glam::DVec3::ZERO,
            vel: Vec3::ZERO,
            age,
            life: 2.0,
            size0: 0.5,
            grow: 3.0,
            alpha0: 0.2,
            alpha1: 10.0,
            color: [0.66, 0.66, 0.8],
            brake: 0.95,
            gravity: -0.2,
            seed,
        }
    }

    fn with_alpha(mut p: Particle, a0: f32, a1: f32) -> Particle {
        p.alpha0 = a0;
        p.alpha1 = a1;
        p
    }

    #[test]
    fn plume_starts_and_ends_invisible() {
        assert_eq!(plume_alpha(&puff(0.0, 0.5)), 0.0);
        assert!(plume_alpha(&puff(1.999, 0.5)) < 1e-3);
        assert!(plume_alpha(&puff(0.3, 0.5)) > 0.1);
    }

    #[test]
    fn plume_keeps_its_smoke_while_spreading() {
        let young = with_alpha(puff(0.3, 0.5), 0.2, 0.2);
        let old = with_alpha(puff(0.6, 0.5), 0.2, 0.2);
        let amount = |p: &Particle| -(1.0 - plume_alpha(p)).ln() * plume_diameter(p).powi(2);
        assert!((amount(&young) - amount(&old)).abs() < 1e-4);
        assert!(plume_alpha(&old) < plume_alpha(&young));
    }

    #[test]
    fn thicker_smoke_stays_thicker() {
        let thin = plume_alpha(&with_alpha(puff(0.1, 0.5), 0.2, 0.2));
        let thick = plume_alpha(&with_alpha(puff(0.1, 0.5), 0.4, 0.4));
        assert!(thin < thick && thick < 1.0);
        assert!(plume_alpha(&with_alpha(puff(1.0, 0.5), 0.0, 1.0)) > 0.1);
    }

    #[test]
    fn only_rising_vehicle_smoke_is_a_plume() {
        let def = ::model::ParticleSystemDef::default();
        let mut spray = puff(0.5, 0.5);
        spray.gravity = 1.0;
        assert!(is_plume(&puff(0.5, 0.5), &def, true, false));
        assert!(!is_plume(&spray, &def, true, false));
        assert!(!is_plume(&puff(0.5, 0.5), &def, false, false));
        assert!(!is_plume(&puff(0.5, 0.5), &def, true, true));
    }

    #[test]
    fn pull_is_half_the_radius_up_to_30_cm() {
        let (a, pull) = plume_turn_and_pull(&puff(0.0, 0.25), 0.4);
        assert!((a - std::f32::consts::FRAC_PI_2).abs() < 1e-5);
        assert_eq!(pull, 0.2);
        assert_eq!(plume_turn_and_pull(&puff(0.0, 0.25), 5.0).1, 0.3);
    }
}
