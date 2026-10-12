use super::*;

/// Leave room for in-flight uploads and allocations outside the scene's texture array.
/// DXGI's per-process budget already accounts for other GPU users, including the launcher.
fn cache_texture_limit(requested: u64, textures: u64, memory: Option<(u64, u64)>) -> u64 {
    let Some((used, budget)) = memory.filter(|(_, budget)| *budget > 0) else {
        return requested;
    };
    let reserve = (budget / 20).clamp(128_000_000, 512_000_000);
    let available = budget.saturating_sub(used.saturating_sub(textures) + reserve);
    requested.min(available.max(64_000_000))
}

impl World {
    /// Optional fleet uploads must leave room for visible vehicles and their displays.
    /// Mandatory uploads retain every texture level, regardless of this cache limit.
    pub fn can_precache_vehicle(&self, renderer: &Renderer, scene: &Scene) -> bool {
        let requested = self.texture_budget_bytes();
        if requested == 0 {
            return true;
        }
        let live = self
            .texture_cache_limit
            .load(std::sync::atomic::Ordering::Relaxed);
        let textures = renderer.texture_bytes(scene);
        let limit = if live == 0 {
            // Before the first rendered frame (and after a live setting change), obtain
            // the driver limit before preloading. Small cards must not allocate the
            // entire requested fleet first and only discover pressure a second later.
            let initial = cache_texture_limit(requested, textures, ::render::video_memory());
            self.texture_cache_limit
                .store(initial, std::sync::atomic::Ordering::Relaxed);
            initial
        } else {
            requested.min(live)
        };
        textures < limit - limit / 10
    }

    /// Swap in the textures compressed on the workers since the last call (until
    /// `deadline`), and start compressing the ones uploaded as RGBA since. Returns how many
    /// were swapped.
    pub fn apply_texture_upgrades(
        &self,
        renderer: &Renderer,
        scene: &mut Scene,
        deadline: Option<std::time::Instant>,
    ) -> usize {
        let (wanted, restores) = {
            let mut gpu = self.gpu.lock();
            let mut w = std::mem::take(&mut gpu.wants_upgrade);
            w.append(&mut self.freetex_upgrades.lock());
            let r = std::mem::take(&mut gpu.wants_restore);
            w.extend(r.iter().cloned());
            (w, r)
        };
        if !wanted.is_empty() {
            let mut pending = self.upgrades_pending.lock();
            for path in wanted {
                if !pending.insert(path.clone()) {
                    continue;
                }
                let done = self.upgrades_done.clone();
                let restore = restores.contains(&path);
                // (off the frame's pool: see `threads`)
                crate::threads::background_pool().spawn(move || {
                    if let Some(t) = load_texture_key(&path, true) {
                        // (a texture over the budget comes back whatever it is, and so
                        // does one put up at half its size meanwhile: see
                        // `::texture::gpu::halved_for_now`)
                        if t.format.is_compressed()
                            || restore
                            || (t.width >= 256 && t.height >= 256)
                        {
                            done.lock().push((path, Arc::new(t)));
                            return;
                        }
                    }
                    // nothing better to be had: it stays as it is
                    done.lock().push((
                        path,
                        Arc::new(TextureData {
                            width: 0,
                            height: 0,
                            format: ::texture::PixelFormat::Rgba8,
                            levels: Vec::new(),
                            has_alpha: false,
                            gpu_mips: false,
                        }),
                    ));
                });
            }
        }
        let mut swapped: Vec<TextureId> = Vec::new();
        loop {
            if deadline
                .map(|d| std::time::Instant::now() >= d)
                .unwrap_or(false)
                && !swapped.is_empty()
            {
                break;
            }
            let Some((path, data)) = self.upgrades_done.lock().pop() else {
                break;
            };
            self.upgrades_pending.lock().remove(&path);
            // the texture is still up under that name (it may have gone meanwhile)
            let vid = self.vehicle_textures.lock().get(&path).map(|e| e.0);
            let id = match vid {
                Some(id) => Some(id),
                None => self.gpu.lock().textures.get(&path).map(|e| e.id),
            };
            let Some(id) = id else { continue };
            if data.levels.is_empty() {
                // nothing better to be had (an upgrade that stays RGBA)
                continue;
            }
            renderer.replace_texture(scene, id, &data);
            if let Some(e) = self.gpu.lock().textures.get_mut(&path) {
                e.bytes = scene.texture_bytes_of(id);
                e.format = data.format;
                e.dropped = 0;
            }
            swapped.push(id);
        }
        let n = swapped.len();
        if n > 0 {
            let t = std::time::Instant::now();
            let rebound = renderer.rebind_textures(scene, &swapped);
            if ::legacy_config::env::var_os("OMSI_PROFILE").is_some() {
                log::info!(
                    "textures: {n} compressed ones swapped in, {rebound} materials rebound in {:.1} ms",
                    t.elapsed().as_secs_f64() * 1000.0
                );
            }
        }
        n
    }

