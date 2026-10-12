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

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ModEntry {
    pub name: String,
    pub path: PathBuf,
    pub kind: ModKind,
}

pub fn ensure_layout(mods_dir: &Path) -> std::io::Result<()> {
    std::fs::create_dir_all(mods_dir.join(SHARED_DIR))
}

/// The directory inside a folder mod that holds OMSI's content. Downloaded packs often
/// wrap their files in `OMSI 2` or `OMSI2`; ZIP mounts already remove that wrapper, but a
/// folder needs the equivalent step before it becomes a content root.
pub fn folder_content_root(path: &Path) -> PathBuf {
    let Some(wrapper) = legacy_config::vfs::list_dir(path).and_then(|entries| {
        entries.into_iter().find_map(|(name, is_dir)| {
            (is_dir
                && (name.eq_ignore_ascii_case("OMSI 2") || name.eq_ignore_ascii_case("OMSI2")))
            .then_some(path.join(name))
        })
    }) else {
        return path.to_path_buf();
    };

    let has_content = legacy_config::vfs::list_dir(&wrapper).is_some_and(|entries| {
        entries.into_iter().any(|(name, is_dir)| {
            is_dir
                && legacy_config::CONTENT_FOLDERS
                    .iter()
                    .any(|folder| name.eq_ignore_ascii_case(folder))
        })
    });
    has_content.then_some(wrapper).unwrap_or_else(|| path.to_path_buf())
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

#[cfg(test)]
mod tests {
    #[test]
    fn folder_mod_uses_an_omsi_wrapper_that_contains_content() {
        let root = std::env::temp_dir().join(format!(
            "neoomsi-folder-mod-wrapper-{}",
            std::process::id()
        ));
        let wrapper = root.join("OMSI2");
        std::fs::create_dir_all(wrapper.join("Vehicles/Test Bus")).unwrap();

        assert_eq!(super::folder_content_root(&root), wrapper);

        std::fs::remove_dir_all(root).ok();
    }

    #[test]
    fn folder_mod_keeps_an_unrelated_omsi_wrapper_at_its_own_root() {
        let root = std::env::temp_dir().join(format!(
            "neoomsi-folder-mod-no-content-{}",
            std::process::id()
        ));
        std::fs::create_dir_all(root.join("OMSI 2/Documents")).unwrap();

        assert_eq!(super::folder_content_root(&root), root);

        std::fs::remove_dir_all(root).ok();
    }
}
