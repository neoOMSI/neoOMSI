use super::*;

/// The rotation of a knocked-over post: turned about its foot by 86° (it rests on its own
/// thickness) in the direction of `push`.
/// A file that belongs to a tile (`.terrain`, …): beside the tile file, or - for a tile the
/// object editor saved as a copy in the content folder, which has only the tile itself -
/// in the same map folder under the other content roots.
pub fn tile_companion(path: &Path, ext: &str) -> PathBuf {
    // a copy in a content root before the installation's (the editor's ground, a mod's)
    if let (Some(dir), Some(name)) = (path.parent(), path.file_name()) {
        let p = ::legacy_config::resolve_path(dir, &format!("{}{}", name.to_string_lossy(), ext));
        if ::legacy_config::vfs::exists(&p) {
            return p;
        }
    }
    let direct = PathBuf::from(format!("{}{}", path.display(), ext));
    if ::legacy_config::vfs::exists(&direct) {
        return direct;
    }
    let (Some(dir), Some(name)) = (path.parent(), path.file_name()) else {
        return direct;
    };
    let name = format!("{}{}", name.to_string_lossy(), ext);
    ::legacy_config::mirrored_dirs(dir)
        .into_iter()
        .map(|d| ::legacy_config::resolve_path(&d, &name))
        .find(|p| ::legacy_config::vfs::exists(p))
        .unwrap_or(direct)
}

impl World {
    /// The clock a scenery script starts on: the simulation's, else the run's start.
    pub fn script_clock(&self) -> ::simulation::SimClock {
        self.timetable_boards
            .lock()
            .clock
            .clone()
            .unwrap_or_else(|| self.start_clock.lock().clone())
    }

