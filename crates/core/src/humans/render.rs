use super::*;

const HUMAN_SHADOW_RANGE: f64 = 45.0;

fn contact_shadow(renderer: &Renderer, scene: &mut Scene) -> (MeshId, MaterialId) {
    const N: u32 = 64;
    let mut rgba = Vec::with_capacity((N * N * 4) as usize);
    let edge = (-4.5f32).exp();
    for y in 0..N {
        for x in 0..N {
            let u = (x as f32 + 0.5) / N as f32 * 2.0 - 1.0;
            let v = (y as f32 + 0.5) / N as f32 * 2.0 - 1.0;
            let r2 = u * u + v * v;
            let a = 0.6 * (((-4.5 * r2).exp() - edge) / (1.0 - edge)).max(0.0);
            rgba.extend_from_slice(&[0, 0, 0, (a * 255.0).round() as u8]);
        }
    }
    let tex = renderer.add_texture_data(
        scene,
        &::texture::gpu::TextureData::from_image(::texture::Image {
            width: N,
            height: N,
            rgba,
            has_alpha: true,
        }),
    );
    let corners = [(-0.5, -0.5), (0.5, -0.5), (0.5, 0.5), (-0.5, 0.5)];
    let mesh = ::geometry::MeshData {
        positions: corners.iter().map(|&(x, y)| Vec3::new(x, y, 0.0)).collect(),
        normals: vec![Vec3::Z; 4],
        uvs: corners
            .iter()
            .map(|&(x, y)| glam::Vec2::new(x + 0.5, 0.5 - y))
            .collect(),
        ranges: vec![(0, 6, 0)],
        indices: vec![0, 2, 1, 0, 3, 2],
        one_sided: false,
    };
    let mat = renderer.add_material(scene, Some(tex), AlphaMode::Blend, [1.0; 4], false);
    // drawn with the ground, before the bus: writing depth, it hid the bus floor under it
    renderer.set_no_z_write(scene, mat, true);
    (renderer.add_mesh(scene, &mesh), mat)
}

impl Humans {
    /// Someone has gone: hidden, and their meshes kept for the next person of the type.
    pub(super) fn retire(&mut self, p: &Person) {
        if let Some(blob) = p.render.blob {
            self.render.hidden.push(blob);
            self.render.spare_blobs.push(blob);
        }
        let tkey = Arc::as_ptr(&p.ty) as usize;
        for (mi, m) in p.render.meshes.iter().enumerate() {
            self.render.hidden.push(m.1);
            self.render
                .spare
                .entry((tkey, p.variant, mi))
                .or_default()
                .push(*m);
        }
    }

