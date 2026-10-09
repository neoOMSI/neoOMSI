use super::*;

/// Short static spline segments with matching materials share a mesh within a 48 m cell.
/// Their coordinates, material order, terrain mapping and shadow flag stay intact; long segments retain
/// their own culling bounds. The original meshes remain in the staging/collision data.
pub(super) fn batch_static_splines(
    splines: Vec<(Arc<MeshData>, Arc<SplineType>, bool, DVec3)>,
) -> Vec<(Arc<MeshData>, Arc<SplineType>, bool, DVec3)> {
    if ::legacy_config::env::var_os("OMSI_NO_SPLINE_BATCHING").is_some() {
        return splines;
    }
    let mut groups: Vec<(Vec<Arc<MeshData>>, Arc<SplineType>, bool, DVec3)> = Vec::new();
    let mut cells = HashMap::new();
    let mut signatures = HashMap::new();
    let mut type_materials = HashMap::new();
    let material_batching = ::legacy_config::env::var_os("OMSI_NO_MATERIAL_SPLINE_BATCHING").is_none();
    for (mesh, ty, casts, sort_origin) in splines {
        let (lo, hi) = mesh.positions.iter().fold(
            (
                glam::Vec3::splat(f32::INFINITY),
                glam::Vec3::splat(f32::NEG_INFINITY),
            ),
            |(lo, hi), &p| (lo.min(p), hi.max(p)),
        );
        let centre = (lo + hi) * 0.5;
        // Blended segments retain their individual placement origins and draw order.
        let blended = mesh.ranges.iter().any(|r| {
            ty.def
                .textures
                .get(r.2 as usize)
                .is_some_and(|t| t.alpha >= 2)
        });
        let short = !blended && centre.is_finite() && (hi - lo).length() <= 48.0;
        let group = if short {
            let slots: Vec<_> = mesh.ranges.iter().map(|r| r.2).collect();
            // UV generation is already complete. Only the textures of the remaining
            // ranges matter now; an unused terrain slot must not split identical curbs.
            // Keep the lookup directory and alpha mode exact so similarly named files
            // in different content packs cannot be combined.
            let material_type = if material_batching
                && slots.iter().all(|&s| (s as usize) < ty.def.textures.len())
            {
                (
                    true,
                    *type_materials
                        .entry((Arc::as_ptr(&ty) as usize, slots.clone()))
                        .or_insert_with(|| {
                            let signature = (
                                ty.dir.clone(),
                                slots
                                    .iter()
                                    .map(|&s| {
                                        let texture = &ty.def.textures[s as usize];
                                        (s, texture.file.clone(), texture.alpha)
                                    })
                                    .collect::<Vec<_>>(),
                            );
                            let next = signatures.len();
                            *signatures.entry(signature).or_insert(next)
                        }),
                )
            } else {
                (false, Arc::as_ptr(&ty) as usize)
            };
            let key = (
                material_type,
                (centre.x / 48.0).floor() as i32,
                (centre.y / 48.0).floor() as i32,
                casts,
                mesh.one_sided,
                slots,
            );
            *cells.entry(key).or_insert_with(|| {
                let i = groups.len();
                groups.push((Vec::new(), ty.clone(), casts, sort_origin));
                i
            })
        } else {
            let i = groups.len();
            groups.push((Vec::new(), ty, casts, sort_origin));
            i
        };
        groups[group].0.push(mesh);
    }
    groups
        .into_iter()
        .map(|(mut meshes, ty, casts, sort_origin)| {
            let mesh = if meshes.len() == 1 {
                meshes.pop().unwrap()
            } else {
                Arc::new(MeshData::merge_static(
                    &meshes.iter().map(AsRef::as_ref).collect::<Vec<_>>(),
                ))
            };
            (mesh, ty, casts, sort_origin)
        })
        .collect()
}

/// These faces all use the tile's ground materials, irrespective of the source .sli.
/// Pool them before upload so grass widths and curb types can share a ground draw.
pub(super) fn batch_ground_splines(meshes: Vec<Arc<MeshData>>) -> Vec<Arc<MeshData>> {
    let mut groups: Vec<Vec<Arc<MeshData>>> = Vec::new();
    let mut cells = HashMap::new();
    for mesh in meshes {
        let (lo, hi) = mesh.positions.iter().fold(
            (
                glam::Vec3::splat(f32::INFINITY),
                glam::Vec3::splat(f32::NEG_INFINITY),
            ),
            |(lo, hi), &p| (lo.min(p), hi.max(p)),
        );
        let centre = (lo + hi) * 0.5;
        let group = if centre.is_finite() && (hi - lo).length() <= 48.0 {
            let key = (
                (centre.x / 48.0).floor() as i32,
                (centre.y / 48.0).floor() as i32,
                (centre.z / 48.0).floor() as i32,
                mesh.one_sided,
            );
            *cells.entry(key).or_insert_with(|| {
                let i = groups.len();
                groups.push(Vec::new());
                i
            })
        } else {
            let i = groups.len();
            groups.push(Vec::new());
            i
        };
        groups[group].push(mesh);
    }
    groups
        .into_iter()
        .map(|mut meshes| {
            if meshes.len() == 1 {
                meshes.pop().unwrap()
            } else {
                Arc::new(MeshData::merge_static(
                    &meshes.iter().map(AsRef::as_ref).collect::<Vec<_>>(),
                ))
            }
        })
        .collect()
}

