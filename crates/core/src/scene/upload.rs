use super::*;

impl World {
    /// Texture names (with their search folders) a prepared tile will ask for that are not
    /// on the GPU yet.
    ///
    /// Runs on the loader thread. The GPU cache is only locked for two quick looks (which
    /// types are there, then which of the found files are): the lookups themselves go to
    /// the disk or into an archive the first time a name comes up, and the thread that
    /// draws waited for them - a tile upload of the Ahlheim main station took 372 ms.
    pub(super) fn wanted_textures(&self, p: &Prepared) -> Vec<(String, Vec<PathBuf>)> {
        let (have_types, have_splines, have_trees): (
            hashbrown::HashSet<usize>,
            hashbrown::HashSet<usize>,
            hashbrown::HashSet<String>,
        ) = {
            let gpu = self.gpu.lock();
            (
                p.objects
                    .iter()
                    .map(|o| Arc::as_ptr(&o.ot) as usize)
                    .filter(|k| gpu.types.contains_key(k))
                    .collect(),
                p.splines
                    .iter()
                    .map(|(_, st, _, _)| Arc::as_ptr(st) as usize)
                    .filter(|k| gpu.splines.contains_key(k))
                    .collect(),
                p.trees
                    .iter()
                    .map(|t| t.1.to_ascii_lowercase())
                    .filter(|k| gpu.trees.contains_key(k))
                    .collect(),
            )
        };
        let mut names: Vec<(String, Vec<PathBuf>)> = Vec::new();
        let push = |name: &str, dirs: &[PathBuf], out: &mut Vec<(String, Vec<PathBuf>)>| {
            if name.trim().is_empty() || out.len() > 4096 {
                return;
            }
            out.push((name.to_string(), dirs.to_vec()));
        };
        let mut seen: hashbrown::HashSet<*const ObjectType> = hashbrown::HashSet::new();
        for o in &p.objects {
            if let Some(inst) = &o.script {
                let selection = scenery_texture_selection(&o.ot, inst);
                for (group, &index) in o.ot.dynamic_textures.iter().zip(&selection) {
                    for (_, file, dir) in group.choices.get(index).into_iter().flatten() {
                        let mut dirs = texture_dirs(&self.root, &o.ot.model_dir);
                        dirs.insert(0, dir.clone());
                        push(file, &dirs, &mut names);
                    }
                }
            }
            if o.lamp.is_none() {
                for (_, _, overrides) in &o.ot.meshes {
                    for ov in overrides.iter().filter(|m| !m.item && m.freetex.is_some()) {
                        let Some((_, var)) = &ov.freetex else {
                            continue;
                        };
                        if let Some(name) = resolve_scenery_freetex_name(
                            var,
                            ov,
                            overrides,
                            o.script.as_ref(),
                            None,
                            &o.strings,
                        ) {
                            push(name, &texture_dirs(&self.root, &o.ot.model_dir), &mut names);
                        }
                    }
                }
            }
            let key = Arc::as_ptr(&o.ot);
            if have_types.contains(&(key as usize)) || !seen.insert(key) {
                continue;
            }
            let dirs = texture_dirs(&self.root, &o.ot.model_dir);
            for (_, mats, overrides) in
                o.ot.meshes
                    .iter()
                    .chain(o.ot.lower_lods.iter().flat_map(|l| l.1.iter()))
            {
                for m in mats {
                    push(&m.texture, &dirs, &mut names);
                    // and its copy in the `night` folder, which `type_gpu` looks for: left to
                    // the main thread, big night JPEGs of a mod map were decoded there, up to
                    // 1.7 s for one object type (the freezes on Grande Porto, Novi Sad)
                    if !is_null_texture(&m.texture) {
                        push(&night_texture_name(&m.texture), &dirs, &mut names);
                    }
                }
                for ov in overrides {
                    push(&ov.texture, &dirs, &mut names);
                    if let Some(n) = &ov.nightmap {
                        push(n, &dirs, &mut names);
                    }
                    if let Some(t) = &ov.transmap {
                        push(t, &dirs, &mut names);
                    }
                    if let Some((e, _)) = &ov.envmap {
                        push(e, &dirs, &mut names);
                    }
                    if let Some(m) = &ov.envmap_mask {
                        push(m, &dirs, &mut names);
                    }
                }
            }
        }
        for (_, st, _, _) in &p.splines {
            if have_splines.contains(&(Arc::as_ptr(st) as usize)) {
                continue;
            }
            let dirs = texture_dirs(&self.root, &st.dir);
            for t in &st.def.textures {
                push(&t.file, &dirs, &mut names);
            }
        }
        for (ot, tex, ..) in &p.trees {
            if !have_trees.contains(&tex.to_ascii_lowercase()) {
                push(tex, &texture_dirs(&self.root, &ot.model_dir), &mut names);
            }
        }
        // the painted ground layers (and their detail textures) of the tile
        let ground_dirs = vec![self.root.clone()];
        for (layer, _) in &p.paint_masks {
            if let Some(gt) = self.global.ground_textures.get(*layer) {
                push(&gt.texture, &ground_dirs, &mut names);
                push(&gt.detail_texture, &ground_dirs, &mut names);
            }
        }
        names.sort();
        names.dedup();
        // find the files without the lock, then keep what the GPU does not have yet
        let found: Vec<(PathBuf, (String, Vec<PathBuf>))> = names
            .into_iter()
            .filter_map(|(name, dirs)| {
                let dirs_ref: Vec<&Path> = dirs.iter().map(|d| d.as_path()).collect();
                ::texture::find_texture(&name, &dirs_ref).map(|path| (path, (name, dirs)))
            })
            .collect();
        let gpu = self.gpu.lock();
        found
            .into_iter()
            .filter(|(path, _)| !gpu.textures.contains_key(path))
            .map(|(_, n)| n)
            .collect()
    }

