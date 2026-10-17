//! The player's load order, kept in `<content>/mods/.load-order.json` (highest priority first).

use legacy_config::name_key;
use serde::{Deserialize, Serialize};
use std::collections::{HashMap, HashSet};
use std::path::Path;

pub const ORDER_FILE: &str = ".load-order.json";

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum NewMods {
    /// The default: a new mod wins, as if it were copied over an OMSI 2 installation.
    #[default]
    Top,
    Bottom,
}

#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize)]
pub struct OrderFile {
    pub version: u32,
    /// Increased on every save, so a save based on an outdated read can be refused.
    pub revision: u64,
    #[serde(default)]
    pub new_mods: NewMods,
    pub order: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Item {
    pub name: String,
    pub available: bool,
}

pub fn load(mods_dir: &Path) -> Result<Option<OrderFile>, String> {
    let text = match std::fs::read_to_string(mods_dir.join(ORDER_FILE)) {
        Ok(text) => text,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(None),
        Err(error) => return Err(error.to_string()),
    };
    let file: OrderFile = serde_json::from_str(&text).map_err(|error| error.to_string())?;
    if file.version != 1 {
        return Err(format!("version {} is not known", file.version));
    }
    Ok(Some(file))
}

/// Saves the order. A file that could not be read is kept as `.load-order.json.bad-<time>`.
pub fn save(mods_dir: &Path, file: &OrderFile) -> std::io::Result<()> {
    let path = mods_dir.join(ORDER_FILE);
    if load(mods_dir).is_err() {
        let secs = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|elapsed| elapsed.as_secs())
            .unwrap_or(0);
        let _ = std::fs::rename(&path, mods_dir.join(format!("{ORDER_FILE}.bad-{secs}")));
    }
    crate::write_json(&path, file)
}

/// The saved order, with entries it doesn't list yet added at the top or bottom.
pub fn effective(present: &[String], saved: Option<&OrderFile>) -> Vec<Item> {
    let Some(saved) = saved else {
        return present
            .iter()
            .map(|name| Item {
                name: name.clone(),
                available: true,
            })
            .collect();
    };
    let mut on_disk: HashMap<String, &String> = HashMap::new();
    for name in present {
        on_disk.entry(name_key(name)).or_insert(name);
    }
    let mut seen: HashSet<String> = HashSet::new();
    let mut listed: Vec<Item> = Vec::new();
    for name in &saved.order {
        let key = name_key(name);
        // Older versions saved `shared` as an ordinary entry.
        if crate::is_reserved(name) || seen.contains(&key) {
            continue;
        }
        let found = on_disk.get(&key).copied();
        listed.push(Item {
            name: found.unwrap_or(name).clone(),
            available: found.is_some(),
        });
        seen.insert(key);
    }
    let new = present
        .iter()
        .filter(|name| !seen.contains(&name_key(name)))
        .map(|name| Item {
            name: name.clone(),
            available: true,
        });
    match saved.new_mods {
        NewMods::Top => new.chain(listed).collect(),
        NewMods::Bottom => listed.into_iter().chain(new).collect(),
    }
}

fn write(
    content: &Path,
    change: impl FnOnce(&crate::LoadOrder) -> Result<(Vec<String>, NewMods), String>,
) -> Result<(), String> {
    static WRITE: std::sync::Mutex<()> = std::sync::Mutex::new(());
    let _one = WRITE
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner());
    let current = crate::load_order(content);
    let (order, new_mods) = change(&current)?;
    if current
        .saved
        .as_ref()
        .is_some_and(|saved| saved.order == order && saved.new_mods == new_mods)
    {
        return Ok(());
    }
    let file = OrderFile {
        version: 1,
        revision: revision(&current) + 1,
        new_mods,
        order,
    };
    save(&content.join(crate::MODS_DIR), &file).map_err(|error| error.to_string())
}

pub fn revision(current: &crate::LoadOrder) -> u64 {
    current
        .saved
        .as_ref()
        .map(|saved| saved.revision)
        .unwrap_or(0)
}

fn listed(current: &crate::LoadOrder) -> Vec<String> {
    current.items.iter().map(|item| item.name.clone()).collect()
}

fn placement(current: &crate::LoadOrder) -> NewMods {
    current
        .saved
        .as_ref()
        .map(|saved| saved.new_mods)
        .unwrap_or_default()
}

fn unchanged_since(current: &crate::LoadOrder, read: u64) -> Result<(), String> {
    if read != revision(current) {
        return Err("the load order was changed elsewhere - it was read again".into());
    }
    Ok(())
}

/// Saves the order in effect, so that the next new mod is placed relative to it.
pub fn materialize(content: &Path) -> Result<(), String> {
    write(content, |current| Ok((listed(current), placement(current))))
}

pub fn forget(content: &Path, name: &str) -> Result<(), String> {
    write(content, |current| {
        let mut order = listed(current);
        order.retain(|entry| name_key(entry) != name_key(name));
        Ok((order, placement(current)))
    })
}

pub fn rename(content: &Path, old: &str, new: &str) -> Result<(), String> {
    write(content, |current| {
        let mut order = listed(current);
        order.retain(|entry| name_key(entry) != name_key(new));
        if let Some(index) = order
            .iter()
            .position(|entry| name_key(entry) == name_key(old))
        {
            order[index] = new.to_string();
        }
        Ok((order, placement(current)))
    })
}

