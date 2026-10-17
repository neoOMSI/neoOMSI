//! The files of the content roots: plain folders, and `.zip` archives mounted as folders.
//!
//! A map or scenery pack is often shipped as one archive of several gigabytes (Ahlheim V5:
//! 5.2 GB zipped, 15.6 GB and 53 827 files unpacked). Unpacking it is not needed: an archive
//! is *mounted* at its own file path, so `…/AhlheimV5.zip/maps/Ahlheim 5/global.cfg` names
//! the entry `OMSI 2/maps/Ahlheim 5/global.cfg` inside it, and every loader that reads
//! through [`read`] / [`exists`] / [`list_dir`] cannot tell it from an unpacked folder.
//!
//! * The central directory is read once into an index (ZIP64 included - archives past 4 GB
//!   need it); an entry is then one positioned read plus a stored copy or an inflate.
//! * Lookups are case-insensitive, as on Windows, and `\` separators in entry names count
//!   as `/`.
//! * The folder inside the archive that holds OMSI's content folders (`Vehicles`, `maps`,
//!   `Sceneryobjects` …) becomes the mount's root: archives usually wrap everything in an
//!   `OMSI 2/` folder next to documentation.

use std::collections::HashMap;
use std::ffi::OsString;
use std::io::{self, Read};
use std::path::{Component, Path, PathBuf};
use std::sync::{Arc, RwLock};

/// One file inside an archive.
#[derive(Debug, Clone)]
struct ZipEntry {
    method: u16,
    flags: u16,
    crc: u32,
    compressed: u64,
    size: u64,
    /// Offset of the local file header.
    header: u64,
}

/// A mounted archive.
pub struct ZipArchive {
    /// The archive file, which is also the folder it is mounted as.
    path: PathBuf,
    file: std::fs::File,
    /// Size and modification time when it was read, to notice when the file is replaced.
    stamp: Stamp,
    #[cfg(not(unix))]
    lock: std::sync::Mutex<()>,
    /// The archive's own folder that became the mount root (original spelling, `/` at the end
    /// unless empty).
    prefix: String,
    /// Entries by lower-case path relative to the mount root.
    entries: HashMap<String, ZipEntry>,
    /// Folder contents by lower-case folder path ("" = the root): (name as stored, is folder).
    dirs: HashMap<String, Vec<(String, bool)>>,
}

static MOUNTS: RwLock<Vec<Arc<ZipArchive>>> = RwLock::new(Vec::new());

type Stamp = (u64, Option<std::time::SystemTime>);

fn stamp_of(meta: &std::fs::Metadata) -> Stamp {
    (meta.len(), meta.modified().ok())
}

/// The largest entry that is read into memory (a corrupt size field must not allocate
/// gigabytes).
const MAX_ENTRY: u64 = 1 << 31;

fn le16(b: &[u8], o: usize) -> u16 {
    u16::from_le_bytes([b[o], b[o + 1]])
}

fn le32(b: &[u8], o: usize) -> u32 {
    u32::from_le_bytes([b[o], b[o + 1], b[o + 2], b[o + 3]])
}

fn le64(b: &[u8], o: usize) -> u64 {
    u64::from_le_bytes([
        b[o],
        b[o + 1],
        b[o + 2],
        b[o + 3],
        b[o + 4],
        b[o + 5],
        b[o + 6],
        b[o + 7],
    ])
}

fn bad(msg: impl Into<String>) -> io::Error {
    io::Error::new(io::ErrorKind::InvalidData, msg.into())
}

/// Code page 437, the encoding of entry names written without the UTF-8 flag (what the
/// Windows zip tools of the OMSI years produced for "Straße" or "Bahnübergang").
const CP437_HIGH: [char; 128] = [
    'Ç', 'ü', 'é', 'â', 'ä', 'à', 'å', 'ç', 'ê', 'ë', 'è', 'ï', 'î', 'ì', 'Ä', 'Å', 'É', 'æ', 'Æ',
    'ô', 'ö', 'ò', 'û', 'ù', 'ÿ', 'Ö', 'Ü', '¢', '£', '¥', '₧', 'ƒ', 'á', 'í', 'ó', 'ú', 'ñ', 'Ñ',
    'ª', 'º', '¿', '⌐', '¬', '½', '¼', '¡', '«', '»', '░', '▒', '▓', '│', '┤', '╡', '╢', '╖', '╕',
    '╣', '║', '╗', '╝', '╜', '╛', '┐', '└', '┴', '┬', '├', '─', '┼', '╞', '╟', '╚', '╔', '╩', '╦',
    '╠', '═', '╬', '╧', '╨', '╤', '╥', '╙', '╘', '╒', '╓', '╫', '╪', '┘', '┌', '█', '▄', '▌', '▐',
    '▀', 'α', 'ß', 'Γ', 'π', 'Σ', 'σ', 'µ', 'τ', 'Φ', 'Θ', 'Ω', 'δ', '∞', 'φ', 'ε', '∩', '≡', '±',
    '≥', '≤', '⌠', '⌡', '÷', '≈', '°', '∙', '·', '√', 'ⁿ', '²', '■', '\u{a0}',
];

