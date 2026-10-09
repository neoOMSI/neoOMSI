use super::*;

impl World {
    /// What the loaded tiles hold, for the streaming statistics.
    pub fn gpu_summary(&self, scene: &Scene) -> String {
        let (vehicle_tex, vehicle_formats) = {
            let v = self.vehicle_textures.lock();
            let mut f: std::collections::BTreeMap<String, (usize, u64)> =
                std::collections::BTreeMap::new();
            let mut total = 0u64;
            for (t, _) in v.values() {
                let b = scene.texture_bytes_of(*t);
                total += b;
                let e = f.entry(scene.texture_format_of(*t)).or_default();
                e.0 += 1;
                e.1 += b;
            }
            (
                total,
                f.iter()
                    .map(|(k, (n, b))| format!("{n} {k} {:.0} MB", *b as f64 / 1e6))
                    .collect::<Vec<_>>()
                    .join(", "),
            )
        };
        let (tile_tex, tile_formats) = {
            let mut f: std::collections::BTreeMap<String, (usize, u64)> =
                std::collections::BTreeMap::new();
            let mut total = 0u64;
            for t in self
                .tile_state
                .lock()
                .values()
                .flat_map(|s| s.gpu.textures.iter())
            {
                let b = scene.texture_bytes_of(*t);
                total += b;
                let e = f
                    .entry(format!(
                        "{} {}",
                        scene.texture_format_of(*t),
                        scene
                            .texture_size_of(*t)
                            .map(|s| format!("{}x{}", s.0, s.1))
                            .unwrap_or_default()
                    ))
                    .or_default();
                e.0 += 1;
                e.1 += b;
            }
            let mut v: Vec<(String, (usize, u64))> = f.into_iter().collect();
            v.sort_by(|a, b| b.1.1.cmp(&a.1.1));
            (
                total,
                v.iter()
                    .take(6)
                    .map(|(k, (n, b))| format!("{n} {k} {:.0} MB", *b as f64 / 1e6))
                    .collect::<Vec<_>>()
                    .join(", "),
            )
        };
        let all_formats = {
            let mut f: std::collections::BTreeMap<String, (usize, u64)> =
                std::collections::BTreeMap::new();
            for t in 0..scene.textures.len() {
                let b = scene.texture_bytes_of(t);
                let e = f
                    .entry(format!(
                        "{} {}",
                        scene.texture_format_of(t),
                        scene
                            .texture_size_of(t)
                            .map(|s| format!("{}x{}", s.0, s.1))
                            .unwrap_or_default()
                    ))
                    .or_default();
                e.0 += 1;
                e.1 += b;
            }
            let mut v: Vec<(String, (usize, u64))> = f.into_iter().collect();
            v.sort_by(|a, b| b.1.1.cmp(&a.1.1));
            let total: u64 = v.iter().map(|x| x.1.1).sum();
            format!(
                "{:.0} MB in all: {}",
                total as f64 / 1e6,
                v.iter()
                    .take(30)
                    .map(|(k, (n, b))| format!("{n} {k} {:.1} MB", *b as f64 / 1e6))
                    .collect::<Vec<_>>()
                    .join(", ")
            )
        };
        if ::legacy_config::env::var_os("OMSI_DEBUG_TEXTURES").is_some() {
            log::info!("all textures by size: {all_formats}");
        }
        let gpu = self.gpu.lock();
        let texels: u64 = gpu.textures.values().map(|t| t.texels).sum();
        let bytes: u64 = gpu.textures.values().map(|t| t.bytes).sum();
        let mut by_format: std::collections::BTreeMap<String, (usize, u64)> =
            std::collections::BTreeMap::new();
        for t in gpu.textures.values() {
            let e = by_format.entry(format!("{:?}", t.format)).or_default();
            e.0 += 1;
            e.1 += t.bytes;
        }
        let formats: Vec<String> = by_format
            .iter()
            .map(|(k, (n, b))| format!("{n} {k} {:.0} MB", *b as f64 / 1e6))
            .collect();
        let (tex_all, mesh_all, other_all) = scene.gpu_bytes();

        let free_instances: usize = gpu.free_instances.values().map(|v| v.len()).sum();
        format!(
            "{} tiles; {} scenery textures ({:.0} MB of texels, {:.0} MB on the GPU: {}), {} object types on the GPU, {} in the type cache; scene {} meshes / {} textures / {} materials / {} instances, free {} / {} / {} / {}; GPU {:.0} MB textures ({:.0} MB vehicles': {}; {:.0} MB tiles' own: {}), {:.0} MB meshes, {:.0} MB draw data",
            self.tile_state.lock().len(),
            gpu.textures.len(),
            texels as f64 * 4.0 / 1e6,
            bytes as f64 / 1e6,
            formats.join(", "),
            gpu.types.len(),
            self.object_types.lock().len(),
            scene.meshes.len(),
            scene.textures.len(),
            scene.materials.len(),
            scene.instances.len(),
            gpu.free_meshes.len(),
            gpu.free_textures.len(),
            gpu.free_materials.len(),
            free_instances,
            tex_all as f64 / 1e6,
            vehicle_tex as f64 / 1e6,
            vehicle_formats,
            tile_tex as f64 / 1e6,
            tile_formats,
            mesh_all as f64 / 1e6,
            other_all as f64 / 1e6
        )
    }

