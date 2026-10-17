use std::path::{Path, PathBuf};

pub const MODS_DIR: &str = "mods";
pub const UNINSTALLED_DIR: &str = "uninstalled";
/// Where older versions put the .zip files they used in place.
pub const ARCHIVES_DIR: &str = "Archives";
/// A library below every mod: its files are used only where no mod provides them.
pub const SHARED_DIR: &str = "shared";

const RESERVED: &[&str] = &[SHARED_DIR, "waiting", "installed", UNINSTALLED_DIR, "plugins-not-enabled"];

/// The content folders whose sub-folders are packs, such as `Vehicles/<bus>` or `maps/<map>`.
pub(crate) const PACK_FOLDERS: [&str; 6] = [
    "Vehicles",
    "Sceneryobjects",
    "Splines",
    "Humans",
    "Trains",
    "maps",
];

pub(crate) fn is_pack_folder(name: &str) -> bool {
    PACK_FOLDERS
        .iter()
        .any(|folder| folder.eq_ignore_ascii_case(name))
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ModKind {
    Folder,
    Zip,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ModEntry {
    /// `Foo`, `Foo.zip` or `Archives/Foo.zip`.
    pub id: String,
    pub name: String,
    pub path: PathBuf,
    pub kind: ModKind,
}

pub fn is_reserved(name: &str) -> bool {
    RESERVED.iter().any(|reserved| reserved.eq_ignore_ascii_case(name))
}

/// The folder inside a folder mod that holds its content, found the same way as for a .zip.
pub fn folder_content_root(path: &Path) -> PathBuf {
    let mut dirs = Vec::new();
    collect_dirs(path, "", 0, &mut dirs);
    let prefix = legacy_config::vfs::content_prefix(dirs.iter().map(String::as_str));
    let mut root = path.to_path_buf();
    root.extend(prefix.split('/').filter(|component| !component.is_empty()));
    root
}

fn collect_dirs(dir: &Path, rel: &str, depth: usize, out: &mut Vec<String>) {
    for (name, is_dir) in legacy_config::vfs::list_dir(dir).unwrap_or_default() {
        if !is_dir {
            continue;
        }
        let name = name.to_string_lossy();
        let sub = format!("{rel}{name}/");
        if depth < 5 && !legacy_config::is_content_folder(&name) {
            collect_dirs(&dir.join(&*name), &sub, depth + 1, out);
        }
        out.push(sub);
    }
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
            (file.clone(), ModKind::Folder)
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
        if name.starts_with('.') || is_reserved(&name) {
            continue;
        }
        out.push(ModEntry { id: file, name, path, kind });
    }
    out.sort_by(|a, b| {
        a.name
            .to_lowercase()
            .cmp(&b.name.to_lowercase())
            .then((a.kind == ModKind::Zip).cmp(&(b.kind == ModKind::Zip)))
            // Keeps the order stable for names that differ only in case.
            .then_with(|| a.name.cmp(&b.name))
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

    #[test]
    fn folder_mod_root_follows_the_zip_rule() {
        let root = std::env::temp_dir().join(format!("neoomsi-folder-mod-rule-{}", std::process::id()));
        // A wrapper folder with any name, next to a documentation folder.
        let wrapped = root.join("wrapped");
        std::fs::create_dir_all(wrapped.join("Content/Vehicles/Bus/Texture")).unwrap();
        std::fs::create_dir_all(wrapped.join("Docs")).unwrap();
        assert_eq!(super::folder_content_root(&wrapped), wrapped.join("Content"));
        // With content folders at the top, the mod folder itself is the root.
        let flat = root.join("flat");
        std::fs::create_dir_all(flat.join("Vehicles/Bus/Sound")).unwrap();
        assert_eq!(super::folder_content_root(&flat), flat);

        std::fs::remove_dir_all(root).ok();
    }
}
