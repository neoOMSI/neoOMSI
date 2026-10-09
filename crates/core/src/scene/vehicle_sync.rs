use super::*;

/// Give the `[smoothskin]` meshes of a vehicle instance copies of their own: their vertices
/// are rewritten as the joint turns, which a mesh shared between every instance of the type
/// (the AI pool, and the player's own set before this ran) cannot be. Called for the
/// player's vehicle and for every AI copy alike (`render.set` says which).
pub(super) fn own_skinned_meshes(
    renderer: &Renderer,
    scene: &mut Scene,
    vt: &::simulation::VehicleType,
    render: &mut VehicleRender,
) {
    for (i, vm) in vt.meshes.iter().enumerate() {
        if vm.skin.is_empty() || vm.data.positions.is_empty() {
            continue;
        }
        let Some(&inst) = render.instances.get(i) else {
            continue;
        };
        // (a mesh OMSI_ONLY_MESH / OMSI_HIDE_MESH left out stays out)
        if scene
            .meshes
            .get(scene.instances[inst].mesh)
            .map(|m| m.ranges.is_empty())
            .unwrap_or(true)
        {
            continue;
        }
        let id = renderer.add_mesh(scene, &vm.data);
        renderer.set_instance_mesh(scene, inst, id);
        render.skinned.push((i, id, Vec::new()));
    }
    if !render.skinned.is_empty() {
        let names = render
            .skinned
            .iter()
            .map(|(i, _, _)| vt.model.meshes[vt.meshes[*i].def_index].file.as_str())
            .collect::<Vec<_>>()
            .join(", ");
        let name = vt
            .def
            .path
            .file_name()
            .unwrap_or_default()
            .to_string_lossy();
        // the player's own vehicle says so once; an AI copy (the timetable's articulated
        // buses spawn dozens of these) only on demand
        match render.set {
            None => log::info!("{name}: {} skinned meshes ({names})", render.skinned.len()),
            Some(_) => log::debug!(
                "{name} (AI): {} skinned meshes ({names})",
                render.skinned.len()
            ),
        }
    }
}

/// Reshape the skinned meshes of a vehicle and its coupled parts whose bones moved (the
/// player's own, or an AI copy's - see `own_skinned_meshes`).
pub fn sync_skinned(
    renderer: &Renderer,
    scene: &mut Scene,
    vehicle: &mut ::simulation::VehicleInstance,
    render: &mut VehicleRender,
    parts: &mut [VehicleRender],
) {
    for (i, id, last) in render.skinned.iter_mut() {
        let key = vehicle.skin_key(*i);
        if key == *last {
            continue;
        }
        if let Some((pos, nrm)) = vehicle.skinned(*i) {
            renderer.update_mesh(scene, *id, &pos, &nrm, &vehicle.ty.meshes[*i].data.uvs);
        }
        *last = key;
    }
    let n_vars = vehicle.state.vars.len();
    for (t, r) in vehicle.trailers.iter_mut().zip(parts.iter_mut()) {
        for (i, id, last) in r.skinned.iter_mut() {
            let key = t.skin_key(*i);
            if key == *last {
                continue;
            }
            if let Some((pos, nrm)) = t.skinned(*i, n_vars) {
                renderer.update_mesh(scene, *id, &pos, &nrm, &t.ty.meshes[*i].data.uvs);
            }
            *last = key;
        }
    }
}

/// Upload the text and script textures of a vehicle that changed since the last frame.
/// Switch the material of every slot a variable controls: `[matl_freetex]` loads the file a
/// string variable names, `[texchanges]` picks an entry of its master, `[matl_change]` picks
/// between the plain material and the `[matl_item]` variant.
pub fn sync_vehicle_materials(
    renderer: &Renderer,
    scene: &mut Scene,
    vehicle: &::simulation::VehicleInstance,
    render: &mut VehicleRender,
) {
    sync_materials(renderer, scene, vehicle, render);
}

