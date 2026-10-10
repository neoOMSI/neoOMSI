use super::*;

impl World {
    /// Place a tile whose textures and object types are on the GPU - the ground, then the
    /// splines, the trees and the objects - until `deadline` (always a little: at least the
    /// ground or one spline, a few trees or one object). True when all of it is placed.
    /// A big tile (a main station with a few thousand objects and signs) took a third of a
    /// second in one piece.
    pub fn place_step(
        &self,
        renderer: &Renderer,
        scene: &mut Scene,
        u: &mut PendingUpload,
        deadline: Option<std::time::Instant>,
    ) -> bool {
        let t_lock = std::time::Instant::now();
        let mut gpu_guard = self.gpu.lock();
        let lock_wait = t_lock.elapsed().as_secs_f64();
        let gpu = &mut *gpu_guard;
        self.ensure_ground(renderer, scene, gpu);
        let (
            ground_id,
            ground_mat,
            plain_terrain_mat,
            ground_detail,
            ground_repeats,
            ground_wet,
            water_mat,
            tree_mesh,
        ) = {
            let g = gpu.ground.as_ref().unwrap();
            (
                g.ground_id,
                g.ground_mat,
                g.plain_terrain_mat,
                g.ground_detail,
                g.ground_repeats,
                g.ground_wet,
                g.water_mat,
                g.tree_mesh,
            )
        };
        let PendingUpload {
            prepared: p,
            tg,
            placing: pl,
            ..
        } = u;
        let key = (p.tx, p.ty);
        let images: &HashMap<PathBuf, Arc<TextureData>> = &p.images;
        let ground_dirs = vec![self.root.clone()];
        let only_object = ::legacy_config::env::var("OMSI_ONLY_OBJECT").is_ok();
        let decodes_before = (gpu.sync_decodes, gpu.sync_decode_secs);
        let t_start = std::time::Instant::now();
        let out_of_time = |done_some: bool| {
            done_some
                && deadline
                .map(|d| std::time::Instant::now() >= d)
                .unwrap_or(false)
        };
        macro_rules! instance {
            ($id:expr) => {{
                let new = $id;
                let i = gpu.instance(renderer, scene, new);
                tg.instances.push(i);
                i
            }};
        }
        let mut done_some = false;
        while pl.phase < 4 && !out_of_time(done_some) {
            let t_phase = std::time::Instant::now();
            let phase = pl.phase;
            match phase {
                0 => {
                    if let (Some(mesh), false) = (&p.terrain, only_object) {
                        let id = gpu.add_mesh(renderer, scene, mesh);
                        tg.meshes.push(id);
                        // the tile's night light map (lamp light pools on the ground)
                        let lm = p.light_map.as_ref().map(|img| {
                            let t = gpu.add_data(renderer, scene, img);
                            tg.textures.push(t);
                            t
                        });
                        let mat = match &p.cut {
                            Some(img) => {
                                let tex = gpu.add_data(renderer, scene, img);
                                tg.textures.push(tex);
                                let m = renderer.add_terrain_material(
                                    scene,
                                    ground_id,
                                    Some(tex),
                                    ground_detail,
                                    ground_repeats,
                                    lm,
                                    ground_wet,
                                );
                                let m = gpu.material(renderer, scene, m);
                                tg.materials.push(m);
                                m
                            }
                            None if lm.is_some() => {
                                let m = renderer.add_terrain_material(
                                    scene,
                                    ground_id,
                                    None,
                                    ground_detail,
                                    ground_repeats,
                                    lm,
                                    ground_wet,
                                );
                                let m = gpu.material(renderer, scene, m);
                                tg.materials.push(m);
                                m
                            }
                            None => plain_terrain_mat,
                        };
                        let ground_instance = instance!(renderer.add_instance(
                            scene,
                            id,
                            p.origin,
                            Mat4::IDENTITY,
                            vec![mat]
                        ));
                        if let Some(inst) = scene.instances.get_mut(ground_instance) {
                            inst.render_phase = RenderPhase::Terrain;
                        }
                        // (the base layer once more without the cut, when the tile has one)
                        let uncut = match (&p.cut, lm) {
                            (None, _) => mat,
                            (Some(_), None) => plain_terrain_mat,
                            (Some(_), Some(_)) => {
                                let m = renderer.add_terrain_material(
                                    scene,
                                    ground_id,
                                    None,
                                    ground_detail,
                                    ground_repeats,
                                    lm,
                                    ground_wet,
                                );
                                let m = gpu.material(renderer, scene, m);
                                tg.materials.push(m);
                                m
                            }
                        };
                        pl.terrain_mapping_mat = Some(uncut);
                        // The painted ground: every further [groundtex] the editor's brush put on this
                        // tile is the same tile mesh once more, blended in through its own mask - which
                        // is how OMSI's car parks get their asphalt, its side streets their cobbles and
                        // its meadows their fields.
                        let no_paint = ::legacy_config::env::var_os("OMSI_NO_GROUND_PAINT").is_some();
                        for (layer, mask, painted) in p.paint.iter().filter(|_| !no_paint) {
                            let Some(gt) = self.global.ground_textures.get(*layer) else {
                                continue;
                            };
                            let tex = gpu.add_data(renderer, scene, mask);
                            tg.textures.push(tex);
                            let layer_tex = gpu
                                .texture(renderer, scene, &gt.texture, &ground_dirs, images)
                                .map(|(id, path)| {
                                    tg.shared_textures.push(path);
                                    id
                                });
                            let detail = gpu
                                .texture(renderer, scene, &gt.detail_texture, &ground_dirs, images)
                                .map(|(id, path)| {
                                    tg.shared_textures.push(path);
                                    (id, gt.detail_repeats())
                                });
                            let gdirs: Vec<&Path> =
                                ground_dirs.iter().map(|p| p.as_path()).collect();
                            let gcfg = self.textures.cfg(&gt.texture, &gdirs);
                            let wet = gcfg.moisture || gcfg.puddles;
                            let m = renderer.add_terrain_layer_material(
                                scene,
                                layer_tex,
                                tex,
                                detail,
                                gt.repeats(),
                                lm,
                                if wet { 1.0 } else { 0.0 },
                            );
                            let m = gpu.material(renderer, scene, m);
                            tg.materials.push(m);
                            let li = instance!(renderer.add_surface_instance(
                                scene,
                                id,
                                p.origin,
                                Mat4::IDENTITY,
                                vec![m]
                            ));
                            if let Some(inst) = scene.instances.get_mut(li) {
                                inst.ground_layer = true;
                                inst.render_phase = RenderPhase::Terrain;
                            }
                            if ::legacy_config::env::var_os("OMSI_DEBUG_SURFACES").is_some() {
                                log::info!(
                                    "tile ({}, {}): ground layer {layer} '{}' painted on {:.1} % of the tile, mask {:?}",
                                    p.tx,
                                    p.ty,
                                    gt.texture,
                                    painted * 100.0,
                                    mask.format
                                );
                            }
                        }
                        // the tile's water surface: one quad at the four corner heights, drawn over the
                        // riverbed. OMSI keeps it in `tile.map.water`, one height per corner.
                        if let Some(h) = p.water {
                            let t = tile_size() as f32;
                            let mut wm = MeshData::default();
                            for (i, (x, y)) in [(0.0, 0.0), (t, 0.0), (0.0, t), (t, t)]
                                .into_iter()
                                .enumerate()
                            {
                                wm.positions.push(glam::Vec3::new(x, y, h[i]));
                                wm.normals.push(glam::Vec3::Z);
                                wm.uvs.push(glam::Vec2::new(x / 40.0, y / 40.0));
                            }
                            wm.indices.extend_from_slice(&[0, 1, 2, 2, 1, 3]);
                            wm.ranges.push((0, 6, 0));
                            let wid = gpu.add_mesh(renderer, scene, &wm);
                            tg.meshes.push(wid);
                            if ::legacy_config::env::var_os("OMSI_DEBUG_SURFACES").is_some() {
                                log::info!(
                                    "tile ({}, {}): water at {:.1}..{:.1} m, centred ({:.0}, {:.0})",
                                    p.tx,
                                    p.ty,
                                    h.iter().cloned().fold(f32::MAX, f32::min),
                                    h.iter().cloned().fold(f32::MIN, f32::max),
                                    p.origin.x + tile_size() / 2.0,
                                    p.origin.y + tile_size() / 2.0
                                );
                            }
                            // an ordinary instance, not a surface: the surface depth bias would let a
                            // tile-wide water quad win the depth test against the banks and flood the
                            // whole tile when seen at a shallow angle
                            let _ = instance!(renderer.add_instance(
                                scene,
                                wid,
                                p.origin,
                                Mat4::IDENTITY,
                                vec![water_mat]
                            ));
                        }
                    }
                    pl.phase = 1;
                    pl.next = 0;
                    done_some = true;
                }
                1 => {
                    if !only_object && pl.ground_next < p.ground_splines.len() {
                        let mesh = &p.ground_splines[pl.ground_next];
                        pl.ground_next += 1;
                        let id = gpu.add_mesh(renderer, scene, mesh);
                        scene.meshes[id].source = Some("terrain-mapped spline cells".to_string());
                        tg.meshes.push(id);
                        if let Some(mat) = pl.terrain_mapping_mat {
                            let si = instance!(renderer.add_surface_instance(
                                scene,
                                id,
                                p.origin,
                                Mat4::from_translation(glam::Vec3::Z * OMSI_SURFACE_LIFT),
                                vec![mat]
                            ));
                            if let Some(inst) = scene.instances.get_mut(si) {
                                inst.render_phase = RenderPhase::Spline;
                                inst.surface_bias = false;
                            }
                        }
                        done_some = true;
                        pl.secs[1] += t_phase.elapsed().as_secs_f64();
                        continue;
                    }
                    if only_object || pl.next >= p.splines.len() {
                        pl.phase = 2;
                        pl.next = 0;
                        continue;
                    }
                    let (mesh, st, casts_shadow, sort_origin) = &p.splines[pl.next];
                    pl.next += 1;
                    let skey = Arc::as_ptr(st) as usize;
                    if !gpu.splines.contains_key(&skey) {
                        let dirs = texture_dirs(&self.root, &st.dir);
                        let mut sg = SplineGpu {
                            _st: st.clone(),
                            materials: Vec::new(),
                            textures: Vec::new(),
                            users: 0,
                            terrain: Vec::new(),
                        };
                        for t in &st.def.textures {
                            let (tex, texture_has_alpha) =
                                match gpu.texture(renderer, scene, &t.file, &dirs, images) {
                                    Some((id, path)) => {
                                        let has_alpha = gpu.has_alpha(&path);
                                        sg.textures.push(path);
                                        (Some(id), has_alpha)
                                    }
                                    None => (None, false),
                                };
                            // OMSI spline [matl_alpha] uses 0 = opaque, 1 = alpha test,
                            // and 2 = blend.  Like the C++ handler, a declared blend on
                            // a texture without alpha is opaque; otherwise the surface
                            // belongs in the blended pass, not the depth-writing cutout
                            // pass.  Blended spline overlaps also need depth writes off,
                            // matching the reference handler's far-to-near spline pass.
                            let alpha = match (t.alpha, texture_has_alpha) {
                                (1, _) => AlphaMode::Test,
                                (mode, true) if mode >= 2 => AlphaMode::Blend,
                                _ => AlphaMode::Opaque,
                            };
                            let dirs_ref: Vec<&Path> = dirs.iter().map(|p| p.as_path()).collect();
                            let cfg = self.textures.cfg(&t.file, &dirs_ref);
                            let wet = cfg.moisture || cfg.puddles;
                            if cfg.terrain_mapping {
                                sg.terrain.push(sg.materials.len());
                            }
                            // (lit at night by the tile's light map, as OMSI lights the roads)
                            renderer.light_map_next.set(true);
                            let m = renderer.add_material_extra(
                                scene,
                                tex,
                                alpha,
                                [1.0; 4],
                                false,
                                None,
                                None,
                                None,
                                None,
                                [0.0; 3],
                                MaterialExtra {
                                    no_z_write: alpha == AlphaMode::Blend,
                                    moisture: if wet { 1.0 } else { 0.0 },
                                    ..MaterialExtra::default()
                                },
                            );
                            let m = gpu.material(renderer, scene, m);
                            sg.materials.push(m);
                        }
                        gpu.splines.insert(skey, sg);
                    }
                    let sg = gpu.splines.get_mut(&skey).unwrap();
                    if !tg.spline_types.contains(&skey) {
                        sg.users += 1;
                        tg.spline_types.push(skey);
                    }
                    let mats = if sg.materials.is_empty() {
                        vec![ground_mat]
                    } else {
                        sg.materials.clone()
                    };
                    if ::legacy_config::env::var_os("OMSI_DEBUG_SPLINES").is_some() {
                        let mean_nz = mesh.normals.iter().map(|n| n.z).sum::<f32>()
                            / mesh.normals.len().max(1) as f32;
                        log::info!(
                            "upload spline {} origin={:?} ranges={:?} mats={:?} mean normal z={mean_nz:+.2} verts={} first positions {:?}",
                            st.def.path.display(),
                            p.origin,
                            &mesh.ranges[..mesh.ranges.len().min(3)],
                            mats,
                            mesh.positions.len(),
                            &mesh.positions[..mesh.positions.len().min(3)]
                        );
                    }
                    // [terrainmapping] slots take only the first ground texture. The
                    // spline mesh is already in tile space, which supplies the ground UVs.
                    let terrain: Vec<usize> = if pl.terrain_mapping_mat.is_none() {
                        Vec::new()
                    } else {
                        sg.terrain
                            .iter()
                            .copied()
                            .filter(|t| mesh.ranges.iter().any(|r| r.2 as usize == *t))
                            .collect()
                    };
                    let id = if terrain.is_empty() {
                        gpu.add_mesh(renderer, scene, mesh)
                    } else {
                        let ground =
                            terrain_ground(mesh, &terrain, p.origin, Mat4::IDENTITY, p.origin);
                        let gid = gpu.add_mesh(renderer, scene, &ground);
                        scene.meshes[gid].source = Some(st.def.path.display().to_string());
                        tg.meshes.push(gid);
                        if let Some(mat) = pl.terrain_mapping_mat {
                            let terrain_instance = instance!(renderer.add_surface_instance(
                                scene,
                                gid,
                                p.origin,
                                Mat4::from_translation(glam::Vec3::Z * OMSI_SURFACE_LIFT),
                                vec![mat],
                            ));
                            if let Some(inst) = scene.instances.get_mut(terrain_instance) {
                                inst.render_phase = RenderPhase::Spline;
                                inst.surface_bias = false;
                                inst.blend_sort_origin = Some(*sort_origin);
                            }
                        }
                        gpu.add_mesh(renderer, scene, &terrain_rest(mesh, &terrain))
                    };
                    tg.meshes.push(id);
                    scene.meshes[id].source = Some(st.def.path.display().to_string());
                    let si = instance!(renderer.add_surface_instance(
                        scene,
                        id,
                        p.origin,
                        Mat4::from_translation(glam::Vec3::Z * OMSI_SURFACE_LIFT),
                        mats
                    ));
                    if let Some(inst) = scene.instances.get_mut(si) {
                        inst.render_phase = RenderPhase::Spline;
                        inst.surface_bias = false;
                        inst.blend_sort_origin = Some(*sort_origin);
                    }
                    // a bridge deck or an elevated railway casts a sun shadow (see
                    // `SPLINE_SHADOW_CLEARANCE`)
                    if *casts_shadow {
                        renderer.set_casts_shadow(scene, si, true);
                    }
                    pl.splines += 1;
                    done_some = true;
                }
                2 => {
                    if only_object || pl.next >= p.trees.len() {
                        pl.phase = 3;
                        pl.next = 0;
                        continue;
                    }
                    // trees are cheap: a few dozen at a time
                    let end = (pl.next + 64).min(p.trees.len());
                    for (ot, texture, pos, height, width, heading) in &p.trees[pl.next..end] {
                        let tkey = texture.to_ascii_lowercase();
                        if !gpu.trees.contains_key(&tkey) {
                            let dirs = texture_dirs(&self.root, &ot.model_dir);
                            let found = gpu.texture(renderer, scene, texture, &dirs, images);
                            let m = renderer.add_material_extra(
                                scene,
                                found.as_ref().map(|f| f.0),
                                AlphaMode::Test,
                                [1.0; 4],
                                false,
                                None,
                                None,
                                None,
                                None,
                                [0.0; 3],
                                ::render::MaterialExtra {
                                    tree: true,
                                    ..Default::default()
                                },
                            );
                            let m = gpu.material(renderer, scene, m);
                            gpu.trees.insert(
                                tkey.clone(),
                                TreeGpu {
                                    material: m,
                                    texture: found.map(|f| f.1),
                                    users: 0,
                                },
                            );
                        }
                        let tr = gpu.trees.get_mut(&tkey).unwrap();
                        if !tg.trees.contains(&tkey) {
                            tr.users += 1;
                            tg.trees.push(tkey.clone());
                        }
                        let mat = tr.material;
                        let xf = Mat4::from_rotation_z((-heading).to_radians() as f32)
                            * Mat4::from_scale(glam::Vec3::new(
                            *width as f32,
                            *width as f32,
                            *height as f32,
                        ));
                        let _ =
                            instance!(renderer.add_instance(scene, tree_mesh, *pos, xf, vec![mat]));
                        pl.trees += 1;
                    }
                    pl.next = end;
                    done_some = true;
                }
                _ => {
                    let Some(o) = p.objects.pop() else {
                        pl.phase = 4;
                        continue;
                    };
                    let PlacedObject {
                        ot,
                        pos,
                        xf,
                        lamp,
                        map_id,
                        key: collision_key,
                        controller,
                        strings,
                        var_parent,
                        parked,
                        editable,
                        script: mut early_script,
                    } = o;
                    let tkey = self.type_gpu(renderer, scene, gpu, &ot, images, ground_mat);
                    if !tg.types.contains(&tkey) {
                        gpu.types.get_mut(&tkey).unwrap().users += 1;
                        tg.types.push(tkey);
                    }
                    let (
                        type_meshes,
                        type_variants,
                        mut type_lods,
                        type_auto_night,
                        lod0_lo,
                        lod0_max,
                        terrain_slots,
                    ) = {
                        let t = &gpu.types[&tkey];
                        (
                            t.meshes.clone(),
                            t.variants.clone(),
                            t.lods.clone(),
                            t.auto_night,
                            t.lod0_lo,
                            t.lod0_max,
                            t.terrain_slots.clone(),
                        )
                    };
                    let surface = ot.sco.render_type.is_ground_layer() || ot.sco.surface;
                    let render_phase = scenery_render_phase(ot.sco.render_type);
                    let draw_pos = scenery_draw_position(pos, drawn_on_surfaces(&ot.sco));
                    let has_lower = !type_lods.is_empty();
                    let mut lamp_instances = Vec::new();
                    let mut lamp_slots = Vec::new();
                    let mut all_instances = Vec::new();
                    let mut object_variants: Vec<SceneryVariant> =
                        Vec::new();
                    let mut script_texts: Vec<(TextureId, ::simulation::texttex::TextTextureState)> =
                        Vec::new();
                    // `[htmltexture]` pages shown on this object: (script texture index, texture)
                    let mut html_pages: Vec<(usize, TextureId)> = Vec::new();
                    let mut html_mats: HashMap<usize, MaterialId> = HashMap::new();
                    // Run {init} once for this placement: its variable values choose CTC
                    // schemes and its strings can name [matl_freetex] pictures.
                    let needs_own_script = lamp.is_none()
                        || ot.meshes.iter().any(|(_, _, overrides)| {
                        overrides.iter().any(|o| !o.item && o.freetex.is_some())
                    });
                    // (a model with `[htmltexture]` pages or `[matl_freetex]` needs a script instance to feed them,
                    // also when the object has no script of its own)
                    let has_pages = lamp.is_none() && !ot.model.html_textures.is_empty();
                    let has_freetex = ot.meshes.iter().any(|(_, _, overrides)| {
                        overrides.iter().any(|o| !o.item && o.freetex.is_some())
                    });
                    let mut object_script = if needs_own_script {
                        let program = ot.program.clone().or_else(|| {
                            (has_pages || (has_freetex && !strings.is_empty()))
                                .then(|| Arc::new(::legacy_script::Program::default()))
                        });
                        program.map(|program| {
                            let mut inst = early_script.take().unwrap_or_else(|| {
                                ::simulation::scenery::SceneryInstance::new(
                                    program,
                                    &ot.mesh_defs(),
                                    self.script_clock(),
                                    &strings,
                                )
                            });
                            if has_pages {
                                let object_dir =
                                    ot.sco.path.parent().unwrap_or(std::path::Path::new(""));
                                inst.init_html_textures(
                                    &ot.model.html_textures,
                                    &ot.model_dir,
                                    object_dir,
                                );
                            }
                            inst
                        })
                    } else {
                        None
                    };
                    // Some signs derive filenames in {frame}. Probe on a separate
                    // instance: its placeholder inputs must not mutate the live script
                    // state or retain queued sounds/animations.
                    let freetex_probe = if ot.meshes.iter().any(|(_, _, overrides)| {
                        overrides.iter().any(|o| !o.item && o.freetex.is_some())
                    }) {
                        ot.program.as_ref().map(|program| {
                            let mut probe = ::simulation::scenery::SceneryInstance::new(
                                program.clone(),
                                &ot.mesh_defs(),
                                self.script_clock(),
                                &strings,
                            );
                            probe.update(
                                0.0,
                                &::simulation::scenery::SceneryVars {
                                    in_use: 1.0,
                                    ..Default::default()
                                },
                            );
                            probe
                        })
                    } else {
                        None
                    };
                    let mut mesh_list = type_meshes.clone();
                    // [terrainmapping] slots: drawn with the uncut base ground, from a mesh
                    // of this placement's own (see split_terrain_mapped); (level, mesh, id)
                    let mut ground_meshes: Vec<(usize, usize, MeshId)> = Vec::new();
                    if pl.terrain_mapping_mat.is_some() {
                        let mut parts: Vec<(usize, usize)> =
                            terrain_slots.iter().map(|t| (t.0, t.1)).collect();
                        parts.dedup();
                        for (level, mi) in parts {
                            let slots: Vec<usize> = terrain_slots
                                .iter()
                                .filter(|t| (t.0, t.1) == (level, mi))
                                .map(|t| t.2)
                                .collect();
                            let src = if level == 0 {
                                ot.meshes.get(mi).map(|m| &m.0)
                            } else {
                                ot.lower_lods
                                    .get(level - 1)
                                    .and_then(|l| l.1.get(mi))
                                    .map(|m| &m.0)
                            };
                            let Some(src) = src else { continue };
                            let ground = terrain_ground(src, &slots, pos, xf, p.origin);
                            if ground.is_empty() {
                                continue;
                            }
                            let rest_id = if let Some(&(_, id)) = gpu.types[&tkey]
                                .terrain_rest
                                .iter()
                                .find(|r| r.0 == (level, mi))
                            {
                                id
                            } else {
                                let id = gpu.add_mesh(renderer, scene, &terrain_rest(src, &slots));
                                scene.meshes[id].source = Some(ot.sco.path.display().to_string());
                                if let Some(t) = gpu.types.get_mut(&tkey) {
                                    t.terrain_rest.push(((level, mi), id));
                                }
                                id
                            };
                            let slot = if level == 0 {
                                mesh_list.get_mut(mi)
                            } else {
                                type_lods.get_mut(level - 1).and_then(|l| l.2.get_mut(mi))
                            };
                            if let Some(slot) = slot {
                                slot.0 = rest_id;
                            }
                            let ground_id = gpu.add_mesh(renderer, scene, &ground);
                            scene.meshes[ground_id].source =
                                Some(ot.sco.path.display().to_string());
                            tg.meshes.push(ground_id);
                            ground_meshes.push((level, mi, ground_id));
                        }
                    }
                    for (mi, (mesh_id, mats)) in mesh_list.iter().enumerate() {
                        let inst = if surface || ot.mesh_shadow.get(mi).copied().unwrap_or(false) {
                            let i = instance!(renderer.add_surface_instance(
                                scene,
                                *mesh_id,
                                draw_pos,
                                xf,
                                mats.clone()
                            ));
                            i
                        } else {
                            let i = instance!(renderer.add_instance(
                                scene,
                                *mesh_id,
                                draw_pos,
                                xf,
                                mats.clone()
                            ));
                            renderer.set_omsi_caster(
                                scene,
                                i,
                                ot.mesh_casts.get(mi).copied().unwrap_or(false),
                            );
                            i
                        };
                        if let Some(inst) = scene.instances.get_mut(inst) {
                            inst.render_phase = render_phase;
                            if surface {
                                // The authored OMSI phase and fixed `[surface]` lift replace
                                // camera-angle-sensitive bias for these road-layer meshes.
                                inst.decal = true;
                                inst.surface_bias = false;
                            }
                        }
                        // Scenery signs use [matl_freetex] with a string from the map
                        // object's [object] / [splineAttachement] record. The type's
                        // material is shared, so make a material for this placement only.
                        // (the map's strings are the object's string variables, and its
                        // {init} may make the file name of them: read after it has run)
                        if let Some((_, o3d_mats, overrides)) = ot.meshes.get(mi) {
                            for override_ in
                                overrides.iter().filter(|o| !o.item && o.freetex.is_some())
                            {
                                let Some(slot) =
                                    ::simulation::vehicle::override_slot(o3d_mats, override_)
                                else {
                                    continue;
                                };
                                let Some((_, var)) = &override_.freetex else {
                                    continue;
                                };
                                let Some(name) = resolve_scenery_freetex_name(
                                    var,
                                    override_,
                                    overrides,
                                    object_script.as_ref(),
                                    freetex_probe.as_ref(),
                                    &strings,
                                ) else {
                                    continue;
                                };
                                let dirs = texture_dirs(&self.root, &ot.model_dir);
                                let Some((tex, path)) =
                                    gpu.texture(renderer, scene, name, &dirs, images)
                                else {
                                    continue;
                                };
                                let Some(base) =
                                    mats.get(slot).and_then(|id| scene.materials.get(*id))
                                else {
                                    gpu.release_texture(renderer, scene, &path);
                                    continue;
                                };
                                let (alpha, color, unlit, transmap, night, light, env, emissive) = (
                                    base.alpha,
                                    base.color,
                                    base.unlit,
                                    base.transmap,
                                    base.nightmap,
                                    base.lightmap,
                                    base.envmap,
                                    base.emissive,
                                );
                                let slot_ov: Vec<&MaterialDef> = overrides
                                    .iter()
                                    .filter(|o| {
                                        !o.item
                                            && ::simulation::vehicle::override_slot(o3d_mats, o)
                                            == Some(slot)
                                    })
                                    .collect();
                                let mut extra =
                                    material_extra(&slot_ov, base.env_mask, base.bump, [0.0; 4]);
                                extra.ambient = o3d_mats.get(slot).map(|m| {
                                    d3d_material(m, slot_ov.iter().find_map(|o| o.allcolor), true).3
                                });
                                renderer
                                    .address_next
                                    .set(tex_addressing(slot_ov.iter().copied()));
                                let mat = renderer.add_material_extra(
                                    scene,
                                    Some(tex),
                                    alpha,
                                    color,
                                    unlit,
                                    transmap,
                                    night,
                                    light,
                                    env,
                                    emissive,
                                    extra,
                                );
                                let mat = gpu.material(renderer, scene, mat);
                                tg.materials.push(mat);
                                tg.shared_textures.push(path);
                                renderer.set_material(scene, inst, slot, mat);
                            }
                        }
                        // (only where the lower levels are drawn instead: a scripted object
                        // or a lamp keeps its first level, which alone the script poses -
                        // limited as well, it vanished when small, with nothing in its place)
                        if has_lower && !surface && lamp.is_none() && ot.program.is_none() {
                            renderer.set_lod_range(scene, inst, lod0_lo, lod0_max);
                        }
                        // [matl_change] variants of this mesh
                        for (_, slot, base, item, var, more) in
                            type_variants.iter().filter(|v| v.0 == mi)
                        {
                            // Traffic lamps are updated by Traffic::sync; keep their
                            // switches even when no custom script was loaded.
                            if lamp.is_some() || ot.program.is_some() {
                                object_variants.push((inst, *slot, *base, *item, var.clone(), more.clone()));
                            } else if var.trim().eq_ignore_ascii_case("NightlightA") {
                                pl.night_slots.push((inst, *slot, *item, *base));
                            } else if let Some(x) =
                                var.trim().parse::<f32>().ok().filter(|x| *x > 0.5)
                            {
                                renderer.set_material(
                                    scene,
                                    inst,
                                    *slot,
                                    pick_variant(x, *base, *item, more),
                                );
                            }
                        }
                        if lamp.is_some() {
                            lamp_instances.push((inst, ot.mesh_visible.get(mi).cloned().flatten()));
                            lamp_slots.push(
                                ot.meshes
                                    .get(mi)
                                    .map(|(_, o3d_mats, overrides)| {
                                        LampSlots::of_mesh(o3d_mats, overrides, mats.len())
                                    })
                                    .unwrap_or_default(),
                            );
                        }
                        if type_auto_night && (2..=4).contains(&ot.sco.night_map_mode) {
                            // each house its own hours (OMSI draws them once per object)
                            pl.night_modes.push(NightMode {
                                inst,
                                use_: InUse::new(ot.sco.night_map_mode, map_id as u64),
                                slots: mats.len().max(1),
                            });
                        }
                        // [texttexture] + [useTextTexture]: street names etc. from the map strings
                        if !ot.model.text_textures.is_empty() {
                            if let Some((_, o3d_mats, overrides)) = ot.meshes.get(mi) {
                                for o in overrides.iter().filter(|o| o.use_text_texture.is_some()) {
                                    let (Some(slot), Some(tt)) = (
                                        ::simulation::vehicle::override_slot(o3d_mats, o),
                                        ot.model
                                            .text_textures
                                            .get(o.use_text_texture.unwrap().max(0) as usize),
                                    ) else {
                                        continue;
                                    };
                                    // a script's string variable (the stock bus stop display's
                                    // departures): a texture of the object's own, drawn by
                                    // `update_scripted` whenever the script refreshes it
                                    let scripted_text = lamp.is_none()
                                        && tt.variable.trim().parse::<usize>().is_err()
                                        && ot
                                        .program
                                        .as_ref()
                                        .map(|p| p.str_var(tt.variable.trim()).is_some())
                                        .unwrap_or(false);
                                    if scripted_text {
                                        let atlas = self.fonts.lock().get(&tt.font, &|p| {
                                            ::texture::decode_file(p)
                                                .ok()
                                                .map(|i| (i.width, i.height, i.rgba))
                                        });
                                        let state = ::simulation::texttex::TextTextureState::new(
                                            tt.clone(),
                                            atlas,
                                        );
                                        let (w, h) =
                                            (tt.width.max(1) as u32, tt.height.max(1) as u32);
                                        let tex = gpu.add_blank_mips(renderer, scene, w, h);
                                        let mat = renderer.add_material(
                                            scene,
                                            Some(tex),
                                            AlphaMode::Blend,
                                            [1.0; 4],
                                            true,
                                        );
                                        let mat = gpu.material(renderer, scene, mat);
                                        tg.textures.push(tex);
                                        tg.materials.push(mat);
                                        renderer.set_material(scene, inst, slot, mat);
                                        script_texts.push((tex, state));
                                        continue;
                                    }
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
                                    // drawn as they are: the street name signs that seemed to want
                                    // their text turned by 180° were `.x` meshes whose frames were
                                    // read transposed (upside down), the stop name plates are not
                                    let image = scenery_text_image(tt, atlas, &text);
                                    let tex = gpu.add_image(renderer, scene, &image, true);
                                    // (lit like the rest of the object: Omsi.exe only swaps
                                    // the slot's texture, a sign does not shine at night)
                                    let mat = renderer.add_material(
                                        scene,
                                        Some(tex),
                                        alpha,
                                        [1.0; 4],
                                        false,
                                    );
                                    let mat = gpu.material(renderer, scene, mat);
                                    gpu.text_textures.insert(key.clone(), (tex, mat, 1));
                                    tg.texts.push(key);
                                    renderer.set_material(scene, inst, slot, mat);
                                }
                            }
                        }
                        // [htmltexture] + [useHtmlTexture]: a page drawn onto the slot; the
                        // pictures come from `update_scripted`
                        if has_pages && object_script.is_some() {
                            if let Some((_, o3d_mats, overrides)) = ot.meshes.get(mi) {
                                for o in overrides.iter().filter(|o| !o.item) {
                                    let Some(page) =
                                        o.use_script_texture.map(|n| n.max(0) as usize)
                                    else {
                                        continue;
                                    };
                                    if !ot
                                        .model
                                        .html_textures
                                        .iter()
                                        .any(|d| d.script_index == page)
                                    {
                                        continue;
                                    }
                                    let Some(slot) = ::simulation::vehicle::override_slot(o3d_mats, o)
                                    else {
                                        continue;
                                    };
                                    let mat = match html_mats.get(&page) {
                                        Some(m) => *m,
                                        None => {
                                            let tex = gpu.add_image(
                                                renderer,
                                                scene,
                                                &Image {
                                                    width: 4,
                                                    height: 4,
                                                    rgba: [0, 0, 0, 255].repeat(16),
                                                    has_alpha: true,
                                                },
                                                false,
                                            );
                                            let mat = renderer.add_material_extra(
                                                scene,
                                                Some(tex),
                                                text_alpha(o3d_mats, slot, overrides),
                                                [1.0; 4],
                                                true,
                                                None,
                                                None,
                                                None,
                                                None,
                                                [0.0; 3],
                                                MaterialExtra {
                                                    screen: true,
                                                    html: true,
                                                    ..Default::default()
                                                },
                                            );
                                            let mat = gpu.material(renderer, scene, mat);
                                            tg.textures.push(tex);
                                            tg.materials.push(mat);
                                            html_pages.push((page, tex));
                                            html_mats.insert(page, mat);
                                            mat
                                        }
                                    };
                                    renderer.set_material(scene, inst, slot, mat);
                                }
                            }
                        }
                        all_instances.push(inst);
                    }
                    let mut lod_instances = Vec::new();
                    // (the meshes' own instances: a script poses them one by one, the ground
                    // drawn in the [terrainmapping] slots after them keeps the object's place)
                    let mesh_instances = all_instances.len();
                    let lod_drawn = has_lower && !surface && lamp.is_none() && ot.program.is_none();
                    for &(level, _, ground_id) in &ground_meshes {
                        // the first level without lower ones is drawn at any size
                        let range = if level == 0 {
                            lod_drawn.then_some((lod0_lo, lod0_max))
                        } else if lod_drawn {
                            type_lods.get(level - 1).map(|l| (l.0, l.1))
                        } else {
                            continue;
                        };
                        // Keep the first ground texture on the object even where the map
                        // author painted asphalt or another layer on the terrain below it.
                        if let Some(mat) = pl.terrain_mapping_mat {
                            let inst = if surface {
                                instance!(renderer.add_surface_instance(
                                    scene,
                                    ground_id,
                                    draw_pos,
                                    xf,
                                    vec![mat]
                                ))
                            } else {
                                instance!(renderer.add_instance(
                                    scene,
                                    ground_id,
                                    draw_pos,
                                    xf,
                                    vec![mat]
                                ))
                            };
                            if let Some(x) = scene.instances.get_mut(inst) {
                                x.decal = surface;
                                x.render_phase = render_phase;
                                if surface {
                                    x.surface_bias = false;
                                }
                            }
                            if let Some((lo, hi)) = range {
                                renderer.set_lod_range(scene, inst, lo, hi);
                            }
                            if level == 0 {
                                all_instances.push(inst);
                            } else {
                                lod_instances.push(inst);
                            }
                        }
                    }
                    if lod_drawn {
                        for (min_size, max_size, meshes) in &type_lods {
                            for (mesh_id, mats) in meshes {
                                let inst = instance!(renderer.add_instance(
                                    scene,
                                    *mesh_id,
                                    pos,
                                    xf,
                                    mats.clone()
                                ));
                                renderer.set_lod_range(scene, inst, *min_size, *max_size);
                                lod_instances.push(inst);
                            }
                        }
                    }
                    // the object is drawn, left out and switched to another LOD as one
                    // (performance_minObjSize, performance_maxObjDist, [detail_factor],
                    // [noDistanceCheck]); its sphere about its origin holds every level
                    {
                        let radius = mesh_list
                            .iter()
                            .map(|m| m.0)
                            .chain(type_lods.iter().flat_map(|l| l.2.iter().map(|m| m.0)))
                            .filter_map(|id| scene.meshes.get(id))
                            .filter(|m| m.bounds_radius > 0.0)
                            .map(|m| m.bounds_center.length() + m.bounds_radius)
                            .fold(0.0f32, f32::max);
                        let detail = if ot.sco.model.detail_factor != 1.0 {
                            ot.sco.model.detail_factor
                        } else {
                            ot.model.detail_factor
                        };
                        let any_distance = ot.sco.model.no_distance_check
                            || ot.model.no_distance_check
                            || ot.model.meshes.iter().any(|m| m.no_distance_check);
                        let near_only = stand_in_area(&ot, &xf, pos, (p.tx, p.ty));
                        for inst in all_instances.iter().chain(&lod_instances) {
                            scene.instances[*inst].presurface =
                                ot.sco.render_type == ::scenery::sco::RenderType::PreSurface;
                            renderer.set_object_culling(scene, *inst, radius, detail, any_distance);
                            renderer.set_near_only(scene, *inst, near_only);
                        }
                    }
                    if ot.sco.crash_mode_pole.is_some() && !ot.sco.no_collision {
                        let instances: Vec<usize> = all_instances
                            .iter()
                            .chain(&lod_instances)
                            .copied()
                            .collect();
                        // knocked over before the tile went away: it lies where it fell
                        if let Some(push) = self.fallen_poles.lock().get(&collision_key) {
                            let fallen = fallen_pole(xf, *push);
                            for inst in &instances {
                                renderer.set_transform(scene, *inst, pos, fallen);
                            }
                        }
                        self.poles
                            .lock()
                            .insert(collision_key, (pos, xf, instances));
                        pl.poles.push(collision_key);
                    }
                    if parked {
                        let instances: Vec<usize> = all_instances
                            .iter()
                            .chain(&lod_instances)
                            .copied()
                            .collect();
                        if self.departed.lock().contains(&collision_key) {
                            for inst in &instances {
                                hide_instance(renderer, scene, *inst);
                            }
                        } else {
                            self.parked_objects.lock().insert(
                                collision_key,
                                ParkedObject {
                                    tile: key,
                                    pos,
                                    heading: Pose { pos, rot: xf }.heading(),
                                    sco: ot.sco.path.clone(),
                                    instances,
                                },
                            );
                        }
                    }
                    if editable {
                        let instances: Vec<usize> = all_instances
                            .iter()
                            .chain(&lod_instances)
                            .copied()
                            .collect();
                        let eo = EditObject {
                            tile: key,
                            pos,
                            xf,
                            key: collision_key,
                            instances,
                            sco: ot.sco.path.clone(),
                        };
                        // an object edited before its tile went shows the edit again
                        if let Some(e) = self.object_edits.lock().get(&map_id).copied() {
                            show_edit(renderer, scene, &eo, e);
                        }
                        self.edit_objects.lock().insert(map_id, eo);
                    }
                    if let Some((parent, index, any_light)) = lamp {
                        let names: Mutex<Vec<LightSwitch>> = Mutex::new(Vec::new());
                        let lights = model_lights_owned(
                            &ot.model,
                            &|_| xf,
                            pos,
                            &|var| {
                                names.lock().push(LightSwitch::parse(var));
                                1.0
                            },
                            &[],
                        );
                        let names = names.into_inner();
                        let sources = model_light_sources(&ot.model);
                        let (coronas, corona_mesh): (
                            Vec<(::render::Corona, String)>,
                            Vec<(usize, glam::Vec3, glam::Vec3)>,
                        ) = lights
                            .into_iter()
                            .filter_map(|(c, k)| match names.get(k) {
                                Some(LightSwitch::Variable(v)) => Some((
                                    (c, v.clone()),
                                    sources.get(k).copied().unwrap_or((
                                        0,
                                        glam::Vec3::ZERO,
                                        glam::Vec3::ZERO,
                                    )),
                                )),
                                _ => None,
                            })
                            .unzip();
                        let script = ot.program.as_ref().map(|p| {
                            Arc::new(Mutex::new(::simulation::scenery::SceneryInstance::new(
                                p.clone(),
                                &ot.mesh_defs(),
                                self.script_clock(),
                                &strings,
                            )))
                        });
                        let lit = vec![0.0; coronas.len()];
                        let animated = script
                            .as_ref()
                            .map(|s| s.lock().animated())
                            .unwrap_or(false);
                        let sound = ot.sco.sound.as_ref().map(|rel| {
                            let dir = ot
                                .sco
                                .path
                                .parent()
                                .map(|p| p.to_path_buf())
                                .unwrap_or_default();
                            ::legacy_config::resolve_path(&dir, rel)
                        });
                        pl.light_objects.push(LightObject {
                            parent,
                            index,
                            any_light,
                            instances: lamp_instances,
                            slots: lamp_slots,
                            variants: object_variants,
                            pos,
                            script,
                            coronas,
                            corona_mesh,
                            lit,
                            xf,
                            animated,
                            sound,
                            sounds: Default::default(),
                        });
                    } else if let Some(inst) = object_script.take() {
                        let texture_selection = scenery_texture_selection(&ot, &inst);
                        if !ot.dynamic_textures.is_empty() {
                            if let Some(rows) = gpu.dynamic_texture_variant(
                                renderer,
                                scene,
                                tkey,
                                &texture_selection,
                                &self.root,
                                images,
                            ) {
                                for (mi, row) in rows.iter().enumerate() {
                                    let Some(&mesh_inst) = all_instances.get(mi) else {
                                        continue;
                                    };
                                    for (slot, pair) in row.iter().enumerate() {
                                        let Some(looks) = pair else { continue };
                                        let picked = object_variants
                                            .iter()
                                            .find(|v| v.0 == mesh_inst && v.1 == slot)
                                            .map(|v| {
                                                v.4.trim()
                                                    .parse::<f32>()
                                                    .ok()
                                                    .or_else(|| inst.var(&v.4))
                                                    .map(variant_number)
                                                    .unwrap_or(0)
                                            })
                                            .unwrap_or(0);
                                        renderer.set_material(
                                            scene,
                                            mesh_inst,
                                            slot,
                                            look_of(looks, picked),
                                        );
                                    }
                                }
                            }
                        }
                        if inst.is_dynamic()
                            || !object_variants.is_empty()
                            || ot.sco.sound.is_some()
                            || !script_texts.is_empty()
                            || !html_pages.is_empty()
                            || !ot.dynamic_textures.is_empty()
                        {
                            let arrivals = inst.wants_arrivals();
                            // (a scripted object with [terrainmapping] slots had more instances
                            // than its script has meshes: "index out of bounds", #111)
                            all_instances.truncate(mesh_instances);
                            self.push_scripted(ScriptedObject {
                                ty: ot.clone(),
                                pos,
                                xf,
                                instances: all_instances,
                                inst,
                                controller,
                                light_index: 0,
                                map_id,
                                variants: object_variants,
                                sounds: None,
                                tile: key,
                                var_parent,
                                texts: script_texts,
                                arrivals,
                                htmls: html_pages,
                                last_tex_selection: None,
                                dynamic_materials: HashMap::new(),
                            });
                        }
                    }
                    pl.objects += 1;
                    done_some = true;
                }
            }
            pl.secs[phase.min(3) as usize] += t_phase.elapsed().as_secs_f64();
        }
        let done = pl.phase >= 4;
        if ::legacy_config::env::var_os("OMSI_PROFILE").is_some() {
            let took = t_start.elapsed().as_secs_f64();
            let decodes = (
                gpu.sync_decodes - decodes_before.0,
                gpu.sync_decode_secs - decodes_before.1,
            );
            if took + lock_wait > 0.02 || decodes.0 > 0 {
                log::info!(
                    "place tile ({}, {}): {:.1} ms this step (+{:.1} ms waiting for the GPU cache), {} textures decoded here in {:.1} ms; so far ground {:.1} ms, {} splines {:.1} ms, {} trees {:.1} ms, {} objects {:.1} ms{}",
                    key.0,
                    key.1,
                    took * 1000.0,
                    lock_wait * 1000.0,
                    decodes.0,
                    decodes.1 * 1000.0,
                    pl.secs[0] * 1000.0,
                    pl.splines,
                    pl.secs[1] * 1000.0,
                    pl.trees,
                    pl.secs[2] * 1000.0,
                    pl.objects,
                    pl.secs[3] * 1000.0,
                    if done { ", done" } else { "" }
                );
            }
        }
        done
    }
}