    /// Skin the people due for a new pose and push transforms to the renderer. Near people
    /// are posed every frame, far ones every few frames and people out of view rarely; the
    /// posing and skinning run in parallel.
    pub fn sync(&mut self, renderer: &Renderer, scene: &mut Scene, camera: DVec3) {
        for inst in self.render.hidden.drain(..) {
            renderer.set_params(scene, inst, &[], false, &[]);
        }
        let started = std::time::Instant::now();
        self.render.sync_frame = self.render.sync_frame.wrapping_add(1);
        let eye = self.eye;
        let from = eye.map(|e| e.pos).unwrap_or(camera);
        // synced only now and then (offscreen snapshots): everybody is posed afresh
        let all = self.time - self.last_sync > 0.12;
        let sdt = (self.time - self.last_sync).clamp(0.0, 0.5) as f32;
        self.last_sync = self.time;
        let mut due: Vec<bool> = Vec::with_capacity(self.people.len());
        for (k, p) in self.people.iter_mut().enumerate() {
            p.render.since_posed = p.render.since_posed.saturating_add(1);
            let distance = (p.position - from).length();
            let fov = eye.map_or(1.0, |e| e.fov_y);
            let size = if distance <= p.ty.radius() as f64 {
                f32::MAX
            } else {
                (2.0 * p.ty.radius() as f64 / (distance * fov)) as f32
            };
            let level = p.ty.level_at(size, Some(p.render.level));
            if level != p.render.level {
                p.render.level = level;
                p.render.skinned = false;
            }
            let d = p.position + DVec3::Z * 0.9 - from;
            let dist = d.length();
            let visible = match eye {
                Some(e) => dist < 4.0 || d.dot(e.fwd) / dist.max(1e-3) > e.cos_half - 0.15,
                None => true,
            };
            // everybody the eye can make out is posed every frame: a pose every other
            // frame at 12-30 m moved walkers in steps and made planted feet shiver
            // (within 30 m everybody, seen or not: the mirrors show the people behind the
            // bus, who were posed every twelfth frame and moved in jerks there)
            let every = if dist < 30.0 {
                1
            } else if !visible {
                12
            } else if dist < 45.0 {
                1
            } else if dist < 90.0 {
                2
            } else if dist < 160.0 {
                3
            } else {
                6
            };
            let every = if p.vel.length_squared() < 1e-4 && dist > 20.0 {
                every * 2
            } else {
                every
            };
            // spread the far ones over the frames
            let turn = (self.render.sync_frame + k as u32) % every == 0;
            due.push(
                !p.render.skinned
                    || all
                    || (p.render.since_posed >= every
                        && (turn || p.render.since_posed >= 2 * every)),
            );
        }
        let n_due = due.iter().filter(|d| **d).count();
        let ik = self.ik;
        let natural = self.natural;
        let pose_one = move |p: &mut Person| {
            let Person {
                anim,
                pose,
                ty,
                render:
                    PersonRender {
                        skins,
                        skin_bones,
                        pose_changed,
                        active_bones,
                        ..
                    },
                ..
            } = p;
            *pose_changed = false;
            let bones = if let Some(bones) = active_bones {
                *bones
            } else if ik {
                let posed = pose.bones(&ty.rig);
                if posed.ok {
                    posed.bones
                } else {
                    ::simulation::human::slots_from_omsi_grounded(
                        &anim.bones(&ty.omsi),
                        &ty.rig,
                        anim.angles[0].abs() < 45.0 && anim.angles[1].abs() < 45.0,
                    )
                }
            } else if natural {
                ::simulation::human::slots_from_omsi_grounded(
                    &anim.bones(&ty.omsi),
                    &ty.rig,
                    anim.angles[0].abs() < 45.0 && anim.angles[1].abs() < 45.0,
                )
            } else {
                ::simulation::human::slots_from_omsi(&anim.bones(&ty.omsi))
            };
            if bones.iter().any(|b| !b.is_finite()) && !skins.is_empty() {
                // keep the last good mesh (the rest pose would be the file's T-pose)
                return;
            }
            // (the same bones as the mesh was made with: nothing to skin or upload)
            if skins.len() == ty.mesh_count()
                && skin_bones
                    .as_ref()
                    .is_some_and(|b| b.iter().zip(&bones).all(|(a, c)| a.abs_diff_eq(*c, 1e-6)))
            {
                return;
            }
            skins.resize_with(ty.mesh_count(), Default::default);
            for k in 0..ty.mesh_count() {
                let (_, m) = ty.mesh_at(k);
                let (pos, nrm) = &mut skins[k];
                skin(m, &bones, pos, nrm);
            }
            *skin_bones = Some(bones);
            *pose_changed = true;
        };
        // a handful is quicker on this thread than handed to the pool
        if n_due >= 8 {
            self.people
                .par_iter_mut()
                .zip(due.par_iter())
                .with_min_len(2)
                .filter(|(_, go)| **go)
                .for_each(|(p, _)| pose_one(p));
        } else {
            self.people
                .iter_mut()
                .zip(&due)
                .filter(|(_, go)| **go)
                .for_each(|(p, _)| pose_one(p));
        }
        let upload = std::time::Instant::now();
        for (p, &go) in self.people.iter_mut().zip(&due) {
            if go {
                if p.render.pose_changed || !p.render.skinned {
                    for (k, (id, _)) in p.render.meshes.iter().enumerate() {
                        if let Some((pos, nrm)) = p.render.skins.get(k) {
                            renderer.update_mesh(scene, *id, pos, nrm, &p.ty.mesh_at(k).1.data.uvs);
                        }
                    }
                }
                p.render.skinned = true;
                p.render.since_posed = 0;
                p.render.posed_at = (p.position, p.heading);
            }
            // riders go with their bus; on the ground a mesh not posed this frame goes on
            // with the body too (left where it was posed, a far walker moved in jerks -
            // its feet slide a few centimetres instead, which nobody sees at that distance)
            let (at, heading) = match (p.puppet, p.place) {
                (_, Place::Ground) if go => p.render.posed_at,
                _ => (p.position, p.heading),
            };
            // (riders with the tilt of their floor)
            let tilt = if matches!(p.place, Place::Bus(..)) {
                p.tilt
            } else {
                Mat4::IDENTITY
            };
            let xf = tilt * Mat4::from_rotation_z((-heading).to_radians() as f32);
            let lit_to = if matches!(p.place, Place::Bus(..)) {
                p.interior
            } else {
                0.0
            };
            p.render.lit += (lit_to - p.render.lit) * (sdt / 0.4).min(1.0);
            let casts = (p.position - from).length() < HUMAN_SHADOW_RANGE;
            for (_, inst) in &p.render.meshes {
                renderer.set_transform(scene, *inst, at, xf);
                renderer.set_interior(scene, *inst, p.render.lit * 0.5);
                renderer.set_cabin(scene, *inst, matches!(p.place, Place::Bus(..)));
                renderer.set_casts_shadow(scene, *inst, casts);
            }
            if self.avatars.avatar_hidden.contains_key(&p.id)
                && ::legacy_config::env::var_os("OMSI_DEBUG_FOOT").is_some()
                && self.render.sync_frame % 30 == 0
            {
                log::info!(
                    "avatar drawn at ({:.2}, {:.2}, {:.2}) heading {:.0} place {:?} go {}",
                    at.x,
                    at.y,
                    at.z,
                    heading,
                    matches!(p.place, Place::Ground),
                    go
                );
            }
            let hidden = self
                .avatars
                .avatar_hidden
                .get(&p.id)
                .copied()
                .unwrap_or(false);
            let seated = if self.ik {
                p.pose.sit_amount() > 0.1
            } else {
                p.anim.angles[0].abs() >= 45.0 || p.anim.angles[1].abs() >= 45.0
            };
            let show_blob = !hidden && !seated && (at - from).length() < 90.0;
            if show_blob && p.render.blob.is_none() {
                let instance = match self.render.spare_blobs.pop() {
                    Some(instance) => {
                        self.render.hidden.retain(|i| *i != instance);
                        instance
                    }
                    None => {
                        let (mesh, mat) = *self
                            .render
                            .blob
                            .get_or_insert_with(|| contact_shadow(renderer, scene));
                        renderer.add_shadow_blob_instance(
                            scene,
                            mesh,
                            at,
                            Mat4::IDENTITY,
                            vec![mat],
                        )
                    }
                };
                p.render.blob = Some(instance);
            }
            if let Some(blob) = p.render.blob {
                if show_blob != p.render.blob_shown {
                    renderer.set_params(scene, blob, &[], show_blob, &[]);
                    p.render.blob_shown = show_blob;
                }
                if show_blob {
                    let size = p.ty.def.height.clamp(0.9, 2.1) / 1.75 * 0.6;
                    let stretch = 1.0 + 0.3 * (p.vel.length() as f32 / 1.5).min(1.0);
                    let scale = Mat4::from_scale(Vec3::new(size, size * 1.15 * stretch, 1.0));
                    renderer.set_transform(scene, blob, at + DVec3::Z * 0.01, xf * scale);
                }
            }
            for (k, (_, inst)) in p.render.meshes.iter().enumerate() {
                renderer.set_params(
                    scene,
                    *inst,
                    &[],
                    !hidden && p.ty.mesh_at(k).0 == p.render.level,
                    &[],
                );
            }
            if let Some(t) = self.trace.as_mut() {
                // OMSI_TRACE_PAX: where the mesh is drawn and where its ankles are, per frame
                if (at - from).length() < 40.0 {
                    use std::io::Write;
                    let a = |k: usize| at + (xf.transform_vector3(p.render.ankles[k])).as_dvec3();
                    let (l, r) = (a(0), a(1));
                    let details = match &p.state {
                        State::Pax(x) => {
                            let local = x
                                .bus
                                .or(x.inside)
                                .and_then(|b| self.buses.last_buses.iter().find(|n| n.id == b))
                                .map(|n| n.to_local(at))
                                .map(|l| format!("{:.3},{:.3}", l.x, l.y))
                                .unwrap_or_else(|| ",".into());
                            format!(
                                "{},{},{},{},{},{},{},{}",
                                x.task.name(),
                                x.movement,
                                x.stop.map(|s| s.to_string()).unwrap_or_default(),
                                x.door.map(|s| s.to_string()).unwrap_or_default(),
                                x.pt.map(|s| s.to_string()).unwrap_or_default(),
                                x.pt_target.map(|s| s.to_string()).unwrap_or_default(),
                                p.why,
                                local
                            )
                        }
                        _ => format!(",,,,,,{},,", p.why),
                    };
                    let _ = writeln!(
                        t,
                        "{:.4},{},{},{},{},{:.4},{:.4},{:.4},{:.2},{:.4},{:.4},{:.4},{:.4},{:.4},{:.4},{:.3},{:.3},{}",
                        self.time,
                        p.id,
                        p.state.name(),
                        matches!(p.place, Place::Ground) as u8,
                        go as u8,
                        at.x,
                        at.y,
                        at.z,
                        heading,
                        l.x,
                        l.y,
                        l.z,
                        r.x,
                        r.y,
                        r.z,
                        p.vel.x,
                        p.vel.y,
                        details
                    );
                }
            }
        }
        self.pose_stats.0 += 1;
        self.pose_stats.1 += n_due;
        self.pose_stats.2 += started.elapsed().as_secs_f64() * 1000.0;
        self.pose_stats.3 += upload.elapsed().as_secs_f64() * 1000.0;
    }