/// A coupled part's switched materials and text textures, driven by the variables of the
/// vehicle it is coupled to (its scripts are shared with it).
pub fn sync_vehicle_part(
    renderer: &Renderer,
    scene: &mut Scene,
    main: &::simulation::VehicleInstance,
    part: &mut ::simulation::vehicle::TrailerPart,
    render: &mut VehicleRender,
) {
    sync_interior_lamps(
        renderer,
        scene,
        &part.ty,
        part.position,
        part.body_rotation(),
        |n| main.var(n),
        render,
    );
    sync_materials(renderer, scene, main, render);
    for i in part.update_text_textures(main) {
        if let (Some(Some(tex)), Some(img)) = (
            render.text_textures.get(i),
            part.text_textures[i].pending.take(),
        ) {
            let d = &part.text_textures[i].def;
            renderer.update_texture_mips(
                scene,
                *tex,
                &Image {
                    width: d.width.max(1) as u32,
                    height: d.height.max(1) as u32,
                    rgba: img,
                    has_alpha: true,
                },
            );
        }
    }
}

/// Resolve a vehicle `[matl_freetex]` name. OMSI add-ons often write paths such as
/// `..\\Texture\\mb_pmon\\warning.bmp`: if the normal lookup misses, retry the part
/// below the `Texture` component against the vehicle's texture search directories.
pub(super) fn find_vehicle_freetex(name: &str, dirs: &[&Path]) -> Option<PathBuf> {
    if let Some(path) = ::texture::find_texture(name, dirs) {
        return Some(path);
    }
    let normalized = name.trim().replace('\\', "/");
    let parts: Vec<&str> = normalized.split('/').filter(|p| !p.is_empty()).collect();
    let texture = parts
        .iter()
        .position(|p| p.eq_ignore_ascii_case("Texture"))?;
    let rel = parts.get(texture + 1..)?.join("/");
    if rel.is_empty() {
        return None;
    }
    ::texture::find_texture(&rel, dirs)
}

pub(super) fn sync_materials(
    renderer: &Renderer,
    scene: &mut Scene,
    vehicle: &::simulation::VehicleInstance,
    render: &mut VehicleRender,
) {
    for v in &mut render.variants {
        let item_has_freetex = v.free.iter().any(|f| f.item_only);
        for f in &mut v.free {
            let raw = vehicle.str_var_str(&f.var);
            let trimmed = raw.trim();
            if f.current.as_deref().is_some_and(|cur| cur.eq_ignore_ascii_case(trimmed)) {
                continue;
            }
            let name = trimmed.to_string();
            let key = name.to_ascii_lowercase();
            f.current = Some(key.clone());
                let pair = match f.cache.get(&key) {
                    Some(p) => *p,
                    None => {
                        let dirs: Vec<&Path> = f.dirs.iter().map(|p| p.as_path()).collect();
                        let found = if name.is_empty() {
                            None
                        } else {
                            let resolved = find_vehicle_freetex(&name, &dirs);
                            if resolved.is_none() {
                                log::warn!(
                                    "vehicle [matl_freetex] '{}' = {:?}: texture not found",
                                    f.var,
                                    name
                                );
                            }
                            resolved.and_then(|path| {
                                let mut shared = f.shared.lock();
                                if let Some(e) = shared.get_mut(&path) {
                                    e.1 += 1;
                                    f.held.push(path);
                                    return Some(e.0);
                                }
                                let (img, worth) = f.textures.get_gpu_fast(&path)?;
                                let id = renderer.add_texture_data(scene, &img);
                                if worth {
                                    f.wants_upgrade.lock().push(path.clone());
                                }
                                attach_pbr(renderer, scene, &path, id);
                                shared.insert(path.clone(), (id, 1));
                                f.held.push(path);
                                Some(id)
                            })
                        };
                        // An empty string or a file not found leaves the slot its own
                        // texture from the mesh (with its addressing): a roller blind's idle
                        // "next" band then stays out of sight in its transparent border
                        // instead of covering the display as an untextured white plane.
                        let spec = match found {
                            Some(tex) => v.spec.with_freetex(f.key, tex, f.diffuse, f.item_only),
                            None => v.spec.clone(),
                        };
                        let p = spec.build(renderer, scene, v.base_tex);
                        f.cache.insert(key, p);
                        p
                    }
                };
            if !f.item_only {
                v.base = pair.0;
            }
            if f.item_only || !item_has_freetex {
                v.item = pair.1;
            }
        }
        if let Some(l) = &mut v.lights {
            let mut mask = 0u32;
            for (k, (_, var)) in l.maps.iter().enumerate() {
                let x = var
                    .trim()
                    .parse::<f32>()
                    .ok()
                    .or_else(|| vehicle.var(var))
                    .unwrap_or(0.0);
                // (on at 0.5, as each map's texture stage is, 0x7fe51f: a variable a script
                // dims through 0.1 lit the map at full)
                if x >= 0.5 {
                    mask |= 1 << k;
                }
            }
            if mask != l.current {
                l.current = mask;
                let pair = if mask == 0 {
                    l.plain
                } else if let Some(p) = l.cache.get(&mask) {
                    *p
                } else {
                    let tex = l.composite(renderer, scene, mask);
                    let mut spec = v.spec.clone();
                    spec.set_lightmap(tex);
                    let p = spec.build(renderer, scene, v.base_tex);
                    l.cache.insert(mask, p);
                    p
                };
                v.base = pair.0;
                v.item = pair.1;
            }
        }
        if let Some(inst) = render.instances.get(v.mesh) {
            let m = v.material(|n| vehicle.var(n));
            if ::legacy_config::env::var("OMSI_DEBUG_VARIANTS")
                .ok()
                .is_some_and(|f| {
                    !f.is_empty() && v.var.to_ascii_lowercase().contains(&f.to_ascii_lowercase())
                })
            {
                log::info!(
                    "variant mesh {} slot {} var {} = {:?}: material {m} (base {}, item {})",
                    v.mesh,
                    v.slot,
                    v.var,
                    vehicle.var(&v.var),
                    v.base,
                    v.item
                );
            }
            renderer.set_material(scene, *inst, v.slot, m);
        }
    }
}

