//! The map's surfaces kept between sessions (`~/.neoomsi/cache/navmap`): building them takes
//! a minute of a few cores on a big map, every start, while the player is already driving.
//! A cached build holds the files it was read from with their sizes and times; one changed,
//! added or gone, and the surfaces are built again :O

use std::path::{Path, PathBuf};
use std::sync::Arc;

use glam::DVec3;
use hashbrown::HashSet;
use parking_lot::Mutex;

use super::*;
use crate::scene::World;

/// Raise when what the build makes changes: older files are then built again
const FORMAT: u32 = 13;
const MAGIC: &[u8; 8] = b"NOMSINAV";

/// The files a build read, or looked for and did not find
#[derive(Default)]
pub(crate) struct Deps(Mutex<HashSet<PathBuf>>);

impl Deps {
    pub(crate) fn add(&self, p: &Path) {
        let mut s = self.0.lock();
        if !s.contains(p) {
            s.insert(p.to_path_buf());
        }
    }

    pub(crate) fn into_vec(self) -> Vec<PathBuf> {
        let mut v: Vec<PathBuf> = self.0.into_inner().into_iter().collect();
        v.sort();
        v
    }
}

/// A file's size and time (ns), those of the archive it lies in if it lies in one, or
/// `u64::MAX` twice if it is not there
fn stamp(p: &Path) -> (u64, u64) {
    let meta = std::fs::metadata(p)
        .ok()
        .or_else(|| ::legacy_config::vfs::archive_of(p).and_then(|a| std::fs::metadata(a).ok()));
    match meta {
        Some(m) => (
            m.len(),
            m.modified()
                .ok()
                .and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok())
                .map_or(0, |d| d.as_nanos() as u64),
        ),
        None => (u64::MAX, u64::MAX),
    }
}

/// The cache file of the map as it is opened: its folder, the content folders and archives
/// files are looked for in, the chrono folders and the detail.
///
/// The file is named after the map and then after the rest: a new one replaces the map's
/// others (see `save`).
fn path_of(world: &World) -> Option<PathBuf> {
    use std::hash::{Hash, Hasher};
    let mut h = std::collections::hash_map::DefaultHasher::new();
    world.root.hash(&mut h);
    world.map_dir.hash(&mut h);
    let map = h.finish();
    FORMAT.hash(&mut h);
    world.chrono_dirs.read().hash(&mut h);
    world.map_detail().hash(&mut h);
    ::mods::content_roots().hash(&mut h);
    for m in ::legacy_config::vfs::mounts() {
        m.path().hash(&mut h);
    }
    let home = std::env::var_os("HOME").or_else(|| std::env::var_os("USERPROFILE"))?;
    Some(
        PathBuf::from(home)
            .join(".neoomsi")
            .join("cache")
            .join("navmap")
            .join(format!("{map:016x}-{:016x}.bin", h.finish())),
    )
}

/// The map's surfaces as a build left them, if none of the files it read has changed since
pub fn load(world: &World) -> Option<(Vec<((i32, i32), Arc<Chunk>)>, SurfaceMap)> {
    let t0 = std::time::Instant::now();
    let path = path_of(world)?;
    let bytes = std::fs::read(&path).ok()?;
    let mut r = Reader { b: &bytes, at: 0 };
    if r.take(8)? != MAGIC || r.u32()? != FORMAT {
        return None;
    }
    let deps = r.u32()?;
    for _ in 0..deps {
        let p = PathBuf::from(r.str()?);
        let st = (r.u64()?, r.u64()?);
        if stamp(&p) != st {
            log::info!("navigator surfaces: {} changed, built again", p.display());
            return None;
        }
    }
    let mut map = SurfaceMap::default();
    for _ in 0..r.u32()? {
        let name = r.str()?;
        let surface = Surface::from_u8(r.u8()?);
        map.materials.push(MaterialInfo {
            name,
            surface,
            area: r.f64()?,
            car: r.f64()?,
            walk: r.f64()?,
            rail: r.f64()?,
        });
    }
    for _ in 0..r.u32()? {
        let n = r.u32()?;
        let mut rail = Vec::with_capacity(n as usize);
        for _ in 0..n {
            rail.push(DVec3::new(r.f64()?, r.f64()?, r.f64()?));
        }
        map.rails.push(rail);
    }
    let count = r.u32()?;
    let mut chunks = Vec::with_capacity(count as usize);
    for _ in 0..count {
        let key = (r.u32()? as i32, r.u32()? as i32);
        let areas = r.areas()?;
        let coarse = r.areas()?;
        chunks.push((key, Arc::new(Chunk { areas, coarse })));
    }
    (r.at == bytes.len()).then_some(())?;
    log::info!(
        "navigator surfaces: {} chunks read from {} in {:.2} s ({} files checked)",
        chunks.len(),
        path.display(),
        t0.elapsed().as_secs_f64(),
        deps
    );
    Some((chunks, map))
}