    pub fn sync_money(&mut self, renderer: &Renderer, scene: &mut Scene, bus: &VehicleInstance) {
        if let Some(m) = self.money.as_mut() {
            m.sync(renderer, scene, bus);
        }
    }
}

pub(in crate::humans) struct RenderResources {
    pub(in crate::humans) blob: Option<(MeshId, MaterialId)>,
    pub(in crate::humans) spare_blobs: Vec<usize>,
    pub(in crate::humans) hidden: Vec<usize>,
    /// GPU side of the human types, shared by everyone of a type: textures by file and the
    /// materials of every (type, mesh) - each person used to upload its own copies - and
    /// the meshes and instances of the people who have gone, taken over by the next person
    /// of the same type (the skinned vertices are rewritten anyway). Without that every
    /// passenger who ever appeared kept a mesh, its textures and materials on the GPU.
    pub(in crate::humans) gpu_textures: HashMap<PathBuf, Option<::render::TextureId>>,
    /// Per (type, clothing variant, mesh): its materials, and the meshes and instances of
    /// people who have gone, kept for the next person dressed alike.
    pub(in crate::humans) gpu_materials: HashMap<(usize, usize, usize), Vec<MaterialId>>,
    pub(in crate::humans) spare: HashMap<(usize, usize, usize), Vec<(MeshId, usize)>>,
    pub(in crate::humans) sync_frame: u32,
}