/// A vehicle's `[interiorlight]`s (`variable range r g b x y z`) as lamps for this frame:
/// points of light at their place in the vehicle, as strong as their variable (0..1) times
/// `range` (1 for a saloon lamp, 0.4 for a door lamp, 2 for the LiAZ's saloon rows - a door
/// lamp 0.4 m across could not reach the step 2 m below it, so it is no distance), each
/// lighting only the meshes that list it in
/// their `[illumination_interior]` - OMSI's four per mesh, or as many as a model lists
/// (up to `::render::MAX_LAMPS_PER_MESH`), as OMSI switches those lights on
/// for just that mesh. Every set of lamps some mesh names gets a run of slots of its own
/// (the LiAZ 5292 has 32 lamps; only the first eight were drawn, and its saloon stayed dark).
/// The seats' sets (`PassPos::illumination`, the lamps that light a person sitting there)
/// get theirs too: see [`VehicleRender::seat_lamps`].
pub(super) fn sync_interior_lamps(
    renderer: &Renderer,
    scene: &mut Scene,
    ty: &::simulation::VehicleType,
    position: DVec3,
    rotation: Mat4,
    var: impl Fn(&str) -> Option<f32>,
    render: &VehicleRender,
) {
    let n = ty.model.interior_lights.len();
    if n == 0 || render.instances.is_empty() {
        return;
    }
    let blocks = render.interior_blocks.get_or_init(|| {
        let mut blocks: Vec<(u32, Vec<usize>)> = Vec::new();
        let mut sets: Vec<(usize, Vec<usize>)> = Vec::new();
        for (i, _) in render.instances.iter().enumerate() {
            let Some(vm) = ty.meshes.get(i) else { continue };
            let set = lamp_set(&ty.model.meshes[vm.def_index].illumination_interior, n);
            if !set.is_empty() {
                sets.push((i, set));
            }
        }
        let mut distinct: Vec<Vec<usize>> = Vec::new();
        for (_, set) in &sets {
            if !distinct.contains(set) {
                distinct.push(set.clone());
            }
        }
        if let Some(cabin) = crate::driver::cabin_of(&ty.def) {
            for seat in cabin.driver_positions.iter().chain(&cabin.pass_positions) {
                let set = lamp_set(&seat.illumination, n);
                if !set.is_empty() && !distinct.contains(&set) {
                    distinct.push(set);
                }
            }
        }
        let total: u32 = distinct.iter().map(|d| d.len() as u32).sum();
        if ::legacy_config::env::var_os("OMSI_DEBUG_INTERIOR").is_some() {
            log::info!(
                "interior lamps of {}: {} lamps, {} instances, {} meshes lit by sets {:?}",
                ty.def.path.display(),
                n,
                render.instances.len(),
                sets.len(),
                distinct
            );
        }
        if total == 0 {
            return blocks;
        }
        let first = renderer.alloc_interior_lights(scene, total);
        render.interior_lamps.set(Some((first, total)));
        let mut at = first;
        for d in distinct {
            blocks.push((at, d.clone()));
            at += d.len() as u32;
        }
        for (i, set) in &sets {
            if let Some(b) = blocks.iter().find(|b| &b.1 == set) {
                renderer.set_interior_lamps(scene, render.instances[*i], b.0, set.len() as u32);
            }
        }
        blocks
    });
    for (first, set) in blocks {
        for (k, &li) in set.iter().enumerate() {
            let il = &ty.model.interior_lights[li];
            // on or off: OMSI enables the lamp when its variable is 0.5 or more
            //, there is no dimming
            let on = il
                .variable
                .trim()
                .parse::<f32>()
                .ok()
                .or_else(|| var(&il.variable))
                .unwrap_or(0.0)
                >= 0.5;
            let ic = crate::lights::interior_cfg(li);
            let on = on && !ic.off;
            let at =
                rotation.transform_vector3(glam::Vec3::from(il.pos) + glam::Vec3::from(ic.shift));
            renderer.set_interior_light(
                scene,
                first + k as u32,
                ::render::PointLight {
                    position: position + at.as_dvec3(),
                    // OMSI's Direct3D light: a point light of the
                    // colour / 255, Range 100 m, attenuation 1 / (d² / range²) - full
                    // light at `range` metres, stronger closer in, a quarter at twice
                    radius: 100.0,
                    core: (il.range * ic.range).max(0.01),
                    color: [
                        il.color[0] / 255.0 * ic.color[0],
                        il.color[1] / 255.0 * ic.color[1],
                        il.color[2] / 255.0 * ic.color[2],
                    ],
                    intensity: if on { ic.gain } else { 0.0 },
                    ..Default::default()
                },
            );
        }
    }
}

