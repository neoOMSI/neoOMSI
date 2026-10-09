use super::*;

/// A street name sign object (the stock Verkehrszeichen_MC `StreetSign_*`, and the
/// German `Strassenschild`/`StrSchild` names add-on maps use).
pub(super) fn is_street_sign(file: &str) -> bool {
    let f = file.to_ascii_lowercase().replace(['\\', '_', ' ', '-'], "");
    let name = f.rsplit('/').next().unwrap_or(&f);
    [
        "streetsign",
        "streetname",
        "strschild",
        "strassenschild",
        "straßenschild",
        "strassenname",
        "roadsignname",
        "roadname",
    ]
        .iter()
        .any(|k| name.contains(k))
}

impl World {
    pub fn object_type(&self, rel: &str) -> Option<Arc<ObjectType>> {
        self.object_type_scheme(rel, None)
    }

    /// An object type with one of its `[CTC]` paint schemes applied (parked cars).
    pub fn object_type_scheme(&self, rel: &str, scheme: Option<usize>) -> Option<Arc<ObjectType>> {
        let mut key = rel.to_ascii_lowercase().replace('\\', "/");
        if let Some(i) = scheme {
            key = format!("{key}#{i}");
        }
        if let Some(t) = self.object_types.lock().get(&key) {
            return t.clone();
        }
        let path = ::legacy_config::resolve_path(&self.root, rel);
        let loaded = (|| -> Option<Arc<ObjectType>> {
            let mut sco = SceneryObject::load(&path)
                .map_err(|e| log::warn!("{e}"))
                .ok()?;
            let sco_dir = path.parent()?.to_path_buf();
            let (model, model_dir) = match &sco.model_file {
                Some(m) => {
                    let mp = ::legacy_config::resolve_path(&sco_dir, m);
                    let model = Model::load(&mp).map_err(|e| log::warn!("{e}")).ok()?;
                    (model, mp.parent()?.to_path_buf())
                }
                None => (sco.model.clone(), sco_dir.clone()),
            };
            // OMSI reads these world-pass tags from a referenced model.cfg as well as from
            // the .sco wrapper. Preserve explicit wrapper values, including an explicit
            // Normal/false override; otherwise inherit the model definition as the C++ path
            // does. Missing render phases leave junction geometry in Normal, after splines.
            sco.inherit_model_tags(&model);
            let mut meshes = Vec::new();
            let mut mesh_visible = Vec::new();
            let mut mesh_def_index = Vec::new();
            let mut mesh_pivots = Vec::new();
            if !model.lods.is_empty() {
                let start = model.lods[0].first_mesh;
                for (i, md) in model.lod_meshes(0).iter().enumerate() {
                    let mesh_path = ::legacy_config::resolve_path(
                        &::legacy_config::resolve_path(&model_dir, "model"),
                        &md.file,
                    );
                    let mesh_path = if ::legacy_config::vfs::is_file(&mesh_path) {
                        mesh_path
                    } else {
                        ::legacy_config::resolve_path(&model_dir, &md.file)
                    };
                    match ::legacy_o3d::load_mesh(&mesh_path) {
                        Ok(m) => {
                            meshes.push((
                                mesh_from_o3d(&m),
                                m.materials.clone(),
                                md.materials.clone(),
                            ));
                            mesh_visible.push(md.visible.clone());
                            mesh_def_index.push(start + i);
                            mesh_pivots.push(::simulation::anim::pivot_from_mesh(&m));
                        }
                        Err(e) => log::debug!("{}: {e}", mesh_path.display()),
                    }
                }
            }
            // lower detail levels
            let mut lower_lods = Vec::new();
            for l in 1..model.lods.len() {
                let mut list = Vec::new();
                for md in model.lod_meshes(l) {
                    let mesh_path = ::legacy_config::resolve_path(
                        &::legacy_config::resolve_path(&model_dir, "model"),
                        &md.file,
                    );
                    let mesh_path = if ::legacy_config::vfs::is_file(&mesh_path) {
                        mesh_path
                    } else {
                        ::legacy_config::resolve_path(&model_dir, &md.file)
                    };
                    if let Ok(m) = ::legacy_o3d::load_mesh(&mesh_path) {
                        list.push((mesh_from_o3d(&m), m.materials.clone(), md.materials.clone()));
                    }
                }
                lower_lods.push((model.lods[l].min_size, list));
            }
            let lod0_min = model.lods.first().map(|l| l.min_size).unwrap_or(0.0);
            // [CTC] paint schemes (.cti items): retain their texture keys and folders so
            // the selected advertisements can be resolved when a placement chooses them.
            let ctc_schemes: Vec<(String, Vec<::simulation::vehicle::PaintScheme>)> = model
                .ctc
                .iter()
                .map(|c| {
                    (
                        c.variable.clone(),
                        ::simulation::vehicle::load_paint_schemes(&::legacy_config::resolve_path(
                            &sco_dir, &c.path,
                        )),
                    )
                })
                .collect();
            let paint_schemes: Vec<::simulation::vehicle::PaintScheme> = ctc_schemes
                .iter()
                .flat_map(|(_, schemes)| schemes.iter().cloned())
                .collect();
            let mut dynamic_textures: Vec<DynamicTextureGroup> = ctc_schemes
                .iter()
                .map(|(variable, schemes)| DynamicTextureGroup {
                    variable: variable.clone(),
                    choices: schemes
                        .iter()
                        .map(|scheme| {
                            scheme
                                .textures
                                .iter()
                                .filter_map(|(name, file)| {
                                    model
                                        .ctc_textures
                                        .iter()
                                        .find(|(ctc_name, _)| ctc_name.eq_ignore_ascii_case(name))
                                        .map(|(_, default)| {
                                            (default.clone(), file.clone(), scheme.dir.clone())
                                        })
                                })
                                .collect()
                        })
                        .collect(),
                })
                .collect();
            // Scenery models can also use the same script-variable texture selectors as
            // vehicles. Each [newtexchangemaster] is an independent dynamic texture group.
            dynamic_textures.extend(
                ::model::load_texchanges(&model_dir, &model.texchanges)
                    .into_iter()
                    .map(|master| {
                        let ::model::TexChangeMaster {
                            texture,
                            variable,
                            entries,
                            dir,
                        } = master;
                        DynamicTextureGroup {
                            variable,
                            choices: entries
                                .into_iter()
                                .map(|file| vec![(texture.clone(), file, dir.clone())])
                                .collect(),
                        }
                    }),
            );
            if let Some(ps) = scheme.and_then(|i| paint_schemes.get(i)) {
                let mut map: HashMap<String, String> = HashMap::new();
                for (ctc_name, file) in &ps.textures {
                    // (the scheme's picture lies in the scheme's folder, as for the buses;
                    // taken as a bare name it was looked for among the model's textures, not
                    // found, and the parked car stood there white)
                    let in_scheme = ::legacy_config::resolve_path(&ps.dir, file);
                    let file = if ::legacy_config::vfs::is_file(&in_scheme) {
                        in_scheme.to_string_lossy().into_owned()
                    } else {
                        file.clone()
                    };
                    for (name, default) in &model.ctc_textures {
                        if name.eq_ignore_ascii_case(ctc_name) {
                            map.insert(default.to_ascii_lowercase(), file.clone());
                        }
                    }
                }
                let subst = |t: &mut String| {
                    if let Some(n) = map.get(&t.to_ascii_lowercase()) {
                        *t = n.clone();
                    }
                };
                for (_, mats, overrides) in meshes
                    .iter_mut()
                    .chain(lower_lods.iter_mut().flat_map(|l| l.1.iter_mut()))
                {
                    mats.iter_mut().for_each(|m| subst(&mut m.texture));
                    overrides.iter_mut().for_each(|o| subst(&mut o.texture));
                }
            }
            let surface_maps = if sco.surface {
                let dirs = texture_dirs(&self.root, &model_dir);
                mesh_def_index
                    .iter()
                    .position(|&d| d == 0)
                    .and_then(|k| meshes.get(k))
                    .and_then(|(_, materials, _)| {
                        surface_faces(materials.iter().map(|m| m.texture.as_str()), &dirs)
                    })
            } else {
                None
            };
            let paint_scheme_count = paint_schemes.len();
            // scripts (or an empty program) for objects that are scripted or animated
            let animated = mesh_def_index.iter().any(|d| {
                !model.meshes[*d].animations.is_empty() || model.meshes[*d].visible.is_some()
            });
            let has_freetex = mesh_def_index.iter().any(|d| {
                model.meshes[*d]
                    .materials
                    .iter()
                    .any(|o| !o.item && o.freetex.is_some())
            });
            let program = if !sco.scripts.scripts.is_empty()
                || !sco.scripts.stringvarlists.is_empty()
                || !sco.scripts.varlists.is_empty()
                || has_freetex
                || animated
                || sco.sound.is_some()
            {
                Some(Arc::new(::simulation::scenery::compile_scenery(
                    &self.root,
                    &sco.scripts,
                )))
            } else {
                None
            };
            let mesh_shadow = mesh_def_index
                .iter()
                .map(|d| model.meshes[*d].is_shadow)
                .collect();
            let mesh_casts = mesh_def_index
                .iter()
                .map(|d| model.meshes[*d].shadow)
                .collect();
            let deform = load_crossing_field(&sco, &model_dir);
            // Deformation belongs to the definition's local frame, before placement.
            // Share the result across instances, every visual LOD and the collision mesh.
            if let Some(field) = &deform {
                for (mesh, _, _) in meshes
                    .iter_mut()
                    .chain(lower_lods.iter_mut().flat_map(|l| l.1.iter_mut()))
                {
                    deform_mesh(mesh, field);
                }
            }
            // [terrainhole] <mesh>: the cutter that takes the ground away under a junction
            // or an underpass, so the carriageway is not buried under a mound of terrain
            let holes: Vec<MeshData> = sco
                .terrain_hole_sources(&model)
                .filter_map(|(hole_dir, f)| {
                    // the cutter sits next to the model, which is either the object's own
                    // folder or a `model` folder inside it
                    let mp = ::legacy_config::resolve_path(hole_dir, f);
                    let mp = if ::legacy_config::vfs::is_file(&mp) {
                        mp
                    } else {
                        ::legacy_config::resolve_path(&::legacy_config::resolve_path(hole_dir, "model"), f)
                    };
                    match ::legacy_o3d::load_mesh(&mp) {
                        Ok(m) => Some(mesh_from_o3d(&m)),
                        Err(e) => {
                            log::warn!("terrain hole {}: {e}", mp.display());
                            None
                        }
                    }
                })
                .collect();
            let mut collision = sco
                .collision_mesh
                .as_ref()
                .filter(|_| !sco.no_collision)
                .and_then(|f| {
                    let mp =
                        ::legacy_config::resolve_path(&::legacy_config::resolve_path(&model_dir, "model"), f);
                    let mp = if ::legacy_config::vfs::is_file(&mp) {
                        mp
                    } else {
                        ::legacy_config::resolve_path(&model_dir, f)
                    };
                    let mp = if ::legacy_config::vfs::is_file(&mp) {
                        mp
                    } else {
                        ::legacy_config::resolve_path(&sco_dir, f)
                    };
                    ::legacy_o3d::load_mesh(&mp)
                        .map(|m| mesh_from_o3d(&m))
                        .map_err(|e| log::debug!("collision mesh {}: {e}", mp.display()))
                        .ok()
                });
            if let (Some(mesh), Some(field)) = (&mut collision, &deform) {
                deform_mesh(mesh, field);
            }
            Some(Arc::new(ObjectType {
                sco,
                sound_path: Default::default(),
                model,
                model_dir,
                meshes,
                surface_maps,
                mesh_visible,
                mesh_def_index,
                mesh_pivots,
                mesh_shadow,
                mesh_casts,
                program,
                lower_lods,
                lod0_min,
                paint_scheme_count,
                dynamic_textures,
                holes,
                deform,
                collision,
                camera: Default::default(),
                collision_shape: Default::default(),
                light_shape: Default::default(),
            }))
        })();
        // two loaders may have read the same type at once: all of them get the first copy,
        // so that it is uploaded (and evicted) once
        self.object_types
            .lock()
            .entry(key)
            .or_insert(loaded)
            .clone()
    }

