//! The launcher's data side: what the window asks for (maps, buses, lines, tours, the
//! roadbook, weather, profiles, settings) and how a duty is turned into a command line for
//! the game. Everything here is plain functions over the OMSI content crates; the window
//! (the game binary's `launcher` module, drawn with wgpu) calls them directly, and
//! `cli()` exposes the same functions to a terminal (`omsi-launcher --cli ...`).
//!
//! `install` runs mod installs as background jobs, `index` caches the content lists and
//! tells the page when they changed, `instances` keeps track of the games started.

pub mod index;
pub mod install;
pub mod instances;
pub mod servers;

use anyhow::{Context, Result, anyhow};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use std::path::{Path, PathBuf};

// ---------------------------------------------------------------------------------------
// configuration: where the game and the OMSI content are

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct Config {
    /// The OMSI 2 folder (maps, Vehicles, ...).
    pub root: String,
    /// The `omsi` game binary.
    pub game: String,
    /// The current profile name.
    pub profile: String,
}

fn home() -> PathBuf {
    std::env::var_os("HOME")
        .or_else(|| std::env::var_os("USERPROFILE"))
        .map(PathBuf::from)
        .unwrap_or_default()
}

pub fn data_dir() -> PathBuf {
    let d = home().join(".neoomsi");
    let _ = std::fs::create_dir_all(&d);
    d
}

fn config_path() -> PathBuf {
    data_dir().join("launcher.json")
}

/// A complete installation of the original OMSI 2 (the same test the game makes: it refuses
/// to start on anything else).
fn is_omsi_root(p: &Path) -> bool {
    ::legacy_config::missing_original_essentials(p).is_empty()
}

/// Where the game binary is: the configured path, next to the launcher, or the
/// development build in the source tree.
fn find_game(configured: &str) -> Option<PathBuf> {
    let mut cands: Vec<PathBuf> = Vec::new();
    // the game that came with this launcher first: a path remembered from an older
    // installation (`target/release/omsi` of the days before the rename) kept starting an
    // old build after every update - the new pause menu "was not there" on macOS
    if let Ok(exe) = std::env::current_exe() {
        if let Some(dir) = exe.parent() {
            cands.push(dir.join(if cfg!(windows) {
                "neoomsi.exe"
            } else {
                "neoomsi"
            }));
        }
    }
    if !configured.trim().is_empty() {
        let c = PathBuf::from(configured.trim());
        // (only a game of today's name: the old `omsi` binary is not taken any more)
        let stem = c
            .file_stem()
            .map(|s| s.to_string_lossy().to_ascii_lowercase())
            .unwrap_or_default();
        if stem == "neoomsi" {
            cands.push(c);
        }
    }
    if let Some(p) = std::env::var_os("neoomsi_BIN") {
        cands.push(PathBuf::from(p));
    }
    if let Ok(exe) = std::env::current_exe() {
        for a in exe.ancestors().skip(1).take(7) {
            cands.push(a.join("neoomsi"));
            cands.push(a.join("neoomsi.exe"));
            cands.push(a.join("target").join("release").join("neoomsi"));
            cands.push(a.join("target").join("release").join("neoomsi.exe"));
            cands.push(a.join("Resources").join("neoomsi"));
        }
    }
    if let Ok(cwd) = std::env::current_dir() {
        for a in cwd.ancestors().take(4) {
            cands.push(a.join("target").join("release").join("neoomsi"));
        }
    }
    cands.push(data_dir().join("neoomsi"));
    cands.into_iter().find(|p| p.is_file())
}

/// The last search for the OMSI folder: (the places given first, what was found).
static FOUND: std::sync::Mutex<Option<(Vec<PathBuf>, Option<PathBuf>)>> =
    std::sync::Mutex::new(None);

/// The OMSI folder: configured, remembered by the game, or found in any usual place
/// (beside the program, Steam libraries, Wine bottles, the user's folders).
fn find_root(configured: &str) -> Option<PathBuf> {
    let mut first: Vec<PathBuf> = Vec::new();
    if !configured.trim().is_empty() {
        first.push(PathBuf::from(configured.trim()));
    }
    if let Some(p) = std::env::var_os("OMSI_ROOT") {
        first.push(PathBuf::from(p));
    }
    if let Ok(t) = std::fs::read_to_string(home().join(".neoomsi-root")) {
        first.push(PathBuf::from(t.trim()));
    }
    // searching the disk costs a moment: once per process is enough (until the settings
    // are saved again, see `save_config`)
    let mut g = FOUND.lock().unwrap_or_else(|e| e.into_inner());
    if let Some((k, v)) = g.as_ref() {
        if *k == first && v.as_ref().map(|p| is_omsi_root(p)).unwrap_or(true) {
            return v.clone();
        }
    }
    let r = ::legacy_config::find_original_install(&first);
    if let Some(p) = &r {
        // the game finds it the same way next time
        let _ = std::fs::write(home().join(".neoomsi-root"), p.to_string_lossy().as_bytes());
    }
    *g = Some((first, r.clone()));
    r
}

pub fn load_config() -> Config {
    let mut c: Config = std::fs::read_to_string(config_path())
        .ok()
        .and_then(|t| serde_json::from_str(&t).ok())
        .unwrap_or_default();
    if let Some(r) = find_root(&c.root) {
        c.root = r.to_string_lossy().to_string();
    }
    if let Some(g) = find_game(&c.game) {
        c.game = g.to_string_lossy().to_string();
    }
    if c.profile.trim().is_empty() {
        // a first start takes the driver OMSI 2 had last ([last_driver] of its options.cfg)
        c.profile = Path::new(&c.root)
            .is_dir()
            .then(|| omsi_options(Path::new(&c.root)))
            .flatten()
            .and_then(|o| o.last_driver)
            .unwrap_or_else(|| "Driver".into());
    }
    c
}

/// What the player's own OMSI 2 remembers in its `options.cfg`: the settings in the
/// launcher's keys, the map and the driver played last.
pub struct OmsiOptions {
    pub settings: Value,
    /// `maps/<name>/global.cfg`, as the launcher names maps.
    pub last_map: Option<String>,
    /// The personnel file's name without `.odr`.
    pub last_driver: Option<String>,
}

/// Read the original's `options.cfg` (never written: the game keeps its own settings).
pub fn omsi_options(root: &Path) -> Option<OmsiOptions> {
    let o = ::content::options::Options::load(&root.join("options.cfg")).ok()?;
    let mut v = json!({});
    let num = |k: &str| {
        o.str(k)
            .and_then(|x| x.trim().replace(',', ".").parse::<f64>().ok())
            .filter(|x| x.is_finite())
    };
    // (not on a phone: the PC's OMSI caps at 30, and a phone played at 30 frames)
    if let Some(x) = num("maxfps").filter(|_| !cfg!(target_os = "android")) {
        v["max_fps"] = json!(x.max(0.0) as i64);
    }
    if let Some(x) = num("maxcomplexity_map") {
        v["map_detail"] = json!(x.clamp(0.0, 255.0) as u8);
    }
    if let Some(x) = num("performance_minobjsize") {
        v["min_obj_size"] = json!(x.clamp(0.0, 0.2));
    }
    if let Some(x) = num("performance_maxobjdist") {
        v["max_obj_dist"] = json!((x.round() as i64).max(0).to_string());
    }
    if let Some(af) = o
        .values
        .get("texfilter")
        .and_then(|x| x.get(1))
        .and_then(|x| x.trim().parse::<i64>().ok())
    {
        v["anisotropy"] = json!(af.clamp(1, 16));
    }
    if let Some(x) = num("texmemlimit").filter(|x| *x > 0.0) {
        v["texture_memory"] = json!(x as i64);
    }
    if let Some(x) = num("performance_refltexsize") {
        v["mirror_size"] = json!(1i64 << (x as i64).clamp(6, 11));
    }
    if let Some(x) = o.str("performance_realreflexions") {
        v["mirror_refresh"] = json!(mirror_refresh(x));
    }
    if let Some(x) = num("sound_vol_master") {
        v["volume"] = json!(x.clamp(0.0, 1.0));
    }
    if let Some(d) = o.str("sound_doppler") {
        v["doppler"] = json!(!d.trim().eq_ignore_ascii_case("off"));
    }
    if let Some(l) = o.str("language").filter(|l| !l.trim().is_empty()) {
        v["language"] = json!(language_code(l));
    }
    if let Some(x) = num("wear_lifespan") {
        v["maintenance"] = json!((x as i64).clamp(0, 4));
    }
    if let Some(x) = num("aiunschedfactor") {
        v["ai_unsched_factor"] = json!((x as i64).clamp(0, 300));
    }
    if let Some(x) = num("aimaxcountscheduled") {
        v["ai_max_scheduled"] = json!((x as i64).max(0));
    }
    if let Some(x) = num("aimaxcountparked") {
        v["ai_max_parked"] = json!((x as i64).max(0));
    }
    if let Some(x) = num("aipassfactor") {
        v["pax_density"] = json!((x / 100.0).clamp(0.0, 3.0));
    }
    // flags: present or not
    v["head_movement"] = json!(o.flag("driverview_moving"));
    v["driverview_smooth"] = json!(o.flag("driverview_smooth"));
    v["collision_vehicles"] = json!(!o.flag("no_collision_vehtoveh"));
    v["collision_objects"] = json!(!o.flag("no_collision"));
    v["driver"] = json!(o.flag("see_own_driver"));
    if let Some(x) = num("ticketselling") {
        v["boarding"] = json!(if x > 0.5 { "pay" } else { "auto" });
    }
    let file_stem = |p: &str| {
        Path::new(&p.replace('\\', "/"))
            .file_stem()
            .map(|s| s.to_string_lossy().to_string())
    };
    Some(OmsiOptions {
        settings: v,
        last_map: o
            .str("last_map")
            .map(|m| m.trim().replace('\\', "/"))
            .filter(|m| !m.is_empty()),
        last_driver: o
            .str("last_driver")
            .and_then(file_stem)
            .filter(|d| !d.is_empty()),
    })
}

pub fn save_config(c: &Config) -> Result<()> {
    std::fs::write(config_path(), serde_json::to_vec_pretty(c)?)?;
    // the folder is looked for again: a folder that was not a complete OMSI 2 when it was
    // first chosen (still being copied, a part missing) and is now stayed "not found" until
    // the launcher was restarted, however often it was chosen and saved again
    *FOUND.lock().unwrap_or_else(|e| e.into_inner()) = None;
    Ok(())
}

fn root() -> Result<PathBuf> {
    let c = load_config();
    let r = PathBuf::from(&c.root);
    if c.root.trim().is_empty() {
        return Err(anyhow!("no OMSI 2 folder configured (set it under Setup)"));
    }
    let missing = ::legacy_config::missing_original_essentials(&r);
    if !missing.is_empty() {
        return Err(anyhow!(
            "{} is not a complete OMSI 2 installation (missing: {}); choose the original game's folder under Setup",
            r.display(),
            missing.join(", ")
        ));
    }
    Ok(r)
}

/// The game's own content folder: the folder of the game binary, laid out like OMSI 2
/// (Vehicles, maps, Sceneryobjects ...). Installed mods live here; the game searches it
/// before the original installation.
pub fn content_dir() -> Option<PathBuf> {
    // the same rules as the game: $OMSI_CONTENT, else the folder of the game binary (beside
    // the bundle when the binary sits inside a macOS .app)
    let dir = match std::env::var_os("OMSI_CONTENT") {
        Some(d) => PathBuf::from(d),
        None => {
            let c = load_config_raw();
            let game = find_game(&c.game)?;
            let dir = game.parent()?.to_path_buf();
            let beside = if dir.ends_with("Contents/MacOS") {
                dir.parent()?.parent()?.parent()?.to_path_buf()
            } else {
                dir
            };
            let cand = ::legacy_config::content_folder_of(&beside);
            if (cand.exists() || std::fs::create_dir_all(&cand).is_ok())
                && ::legacy_config::is_writable(&cand)
            {
                cand
            } else {
                data_dir().join("content")
            }
        }
    };
    let _ = ::legacy_config::ensure_content_layout(&dir);
    register_roots(&dir);
    Some(dir)
}

/// Tell the OMSI readers about the content folder, the archives used in place in its
/// `Archives` folder and the OMSI folder (in that order, as the game has them), so that a
/// path inside one is also looked for in the others (a repaint's `.cti` in the content
/// folder's copy of a stock bus folder, a map inside an archive).
fn register_roots(content: &Path) {
    static DONE: std::sync::Once = std::sync::Once::new();
    let content = content.to_path_buf();
    DONE.call_once(|| {
        ::legacy_config::add_content_root(content.clone());
        mount_archives(&content);
    });
    // the OMSI folder - also one chosen under Setup while the launcher runs (it was only
    // looked for once, at the start, and a folder set later was never a root)
    if let Some(r) = find_root(&load_config_raw().root) {
        ::legacy_config::add_content_root(r);
    }
}

/// Mount the archives in `<content>/Archives` that are not mounted yet (an install may
/// have put one there, or the user did), each as a content root in front of the OMSI
/// folder. Archives that went away stay mounted until the launcher restarts.
fn mount_archives(content: &Path) {
    let dir = content.join(install::ARCHIVES);
    let Ok(rd) = std::fs::read_dir(&dir) else {
        return;
    };
    let mut zips: Vec<PathBuf> = rd
        .flatten()
        .map(|e| e.path())
        .filter(|p| {
            p.is_file()
                && p.extension()
                .map(|e| e.eq_ignore_ascii_case("zip"))
                .unwrap_or(false)
        })
        .collect();
    zips.sort();
    let mounted: Vec<PathBuf> = ::legacy_config::vfs::mounts()
        .iter()
        .map(|m| m.path().to_path_buf())
        .collect();
    for z in zips.into_iter().filter(|z| !mounted.contains(z)) {
        mount_archive(&z);
    }
}

/// Mount one archive of the content folder for the lists.
pub(crate) fn mount_archive(zip: &Path) {
    match ::legacy_config::vfs::mount_zip(zip) {
        Ok(m) => match find_root(&load_config_raw().root) {
            Some(r) => ::legacy_config::add_content_root_before(m, &r),
            None => ::legacy_config::add_content_root(m),
        },
        Err(e) => {
            log_to_file(&format!("archive {}: {e}", zip.display()));
        }
    }
}

/// The archives of the content folder that are mounted, in name order.
fn archive_roots(content: &Path) -> Vec<PathBuf> {
    let dir = content.join(install::ARCHIVES);
    let mut v: Vec<PathBuf> = ::legacy_config::vfs::mounts()
        .iter()
        .map(|m| m.path().to_path_buf())
        .filter(|p| p.starts_with(&dir) && p.exists())
        .collect();
    v.sort();
    v
}

fn load_config_raw() -> Config {
    std::fs::read_to_string(config_path())
        .ok()
        .and_then(|t| serde_json::from_str(&t).ok())
        .unwrap_or_default()
}

/// `rel` (a path as the game takes it, `Vehicles/Foo/foo.bus`) in the content folder if it
/// is there, else in the OMSI folder.
fn resolve_content(rel: &str) -> Result<PathBuf> {
    let root = root()?;
    for b in bases() {
        if b == root {
            continue;
        }
        let p = ::legacy_config::resolve_path(&b, rel);
        if ::legacy_config::vfs::exists(&p) {
            return Ok(p);
        }
    }
    Ok(::legacy_config::resolve_path(&root, rel))
}

/// The folders the lists are made of: the content folder first, then the archives used in
/// place (mounted as folders), then the OMSI folder.
fn bases() -> Vec<PathBuf> {
    let mut v = Vec::new();
    if let Some(c) = content_dir() {
        mount_archives(&c);
        v.push(c.clone());
        v.extend(archive_roots(&c));
    }
    if let Ok(r) = root() {
        v.push(r);
    }
    v
}

/// Entries of `rel` (e.g. "Vehicles") across the content folder and the OMSI 2 folder;
/// a name in the content folder hides the same name in the installation.
fn merged_entries(rel: &str) -> Vec<PathBuf> {
    let mut out: Vec<PathBuf> = Vec::new();
    let mut seen: std::collections::HashSet<String> = std::collections::HashSet::new();
    let dirs: Vec<PathBuf> = bases().iter().map(|b| b.join(rel)).collect();
    for d in dirs {
        for (name, _) in ::legacy_config::vfs::list_dir(&d).unwrap_or_default() {
            if seen.insert(name.to_string_lossy().to_ascii_lowercase()) {
                out.push(d.join(name));
            }
        }
    }
    out.sort();
    out
}

/// Folders of `rel` by name across the content folder and the OMSI folder: each name with
/// all its copies, the content folder's first (a mod may add files to a stock folder).
fn merged_folders(rel: &str) -> Vec<(String, Vec<PathBuf>)> {
    let mut out: Vec<(String, Vec<PathBuf>)> = Vec::new();
    let mut at: std::collections::HashMap<String, usize> = std::collections::HashMap::new();
    for base in bases() {
        let dir = base.join(rel);
        let Some(list) = ::legacy_config::vfs::list_dir(&dir) else {
            continue;
        };
        let mut names: Vec<PathBuf> = list
            .into_iter()
            .filter(|(_, is_dir)| *is_dir)
            .map(|(n, _)| dir.join(n))
            .collect();
        names.sort();
        for p in names {
            let name = p
                .file_name()
                .unwrap_or_default()
                .to_string_lossy()
                .to_string();
            if name.starts_with('.') {
                continue;
            }
            // (by an index: thousands of folders compared each with all before took long)
            match at.get(&name.to_ascii_lowercase()) {
                Some(&i) => out[i].1.push(p),
                None => {
                    at.insert(name.to_ascii_lowercase(), out.len());
                    out.push((name, vec![p]));
                }
            }
        }
    }
    out.sort_by(|a, b| a.0.to_ascii_lowercase().cmp(&b.0.to_ascii_lowercase()));
    out
}

/// Is `p` in the content folder (a mod) rather than the OMSI folder?
/// (An archive used in place lies in the content folder's `Archives` too.)
fn in_content(p: &Path) -> bool {
    content_dir().map(|c| p.starts_with(c)).unwrap_or(false)
}

// ---------------------------------------------------------------------------------------
// mods: sorting a mod's folders into the content folder, the way it would be copied into
// an OMSI 2 installation by hand

