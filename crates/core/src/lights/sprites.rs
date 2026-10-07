use super::*;

pub fn particle_sprites(
    set: &::simulation::particles::ParticleSet,
    smoke: &mut Vec<::render::SmokeParticle>,
    coronas: &mut Vec<Corona>,
) {
    let classic = crate::startup::CLASSIC.load(std::sync::atomic::Ordering::Relaxed);
    for (p, def) in set.particles() {
        // (Vanilla+ and Enhanced draw smoke smaller, thinning as it spreads, and turned)
        let (alpha, radius, angle, pull) = if classic || def.emissive {
            (p.alpha(), p.size() * 0.5, 0.0, 0.0)
        } else {
            let radius = smoke_diameter(p) * 0.5;
            let (angle, pull) = smoke_turn_and_pull(p, radius);
            (smoke_alpha(p), radius, angle, pull)
        };
        if alpha <= 0.002 {
            continue;
        }
        if def.emissive {
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
        } else {
            smoke.push(::render::SmokeParticle {
                position: p.pos,
                size: radius,
                color: p.color,
                alpha,
                angle,
                pull,
            });
        }
    }
}

/// The share of the `[smoke]` sizes Vanilla+ and Enhanced draw: a real exhaust plume starts
/// at the pipe and widens by about a fifth of its way, several times less than vehicles give.
const SMOKE_SIZE: f32 = 0.3;

/// Vanilla+ and Enhanced smoke: the puff's width (m).
fn smoke_diameter(p: &::simulation::particles::Particle) -> f32 {
    p.size() * SMOKE_SIZE
}

/// Vanilla+ and Enhanced smoke: the vehicle's initial alpha at its start size, spread over
/// the puff's area as it grows; faded in quickly (the plume is densest at the pipe) and out
/// towards the end of its life.
fn smoke_alpha(p: &::simulation::particles::Particle) -> f32 {
    let smooth = |a: f32, b: f32, x: f32| {
        let t = ((x - a) / (b - a)).clamp(0.0, 1.0);
        t * t * (3.0 - 2.0 * t)
    };
    let t = p.age / p.life.max(1e-3);
    let d = smoke_diameter(p).max(0.05);
    let amount = p.alpha0.clamp(0.0, 1.0) * p.size0.max(0.0).powi(2);
    let alpha = (amount / (d * d)).min(1.0);
    alpha * smooth(0.0, 0.06, p.age) * (1.0 - smooth(0.4, 1.0, t))
}

/// Vanilla+ and Enhanced smoke: each puff slowly turning from its own angle (one turn for
/// all shows the picture's pattern), and drawn its radius (at most 2 m) towards the viewer
/// so that the ground does not cut a hard edge through it.
fn smoke_turn_and_pull(p: &::simulation::particles::Particle, radius: f32) -> (f32, f32) {
    let angle = p.seed * std::f32::consts::TAU + (p.seed - 0.5) * 0.8 * p.age;
    (angle, radius.min(2.0))
}

pub fn load_smoke_texture(renderer: &mut ::render::Renderer, root: &std::path::Path) {
    let path = ::legacy_config::resolve_path(root, "Texture/rauch.tga");
    match ::texture::decode_file(&path) {
        Ok(img) => renderer.set_smoke_texture(&img),
        Err(e) => log::warn!("smoke texture {}: {e}", path.display()),
    }
}

pub(super) struct CoronaTextures {
    pub(super) ids: std::collections::HashMap<std::path::PathBuf, u16>,
    pub(super) pending: Vec<(u16, std::path::PathBuf)>,
    pub(super) root: Option<std::path::PathBuf>,
}

pub(super) static CORONA_TEXTURES: std::sync::Mutex<Option<CoronaTextures>> =
    std::sync::Mutex::new(None);

pub fn set_corona_root(root: &std::path::Path) {
    let mut g = CORONA_TEXTURES.lock().unwrap_or_else(|e| e.into_inner());
    let t = g.get_or_insert_with(|| CoronaTextures {
        ids: Default::default(),
        pending: Vec::new(),
        root: None,
    });
    t.root = Some(root.to_path_buf());
}

pub(super) fn texture_id_of(path: std::path::PathBuf) -> u16 {
    let mut g = CORONA_TEXTURES.lock().unwrap_or_else(|e| e.into_inner());
    let t = g.get_or_insert_with(|| CoronaTextures {
        ids: Default::default(),
        pending: Vec::new(),
        root: None,
    });
    if let Some(id) = t.ids.get(&path) {
        return *id;
    }
    let id = (t.ids.len() + 1).min(u16::MAX as usize) as u16;
    t.ids.insert(path.clone(), id);
    t.pending.push((id, path));
    id
}

pub fn corona_texture_id(model_dir: &std::path::Path, name: &str) -> u16 {
    static KNOWN: std::sync::Mutex<
        Option<std::collections::HashMap<(std::path::PathBuf, String), u16>>,
    > = std::sync::Mutex::new(None);
    let key = (model_dir.to_path_buf(), name.to_string());
    if let Some(&id) = KNOWN
        .lock()
        .unwrap_or_else(|e| e.into_inner())
        .as_ref()
        .and_then(|m| m.get(&key))
    {
        return id;
    }
    let root = CORONA_TEXTURES
        .lock()
        .unwrap_or_else(|e| e.into_inner())
        .as_ref()
        .and_then(|t| t.root.clone())
        .unwrap_or_default();
    let mut id = 0;
    for d in crate::scene::texture_dirs(&root, model_dir) {
        let p = ::legacy_config::resolve_path(&d, name);
        if ::legacy_config::vfs::is_file(&p) {
            id = texture_id_of(p);
            break;
        }
    }
    KNOWN
        .lock()
        .unwrap_or_else(|e| e.into_inner())
        .get_or_insert_with(Default::default)
        .insert(key, id);
    id
}