    /// What the loaded map holds in memory on the CPU side (MB, estimated from the sizes of
    /// the big buffers), for OMSI_PROFILE.
    pub fn cpu_summary(&self) -> String {
        let mb = |b: usize| b as f64 / 1e6;
        let types: Vec<Arc<ObjectType>> = self
            .object_types
            .lock()
            .values()
            .flatten()
            .cloned()
            .collect();
        let type_bytes: usize = types.iter().map(|t| t.mesh_bytes()).sum();
        let (mut staged_n, mut staged_bytes) = (0usize, 0usize);
        for st in self.staged.lock().values() {
            staged_n += 1;
            staged_bytes += st
                .splines
                .iter()
                .map(|s| s.shape.heap_bytes())
                .sum::<usize>()
                + st.drive
                .iter()
                .map(|d| match d {
                    StagedDrive::HeightProfiles(mesh, _) => mesh.heap_bytes(),
                    StagedDrive::DrawnMesh { mesh, .. } => mesh.heap_bytes(),
                })
                .sum::<usize>()
                + st.base_terrain.heights.capacity() * 4;
            staged_bytes += st
                .meshes
                .lock()
                .as_ref()
                .map(|m| m.iter().map(|x| x.heap_bytes()).sum::<usize>())
                .unwrap_or(0);
            if let Some(r) = st.resolved.get() {
                staged_bytes += r.terrain.heights.capacity() * 4;
            }
        }
        let (mut rasters, mut drive, mut tris) = (0usize, 0usize, 0usize);
        let surfaces = self.surfaces.read();
        for sf in surfaces.values() {
            let (r, d) = sf.heap_bytes();
            rasters += r;
            drive += d;
            tris += sf.drive.tris.len();
        }
        let terrains: usize = self
            .terrains
            .read()
            .values()
            .map(|t| t.heights.capacity() * 4)
            .sum();
        let vehicle_textures = self.vehicle_textures.lock().len();
        format!(
            "CPU: {} object types {:.0} MB of meshes, {} staged tiles {:.0} MB, {} surfaces {:.0} MB rasters + {:.0} MB wheel grids ({} faces), terrains {:.0} MB, decoded textures held {:.0} MB; {} vehicle textures, {} vehicle sets on the GPU",
            types.len(),
            mb(type_bytes),
            staged_n,
            mb(staged_bytes),
            surfaces.len(),
            mb(rasters),
            mb(drive),
            tris,
            mb(terrains),
            mb(self.textures.held_bytes()),
            vehicle_textures,
            self.vehicle_gpu.lock().len()
        )
    }
}
