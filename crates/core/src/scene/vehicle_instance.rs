use super::*;

pub(super) fn vehicle_texture_names(
    root: &Path,
    vt: &::simulation::VehicleType,
    scheme: Option<usize>,
) -> Vec<(String, Vec<PathBuf>)> {
    let mut dirs = vt.texture_dirs(root);
    let (subst, scheme_dir) = match scheme {
        Some(i) => vt.scheme_substitutions(i),
        None => (vt.default_substitutions(root), None),
    };
    if let Some(d) = scheme_dir {
        dirs.insert(0, d);
    }
    let subst = |name: &str| -> String {
        subst
            .get(&name.to_ascii_lowercase())
            .cloned()
            .unwrap_or_else(|| name.to_string())
    };
    let mut out: Vec<(String, Vec<PathBuf>)> = Vec::new();
    let mut push = |name: String, d: &Vec<PathBuf>| {
        let name = name.trim().to_string();
        if !name.is_empty() && !name.starts_with("\\S:") && mirror_index(&name).is_none() {
            out.push((name, d.clone()));
        }
    };
    for vm in &vt.meshes {
        for m in &vm.materials {
            match vt.texchange(&m.texture) {
                Some(master) => {
                    let mut edirs = vec![master.dir.clone()];
                    edirs.extend(dirs.iter().cloned());
                    for e in &master.entries {
                        push(subst(e), &edirs);
                    }
                }
                None => push(subst(&m.texture), &dirs),
            }
        }
        for o in &vm.overrides {
            for name in [
                o.nightmap.clone(),
                o.transmap.clone().map(|t| subst(&t)),
                o.lightmap.clone().map(|l| l.0),
                o.envmap.clone().map(|e| e.0),
                o.envmap_mask
                    .clone()
                    .filter(|_| o.envmap.is_some())
                    .map(|t| subst(&t)),
            ]
                .into_iter()
                .flatten()
            {
                push(name, &dirs);
            }
        }
    }
    out.sort();
    out.dedup();
    out
}

pub(super) fn vehicle_bump_names(
    root: &Path,
    vt: &::simulation::VehicleType,
    scheme: Option<usize>,
) -> Vec<(String, Vec<PathBuf>)> {
    if ::legacy_config::env::var_os("OMSI_NO_BUMP").is_some()
        || ::legacy_config::env::var_os("OMSI_NO_ENVMAP").is_some()
    {
        return Vec::new();
    }
    let mut dirs = vt.texture_dirs(root);
    let (subst, scheme_dir) = match scheme {
        Some(i) => vt.scheme_substitutions(i),
        None => (vt.default_substitutions(root), None),
    };
    if let Some(d) = scheme_dir {
        dirs.insert(0, d);
    }
    let mut out: Vec<(String, Vec<PathBuf>)> = Vec::new();
    for vm in &vt.meshes {
        for o in vm.overrides.iter().filter(|o| o.envmap.is_some()) {
            if let Some((t, _)) = &o.bumpmap {
                let name = subst
                    .get(&t.to_ascii_lowercase())
                    .cloned()
                    .unwrap_or_else(|| t.clone());
                if !name.trim().is_empty() {
                    out.push((name.trim().to_string(), dirs.clone()));
                }
            }
        }
    }
    out.sort();
    out.dedup();
    out
}

impl World {
    pub(super) fn vehicle_texture(
        &self,
        renderer: &Renderer,
        scene: &mut Scene,
        tex_ids: &mut HashMap<PathBuf, (TextureId, usize)>,
        held: &mut Vec<PathBuf>,
        name: &str,
        dirs: &[&Path],
    ) -> Option<TextureId> {
        let path = ::texture::find_texture(name, dirs)?;
        if let Some(e) = tex_ids.get_mut(&path) {
            self.textures.release(&path);
            self.vehicle_ready.lock().textures.remove(&path);
            if !held.contains(&path) {
                e.1 += 1;
                held.push(path);
            }
            return Some(e.0);
        }
        let prepared = self.vehicle_ready.lock().textures.remove(&path);
        if let Some((t, format)) = prepared {
            let id = {
                let mut gpu = self.gpu.lock();
                let id = renderer.add_prepared_texture(scene, t);
                gpu.take_texture_slot(renderer, scene, id)
            };
            if ::legacy_config::env::var_os("OMSI_DEBUG_TEXTURES").is_some() {
                log::info!(
                    "vehicle texture {} (made ahead) {:?}, {:.2} MB",
                    path.display(),
                    format,
                    scene.texture_bytes_of(id) as f64 / 1e6
                );
            }
            attach_pbr(renderer, scene, &path, id);
            tex_ids.insert(path.clone(), (id, 1));
            held.push(path);
            return Some(id);
        }
        let fast = self.gpu.lock().fast_loads;
        let (img, worth) = if fast {
            self.textures.get_gpu_fast(&path)?
        } else {
            (self.textures.get_gpu_path(&path)?, false)
        };
        let id = {
            let mut gpu = self.gpu.lock();
            let id = gpu.add_data(renderer, scene, &img);
            if worth {
                gpu.wants_upgrade.push(path.clone());
            }
            id
        };
        if ::legacy_config::env::var_os("OMSI_DEBUG_TEXTURES").is_some() {
            log::info!(
                "vehicle texture {} {}x{} {:?} {} levels, {:.2} MB",
                path.display(),
                img.width,
                img.height,
                img.format,
                img.levels.len(),
                scene.texture_bytes_of(id) as f64 / 1e6
            );
        }
        self.textures.release(&path);
        attach_pbr(renderer, scene, &path, id);
        tex_ids.insert(path.clone(), (id, 1));
        held.push(path);
        Some(id)
    }

