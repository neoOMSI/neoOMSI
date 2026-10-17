use legacy_config::vfs;
use std::ffi::OsString;
use std::path::{Path, PathBuf};
use std::sync::{Mutex, RwLock};
use std::time::SystemTime;

mod conflicts;
pub mod diagnostics;
mod layout;
pub mod order;
mod resolver;
pub use conflicts::{Clash, Conflict, conflicts};
pub use layout::{
    ARCHIVES_DIR, MODS_DIR, ModEntry, ModKind, SHARED_DIR, UNINSTALLED_DIR, discover,
    folder_content_root, is_reserved,
};
pub use order::{NewMods, OrderFile};
pub use resolver::*;

struct State {
    content: PathBuf,
    legacy: Option<PathBuf>,
    mods: Vec<ModEntry>,
    stamps: Vec<Stamp>,
    /// The content folder's own packs, which the resolver's pack index lists too.
    content_packs: Vec<(OsString, OsString)>,
    failed: Vec<(PathBuf, String)>,
    roots: Vec<(ModEntry, PathBuf)>,
}

static STATE: RwLock<Option<State>> = RwLock::new(None);
static REGISTERED: Mutex<Vec<PathBuf>> = Mutex::new(Vec::new());

/// What an entry is mounted from; when it changes, the entry is mounted again.
#[derive(Debug, Clone, PartialEq)]
enum Stamp {
    /// A folder's content root, which moves when a mod copied in by hand gains a wrapper folder,
    /// and its packs, so that a pack added later reaches the resolver's pack index.
    Root(PathBuf, Vec<(OsString, OsString)>),
    /// A .zip's size and modification time.
    File(Option<(u64, SystemTime)>),
}

fn stamp(entry: &ModEntry) -> Stamp {
    match entry.kind {
        ModKind::Folder => {
            let root = folder_content_root(&entry.path);
            let packs = resolver::pack_dirs(&root);
            Stamp::Root(root, packs)
        }
        ModKind::Zip => Stamp::File(
            std::fs::metadata(&entry.path)
                .ok()
                .filter(|meta| meta.is_file())
                .and_then(|meta| Some((meta.len(), meta.modified().ok()?))),
        ),
    }
}

fn is_current(
    state: &State,
    content: &Path,
    legacy: &Option<PathBuf>,
    mods: &[ModEntry],
    stamps: &[Stamp],
    content_packs: &[(OsString, OsString)],
) -> bool {
    state.content == content
        && state.legacy == *legacy
        && state.mods == mods
        && state.stamps == stamps
        && state.content_packs == content_packs
}

fn archives(content: &Path) -> Vec<ModEntry> {
    let mut out: Vec<ModEntry> = std::fs::read_dir(content.join(ARCHIVES_DIR))
        .into_iter()
        .flatten()
        .flatten()
        .map(|dir_entry| dir_entry.path())
        .filter(|path| {
            path.is_file() && path.extension().is_some_and(|ext| ext.eq_ignore_ascii_case("zip"))
        })
        .map(|path| {
            let file = path.file_name().unwrap_or_default().to_string_lossy().into_owned();
            ModEntry {
                id: format!("{ARCHIVES_DIR}/{file}"),
                name: path.file_stem().unwrap_or_default().to_string_lossy().into_owned(),
                path,
                kind: ModKind::Zip,
            }
        })
        .collect();
    out.sort_by(|left, right| left.path.cmp(&right.path));
    out
}

/// Every entry the player can order, in the default order.
pub fn entries(content: &Path) -> Vec<ModEntry> {
    let mut out = archives(content);
    out.extend(discover(&content.join(MODS_DIR)));
    out
}

pub struct LoadOrder {
    /// Highest priority first.
    pub items: Vec<order::Item>,
    pub entries: Vec<ModEntry>,
    pub saved: Option<OrderFile>,
    pub error: Option<String>,
}

