use crate::*;

impl Renderer {
    #[allow(dead_code)]
    pub(crate) fn ensure_depth(&mut self, w: u32, h: u32) {
        if let Some((_, _, dw, dh)) = &self.depth {
            if *dw == w && *dh == h {
                return;
            }
        }
        let tex = self.device.create_texture(&wgpu::TextureDescriptor {
            label: Some("depth"),
            size: wgpu::Extent3d {
                width: w,
                height: h,
                depth_or_array_layers: 1,
            },
            mip_level_count: 1,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format: DEPTH_FORMAT,
            usage: wgpu::TextureUsages::RENDER_ATTACHMENT,
            view_formats: &[],
        });
        let view = tex.create_view(&Default::default());
        self.depth = Some((tex, view, w, h));
    }

    pub(crate) fn instance_entries(
        i: &Instance,
        ro: DVec3,
        mats: &mut Vec<[[f32; 4]; 4]>,
        params: &mut Vec<[f32; 4]>,
    ) {
        let m =
            (Mat4::from_translation((i.origin - ro).as_vec3()) * i.transform).to_cols_array_2d();
        // Visibility plus the cabin marker for the shader.
        let vis = if i.visible {
            1.0 + i.cabin as u8 as f32
        } else {
            0.0
        };
        for (k, a) in i.slot_alpha.iter().enumerate() {
            mats.push(m);
            let uv = i.slot_uv.get(k).copied().unwrap_or([0.0; 2]);
            params.push([*a, vis, uv[0], uv[1]]);
            params.push([
                i.slot_light.get(k).copied().unwrap_or(1.0),
                i.slot_night.get(k).copied().unwrap_or(1.0),
                if i.interior_lamps != 0 {
                    i.interior_lamps as f32
                } else {
                    i.interior.min(0.99)
                },
                if let Some(roof) = i.roof.filter(|_| !i.blob && !i.surface) {
                    let z = (i.origin - ro).z as f32
                        + i.transform.transform_point3(Vec3::new(0.0, 0.0, roof)).z;
                    -(5000.0 + z.clamp(-4000.0, 4000.0))
                } else {
                    surface_instance_code(
                        i.blob,
                        i.ground_layer,
                        i.decal,
                        i.surface,
                        i.surface_bias,
                    )
                },
            ]);
        }
    }