fn decode_name(raw: &[u8], utf8: bool) -> String {
    if utf8 || raw.is_ascii() {
        return String::from_utf8_lossy(raw).into_owned();
    }
    match std::str::from_utf8(raw) {
        // many tools write UTF-8 without setting the flag
        Ok(s) => s.to_string(),
        Err(_) => raw
            .iter()
            .map(|&b| {
                if b < 0x80 {
                    b as char
                } else {
                    CP437_HIGH[(b - 0x80) as usize]
                }
            })
            .collect(),
    }
}

/// Lower-case, `/`-separated, without `.`/`..`/empty components.
fn normalize(rel: &str) -> String {
    let mut parts: Vec<String> = Vec::new();
    for c in rel.split(['/', '\\']) {
        match c {
            "" | "." => {}
            ".." => {
                parts.pop();
            }
            c => parts.push(c.to_lowercase()),
        }
    }
    parts.join("/")
}

#[derive(Debug, Default)]
pub struct ZipSummary {
    pub files: u64,
    pub unpacked_bytes: u64,
    /// `Some(reason)` if the game can't read the archive in place.
    pub unreadable: Option<String>,
}

/// Reads only the central directory, never the entries' local headers. Seeking to each entry's
/// local header took minutes on large archives while an install was busy with the disk.
pub fn zip_summary(path: &Path) -> io::Result<ZipSummary> {
    let mut summary = ZipSummary::default();
    for (name, entry, is_dir) in ZipArchive::unindexed(path)?.central_directory()? {
        if !is_dir {
            summary.files += 1;
            summary.unpacked_bytes = summary.unpacked_bytes.saturating_add(entry.size);
        }
        if summary.unreadable.is_none() {
            summary.unreadable = if entry.flags & 1 != 0 {
                Some(format!("{name} is encrypted"))
            } else if !matches!(entry.method, 0 | 8) {
                Some(format!(
                    "{name} is compressed with method {} (only stored and deflated entries can be read in place)",
                    entry.method
                ))
            } else {
                None
            };
        }
    }
    Ok(summary)
}

impl ZipArchive {
    fn unindexed(path: &Path) -> io::Result<ZipArchive> {
        let file = std::fs::File::open(path)?;
        let stamp = stamp_of(&file.metadata()?);
        Ok(ZipArchive {
            path: path.to_path_buf(),
            file,
            stamp,
            #[cfg(not(unix))]
            lock: std::sync::Mutex::new(()),
            prefix: String::new(),
            entries: HashMap::new(),
            dirs: HashMap::new(),
        })
    }

    /// Read the central directory of `path`.
    pub fn open(path: &Path) -> io::Result<ZipArchive> {
        let mut archive = ZipArchive::unindexed(path)?;
        let raw = archive.central_directory()?;
        archive.prefix = content_prefix(raw.iter().map(|(name, _, _)| name.as_str()));
        let prefix_lower = archive.prefix.to_lowercase();
        let mut seen: std::collections::HashSet<String> = std::collections::HashSet::new();
        for (name, entry, is_dir) in raw {
            let lower = name.to_lowercase();
            let Some(rel_lower) = lower.strip_prefix(&prefix_lower) else {
                continue;
            };
            let rel_lower = normalize(rel_lower);
            if rel_lower.is_empty() {
                continue;
            }
            // The original spelling of every component, for listings.
            let rel: Vec<&str> = name[archive.prefix.len()..]
                .split('/')
                .filter(|part| !part.is_empty() && *part != ".")
                .collect();
            let lower_parts: Vec<&str> = rel_lower.split('/').collect();
            if rel.len() != lower_parts.len() {
                continue;
            }
            for depth in 0..rel.len() {
                let parent = lower_parts[..depth].join("/");
                let child_is_dir = depth + 1 < rel.len() || is_dir;
                if seen.insert(lower_parts[..=depth].join("/")) {
                    archive
                        .dirs
                        .entry(parent)
                        .or_default()
                        .push((rel[depth].to_string(), child_is_dir));
                }
            }
            if is_dir {
                archive.dirs.entry(rel_lower).or_default();
            } else {
                archive.entries.insert(rel_lower, entry);
            }
        }
        Ok(archive)
    }

