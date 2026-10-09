use super::*;

/// The whole map for the navigator (see [`World::navigation_map`]).
pub struct NavigationMap {
    pub lanes: Vec<Lane>,
    /// Asphalt footprints used to corroborate editor-only driving paths. They are evidence
    /// for roads, not streets to draw: a paved yard or median has no road centre line.
    pub road_surfaces: Vec<(Vec<DVec3>, f32)>,
    /// Every placed object's position by id (bus stops beyond the loaded tiles).
    pub places: HashMap<i64, DVec3>,
    /// Street name signs: where, the object's heading and the name on it.
    pub signs: Vec<(DVec3, f64, String)>,
}

/// Whether an asset name describes a road surface. Match whole filename tokens so objects
/// such as `StreetLight.sli` do not become roads just because their name contains "street".
pub(super) fn road_surface_name(file: &str) -> bool {
    let base = file
        .replace('\\', "/")
        .rsplit('/')
        .next()
        .unwrap_or("")
        .to_ascii_lowercase();
    let stem = base.rsplit_once('.').map(|(s, _)| s).unwrap_or(&base);
    let tokens: Vec<&str> = stem
        .split(|c: char| !c.is_ascii_alphanumeric() && c != 'ß')
        .filter(|t| !t.is_empty())
        .collect();
    let excludes = [
        "light",
        "lamp",
        "sign",
        "schild",
        "rail",
        "track",
        "gleis",
        "tram",
        "strab",
        "wire",
        "mast",
        "wall",
        "fence",
        "leitplanke",
        "gehweg",
        "side",
        "bord",
        "pavement",
        "fahrrad",
        "radweg",
        "cycle",
        "parking",
        "parkplatz",
        "gruen",
        "gras",
    ];
    if tokens
        .iter()
        .any(|t| excludes.iter().any(|x| t.contains(x)))
    {
        return false;
    }
    stem.starts_with("str_")
        || tokens.iter().any(|t| {
        [
            "str",
            "strasse",
            "straße",
            "road",
            "roads",
            "street",
            "streets",
            "fahrbahn",
            "pflaster",
            "kopfstein",
            "cobble",
        ]
            .contains(t)
            || t.starts_with("asph")
    })
}

/// The horizontal road surfaces actually drawn by a pathless spline: lateral bounds and
/// height. A texture merely listed in the file is not evidence of a road, and the origin
/// need not be in the middle of the surface. Keep medians and pavements out of its width.
pub(super) fn road_sections(file: &str, def: &::scenery::sli::Spline) -> Vec<(f32, f32, f32)> {
    let name = file.to_ascii_lowercase();
    if def.only_editor
        || [
        "gehweg", "radweg", "fahrrad", "tram", "strab", "gleis", "rail", "parking",
    ]
        .iter()
        .any(|s| name.contains(s))
        || def.paths.iter().any(|p| p.kind == 2)
    {
        return Vec::new();
    }
    let mut sections = Vec::new();
    for profile in &def.profiles {
        let road = def
            .textures
            .get(profile.texture)
            .map(|t| road_surface_name(&t.file))
            .unwrap_or_else(|| def.textures.is_empty() && road_surface_name(file));
        if !road {
            continue;
        }
        for pair in profile.points.windows(2) {
            let (a, b) = (&pair[0], &pair[1]);
            let (lo, hi) = (a.x.min(b.x), a.x.max(b.x));
            if lo.is_finite()
                && hi.is_finite()
                && a.z.is_finite()
                && b.z.is_finite()
                && hi - lo > 0.1
                && (a.z - b.z).abs() <= (hi - lo) * 0.15
            {
                sections.push((lo, hi, (a.z + b.z) * 0.5));
            }
        }
    }
    sections.sort_by(|a, b| a.0.total_cmp(&b.0));
    let mut merged: Vec<(f32, f32, f32)> = Vec::new();
    for (lo, hi, z) in sections {
        if let Some(last) = merged
            .last_mut()
            .filter(|s| lo <= s.1 + 0.2 && (z - s.2).abs() < 0.2)
        {
            last.1 = last.1.max(hi);
        } else {
            merged.push((lo, hi, z));
        }
    }
    merged.retain(|(lo, hi, _)| hi - lo >= 4.0);
    merged
}

