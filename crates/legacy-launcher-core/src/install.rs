
use anyhow::{Context, Result, anyhow};
use serde::Serialize;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{Arc, Mutex};

const MIN_MARGIN: u64 = 512 * 1024 * 1024;
const ABORT_BELOW: u64 = 1024 * 1024 * 1024;
pub const STAGING: &str = ".install-staging";
pub const WAITING: &str = "waiting";
pub const ARCHIVES: &str = "Archives";
pub const PLUGINS_HELD: &str = "plugins-not-enabled";
pub const UNINSTALLED: &str = "uninstalled";

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Default)]
#[serde(rename_all = "lowercase")]
pub enum InstallMode {
    #[default]
    Extract,
    InPlace,
    Auto,
}

impl InstallMode {
    pub fn parse(s: &str) -> InstallMode {
        match s.trim().to_ascii_lowercase().as_str() {
            "inplace" | "in-place" | "mount" => InstallMode::InPlace,
            "auto" => InstallMode::Auto,
            _ => InstallMode::Extract,
        }
    }
}

#[derive(Debug, Clone, Serialize, Default)]
pub struct Progress {
    pub id: u64,
    pub source: String,
    pub name: String,
    pub state: String,
    pub mode: InstallMode,
    pub files_done: u64,
    pub files_total: u64,
    pub bytes_done: u64,
    pub bytes_total: u64,
    pub free_bytes: u64,
    pub needed_bytes: u64,
    pub message: String,
    pub report: Vec<String>,
    pub warnings: Vec<String>,
    pub installed: Vec<String>,
    pub kept_aside: Vec<String>,
    pub from_inbox: bool,
    pub started: u64,
    pub finished: Option<u64>,
}

pub struct Job {
    pub id: u64,
    pub source: PathBuf,
    pub mode: InstallMode,
    pub from_inbox: bool,
    cancel: AtomicBool,
    progress: Mutex<Progress>,
    bytes_done: AtomicU64,
    files_done: AtomicU64,
}

impl Job {
    pub fn snapshot(&self) -> Progress {
        let mut p = self.progress.lock().unwrap().clone();
        p.bytes_done = self.bytes_done.load(Ordering::Relaxed);
        p.files_done = self.files_done.load(Ordering::Relaxed);
        p
    }

    pub fn cancel(&self) {
        self.cancel.store(true, Ordering::Relaxed);
    }

    fn cancelled(&self) -> bool {
        self.cancel.load(Ordering::Relaxed)
    }

    fn set(&self, f: impl FnOnce(&mut Progress)) {
        f(&mut self.progress.lock().unwrap());
    }

    fn state(&self, s: &str, msg: impl Into<String>) {
        let msg = msg.into();
        self.set(|p| {
            p.state = s.into();
            p.message = msg;
        });
    }

    fn is_active(&self) -> bool {
        self.progress.lock().unwrap().finished.is_none()
    }

    fn check_cancel(&self) -> Result<()> {
        if self.cancelled() {
            Err(anyhow!(Cancelled))
        } else {
            Ok(())
        }
    }
}

static JOBS: Mutex<Vec<Arc<Job>>> = Mutex::new(Vec::new());
static NEXT_ID: AtomicU64 = AtomicU64::new(1);
static RUNNING: Mutex<()> = Mutex::new(());

pub fn now_secs() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0)
}

pub fn jobs() -> Vec<Progress> {
    let mut v: Vec<Progress> = JOBS.lock().unwrap().iter().map(|j| j.snapshot()).collect();
    v.sort_by(|a, b| b.id.cmp(&a.id));
    v
}

pub fn cancel(id: u64) -> bool {
    match JOBS.lock().unwrap().iter().find(|j| j.id == id) {
        Some(j) if j.is_active() => {
            j.cancel();
            true
        }
        _ => false,
    }
}

pub fn clear_finished() {
    JOBS.lock().unwrap().retain(|j| j.is_active());
}

