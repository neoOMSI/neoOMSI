use super::*;

/// Load only the crossing field, also used by the mesh-free navigation pass.
pub(super) fn load_crossing_field(sco: &SceneryObject, model_dir: &Path) -> Option<MeshData> {
    let file = sco.crossing_height_deformation.as_ref()?;
    let path = ::legacy_config::resolve_path(&::legacy_config::resolve_path(model_dir, "model"), file);
    let path = if ::legacy_config::vfs::is_file(&path) {
        path
    } else {
        ::legacy_config::resolve_path(model_dir, file)
    };
    match ::legacy_o3d::load_mesh(&path) {
        Ok(mesh) => Some(mesh_from_o3d(&mesh)),
        Err(e) => {
            log::warn!("crossing height deformation {}: {e}", path.display());
            None
        }
    }
}

/// Apply a crossing field before any map placement. Vertices outside the field stay
/// unchanged: a nearby corner is not an authored height at that position.
pub(super) fn deform_mesh(mesh: &mut MeshData, field: &MeshData) {
    for p in &mut mesh.positions {
        if let Some(d) = field_height(field, p.x, p.y) {
            p.z += d;
        }
    }
    ::geometry::compute_normals_d3d(mesh);
}

/// The height of a `[crossing_heightdeformation]` field at (x, y) of its object's frame.
pub(super) fn field_height(m: &MeshData, x: f32, y: f32) -> Option<f32> {
    let mut best: Option<f32> = None;
    for t in m.indices.chunks_exact(3) {
        let (a, b, c) = (
            m.positions[t[0] as usize],
            m.positions[t[1] as usize],
            m.positions[t[2] as usize],
        );
        let det = (b.x - a.x) * (c.y - a.y) - (c.x - a.x) * (b.y - a.y);
        if det.abs() < 1e-9 {
            continue;
        }
        let l1 = ((b.x - a.x) * (y - a.y) - (x - a.x) * (b.y - a.y)) / det;
        let l2 = ((x - a.x) * (c.y - a.y) - (c.x - a.x) * (y - a.y)) / det;
        let l0 = 1.0 - l1 - l2;
        if l0 >= -1e-4 && l1 >= -1e-4 && l2 >= -1e-4 {
            let h = l0 * a.z + l2 * b.z + l1 * c.z;
            best = Some(best.map_or(h, |o: f32| o.max(h)));
        }
    }
    best
}
