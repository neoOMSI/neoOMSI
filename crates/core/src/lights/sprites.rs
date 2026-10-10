use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::Mutex;

#[derive(Default)]
struct Registry {
    root: Option<PathBuf>,
    ids: HashMap<PathBuf, u16>,
    pending: Vec<(u16, PathBuf)>,
    named: HashMap<(PathBuf, String), u16>,
}

static REGISTRY: Mutex<Option<Registry>> = Mutex::new(None);

fn with<R>(f: impl FnOnce(&mut Registry) -> R) -> R {
    let mut g = REGISTRY.lock().unwrap_or_else(|e| e.into_inner());
    f(g.get_or_insert_with(Registry::default))
}

impl Registry {
    fn id_of(&mut self, path: PathBuf) -> u16 {
        if let Some(id) = self.ids.get(&path) {
            return *id;
        }
        let id = (self.ids.len() + 1).min(u16::MAX as usize) as u16;
        self.ids.insert(path.clone(), id);
        self.pending.push((id, path));
        id
    }
}

pub fn set_corona_root(root: &Path) {
    with(|r| r.root = Some(root.to_path_buf()));
}

pub fn corona_texture_id(model_dir: &Path, name: &str) -> u16 {
    let key = (model_dir.to_path_buf(), name.to_string());
    let (known, root) = with(|r| (r.named.get(&key).copied(), r.root.clone().unwrap_or_default()));
    if let Some(id) = known {
        return id;
    }
    let found = crate::scene::texture_dirs(&root, model_dir)
        .into_iter()
        .map(|d| ::legacy_config::resolve_path(&d, name))
        .find(|p| ::legacy_config::vfs::is_file(p));
    with(|r| {
        let id = found.map_or(0, |p| r.id_of(p));
        r.named.insert(key, id);
        id
    })
}

fn stock_texture_id(name: &str) -> u16 {
    let root = with(|r| r.root.clone().unwrap_or_default());
    let key = (root, name.to_string());
    if let Some(id) = with(|r| r.named.get(&key).copied()) {
        return id;
    }
    let path = ::legacy_config::resolve_path(&key.0, &format!("Texture/{name}"));
    let exists = ::legacy_config::vfs::is_file(&path);
    with(|r| {
        let id = if exists { r.id_of(path) } else { 0 };
        r.named.insert(key, id);
        id
    })
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
    let pending = with(|r| std::mem::take(&mut r.pending));
    for (id, path) in pending {
        match ::texture::decode_file(&path) {
            Ok(img) => renderer.set_corona_texture(id, &img),
            Err(e) => log::warn!("corona picture {}: {e}", path.display()),
        }
    }
}

pub fn load_smoke_texture(renderer: &mut ::render::Renderer, root: &Path) {
    let path = ::legacy_config::resolve_path(root, "Texture/rauch.tga");
    match ::texture::decode_file(&path) {
        Ok(img) => renderer.set_smoke_texture(&img),
        Err(e) => log::warn!("smoke texture {}: {e}", path.display()),
    }
}