pub fn start(
    content: PathBuf,
    _root: Option<PathBuf>,
    source: PathBuf,
    mode: InstallMode,
    from_inbox: bool,
) -> Arc<Job> {
    let id = NEXT_ID.fetch_add(1, Ordering::Relaxed);
    let name = source
        .file_name()
        .map(|s| s.to_string_lossy().to_string())
        .unwrap_or_else(|| source.display().to_string());
    let job = Arc::new(Job {
        id,
        source: source.clone(),
        mode,
        from_inbox,
        cancel: AtomicBool::new(false),
        progress: Mutex::new(Progress {
            id,
            source: source.to_string_lossy().to_string(),
            name,
            state: "queued".into(),
            mode,
            from_inbox,
            started: now_secs(),
            message: "waiting for the install before it".into(),
            ..Default::default()
        }),
        bytes_done: AtomicU64::new(0),
        files_done: AtomicU64::new(0),
    });
    JOBS.lock().unwrap().push(job.clone());
    let j = job.clone();
    std::thread::spawn(move || {
        let _one = RUNNING.lock().unwrap_or_else(|e| e.into_inner());
        let result = j.check_cancel().and_then(|_| {
            std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| run(&j, &content)))
                .unwrap_or_else(|_| Err(anyhow!("the install stopped with an internal error")))
        });
        let _ = std::fs::remove_dir_all(staging_dir(&content, j.id));
        let _ = std::fs::remove_dir(content.join(STAGING));
        ::legacy_config::content_changed();
        match result {
            Ok(()) => j.set(|p| {
                p.state = "done".into();
                p.finished = Some(now_secs());
            }),
            Err(e) if e.downcast_ref::<Cancelled>().is_some() => j.set(|p| {
                p.state = "cancelled".into();
                p.message = "cancelled - nothing was installed".into();
                p.finished = Some(now_secs());
            }),
            Err(e) => j.set(|p| {
                p.state = "failed".into();
                p.message = format!("{e:#}");
                p.report.push(format!("failed: {e:#}"));
                p.finished = Some(now_secs());
            }),
        }
    });
    job
}

pub fn run_blocking(
    content: PathBuf,
    root: Option<PathBuf>,
    source: PathBuf,
    mode: InstallMode,
    cancel_after: Option<std::time::Duration>,
    verbose: bool,
) -> Progress {
    let job = start(content, root, source, mode, false);
    let t0 = std::time::Instant::now();
    let mut last = String::new();
    loop {
        let p = job.snapshot();
        if verbose {
            let line = format!(
                "{} {} {}/{} files {}",
                p.state, p.name, p.files_done, p.files_total, p.message
            );
            if line != last {
                eprintln!("{line}");
                last = line;
            }
        }
        if p.finished.is_some() {
            return p;
        }
        if cancel_after.is_some_and(|c| t0.elapsed() >= c) {
            job.cancel();
        }
        std::thread::sleep(std::time::Duration::from_millis(50));
    }
}

#[derive(Debug)]
struct Cancelled;
impl std::fmt::Display for Cancelled {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("cancelled")
    }
}
impl std::error::Error for Cancelled {}

fn staging_dir(content: &Path, id: u64) -> PathBuf {
    content
        .join(STAGING)
        .join(format!("{}-{id}", std::process::id()))
}

pub fn free_space(path: &Path) -> Option<u64> {
    let mut p = path.to_path_buf();
    while !p.exists() {
        if !p.pop() {
            return None;
        }
    }
    free_space_impl(&p)
}

#[cfg(unix)]
fn free_space_impl(p: &Path) -> Option<u64> {
    use std::os::unix::ffi::OsStrExt;
    let c = std::ffi::CString::new(p.as_os_str().as_bytes()).ok()?;
    let mut s: libc::statvfs = unsafe { std::mem::zeroed() };
    if unsafe { libc::statvfs(c.as_ptr(), &mut s) } != 0 {
        return None;
    }
    Some(s.f_bavail as u64 * s.f_frsize as u64)
}

