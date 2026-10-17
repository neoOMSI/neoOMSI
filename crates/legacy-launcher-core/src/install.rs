
use anyhow::{Context, Result, anyhow};
use serde::Serialize;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{Arc, Mutex};

/// The free space an install must leave on the disk.
const MIN_MARGIN: u64 = 1024 * 1024 * 1024;
/// A running install stops below this. It is lower than `MIN_MARGIN`, so an install that passed
/// the space check doesn't fail near its end.
const ABORT_BELOW: u64 = 512 * 1024 * 1024;
pub const STAGING: &str = ".install-staging";
/// Prefix for a mod while it is copied into `mods/`; discovery skips names that start with a dot.
const PARTIAL: &str = ".partial-";
pub const UNINSTALLED: &str = ::mods::UNINSTALLED_DIR;

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
    pub started: u64,
    pub finished: Option<u64>,
}

pub struct Job {
    pub id: u64,
    pub source: PathBuf,
    pub mode: InstallMode,
    pub replace: bool,
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

pub fn start(content: PathBuf, source: PathBuf, mode: InstallMode, replace: bool) -> Arc<Job> {
    let id = NEXT_ID.fetch_add(1, Ordering::Relaxed);
    let name = source
        .file_name()
        .map(|s| s.to_string_lossy().to_string())
        .unwrap_or_else(|| source.display().to_string());
    let job = Arc::new(Job {
        id,
        source: source.clone(),
        mode,
        replace,
        cancel: AtomicBool::new(false),
        progress: Mutex::new(Progress {
            id,
            source: source.to_string_lossy().to_string(),
            name,
            state: "queued".into(),
            mode,
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
    source: PathBuf,
    mode: InstallMode,
    replace: bool,
    cancel_after: Option<std::time::Duration>,
    verbose: bool,
) -> Progress {
    let job = start(content, source, mode, replace);
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
    let active: Vec<u64> = JOBS
        .lock()
        .unwrap()
        .iter()
        .filter(|j| j.is_active())
        .map(|j| j.id)
        .collect();
    // Leftovers are named `<pid>-<job>` in the staging folder and `.partial-<pid>-<job>-<name>`
    // in `mods/`.
    let stale = |tag: &str| {
        let mut parts = tag.splitn(3, '-');
        let pid = parts.next().and_then(|pid| pid.parse::<u32>().ok()).unwrap_or(0);
        let id = parts.next().and_then(|job| job.parse::<u64>().ok()).unwrap_or(0);
        if pid == std::process::id() {
            !active.contains(&id)
        } else {
            pid == 0 || !pid_alive(pid)
        }
    };
    let staging = content.join(STAGING);
    let mods = content.join("mods");
    let staged = std::fs::read_dir(&staging)
        .into_iter()
        .flatten()
        .flatten()
        .filter(|dir_entry| stale(&dir_entry.file_name().to_string_lossy()));
    let partial = std::fs::read_dir(&mods).into_iter().flatten().flatten().filter(|dir_entry| {
        dir_entry
            .file_name()
            .to_string_lossy()
            .strip_prefix(PARTIAL)
            .is_some_and(&stale)
    });
    for leftover in staged.chain(partial) {
        if remove_any(&leftover.path()).is_ok() {
            out.push(format!(
                "removed partial files of an interrupted install ({})",
                leftover.path().display()
            ));
        }
    }
    let _ = std::fs::remove_dir(&staging);
    out
}

fn remove_any(path: &Path) -> std::io::Result<()> {
    if path.is_dir() {
        std::fs::remove_dir_all(path)
    } else {
        std::fs::remove_file(path)
    }
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

/// The archive's file count and unpacked size, and why it can't be used in place (empty when
/// it can).
fn archive_facts(src: &Path, kind: ArchiveKind) -> Result<(u64, u64, String)> {
    let walk = match kind {
        ArchiveKind::Zip => {
            let summary = zip_summary(src)?;
            let in_place = summary.unreadable.unwrap_or_default();
            return Ok((summary.files, summary.unpacked_bytes, in_place));
        }
        ArchiveKind::SevenZip => sevenz_stats,
        ArchiveKind::Rar => rar_stats,
    };
    let (files, unpacked) = remembered(src, walk)?;
    Ok((
        files,
        unpacked,
        "7z and RAR archives must be unpacked".into(),
    ))
}

/// Keeps the last walk, so the install that follows `inspect` doesn't walk the archive again:
/// without a RAR5 QuickOpen record, that walk seeks once per member.
fn remembered(src: &Path, walk: fn(&Path) -> Result<(u64, u64)>) -> Result<(u64, u64)> {
    type Walked = (PathBuf, (u64, Option<std::time::SystemTime>), (u64, u64));
    static LAST: Mutex<Option<Walked>> = Mutex::new(None);
    let meta = std::fs::metadata(src).with_context(|| format!("opening {}", src.display()))?;
    let stamp = (meta.len(), meta.modified().ok());
    if let Some((path, seen, stats)) = LAST
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner())
        .as_ref()
        && path == src
        && *seen == stamp
    {
        return Ok(*stats);
    }
    // The lock isn't held during the walk, which can take minutes for a large RAR archive.
    let stats = walk(src)?;
    *LAST.lock().unwrap_or_else(|poisoned| poisoned.into_inner()) =
        Some((src.to_path_buf(), stamp, stats));
    Ok(stats)
}

fn sevenz_stats(src: &Path) -> Result<(u64, u64)> {
    let archive = sevenz_rust2::Archive::open(src)
        .with_context(|| format!("{} is not a readable 7z archive", src.display()))?;
    let files = archive
        .files
        .iter()
        .filter(|file| file.has_stream && !file.is_directory);
    Ok((
        files.clone().count() as u64,
        files.fold(0u64, |total, file| total.saturating_add(file.size)),
    ))
}

fn rar_stats(src: &Path) -> Result<(u64, u64)> {
    // unrar-rs reads headers in small pieces, a QuickOpen record even byte by byte.
    let file = std::io::BufReader::new(std::fs::File::open(src)?);
    let archive = unrar_rs::RarArchive::open(file)
        .with_context(|| format!("{} is not a readable RAR archive", src.display()))?;
    let members: Vec<_> = archive
        .entries()
        .filter(|member| !member.is_directory && !member.is_symlink && !member.is_hardlink)
        .collect();
    if members.iter().any(|member| member.unpacked_size.is_none()) {
        return Err(anyhow!(
            "{} is a multi-volume RAR archive; add all parts and start with the first .rar file",
            src.display()
        ));
    }
    Ok((
        members.len() as u64,
        members.iter().fold(0u64, |total, member| {
            total.saturating_add(member.unpacked_size.unwrap_or(0))
        }),
    ))
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
    if ::mods::is_reserved(&name) {
        return Err(anyhow!("'{name}' is a reserved name, rename the mod"));
    }
    std::fs::create_dir_all(&mods_dir)?;

    let free = free_space(content).unwrap_or(u64::MAX);
    let mut keep_zip = archive == Some(ArchiveKind::Zip) && job.mode == InstallMode::InPlace;
    let facts = archive.map(|kind| archive_facts(&src, kind)).transpose()?;
    if let (Some(kind), Some(&(files, bytes, _))) = (archive, facts.as_ref()) {
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
    if keep_zip {
        if let Some((_, _, problem)) = &facts
            && !problem.is_empty()
        {
            return Err(anyhow!("{name} cannot be used in place: {problem}"));
        }
        let size = std::fs::metadata(&src)?.len();
        if size.saturating_add(MIN_MARGIN) > free {
            return Err(anyhow!(
                "not enough disk space: copying {name} takes {} and {} must stay free, but only {} is free",
                gb(size),
                gb(MIN_MARGIN),
                gb(free)
            ));
        }
        job.set(|progress| {
            progress.files_total = 1;
            progress.bytes_total = size;
        });
    }
    job.set(|p| p.mode = if keep_zip { InstallMode::InPlace } else { InstallMode::Extract });

    let dest = if keep_zip {
        mods_dir.join(format!("{name}.zip"))
    } else {
        mods_dir.join(&name)
    };
    let existing: Vec<PathBuf> = [mods_dir.join(&name), mods_dir.join(format!("{name}.zip"))]
        .into_iter()
        .filter(|path| path.exists())
        .collect();
    if !existing.is_empty() && !job.replace {
        return Err(anyhow!(
            "mod '{name}' is already installed - install it with Replace to update it"
        ));
    }
    let staging = staging_dir(content, job.id);
    std::fs::create_dir_all(&staging)?;
    let staged = staging.join("mod");
    job.state("copying", format!("installing {name} into the mods folder"));
    match archive {
        Some(ArchiveKind::Zip) if keep_zip => copy_file(job, content, &src, &staged)?,
        Some(ArchiveKind::Zip) => extract_zip(job, content, &src, &staged)?,
        Some(kind) => unpack_other(job, content, &src, &staged, kind)?,
        None => copy_tree(job, content, &src, &staged)?,
    }
    job.check_cancel()?;
    let has_content = exposes_content(&staged, keep_zip);
    let has_plugins = crate::mod_has_plugins(&staged);

    job.state("moving", "moving it into place");
    let order_note = |error: String| format!("load order not saved: {error}");
    if let Err(error) = ::mods::order::materialize(content) {
        job.set(|progress| progress.warnings.push(order_note(error)));
    }
    // Puts the previous versions back, and adds any that couldn't be restored to the error.
    let restore = |set_aside: &[(PathBuf, PathBuf)], mut error: anyhow::Error| {
        for (old, aside) in set_aside {
            if let Err(restore_error) = std::fs::rename(aside, old) {
                let note = format!("the previous version stays in {}: {restore_error}", aside.display());
                error = error.context(note);
            }
        }
        error
    };
    let mut set_aside = Vec::new();
    for old in &existing {
        match uninstall_path(content, old) {
            Ok(aside) => set_aside.push((old.clone(), aside)),
            Err(error) => return Err(restore(&set_aside, error)),
        }
    }
    if let Err(error) = move_into_place(job, content, &staged, &dest, keep_zip) {
        return Err(restore(&set_aside, error));
    }
    let id = dest.file_name().unwrap_or_default().to_string_lossy().to_string();
    // An update takes the place of the version it replaces in the load order.
    let placed = match existing.iter().find(|old| **old != dest) {
        Some(old) => {
            let old = old.file_name().unwrap_or_default().to_string_lossy();
            ::mods::order::rename(content, &old, &id)
        }
        None => ::mods::order::materialize(content),
    };
    if let Err(error) = placed {
        job.set(|progress| progress.warnings.push(order_note(error)));
    }
    let shown = format!(
        "mods/{}",
        dest.file_name().unwrap_or_default().to_string_lossy()
    );
    job.set(|p| {
        p.installed.push(shown.clone());
        p.report.push(format!("installed as {shown}"));
        p.message = format!("installed as {shown}");
        for (_, aside) in &set_aside {
            p.report
                .push(format!("the previous version was moved to {}", aside.display()));
        }
        if has_plugins {
            p.warnings.push(format!(
                "{name} brings plugins (program code that runs in the game): they stay off until you allow them for this mod"
            ));
        }
        if !has_content {
            p.warnings.push(format!(
                "no OMSI content folders (Vehicles, maps, Sceneryobjects ...) found in {name}: it adds nothing to the game"
            ));
        }
    });
    Ok(())
}

fn uninstall_path(content: &Path, entry: &Path) -> Result<PathBuf> {
    let dir = content.join("mods").join(UNINSTALLED);
    std::fs::create_dir_all(&dir)?;
    let name = entry.file_name().unwrap_or_default().to_string_lossy();
    let secs = now_secs();
    // A Replace and an uninstall of the same mod can happen within the same second.
    let target = (1..)
        .map(|attempt| match attempt {
            1 => dir.join(format!("{secs}-{name}")),
            attempt => dir.join(format!("{secs}-{attempt}-{name}")),
        })
        .find(|target| !target.exists())
        .unwrap();
    std::fs::rename(entry, &target)
        .with_context(|| format!("moving {} to {}", entry.display(), target.display()))?;
    Ok(target)
}

/// The path of the installed mod `name`: a folder or .zip directly in `mods`, not a reserved one.
pub fn installed_entry(content: &Path, name: &str) -> Result<PathBuf> {
    let entry = content.join("mods").join(name);
    let stem = Path::new(name).file_stem().unwrap_or_default().to_string_lossy();
    let is_mod =
        entry.is_dir() || (entry.is_file() && archive_kind(&entry) == Some(ArchiveKind::Zip));
    if safe_rel(name).as_deref() != Some(name)
        || name.contains('/')
        || name.starts_with('.')
        || ::mods::is_reserved(&stem)
        || !is_mod
    {
        return Err(anyhow!("'{name}' is not an installed mod"));
    }
    Ok(entry)
}

pub fn uninstall(content: &Path, name: &str) -> Result<PathBuf> {
    let entry = installed_entry(content, name)?;
    // An install might be replacing this very mod.
    let _no_install = match RUNNING.try_lock() {
        Ok(guard) => guard,
        Err(std::sync::TryLockError::Poisoned(poisoned)) => poisoned.into_inner(),
        Err(std::sync::TryLockError::WouldBlock) => {
            return Err(anyhow!("an install is running - try again when it has finished"));
        }
    };
    // Revoke plugin consent first, so a copy moved back by hand has to be allowed again.
    crate::allow_plugins_of(&entry, false)?;
    let moved = uninstall_path(content, &entry)?;
    if let Err(error) = ::mods::order::forget(content, name) {
        crate::log_to_file(&format!("load order: {error}"));
    }
    ::legacy_config::content_changed();
    Ok(moved)
}

/// If the rename fails (another disk, antivirus), copies under a name that discovery skips and
/// renames that, so a half-copied mod is never mounted.
fn move_into_place(job: &Job, content: &Path, staged: &Path, dest: &Path, file: bool) -> Result<()> {
    if std::fs::rename(staged, dest).is_ok() {
        return Ok(());
    }
    let partial = dest.with_file_name(format!(
        "{PARTIAL}{}-{}-{}",
        std::process::id(),
        job.id,
        dest.file_name().unwrap_or_default().to_string_lossy()
    ));
    // The copy reports its progress from zero again.
    job.bytes_done.store(0, Ordering::Relaxed);
    job.files_done.store(0, Ordering::Relaxed);
    let copied = if file {
        copy_file(job, content, staged, &partial)
    } else {
        copy_tree(job, content, staged, &partial)
    };
    let result = copied.and_then(|_| Ok(std::fs::rename(&partial, dest)?));
    if result.is_err() {
        let _ = remove_any(&partial);
    }
    result.with_context(|| format!("moving it to {}", dest.display()))
}

fn exposes_content(staged: &Path, zip: bool) -> bool {
    if zip {
        return ::legacy_config::vfs::ZipArchive::open(staged).is_ok_and(|archive| archive.has_content());
    }
    let root = ::mods::folder_content_root(staged);
    ::legacy_config::vfs::list_dir(&root).is_some_and(|listing| {
        listing.iter().any(|(name, is_dir)| {
            *is_dir && ::legacy_config::is_content_folder(&name.to_string_lossy())
        })
    })
}

fn zip_summary(src: &Path) -> Result<::legacy_config::vfs::ZipSummary> {
    ::legacy_config::vfs::zip_summary(src)
        .with_context(|| format!("{} is not a readable zip archive", src.display()))
}

/// Copies one file in chunks, reporting progress and stopping when cancelled or when the disk
/// is full.
fn copy_file(job: &Job, content: &Path, from: &Path, to: &Path) -> Result<()> {
    use std::io::{Read, Write};
    let mut input =
        std::fs::File::open(from).with_context(|| format!("opening {}", from.display()))?;
    let mut output = std::fs::File::create(to)?;
    let mut buf = vec![0u8; 8 << 20];
    loop {
        job.check_cancel()?;
        let read = input.read(&mut buf)?;
        if read == 0 {
            break;
        }
        output.write_all(&buf[..read])?;
        job.bytes_done.fetch_add(read as u64, Ordering::Relaxed);
        check_space(content)?;
    }
    job.files_done.fetch_add(1, Ordering::Relaxed);
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

/// Unpacks the archive as it is. The folder the game mounts is found later, the same way as
/// for a .zip used in place (`mods::folder_content_root`).
fn extract_zip(job: &Job, content: &Path, src: &Path, dest: &Path) -> Result<()> {
    let f = std::fs::File::open(src).with_context(|| format!("opening {}", src.display()))?;
    let mut z = zip::ZipArchive::new(std::io::BufReader::with_capacity(1 << 13, f))
        .with_context(|| format!("{} is not a readable zip archive", src.display()))?;
    std::fs::create_dir_all(dest)?;
    for i in 0..z.len() {
        job.check_cancel()?;
        let mut e = z.by_index(i)?;
        let Some(rel) = safe_rel(&e.name()?) else { continue };
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
                if is_junk(&entry.name().replace('\\', "/")) {
                    // Read it anyway: in a solid archive, the next entry starts after this one.
                    std::io::copy(reader, &mut std::io::sink())?;
                    return Ok(true);
                }
                let wrote = sevenz_rust2::default_entry_extract_fn(entry, reader, target)?;
                if wrote && entry.has_stream && !entry.is_directory {
                    job.bytes_done.fetch_add(entry.size, Ordering::Relaxed);
                    if job.files_done.fetch_add(1, Ordering::Relaxed) % 256 == 255
                        && check_space(content).is_err()
                    {
                        return Err(sevenz_rust2::Error::Io(
                            std::io::Error::other("free disk space fell below the limit"),
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
            let file = std::io::BufReader::new(std::fs::File::open(src)?);
            let mut archive = unrar_rs::RarArchive::open(file)
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
                if is_junk(&rel) {
                    continue;
                }
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

pub fn inspect(content: &Path, src: &Path) -> Result<SourceInfo> {
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
    let is_zip = kind == ArchiveKind::Zip;
    let (files, unpacked, in_place) = archive_facts(src, kind)?;
    let needed = unpacked.saturating_add(MIN_MARGIN);
    let fits = needed <= free;
    let in_place_ok = in_place.is_empty();
    Ok(SourceInfo {
        is_archive: true,
        is_zip,
        files,
        unpacked_bytes: unpacked,
        archive_bytes: std::fs::metadata(src).map(|m| m.len()).unwrap_or(0),
        needed_bytes: needed,
        free_bytes: free,
        fits,
        in_place,
        in_place_ok,
        suggested: if !fits && in_place_ok {
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

    /// Runs the tests that install one at a time: uninstall refuses while another install runs.
    fn installing() -> std::sync::MutexGuard<'static, ()> {
        static INSTALLS: Mutex<()> = Mutex::new(());
        INSTALLS
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
    }

    #[test]
    fn zip_goes_into_mods() {
        let _installing = installing();
        let dir = tmp("zip");
        let content = dir.join("content");
        std::fs::create_dir_all(&content).unwrap();
        let zip = dir.join("Foo Bus.zip");
        write_zip(&zip, &[("Foo/Vehicles/Foo/foo.bus", 10), ("Foo/Vehicles/Foo/a.dds", 100)]);
        let p = run_blocking(content.clone(), zip.clone(), InstallMode::Extract, false, None, false);
        assert_eq!(p.state, "done", "{p:?}");
        assert!(p.warnings.is_empty(), "{p:?}");
        assert!(content.join("mods/Foo Bus/Foo/Vehicles/Foo/foo.bus").exists());
        assert!(!content.join(STAGING).exists());
        let p = run_blocking(content.clone(), zip, InstallMode::InPlace, false, None, false);
        assert_eq!(p.state, "failed", "{p:?}");
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn an_archive_is_walked_once_until_it_is_replaced() {
        static WALKS: AtomicU64 = AtomicU64::new(0);
        fn walk(_: &Path) -> Result<(u64, u64)> {
            WALKS.fetch_add(1, Ordering::Relaxed);
            Ok((1, 10))
        }
        let dir = tmp("walks");
        let rar = dir.join("Mod.rar");
        std::fs::write(&rar, b"first").unwrap();
        assert_eq!(remembered(&rar, walk).unwrap(), (1, 10));
        assert_eq!(remembered(&rar, walk).unwrap(), (1, 10));
        assert_eq!(
            WALKS.load(Ordering::Relaxed),
            1,
            "the install after inspect reuses the walk"
        );
        std::fs::write(&rar, b"a replaced archive").unwrap();
        remembered(&rar, walk).unwrap();
        assert_eq!(
            WALKS.load(Ordering::Relaxed),
            2,
            "a replaced archive is walked again"
        );
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// An archive unpacked and the same archive used in place show the game the same files.
    #[test]
    fn extracted_and_in_place_mods_match() {
        let _installing = installing();
        let layouts: [&[&str]; 4] = [
            &["Vehicles/Bus/bus.bus", "Vehicles/Bus/Texture/a.dds"],
            &["Pack/Vehicles/Bus/bus.bus", "Pack/Vehicles/Bus/Texture/a.dds"],
            &["readme.txt", "Content/Vehicles/Bus/bus.bus", "Content/Vehicles/Bus/Texture/a.dds"],
            &["OMSI 2/Sceneryobjects/Signs/a.sco", "Docs/manual.txt"],
        ];
        for (layout, files) in layouts.iter().enumerate() {
            let dir = tmp(&format!("parity-{layout}"));
            let zip = dir.join("Mod.zip");
            write_zip(&zip, &files.iter().map(|file| (*file, 4)).collect::<Vec<_>>());
            let (extracted, in_place) = (dir.join("a"), dir.join("b"));
            let progress = run_blocking(extracted.clone(), zip.clone(), InstallMode::Extract, false, None, false);
            assert!(progress.state == "done" && progress.warnings.is_empty(), "{progress:?}");
            let progress = run_blocking(in_place.clone(), zip, InstallMode::InPlace, false, None, false);
            assert!(progress.state == "done" && progress.warnings.is_empty(), "{progress:?}");
            let root = ::mods::folder_content_root(&extracted.join("mods/Mod"));
            let mounted = ::legacy_config::vfs::ZipArchive::open(&in_place.join("mods/Mod.zip")).unwrap();
            let rel = files
                .iter()
                .find(|file| file.ends_with(".bus") || file.ends_with(".sco"))
                .unwrap();
            let rel = &rel[mounted.prefix().len()..];
            assert!(root.join(rel).is_file(), "layout {layout}: {}", root.join(rel).display());
            assert!(mounted.read(rel).is_ok(), "layout {layout}: {rel}");
            let _ = std::fs::remove_dir_all(&dir);
        }
    }

    #[test]
    fn a_mod_without_content_folders_is_reported() {
        let _installing = installing();
        let dir = tmp("nocontent");
        let zip = dir.join("Docs.zip");
        write_zip(&zip, &[("readme.txt", 4)]);
        let progress = run_blocking(dir.join("c"), zip, InstallMode::Extract, false, None, false);
        assert_eq!(progress.state, "done", "{progress:?}");
        assert!(
            matches!(&progress.warnings[..], [warning] if warning.starts_with("no OMSI content folders")),
            "{progress:?}"
        );
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn a_move_that_fails_leaves_no_half_mod() {
        let dir = tmp("move");
        let staged = dir.join("staged");
        std::fs::create_dir_all(staged.join("Vehicles/Bus")).unwrap();
        std::fs::write(staged.join("Vehicles/Bus/bus.bus"), b"x").unwrap();
        // A folder already in the way makes the rename fail, and the fallback copy's rename too.
        let dest = dir.join("mods/Bus");
        std::fs::create_dir_all(dest.join("other")).unwrap();
        let job = Job {
            id: 1,
            source: staged.clone(),
            mode: InstallMode::Extract,
            replace: false,
            cancel: AtomicBool::new(false),
            progress: Mutex::new(Progress::default()),
            bytes_done: AtomicU64::new(1),
            // The progress left by the copy into the staging folder.
            files_done: AtomicU64::new(1),
        };
        assert!(move_into_place(&job, &dir, &staged, &dest, false).is_err());
        let left: Vec<_> = std::fs::read_dir(dir.join("mods"))
            .unwrap()
            .flatten()
            .map(|dir_entry| dir_entry.file_name())
            .collect();
        assert_eq!(left, ["Bus"]);
        assert_eq!(job.files_done.load(Ordering::Relaxed), 1, "counted once");
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn replace_and_uninstall_keep_the_old_version() {
        let _installing = installing();
        let dir = tmp("replace");
        let content = dir.join("content");
        let zip = dir.join("Bus.zip");
        write_zip(&zip, &[("Vehicles/Bus/bus.bus", 4)]);
        let progress = run_blocking(content.clone(), zip.clone(), InstallMode::Extract, false, None, false);
        assert_eq!(progress.state, "done", "{progress:?}");
        let mods = content.join("mods");
        let order = || ::mods::order::load(&mods).unwrap().unwrap().order;
        assert_eq!(order(), ["Bus"]);
        let other = dir.join("Other.zip");
        write_zip(&other, &[("Vehicles/Other/o.bus", 4)]);
        let progress = run_blocking(content.clone(), other, InstallMode::Extract, false, None, false);
        assert_eq!(progress.state, "done", "{progress:?}");
        assert_eq!(order(), ["Other", "Bus"], "a new mod goes to the top");
        write_zip(&zip, &[("Vehicles/Bus/bus.bus", 8)]);
        let progress = run_blocking(content.clone(), zip.clone(), InstallMode::InPlace, true, None, false);
        assert_eq!(progress.state, "done", "{progress:?}");
        assert!(mods.join("Bus.zip").is_file() && !mods.join("Bus").exists());
        assert_eq!(order(), ["Other", "Bus.zip"], "an update keeps its place");
        let aside = |suffix: &str| {
            std::fs::read_dir(mods.join(UNINSTALLED))
                .unwrap()
                .flatten()
                .filter(|dir_entry| dir_entry.file_name().to_string_lossy().ends_with(suffix))
                .count()
        };
        assert_eq!(aside("-Bus"), 1);
        let moved = uninstall(&content, "Bus.zip").unwrap();
        assert!(moved.is_file() && !mods.join("Bus.zip").exists());
        assert_eq!(aside("-Bus.zip"), 1);
        assert_eq!(order(), ["Other"]);
        for bad in ["uninstalled", "../Bus", "Missing", ".hidden"] {
            assert!(uninstall(&content, bad).is_err(), "{bad}");
        }
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn leftovers_of_an_interrupted_install_are_removed() {
        let dir = tmp("leftovers");
        let partial = dir.join("mods").join(format!("{PARTIAL}0-1-Bus"));
        let staged = dir.join(STAGING).join("0-2");
        std::fs::create_dir_all(&partial).unwrap();
        std::fs::create_dir_all(&staged).unwrap();
        let cleaned = cleanup_stale(&dir.join("data"), Some(&dir));
        assert_eq!(cleaned.len(), 2, "{cleaned:?}");
        assert!(!partial.exists() && !staged.exists());
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