    /// The materials every tile shares: the plain ground, the water, the tree quad.
    pub(super) fn ensure_ground(&self, renderer: &Renderer, scene: &mut Scene, gpu: &mut GpuCache) {
        if gpu.ground.is_some() {
            return;
        }
        let none: HashMap<PathBuf, Arc<TextureData>> = HashMap::new();
        let ground_dirs = vec![self.root.clone()];
        let ground0 = self
            .global
            .ground_textures
            .first()
            .cloned()
            .unwrap_or_default();
        let ground_tex = if ground0.texture.trim().is_empty() {
            "Texture/gras.bmp".to_string()
        } else {
            ground0.texture.clone()
        };
        let ground_id = gpu
            .texture(renderer, scene, &ground_tex, &ground_dirs, &none)
            .map(|t| t.0);
        let ground_mat = renderer.add_material_night(
            scene,
            ground_id,
            AlphaMode::Opaque,
            [1.0; 4],
            false,
            None,
            None,
        );
        // The first [groundtex] is what the whole map starts as; its two numbers say how
        // often the texture and its detail texture repeat across one tile.
        let ground_repeats = if ground0.params[1] > 0.0 {
            ground0.repeats()
        } else {
            (tile_size() / 12.0) as f32
        };
        let ground_detail = gpu
            .texture(
                renderer,
                scene,
                &ground0.detail_texture,
                &ground_dirs,
                &none,
            )
            .map(|t| (t.0, ground0.detail_repeats()));
        // The base layer wets in the rain exactly like a painted one when its own
        // <texture>.cfg carries [moisture]/[puddles] - this used to be dropped on the
        // floor (add_terrain_material had no moisture parameter at all), so a map whose
        // default ground is a wet-tagged surface (rather than the untagged stock grass)
        // never showed it, while the very same texture painted as a later [groundtex]
        // layer (add_terrain_layer_material) got it right: a patchwork of wet and dry
        // that had nothing to do with the weather.
        let ground_dirs_ref: Vec<&Path> = ground_dirs.iter().map(|p| p.as_path()).collect();
        let ground_cfg = self.textures.cfg(&ground_tex, &ground_dirs_ref);
        let ground_wet = if ground_cfg.moisture || ground_cfg.puddles {
            1.0
        } else {
            0.0
        };
        let plain_terrain_mat = renderer.add_terrain_material(
            scene,
            ground_id,
            None,
            ground_detail,
            ground_repeats,
            None,
            ground_wet,
        );
        // Water: the map carries its own colour in `texture/water.tga` (the stock maps use
        // an 8x8 swatch of 47, 74, 83 at three quarters opacity, so the riverbed shows
        // through) and its own sphere map in `texture/water_envmap.bmp`. A map or mod that
        // ships different ones gets its own water.
        let water_mat = {
            let wdir = ::legacy_config::resolve_path(&self.map_dir, "texture");
            let dirs: Vec<&Path> = vec![wdir.as_path(), self.root.as_path()];
            let tex = match self.textures.get("water.tga", &dirs) {
                Some(img) => renderer.add_texture(scene, &img, true),
                None => {
                    let img = Image {
                        width: 1,
                        height: 1,
                        rgba: vec![47, 74, 83, 192],
                        has_alpha: true,
                    };
                    renderer.add_texture(scene, &img, false)
                }
            };
            let env = self
                .textures
                .get("water_envmap.bmp", &dirs)
                .or_else(|| {
                    self.textures.get(
                        "envmap_unscharf.bmp",
                        &[
                            &self.root.join("Vehicles/MAN_SD202/Texture"),
                            &::legacy_config::resolve_path(&self.root, "Texture"),
                        ],
                    )
                })
                .map(|i| renderer.add_texture(scene, &i, true));
            renderer.add_material_extra(
                scene,
                Some(tex),
                AlphaMode::Blend,
                [1.0; 4],
                false,
                None,
                None,
                None,
                env.map(|e| (e, 0.45)),
                [0.0; 3],
                ::render::MaterialExtra {
                    water: true,
                    ..Default::default()
                },
            )
        };
        let tree_mesh = renderer.add_mesh(scene, &tree_quad_mesh());
        gpu.ground = Some(GroundGpu {
            ground_id,
            ground_mat,
            plain_terrain_mat,
            ground_detail,
            ground_repeats,
            ground_wet,
            water_mat,
            tree_mesh,
        });
    }