#[cfg(windows)]
fn free_space_impl(p: &Path) -> Option<u64> {
    use std::os::windows::ffi::OsStrExt;
    #[link(name = "kernel32")]
    unsafe extern "system" {
        fn GetDiskFreeSpaceExW(
            dir: *const u16,
            avail: *mut u64,
            total: *mut u64,
            free: *mut u64,
        ) -> i32;
    }
    let wide: Vec<u16> = p.as_os_str().encode_wide().chain(Some(0)).collect();
    let (mut a, mut t, mut f) = (0u64, 0u64, 0u64);
    (unsafe { GetDiskFreeSpaceExW(wide.as_ptr(), &mut a, &mut t, &mut f) } != 0).then_some(a)
}

#[cfg(not(any(unix, windows)))]
fn free_space_impl(_p: &Path) -> Option<u64> {
    None
}

#[cfg(unix)]
pub fn pid_alive(pid: u32) -> bool {
    let r = unsafe { libc::kill(pid as libc::pid_t, 0) };
    r == 0 || std::io::Error::last_os_error().raw_os_error() == Some(libc::EPERM)
}

#[cfg(windows)]
pub fn pid_alive(pid: u32) -> bool {
    #[link(name = "kernel32")]
    unsafe extern "system" {
        fn OpenProcess(access: u32, inherit: i32, pid: u32) -> isize;
        fn GetExitCodeProcess(h: isize, code: *mut u32) -> i32;
        fn CloseHandle(h: isize) -> i32;
    }
    const QUERY_LIMITED: u32 = 0x1000;
    const STILL_ACTIVE: u32 = 259;
    unsafe {
        let h = OpenProcess(QUERY_LIMITED, 0, pid);
        if h == 0 {
            return false;
        }
        let mut code = 0u32;
        let ok = GetExitCodeProcess(h, &mut code) != 0 && code == STILL_ACTIVE;
        CloseHandle(h);
        ok
    }
}

#[cfg(not(any(unix, windows)))]
pub fn pid_alive(_pid: u32) -> bool {
    true
}

fn gb(b: u64) -> String {
    if b >= 1 << 30 {
        format!("{:.1} GB", b as f64 / (1u64 << 30) as f64)
    } else if b >= 1 << 20 {
        format!("{:.0} MB", b as f64 / (1u64 << 20) as f64)
    } else {
        format!("{} KB", b.div_ceil(1024))
    }
}

fn check_space(content: &Path) -> Result<()> {
    match free_space(content) {
        Some(free) if free < ABORT_BELOW => Err(anyhow!(
            "the disk is almost full ({} free) - stopped and removed the partial files",
            gb(free)
        )),
        _ => Ok(()),
    }
}

pub fn cleanup_stale(data_dir: &Path, content: Option<&Path>) -> Vec<String> {
    let mut out = Vec::new();
    let legacy = data_dir.join("unzip");
    if legacy.exists() && std::fs::remove_dir_all(&legacy).is_ok() {
        out.push(format!("removed {}", legacy.display()));
    }
    let Some(content) = content else { return out };
    let dir = content.join(STAGING);
    let Ok(rd) = std::fs::read_dir(&dir) else {
        return out;
    };
    let active: Vec<u64> = JOBS
        .lock()
        .unwrap()
        .iter()
        .filter(|j| j.is_active())
        .map(|j| j.id)
        .collect();
    for e in rd.flatten() {
        let name = e.file_name().to_string_lossy().to_string();
        let (pid, id) = match name.split_once('-') {
            Some((p, i)) => (p.parse::<u32>().unwrap_or(0), i.parse::<u64>().unwrap_or(0)),
            None => (0, 0),
        };
        let stale = if pid == std::process::id() {
            !active.contains(&id)
        } else {
            pid == 0 || !pid_alive(pid)
        };
        if stale && std::fs::remove_dir_all(e.path()).is_ok() {
            out.push(format!(
                "removed partial files of an interrupted install ({})",
                e.path().display()
            ));
        }
    }
    let _ = std::fs::remove_dir(&dir);
    out
}

