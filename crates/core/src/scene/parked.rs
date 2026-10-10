use super::*;

impl World {
    /// Lay the `[crashmode_pole]` post `key` on the ground from its foot, fallen the way it
    /// was pushed (`push`), and remember that for when its tile comes back.
    pub fn lay_down_pole(
        &self,
        renderer: &Renderer,
        scene: &mut Scene,
        key: i64,
        push: DVec3,
    ) -> Option<DVec3> {
        self.fallen_poles.lock().insert(key, push);
        let (pos, xf, instances) = self.poles.lock().get(&key).cloned()?;
        let fallen = fallen_pole(xf, push);
        for inst in instances {
            renderer.set_transform(scene, inst, pos, fallen);
        }
        Some(pos)
    }

    /// Parked car `key` drives off: it is hidden, its box leaves the obstacles, and its space
    /// stays empty for the rest of the run. What it was, for the car that takes its place.
    pub fn depart_parked(
        &self,
        renderer: &Renderer,
        scene: &mut Scene,
        key: i64,
    ) -> Option<ParkedObject> {
        let p = self.parked_objects.lock().remove(&key)?;
        self.departed.lock().insert(key);
        for inst in &p.instances {
            hide_instance(renderer, scene, *inst);
        }
        let (mut obst, mut boxes) = (Vec::new(), Vec::new());
        if let Some(st) = self.tile_state.lock().get_mut(&p.tile) {
            obst = st
                .obstacles
                .iter()
                .filter(|b| b.id == key)
                .cloned()
                .collect();
            boxes = st
                .parked_boxes
                .iter()
                .filter(|b| b.id == key)
                .cloned()
                .collect();
            st.obstacles.retain(|b| b.id != key);
            st.parked_boxes.retain(|b| b.id != key);
        }
        self.departed_objects
            .lock()
            .insert(key, (p.clone(), obst, boxes));
        self.refresh_tile_lists();
        Some(p)
    }

    /// The parking spaces whose cars have driven off (LAN host: the clients take the same
    /// cars away).
    pub fn departed_keys(&self) -> Vec<i64> {
        let mut k: Vec<i64> = self.departed.lock().iter().copied().collect();
        k.sort_unstable();
        k
    }

    /// LAN client: the parked cars as the host has them - the spaces it lists empty, and
    /// (when the list is `complete`) every other one taken again. A space on a tile not
    /// loaded here yet is remembered: the tile comes up with it empty.
    pub fn mirror_departed(
        &self,
        renderer: &Renderer,
        scene: &mut Scene,
        keys: &[i64],
        complete: bool,
    ) {
        let before = self.departed.lock().len();
        for &k in keys {
            if !self.departed.lock().contains(&k)
                && self.depart_parked(renderer, scene, k).is_none()
            {
                self.departed.lock().insert(k);
            }
        }
        if complete {
            let back: Vec<i64> = self
                .departed
                .lock()
                .iter()
                .copied()
                .filter(|k| !keys.contains(k))
                .collect();
            for k in back {
                if !self.return_parked(renderer, scene, k) {
                    self.departed.lock().remove(&k);
                }
            }
        }
        let after = self.departed.lock().len();
        if after != before {
            log::info!(
                "LAN: parked cars as the host has them: {after} spaces empty (were {before})"
            );
        }
    }

    /// Parked cars standing where `b` is (the player's bus just put down at a depot's entry
    /// point, over a parked bus): they go, as parked cars that drive off do.
    pub fn clear_parked_under(
        &self,
        renderer: &Renderer,
        scene: &mut Scene,
        b: &::simulation::collision::Obb,
    ) -> usize {
        let keys: Vec<i64> = self
            .tile_state
            .lock()
            .values()
            .flat_map(|st| {
                st.parked_boxes
                    .iter()
                    .filter(|p| p.overlaps_plan(b))
                    .map(|p| p.id)
                    .collect::<Vec<_>>()
            })
            .collect();
        let mut n = 0;
        for k in keys {
            if self.depart_parked(renderer, scene, k).is_some() {
                n += 1;
            }
        }
        n
    }