    pub fn prepare(&self, scene: &mut Scene) {
        Self::prepare_bounds(scene);
        scene.bind_groups.clear();
        if !scene.dirty
            && scene.instances.len() > scene.uploaded_instances
            && scene.model_buf.is_some()
        {
            let ro = scene.render_origin;
            let mut mats: Vec<[[f32; 4]; 4]> = Vec::new();
            let mut params: Vec<[f32; 4]> = Vec::new();
            let first = scene.uploaded_instances;
            let mut base = scene.uploaded_entries;
            for i in scene.instances[first..].iter_mut() {
                i.base = base;
                Self::instance_entries(i, ro, &mut mats, &mut params);
                base = scene.uploaded_entries + mats.len() as u32;
            }
            let (buf, params_buf) = (
                scene.model_buf.as_ref().unwrap(),
                scene.params_buf.as_ref().unwrap(),
            );
            let mo = scene.uploaded_entries as u64 * 64;
            let po = scene.uploaded_entries as u64 * 32;
            let mb: &[u8] = bytemuck::cast_slice(&mats);
            let pb: &[u8] = bytemuck::cast_slice(&params);
            if mo + mb.len() as u64 <= buf.size()
                && po + pb.len() as u64 <= params_buf.size()
                && scene.cpu_models.len() == scene.uploaded_entries as usize
            {
                if !mb.is_empty() {
                    self.queue.write_buffer(buf, mo, mb);
                    self.queue.write_buffer(params_buf, po, pb);
                }
                scene.cpu_models.extend_from_slice(&mats);
                scene.cpu_params.extend_from_slice(&params);
                scene.uploaded_instances = scene.instances.len();
                scene.uploaded_entries = base;
            } else {
                scene.dirty = true;
            }
        }
        if !scene.dirty {
            if !scene.changed.is_empty() {
                if ::legacy_config::env::var_os("OMSI_DEBUG_DRAWS").is_some() {
                    log::info!(
                        "prepare: {} changed instances of {}",
                        scene.changed.len(),
                        scene.instances.len()
                    );
                }
                if let (Some(buf), Some(params_buf)) = (&scene.model_buf, &scene.params_buf) {
                    let ro = scene.render_origin;
                    const MERGE_GAP: u32 = 4096;
                    scene.changed.sort_unstable_by_key(|&i| {
                        scene.instances.get(i).map(|x| x.base).unwrap_or(u32::MAX)
                    });
                    let mut mats: Vec<[[f32; 4]; 4]> = Vec::new();
                    let mut params: Vec<[f32; 4]> = Vec::new();
                    let mut ranges: Vec<(u32, u32)> = Vec::new();
                    for &idx in &scene.changed {
                        let Some(i) = scene.instances.get(idx) else {
                            continue;
                        };
                        mats.clear();
                        params.clear();
                        Self::instance_entries(i, ro, &mut mats, &mut params);
                        let (b, n) = (i.base as usize, mats.len());
                        if b + n > scene.cpu_models.len() || (b + n) * 2 > scene.cpu_params.len() {
                            continue;
                        }
                        scene.cpu_models[b..b + n].copy_from_slice(&mats);
                        scene.cpu_params[b * 2..(b + n) * 2].copy_from_slice(&params);
                        let (start, end) = (b as u32, (b + n) as u32);
                        match ranges.last_mut() {
                            Some(r) if start <= r.1 + MERGE_GAP => r.1 = r.1.max(end),
                            _ => ranges.push((start, end)),
                        }
                    }
                    for (start, end) in ranges {
                        let mb: &[u8] =
                            bytemuck::cast_slice(&scene.cpu_models[start as usize..end as usize]);
                        let pb: &[u8] = bytemuck::cast_slice(
                            &scene.cpu_params[start as usize * 2..end as usize * 2],
                        );
                        let (mo, po) = (start as u64 * 64, start as u64 * 32);
                        if mo + mb.len() as u64 <= buf.size()
                            && po + pb.len() as u64 <= params_buf.size()
                        {
                            self.queue.write_buffer(buf, mo, mb);
                            self.queue.write_buffer(params_buf, po, pb);
                        }
                    }
                }
                for &idx in &scene.changed {
                    if let Some(m) = scene.changed_mark.get_mut(idx) {
                        *m = false;
                    }
                }
                scene.changed.clear();
            }
            return;
        }
        scene.changed.clear();
        scene.changed_mark.clear();
        let mut mats: Vec<[[f32; 4]; 4]> = Vec::new();
        let mut params: Vec<[f32; 4]> = Vec::new();
        let ro = scene.render_origin;
        for i in scene.instances.iter_mut() {
            i.base = mats.len() as u32;
            Self::instance_entries(i, ro, &mut mats, &mut params);
        }
        if mats.is_empty() {
            mats.push(Mat4::IDENTITY.to_cols_array_2d());
            params.push([1.0, 1.0, 0.0, 0.0]);
            params.push([1.0, 1.0, 0.0, 0.0]);
        }
        scene.uploaded_instances = scene.instances.len();
        scene.uploaded_entries = mats.len() as u32;
        scene.cpu_models = mats;
        scene.cpu_params = params;
        let bytes: &[u8] = bytemuck::cast_slice(&scene.cpu_models);
        let cpu_param_bytes: &[u8] = bytemuck::cast_slice(&scene.cpu_params);
        if let (Some(buf), Some(params_buf), Some(_)) = (
            &scene.model_buf,
            &scene.params_buf,
            &scene.camera_bind_group,
        ) {
            if buf.size() as usize >= bytes.len()
                && params_buf.size() as usize >= cpu_param_bytes.len()
            {
                self.queue.write_buffer(buf, 0, bytes);
                self.queue.write_buffer(params_buf, 0, cpu_param_bytes);
                scene.dirty = false;
                return;
            }
        }
        let cap = |n: usize| ((n as f64 * 1.35) as u64 + 65536).max(256).div_ceil(256) * 256;
        let model_buf = self.device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("models"),
            size: cap(bytes.len()),
            usage: wgpu::BufferUsages::STORAGE | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });
        let params_buf = self.device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("params"),
            size: cap(cpu_param_bytes.len()),
            usage: wgpu::BufferUsages::STORAGE | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });
        self.queue.write_buffer(&model_buf, 0, bytes);
        self.queue.write_buffer(&params_buf, 0, cpu_param_bytes);
        scene.model_buf = Some(model_buf);
        scene.params_buf = Some(params_buf);
        self.rebuild_camera_bind_group(scene);
        scene.dirty = false;
    }

    pub(crate) fn rebuild_camera_bind_group(&self, scene: &mut Scene) {
        let (Some(model_buf), Some(params_buf), Some(light_buf), Some(grid_buf), Some(draw_buf)) = (
            &scene.model_buf,
            &scene.params_buf,
            &scene.light_buf,
            &scene.grid_buf,
            &scene.draw_buf,
        ) else {
            return;
        };
        let bg = self.device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("camera"),
            layout: &self.camera_layout,
            entries: &[
                wgpu::BindGroupEntry {
                    binding: 0,
                    resource: self.camera_buf.as_entire_binding(),
                },
                wgpu::BindGroupEntry {
                    binding: 1,
                    resource: model_buf.as_entire_binding(),
                },
                wgpu::BindGroupEntry {
                    binding: 2,
                    resource: params_buf.as_entire_binding(),
                },
                wgpu::BindGroupEntry {
                    binding: 3,
                    resource: light_buf.as_entire_binding(),
                },
                wgpu::BindGroupEntry {
                    binding: 4,
                    resource: grid_buf.as_entire_binding(),
                },
                wgpu::BindGroupEntry {
                    binding: 5,
                    resource: wgpu::BindingResource::TextureView(&self.shadow_view),
                },
                wgpu::BindGroupEntry {
                    binding: 6,
                    resource: wgpu::BindingResource::Sampler(&self.shadow_sampler),
                },
                wgpu::BindGroupEntry {
                    binding: 7,
                    resource: wgpu::BindingResource::TextureView(&self.shadow_view_far),
                },
                wgpu::BindGroupEntry {
                    binding: 8,
                    resource: wgpu::BindingResource::TextureView(
                        self.ao
                            .as_ref()
                            .map(|a| &a.blur_view)
                            .unwrap_or(&self.white_texture.view),
                    ),
                },
                wgpu::BindGroupEntry {
                    binding: 9,
                    resource: wgpu::BindingResource::Sampler(&self.ao_sampler),
                },
                wgpu::BindGroupEntry {
                    binding: 10,
                    resource: draw_buf.as_entire_binding(),
                },
                wgpu::BindGroupEntry {
                    binding: 11,
                    resource: self.enh_buf.as_entire_binding(),
                },
                wgpu::BindGroupEntry {
                    binding: 12,
                    resource: wgpu::BindingResource::TextureView(
                        &self.probe.as_ref().expect("reflection probe").view,
                    ),
                },
                wgpu::BindGroupEntry {
                    binding: 13,
                    resource: wgpu::BindingResource::Sampler(&self.lin_sampler),
                },
                wgpu::BindGroupEntry {
                    binding: 14,
                    resource: wgpu::BindingResource::TextureView(&self.sky_lut_view),
                },
                wgpu::BindGroupEntry {
                    binding: 18,
                    resource: wgpu::BindingResource::TextureView(&self.lm_atlas_view),
                },
                wgpu::BindGroupEntry {
                    binding: 19,
                    resource: self.lm_uniform.as_entire_binding(),
                },
                wgpu::BindGroupEntry {
                    binding: 20,
                    resource: wgpu::BindingResource::TextureView(&self.cookie_atlas_view),
                },
                wgpu::BindGroupEntry {
                    binding: 21,
                    resource: wgpu::BindingResource::Sampler(&self.cookie_sampler),
                },
            ],
        });
        scene.camera_bind_group = Some(bg);
        let sbg = self.device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("shadow camera"),
            layout: &self.shadow_layout,
            entries: &[
                wgpu::BindGroupEntry {
                    binding: 0,
                    resource: self.camera_buf.as_entire_binding(),
                },
                wgpu::BindGroupEntry {
                    binding: 1,
                    resource: model_buf.as_entire_binding(),
                },
                wgpu::BindGroupEntry {
                    binding: 2,
                    resource: params_buf.as_entire_binding(),
                },
                wgpu::BindGroupEntry {
                    binding: 10,
                    resource: draw_buf.as_entire_binding(),
                },
            ],
        });
        scene.shadow_bind_group = Some(sbg);
        scene.spot_bind_groups = self
            .spot_cam_bufs
            .iter()
            .map(|buf| {
                self.device.create_bind_group(&wgpu::BindGroupDescriptor {
                    label: Some("spot shadow camera"),
                    layout: &self.shadow_layout,
                    entries: &[
                        wgpu::BindGroupEntry {
                            binding: 0,
                            resource: buf.as_entire_binding(),
                        },
                        wgpu::BindGroupEntry {
                            binding: 1,
                            resource: model_buf.as_entire_binding(),
                        },
                        wgpu::BindGroupEntry {
                            binding: 2,
                            resource: params_buf.as_entire_binding(),
                        },
                        wgpu::BindGroupEntry {
                            binding: 10,
                            resource: draw_buf.as_entire_binding(),
                        },
                    ],
                })
            })
            .collect();
    }

    pub(crate) fn upload_draw_list(&self, scene: &mut Scene, list: &[u32]) {
        let bytes: &[u8] = bytemuck::cast_slice(if list.is_empty() { &[0u32] } else { list });
        let fits = scene
            .draw_buf
            .as_ref()
            .map(|b| b.size() >= bytes.len() as u64)
            .unwrap_or(false);
        if !fits {
            let cap = (bytes.len() as u64 * 3 / 2).max(1 << 16).div_ceil(4) * 4;
            scene.draw_buf = Some(self.device.create_buffer(&wgpu::BufferDescriptor {
                label: Some("draw list"),
                size: cap,
                usage: wgpu::BufferUsages::STORAGE | wgpu::BufferUsages::COPY_DST,
                mapped_at_creation: false,
            }));
            self.rebuild_camera_bind_group(scene);
        } else if scene.camera_bind_group.is_none() || scene.shadow_bind_group.is_none() {
            self.rebuild_camera_bind_group(scene);
        }
        if let Some(b) = &scene.draw_buf {
            self.queue.write_buffer(b, 0, bytes);
        }
    }
}