#[derive(Serialize, Clone, Debug)]
pub struct ModsStatus {
    pub content_dir: String,
    /// (folder, number of entries)
    pub folders: Vec<(String, usize)>,
    pub inbox: String,
    /// What lies in the inbox now.
    pub inbox_items: Vec<String>,
    /// Packs waiting in Mods/waiting for the bus they belong to.
    pub waiting: Vec<String>,
    /// Archives used in place (in `<content>/Archives`): (name, size in bytes).
    pub archives: Vec<(String, u64)>,
    /// Free space on the content folder's disk (bytes).
    pub free_bytes: u64,
    /// What an earlier, interrupted install left and was removed now.
    pub cleaned: Vec<String>,
    pub jobs: Vec<install::Progress>,
}

/// Start installing the mod at `src` (a folder, .zip, .7z or .rar) into the content folder in the
/// background; `mode` is "auto" (unpack, or use a .zip in place when it does not fit; .7z
/// and .rar are always unpacked),
/// "extract" or "inplace".
pub fn start_install(src: &Path, mode: &str) -> Result<install::Progress> {
    let content =
        content_dir().ok_or_else(|| anyhow!("no game binary configured, so no content folder"))?;
    if !src.exists() {
        return Err(anyhow!("{} does not exist", src.display()));
    }
    let from_inbox = src.starts_with(content.join("Mods"));
    let job = install::start(
        content,
        root().ok(),
        src.to_path_buf(),
        install::InstallMode::parse(mode),
        from_inbox,
    );
    Ok(job.snapshot())
}

/// How big the mod at `src` is unpacked, what is free, and whether it can be used in place.
pub fn inspect_mod(src: &Path) -> Result<install::SourceInfo> {
    let content =
        content_dir().ok_or_else(|| anyhow!("no game binary configured, so no content folder"))?;
    install::inspect(&content, root().ok().as_deref(), src)
}

/// Install the mod at `src` and wait for it (the terminal), printing the progress.
pub fn install_mod_blocking(
    src: &Path,
    mode: &str,
    cancel_after_ms: Option<u64>,
) -> Result<install::Progress> {
    let content =
        content_dir().ok_or_else(|| anyhow!("no game binary configured, so no content folder"))?;
    Ok(install::run_blocking(
        content,
        root().ok(),
        src.to_path_buf(),
        install::InstallMode::parse(mode),
        cancel_after_ms.map(std::time::Duration::from_millis),
        true,
    ))
}

fn inbox_entries(content: &Path) -> Vec<PathBuf> {
    let inbox = content.join("Mods");
    let Ok(rd) = std::fs::read_dir(&inbox) else {
        return Vec::new();
    };
    let mut v: Vec<PathBuf> = rd
        .flatten()
        .map(|e| e.path())
        .filter(|p| {
            let name = p
                .file_name()
                .unwrap_or_default()
                .to_string_lossy()
                .to_string();
            !(name.starts_with('.')
                || name.eq_ignore_ascii_case("installed")
                || name.eq_ignore_ascii_case(install::WAITING)
                || name.eq_ignore_ascii_case(install::PLUGINS_HELD)
                || name.eq_ignore_ascii_case(install::UNINSTALLED)
                || name.eq_ignore_ascii_case("README.txt"))
                && (p.is_dir()
                || p.extension()
                .map(|x| {
                    ["zip", "7z", "rar"]
                        .iter()
                        .any(|ext| x.eq_ignore_ascii_case(ext))
                })
                .unwrap_or(false))
        })
        .collect();
    v.sort();
    v
}

/// (size, newest time, files) of a file or a folder tree.
fn tree_signature(p: &Path) -> (u64, u64, u64) {
    let Ok(md) = std::fs::symlink_metadata(p) else {
        return (0, 0, 0);
    };
    let t = md
        .modified()
        .ok()
        .and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok())
        .map(|d| d.as_nanos() as u64)
        .unwrap_or(0);
    if !md.is_dir() {
        return (md.len(), t, 1);
    }
    let mut acc = (0, t, 0);
    if let Ok(rd) = std::fs::read_dir(p) {
        for e in rd.flatten() {
            let (s, m, n) = tree_signature(&e.path());
            acc = (acc.0 + s, acc.1.max(m), acc.2 + n);
        }
    }
    acc
}

/// The inbox watcher: something dropped into Mods/ is installed once it has stopped
/// growing (the same size and time on two looks at least two seconds apart); packs in
/// Mods/waiting are installed once their bus is. Returns the sources of the jobs started.
fn watch_inbox(content: &Path) -> Vec<String> {
    struct Seen {
        sig: (u64, u64, u64),
        at: std::time::Instant,
        started: bool,
        /// When the signature was last read (a big folder whose install failed is looked
        /// at again only now and then).
        checked: std::time::Instant,
    }
    static SEEN: std::sync::Mutex<Option<std::collections::HashMap<PathBuf, Seen>>> =
        std::sync::Mutex::new(None);
    // a mod deleted from Mods/installed is taken out of the lists (#819)
    let gone = install::uninstall_removed(content);
    for g in &gone {
        log_line(&format!(
            "mods: {g} was deleted from Mods/installed - uninstalled, its folders are in Mods/{}/{g}",
            install::UNINSTALLED
        ));
    }
    if !gone.is_empty() {
        ::legacy_config::content_changed();
    }
    let mut started = Vec::new();
    let mut guard = SEEN.lock().unwrap_or_else(|e| e.into_inner());
    let seen = guard.get_or_insert_with(Default::default);
    let waiting_dir = content.join("Mods").join(install::WAITING);
    let items = inbox_entries(content);
    seen.retain(|p, _| items.contains(p) || (p.starts_with(&waiting_dir) && p.exists()));
    for p in items {
        if install::is_busy(&p) {
            continue;
        }
        let now = std::time::Instant::now();
        if seen
            .get(&p)
            .map(|s| {
                s.started && now.duration_since(s.checked) < std::time::Duration::from_secs(30)
            })
            .unwrap_or(false)
        {
            continue;
        }
        let sig = tree_signature(&p);
        if let Some(s) = seen.get_mut(&p) {
            s.checked = now;
        }
        match seen.get_mut(&p) {
            Some(s) if s.sig == sig => {
                if !s.started && now.duration_since(s.at) >= std::time::Duration::from_secs(2) {
                    s.started = true;
                    install::start(
                        content.to_path_buf(),
                        root().ok(),
                        p.clone(),
                        install::InstallMode::Auto,
                        true,
                    );
                    started.push(p.to_string_lossy().to_string());
                }
            }
            // new, still growing (a copy in progress), or changed after a failed try
            _ => {
                seen.insert(
                    p.clone(),
                    Seen {
                        sig,
                        at: now,
                        started: false,
                        checked: now,
                    },
                );
            }
        }
    }
    let busy = install::jobs().iter().any(|j| j.finished.is_none());
    if !busy {
        for p in install::waiting_ready(content, root().ok().as_deref()) {
            let sig = tree_signature(&p);
            if seen
                .get(&p)
                .map(|s| s.started && s.sig == sig)
                .unwrap_or(false)
            {
                continue;
            }
            seen.insert(
                p.clone(),
                Seen {
                    sig,
                    at: std::time::Instant::now(),
                    started: true,
                    checked: std::time::Instant::now(),
                },
            );
            install::start(
                content.to_path_buf(),
                root().ok(),
                p.clone(),
                install::InstallMode::Extract,
                true,
            );
            started.push(p.to_string_lossy().to_string());
        }
    }
    started
}

/// Install everything in the inbox (and the waiting packs whose bus is there) now, and
/// wait (the terminal's `mods`).
pub fn install_inbox_blocking() -> Vec<install::Progress> {
    let Some(content) = content_dir() else {
        return Vec::new();
    };
    let mut out = Vec::new();
    for p in inbox_entries(&content)
        .into_iter()
        .chain(install::waiting_ready(&content, root().ok().as_deref()))
    {
        let job = install::start(
            content.clone(),
            root().ok(),
            p,
            install::InstallMode::Auto,
            true,
        );
        while job.snapshot().finished.is_none() {
            std::thread::sleep(std::time::Duration::from_millis(50));
        }
        out.push(job.snapshot());
    }
    out
}

pub fn mods_status() -> Result<ModsStatus> {
    let content =
        content_dir().ok_or_else(|| anyhow!("no game binary configured, so no content folder"))?;
    let cleaned = install::cleanup_stale(&data_dir(), Some(&content));
    let folders = ::legacy_config::CONTENT_FOLDERS
        .iter()
        .map(|f| {
            let n = std::fs::read_dir(content.join(f))
                .map(|rd| {
                    rd.flatten()
                        .filter(|e| !e.file_name().to_string_lossy().starts_with('.'))
                        .count()
                })
                .unwrap_or(0);
            (f.to_string(), n)
        })
        .collect();
    let names = |v: Vec<PathBuf>| {
        v.iter()
            .filter_map(|p| p.file_name().map(|n| n.to_string_lossy().to_string()))
            .collect::<Vec<_>>()
    };
    let waiting: Vec<PathBuf> = std::fs::read_dir(content.join("Mods").join(install::WAITING))
        .map(|rd| rd.flatten().map(|e| e.path()).collect())
        .unwrap_or_default();
    let mut archives: Vec<(String, u64)> = std::fs::read_dir(content.join(install::ARCHIVES))
        .map(|rd| {
            rd.flatten()
                .filter(|e| {
                    e.path()
                        .extension()
                        .map(|x| x.eq_ignore_ascii_case("zip"))
                        .unwrap_or(false)
                })
                .map(|e| {
                    (
                        e.file_name().to_string_lossy().to_string(),
                        e.metadata().map(|m| m.len()).unwrap_or(0),
                    )
                })
                .collect()
        })
        .unwrap_or_default();
    archives.sort();
    Ok(ModsStatus {
        content_dir: content.to_string_lossy().to_string(),
        folders,
        inbox: content.join("Mods").to_string_lossy().to_string(),
        inbox_items: names(inbox_entries(&content)),
        waiting: names(waiting),
        archives,
        free_bytes: install::free_space(&content).unwrap_or(0),
        cleaned,
        jobs: install::jobs(),
    })
}

/// What the page asks every few seconds: whether the content changed (then it asks for the
/// lists again), the install jobs and the running games. It also starts the inbox installs.
#[derive(Serialize, Clone, Debug)]
pub struct Poll {
    pub stamp: String,
    pub jobs: Vec<install::Progress>,
    pub instances: Vec<instances::Instance>,
    /// Inbox items whose install started with this poll.
    pub started: Vec<String>,
}

pub fn poll() -> Result<Poll> {
    let content = content_dir();
    let started = content.as_deref().map(watch_inbox).unwrap_or_default();
    let stamp = index::content_stamp(
        &bases(),
        content.as_ref().map(|c| c.join("Mods")).as_deref(),
    );
    Ok(Poll {
        stamp,
        jobs: install::jobs(),
        instances: instances::list(),
        started,
    })
}

// ---------------------------------------------------------------------------------------
// content lists

#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct MapInfo {
    pub name: String,
    pub friendly: String,
    pub file: String,
    pub description: String,
    pub entry_points: Vec<EntryInfo>,
    /// The depot file (`.hof` name) the map's own buses use, from ailists.cfg.
    pub hof: String,
    /// Installed as a mod (in the content folder).
    #[serde(default)]
    pub installed: bool,
}

#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct EntryInfo {
    pub index: i32,
    pub name: String,
}

pub fn list_maps() -> Result<Vec<MapInfo>> {
    root()?;
    let lang = content_language();
    let mut out = Vec::new();
    let mut keys = Vec::new();
    for (folder, dirs) in merged_folders("maps") {
        // a map is one folder: the first copy that has a global.cfg
        let Some(d) = dirs
            .into_iter()
            .find(|d| ::legacy_config::vfs::is_file(&d.join("global.cfg")))
        else {
            continue;
        };
        let key = format!("map|{lang}|{}", d.display());
        let mut stamped = vec![d.clone(), d.join("global.cfg"), d.join("ailists.cfg")];
        stamped.extend(dsc_candidates(&d.join("global.cfg"), lang));
        let stamp = index::files_stamp(&stamped);
        keys.push(key.clone());
        let info: Option<MapInfo> =
            index::cached(&key, stamp, || (read_map(&d, &folder, lang), Vec::new()));
        out.extend(info);
    }
    index::save("map|", Some(&keys));
    if out.is_empty() {
        log_empty("maps", "global.cfg");
    }
    Ok(out)
}

/// Say in launcher.log where a list that came out empty was looked for: each folder of
/// `rel` and how many entries it has (a player whose lists stay empty sends the log).
fn log_empty(rel: &str, what: &str) {
    let places: Vec<String> = bases()
        .iter()
        .map(|b| {
            let d = b.join(rel);
            match ::legacy_config::vfs::list_dir(&d) {
                Some(l) => format!("{} ({} entries)", d.display(), l.len()),
                None => format!(
                    "{} (cannot be read: {})",
                    d.display(),
                    std::fs::read_dir(&d)
                        .err()
                        .map(|e| e.to_string())
                        .unwrap_or_else(|| "not a folder".into())
                ),
            }
        })
        .collect();
    log_line(&format!(
        "{rel}: nothing with a {what} found in {}",
        if places.is_empty() {
            "no folder (no OMSI 2 folder and no content folder)".to_string()
        } else {
            places.join(", ")
        }
    ));
}

fn read_map(d: &Path, folder: &str, lang: &str) -> Option<MapInfo> {
    let g = match ::map::GlobalCfg::load(&d.join("global.cfg")) {
        Ok(g) => g,
        Err(e) => {
            log_line(&format!(
                "maps: {} cannot be read ({e:#}) - not listed",
                d.join("global.cfg").display()
            ));
            return None;
        }
    };
    // `global_ENG.dsc` (the language of the settings) names and describes the map
    let dsc = find_dsc(&d.join("global.cfg"), lang);
    let friendly = dsc
        .as_ref()
        .and_then(|x| x.name.first().cloned())
        .unwrap_or_else(|| g.friendly_name.trim().to_string());
    let description = dsc
        .map(|x| x.description)
        .filter(|t| !t.is_empty())
        .unwrap_or_else(|| g.description.trim().to_string());
    let mut entries: Vec<EntryInfo> = g
        .entry_points
        .iter()
        .map(|e| EntryInfo {
            index: e.index,
            name: e.name.trim().to_string(),
        })
        .collect();
    // the game's --entry is the position in the list, not the index field; several
    // entries often share a name (one per stop position), so number them
    let mut seen: std::collections::HashMap<String, usize> = std::collections::HashMap::new();
    let total: std::collections::HashMap<String, usize> =
        entries
            .iter()
            .fold(std::collections::HashMap::new(), |mut m, e| {
                *m.entry(e.name.clone()).or_default() += 1;
                m
            });
    for (i, e) in entries.iter_mut().enumerate() {
        e.index = i as i32;
        let n = seen.entry(e.name.clone()).or_default();
        *n += 1;
        if total.get(&e.name).copied().unwrap_or(0) > 1 {
            e.name = format!("{} ({})", e.name, n);
        }
    }
    // (the depot groups' first: a plain car group's name line is no depot)
    let hof = ::map::ailists::AiLists::load(&d.join("ailists.cfg"))
        .ok()
        .and_then(|l| {
            l.groups
                .iter()
                .filter(|g| g.is_depot)
                .chain(l.groups.iter())
                .find_map(|g| g.hof.clone())
        })
        .unwrap_or_default();
    Some(MapInfo {
        name: if g.name.trim().is_empty() {
            folder.to_string()
        } else {
            g.name.trim().to_string()
        },
        friendly,
        file: format!("maps/{folder}/global.cfg"),
        description,
        entry_points: entries,
        hof,
        installed: in_content(d),
    })
}

#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct VehicleInfo {
    pub name: String,
    pub manufacturer: String,
    pub type_name: String,
    pub file: String,
    pub folder: String,
    pub description: String,
    /// The original third [friendlyname] line, shown for the bus's standard paint.
    #[serde(default)]
    pub default_paint: String,
    pub paints: Vec<String>,
    pub hofs: Vec<String>,
    /// Installed as a mod (the bus file is in the content folder).
    #[serde(default)]
    pub installed: bool,
    /// Vehicle packs this bus borrows parts from that are not installed (the Ahlheim
    /// Citaro's dashboard, steering wheel and ticket machine come from `Urbino_II`): it
    /// drives, but with holes in the cockpit, as it would in OMSI.
    #[serde(default)]
    pub missing_packs: Vec<String>,
    /// The fleet numbers of the bus's `[number]` list with the plate each comes with
    /// (Omsi.exe's number combo in the vehicle dialog; empty: the bus has no list).
    #[serde(default)]
    pub numbers: Vec<(String, String)>,
}

/// Names in older vehicle packs use underscores as spaces.
pub fn display_bus_name(name: &str) -> String {
    name.replace('_', " ")
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
}

/// The displayed vehicle type, using its file name when [friendlyname] leaves it empty.
pub fn vehicle_type_label(type_name: &str, path: &Path) -> String {
    if type_name.trim().is_empty() {
        display_bus_name(&path.file_stem().unwrap_or_default().to_string_lossy())
    } else {
        display_bus_name(type_name)
    }
}

/// The vehicle packs whose parts a model file names and that are installed nowhere.
fn missing_packs_of(model: &Path) -> Vec<String> {
    let Ok(text) = ::legacy_config::vfs::read(model) else {
        return Vec::new();
    };
    let text = ::legacy_config::codepage::decode(&text);
    let dir = model.parent().unwrap_or(Path::new("."));
    let mut out: Vec<String> = Vec::new();
    for line in text.lines() {
        let l = line.trim();
        if !l.starts_with("..") {
            continue;
        }
        let p = ::legacy_config::resolve_path(dir, l);
        // (the game also finds a part from the model's parent folders - `<vehicle>/model`
        // and the vehicle folder for a cfg in `model/Configuration Files`: mesh_path)
        let found = |d: &Path| ::legacy_config::vfs::is_file(&::legacy_config::resolve_path(d, l));
        if ::legacy_config::vfs::is_file(&p) || dir.ancestors().skip(1).take(2).any(found) {
            continue;
        }
        if let Some(pack) = ::legacy_config::missing_vehicle_pack(&p) {
            if !out.contains(&pack) {
                out.push(pack);
            }
        }
    }
    out
}

