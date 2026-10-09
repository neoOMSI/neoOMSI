use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};

/// The oldest pack this game reads.
const VERSION: u64 = 2;
/// How far `refresh` counts up past the newest pack it knows.
const LOOK_AHEAD: u64 = 20;

fn tag(version: u64) -> String {
    format!("realistic-pax-v{version}")
}

fn file(version: u64) -> String {
    format!("RealisticPax-v{version}.zip")
}

#[derive(Clone, Debug, PartialEq)]
pub struct PaxRelease {
    pub version: u64,
    pub notes: String,
    pub page: String,
    pub published: String,
}

static LATEST: Mutex<Option<PaxRelease>> = Mutex::new(None);

pub fn latest() -> Option<PaxRelease> {
    LATEST.lock().unwrap_or_else(|e| e.into_inner()).clone()
}

fn release(version: u64) -> Option<PaxRelease> {
    let url = format!(
        "https://api.github.com/repos/{}/releases/tags/{}",
        crate::updater::REPO,
        tag(version)
    );
    let v: serde_json::Value = serde_json::from_str(&crate::updater::fetch_text(&url).ok()?).ok()?;
    let has_file = v["assets"]
        .as_array()?
        .iter()
        .any(|a| a["name"].as_str() == Some(file(version).as_str()));
    if v["draft"].as_bool() == Some(true) || !has_file {
        return None;
    }
    let text = |k: &str| v[k].as_str().unwrap_or("").to_string();
    Some(PaxRelease {
        version,
        notes: text("body"),
        page: text("html_url"),
        published: text("published_at"),
    })
}

pub fn refresh() {
    let from = latest().map_or(VERSION, |r| r.version);
    let mut found = None;
    for n in from..from + LOOK_AHEAD {
        match release(n) {
            Some(r) => found = Some(r),
            None => break,
        }
    }
    if let Some(r) = found {
        *LATEST.lock().unwrap_or_else(|e| e.into_inner()) = Some(r);
    }
}

/// None for a pack without `pack.json` (put together by hand: never offered an update).
pub fn installed_version(content: &Path) -> Option<u64> {
    manifest(&folder(content)).map(|m| m["version"].as_u64().unwrap_or(0))
}

#[derive(Clone, Debug, PartialEq)]
pub enum Status {
    Missing,
    Outdated,
    Downloading { done: u64, total: u64 },
    Installing,
    Installed,
    Failed(String),
}

pub fn folder(content: &Path) -> PathBuf {
    content.join("Packs").join("RealisticPax")
}

fn status_of(content: &Path) -> Status {
    let dir = folder(content);
    if !dir.join("Humans").is_dir() {
        return Status::Missing;
    }
    match installed_version(content) {
        Some(v) if v < VERSION || latest().is_some_and(|l| l.version > v) => Status::Outdated,
        _ => Status::Installed,
    }
}

fn manifest(dir: &Path) -> Option<serde_json::Value> {
    std::fs::read_to_string(dir.join("pack.json"))
        .ok()
        .and_then(|t| serde_json::from_str(&t).ok())
}

pub struct PaxPack {
    content: Option<PathBuf>,
    status: Arc<Mutex<Status>>,
}

fn lock(s: &Mutex<Status>) -> std::sync::MutexGuard<'_, Status> {
    s.lock().unwrap_or_else(|e| e.into_inner())
}

impl PaxPack {
    pub fn new(content: Option<PathBuf>) -> PaxPack {
        let status = content.as_deref().map_or(Status::Missing, status_of);
        PaxPack {
            content,
            status: Arc::new(Mutex::new(status)),
        }
    }

    pub fn status(&self) -> Status {
        lock(&self.status).clone()
    }

    /// Download and install it (in the background).
    pub fn start(&mut self) {
        if matches!(
            self.status(),
            Status::Downloading { .. } | Status::Installing | Status::Installed
        ) {
            return;
        }
        let Some(content) = self.content.clone() else {
            *lock(&self.status) = Status::Failed("There is no content folder to put it in.".into());
            return;
        };
        *lock(&self.status) = Status::Downloading { done: 0, total: 0 };
        let status = self.status.clone();
        std::thread::spawn(move || match install(&content, &status) {
            Ok(()) => {
                log::info!(
                    "realistic passengers installed in {}",
                    folder(&content).display()
                );
                *lock(&status) = Status::Installed;
            }
            Err(e) => {
                log::warn!("realistic passengers: {e:#}");
                *lock(&status) =
                    Status::Failed(format!("The realistic passengers were not installed: {e}"));
            }
        });
    }
}

