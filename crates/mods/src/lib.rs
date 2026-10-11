use legacy_config::vfs;
use std::path::{Path, PathBuf};
use std::sync::{Mutex, RwLock};

mod layout;
mod resolver;
pub use layout::{Layer, ModEntry, ModKind, SHARED_DIR, discover, ensure_layout};
pub use resolver::*;

struct State {
    mods_dir: PathBuf,
    legacy: Option<PathBuf>,
    mods: Vec<ModEntry>,
}

static STATE: RwLock<Option<State>> = RwLock::new(None);
static REGISTERED: Mutex<Vec<PathBuf>> = Mutex::new(Vec::new());

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Resolved {
    pub path: PathBuf,
    pub layer: Layer,
}

pub fn init(mods_dir: &Path, legacy: Option<&Path>) -> std::io::Result<()> {
    ensure_layout(mods_dir)?;
    let shared = mods_dir.join(SHARED_DIR);
    let legacy = legacy.map(Path::to_path_buf);
    let mods = discover(mods_dir);

    // The launcher calls this from its regular content poll. Re-registering an unchanged
    // layout remounts every mod and turns startup diagnostics into continuous log noise.
    if STATE.read().unwrap().as_ref().is_some_and(|state| {
        state.mods_dir == mods_dir && state.legacy == legacy && state.mods == mods
    }) {
        return Ok(());
    }

    // forget the previous registration
    for r in REGISTERED.lock().unwrap().drain(..) {
        clear_root_scope(&r);
        remove_content_root(&r);
    }

    let mut registered = Vec::new();
    let mut register = |root: PathBuf, fallbacks: Vec<PathBuf>| {
        match &legacy {
            Some(l) => add_content_root_before(root.clone(), l),
            None => add_content_root(root.clone()),
        }
        set_root_scope(root.clone(), fallbacks);
        registered.push(root);
    };

    let mut tail = vec![shared.clone()];
    tail.extend(legacy.clone());

    for m in &mods {
        let root = match m.kind {
            ModKind::Folder => m.path.clone(),
            ModKind::Zip => match vfs::mount_zip(&m.path) {
                Ok(mount) => mount,
                Err(e) => {
                    log::warn!("mod {}: {e}", m.path.display());
                    continue;
                }
            },
        };
        register(root, tail.clone());
        log::info!("mod '{}' ({:?}): {}", m.name, m.kind, m.path.display());
    }
    register(shared.clone(), legacy.iter().cloned().collect());
    if let Some(l) = &legacy {
        // the legacy root ends its own chain: a stock file never picks up a mod's copy
        add_content_root(l.clone());
        set_root_scope(l.clone(), Vec::new());
    }
    *REGISTERED.lock().unwrap() = registered;
    *STATE.write().unwrap() = Some(State {
        mods_dir: mods_dir.to_path_buf(),
        legacy,
        mods,
    });
    Ok(())
}

pub fn mods_dir() -> Option<PathBuf> {
    STATE.read().unwrap().as_ref().map(|s| s.mods_dir.clone())
}

pub fn shared_dir() -> Option<PathBuf> {
    mods_dir().map(|d| d.join(SHARED_DIR))
}

pub fn legacy_dir() -> Option<PathBuf> {
    STATE.read().unwrap().as_ref().and_then(|s| s.legacy.clone())
}

pub fn list() -> Vec<ModEntry> {
    STATE
        .read()
        .unwrap()
        .as_ref()
        .map(|s| s.mods.clone())
        .unwrap_or_default()
}

pub fn layer_of(path: &Path) -> Option<Layer> {
    let st = STATE.read().unwrap();
    let st = st.as_ref()?;
    if path.starts_with(st.mods_dir.join(SHARED_DIR)) {
        return Some(Layer::Shared);
    }
    if st.mods.iter().any(|m| path.starts_with(&m.path)) {
        return Some(Layer::Mod);
    }
    if st.legacy.as_ref().is_some_and(|l| path.starts_with(l)) {
        return Some(Layer::Legacy);
    }
    None
}

pub fn resolve(base: &Path, rel: &str) -> PathBuf {
    resolve_path(base, rel)
}

pub fn resolve_layered(base: &Path, rel: &str) -> Option<Resolved> {
    let path = resolve(base, rel);
    if !vfs::exists(&path) {
        return None;
    }
    let layer = layer_of(&path).unwrap_or(Layer::Legacy);
    Some(Resolved { path, layer })
}

pub fn find(mod_name: Option<&str>, rel: &str) -> Option<Resolved> {
    let (mods_dir, legacy, mods) = {
        let st = STATE.read().unwrap();
        let st = st.as_ref()?;
        (st.mods_dir.clone(), st.legacy.clone(), st.mods.clone())
    };
    let mut chain: Vec<(PathBuf, Layer)> = Vec::new();
    if let Some(name) = mod_name {
        let m = mods.iter().find(|m| m.name.eq_ignore_ascii_case(name))?;
        let root = match m.kind {
            ModKind::Folder => m.path.clone(),
            ModKind::Zip => m.path.clone(), // the mount point is the archive path
        };
        chain.push((root, Layer::Mod));
    }
    chain.push((mods_dir.join(SHARED_DIR), Layer::Shared));
    if let Some(l) = legacy {
        chain.push((l, Layer::Legacy));
    }
    let comps = legacy_config::windows_components(rel);
    chain.into_iter().find_map(|(root, layer)| {
        let mut cur = root;
        for c in &comps {
            let name = list_find(&cur, c)?;
            cur = cur.join(name);
        }
        Some(Resolved { path: cur, layer })
    })
}

fn list_find(dir: &Path, name: &str) -> Option<std::ffi::OsString> {
    let key = name.to_lowercase();
    vfs::list_dir(dir)?
        .into_iter()
        .find(|(n, _)| n.to_string_lossy().to_lowercase() == key)
        .map(|(n, _)| n)
}

pub fn read(path: &Path) -> std::io::Result<Vec<u8>> {
    vfs::read(path)
}

pub fn exists(path: &Path) -> bool {
    vfs::exists(path)
}