    /// OMSI's `[texmemlimit]`: the scenery and vehicle textures may take `bytes` on the GPU
    /// (0 = no limit), see [`World::update_texture_budget`].
    pub fn set_texture_budget(&self, bytes: u64) {
        self.texture_limit
            .store(bytes, std::sync::atomic::Ordering::Relaxed);
        self.texture_cache_limit
            .store(0, std::sync::atomic::Ordering::Relaxed);
    }

    /// The textures' budget now (bytes, 0 = none).
    pub fn texture_budget_bytes(&self) -> u64 {
        self.texture_limit
            .load(std::sync::atomic::Ordering::Relaxed)
    }

    /// Keep the textures within their budget, once a second (`force`: now): while they take
    /// more, the scenery textures only far tiles use lose their finest mip level, the
    /// farthest first, down to 64 texels a side and never within 150 m of `centers` (the
    /// camera, the player's bus); when there is room again, those that came within 400 m
    /// are read again whole on a worker and swapped back. Vehicle textures count but keep
    /// their levels (a fleet set nobody draws leaves the GPU anyway). Returns the textures
    /// shrunk or sent to be read again.
    pub fn update_texture_budget(
        &self,
        renderer: &Renderer,
        scene: &mut Scene,
        centers: &[DVec3],
        force: bool,
    ) -> usize {
        const NEAR: f64 = 150.0;
        const RESTORE: f64 = 400.0;
        const MIN_SIDE: u32 = 64;
        let limit = self
            .texture_limit
            .load(std::sync::atomic::Ordering::Relaxed);
        if limit == 0 || centers.is_empty() {
            return 0;
        }
        {
            let mut last = self.budget_checked.lock();
            if !force
                && last
                .map(|t| t.elapsed().as_secs_f32() < 1.0)
                .unwrap_or(false)
            {
                return 0;
            }
            *last = Some(std::time::Instant::now());
        }
        let t0 = std::time::Instant::now();
        // Count the complete texture array: PBR maps, generated displays, tile-owned
        // textures and variant maps all consume VRAM even without a named-cache entry.
        // Release unused fleet sets first. This changes residency, never their appearance.
        let all_bytes = renderer.texture_bytes(scene);
        let cache_limit = cache_texture_limit(limit, all_bytes, ::render::video_memory());
        self.texture_cache_limit
            .store(cache_limit, std::sync::atomic::Ordering::Relaxed);
        let evicted = if all_bytes > cache_limit {
            self.trim_vehicle_cache(renderer, scene, cache_limit - cache_limit / 10)
        } else {
            0
        };
        if evicted > 0 {
            log::info!(
                "texture cache: {:.0} -> {:.0} MB, {:.0} MB limit ({:.0} MB requested), {evicted} unused vehicle sets released",
                all_bytes as f64 / 1e6,
                renderer.texture_bytes(scene) as f64 / 1e6,
                cache_limit as f64 / 1e6,
                limit as f64 / 1e6,
            );
        }
        // Preserve the existing scenery mip policy below. Additional pressure from
        // PBR/display textures or other applications must not blur visible assets.
        let vehicle_bytes: u64 = self
            .vehicle_textures
            .lock()
            .values()
            .map(|(t, _)| scene.texture_bytes_of(*t))
            .sum();
        let ts = tile_size();
        let tile_distance = |k: &(i32, i32)| -> f64 {
            centers
                .iter()
                .map(|c| {
                    let (x0, y0) = (k.0 as f64 * ts, k.1 as f64 * ts);
                    let dx = (x0 - c.x).max(c.x - (x0 + ts)).max(0.0);
                    let dy = (y0 - c.y).max(c.y - (y0 + ts)).max(0.0);
                    (dx * dx + dy * dy).sqrt()
                })
                .fold(f64::MAX, f64::min)
        };
        let states = self.tile_state.lock();
        let mut gpu = self.gpu.lock();
        if gpu.textures.values().map(|e| e.bytes).sum::<u64>() + vehicle_bytes <= limit
            && gpu.textures.values().all(|e| e.dropped == 0)
        {
            return evicted;
        }
        // how near each scenery texture is: the nearest tile that uses it
        let mut near: hashbrown::HashMap<TextureId, f64> = hashbrown::HashMap::new();
        let mut spline_textures = hashbrown::HashSet::new();
        let (mut types, mut splines): (
            hashbrown::HashMap<usize, f64>,
            hashbrown::HashMap<usize, f64>,
        ) = Default::default();
        let (mut trees, mut shared): (
            hashbrown::HashMap<&str, f64>,
            hashbrown::HashMap<&PathBuf, f64>,
        ) = Default::default();
        for (key, st) in states.iter() {
            let d = tile_distance(key);
            let nearer = |e: &mut f64| *e = e.min(d);
            st.gpu
                .types
                .iter()
                .for_each(|t| nearer(types.entry(*t).or_insert(f64::MAX)));
            st.gpu
                .spline_types
                .iter()
                .for_each(|t| nearer(splines.entry(*t).or_insert(f64::MAX)));
            st.gpu
                .trees
                .iter()
                .for_each(|t| nearer(trees.entry(t.as_str()).or_insert(f64::MAX)));
            st.gpu
                .shared_textures
                .iter()
                .for_each(|p| nearer(shared.entry(p).or_insert(f64::MAX)));
        }
        {
            let g = &*gpu;
            let mut see = |p: &PathBuf, d: f64| {
                if let Some(e) = g.textures.get(p) {
                    let n = near.entry(e.id).or_insert(f64::MAX);
                    *n = n.min(d);
                }
            };
            for (t, d) in &types {
                g.types
                    .get(t)
                    .into_iter()
                    .flat_map(|t| t.textures.iter())
                    .for_each(|p| see(p, *d));
            }
            for (t, d) in &splines {
                for p in g.splines.get(t).into_iter().flat_map(|s| s.textures.iter()) {
                    see(p, *d);
                    spline_textures.insert(p.clone());
                }
            }
            for (t, d) in &trees {
                g.trees
                    .get(*t)
                    .and_then(|t| t.texture.as_ref())
                    .into_iter()
                    .for_each(|p| see(p, *d));
            }
            for (p, d) in &shared {
                see(p, *d);
            }
        }
        drop((trees, shared));
        drop(states);
        let usage: u64 = gpu.textures.values().map(|e| e.bytes).sum::<u64>() + vehicle_bytes;
        let mut entries: Vec<(f64, PathBuf)> = gpu
            .textures
            .iter()
            .map(|(p, e)| (near.get(&e.id).copied().unwrap_or(f64::MAX), p.clone()))
            .collect();
        let mut shrunk: Vec<TextureId> = Vec::new();
        let mut restoring = 0usize;
        // A texture shrunk while its tiles were far that is near now comes back whole at
        // once, room or not: the far ones give way for it in the seconds after. (Waiting for
        // room left the buildings right in front of the bus blurred for good on a map that
        // filled the budget - they had lost their levels on the way in.)
        {
            let pending = self.upgrades_pending.lock();
            for (d, p) in &entries {
                if *d >= NEAR || restoring >= 24 || usage > limit + limit / 32 {
                    continue;
                }
                if gpu.textures.get(p).is_some_and(|e| e.dropped > 0)
                    && !pending.contains(p)
                    && !gpu.wants_restore.contains(p)
                {
                    gpu.wants_restore.push(p.clone());
                    restoring += 1;
                }
            }
        }
        if usage > limit + limit / 32 {
            entries.sort_by(|a, b| b.0.total_cmp(&a.0));
            let mut over = usage - (limit - limit / 10);
            for (d, p) in &entries {
                // (96 a second: at 16 a map's first tiles stayed over a small card's budget
                // for a minute and a half)
                if over == 0 || shrunk.len() >= 256 || *d < NEAR * 1.0 {
                    break;
                }
                if spline_textures.contains(p) {
                    continue;
                }
                let Some(e) = gpu.textures.get_mut(p) else {
                    continue;
                };
                let Some((w, h, levels)) = renderer.texture_levels(scene, e.id) else {
                    continue;
                };
                if w.min(h) / 2 < MIN_SIDE || levels < 2 {
                    continue;
                }
                let before = e.bytes;
                // far away and big: two levels at once
                let n = if *d > 700.0 && w.min(h) / 4 >= MIN_SIDE.max(256) && levels > 2 {
                    2
                } else {
                    1
                };
                if renderer.drop_top_levels(scene, e.id, n) {
                    e.bytes = scene.texture_bytes_of(e.id);
                    e.dropped += n;
                    over = over.saturating_sub(before - e.bytes);
                    shrunk.push(e.id);
                }
            }
        } else {
            // room for the near ones to come back (with a tenth kept free)
            entries.sort_by(|a, b| a.0.total_cmp(&b.0));
            let mut room = (limit - limit / 10).saturating_sub(usage);
            let pending = self.upgrades_pending.lock();
            for (d, p) in &entries {
                if *d > RESTORE || restoring >= 16 {
                    break;
                }
                let Some(e) = gpu.textures.get(p) else {
                    continue;
                };
                if e.dropped == 0 || pending.contains(p) {
                    continue;
                }
                let whole = e.bytes << (2 * e.dropped.min(8));
                if whole - e.bytes > room {
                    break;
                }
                room -= whole - e.bytes;
                gpu.wants_restore.push(p.clone());
                restoring += 1;
            }
        }
        drop(gpu);
        let rebound = renderer.rebind_textures(scene, &shrunk);
        if (!shrunk.is_empty() || restoring > 0) && ::legacy_config::env::var_os("OMSI_PROFILE").is_some()
        {
            log::info!(
                "texture budget: {:.0} of {:.0} MB in use, {} textures lost a level ({} materials rebound), {} coming back, in {:.1} ms",
                usage as f64 / 1e6,
                limit as f64 / 1e6,
                shrunk.len(),
                rebound,
                restoring,
                t0.elapsed().as_secs_f64() * 1000.0
            );
        }
        shrunk.len() + restoring + evicted
    }

