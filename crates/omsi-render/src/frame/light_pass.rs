use crate::*;

impl Renderer {
    pub(crate) fn prepare_lights(
        &self,
        scene: &mut Scene,
        cam_rel: Vec3,
        enhanced: bool,
        plan_spots: bool,
    ) -> [f32; 4] {
        let ro = scene.render_origin;
        let spot_slots =
            self.plan_spot_shadows(scene, ro + cam_rel.as_dvec3(), enhanced, plan_spots);
        let side = LIGHT_GRID_SIDE;
        let half = side as f32 * LIGHT_CELL * 0.5;
        let origin = [
            ((cam_rel.x - half) / LIGHT_CELL).floor() * LIGHT_CELL,
            ((cam_rel.y - half) / LIGHT_CELL).floor() * LIGHT_CELL,
        ];
        let mut gpu_lights: Vec<GpuPointLight> =
            Vec::with_capacity(scene.interior_lights.len() + scene.lights.len().max(1));
        for l in &scene.interior_lights {
            gpu_lights.push(gpu_light(l, (l.position - ro).as_vec3()));
        }
        let mut grid = vec![u32::MAX; side * side * LIGHT_CELL_CAP];
        let mut occ_users: Vec<(usize, u32)> = Vec::new();
        for (li, l) in scene.lights.iter().enumerate() {
            if !drawn_by(l, enhanced) {
                continue;
            }
            let p = (l.position - ro).as_vec3();
            let x0 = ((p.x - l.radius - origin[0]) / LIGHT_CELL).floor();
            let x1 = ((p.x + l.radius - origin[0]) / LIGHT_CELL).floor();
            let y0 = ((p.y - l.radius - origin[1]) / LIGHT_CELL).floor();
            let y1 = ((p.y + l.radius - origin[1]) / LIGHT_CELL).floor();
            if x1 < 0.0 || y1 < 0.0 || x0 >= side as f32 || y0 >= side as f32 {
                continue;
            }
            let idx = gpu_lights.len() as u32;
            gpu_lights.push(gpu_light(l, p));
            gpu_lights[idx as usize].occ[2] = spot_slots[li] as f32;
            if l.occ_count > 0
                && (l.occ_first as usize + l.occ_count as usize) <= scene.occluders.len()
            {
                occ_users.push((idx as usize, l.occ_first));
                gpu_lights[idx as usize].occ[1] = l.occ_count as f32;
            }
            for y in (y0.max(0.0) as usize)..=(y1.min(side as f32 - 1.0) as usize) {
                for x in (x0.max(0.0) as usize)..=(x1.min(side as f32 - 1.0) as usize) {
                    let base = (y * side + x) * LIGHT_CELL_CAP;
                    if let Some(slot) = grid[base..base + LIGHT_CELL_CAP]
                        .iter()
                        .position(|v| *v == u32::MAX)
                    {
                        grid[base + slot] = idx;
                    } else if omsi_cfg::env::var_os("OMSI_DEBUG_LIGHT_GRID").is_some() {
                        log::info!(
                            "light grid: cell ({x}, {y}) full, light at ({:.1}, {:.1}) radius {:.0} {} left out",
                            p.x,
                            p.y,
                            l.radius,
                            if l.direction.length_squared() > 0.5 {
                                "spot"
                            } else {
                                "point"
                            }
                        );
                    }
                }
            }
        }
        if !occ_users.is_empty() {
            let base = gpu_lights.len() as u32;
            for o in &scene.occluders {
                if let Some(t) = o.tri {
                    let v = t.map(|p| (p - ro).as_vec3());
                    gpu_lights.push(GpuPointLight {
                        pos: [v[0].x, v[0].y, v[0].z, 0.0],
                        color: [v[1].x, v[1].y, v[1].z, 0.0],
                        dir: [v[2].x, v[2].y, v[2].z, 0.0],
                        extra: [1.0, 0.0, 0.0, 0.0],
                        occ: [0.0; 4],
                    });
                    continue;
                }
                let c = o.center - ro.truncate();
                let (sa, ca) = (o.heading.sin() as f32, o.heading.cos() as f32);
                gpu_lights.push(GpuPointLight {
                    pos: [c.x as f32, c.y as f32, (o.z0 - ro.z) as f32, o.half.x],
                    color: [o.half.y, (o.z1 - ro.z) as f32, ca, sa],
                    dir: [0.0; 4],
                    extra: [0.0; 4],
                    occ: [0.0; 4],
                });
            }
            for (i, first) in occ_users {
                gpu_lights[i].occ[0] = (base + first) as f32;
            }
        }
        if gpu_lights.is_empty() {
            gpu_lights.push(GpuPointLight {
                pos: [0.0; 4],
                color: [0.0; 4],
                dir: [0.0; 4],
                extra: [0.0; 4],
                occ: [0.0; 4],
            });
        }
        let gpu_lights_bytes: &[u8] = bytemuck::cast_slice(&gpu_lights);
        let grid_bytes: &[u8] = bytemuck::cast_slice(&grid);
        let mut rebuilt = false;
        match &scene.light_buf {
            Some(b) if b.size() as usize >= gpu_lights_bytes.len() => {
                if scene.last_lights != gpu_lights_bytes {
                    self.queue.write_buffer(b, 0, gpu_lights_bytes);
                }
            }
            _ => {
                let cap = (gpu_lights_bytes.len() * 2).max(64 * size_of::<GpuPointLight>());
                let b = self.device.create_buffer(&wgpu::BufferDescriptor {
                    label: Some("lights"),
                    size: cap as u64,
                    usage: wgpu::BufferUsages::STORAGE | wgpu::BufferUsages::COPY_DST,
                    mapped_at_creation: false,
                });
                self.queue.write_buffer(&b, 0, gpu_lights_bytes);
                scene.light_buf = Some(b);
                rebuilt = true;
            }
        }
        match &scene.grid_buf {
            Some(b) => {
                if scene.last_grid != grid {
                    self.queue.write_buffer(b, 0, grid_bytes);
                }
            }
            None => {
                scene.grid_buf = Some(buffer_init(
                    &self.device,
                    &self.queue,
                    Some("light grid"),
                    grid_bytes,
                    wgpu::BufferUsages::STORAGE | wgpu::BufferUsages::COPY_DST,
                ));
                rebuilt = true;
            }
        }
        scene.last_lights.clear();
        scene.last_lights.extend_from_slice(gpu_lights_bytes);
        scene.last_grid = grid;
        if rebuilt {
            self.rebuild_camera_bind_group(scene);
        }
        [origin[0], origin[1], LIGHT_CELL, side as f32]
    }