    pub fn open(root: &Path, global_cfg: &Path, date: i32) -> Result<World> {
        let global = GlobalCfg::load(global_cfg)
            .with_context(|| format!("loading {}", global_cfg.display()))?;
        let map_dir = global.dir().to_path_buf();
        ::map::configure_grid(&global);
        crate::humans::LEFT_HAND.store(
            global.left_hand_traffic,
            std::sync::atomic::Ordering::Relaxed,
        );
        log::info!(
            "tile size {:.1} m ({})",
            ::map::tile_size(),
            if global.world_coordinates {
                "[worldcoordinates]"
            } else {
                "plain map"
            }
        );
        // where the sun is: the map's time zone, place and summer time
        let tz_path = ::legacy_config::resolve_path(&map_dir, "timezone.txt");
        let mut place = ::simulation::daylight::SunPlace::default();
        if let Ok(tz) = ::map::TimeZone::load(&tz_path) {
            place.timezone = tz.offset_hours as f64;
            if let Some((lat, lon)) = tz.lat_lon() {
                place.latitude = lat;
                place.longitude = lon;
            }
            place.dst = tz
                .dst
                .iter()
                .map(|d| (d.start, d.end, d.params[0], d.params[1], d.params[2]))
                .collect();
            log::info!(
                "sun: {:.3} N {:.3} E, UTC{:+}, {} summer time periods",
                place.latitude,
                place.longitude,
                place.timezone,
                place.dst.len()
            );
        }
        ::simulation::daylight::set_place(place);
        let signal_routes =
            ::legacy_config::CfgFile::read(&::legacy_config::resolve_path(&map_dir, "signalroutes.cfg"))
                .map(|f| ::map::ailists::parse_signalroutes(&f))
                .unwrap_or_default();
        if !signal_routes.is_empty() {
            log::info!(
                "signal routes: {} for {} signals",
                signal_routes.len(),
                signal_routes
                    .iter()
                    .map(|r| r.signal.0)
                    .collect::<hashbrown::HashSet<_>>()
                    .len()
            );
        }
        let chrono_dirs = ::map::active_chrono_dirs(&map_dir, date);
        // AI lists: the map's plus the chrono updates; depot entries filtered by validity date
        let mut ailists = ::map::ailists::ailists_with_chrono(&map_dir, &chrono_dirs);
        let mut ticket_pack = global.ticket_pack.clone();
        for c in &chrono_dirs {
            if let Some(cfg) = Some(::legacy_config::resolve_path(c, "Chrono.cfg"))
                .filter(|p| ::legacy_config::vfs::is_file(p))
                .and_then(|p| ::legacy_config::CfgFile::read(&p).ok())
            {
                let cc = ::map::ailists::parse_chrono_cfg(&cfg);
                if let Some(t) = cc.ticket_pack {
                    ticket_pack = t;
                }
            }
        }
        for g in ailists.groups.iter_mut() {
            for tg in g.typgroups.iter_mut() {
                tg.entries
                    .retain(|e| ::map::typgroup_entry_valid(e, date));
            }
        }
        if !chrono_dirs.is_empty() {
            log::info!(
                "chrono: {} folders active on {date}: {:?}",
                chrono_dirs.len(),
                chrono_dirs
                    .iter()
                    .map(|d| d.file_name().unwrap().to_string_lossy().into_owned())
                    .collect::<Vec<_>>()
            );
        }
        Ok(World {
            root: root.to_path_buf(),
            global,
            map_dir,
            map_detail: match ::config::get_int("graphics", "map_detail").unwrap_or(-1) {
                d if d >= 0 => d as u8,
                _ => ::content::options::Options::load(&root.join("options.cfg"))
                    .map(|o| o.i32("maxcomplexity_map", 2).clamp(0, 255) as u8)
                    .unwrap_or(2),
            },
            parklist: Mutex::new(HashMap::new()),
            mirror_textures: Mutex::new(Vec::new()),
            chrono_dirs: parking_lot::RwLock::new(chrono_dirs),
            ailists,
            date,
            ticket_pack,
            object_types: Mutex::new(HashMap::new()),
            spline_types: Mutex::new(HashMap::new()),
            textures: Arc::new(TextureCache::new()),
            object_positions: Mutex::new(HashMap::new()),
            object_dups: Mutex::new(HashMap::new()),
            terrains: Arc::new(RwLock::new(HashMap::new())),
            surfaces: Arc::new(RwLock::new(HashMap::new())),
            vehicle_gpu: Mutex::new(HashMap::new()),
            vehicle_textures: Default::default(),
            vehicle_meshes: Default::default(),
            vehicle_ready: Default::default(),
            upgrades_pending: Default::default(),
            upgrades_done: Default::default(),
            freetex_upgrades: Default::default(),
            texture_limit: Default::default(),
            budget_checked: Default::default(),
            lanes: Mutex::new(Vec::new()),
            lane_tiles: Mutex::new(Vec::new()),
            traffic_lights: Mutex::new(Vec::new()),
            controller_of_object: Mutex::new(HashMap::new()),
            light_objects: Mutex::new(Vec::new()),
            scripted: Mutex::new(Vec::new()),
            collision: Mutex::new(Default::default()),
            light_occluders: Mutex::new(Default::default()),
            poles: Mutex::new(HashMap::new()),
            fallen_poles: Mutex::new(HashMap::new()),
            parked_objects: Mutex::new(HashMap::new()),
            departed: Mutex::new(std::collections::HashSet::new()),
            departed_objects: Mutex::new(std::collections::HashMap::new()),
            edit_objects: Mutex::new(HashMap::new()),
            object_edits: Mutex::new(HashMap::new()),
            terrain_edits: Mutex::new(HashMap::new()),
            bus_stops: Mutex::new(Vec::new()),
            waiting_places: Mutex::new(Vec::new()),
            waiting_cabins: Mutex::new(HashMap::new()),
            tiles_generation: std::sync::atomic::AtomicU64::new(0),
            parked_cars: Mutex::new(Vec::new()),
            parked_boxes: Mutex::new(Arc::new(Vec::new())),
            petrol_stations: Mutex::new(Vec::new()),
            reverb_zones: Mutex::new(Vec::new()),
            light_maps: Mutex::new(HashMap::new()),
            light_maps_generation: std::sync::atomic::AtomicU64::new(0),
            light_map_atlas: Mutex::new(None),
            parked_live: std::sync::atomic::AtomicUsize::new(0),
            parked_max: ::config::get_int("ai", "max_parked").unwrap_or(0),
            signal_routes,
            particle_objects: Mutex::new(HashMap::new()),
            fonts: Arc::new(Mutex::new(::simulation::texttex::FontLibrary::new(root))),
            night_slots: Mutex::new(Vec::new()),
            night_modes: Mutex::new(Vec::new()),
            static_coronas: Mutex::new(Vec::new()),
            static_lights: Mutex::new(Vec::new()),
            index: Mutex::new(None),
            tile_state: Mutex::new(HashMap::new()),
            seeded: Mutex::new(Default::default()),
            gpu: Mutex::new(GpuCache::default()),
            missing: Mutex::new(Default::default()),
            staged: Mutex::new(HashMap::new()),
            layout: Mutex::new(None),
            sound_cfgs: Mutex::new(HashMap::new()),
            timetable_boards: Mutex::new(StopBoards::default()),
            calendar: std::sync::OnceLock::new(),
            registrations: std::sync::OnceLock::new(),
            start_clock: Mutex::new(::simulation::SimClock::default()),
        })
    }