    /// After big unloads, cut the free tail off the scene's arrays (meshes, textures,
    /// materials, instances), so that a long drive across a big map does not keep the
    /// arrays at their largest. Only when a quarter of an array or more would go (cutting
    /// instances makes the renderer rebuild its per-draw buffers once).
    pub fn compact_slots(&self, renderer: &Renderer, scene: &mut Scene) -> [usize; 4] {
        let mut gpu = self.gpu.lock();
        let worth = |len: usize, keep: usize| len - keep >= 1024 && (len - keep) * 4 >= len;
        let meshes = gpu.free_meshes.free_tail(scene.meshes.len());
        let textures = gpu.free_textures.free_tail(scene.textures.len());
        let materials = gpu.free_materials.free_tail(scene.materials.len());
        // instances are free by slot count: their tail across all counts
        let mut instances = scene.instances.len();
        // Most checks have no free slot at the end. Avoid sorting every free instance
        // after each streaming burst just to discover that the tail cannot shrink.
        if instances > 0
            && gpu
            .free_instances
            .values()
            .any(|l| l.0.iter().any(|r| r.0 == instances - 1))
        {
            let free: hashbrown::HashSet<usize> = gpu
                .free_instances
                .values()
                .flat_map(|l| l.0.iter().map(|r| r.0))
                .collect();
            while instances > 0 && free.contains(&(instances - 1)) {
                instances -= 1;
            }
        }
        let lens = [
            scene.meshes.len(),
            scene.textures.len(),
            scene.materials.len(),
            scene.instances.len(),
        ];
        let keep = [meshes, textures, materials, instances];
        let keep: Vec<usize> = lens
            .iter()
            .zip(keep)
            .map(|(l, k)| if worth(*l, k) { k } else { *l })
            .collect();
        if keep.iter().zip(lens).all(|(k, l)| *k == l) {
            return [0; 4];
        }
        let t = std::time::Instant::now();
        renderer.truncate(scene, keep[0], keep[1], keep[2], keep[3]);
        gpu.free_meshes.keep_below(keep[0]);
        gpu.free_textures.keep_below(keep[1]);
        gpu.free_materials.keep_below(keep[2]);
        for l in gpu.free_instances.values_mut() {
            l.keep_below(keep[3]);
        }
        let cut = [
            lens[0] - keep[0],
            lens[1] - keep[1],
            lens[2] - keep[2],
            lens[3] - keep[3],
        ];
        if ::legacy_config::env::var_os("OMSI_PROFILE").is_some() {
            log::info!(
                "scene slots: cut {} meshes, {} textures, {} materials, {} instances off the end in {:.1} ms",
                cut[0],
                cut[1],
                cut[2],
                cut[3],
                t.elapsed().as_secs_f64() * 1000.0
            );
        }
        cut
    }