    pub(super) fn snow_glass_texture(
        &self,
        renderer: &Renderer,
        scene: &mut Scene,
        tex_ids: &mut HashMap<PathBuf, (TextureId, usize)>,
        held: &mut Vec<PathBuf>,
        _name: &str,
        _dirs: &[&Path],
    ) -> Option<TextureId> {
        let key = PathBuf::from("<snow on glass>");
        if let Some(e) = tex_ids.get_mut(&key) {
            if !held.contains(&key) {
                e.1 += 1;
                held.push(key);
            }
            return Some(e.0);
        }
        let img = crate::rain::snow_on_glass(&self.root);
        let id = renderer.add_texture(scene, &img, false);
        tex_ids.insert(key.clone(), (id, 1));
        held.push(key);
        Some(id)
    }

    pub(super) fn vehicle_bump_texture(
        &self,
        renderer: &Renderer,
        scene: &mut Scene,
        tex_ids: &mut HashMap<PathBuf, (TextureId, usize)>,
        held: &mut Vec<PathBuf>,
        name: &str,
        dirs: &[&Path],
    ) -> Option<TextureId> {
        let key = bump_key(&::texture::find_texture(name, dirs)?);
        if let Some(e) = tex_ids.get_mut(&key) {
            self.vehicle_ready.lock().textures.remove(&key);
            if !held.contains(&key) {
                e.1 += 1;
                held.push(key);
            }
            return Some(e.0);
        }
        let prepared = self.vehicle_ready.lock().textures.remove(&key);
        let id = match prepared {
            Some((t, _)) => {
                let mut gpu = self.gpu.lock();
                let id = renderer.add_prepared_texture(scene, t);
                gpu.take_texture_slot(renderer, scene, id)
            }
            None => {
                let fast = self.gpu.lock().fast_loads;
                let data = load_texture_key(&key, !fast)?;
                self.gpu.lock().add_data(renderer, scene, &data)
            }
        };
        if ::legacy_config::env::var_os("OMSI_DEBUG_TEXTURES").is_some() {
            log::info!(
                "vehicle bump map {}, {:.2} MB",
                key.display(),
                scene.texture_bytes_of(id) as f64 / 1e6
            );
        }
        tex_ids.insert(key.clone(), (id, 1));
        held.push(key);
        Some(id)
    }

    pub(super) fn vehicle_mesh(
        &self,
        renderer: &Renderer,
        scene: &mut Scene,
        mesh_ids: &mut HashMap<(PathBuf, usize), (MeshId, usize)>,
        keys: &mut Vec<(PathBuf, usize)>,
        vt: &::simulation::VehicleType,
        i: usize,
    ) -> MeshId {
        let key = (vt.def.path.clone(), i);
        if let Some(e) = mesh_ids.get_mut(&key) {
            self.vehicle_ready.lock().meshes.remove(&key);
            if !keys.contains(&key) {
                e.1 += 1;
                keys.push(key);
            }
            return e.0;
        }
        let pre = self.vehicle_ready.lock().meshes.remove(&key);
        let id = match pre {
            Some(m) => {
                let mut gpu = self.gpu.lock();
                let id = renderer.add_prepared_mesh(scene, m);
                gpu.take_mesh_slot(renderer, scene, id)
            }
            None => {
                let data = vt.mesh_data(i);
                let empty = MeshData {
                    ranges: vt.meshes[i].data.ranges.clone(),
                    ..Default::default()
                };
                let mut gpu = self.gpu.lock();
                gpu.add_mesh(renderer, scene, data.as_deref().unwrap_or(&empty))
            }
        };
        mesh_ids.insert(key.clone(), (id, 1));
        scene.meshes[id].source = Some(vt.def.path.display().to_string());
        keys.push(key);
        id
    }

    pub(super) fn recycle_pair(
        &self,
        renderer: &Renderer,
        scene: &mut Scene,
        pair: (MaterialId, MaterialId),
    ) -> (MaterialId, MaterialId) {
        let mut gpu = self.gpu.lock();
        if pair.0 == pair.1 {
            let m = gpu.material(renderer, scene, pair.0);
            (m, m)
        } else {
            let item = gpu.material(renderer, scene, pair.1);
            let base = gpu.material(renderer, scene, pair.0);
            (base, item)
        }
    }