/// `[item]` names of every `.cti` in the model's `[CTC]` folders (in every content root):
/// the paint schemes, and the folders they were read from.
fn paint_schemes(vehicle: &::legacy_vehicle::Vehicle) -> (Vec<String>, Vec<PathBuf>) {
    let mut names: Vec<String> = Vec::new();
    let mut dirs_read: Vec<PathBuf> = Vec::new();
    let Some(model_rel) = vehicle.model.as_ref() else {
        return (names, dirs_read);
    };
    let model_path = ::legacy_config::resolve_path(vehicle.dir(), model_rel);
    let Ok(model) = ::model::Model::load(&model_path) else {
        return (names, dirs_read);
    };
    for c in &model.ctc {
        let dir = ::legacy_config::resolve_path(vehicle.dir(), &c.path);
        let mut files: Vec<PathBuf> = Vec::new();
        let mut seen: std::collections::HashSet<String> = std::collections::HashSet::new();
        let mut copies = ::legacy_config::mirrored_dirs(&dir);
        if !copies.contains(&dir) {
            copies.push(dir.clone());
        }
        for d in copies {
            dirs_read.push(d.clone());
            let Some(list) = ::legacy_config::vfs::list_dir(&d) else {
                continue;
            };
            let mut here: Vec<PathBuf> = list
                .into_iter()
                .map(|(n, _)| d.join(n))
                .filter(|p| {
                    p.extension()
                        .map(|e| e.eq_ignore_ascii_case("cti"))
                        .unwrap_or(false)
                })
                .collect();
            here.sort();
            for f in here {
                if seen.insert(
                    f.file_name()
                        .unwrap_or_default()
                        .to_string_lossy()
                        .to_ascii_lowercase(),
                ) {
                    files.push(f);
                }
            }
        }
        for f in files {
            let Ok(cfg) = ::legacy_config::CfgFile::read(&f) else {
                continue;
            };
            let mut r = cfg.reader();
            while let Some(k) = r.next_keyword() {
                if k == "item" {
                    let name = r.str().trim().to_string();
                    if !name.is_empty() && !names.iter().any(|n| n.eq_ignore_ascii_case(&name)) {
                        names.push(name);
                    }
                }
            }
        }
    }
    dirs_read.sort();
    dirs_read.dedup();
    (names, dirs_read)
}

/// One line into ~/.neoomsi/launcher.log.
fn log_line(line: &str) {
    use std::io::Write;
    let p = data_dir().join("launcher.log");
    if let Ok(mut f) = std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(&p)
    {
        let now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_secs())
            .unwrap_or(0);
        let _ = writeln!(f, "{now} {line}");
    }
}

pub fn list_vehicles() -> Result<Vec<VehicleInfo>> {
    list_vehicles_progress(|_, _, _| {})
}

/// The buses, as `list_vehicles`, with `progress` told after every few folders what they
/// held, how many folders are done and how many there are: a big installation's first
/// reading (thousands of vehicle folders, nothing in the cache yet) takes minutes, and the
/// page showed nothing at all until the last folder was read.
pub fn list_vehicles_progress(
    progress: impl Fn(&[VehicleInfo], usize, usize),
) -> Result<Vec<VehicleInfo>> {
    use rayon::prelude::*;
    root()?;
    let lang = content_language();
    let folders = merged_folders("Vehicles");
    let keys: Vec<String> = folders
        .iter()
        .map(|(_, dirs)| {
            format!(
                "bus4|{lang}|{}",
                dirs.iter()
                    .map(|d| d.to_string_lossy())
                    .collect::<Vec<_>>()
                    .join("|")
            )
        })
        .collect();
    let read = |(folder, dirs): &(String, Vec<PathBuf>), key: &String| -> Vec<VehicleInfo> {
        // the stamp covers every copy of the folder and their direct entries (Model/,
        // Texture/ ...); the paint folders the entry read are its dependencies
        let mut stamped: Vec<PathBuf> = dirs.clone();
        for d in dirs {
            if let Some(list) = ::legacy_config::vfs::list_dir(d) {
                let mut subs: Vec<PathBuf> = list
                    .into_iter()
                    .filter(|(_, is_dir)| *is_dir)
                    .map(|(n, _)| d.join(n))
                    .collect();
                subs.sort();
                stamped.extend(subs);
            }
        }
        index::cached(key, index::folder_stamp(&stamped), || {
            read_vehicle_folder(folder, dirs, lang)
        })
    };
    // folders side by side (the files of one wait for the disk while another's are parsed),
    // a handful of threads so that a hard disk is not sent seeking all over
    let pool = rayon::ThreadPoolBuilder::new()
        .num_threads(
            std::thread::available_parallelism()
                .map(|n| n.get())
                .unwrap_or(4)
                .clamp(2, 8),
        )
        .build()
        .ok();
    let mut out = Vec::new();
    let total = folders.len();
    let mut done = 0;
    let mut saved = std::time::Instant::now();
    for (chunk, chunk_keys) in folders.chunks(32).zip(keys.chunks(32)) {
        let lists: Vec<Vec<VehicleInfo>> = match &pool {
            Some(pool) => pool.install(|| {
                chunk
                    .par_iter()
                    .zip(chunk_keys.par_iter())
                    .map(|(f, k)| read(f, k))
                    .collect()
            }),
            None => chunk
                .iter()
                .zip(chunk_keys.iter())
                .map(|(f, k)| read(f, k))
                .collect(),
        };
        let batch: Vec<VehicleInfo> = lists.into_iter().flatten().collect();
        done += chunk.len();
        // what was read is kept every few seconds: a first reading left half-way (the
        // launcher closed) starts from there the next time
        if saved.elapsed().as_secs() >= 10 {
            index::save("bus4|", None);
            saved = std::time::Instant::now();
        }
        progress(&batch, done, total);
        out.extend(batch);
    }
    index::save("bus4|", Some(&keys));
    if out.is_empty() {
        log_empty("Vehicles", ".bus file");
    }
    Ok(out)
}

/// The buses of one vehicle folder (all its copies; a bus file in the content folder hides
/// the one of the same name in the OMSI folder), and the folders read besides its own.
fn read_vehicle_folder(
    folder: &str,
    dirs: &[PathBuf],
    lang: &str,
) -> (Vec<VehicleInfo>, Vec<PathBuf>) {
    // Every file of the folder and of the folders in it, however deep: OMSI's bus list looks
    // for `*.bus` and `*.ovh` under Vehicles to any depth (Omsi.exe 0x67bdf9, the search
    // with depth 255) - `Vehicles\Pack\Variant\x.bus` was not listed here (#137). A file of
    // the content folder hides the one at the same place in the installation.
    fn walk(d: &Path, rel: &str, depth: u32, out: &mut Vec<(String, PathBuf)>) {
        let mut list = ::legacy_config::vfs::list_dir(d).unwrap_or_default();
        list.sort();
        for (n, is_dir) in list {
            let name = n.to_string_lossy().to_string();
            let r = if rel.is_empty() {
                name.clone()
            } else {
                format!("{rel}/{name}")
            };
            if is_dir {
                if depth > 0 && !name.starts_with('.') {
                    walk(&d.join(&n), &r, depth - 1, out);
                }
            } else {
                out.push((r, d.join(&n)));
            }
        }
    }
    let mut files: Vec<PathBuf> = Vec::new();
    let mut rel_of: std::collections::HashMap<PathBuf, String> = std::collections::HashMap::new();
    let mut seen: std::collections::HashSet<String> = std::collections::HashSet::new();
    for d in dirs {
        let mut here = Vec::new();
        walk(d, "", 8, &mut here);
        for (r, f) in here {
            if seen.insert(r.to_ascii_lowercase()) {
                rel_of.insert(f.clone(), r);
                files.push(f);
            }
        }
    }
    let mut out = Vec::new();
    let mut deps = Vec::new();
    if !files.iter().any(|f| {
        f.extension()
            .map(|e| e.eq_ignore_ascii_case("bus") || e.eq_ignore_ascii_case("ovh"))
            .unwrap_or(false)
    }) {
        // textures, or a repaint for a bus that is not installed: nothing to drive
        log_line(&format!(
            "vehicles: Vehicles/{folder} has no .bus file (a repaint or textures for a bus that is not installed?) - not listed"
        ));
        return (out, deps);
    }
    // the depot files beside each bus (a bus in a folder of the pack: those of its folder,
    // else those of the pack's own)
    let hof_dir = |f: &PathBuf| {
        rel_of
            .get(f)
            .map(|r| {
                r.rsplit_once('/')
                    .map(|(d, _)| d.to_ascii_lowercase())
                    .unwrap_or_default()
            })
            .unwrap_or_default()
    };
    let mut hofs_in: std::collections::HashMap<String, Vec<String>> =
        std::collections::HashMap::new();
    for f in files.iter().filter(|f| {
        f.extension()
            .map(|e| e.eq_ignore_ascii_case("hof"))
            .unwrap_or(false)
    }) {
        // (the name alone: UK depot files carry megabytes of trips)
        if let Some(name) = ::legacy_vehicle::Hof::read_name(f) {
            hofs_in
                .entry(hof_dir(f))
                .or_default()
                .push(name.trim().to_string());
        }
    }
    // OMSI offers what has a [friendlyname]: never the rear section of an articulated bus
    // (its front brings it along), an AI-only variant or a car
    for (f, v) in ::legacy_vehicle::vehicle::offered_vehicles(&files) {
        let f = &f;
        let stem = f
            .file_stem()
            .unwrap()
            .to_string_lossy()
            .to_ascii_lowercase();
        // a vehicle file whose model is not there would load as nothing
        let model = v.model.as_ref().map(|m| ::legacy_config::resolve_path(v.dir(), m));
        if !model
            .as_ref()
            .map(|m| ::legacy_config::vfs::is_file(m))
            .unwrap_or(false)
        {
            log_line(&format!(
                "vehicles: {} - its model {} is missing, not listed",
                f.display(),
                model
                    .map(|m| m.display().to_string())
                    .unwrap_or_else(|| "(none)".into())
            ));
            continue;
        }
        let rel = format!(
            "Vehicles/{}/{}",
            folder,
            rel_of.get(f).cloned().unwrap_or_else(|| f
                .file_name()
                .unwrap()
                .to_string_lossy()
                .to_string())
        );
        let hofs = hofs_in
            .get(&hof_dir(f))
            .or_else(|| hofs_in.get(""))
            .cloned()
            .unwrap_or_default();
        let name = format!("{} {}", v.manufacturer.trim(), v.type_name.trim())
            .trim()
            .to_string();
        let (paints, paint_dirs) = paint_schemes(&v);
        deps.extend(paint_dirs);
        // `<bus>_ENG.dsc` beside the bus file (the language of the settings) describes it
        let description = find_dsc(f, lang)
            .map(|x| x.description)
            .filter(|t| !t.is_empty())
            .unwrap_or_else(|| v.description.trim().to_string());
        let missing_packs = model.as_deref().map(missing_packs_of).unwrap_or_default();
        if !missing_packs.is_empty() {
            log_line(&format!(
                "vehicles: {} borrows parts from packs that are not installed: {}",
                f.display(),
                missing_packs.join(", ")
            ));
        }
        out.push(VehicleInfo {
            name: if name.is_empty() { stem.clone() } else { name },
            manufacturer: v.manufacturer.trim().to_string(),
            type_name: v.type_name.trim().to_string(),
            file: rel,
            folder: folder.to_string(),
            description: description.chars().take(600).collect(),
            default_paint: v.default_paint.trim().to_string(),
            paints,
            hofs,
            installed: in_content(f),
            missing_packs,
            numbers: v.numbers_with_plates(),
        });
    }
    deps.sort();
    deps.dedup();
    // the folders themselves are stamped by the caller
    deps.retain(|d| !dirs.contains(d));
    (out, deps)
}

#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct WeatherInfo {
    pub name: String,
    pub file: String,
    pub description: String,
    pub fog_m: f32,
    pub temp: f32,
    pub clouds: String,
    pub precip: String,
    pub snow: bool,
    #[serde(default)]
    pub installed: bool,
}

pub fn list_weather() -> Result<Vec<WeatherInfo>> {
    root()?;
    let mut out = Vec::new();
    let mut keys = Vec::new();
    let lang = content_language();
    let mut files: Vec<PathBuf> = merged_entries("Weather")
        .into_iter()
        .filter(|p| {
            p.extension()
                .map(|e| e.eq_ignore_ascii_case("owt"))
                .unwrap_or(false)
        })
        .collect();
    files.sort();
    for f in files {
        // the description in the settings' language lives in `<name>_ENG.dsc`
        let mut stamped = vec![f.clone()];
        stamped.extend(dsc_candidates(&f, lang));
        let key = format!("wx|{lang}|{}", f.display());
        keys.push(key.clone());
        let info: Option<WeatherInfo> = index::cached(&key, index::files_stamp(&stamped), || {
            (read_weather(&f, lang), Vec::new())
        });
        out.extend(info);
    }
    index::save("wx|", Some(&keys));
    Ok(out)
}

fn read_weather(f: &Path, lang: &str) -> Option<WeatherInfo> {
    let w = ::content::weather::Weather::load(f).ok()?;
    let stem = f.file_stem().unwrap().to_string_lossy().to_string();
    // (the `[name]` of the file is not part of the description: "Ground Fog Heavy ground fog ...")
    let description = find_dsc(f, lang)
        .map(|x| x.description.replace('\n', " "))
        .filter(|t| !t.is_empty())
        .unwrap_or_else(|| w.description.replace('\n', " "));
    let precip = match w.precip.first().copied().unwrap_or(0.0) as i32 {
        1 => format!(
            "rain {:.0}%",
            w.precip.get(1).copied().unwrap_or(0.0) / 255.0 * 100.0
        ),
        2 => format!(
            "snow {:.0}%",
            w.precip.get(1).copied().unwrap_or(0.0) / 255.0 * 100.0
        ),
        _ => "dry".into(),
    };
    Some(WeatherInfo {
        name: stem.trim_start_matches('#').to_string(),
        file: format!("Weather/{}", f.file_name().unwrap().to_string_lossy()),
        description,
        fog_m: w.fog.0,
        temp: w.temp.0,
        clouds: if w.clouds.0.trim().starts_with("-1") {
            "clear".into()
        } else {
            w.clouds.0.trim().to_string()
        },
        precip,
        snow: w.snow,
        installed: in_content(f),
    })
}

/// The content language of the settings (`language=`, English by default): which
/// `<file>_<LANG>.dsc` names and describes maps, buses and weathers.
fn content_language() -> &'static str {
    language_code(current_settings()["language"].as_str().unwrap_or("en"))
}

/// The description files OMSI reads for `file` (`global.cfg` -> `global_ENG.dsc`), in the
/// order they are tried: the language itself, then English for a language that has none
/// (German is the files' own language, so German falls back to the file itself).
fn dsc_candidates(file: &Path, lang: &str) -> Vec<PathBuf> {
    let stem = file
        .file_stem()
        .map(|s| s.to_string_lossy().to_string())
        .unwrap_or_default();
    let mut langs = vec![lang];
    if lang != "de" && lang != "en" {
        langs.push("en");
    }
    langs
        .into_iter()
        .map(|l| file.with_file_name(format!("{stem}_{}.dsc", omsi_suffix(l))))
        .collect()
}

fn omsi_suffix(lang: &str) -> &str {
    match lang {
        "en" => "ENG",
        "de" => "DEU",
        other => other,
    }
}

/// A `.dsc` file: the `[name]` / `[friendlyname]` lines and the `[description]` text.
struct Dsc {
    name: Vec<String>,
    description: String,
}

fn parse_dsc(text: &str) -> Dsc {
    let mut name = Vec::new();
    let mut desc: Vec<&str> = Vec::new();
    let mut section = "";
    for line in text.lines() {
        let t = line.trim();
        if t.starts_with('[') && t.ends_with(']') {
            section =
                if t.eq_ignore_ascii_case("[name]") || t.eq_ignore_ascii_case("[friendlyname]") {
                    "name"
                } else if t.eq_ignore_ascii_case("[description]") {
                    "description"
                } else {
                    ""
                };
            continue;
        }
        match section {
            "name" if !t.is_empty() => name.push(t.to_string()),
            "name" => section = "",
            "description" => desc.push(line.trim_end()),
            _ => {}
        }
    }
    Dsc {
        name,
        description: desc.join("\n").trim().to_string(),
    }
}

fn find_dsc(file: &Path, lang: &str) -> Option<Dsc> {
    dsc_candidates(file, lang)
        .iter()
        .find_map(|p| ::legacy_config::vfs::read(p).ok())
        .map(|b| parse_dsc(&encoding_latin1(&b)))
}

/// OMSI's text files are Latin-1, except that some description files were saved as
/// UTF-16 with a byte-order mark.
fn encoding_latin1(b: &[u8]) -> String {
    if b.len() >= 2 && b[0] == 0xFF && b[1] == 0xFE {
        let u: Vec<u16> = b[2..]
            .chunks_exact(2)
            .map(|c| u16::from_le_bytes([c[0], c[1]]))
            .collect();
        return String::from_utf16_lossy(&u);
    }
    if b.len() >= 2 && b[0] == 0xFE && b[1] == 0xFF {
        let u: Vec<u16> = b[2..]
            .chunks_exact(2)
            .map(|c| u16::from_be_bytes([c[0], c[1]]))
            .collect();
        return String::from_utf16_lossy(&u);
    }
    b.iter().map(|&c| c as char).collect()
}

// ---------------------------------------------------------------------------------------
// timetable: lines, tours, trips and the roadbook

#[derive(Serialize, Clone, Debug)]
pub struct StopInfo {
    pub name: String,
    pub arr: f64,
    pub dep: f64,
}

/// One trip of a tour, as OMSI's timetable dialog lists it: when it leaves, from where to
/// where, on which line, and the trip's (route's) own name.
#[derive(Serialize, Clone, Debug)]
pub struct TripInfo {
    /// The trip file's name ("4 Liman-ZS"): the name the map gives the route.
    pub name: String,
    /// Its place in the tour, 1 = the first (what `--trip` takes, as does its departure).
    pub index: usize,
    /// The line its displays show (a depot run has none).
    pub line: String,
    /// First and last stop.
    pub from: String,
    pub terminus: String,
    pub departure: f64,
    pub arrival: f64,
    pub stops: Vec<StopInfo>,
    pub km: f64,
}