    /// Every record of the central directory: the name with `/` separators, the entry, and
    /// whether it is a folder.
    fn central_directory(&self) -> io::Result<Vec<(String, ZipEntry, bool)>> {
        let len = self.stamp.0;
        // end of central directory: the last 22 bytes plus a comment of up to 65 535
        let tail_len = len.min(22 + 65_535);
        let tail = self.read_at(len - tail_len, tail_len as usize)?;
        let eocd = (0..tail.len().saturating_sub(21))
            .rev()
            .find(|&i| le32(&tail, i) == 0x0605_4b50)
            .ok_or_else(|| bad(format!("{}: not a zip archive", self.path.display())))?;
        let e = &tail[eocd..];
        let mut count = le16(e, 10) as u64;
        let mut cd_size = le32(e, 12) as u64;
        let mut cd_offset = le32(e, 16) as u64;
        // ZIP64: a locator right before the classic record points at the 64-bit one
        if eocd >= 20 && le32(&tail, eocd - 20) == 0x0706_4b50 {
            let at = le64(&tail, eocd - 20 + 8);
            let z = self.read_at(at, 56)?;
            if le32(&z, 0) != 0x0606_4b50 {
                return Err(bad(format!(
                    "{}: broken ZIP64 end record",
                    self.path.display()
                )));
            }
            count = le64(&z, 32);
            cd_size = le64(&z, 40);
            cd_offset = le64(&z, 48);
        }
        if cd_offset
            .checked_add(cd_size)
            .map(|end| end > len)
            .unwrap_or(true)
            || cd_size > 1 << 32
        {
            return Err(bad(format!(
                "{}: central directory out of range",
                self.path.display()
            )));
        }
        let cd = self.read_at(cd_offset, cd_size as usize)?;
        let mut raw: Vec<(String, ZipEntry, bool)> = Vec::with_capacity(count as usize);
        let mut p = 0usize;
        while p + 46 <= cd.len() && le32(&cd, p) == 0x0201_4b50 {
            let flags = le16(&cd, p + 8);
            let method = le16(&cd, p + 10);
            let crc = le32(&cd, p + 16);
            let mut compressed = le32(&cd, p + 20) as u64;
            let mut size = le32(&cd, p + 24) as u64;
            let name_len = le16(&cd, p + 28) as usize;
            let extra_len = le16(&cd, p + 30) as usize;
            let comment_len = le16(&cd, p + 32) as usize;
            let mut header = le32(&cd, p + 42) as u64;
            let end = p + 46 + name_len + extra_len + comment_len;
            if end > cd.len() {
                break;
            }
            let name_raw = &cd[p + 46..p + 46 + name_len];
            let extra = &cd[p + 46 + name_len..p + 46 + name_len + extra_len];
            let mut name = decode_name(name_raw, flags & 0x800 != 0);
            // extra fields: ZIP64 sizes/offset, and Info-ZIP's Unicode path
            let mut q = 0usize;
            while q + 4 <= extra.len() {
                let id = le16(extra, q);
                let n = le16(extra, q + 2) as usize;
                let body = &extra[(q + 4).min(extra.len())..(q + 4 + n).min(extra.len())];
                if id == 0x0001 {
                    let mut k = 0usize;
                    let mut take = |v: &mut u64| {
                        if *v == 0xFFFF_FFFF && k + 8 <= body.len() {
                            *v = le64(body, k);
                            k += 8;
                        }
                    };
                    take(&mut size);
                    take(&mut compressed);
                    take(&mut header);
                } else if id == 0x7075 && body.len() > 5 {
                    if let Ok(s) = std::str::from_utf8(&body[5..]) {
                        name = s.to_string();
                    }
                }
                q += 4 + n;
            }
            p = end;
            let name = name.replace('\\', "/");
            let is_dir = name.ends_with('/');
            raw.push((
                name,
                ZipEntry {
                    method,
                    flags,
                    crc,
                    compressed,
                    size,
                    header,
                },
                is_dir,
            ));
        }
        // Fewer records than declared means a damaged directory. More are fine: some old tools
        // write the count modulo 65536.
        if (raw.len() as u64) < count {
            return Err(bad(format!(
                "{}: damaged central directory ({} of {count} entries readable)",
                self.path.display(),
                raw.len()
            )));
        }
        Ok(raw)
    }