    /// Every tile of global.cfg's `[map]` list whose file exists, with its index in that list
    /// (repeaters and timetable tracks name a tile by that index).
    pub fn map_tiles(&self) -> Vec<(usize, i32, i32, PathBuf)> {
        self.global
            .tiles
            .iter()
            .map(|t| {
                (
                    t.index,
                    t.x,
                    t.y,
                    ::legacy_config::resolve_path(&self.map_dir, &t.file),
                )
            })
            .filter(|t| ::legacy_config::vfs::is_file(&t.3))
            .collect()
    }

    /// The map has tile `key` (listed in global.cfg, and its file exists).
    pub fn has_tile(&self, key: (i32, i32)) -> bool {
        self.layout().paths.contains_key(&key)
    }

    /// Tiles within `radius` (in tiles, Chebyshev) of `center`; `None` = all.
    pub fn select_tiles(
        &self,
        center: Option<(i32, i32)>,
        radius: Option<i32>,
    ) -> Vec<(i32, i32, PathBuf)> {
        self.global
            .tiles
            .iter()
            .filter(|t| match (center, radius) {
                (Some((cx, cy)), Some(r)) => (t.x - cx).abs() <= r && (t.y - cy).abs() <= r,
                _ => true,
            })
            .map(|t| (t.x, t.y, ::legacy_config::resolve_path(&self.map_dir, &t.file)))
            .filter(|(_, _, p)| ::legacy_config::vfs::is_file(p))
            .collect()
    }