    /// The GPU side of an object type (meshes, materials with their variants and night maps,
    /// lower LODs), uploaded once while any loaded tile uses it.
    pub(super) fn type_gpu(
        &self,
        renderer: &Renderer,
        scene: &mut Scene,
        gpu: &mut GpuCache,
        ot: &Arc<ObjectType>,
        images: &HashMap<PathBuf, Arc<TextureData>>,
        ground_mat: MaterialId,
    ) -> usize {
        let key = Arc::as_ptr(ot) as usize;
        if gpu.types.contains_key(&key) {
            return key;
        }
        let dirs = texture_dirs(&self.root, &ot.model_dir);
        let mut t = TypeGpu {
            ot: ot.clone(),
            meshes: Vec::new(),
            variants: Vec::new(),
            dynamic_texture_variants: HashMap::new(),
            lods: Vec::new(),
            materials: Vec::new(),
            textures: Vec::new(),
            users: 0,
            auto_night: false,
            lod0_lo: 0.0,
            lod0_max: f32::MAX,
            terrain_slots: Vec::new(),
            terrain_rest: Vec::new(),
        };
        for (mesh, o3d_mats, overrides) in &ot.meshes {
            let mut mats: Vec<MaterialId> = Vec::new();
            for (slot, m) in o3d_mats.iter().enumerate() {
                let tex_of = |gpu: &mut GpuCache,
                              scene: &mut Scene,
                              name: &str,
                              t: &mut TypeGpu|
                              -> Option<TextureId> {
                    let (id, path) = gpu.texture(renderer, scene, name, &dirs, images)?;
                    t.textures.push(path);
                    Some(id)
                };
                let for_slot =
                    |o: &&MaterialDef| ::simulation::vehicle::override_slot(o3d_mats, o) == Some(slot);
                // a slot fed by [useTextTexture] / [useScriptTexture] gets a generated picture:
                // the name in the mesh is a placeholder (the stop poles' Textfeld_1.bmp, the
                // street signs' StrSchild_Text1.bmp), and looking for it on disk only wrote
                // "Did not find texture file" into the log for every map
                let generated = overrides
                    .iter()
                    .filter(|o| !o.item)
                    .filter(for_slot)
                    .any(|o| o.use_text_texture.is_some() || o.use_script_texture.is_some());
                let tex = if is_null_texture(&m.texture) || generated {
                    None
                } else {
                    tex_of(gpu, scene, &m.texture, &mut t)
                };
                let base_ov: Vec<MaterialDef> =
                    overrides.iter().filter(|o| !o.item).cloned().collect();
                let alpha = material_alpha(o3d_mats, slot, &base_ov);
                let night = match overrides
                    .iter()
                    .filter(|o| !o.item && o.nightmap.is_some())
                    .filter(for_slot)
                    .find_map(|o| o.nightmap.clone())
                {
                    Some(n) => tex_of(gpu, scene, &n, &mut t),
                    // OMSI's own night textures: a copy of the texture in the `night` folder
                    // beside it, black but for the lit windows and signs, added at night as
                    // the object's [NightMapMode] says. Every stock building has them (the
                    // Buildings_RW1HH folder alone 60), and without them the city stood dark.
                    None if !is_null_texture(&m.texture) => {
                        let rel = night_texture_name(&m.texture);
                        let dirs_ref: Vec<&Path> = dirs.iter().map(|p| p.as_path()).collect();
                        if night_texture_exists(&rel, &dirs_ref) {
                            t.auto_night = true;
                            tex_of(gpu, scene, &rel, &mut t)
                        } else {
                            None
                        }
                    }
                    None => None,
                };
                let slot_ov: Vec<&MaterialDef> = overrides
                    .iter()
                    .filter(|o| !o.item)
                    .filter(for_slot)
                    .collect();
                let (color, emissive, specular, ambient) =
                    d3d_material(m, slot_ov.iter().find_map(|o| o.allcolor), tex.is_some());
                // [matl_transmap]: transparency from a separate map (parked cars: body opaque, windows clear)
                let transmap = match overrides
                    .iter()
                    .filter(|o| !o.item)
                    .filter(for_slot)
                    .find_map(|o| o.transmap.clone())
                    .filter(|t| !t.trim().is_empty() && !t.trim().starts_with("\\S:"))
                {
                    Some(name) => {
                        let dirs_ref: Vec<&Path> = dirs.iter().map(|p| p.as_path()).collect();
                        match tex_of(gpu, scene, &name, &mut t) {
                            Some(id) => {
                                let has_alpha = ::texture::find_texture(&name, &dirs_ref)
                                    .map(|p| gpu.has_alpha(&p))
                                    .unwrap_or(false);
                                Some((id, has_alpha))
                            }
                            None => None,
                        }
                    }
                    None => None,
                };
                // A separate transmap is a mask, not an automatic instruction to make the
                // whole material transparent. Opaque body panels must stay opaque unless the
                // model's `[matl_alpha]` or a material override explicitly says otherwise.
                let alpha = alpha;
                // [matl_envmap]: the same reflection rule as on vehicles (factor x mask; a
                // texture without an alpha channel reads as a full mask)
                let envmap = match slot_ov.iter().find_map(|o| o.envmap.clone()) {
                    Some((name, f)) => tex_of(gpu, scene, &name, &mut t).map(|id| (id, f)),
                    None => None,
                };
                let env_mask = match slot_ov
                    .iter()
                    .find_map(|o| o.envmap_mask.clone())
                    .filter(|_| envmap.is_some())
                {
                    Some(name) => tex_of(gpu, scene, &name, &mut t),
                    None => None,
                };
                let bump =
                    match slot_ov.iter().find_map(|o| o.bumpmap.clone()).filter(|_| {
                        envmap.is_some() && ::legacy_config::env::var_os("OMSI_NO_BUMP").is_none()
                    }) {
                        Some((name, f)) => {
                            gpu.bump_texture(renderer, scene, &name, &dirs)
                                .map(|(id, key)| {
                                    t.textures.push(key);
                                    (id, f)
                                })
                        }
                        None => None,
                    };
                let mut extra = material_extra(&slot_ov, env_mask, bump, specular);
                extra.ambient = Some(ambient);
                extra.no_map_lights = ot.sco.no_map_lighting;
                if tex.is_some() {
                    let dirs_ref: Vec<&Path> = dirs.iter().map(|p| p.as_path()).collect();
                    let c = self.textures.cfg(&m.texture, &dirs_ref);
                    if c.moisture || c.puddles {
                        extra.moisture = 1.0;
                    }
                    if c.terrain_mapping {
                        t.terrain_slots.push((0, t.meshes.len(), slot));
                    }
                }
                let address = tex_addressing(overrides.iter().filter(for_slot));
                renderer.address_next.set(address);
                renderer.light_map_next.set(ot.sco.light_map_mapping);
                // OMSI_DEBUG_OBJMAT=<part of the object's file name>: how its slots are made
                if let Ok(f) = ::legacy_config::env::var("OMSI_DEBUG_OBJMAT") {
                    if ot
                        .sco
                        .path
                        .to_string_lossy()
                        .to_ascii_lowercase()
                        .contains(&f.to_ascii_lowercase())
                    {
                        log::info!(
                            "{} slot {slot} '{}': tex {} alpha {:?} color {:?} emissive {:?} night {} transmap {:?} envmap {:?} auto_night {}",
                            ot.sco.path.display(),
                            m.texture,
                            tex.is_some(),
                            alpha,
                            color,
                            emissive,
                            night.is_some(),
                            transmap.map(|t| t.1),
                            envmap.map(|e| e.1),
                            t.auto_night
                        );
                    }
                }
                // [matl_lightmap]: laid on the light as on a vehicle (a lamp's lens, a lit
                // shelter or advertising pillar); switched by its variable per placement
                // (`LampSlots`), on where nothing switches it. (Left out, a signal whose
                // lenses are lit by their light maps stayed dark, #826.)
                let light = match slot_ov.iter().find_map(|o| o.lightmap.clone()) {
                    Some((name, _)) => tex_of(gpu, scene, &name, &mut t),
                    None => None,
                };
                let base = renderer.add_material_extra(
                    scene, tex, alpha, color, false, transmap, night, light, envmap, emissive,
                    extra,
                );
                let base = gpu.material(renderer, scene, base);
                t.materials.push(base);
                // variant of a [matl_change]
                let change_var = overrides
                    .iter()
                    .filter(|o| !o.item)
                    .filter(for_slot)
                    .find_map(|o| o.change.as_ref().map(|c| c.2.clone()));
                let items: Vec<&MaterialDef> = overrides
                    .iter()
                    .filter(|o| o.item)
                    .filter(for_slot)
                    .collect();
                if let (Some(var), false) = (change_var, items.is_empty()) {
                    // every [matl_item] of the change is a material of its own (item n shows
                    // at the variable n); the commands after one change that one only
                    let mut item_ids: Vec<MaterialId> = Vec::new();
                    for it in &items {
                        let one: [&MaterialDef; 1] = [*it];
                        let it_night = match one.iter().find_map(|o| o.nightmap.clone()) {
                            Some(n) => tex_of(gpu, scene, &n, &mut t).or(night),
                            None => night,
                        };
                        let (ic, ie, is, ia) = d3d_material(
                            m,
                            one.iter()
                                .find_map(|o| o.allcolor)
                                .or(slot_ov.iter().find_map(|o| o.allcolor)),
                            tex.is_some(),
                        );
                        let it_alpha = one.first().map(|o| alpha_mode(o.alpha)).unwrap_or(alpha);
                        let mut it_extra = material_extra(&one, env_mask, bump, is);
                        it_extra.ambient = Some(ia);
                        it_extra.night_switched = one.iter().any(|o| o.nightmap.is_some());
                        it_extra.no_z_write |= extra.no_z_write;
                        it_extra.no_z_check |= extra.no_z_check;
                        it_extra.glass |= extra.glass;
                        renderer.address_next.set(address);
                        renderer.light_map_next.set(ot.sco.light_map_mapping);
                        let it_light = match one.iter().find_map(|o| o.lightmap.clone()) {
                            Some((name, _)) => tex_of(gpu, scene, &name, &mut t).or(light),
                            None => light,
                        };
                        let item = renderer.add_material_extra(
                            scene, tex, it_alpha, ic, false, transmap, it_night, it_light, envmap,
                            ie, it_extra,
                        );
                        let item = gpu.material(renderer, scene, item);
                        t.materials.push(item);
                        item_ids.push(item);
                    }
                    let item = item_ids[0];
                    let more = item_ids[1..].to_vec();
                    t.variants.push((t.meshes.len(), slot, base, item, var, more));
                }
                mats.push(base);
            }
            let id = gpu.add_mesh(renderer, scene, mesh);
            scene.meshes[id].source = Some(ot.sco.path.display().to_string());
            t.meshes.push((
                id,
                if mats.is_empty() {
                    vec![ground_mat]
                } else {
                    mats
                },
            ));
        }
        // lower LODs (plain materials). OMSI picks a level the way the model lists them
        // (Omsi.exe 0x5ef860): the first whose least size the object's screen size reaches,
        // else the last one whatever its own. A level is so drawn from its least size (the
        // last from 0) up to the least of the sizes listed before it. The stock Sv signals
        // say [LOD] 0.1 (the signal) before [LOD] 1 (its low version): taken as size bands
        // the low version stood in close up and the signal vanished in the distance; and a
        // model with a single [LOD] 0.5 is drawn at any size.
        let mins: Vec<f32> = std::iter::once(ot.lod0_min)
            .chain(ot.lower_lods.iter().map(|l| l.0))
            .collect();
        let band = |i: usize| -> (f32, f32) {
            let lo = if i + 1 == mins.len() { 0.0 } else { mins[i] };
            (lo, mins[..i].iter().copied().fold(f32::MAX, f32::min))
        };
        (t.lod0_lo, t.lod0_max) = band(0);
        for (k, (_, meshes)) in ot.lower_lods.iter().enumerate() {
            let (lo, upper) = band(k + 1);
            let mut l = Vec::new();
            for (mesh, o3d_mats, overrides) in meshes {
                let mut mats = Vec::new();
                for (slot, m) in o3d_mats.iter().enumerate() {
                    // (a text or script texture slot's name is a placeholder, see above)
                    let generated = overrides.iter().any(|o| {
                        !o.item
                            && ::simulation::vehicle::override_slot(o3d_mats, o) == Some(slot)
                            && (o.use_text_texture.is_some() || o.use_script_texture.is_some())
                    });
                    let tex = if is_null_texture(&m.texture) || generated {
                        None
                    } else {
                        match gpu.texture(renderer, scene, &m.texture, &dirs, images) {
                            Some((id, path)) => {
                                t.textures.push(path);
                                Some(id)
                            }
                            None => None,
                        }
                    };
                    if tex.is_some() {
                        let dirs_ref: Vec<&Path> = dirs.iter().map(|p| p.as_path()).collect();
                        if self.textures.cfg(&m.texture, &dirs_ref).terrain_mapping {
                            t.terrain_slots.push((k + 1, l.len(), slot));
                        }
                    }
                    let mat = renderer.add_material_night(
                        scene,
                        tex,
                        material_alpha(o3d_mats, slot, overrides),
                        [1.0; 4],
                        false,
                        None,
                        None,
                    );
                    let mat = gpu.material(renderer, scene, mat);
                    t.materials.push(mat);
                    mats.push(mat);
                }
                let id = gpu.add_mesh(renderer, scene, mesh);
                scene.meshes[id].source = Some(ot.sco.path.display().to_string());
                l.push((
                    id,
                    if mats.is_empty() {
                        vec![ground_mat]
                    } else {
                        mats
                    },
                ));
            }
            t.lods.push((lo, upper, l));
        }
        gpu.types.insert(key, t);
        key
    }