    #[cfg(unix)]
    fn read_at(&self, offset: u64, len: usize) -> io::Result<Vec<u8>> {
        use std::os::unix::fs::FileExt;
        let mut buf = vec![0u8; len];
        self.file.read_exact_at(&mut buf, offset)?;
        Ok(buf)
    }

    #[cfg(not(unix))]
    fn read_at(&self, offset: u64, len: usize) -> io::Result<Vec<u8>> {
        use std::io::Seek;
        let _g = self.lock.lock().unwrap();
        let mut f = &self.file;
        f.seek(io::SeekFrom::Start(offset))?;
        let mut buf = vec![0u8; len];
        f.read_exact(&mut buf)?;
        Ok(buf)
    }

    /// The archive file (= the mount point).
    pub fn path(&self) -> &Path {
        &self.path
    }

    /// The folder inside the archive the mount starts at.
    pub fn prefix(&self) -> &str {
        &self.prefix
    }

    pub fn file_count(&self) -> usize {
        self.entries.len()
    }

    /// Unpacked size of all files below the mount root.
    pub fn total_size(&self) -> u64 {
        self.entries.values().map(|e| e.size).sum()
    }

    /// Whether there is a file or folder at `rel` (relative to the mount root, any case).
    pub fn contains(&self, rel: &str) -> bool {
        let key = normalize(rel);
        self.is_file(&key) || self.is_dir(&key)
    }

    pub fn has_content(&self) -> bool {
        self.dirs
            .get("")
            .is_some_and(|listing| {
                listing
                    .iter()
                    .any(|(name, is_dir)| *is_dir && crate::is_content_folder(name))
            })
    }

    /// Contents of the entry at `rel` (relative to the mount root, any case).
    pub fn read(&self, rel: &str) -> io::Result<Vec<u8>> {
        let key = normalize(rel);
        let e = self.entries.get(&key).ok_or_else(|| {
            io::Error::new(
                io::ErrorKind::NotFound,
                format!("{}: no entry {rel}", self.path.display()),
            )
        })?;
        if e.flags & 1 != 0 {
            return Err(io::Error::new(
                io::ErrorKind::Unsupported,
                format!("{}: {rel} is encrypted", self.path.display()),
            ));
        }
        if e.size > MAX_ENTRY || e.compressed > MAX_ENTRY {
            return Err(bad(format!(
                "{}: {rel} is too large ({} bytes)",
                self.path.display(),
                e.size
            )));
        }
        let local = self.read_at(e.header, 30)?;
        if le32(&local, 0) != 0x0403_4b50 {
            return Err(bad(format!(
                "{}: {rel}: bad local header",
                self.path.display()
            )));
        }
        let data_at = e.header + 30 + le16(&local, 26) as u64 + le16(&local, 28) as u64;
        let packed = self.read_at(data_at, e.compressed as usize)?;
        let out = match e.method {
            0 => packed,
            8 => {
                let mut out = Vec::with_capacity(e.size as usize);
                flate2::read::DeflateDecoder::new(&packed[..])
                    .take(e.size + 1)
                    .read_to_end(&mut out)?;
                out
            }
            m => {
                return Err(io::Error::new(
                    io::ErrorKind::Unsupported,
                    format!(
                        "{}: {rel}: compression method {m} is not supported",
                        self.path.display()
                    ),
                ));
            }
        };
        if out.len() as u64 != e.size {
            return Err(bad(format!(
                "{}: {rel}: {} bytes unpacked, {} expected",
                self.path.display(),
                out.len(),
                e.size
            )));
        }
        let mut crc = flate2::Crc::new();
        crc.update(&out);
        if crc.sum() != e.crc {
            return Err(bad(format!(
                "{}: {rel}: checksum mismatch",
                self.path.display()
            )));
        }
        Ok(out)
    }

    fn is_file(&self, key: &str) -> bool {
        self.entries.contains_key(key)
    }

    fn is_dir(&self, key: &str) -> bool {
        self.dirs.contains_key(key)
    }
}

