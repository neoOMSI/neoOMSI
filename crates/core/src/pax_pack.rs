use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};

const TAG: &str = "realistic-pax-v2";
const FILE: &str = "RealisticPax-v2.zip";
const VERSION: u64 = 2;

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
    let version = manifest(&dir).map(|v| v["version"].as_u64().unwrap_or(0));
    match version {
        Some(v) if v < VERSION => Status::Outdated,
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
    finished: Arc<AtomicBool>,
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
            finished: Arc::new(AtomicBool::new(false)),
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
        let (status, finished) = (self.status.clone(), self.finished.clone());
        std::thread::spawn(move || match install(&content, &status) {
            Ok(()) => {
                log::info!(
                    "realistic passengers installed in {}",
                    folder(&content).display()
                );
                *lock(&status) = Status::Installed;
                finished.store(true, Ordering::Relaxed);
            }
            Err(e) => {
                log::warn!("realistic passengers: {e:#}");
                *lock(&status) =
                    Status::Failed(format!("The realistic passengers were not installed: {e}"));
            }
        });
    }

    pub fn take_finished(&self) -> bool {
        self.finished.swap(false, Ordering::Relaxed)
    }
}

fn install(content: &Path, status: &Mutex<Status>) -> anyhow::Result<()> {
    // OMSI_PAX_PACK_URL: another archive (`file://` too), unchecked
    let (url, size, sha256) = match legacy_config::env::var("OMSI_PAX_PACK_URL") {
        Ok(url) => (url, 0, None),
        Err(_) => crate::updater::release_file(TAG, FILE)
            .map_err(|e| anyhow::anyhow!("they are not available for download yet ({e})"))?,
    };
    let zip = crate::updater::download_dir().join(FILE);
    fetch_and_place(content, &url, size, sha256.as_deref(), &zip, status)
}

fn fetch_and_place(
    content: &Path,
    url: &str,
    size: u64,
    sha256: Option<&str>,
    zip: &Path,
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
    );
    let _ = std::fs::remove_dir_all(&staging);
    let _ = std::fs::remove_file(zip);
    result
}

/// Unpacked beside the pack first: a failed download leaves the old pack as it was.
fn place(zip: &Path, staging: &Path, dest: &Path, old: &Path) -> anyhow::Result<()> {
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
    if m["name"] != "RealisticPax" || m["version"].as_u64() != Some(VERSION) {
        anyhow::bail!("the archive is not version {VERSION} of the pack (its pack.json: {m})");
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
        assert!(place(&bad, &staging, &dest, &old).is_err());
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
            assert!(place(&bad, &staging, &dest, &old).is_err(), "{manifest:?}");
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
        place(&good, &staging, &dest, &old).unwrap();
        assert!(dest.join("Humans/Other/man01.hum").exists() && !dest.join("old.txt").exists());
        assert!(!old.exists());
        assert_eq!(status_of(&content), Status::Installed);
        std::fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn a_download_is_checked_before_it_replaces_anything() {
        use sha2::Digest;
        let dir = std::env::temp_dir().join(format!("omsi-pax-fetch-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let content = dir.join("content");
        let source = dir.join(FILE);
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
        assert!(fetch_and_place(&content, &url, 0, Some(&wrong), &zip, &status).is_err());
        assert_eq!(status_of(&content), Status::Missing);
        fetch_and_place(
            &content,
            &url,
            bytes.len() as u64,
            Some(&sha),
            &zip,
            &status,
        )
        .unwrap();
        assert_eq!(status_of(&content), Status::Installed);
        assert!(folder(&content).join("Humans/Other/man01.hum").exists() && !zip.exists());
        std::fs::remove_dir_all(&dir).unwrap();
    }
}
