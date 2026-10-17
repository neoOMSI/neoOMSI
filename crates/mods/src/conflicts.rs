//! Which mounted entry provides which files, for the launcher to show. The game doesn't use it.

use legacy_config::{name_key, resolve_existing, vfs};
use std::collections::HashMap;
use std::path::Path;

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Conflict {
    pub id: String,
    pub files: usize,
    /// Files this entry provides that also exist in an entry below it.
    pub overrides: usize,
    /// Files this entry provides that an entry above it provides too.
    pub overridden: usize,
    /// Files that take the place of an original OMSI 2 file.
    pub replaces_original: usize,
    pub hidden_by: Vec<String>,
    pub overrides_mods: Vec<String>,
    /// `Vehicles` folders that another entry fills with different vehicles.
    pub clashes: Vec<Clash>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Clash {
    /// The folder name inside `Vehicles`, spelled as in this entry.
    pub folder: String,
    pub other: String,
}

pub fn conflicts() -> Vec<Conflict> {
    let (roots, legacy) = {
        let state = crate::STATE.read().unwrap();
        let Some(state) = state.as_ref() else {
            return Vec::new();
        };
        (state.roots.clone(), state.legacy.clone())
    };
    let files: Vec<Vec<String>> = roots.iter().map(|(_, root)| files_of(root)).collect();
    let mut out: Vec<Conflict> = roots
        .iter()
        .zip(&files)
        .map(|((entry, _), entry_files)| Conflict {
            id: entry.id.clone(),
            files: entry_files.len(),
            ..Default::default()
        })
        .collect();
    let mut winner: HashMap<&str, usize> = HashMap::new();
    for (index, list) in files.iter().enumerate() {
        for file in list {
            match winner.get(file.as_str()) {
                Some(&above) => {
                    out[index].overridden += 1;
                    let (above_id, below_id) = (out[above].id.clone(), out[index].id.clone());
                    add(&mut out[index].hidden_by, above_id);
                    out[above].overrides += 1;
                    add(&mut out[above].overrides_mods, below_id);
                }
                None => {
                    winner.insert(file, index);
                }
            }
        }
    }
    if let Some(legacy) = &legacy {
        for (file, &index) in &winner {
            let comps: Vec<&str> = file.split('/').collect();
            if resolve_existing(legacy, &comps).is_some() {
                out[index].replaces_original += 1;
            }
        }
    }
    let vehicles: Vec<HashMap<&str, Vec<&str>>> =
        files.iter().map(|list| vehicle_definitions(list)).collect();
    for index in 0..out.len() {
        for (folder, definitions) in &vehicles[index] {
            for other in (0..out.len()).filter(|&other| other != index) {
                if vehicles[other]
                    .get(folder)
                    .is_some_and(|theirs| theirs != definitions)
                {
                    let spelt =
                        resolve_existing(&roots[index].1, &["Vehicles", folder]).and_then(|path| {
                            path.file_name()
                                .map(|name| name.to_string_lossy().into_owned())
                        });
                    let clash = Clash {
                        folder: spelt.unwrap_or_else(|| folder.to_string()),
                        other: out[other].id.clone(),
                    };
                    add(&mut out[index].clashes, clash);
                }
            }
        }
    }
    out
}

fn add<T: PartialEq>(list: &mut Vec<T>, item: T) {
    if !list.contains(&item) {
        list.push(item);
    }
}

/// Every file below `root`, as a `/`-separated path of [`name_key`]s.
fn files_of(root: &Path) -> Vec<String> {
    fn walk(dir: &Path, rel: &str, out: &mut Vec<String>) {
        for (name, is_dir) in vfs::list_dir(dir).unwrap_or_default() {
            let key = name_key(&name.to_string_lossy());
            let path = if rel.is_empty() {
                key
            } else {
                format!("{rel}/{key}")
            };
            if is_dir {
                walk(&dir.join(&name), &path, out);
            } else {
                out.push(path);
            }
        }
    }
    let mut out = Vec::new();
    walk(root, "", &mut out);
    out
}

/// Maps each folder in `vehicles/` to the sorted `.bus`/`.ovh`/`.zug` files it contains.
fn vehicle_definitions(files: &[String]) -> HashMap<&str, Vec<&str>> {
    let mut out: HashMap<&str, Vec<&str>> = HashMap::new();
    for path in files {
        let parts: Vec<&str> = path.splitn(3, '/').collect();
        if let ["vehicles", folder, file] = parts[..]
            && !file.contains('/')
            && [".bus", ".ovh", ".zug"]
                .iter()
                .any(|extension| file.ends_with(extension))
        {
            out.entry(folder).or_default().push(file);
        }
    }
    for definitions in out.values_mut() {
        definitions.sort_unstable();
    }
    out
}

#[cfg(test)]
mod tests {
    #[test]
    fn vehicle_folders_are_told_apart_by_their_vehicles() {
        let files: Vec<String> = [
            "vehicles/lion/lion.bus",
            "vehicles/lion/model/m.cfg",
            "maps/x/global.cfg",
        ]
        .iter()
        .map(|path| path.to_string())
        .collect();
        let definitions = super::vehicle_definitions(&files);
        assert_eq!(definitions.len(), 1);
        assert_eq!(definitions["lion"], ["lion.bus"]);
    }

    #[test]
    fn originals_are_looked_up_again_when_the_content_changes() {
        let _roots = crate::roots_guard();
        let content = std::env::temp_dir().join(format!("mods-originals-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&content);
        let put = |rel: &str| {
            let path = content.join(rel);
            std::fs::create_dir_all(path.parent().unwrap()).unwrap();
            std::fs::write(path, rel).unwrap();
        };
        put("mods/Bus/Texture/a.dds");
        put("mods/Bus/Texture/b.dds");
        put("omsi/Texture/a.dds");
        let omsi = content.join("omsi");
        crate::init(&content, Some(&omsi)).unwrap();
        let replaced = || {
            let bus = super::conflicts()
                .into_iter()
                .find(|conflict| conflict.id == "Bus");
            bus.unwrap().replaces_original
        };
        assert_eq!(replaced(), 1);
        put("omsi/Texture/b.dds");
        legacy_config::content_changed();
        assert_eq!(replaced(), 2);
        std::fs::remove_dir_all(&content).ok();
    }
}
