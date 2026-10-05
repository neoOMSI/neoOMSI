use crate::*;

impl Renderer {
    pub fn new_scene(&self) -> Scene {
        Scene {
            meshes: Vec::new(),
            textures: Vec::new(),
            tex_luma: Default::default(),
            glass_slot: None,
            glass_key: None,
            materials: Vec::new(),
            instances: Vec::new(),
            render_origin: DVec3::ZERO,
            lights: Vec::new(),
            interior_lights: Vec::new(),
            interior_free: Vec::new(),
            coronas: Vec::new(),
            occluders: Vec::new(),
            model_buf: None,
            params_buf: None,
            light_buf: None,
            grid_buf: None,
            corona_buf: None,
            corona_count: 0,
            smoke: Vec::new(),
            smoke_buf: None,
            smoke_count: 0,
            corona_runs: Vec::new(),
            draw_buf: None,
            camera_bind_group: None,
            shadow_bind_group: None,
            spot_bind_groups: Vec::new(),
            sky_bind_group: None,
            overlays: Vec::new(),
            premultiplied: Default::default(),
            overlay_res: Vec::new(),
            dirty: true,
            changed: Vec::new(),
            changed_mark: Vec::new(),
            cache_bounds: omsi_cfg::env::var_os("OMSI_NO_BOUNDS_CACHE").is_none(),
            bounds_meshes: Vec::new(),
            bounds_dirty: false,
            uploaded_instances: 0,
            uploaded_entries: 0,
            cpu_models: Vec::new(),
            cpu_params: Vec::new(),
            last_grid: Vec::new(),
            last_lights: Vec::new(),
            bind_groups: HashMap::new(),
            pbr_maps: HashMap::new(),
            snow_textures: Default::default(),
        }
    }

    pub fn add_instance(
        &self,
        scene: &mut Scene,
        mesh: MeshId,
        origin: DVec3,
        transform: Mat4,
        materials: Vec<MaterialId>,
    ) -> usize {
        let slots = scene.meshes[mesh]
            .ranges
            .iter()
            .map(|r| r.2)
            .max()
            .map(|m| m as usize + 1)
            .unwrap_or(1);
        scene.instances.push(Instance {
            mesh,
            transform,
            origin,
            materials,
            slot_alpha: vec![1.0; slots],
            slot_light: vec![1.0; slots],
            slot_night: vec![1.0; slots],
            visible: true,
            slot_uv: vec![[0.0; 2]; slots],
            interior: 0.0,
            interior_lamps: 0,
            cabin: false,
            base: 0,
            bounds: InstanceBounds::default(),
            surface: false,
            presurface: false,
            render_phase: RenderPhase::Normal,
            surface_bias: false,
            blend_sort_origin: None,
            lod: (0.0, f32::MAX),
            blob: false,
            ground_layer: false,
            decal: false,
            object_radius: 0.0,
            detail: 1.0,
            any_distance: false,
            near_only: None,
            mirror_only: false,
            omsi_caster: false,
            ordered: false,
            casts_shadow: true,
            roof: None,
        });
        scene.instances.len() - 1
    }

    pub fn add_surface_instance(
        &self,
        scene: &mut Scene,
        mesh: MeshId,
        origin: DVec3,
        transform: Mat4,
        materials: Vec<MaterialId>,
    ) -> usize {
        let slots = scene.meshes[mesh]
            .ranges
            .iter()
            .map(|r| r.2)
            .max()
            .map(|m| m as usize + 1)
            .unwrap_or(1);
        scene.instances.push(Instance {
            mesh,
            transform,
            origin,
            materials,
            slot_alpha: vec![1.0; slots],
            slot_light: vec![1.0; slots],
            slot_night: vec![1.0; slots],
            visible: true,
            slot_uv: vec![[0.0; 2]; slots],
            interior: 0.0,
            interior_lamps: 0,
            cabin: false,
            base: 0,
            bounds: InstanceBounds::default(),
            surface: true,
            presurface: false,
            render_phase: RenderPhase::Normal,
            surface_bias: true,
            blend_sort_origin: None,
            lod: (0.0, f32::MAX),
            blob: false,
            ground_layer: false,
            decal: false,
            object_radius: 0.0,
            detail: 1.0,
            any_distance: false,
            near_only: None,
            mirror_only: false,
            omsi_caster: false,
            ordered: false,
            casts_shadow: false,
            roof: None,
        });
        scene.instances.len() - 1
    }