    /// Scenery the bus is put down inside (a mod map's static buses standing in its depot
    /// where the entry point is): objects no taller than a vehicle whose collision boxes lie
    /// for a third or more inside the bus's footprint are taken away, as the object editor
    /// takes one away. A shelter or a sign at the kerb that the bus only touches stays.
    pub fn clear_props_under(
        &self,
        renderer: &Renderer,
        scene: &mut Scene,
        b: &::simulation::collision::Obb,
    ) -> usize {
        let [ax, ay] = b.axes();
        let inside = |p: glam::DVec2| {
            let d = p - b.center;
            d.dot(ax).abs() <= b.half.x && d.dot(ay).abs() <= b.half.y
        };
        let mut keys: Vec<i64> = Vec::new();
        for st in self.tile_state.lock().values() {
            for o in st.obstacles.iter() {
                if o.id < 0
                    || o.mass > 0.0
                    || o.z1 - o.z0 > 5.0
                    || o.z1 < b.z0
                    || o.z0 > b.z1
                    || !o.overlaps_plan(b)
                    || keys.contains(&o.id)
                {
                    continue;
                }
                let [ox, oy] = o.axes();
                let mut n = 0;
                for i in 0..5 {
                    for j in 0..5 {
                        let p = o.center
                            + ox * o.half.x * (i as f64 / 2.0 - 1.0)
                            + oy * o.half.y * (j as f64 / 2.0 - 1.0);
                        if inside(p) {
                            n += 1;
                        }
                    }
                }
                if n >= 9 {
                    keys.push(o.id);
                }
            }
        }
        // (a prop without a collision box - most static vehicles of mod maps have none: its
        // drawn meshes, a third of their points inside the bus's footprint)
        let types: Vec<Arc<ObjectType>> = self
            .object_types
            .lock()
            .values()
            .flatten()
            .cloned()
            .collect();
        let mut by_mesh: Vec<i64> = Vec::new();
        for (id, eo) in self.edit_objects.lock().iter() {
            if keys.contains(&eo.key) || (eo.pos.truncate() - b.center).length() > 25.0 {
                continue;
            }
            let Some(ot) = types.iter().find(|t| t.sco.path == eo.sco) else {
                continue;
            };
            if ot.sco.surface || ot.sco.render_type.is_ground_layer() {
                continue;
            }
            let (mut n, mut inn, mut z0, mut z1) = (0usize, 0usize, f32::MAX, f32::MIN);
            for (m, _, _) in ot.meshes.iter() {
                for p in m.positions.iter().step_by(4) {
                    let w = eo.xf.transform_point3(*p);
                    n += 1;
                    z0 = z0.min(w.z);
                    z1 = z1.max(w.z);
                    if inside(eo.pos.truncate() + glam::DVec2::new(w.x as f64, w.y as f64)) {
                        inn += 1;
                    }
                }
            }
            if n >= 8 && z1 - z0 <= 5.0 && inn * 3 >= n {
                by_mesh.push(*id);
            }
        }
        if keys.is_empty() && by_mesh.is_empty() {
            return 0;
        }
        let ids: Vec<(i64, std::path::PathBuf)> = self
            .edit_objects
            .lock()
            .iter()
            .filter(|(id, eo)| keys.contains(&eo.key) || by_mesh.contains(id))
            .map(|(id, eo)| (*id, eo.sco.clone()))
            .collect();
        for (id, sco) in &ids {
            log::info!(
                "spawn: scenery object {id} ({}) stood where the bus is put: taken away",
                sco.display()
            );
            self.apply_object_edit(
                renderer,
                scene,
                *id,
                ObjectEdit {
                    deleted: true,
                    ..Default::default()
                },
            );
        }
        ids.len()
    }