#[derive(Serialize, Clone, Debug)]
pub struct TourInfo {
    pub number: String,
    pub ai_group: String,
    pub first: f64,
    pub last: f64,
    /// The days it runs, in words ("Mon-Fri", "Sat", "daily", ...), from its validity mask.
    pub days: String,
    /// It runs on the date asked for (the game's timetable has only these tours that day).
    pub runs: bool,
    /// The first date from the one asked for on which it runs (`YYYY-MM-DD`): the game's
    /// timetable dialog lists only the tours of the chosen day, so a
    /// tour of another day is picked by moving the date to it.
    pub next_run: Option<String>,
    pub trips: Vec<TripInfo>,
}

#[derive(Serialize, Clone, Debug)]
pub struct LineInfo {
    pub name: String,
    pub user_allowed: bool,
    pub termini: Vec<String>,
    pub tours: Vec<TourInfo>,
}

/// The date the game starts on without `--date` (its clock's default, day 150 of 1989), and
/// the launcher's own default date.
pub const DEFAULT_DATE: &str = "1989-05-30";

/// The lines of a map's timetable on `date` (`YYYY-MM-DD`, the game's default when empty):
/// the chrono folders active that day add their lines and take theirs off, as the game does
/// - Spandau's 1991 timetable change replaces line "5 & 5N" and sixteen others.
pub fn list_lines(map: &str, date: &str) -> Result<Vec<LineInfo>> {
    let map_dir = resolve_content(map)?
        .parent()
        .map(|p| p.to_path_buf())
        .context("map folder")?;
    lines_on(&map_dir, date)
}

fn lines_on(map_dir: &Path, date: &str) -> Result<Vec<LineInfo>> {
    let date = if date.trim().is_empty() {
        DEFAULT_DATE
    } else {
        date.trim()
    };
    let code = ::map::date_code(date)
        .with_context(|| format!("'{date}' is not a date (YYYY-MM-DD)"))?;
    let chrono = ::map::active_chrono_dirs(map_dir, code);
    let off = ::map::chrono_deactivated_lines(&chrono);
    let data = ::timetable::TimetableData::load_with_chrono(map_dir, &chrono, &off);
    // which tours run that day: the tour's mask as the game reads it (bits 0-6 Monday to
    // Sunday, 7 a public holiday, 8 school holidays, 9 school days: the original)
    let calendar = ::map::Calendar::load(&map_dir.join("Holidays.txt")).unwrap_or_default();
    let day_bit = if calendar.is_holiday(code) {
        1 << 7
    } else {
        1 << weekday(code)
    };
    let school_bit = if calendar.in_holiday_range(code) {
        1 << 8
    } else {
        1 << 9
    };
    let mut out = Vec::new();
    for l in &data.lines {
        let mut termini: Vec<String> = Vec::new();
        let mut tours = Vec::new();
        for t in &l.tours {
            let mask = t.extra.trim().parse::<i32>().unwrap_or(1023);
            let runs_on = |c: i32| {
                let day = if calendar.is_holiday(c) {
                    1 << 7
                } else {
                    1 << weekday(c)
                };
                let school = if calendar.in_holiday_range(c) {
                    1 << 8
                } else {
                    1 << 9
                };
                mask & day != 0 && mask & school != 0
            };
            let runs = mask & day_bit != 0 && mask & school_bit != 0;
            let next_run = (0..400)
                .map(|k| add_days(code, k))
                .find(|c| runs_on(*c))
                .map(|c| format!("{:04}-{:02}-{:02}", c / 10000, c / 100 % 100, c % 100));
            let mut trips = Vec::new();
            for tt in &t.trips {
                let Some(trip) = data
                    .trips
                    .iter()
                    .find(|x| x.name.eq_ignore_ascii_case(&tt.trip))
                else {
                    continue;
                };
                let departure = tt.departure as f64 * 60.0;
                let duration = trip
                    .profiles
                    .get(tt.profile.max(0) as usize)
                    .or(trip.profiles.first())
                    .map(|p| p.factor as f64 * 60.0)
                    .filter(|d| *d > 0.0)
                    .unwrap_or(600.0);
                // its stations: [station_typ2] objects, or the [station] records of a type-1
                // trip (all of Novi Sad), which carry their stop's name themselves
                let legacy: Vec<(i64, String)> = trip
                    .stations_legacy
                    .iter()
                    .filter_map(|r| {
                        Some((
                            r.first()?.trim().parse::<i64>().ok()?,
                            r.get(2).map(|n| n.trim().to_string()).unwrap_or_default(),
                        ))
                    })
                    .collect();
                let stations: Vec<i64> = if trip.stations.is_empty() {
                    legacy.iter().map(|x| x.0).collect()
                } else {
                    trip.stations.clone()
                };
                let mut lens = Vec::new();
                for w in stations.windows(2) {
                    lens.push(
                        data.stn_links
                            .iter()
                            .find(|k| k.from_id == w[0] && k.to_id == w[1])
                            .map(|k| k.length.max(1.0))
                            .unwrap_or(500.0),
                    );
                }
                let total: f64 = lens.iter().sum::<f64>().max(1.0);
                let mut acc = 0.0;
                let mut stops = Vec::new();
                for (i, id) in stations.iter().enumerate() {
                    let t_at = departure + duration * acc / total;
                    let name = data
                        .bus_stops
                        .iter()
                        .find(|b| b.object_id == *id)
                        .map(|b| b.name.trim().to_string())
                        .or_else(|| {
                            legacy
                                .iter()
                                .find(|x| x.0 == *id)
                                .map(|x| x.1.clone())
                                .filter(|n| !n.is_empty())
                        })
                        .unwrap_or_else(|| format!("stop {id}"));
                    stops.push(StopInfo {
                        name,
                        arr: t_at,
                        dep: if i == 0 { departure } else { t_at },
                    });
                    if i < lens.len() {
                        acc += lens[i];
                    }
                }
                if !trip.terminus.trim().is_empty()
                    && !termini.iter().any(|x| x == trip.terminus.trim())
                {
                    termini.push(trip.terminus.trim().to_string());
                }
                let from = stops.first().map(|s| s.name.clone()).unwrap_or_default();
                let index = trips.len() + 1;
                trips.push(TripInfo {
                    name: trip.name.clone(),
                    index,
                    line: trip.line.trim().to_string(),
                    from,
                    terminus: trip.terminus.trim().to_string(),
                    departure,
                    arrival: departure + duration,
                    stops,
                    km: total / 1000.0,
                });
            }
            // (a tour with no trip the timetable knows cannot be driven: not offered)
            if trips.is_empty() {
                continue;
            }
            let first = trips.first().map(|t| t.departure).unwrap_or(0.0);
            let last = trips.last().map(|t| t.arrival).unwrap_or(0.0);
            tours.push(TourInfo {
                number: t.number.clone(),
                ai_group: t.ai_group.clone(),
                first,
                last,
                days: days_of(mask),
                runs,
                next_run,
                trips,
            });
        }
        tours.sort_by(|a, b| a.first.total_cmp(&b.first));
        out.push(LineInfo {
            name: l.name.clone(),
            user_allowed: l.user_allowed,
            termini,
            tours,
        });
    }
    out.sort_by(|a, b| natural_key(&a.name).cmp(&natural_key(&b.name)));
    Ok(out)
}

/// A date code (YYYYMMDD) `k` days later.
fn add_days(code: i32, k: i32) -> i32 {
    let (mut y, mut m, mut d) = (code / 10000, code / 100 % 100, code % 100 + k);
    let len = |y: i32, m: i32| match m {
        2 if (y % 4 == 0 && y % 100 != 0) || y % 400 == 0 => 29,
        2 => 28,
        4 | 6 | 9 | 11 => 30,
        _ => 31,
    };
    while d > len(y, m) {
        d -= len(y, m);
        m += 1;
        if m > 12 {
            m = 1;
            y += 1;
        }
    }
    y * 10000 + m * 100 + d
}

/// Day of the week of a date code (YYYYMMDD): 0 = Monday … 6 = Sunday, as the game's clock.
fn weekday(code: i32) -> i32 {
    let (mut y, m, d) = (code / 10000, code / 100 % 100, code % 100);
    // Sakamoto's method (0 = Sunday)
    let t = [0, 3, 2, 5, 0, 3, 5, 1, 4, 6, 2, 4];
    if m < 3 {
        y -= 1;
    }
    let sunday0 =
        (y + y / 4 - y / 100 + y / 400 + t[((m - 1).clamp(0, 11)) as usize] + d).rem_euclid(7);
    (sunday0 + 6) % 7
}

/// A tour's validity mask in words: "Mon-Fri", "Sat", "Sun", "daily", ...
fn days_of(mask: i32) -> String {
    let week = mask & 0x7f;
    let names = ["Mon", "Tue", "Wed", "Thu", "Fri", "Sat", "Sun"];
    let mut out = match week {
        0x7f => "daily".to_string(),
        0x1f => "Mon-Fri".to_string(),
        0x3f => "Mon-Sat".to_string(),
        0x60 => "Sat-Sun".to_string(),
        0 => String::new(),
        w => (0..7)
            .filter(|i| w & (1 << i) != 0)
            .map(|i| names[i])
            .collect::<Vec<_>>()
            .join(", "),
    };
    if mask & (1 << 7) != 0 && week != 0x7f {
        out.push_str(if out.is_empty() {
            "holidays"
        } else {
            " & holidays"
        });
    }
    match (mask & (1 << 8) != 0, mask & (1 << 9) != 0) {
        (true, false) => out.push_str(", school holidays"),
        (false, true) => out.push_str(", school days"),
        _ => {}
    }
    out
}

fn natural_key(s: &str) -> (u64, String) {
    let digits: String = s.chars().take_while(|c| c.is_ascii_digit()).collect();
    (digits.parse().unwrap_or(u64::MAX), s.to_ascii_lowercase())
}

/// What the driver types into the IBIS for a line, from the bus's depot file: the line
/// code, and for each destination the route code and the terminus code.
#[derive(Serialize, Clone, Debug)]
pub struct IbisInfo {
    pub hof: String,
    pub line_code: String,
    pub routes: Vec<IbisRoute>,
}

#[derive(Serialize, Clone, Debug)]
pub struct IbisRoute {
    pub code: String,
    pub route: String,
    pub name: String,
    pub terminus_code: i32,
    pub terminus: String,
}

pub fn ibis_info(bus: &str, hof_name: &str, line: &str) -> Result<IbisInfo> {
    let bus_path = resolve_content(bus)?;
    let dir = bus_path.parent().context("bus folder")?;
    // the bus's own depot file of that name (every content root's copy of its folder), else
    // the one another bus brings (the game borrows it the same way), else the bus's first
    let hof = ::legacy_vehicle::hof::depot_in(dir, hof_name)
        .or_else(|| ::legacy_vehicle::hof::depot_anywhere(hof_name))
        .or_else(|| {
            ::legacy_vehicle::hof::depot_files(dir)
                .iter()
                .find_map(|f| ::legacy_vehicle::Hof::load(f).ok())
        })
        .context("no depot file next to the bus")?;
    let hof = &hof;
    let line_digits: String = line.chars().take_while(|c| c.is_ascii_digit()).collect();
    let mut routes = Vec::new();
    for t in &hof.info_trips {
        let matches = t.line.trim().eq_ignore_ascii_case(line.trim())
            || (!line_digits.is_empty()
            && t.code
            .trim_start_matches('0')
            .starts_with(line_digits.trim_start_matches('0'))
            && t.code.len() >= line_digits.len());
        if !matches {
            continue;
        }
        let code = ::legacy_config::parse_i32(&t.route);
        let terminus = hof
            .termini
            .iter()
            .find(|x| x.code == code)
            .and_then(|x| x.strings.first().cloned())
            .unwrap_or_default();
        routes.push(IbisRoute {
            code: t.code.clone(),
            route: t.route.clone(),
            name: t.name.clone(),
            terminus_code: code,
            terminus,
        });
    }
    Ok(IbisInfo {
        hof: hof.name.clone(),
        line_code: line_digits,
        routes,
    })
}

// ---------------------------------------------------------------------------------------
// profiles: the driver's personnel file plus the sessions the game writes

#[derive(Serialize, Deserialize, Clone, Default, Debug)]
pub struct Session {
    pub time: u64,
    pub driver: String,
    pub map: String,
    pub bus: String,
    pub line: Option<String>,
    pub tour: Option<String>,
    pub seconds: f64,
    pub metres: f64,
    pub stops: i32,
    pub early: i32,
    pub late: i32,
    pub tickets: i32,
    pub cash: f64,
    pub crashes: i32,
    pub hurt: i32,
    pub jolts: i32,
    pub driving: f64,
    pub comfort: f64,
    pub ticketing: f64,
}

#[derive(Serialize, Clone, Debug)]
pub struct Profile {
    pub name: String,
    pub file: String,
    pub hours: f64,
    pub km: f64,
    pub xp: i64,
    pub level: i64,
    pub next_level_xp: i64,
    pub stops: i32,
    pub early: i32,
    pub late: i32,
    pub tickets: f64,
    pub cash: f64,
    pub crashes: i32,
    pub hurt: i32,
    pub rating_driving: f64,
    pub rating_comfort: f64,
    pub rating_tickets: f64,
    pub sessions: Vec<Session>,
    pub exists: bool,
}

fn sessions() -> Vec<Session> {
    let dir = data_dir().join("sessions");
    let mut out: Vec<Session> = std::fs::read_dir(&dir)
        .map(|rd| {
            rd.flatten()
                .filter_map(|e| std::fs::read_to_string(e.path()).ok())
                .filter_map(|t| serde_json::from_str::<Session>(&t).ok())
                .collect()
        })
        .unwrap_or_default();
    out.sort_by(|a, b| b.time.cmp(&a.time));
    out
}

/// Experience: a point per hundred metres, five per stop served on time, two per ticket,
/// minus twenty per crash and fifty per pedestrian; the level grows with the square root.
fn xp_of(s: &Session) -> i64 {
    let on_time = (s.stops - s.early - s.late).max(0) as i64;
    let xp =
        (s.metres / 100.0) as i64 + on_time * 5 + s.tickets as i64 * 2 + (s.seconds / 60.0) as i64
            - s.crashes as i64 * 20
            - s.hurt as i64 * 50
            - s.jolts as i64;
    xp.max(0)
}

fn level_of(xp: i64) -> (i64, i64) {
    let level = ((xp as f64 / 250.0).sqrt().floor() as i64) + 1;
    let next = (level * level) as i64 * 250;
    (level, next)
}

/// Where a driver's personnel file is written: the content folder's `Drivers` (the
/// original installation is only ever read).
fn driver_write_path(root: &Path, name: &str) -> PathBuf {
    content_dir()
        .unwrap_or_else(|| root.to_path_buf())
        .join("Drivers")
        .join(format!("{name}.odr"))
}

/// Where a driver's personnel file is read: the content folder's copy once the game has
/// written one, else the original installation's (the stock `OMSI-Fan.odr`).
fn driver_read_path(root: &Path, name: &str) -> PathBuf {
    let own = driver_write_path(root, name);
    if own.exists() {
        own
    } else {
        root.join("Drivers").join(format!("{name}.odr"))
    }
}

pub fn list_profiles() -> Result<Vec<String>> {
    let root = root()?;
    let odr_names = |dir: PathBuf| -> Vec<String> {
        std::fs::read_dir(dir)
            .map(|rd| {
                rd.flatten()
                    .map(|e| e.path())
                    .filter(|p| {
                        p.extension()
                            .map(|e| e.eq_ignore_ascii_case("odr"))
                            .unwrap_or(false)
                    })
                    .filter_map(|p| p.file_stem().map(|s| s.to_string_lossy().to_string()))
                    .collect()
            })
            .unwrap_or_default()
    };
    let mut names = odr_names(root.join("Drivers"));
    if let Some(c) = content_dir() {
        names.extend(odr_names(c.join("Drivers")));
    }
    for s in sessions() {
        if !names.iter().any(|n| n.eq_ignore_ascii_case(&s.driver)) {
            names.push(s.driver.clone());
        }
    }
    names.sort();
    names.dedup();
    Ok(names)
}

pub fn get_profile(name: &str) -> Result<Profile> {
    let root = root()?;
    let name = name.trim();
    if name.is_empty() {
        return Err(anyhow!("a profile needs a name"));
    }
    let file = driver_read_path(&root, name);
    let driver = ::content::driver::Driver::load(&file).ok();
    let mine: Vec<Session> = sessions()
        .into_iter()
        .filter(|s| s.driver.eq_ignore_ascii_case(name))
        .collect();
    let xp: i64 = mine.iter().map(xp_of).sum();
    let (level, next) = level_of(xp);
    let hours = mine.iter().map(|s| s.seconds).sum::<f64>() / 3600.0;
    let (km, stops, early, late, tickets, cash, crashes, hurt, rating) = match &driver {
        Some(d) => (
            d.hektom / 10.0,
            d.bus_stops[0],
            d.bus_stops[1],
            d.bus_stops[2],
            d.tickets[0],
            d.tickets[1],
            d.crashes[0],
            d.crashes[1],
            [
                d.driving_percent(),
                d.comfort_percent().unwrap_or(100.0),
                d.ticket_percent().unwrap_or(100.0),
            ],
        ),
        None => {
            let km = mine.iter().map(|s| s.metres).sum::<f64>() / 1000.0;
            let n = mine.len().max(1) as f64;
            (
                km,
                mine.iter().map(|s| s.stops).sum(),
                mine.iter().map(|s| s.early).sum(),
                mine.iter().map(|s| s.late).sum(),
                mine.iter().map(|s| s.tickets as f64).sum(),
                mine.iter().map(|s| s.cash).sum(),
                mine.iter().map(|s| s.crashes).sum(),
                mine.iter().map(|s| s.hurt).sum(),
                [
                    mine.iter().map(|s| s.driving).sum::<f64>() / n,
                    mine.iter().map(|s| s.comfort).sum::<f64>() / n,
                    mine.iter().map(|s| s.ticketing).sum::<f64>() / n,
                ],
            )
        }
    };
    Ok(Profile {
        name: name.to_string(),
        file: format!("Drivers/{name}.odr"),
        hours,
        km,
        xp,
        level,
        next_level_xp: next,
        stops,
        early,
        late,
        tickets,
        cash,
        crashes,
        hurt,
        rating_driving: rating[0],
        rating_comfort: rating[1],
        rating_tickets: rating[2],
        sessions: mine.into_iter().take(40).collect(),
        exists: driver.is_some(),
    })
}

