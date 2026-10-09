use super::*;

/// A vehicle type with a paint scheme, as its GPU set is known by.
pub type VehicleKey = (PathBuf, Option<usize>);

/// The GPU side of a vehicle type in one paint scheme, shared by its AI copies: meshes and
/// materials, the slots every copy fills itself, and what the set holds of the shared
/// textures and meshes (given back when the set is trimmed).
#[derive(Clone)]
pub struct VehicleSet {
    pub(super) meshes: Vec<(MeshId, Vec<MaterialId>)>,
    pub(super) dyn_slots: Vec<DynSlot>,
    pub(super) variants: Vec<VariantSlot>,
    pub(super) textures: Vec<PathBuf>,
    pub(super) mesh_keys: Vec<(PathBuf, usize)>,
    pub(super) materials: Vec<MaterialId>,
    /// Vehicles drawn with it now, and since when nobody is.
    pub(super) users: usize,
    pub(super) idle_since: Option<std::time::Instant>,
}

/// What reads vehicle sets ahead on a worker thread: their textures and meshes, made on the
/// GPU right there (the device takes calls from any thread) and waiting in `ready` until the
/// set is uploaded - which then only puts them into the scene and makes the materials. A
/// C2's set took 60 ms on the thread that draws when it had to read and upload it all.
#[derive(Clone)]
pub struct VehiclePrefetch {
    pub(super) root: PathBuf,
    pub(super) textures: Arc<TextureCache>,
    pub(super) on_gpu: Arc<Mutex<HashMap<PathBuf, (TextureId, usize)>>>,
    pub(super) meshes_on_gpu: Arc<Mutex<HashMap<(PathBuf, usize), (MeshId, usize)>>>,
    pub(super) ready: Arc<Mutex<PreparedVehicles>>,
    pub(super) gpu: (wgpu::Device, wgpu::Queue),
}

/// Vehicle meshes and textures made on a worker, by (bus file, mesh) and by file.
#[derive(Default)]
pub(super) struct PreparedVehicles {
    pub(super) meshes: HashMap<(PathBuf, usize), ::render::PreparedMesh>,
    pub(super) textures: HashMap<PathBuf, (::render::PreparedTexture, ::texture::PixelFormat)>,
}

impl VehiclePrefetch {
    /// Read what uploading `vt` in `scheme` will ask for and the GPU does not have.
    pub fn prefetch(&self, vt: &::simulation::VehicleType, scheme: Option<usize>) {
        for (name, dirs) in vehicle_texture_names(&self.root, vt, scheme) {
            let refs: Vec<&Path> = dirs.iter().map(|p| p.as_path()).collect();
            let Some(path) = ::texture::find_texture(&name, &refs) else {
                continue;
            };
            if self.on_gpu.lock().contains_key(&path)
                || self.ready.lock().textures.contains_key(&path)
            {
                continue;
            }
            let Some(data) = self.textures.get_gpu_path(&path) else {
                continue;
            };
            // on the GPU now, unless the GPU has to make its chain (the upload does that)
            if let Some(t) = ::render::prepare_texture(&self.gpu.0, &self.gpu.1, &data) {
                self.textures.release(&path);
                self.ready
                    .lock()
                    .textures
                    .entry(path)
                    .or_insert((t, data.format));
            }
        }
        // [matl_bumpmap] height maps, kept under their own key
        for (name, dirs) in vehicle_bump_names(&self.root, vt, scheme) {
            let refs: Vec<&Path> = dirs.iter().map(|p| p.as_path()).collect();
            let Some(path) = ::texture::find_texture(&name, &refs) else {
                continue;
            };
            let key = bump_key(&path);
            if self.on_gpu.lock().contains_key(&key)
                || self.ready.lock().textures.contains_key(&key)
            {
                continue;
            }
            let Some(data) = load_texture_key(&key, true) else {
                continue;
            };
            if let Some(t) = ::render::prepare_texture(&self.gpu.0, &self.gpu.1, &data) {
                self.ready
                    .lock()
                    .textures
                    .entry(key)
                    .or_insert((t, data.format));
            }
        }
        for i in 0..vt.meshes.len() {
            let key = (vt.def.path.clone(), i);
            if self.meshes_on_gpu.lock().contains_key(&key)
                || self.ready.lock().meshes.contains_key(&key)
            {
                continue;
            }
            if let Some(d) = vt.mesh_data(i) {
                let m = ::render::prepare_mesh(&self.gpu.0, &self.gpu.1, &d);
                self.ready.lock().meshes.entry(key).or_insert(m);
            }
        }
    }
}