    /// Wait for the textures being compressed and swap them all in (offscreen pictures).
    pub fn finish_texture_upgrades(&self, renderer: &Renderer, scene: &mut Scene) {
        loop {
            self.apply_texture_upgrades(renderer, scene, None);
            if self.upgrades_pending.lock().is_empty() && self.gpu.lock().wants_upgrade.is_empty() {
                break;
            }
            std::thread::sleep(std::time::Duration::from_millis(5));
        }
    }
}

#[cfg(test)]
mod residency_tests {
    use super::*;

    #[test]
    fn driver_budget_reserves_non_texture_allocations_and_headroom() {
        // A requested 6 GB remains available on an 8 GB card when the non-texture
        // allocation and driver headroom fit. A lower setting must still cap it.
        let memory = Some((2_000_000_000, 7_500_000_000));
        assert_eq!(cache_texture_limit(6_000_000_000, 1_000_000_000, memory), 6_000_000_000);
        assert_eq!(cache_texture_limit(2_000_000_000, 1_000_000_000, memory), 2_000_000_000);
        // 2 GB textures + 3 GB targets/buffers; another process reduces our budget to 4 GB.
        assert_eq!(
            cache_texture_limit(
                3_000_000_000,
                2_000_000_000,
                Some((5_000_000_000, 4_000_000_000))
            ),
            800_000_000,
        );
        assert_eq!(cache_texture_limit(2_000_000_000, 1, None), 2_000_000_000);
        assert_eq!(
            cache_texture_limit(2_000_000_000, 1, Some((1, 0))),
            2_000_000_000
        );
        assert_eq!(
            cache_texture_limit(2_000_000_000, 0, Some((8_000_000_000, 4_000_000_000))),
            64_000_000
        );
        // A user's lower limit remains authoritative, also below the emergency floor.
        assert_eq!(
            cache_texture_limit(32_000_000, 0, Some((8_000_000_000, 4_000_000_000))),
            32_000_000
        );
    }