/// Create the personnel file for a new driver (OMSI's own `.odr` format), so that the game
/// can add every run to it.
pub fn create_profile(name: &str, sex: &str) -> Result<Profile> {
    let root = root()?;
    let name = name.trim();
    if name.is_empty() || name.contains(['/', '\\', ':']) {
        return Err(anyhow!("the name must be a plain file name"));
    }
    let file = driver_write_path(&root, name);
    if let Some(dir) = file.parent() {
        std::fs::create_dir_all(dir)?;
    }
    if !file.exists() && !driver_read_path(&root, name).exists() {
        let d = ::content::driver::Driver {
            path: file.clone(),
            name: name.to_string(),
            sex: if sex.trim().is_empty() {
                "M".into()
            } else {
                sex.trim().to_string()
            },
            ..Default::default()
        };
        d.save(&file)?;
    }
    let mut c = load_config();
    c.profile = name.to_string();
    save_config(&c)?;
    get_profile(name)
}

/// Delete a driver's personnel file from the content folder. A driver who exists only in
/// the original installation is left there: the original is never written.
pub fn delete_profile(name: &str) -> Result<()> {
    let root = root()?;
    let file = driver_write_path(&root, name.trim());
    if file.exists() {
        std::fs::remove_file(&file)?;
    } else if root
        .join("Drivers")
        .join(format!("{}.odr", name.trim()))
        .exists()
    {
        return Err(anyhow!(
            "'{}' belongs to the original OMSI 2 installation, which is not changed",
            name.trim()
        ));
    }
    Ok(())
}

// ---------------------------------------------------------------------------------------
// key bindings: the content folder's Inputs/keyboard.cfg once the page has saved one (the
// game reads that first), else the original installation's, which is never written

fn keyboard_cfg_write_path() -> Result<PathBuf> {
    let cand = content_dir()
        .unwrap_or(root()?)
        .join("Inputs")
        .join("keyboard.cfg");
    if let Some(p) = cand.parent() {
        if (p.exists() || std::fs::create_dir_all(p).is_ok()) && ::legacy_config::is_writable(p) {
            return Ok(cand);
        }
    }
    let fallback = data_dir().join("Inputs").join("keyboard.cfg");
    if let Some(p) = fallback.parent() {
        let _ = std::fs::create_dir_all(p);
    }
    Ok(fallback)
}

fn keyboard_cfg_read_path() -> Result<PathBuf> {
    let own = keyboard_cfg_write_path()?;
    if own.exists() {
        return Ok(own);
    }
    let fallback = data_dir().join("Inputs").join("keyboard.cfg");
    if fallback.exists() {
        return Ok(fallback);
    }
    Ok(::legacy_config::original_keyboard_cfg(&root()?))
}

/// The `keyboard.cfg` the launcher saved, if there is one: where [`save_keybindings`] writes it.
pub fn saved_keyboard_cfg() -> Option<PathBuf> {
    let own = keyboard_cfg_write_path().ok()?;
    if own.exists() {
        return Some(own);
    }
    let fallback = data_dir().join("Inputs").join("keyboard.cfg");
    fallback.exists().then_some(fallback)
}

fn binding_to_json(b: &::content::input::KeyBinding) -> Value {
    json!({ "action": b.action, "scan_code": b.scan_code, "modifier": b.modifier })
}

fn binding_from_json(v: &Value) -> Option<::content::input::KeyBinding> {
    Some(::content::input::KeyBinding {
        action: v.get("action")?.as_str()?.to_string(),
        scan_code: v.get("scan_code")?.as_i64()? as i32,
        modifier: v.get("modifier").and_then(|x| x.as_i64()).unwrap_or(0) as i32,
    })
}

pub fn get_keybindings() -> Result<Value> {
    let path = keyboard_cfg_read_path()?;
    let k = ::content::input::KeyboardCfg::load(&path)?
        .with_game_defaults()
        .with_vr_defaults();
    Ok(
        json!({ "game": k.game.iter().map(binding_to_json).collect::<Vec<_>>(), "vehicles": k.vehicles.iter().map(binding_to_json).collect::<Vec<_>>() }),
    )
}

/// Replace the bindings with the page's list. Written to a temp file and read back through
/// the same loader the game uses before it replaces the real file, so a page bug never
/// leaves the player with a `keyboard.cfg` the game itself cannot parse.
pub fn save_keybindings(v: &Value) -> Result<()> {
    let list = |k: &str| -> Vec<::content::input::KeyBinding> {
        v.get(k)
            .and_then(|x| x.as_array())
            .map(|a| a.iter().filter_map(binding_from_json).collect())
            .unwrap_or_default()
    };
    let k = ::content::input::KeyboardCfg {
        game: list("game"),
        vehicles: list("vehicles"),
    };
    let path = keyboard_cfg_write_path()?;
    if let Some(dir) = path.parent() {
        std::fs::create_dir_all(dir)?;
    }
    let tmp = path.with_extension("cfg.tmp");
    k.save(&tmp)?;
    ::content::input::KeyboardCfg::load(&tmp)?;
    std::fs::rename(&tmp, &path)?;
    Ok(())
}

// ---------------------------------------------------------------------------------------
// settings (`::config`: settings.toml)

pub fn get_settings() -> Result<Value> {
    let mut v = current_settings();
    // no settings of our own yet: start from what the player set in OMSI 2
    if !::config::default_path().exists() {
        if let Some(o) = root().ok().and_then(|r| omsi_options(&r)) {
            if let (Some(dst), Some(src)) = (v.as_object_mut(), o.settings.as_object()) {
                dst.extend(src.clone());
            }
            v["imported_from_omsi"] = json!(true);
        }
    }
    // what "automatic" texture memory is on this machine (the game takes an eighth of it)
    v["texture_memory_auto"] = json!(physical_memory().map(|m| m / 8 / 1_000_000).unwrap_or(2000));
    Ok(v)
}

/// Load the settings file into `::config` (the game does the same at its start).
pub fn init_settings() {
    if let Err(e) = ::config::init(::config::default_path()) {
        eprintln!("settings not loaded: {e}");
    }
}

use ::config::Value as Toml;

/// How a flat key of the page is kept in `::config`.
enum Kind {
    Bool,
    Int(i64, i64),
    Float(f64, f64),
    /// a number the page's select gives as a string
    FloatText(f64, f64),
    /// "auto" in the page, this value (or less) in the config
    Auto(f64),
    /// a factor in the config, percent in the page
    Percent,
    Mirror,
    Choice(&'static [&'static str]),
    Text,
    Language,
    Graphics,
    Station,
}

use Kind::*;

/// (page key, config category, config key, kind)
const SETTINGS: &[(&str, &str, &str, Kind)] = &[
    ("msaa", "graphics", "msaa", Int(0, 16)),
    ("anisotropy", "graphics", "anisotropy", Int(1, 16)),
    ("ssao", "graphics", "ssao", Bool),
    ("shadows", "graphics", "shadows", Bool),
    ("shadow_size", "graphics", "shadow_size", Int(0, i64::MAX)),
    ("detail_textures", "graphics", "detail_textures", Bool),
    ("window_mode", "graphics", "window_mode", Choice(&["windowed", "borderless", "fullscreen"])),
    ("vsync", "graphics", "vsync", Bool),
    ("texture_memory", "graphics", "texture_memory", Int(0, i64::MAX)),
    ("texture_compression", "graphics", "texture_compression", Bool),
    ("clouds", "graphics", "clouds", Bool),
    ("mirror_size", "graphics", "mirror_size", Mirror),
    ("mirror_refresh", "graphics", "mirror_refresh", Choice(&["full", "eco", "off"])),
    ("max_fps", "graphics", "max_fps", Int(0, i64::MAX)),
    ("min_obj_size", "graphics", "min_obj_size", Float(0.0, f64::MAX)),
    ("max_obj_dist", "graphics", "max_obj_dist", Auto(-1.0)),
    ("view_distance", "graphics", "view_distance", Auto(0.0)),
    ("render_scale", "graphics", "render_scale", Auto(0.0)),
    ("shadow_casters", "graphics", "shadow_casters", Choice(&["all", "omsi"])),
    ("shadow_blobs", "graphics", "shadow_blobs", Bool),
    ("reflections", "graphics", "reflections", Bool),
    ("graphics_api", "graphics", "graphics_api", Choice(&["auto", "vulkan", "dx12"])),
    ("graphics", "graphics", "graphics", Graphics),
    ("led_glow", "graphics", "led_glow", Int(0, 15)),
    ("nightmap_glow", "graphics", "nightmap_glow", Int(0, 15)),
    ("led_mips", "graphics", "led_mips", Float(0.0, 4.0)),
    ("atmosphere_brightness", "graphics", "atmosphere_brightness", Float(0.0, 2.0)),
    ("map_detail", "graphics", "map_detail", Int(-1, 255)),
    ("navigator", "ui", "navigator", Bool),
    ("ui_opacity", "ui", "opacity", Float(0.0, 1.0)),
    ("navigator_corner", "ui", "navigator_corner", Text),
    ("ui_scale", "ui", "scale", Float(0.5, 2.0)),
    ("ui_scale_window", "ui", "scale_window", Bool),
    ("notes", "ui", "notes", Bool),
    ("show_fps", "ui", "show_fps", Bool),
    ("chat", "ui", "chat", Bool),
    ("tooltips", "ui", "tooltips", Bool),
    ("name_tags", "ui", "name_tags", Bool),
    ("language", "ui", "language", Language),
    ("units", "ui", "units", Choice(&["metric", "uk", "imperial"])),
    ("boarding", "gameplay", "boarding", Text),
    ("pax_prefer_seats", "gameplay", "pax_prefer_seats", Bool),
    ("pax_rear_entry", "gameplay", "pax_rear_entry", Bool),
    ("exact_fare", "gameplay", "exact_fare", Bool),
    ("driver", "gameplay", "driver", Bool),
    ("maintenance", "gameplay", "maintenance", Int(0, 4)),
    ("collision_vehicles", "gameplay", "collision_vehicles", Bool),
    ("collision_objects", "gameplay", "collision_objects", Bool),
    ("collision_pedestrians", "gameplay", "collision_pedestrians", Bool),
    ("hands_in_cab", "gameplay", "hands_in_cab", Bool),
    ("time_speed", "gameplay", "time_speed", FloatText(1.0, 30.0)),
    ("time_sync", "gameplay", "time_sync", Bool),
    ("metar_sync", "gameplay", "metar_sync", Bool),
    ("metar_station", "gameplay", "metar_station", Station),
    ("auto_clutch", "gameplay", "auto_clutch", Bool),
    ("auto_ibis", "gameplay", "auto_ibis", Bool),
    ("momentary_gears", "gameplay", "momentary_gears", Bool),
    ("auto_shift", "gameplay", "auto_shift", Bool),
    ("steering_linear", "controls", "steering_linear", Bool),
    ("old_steering", "controls", "old_steering", Bool),
    ("red_steer_spd", "controls", "red_steer_spd", Bool),
    ("mouse_sens", "controls", "mouse_sens", Float(0.1, 3.0)),
    ("stick_sens", "controls", "stick_sens", Float(0.1, 2.0)),
    ("steer_center", "controls", "steer_center", Bool),
    ("brake_hold", "controls", "brake_hold", Bool),
    ("mouse_steering", "controls", "mouse_steering", Bool),
    ("mouse_right_off", "controls", "mouse_right_off", Bool),
    ("blinker_cancel", "controls", "blinker_cancel", Bool),
    ("wheel_range", "controls", "wheel_range", Float(90.0, 2880.0)),
    ("wheel_lock", "controls", "wheel_lock", Float(0.0, 2880.0)),
    ("pedal_throttle", "controls", "pedal_throttle", Float(0.25, 4.0)),
    ("pedal_brake", "controls", "pedal_brake", Float(0.25, 4.0)),
    ("ai_unsched_factor", "ai", "unsched_factor", Percent),
    ("ai_max_scheduled", "ai", "max_scheduled", Int(0, i64::MAX)),
    ("ai_max_parked", "ai", "max_parked", Int(-1, i64::MAX)),
    ("nav_arrows", "navigator", "arrows", Bool),
    ("nav_ai", "navigator", "ai", Bool),
    ("nav_topbar", "navigator", "topbar", Bool),
    ("nav_turn", "navigator", "turn", Bool),
    ("nav_stoplist", "navigator", "stoplist", Bool),
    ("nav_stops_ext", "navigator", "stops_ext", Bool),
    ("pax_voices", "passengers", "voices", Choice(&["all", "tickets", "off"])),
    ("pax_models", "passengers", "models", Choice(&["omsi", "realistic"])),
    ("pax_motion", "passengers", "motion", Choice(&["natural", "omsi"])),
    ("pax_ik", "passengers", "ik", Bool),
    ("pax_density", "passengers", "density", Float(0.0, 5.0)),
    ("discord_status", "discord", "status", Bool),
    ("discord_app_id", "discord", "app_id", Text),
    ("volume", "audio", "master-volume", Float(0.0, 1.0)),
    ("vol_ai", "audio", "ai-volume", Float(0.0, 1.0)),
    ("vol_scenery", "audio", "scenery-volume", Float(0.0, 1.0)),
    ("doppler", "audio", "doppler", Bool),
    ("ff_enabled", "controller", "ff_enabled", Bool),
    ("ff_invert", "controller", "ff_invert", Bool),
    ("ctrl_assign", "controller", "assign", Text),
    ("fov", "camera", "fov", Float(0.0, 120.0)),
    ("camera_collision", "camera", "collision", Bool),
    ("driverview_smooth", "camera", "smooth", Bool),
    ("head_movement", "camera", "head_movement", Bool),
    ("steer_look", "camera", "steer_look", Bool),
    ("steer_look_angle", "camera", "steer_look_angle", Float(0.0, 60.0)),
    ("steer_look_response", "camera", "steer_look_response", Float(0.05, 1.0)),
    ("seat_x", "camera", "seat_x", Float(-1.5, 1.5)),
    ("seat_y", "camera", "seat_y", Float(-1.5, 1.5)),
    ("seat_z", "camera", "seat_z", Float(-1.5, 1.5)),
    ("look_sens", "camera", "look_sens", Float(0.1, 2.0)),
    ("alt_view", "camera", "alt_view", Bool),
    ("free_look", "camera", "free_look", Bool),
    ("crosshair", "camera", "crosshair", Bool),
    ("head_tracking", "camera", "head_tracking", Bool),
    ("vr", "vr", "enabled", Bool),
    ("vr_scale", "vr", "scale", Float(0.5, 1.0)),
    ("vr_head_smoothing_ms", "vr", "head-smoothing-ms", Float(0.0, 30.0)),
    ("vr_mirror_rate", "vr", "mirror-rate", Float(-1.0, 360.0)),
    ("vr_desktop_mirror", "vr", "desktop-mirror", Bool),
    ("update_check", "launcher", "update_check", Bool),
    ("update_auto", "launcher", "update_auto", Bool),
];

fn toml_num(v: &Toml) -> Option<f64> {
    v.as_float().or_else(|| v.as_integer().map(|i| i as f64))
}

fn page_num(v: &Value) -> Option<f64> {
    v.as_f64()
        .or_else(|| v.as_str().and_then(|s| s.trim().parse().ok()))
        .filter(|x| x.is_finite())
}

fn metar_station(s: &str) -> String {
    s.chars()
        .filter(|c| c.is_ascii_alphabetic())
        .take(4)
        .collect::<String>()
        .to_ascii_uppercase()
}

fn choice(options: &[&str], s: &str) -> String {
    let s = s.trim().to_ascii_lowercase();
    options
        .iter()
        .find(|o| **o == s)
        .unwrap_or(&options[0])
        .to_string()
}

fn mirror_size(x: f64) -> i64 {
    match x as i64 {
        0 => 0,
        x => x.clamp(64, 2048),
    }
}

fn to_page(kind: &Kind, v: &Toml) -> Option<Value> {
    Some(match kind {
        Bool => json!(v.as_bool()?),
        Int(lo, hi) => json!((toml_num(v)? as i64).clamp(*lo, *hi)),
        Float(lo, hi) => json!(toml_num(v)?.clamp(*lo, *hi)),
        FloatText(lo, hi) => json!(toml_num(v)?.clamp(*lo, *hi).to_string()),
        Auto(off) => {
            let x = toml_num(v)?;
            if x <= *off {
                json!("auto")
            } else {
                json!(x.to_string())
            }
        }
        Percent => json!((toml_num(v)? * 100.0).round().clamp(0.0, 300.0) as i64),
        Mirror => json!(mirror_size(toml_num(v)?)),
        Choice(options) => json!(choice(options, v.as_str()?)),
        Text => json!(v.as_str()?),
        Language => json!(language_code(v.as_str()?)),
        Graphics => json!(graphics_mode(v.as_str()?)),
        Station => json!(metar_station(v.as_str()?)),
    })
}

fn to_config(kind: &Kind, v: &Value) -> Option<Toml> {
    Some(match kind {
        Bool => Toml::Boolean(v.as_bool()?),
        Int(lo, hi) => Toml::Integer((page_num(v)? as i64).clamp(*lo, *hi)),
        Float(lo, hi) | FloatText(lo, hi) => Toml::Float(page_num(v)?.clamp(*lo, *hi)),
        Auto(off) => Toml::Float(page_num(v).filter(|x| x > off).unwrap_or(*off)),
        Percent => Toml::Float(page_num(v)?.clamp(0.0, 300.0) / 100.0),
        Mirror => Toml::Integer(mirror_size(page_num(v)?)),
        Choice(options) => Toml::String(choice(options, v.as_str()?)),
        Text => Toml::String(v.as_str()?.to_string()),
        Language => Toml::String(language_code(v.as_str()?).to_string()),
        Graphics => Toml::String(graphics_mode(v.as_str()?).to_string()),
        Station => Toml::String(metar_station(v.as_str()?)),
    })
}