    #[allow(clippy::too_many_arguments)]
    pub(super) fn instantiate_vehicle(
        &self,
        renderer: &Renderer,
        scene: &mut Scene,
        vt: &::simulation::VehicleType,
        set: &VehicleSet,
        key: Option<VehicleKey>,
        shared_script: Option<&[Option<TextureId>]>,
    ) -> VehicleRender {
        let mut gpu = self.gpu.lock();
        let blank = |gpu: &mut GpuCache, scene: &mut Scene, w: i32, h: i32| {
            Some(gpu.add_blank(renderer, scene, w.max(1) as u32, h.max(1) as u32))
        };
        let blank_text = |gpu: &mut GpuCache, scene: &mut Scene, w: i32, h: i32| {
            Some(gpu.add_blank_mips(renderer, scene, w.max(1) as u32, h.max(1) as u32))
        };
        let sizes: Vec<(i32, i32)> = vt
            .model
            .text_textures
            .iter()
            .map(|t| (t.width, t.height))
            .collect();
        let text_textures: Vec<Option<TextureId>> = sizes
            .iter()
            .map(|(w, h)| blank_text(&mut gpu, scene, *w, *h))
            .collect();
        let script_textures: Vec<Option<TextureId>> = match shared_script {
            Some(s) => s.to_vec(),
            None => vt
                .model
                .script_textures
                .iter()
                .map(|(w, h)| blank(&mut gpu, scene, *w, *h))
                .collect(),
        };
        let mut instances = Vec::new();
        let mut own_materials = Vec::new();
        let mut variants = set.variants.to_vec();
        for (mi, (id, mats)) in set.meshes.iter().enumerate() {
            let mut mats = mats.clone();
            for v in variants
                .iter_mut()
                .filter(|v| v.mesh == mi && v.spec.per_vehicle())
            {
                let spec = v.spec.for_vehicle(&text_textures, &script_textures);
                let make = |gpu: &mut GpuCache, scene: &mut Scene, tex: Option<TextureId>| {
                    let (base, item) = spec.build(renderer, scene, tex);
                    if base == item {
                        let m = gpu.material(renderer, scene, base);
                        (m, m)
                    } else {
                        let item = gpu.material(renderer, scene, item);
                        (gpu.material(renderer, scene, base), item)
                    }
                };
                let (base, item) = make(&mut gpu, scene, v.base_tex);
                let entries: Vec<(MaterialId, MaterialId)> = v
                    .entry_tex
                    .iter()
                    .map(|t| make(&mut gpu, scene, *t))
                    .collect();
                own_materials.extend([base, item]);
                own_materials.extend(entries.iter().flat_map(|e| [e.0, e.1]));
                let more = spec.build_more(renderer, scene, v.base_tex, |scene, m| {
                    gpu.material(renderer, scene, m)
                });
                own_materials.extend(more.iter().copied());
                (v.base, v.item, v.more, v.entries, v.spec) = (base, item, more, entries, spec);
                if let Some(l) = &mut v.lights {
                    l.plain = (base, item);
                }
                if let Some(x) = mats.get_mut(v.slot) {
                    *x = v.entries.first().map(|e| e.0).unwrap_or(v.base);
                }
            }
            for d in set.dyn_slots.iter().filter(|d| d.mesh == mi) {
                let Some(x) = mats.get_mut(d.slot) else {
                    continue;
                };
                if let Some(Some(tex)) = d.text.and_then(|i| text_textures.get(i)) {
                    renderer.address_next.set(d.address);
                    let mut extra = d.extra;
                    extra.display = text_is_display(d.lightmap.is_some(), d.night.is_some());
                    extra.screen = true;
                    let m = renderer.add_material_extra(
                        scene,
                        Some(*tex),
                        AlphaMode::Blend,
                        [1.0; 4],
                        false,
                        None,
                        d.night,
                        d.lightmap,
                        None,
                        [0.0; 3],
                        extra,
                    );
                    *x = gpu.material(renderer, scene, m);
                    own_materials.push(*x);
                    continue;
                }
                let tex = d
                    .script
                    .and_then(|i| script_textures.get(i).copied().flatten())
                    .or(d.tex);
                let transmap = d
                    .script_trans
                    .and_then(|i| script_textures.get(i).copied().flatten())
                    .map(|t| (t, true))
                    .or(d.transmap);
                let alpha = d.alpha;
                let (color, emissive) = if d.script.is_some() {
                    ([1.0; 4], [0.0; 3])
                } else {
                    (d.color, d.emissive)
                };
                renderer.address_next.set(d.address);
                let mut extra = d.extra;
                extra.screen = d.script.is_some() || d.script_trans.is_some();
                extra.led = d.script_trans.is_some() && d.extra.led;
                let m = renderer.add_material_extra(
                    scene,
                    tex,
                    alpha,
                    color,
                    d.script.is_some(),
                    transmap,
                    if d.script.is_some() { None } else { d.night },
                    if d.script.is_some() { None } else { d.lightmap },
                    if d.script.is_some() { None } else { d.envmap },
                    emissive,
                    extra,
                );
                *x = gpu.material(renderer, scene, m);
                own_materials.push(*x);
            }
            let shadow = vt
                .meshes
                .get(mi)
                .map(|m| vt.model.meshes[m.def_index].is_shadow)
                .unwrap_or(false);
            let inst = if shadow {
                renderer.add_shadow_blob_instance(scene, *id, DVec3::ZERO, Mat4::IDENTITY, mats)
            } else {
                let i = renderer.add_instance(scene, *id, DVec3::ZERO, Mat4::IDENTITY, mats);
                let casts = vt
                    .meshes
                    .get(mi)
                    .map(|m| vt.model.meshes[m.def_index].shadow)
                    .unwrap_or(false);
                renderer.set_omsi_caster(scene, i, casts);
                renderer.set_roof(scene, i, vt.def.bounding_box.map(|b| b[5] + b[2] * 0.5));
                i
            };
            instances.push(if key.is_some() {
                gpu.instance(renderer, scene, inst)
            } else {
                inst
            });
        }
        let slots_in_order = |i: usize| -> Vec<(::render::AlphaMode, bool)> {
            let Some(inst) = scene.instances.get(i) else {
                return Vec::new();
            };
            let Some(mesh) = scene.meshes.get(inst.mesh) else {
                return Vec::new();
            };
            mesh.ranges
                .iter()
                .filter_map(|(_, _, slot)| inst.materials.get(*slot as usize))
                .filter_map(|&m| scene.materials.get(m))
                .map(|m| (m.alpha, !m.no_z_write && !m.no_z_check))
                .collect()
        };
        let mut blended_first = false;
        let mut ordered = false;
        for &i in &instances {
            if scene.instances.get(i).is_none_or(|x| x.blob) {
                continue;
            }
            for (alpha, writes) in slots_in_order(i) {
                match alpha {
                    ::render::AlphaMode::Blend if writes => blended_first = true,
                    ::render::AlphaMode::Blend => {}
                    _ if blended_first => ordered = true,
                    _ => {}
                }
            }
        }
        if ordered && key.is_none() && ::legacy_config::env::var_os("OMSI_NO_MODEL_ORDER").is_none() {
            log::debug!(
                "{}: drawn in model order (a blended slot writes depth before an opaque one)",
                vt.def.path.display()
            );
            for &i in &instances {
                if scene.instances.get(i).is_some_and(|x| !x.blob) {
                    renderer.set_ordered(scene, i, true);
                }
            }
        }
        let radius = set
            .meshes
            .iter()
            .filter_map(|(id, _)| scene.meshes.get(*id))
            .filter(|m| m.bounds_radius > 0.0)
            .map(|m| m.bounds_center.length() + m.bounds_radius)
            .fold(0.0f32, f32::max);
        let any_distance =
            vt.model.no_distance_check || vt.model.meshes.iter().any(|m| m.no_distance_check);
        for inst in &instances {
            renderer.set_object_culling(scene, *inst, radius, vt.model.detail_factor, any_distance);
        }
        VehicleRender {
            window_wipers: None,
            instances,
            text_textures,
            script_textures,
            shared_script: shared_script.is_some(),
            variants,
            own_materials,
            set: key,
            displays_far: false,
            display_tick: 0,
            skinned: Vec::new(),
            hidden: false,
            interior_lamps: std::cell::Cell::new(None),
            interior_blocks: std::cell::OnceCell::new(),
        }
    }