/// GPU instances, skin caches and eased drawing state never own passenger motion.
pub(super) struct PersonRender {
    pub(super) level: usize,
    pub(super) blob: Option<usize>,
    pub(super) blob_shown: bool,
    /// Host-provided seat hint for a remote viewer; it does not reserve a local place.
    pub(super) mirror_seat: Option<usize>,
    pub(super) active_bones: Option<[glam::Affine3A; ::simulation::human::SLOTS]>,
    pub(super) meshes: Vec<(MeshId, usize)>,
    /// Skinned positions and normals, per mesh.
    pub(super) skins: Vec<(Vec<Vec3>, Vec<Vec3>)>,
    /// The bones the skins were made with, and whether this frame's pose changed them
    /// (somebody standing still keeps the mesh of the frame before: skinning and uploading
    /// thirty waiting people every frame took 2 ms of the frame at a bus station).
    pub(super) skin_bones: Option<[glam::Affine3A; ::simulation::human::SLOTS]>,
    pub(super) pose_changed: bool,
    /// The interior light as drawn: it follows `interior` over a moment (stepping through
    /// the door, people lit up and went dark again from one frame to the next).
    pub(super) lit: f32,
    /// Whether this person has ever been posed (an unposed model is the file's T-pose).
    pub(super) skinned: bool,
    /// Frames since the last pose and where the person stood then (the
    /// feet of a mesh posed a frame ago stay on the floor when it is drawn there).
    pub(super) since_posed: u32,
    pub(super) posed_at: (DVec3, f64),
    /// Ankles of the last pose (model frame), for `OMSI_TRACE_PAX`.
    pub(super) ankles: [Vec3; 2],
}

impl Humans {
    /// Coins the driver handed out (from the host's GiveChangeCoin list) onto the change point.
    pub fn give_change(
        &mut self,
        world: &World,
        renderer: &Renderer,
        scene: &mut Scene,
        coins: &[usize],
    ) {
        if coins.is_empty() {
            return;
        }
        let point = self.buses.player_cabin.as_ref().and_then(|c| {
            c.data
                .change_points
                .first()
                .or(c.data.money_points.first())
                .map(|p| (Vec3::from(p.pos), p.var, c.change_parent))
        });
        if let (Some(m), Some((pos, var, parent))) = (self.money.as_mut(), point) {
            m.place(world, renderer, scene, coins, pos, var, parent, true);
        }
    }
}
