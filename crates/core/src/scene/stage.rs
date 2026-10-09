use super::*;

impl World {
    /// Drop the read tiles `keys` from the staging cache (they are read again when asked).
    pub fn forget_staged(&self, keys: &[(i32, i32)]) {
        let mut st = self.staged.lock();
        for k in keys {
            st.remove(k);
        }
    }

    /// Drop every read tile from the staging cache.
    pub fn forget_all_staged(&self) {
        self.staged.lock().clear();
    }

    /// Everything a batch of tiles needs before the GPU. Nothing here needs the renderer,
    /// so the window runs it on a worker thread.
    ///
    /// A tile's final ground depends on the roads and crossings around it, and its road cut
    /// on the surfaces of its neighbours as they finally stand, so a tile cannot be finished
    /// from its own file. Every tile is therefore staged first (read and typed once, see
    /// [`StagedTile`], kept in a cache), and a tile is only placed once everything its
    /// neighbourhood depends on is staged. What a tile works with is fixed by the map
    /// ([`TileLayout::sources_of`]), not by what else happens to be loaded, so a tile
    /// streamed in gets exactly the ground and the cut a whole-map load gives it.
    pub fn prepare_tiles(&self, tiles: &[(i32, i32, PathBuf)]) -> (Vec<Prepared>, LoadStats) {
        let profile = ::legacy_config::env::var_os("OMSI_PROFILE").is_some();
        let t0 = std::time::Instant::now();
        let index = self.index();
        let layout = self.layout();
        let t1 = std::time::Instant::now();
        let keys: Vec<(i32, i32)> = tiles.iter().map(|t| (t.0, t.1)).collect();
        // the batch needs the sources of every tile next to it (their placed surfaces go
        // into its cut)
        let mut wanted: Vec<(i32, i32)> = Vec::new();
        for k in &keys {
            for q in layout.ring(*k) {
                wanted.extend(layout.sources_of(q).iter().copied());
            }
        }
        wanted.sort();
        wanted.dedup();
        let missing: Vec<(i32, i32)> = {
            let cache = self.staged.lock();
            wanted
                .iter()
                .copied()
                // a tile placed before gave its meshes to the GPU and is read again
                .filter(|k| {
                    cache
                        .get(k)
                        .map(|s| keys.contains(k) && s.meshes.lock().is_none())
                        .unwrap_or(true)
                })
                .collect()
        };
        let fresh: Vec<((i32, i32), Arc<StagedTile>)> = missing
            .par_iter()
            .filter_map(|k| {
                Some((
                    *k,
                    Arc::new(self.stage_tile(k.0, k.1, layout.paths.get(k)?, &index)),
                ))
            })
            .collect();
        // what this batch works with, held here: the cache may drop entries meanwhile
        let staged: HashMap<(i32, i32), Arc<StagedTile>> = {
            let mut cache = self.staged.lock();
            for (k, st) in fresh {
                cache.insert(k, st);
            }
            wanted
                .iter()
                .filter_map(|k| cache.get(k).map(|s| (*k, s.clone())))
                .collect()
        };
        let t2 = std::time::Instant::now();
        let stats = Mutex::new(LoadStats {
            tiles: tiles.len(),
            ..Default::default()
        });
        let mut prepared: Vec<Prepared> = keys
            .par_iter()
            .filter_map(|k| self.place_tile(*k, &staged, &layout, &stats))
            .collect();
        let t3 = std::time::Instant::now();
        self.cut_terrain(&mut prepared, &staged, &layout);
        if profile {
            log::info!(
                "prepare {} tiles ({} staged, {} read now): index {:.2} s, read {:.2} s, place {:.2} s, cut + textures {:.2} s",
                tiles.len(),
                staged.len(),
                missing.len(),
                (t1 - t0).as_secs_f64(),
                (t2 - t1).as_secs_f64(),
                (t3 - t2).as_secs_f64(),
                t3.elapsed().as_secs_f64()
            );
        }
        let stats = stats.into_inner();
        (prepared, stats)
    }

    /// Forget staged tiles no loaded (or requested) tile depends on any more.
    pub fn trim_staged(&self, requested: &hashbrown::HashSet<(i32, i32)>) {
        let layout = self.layout();
        let mut keep: hashbrown::HashSet<(i32, i32)> = hashbrown::HashSet::new();
        let loaded: Vec<(i32, i32)> = self.tile_state.lock().keys().copied().collect();
        for k in loaded.iter().chain(requested.iter()) {
            for q in layout.ring(*k) {
                keep.extend(layout.sources_of(q).iter().copied());
            }
        }
        self.staged.lock().retain(|k, _| keep.contains(k));
    }

    /// The staged tiles `key` depends on, from `staged`.
    pub(super) fn sources(
        layout: &TileLayout,
        staged: &HashMap<(i32, i32), Arc<StagedTile>>,
        key: (i32, i32),
    ) -> HashMap<(i32, i32), Arc<StagedTile>> {
        layout
            .sources_of(key)
            .iter()
            .filter_map(|k| staged.get(k).map(|s| (*k, s.clone())))
            .collect()
    }