    pub(super) fn upload_vehicle(
        &self,
        renderer: &Renderer,
        scene: &mut Scene,
        vt: &::simulation::VehicleType,
        scheme: Option<usize>,
        mirror_base: usize,
    ) -> VehicleSet {
        let mut dyn_slots: Vec<DynSlot> = Vec::new();
        let mut variants: Vec<VariantSlot> = Vec::new();
        let mut dirs = vt.texture_dirs(&self.root);
        let (subst, scheme_dir) = match scheme {
            Some(i) => vt.scheme_substitutions(i),
            None => (vt.default_substitutions(&self.root), None),
        };
        if let Some(d) = scheme_dir {
            dirs.insert(0, d);
        }
        let subst = |name: &str| -> String {
            subst
                .get(&name.to_ascii_lowercase())
                .cloned()
                .unwrap_or_else(|| name.to_string())
        };
        let dirs_ref: Vec<&Path> = dirs.iter().map(|p| p.as_path()).collect();
        let mut tex_ids = self.vehicle_textures.lock();
        let mut mesh_ids = self.vehicle_meshes.lock();
        let mut held: Vec<PathBuf> = Vec::new();
        let mut mesh_keys: Vec<(PathBuf, usize)> = Vec::new();
        let mut materials: Vec<MaterialId> = Vec::new();
        let t_all = std::time::Instant::now();
        let mut mesh_secs = 0.0f64;
        let tex_time = std::cell::RefCell::new((0usize, 0.0f64));
        macro_rules! tex {
            ($name:expr, $dirs:expr) => {{
                let nm: &str = &$name;
                match mirror_index(nm) {
                    Some(mi) => Some(self.mirror_texture(renderer, scene, mi + mirror_base)),
                    None => tex!(nm, $dirs, vehicle_texture),
                }
            }};
            ($name:expr, $dirs:expr, $how:ident) => {{
                let t = std::time::Instant::now();
                let n = tex_ids.len();
                let r = self.$how(renderer, scene, &mut tex_ids, &mut held, $name, $dirs);
                if tex_ids.len() > n {
                    let mut tt = tex_time.borrow_mut();
                    tt.0 += 1;
                    tt.1 += t.elapsed().as_secs_f64();
                }
                r
            }};
        }
        let mut instances = Vec::new();
        let mut missing_tex: Vec<String> = Vec::new();
        let only = ::legacy_config::env::var("OMSI_ONLY_MESH").ok();
        let hide = ::legacy_config::env::var("OMSI_HIDE_MESH").ok();
        let matches = |list: &str, file: &str| {
            list.split('|').any(|f| {
                !f.is_empty() && file.to_ascii_lowercase().contains(&f.to_ascii_lowercase())
            })
        };
        for (mesh_index, vm) in vt.meshes.iter().enumerate() {
            let def = &vt.model.meshes[vm.def_index];
            if only.as_deref().is_some_and(|f| !matches(f, &def.file))
                || hide.as_deref().is_some_and(|f| matches(f, &def.file))
            {
                let id = renderer.add_mesh(scene, &MeshData::default());
                instances.push((id, vec![]));
                continue;
            }
            let mats: Vec<MaterialId> = vm
                .materials
                .iter()
                .enumerate()
                .map(|(slot, m)| {
                    let of_slot = |o: &&MaterialDef| ::simulation::vehicle::override_slot(&vm.materials, o) == Some(slot);
                    let itemised = def.materials.iter().filter(of_slot).any(|o| o.item) && def.materials.iter().filter(of_slot).any(|o| !o.item && o.change.is_some());
                    let text_of = |item: Option<bool>| def.materials.iter().filter(of_slot).find(|o| o.use_text_texture.is_some() && item.is_none_or(|i| o.item == i)).map(|o| o.use_text_texture.unwrap().max(0) as usize);
                    let script_of = |item: Option<bool>| def.materials.iter().filter(of_slot).find(|o| o.use_script_texture.is_some() && item.is_none_or(|i| o.item == i)).map(|o| o.use_script_texture.unwrap().max(0) as usize);
                    let (text_slot, script_slot) = (text_of(None), script_of(None));
                    let (text_base, script_base) = if itemised { (text_of(Some(false)), script_of(Some(false))) } else { (text_slot, script_slot) };
                    let (text_item, script_item) = (text_of(Some(true)).or(text_base), script_of(Some(true)).or(script_base));
                    let tex_name = subst(&m.texture);
                    let rain_layer = vm.overrides.iter().filter(of_slot).any(|o| o.alphascale.as_deref().is_some_and(|v| v.trim().to_ascii_lowercase().starts_with("rain_window")));
                    let tex = if is_null_texture(&m.texture) || text_base.is_some() || script_base.is_some() || vt.texchange(&m.texture).is_some() {
                        None
                    } else if let Some(mi) = mirror_index(&tex_name) {
                        Some(self.mirror_texture(renderer, scene, mi + mirror_base))
                    } else if rain_layer && snowing() && !seasonal_texture(&tex_name, &dirs_ref) {
                        tex!("", &dirs_ref, snow_glass_texture)
                    } else {
                        tex!(&tex_name, &dirs_ref)
                    };
                    let ov_all: Vec<&MaterialDef> = vm.overrides.iter().filter(|o| ::simulation::vehicle::override_slot(&vm.materials, o) == Some(slot)).collect();
                    let ov_item: Vec<&MaterialDef> = ov_all.iter().copied().filter(|o| o.item).collect();
                    let ov: Vec<&MaterialDef> = ov_all.iter().copied().filter(|o| !o.item).collect();
                    let change_vars: Vec<String> = ov.iter().filter_map(|o| o.change.as_ref().map(|c| c.2.clone())).collect();
                    let change_var = change_vars.first().cloned();
                    let base_overrides: Vec<MaterialDef> = ov.iter().map(|o| (*o).clone()).collect();
                    let mut alpha = material_alpha(&vm.materials, slot, &base_overrides);
                    let declared_alpha = alpha;
                    let dirt_overlay = ov.iter().any(|o| o.alphascale.as_deref().is_some_and(|v| matches!(v.trim().to_ascii_lowercase().as_str(), "dirt_norm" | "dirt_wiped")));
                    if dirt_overlay {
                        alpha = AlphaMode::Blend;
                    }
                    if def.is_shadow {
                        alpha = AlphaMode::Blend;
                    }
                    let script_trans = ov.iter().find_map(|o| o.transmap.clone()).and_then(|t| t.trim().strip_prefix("\\S:").and_then(|n| n.trim().parse::<usize>().ok()));
                    let transmap = ov.iter().find_map(|o| o.transmap.clone()).filter(|t| !t.trim().is_empty() && !t.trim().starts_with("\\S:")).map(|t| subst(&t)).and_then(|t| {
                        let id = tex!(&t, &dirs_ref)?;
                        let has_alpha = self.textures.has_alpha(&t, &dirs_ref).unwrap_or(false);
                        Some((id, has_alpha))
                    });
                    let mesh_name = def.file.to_ascii_lowercase();
                    let transparent_layer_name = ["regen", "dreck", "dirt", "folie"];
                    let material_name = format!("{} {}", mesh_name, m.texture).to_ascii_lowercase();
                    let named_pane = GLASS_WORDS
                        .iter()
                        .chain(transparent_layer_name.iter())
                        .any(|part| material_name.contains(part));
                    let see_through = !named_pane
                        && declared_alpha == AlphaMode::Blend
                        && ::texture::find_texture(&subst(&m.texture), &dirs_ref).and_then(|p| alpha_mask(&p)).is_some_and(|mask| slot_is_see_through(&vm.data, slot, &mask));
                    if see_through {
                        log::debug!("  {} slot {slot} '{}': see-through by its texture's alpha, writes no depth", def.file, m.texture);
                    }
                    let transparent_layer_hint = named_pane;
                    let named_body = ["body", "wagenkasten", "karos", "chassis", "kuzov"].iter().any(|part| mesh_name.contains(part));
                    let mesh_has_overlay = def.materials.iter().any(|o| o.no_z_write);
                    let body_hint = (named_body || ov.iter().any(|o| o.bumpmap.is_some()) || !mesh_has_overlay)
                        && material_has_vehicle_volume(&vm.data, slot);
                    let layer = vt.mesh_boxes.get(mesh_index).is_some_and(|&(lo, hi)| {
                        (hi - lo).max_element() > 0.5
                            && vt.mesh_boxes[..mesh_index].iter().any(|&(l2, h2)| (l2 - lo).abs().max_element() < 0.03 && (h2 - hi).abs().max_element() < 0.03)
                    });
                    let repair_body_depth = ::legacy_config::env::var_os("OMSI_REPAIR_BODY_DEPTH").is_some() && !layer && is_vehicle_body_material(&def.file, &m.texture, tex.is_some(), transmap.is_some(), ov.iter().any(|o| o.no_z_write), body_hint);
                    if repair_body_depth && alpha == AlphaMode::Blend && !dirt_overlay && !transparent_layer_hint && !slot_overlays_another(&vm.data, slot) {
                        alpha = AlphaMode::Opaque;
                    }
                    if transparent_layer_hint && !dirt_overlay && declared_alpha == AlphaMode::Blend {
                        alpha = AlphaMode::Blend;
                    }
                    if ::legacy_config::env::var_os("OMSI_FORCE_OPAQUE").is_some() && !dirt_overlay {
                        alpha = AlphaMode::Opaque;
                    }
                    let night = ov.iter().rev().find_map(|o| o.nightmap.clone()).and_then(|t| {
                        tex!(&t, &dirs_ref)
                    });
                    let lightmap = ov.iter().rev().find_map(|o| o.lightmap.clone()).and_then(|(t, _)| {
                        tex!(&t, &dirs_ref)
                    });
                    let html_page = |n: Option<usize>| n.is_some_and(|n| vt.model.html_textures.iter().any(|d| d.script_index == n));
                    let lm_white = |ov: &[&MaterialDef]| -> bool {
                        let named = ov.iter().filter_map(|o| o.lightmap.as_ref().map(|l| l.0.as_str())).chain(std::iter::once(m.texture.as_str())).any(is_led_name);
                        named && ov.iter().rev().find_map(|o| o.lightmap.as_ref()).and_then(|(t, _)| lightmap_is_white(t, &dirs_ref)).unwrap_or(true)
                    };
                    let envmap = ov.iter().find_map(|o| o.envmap.clone()).filter(|_| ::legacy_config::env::var_os("OMSI_NO_ENVMAP").is_none()).and_then(|(t, f)| {
                        let id = tex!(&t, &dirs_ref)?;
                        Some((id, f))
                    });
                    let env_mask = ov.iter().find_map(|o| o.envmap_mask.clone()).filter(|t| envmap.is_some() && !t.trim().is_empty()).and_then(|t| tex!(&subst(&t), &dirs_ref));
                    let bump = ov.iter().find_map(|o| o.bumpmap.clone()).filter(|_| envmap.is_some() && ::legacy_config::env::var_os("OMSI_NO_BUMP").is_none()).and_then(|(t, f)| tex!(&subst(&t), &dirs_ref, vehicle_bump_texture).map(|id| (id, f)));
                    let freetex = ov_all.iter().any(|o| o.freetex.is_some());
                    if tex.is_none() && !is_null_texture(&m.texture) && text_slot.is_none() && script_slot.is_none() && !freetex && vt.texchange(&m.texture).is_none() {
                        missing_tex.push(format!("{} ({})", tex_name, def.file));
                    }
                    if only.is_some() {
                        log::info!("  {} slot {slot} '{}' diffuse={:?} emissive={:?} specular={:?}/{} tex={:?} alpha={:?} transmap={:?} night={:?} light={:?} env={:?} mask={:?} bump={:?} text={:?} script={:?} script_trans={:?} noZwrite={} noZcheck={} zbias={}", def.file, m.texture, m.diffuse, m.emissive, m.specular, m.specular_power, tex, alpha, transmap, night, lightmap, envmap, env_mask, bump, text_slot, script_slot, script_trans, ov.iter().any(|o| o.no_z_write), ov.iter().any(|o| o.no_z_check), ov.iter().map(|o| o.z_bias).find(|b| *b != 0).unwrap_or(0));
                    }
                    let textured = tex.is_some() || text_slot.is_some() || script_slot.is_some() || freetex || vt.texchange(&m.texture).is_some();
                    let (color, emissive, specular, ambient) = d3d_material(m, ov.iter().find_map(|o| o.allcolor), textured);
                    let mut extra = material_extra(&ov, env_mask, bump, specular);
                    extra.ambient = Some(ambient);
                    extra.night_switched = night.is_some();
                    extra.screen = script_slot.is_some() || script_trans.is_some();
                    extra.led = script_trans.is_some() && !html_page(script_trans) && lm_white(&ov);
                    extra.html = !extra.led && (html_page(script_slot) || html_page(script_trans));
                    if dirt_overlay {
                        extra.no_z_write = true;
                    }
                    extra.metal_ok = envmap.is_some() && alpha == AlphaMode::Opaque && !named_body && !material_has_vehicle_volume(&vm.data, slot);
                    if (transparent_layer_hint || see_through) && alpha == AlphaMode::Blend {
                        extra.no_z_write = true;
                    }
                    extra.glass = transparent_layer_hint
                        && alpha == AlphaMode::Blend
                        && !dirt_overlay
                        && !rain_layer;
                    extra.rain_film = rain_layer && !snowing() && ::legacy_config::env::var_os("OMSI_TEXTURE_RAIN").is_none();
                    if repair_body_depth {
                        extra.no_z_check = false;
                    }
                    let address = tex_addressing(ov.iter().copied());
                    let base_dyn = DynTex { text: text_base, script: script_base, script_trans, address };
                    let unlit = mirror_index(&tex_name).is_some();
                    let later_items: Vec<&MaterialDef> = {
                        let mut changes = 0;
                        let mut first = Vec::new();
                        for o in &ov_all {
                            if !o.item && o.change.is_some() {
                                changes += 1;
                            } else if o.item && changes == 1 {
                                first.push(*o);
                            }
                        }
                        first.into_iter().skip(1).collect()
                    };
                    let mut item_look = |ov_item: &Vec<&MaterialDef>| -> Look {
                        let mut find_tex = |t: &str| -> Option<TextureId> { tex!(t, &dirs_ref) };
                        let it_night = ov_item.iter().rev().find_map(|o| o.nightmap.clone()).and_then(|t| find_tex(&t)).or(night);
                        let it_light = ov_item.iter().rev().find_map(|o| o.lightmap.clone()).and_then(|(t, _)| find_tex(&t)).or(lightmap);
                        let it_script_trans = match ov_item.iter().find_map(|o| o.transmap.clone()) {
                            Some(t) => t.trim().strip_prefix("\\S:").and_then(|n| n.trim().parse::<usize>().ok()),
                            None => script_trans,
                        };
                        let it_trans = ov_item.iter().find_map(|o| o.transmap.clone()).filter(|t| !t.trim().is_empty() && !t.trim().starts_with("\\S:")).and_then(|t| {
                            let id = find_tex(&subst(&t))?;
                            let has_alpha = self.textures.has_alpha(&t, &dirs_ref).unwrap_or(false);
                            Some((id, has_alpha))
                        }).or(transmap);
                        let it_alpha = if repair_body_depth { AlphaMode::Opaque } else { ov_item.iter().find(|o| o.alpha_set).map(|o| alpha_mode(o.alpha)).unwrap_or(alpha) };
                        let (it_color, it_emissive, it_specular, it_ambient) = d3d_material(m, ov_item.iter().find_map(|o| o.allcolor).or(ov.iter().find_map(|o| o.allcolor)), textured);
                        let mut it_extra = material_extra(&ov_item, env_mask, bump, it_specular);
                        it_extra.ambient = Some(it_ambient);
                        it_extra.night_switched = it_night.is_some();
                        it_extra.screen = script_item.is_some() || it_script_trans.is_some();
                        it_extra.led = it_script_trans.is_some() && !html_page(it_script_trans) && if ov_item.iter().any(|o| o.lightmap.is_some()) { lm_white(ov_item) } else { lm_white(&ov) };
                        it_extra.html = !it_extra.led && (html_page(script_item) || html_page(it_script_trans));
                        it_extra.no_z_write |= extra.no_z_write;
                        it_extra.no_z_check |= extra.no_z_check;
                        it_extra.glass |= extra.glass;
                        if repair_body_depth {
                            it_extra.no_z_check = false;
                        }
                        let it_dyn = DynTex { text: text_item, script: script_item, script_trans: it_script_trans, address };
                        Look { alpha: it_alpha, color: it_color, emissive: it_emissive, unlit: false, diffuse: None, transmap: it_trans, night: it_night, lightmap: it_light, envmap, extra: it_extra, dyn_tex: it_dyn }
                    };
                    let first_item: Vec<&MaterialDef> = ov_item.iter().copied().filter(|o| !later_items.iter().any(|l| std::ptr::eq(*l, *o))).collect();
                    let item_spec = (change_var.is_some() && !ov_item.is_empty()).then(|| item_look(&first_item));
                    let more_items: Vec<Look> = if item_spec.is_some() { later_items.iter().map(|o| item_look(&vec![*o])).collect() } else { Vec::new() };
                    if only.is_some() {
                        if let Some(it) = &item_spec {
                            log::info!("  {} slot {slot} item (switched by {:?}): alpha={:?} night={:?} light={:?} switched={}", def.file, change_var, it.alpha, it.night, it.lightmap, it.extra.night_switched);
                        }
                    }
                    let spec = SlotSpec { base: Look { alpha, color, emissive, unlit, diffuse: None, transmap, night, lightmap, envmap, extra, dyn_tex: base_dyn }, item: item_spec, more: more_items };
                    let master = vt.texchange(&m.texture);
                    let entry_tex: Vec<Option<TextureId>> = match master {
                        Some(master) => {
                            let mut edirs: Vec<&Path> = vec![master.dir.as_path()];
                            edirs.extend(dirs_ref.iter().copied());
                            let mut find_tex = |t: &str| -> Option<TextureId> { tex!(t, &edirs) };
                            master.entries.iter().map(|e| find_tex(&subst(e))).collect()
                        }
                        None => Vec::new(),
                    };
                    if let (Some(master), true) = (master, only.is_some()) {
                        log::info!("    [texchanges] {} -> {} entries by '{}', loaded {:?}", master.texture, master.entries.len(), master.variable, entry_tex);
                    }
                    let base_tex = if master.is_some() { entry_tex.first().copied().flatten() } else { tex };
                    let built = spec.build(renderer, scene, base_tex);
                    let (base, item) = self.recycle_pair(renderer, scene, built);
                    materials.extend([base, item]);
                    let entries: Vec<(MaterialId, MaterialId)> = entry_tex
                        .iter()
                        .map(|t| {
                            let built = spec.build(renderer, scene, *t);
                            self.recycle_pair(renderer, scene, built)
                        })
                        .collect();
                    materials.extend(entries.iter().flat_map(|e| [e.0, e.1]));
                    let more = spec.build_more(renderer, scene, base_tex, |scene, m| self.gpu.lock().material(renderer, scene, m));
                    materials.extend(more.iter().copied());
                    let free: Vec<FreeTex> = free_texture_defs(&ov_all).into_iter().map(|(item_only, key, var)| FreeTex {
                        var,
                        diffuse: key.eq_ignore_ascii_case(&m.texture),
                        key: tex!(&subst(&key), &dirs_ref),
                        item_only,
                        dirs: dirs.clone(),
                        textures: self.textures.clone(),
                        cache: HashMap::new(),
                        more_cache: HashMap::new(),
                        current: None,
                        shared: self.vehicle_textures.clone(),
                        held: Vec::new(),
                        wants_upgrade: self.freetex_upgrades.clone(),
                    }).collect();
                    let multi_light = |base: MaterialId, item: MaterialId| -> Option<MultiLight> {
                        let list = ov.iter().rev().map(|o| &o.lightmaps).find(|l| !l.is_empty())?;
                        let maps: Vec<(PathBuf, String)> = list
                            .iter()
                            .filter_map(|(t, v)| ::texture::find_texture(t, &dirs_ref).map(|p| (p, v.clone())))
                            .collect();
                        (maps.len() >= 2 && maps.len() <= 8).then(|| MultiLight {
                            maps,
                            plain: (base, item),
                            cache: HashMap::new(),
                            current: 0,
                            shared: self.vehicle_textures.clone(),
                            held: Vec::new(),
                        })
                    };
                    if spec.item.is_some() || !entries.is_empty() || !free.is_empty() {
                        let tex_var = master.map(|m| m.variable.clone()).unwrap_or_default();
                        variants.push(VariantSlot { mesh: instances.len(), slot, base, item, more, var: change_var.unwrap_or_default(), more_vars: change_vars.iter().skip(1).cloned().collect(), entries, tex_var, free, spec, base_tex, entry_tex, lights: multi_light(base, item) });
                    } else if let Some(lights) = multi_light(base, item) {
                        variants.push(VariantSlot { mesh: instances.len(), slot, base, item, more: Vec::new(), var: String::new(), more_vars: Vec::new(), entries, tex_var: String::new(), free: Vec::new(), spec, base_tex, entry_tex, lights: Some(lights) });
                    } else if base_dyn.any() {
                        dyn_slots.push(DynSlot { mesh: instances.len(), slot, text: text_slot, script: script_slot, script_trans, tex, alpha, transmap, night, lightmap, envmap, address, extra, color, emissive });
                    }
                    base
                })
                .collect();
            let t_mesh = std::time::Instant::now();
            let id = self.vehicle_mesh(
                renderer,
                scene,
                &mut mesh_ids,
                &mut mesh_keys,
                vt,
                mesh_index,
            );
            mesh_secs += t_mesh.elapsed().as_secs_f64();
            instances.push((id, mats));
        }
        if !missing_tex.is_empty() {
            missing_tex.sort();
            missing_tex.dedup();
            log::warn!(
                "{}: {} material slots have no texture: {:?}",
                vt.def
                    .path
                    .file_name()
                    .unwrap_or_default()
                    .to_string_lossy(),
                missing_tex.len(),
                missing_tex
            );
        }
        materials.sort_unstable();
        materials.dedup();
        if ::legacy_config::env::var_os("OMSI_PROFILE").is_some() {
            let (tn, ts) = *tex_time.borrow();
            log::info!(
                "  vehicle set {}: {} meshes ({:.1} ms), {} textures uploaded ({:.1} ms), {} materials, {:.1} ms in all",
                vt.def
                    .path
                    .file_name()
                    .unwrap_or_default()
                    .to_string_lossy(),
                mesh_keys.len(),
                mesh_secs * 1000.0,
                tn,
                ts * 1000.0,
                materials.len(),
                t_all.elapsed().as_secs_f64() * 1000.0
            );
        }
        VehicleSet {
            meshes: instances,
            dyn_slots,
            variants,
            textures: held,
            mesh_keys,
            materials,
            users: 0,
            idle_since: Some(std::time::Instant::now()),
        }
    }
}
