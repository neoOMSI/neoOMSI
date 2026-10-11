use std::path::{Path, PathBuf};

pub const SHARED_DIR: &str = "shared";

const RESERVED: &[&str] = &[SHARED_DIR, "waiting", "installed", "uninstalled", "plugins-not-enabled"];

/// File resolve order
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum Layer {
    Mod,
    Shared,
    Legacy,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ModKind {
    Folder,
    Zip,
}

#[derive(Debug, Clone)]
pub struct ModEntry {
    pub name: String,
    pub path: PathBuf,
    pub kind: ModKind,
}

pub fn ensure_layout(mods_dir: &Path) -> std::io::Result<()> {
    std::fs::create_dir_all(mods_dir.join(SHARED_DIR))
}

pub fn discover(mods_dir: &Path) -> Vec<ModEntry> {
    let Ok(rd) = std::fs::read_dir(mods_dir) else {
        return Vec::new();
    };
    let mut out: Vec<ModEntry> = Vec::new();
    for e in rd.flatten() {
        let path = e.path();
        let file = e.file_name().to_string_lossy().into_owned();
        let (name, kind) = if path.is_dir() {
            (file, ModKind::Folder)
        } else if path.is_file()
            && path
            .extension()
            .is_some_and(|x| x.eq_ignore_ascii_case("zip"))
        {
            (
                path.file_stem().unwrap_or_default().to_string_lossy().into_owned(),
                ModKind::Zip,
            )
        } else {
            continue;
        };
        if name.starts_with('.') || RESERVED.iter().any(|r| r.eq_ignore_ascii_case(&name)) {
            continue;
        }
        out.push(ModEntry { name, path, kind });
    }
    out.sort_by(|a, b| {
        a.name
            .to_lowercase()
            .cmp(&b.name.to_lowercase())
            .then((a.kind == ModKind::Zip).cmp(&(b.kind == ModKind::Zip)))
    });
    out.dedup_by(|b, a| a.name.eq_ignore_ascii_case(&b.name));
    out
}