    pub(crate) fn prepare_smoke(&self, scene: &mut Scene, eye: DVec3) {
        let ro = scene.render_origin;
        let mut order: Vec<(f64, usize)> = scene
            .smoke
            .iter()
            .enumerate()
            .filter(|(_, p)| p.alpha > 0.002 && p.size > 0.0)
            .map(|(i, p)| (-(p.position - eye).length_squared(), i))
            .collect();
        order.sort_by(|a, b| a.0.total_cmp(&b.0));
        let data: Vec<GpuCorona> = order
            .iter()
            .map(|&(_, i)| {
                let p = &scene.smoke[i];
                GpuCorona {
                    pos: (p.position - ro).as_vec3().to_array(),
                    size: p.size,
                    color: [p.color[0], p.color[1], p.color[2], p.alpha.clamp(0.0, 1.0)],
                    dir: [0.0, 0.0, 0.0, -1.0],
                    up: [0.0, 0.0, 1.0, 2.0],
                    extra: [-2.0, 0.0, 0.0, 1.0],
                }
            })
            .collect();
        scene.smoke_count = data.len() as u32;
        if data.is_empty() {
            return;
        }
        let bytes: &[u8] = bytemuck::cast_slice(&data);
        match &scene.smoke_buf {
            Some(b) if b.size() as usize >= bytes.len() => self.queue.write_buffer(b, 0, bytes),
            _ => {
                let b = self.device.create_buffer(&wgpu::BufferDescriptor {
                    label: Some("smoke"),
                    size: (bytes.len() * 2) as u64,
                    usage: wgpu::BufferUsages::VERTEX | wgpu::BufferUsages::COPY_DST,
                    mapped_at_creation: false,
                });
                self.queue.write_buffer(&b, 0, bytes);
                scene.smoke_buf = Some(b);
            }
        }
    }