    #[test]
    fn budget_releases_unused_sets_but_preserves_active_and_shared_textures() {
        let root = std::env::temp_dir().join(format!("neoomsi-residency-{}", std::process::id()));
        std::fs::create_dir_all(&root).unwrap();
        std::fs::write(root.join("global.cfg"), "[name]\nResidency test\n").unwrap();
        let world = World::open(&root, &root.join("global.cfg"), 20261012).unwrap();
        let mut desc = wgpu::InstanceDescriptor::new_without_display_handle();
        desc.backends = wgpu::Backends::NOOP;
        desc.backend_options.noop = wgpu::NoopBackendOptions::enabled();
        let renderer = pollster::block_on(Renderer::new_with(
            &wgpu::Instance::new(desc),
            None,
            Some(wgpu::TextureFormat::Rgba8UnormSrgb),
            ::render::RenderOptions {
                msaa: 1,
                ssao: false,
                no_enhanced: true,
                shadow_size: 512,
                ..Default::default()
            },
        ))
        .unwrap();
        let mut scene = renderer.new_scene();
        let image = ::texture::Image {
            width: 128,
            height: 128,
            rgba: vec![128; 128 * 128 * 4],
            has_alpha: false,
        };
        let active = renderer.add_texture(&mut scene, &image, false);
        let shared = renderer.add_texture(&mut scene, &image, false);
        let unused = renderer.add_texture(&mut scene, &image, false);
        // PBR storage is outside vehicle_textures but must disappear with its diffuse.
        renderer.add_pbr_maps(
            &mut scene,
            unused,
            &::texture::pbr::PbrImages {
                normal: Some(image.clone()),
                orm: None,
                flags: [1.0, 0.0, 0.0, 0.0],
            },
        );
        let normal = scene.pbr_maps[&unused].normal.unwrap();
        let a = PathBuf::from("active.dds");
        let b = PathBuf::from("shared.dds");
        let c = PathBuf::from("unused.dds");
        world.vehicle_textures.lock().extend([
            (a.clone(), (active, 1)),
            (b.clone(), (shared, 2)),
            (c.clone(), (unused, 1)),
        ]);
        let set = |users, textures| super::super::vehicles::VehicleSet {
            meshes: Vec::new(),
            dyn_slots: Vec::new(),
            variants: Vec::new(),
            textures,
            mesh_keys: Vec::new(),
            materials: Vec::new(),
            users,
            idle_since: (users == 0).then(std::time::Instant::now),
        };
        let active_key = (PathBuf::from("active.bus"), None);
        let unused_key = (PathBuf::from("unused.bus"), None);
        world.vehicle_gpu.lock().extend([
            (active_key.clone(), set(1, vec![a, b.clone()])),
            (unused_key.clone(), set(0, vec![b.clone(), c])),
        ]);
        let active_bytes = scene.texture_bytes_of(active) + scene.texture_bytes_of(shared);
        world.set_texture_budget(active_bytes);
        assert!(!world.can_precache_vehicle(&renderer, &scene));
        assert_eq!(
            world.trim_vehicle_cache(&renderer, &mut scene, active_bytes),
            1
        );
        assert!(world.has_vehicle_set(&active_key));
        assert!(!world.has_vehicle_set(&unused_key));
        assert_eq!(renderer.texture_bytes(&scene), active_bytes);
        assert_eq!(scene.texture_bytes_of(normal), 0);
        assert_eq!(world.vehicle_textures.lock()[&b].1, 1);
        assert_eq!(renderer.texture_levels(&scene, active), Some((128, 128, 1)));
        // An impossible limit cannot evict an active vehicle or reduce its texture.
        assert_eq!(world.trim_vehicle_cache(&renderer, &mut scene, 1), 0);
        assert_eq!(renderer.texture_bytes(&scene), active_bytes);
        // Check the resolved test directory before recursive cleanup.
        let actual = root.canonicalize().unwrap();
        let temp = std::env::temp_dir().canonicalize().unwrap();
        assert!(actual.starts_with(&temp) && actual != temp);
        std::fs::remove_dir_all(actual).unwrap();
    }
}