pub fn set(content: &Path, order: &[String], new_mods: NewMods, read: u64) -> Result<(), String> {
    write(content, |current| {
        unchanged_since(current, read)?;
        let known: HashSet<String> = current
            .items
            .iter()
            .map(|item| name_key(&item.name))
            .collect();
        let mut seen: HashSet<String> = HashSet::new();
        // Entries missing from the player's list keep their current relative order at the end.
        let out = order
            .iter()
            .cloned()
            .chain(listed(current))
            .filter(|name| {
                let key = name_key(name);
                known.contains(&key) && seen.insert(key)
            })
            .collect();
        Ok((out, new_mods))
    })
}

pub fn reset(content: &Path, read: u64) -> Result<(), String> {
    write(content, |current| {
        unchanged_since(current, read)?;
        Ok((
            current
                .entries
                .iter()
                .map(|entry| entry.id.clone())
                .collect(),
            placement(current),
        ))
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn names(items: &[Item]) -> Vec<(&str, bool)> {
        items
            .iter()
            .map(|item| (item.name.as_str(), item.available))
            .collect()
    }

    fn present(names: &[&str]) -> Vec<String> {
        names.iter().map(|name| name.to_string()).collect()
    }

    #[test]
    fn without_a_saved_order_the_content_keeps_its_order() {
        let entries = present(&["Archives/A.zip", "alpha", "Beta.zip"]);
        assert_eq!(
            effective(&entries, None)
                .iter()
                .map(|item| &item.name)
                .collect::<Vec<_>>(),
            entries.iter().collect::<Vec<_>>()
        );
    }

    #[test]
    fn the_players_order_is_kept_and_new_mods_go_where_chosen() {
        let saved = OrderFile {
            version: 1,
            order: present(&["mod2", "Gone", "mod1"]),
            ..Default::default()
        };
        let entries = present(&["mod1", "MOD2", "new_a", "new_b"]);
        assert_eq!(
            names(&effective(&entries, Some(&saved))),
            [
                ("new_a", true),
                ("new_b", true),
                ("MOD2", true),
                ("Gone", false),
                ("mod1", true)
            ]
        );
        let bottom = OrderFile {
            new_mods: NewMods::Bottom,
            ..saved
        };
        assert_eq!(
            names(&effective(&entries, Some(&bottom))),
            [
                ("MOD2", true),
                ("Gone", false),
                ("mod1", true),
                ("new_a", true),
                ("new_b", true)
            ]
        );
    }

    #[test]
    fn an_order_saved_when_shared_was_a_mod_drops_it() {
        let saved = OrderFile {
            version: 1,
            order: present(&["Bus", "Shared"]),
            ..Default::default()
        };
        assert_eq!(
            names(&effective(&present(&["Bus"]), Some(&saved))),
            [("Bus", true)]
        );
    }

    #[test]
    fn the_order_is_changed_one_read_at_a_time() {
        let content = std::env::temp_dir().join(format!("mods-order-write-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&content);
        let mods = content.join(crate::MODS_DIR);
        std::fs::create_dir_all(mods.join("A")).unwrap();
        std::fs::write(mods.join("B.zip"), b"").unwrap();
        let order = || load(&mods).unwrap().unwrap().order;
        materialize(&content).unwrap();
        assert_eq!(order(), ["A", "B.zip"]);
        let read = load(&mods).unwrap().unwrap().revision;
        set(&content, &present(&["B.zip", "A"]), NewMods::Top, read).unwrap();
        assert!(
            set(&content, &present(&["A"]), NewMods::Top, read).is_err(),
            "stale read"
        );
        assert_eq!(order(), ["B.zip", "A"]);
        // A mod that appears goes to the top, and can be forgotten again.
        std::fs::create_dir_all(mods.join("C")).unwrap();
        materialize(&content).unwrap();
        assert_eq!(order(), ["C", "B.zip", "A"]);
        forget(&content, "a").unwrap();
        assert_eq!(order(), ["C", "B.zip"]);
        reset(&content, load(&mods).unwrap().unwrap().revision).unwrap();
        assert_eq!(order(), ["A", "B.zip", "C"]);
        std::fs::remove_dir_all(&content).ok();
    }

    #[test]
    fn a_file_that_cannot_be_read_is_reported_and_kept() {
        let dir = std::env::temp_dir().join(format!("mods-order-file-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        assert_eq!(load(&dir), Ok(None));
        std::fs::write(dir.join(ORDER_FILE), b"{ not json").unwrap();
        assert!(load(&dir).is_err());
        let file = OrderFile {
            version: 1,
            revision: 3,
            order: present(&["a"]),
            ..Default::default()
        };
        save(&dir, &file).unwrap();
        assert_eq!(load(&dir), Ok(Some(file)));
        let kept = std::fs::read_dir(&dir)
            .unwrap()
            .flatten()
            .filter(|entry| entry.file_name().to_string_lossy().contains(".bad-"))
            .count();
        assert_eq!(kept, 1);
        std::fs::remove_dir_all(&dir).ok();
    }
}