    pub fn spline_type(&self, rel: &str) -> Option<Arc<SplineType>> {
        let key = rel.to_ascii_lowercase().replace('\\', "/");
        if let Some(t) = self.spline_types.lock().get(&key) {
            return t.clone();
        }
        let path = ::legacy_config::resolve_path(&self.root, rel);
        let loaded = Spline::load(&path)
            .map_err(|e| log::warn!("{e}"))
            .ok()
            .map(|def| {
                ::geometry::register_half_cant_width(rel, &def);
                let dir = path.parent().map(|p| p.to_path_buf()).unwrap_or_default();
                let dirs = texture_dirs(&self.root, &dir);
                let surface_maps = surface_faces(
                    def.textures.iter().map(|texture| texture.file.as_str()),
                    &dirs,
                );
                Arc::new(SplineType {
                    dir,
                    def,
                    surface_maps,
                })
            });
        self.spline_types
            .lock()
            .entry(key)
            .or_insert(loaded)
            .clone()
    }

    /// Object types no loaded tile uses any more leave the type cache (with their meshes on
    /// the CPU side).
    pub fn trim_object_types(&self) {
        self.object_types
            .lock()
            .retain(|_, t| t.as_ref().map(|t| Arc::strong_count(t) > 1).unwrap_or(true));
    }
}