fn page_settings(get: impl Fn(&str, &str) -> Option<Toml>) -> Value {
    let mut v = serde_json::Map::new();
    for (flat, cat, key, kind) in SETTINGS {
        if let Some(x) = get(cat, key).and_then(|x| to_page(kind, &x)) {
            v.insert(flat.to_string(), x);
        }
    }
    let mut v = Value::Object(v);
    v["enhanced"] = json!(v["graphics"] == "enhanced");
    v
}

/// The page's view of the config's defaults.
pub fn default_settings() -> Value {
    let defaults = ::config::defaults();
    page_settings(|cat, key| defaults.get(cat)?.get(key).cloned())
}

/// The page's view of the settings as `::config` holds them now.
pub fn current_settings() -> Value {
    page_settings(::config::get_setting)
}

/// Put the page's values into `::config` (the keys the page does not manage stay).
pub fn apply_settings(v: &Value) {
    for (flat, cat, key, kind) in SETTINGS {
        if let Some(x) = v.get(*flat).and_then(|x| to_config(kind, x)) {
            ::config::set_setting(cat, key, x);
        }
    }
}

/// The interface's languages: the settings' code (OMSI's three-letter style), the name in
/// the language itself, the interface tables' code, and other spellings a file may use.
/// OMSI's own texts (key names, `.dsc` descriptions, tutorials) exist in English, German
/// and French: every other language shows those in English.
pub const LANGUAGES: &[(&str, &str, &str, &[&str])] = &[
    ("en", "English", "en", &["en", "english"]),
    ("de", "Deutsch", "de", &["de", "ger", "german", "deutsch"]),
];

/// The settings' language code from any spelling the game accepts (English when unknown).
pub fn language_code(s: &str) -> &'static str {
    let s = s.trim().to_lowercase();
    LANGUAGES
        .iter()
        .find(|(code, _, _, aliases)| {
            code.eq_ignore_ascii_case(&s) || aliases.iter().any(|a| *a == s)
        })
        .map(|l| l.0)
        .unwrap_or("en")
}

/// The interface tables' code of a language (`ru`, `ja` ...; empty for English).
pub fn language_iso(code: &str) -> &'static str {
    let c = language_code(code);
    LANGUAGES
        .iter()
        .find(|l| l.0 == c)
        .map(|l| if l.2 == "en" { "" } else { l.2 })
        .unwrap_or("")
}

/// `vanilla` (as OMSI 2), `vanilla_plus` or `enhanced`, from the ways a file may spell them
/// (as the game's `settings::graphics_mode`).
pub fn graphics_mode(v: &str) -> &'static str {
    match v
        .trim()
        .to_ascii_lowercase()
        .replace(['-', ' '], "_")
        .as_str()
    {
        "enhanced" | "1" => "enhanced",
        "vanilla" | "classic" | "original" | "omsi" | "omsi2" | "omsi_2" => "vanilla",
        _ => "vanilla_plus",
    }
}

/// How often the mirrors are drawn: `off`, `eco` or `full`, also from OMSI's
/// `performance_realreflexions` (none, economy, full).
fn mirror_refresh(x: &str) -> &'static str {
    match x.trim().to_ascii_lowercase().as_str() {
        "off" | "none" => "off",
        "eco" | "economy" => "eco",
        _ => "full",
    }
}

/// OMSI's tutorials (`Tutorials/menu_<n>_<LANG>.html`): per lesson its title and what
/// it teaches, in the settings' language.
pub fn tutorials() -> Vec<(usize, String, String)> {
    let Ok(r) = root() else { return Vec::new() };
    tutorials_in(&r, content_language())
}

fn tutorials_in(r: &Path, lang: &str) -> Vec<(usize, String, String)> {
    let mut out = Vec::new();
    for n in 1..=4usize {
        let p = [lang, "en", "de"]
            .iter()
            .map(|l| r.join("Tutorials").join(format!("menu_{n}_{}.html", omsi_suffix(l))))
            .find(|p| p.is_file());
        let Some(p) = p else { continue };
        let Ok(bytes) = std::fs::read(&p) else {
            continue;
        };
        let html = ::legacy_config::codepage::decode(&bytes);
        let body = html.split_once("</style>").map(|x| x.1).unwrap_or(&html);
        let mut text = String::new();
        let mut tag = false;
        for c in body
            .replace("</p>", "\n")
            .replace("<br>", "\n")
            .replace("</h2>", "\n")
            .replace("<li>", "\n• ")
            .chars()
        {
            match c {
                '<' => tag = true,
                '>' => tag = false,
                _ if !tag => text.push(c),
                _ => {}
            }
        }
        let text = text
            .replace("&quot;", "\"")
            .replace("&amp;", "&")
            .replace("&nbsp;", " ");
        let mut lines = text
            .lines()
            .map(|l| l.split_whitespace().collect::<Vec<_>>().join(" "))
            .filter(|l| !l.is_empty());
        let title = lines.next().unwrap_or_default();
        let rest: Vec<String> = lines.collect();
        out.push((n, title, rest.join("\n")));
    }
    out
}

/// OMSI's option presets (`option_presets/*.oop`): their names and what they say, in the
/// settings' own keys (maxFPS, performance_minObjSize/maxObjDist, texFilter, texmemlimit,
/// performance_reflTexSize).
pub fn option_presets() -> Vec<(String, Value)> {
    let Ok(r) = root() else { return Vec::new() };
    let mut out = Vec::new();
    let Ok(dir) = std::fs::read_dir(r.join("option_presets")) else {
        return out;
    };
    let mut files: Vec<PathBuf> = dir
        .flatten()
        .map(|e| e.path())
        .filter(|p| {
            p.extension()
                .map(|e| e.eq_ignore_ascii_case("oop"))
                .unwrap_or(false)
        })
        .collect();
    files.sort();
    for f in files {
        let Ok(o) = ::content::options::Options::load(&f) else {
            continue;
        };
        let name = f
            .file_stem()
            .unwrap_or_default()
            .to_string_lossy()
            .to_string();
        let mut v = json!({});
        if !cfg!(target_os = "android") {
            v["max_fps"] = json!(o.i32("maxfps", 0).max(0));
        }
        v["map_detail"] = json!(o.i32("maxcomplexity_map", 2).clamp(0, 255));
        v["min_obj_size"] = json!(o.f32("performance_minobjsize", 0.013) as f64);
        v["max_obj_dist"] =
            json!((o.f32("performance_maxobjdist", 900.0).round() as i64).to_string());
        if let Some(af) = o
            .values
            .get("texfilter")
            .and_then(|x| x.get(1))
            .and_then(|x| x.parse::<i64>().ok())
        {
            v["anisotropy"] = json!(af.clamp(1, 16));
        }
        let mem = o.f32("texmemlimit", 0.0);
        if mem > 0.0 {
            v["texture_memory"] = json!(mem as i64);
        }
        let refl = o.i32("performance_refltexsize", 8);
        v["mirror_size"] = json!(1i64 << refl.clamp(6, 11));
        if let Some(x) = o.str("performance_realreflexions") {
            v["mirror_refresh"] = json!(mirror_refresh(x));
        }
        out.push((name, v));
    }
    out
}

/// The machine's memory in bytes.
#[cfg(unix)]
fn physical_memory() -> Option<u64> {
    // SAFETY: sysconf only reads system values
    let (pages, size) = unsafe {
        (
            libc::sysconf(libc::_SC_PHYS_PAGES),
            libc::sysconf(libc::_SC_PAGESIZE),
        )
    };
    (pages > 0 && size > 0).then(|| pages as u64 * size as u64)
}

#[cfg(not(unix))]
fn physical_memory() -> Option<u64> {
    None
}

pub fn save_settings(v: &Value) -> Result<()> {
    apply_settings(v);
    ::config::save()?;
    Ok(())
}

/// The settings a graphics profile holds: what the Graphics tab shows, except the machine's
/// own (fullscreen, graphics API).
pub const GRAPHICS_PROFILE_KEYS: [&str; 23] = [
    "graphics",
    "msaa",
    "render_scale",
    "anisotropy",
    "shadow_size",
    "ssao",
    "shadows",
    "shadow_casters",
    "detail_textures",
    "led_glow",
    "nightmap_glow",
    "led_mips",
    "atmosphere_brightness",
    "reflections",
    "clouds",
    "vsync",
    "max_fps",
    "view_distance",
    "max_obj_dist",
    "min_obj_size",
    "mirror_size",
    "texture_memory",
    "texture_compression",
];

fn graphics_profiles_path() -> PathBuf {
    data_dir().join("graphics_profiles.json")
}

/// The saved graphics profiles by name (`~/.neoomsi/graphics_profiles.json`).
pub fn graphics_profiles() -> std::collections::BTreeMap<String, Value> {
    std::fs::read_to_string(graphics_profiles_path())
        .ok()
        .and_then(|t| serde_json::from_str(&t).ok())
        .unwrap_or_default()
}

/// Keep the graphics of `settings` as profile `name` (an existing one of that name is
/// replaced). Returns the name as kept.
pub fn save_graphics_profile(name: &str, settings: &Value) -> Result<String> {
    let name: String = name
        .chars()
        .filter(|c| !c.is_control())
        .collect::<String>()
        .trim()
        .chars()
        .take(40)
        .collect();
    if name.is_empty() {
        return Err(anyhow!("Give the profile a name."));
    }
    let mut profile = serde_json::Map::new();
    for k in GRAPHICS_PROFILE_KEYS {
        if let Some(x) = settings.get(k) {
            profile.insert(k.to_string(), x.clone());
        }
    }
    let mut all = graphics_profiles();
    all.insert(name.clone(), Value::Object(profile));
    std::fs::write(
        graphics_profiles_path(),
        serde_json::to_string_pretty(&all)?,
    )?;
    Ok(name)
}

/// Remove profile `name`.
pub fn delete_graphics_profile(name: &str) -> Result<()> {
    let mut all = graphics_profiles();
    all.remove(name);
    std::fs::write(
        graphics_profiles_path(),
        serde_json::to_string_pretty(&all)?,
    )?;
    Ok(())
}

/// Put a profile's values into the page's `settings` (only the keys a profile may hold).
pub fn apply_graphics_profile(profile: &Value, settings: &mut Value) {
    for k in GRAPHICS_PROFILE_KEYS {
        if let Some(x) = profile.get(k) {
            settings[k] = x.clone();
        }
    }
}

// ---------------------------------------------------------------------------------------
// the 3D preview and launching the game

/// Ask the game to write the bus as glTF (cached by bus and paint) and return the path.
pub fn bus_preview(bus: &str, paint: &str) -> Result<String> {
    let c = load_config();
    let game = find_game(&c.game).context("the game binary was not found (set it under Setup)")?;
    let root = root()?;
    let key: String = format!("{bus}|{paint}")
        .chars()
        .map(|ch| if ch.is_ascii_alphanumeric() { ch } else { '_' })
        .collect();
    let out = data_dir().join("cache").join(format!("{key}.glb"));
    let bus_path = resolve_content(bus)?;
    // a bus inside an archive used in place changes with the archive
    let bus_path = ::legacy_config::vfs::archive_of(&bus_path).unwrap_or(bus_path);
    let newest_input = [&bus_path, &game]
        .iter()
        .filter_map(|p| p.metadata().and_then(|m| m.modified()).ok())
        .max();
    let fresh = out
        .metadata()
        .and_then(|m| m.modified())
        .ok()
        .zip(newest_input)
        .map(|(o, b)| o >= b)
        .unwrap_or(false);
    if !fresh {
        let mut cmd = std::process::Command::new(&game);
        cmd.stdin(std::process::Stdio::null())
            .arg("--root")
            .arg(&root)
            .arg("--bus")
            .arg(bus)
            .arg("--export-glb")
            .arg(&out);
        if !paint.trim().is_empty() {
            cmd.arg("--paint").arg(paint.trim());
        }
        let status = cmd
            .env("RUST_LOG", "warn")
            .status()
            .context("running the game for the preview")?;
        if !status.success() || !out.exists() {
            return Err(anyhow!("the game could not export {bus}"));
        }
    }
    Ok(out.to_string_lossy().to_string())
}

#[derive(Deserialize, Default, Debug, Clone)]
pub struct Duty {
    /// Transient Discord session identity passed by the desktop launcher, never saved.
    #[serde(skip)]
    pub discord_session_start: Option<u64>,
    pub map: String,
    pub bus: String,
    pub paint: Option<String>,
    /// The number plate (registration) the player typed: it wins over the plate the bus's
    /// `[number]` list or the map's `registrations.txt` gives it (empty: as the content says).
    #[serde(default)]
    pub plate: Option<String>,
    /// The fleet number picked from the bus's `[number]` list (none: its first).
    #[serde(default)]
    pub number: Option<String>,
    pub hof: Option<String>,
    pub entry: Option<i32>,
    pub line: Option<String>,
    pub tour: Option<String>,
    /// The trip of the tour to start with: its departure (HH:MM) or its place in the tour.
    #[serde(default)]
    pub trip: Option<String>,
    #[serde(default)]
    pub whole_tour: bool,
    /// HH:MM
    pub time: String,
    /// YYYY-MM-DD
    pub date: Option<String>,
    pub weather: Option<String>,
    pub traffic: Option<u32>,
    pub passengers: Option<bool>,
    pub schedule: Option<bool>,
    pub autostart: Option<bool>,
    /// Start as a pedestrian beside the bus (which is then left out): a bus is placed or
    /// taken over from the game menu later.
    #[serde(default)]
    pub on_foot: Option<bool>,
    pub profile: Option<String>,
    /// LAN play: "host", or "join:<session code | ip[:port] | port | empty = search>".
    pub lan: Option<String>,
    /// The name the other LAN players see (default: the profile).
    pub lan_name: Option<String>,
    /// Season override: spring / summer / autumn / winter (empty = by date).
    pub season: Option<String>,
    /// One of OMSI's tutorials (1..4): its own situation, nothing else of the duty.
    #[serde(default)]
    pub tutorial: Option<usize>,
    /// A situation file to continue (the map's `laststn.osn`): nothing else of the duty.
    #[serde(default)]
    pub situation: Option<String>,
    #[serde(skip)]
    pub again: Option<Vec<String>>,
}

impl Duty {
    pub fn again(i: &Instance) -> Duty {
        Duty {
            map: i.map.clone(),
            bus: i.bus.clone(),
            entry: i.entry,
            line: i.line.clone(),
            tour: i.tour.clone(),
            profile: Some(i.profile.clone()).filter(|p| !p.is_empty()),
            lan: Some(i.lan.clone()),
            again: Some(i.args.clone()),
            ..Default::default()
        }
    }

    fn args(&self) -> Result<Vec<String>> {
        match &self.again {
            Some(a) => Ok(a.clone()),
            None => duty_args(self),
        }
    }
}

#[cfg(test)]
mod discord_session_tests {
    #[test]
    fn session_start_is_not_loaded_from_saved_duties() {
        let duty: super::Duty = serde_json::from_value(serde_json::json!({
            "map": "Map", "bus": "Bus", "time": "09:00",
            "discord_session_start": 1234,
        }))
            .unwrap();
        assert_eq!(duty.discord_session_start, None);
    }
}

/// The situation the game left on `map` last (`laststn.osn` in the map's folder: the
/// content folder's copy first, then OMSI 2's own), if there is one.
pub fn last_situation(map: &str) -> Option<PathBuf> {
    let dir = Path::new(&map.replace('\\', "/")).parent()?.to_path_buf();
    content_dir()
        .map(|c| c.join(&dir).join("laststn.osn"))
        .into_iter()
        .chain(root().ok().map(|r| r.join(&dir).join("laststn.osn")))
        .find(|p| p.is_file())
}

/// A situation saved on a map to continue from: the file, its `[name]`, when it was written
/// (seconds since 1970).
#[derive(Debug, Clone, PartialEq)]
pub struct SavedSituation {
    pub file: PathBuf,
    pub name: String,
    pub saved: u64,
}

/// What can be continued on `map` (#341): the last situation, then the save slots the game
/// writes into `Saves` of the map's folder in the content folder, the newest first.
pub fn saved_situations(map: &str) -> Vec<SavedSituation> {
    let mut out: Vec<SavedSituation> = last_situation(map)
        .map(|f| SavedSituation {
            saved: modified_secs(&f),
            file: f,
            name: "Last situation".into(),
        })
        .into_iter()
        .collect();
    if let (Some(dir), Some(c)) = (Path::new(&map.replace('\\', "/")).parent(), content_dir()) {
        out.extend(save_slots(&c.join(dir).join("Saves")));
    }
    out
}

fn modified_secs(p: &Path) -> u64 {
    std::fs::metadata(p)
        .and_then(|m| m.modified())
        .ok()
        .and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok())
        .map(|d| d.as_secs())
        .unwrap_or(0)
}

/// The situations in a map's `Saves` folder, the newest first, by their `[name]`.
fn save_slots(dir: &Path) -> Vec<SavedSituation> {
    let mut slots: Vec<SavedSituation> = std::fs::read_dir(dir)
        .into_iter()
        .flatten()
        .flatten()
        .map(|e| e.path())
        .filter(|p| p.extension().is_some_and(|x| x.eq_ignore_ascii_case("osn")))
        .map(|f| {
            // (the `[name]` line alone: the lists ask again every few seconds, and a whole
            // situation holds every variable of every vehicle; the game writes them in
            // UTF-16, as OMSI does)
            let text = std::fs::read(&f)
                .map(|b| ::legacy_config::decode_text(&b[..b.len().min(8192) & !1]))
                .unwrap_or_default();
            let mut lines = text.lines().map(str::trim);
            let name = lines
                .by_ref()
                .find(|l| l.eq_ignore_ascii_case("[name]"))
                .and_then(|_| lines.next())
                .map(str::to_string)
                .filter(|n| !n.is_empty())
                .unwrap_or_else(|| {
                    f.file_stem()
                        .map(|s| s.to_string_lossy().to_string())
                        .unwrap_or_default()
                });
            SavedSituation {
                saved: modified_secs(&f),
                file: f,
                name,
            }
        })
        .collect();
    slots.sort_by(|a, b| b.saved.cmp(&a.saved).then_with(|| b.name.cmp(&a.name)));
    slots
}