impl LoadOrder {
    pub fn entry(&self, id: &str) -> Option<&ModEntry> {
        self.entries.iter().find(|entry| entry.id == id)
    }

    pub fn active(&self) -> Vec<ModEntry> {
        self.items
            .iter()
            .filter(|item| item.available)
            .filter_map(|item| self.entry(&item.name).cloned())
            .collect()
    }
}

pub fn load_order(content: &Path) -> LoadOrder {
    let entries = entries(content);
    let (saved, error) = match order::load(&content.join(MODS_DIR)) {
        Ok(saved) => (saved, None),
        Err(error) => (None, Some(error)),
    };
    let ids: Vec<String> = entries.iter().map(|entry| entry.id.clone()).collect();
    let items = order::effective(&ids, saved.as_ref());
    LoadOrder { items, entries, saved, error }
}

/// Registers the mods of `content` in load order, ahead of the OMSI folder `legacy`.
pub fn init(content: &Path, legacy: Option<&Path>) -> std::io::Result<()> {
    // The launcher's requests and its content poll can call this at the same time.
    static ONE: Mutex<()> = Mutex::new(());
    let _one = ONE.lock().unwrap_or_else(|poisoned| poisoned.into_inner());
    let shared = content.join(MODS_DIR).join(SHARED_DIR);
    std::fs::create_dir_all(&shared)?;
    let legacy = legacy.map(Path::to_path_buf);
    let order = load_order(content);
    // The shared library goes last: every mod overrides it, and it overrides OMSI 2.
    let mut all = order.active();
    all.push(ModEntry {
        id: SHARED_DIR.into(),
        name: SHARED_DIR.into(),
        path: shared,
        kind: ModKind::Folder,
    });
    let stamps: Vec<Stamp> = all.iter().map(stamp).collect();
    let content_packs = resolver::pack_dirs(content);

    // Registering again clears every cache, so skip it when nothing changed.
    if STATE
        .read()
        .unwrap()
        .as_ref()
        .is_some_and(|state| is_current(state, content, &legacy, &all, &stamps, &content_packs))
    {
        return Ok(());
    }
    if let Some(error) = &order.error {
        log::warn!("load order of {}: {error} - the default order is used", content.display());
    }

    for root in REGISTERED.lock().unwrap().drain(..) {
        remove_content_root(&root);
        if !all.iter().any(|entry| entry.path == root) {
            vfs::unmount_zip(&root);
        }
    }

    let mut registered = Vec::new();
    let mut roots = Vec::new();
    let mut failed = Vec::new();
    for (entry, stamp) in all.iter().zip(&stamps) {
        let root = match stamp {
            Stamp::Root(root, _) => root.clone(),
            Stamp::File(_) => match vfs::mount_zip(&entry.path) {
                Ok(mount) => mount,
                Err(error) => {
                    log::warn!("mod {}: {error}", entry.path.display());
                    failed.push((entry.path.clone(), error.to_string()));
                    continue;
                }
            },
        };
        match &legacy {
            Some(legacy) => add_content_root_before(root.clone(), legacy),
            None => add_content_root(root.clone()),
        }
        log::info!("mod '{}' ({:?}): {}", entry.id, entry.kind, entry.path.display());
        registered.push(root.clone());
        roots.push((entry.clone(), root));
    }
    if let Some(legacy) = &legacy {
        add_content_root(legacy.clone());
    }
    *REGISTERED.lock().unwrap() = registered;
    *STATE.write().unwrap() = Some(State {
        content: content.to_path_buf(),
        legacy,
        mods: all,
        stamps,
        content_packs,
        failed,
        roots,
    });
    Ok(())
}

pub fn mount_error(path: &Path) -> Option<String> {
    let state = STATE.read().unwrap();
    let (_, error) = state.as_ref()?.failed.iter().find(|(failed, _)| failed == path)?;
    Some(error.clone())
}

pub fn mods_dir() -> Option<PathBuf> {
    STATE.read().unwrap().as_ref().map(|state| state.content.join(MODS_DIR))
}

