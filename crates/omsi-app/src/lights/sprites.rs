use super::*;

pub fn particle_sprites(
    set: &omsi_sim::particles::ParticleSet,
    smoke: &mut Vec<omsi_render::SmokeParticle>,
    coronas: &mut Vec<Corona>,
) {
    for (p, def) in set.particles() {
        let alpha = p.alpha();
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
            smoke.push(omsi_render::SmokeParticle {
                position: p.pos,
                size: p.size() * 0.5,
                color: p.color,
                alpha,
            });
        }
    }
}

pub fn load_smoke_texture(renderer: &mut omsi_render::Renderer, root: &std::path::Path) {
    let path = omsi_cfg::resolve_path(root, "Texture/rauch.tga");
    match omsi_texture::decode_file(&path) {
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
        let p = omsi_cfg::resolve_path(&d, name);
        if omsi_cfg::vfs::is_file(&p) {
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
    let p = omsi_cfg::resolve_path(&key.0, &format!("Texture/{name}"));
    let id = if omsi_cfg::vfs::is_file(&p) {
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

pub fn upload_corona_textures(renderer: &mut omsi_render::Renderer) {
    let pending = {
        let mut g = CORONA_TEXTURES.lock().unwrap_or_else(|e| e.into_inner());
        match g.as_mut() {
            Some(t) => std::mem::take(&mut t.pending),
            None => return,
        }
    };
    for (id, path) in pending {
        match omsi_texture::decode_file(&path) {
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

pub fn vehicle_velocity(v: &omsi_sim::VehicleInstance) -> glam::Vec3 {
    let h = v.heading.to_radians();
    glam::Vec3::new(h.sin() as f32, h.cos() as f32, 0.0) * v.physics.speed
}