    /// Load, tessellate and upload `tiles` all at once (offscreen runs; the window streams).
    pub fn build_scene(
        &self,
        renderer: &Renderer,
        scene: &mut Scene,
        tiles: &[(i32, i32, PathBuf)],
    ) -> Result<LoadStats> {
        let t0 = std::time::Instant::now();
        // OMSI_BATCH=n: prepare the tiles a few at a time, as the window's streaming does
        let batch = ::legacy_config::env::var("OMSI_BATCH")
            .ok()
            .and_then(|v| v.parse::<usize>().ok())
            .filter(|n| *n > 0)
            .unwrap_or(tiles.len().max(1));
        let mut stats = LoadStats {
            tiles: tiles.len(),
            ..Default::default()
        };
        let mut t_prepare = std::time::Duration::ZERO;
        for chunk in tiles.chunks(batch) {
            let t = std::time::Instant::now();
            let (prepared, s) = self.prepare_tiles(chunk);
            t_prepare += t.elapsed();
            stats.add_prepared(&s);
            for p in prepared {
                self.upload_tile(renderer, scene, p, &mut stats);
            }
        }
        let t1 = t0 + t_prepare;
        // nothing more is loaded after this
        self.staged.lock().clear();
        self.refresh_tile_lists();
        stats.log_ground();
        if ::legacy_config::env::var_os("OMSI_PROFILE").is_some() {
            log::info!(
                "build_scene: prepared in {:.2} s, uploaded in {:.2} s",
                (t1 - t0).as_secs_f64(),
                t1.elapsed().as_secs_f64()
            );
        }
        // OMSI_DUMP_GROUND=file: every loaded tile's final terrain and how much of it the
        // roads cut away, as text (to compare a whole-map load with a streamed one)
        if let Some(path) = ::legacy_config::env::var_os("OMSI_DUMP_GROUND") {
            let mut out = String::new();
            let terrains = self.terrains.read();
            let surfaces = self.surfaces.read();
            let mut keys: Vec<&(i32, i32)> = terrains.keys().collect();
            keys.sort();
            for k in keys {
                let t = &terrains[k];
                let cut = surfaces
                    .get(k)
                    .map(|sf| {
                        let n = sf.size;
                        let cell = tile_size() as f32 / n as f32;
                        (0..n * n)
                            .filter(|&i| {
                                sf.cut_at(
                                    (i % n) as f32 * cell + cell * 0.5,
                                    (i / n) as f32 * cell + cell * 0.5,
                                    t.sample(
                                        (i % n) as f32 * cell + cell * 0.5,
                                        (i / n) as f32 * cell + cell * 0.5,
                                    ),
                                    surface_flush(),
                                )
                            })
                            .count()
                    })
                    .unwrap_or(0);
                out.push_str(&format!(
                    "{} {} {} {}
",
                    k.0,
                    k.1,
                    cut,
                    t.heights
                        .iter()
                        .map(|h| format!("{h:.3}"))
                        .collect::<Vec<_>>()
                        .join(" ")
                ));
            }
            let _ = std::fs::write(path, out);
        }
        stats.textures = self.gpu.lock().textures.len();
        Ok(stats)
    }

    /// The sim date moved on (midnight, a clock set by hand): the chrono scenarios in force
    /// then (OMSI re-evaluates them at the day's change: the original → the original).
    /// Returns the tiles a scenario that came or went changes (to be read again), and
    /// forgets the map index built with the old ones.
    pub fn set_date(&self, date: i32) -> Vec<(i32, i32)> {
        let new = ::map::active_chrono_dirs(&self.map_dir, date);
        let old = self.chrono_dirs.read().clone();
        if new == old {
            return Vec::new();
        }
        let mut tiles: hashbrown::HashSet<(i32, i32)> = hashbrown::HashSet::new();
        for d in old
            .iter()
            .filter(|d| !new.contains(d))
            .chain(new.iter().filter(|d| !old.contains(d)))
        {
            log::info!(
                "chrono: {} {} on {date}",
                d.display(),
                if new.contains(d) {
                    "comes into force"
                } else {
                    "ends"
                }
            );
            for (name, is_dir) in ::legacy_config::vfs::list_dir(d).unwrap_or_default() {
                if !is_dir {
                    if let Some(k) = ::map::tile_index_of(&name.to_string_lossy()) {
                        tiles.insert(k);
                    }
                }
            }
        }
        *self.chrono_dirs.write() = new;
        *self.index.lock() = None;
        let keys: Vec<(i32, i32)> = tiles.into_iter().collect();
        self.forget_staged(&keys);
        keys
    }