impl World {
    /// Upload a vehicle type's meshes and create render instances for one vehicle (the
    /// player's: its set is not shared and stays).
    pub fn add_vehicle(
        &self,
        renderer: &Renderer,
        scene: &mut Scene,
        vt: &::simulation::VehicleType,
        scheme: Option<usize>,
    ) -> VehicleRender {
        let set = self.upload_vehicle(renderer, scene, vt, scheme, 0);
        let mut render = self.instantiate_vehicle(renderer, scene, vt, &set, None, None);
        own_skinned_meshes(renderer, scene, vt, &mut render);
        render
    }

    /// A part coupled behind the player's vehicle (the rear section of an articulated bus).
    /// With `[scriptshare]` it has no scripts of its own: its matrix displays (`\S:n`,
    /// `[useScriptTexture] n`) are the leading vehicle's script textures, which is why the
    /// O530G's rear section declares no `[scripttexture]` at all.
    pub fn add_vehicle_part(
        &self,
        renderer: &Renderer,
        scene: &mut Scene,
        vt: &::simulation::VehicleType,
        scheme: Option<usize>,
        lead: &VehicleRender,
    ) -> VehicleRender {
        self.add_vehicle_part_mirrors(renderer, scene, vt, scheme, lead, 0)
    }

    pub fn add_vehicle_part_mirrors(
        &self,
        renderer: &Renderer,
        scene: &mut Scene,
        vt: &::simulation::VehicleType,
        scheme: Option<usize>,
        lead: &VehicleRender,
        mirror_base: usize,
    ) -> VehicleRender {
        let set = self.upload_vehicle(renderer, scene, vt, scheme, mirror_base);
        let shared = if vt.def.script_share || vt.model.script_textures.is_empty() {
            Some(lead.script_textures.as_slice())
        } else {
            None
        };
        let mut render = self.instantiate_vehicle(renderer, scene, vt, &set, None, shared);
        own_skinned_meshes(renderer, scene, vt, &mut render);
        render
    }

    /// The worker-side reader of vehicle sets (see [`VehiclePrefetch`]).
    pub fn vehicle_prefetch(&self, renderer: &Renderer) -> VehiclePrefetch {
        VehiclePrefetch {
            root: self.root.clone(),
            textures: self.textures.clone(),
            on_gpu: self.vehicle_textures.clone(),
            meshes_on_gpu: self.vehicle_meshes.clone(),
            ready: self.vehicle_ready.clone(),
            gpu: (renderer.device.clone(), renderer.queue.clone()),
        }
    }

    /// Read these vehicle sets on the worker pool and wait for them (a timetable's first
    /// buses at load time). Returns the bytes held until they are uploaded.
    pub fn prefetch_vehicle_sets(
        &self,
        renderer: &Renderer,
        sets: &[(Arc<::simulation::VehicleType>, Option<usize>)],
    ) -> usize {
        use rayon::prelude::*;
        let p = self.vehicle_prefetch(renderer);
        sets.par_iter()
            .for_each(|(vt, scheme)| p.prefetch(vt, *scheme));
        self.textures.held_bytes()
            + self
            .vehicle_ready
            .lock()
            .textures
            .values()
            .map(|t| t.0.bytes() as usize)
            .sum::<usize>()
    }

    /// Textures the thread that draws has to read itself are read the quick way and
    /// compressed afterwards on the workers (a window: no frame waits for a compression).
    pub fn set_fast_texture_loads(&self, on: bool) {
        self.gpu.lock().fast_loads = on;
    }

    /// Drop what was read ahead for vehicle sets and not uploaded.
    pub fn forget_prefetched(&self) {
        self.textures.release_all();
        let mut r = self.vehicle_ready.lock();
        r.meshes.clear();
        r.textures.clear();
    }

    /// Whether the set of `vt` in `scheme` is on the GPU.
    pub fn has_vehicle_set(&self, key: &VehicleKey) -> bool {
        self.vehicle_gpu.lock().contains_key(key)
    }

