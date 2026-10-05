use super::*;

/// Content paths name files case-insensitively (a mod writes `[couple_back]` however it
/// likes, and a case-insensitive file system hands the spelling back unchanged).
fn same_file(a: &Path, b: &Path) -> bool {
    let (x, y) = (
        a.canonicalize().unwrap_or_else(|_| a.to_path_buf()),
        b.canonicalize().unwrap_or_else(|_| b.to_path_buf()),
    );
    x.to_string_lossy()
        .eq_ignore_ascii_case(&y.to_string_lossy())
}

/// Of the vehicle files of one vehicle folder (every content root's copy together), the
/// ones offered for driving, loaded and in the given order: those with a `[friendlyname]`
/// - OMSI's own rule - that no other file of the folder couples behind itself. The second
/// part keeps a rear section out of the list even when a mod copied the front section's
/// name into it: a coupled part only ever comes with its front.
pub fn offered_vehicles(files: &[PathBuf]) -> Vec<(PathBuf, Vehicle)> {
    let loaded: Vec<(PathBuf, Vehicle)> = files
        .iter()
        .filter(|f| {
            f.extension()
                .map(|e| e.eq_ignore_ascii_case("bus") || e.eq_ignore_ascii_case("ovh"))
                .unwrap_or(false)
        })
        .filter_map(|f| Vehicle::load(f).ok().map(|v| (f.clone(), v)))
        .collect();
    let coupled: Vec<PathBuf> = loaded
        .iter()
        .filter_map(|(_, v)| v.couple_back_path())
        .collect();
    loaded
        .into_iter()
        .filter(|(f, v)| {
            v.is_selectable()
                && !(v.coupling_front.is_some() && coupled.iter().any(|c| same_file(c, f)))
        })
        .collect()
}

/// The vehicles that couple `rear` behind them (`[couple_back]`), looked for in its folder
/// and in the same folder of every other content root; for the rear section of an
/// articulated bus, the front section it belongs to. A chain (front → middle → rear) is
/// followed to its selectable front.
pub fn front_sections_of(rear: &Path) -> Vec<PathBuf> {
    fn direct(rear: &Path) -> Vec<(PathBuf, Vehicle)> {
        let Some(dir) = rear.parent() else {
            return Vec::new();
        };
        // the folder under every content root (archives read in place too)
        let mut dirs = vec![dir.to_path_buf()];
        for d in omsi_cfg::mirrored_dirs(dir) {
            if !dirs.contains(&d) {
                dirs.push(d);
            }
        }
        let mut out = Vec::new();
        for d in dirs {
            let mut files: Vec<PathBuf> = omsi_cfg::vfs::read_dir_paths(&d)
                .into_iter()
                .filter(|p| {
                    p.extension()
                        .map(|e| e.eq_ignore_ascii_case("bus") || e.eq_ignore_ascii_case("ovh"))
                        .unwrap_or(false)
                })
                .collect();
            files.sort();
            for f in files {
                if same_file(&f, rear) {
                    continue;
                }
                let Ok(v) = Vehicle::load(&f) else { continue };
                if v.couple_back_path()
                    .map(|p| same_file(&p, rear))
                    .unwrap_or(false)
                {
                    out.push((f, v));
                }
            }
        }
        out
    }
    let mut out = Vec::new();
    let mut todo = vec![rear.to_path_buf()];
    let mut seen: Vec<PathBuf> = Vec::new();
    while let Some(p) = todo.pop() {
        if seen.iter().any(|s| same_file(s, &p)) || seen.len() > 8 {
            continue;
        }
        seen.push(p.clone());
        for (f, v) in direct(&p) {
            if v.is_selectable() {
                out.push(f);
            } else {
                todo.push(f);
            }
        }
    }
    out
}