fn install(content: &Path, status: &Mutex<Status>) -> anyhow::Result<()> {
    refresh();
    let version = latest().map_or(VERSION, |r| r.version).max(VERSION);
    // OMSI_PAX_PACK_URL: another archive (`file://` too), unchecked
    let (url, size, sha256) = match legacy_config::env::var("OMSI_PAX_PACK_URL") {
        Ok(url) => (url, 0, None),
        Err(_) => release_file(version)
            .map_err(|e| anyhow::anyhow!("they are not available for download yet ({e})"))?,
    };
    let zip = crate::updater::download_dir().join(file(version));
    fetch_and_place(content, &url, size, sha256.as_deref(), &zip, version, status)
}

/// Its address, size and SHA-256 as GitHub lists them.
fn release_file(version: u64) -> anyhow::Result<(String, u64, Option<String>)> {
    let (tag, name) = (tag(version), file(version));
    let url = format!(
        "https://api.github.com/repos/{}/releases/tags/{tag}",
        crate::updater::REPO
    );
    let v: serde_json::Value = serde_json::from_str(&crate::updater::fetch_text(&url)?)?;
    let a = v["assets"]
        .as_array()
        .and_then(|a| a.iter().find(|a| a["name"].as_str() == Some(name.as_str())))
        .ok_or_else(|| anyhow::anyhow!("the release {tag} has no {name}"))?;
    Ok((
        a["browser_download_url"]
            .as_str()
            .ok_or_else(|| anyhow::anyhow!("{name} has no address"))?
            .to_string(),
        a["size"].as_u64().unwrap_or(0),
        a["digest"]
            .as_str()
            .and_then(|d| d.strip_prefix("sha256:"))
            .map(|h| h.to_ascii_lowercase()),
    ))
}

fn fetch_and_place(
    content: &Path,
    url: &str,
    size: u64,
    sha256: Option<&str>,
    zip: &Path,
    version: u64,
    status: &Mutex<Status>,
) -> anyhow::Result<()> {
    crate::updater::fetch_file(url, size, sha256, zip, &mut |done, total| {
        *lock(status) = Status::Downloading { done, total }
    })?;
    *lock(status) = Status::Installing;
    let packs = content.join("Packs");
    std::fs::create_dir_all(&packs)?;
    let staging = packs.join(".RealisticPax-new");
    let result = place(
        zip,
        &staging,
        &folder(content),
        &packs.join(".RealisticPax-old"),
        version,
    );
    let _ = std::fs::remove_dir_all(&staging);
    let _ = std::fs::remove_file(zip);
    result
}