    /// Read one tile: its records with the chrono patches applied, the terrain as the file has
    /// it, the water, the spline meshes and lanes, and every object with its type.
    pub(super) fn stage_tile(&self, tx: i32, ty: i32, path: &Path, index: &MapIndex) -> StagedTile {
        let origin2 = DVec2::new(tx as f64 * tile_size(), ty as f64 * tile_size());
        let origin = DVec3::new(origin2.x, origin2.y, 0.0);
        let terrain_path = tile_companion(path, ".terrain");
        let edited = self.terrain_edits.lock().get(&(tx, ty)).cloned();
        let base_terrain = edited
            .unwrap_or_else(|| Terrain::load(&terrain_path).unwrap_or_else(|_| Terrain::flat()));
        let counts = Mutex::new(LoadStats::default());
        let mut out = StagedTile {
            tx,
            ty,
            origin,
            path: path.to_path_buf(),
            base_terrain,
            align: Vec::new(),
            hole_outlines: Vec::new(),
            water: None,
            splines: Vec::new(),
            meshes: Mutex::new(Some(Vec::new())),
            drive: Vec::new(),
            lanes: Mutex::new(Vec::new()),
            street_points: Vec::new(),
            objects: Vec::new(),
            anchors: Vec::new(),
            counts: LoadStats::default(),
            resolved: std::sync::OnceLock::new(),
        };
        let Some(tile) = crate::tiles::read_tile(path, &self.chrono_dirs.read(), self.map_detail)
        else {
            return out;
        };
        // a tile with water carries one surface with a height at each corner. As in Omsi.exe
        // (TMapKachel.loadMapFile 0x792188) only a `[water]` tile has it: the editor never
        // deletes the `.water` file of a tile whose water was removed. The corners start at
        // -5 m (TFileWater 0x7ab6f0) and take the file's heights only when its count is 1
        // (0x7ab760).
        out.water = tile.has_water.then(|| {
            match ::legacy_config::vfs::read(&PathBuf::from(format!("{}.water", path.display())))
                .ok()
                .map(|b| ::map::terrain::Water::parse(&b))
            {
                Some(w) if w.count == 1 && w.values.len() >= 4 => {
                    [w.values[0], w.values[1], w.values[2], w.values[3]]
                }
                _ => [-5.0; 4],
            }
        });
        let debug_splines = ::legacy_config::env::var_os("OMSI_DEBUG_SPLINES").is_some();
        let mut lanes: Vec<Lane> = Vec::new();
        let mut meshes: Vec<Arc<MeshData>> = Vec::new();
        for s in tile
            .splines
            .iter()
            .filter(|s| !s.deleted && !s.file.trim().is_empty())
        {
            let Some(st) = self.spline_type(&s.file) else {
                self.note_missing(&s.file, "spline", tx, ty, s.id);
                continue;
            };
            let curve = SplineCurve::from_map(s, origin2);
            if ::legacy_config::env::var_os("OMSI_CHECK_SPLINES").is_some() {
                SPLINE_ENDS.lock().insert(
                    s.id,
                    (
                        curve.point_at(0.0),
                        curve.end_point(),
                        s.prev_id,
                        s.next_id,
                        s.file.clone(),
                    ),
                );
            }
            // editor-only splines (invisible streets, flight paths) still carry lanes
            let mut new_lanes = spline_lanes(&st.def, s, &curve, (tx, ty));
            for l in new_lanes.iter_mut() {
                l.invisible = st.def.only_editor;
            }
            lanes.extend(new_lanes);
            if st.def.only_editor {
                if debug_splines {
                    log::info!(
                        "tile {tx},{ty} spline {} {} is editor-only (lanes only, no mesh) at ({:.1},{:.1},{:.2})",
                        s.id,
                        s.file,
                        s.pos[0],
                        s.pos[1],
                        s.pos[2]
                    );
                }
                continue;
            }
            // What the wheels stand on is the spline's drawn mesh, not its `[heightprofile]`:
            // Omsi.exe's ground query (0x7a0814) casts a ray from 3 m over the point down
            // into each spline segment of the tile (0x5b2d94 -> 0x7c40c8, D3DXIntersect on
            // the segment's mesh +0xa0), and that mesh is the one TSplineSegment.Generate
            // (0x5b1e14) builds from the `[profile]`/`[profilepnt]` lists (+0xc) for drawing.
            // The height profile (+0x20) is read by the editor's "is the point on this
            // spline" test alone (0x5b2b1c). Taken as the ground, a height profile wider than
            // the drawn road reached under the bus from the road beside it, one lower than
            // the asphalt sank the wheels into it, and a road without one had no ground at
            // all. `OMSI_HEIGHTPROFILE_GROUND=1` goes back to the height profiles (A/B).
            if heightprofile_ground() {
                let hp =
                    ::geometry::build_height_profile_mesh(&st.def, &curve, s.mirror, origin);
                if !hp.is_empty() {
                    let b = mesh_bounds(&hp, &Mat4::IDENTITY, origin);
                    out.drive.push(StagedDrive::HeightProfiles(hp, b));
                }
            }
            let mesh = build_spline_mesh(&st.def, &curve, s.mirror, origin);
            // OMSI_CHECK_SPIKES: a face standing taller than the profile, the gradient and
            // the cant allow (a spike out of the road)
            if ::legacy_config::env::var_os("OMSI_CHECK_SPIKES").is_some() && !mesh.is_empty() {
                let (zlo, zhi) = st
                    .def
                    .profiles
                    .iter()
                    .flat_map(|p| p.points.iter().map(|q| q.z))
                    .fold((f32::MAX, f32::MIN), |(a, b), z| (a.min(z), b.max(z)));
                let n = ::geometry::spline_station_count(&st.def, &curve).max(1);
                let step = curve.length / n as f64;
                let slope = curve.grad_start.abs().max(curve.grad_end.abs()) / 100.0;
                let cant = curve.cant_start.abs().max(curve.cant_end.abs()) / 100.0
                    * 2.0
                    * ::geometry::half_cant_width(&st.def).min(20.0);
                let allow = (zhi - zlo) as f64 + slope * step * 2.0 + cant + 0.5;
                let worst = mesh
                    .indices
                    .chunks_exact(3)
                    .map(|t| {
                        let z = [t[0], t[1], t[2]].map(|i| mesh.positions[i as usize].z);
                        (z.iter().cloned().fold(f32::MIN, f32::max)
                            - z.iter().cloned().fold(f32::MAX, f32::min))
                            as f64
                    })
                    .fold(0.0f64, f64::max);
                if worst > allow {
                    log::info!(
                        "spike: tile {tx},{ty} spline {} {} face {worst:.1} m tall (allowed {allow:.1}) len {:.1} r {:.1} grad {:.2}/{:.2} h {:?} cant {:.1}/{:.1} skew {:.2}/{:.2} at ({:.1}, {:.1}, {:.1})",
                        s.id,
                        s.file,
                        s.length,
                        s.radius,
                        s.grad_start,
                        s.grad_end,
                        s.delta_h,
                        s.cant_start,
                        s.cant_end,
                        s.skew_start,
                        s.skew_end,
                        origin2.x + s.pos[0],
                        origin2.y + s.pos[1],
                        s.pos[2]
                    );
                }
            }
            if debug_splines {
                let (lo, hi) = mesh.positions.iter().fold(
                    (glam::Vec3::splat(f32::MAX), glam::Vec3::splat(f32::MIN)),
                    |(lo, hi), v| (lo.min(*v), hi.max(*v)),
                );
                let tz = out.base_terrain.sample(
                    s.pos[0].clamp(0.0, tile_size()) as f32,
                    s.pos[1].clamp(0.0, tile_size()) as f32,
                );
                log::info!(
                    "tile {tx},{ty} spline {} {} h={} len={:.1} r={:.1} start=({:.1},{:.1},{:.2}) terrain_z={:.2} verts={} tris={} profiles={} tex={} local z {:.2}..{:.2}",
                    s.id,
                    s.file,
                    s.is_h,
                    s.length,
                    s.radius,
                    s.pos[0],
                    s.pos[1],
                    s.pos[2],
                    tz,
                    mesh.positions.len(),
                    mesh.indices.len() / 3,
                    st.def.profiles.len(),
                    st.def.textures.len(),
                    lo.z,
                    hi.z
                );
            }
            if !mesh.is_empty() {
                // `[spline_terrain_align]` / `_2 <m>`: the editor pulled the ground onto
                // this road, and OMSI does it again on every load.
                let aligned = s.terrain_align_flag || s.terrain_align.is_some();
                // (a road the ground was pulled onto lies on it whatever the heights say; a
                // wire - under a metre across - would only flicker in the shadow map)
                let (lo_x, hi_x) = st
                    .def
                    .profiles
                    .iter()
                    .flat_map(|p| p.points.iter().map(|q| q.x))
                    .fold((f32::MAX, f32::MIN), |(a, b), x| (a.min(x), b.max(x)));
                let casts_shadow = !aligned && hi_x - lo_x >= 1.0 && {
                    let step = mesh.positions.len().div_ceil(64).max(1);
                    mesh.positions.iter().step_by(step).all(|p| {
                        p.z - out.base_terrain.sample(
                            p.x.clamp(0.0, tile_size() as f32),
                            p.y.clamp(0.0, tile_size() as f32),
                        ) > SPLINE_SHADOW_CLEARANCE
                    })
                };
                if casts_shadow && debug_splines {
                    log::info!(
                        "tile {tx},{ty} spline {} {} stands clear of the ground: it casts a sun shadow",
                        s.id,
                        s.file
                    );
                }
                if aligned {
                    out.align
                        .push((out.splines.len(), s.terrain_align.unwrap_or(1.0) as f32));
                    // Omsi.exe cuts the ground out under such a spline (the flag, or the
                    // `_2` number, goes to the segment, 0x79b95e -> +0x205, and Generate
                    // makes the outline from it; the terrain takes it with the `[terrainhole]`
                    // meshes, "Terrain hole cutting: Spline")
                    let mode = s
                        .terrain_align
                        .map(|v| v.clamp(0.0, 255.0) as u8)
                        .unwrap_or(1);
                    if ::legacy_config::env::var_os("OMSI_LIST_ALIGNED").is_some() {
                        let p = curve.point_at(curve.length * 0.5);
                        log::info!(
                            "aligned spline {} {} mode {mode} mid ({:.1}, {:.1}, {:.1}) heading {:.0}",
                            s.id,
                            s.file,
                            p.x,
                            p.y,
                            p.z,
                            curve.heading_at(curve.length * 0.5)
                        );
                    }
                    for ring in ::geometry::spline_hole_outlines(&st.def, &curve, s.mirror, mode)
                    {
                        if ::geometry::outline_crosses_itself(&ring) {
                            if debug_splines {
                                log::info!(
                                    "tile {tx},{ty} spline {} {}: its hole outline crosses itself, no hole (as in Omsi.exe)",
                                    s.id,
                                    s.file
                                );
                            }
                            continue;
                        }
                        out.hole_outlines.push(ring);
                    }
                }
                let bounds = mesh_bounds(&mesh, &Mat4::IDENTITY, origin);
                // the rasters only need the shape; the whole mesh waits for the upload
                let shape = MeshData {
                    positions: mesh.positions.clone(),
                    uvs: st
                        .surface_maps
                        .as_ref()
                        .map(|_| mesh.uvs.clone())
                        .unwrap_or_default(),
                    ranges: st
                        .surface_maps
                        .as_ref()
                        .map(|_| mesh.ranges.clone())
                        .unwrap_or_default(),
                    indices: mesh.indices.clone(),
                    ..Default::default()
                };
                // (every spline the game draws: Omsi.exe asks them all, roads or not)
                if !heightprofile_ground() {
                    out.drive.push(StagedDrive::DrawnMesh {
                        mesh: shape.clone(),
                        bounds,
                        surface_maps: st.surface_maps.clone(),
                    });
                }
                let drivable = st.def.paths.iter().any(|pd| pd.kind == 0 || pd.kind == 1);
                let overlay = !st.def.profiles.is_empty()
                    && st
                    .def
                    .profiles
                    .iter()
                    .all(|p| st.def.textures.get(p.texture).is_some_and(|t| t.alpha == 2));
                // A spline's visible profile is not necessarily a ground surface: power
                // cables and overhead trim have horizontal quads, and in the raster they cut
                // the ground up to their own height and stood in for the surface there. Only
                // one whose every profile hangs at least `SPLINE_OVERHEAD` over the spline's
                // line stays out; a wall, an embankment or a waterside without paths or
                // height profiles (Moges' `embankment.sli`) is ground all the same.
                let cuts_terrain =
                    !overhead_only(&st.def) || !st.def.height_profiles.is_empty() || drivable;
                out.splines.push(StagedSpline {
                    shape,
                    ty: st,
                    bounds,
                    drivable,
                    overlay,
                    cuts_terrain,
                    casts_shadow,
                    sort_origin: curve.point_at(0.0),
                });
                meshes.push(Arc::new(mesh));
            }
        }
        // (every 2 m along the streets)
        out.street_points = lanes
            .iter()
            .filter(|l| l.kind == LaneKind::Street)
            .flat_map(|l| {
                let n = (l.length() / 2.0).ceil().max(1.0) as usize;
                (0..=n).map(move |k| l.at(l.length() * k as f32 / n as f32).0)
            })
            .collect();
        *out.lanes.lock() = lanes;
        *out.meshes.lock() = Some(meshes);
        let only_object = ::legacy_config::env::var("OMSI_ONLY_OBJECT")
            .ok()
            .map(|f| f.to_ascii_lowercase());
        let skip_object = ::legacy_config::env::var("OMSI_SKIP_OBJECT")
            .ok()
            .map(|f| f.to_ascii_lowercase());
        let wanted = |file: &str| {
            let f = file.to_ascii_lowercase();
            only_object
                .as_ref()
                .map(|o| f.contains(o.as_str()))
                .unwrap_or(true)
                && !skip_object
                .as_ref()
                .map(|s| f.contains(s.as_str()))
                .unwrap_or(false)
        };
        // [object]
        for o in &tile.objects {
            if !wanted(&o.file) {
                continue;
            }
            let Some((ot, parked)) = self.placed_type(&o.file, &o.extra, o.id, tx, ty, &counts)
            else {
                continue;
            };
            // Objects with traffic paths (crossings, switches, road pieces) are stored with
            // absolute heights like the splines themselves; so are [absheight] ones.
            let absolute = ot.sco.absolute_height();
            let (x, y) = (origin2.x + o.pos[0], origin2.y + o.pos[1]);
            let place = if absolute {
                // On a `[worldcoordinates]` map the tile's splines are stretched onto the
                // grid with it, lengths included (`fit_to_world_grid`); a crossing has to
                // stretch as well, or the roads ending at its edges stop short of it - 1.6 cm
                // at a 27 m arm in Spandau, a line of sky across the road where the ground
                // is cut out underneath.
                let (kx, ky) = ::map::world_tile_scale(ty);
                Placement::Pose(Pose {
                    pos: DVec3::new(x, y, o.pos[2]),
                    rot: Mat4::from_scale(glam::Vec3::new(kx as f32, ky as f32, 1.0))
                        * object_rotation(::geometry::map_rotation(o.rot)),
                })
            } else {
                Placement::Ground {
                    x,
                    y,
                    z: o.pos[2],
                    rot: ::geometry::map_rotation(o.rot),
                }
            };
            out.objects.push(StagedObject {
                ot,
                id: o.id,
                place,
                rules: o.rules.clone(),
                extra: o.extra.clone(),
                lamp_parent: o.var_parent,
                parked,
                map_object: true,
                instance: 0,
                key: o.id,
            });
        }
        // [attachObj]
        for o in &tile.attach_objects {
            if !wanted(&o.file) {
                continue;
            }
            let Some((ot, parked)) = self.placed_type(&o.file, &o.extra, o.id, tx, ty, &counts)
            else {
                continue;
            };
            let Some(parent) = o.parent_id else { continue };
            out.objects.push(StagedObject {
                ot,
                id: o.id,
                place: Placement::Attached {
                    parent,
                    index: o.attach_index,
                    rot: ::geometry::map_rotation(o.rot),
                },
                rules: o.rules.clone(),
                extra: o.extra.clone(),
                lamp_parent: o.var_parent.or(Some(parent)),
                parked,
                map_object: false,
                instance: 0,
                key: o.id,
            });
        }
        // [splineAttachement] rows and their repeaters
        let mut rows = 0usize;
        for a in &tile.spline_attachments {
            if !wanted(&a.file) {
                continue;
            }
            let objs = crate::tiles::tile_row_objects(a, &tile.splines, origin2, Some(index));
            if objs.is_empty() || a.file.trim().is_empty() {
                continue;
            }
            // without its own type the whole row is missing; otherwise each object is on
            // its own (a car park row leaves some spaces empty)
            let Some(row_type) = self.object_type(&a.file) else {
                self.note_missing(&a.file, "scenery object", tx, ty, a.id);
                counts.lock().failed_objects += 1;
                continue;
            };
            rows += 1;
            let first = objs.iter().map(|o| o.1.index).min().unwrap_or(0);
            for (_, ro) in objs {
                if a.repeater.is_none() && ro.index == first {
                    out.anchors.push((a.id, ro.pose, row_type.clone()));
                }
                // every car park of a row draws its own car
                let key = a.id.wrapping_mul(1_000_003).wrapping_add(ro.index as i64);
                let Some((ot, parked)) =
                    self.placed_type(&a.file, &a.strings, key, tx, ty, &counts)
                else {
                    continue;
                };
                let key = row_object_key(tx, ty, a.id, ro.index);
                out.objects.push(StagedObject {
                    ot,
                    id: a.id,
                    place: Placement::Pose(ro.pose),
                    rules: a.rules.clone(),
                    extra: a.strings.clone(),
                    lamp_parent: a.var_parent,
                    parked,
                    map_object: false,
                    instance: ro.index,
                    key,
                });
            }
        }
        let mut counts = counts.into_inner();
        counts.rows = rows;
        counts.attached = tile.attach_objects.len();
        out.counts = counts;
        out
    }

