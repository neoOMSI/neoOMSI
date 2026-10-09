use super::*;

/// Where textures are looked up for a given content directory.
/// How far a road surface may ride above the ground and still have the ground cut away
/// under it. Anything higher is a bridge or an embankment, where cutting would open a hole.
/// `OMSI_HEIGHTPROFILE_GROUND=1`: the wheels stand on the splines' `[heightprofile]`s as
/// they did before, instead of on the drawn splines as Omsi.exe stands them (A/B runs).
/// `OMSI_CHECK_ROADS`: road points under the ground, and where.
pub(super) static OVER_ROAD: std::sync::atomic::AtomicUsize = std::sync::atomic::AtomicUsize::new(0);

pub(super) static OVER_ROAD_AT: std::sync::Mutex<Vec<(f64, f64, f32, f32)>> =
    std::sync::Mutex::new(Vec::new());

pub(super) fn heightprofile_ground() -> bool {
    static ON: std::sync::OnceLock<bool> = std::sync::OnceLock::new();
    *ON.get_or_init(|| ::legacy_config::env::var_os("OMSI_HEIGHTPROFILE_GROUND").is_some())
}

pub(super) fn surface_flush() -> f32 {
    // (asked a few hundred times a frame)
    static FLUSH: std::sync::OnceLock<f32> = std::sync::OnceLock::new();
    *FLUSH.get_or_init(|| {
        ::legacy_config::env::var("OMSI_SURFACE_FLUSH")
            .ok()
            .and_then(|v| v.parse().ok())
            .unwrap_or(0.12)
    })
}

/// Whether this session's weather lies as snow (`[snow]` in the `.owt`), set by the app
/// when it reads the weather and asked while the vehicles go onto the GPU.
pub static SNOW_WEATHER: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);

pub(super) fn snowing() -> bool {
    SNOW_WEATHER.load(std::sync::atomic::Ordering::Relaxed)
}

/// Whether a texture name resolves to the season's own copy of it (`texture\WinterSnow\…`):
/// a vehicle that brings its own winter picture keeps it.
pub(super) fn seasonal_texture(name: &str, dirs: &[&Path]) -> bool {
    match (
        ::texture::season_folder(),
        ::texture::find_texture(name, dirs),
    ) {
        (Some(season), Some(p)) => p.components().any(|c| {
            c.as_os_str()
                .to_string_lossy()
                .eq_ignore_ascii_case(&season)
        }),
        _ => false,
    }
}

pub fn texture_dirs(root: &Path, content_dir: &Path) -> Vec<PathBuf> {
    // (found whatever its case: `Texture` of a parked car's folder on Linux, whose file
    // system tells `texture` from `Texture`, left the car white)
    let mut dirs = vec![
        ::legacy_config::resolve_path(content_dir, "texture"),
        content_dir.to_path_buf(),
    ];
    // vehicle folders keep the model in `model\` and the textures in `texture\` next to it
    if let Some(parent) = content_dir.parent() {
        dirs.push(::legacy_config::resolve_path(parent, "texture"));
    }
    dirs.push(::legacy_config::resolve_path(root, "Texture"));
    dirs
}

/// Load a texture's adjacent OMSI surface height map. Both the filename as authored by the
/// spline/material and the actual texture selected by OMSI's resolver can have a `.surf`
/// sidecar (for example `road.bmp.surf` beside a selected `road.dds`).
pub(super) fn surface_height_map(name: &str, dirs: &[PathBuf]) -> Option<Arc<HeightMap>> {
    if ::legacy_config::env::var_os("OMSI_NO_SURF").is_some() || name.trim().is_empty() {
        return None;
    }
    static MAPS: std::sync::OnceLock<Mutex<HashMap<PathBuf, (u64, Option<Arc<HeightMap>>)>>> =
        std::sync::OnceLock::new();
    let generation = ::legacy_config::content_generation();
    let cache = MAPS.get_or_init(|| Mutex::new(HashMap::new()));
    let dirs_ref: Vec<&Path> = dirs.iter().map(PathBuf::as_path).collect();
    let mut candidates = Vec::new();
    if let Some(texture) = ::texture::find_texture(name, &dirs_ref) {
        candidates.push(PathBuf::from(format!("{}.surf", texture.display())));
    }
    let requested = format!("{}.surf", name.trim().replace('\\', "/"));
    for dir in dirs {
        let path = ::legacy_config::resolve_path(dir, &requested);
        if !candidates.contains(&path) {
            candidates.push(path);
        }
    }
    for path in candidates {
        if let Some((seen, map)) = cache.lock().get(&path).cloned() {
            if map.is_some() || seen == generation {
                if map.is_some() {
                    return map;
                }
                continue;
            }
        }
        let map = if ::legacy_config::vfs::is_file(&path) {
            ::texture::decode_file(&path)
                .ok()
                .and_then(|image| HeightMap::from_rgba(image.width, image.height, &image.rgba))
                .map(Arc::new)
        } else {
            None
        };
        cache.lock().insert(path, (generation, map.clone()));
        if map.is_some() {
            return map;
        }
    }
    None
}

/// Surface maps indexed exactly like a mesh's material slots; absent sidecars leave a slot
/// flat and allocate no per-face data in the drive grid.
pub(super) fn surface_faces<'a>(
    names: impl IntoIterator<Item = &'a str>,
    dirs: &[PathBuf],
) -> Option<Arc<SurfFaces>> {
    if ::legacy_config::env::var_os("OMSI_NO_SURF").is_some() {
        return None;
    }
    let slots: Vec<Option<Arc<HeightMap>>> = names
        .into_iter()
        .map(|name| surface_height_map(name, dirs))
        .collect();
    slots
        .iter()
        .any(Option::is_some)
        .then(|| Arc::new(SurfFaces { slots }))
}
