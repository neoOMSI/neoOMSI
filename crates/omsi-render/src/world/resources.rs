use crate::*;

pub(crate) struct Freed {
    pub(crate) vertex_buf: wgpu::Buffer,
    pub(crate) index_buf: wgpu::Buffer,
    pub(crate) bind_group: wgpu::BindGroup,
    pub(crate) buf: wgpu::Buffer,
}

impl Renderer {
    pub fn add_render_texture(&self, scene: &mut Scene, width: u32, height: u32) -> TextureId {
        let size = wgpu::Extent3d {
            width,
            height,
            depth_or_array_layers: 1,
        };
        let texture = self.device.create_texture(&wgpu::TextureDescriptor {
            label: Some("render target"),
            size,
            mip_level_count: 1,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format: self.format,
            usage: wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::TEXTURE_BINDING,
            view_formats: &[],
        });
        let view = texture.create_view(&Default::default());
        let bytes = texture_bytes(self.format, width, height, 1);
        scene.textures.push(GpuTexture {
            texture,
            view,
            size: (width, height),
            bytes,
            generation: next_gen(),
        });
        scene.textures.len() - 1
    }

    pub fn render_to_texture(
        &mut self,
        scene: &mut Scene,
        id: TextureId,
        camera: &Camera,
        lighting: &Lighting,
        aspect: f32,
    ) {
        let Some(t) = scene.textures.get(id) else {
            return;
        };
        let view = t.view.clone();
        let (w, h) = t.size;
        self.texture_aspect = Some(aspect);
        self.render_inner(
            scene,
            &view,
            w,
            h,
            camera,
            lighting,
            false,
            Some(id),
            None,
            false,
        );
        self.texture_aspect = None;
    }

    /// Refraction reads a half-resolution current scene before films are drawn, avoiding feedback.
    pub(crate) fn prepare_glass_behind(
        &mut self,
        scene: &mut Scene,
        width: u32,
        height: u32,
    ) {
        let Some(id) = scene.glass_slot else { return };
        let (width, height) = (width.div_ceil(2).max(1), height.div_ceil(2).max(1));
        let format = self.glass_picture_format;
        if self.glass_picture.as_ref().is_none_or(|v| {
            v.texture().width() != width
                || v.texture().height() != height
                || v.texture().format() != format
        }) {
            let texture = self.device.create_texture(&wgpu::TextureDescriptor {
                label: Some("current picture behind glass"),
                size: wgpu::Extent3d { width, height, depth_or_array_layers: 1 },
                mip_level_count: 1,
                sample_count: 1,
                dimension: wgpu::TextureDimension::D2,
                format,
                usage: wgpu::TextureUsages::RENDER_ATTACHMENT
                    | wgpu::TextureUsages::TEXTURE_BINDING,
                view_formats: &[],
            });
            self.glass_picture = Some(texture.create_view(&Default::default()));
        }
        let view = self.glass_picture.as_ref().unwrap();
        if scene.textures[id].view != *view {
            scene.textures[id] = GpuTexture::showing(
                view.texture().clone(), view.clone(), (width, height),
            );
            self.rebind_textures(scene, &[id]);
        }
    }

    pub fn set_sky_textures(&self, scene: &mut Scene, textures: [TextureId; 3]) {
        self.set_sky_textures_clouds(scene, textures, None)
    }