    pub fn add_shadow_blob_instance(
        &self,
        scene: &mut Scene,
        mesh: MeshId,
        origin: DVec3,
        transform: Mat4,
        materials: Vec<MaterialId>,
    ) -> usize {
        let i = self.add_surface_instance(scene, mesh, origin, transform, materials);
        scene.instances[i].blob = true;
        i
    }

    pub(crate) fn mark_changed(scene: &mut Scene, i: usize) {
        if scene.dirty || i >= scene.uploaded_instances {
            return;
        }
        if scene.changed_mark.len() < scene.instances.len() {
            scene.changed_mark.resize(scene.instances.len(), false);
        }
        if !scene.changed_mark[i] {
            scene.changed_mark[i] = true;
            scene.changed.push(i);
        }
    }

    pub fn alloc_interior_lights(&self, scene: &mut Scene, n: u32) -> u32 {
        if let Some(k) = scene.interior_free.iter().position(|f| f.1 >= n) {
            let (first, len) = scene.interior_free[k];
            if len == n {
                scene.interior_free.remove(k);
            } else {
                scene.interior_free[k] = (first + n, len - n);
            }
            return first;
        }
        let first = scene.interior_lights.len() as u32;
        scene.interior_lights.extend((0..n).map(|_| PointLight {
            intensity: 0.0,
            ..Default::default()
        }));
        first
    }

    pub fn free_interior_lights(&self, scene: &mut Scene, first: u32, n: u32) {
        for l in scene
            .interior_lights
            .iter_mut()
            .skip(first as usize)
            .take(n as usize)
        {
            l.intensity = 0.0;
        }
        scene.interior_free.push((first, n));
    }

    pub fn set_interior_light(&self, scene: &mut Scene, slot: u32, light: PointLight) {
        if let Some(l) = scene.interior_lights.get_mut(slot as usize) {
            *l = light;
        }
    }

    pub fn set_interior_lamps(&self, scene: &mut Scene, instance: usize, first: u32, count: u32) {
        let code = if count == 0 {
            0
        } else {
            first * LAMP_CODE_STRIDE + count.min(MAX_LAMPS_PER_MESH)
        };
        let i = &mut scene.instances[instance];
        if i.interior_lamps != code {
            i.interior_lamps = code;
            Self::mark_changed(scene, instance);
        }
    }

    pub fn set_omsi_caster(&self, scene: &mut Scene, instance: usize, on: bool) {
        if let Some(i) = scene.instances.get_mut(instance) {
            i.omsi_caster = on;
        }
    }

    pub fn set_ordered(&self, scene: &mut Scene, instance: usize, on: bool) {
        if let Some(i) = scene.instances.get_mut(instance) {
            i.ordered = on;
        }
    }

    pub fn set_casts_shadow(&self, scene: &mut Scene, instance: usize, on: bool) {
        if let Some(i) = scene.instances.get_mut(instance) {
            i.casts_shadow = on;
        }
    }

    pub fn set_roof(&self, scene: &mut Scene, instance: usize, roof: Option<f32>) {
        if let Some(i) = scene.instances.get_mut(instance) {
            if i.roof != roof {
                i.roof = roof;
                Self::mark_changed(scene, instance);
            }
        }
    }

    pub fn set_mirror_only(&self, scene: &mut Scene, instance: usize, on: bool) {
        if let Some(i) = scene.instances.get_mut(instance) {
            i.mirror_only = on;
        }
    }

    pub fn set_interior(&self, scene: &mut Scene, instance: usize, interior: f32) {
        let i = &mut scene.instances[instance];
        if (i.interior - interior).abs() > 1e-4 {
            i.interior = interior;
            Self::mark_changed(scene, instance);
        }
    }

    pub fn set_cabin(&self, scene: &mut Scene, instance: usize, cabin: bool) {
        let i = &mut scene.instances[instance];
        if i.cabin != cabin {
            i.cabin = cabin;
            Self::mark_changed(scene, instance);
        }
    }

    pub fn set_slot_light(&self, scene: &mut Scene, instance: usize, light: &[f32]) {
        let i = &mut scene.instances[instance];
        let mut changed = false;
        for (k, l) in i.slot_light.iter_mut().enumerate() {
            let v = light.get(k).copied().unwrap_or(1.0);
            changed |= *l != v;
            *l = v;
        }
        if changed {
            Self::mark_changed(scene, instance);
        }
    }

