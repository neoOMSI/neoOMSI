//! Puddle splashes: what a wheel throws up crossing standing water. The puddle itself - the
//! glass-flat, reflective patches on a wet `[moisture]` road, with the raindrop ripples
//! crossing it - is `enhanced.wgsl`'s; this file only works out *where* one sits (the same
//! low-frequency mask, evaluated here so a wheel can be asked whether it stands in one) and
//! spawns the spray as the renderer's smoke particles: soft, lit by the scene, blended - a
//! mist of water that widens and thins out. (Drawn as corona sprites before, the spray was
//! rings of glowing light flying off the wheels.)

use glam::{DVec3, Vec3};
use omsi_render::SmokeParticle;

/// The puddle mask `enhanced.wgsl` paints on a `[moisture]` road, evaluated on the CPU with
/// the same two octaves of value noise so a splash starts exactly where the reflection does.
/// `wet_road` is the moisture-weighted wetness at this point (global wetness where the
/// surface is a `[moisture]` road, 0 elsewhere - a puddle never sits on bare terrain).
pub fn puddle_coverage(x: f64, y: f64, wet_road: f32) -> f32 {
    if wet_road <= 0.0 {
        return 0.0;
    }
    let (wx, wy) = (x as f32, y as f32);
    let pn = vnoise(wx * 0.22 + 17.3, wy * 0.22 - 9.1) * 0.65
        + vnoise(wx * 0.9 - 4.0, wy * 0.9 + 8.0) * 0.35;
    let t = 1.0 - wet_road * 1.15;
    smoothstep(t - 0.06, t + 0.06, pn)
}

fn hash2(x: f32, y: f32) -> f32 {
    let s = (x * 127.1 + y * 311.7).sin() * 43758.5453;
    s - s.floor()
}

fn vnoise(x: f32, y: f32) -> f32 {
    let (ix, iy) = (x.floor(), y.floor());
    let (fx, fy) = (x - ix, y - iy);
    let (ux, uy) = (fx * fx * (3.0 - 2.0 * fx), fy * fy * (3.0 - 2.0 * fy));
    let a = hash2(ix, iy);
    let b = hash2(ix + 1.0, iy);
    let c = hash2(ix, iy + 1.0);
    let d = hash2(ix + 1.0, iy + 1.0);
    let ab = a + (b - a) * ux;
    let cd = c + (d - c) * ux;
    ab + (cd - ab) * uy
}

fn smoothstep(e0: f32, e1: f32, x: f32) -> f32 {
    let t = ((x - e0) / (e1 - e0)).clamp(0.0, 1.0);
    t * t * (3.0 - 2.0 * t)
}

/// The player's wheel contact points this frame (world) and the vehicle's forward speed
/// (m/s, unsigned) - [`Splashes::update`] does not need each wheel's own speed, only
/// whether the bus is moving through what its own wheels stand on.
pub fn wheel_contacts(v: &omsi_sim::VehicleInstance) -> Vec<DVec3> {
    let Some(rb) = v.rigid.as_ref() else {
        return Vec::new();
    };
    let xf = v.world_transform();
    rb.wheels
        .iter()
        .filter(|w| w.on_ground)
        .map(|w| {
            xf.transform_point3(Vec3::new(w.attach.x, w.attach.y, w.attach.z - w.radius))
                .as_dvec3()
        })
        .collect()
}

struct Drop {
    pos: DVec3,
    vel: Vec3,
    age: f32,
    life: f32,
}

/// At most this many splash droplets alive at once (a bus idling on a flooded stop should
/// not slowly fill the frame budget).
const MAX_DROPS: usize = 300;

pub struct Splashes {
    drops: Vec<Drop>,
    /// Fractional droplets owed to each wheel (index matches last frame's `wheel_contacts`),
    /// so the spray is continuous while a wheel stays in the puddle rather than one pop.
    debt: Vec<f32>,
    rng: u64,
    /// Wheels standing in a puddle right now, and how many droplets are alive, for
    /// `OMSI_DEBUG_RAIN`.
    pub wheels_in_puddle: u32,
}

impl Splashes {
    pub fn new() -> Splashes {
        Splashes {
            drops: Vec::new(),
            debt: Vec::new(),
            rng: 0xD00D_F00D_1234_5678,
            wheels_in_puddle: 0,
        }
    }

    fn rand(&mut self) -> f32 {
        self.rng ^= self.rng << 13;
        self.rng ^= self.rng >> 7;
        self.rng ^= self.rng << 17;
        (self.rng >> 40) as f32 / (1u64 << 24) as f32
    }