#[cfg(test)]
mod save_slot_tests {
    use super::*;

    #[test]
    fn the_slots_of_a_map_are_listed_by_their_names() {
        let dir = std::env::temp_dir().join(format!("omsi_slots_{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        // as the game writes them: UTF-16 with its mark
        let utf16 = |t: &str| {
            [0xFFu8, 0xFE]
                .into_iter()
                .chain(t.encode_utf16().flat_map(|u| u.to_le_bytes()))
                .collect::<Vec<u8>>()
        };
        std::fs::write(
            dir.join("Slot 1.osn"),
            utf16("\r\n[name]\r\nSlot 1: SD202, 09:00\r\n[description]\r\nx\r\n"),
        )
            .unwrap();
        std::fs::write(
            dir.join("Slot 2.osn"),
            utf16("[name]\r\nSlot 2: NG272, 10:30\r\n"),
        )
            .unwrap();
        std::fs::write(dir.join("notes.txt"), "not a situation").unwrap();
        let mut names: Vec<String> = save_slots(&dir).into_iter().map(|s| s.name).collect();
        names.sort();
        assert_eq!(names, ["Slot 1: SD202, 09:00", "Slot 2: NG272, 10:30"]);
        assert!(save_slots(&dir.join("none")).is_empty());
        let _ = std::fs::remove_dir_all(&dir);
    }
}

/// The command line a duty becomes.
pub fn duty_args(d: &Duty) -> Result<Vec<String>> {
    let root = root()?;
    duty_args_from_root(&root, d)
}

fn duty_args_from_root(root: &Path, d: &Duty) -> Result<Vec<String>> {
    if let Some(t) = d.tutorial {
        return Ok(vec![
            "--root".into(),
            root.to_string_lossy().to_string(),
            "--no-menu".into(),
            "--tutorial".into(),
            t.to_string(),
        ]);
    }
    if let Some(sit) = d.situation.as_deref().filter(|s| !s.trim().is_empty()) {
        let mut a = vec![
            "--root".into(),
            root.to_string_lossy().to_string(),
            "--no-menu".into(),
            "--situation".into(),
            sit.to_string(),
        ];
        if let Some(p) = d.profile.as_deref().filter(|p| !p.trim().is_empty()) {
            a.extend(["--driver".into(), format!("Drivers/{}.odr", p.trim())]);
        }
        // the traffic and the people as for any other start: a situation file keeps the
        // vehicles the player placed, not how busy the streets are (OMSI takes that from its
        // options), and without these a continued session had the timetable buses alone -
        // no cars, nobody at the stops (#136)
        a.extend(["--traffic".into(), d.traffic.unwrap_or(30).to_string()]);
        if d.passengers.unwrap_or(true) {
            a.push("--passengers".into());
        }
        return Ok(a);
    }
    let mut a: Vec<String> = vec![
        "--root".into(),
        root.to_string_lossy().to_string(),
        "--no-menu".into(),
        "--map".into(),
        d.map.clone(),
        "--bus".into(),
        d.bus.clone(),
        "--time".into(),
        if d.time.trim().is_empty() {
            "09:00".into()
        } else {
            d.time.trim().to_string()
        },
    ];
    if let Some(p) = d.paint.as_deref().filter(|p| !p.trim().is_empty()) {
        a.extend(["--paint".into(), p.trim().to_string()]);
    }
    if let Some(pl) = d.plate.as_deref().map(str::trim).filter(|p| !p.is_empty()) {
        a.extend(["--plate".into(), pl.to_string()]);
    }
    if let Some(n) = d.number.as_deref().map(str::trim).filter(|n| !n.is_empty()) {
        a.extend(["--number".into(), n.to_string()]);
    }
    // (a vehicle file taken for a depot from a broken ailists.cfg by older launchers is none)
    if let Some(h) = d.hof.as_deref().filter(|h| {
        !h.trim().is_empty()
            && !h.to_ascii_lowercase().contains(".bus")
            && !h.to_ascii_lowercase().contains(".ovh")
    }) {
        a.extend(["--hof".into(), h.trim().to_string()]);
    }
    // the entry point's place in the map's list; -1: the one nearest to the duty's first
    // stop (a free drive takes the list's first then)
    match d.entry {
        Some(e) if e < 0 => {
            if d.line
                .as_deref()
                .map(|l| !l.trim().is_empty())
                .unwrap_or(false)
            {
                a.push("--auto-entry".into());
            }
        }
        Some(e) => a.extend(["--entry".into(), e.to_string()]),
        None => {}
    }
    let date = d
        .date
        .as_deref()
        .map(|x| x.trim().to_string())
        .filter(|x| !x.is_empty());
    if let Some(dt) = date {
        a.extend(["--date".into(), dt]);
    }
    if let Some(w) = d.weather.as_deref().filter(|x| !x.trim().is_empty()) {
        a.extend(["--weather".into(), w.trim().to_string()]);
    }
    a.extend(["--traffic".into(), d.traffic.unwrap_or(30).to_string()]);
    if d.passengers.unwrap_or(true) {
        a.push("--passengers".into());
    }
    let schedule = d.schedule.unwrap_or(true) || d.line.is_some();
    if schedule {
        a.push("--schedule".into());
    }
    if let (Some(l), true) = (d.line.as_deref().filter(|x| !x.trim().is_empty()), schedule) {
        a.extend(["--line".into(), l.trim().to_string()]);
        if let Some(t) = d.tour.as_deref().filter(|x| !x.trim().is_empty()) {
            a.extend(["--tour".into(), t.trim().to_string()]);
            if let Some(tr) = d.trip.as_deref().filter(|x| !x.trim().is_empty()) {
                a.extend(["--trip".into(), tr.trim().to_string()]);
                if d.whole_tour {
                    a.push("--whole-tour".into());
                }
            }
        }
    }
    if d.autostart.unwrap_or(false) {
        a.push("--autostart".into());
    }
    if d.on_foot.unwrap_or(false) {
        a.push("--on-foot".into());
    }
    let profile = d
        .profile
        .clone()
        .filter(|p| !p.trim().is_empty())
        .unwrap_or_else(|| load_config().profile);
    if let Some(season) = d
        .season
        .as_deref()
        .map(str::trim)
        .filter(|x| !x.is_empty() && !x.eq_ignore_ascii_case("auto"))
    {
        a.extend(["--season".into(), season.to_ascii_lowercase()]);
    }
    if let Some(lan) = d
        .lan
        .as_deref()
        .map(str::trim)
        .filter(|l| !l.is_empty() && !l.eq_ignore_ascii_case("off"))
    {
        if lan.eq_ignore_ascii_case("host") {
            // 0: the default port, or the next free one when a session runs here already
            a.extend(["--lan-host".into(), "0".into()]);
        } else if let Some(target) = lan.strip_prefix("join:") {
            let target = target.trim();
            ::network::describe_join(target).map_err(|e| anyhow!("LAN join: {e}"))?;
            a.extend([
                "--lan-join".into(),
                if target.is_empty() {
                    "auto".into()
                } else {
                    target.to_string()
                },
            ]);
        } else {
            return Err(anyhow!(
                "LAN play: '{lan}' is neither host nor join:<code or address>"
            ));
        }
        let name = d
            .lan_name
            .clone()
            .filter(|n| !n.trim().is_empty())
            .unwrap_or_else(|| profile.clone());
        if !name.trim().is_empty() {
            a.extend(["--lan-name".into(), name.trim().to_string()]);
        }
    }
    if !profile.trim().is_empty() {
        a.extend(["--driver".into(), format!("Drivers/{}.odr", profile.trim())]);
    }
    Ok(a)
}

#[derive(Serialize, Clone, Debug)]
pub struct Launched {
    pub pid: u32,
    pub log: String,
    pub command: String,
    /// Games that were running already (and keep running).
    pub others: usize,
}

/// Start a game for the duty. Any number may run at once; each writes its own log.
pub fn launch(d: &Duty) -> Result<Launched> {
    if IN_PROCESS_GAMES {
        let args = d.args()?;
        let command = args.join(" ");
        log_to_file(&format!("game in this process: {command}"));
        *IN_PROCESS.lock().unwrap_or_else(|e| e.into_inner()) = Some(args);
        return Ok(Launched {
            pid: std::process::id(),
            log: data_dir().join("game.log").to_string_lossy().to_string(),
            command,
            others: 0,
        });
    }
    let c = load_config();
    let game = find_game(&c.game).context("the game binary was not found (set it under Setup)")?;
    let args = d.args()?;
    let profile = d
        .profile
        .clone()
        .filter(|p| !p.trim().is_empty())
        .unwrap_or(c.profile);
    let s = instances::start(&game, &args, d, &profile)?;
    Ok(Launched {
        pid: s.pid,
        log: s.log.to_string_lossy().to_string(),
        command: s.command,
        others: s.others,
    })
}

/// What a LAN join field means (or what is wrong with it), and the sessions hosted here.
pub fn check_join(text: &str) -> Value {
    match ::network::describe_join(text) {
        Ok(d) => json!({ "ok": true, "text": d, "local": instances::local_hosts() }),
        Err(e) => json!({ "ok": false, "text": e, "local": instances::local_hosts() }),
    }
}

// ---------------------------------------------------------------------------------------
// small services for the window

/// A line into ~/.neoomsi/launcher.log.
pub fn log_to_file(line: &str) {
    use std::io::Write;
    let p = data_dir().join("launcher.log");
    if let Ok(mut f) = std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(&p)
    {
        let now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_secs())
            .unwrap_or(0);
        let _ = writeln!(f, "{now} {line}");
    }
}

/// What interrupted installs left behind (and the old unzip folder): cleaned up once when
/// the launcher opens.
pub fn cleanup() {
    for line in install::cleanup_stale(&data_dir(), content_dir().as_deref()) {
        log_to_file(&format!("cleanup: {line}"));
    }
}

/// Native folder / file picker (Finder, Explorer, the GTK dialog) for a mod. Must run on
/// the main thread. (None on a phone: the launcher browses the storage itself there.)
pub fn pick_mod(zip: bool) -> Option<PathBuf> {
    #[cfg(not(target_os = "android"))]
    {
        if zip {
            rfd::FileDialog::new()
                .set_title("Choose a mod archive")
                .add_filter("Mod archive", &["zip", "7z", "rar"])
                .pick_file()
        } else {
            rfd::FileDialog::new()
                .set_title("Choose the mod folder")
                .pick_folder()
        }
    }
    #[cfg(target_os = "android")]
    {
        let _ = zip;
        None
    }
}

/// Folder picker (Setup: the OMSI 2 folder).
pub fn pick_folder(title: &str) -> Option<PathBuf> {
    #[cfg(not(target_os = "android"))]
    {
        rfd::FileDialog::new().set_title(title).pick_folder()
    }
    #[cfg(target_os = "android")]
    {
        let _ = title;
        None
    }
}

/// File picker (Setup: the game program).
pub fn pick_file(title: &str) -> Option<PathBuf> {
    #[cfg(not(target_os = "android"))]
    {
        rfd::FileDialog::new().set_title(title).pick_file()
    }
    #[cfg(target_os = "android")]
    {
        let _ = title;
        None
    }
}

pub fn external_launcher(game: &Path) -> Option<PathBuf> {
    if std::env::var_os("OMSI_BUILTIN_LAUNCHER").is_some() {
        return None;
    }
    let app = match std::env::var_os("OMSI_LAUNCHER") {
        Some(p) => PathBuf::from(p),
        None => shipped_launcher(game)?,
    };
    let current = std::env::current_exe().ok();
    (app.is_file() && !same_file(&app, game) && !current.is_some_and(|c| same_file(&app, &c)))
        .then_some(app)
}

/// An old `OMSI_LAUNCHER` may point back at this program or the game: they would start
/// each other without end.
fn same_file(a: &Path, b: &Path) -> bool {
    std::fs::canonicalize(a).ok().is_some_and(|a| std::fs::canonicalize(b).ok() == Some(a))
}

fn shipped_launcher(game: &Path) -> Option<PathBuf> {
    let dir = game.parent()?;
    Some(if cfg!(target_os = "macos") {
        dir.parent()?
            .join("Resources/launcher/neoOMSI Launcher.app/Contents/MacOS/neoOMSI Launcher")
    } else if cfg!(windows) {
        dir.join("launcher").join("neoOMSI Launcher.exe")
    } else {
        dir.join("launcher").join("neoomsi-launcher-app")
    })
}

/// False when there is none: the built-in launcher is the one then.
pub fn start_external_launcher(game: &Path) -> Result<bool> {
    let Some(app) = external_launcher(game) else {
        return Ok(false);
    };
    #[cfg(target_os = "linux")]
    if !linux_sandbox_works(&app) {
        let helper = app.with_file_name("chrome-sandbox");
        return Err(anyhow!(
            "{} needs Chromium's sandbox, which this system blocks (user namespaces are restricted); set its helper up once with `sudo chown root:root {h} && sudo chmod 4755 {h}`",
            app.display(),
            h = helper.display(),
        ));
    }
    std::process::Command::new(&app)
        .env("NEOOMSI_ENGINE_PATH", game)
        .stdin(std::process::Stdio::null())
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .spawn()
        .with_context(|| format!("starting {}", app.display()))?;
    Ok(true)
}

#[cfg_attr(not(target_os = "linux"), allow(dead_code))]
fn chromium_sandbox_available(
    helper: Option<(u32, u32)>,
    sysctl: impl Fn(&str) -> Option<String>,
) -> bool {
    if helper.is_some_and(|(uid, mode)| uid == 0 && mode & 0o4000 != 0) {
        return true;
    }
    let is = |key: &str, off: &str| sysctl(key).is_some_and(|v| v.trim() == off);
    !(is("kernel/unprivileged_userns_clone", "0")
        || is("kernel/apparmor_restrict_unprivileged_userns", "1")
        || is("user/max_user_namespaces", "0"))
}

#[cfg(target_os = "linux")]
fn linux_sandbox_works(app: &Path) -> bool {
    use std::os::unix::fs::MetadataExt;
    let helper = std::fs::metadata(app.with_file_name("chrome-sandbox"))
        .ok()
        .map(|m| (m.uid(), m.mode()));
    chromium_sandbox_available(helper, |key| {
        std::fs::read_to_string(Path::new("/proc/sys").join(key)).ok()
    })
}

/// A phone runs one program: the launcher hands the game's command line over here and the
/// same process plays it in the same window (see the app's `android.rs`) instead of starting
/// another process.
static IN_PROCESS: std::sync::Mutex<Option<Vec<String>>> = std::sync::Mutex::new(None);

/// The command line of a game the launcher asked for (taken once).
pub fn take_in_process_launch() -> Option<Vec<String>> {
    IN_PROCESS.lock().unwrap_or_else(|e| e.into_inner()).take()
}

/// Whether games run inside the launcher's own process (a phone).
pub const IN_PROCESS_GAMES: bool = cfg!(target_os = "android");

pub use instances::{Instance, list as list_instances, log_tail, stop as stop_instance};

/// Terminal access to the same functions: `--cli lines '{"map":"maps/Grundorf/global.cfg"}'`.
pub fn cli(cmd: &str, arg: &str) -> Result<Value> {
    init_settings();
    let a: Value = serde_json::from_str(arg).unwrap_or(json!({}));
    let s = |k: &str| a.get(k).and_then(|v| v.as_str()).unwrap_or("").to_string();
    Ok(match cmd {
        "config" => serde_json::to_value(load_config())?,
        "maps" => serde_json::to_value(list_maps()?)?,
        "vehicles" => serde_json::to_value(list_vehicles()?)?,
        "weather" => serde_json::to_value(list_weather()?)?,
        "lines" => serde_json::to_value(list_lines(&s("map"), &s("date"))?)?,
        "ibis" => serde_json::to_value(ibis_info(&s("bus"), &s("hof"), &s("line"))?)?,
        "profiles" => serde_json::to_value(list_profiles()?)?,
        "profile" => serde_json::to_value(get_profile(&s("name"))?)?,
        "mods" => {
            // the inbox is installed right away here (there is no page to follow it)
            let done = install_inbox_blocking();
            let mut v = serde_json::to_value(mods_status()?)?;
            v["installed_now"] = serde_json::to_value(done)?;
            v
        }
        "install" => {
            let cancel = a.get("cancel_after_ms").and_then(|v| v.as_u64());
            let mode = s("mode");
            let p = install_mod_blocking(
                Path::new(&s("path")),
                if mode.is_empty() {
                    "auto"
                } else {
                    mode.as_str()
                },
                cancel,
            )?;
            if p.state == "failed" {
                return Err(anyhow!("{}", p.message));
            }
            serde_json::to_value(p)?
        }
        "modinfo" => serde_json::to_value(inspect_mod(Path::new(&s("path")))?)?,
        "stamp" => json!(poll()?.stamp),
        "poll" => {
            // {"watch": seconds}: keep polling like the page does (the inbox watcher needs
            // two looks), then wait for the installs it started
            let watch = a.get("watch").and_then(|v| v.as_f64()).unwrap_or(0.0);
            let t0 = std::time::Instant::now();
            let mut stamps = vec![poll()?.stamp];
            let mut started = Vec::new();
            while t0.elapsed().as_secs_f64() < watch
                || install::jobs().iter().any(|j| j.finished.is_none())
            {
                std::thread::sleep(std::time::Duration::from_millis(1000));
                let p = poll()?;
                started.extend(p.started);
                if stamps.last() != Some(&p.stamp) {
                    stamps.push(p.stamp);
                }
            }
            let mut v = serde_json::to_value(poll()?)?;
            v["started"] = json!(started);
            v["stamps"] = json!(stamps);
            v
        }
        "instances" => serde_json::to_value(instances::list())?,
        "stop" => {
            let by_itself =
                instances::stop(a.get("pid").and_then(|v| v.as_u64()).unwrap_or(0) as u32)?;
            json!({ "stopped": true, "ended_by_itself": by_itself })
        }
        "log" => json!(instances::log_tail(
            a.get("pid").and_then(|v| v.as_u64()).unwrap_or(0) as u32,
            a.get("lines").and_then(|v| v.as_u64()).unwrap_or(40) as usize
        )?),
        "join" => check_join(&s("text")),
        "settings" => get_settings()?,
        // what the page's Save does: `--cli save_settings '{"view_distance":"1500"}'` (the
        // other values from the file as it is)
        "save_settings" => {
            let mut v = get_settings()?;
            if let (Some(v), Some(changes)) = (v.as_object_mut(), a.as_object()) {
                v.extend(changes.clone());
            }
            save_settings(&v)?;
            get_settings()?
        }
        "keybindings" => get_keybindings()?,
        // `--cli save_keybindings '{"game":[...],"vehicles":[...]}'`: the whole list, as
        // `keybindings` returns it - a partial update reads the current file first
        "save_keybindings" => {
            save_keybindings(&a)?;
            get_keybindings()?
        }
        "preview" => json!(bus_preview(&s("bus"), &s("paint"))?),
        "args" => json!(duty_args(&serde_json::from_value(a.clone())?)?),
        "launch" => serde_json::to_value(launch(&serde_json::from_value(a.clone())?)?)?,
        _ => return Err(anyhow!("unknown command {cmd}")),
    })
}

#[cfg(test)]
mod tests {
    #[test]
    fn vehicle_type_label_falls_back_to_the_file_name() {
        let path = std::path::Path::new("Vehicles/Pack/NL_202.bus");
        for empty in ["", "   "] {
            assert_eq!(super::vehicle_type_label(empty, path), "NL 202");
        }
        assert_eq!(
            super::vehicle_type_label("  MAN_NL202  ", path),
            "MAN NL202"
        );
    }

