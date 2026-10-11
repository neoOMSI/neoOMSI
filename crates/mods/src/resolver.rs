use legacy_config::{content_changed, find_in_dir, name_key, resolve_existing_with, vfs, windows_components};
use std::io;
use std::path::{Path, PathBuf};

fn resolve_existing(base: &Path, rel: &[&str]) -> Option<PathBuf> {
    resolve_existing_with(base, rel, &|cur| {
        !CONTENT_ROOTS.lock().unwrap().iter().any(|r| r == cur)
    })
}

fn pop_within_roots(cur: &mut PathBuf) {
    if CONTENT_ROOTS.lock().unwrap().iter().any(|r| r == cur) {
        return;
    }
    cur.pop();
}

static CONTENT_ROOTS: std::sync::Mutex<Vec<PathBuf>> = std::sync::Mutex::new(Vec::new());

pub fn add_content_root(root: PathBuf) {
    let mut r = CONTENT_ROOTS.lock().unwrap();
    if !r.iter().any(|x| x == &root) {
        r.push(root);
        drop(r);
        content_changed();
    }
}

pub fn add_content_root_before(root: PathBuf, before: &Path) {
    let mut r = CONTENT_ROOTS.lock().unwrap();
    if r.iter().any(|x| x == &root) {
        return;
    }
    match r.iter().position(|x| x == before) {
        Some(i) => r.insert(i, root),
        None => r.push(root),
    }
    drop(r);
    content_changed();
}

pub fn add_content_root_first(root: PathBuf) {
    let mut r = CONTENT_ROOTS.lock().unwrap();
    r.retain(|x| x != &root);
    r.insert(0, root);
    drop(r);
    content_changed();
}

pub fn remove_content_root(root: &Path) {
    CONTENT_ROOTS.lock().unwrap().retain(|x| x != root);
    content_changed();
}

static ROOT_SCOPES: std::sync::Mutex<Vec<(PathBuf, Vec<PathBuf>)>> =
    std::sync::Mutex::new(Vec::new());

pub fn set_root_scope(root: PathBuf, fallbacks: Vec<PathBuf>) {
    let mut s = ROOT_SCOPES.lock().unwrap();
    s.retain(|(r, _)| r != &root);
    s.push((root, fallbacks));
    drop(s);
    content_changed();
}

pub fn clear_root_scope(root: &Path) {
    ROOT_SCOPES.lock().unwrap().retain(|(r, _)| r != root);
    content_changed();
}

fn chain_for(base: &Path) -> Vec<PathBuf> {
    let roots = content_roots();
    let Some(owner) = owner_root(base, &roots).cloned() else {
        return roots;
    };
    let scopes = ROOT_SCOPES.lock().unwrap();
    match scopes.iter().find(|(r, _)| r == &owner) {
        Some((_, fallbacks)) if fallbacks.is_empty() && owner == base => roots,
        Some((_, fallbacks)) => {
            let mut chain = vec![owner];
            for f in fallbacks {
                if !chain.contains(f) {
                    chain.push(f.clone());
                }
            }
            chain
        }
        None => roots,
    }
}

static SANDBOX_ROOTS: std::sync::Mutex<Vec<PathBuf>> = std::sync::Mutex::new(Vec::new());

pub fn mark_sandbox(root: PathBuf) {
    let mut r = SANDBOX_ROOTS.lock().unwrap();
    if !r.contains(&root) {
        r.push(root);
    }
}

pub fn is_sandbox(path: &Path) -> bool {
    SANDBOX_ROOTS
        .lock()
        .unwrap()
        .iter()
        .any(|r| path.starts_with(r))
}

pub fn find_in_roots(rel: &str) -> Option<(PathBuf, PathBuf)> {
    let comps = windows_components(rel);
    content_roots()
        .into_iter()
        .find_map(|r| resolve_existing(&r, &comps).map(|p| (r, p)))
}

pub fn content_roots() -> Vec<PathBuf> {
    CONTENT_ROOTS.lock().unwrap().clone()
}

pub fn mirrored_dirs(dir: &Path) -> Vec<PathBuf> {
    let roots = content_roots();
    let Some(suffix) = owner_root(dir, &roots).and_then(|r| dir.strip_prefix(r).ok()) else {
        return vec![dir.to_path_buf()];
    };
    let rel: Vec<String> = suffix
        .components()
        .map(|c| c.as_os_str().to_string_lossy().into_owned())
        .collect();
    let rel: Vec<&str> = rel.iter().map(|s| s.as_str()).collect();
    let mut out: Vec<PathBuf> = Vec::new();
    for r in &roots {
        let found = if rel.is_empty() {
            Some(r.clone())
        } else {
            resolve_existing(r, &rel)
        };
        if let Some(p) = found.filter(|p| vfs::is_dir(p)) {
            if !out.contains(&p) {
                out.push(p);
            }
        }
    }
    if out.is_empty() {
        out.push(dir.to_path_buf());
    }
    out
}