/// The folder that holds OMSI's content folders ("" for the top). It prefers the most top-level
/// content folders, then the most add-on ones (`Sound`, `Texture`, `Scripts`), then the shallowest.
pub fn content_prefix<'a>(names: impl Iterator<Item = &'a str>) -> String {
    const SHARED: [&str; 3] = ["Texture", "Sound", "Scripts"];
    let is_content = crate::is_content_folder;
    // folder (lower case) -> (spelling, lower-case names of its sub-folders)
    let mut children: HashMap<String, (String, Vec<String>)> = HashMap::new();
    for name in names {
        let parts: Vec<&str> = name.split('/').filter(|part| !part.is_empty()).collect();
        let folders = if name.ends_with('/') {
            parts.len()
        } else {
            parts.len().saturating_sub(1)
        };
        for depth in 0..folders.min(6) {
            // Folders inside a content folder belong to an add-on, so they can't be the root.
            if depth > 0 && is_content(parts[depth - 1]) {
                break;
            }
            let dir: String = parts[..depth].iter().map(|part| format!("{part}/")).collect();
            let found = children
                .entry(dir.to_lowercase())
                .or_insert_with(|| (dir.clone(), Vec::new()));
            let child = parts[depth].to_lowercase();
            if !found.1.contains(&child) {
                found.1.push(child);
            }
        }
    }
    children
        .iter()
        .filter_map(|(key, (dir, kids))| {
            let present: Vec<&str> = crate::CONTENT_FOLDERS
                .iter()
                .copied()
                .filter(|folder| kids.iter().any(|kid| kid.eq_ignore_ascii_case(folder)))
                .collect();
            if present.is_empty() {
                return None;
            }
            let shared = present
                .iter()
                .filter(|folder| SHARED.iter().any(|addon| addon.eq_ignore_ascii_case(folder)))
                .count();
            let depth = key.matches('/').count();
            Some((
                (
                    present.len() - shared,
                    shared,
                    std::cmp::Reverse(depth),
                    std::cmp::Reverse(key.as_str()),
                ),
                dir,
            ))
        })
        .max_by(|left, right| left.0.cmp(&right.0))
        .map(|(_, dir)| dir.clone())
        .unwrap_or_default()
}

/// Mounts the archive at `path` as the folder `path` and returns the mount point. Mounting it
/// again does nothing, unless the file was replaced since: then it is read again.
pub fn mount_zip(path: &Path) -> io::Result<PathBuf> {
    let now = std::fs::metadata(path).map(|meta| stamp_of(&meta)).ok();
    if let Some(mounted) = MOUNTS.read().unwrap().iter().find(|mounted| mounted.path == path)
        && now == Some(mounted.stamp)
    {
        return Ok(mounted.path.clone());
    }
    let t0 = std::time::Instant::now();
    let archive = ZipArchive::open(path)?;
    log::info!(
        "zip {}: {} files ({:.1} GB unpacked) under '{}', indexed in {:.0} ms",
        path.display(),
        archive.file_count(),
        archive.total_size() as f64 / 1e9,
        archive.prefix,
        t0.elapsed().as_secs_f64() * 1000.0
    );
    if !archive.has_content() {
        log::warn!(
            "zip {}: no OMSI content folders (Vehicles, maps, Sceneryobjects ...) found; mounted as it is",
            path.display()
        );
    }
    let mount = archive.path.clone();
    let mut mounts = MOUNTS.write().unwrap();
    mounts.retain(|mounted| mounted.path != mount);
    mounts.push(Arc::new(archive));
    Ok(mount)
}

pub fn unmount_zip(path: &Path) {
    MOUNTS.write().unwrap().retain(|mounted| mounted.path != path);
}

/// The mounted archives.
pub fn mounts() -> Vec<Arc<ZipArchive>> {
    MOUNTS.read().unwrap().clone()
}