pub(super) fn stock_texture_id(name: &str) -> u16 {
    static KNOWN: std::sync::Mutex<
        Option<std::collections::HashMap<(std::path::PathBuf, String), u16>>,
    > = std::sync::Mutex::new(None);
    let root = CORONA_TEXTURES
        .lock()
        .unwrap_or_else(|e| e.into_inner())
        .as_ref()
        .and_then(|t| t.root.clone())
        .unwrap_or_default();
    let key = (root, name.to_string());
    if let Some(&id) = KNOWN
        .lock()
        .unwrap_or_else(|e| e.into_inner())
        .as_ref()
        .and_then(|m| m.get(&key))
    {
        return id;
    }
    let p = ::legacy_config::resolve_path(&key.0, &format!("Texture/{name}"));
    let id = if ::legacy_config::vfs::is_file(&p) {
        texture_id_of(p)
    } else {
        0
    };
    KNOWN
        .lock()
        .unwrap_or_else(|e| e.into_inner())
        .get_or_insert_with(Default::default)
        .insert(key, id);
    id
}

pub fn cone_texture_id() -> u16 {
    stock_texture_id("light_cone.bmp")
}

pub fn glow_texture_id() -> u16 {
    stock_texture_id("licht.bmp")
}

pub fn star_texture_id() -> u16 {
    stock_texture_id("light_effect1.bmp")
}

pub fn upload_corona_textures(renderer: &mut ::render::Renderer) {
    let pending = {
        let mut g = CORONA_TEXTURES.lock().unwrap_or_else(|e| e.into_inner());
        match g.as_mut() {
            Some(t) => std::mem::take(&mut t.pending),
            None => return,
        }
    };
    for (id, path) in pending {
        match ::texture::decode_file(&path) {
            Ok(img) => renderer.set_corona_texture(id, &img),
            Err(e) => log::warn!("corona picture {}: {e}", path.display()),
        }
    }
}

pub(super) static CONE: std::sync::atomic::AtomicU32 = std::sync::atomic::AtomicU32::new(0);

pub(super) static CONE_NIGHT: std::sync::atomic::AtomicU32 = std::sync::atomic::AtomicU32::new(0);

pub fn set_cone_strength(fog_visibility_m: f32, _precip: f32, night: f32) {
    CONE.store(
        fog_visibility_m.to_bits(),
        std::sync::atomic::Ordering::Relaxed,
    );
    CONE_NIGHT.store(
        night.clamp(0.0, 1.0).to_bits(),
        std::sync::atomic::Ordering::Relaxed,
    );
}

pub(super) fn cone_weather() -> (f32, f32) {
    let vis = f32::from_bits(CONE.load(std::sync::atomic::Ordering::Relaxed));
    let night = f32::from_bits(CONE_NIGHT.load(std::sync::atomic::Ordering::Relaxed));
    (if vis > 0.0 { vis } else { 1.0e6 }, night)
}

/// How far lights, coronas and particles are worth making: the loaded area, and no further
/// than the weather's fog lets anything be seen (it swallows 99 % at twice the visibility).
pub(super) fn visible_range() -> f64 {
    let (vis, _) = cone_weather();
    CORONA_RANGE.min((vis as f64 * 2.0).max(60.0))
}

pub fn vehicle_velocity(v: &::simulation::VehicleInstance) -> glam::Vec3 {
    let h = v.heading.to_radians();
    glam::Vec3::new(h.sin() as f32, h.cos() as f32, 0.0) * v.physics.speed
}

#[cfg(test)]
mod tests {
    use super::*;
    use ::simulation::particles::Particle;

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

    /// Vanilla+ smoke starts and ends invisible.
    #[test]
    fn smoke_fades_in_and_out() {
        assert_eq!(smoke_alpha(&puff(0.0, 0.5)), 0.0);
        assert!(smoke_alpha(&puff(1.999, 0.5)) < 1e-3);
        assert!(smoke_alpha(&puff(0.3, 0.5)) > 0.1);
    }

    /// A Vanilla+ puff keeps its smoke while it spreads: alpha times area stays the same.
    #[test]
    fn smoke_spreads_its_amount() {
        let (young, old) = (puff(0.3, 0.5), puff(0.6, 0.5));
        assert_eq!(smoke_diameter(&young), young.size() * SMOKE_SIZE);
        let amount = |p: &Particle| smoke_alpha(p) * smoke_diameter(p).powi(2);
        assert!((amount(&young) - amount(&old)).abs() < 1e-4);
        assert!(smoke_alpha(&old) < smoke_alpha(&young));
    }

    /// Puffs are turned by their seed and pulled towards the viewer by their radius (2 m at most).
    #[test]
    fn smoke_turns_by_seed_and_pulls_by_radius() {
        let (a, pull) = smoke_turn_and_pull(&puff(0.0, 0.25), 0.7);
        assert!((a - std::f32::consts::FRAC_PI_2).abs() < 1e-5);
        assert_eq!(pull, 0.7);
        assert_eq!(smoke_turn_and_pull(&puff(0.0, 0.25), 5.0).1, 2.0);
        assert_ne!(
            smoke_turn_and_pull(&puff(1.0, 0.1), 1.0).0,
            smoke_turn_and_pull(&puff(1.0, 0.6), 1.0).0
        );
    }
}