    /// The map index, built on first use (every tile file read once, in parallel).
    pub fn index(&self) -> Arc<MapIndex> {
        let mut g = self.index.lock();
        if let Some(ix) = g.as_ref() {
            return ix.clone();
        }
        let mut built = MapIndex::build(
            &self.map_tiles(),
            &self.chrono_dirs.read(),
            &self.root,
            self.map_detail,
        );
        // the index's object positions go to `object_positions` (kept once, not twice: 345 000
        // objects on Ahlheim took 30 MB in each)
        let objects = std::mem::take(&mut built.objects);
        {
            let dups = std::mem::take(&mut built.duplicates);
            if !dups.is_empty() {
                log::info!(
                    "map index: {} objects share their id with an object of another tile",
                    dups.len()
                );
            }
            let mut d = self.object_dups.lock();
            for (k, v) in dups {
                d.entry(k).or_insert(v);
            }
        }
        let ix = Arc::new(built);
        // what the map names that is not installed: counted always, listed on request
        // (OMSI_CHECK_TYPES=1); loading leaves those objects out either way
        let groups = ix.missing_files(&self.root);
        let files: usize = groups.iter().map(|g| g.1.len()).sum();
        if files > 0 {
            let records: usize = groups.iter().flat_map(|g| g.1.iter().map(|m| m.1)).sum();
            log::warn!(
                "{} object and spline files named by the map are not installed ({} records, in {} add-on folders; OMSI_CHECK_TYPES=1 lists them)",
                files,
                records,
                groups.len()
            );
            if ::legacy_config::env::var_os("OMSI_CHECK_TYPES").is_some() {
                for (folder, list) in &groups {
                    log::info!(
                        "  missing from {folder}: {} files, {} records",
                        list.len(),
                        list.iter().map(|m| m.1).sum::<usize>()
                    );
                    for (f, n, t) in list {
                        log::info!("    {f}  ({n} records, e.g. tile {},{})", t.0, t.1);
                    }
                }
            }
        }
        {
            let mut positions = self.object_positions.lock();
            positions.reserve(objects.len());
            for (id, (_, pos, rot)) in objects {
                positions.entry(id).or_insert((pos, rot));
            }
        }
        *g = Some(ix.clone());
        ix
    }

    /// Which tiles the map has and which tiles each one depends on, built on first use.
    pub fn layout(&self) -> Arc<TileLayout> {
        if let Some(l) = self.layout.lock().as_ref() {
            return l.clone();
        }
        let index = self.index();
        let paths: HashMap<(i32, i32), PathBuf> = self
            .select_tiles(None, None)
            .into_iter()
            .map(|(x, y, p)| ((x, y), p))
            .collect();
        let mut sources: HashMap<(i32, i32), Vec<(i32, i32)>> = HashMap::new();
        for k in paths.keys() {
            let list = sources.entry(*k).or_default();
            for (dx, dy) in NEIGHBOURHOOD {
                if paths.contains_key(&(k.0 + dx, k.1 + dy)) {
                    list.push((k.0 + dx, k.1 + dy));
                }
            }
        }
        // a spline reaching further than its neighbours shapes the tiles it reaches
        let (lo, hi) = paths.keys().fold(
            ((i32::MAX, i32::MAX), (i32::MIN, i32::MIN)),
            |(lo, hi), k| {
                (
                    (lo.0.min(k.0), lo.1.min(k.1)),
                    (hi.0.max(k.0), hi.1.max(k.1)),
                )
            },
        );
        let ts = tile_size();
        let mut extra = 0usize;
        for (s, c) in &index.covers {
            if !paths.contains_key(s) {
                continue;
            }
            let kx0 = (((c[0] - SOURCE_MARGIN) / ts).floor() as i32 - 1).max(lo.0);
            let kx1 = (((c[2] + SOURCE_MARGIN) / ts).floor() as i32).min(hi.0);
            let ky0 = (((c[1] - SOURCE_MARGIN) / ts).floor() as i32 - 1).max(lo.1);
            let ky1 = (((c[3] + SOURCE_MARGIN) / ts).floor() as i32).min(hi.1);
            for kx in kx0..=kx1 {
                for ky in ky0..=ky1 {
                    if (kx - s.0).abs() <= 1 && (ky - s.1).abs() <= 1 {
                        continue;
                    }
                    let (x0, y0) = (
                        kx as f64 * ts - SOURCE_MARGIN,
                        ky as f64 * ts - SOURCE_MARGIN,
                    );
                    let (x1, y1) = (
                        (kx + 1) as f64 * ts + SOURCE_MARGIN,
                        (ky + 1) as f64 * ts + SOURCE_MARGIN,
                    );
                    if c[2] < x0 || c[0] > x1 || c[3] < y0 || c[1] > y1 {
                        continue;
                    }
                    if let Some(list) = sources.get_mut(&(kx, ky)) {
                        list.push(*s);
                        extra += 1;
                    }
                }
            }
        }
        for l in sources.values_mut() {
            l.sort();
            l.dedup();
        }
        if extra > 0 {
            log::info!(
                "tile layout: {extra} times a tile takes a long spline from beyond its neighbours"
            );
        }
        let layout = Arc::new(TileLayout { paths, sources });
        *self.layout.lock() = Some(layout.clone());
        layout
    }