/// The archive `path` lies in, and its lower-case path inside the mount. Names are read as
/// Windows reads them (a folder loses one trailing dot, the last name all trailing dots and
/// spaces), since the archive can only hold names Windows could write.
fn locate(path: &Path) -> Option<(Arc<ZipArchive>, String)> {
    let mounts = MOUNTS.read().unwrap();
    if mounts.is_empty() {
        return None;
    }
    for m in mounts.iter() {
        if let Ok(rest) = path.strip_prefix(&m.path) {
            let mut parts: Vec<String> = Vec::new();
            let comps: Vec<Component> = rest.components().collect();
            for (i, c) in comps.iter().enumerate() {
                match c {
                    Component::Normal(s) => {
                        let s = s.to_string_lossy();
                        let s: &str = &s;
                        let s = if i + 1 == comps.len() {
                            s.trim_end_matches(['.', ' '])
                        } else {
                            s.strip_suffix('.').unwrap_or(s)
                        };
                        if !s.is_empty() {
                            parts.push(s.to_lowercase());
                        }
                    }
                    Component::ParentDir => {
                        parts.pop();
                    }
                    _ => {}
                }
            }
            return Some((m.clone(), parts.join("/")));
        }
    }
    None
}

/// The archive file `path` lies in (the mount point), if it lies in a mounted one.
pub fn archive_of(path: &Path) -> Option<PathBuf> {
    locate(path).map(|(m, _)| m.path.clone())
}

/// Read a whole file, from a folder or from a mounted archive.
pub fn read(path: &Path) -> io::Result<Vec<u8>> {
    match locate(path) {
        Some((m, key)) => m.read(&key),
        None => std::fs::read(path),
    }
}

/// Read a text file with OMSI's encoding rules.
pub fn read_text(path: &Path) -> io::Result<String> {
    read(path).map(|b| crate::decode_text(&b))
}

pub fn exists(path: &Path) -> bool {
    match locate(path) {
        Some((m, key)) => key.is_empty() || m.is_file(&key) || m.is_dir(&key),
        None => path.exists(),
    }
}

pub fn is_file(path: &Path) -> bool {
    match locate(path) {
        Some((m, key)) => m.is_file(&key),
        None => path.is_file(),
    }
}

pub fn is_dir(path: &Path) -> bool {
    match locate(path) {
        Some((m, key)) => key.is_empty() || m.is_dir(&key),
        None => path.is_dir(),
    }
}

/// Names in a folder with a flag for sub-folders; `None` when it is not a folder.
pub fn list_dir(path: &Path) -> Option<Vec<(OsString, bool)>> {
    match locate(path) {
        Some((m, key)) => m
            .dirs
            .get(&key)
            .map(|l| l.iter().map(|(n, d)| (OsString::from(n), *d)).collect()),
        None => {
            let rd = std::fs::read_dir(path).ok()?;
            // the entry's own type costs nothing; only a symbolic link needs a stat
            Some(
                rd.flatten()
                    .map(|e| {
                        let dir = match e.file_type() {
                            Ok(t) if t.is_symlink() => e.path().is_dir(),
                            Ok(t) => t.is_dir(),
                            Err(_) => false,
                        };
                        (e.file_name(), dir)
                    })
                    .collect(),
            )
        }
    }
}