    /// Upload a vehicle type's meshes, textures and materials ahead of time, so that the
    /// first bus of that type and paint scheme does not cost a frame when it spawns.
    pub fn precache_vehicle(
        &self,
        renderer: &Renderer,
        scene: &mut Scene,
        vt: &::simulation::VehicleType,
        scheme: Option<usize>,
    ) {
        let key = (vt.def.path.clone(), scheme);
        if self.vehicle_gpu.lock().contains_key(&key) {
            // uploaded meanwhile (a bus came first): what was read for it can go
            let dirs_of = vehicle_texture_names(&self.root, vt, scheme);
            for (name, dirs) in dirs_of {
                let refs: Vec<&Path> = dirs.iter().map(|p| p.as_path()).collect();
                if let Some(p) = ::texture::find_texture(&name, &refs) {
                    if self.vehicle_textures.lock().contains_key(&p) {
                        self.textures.release(&p);
                        self.vehicle_ready.lock().textures.remove(&p);
                    }
                }
            }
            let on_gpu: Vec<(PathBuf, usize)> = (0..vt.meshes.len())
                .map(|i| (vt.def.path.clone(), i))
                .filter(|k| self.vehicle_meshes.lock().contains_key(k))
                .collect();
            let mut r = self.vehicle_ready.lock();
            for k in on_gpu {
                r.meshes.remove(&k);
            }
            return;
        }
        let c = self.upload_vehicle(renderer, scene, vt, scheme, 0);
        self.vehicle_gpu.lock().insert(key, c);
    }

    /// Like `add_vehicle`, but meshes and static materials uploaded for the same vehicle
    /// type and scheme are shared between instances (AI traffic). Give the render back with
    /// [`World::release_vehicle`].
    /// `lead` is the vehicle this one is coupled behind, if any: a rear section takes the
    /// leading vehicle's script textures (`[scriptshare]`, its `[matl_transmap] \S:n`
    /// displays), which its own model declares none of. Built without them its matrix slot
    /// had no mask and drew the lit panel's own picture instead of the dots the leading
    /// vehicle's scripts put there.
    pub fn add_vehicle_shared(
        &self,
        renderer: &Renderer,
        scene: &mut Scene,
        vt: &::simulation::VehicleType,
        scheme: Option<usize>,
        lead: Option<&VehicleRender>,
    ) -> VehicleRender {
        let shared = lead.and_then(|l| {
            (vt.def.script_share || vt.model.script_textures.is_empty())
                .then(|| l.script_textures.as_slice())
        });
        let key = (vt.def.path.clone(), scheme);
        let cached = self.vehicle_gpu.lock().get(&key).cloned();
        let set = match cached {
            Some(c) => c,
            None => {
                let c = self.upload_vehicle(renderer, scene, vt, scheme, 0);
                self.vehicle_gpu.lock().insert(key.clone(), c.clone());
                c
            }
        };
        if let Some(s) = self.vehicle_gpu.lock().get_mut(&key) {
            s.users += 1;
            s.idle_since = None;
        }
        let mut render = self.instantiate_vehicle(renderer, scene, vt, &set, Some(key), shared);
        // an articulated AI bus (timetable or random traffic, and its coupled rear section)
        // bends its own bellows too, from a mesh copy of its own (freed again in
        // `release_vehicle`) - the shared set's copy has to stay in the rest pose, since
        // every other instance of the type still draws it
        own_skinned_meshes(renderer, scene, vt, &mut render);
        render
    }