    /// The spaces parked cars left (key, the object that stood there), whose tile is still
    /// loaded.
    pub fn free_parking(&self) -> Vec<(i64, ParkedObject)> {
        let states = self.tile_state.lock();
        self.departed_objects
            .lock()
            .iter()
            .filter(|(_, (p, _, _))| states.contains_key(&p.tile))
            .map(|(k, (p, _, _))| (*k, p.clone()))
            .collect()
    }

    /// An AI car has parked in the space of departed parked car `key`: the parked object is
    /// there again (shown, an obstacle, a parked car the traffic keeps clear of).
    pub fn return_parked(&self, renderer: &Renderer, scene: &mut Scene, key: i64) -> bool {
        let Some((p, obst, boxes)) = self.departed_objects.lock().remove(&key) else {
            return false;
        };
        let mut states = self.tile_state.lock();
        let Some(st) = states.get_mut(&p.tile) else {
            return false;
        };
        st.obstacles.extend(obst);
        st.parked_boxes.extend(boxes);
        drop(states);
        for &inst in &p.instances {
            if let Some(i) = scene.instances.get(inst) {
                let (alpha, uv) = (i.slot_alpha.clone(), i.slot_uv.clone());
                renderer.set_params(scene, inst, &alpha, true, &uv);
            }
        }
        self.departed.lock().remove(&key);
        self.parked_objects.lock().insert(key, p);
        self.refresh_tile_lists();
        true
    }

    /// The object editor changed map object `id` (the whole edit so far, from where the
    /// tile put it): its instances and its collision boxes follow.
    pub fn apply_object_edit(
        &self,
        renderer: &Renderer,
        scene: &mut Scene,
        id: i64,
        edit: ObjectEdit,
    ) {
        let before = self
            .object_edits
            .lock()
            .insert(id, edit)
            .unwrap_or_default();
        let Some(eo) = self.edit_objects.lock().get(&id).cloned() else {
            return;
        };
        show_edit(renderer, scene, &eo, edit);
        // the boxes: from the previous edit to this one, turned about the object's place
        if let Some(st) = self.tile_state.lock().get_mut(&eo.tile) {
            let from = eo.pos + before.moved;
            let to = eo.pos + edit.moved;
            let turn = (edit.turned - before.turned).to_radians();
            let (sin, cos) = turn.sin_cos();
            let spin = |c: glam::DVec2| {
                let d = c - from.truncate();
                // clockwise, as headings go
                to.truncate() + glam::DVec2::new(d.x * cos + d.y * sin, -d.x * sin + d.y * cos)
            };
            // (a deleted object's boxes go under the ground with it)
            let sunk = |e: &ObjectEdit| e.moved.z - if e.deleted { 10_000.0 } else { 0.0 };
            let dz = sunk(&edit) - sunk(&before);
            for b in st.obstacles.iter_mut().filter(|b| b.id == eo.key) {
                b.center = spin(b.center);
                b.heading += turn;
                b.z0 += dz;
                b.z1 += dz;
            }
            for m in st.mesh_obstacles.iter_mut().filter(|m| m.id == eo.key) {
                let c = spin(m.pos.truncate());
                m.pos = DVec3::new(c.x, c.y, m.pos.z + dz);
                m.heading += turn;
                m.bounds.center = spin(m.bounds.center);
                m.bounds.heading += turn;
                m.bounds.z0 += dz;
                m.bounds.z1 += dz;
            }
        }
        self.refresh_tile_lists();
    }