    #[test]
    fn a_part_found_from_the_vehicle_folder_is_no_missing_pack() {
        let root = std::env::temp_dir().join(format!("neoomsi-packs-{}", std::process::id()));
        let obj = root.join("Sceneryobjects/X");
        let cfgs = root.join("Vehicles/B/model/Configuration Files");
        std::fs::create_dir_all(&obj).unwrap();
        std::fs::create_dir_all(&cfgs).unwrap();
        std::fs::write(obj.join("a.o3d"), b"x").unwrap();
        ::legacy_config::add_content_root(root.clone());
        let model = cfgs.join("m.cfg");
        std::fs::write(
            &model,
            "[mesh]\r\n..\\..\\..\\Sceneryobjects\\X\\a.o3d\r\n..\\..\\..\\Other\\b.o3d\r\n",
        )
            .unwrap();
        let packs = super::missing_packs_of(&model);
        ::legacy_config::remove_content_root(&root);
        let _ = std::fs::remove_dir_all(&root);
        assert_eq!(packs, vec!["Other".to_string()]);
    }

    #[test]
    fn the_launcher_keeps_its_sandbox_whenever_chromium_can_have_one() {
        let sysctl = |pairs: &'static [(&'static str, &'static str)]| {
            move |key: &str| {
                pairs
                    .iter()
                    .find(|(k, _)| *k == key)
                    .map(|(_, v)| format!("{v}
"))
            }
        };
        let none: &[(&str, &str)] = &[];
        let ubuntu: &[(&str, &str)] = &[("kernel/apparmor_restrict_unprivileged_userns", "1")];
        let debian_off: &[(&str, &str)] = &[("kernel/unprivileged_userns_clone", "0")];
        let open: &[(&str, &str)] = &[
            ("kernel/unprivileged_userns_clone", "1"),
            ("kernel/apparmor_restrict_unprivileged_userns", "0"),
        ];
        assert!(chromium_sandbox_available(None, sysctl(none)), "namespaces allowed");
        assert!(chromium_sandbox_available(None, sysctl(open)));
        assert!(!chromium_sandbox_available(None, sysctl(ubuntu)), "Ubuntu 24.04 restricts them");
        assert!(!chromium_sandbox_available(None, sysctl(debian_off)));
        let setuid_root = Some((0, 0o104755));
        assert!(chromium_sandbox_available(setuid_root, sysctl(ubuntu)), "the helper is enough");
        assert!(!chromium_sandbox_available(Some((1000, 0o104755)), sysctl(ubuntu)), "not root's");
        assert!(!chromium_sandbox_available(Some((0, 0o100755)), sysctl(ubuntu)), "not setuid");
    }

    #[test]
    fn the_shipped_launcher_is_found_beside_the_game() {
        let root = std::env::temp_dir().join(format!("neoomsi-ui-{}", std::process::id()));
        let (game, app) = if cfg!(target_os = "macos") {
            let c = root.join("neoOMSI.app/Contents");
            (
                c.join("MacOS/neoomsi"),
                c.join("Resources/launcher/neoOMSI Launcher.app/Contents/MacOS/neoOMSI Launcher"),
            )
        } else if cfg!(windows) {
            (root.join("neoomsi.exe"), root.join("launcher/neoOMSI Launcher.exe"))
        } else {
            (root.join("neoomsi"), root.join("launcher/neoomsi-launcher-app"))
        };
        assert_eq!(super::shipped_launcher(&game).as_deref(), Some(app.as_path()));
        let overridden = ["OMSI_LAUNCHER", "OMSI_BUILTIN_LAUNCHER"]
            .iter()
            .any(|k| std::env::var_os(k).is_some());
        if !overridden {
            assert_eq!(super::external_launcher(&game), None, "none shipped");
            std::fs::create_dir_all(app.parent().unwrap()).unwrap();
            std::fs::write(&app, b"").unwrap();
            assert_eq!(super::external_launcher(&game), Some(app.clone()));
        }
        std::fs::create_dir_all(game.parent().unwrap()).unwrap();
        std::fs::write(&game, b"").unwrap();
        let dir = game.parent().unwrap();
        assert!(super::same_file(&game, &dir.join(".").join(game.file_name().unwrap())));
        assert!(!super::same_file(&game, &app));
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn tutorials_are_found_by_omsis_language_suffix() {
        let root = std::env::temp_dir().join(format!("omsi-tutorials-{}", std::process::id()));
        let dir = root.join("Tutorials");
        std::fs::create_dir_all(&dir).unwrap();
        let page = |title: &str| format!("<style></style><h2>{title}</h2><p>Text</p>");
        std::fs::write(dir.join("menu_1_ENG.html"), page("Driving")).unwrap();
        std::fs::write(dir.join("menu_1_DEU.html"), page("Fahren")).unwrap();
        std::fs::write(dir.join("menu_2_DEU.html"), page("Tickets")).unwrap();
        let titles = |lang| {
            super::tutorials_in(&root, lang)
                .into_iter()
                .map(|(n, title, _)| (n, title))
                .collect::<Vec<_>>()
        };
        assert_eq!(titles("en"), [(1, "Driving".into()), (2, "Tickets".into())]);
        assert_eq!(titles("de"), [(1, "Fahren".to_string()), (2, "Tickets".into())]);
        std::fs::remove_dir_all(&root).unwrap();
    }

    #[test]
    fn dsc_files_give_name_and_description() {
        let d = super::parse_dsc(
            "\r\n[friendlyname]\r\nMAN\r\nNL202 - EN92\r\nBeige\r\n\r\n[description]\r\nAlthough the BVG did not purchase\r\n\r\n-Technical specifications-\r\n[end]\r\n",
        );
        assert_eq!(d.name, vec!["MAN", "NL202 - EN92", "Beige"]);
        assert_eq!(
            d.description,
            "Although the BVG did not purchase\n\n-Technical specifications-"
        );
        let w = super::parse_dsc(
            "[name]\r\nGround Fog\r\n\r\n[description]\r\nHeavy ground fog limits the maximum visibility dangerously!\r\n[end]\r\n",
        );
        assert_eq!(w.name, vec!["Ground Fog"]);
        assert_eq!(
            w.description,
            "Heavy ground fog limits the maximum visibility dangerously!"
        );
        let p = std::path::Path::new("/x/maps/Spandau/global.cfg");
        assert_eq!(
            super::dsc_candidates(p, "en"),
            vec![std::path::PathBuf::from("/x/maps/Spandau/global_ENG.dsc")]
        );
        assert_eq!(super::dsc_candidates(p, "FRA").len(), 2);
        assert_eq!(
            super::dsc_candidates(std::path::Path::new("/v/MAN_EN92_main.bus"), "de"),
            vec![std::path::PathBuf::from("/v/MAN_EN92_main_DEU.dsc")]
        );
    }

    use super::*;

    /// A duty file written before the number plate field (or one that leaves it out) loads
    /// with no plate, and a plate the player typed is kept as it stands.
    #[test]
    fn a_picked_trip_starts_the_rest_of_the_tour() {
        let d = Duty {
            map: "maps/x/global.cfg".into(),
            bus: "Vehicles/x.bus".into(),
            time: "09:43".into(),
            line: Some("14".into()),
            tour: Some("1".into()),
            trip: Some("5".into()),
            whole_tour: true,
            ..Default::default()
        };
        let a = duty_args_from_root(Path::new("C:/OMSI 2"), &d).unwrap();
        let k = a.iter().position(|x| x == "--trip").unwrap();
        assert_eq!(
            (a[k + 1].as_str(), a[k + 2].as_str()),
            ("5", "--whole-tour")
        );
        let alone = duty_args_from_root(
            Path::new("C:/OMSI 2"),
            &Duty {
                whole_tour: false,
                ..d
            },
        )
            .unwrap();
        assert!(!alone.iter().any(|x| x == "--whole-tour"));
    }

    #[test]
    fn a_game_started_again_keeps_its_command_line() {
        let i = Instance {
            map: "maps/x/global.cfg".into(),
            bus: "Vehicles/x.bus".into(),
            profile: "Jo".into(),
            lan: "host".into(),
            args: vec!["--lan-host".into(), "0".into()],
            ..Default::default()
        };
        let d = Duty::again(&i);
        assert_eq!(d.args().unwrap(), i.args);
        assert_eq!(
            (d.map.as_str(), d.profile.as_deref(), d.lan.as_deref()),
            ("maps/x/global.cfg", Some("Jo"), Some("host"))
        );
    }

    #[test]
    fn a_duty_keeps_its_plate_and_older_files_load_without_one() {
        let old: Duty = serde_json::from_str(
            r#"{"map":"maps/x/global.cfg","bus":"Vehicles/x.bus","time":"09:00"}"#,
        )
            .unwrap();
        assert_eq!(old.plate, None);
        let typed: Duty = serde_json::from_str(r#"{"map":"maps/x/global.cfg","bus":"Vehicles/x.bus","time":"09:00","plate":"B-AB 1234"}"#).unwrap();
        assert_eq!(typed.plate.as_deref(), Some("B-AB 1234"));
    }

    /// The fleet number picked in the launcher reaches the game.
    #[test]
    fn a_duty_passes_its_fleet_number() {
        let d: Duty = serde_json::from_str(
            r#"{"map":"maps/x/global.cfg","bus":"Vehicles/x.bus","time":"09:00","number":"4711"}"#,
        )
            .unwrap();
        let a = duty_args_from_root(Path::new("C:/OMSI 2"), &d).unwrap();
        assert!(
            a.windows(2).any(|w| w[0] == "--number" && w[1] == "4711"),
            "{a:?}"
        );
    }

    fn settings_guard() -> std::sync::MutexGuard<'static, ()> {
        static LOCK: std::sync::Mutex<()> = std::sync::Mutex::new(());
        let guard = LOCK.lock().unwrap_or_else(|e| e.into_inner());
        ::config::reset_all();
        guard
    }

    #[test]
    fn settings_start_from_the_config_defaults() {
        let _g = settings_guard();
        let v = default_settings();
        assert_eq!(v["view_distance"], "auto");
        assert_eq!(v["max_obj_dist"], "auto");
        assert_eq!(v["render_scale"], "auto");
        assert_eq!(v["language"], "en");
        assert_eq!(v["graphics"], "vanilla_plus");
        assert_eq!(v["enhanced"], false);
        assert_eq!(v["ai_unsched_factor"], 100);
        assert_eq!(v["time_speed"], "1");
        assert_eq!(v["mirror_refresh"], "full");
        assert_eq!(v["pax_prefer_seats"], false);
        assert_eq!(v["ui_scale"], 1.0);
        assert_eq!(current_settings(), v);
    }

    #[test]
    fn settings_round_trip_through_the_config() {
        let _g = settings_guard();
        let changes = [
            ("steer_look", json!(true)),
            ("pax_prefer_seats", json!(true)),
            ("discord_status", json!(false)),
            ("discord_app_id", json!("123456")),
            ("update_auto", json!(true)),
            ("camera_collision", json!(false)),
            ("brake_hold", json!(false)),
            ("led_mips", json!(2.5)),
            ("led_glow", json!(11)),
            ("look_sens", json!(0.5)),
            ("pedal_brake", json!(1.5)),
            ("seat_y", json!(-0.1)),
            ("vr", json!(true)),
            ("vr_scale", json!(0.8)),
            ("vr_mirror_rate", json!(120.0)),
            ("pax_motion", json!("omsi")),
            ("pax_ik", json!(false)),
            ("units", json!("imperial")),
            ("ai_unsched_factor", json!(150)),
        ];
        let mut v = default_settings();
        for (k, x) in &changes {
            v[*k] = x.clone();
        }
        apply_settings(&v);
        let back = current_settings();
        for (k, x) in &changes {
            assert_eq!(back[*k], *x, "{k}");
        }
        assert_eq!(::config::get_float("ai", "unsched_factor"), Some(1.5));
        assert_eq!(::config::get_bool("vr", "enabled"), Some(true));
        assert_eq!(::config::get_string("passengers", "motion").as_deref(), Some("omsi"));
    }

    #[test]
    fn settings_are_clamped_and_automatic_values_kept() {
        let _g = settings_guard();
        let mut v = default_settings();
        v["ui_scale"] = json!(9);
        v["mirror_size"] = json!(10);
        v["anisotropy"] = json!(32);
        v["view_distance"] = json!("1500");
        v["max_obj_dist"] = json!("900");
        v["render_scale"] = json!("0.75");
        v["time_speed"] = json!("2.5");
        v["language"] = json!("Deutsch");
        v["graphics"] = json!("enhanced");
        apply_settings(&v);
        let back = current_settings();
        assert_eq!(back["ui_scale"], 2.0);
        assert_eq!(back["mirror_size"], 64);
        assert_eq!(back["anisotropy"], 16);
        assert_eq!(back["view_distance"], "1500");
        assert_eq!(back["max_obj_dist"], "900");
        assert_eq!(back["render_scale"], "0.75");
        assert_eq!(back["time_speed"], "2.5");
        assert_eq!(back["language"], "de");
        assert_eq!(back["graphics"], "enhanced");
        assert_eq!(back["enhanced"], true);
        assert_eq!(::config::get_float("graphics", "view_distance"), Some(1500.0));
        v["view_distance"] = json!("auto");
        v["max_obj_dist"] = json!("auto");
        v["mirror_size"] = json!(0);
        apply_settings(&v);
        let back = current_settings();
        assert_eq!(back["view_distance"], "auto");
        assert_eq!(back["max_obj_dist"], "auto");
        assert_eq!(back["mirror_size"], 0);
        assert_eq!(::config::get_float("graphics", "view_distance"), Some(0.0));
        assert_eq!(::config::get_float("graphics", "max_obj_dist"), Some(-1.0));
    }

    #[test]
    fn only_english_and_german_are_supported() {
        assert_eq!(language_code("English"), "en");
        assert_eq!(language_iso("en"), "");
        assert_eq!(language_code("Deutsch"), "de");
        assert_eq!(language_iso("de"), "de");
        assert_eq!(language_code("pt-BR"), "en");
    }

    /// The lines follow the date as the game's do: Spandau's 1991 timetable change takes
    /// "5 & 5N" off and brings "130 & N30"; without a date it is the game's default day.
    #[test]
    fn lines_follow_the_chrono_date() {
        let root = std::env::var_os("OMSI_ROOT")
            .map(PathBuf::from)
            .unwrap_or_else(|| PathBuf::from("../../../OMSI 2 Original"));
        let map = root.join("maps/Berlin-Spandau");
        if !map.join("TTData").is_dir() {
            eprintln!("skipped: no {}", map.display());
            return;
        }
        let names = |date: &str| {
            lines_on(&map, date)
                .unwrap()
                .into_iter()
                .map(|l| l.name)
                .collect::<Vec<_>>()
        };
        let (then, now) = (names(""), names("2026-09-17"));
        assert_eq!(then, names(DEFAULT_DATE));
        assert!(
            then.iter().any(|l| l == "5 & 5N") && !then.iter().any(|l| l == "130 & N30"),
            "{then:?}"
        );
        assert!(
            !now.iter().any(|l| l == "5 & 5N") && now.iter().any(|l| l == "130 & N30"),
            "{now:?}"
        );
        assert!(
            names("1991-06-01").iter().any(|l| l == "5 & 5N")
                && !names("1991-06-02").iter().any(|l| l == "5 & 5N")
        );
        assert!(lines_on(&map, "someday").is_err());
    }

}

#[cfg(test)]
mod omsi_options_tests {
    #[test]
    fn the_originals_options_are_read() {
        let root = std::path::Path::new("../../../OMSI 2 Original");
        let Some(o) = super::omsi_options(root) else {
            return;
        };
        assert_eq!(
            o.last_map.as_deref(),
            Some("maps/Berlin-Spandau/global.cfg")
        );
        assert_eq!(o.last_driver.as_deref(), Some("OMSI-Fan"));
        assert_eq!(o.settings["max_fps"], 30);
        assert_eq!(o.settings["mirror_size"], 512);
        assert_eq!(o.settings["language"], "en");
        assert_eq!(o.settings["head_movement"], true);
        assert_eq!(o.settings["collision_vehicles"], false);
    }

    #[test]
    fn the_real_time_reflections_are_read_as_omsi_writes_them() {
        let root = std::env::temp_dir().join(format!("omsi-realrefl-{}", std::process::id()));
        std::fs::create_dir_all(&root).unwrap();
        for (word, mode) in [("none", "off"), ("economy", "eco"), ("full", "full")] {
            std::fs::write(root.join("options.cfg"), format!("[performance_realreflexions]\r\n{word}\r\n\r\n[performance_reflTexSize]\r\n9\r\n")).unwrap();
            let o = super::omsi_options(&root).unwrap();
            assert_eq!(o.settings["mirror_refresh"], mode, "{word}");
        }
        let _ = std::fs::remove_dir_all(&root);
    }
}