/// Keep a build of the map's surfaces (`read`: the files it read) for the next session
pub fn save(
    world: &World,
    chunks: &[((i32, i32), Arc<Chunk>)],
    map: &SurfaceMap,
    read: Vec<PathBuf>,
) {
    let Some(path) = path_of(world) else {
        return;
    };
    let mut w = Vec::with_capacity(64 << 20);
    w.extend_from_slice(MAGIC);
    put_u32(&mut w, FORMAT);
    put_u32(&mut w, read.len() as u32);
    for p in &read {
        put_str(&mut w, &p.to_string_lossy());
        let (size, time) = stamp(p);
        w.extend_from_slice(&size.to_le_bytes());
        w.extend_from_slice(&time.to_le_bytes());
    }
    put_u32(&mut w, map.materials.len() as u32);
    for m in &map.materials {
        put_str(&mut w, &m.name);
        w.push(m.surface.map_or(0, |s| s as u8));
        for v in [m.area, m.car, m.walk, m.rail] {
            w.extend_from_slice(&v.to_le_bytes());
        }
    }
    put_u32(&mut w, map.rails.len() as u32);
    for rail in &map.rails {
        put_u32(&mut w, rail.len() as u32);
        for p in rail {
            for v in [p.x, p.y, p.z] {
                w.extend_from_slice(&v.to_le_bytes());
            }
        }
    }
    put_u32(&mut w, chunks.len() as u32);
    for (k, c) in chunks {
        put_u32(&mut w, k.0 as u32);
        put_u32(&mut w, k.1 as u32);
        put_areas(&mut w, &c.areas);
        put_areas(&mut w, &c.coarse);
    }
    // (written beside and moved over: a game stopped while writing leaves no half a file)
    let tmp = path.with_extension(format!("tmp{}", std::process::id()));
    let written = path
        .parent()
        .map_or(Ok(()), std::fs::create_dir_all)
        .and_then(|_| std::fs::write(&tmp, &w))
        .and_then(|_| std::fs::rename(&tmp, &path));
    match written {
        Ok(()) => {
            log::info!(
                "navigator surfaces: kept in {} ({:.0} MB)",
                path.display(),
                w.len() as f64 / 1e6
            );
            forget_others(&path);
        }
        Err(e) => {
            let _ = std::fs::remove_file(&tmp);
            log::warn!("navigator surfaces: not kept in {}: {e}", path.display());
        }
    }
}

/// Remove the map's other files beside `path` (an older format, other content folders or
/// chrono days: tens of megabytes each, never read again until they come back) and those of
/// the first format, named without the map.
fn forget_others(path: &Path) {
    let (Some(dir), Some(name)) = (path.parent(), path.file_name().and_then(|n| n.to_str())) else {
        return;
    };
    let Some((map, _)) = name.split_once('-') else {
        return;
    };
    let Ok(entries) = std::fs::read_dir(dir) else {
        return;
    };
    for e in entries.flatten() {
        let other = e.file_name();
        let Some(other) = other.to_str() else {
            continue;
        };
        let ours = other.split_once('-').is_none_or(|(m, _)| m == map);
        if ours && other != name && other.ends_with(".bin") {
            let _ = std::fs::remove_file(e.path());
        }
    }
}

fn put_u32(w: &mut Vec<u8>, v: u32) {
    w.extend_from_slice(&v.to_le_bytes());
}

fn put_str(w: &mut Vec<u8>, s: &str) {
    put_u32(w, s.len() as u32);
    w.extend_from_slice(s.as_bytes());
}