    /// One frame: `wheels` are this frame's ground contact points, `speed` the vehicle's own
    /// (m/s), `coverage_at` the puddle mask at a world (x, y) - see [`puddle_coverage`]. The
    /// live droplets come back as coronas for the caller to push onto `scene.coronas`,
    /// exactly as `rain::Rain::tick` pushes rain - a plain `Vec` so the spawning and ageing
    /// above stay testable without a real, GPU-backed `Scene`.
    pub fn update(
        &mut self,
        dt: f32,
        wheels: &[DVec3],
        speed: f32,
        coverage_at: &dyn Fn(f64, f64) -> f32,
    ) -> Vec<SmokeParticle> {
        if self.debt.len() != wheels.len() {
            self.debt = vec![0.0; wheels.len()];
        }
        self.wheels_in_puddle = 0;
        for (i, pos) in wheels.iter().enumerate() {
            let cov = coverage_at(pos.x, pos.y);
            if cov <= 0.05 || speed < 0.6 {
                self.debt[i] = 0.0;
                continue;
            }
            self.wheels_in_puddle += 1;
            // a continuous spray while the wheel stays in the puddle, thicker the faster the
            // bus goes and the deeper the puddle's own mask reads
            let rate = (4.0 + 18.0 * (speed / 12.0).min(1.0)) * cov;
            self.debt[i] += rate * dt;
            while self.debt[i] >= 1.0 && self.drops.len() < MAX_DROPS {
                self.debt[i] -= 1.0;
                let a = self.rand() * std::f32::consts::TAU;
                let r = self.rand() * 0.22;
                // a low sheet of spray thrown out to the side, not a fountain
                let up = 0.5 + self.rand() * 0.8 + (speed * 0.03).min(0.8);
                let out = 0.4 + self.rand() * 0.9 + (speed * 0.04).min(0.8);
                let life = 0.5 + self.rand() * 0.4;
                self.drops.push(Drop {
                    pos: *pos + DVec3::new((a.cos() * r) as f64, (a.sin() * r) as f64, 0.04),
                    vel: Vec3::new(a.cos() * out, a.sin() * out, up),
                    age: 0.0,
                    life,
                });
            }
        }
        let mut out = Vec::with_capacity(self.drops.len());
        let mut i = 0;
        while i < self.drops.len() {
            let d = &mut self.drops[i];
            d.age += dt;
            if d.age >= d.life || d.pos.z < -50.0 {
                self.drops.swap_remove(i);
                continue;
            }
            // the mist slows in the air and sinks, widening as it thins
            d.vel *= (1.0 - 2.5 * dt).max(0.0);
            d.vel.z -= 3.0 * dt;
            d.pos += d.vel.as_dvec3() * dt as f64;
            let t = (d.age / d.life).clamp(0.0, 1.0);
            let fade = (1.0 - t) * (t * 6.0).min(1.0);
            out.push(SmokeParticle {
                position: d.pos,
                size: 0.06 + 0.3 * t,
                color: [0.86, 0.89, 0.92],
                alpha: 0.3 * fade,
            });
            i += 1;
        }
        if omsi_cfg::env::var_os("OMSI_DEBUG_RAIN").is_some()
            && (self.wheels_in_puddle > 0 || !self.drops.is_empty())
        {
            log::info!(
                "splashes: {} of {} wheels in a puddle, {} droplets",
                self.wheels_in_puddle,
                wheels.len(),
                self.drops.len()
            );
        }
        out
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn coverage_is_zero_off_the_moisture_mask_and_grows_with_wetness() {
        assert_eq!(
            puddle_coverage(1000.0, 2000.0, 0.0),
            0.0,
            "no [moisture] under the wheel: never a puddle"
        );
        // scan a stretch of road for a spot the mask calls a puddle at full wetness, and
        // check it grows monotonically as the road dries back out from there
        let mut best = None;
        for i in 0..400 {
            let x = i as f64 * 1.3;
            let c = puddle_coverage(x, 500.0, 1.0);
            if c > 0.4 {
                best = Some((x, c));
                break;
            }
        }
        let (x, full) =
            best.expect("some spot along 520 m of road should read as a puddle at full wetness");
        let half = puddle_coverage(x, 500.0, 0.5);
        let dry = puddle_coverage(x, 500.0, 0.05);
        assert!(
            full >= half && half >= dry && full > dry,
            "a puddle should shrink as the road dries: {full} (wet) vs {half} (half) vs {dry} (drier)"
        );
    }

    #[test]
    fn a_road_wet_through_is_all_puddle() {
        for i in 0..500 {
            let (x, y) = (i as f64 * 0.77, i as f64 * 1.9);
            assert!(
                puddle_coverage(x, y, 1.0) > 0.99,
                "dry island at ({x}, {y}) on a soaked road"
            );
        }
    }

    #[test]
    fn a_fast_wheel_in_a_puddle_sprays_continuously() {
        let mut s = Splashes::new();
        let wheels = [DVec3::new(0.0, 0.0, 33.0)];
        let mut seen_drops = false;
        for _ in 0..30 {
            if !s.update(1.0 / 30.0, &wheels, 8.0, &|_, _| 1.0).is_empty() {
                seen_drops = true;
            }
        }
        assert_eq!(s.wheels_in_puddle, 1);
        assert!(
            seen_drops,
            "a wheel sitting in a full puddle at speed should have sprayed something"
        );
        // standing still or off the puddle stops the spray without leaving debt behind
        s.update(1.0 / 30.0, &wheels, 0.0, &|_, _| 1.0);
        assert_eq!(s.wheels_in_puddle, 0);
        assert_eq!(s.debt[0], 0.0);
    }
}