fn owner_root<'a>(path: &Path, roots: &'a [PathBuf]) -> Option<&'a PathBuf> {
    roots
        .iter()
        .filter(|r| path.starts_with(r))
        .max_by_key(|r| r.components().count())
}

fn mirrored_base(base: &Path, root: &Path, roots: &[PathBuf]) -> Option<PathBuf> {
    let owner = owner_root(base, roots)?;
    if owner == root {
        return None;
    }
    base.strip_prefix(owner)
        .ok()
        .map(|suffix| root.join(suffix))
}

fn vehicle_package(base: &Path, owner: &Path, rel: &[&str]) -> Option<String> {
    let suffix = base.strip_prefix(owner).ok()?;
    let mut parts: Vec<String> = suffix
        .components()
        .filter_map(|c| match c {
            std::path::Component::Normal(n) => Some(n.to_string_lossy().into_owned()),
            _ => None,
        })
        .collect();
    for comp in rel {
        if *comp == ".." {
            parts.pop();
        } else {
            parts.push(comp.to_string());
        }
    }
    if parts.len() >= 3 && parts[0].eq_ignore_ascii_case("Vehicles") {
        Some(format!("Vehicles/{}", parts[1]))
    } else {
        None
    }
}

fn package_definitions(root: &Path, package: &str) -> std::collections::HashSet<String> {
    let comps: Vec<&str> = package.split('/').collect();
    let Some(dir) = resolve_existing(root, &comps) else {
        return Default::default();
    };
    vfs::list_dir(&dir)
        .unwrap_or_default()
        .into_iter()
        .filter(|(_, is_dir)| !is_dir)
        .map(|(n, _)| name_key(&n.to_string_lossy()))
        .filter(|n| n.ends_with(".bus") || n.ends_with(".ovh") || n.ends_with(".zug"))
        .collect()
}

fn overlays_package(over: &Path, owner: &Path, package: &str) -> bool {
    type Key = (PathBuf, PathBuf, String);
    static CACHE: std::sync::OnceLock<std::sync::Mutex<std::collections::HashMap<Key, bool>>> =
        std::sync::OnceLock::new();
    let cache = CACHE.get_or_init(Default::default);
    let key = (
        over.to_path_buf(),
        owner.to_path_buf(),
        package.to_ascii_lowercase(),
    );
    if let Some(v) = cache.lock().unwrap().get(&key) {
        return *v;
    }
    let comps: Vec<&str> = package.split('/').collect();
    let owner_has = resolve_existing(owner, &comps)
        .map(|d| vfs::is_dir(&d))
        .unwrap_or(false);
    let theirs = package_definitions(over, package);
    let ok =
        !owner_has || theirs.is_empty() || theirs.is_subset(&package_definitions(owner, package));
    cache.lock().unwrap().insert(key, ok);
    ok
}

pub fn missing_vehicle_pack(path: &Path) -> Option<String> {
    let roots = content_roots();
    let owner = owner_root(path, &roots)?.clone();
    let suffix = path.strip_prefix(&owner).ok()?;
    let mut comps = suffix.components().filter_map(|c| match c {
        std::path::Component::Normal(n) => Some(n.to_string_lossy().into_owned()),
        _ => None,
    });
    let first = comps.next()?;
    let pack = comps.next()?;
    if !first.eq_ignore_ascii_case("Vehicles") || comps.next().is_none() {
        return None;
    }
    let installed = roots.iter().any(|r| {
        resolve_existing(r, &["Vehicles", pack.as_str()])
            .map(|d| vfs::is_dir(&d))
            .unwrap_or(false)
    });
    (!installed).then_some(pack)
}

pub fn content_dirs(rel: &str) -> Vec<PathBuf> {
    let rel = windows_components(rel);
    content_roots()
        .iter()
        .filter_map(|r| resolve_existing(r, &rel))
        .filter(|p| vfs::is_dir(p))
        .collect()
}

pub fn read_dir_merged(rel: &str) -> Vec<PathBuf> {
    let mut out: Vec<PathBuf> = Vec::new();
    let mut seen: std::collections::HashSet<String> = std::collections::HashSet::new();
    for d in content_dirs(rel) {
        for (name, _) in vfs::list_dir(&d).unwrap_or_default() {
            if seen.insert(name_key(&name.to_string_lossy())) {
                out.push(d.join(name));
            }
        }
    }
    out
}