    pub fn set_slot_night(&self, scene: &mut Scene, instance: usize, night: &[f32]) {
        let i = &mut scene.instances[instance];
        let mut changed = false;
        for (k, l) in i.slot_night.iter_mut().enumerate() {
            let v = night.get(k).copied().unwrap_or(1.0);
            changed |= *l != v;
            *l = v;
        }
        if changed {
            Self::mark_changed(scene, instance);
        }
    }

    pub fn set_lod_range(&self, scene: &mut Scene, instance: usize, min: f32, max: f32) {
        scene.instances[instance].lod = (min, max);
    }

    pub fn set_object_culling(
        &self,
        scene: &mut Scene,
        instance: usize,
        radius: f32,
        detail: f32,
        any_distance: bool,
    ) {
        let i = &mut scene.instances[instance];
        i.object_radius = radius.max(0.0);
        i.detail = if detail > 0.0 { detail } else { 1.0 };
        i.any_distance = any_distance;
    }

    pub fn set_near_only(&self, scene: &mut Scene, instance: usize, area: Option<[f64; 4]>) {
        scene.instances[instance].near_only = area;
    }

    pub fn set_transform(
        &self,
        scene: &mut Scene,
        instance: usize,
        origin: DVec3,
        transform: Mat4,
    ) {
        let i = &mut scene.instances[instance];
        if i.transform != transform || i.origin != origin {
            i.transform = transform;
            i.origin = origin;
            Self::mark_changed(scene, instance);
        }
    }

    pub fn set_render_origin(&self, scene: &mut Scene, origin: DVec3) {
        if scene.render_origin != origin {
            scene.render_origin = origin;
            scene.dirty = true;
        }
    }

    pub(crate) fn bounding_sphere(scene: &Scene, i: &Instance) -> (Vec3, f32) {
        let b = if scene.cache_bounds {
            i.bounds
        } else {
            InstanceBounds::new(&scene.meshes[i.mesh], i.transform)
        };
        (
            b.centre + (i.origin - scene.render_origin).as_vec3(),
            b.radius,
        )
    }

    pub(crate) fn instance_scale(scene: &Scene, i: &Instance) -> f32 {
        if scene.cache_bounds {
            i.bounds.scale
        } else {
            transform_scale(i.transform)
        }
    }

    pub(crate) fn mesh_bounds_changed(scene: &mut Scene, mesh: MeshId) {
        scene.bounds_meshes.resize(scene.meshes.len(), false);
        scene.bounds_meshes[mesh] = true;
        scene.bounds_dirty = true;
    }

    pub(crate) fn prepare_bounds(scene: &mut Scene) {
        if !scene.cache_bounds {
            return;
        }
        if scene.dirty {
            for i in &mut scene.instances {
                i.bounds = InstanceBounds::new(&scene.meshes[i.mesh], i.transform);
            }
        } else {
            for i in &mut scene.instances[scene.uploaded_instances..] {
                i.bounds = InstanceBounds::new(&scene.meshes[i.mesh], i.transform);
            }
            for &idx in &scene.changed {
                if let Some(i) = scene.instances.get_mut(idx) {
                    i.bounds = InstanceBounds::new(&scene.meshes[i.mesh], i.transform);
                }
            }
            if scene.bounds_dirty {
                for i in &mut scene.instances {
                    if scene.bounds_meshes.get(i.mesh).copied().unwrap_or(false) {
                        i.bounds = InstanceBounds::new(&scene.meshes[i.mesh], i.transform);
                    }
                }
            }
        }
        if scene.bounds_dirty {
            scene.bounds_meshes.fill(false);
            scene.bounds_dirty = false;
        }
    }

    pub fn set_params(
        &self,
        scene: &mut Scene,
        instance: usize,
        slot_alpha: &[f32],
        visible: bool,
        slot_uv: &[[f32; 2]],
    ) {
        let i = &mut scene.instances[instance];
        let mut changed = i.visible != visible;
        for (k, a) in i.slot_alpha.iter_mut().enumerate() {
            let requested = slot_alpha.get(k).copied().unwrap_or(1.0);
            let v = i
                .materials
                .get(k)
                .and_then(|id| scene.materials.get(*id))
                .map_or(requested, |m| {
                    Self::clamp_slot_alpha(requested, m.alpha, m.transmap_declared())
                });
            changed |= *a != v;
            *a = v;
        }
        for (k, u) in i.slot_uv.iter_mut().enumerate() {
            let v = slot_uv.get(k).copied().unwrap_or([0.0; 2]);
            changed |= *u != v;
            *u = v;
        }
        i.visible = visible;
        if changed {
            Self::mark_changed(scene, instance);
        }
    }
}