    /// Take a tile off the GPU and out of the world's lists (its lanes, traffic light
    /// programs and parked cars stay: the traffic holds on to them by index).
    ///
    /// True when object types went with it: [`World::trim_object_types`] then lets their
    /// meshes go on the CPU side too (once after a batch of tiles, not per tile).
    pub fn unload_tile(
        &self,
        renderer: &Renderer,
        scene: &mut Scene,
        key: (i32, i32),
        audio: Option<&::audio::AudioEngine>,
    ) -> bool {
        let Some(state) = self.tile_state.lock().remove(&key) else {
            self.drop_scripted(key, audio);
            return false;
        };
        let _ = self.parked_live.try_update(
            std::sync::atomic::Ordering::Relaxed,
            std::sync::atomic::Ordering::Relaxed,
            |n| Some(n.saturating_sub(state.parked_count)),
        );
        self.parked_objects.lock().retain(|_, p| p.tile != key);
        self.departed_objects
            .lock()
            .retain(|_, (p, _, _)| p.tile != key);
        self.edit_objects.lock().retain(|_, o| o.tile != key);
        {
            // the posts' instances go back to the pool with the tile
            let mut poles = self.poles.lock();
            for k in &state.poles {
                poles.remove(k);
            }
        }
        let freed = self.gpu.lock().release_tile(renderer, scene, state.gpu);
        self.drop_scripted(key, audio);
        self.terrains.write().remove(&key);
        self.surfaces.write().remove(&key);
        freed > 0
    }

    /// Put scenery object `rel` (a `.sco`) at `pos` turned to `heading` (degrees), outside
    /// any tile, its `[texttexture]` strings taken from `strings` - the game's own helpers,
    /// as OMSI puts the dynamic route arrows. Taken away again with
    /// `remove_helper_object`.
    pub fn add_helper_object(
        &self,
        renderer: &Renderer,
        scene: &mut Scene,
        rel: &str,
        pos: DVec3,
        heading: f64,
        strings: &[String],
    ) -> Option<TileGpu> {
        let ot = self.object_type(rel)?;
        let mut guard = self.gpu.lock();
        let gpu = &mut *guard;
        self.ensure_ground(renderer, scene, gpu);
        let ground_mat = gpu.ground.as_ref()?.ground_mat;
        let tkey = self.type_gpu(renderer, scene, gpu, &ot, &HashMap::new(), ground_mat);
        let mut tg = TileGpu::default();
        gpu.types.get_mut(&tkey)?.users += 1;
        tg.types.push(tkey);
        let meshes = gpu.types[&tkey].meshes.clone();
        let xf = Mat4::from_rotation_z((-heading).to_radians() as f32);
        for (mi, (mesh_id, mats)) in meshes.iter().enumerate() {
            let new = renderer.add_instance(scene, *mesh_id, pos, xf, mats.clone());
            renderer.set_omsi_caster(scene, new, ot.mesh_casts.get(mi).copied().unwrap_or(false));
            // a route arrow casts no shadow (only [shadow] meshes do in Omsi.exe)
            if ot.sco.is_help_arrow {
                renderer.set_casts_shadow(scene, new, false);
            }
            let inst = gpu.instance(renderer, scene, new);
            tg.instances.push(inst);
            let Some((_, o3d_mats, overrides)) = ot.meshes.get(mi) else {
                continue;
            };
            for o in overrides.iter().filter(|o| o.use_text_texture.is_some()) {
                let (Some(slot), Some(tt)) = (
                    ::simulation::vehicle::override_slot(o3d_mats, o),
                    ot.model
                        .text_textures
                        .get(o.use_text_texture.unwrap().max(0) as usize),
                ) else {
                    continue;
                };
                let text = tt
                    .variable
                    .trim()
                    .parse::<usize>()
                    .ok()
                    .and_then(|k| strings.get(k))
                    .cloned()
                    .unwrap_or_default();
                let alpha = text_alpha(o3d_mats, slot, overrides);
                let key = scenery_text_key(tt, &text, alpha);
                if let Some(e) = gpu.text_textures.get_mut(&key) {
                    e.2 += 1;
                    let mat = e.1;
                    tg.texts.push(key);
                    renderer.set_material(scene, inst, slot, mat);
                    continue;
                }
                let atlas = self.fonts.lock().get(&tt.font, &|p| {
                    ::texture::decode_file(p)
                        .ok()
                        .map(|i| (i.width, i.height, i.rgba))
                });
                let image = scenery_text_image(tt, atlas, &text);
                let tex = gpu.add_image(renderer, scene, &image, true);
                let mat = renderer.add_material(scene, Some(tex), alpha, [1.0; 4], false);
                let mat = gpu.material(renderer, scene, mat);
                gpu.text_textures.insert(key.clone(), (tex, mat, 1));
                tg.texts.push(key);
                renderer.set_material(scene, inst, slot, mat);
            }
        }
        Some(tg)
    }