/// The lamps (indices into the model's `[interiorlight]`s) a mesh's or a seat's
/// `[illumination_interior]` names: those that exist, each once, as many as a mesh may have.
pub(super) fn lamp_set(indices: &[i32], n: usize) -> Vec<usize> {
    let mut set: Vec<usize> = Vec::new();
    for &k in indices {
        if k >= 0
            && (k as usize) < n
            && !set.contains(&(k as usize))
            && set.len() < ::render::MAX_LAMPS_PER_MESH as usize
        {
            set.push(k as usize);
        }
    }
    set
}

impl VehicleRender {
    /// The lamp slots (first, count) for `set_interior_lamps` that light a person on a seat
    /// with these four lamps (`PassPos::illumination`) in a vehicle of `n` lamps; None
    /// before the vehicle's lamps are first synced, or when none of them exists.
    pub fn seat_lamps(&self, n: usize, lamps: &[i32; 4]) -> Option<(u32, u32)> {
        let set = lamp_set(lamps, n);
        let blocks = self.interior_blocks.get()?;
        blocks
            .iter()
            .find(|b| b.1 == set)
            .map(|b| (b.0, set.len() as u32))
    }
}

pub fn sync_vehicle_textures(
    renderer: &Renderer,
    scene: &mut Scene,
    vehicle: &mut ::simulation::VehicleInstance,
    render: &VehicleRender,
    budget: &mut usize,
) {
    vehicle.update_html_textures();
    sync_interior_lamps(
        renderer,
        scene,
        &vehicle.ty,
        vehicle.position,
        vehicle.body_rotation(),
        |n| vehicle.var(n),
        render,
    );
    for i in vehicle.update_text_textures() {
        if let (Some(Some(tex)), Some(img)) = (
            render.text_textures.get(i),
            vehicle.text_textures[i].pending.take(),
        ) {
            let d = &vehicle.text_textures[i].def;
            renderer.update_texture_mips(
                scene,
                *tex,
                &Image {
                    width: d.width.max(1) as u32,
                    height: d.height.max(1) as u32,
                    rgba: img,
                    has_alpha: true,
                },
            );
        }
    }
    let mut rebound = Vec::new();
    for (i, st) in vehicle.host.script_textures.iter_mut().enumerate() {
        // (far away what the scripts redraw goes up every half second: `displays_far`)
        if !render.displays_far {
            if let Some(Some(tex)) = render.script_textures.get(i) {
                if *budget == 0 {
                    continue;
                }
                let Some(rgba) = st.take_upload() else {
                    continue;
                };
                *budget = budget.saturating_sub(rgba.len());
                let img = Image {
                    width: st.width,
                    height: st.height,
                    rgba,
                    has_alpha: true,
                };
                if st.mipmaps {
                    if renderer.update_texture_mips(scene, *tex, &img) {
                        rebound.push(*tex);
                    }
                } else {
                    renderer.update_texture(scene, *tex, &img);
                }
            }
        }
    }
    renderer.rebind_textures(scene, &rebound);
}