    pub fn set_sky_textures_clouds(
        &self,
        scene: &mut Scene,
        textures: [TextureId; 3],
        clouds: Option<TextureId>,
    ) {
        let views: Vec<&wgpu::TextureView> =
            textures.iter().map(|t| &scene.textures[*t].view).collect();
        let bg = self.device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("sky"),
            layout: &self.sky_layout,
            entries: &[
                wgpu::BindGroupEntry {
                    binding: 0,
                    resource: wgpu::BindingResource::TextureView(views[0]),
                },
                wgpu::BindGroupEntry {
                    binding: 1,
                    resource: wgpu::BindingResource::TextureView(views[1]),
                },
                wgpu::BindGroupEntry {
                    binding: 2,
                    resource: wgpu::BindingResource::TextureView(views[2]),
                },
                wgpu::BindGroupEntry {
                    binding: 3,
                    resource: wgpu::BindingResource::Sampler(&self.sky_sampler),
                },
                wgpu::BindGroupEntry {
                    binding: 4,
                    resource: wgpu::BindingResource::TextureView(match clouds {
                        Some(c) => &scene.textures[c].view,
                        None => &self.black_texture.view,
                    }),
                },
                wgpu::BindGroupEntry {
                    binding: 5,
                    resource: wgpu::BindingResource::Sampler(&self.sampler),
                },
                wgpu::BindGroupEntry {
                    binding: 6,
                    resource: wgpu::BindingResource::TextureView(&self.cloud_shape_view),
                },
                wgpu::BindGroupEntry {
                    binding: 7,
                    resource: wgpu::BindingResource::TextureView(&self.cloud_detail_view),
                },
                wgpu::BindGroupEntry {
                    binding: 8,
                    resource: wgpu::BindingResource::Sampler(&self.cloud_sampler),
                },
            ],
        });
        scene.sky_bind_group = Some(bg);
    }

    pub(crate) fn freed(&self, scene: &mut Scene) -> &Freed {
        if self.freed.get().is_none() {
            let address = self.address_next.replace(TexAddressing::Wrap);
            let plain = self.add_material(scene, None, AlphaMode::Opaque, [1.0; 4], false);
            self.address_next.set(address);
            let m = scene.materials.swap_remove(plain);
            let (bind_group, buf) = (m.bind_group, m.buf);
            let vertex_buf = self.device.create_buffer(&wgpu::BufferDescriptor {
                label: Some("freed mesh"),
                size: size_of::<Vertex>() as u64,
                usage: wgpu::BufferUsages::VERTEX | wgpu::BufferUsages::COPY_DST,
                mapped_at_creation: false,
            });
            let index_buf = self.device.create_buffer(&wgpu::BufferDescriptor {
                label: Some("freed mesh"),
                size: 4,
                usage: wgpu::BufferUsages::INDEX,
                mapped_at_creation: false,
            });
            let _ = self.freed.set(Freed {
                vertex_buf,
                index_buf,
                bind_group,
                buf,
            });
        }
        self.freed.get().unwrap()
    }

    pub fn free_mesh(&self, scene: &mut Scene, id: MeshId) {
        if id >= scene.meshes.len() {
            return;
        }
        let (vertex_buf, index_buf) = {
            let f = self.freed(scene);
            (f.vertex_buf.clone(), f.index_buf.clone())
        };
        let m = &mut scene.meshes[id];
        m.vertex_buf = vertex_buf;
        m.index_buf = index_buf;
        m.ranges.clear();
        m.bounds_center = Vec3::ZERO;
        m.bounds_radius = 0.0;
        Self::mesh_bounds_changed(scene, id);
    }

    pub fn free_texture(&self, scene: &mut Scene, id: TextureId) {
        scene.snow_textures.remove(&id);
        if let Some(m) = scene.pbr_maps.remove(&id) {
            for t in [m.normal, m.orm].into_iter().flatten() {
                self.free_texture(scene, t);
            }
        }
        if let Some(t) = scene.textures.get_mut(id) {
            *t = GpuTexture {
                texture: self.white_texture.texture.clone(),
                view: self.white_texture.view.clone(),
                size: (1, 1),
                bytes: 0,
                generation: next_gen(),
            };
        }
    }

    pub fn free_material(&self, scene: &mut Scene, id: MaterialId) {
        if id >= scene.materials.len() {
            return;
        }
        let (bind_group, buf) = {
            let f = self.freed(scene);
            (f.bind_group.clone(), f.buf.clone())
        };
        scene.materials[id] = Material {
            texture: None,
            alpha: AlphaMode::Opaque,
            color: [1.0; 4],
            unlit: false,
            no_z_write: false,
            no_z_check: false,
            z_bias: 0,
            nightmap: None,
            lightmap: None,
            envmap: None,
            env_mask: None,
            bump: None,
            emissive: [0.0; 3],
            transmap: None,
            address: TexAddressing::Wrap,
            uniform: <MaterialUniform as bytemuck::Zeroable>::zeroed(),
            buf,
            bind_group,
        };
    }

    pub fn remove_instance(&self, scene: &mut Scene, instance: usize) {
        if instance >= scene.instances.len() {
            return;
        }
        self.set_params(scene, instance, &[], false, &[]);
        scene.instances[instance].lod = (0.0, f32::MAX);
        scene.instances[instance].interior_lamps = 0;
    }

    pub fn truncate(
        &self,
        scene: &mut Scene,
        meshes: usize,
        textures: usize,
        materials: usize,
        instances: usize,
    ) {
        fn cut<T>(v: &mut Vec<T>, n: usize) {
            if n < v.len() {
                v.truncate(n);
                if v.capacity() > v.len() * 2 + 64 {
                    v.shrink_to_fit();
                }
            }
        }
        cut(&mut scene.meshes, meshes);
        cut(&mut scene.textures, textures);
        cut(&mut scene.materials, materials);
        let (nm, nt) = (scene.meshes.len(), scene.materials.len());
        for inst in scene.instances.iter_mut() {
            if inst.mesh >= nm {
                debug_assert!(!inst.visible, "a drawn instance lost its mesh");
                inst.mesh = 0;
                inst.visible = false;
            }
            for m in inst.materials.iter_mut() {
                if *m >= nt {
                    *m = 0;
                }
            }
        }
        if instances < scene.instances.len() {
            cut(&mut scene.instances, instances);
            scene.changed.retain(|i| *i < instances);
            scene.changed_mark.truncate(instances);
            scene.uploaded_instances = scene.uploaded_instances.min(instances);
            scene.dirty = true;
        }
    }

    pub fn recycle_mesh(&self, scene: &mut Scene, new: MeshId, into: MeshId) -> MeshId {
        if new + 1 != scene.meshes.len() || into >= new {
            return new;
        }
        let m = scene.meshes.pop().unwrap();
        scene.meshes[into] = m;
        Self::mesh_bounds_changed(scene, into);
        into
    }

    pub fn recycle_texture(&self, scene: &mut Scene, new: TextureId, into: TextureId) -> TextureId {
        if new + 1 != scene.textures.len() || into >= new {
            return new;
        }
        let t = scene.textures.pop().unwrap();
        scene.textures[into] = t;
        into
    }

    pub fn recycle_material(
        &self,
        scene: &mut Scene,
        new: MaterialId,
        into: MaterialId,
    ) -> MaterialId {
        if new + 1 != scene.materials.len() || into >= new {
            return new;
        }
        let m = scene.materials.pop().unwrap();
        scene.materials[into] = m;
        into
    }

    pub fn recycle_instance(&self, scene: &mut Scene, new: usize, into: usize) -> usize {
        if new + 1 != scene.instances.len() || into >= new || new < scene.uploaded_instances {
            return new;
        }
        if scene.instances[into].slot_alpha.len() != scene.instances[new].slot_alpha.len() {
            return new;
        }
        let mut inst = scene.instances.pop().unwrap();
        inst.base = scene.instances[into].base;
        scene.instances[into] = inst;
        Self::mark_changed(scene, into);
        into
    }

    pub fn instance_slots(&self, scene: &Scene, instance: usize) -> usize {
        scene
            .instances
            .get(instance)
            .map(|i| i.slot_alpha.len())
            .unwrap_or(0)
    }
}