pub fn waiting_ready(_content: &Path, _root: Option<&Path>) -> Vec<PathBuf> {
    Vec::new()
}

fn safe_rel(name: &str) -> Option<String> {
    let name = name.replace('\\', "/");
    if name.starts_with('/') || name.contains('\0') {
        return None;
    }
    let mut parts = Vec::new();
    for p in name.split('/') {
        match p {
            "" | "." => continue,
            ".." => return None,
            p if p.contains(':') => return None,
            p => parts.push(p),
        }
    }
    (!parts.is_empty()).then(|| parts.join("/"))
}

fn is_junk(rel: &str) -> bool {
    rel.split('/').any(|p| {
        p == "__MACOSX"
            || p == ".DS_Store"
            || p.eq_ignore_ascii_case("thumbs.db")
            || p.eq_ignore_ascii_case("desktop.ini")
            || p.starts_with("._")
    })
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum ArchiveKind {
    Zip,
    SevenZip,
    Rar,
}

fn archive_kind(path: &Path) -> Option<ArchiveKind> {
    if !path.is_file() {
        return None;
    }
    match path
        .extension()?
        .to_string_lossy()
        .to_ascii_lowercase()
        .as_str()
    {
        "zip" => Some(ArchiveKind::Zip),
        "7z" => Some(ArchiveKind::SevenZip),
        "rar" => Some(ArchiveKind::Rar),
        _ => None,
    }
}

fn archive_stats(src: &Path, kind: ArchiveKind) -> Result<(u64, u64)> {
    match kind {
        ArchiveKind::Zip => {
            let f = std::fs::File::open(src)
                .with_context(|| format!("opening {}", src.display()))?;
            let mut z = zip::ZipArchive::new(std::io::BufReader::with_capacity(1 << 13, f))
                .with_context(|| format!("{} is not a readable zip archive", src.display()))?;
            let (mut files, mut bytes) = (0u64, 0u64);
            for i in 0..z.len() {
                let e = z.by_index_raw(i)?;
                if !e.is_dir() {
                    files += 1;
                    bytes = bytes.saturating_add(e.size());
                }
            }
            Ok((files, bytes))
        }
        ArchiveKind::SevenZip => {
            let r = sevenz_rust2::ArchiveReader::open(src, sevenz_rust2::Password::empty())
                .with_context(|| format!("{} is not a readable 7z archive", src.display()))?;
            let it = r.archive().files.iter().filter(|e| e.has_stream && !e.is_directory);
            Ok((
                it.clone().count() as u64,
                it.fold(0u64, |n, e| n.saturating_add(e.size)),
            ))
        }
        ArchiveKind::Rar => {
            let a = unrar_rs::RarArchive::open(std::fs::File::open(src)?)
                .with_context(|| format!("{} is not a readable RAR archive", src.display()))?;
            let members: Vec<_> = a
                .entries()
                .filter(|m| !m.is_directory && !m.is_symlink && !m.is_hardlink)
                .collect();
            if members.iter().any(|m| m.unpacked_size.is_none()) {
                return Err(anyhow!(
                    "{} is a multi-volume RAR archive; add all parts and start with the first .rar file",
                    src.display()
                ));
            }
            Ok((
                members.len() as u64,
                members
                    .iter()
                    .fold(0u64, |n, m| n.saturating_add(m.unpacked_size.unwrap_or(0))),
            ))
        }
    }
}

fn run(job: &Job, content: &Path) -> Result<()> {
    let src = job.source.clone();
    let archive = archive_kind(&src);
    if archive.is_none() && !src.is_dir() {
        return Err(anyhow!(
            "{} is neither a folder nor a supported mod archive (.zip, .7z, .rar)",
            src.display()
        ));
    }
    let mods_dir = content.join("mods");
    if src.starts_with(&mods_dir) {
        return Err(anyhow!("{} is already in the mods folder", src.display()));
    }
    let name = if archive.is_some() {
        src.file_stem()
    } else {
        src.file_name()
    }
    .map(|s| s.to_string_lossy().to_string())
    .filter(|n| safe_rel(n).as_deref() == Some(n.as_str()) && !n.starts_with('.'))
    .unwrap_or_else(|| "mod".into());
    if ["shared", WAITING, "installed", UNINSTALLED, PLUGINS_HELD]
        .iter()
        .any(|r| r.eq_ignore_ascii_case(&name))
    {
        return Err(anyhow!("'{name}' is a reserved name, rename the mod"));
    }
    std::fs::create_dir_all(mods_dir.join("shared"))?;

    let free = free_space(content).unwrap_or(u64::MAX);
    let mut keep_zip = archive == Some(ArchiveKind::Zip) && job.mode == InstallMode::InPlace;
    if let Some(kind) = archive {
        let (files, bytes) = archive_stats(&src, kind)?;
        let needed = bytes.saturating_add(MIN_MARGIN);
        job.set(|p| {
            p.files_total = files;
            p.bytes_total = bytes;
            p.free_bytes = free;
            p.needed_bytes = needed;
        });
        if needed > free && !keep_zip {
            if kind == ArchiveKind::Zip && job.mode == InstallMode::Auto {
                keep_zip = true;
            } else {
                return Err(anyhow!(
                    "not enough disk space: {name} unpacks to {} and {} must stay free, but only {} is free",
                    gb(bytes),
                    gb(MIN_MARGIN),
                    gb(free)
                ));
            }
        }
    }
    job.set(|p| p.mode = if keep_zip { InstallMode::InPlace } else { InstallMode::Extract });

    let dest = if keep_zip {
        mods_dir.join(format!("{name}.zip"))
    } else {
        mods_dir.join(&name)
    };
    if mods_dir.join(&name).exists() || mods_dir.join(format!("{name}.zip")).exists() {
        return Err(anyhow!(
            "mod '{name}' is already installed - remove it from the mods folder first"
        ));
    }
    let staging = staging_dir(content, job.id);
    std::fs::create_dir_all(&staging)?;
    let staged = staging.join("mod");
    job.state("copying", format!("installing {name} into the mods folder"));
    match archive {
        Some(ArchiveKind::Zip) if keep_zip => {
            std::fs::copy(&src, &staged)
                .with_context(|| format!("copying {}", src.display()))?;
        }
        Some(ArchiveKind::Zip) => extract_zip(job, content, &src, &staged)?,
        Some(kind) => unpack_other(job, content, &src, &staged, kind)?,
        None => copy_tree(job, content, &src, &staged)?,
    }
    job.check_cancel()?;
    job.state("moving", "moving it into place");
    std::fs::rename(&staged, &dest)
        .or_else(|_| {
            if keep_zip {
                std::fs::copy(&staged, &dest).map(|_| ())
            } else {
                copy_tree(job, content, &staged, &dest).map_err(std::io::Error::other)
            }
        })
        .with_context(|| format!("moving it to {}", dest.display()))?;
    let shown = format!(
        "mods/{}",
        dest.file_name().unwrap_or_default().to_string_lossy()
    );
    job.set(|p| {
        p.installed.push(shown.clone());
        p.report.push(format!("installed as {shown}"));
        p.message = format!("installed as {shown}");
    });
    Ok(())
}

fn copy_tree(job: &Job, content: &Path, from: &Path, to: &Path) -> Result<()> {
    std::fs::create_dir_all(to)?;
    for e in std::fs::read_dir(from)? {
        job.check_cancel()?;
        let e = e?;
        let ty = e.file_type()?;
        let target = to.join(e.file_name());
        if ty.is_dir() {
            copy_tree(job, content, &e.path(), &target)?;
        } else if ty.is_file() {
            check_space(content)?;
            let n = std::fs::copy(e.path(), &target)?;
            job.bytes_done.fetch_add(n, Ordering::Relaxed);
            job.files_done.fetch_add(1, Ordering::Relaxed);
        }
    }
    Ok(())
}

fn extract_zip(job: &Job, content: &Path, src: &Path, dest: &Path) -> Result<()> {
    let f = std::fs::File::open(src).with_context(|| format!("opening {}", src.display()))?;
    let mut z = zip::ZipArchive::new(std::io::BufReader::with_capacity(1 << 13, f))
        .with_context(|| format!("{} is not a readable zip archive", src.display()))?;
    let names: Vec<String> = (0..z.len())
        .filter_map(|i| z.by_index_raw(i).ok().map(|e| e.name().replace('\\', "/")))
        .collect();
    let first = names
        .first()
        .and_then(|n| n.split('/').next())
        .map(|s| format!("{s}/"));
    let strip = first.filter(|f| names.iter().all(|n| n.starts_with(f.as_str())));
    std::fs::create_dir_all(dest)?;
    for i in 0..z.len() {
        job.check_cancel()?;
        let mut e = z.by_index(i)?;
        let raw = e.name().replace('\\', "/");
        let rel = match &strip {
            Some(p) => raw.strip_prefix(p.as_str()).unwrap_or(&raw).to_string(),
            None => raw,
        };
        let Some(rel) = safe_rel(&rel) else { continue };
        if is_junk(&rel) {
            continue;
        }
        let target = dest.join(&rel);
        if e.is_dir() {
            std::fs::create_dir_all(&target)?;
            continue;
        }
        if let Some(parent) = target.parent() {
            std::fs::create_dir_all(parent)?;
        }
        let mut out = std::fs::File::create(&target)?;
        let n = std::io::copy(&mut e, &mut out)
            .with_context(|| format!("unpacking {rel} (the archive may be damaged)"))?;
        job.bytes_done.fetch_add(n, Ordering::Relaxed);
        if job.files_done.fetch_add(1, Ordering::Relaxed) % 256 == 255 {
            check_space(content)?;
        }
    }
    Ok(())
}

fn unpack_other(
    job: &Job,
    content: &Path,
    src: &Path,
    dest: &Path,
    kind: ArchiveKind,
) -> Result<()> {
    std::fs::create_dir_all(dest)?;
    match kind {
        ArchiveKind::SevenZip => {
            sevenz_rust2::decompress_file_with_extract_fn(src, dest, |entry, reader, target| {
                if job.cancelled() {
                    return Err(sevenz_rust2::Error::Io(
                        std::io::Error::new(std::io::ErrorKind::Interrupted, "cancelled"),
                        "install cancelled".into(),
                    ));
                }
                let wrote = sevenz_rust2::default_entry_extract_fn(entry, reader, target)?;
                if wrote && entry.has_stream && !entry.is_directory {
                    if job.files_done.fetch_add(1, Ordering::Relaxed) % 256 == 255
                        && check_space(content).is_err()
                    {
                        return Err(sevenz_rust2::Error::Io(
                            std::io::Error::other("free disk space fell below 1 GB"),
                            "install stopped".into(),
                        ));
                    }
                }
                Ok(wrote)
            })
            .with_context(|| {
                format!(
                    "unpacking {} (the archive may be damaged or encrypted)",
                    src.display()
                )
            })?;
        }
        ArchiveKind::Rar => {
            let mut archive = unrar_rs::RarArchive::open(std::fs::File::open(src)?)
                .with_context(|| format!("opening {}", src.display()))?;
            let members: Vec<_> = archive.entries().collect();
            for (index, member) in members.iter().enumerate() {
                job.check_cancel()?;
                if member.is_directory || member.is_symlink || member.is_hardlink {
                    continue;
                }
                let Some(rel) = safe_rel(&member.name) else {
                    job.set(|p| {
                        p.warnings
                            .push(format!("unsafe RAR path refused: {}", member.name))
                    });
                    continue;
                };
                let out = dest.join(&rel);
                if let Some(parent) = out.parent() {
                    std::fs::create_dir_all(parent)?;
                }
                let mut file = std::fs::File::create(&out)?;
                let n = archive
                    .by_index(index)?
                    .copy_to(&mut file)
                    .with_context(|| format!("unpacking {rel} from {}", src.display()))?;
                job.files_done.fetch_add(1, Ordering::Relaxed);
                job.bytes_done.fetch_add(n, Ordering::Relaxed);
                check_space(content)?;
            }
        }
        ArchiveKind::Zip => unreachable!(),
    }
    job.check_cancel()
}

#[derive(Debug, Clone, Serialize, Default)]
pub struct SourceInfo {
    pub is_archive: bool,
    pub is_zip: bool,
    pub files: u64,
    pub unpacked_bytes: u64,
    pub archive_bytes: u64,
    pub needed_bytes: u64,
    pub free_bytes: u64,
    pub fits: bool,
    pub in_place: String,
    pub in_place_ok: bool,
    pub suggested: InstallMode,
}

pub fn inspect(content: &Path, _root: Option<&Path>, src: &Path) -> Result<SourceInfo> {
    let free = free_space(content).unwrap_or(u64::MAX);
    let Some(kind) = archive_kind(src) else {
        return Ok(SourceInfo {
            free_bytes: free,
            fits: true,
            in_place: "only a .zip archive can be used in place".into(),
            suggested: InstallMode::Extract,
            ..Default::default()
        });
    };
    let (files, unpacked) = archive_stats(src, kind)?;
    let needed = unpacked.saturating_add(MIN_MARGIN);
    let fits = needed <= free;
    let is_zip = kind == ArchiveKind::Zip;
    Ok(SourceInfo {
        is_archive: true,
        is_zip,
        files,
        unpacked_bytes: unpacked,
        archive_bytes: std::fs::metadata(src).map(|m| m.len()).unwrap_or(0),
        needed_bytes: needed,
        free_bytes: free,
        fits,
        in_place: if is_zip {
            String::new()
        } else {
            "7z and RAR archives must be unpacked".into()
        },
        in_place_ok: is_zip,
        suggested: if !fits && is_zip {
            InstallMode::InPlace
        } else {
            InstallMode::Extract
        },
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;

    fn tmp(name: &str) -> PathBuf {
        let d = std::env::temp_dir().join(format!("omsi-install-test-{}-{name}", std::process::id()));
        let _ = std::fs::remove_dir_all(&d);
        std::fs::create_dir_all(&d).unwrap();
        d
    }

    fn write_zip(path: &Path, files: &[(&str, usize)]) {
        let mut z = zip::ZipWriter::new(std::fs::File::create(path).unwrap());
        let o = zip::write::SimpleFileOptions::default();
        for (name, size) in files {
            z.start_file(*name, o).unwrap();
            z.write_all(&vec![7u8; *size]).unwrap();
        }
        z.finish().unwrap();
    }

    #[test]
    fn zip_goes_into_mods() {
        let dir = tmp("zip");
        let content = dir.join("content");
        std::fs::create_dir_all(&content).unwrap();
        let zip = dir.join("Foo Bus.zip");
        write_zip(&zip, &[("Foo/Vehicles/Foo/foo.bus", 10), ("Foo/Vehicles/Foo/a.dds", 100)]);
        let p = run_blocking(content.clone(), None, zip.clone(), InstallMode::Extract, None, false);
        assert_eq!(p.state, "done", "{p:?}");
        assert!(content.join("mods/Foo Bus/Vehicles/Foo/foo.bus").exists());
        assert!(!content.join(STAGING).exists());
        let p = run_blocking(content.clone(), None, zip, InstallMode::InPlace, None, false);
        assert_eq!(p.state, "failed", "{p:?}");
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn safe_names() {
        assert_eq!(safe_rel("Vehicles\\Foo\\./foo.bus").as_deref(), Some("Vehicles/Foo/foo.bus"));
        for bad in ["../x", "a/../b", "/etc/passwd", "C:\\x", "a\0b", "", "..\\..\\evil"] {
            assert_eq!(safe_rel(bad), None, "{bad:?}");
        }
    }
}