/// Identify a solid vehicle body material that should participate in the depth buffer.
/// Some bus packs put either `[matl_alpha] 2` or `[matl_noZcheck]` on a complete body mesh.
/// The decision must not depend on one creator's language or on a particular bus name:
/// use the model metadata and the material's actual mesh volume, while keeping thin glass
/// and explicit overlay/transparency materials on their authored paths.
/// Words that name a pane of glass in a mesh or texture file, in the languages OMSI's
/// add-ons are made in. The body-depth repair must not turn one of these opaque when the
/// model.cfg declares it blended: a Czech bus's `okna.o3d` (windows) on the shared
/// `body.png` was drawn as a black wall, where OMSI shows the tinted glass.
pub(super) const GLASS_WORDS: [&str; 20] = [
    "window",
    "fenster",
    "glas",
    "scheibe",
    "windshield",
    "windscreen", // en, de ("glas" is also German: `Leuchtmelderglas.tga`)
    "okn",
    "sklo", // cs, sk (okna, okno, sklo)
    "szyb",
    "okien",  // pl
    "ablak",  // hu
    "steklo", // ru (transliterated)
    "vitre",
    "fenetre", // fr
    "vetro",
    "finestr", // it
    "raam",
    "ruit", // nl
    "ventan",
    "cristal", // es
];

pub(super) fn is_vehicle_body_material(
    mesh_file: &str,
    texture: &str,
    has_texture: bool,
    has_transmap: bool,
    no_z_write: bool,
    body_hint: bool,
) -> bool {
    if !has_texture || has_transmap || no_z_write || !body_hint {
        return false;
    }
    let name = format!("{} {}", mesh_file, texture).to_ascii_lowercase();
    let overlay = [
        "regen", "dirt", "dreck", "wiper", "matrix", "display", "shadow",
    ];
    if GLASS_WORDS
        .iter()
        .chain(overlay.iter())
        .any(|part| name.contains(part))
    {
        return false;
    }
    true
}

/// Side of the square a texture's alpha is kept at for [`slot_is_see_through`].
pub(super) const ALPHA_MASK: usize = 256;