fn put_points(w: &mut Vec<u8>, pts: &[[f32; 2]]) {
    put_u32(w, pts.len() as u32);
    for p in pts {
        w.extend_from_slice(&p[0].to_le_bytes());
        w.extend_from_slice(&p[1].to_le_bytes());
    }
}

fn put_areas(w: &mut Vec<u8>, areas: &[Area]) {
    put_u32(w, areas.len() as u32);
    for a in areas {
        w.push(a.layer as u8);
        w.push(a.level as u8);
        put_points(w, &a.verts);
        put_u32(w, a.tris.len() as u32);
        for t in &a.tris {
            w.extend_from_slice(&t.to_le_bytes());
        }
        put_u32(w, a.edges.len() as u32);
        for e in &a.edges {
            put_points(w, e);
        }
    }
}

struct Reader<'a> {
    b: &'a [u8],
    at: usize,
}

impl<'a> Reader<'a> {
    fn take(&mut self, n: usize) -> Option<&'a [u8]> {
        let s = self.b.get(self.at..self.at.checked_add(n)?)?;
        self.at += n;
        Some(s)
    }

    fn u8(&mut self) -> Option<u8> {
        Some(self.take(1)?[0])
    }

    fn u32(&mut self) -> Option<u32> {
        Some(u32::from_le_bytes(self.take(4)?.try_into().ok()?))
    }

    fn u64(&mut self) -> Option<u64> {
        Some(u64::from_le_bytes(self.take(8)?.try_into().ok()?))
    }

    fn f32(&mut self) -> Option<f32> {
        Some(f32::from_le_bytes(self.take(4)?.try_into().ok()?))
    }

    fn f64(&mut self) -> Option<f64> {
        Some(f64::from_le_bytes(self.take(8)?.try_into().ok()?))
    }

    fn str(&mut self) -> Option<String> {
        let n = self.u32()? as usize;
        String::from_utf8(self.take(n)?.to_vec()).ok()
    }

    fn points(&mut self) -> Option<Vec<[f32; 2]>> {
        let n = self.u32()? as usize;
        // (a damaged count does not get to ask for gigabytes)
        let mut v = Vec::with_capacity(n.min(self.b.len() / 8));
        for _ in 0..n {
            v.push([self.f32()?, self.f32()?]);
        }
        Some(v)
    }

    fn areas(&mut self) -> Option<Vec<Area>> {
        let n = self.u32()?;
        let mut out = Vec::new();
        for _ in 0..n {
            let layer = *Layer::ALL.get(self.u8()? as usize)?;
            let level = self.u8()? as i8;
            let verts = self.points()?;
            let nt = self.u32()? as usize;
            let mut tris = Vec::with_capacity(nt.min(self.b.len() / 4));
            for _ in 0..nt {
                tris.push(self.u32()?);
            }
            if tris.iter().any(|&t| t as usize >= verts.len()) {
                return None;
            }
            let ne = self.u32()?;
            let mut edges = Vec::new();
            for _ in 0..ne {
                edges.push(self.points()?);
            }
            out.push(Area {
                layer,
                level,
                verts,
                tris,
                edges,
            });
        }
        Some(out)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn areas_come_back_as_they_were_kept() {
        let areas = vec![
            Area {
                layer: Layer::Drivable,
                level: -1,
                verts: vec![[0.0, 0.0], [64.0, 0.0], [64.0, 12.5]],
                tris: vec![0, 1, 2],
                edges: vec![vec![[64.0, 12.5], [0.0, 0.0]]],
            },
            Area {
                layer: Layer::Marking,
                level: 1,
                verts: vec![[1.0, 2.0], [3.0, 4.0], [5.0, -6.0]],
                tris: vec![2, 1, 0],
                edges: Vec::new(),
            },
        ];
        let mut w = Vec::new();
        put_areas(&mut w, &areas);
        let mut r = Reader { b: &w, at: 0 };
        let back = r.areas().unwrap();
        assert_eq!(r.at, w.len());
        assert_eq!(format!("{areas:?}"), format!("{back:?}"));
        // a triangle naming a corner the area has not is a damaged file
        let mut bad = areas.clone();
        bad[0].tris = vec![0, 1, 3];
        let mut w = Vec::new();
        put_areas(&mut w, &bad);
        assert!(Reader { b: &w, at: 0 }.areas().is_none());
    }
}