/// Split the material slots of an object mesh whose texture carries `[terrainmapping]`
/// off into a mesh of their own. OMSI does not draw such a slot with its texture (the
/// stock ones are a 1x1 placeholder, TH_Wald's Gras01.dds a single green pixel): the slot
/// takes on the map's first ground texture, so that the grass on top of a rock, a
/// traffic island or a roundabout runs on seamlessly from the meadow around it. The split
/// mesh therefore gets the terrain's own uv (tile space, see `build_terrain_mesh`) for
/// the object placed at `pos`/`xf` on the tile at `origin`, and is drawn with the tile's
/// uncut base material. Returns the mesh without those slots and the split-off one.
#[cfg(test)]
pub(super) fn split_terrain_mapped(
    src: &MeshData,
    slots: &[usize],
    pos: DVec3,
    xf: Mat4,
    origin: DVec3,
) -> (MeshData, MeshData) {
    (
        terrain_rest(src, slots),
        terrain_ground(src, slots, pos, xf, origin),
    )
}

/// The mesh without its `[terrainmapping]` slots (see `split_terrain_mapped`): the same for
/// every placement of a type, so it is made once per type.
pub(super) fn terrain_rest(src: &MeshData, slots: &[usize]) -> MeshData {
    let mut rest = src.clone();
    rest.ranges.retain(|r| !slots.contains(&(r.2 as usize)));
    rest
}

/// The `[terrainmapping]` slots of a mesh in tile space (see `split_terrain_mapped`).
pub(super) fn terrain_ground(
    src: &MeshData,
    slots: &[usize],
    pos: DVec3,
    xf: Mat4,
    origin: DVec3,
) -> MeshData {
    let mut ground = MeshData {
        one_sided: src.one_sided,
        ..MeshData::default()
    };
    let mut map: HashMap<u32, u32> = HashMap::new();
    let to_tile = (pos - origin) / tile_size();
    for &(start, count, slot) in &src.ranges {
        if !slots.contains(&(slot as usize)) {
            continue;
        }
        for &k in &src.indices[start as usize..(start + count) as usize] {
            let v = *map.entry(k).or_insert_with(|| {
                let p = src.positions[k as usize];
                let local = xf.transform_point3(p).as_dvec3() / tile_size() + to_tile;
                ground.positions.push(p);
                ground.normals.push(
                    src.normals
                        .get(k as usize)
                        .copied()
                        .unwrap_or(glam::Vec3::Z),
                );
                ground
                    .uvs
                    .push(glam::Vec2::new(local.x as f32, local.y as f32));
                ground.positions.len() as u32 - 1
            });
            ground.indices.push(v);
        }
    }
    let n = ground.indices.len() as u32;
    if n > 0 {
        ground.ranges.push((0, n, 0));
    }
    ground
}

/// Two crossed unit quads (1 m wide, 1 m tall, centred at x=0, standing on z=0).
pub(super) fn tree_quad_mesh() -> MeshData {
    let mut m = MeshData::default();
    for (dx, dy) in [(0.5f32, 0.0f32), (0.0, 0.5)] {
        let base = m.positions.len() as u32;
        for (sx, z, u, v) in [
            (-1.0f32, 0.0f32, 0.0f32, 1.0f32),
            (1.0, 0.0, 1.0, 1.0),
            (1.0, 1.0, 1.0, 0.0),
            (-1.0, 1.0, 0.0, 0.0),
        ] {
            m.positions.push(glam::Vec3::new(dx * sx, dy * sx, z));
            m.normals.push(glam::Vec3::Z);
            m.uvs.push(glam::Vec2::new(u, v));
        }
        m.indices.extend_from_slice(&[
            base,
            base + 1,
            base + 2,
            base,
            base + 2,
            base + 3,
            base,
            base + 2,
            base + 1,
            base,
            base + 3,
            base + 2,
        ]);
    }
    m.ranges.push((0, m.indices.len() as u32, 0));
    m
}