    /// An AI vehicle has gone: its own instances, textures and materials go back to the free
    /// lists, and its set loses a user.
    pub fn release_vehicle(&self, renderer: &Renderer, scene: &mut Scene, render: VehicleRender) {
        {
            let mut gpu = self.gpu.lock();
            // the mesh copies its `[smoothskin]` meshes were reshaped in belong to this
            // vehicle alone (see `own_skinned_meshes`) and do not go back to any free list
            for (_, mesh, _) in &render.skinned {
                renderer.free_mesh(scene, *mesh);
            }
            for i in render.instances {
                renderer.remove_instance(scene, i);
                let slots = renderer.instance_slots(scene, i);
                gpu.free_instances.entry(slots).or_default().push(i);
            }
            let own_script: &[Option<TextureId>] = if render.shared_script {
                &[]
            } else {
                &render.script_textures
            };
            let mut own_textures: Vec<TextureId> = render
                .text_textures
                .iter()
                .chain(own_script)
                .flatten()
                .copied()
                .collect();
            let mut own_materials = render.own_materials;
            if let Some(wipers) = &render.window_wipers {
                own_textures.extend(wipers.textures());
                own_materials.extend(wipers.materials());
            }
            for v in &render.variants {
                if let Some(l) = &v.lights {
                    for (b, it) in l.cache.values() {
                        own_materials.push(*b);
                        own_materials.push(*it);
                    }
                    let mut shared = l.shared.lock();
                    for p in &l.held {
                        let Some(e) = shared.get_mut(p) else { continue };
                        e.1 = e.1.saturating_sub(1);
                        if e.1 == 0 {
                            own_textures.push(e.0);
                            shared.remove(p);
                        }
                    }
                }
            }
            for v in render.variants {
                for f in v.free {
                    for (b, it) in f.cache.into_values() {
                        own_materials.push(b);
                        own_materials.push(it);
                    }
                    // the pictures it showed: shared, gone with their last holder
                    let mut shared = f.shared.lock();
                    for p in f.held {
                        let Some(e) = shared.get_mut(&p) else {
                            continue;
                        };
                        e.1 = e.1.saturating_sub(1);
                        if e.1 == 0 {
                            own_textures.push(e.0);
                            shared.remove(&p);
                        }
                    }
                }
            }
            own_materials.sort_unstable();
            own_materials.dedup();
            for m in own_materials {
                gpu.free_material(renderer, scene, m);
            }
            own_textures.sort_unstable();
            own_textures.dedup();
            for t in own_textures {
                renderer.free_texture(scene, t);
                gpu.free_textures.push(t);
            }
        }
        if let Some((first, n)) = render.interior_lamps.get() {
            renderer.free_interior_lights(scene, first, n);
        }
        if let Some(key) = render.set {
            if let Some(s) = self.vehicle_gpu.lock().get_mut(&key) {
                s.users = s.users.saturating_sub(1);
                if s.users == 0 {
                    s.idle_since = Some(std::time::Instant::now());
                }
            }
        }
    }

    /// Let go of the vehicle sets nobody has drawn for `idle` and that are not in `keep`
    /// (the timetable's next buses): their materials, and the textures and meshes no other
    /// set holds. Returns how many sets went.
    pub fn trim_vehicle_sets(
        &self,
        renderer: &Renderer,
        scene: &mut Scene,
        keep: &hashbrown::HashSet<VehicleKey>,
        idle: std::time::Duration,
    ) -> usize {
        let now = std::time::Instant::now();
        let mut sets = self.vehicle_gpu.lock();
        let gone: Vec<VehicleKey> = sets
            .iter()
            .filter(|(k, s)| {
                s.users == 0
                    && !keep.contains(*k)
                    && s.idle_since
                    .map(|t| now.duration_since(t) >= idle)
                    .unwrap_or(false)
            })
            .map(|(k, _)| k.clone())
            .collect();
        if gone.is_empty() {
            return 0;
        }
        let mut tex_ids = self.vehicle_textures.lock();
        let mut mesh_ids = self.vehicle_meshes.lock();
        let mut gpu = self.gpu.lock();
        let (mut textures, mut meshes) = (0usize, 0usize);
        for k in &gone {
            let s = sets.remove(k).unwrap();
            for m in s.materials {
                gpu.free_material(renderer, scene, m);
            }
            for p in s.textures {
                let Some(e) = tex_ids.get_mut(&p) else {
                    continue;
                };
                e.1 = e.1.saturating_sub(1);
                if e.1 == 0 {
                    let id = e.0;
                    tex_ids.remove(&p);
                    renderer.free_texture(scene, id);
                    gpu.free_textures.push(id);
                    textures += 1;
                }
            }
            for mk in s.mesh_keys {
                let Some(e) = mesh_ids.get_mut(&mk) else {
                    continue;
                };
                e.1 = e.1.saturating_sub(1);
                if e.1 == 0 {
                    let id = e.0;
                    mesh_ids.remove(&mk);
                    renderer.free_mesh(scene, id);
                    gpu.free_meshes.push(id);
                    meshes += 1;
                }
            }
        }
        if ::legacy_config::env::var_os("OMSI_PROFILE").is_some() {
            log::info!(
                "vehicle sets: {} let go ({textures} textures, {meshes} meshes), {} kept",
                gone.len(),
                sets.len()
            );
        }
        gone.len()
    }
}