/// Full paths of a folder's entries (like `std::fs::read_dir`), empty when it is not a folder.
pub fn read_dir_paths(path: &Path) -> Vec<PathBuf> {
    list_dir(path)
        .map(|l| l.into_iter().map(|(n, _)| path.join(n)).collect())
        .unwrap_or_default()
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;

    /// A small archive with a wrapper folder, a stored and a deflated entry.
    fn build_zip(path: &Path) {
        let files: Vec<(&str, Vec<u8>, bool)> = vec![
            ("Docs/readme.txt", b"hello".to_vec(), false),
            (
                "OMSI 2/maps/Test Map/global.cfg",
                b"[name]\r\nTest\r\n".repeat(20),
                true,
            ),
            (
                "OMSI 2/Sceneryobjects/Stra\u{df}e/Schild.sco",
                b"[mesh]\r\nschild.o3d\r\n".to_vec(),
                false,
            ),
            ("OMSI 2/Vehicles/", Vec::new(), false),
        ];
        write_zip(path, &files);
    }

    /// An archive of `files`: (name, contents, deflated).
    fn write_zip(path: &Path, files: &[(&str, Vec<u8>, bool)]) {
        let mut out: Vec<u8> = Vec::new();
        let mut central: Vec<u8> = Vec::new();
        for (name, data, deflate) in files {
            let mut crc = flate2::Crc::new();
            crc.update(data);
            let packed = if *deflate {
                let mut e =
                    flate2::write::DeflateEncoder::new(Vec::new(), flate2::Compression::default());
                e.write_all(data).unwrap();
                e.finish().unwrap()
            } else {
                data.clone()
            };
            let method: u16 = if *deflate { 8 } else { 0 };
            let offset = out.len() as u32;
            let head = |sig: u32, buf: &mut Vec<u8>, central: bool| {
                buf.extend_from_slice(&sig.to_le_bytes());
                if central {
                    buf.extend_from_slice(&20u16.to_le_bytes());
                }
                buf.extend_from_slice(&20u16.to_le_bytes());
                buf.extend_from_slice(&0x800u16.to_le_bytes());
                buf.extend_from_slice(&method.to_le_bytes());
                buf.extend_from_slice(&[0; 4]);
                buf.extend_from_slice(&crc.sum().to_le_bytes());
                buf.extend_from_slice(&(packed.len() as u32).to_le_bytes());
                buf.extend_from_slice(&(data.len() as u32).to_le_bytes());
                buf.extend_from_slice(&(name.len() as u16).to_le_bytes());
                buf.extend_from_slice(&0u16.to_le_bytes());
                if central {
                    // comment length, disk, internal and external attributes
                    buf.extend_from_slice(&[0; 10]);
                    buf.extend_from_slice(&offset.to_le_bytes());
                }
                buf.extend_from_slice(name.as_bytes());
            };
            head(0x0403_4b50, &mut out, false);
            out.extend_from_slice(&packed);
            head(0x0201_4b50, &mut central, true);
        }
        let cd_offset = out.len() as u32;
        out.extend_from_slice(&central);
        out.extend_from_slice(&0x0605_4b50u32.to_le_bytes());
        out.extend_from_slice(&[0; 4]);
        out.extend_from_slice(&(files.len() as u16).to_le_bytes());
        out.extend_from_slice(&(files.len() as u16).to_le_bytes());
        out.extend_from_slice(&(central.len() as u32).to_le_bytes());
        out.extend_from_slice(&cd_offset.to_le_bytes());
        out.extend_from_slice(&0u16.to_le_bytes());
        std::fs::write(path, out).unwrap();
    }

    #[test]
    fn zip_summary_reads_the_central_directory() {
        let dir =
            std::env::temp_dir().join(format!("legacy-config-summary-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let zip = dir.join("pack.zip");
        build_zip(&zip);
        let summary = zip_summary(&zip).unwrap();
        // Every file counts, including the ones outside the content folder, but folders don't.
        assert_eq!(summary.files, 3);
        assert_eq!(summary.unpacked_bytes, 5 + 14 * 20 + 20);
        assert_eq!(summary.unreadable, None);
        // The first central record is Docs/readme.txt; its flags are at +8 and its method at +10.
        let bytes = std::fs::read(&zip).unwrap();
        let records: Vec<usize> = bytes
            .windows(4)
            .enumerate()
            .filter(|(_, window)| *window == 0x0201_4b50u32.to_le_bytes())
            .map(|(offset, _)| offset)
            .collect();
        for (at, value, expected) in [
            (
                records[0] + 10,
                12u16,
                "Docs/readme.txt is compressed with method 12",
            ),
            (records[0] + 8, 1, "Docs/readme.txt is encrypted"),
        ] {
            let mut patched = bytes.clone();
            patched[at..at + 2].copy_from_slice(&value.to_le_bytes());
            std::fs::write(&zip, &patched).unwrap();
            let problem = zip_summary(&zip).unwrap().unreadable.unwrap();
            assert!(problem.starts_with(expected), "{problem}");
        }
        // A broken second record leaves fewer records than the archive declares.
        let mut damaged = bytes.clone();
        damaged[records[1]] = 0;
        std::fs::write(&zip, &damaged).unwrap();
        let error = zip_summary(&zip).unwrap_err().to_string();
        assert!(
            error.contains("damaged central directory (1 of 4"),
            "{error}"
        );
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn zip_mount() {
        let dir = std::env::temp_dir().join(format!("legacy-config-zip-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let zip = dir.join("pack.zip");
        build_zip(&zip);
        let mount = mount_zip(&zip).unwrap();
        let a = mounts().into_iter().find(|m| m.path() == zip).unwrap();
        assert_eq!(a.prefix(), "OMSI 2/");
        let cfg = mount.join("MAPS").join("test map").join("Global.CFG");
        assert!(is_file(&cfg));
        assert_eq!(read(&cfg).unwrap(), b"[name]\r\nTest\r\n".repeat(20));
        assert!(is_dir(&mount.join("maps")));
        assert!(is_dir(&mount.join("vehicles")));
        assert!(!exists(&mount.join("Docs")));
        assert!(
            crate::resolve_existing(
                &mount,
                &crate::windows_components("sceneryobjects\\STRASSE_DOES_NOT_EXIST\\x.sco")
            )
                .is_none()
        );
        let sco = crate::resolve_existing(
            &mount,
            &crate::windows_components("Sceneryobjects\\straße\\schild.sco"),
        )
            .unwrap();
        assert!(is_file(&sco), "{}", sco.display());
        assert_eq!(read(&sco).unwrap(), b"[mesh]\r\nschild.o3d\r\n");
        let names: Vec<String> = list_dir(&mount)
            .unwrap()
            .into_iter()
            .map(|(n, _)| n.to_string_lossy().into_owned())
            .collect();
        assert!(
            names.contains(&"maps".to_string()) && names.contains(&"Sceneryobjects".to_string()),
            "{names:?}"
        );
        let f = crate::CfgFile::read(&cfg).unwrap();
        assert_eq!(f.lines[1], "Test");
        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn a_replaced_archive_is_read_anew_and_a_removed_one_unmounted() {
        let dir = std::env::temp_dir().join(format!("legacy-config-rezip-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let zip = dir.join("bus.zip");
        let file = |data: &[u8]| vec![("Vehicles/Bus/bus.bus", data.to_vec(), false)];
        write_zip(&zip, &file(b"old"));
        let mount = mount_zip(&zip).unwrap();
        let bus = mount.join("Vehicles/Bus/bus.bus");
        assert_eq!(read(&bus).unwrap(), b"old");
        write_zip(&zip, &file(b"the new one"));
        mount_zip(&zip).unwrap();
        assert_eq!(read(&bus).unwrap(), b"the new one");
        assert_eq!(mounts().iter().filter(|mounted| mounted.path() == zip).count(), 1);
        unmount_zip(&zip);
        assert!(archive_of(&bus).is_none());
        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn mount_root_of_an_addon_archive() {
        let prefix = |names: &[&str]| content_prefix(names.iter().copied());
        // a bus: its own Sound and Texture folders do not make it the root
        let bus = [
            "OMSI 2/Vehicles/MAN_Lion/Lion.bus",
            "OMSI 2/Vehicles/MAN_Lion/Model/model.cfg",
            "OMSI 2/Vehicles/MAN_Lion/Script/main.osc",
            "OMSI 2/Vehicles/MAN_Lion/Sound/sound.cfg",
            "OMSI 2/Vehicles/MAN_Lion/Texture/body.dds",
            "Readme.txt",
        ];
        assert_eq!(prefix(&bus), "OMSI 2/");
        // two buses tie between themselves, never with the root
        let two = [
            "Vehicles/A/Sound/a.wav",
            "Vehicles/A/Texture/a.dds",
            "Vehicles/B/Sound/b.wav",
            "Vehicles/B/Texture/b.dds",
        ];
        assert_eq!(prefix(&two), "");
        // a scenery pack
        let pack = [
            "Pack v2/Sceneryobjects/Pack/model/a.o3d",
            "Pack v2/Sceneryobjects/Pack/texture/a.bmp",
            "Pack v2/Sceneryobjects/Pack/sound/a.wav",
            "Pack v2/Sceneryobjects/Pack/a.sco",
        ];
        assert_eq!(prefix(&pack), "Pack v2/");
        // a folder with the top-level names wins over one with only the shared ones
        let mixed = [
            "Extras/Bonus/Sound/x.wav",
            "Extras/Bonus/Texture/x.dds",
            "OMSI 2/maps/M/global.cfg",
        ];
        assert_eq!(prefix(&mixed), "OMSI 2/");
        // a texture pack still has its root
        assert_eq!(
            prefix(&["Winter/Texture/snow.dds", "Winter/readme.txt"]),
            "Winter/"
        );
        // on a tie, the shallowest, then the first by name
        assert_eq!(prefix(&["b/Vehicles/x.bus", "a/Vehicles/y.bus"]), "a/");
        assert_eq!(prefix(&["docs/readme.txt"]), "");
    }

    #[test]
    fn names() {
        assert_eq!(
            decode_name(&[b'S', b't', b'r', b'a', 0xE1, b'e'], false),
            "Straße"
        );
        assert_eq!(normalize("a\\B/./c/../D"), "a/b/d");
    }
}