    /// Note a type that is not in this installation, once per file.
    pub(super) fn note_missing(&self, file: &str, what: &'static str, tx: i32, ty: i32, id: i64) {
        let key = file.trim().to_ascii_lowercase().replace('\\', "/");
        if self.missing.lock().insert(key.clone(), what).is_none() {
            // the folder under Sceneryobjects/Splines names the add-on it comes with
            let addon = key.split('/').nth(1).unwrap_or("");
            log::warn!(
                "{what} not found: {file} (add-on folder \"{addon}\"; first used by id {id} in tile {tx},{ty}) - left out"
            );
        }
    }

    /// What the map uses and this installation lacks: the objects, splines and parked
    /// cars (file, what) and the textures of the tiles loaded so far.
    pub fn missing_content(&self) -> (Vec<(String, &'static str)>, Vec<String>) {
        let mut files: Vec<(String, &'static str)> = self
            .missing
            .lock()
            .iter()
            .map(|(f, w)| (f.clone(), *w))
            .collect();
        files.sort();
        let mut tex: Vec<String> = self.gpu.lock().misses.iter().cloned().collect();
        tex.sort();
        (files, tex)
    }

    /// The type an object record puts on the map: a parking space gets a random car of the
    /// map's parklist (a quarter of them stay empty, as in the original). None when nothing
    /// is to be placed; a missing type is counted and logged once.
    pub(super) fn placed_type(
        &self,
        file: &str,
        captions: &[String],
        key: i64,
        tx: i32,
        ty: i32,
        stats: &Mutex<LoadStats>,
    ) -> Option<(Arc<ObjectType>, bool)> {
        if file.trim().is_empty() {
            // a damaged record names nothing
            return None;
        }
        let Some(ot) = self.object_type(file) else {
            self.note_missing(file, "scenery object", tx, ty, key);
            stats.lock().failed_objects += 1;
            if ::legacy_config::env::var_os("OMSI_DEBUG_MISSING").is_some() {
                log::info!("object not placed: {file} (tile {tx},{ty}, id {key})");
            }
            return None;
        };
        if !ot.sco.is_car_park {
            return Some((ot, false));
        }
        let list = self.parked_car_types(parklist_index(captions));
        if list.is_empty() {
            return Some((ot, false));
        }
        let h = (key as u64).wrapping_mul(0x9E37_79B9_7F4A_7C15) >> 33;
        // leave some spaces empty like the original, and all once the options' count of
        // parked cars stands
        let full = self.parked_max < 0
            || (self.parked_max > 0
            && self.parked_live.load(std::sync::atomic::Ordering::Relaxed) as i64
            >= self.parked_max);
        if h % 4 == 0 || full {
            stats.lock().empty_spaces += 1;
            return None;
        }
        let car = &list[(h as usize / 4) % list.len()];
        let Some(base) = self.object_type(car) else {
            self.note_missing(car, "parked car", tx, ty, key);
            return None;
        };
        if base.paint_scheme_count > 0 {
            return self
                .object_type_scheme(car, Some((h as usize / 64) % base.paint_scheme_count))
                .map(|t| (t, true));
        }
        Some((base, true))
    }

    /// The ground as the tile files have it at world x, y (None outside `src`).
    pub(super) fn base_ground(src: &HashMap<(i32, i32), Arc<StagedTile>>, x: f64, y: f64) -> Option<f64> {
        let k = (
            (x / tile_size()).floor() as i32,
            (y / tile_size()).floor() as i32,
        );
        let st = src.get(&k)?;
        Some(
            st.base_terrain
                .sample((x - st.origin.x) as f32, (y - st.origin.y) as f32) as f64,
        )
    }

    /// Where a staged object stands before optional terrain alignment.
    pub(super) fn provisional_pose(
        st: &StagedTile,
        o: &StagedObject,
        src: &HashMap<(i32, i32), Arc<StagedTile>>,
    ) -> Option<Pose> {
        match &o.place {
            Placement::Pose(p) => Some(*p),
            Placement::Ground { x, y, z, rot } => {
                let (lx, ly) = (
                    (x - st.origin.x).clamp(0.0, tile_size()) as f32,
                    (y - st.origin.y).clamp(0.0, tile_size()) as f32,
                );
                let base_height = Self::base_ground(src, *x, *y)
                    .unwrap_or_else(|| st.base_terrain.sample(lx, ly) as f64);
                // Omsi.exe sets every object without an absolute height (those are `Pose`s,
                // `SceneryObject::absolute_height`) on the terrain, `[surface]` ones as well
                // (TMap.RefreshObjectsKacheln 0x79e3c8 reads sco+0x194).
                Some(Pose {
                    pos: DVec3::new(*x, *y, z + base_height),
                    rot: object_rotation(*rot),
                })
            }
            Placement::Attached { .. } => None,
        }
    }

    /// A tile's final ground and final object poses (computed once per staging; `src` are
    /// the staged tiles it depends on).
    pub(super) fn resolve(
        &self,
        key: (i32, i32),
        src: &HashMap<(i32, i32), Arc<StagedTile>>,
    ) -> Option<Arc<Resolved>> {
        let st = src.get(&key)?;
        Some(
            st.resolved
                .get_or_init(|| Arc::new(self.compute_resolved(st, src)))
                .clone(),
        )
    }

    pub(super) fn compute_resolved(
        &self,
        st: &StagedTile,
        src: &HashMap<(i32, i32), Arc<StagedTile>>,
    ) -> Resolved {
        let key = (st.tx, st.ty);
        let (terrain, aligned_points, biggest, deformed) = self.final_ground(key, src);
        let ground_at = |x: f64, y: f64| -> f64 {
            let actual_key = (
                (x / tile_size()).floor() as i32,
                (y / tile_size()).floor() as i32,
            );
            if actual_key == key {
                let lx = (x - st.origin.x).clamp(0.0, tile_size()) as f32;
                let ly = (y - st.origin.y).clamp(0.0, tile_size()) as f32;
                terrain.sample(lx, ly) as f64
            } else {
                // Old maps and converted maps can keep an object in the neighbouring tile's
                // file with local coordinates past the edge. Sample the terrain actually
                // under the object instead of pinning it to this tile's border height.
                Self::base_ground(src, x, y).unwrap_or_else(|| {
                    let lx = (x - st.origin.x).clamp(0.0, tile_size()) as f32;
                    let ly = (y - st.origin.y).clamp(0.0, tile_size()) as f32;
                    terrain.sample(lx, ly) as f64
                })
            }
        };
        // poses of everything that can carry an attachment: objects by id, spline rows by
        // their first object
        let mut poses: HashMap<(i64, usize), (Pose, Arc<ObjectType>)> = HashMap::new();
        let mut final_poses: Vec<Option<Pose>> = st
            .objects
            .iter()
            .map(|o| match &o.place {
                // Omsi.exe places every object, a parking space's car as well, with the pitch
                // and bank of the map file on the terrain height at its position (0x79e3c8
                // .. 0x79e5fb: RotationX(pitch), RotationZ(bank), RotationY(heading), the
                // translation) - it is never leaned to the slope. Leaned by the terrain under
                // it, a car at the kerb of a hill street stood crooked on a road that runs
                // on a different grade from the ground beneath.
                Placement::Ground { x, y, z, rot } => Some(Pose {
                    pos: DVec3::new(*x, *y, z + ground_at(*x, *y)),
                    rot: object_rotation(*rot),
                }),
                Placement::Pose(p) => Some(*p),
                Placement::Attached { .. } => None,
            })
            .collect();
        for (o, p) in st.objects.iter().zip(&final_poses) {
            if let Some(p) = p {
                if !matches!(o.place, Placement::Pose(_)) || o.map_object {
                    poses
                        .entry((o.id, o.instance))
                        .or_insert((*p, o.ot.clone()));
                }
            }
        }
        for (id, pose, ot) in &st.anchors {
            poses.entry((*id, 0)).or_insert((*pose, ot.clone()));
        }
        // attachments hang on attachments (a whip beam on a lamp post, a traffic light on
        // the beam): resolve them round by round
        loop {
            let mut progress = false;
            for (o, fp) in st.objects.iter().zip(final_poses.iter_mut()) {
                let Placement::Attached { parent, index, rot } = &o.place else {
                    continue;
                };
                if fp.is_some() {
                    continue;
                }
                // (a spline attachment row by its first object: Omsi.exe refuses objects
                // on its later ones)
                let Some((pp, pt)) = poses.get(&(*parent, 0)) else {
                    continue;
                };
                // a point the parent does not have (its object was changed after the map
                // was made: the Ahlheim signal heads on `Arm_3f` point 10 of 1) is the
                // parent's own origin, as in the original - the object is not dropped
                let m = pt
                    .sco
                    .attachments
                    .get(*index)
                    .map(crate::tiles::attachment_matrix)
                    .unwrap_or(glam::Mat4::IDENTITY);
                if *index >= pt.sco.attachments.len() {
                    // `OMSI_NO_ATTACH_FALLBACK=1` drops them instead, for an A/B
                    if ::legacy_config::env::var_os("OMSI_NO_ATTACH_FALLBACK").is_some() {
                        continue;
                    }
                    if ::legacy_config::env::var_os("OMSI_DEBUG_MISSING").is_some() {
                        log::info!(
                            "attached object {} (id {}) on point index {index} of {parent} ({} has {}): at the parent's origin ({:.1}, {:.1}, {:.1})",
                            o.ot.sco.path.display(),
                            o.id,
                            pt.sco.path.display(),
                            pt.sco.attachments.len(),
                            pp.pos.x,
                            pp.pos.y,
                            pp.pos.z
                        );
                    }
                }
                let pose = pp.attached(&m, *rot);
                *fp = Some(pose);
                poses.entry((o.id, 0)).or_insert((pose, o.ot.clone()));
                progress = true;
            }
            if !progress {
                break;
            }
        }
        let mut unattached = 0usize;
        for (o, fp) in st.objects.iter().zip(&final_poses) {
            if let (Placement::Attached { parent, .. }, None) = (&o.place, fp) {
                unattached += 1;
                if ::legacy_config::env::var_os("OMSI_DEBUG_MISSING").is_some() {
                    log::info!(
                        "attached object {} (id {}) on {parent} not placed: the parent is not in the tile",
                        o.ot.sco.path.display(),
                        o.id
                    );
                }
            }
        }
        Resolved {
            terrain: Arc::new(terrain),
            poses: final_poses,
            unattached,
            aligned_points,
            biggest,
            deformed,
        }
    }

    /// The ground of tile `key`: the tile's `.terrain` as Omsi.exe loads it, which the objects
    /// stand on. The editor's "align the terrain to this spline" (`[spline_terrain_align]`)
    /// and a crossing's `[crossing_heightdeformation]` were applied when the map was made;
    /// `OMSI_TERRAIN_ALIGN=1` and `OMSI_CROSSING_DEFORM=1` apply them again (A/B runs).
    /// Returns the ground, the ground points aligned, the biggest move (with where it was)
    /// and whether a crossing deformed it.
    pub(super) fn final_ground(
        &self,
        key: (i32, i32),
        src: &HashMap<(i32, i32), Arc<StagedTile>>,
    ) -> (Terrain, usize, Option<(f32, f64, f64)>, bool) {
        let mut t = src[&key].base_terrain.clone();
        let (x0, y0) = (key.0 as f64 * tile_size(), key.1 as f64 * tile_size());
        let (x1, y1) = (x0 + tile_size(), y0 + tile_size());
        let n = t.samples();
        let cell = tile_size() as f32 / t.cells.max(1) as f32;
        let (mut aligned_points, mut biggest) = (0usize, None);
        // one order whatever the hash map's (the rasters keep the highest surface, but a
        // tie should not depend on it either)
        let mut order: Vec<&Arc<StagedTile>> = src.values().collect();
        order.sort_by_key(|q| (q.tx, q.ty));
        // (Omsi.exe does not move the ground at all when it loads a map: the editor's "align
        // the terrain to the spline" wrote the heights into the tile's `.terrain`, and the
        // flag left in the map only makes the spline cut its outline out of the ground -
        // see `hole_outlines`. Pulled onto the road again here, every vertex under it took
        // the height of whatever lay over it, and between those five-metre points the
        // ground's triangles cut through the camber and past the kerbs: a piece of road
        // gone under the grass, while beside it the ground stood lifted over the verge.
        // `OMSI_TERRAIN_ALIGN=1` still does it.)
        if ::legacy_config::env::var_os("OMSI_TERRAIN_ALIGN").is_some() {
            let mut ts = TileSurface::new(SURFACE_RASTER);
            let mut reach = 0.0f32;
            let mut any = false;
            for q in &order {
                for (i, r) in &q.align {
                    let Some(sp) = q.splines.get(*i) else {
                        continue;
                    };
                    let b = &sp.bounds;
                    if b[2] < x0 - 20.0 || b[0] > x1 + 20.0 || b[3] < y0 - 20.0 || b[1] > y1 + 20.0
                    {
                        continue;
                    }
                    ts.rasterize(&sp.shape, &Mat4::IDENTITY, q.origin, key.0, key.1);
                    reach = reach.max(*r);
                    any = true;
                }
            }
            if any {
                // a terrain vertex takes the road's height where the road is under it, and
                // half of the difference one cell further out, so the ground runs into the
                // verge instead of stepping
                let ring = (reach / cell).ceil().clamp(1.0, 3.0) as i32;
                let mut heights: Vec<Option<f32>> = vec![None; n * n];
                for iy in 0..n {
                    for ix in 0..n {
                        if let Some(h) = ts.sample(ix as f32 * cell, iy as f32 * cell) {
                            heights[iy * n + ix] = Some(h);
                        }
                    }
                }
                let mut out = t.heights.clone();
                for iy in 0..n {
                    for ix in 0..n {
                        let k = iy * n + ix;
                        if let Some(h) = heights[k] {
                            let d = (h - t.heights[k]).abs();
                            if biggest.map(|(b, _, _)| d > b).unwrap_or(true) {
                                biggest = Some((
                                    d,
                                    x0 + (ix as f32 * cell) as f64,
                                    y0 + (iy as f32 * cell) as f64,
                                ));
                            }
                            out[k] = h;
                            aligned_points += 1;
                            continue;
                        }
                        // the skirt: blend towards the nearest aligned vertex
                        let mut best: Option<(i32, f32)> = None;
                        for dy in -ring..=ring {
                            for dx in -ring..=ring {
                                let (jx, jy) = (ix as i32 + dx, iy as i32 + dy);
                                if jx < 0 || jy < 0 || jx >= n as i32 || jy >= n as i32 {
                                    continue;
                                }
                                if let Some(h) = heights[jy as usize * n + jx as usize] {
                                    let d = dx.abs().max(dy.abs());
                                    if best.map(|(bd, _)| d < bd).unwrap_or(true) {
                                        best = Some((d, h));
                                    }
                                }
                            }
                        }
                        if let Some((d, h)) = best {
                            let w = 1.0 - d as f32 / (ring as f32 + 1.0);
                            let v = t.heights[k] + (h - t.heights[k]) * w;
                            if (v - t.heights[k]).abs() > 0.01 {
                                aligned_points += 1;
                            }
                            out[k] = v;
                        }
                    }
                }
                t.heights = out;
            }
        }
        // (Nor does it press the ground into a crossing's `[crossing_heightdeformation]` mesh:
        // Omsi.exe reads that mesh only to warp the plate and to give its paths their heights
        // (0x7ba818, "Path deform"); the editor's terrain tools left the ground as the
        // `.terrain` has it. Pressed in here, the ground stood up to 2.3 m over a Spandau
        // pavement in front of the houses beside a junction, and everything standing on the
        // ground - every pole, sign and tree there - floated over the pavement with it (#860).
        // `OMSI_CROSSING_DEFORM=1` still does it.)
        let mut deformed = false;
        if ::legacy_config::env::var_os("OMSI_CROSSING_DEFORM").is_some() {
            let mut ds = TileSurface::new(SURFACE_RASTER);
            let mut any = false;
            for q in &order {
                for o in &q.objects {
                    let Some(d) = &o.ot.deform else { continue };
                    let Some(pose) = Self::provisional_pose(q, o, src) else {
                        continue;
                    };
                    let b = mesh_bounds(d, &pose.rot, pose.pos);
                    if b[2] < x0 || b[0] > x1 || b[3] < y0 || b[1] > y1 {
                        continue;
                    }
                    ds.rasterize(d, &pose.rot, pose.pos, key.0, key.1);
                    any = true;
                }
            }
            if any {
                for iy in 0..n {
                    for ix in 0..n {
                        if let Some(h) = ds.sample(ix as f32 * cell, iy as f32 * cell) {
                            let k = iy * n + ix;
                            deformed |= (t.heights[k] - h).abs() > 0.01;
                            t.heights[k] = h;
                        }
                    }
                }
            }
        }
        (t, aligned_points, biggest, deformed)
    }
}