/// A texture's alpha channel, thinned out to [`ALPHA_MASK`] squared (a body texture is
/// 4096 squared, and every blended slot of a bus asks). None for a file that cannot be
/// read or has no alpha.
pub(super) fn alpha_mask(path: &Path) -> Option<Arc<Vec<u8>>> {
    static MASKS: std::sync::OnceLock<Mutex<HashMap<PathBuf, Option<Arc<Vec<u8>>>>>> =
        std::sync::OnceLock::new();
    let masks = MASKS.get_or_init(Default::default);
    if let Some(m) = masks.lock().get(path) {
        return m.clone();
    }
    let mask = ::texture::decode_file(path)
        .ok()
        .filter(|img| img.has_alpha && img.width > 0 && img.height > 0)
        .map(|img| {
            let (w, h) = (img.width as usize, img.height as usize);
            let mut out = vec![255u8; ALPHA_MASK * ALPHA_MASK];
            for y in 0..ALPHA_MASK {
                for x in 0..ALPHA_MASK {
                    let (sx, sy) = (
                        (x * w / ALPHA_MASK).min(w - 1),
                        (y * h / ALPHA_MASK).min(h - 1),
                    );
                    out[y * ALPHA_MASK + x] = img.rgba[(sy * w + sx) * 4 + 3];
                }
            }
            Arc::new(out)
        });
    masks.lock().insert(path.to_path_buf(), mask.clone());
    mask
}

/// Whether the triangles of material `slot` lie on a see-through part of their texture
/// (`mask`, see [`alpha_mask`]): nine in ten of them with an alpha under 0.9 at their
/// middle. A pane does - the SOR NB12's glass is 62 of 255 on its body texture; a door or
/// a body panel blended by `[matl_alpha] 2` has its paint at 255 and does not.
pub(super) fn slot_is_see_through(mesh: &MeshData, slot: usize, mask: &[u8]) -> bool {
    let (mut clear, mut all) = (0usize, 0usize);
    for &(first, count, material) in &mesh.ranges {
        if material as usize != slot {
            continue;
        }
        let start = first as usize;
        let end = start.saturating_add(count as usize).min(mesh.indices.len());
        for tri in mesh
            .indices
            .get(start..end)
            .unwrap_or_default()
            .chunks_exact(3)
        {
            let Some(uv) = tri
                .iter()
                .map(|&i| mesh.uvs.get(i as usize).copied())
                .sum::<Option<glam::Vec2>>()
            else {
                continue;
            };
            let uv = uv / 3.0;
            let at =
                |t: f32| ((t.rem_euclid(1.0) * ALPHA_MASK as f32) as usize).min(ALPHA_MASK - 1);
            all += 1;
            if mask[at(uv.y) * ALPHA_MASK + at(uv.x)] < 230 {
                clear += 1;
            }
        }
    }
    all > 0 && clear * 10 >= all * 9
}

/// Return whether the triangles of one material occupy a volumetric part of the vehicle.
/// Windows and rain films are normally very thin sheets; this lets unnamed/modded body
/// meshes be repaired without maintaining a language-specific list of mesh names.
pub(super) fn material_has_vehicle_volume(mesh: &MeshData, slot: usize) -> bool {
    let mut lo = glam::Vec3::splat(f32::MAX);
    let mut hi = glam::Vec3::splat(f32::MIN);
    let mut points = 0usize;
    for &(first, count, material) in &mesh.ranges {
        if material as usize != slot {
            continue;
        }
        let start = first as usize;
        let end = start.saturating_add(count as usize).min(mesh.indices.len());
        for &index in mesh.indices.get(start..end).unwrap_or_default() {
            if let Some(p) = mesh.positions.get(index as usize) {
                lo = lo.min(*p);
                hi = hi.max(*p);
                points += 1;
            }
        }
    }
    if points < 8 || !lo.x.is_finite() || !hi.x.is_finite() {
        return false;
    }
    let extent = hi - lo;
    let mut sides = [extent.x.abs(), extent.y.abs(), extent.z.abs()];
    sides.sort_by(f32::total_cmp);
    // A solid shell has meaningful thickness compared with both of its other dimensions.
    // A windshield or side pane may be wide and tall, but remains a sheet in its thin axis.
    sides[2] > 3.0
        && sides[1] > 1.0
        && sides[0] > 0.5
        && sides[0] / sides[1] > 0.25
        && sides[0] / sides[2] > 0.02
}