/// Unpacked beside the pack first: a failed download leaves the old pack as it was.
fn place(zip: &Path, staging: &Path, dest: &Path, old: &Path, version: u64) -> anyhow::Result<()> {
    crate::updater::unpack(zip, staging)?;
    let root = if staging.join("RealisticPax").is_dir() {
        staging.join("RealisticPax")
    } else {
        staging.to_path_buf()
    };
    if !root.join("Humans").is_dir() {
        anyhow::bail!("the archive holds no passengers");
    }
    let Some(m) = manifest(&root) else {
        anyhow::bail!("the archive has no readable pack.json");
    };
    if m["name"] != "RealisticPax" || m["version"].as_u64() != Some(version) {
        anyhow::bail!("the archive is not version {version} of the pack (its pack.json: {m})");
    }
    let _ = std::fs::remove_dir_all(old);
    if dest.exists() {
        std::fs::rename(dest, old)?;
    }
    if let Err(e) = std::fs::rename(&root, dest) {
        let _ = std::fs::rename(old, dest);
        return Err(e.into());
    }
    let _ = std::fs::remove_dir_all(old);
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;

    /// `LATEST` is the whole process's.
    static ONE_AT_A_TIME: Mutex<()> = Mutex::new(());

    fn archive(path: &Path, files: &[(&str, &str)]) {
        let mut z = zip::ZipWriter::new(std::fs::File::create(path).unwrap());
        for (name, text) in files {
            z.start_file(*name, zip::write::SimpleFileOptions::default())
                .unwrap();
            z.write_all(text.as_bytes()).unwrap();
        }
        z.finish().unwrap();
    }

    #[test]
    fn a_pack_replaces_the_old_one_and_a_bad_archive_keeps_it() {
        let _one = ONE_AT_A_TIME.lock().unwrap_or_else(|e| e.into_inner());
        let dir = std::env::temp_dir().join(format!("omsi-pax-pack-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        let content = dir.join("content");
        assert_eq!(status_of(&content), Status::Missing);
        let dest = folder(&content);
        std::fs::create_dir_all(dest.join("Humans")).unwrap();
        std::fs::write(dest.join("old.txt"), "").unwrap();
        // built by hand: no pack.json, the player's
        assert_eq!(status_of(&content), Status::Installed);
        std::fs::write(dest.join("pack.json"), r#"{"version": 0}"#).unwrap();
        assert_eq!(status_of(&content), Status::Outdated);

        let (staging, old) = (dir.join("staging"), dir.join("old"));
        let bad = dir.join("bad.zip");
        archive(&bad, &[("readme.txt", "")]);
        assert!(place(&bad, &staging, &dest, &old, VERSION).is_err());
        assert!(dest.join("old.txt").exists());
        for manifest in [
            None,
            Some("not json"),
            Some(r#"{"name": "RealisticPax"}"#),
            Some(r#"{"name": "RealisticPax", "version": 99}"#),
            Some(r#"{"name": "Other", "version": 1}"#),
        ] {
            let mut files = vec![("RealisticPax/Humans/Other/man01.hum", "[model]
")];
            files.extend(manifest.map(|m| ("RealisticPax/pack.json", m)));
            archive(&bad, &files);
            assert!(place(&bad, &staging, &dest, &old, VERSION).is_err(), "{manifest:?}");
            let _ = std::fs::remove_dir_all(&staging);
            assert!(dest.join("old.txt").exists());
        }

        let good = dir.join("good.zip");
        let manifest = format!(r#"{{"name": "RealisticPax", "version": {VERSION}}}"#);
        archive(
            &good,
            &[
                ("RealisticPax/pack.json", &manifest),
                ("RealisticPax/Humans/Other/man01.hum", "[model]\n"),
            ],
        );
        place(&good, &staging, &dest, &old, VERSION).unwrap();
        assert!(dest.join("Humans/Other/man01.hum").exists() && !dest.join("old.txt").exists());
        assert!(!old.exists());
        assert_eq!(status_of(&content), Status::Installed);
        assert_eq!(installed_version(&content), Some(VERSION));
        *LATEST.lock().unwrap() = Some(PaxRelease {
            version: VERSION + 1,
            notes: String::new(),
            page: String::new(),
            published: String::new(),
        });
        assert_eq!(status_of(&content), Status::Outdated, "a newer pack is out");
        *LATEST.lock().unwrap() = None;
        std::fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn a_download_is_checked_before_it_replaces_anything() {
        use sha2::Digest;
        let _one = ONE_AT_A_TIME.lock().unwrap_or_else(|e| e.into_inner());
        let dir = std::env::temp_dir().join(format!("omsi-pax-fetch-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let content = dir.join("content");
        let source = dir.join(file(VERSION));
        let manifest = format!(r#"{{"name": "RealisticPax", "version": {VERSION}}}"#);
        archive(
            &source,
            &[
                ("RealisticPax/pack.json", &manifest),
                ("RealisticPax/Humans/Other/man01.hum", "[model]\n"),
            ],
        );
        let bytes = std::fs::read(&source).unwrap();
        let sha: String = sha2::Sha256::digest(&bytes)
            .iter()
            .map(|b| format!("{b:02x}"))
            .collect();
        let url = format!("file://{}", source.display());
        let status = Mutex::new(Status::Missing);
        let zip = dir.join("download.zip");
        let wrong = "0".repeat(64);
        assert!(fetch_and_place(&content, &url, 0, Some(&wrong), &zip, VERSION, &status).is_err());
        assert_eq!(status_of(&content), Status::Missing);
        fetch_and_place(
            &content,
            &url,
            bytes.len() as u64,
            Some(&sha),
            &zip,
            VERSION,
            &status,
        )
        .unwrap();
        assert_eq!(status_of(&content), Status::Installed);
        assert!(folder(&content).join("Humans/Other/man01.hum").exists() && !zip.exists());
        std::fs::remove_dir_all(&dir).unwrap();
    }
}