pub fn resolve_path(base: &Path, rel: &str) -> PathBuf {
    let rel = windows_components(rel);
    let rel = rel.as_slice();
    let roots = chain_for(base);
    let owner = if roots.len() > 1 {
        owner_root(base, &roots).cloned()
    } else {
        None
    };
    if let Some(owner) = &owner {
        let package = vehicle_package(base, owner, rel);
        for r in roots.iter().take_while(|r| *r != owner) {
            if let Some(pkg) = &package {
                if !overlays_package(r, owner, pkg) {
                    continue;
                }
            }
            if let Some(mb) = mirrored_base(base, r, &roots) {
                if let Some(p) = resolve_existing(&mb, rel) {
                    return p;
                }
            }
        }
    }
    let mut cur = base.to_path_buf();
    for comp in rel {
        if *comp == ".." {
            pop_within_roots(&mut cur);
            continue;
        }
        let direct = cur.join(comp);
        if vfs::exists(&direct) {
            cur = direct;
            continue;
        }
        let found = find_in_dir(&cur, comp).map(|n| cur.join(n));
        cur = found.unwrap_or(direct);
    }
    if let (Some(owner), false) = (&owner, vfs::exists(&cur)) {
        for r in roots.iter().filter(|r| *r != owner) {
            if let Some(mb) = mirrored_base(base, r, &roots) {
                if let Some(p) = resolve_existing(&mb, rel) {
                    return p;
                }
            }
        }
    }
    let all = content_roots();
    if owner_root(base, &all).is_some() && owner_root(&cur, &all).is_none() {
        return base.to_path_buf();
    }
    cur
}

pub fn add_content_zip(path: &Path) -> io::Result<PathBuf> {
    let mount = vfs::mount_zip(path)?;
    add_content_root(mount.clone());
    Ok(mount)
}

pub fn mount_dir_zips(dir: &Path) -> Vec<PathBuf> {
    let mut zips: Vec<PathBuf> = match std::fs::read_dir(dir) {
        Ok(rd) => rd
            .flatten()
            .map(|e| e.path())
            .filter(|p| {
                p.is_file()
                    && p.extension()
                    .map(|e| e.eq_ignore_ascii_case("zip"))
                    .unwrap_or(false)
            })
            .collect(),
        Err(_) => return Vec::new(),
    };
    zips.sort();
    let mut out = Vec::new();
    for p in zips {
        match add_content_zip(&p) {
            Ok(m) => out.push(m),
            Err(e) => log::warn!("content zip {}: {e}", p.display()),
        }
    }
    out
}

pub fn mount_env_zips() -> Vec<PathBuf> {
    let Some(v) = std::env::var_os("OMSI_CONTENT_ZIP") else {
        return Vec::new();
    };
    let mut out = Vec::new();
    for p in std::env::split_paths(&v) {
        if p.as_os_str().is_empty() {
            continue;
        }
        match add_content_zip(&p) {
            Ok(m) => out.push(m),
            Err(e) => log::warn!("content zip {}: {e}", p.display()),
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn resolves_like_windows() {
        let dir = std::env::temp_dir().join(format!("mods-paths-{}", std::process::id()));
        std::fs::create_dir_all(dir.join("Texture").join("Bahn\u{fc}bergang")).unwrap();
        std::fs::write(
            dir.join("Texture").join("Bahn\u{fc}bergang").join("anz-oben.jpg"),
            b"x",
        )
            .unwrap();
        let p = resolve_path(
            &dir.join("model"),
            "..\\TEXTURE.\\BAHN\u{dc}BERGANG/Anz-Oben.JPG. ",
        );
        assert!(vfs::is_file(&p), "{}", p.display());
        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn never_leaves_the_roots() {
        let dir = std::env::temp_dir().join(format!("mods-escape-{}", std::process::id()));
        let root = dir.join("root");
        std::fs::create_dir_all(root.join("Vehicles")).unwrap();
        std::fs::write(dir.join("secret.txt"), b"x").unwrap();
        add_content_root(root.clone());
        let p = resolve_path(&root.join("Vehicles"), "..\\..\\secret.txt");
        assert!(p.starts_with(&root), "{}", p.display());
        assert!(!vfs::exists(&resolve_path(&root, "C:\\secret.txt")));
        remove_content_root(&root);
        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn first_root_wins() {
        let dir = std::env::temp_dir().join(format!("mods-order-{}", std::process::id()));
        let (a, b, legacy) = (dir.join("a"), dir.join("b"), dir.join("legacy"));
        for r in [&a, &b, &legacy] {
            std::fs::create_dir_all(r.join("Vehicles").join("Bus")).unwrap();
        }
        std::fs::write(a.join("Vehicles/Bus/x.txt"), b"a").unwrap();
        std::fs::write(b.join("Vehicles/Bus/x.txt"), b"b").unwrap();
        std::fs::write(legacy.join("Vehicles/Bus/x.txt"), b"legacy").unwrap();
        std::fs::write(b.join("Vehicles/Bus/only_b.txt"), b"b").unwrap();
        add_content_root(a.clone());
        add_content_root(b.clone());
        add_content_root(legacy.clone());
        set_root_scope(legacy.clone(), Vec::new());
        let base = legacy.join("Vehicles").join("Bus");
        assert_eq!(vfs::read(&resolve_path(&base, "x.txt")).unwrap(), b"legacy");
        assert_eq!(vfs::read(&resolve_path(&legacy, "Vehicles/Bus/x.txt")).unwrap(), b"a");
        assert!(!vfs::exists(&resolve_path(&base, "only_b.txt")));
        for r in [&a, &b, &legacy] {
            clear_root_scope(r);
            remove_content_root(r);
        }
        std::fs::remove_dir_all(&dir).ok();
    }
}