/// Whether the triangles of material `slot` lie on the faces of another slot of the same
/// mesh: a layer modelled as a copy of the surface under it with a material of its own (a
/// baked ambient-occlusion or shading film over the floor, `[matl_alpha] 2`), which OMSI
/// blends over the surface as declared.
pub(super) fn slot_overlays_another(mesh: &MeshData, slot: usize) -> bool {
    let key = |p: &glam::Vec3| {
        (
            (p.x * 1000.0).round() as i32,
            (p.y * 1000.0).round() as i32,
            (p.z * 1000.0).round() as i32,
        )
    };
    let mut own = std::collections::HashSet::new();
    let mut others = std::collections::HashSet::new();
    for &(first, count, material) in &mesh.ranges {
        let start = first as usize;
        let end = start.saturating_add(count as usize).min(mesh.indices.len());
        let set = if material as usize == slot {
            &mut own
        } else {
            &mut others
        };
        for &index in mesh.indices.get(start..end).unwrap_or_default() {
            if let Some(p) = mesh.positions.get(index as usize) {
                set.insert(key(p));
            }
        }
    }
    own.len() >= 3 && own.iter().filter(|k| others.contains(*k)).count() * 10 >= own.len() * 9
}

/// GPU-side representation of a vehicle instance: one render instance per mesh.
pub struct VehicleRender {
    pub window_wipers: Option<crate::window_wipers::WindowWipers>,
    pub instances: Vec<usize>,
    /// Materials made for this vehicle alone (its text and script texture slots).
    pub own_materials: Vec<MaterialId>,
    /// The shared set it is drawn with (None for the player's own).
    pub set: Option<VehicleKey>,
    /// `[matl_change]` / `[texchanges]` slots whose material a variable switches.
    pub variants: Vec<VariantSlot>,
    /// GPU texture per `[texttexture]` index.
    pub text_textures: Vec<Option<TextureId>>,
    /// GPU texture per `[scripttexture]` index.
    pub script_textures: Vec<Option<TextureId>>,
    /// `script_textures` belong to the vehicle this part is coupled to (`[scriptshare]`):
    /// they are not this render's to give back.
    pub shared_script: bool,
    /// The script textures (cockpit and passenger displays, 1024×512 pictures for a C2) wait
    /// with their upload this frame: the vehicle is far and it is not its half second.
    pub displays_far: bool,
    /// The half second a far vehicle's displays were last uploaded in.
    pub display_tick: u64,
    /// `[smoothskin]` meshes drawn from a copy of their own (the player's articulated bus's
    /// bellows): (mesh index, the copy, the bone transforms it was last shaped for).
    pub skinned: Vec<(usize, MeshId, Vec<Mat4>)>,
    /// An AI vehicle out of sight: its instances are hidden and not updated (see
    /// `Traffic::sync`).
    pub hidden: bool,
    /// The renderer's slots for the vehicle's `[interiorlight]` lamps (first, count), taken
    /// the first time the vehicle is synced (see `sync_interior_lamps`).
    pub interior_lamps: std::cell::Cell<Option<(u32, u32)>>,
    /// The runs of lamp slots and which of the vehicle's lamps each holds.
    pub interior_blocks: std::cell::OnceCell<Vec<(u32, Vec<usize>)>>,
}

/// A material slot whose texture is generated per vehicle (text or script texture).
#[derive(Debug, Clone)]
pub struct DynSlot {
    pub mesh: usize,
    pub slot: usize,
    pub text: Option<usize>,
    pub script: Option<usize>,
    pub script_trans: Option<usize>,
    pub tex: Option<TextureId>,
    pub alpha: AlphaMode,
    pub transmap: Option<(TextureId, bool)>,
    pub night: Option<TextureId>,
    pub lightmap: Option<TextureId>,
    pub envmap: Option<(TextureId, f32)>,
    /// `[matl_texadress_*]`: how the slot's textures read outside [0, 1].
    pub address: ::render::TexAddressing,
    /// Depth handling, reflection mask and specular term of the slot.
    pub extra: MaterialExtra,
    /// Diffuse and emissive colour of the slot's D3D material (see `d3d_material`); a
    /// script texture is drawn unlit and keeps its own colours.
    pub color: [f32; 4],
    pub emissive: [f32; 3],
}