    /// Put a prepared tile on the GPU in one go.
    pub fn upload_tile(
        &self,
        renderer: &Renderer,
        scene: &mut Scene,
        p: Prepared,
        stats: &mut LoadStats,
    ) {
        let mut u = self.begin_upload(p);
        while !self.upload_step(renderer, scene, &mut u, None) {}
        self.finish_upload(renderer, scene, u, stats);
    }

    /// Give up on a tile on its way in (it went out of range): what it already has on the
    /// GPU - textures and object types it holds, instances placed so far - goes back.
    pub fn abandon_upload(
        &self,
        renderer: &Renderer,
        scene: &mut Scene,
        u: PendingUpload,
        audio: Option<&::audio::AudioEngine>,
    ) {
        let key = u.key();
        let PendingUpload { tg, placing, .. } = u;
        let orphan = {
            let mut states = self.tile_state.lock();
            match states.get_mut(&key) {
                Some(state) => {
                    state.gpu = tg;
                    state.poles = placing.poles;
                    None
                }
                None => Some((tg, placing.poles)),
            }
        };
        let mut freed = false;
        if let Some((tg, poles)) = orphan {
            self.poles.lock().retain(|k, _| !poles.contains(k));
            self.parked_objects.lock().retain(|_, p| p.tile != key);
            self.departed_objects
                .lock()
                .retain(|_, (p, _, _)| p.tile != key);
            self.edit_objects.lock().retain(|_, o| o.tile != key);
            freed = self.gpu.lock().release_tile(renderer, scene, tg) > 0;
        }
        freed |= self.unload_tile(renderer, scene, key, audio);
        if freed {
            self.trim_object_types();
        }
    }