impl World {
    /// The whole map's road network and where its objects stand, read from the tile files
    /// alone - the splines' and objects' paths, no mesh, no texture - for the navigator,
    /// which must route beyond the tiles loaded around the bus. Objects placed on the ground
    /// take the tile's terrain height; editor-only splines and objects count (some maps put
    /// all their traffic paths on invisible splines).
    pub fn navigation_map(&self) -> NavigationMap {
        use rayon::prelude::*;
        let t0 = std::time::Instant::now();
        let scos: Mutex<HashMap<String, Option<Arc<SceneryObject>>>> = Mutex::new(HashMap::new());
        let fields: Mutex<HashMap<PathBuf, Option<Arc<MeshData>>>> = Mutex::new(HashMap::new());
        let sco_of = |file: &str| -> Option<Arc<SceneryObject>> {
            let key = file.trim().to_ascii_lowercase().replace('\\', "/");
            if let Some(v) = scos.lock().get(&key) {
                return v.clone();
            }
            let v = SceneryObject::load(&::legacy_config::resolve_path(&self.root, file))
                .ok()
                .map(Arc::new);
            scos.lock().insert(key, v.clone());
            v
        };
        let tiles = self.map_tiles();
        #[allow(clippy::type_complexity)]
        let parts: Vec<(
            Vec<Lane>,
            Vec<(i64, DVec3)>,
            Vec<(DVec3, f64, String)>,
            Vec<(Vec<DVec3>, f32)>,
        )> = tiles
            .par_iter()
            .map(|(_, tx, ty, path)| {
                let (tx, ty) = (*tx, *ty);
                let mut lanes = Vec::new();
                let mut positions = Vec::new();
                let mut signs = Vec::new();
                let mut roads = Vec::new();
                let Some(tile) =
                    crate::tiles::read_tile(path, &self.chrono_dirs.read(), self.map_detail)
                else {
                    return (lanes, positions, signs, roads);
                };
                let origin2 = DVec2::new(tx as f64 * tile_size(), ty as f64 * tile_size());
                let terrain = Terrain::load(&tile_companion(&path, ".terrain"))
                    .unwrap_or_else(|_| Terrain::flat());
                for sp in tile
                    .splines
                    .iter()
                    .filter(|s| !s.deleted && !s.file.trim().is_empty())
                {
                    let Some(st) = self.spline_type(&sp.file) else {
                        continue;
                    };
                    if !st.def.paths.iter().any(|p| p.kind == 0) {
                        // Short surfaces also corroborate editor-only traffic paths at
                        // junctions; the navigator filters decorative patches for display.
                        if sp.length >= 2.0 {
                            let curve = SplineCurve::from_map(sp, origin2).with_sli(&st.def);
                            let n = ((curve.length / 4.0).ceil() as usize).clamp(1, 400);
                            let side = if sp.mirror { -1.0 } else { 1.0 };
                            for (lo, hi, z) in road_sections(&sp.file, &st.def) {
                                let offset = side * ((lo + hi) * 0.5) as f64;
                                let pts: Vec<DVec3> = (0..=n)
                                    .map(|k| {
                                        curve.offset_point(
                                            curve.length * k as f64 / n as f64,
                                            offset,
                                            z as f64,
                                        )
                                    })
                                    .collect();
                                roads.push((pts, hi - lo));
                            }
                        }
                    }
                    if st.def.paths.is_empty() {
                        continue;
                    }
                    let curve = SplineCurve::from_map(sp, origin2);
                    let mut new_lanes = spline_lanes(&st.def, sp, &curve, (tx, ty));
                    for l in new_lanes.iter_mut() {
                        l.invisible = st.def.only_editor;
                    }
                    lanes.extend(new_lanes);
                }
                for o in &tile.objects {
                    if o.file.trim().is_empty() {
                        continue;
                    }
                    let (x, y) = (origin2.x + o.pos[0], origin2.y + o.pos[1]);
                    let ground = || {
                        let (lx, ly) = (
                            (x - origin2.x).clamp(0.0, tile_size()) as f32,
                            (y - origin2.y).clamp(0.0, tile_size()) as f32,
                        );
                        terrain.sample(lx, ly) as f64
                    };
                    // street name signs carry the street's name as their text
                    if is_street_sign(&o.file) {
                        if let Some(name) = o
                            .extra
                            .first()
                            .map(|t| t.trim())
                            .filter(|t| t.chars().filter(|c| c.is_alphabetic()).count() >= 3)
                        {
                            signs.push((
                                DVec3::new(x, y, o.pos[2] + ground()),
                                o.rot[0],
                                name.to_string(),
                            ));
                        }
                    }
                    let Some(sco) = sco_of(&o.file) else {
                        positions.push((o.id, DVec3::new(x, y, o.pos[2] + ground())));
                        continue;
                    };
                    let absolute = sco.absolute_height();
                    let pos = DVec3::new(
                        x,
                        y,
                        if absolute {
                            o.pos[2]
                        } else {
                            o.pos[2] + ground()
                        },
                    );
                    positions.push((o.id, pos));
                    if !sco.paths.is_empty() {
                        // The navigator needs the same path elevations without loading
                        // an object's visual meshes or textures.
                        let field = fields
                            .lock()
                            .entry(sco.path.clone())
                            .or_insert_with(|| {
                                let sco_dir = sco.path.parent().unwrap_or(&self.root);
                                let model_path = sco
                                    .model_file
                                    .as_ref()
                                    .map(|f| ::legacy_config::resolve_path(sco_dir, f));
                                let model_dir = model_path
                                    .as_deref()
                                    .and_then(Path::parent)
                                    .unwrap_or(sco_dir);
                                load_crossing_field(&sco, model_dir).map(Arc::new)
                            })
                            .clone();
                        lanes.extend(object_lanes(
                            &sco,
                            pos,
                            object_rotation(::geometry::map_rotation(o.rot)),
                            field.as_deref(),
                            None,
                            (tx, ty),
                            o.id,
                            &o.rules,
                        ));
                    }
                }
                (lanes, positions, signs, roads)
            })
            .collect();
        let mut lanes = Vec::new();
        let mut positions = HashMap::new();
        let mut signs = Vec::new();
        let mut roads = Vec::new();
        for (l, p, s, r) in parts {
            lanes.extend(l);
            positions.extend(p);
            signs.extend(s);
            roads.extend(r);
        }
        log::info!(
            "navigation map: {} roads without a path for cars",
            roads.len()
        );
        log::info!(
            "navigation map: {} tiles, {} lanes, {} objects placed, {} street name signs, {} object types, {:.1} s",
            tiles.len(),
            lanes.len(),
            positions.len(),
            signs.len(),
            scos.lock().len(),
            t0.elapsed().as_secs_f64()
        );
        NavigationMap {
            lanes,
            road_surfaces: roads,
            places: positions,
            signs,
        }
    }
}