/// Writes `value` as JSON to a temporary file first, so a reader never sees half a file.
pub fn write_json(path: &Path, value: &impl serde::Serialize) -> std::io::Result<()> {
    if let Some(dir) = path.parent() {
        std::fs::create_dir_all(dir)?;
    }
    let mut tmp = path.as_os_str().to_owned();
    tmp.push(".tmp");
    std::fs::write(&tmp, serde_json::to_vec_pretty(value)?)?;
    std::fs::rename(&tmp, path)
}

/// The content roots are global, so tests that change them must take turns.
#[cfg(test)]
pub(crate) fn roots_guard() -> std::sync::MutexGuard<'static, ()> {
    static LOCK: Mutex<()> = Mutex::new(());
    LOCK.lock().unwrap_or_else(|poisoned| poisoned.into_inner())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_layout_is_registered_anew_only_when_it_changed() {
        let content = PathBuf::from("content");
        let mods = vec![ModEntry {
            id: "Bus.zip".into(),
            name: "Bus".into(),
            path: content.join("mods/Bus.zip"),
            kind: ModKind::Zip,
        }];
        let then = Stamp::File(Some((10, SystemTime::UNIX_EPOCH)));
        let state = State {
            content: content.clone(),
            legacy: None,
            mods: mods.clone(),
            stamps: vec![then.clone()],
            content_packs: Vec::new(),
            failed: Vec::new(),
            roots: Vec::new(),
        };
        assert!(is_current(
            &state,
            &content,
            &None,
            &mods,
            std::slice::from_ref(&then),
            &[]
        ));
        // The .zip was replaced, so it has to be mounted again.
        let now = Stamp::File(Some((11, SystemTime::UNIX_EPOCH)));
        assert!(!is_current(&state, &content, &None, &mods, &[now], &[]));
        assert!(!is_current(&state, &content, &None, &[], &[], &[]));
        // A pack was added to the content folder itself.
        let pack = [(OsString::from("Vehicles"), OsString::from("Bus"))];
        assert!(!is_current(&state, &content, &None, &mods, &[then], &pack));
    }

    #[test]
    fn a_folder_mod_is_mounted_anew_when_its_content_folder_moves() {
        let _roots = roots_guard();
        let content = std::env::temp_dir().join(format!("mods-wrapper-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&content);
        std::fs::create_dir_all(content.join("mods/Bus")).unwrap();
        init(&content, None).unwrap();
        assert!(content_roots().contains(&content.join("mods/Bus")));
        // The rest of a mod being copied in by hand arrives after the first poll.
        std::fs::create_dir_all(content.join("mods/Bus/OMSI 2/Vehicles/Bus")).unwrap();
        init(&content, None).unwrap();
        let roots = content_roots();
        assert!(roots.contains(&content.join("mods/Bus/OMSI 2")), "{roots:?}");
        assert!(!roots.contains(&content.join("mods/Bus")), "{roots:?}");
        std::fs::remove_dir_all(&content).ok();
    }

    #[test]
    fn a_folder_mod_is_registered_anew_when_it_gains_a_pack() {
        let _roots = roots_guard();
        let content = std::env::temp_dir().join(format!("mods-new-pack-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&content);
        std::fs::create_dir_all(content.join("mods/Bus/Vehicles/Bus")).unwrap();
        let omsi = content.join("omsi");
        std::fs::create_dir_all(&omsi).unwrap();
        let stamps = || STATE.read().unwrap().as_ref().unwrap().stamps.clone();
        init(&content, Some(&omsi)).unwrap();
        let (generation, before) = (legacy_config::content_generation(), stamps());
        init(&content, Some(&omsi)).unwrap();
        assert_eq!(
            legacy_config::content_generation(),
            generation,
            "nothing changed"
        );
        // The lookup builds the pack index before the new folder arrives.
        let rel = "Sceneryobjects\\BVG\\Haltestelle.sco";
        assert!(!vfs::exists(&resolve_path(&omsi, rel)));
        let sco = content.join("mods/Bus/Sceneryobjects/BVG/Haltestelle.sco");
        std::fs::create_dir_all(sco.parent().unwrap()).unwrap();
        std::fs::write(&sco, "Bus").unwrap();
        init(&content, Some(&omsi)).unwrap();
        assert_ne!(stamps(), before);
        assert_ne!(legacy_config::content_generation(), generation);
        assert_eq!(resolve_path(&omsi, rel), sco);
        std::fs::remove_dir_all(&content).ok();
    }

    #[test]
    fn a_folder_mod_is_registered_anew_when_a_pack_folder_gains_a_pack() {
        let _roots = roots_guard();
        let content =
            std::env::temp_dir().join(format!("mods-pack-in-folder-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&content);
        std::fs::create_dir_all(content.join("mods/Bus/Vehicles/Bus")).unwrap();
        let omsi = content.join("omsi");
        std::fs::create_dir_all(&omsi).unwrap();
        init(&content, Some(&omsi)).unwrap();
        // The lookup builds the pack index before the new pack arrives.
        let rel = "Vehicles\\Second\\second.bus";
        assert!(!vfs::exists(&resolve_path(&omsi, rel)));
        let bus = content.join("mods/Bus/Vehicles/Second/second.bus");
        std::fs::create_dir_all(bus.parent().unwrap()).unwrap();
        std::fs::write(&bus, "Bus").unwrap();
        init(&content, Some(&omsi)).unwrap();
        assert_eq!(resolve_path(&omsi, rel), bus);
        std::fs::remove_dir_all(&content).ok();
    }

    #[test]
    fn a_pack_added_to_the_content_folder_is_found() {
        let _roots = roots_guard();
        let content =
            std::env::temp_dir().join(format!("mods-content-pack-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&content);
        let omsi = content.join("omsi");
        std::fs::create_dir_all(&omsi).unwrap();
        // The engine registers the content folder itself as a root, as `register_roots` does.
        add_content_root(content.clone());
        init(&content, Some(&omsi)).unwrap();
        let rel = "Vehicles\\ByHand\\byhand.bus";
        assert!(!vfs::exists(&resolve_path(&omsi, rel)));
        let bus = content.join("Vehicles/ByHand/byhand.bus");
        std::fs::create_dir_all(bus.parent().unwrap()).unwrap();
        std::fs::write(&bus, "by hand").unwrap();
        init(&content, Some(&omsi)).unwrap();
        assert_eq!(resolve_path(&omsi, rel), bus);
        remove_content_root(&content);
        std::fs::remove_dir_all(&content).ok();
    }

    /// Three sign packs in four orders: an addon above its base replaces only the files it has.
    #[test]
    fn the_load_order_decides_which_file_the_game_uses() {
        let _roots = roots_guard();
        let content = std::env::temp_dir().join(format!("mods-signs-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&content);
        let mods = content.join(MODS_DIR);
        let put = |entry: &str, rel: &str, data: &str| {
            let path = mods.join(entry).join("Sceneryobjects/BVG").join(rel);
            std::fs::create_dir_all(path.parent().unwrap()).unwrap();
            std::fs::write(path, data).unwrap();
        };
        for (pack, year) in [("BVG_Signs_2015", "2015"), ("BVG_Signs_2023", "2023")] {
            put(pack, "Haltestelle.sco", year);
            put(pack, "model/hst.o3d", year);
            put(pack, "texture/schild.dds", year);
        }
        put("HD_Signs_Patch", "texture/schild.dds", "HD");
        let omsi = content.join("omsi");
        std::fs::create_dir_all(&omsi).unwrap();
        let read = |path: PathBuf| String::from_utf8(vfs::read(&path).unwrap()).unwrap();
        for (order, expected) in [
            (["BVG_Signs_2015", "BVG_Signs_2023", "HD_Signs_Patch"], ["2015", "2015", "2015"]),
            (["HD_Signs_Patch", "BVG_Signs_2023", "BVG_Signs_2015"], ["2023", "2023", "HD"]),
            (["BVG_Signs_2015", "HD_Signs_Patch", "BVG_Signs_2023"], ["2015", "2015", "2015"]),
            (["HD_Signs_Patch", "BVG_Signs_2015", "BVG_Signs_2023"], ["2015", "2015", "HD"]),
        ] {
            let file = OrderFile {
                version: 1,
                order: order.iter().map(|name| name.to_string()).collect(),
                ..Default::default()
            };
            order::save(&mods, &file).unwrap();
            init(&content, Some(&omsi)).unwrap();
            // A map names objects from the top of the OMSI folder; an object names its parts.
            let sco = resolve_path(&omsi, "Sceneryobjects\\BVG\\Haltestelle.sco");
            let dir = sco.parent().unwrap().to_path_buf();
            let used = [
                read(sco),
                read(resolve_path(&dir, "model\\hst.o3d")),
                read(resolve_path(&dir, "texture\\schild.dds")),
            ];
            assert_eq!(used, expected, "order {order:?}");
        }
        std::fs::remove_dir_all(&content).ok();
    }

    #[test]
    fn a_broken_archive_is_remembered_not_mounted() {
        let _roots = roots_guard();
        let content = std::env::temp_dir().join(format!("mods-init-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&content);
        std::fs::create_dir_all(content.join("mods/Bus/Vehicles")).unwrap();
        std::fs::create_dir_all(content.join(ARCHIVES_DIR)).unwrap();
        std::fs::write(content.join(ARCHIVES_DIR).join("old.zip"), b"not a zip").unwrap();
        init(&content, None).unwrap();
        {
            let state = STATE.read().unwrap();
            let state = state.as_ref().unwrap();
            assert_eq!(state.failed.len(), 1);
            assert_eq!(state.failed[0].0, content.join(ARCHIVES_DIR).join("old.zip"));
            assert_eq!(state.mods.len(), 3, "the archive (in the order, not mounted), Bus and shared");
            assert_eq!(state.roots.len(), 2);
        }
        assert!(content_roots().contains(&content.join("mods/Bus")));
        std::fs::remove_dir_all(&content).ok();
    }

    #[test]
    fn the_shared_library_ranks_below_every_mod_and_above_omsi() {
        let _roots = roots_guard();
        let content = std::env::temp_dir().join(format!("mods-shared-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&content);
        let put = |rel: &str, data: &str| {
            let path = content.join(rel);
            std::fs::create_dir_all(path.parent().unwrap()).unwrap();
            std::fs::write(path, data).unwrap();
        };
        put("mods/Bus/Texture/a.dds", "Bus");
        put("mods/shared/Texture/a.dds", "shared");
        put("mods/shared/Texture/b.dds", "shared");
        put("omsi/Texture/b.dds", "OMSI");
        put("omsi/Texture/c.dds", "OMSI");
        let omsi = content.join("omsi");
        init(&content, Some(&omsi)).unwrap();
        let read = |rel: &str| String::from_utf8(vfs::read(&resolve_path(&omsi, rel)).unwrap()).unwrap();
        let used = [read("Texture/a.dds"), read("Texture/b.dds"), read("Texture/c.dds")];
        assert_eq!(used, ["Bus", "shared", "OMSI"]);
        // It is not part of the load order.
        assert_eq!(load_order(&content).items.len(), 1);
        let shared = conflicts().into_iter().find(|conflict| conflict.id == SHARED_DIR).unwrap();
        assert_eq!((shared.overridden, shared.hidden_by), (1, vec!["Bus".to_string()]));
        // It is created when a content folder doesn't have one yet.
        std::fs::remove_dir_all(content.join("mods/shared")).unwrap();
        init(&content, Some(&omsi)).unwrap();
        assert!(content.join("mods/shared").is_dir());
        std::fs::remove_dir_all(&content).ok();
    }
}
