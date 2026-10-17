//! Files that a loader needed but couldn't find, grouped by their likely cause.

use crate::{MODS_DIR, UNINSTALLED_DIR, content_roots, folder_content_root, owner_root};
use legacy_config::{resolve_existing, vfs};
use serde::{Deserialize, Serialize};
use std::collections::{BTreeSet, HashMap};
use std::path::{Path, PathBuf};
use std::sync::Mutex;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Kind {
    Tile,
    Object,
    Spline,
    Model,
    Mesh,
    Texture,
    Script,
    Sound,
    /// A vehicle named in a map's AI lists.
    Vehicle,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Severity {
    Optional,
    Degraded,
    Required,
}

impl Kind {
    fn severity(self) -> Severity {
        match self {
            Kind::Sound => Severity::Optional,
            Kind::Mesh | Kind::Texture => Severity::Degraded,
            Kind::Tile
            | Kind::Object
            | Kind::Spline
            | Kind::Model
            | Kind::Script
            | Kind::Vehicle => Severity::Required,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Class {
    MissingFile,
    IncompleteMod,
    IncompletePack,
    InactiveDependency,
    MissingDependency,
}

impl Class {
    pub fn as_str(self) -> &'static str {
        match self {
            Class::MissingFile => "missing_file",
            Class::IncompleteMod => "incomplete_mod",
            Class::IncompletePack => "incomplete_pack",
            Class::InactiveDependency => "inactive_dependency",
            Class::MissingDependency => "missing_dependency",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Finding {
    pub class: Class,
    /// The entry that asked for the files; `OMSI 2` for the original installation.
    pub requester: String,
    pub pack: Option<String>,
    pub provider: Option<String>,
    pub severity: Severity,
    pub count: usize,
    pub samples: Vec<String>,
}

struct Miss {
    requester: String,
    requester_root: Option<PathBuf>,
    path: String,
    kind: Kind,
}

/// The recorded misses, and a (requester, lower-case path) set to skip repeats.
type Misses = (Vec<Miss>, BTreeSet<(String, String)>);
static MISSES: Mutex<Misses> = Mutex::new((Vec::new(), BTreeSet::new()));
const MAX_MISSES: usize = 5000;
const SAMPLES: usize = 5;

fn miss(requester: &Path, wanted: &Path, kind: Kind, roots: &[PathBuf]) -> Miss {
    let requester_root = owner_root(requester, roots).cloned();
    let path = match owner_root(wanted, roots) {
        Some(root) => content_path(wanted, root),
        None => wanted.display().to_string(),
    };
    Miss {
        requester: entry_name(requester_root.as_deref()),
        requester_root,
        path,
        kind,
    }
}

/// Records that `wanted`, which `requester` needed, could not be loaded.
pub fn missing(requester: &Path, wanted: &Path, kind: Kind) {
    // A file that exists but failed to load is damaged, not missing.
    if vfs::exists(wanted) {
        return;
    }
    let found = miss(requester, wanted, kind, &content_roots());
    let mut misses = MISSES
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner());
    let (list, seen) = &mut *misses;
    let key = (found.requester.clone(), found.path.to_ascii_lowercase());
    if list.len() >= MAX_MISSES || !seen.insert(key) {
        return;
    }
    log::warn!("{}: {} not found", found.requester, found.path);
    list.push(found);
}

/// Forgets the recorded misses, for a new drive.
pub fn clear() {
    *MISSES
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner()) = (Vec::new(), BTreeSet::new());
}

pub fn findings() -> Vec<Finding> {
    group(
        &MISSES
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .0,
    )
}

/// Classifies the files in `wanted` that don't exist, without recording them as misses.
pub fn check(requester: &Path, wanted: &[(PathBuf, Kind)]) -> Vec<Finding> {
    let roots = content_roots();
    let misses: Vec<Miss> = wanted
        .iter()
        .filter(|(path, _)| !vfs::exists(path))
        .map(|(path, kind)| miss(requester, path, *kind, &roots))
        .collect();
    group(&misses)
}

fn group(misses: &[Miss]) -> Vec<Finding> {
    let mut places = Places::new();
    let mut out: Vec<Finding> = Vec::new();
    for missed in misses.iter() {
        let pack = pack_of(&missed.path);
        let (class, provider) = places.classify(missed.requester_root.as_deref(), pack.as_deref());
        let group = out.iter_mut().find(|finding| {
            finding.requester == missed.requester
                && finding.class == class
                && match &pack {
                    Some(pack) => finding
                        .pack
                        .as_deref()
                        .is_some_and(|other| other.eq_ignore_ascii_case(pack)),
                    None => finding.pack.is_none(),
                }
        });
        let finding = match group {
            Some(finding) => finding,
            None => {
                out.push(Finding {
                    class,
                    requester: missed.requester.clone(),
                    pack: pack.clone(),
                    provider,
                    severity: missed.kind.severity(),
                    count: 0,
                    samples: Vec::new(),
                });
                out.last_mut().unwrap()
            }
        };
        finding.count += 1;
        finding.severity = finding.severity.max(missed.kind.severity());
        if finding.samples.len() < SAMPLES {
            finding.samples.push(missed.path.clone());
        }
    }
    out
}

/// A one-line message for the player, or `None` when only optional files are missing.
pub fn summary(findings: &[Finding]) -> Option<String> {
    let findings: Vec<&Finding> = findings
        .iter()
        .filter(|finding| finding.severity > Severity::Optional)
        .collect();
    let packs: Vec<&str> = findings
        .iter()
        .filter(|finding| {
            matches!(
                finding.class,
                Class::MissingDependency | Class::InactiveDependency
            )
        })
        .filter_map(|finding| finding.pack.as_deref())
        .collect();
    let incomplete = findings
        .iter()
        .filter(|finding| matches!(finding.class, Class::IncompleteMod | Class::IncompletePack))
        .count();
    let files = findings
        .iter()
        .filter(|finding| finding.class == Class::MissingFile)
        .count();
    let mut parts = Vec::new();
    if !packs.is_empty() {
        let mut shown = packs[..packs.len().min(3)].join(", ");
        if packs.len() > 3 {
            shown.push_str(", ...");
        }
        parts.push(format!("needs packs that are not in use: {shown}"));
    }
    if incomplete > 0 {
        parts.push(format!("{incomplete} incomplete mod(s) or pack(s)"));
    }
    if files > 0 {
        parts.push(format!("files missing in {files} place(s)"));
    }
    // (the game says where to look, in the player's language)
    (!parts.is_empty()).then(|| format!("Content {}", parts.join("; ")))
}

impl Finding {
    /// Identifies the finding for dismissal. It leaves out the sample files, so a dismissed
    /// finding stays dismissed, but includes the class, so a change of cause shows it again.
    pub fn key(&self) -> String {
        format!(
            "{}|{}|{}",
            self.requester,
            self.class.as_str(),
            self.pack.as_deref().unwrap_or_default()
        )
    }
}

/// Adds `more` to `findings`. Two findings with the same key may name the same files, so the merge
/// keeps the larger count, since at least that many are missing.
pub fn merge(findings: &mut Vec<Finding>, more: Vec<Finding>) {
    for finding in more {
        let Some(known) = findings.iter_mut().find(|known| known.key() == finding.key()) else {
            findings.push(finding);
            continue;
        };
        known.count = known.count.max(finding.count);
        known.severity = known.severity.max(finding.severity);
        for sample in finding.samples {
            if known.samples.len() < SAMPLES
                && !known.samples.iter().any(|kept| kept.eq_ignore_ascii_case(&sample))
            {
                known.samples.push(sample);
            }
        }
    }
}

pub fn dismissed(path: &Path) -> Vec<String> {
    std::fs::read(path)
        .ok()
        .and_then(|bytes| serde_json::from_slice(&bytes).ok())
        .unwrap_or_default()
}

pub fn dismiss(path: &Path, key: &str) -> std::io::Result<()> {
    let mut keys = dismissed(path);
    if !keys.iter().any(|dismissed_key| dismissed_key == key) {
        keys.push(key.to_string());
    }
    crate::write_json(path, &keys)
}

pub fn write(path: &Path, findings: &[Finding]) -> std::io::Result<()> {
    crate::write_json(path, &findings)
}

fn content_path(path: &Path, root: &Path) -> String {
    path.strip_prefix(root)
        .map(|rel| {
            rel.components()
                .map(|component| component.as_os_str().to_string_lossy())
                .collect::<Vec<_>>()
                .join("/")
        })
        .unwrap_or_default()
}

fn entry_name(root: Option<&Path>) -> String {
    let Some(root) = root else {
        return "?".into();
    };
    let state = crate::STATE.read().unwrap();
    if let Some(state) = state.as_ref() {
        if let Some((entry, _)) = state.roots.iter().find(|(_, mounted)| mounted == root) {
            return entry.id.clone();
        }
        if state.legacy.as_deref() == Some(root) {
            return "OMSI 2".into();
        }
    }
    root.file_name()
        .unwrap_or_default()
        .to_string_lossy()
        .into_owned()
}

fn pack_of(path: &str) -> Option<String> {
    let mut parts = path.split('/');
    let (first, pack) = (parts.next()?, parts.next()?);
    parts.next()?;
    let folder = crate::layout::PACK_FOLDERS
        .iter()
        .find(|known| known.eq_ignore_ascii_case(first))?;
    Some(format!("{folder}/{pack}"))
}

/// Caches where each pack lives, because a big map often misses hundreds of files from a handful of
/// packs.
struct Places {
    roots: Vec<PathBuf>,
    /// The entries of `mods/uninstalled`, read the first time they are needed.
    kept: Option<Vec<(String, Kept)>>,
    known: HashMap<(Option<PathBuf>, String), (Class, Option<String>)>,
}

enum Kept {
    Folder(PathBuf),
    Zip(vfs::ZipArchive),
}

impl Places {
    fn new() -> Places {
        Places {
            roots: content_roots(),
            kept: None,
            known: HashMap::new(),
        }
    }

    fn classify(
        &mut self,
        requester_root: Option<&Path>,
        pack: Option<&str>,
    ) -> (Class, Option<String>) {
        let Some(pack) = pack else {
            return (Class::MissingFile, None);
        };
        let key = (requester_root.map(Path::to_path_buf), pack.to_string());
        if let Some(known) = self.known.get(&key) {
            return known.clone();
        }
        let comps: Vec<&str> = pack.split('/').collect();
        let has = |root: &Path| resolve_existing(root, &comps).is_some_and(|dir| vfs::is_dir(&dir));
        let class = if requester_root.is_some_and(has) {
            (Class::IncompleteMod, None)
        } else if let Some(root) = self.roots.iter().find(|root| has(root)) {
            (Class::IncompletePack, Some(entry_name(Some(root))))
        } else if let Some((place, _)) =
            self.kept
                .get_or_insert_with(uninstalled)
                .iter()
                .find(|(_, kept)| match kept {
                    Kept::Folder(root) => resolve_existing(root, &comps).is_some(),
                    Kept::Zip(archive) => archive.contains(pack),
                })
        {
            (Class::InactiveDependency, Some(place.clone()))
        } else {
            (Class::MissingDependency, None)
        };
        self.known.insert(key, class.clone());
        class
    }
}

fn uninstalled() -> Vec<(String, Kept)> {
    let Some(dir) = crate::mods_dir().map(|mods| mods.join(UNINSTALLED_DIR)) else {
        return Vec::new();
    };
    std::fs::read_dir(dir)
        .into_iter()
        .flatten()
        .flatten()
        .filter_map(|dir_entry| {
            let path = dir_entry.path();
            let kept = if path.is_dir() {
                Kept::Folder(folder_content_root(&path))
            } else {
                Kept::Zip(vfs::ZipArchive::open(&path).ok()?)
            };
            let place = format!(
                "{MODS_DIR}/{UNINSTALLED_DIR}/{}",
                dir_entry.file_name().to_string_lossy()
            );
            Some((place, kept))
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_file_in_a_pack_names_the_pack() {
        assert_eq!(
            pack_of("Sceneryobjects/Signs/a.sco").as_deref(),
            Some("Sceneryobjects/Signs")
        );
        assert_eq!(
            pack_of("vehicles/Bus/model/m.o3d").as_deref(),
            Some("Vehicles/Bus")
        );
        assert_eq!(pack_of("Texture/road.dds"), None);
        assert_eq!(pack_of("Sceneryobjects/a.sco"), None);
    }

    #[test]
    fn a_look_before_the_start_reports_only_what_is_not_there() {
        let _roots = crate::roots_guard();
        let dir = std::env::temp_dir().join(format!("mods-check-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        let (map_mod, omsi) = (dir.join("map"), dir.join("omsi"));
        std::fs::create_dir_all(map_mod.join("maps/Town")).unwrap();
        std::fs::create_dir_all(omsi.join("Sceneryobjects/Stock")).unwrap();
        std::fs::write(omsi.join("Sceneryobjects/Stock/a.sco"), b"").unwrap();
        for root in [&map_mod, &omsi] {
            crate::add_content_root(root.clone());
        }
        let found = check(
            &map_mod.join("maps/Town"),
            &[
                (omsi.join("Sceneryobjects/Stock/a.sco"), Kind::Object),
                (omsi.join("Vehicles/OnlyInThisCheck/x.bus"), Kind::Vehicle),
            ],
        );
        assert_eq!(found.len(), 1, "{found:?}");
        assert_eq!(
            (found[0].class, found[0].pack.as_deref()),
            (Class::MissingDependency, Some("Vehicles/OnlyInThisCheck"))
        );
        for root in [&map_mod, &omsi] {
            crate::remove_content_root(root);
        }
        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn a_silent_sound_alone_is_not_worth_a_message() {
        let finding = |severity| Finding {
            class: Class::MissingDependency,
            requester: "Bus".into(),
            pack: Some("Vehicles/Sounds".into()),
            provider: None,
            severity,
            count: 1,
            samples: Vec::new(),
        };
        assert_eq!(summary(&[finding(Severity::Optional)]), None);
        assert!(summary(&[finding(Severity::Degraded)]).is_some());
    }

    #[test]
    fn a_dismissal_outlives_which_file_was_missed_first() {
        let finding = |sample: &str| Finding {
            class: Class::MissingFile,
            requester: "Town".into(),
            pack: None,
            provider: None,
            severity: Severity::Degraded,
            count: 1,
            samples: vec![sample.into()],
        };
        assert_eq!(
            finding("Texture/a.dds").key(),
            finding("Texture/b.dds").key()
        );
    }

    #[test]
    fn merging_keeps_the_files_of_both() {
        let finding = |count, samples: &[&str]| Finding {
            class: Class::MissingFile,
            requester: "Town".into(),
            pack: None,
            provider: None,
            severity: Severity::Degraded,
            count,
            samples: samples.iter().map(|s| s.to_string()).collect(),
        };
        let mut found = vec![finding(2, &["Texture/a.dds", "Texture/b.dds"])];
        merge(&mut found, vec![finding(3, &["texture/A.dds", "Texture/c.dds"])]);
        assert_eq!(found.len(), 1);
        assert_eq!(found[0].count, 3);
        assert_eq!(found[0].samples, ["Texture/a.dds", "Texture/b.dds", "Texture/c.dds"]);
    }

    #[test]
    fn misses_are_told_apart_by_where_the_pack_is() {
        let _roots = crate::roots_guard();
        let dir = std::env::temp_dir().join(format!("mods-diag-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        let (map_mod, signs, omsi) = (dir.join("map"), dir.join("signs"), dir.join("omsi"));
        std::fs::create_dir_all(map_mod.join("maps/Town")).unwrap();
        std::fs::create_dir_all(map_mod.join("Sceneryobjects/TownObjects")).unwrap();
        std::fs::create_dir_all(signs.join("Sceneryobjects/Signs")).unwrap();
        std::fs::create_dir_all(&omsi).unwrap();
        for root in [&map_mod, &signs, &omsi] {
            crate::add_content_root(root.clone());
        }
        let map = map_mod.join("maps/Town");
        let in_omsi = |rel: &str| omsi.join(rel);
        let mut places = Places::new();
        let mut classify = |pack: &str| places.classify(Some(&map_mod), pack_of(pack).as_deref());
        let (own, other, nowhere) = (
            classify("Sceneryobjects/TownObjects/x.sco"),
            classify("Sceneryobjects/Signs/y.sco"),
            classify("Splines/Hamburg/z.sli"),
        );
        assert_eq!(own.0, Class::IncompleteMod);
        assert_eq!(other.0, Class::IncompletePack);
        assert_eq!(nowhere, (Class::MissingDependency, None));
        assert_eq!(
            Places::new()
                .classify(None, pack_of("Texture/a.dds").as_deref())
                .0,
            Class::MissingFile
        );
        // Each file is recorded once per requester, and files that exist are not recorded.
        missing(&map, &in_omsi("Splines/Hamburg/z.sli"), Kind::Spline);
        missing(&map, &in_omsi("Splines/Hamburg/z.sli"), Kind::Spline);
        missing(&map, &in_omsi("Splines/Hamburg/w.sli"), Kind::Spline);
        missing(&map, &signs.join("Sceneryobjects/Signs"), Kind::Object);
        let hamburg: Vec<Finding> = findings()
            .into_iter()
            .filter(|finding| finding.pack.as_deref() == Some("Splines/Hamburg"))
            .collect();
        assert_eq!(hamburg.len(), 1);
        assert_eq!(
            (hamburg[0].count, hamburg[0].severity),
            (2, Severity::Required)
        );
        assert!(summary(&hamburg).unwrap().contains("Splines/Hamburg"));
        for root in [&map_mod, &signs, &omsi] {
            crate::remove_content_root(root);
        }
        std::fs::remove_dir_all(&dir).ok();
    }
}
