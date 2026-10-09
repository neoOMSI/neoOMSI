use super::*;

impl World {
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
            return 0;
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
        shrunk.len() + restoring
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