    /// Take away an object `add_helper_object` put down.
    pub fn remove_helper_object(&self, renderer: &Renderer, scene: &mut Scene, tg: TileGpu) {
        self.gpu.lock().release_tile(renderer, scene, tg);
    }

    /// The scripted objects of tile `key` go (their sounds stop).
    pub(super) fn drop_scripted(&self, key: (i32, i32), audio: Option<&::audio::AudioEngine>) {
        self.particle_objects.lock().remove(&key);
        if self.light_maps.lock().remove(&key).is_some() {
            self.light_maps_generation
                .fetch_add(1, std::sync::atomic::Ordering::Relaxed);
        }
        let mut scripted = self.scripted.lock();
        if !scripted.iter().any(|o| o.tile == key) {
            return;
        }
        let mut kept = Vec::with_capacity(scripted.len());
        for mut o in scripted.drain(..) {
            if o.tile == key {
                if let (Some(a), Some(mut ss)) = (audio, o.sounds.take()) {
                    ss.stop_all(a);
                }
            } else {
                kept.push(o);
            }
        }
        *scripted = kept;
        let mut by_id = self.scripted_of_object.lock();
        by_id.clear();
        for (i, o) in scripted.iter().enumerate() {
            by_id.insert(o.map_id, i);
        }
    }

    /// The placed objects that stop the outside camera and may reach into the rectangle
    /// `lo`..`hi` of the ground plane (with their types, which are alive while a tile uses
    /// them).
    pub fn camera_blockers(
        &self,
        lo: DVec2,
        hi: DVec2,
    ) -> Vec<(Arc<ObjectType>, crate::camera_arm::Blocker)> {
        let ts = tile_size();
        // an object is kept with the tile its origin stands in, but a big one (a school, a
        // supermarket) reaches well into the next: the tiles around are looked at as well
        let (x0, x1) = (
            ((lo.x - ts) / ts).floor() as i32,
            ((hi.x + ts) / ts).floor() as i32,
        );
        let (y0, y1) = (
            ((lo.y - ts) / ts).floor() as i32,
            ((hi.y + ts) / ts).floor() as i32,
        );
        let states = self.tile_state.lock();
        let mut out = Vec::new();
        for ty in y0..=y1 {
            for tx in x0..=x1 {
                let Some(s) = states.get(&(tx, ty)) else {
                    continue;
                };
                for b in &s.blockers {
                    let r = b.radius;
                    if b.pos.x + r < lo.x
                        || b.pos.x - r > hi.x
                        || b.pos.y + r < lo.y
                        || b.pos.y - r > hi.y
                    {
                        continue;
                    }
                    if let Some(t) = b.ty.upgrade() {
                        out.push((t, b.clone()));
                    }
                }
            }
        }
        out
    }

    /// The passenger cabin of a waiting object (a `people_standing_*` marker, a shelter),
    /// read once per file.
    pub(super) fn waiting_cabin(&self, path: &Path) -> Option<Arc<::legacy_vehicle::PassengerCabin>> {
        if let Some(c) = self.waiting_cabins.lock().get(path) {
            return c.clone();
        }
        let c = ::legacy_vehicle::PassengerCabin::load(path)
            .map_err(|e| log::debug!("waiting places {}: {e}", path.display()))
            .ok()
            .map(Arc::new);
        self.waiting_cabins
            .lock()
            .insert(path.to_path_buf(), c.clone());
        c
    }
}