    /// Start putting a prepared tile on the GPU: what it needs that is not there yet (the
    /// textures decoded for it, its new object types) goes up in steps first.
    pub fn begin_upload(&self, p: Prepared) -> PendingUpload {
        let gpu = self.gpu.lock();
        let mut textures: Vec<PathBuf> = p
            .images
            .keys()
            .filter(|k| !gpu.textures.contains_key(*k))
            .cloned()
            .collect();
        textures.sort();
        let mut types: Vec<Arc<ObjectType>> = Vec::new();
        let mut seen: hashbrown::HashSet<usize> = hashbrown::HashSet::new();
        for o in &p.objects {
            let key = Arc::as_ptr(&o.ot) as usize;
            if !gpu.types.contains_key(&key) && seen.insert(key) {
                types.push(o.ot.clone());
            }
        }
        // the objects are placed from the back of the list: in the order of the tile
        let mut p = p;
        p.objects.reverse();
        PendingUpload {
            prepared: p,
            textures,
            types,
            tg: TileGpu::default(),
            placing: Placing::default(),
        }
    }

    /// Upload one texture or one object type after the other until `deadline` (just one
    /// with no deadline). True when only the tile itself is left to place.
    pub fn upload_step(
        &self,
        renderer: &Renderer,
        scene: &mut Scene,
        u: &mut PendingUpload,
        deadline: Option<std::time::Instant>,
    ) -> bool {
        let mut gpu_guard = self.gpu.lock();
        let gpu = &mut *gpu_guard;
        self.ensure_ground(renderer, scene, gpu);
        let ground_mat = gpu.ground.as_ref().unwrap().ground_mat;
        let slow = ::legacy_config::env::var_os("OMSI_DEBUG_UPLOAD").is_some();
        loop {
            let t_item = std::time::Instant::now();
            if let Some(path) = u.textures.pop() {
                if !gpu.textures.contains_key(&path) {
                    if let Some(img) = u.prepared.images.get(&path) {
                        let id = gpu.add_data(renderer, scene, img);
                        // (the PBR maps beside it: only the textures decoded on the spot had
                        // them, the ones the tile's preparation brought - nearly all of a
                        // map's - were drawn flat)
                        if !path.to_string_lossy().ends_with("#bump") {
                            attach_pbr(renderer, scene, &path, id);
                        }
                        gpu.textures.insert(
                            path.clone(),
                            TexEntry {
                                id,
                                alpha: img.has_alpha,
                                users: 1,
                                texels: img.width as u64 * img.height as u64,
                                bytes: renderer.texture_size_bytes(scene, id),
                                format: img.format,
                                dropped: 0,
                            },
                        );
                        // the tile holds the texture until its object types take it over
                        if slow && t_item.elapsed().as_millis() > 8 {
                            log::info!(
                                "upload: texture {} ({}x{} {:?}) took {} ms",
                                path.display(),
                                img.width,
                                img.height,
                                img.format,
                                t_item.elapsed().as_millis()
                            );
                        }
                        u.tg.shared_textures.push(path);
                    }
                }
            } else if let Some(ot) = u.types.pop() {
                let before = (gpu.sync_decodes, gpu.sync_decode_secs);
                let key = self.type_gpu(renderer, scene, gpu, &ot, &u.prepared.images, ground_mat);
                if slow && t_item.elapsed().as_millis() > 8 {
                    log::info!(
                        "upload: object type {} took {} ms ({} meshes, {} textures decoded here in {:.0} ms)",
                        ot.model_dir.display(),
                        t_item.elapsed().as_millis(),
                        ot.meshes.len(),
                        gpu.sync_decodes - before.0,
                        (gpu.sync_decode_secs - before.1) * 1000.0
                    );
                }
                if !u.tg.types.contains(&key) {
                    gpu.types.get_mut(&key).unwrap().users += 1;
                    u.tg.types.push(key);
                }
            } else {
                return true;
            }
            match deadline {
                Some(d) if std::time::Instant::now() < d => {}
                _ => return u.textures.is_empty() && u.types.is_empty(),
            }
        }
    }

    /// Place a tile whose textures and object types are on the GPU, in one go (see
    /// [`World::place_step`]).
    pub fn finish_upload(
        &self,
        renderer: &Renderer,
        scene: &mut Scene,
        mut u: PendingUpload,
        stats: &mut LoadStats,
    ) {
        while !self.place_step(renderer, scene, &mut u, None) {}
        self.commit_upload(u, stats);
    }

    /// Hand a placed tile's records to its [`TileState`], so that [`World::unload_tile`] can
    /// give everything back.
    pub fn commit_upload(&self, u: PendingUpload, stats: &mut LoadStats) {
        let key = u.key();
        let PendingUpload { tg, placing, .. } = u;
        stats.splines += placing.splines;
        stats.trees += placing.trees;
        stats.objects += placing.objects;
        {
            let gpu = self.gpu.lock();
            stats.object_types = gpu.types.len();
            stats.spline_types = gpu.splines.len();
        }
        let mut states = self.tile_state.lock();
        let state = states.entry(key).or_default();
        state.gpu = tg;
        state.night_slots = placing.night_slots;
        state.night_modes = placing.night_modes;
        state.light_objects = placing.light_objects;
        state.poles = placing.poles;
    }
}