    /// The file tile (tx, ty) is read from, and the map folder's place relative to `root`.
    pub fn tile_source(&self, tx: i32, ty: i32) -> Option<PathBuf> {
        let t = self.global.tiles.iter().find(|t| t.x == tx && t.y == ty)?;
        Some(::legacy_config::resolve_path(&self.map_dir, &t.file))
    }

    /// Rebuild the world's lists (stops, obstacles, lights, lamps) from the loaded tiles.
    pub fn refresh_tile_lists(&self) {
        let states = self.tile_state.lock();
        let mut keys: Vec<&(i32, i32)> = states.keys().collect();
        keys.sort();
        let mut stops = Vec::new();
        let mut waiting = Vec::new();
        let mut collision = ::simulation::collision::CollisionWorld::default();
        let mut light_world = ::simulation::collision::CollisionWorld::default();
        let mut parked = Vec::new();
        let mut coronas = Vec::new();
        let mut lights = Vec::new();
        let mut lamps = Vec::new();
        let mut night = Vec::new();
        let mut modes = Vec::new();
        let mut petrol = Vec::new();
        let mut reverb = Vec::new();
        for k in keys {
            let s = &states[k];
            petrol.extend(s.petrol_stations.iter().copied());
            reverb.extend(s.reverb_zones.iter().copied());
            stops.extend(s.bus_stops.iter().cloned());
            waiting.extend(s.waiting_places.iter().cloned());
            for b in &s.obstacles {
                collision.add(*b);
            }
            parked.extend(s.parked_boxes.iter().copied());
            for m in &s.mesh_obstacles {
                collision.add_mesh(m.clone());
            }
            for m in &s.light_obstacles {
                light_world.add_mesh(m.clone());
            }
            coronas.extend(s.coronas.iter().cloned());
            lights.extend(s.lights.iter().cloned());
            lamps.extend(s.light_objects.iter().cloned());
            night.extend(s.night_slots.iter().cloned());
            modes.extend(s.night_modes.iter().cloned());
        }
        *self.bus_stops.lock() = stops;
        *self.waiting_places.lock() = waiting;
        *self.collision.lock() = Arc::new(collision);
        *self.light_occluders.lock() = Arc::new(light_world);
        *self.parked_boxes.lock() = Arc::new(parked);
        *self.static_coronas.lock() = coronas;
        *self.static_lights.lock() = lights;
        *self.light_objects.lock() = lamps;
        *self.night_slots.lock() = night;
        *self.night_modes.lock() = modes;
        *self.petrol_stations.lock() = petrol;
        *self.reverb_zones.lock() = reverb;
        self.tiles_generation
            .fetch_add(1, std::sync::atomic::Ordering::Relaxed);
    }

    /// The tiles whose data is loaded.
    pub fn loaded_tiles(&self) -> Vec<(i32, i32)> {
        self.tile_state.lock().keys().copied().collect()
    }
}