    pub fn set_light_map_tile(&self, slot: (u32, u32), img: Option<&omsi_texture::Image>) {
        let n = LM_TILE_PX as usize;
        let mut px = vec![0u8; n * n * 4];
        if let Some(img) = img.filter(|i| i.width > 0 && i.height > 0) {
            for y in 0..n {
                for x in 0..n {
                    let sx = x * img.width as usize / n;
                    let sy = y * img.height as usize / n;
                    let o = (sy * img.width as usize + sx) * 4;
                    px[(y * n + x) * 4..(y * n + x) * 4 + 4].copy_from_slice(&img.rgba[o..o + 4]);
                }
            }
        }
        self.queue.write_texture(
            wgpu::TexelCopyTextureInfo {
                texture: &self.lm_atlas,
                mip_level: 0,
                origin: wgpu::Origin3d {
                    x: slot.0 * LM_TILE_PX,
                    y: slot.1 * LM_TILE_PX,
                    z: 0,
                },
                aspect: wgpu::TextureAspect::All,
            },
            &px,
            wgpu::TexelCopyBufferLayout {
                offset: 0,
                bytes_per_row: Some(LM_TILE_PX * 4),
                rows_per_image: None,
            },
            wgpu::Extent3d {
                width: LM_TILE_PX,
                height: LM_TILE_PX,
                depth_or_array_layers: 1,
            },
        );
    }

    pub fn set_light_map_place(&self, x: f64, y: f64, side: f64) {
        self.lm_place.set((x, y, side));
    }

    pub fn set_corona_texture(&mut self, id: u16, img: &omsi_texture::Image) {
        let bg = self.picture_bind_group("corona picture", img);
        let i = id as usize;
        if self.corona_textures.len() <= i {
            self.corona_textures.resize_with(i + 1, || None);
        }
        self.corona_textures[i] = Some(bg);
    }

    pub fn set_smoke_texture(&mut self, img: &omsi_texture::Image) {
        self.smoke_bind_group = self.picture_bind_group("smoke", img);
    }

    fn picture_bind_group(&self, label: &str, img: &omsi_texture::Image) -> wgpu::BindGroup {
        let t = upload_texture(&self.device, &self.queue, img, true);
        self.device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some(label),
            layout: &self.corona_layout,
            entries: &[
                wgpu::BindGroupEntry {
                    binding: 0,
                    resource: wgpu::BindingResource::TextureView(&t.view),
                },
                wgpu::BindGroupEntry {
                    binding: 1,
                    resource: wgpu::BindingResource::Sampler(&self.corona_sampler),
                },
            ],
        })
    }

    pub(crate) fn prepare_coronas(&self, scene: &mut Scene, night: f32) {
        let ro = scene.render_origin;
        let mut order: Vec<&Corona> = scene
            .coronas
            .iter()
            .filter(|c| c.brightness > 0.001)
            .collect();
        order.sort_by_key(|c| c.texture);
        let mut runs: Vec<(u16, u32, u32)> = Vec::new();
        for (k, c) in order.iter().enumerate() {
            match runs.last_mut() {
                Some(r) if r.0 == c.texture => r.2 += 1,
                _ => runs.push((c.texture, k as u32, 1)),
            }
        }
        if omsi_cfg::env::var_os("OMSI_DEBUG_CONES").is_some() {
            log::info!(
                "coronas: {} in {} runs {:?}, {} beams",
                order.len(),
                runs.len(),
                runs,
                order.iter().filter(|c| c.beam).count()
            );
        }
        scene.corona_runs = runs;
        let data: Vec<GpuCorona> = order
            .into_iter()
            .map(|c| {
                let p = (c.position - ro).as_vec3();
                let b = if c.cone_cos < -1.5 || c.beam || c.halo {
                    c.brightness
                } else {
                    c.brightness * (night * night + 0.8) * 0.6
                };
                GpuCorona {
                    pos: p.to_array(),
                    size: c.size,
                    color: [c.color[0], c.color[1], c.color[2], b],
                    dir: [c.direction.x, c.direction.y, c.direction.z, c.cone_cos],
                    up: [c.up.x, c.up.y, c.up.z, c.rotating as f32],
                    extra: [
                        c.inner_cos,
                        if c.beam || c.halo {
                            c.beam_width
                        } else {
                            c.z_offset
                        },
                        c.flags as f32,
                        if c.beam {
                            1.0
                        } else if c.halo {
                            2.0
                        } else {
                            0.0
                        },
                    ],
                }
            })
            .collect();
        scene.corona_count = data.len() as u32;
        if data.is_empty() {
            return;
        }
        let bytes: &[u8] = bytemuck::cast_slice(&data);
        match &scene.corona_buf {
            Some(b) if b.size() as usize >= bytes.len() => self.queue.write_buffer(b, 0, bytes),
            _ => {
                let b = self.device.create_buffer(&wgpu::BufferDescriptor {
                    label: Some("coronas"),
                    size: (bytes.len() * 2) as u64,
                    usage: wgpu::BufferUsages::VERTEX | wgpu::BufferUsages::COPY_DST,
                    mapped_at_creation: false,
                });
                self.queue.write_buffer(&b, 0, bytes);
                scene.corona_buf = Some(b);
            }
        }
    }
}
