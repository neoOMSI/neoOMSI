use crate::*;

pub(crate) fn buffer_init(
    device: &wgpu::Device,
    queue: &wgpu::Queue,
    label: Option<&str>,
    contents: &[u8],
    usage: wgpu::BufferUsages,
) -> wgpu::Buffer {
    let align = wgpu::COPY_BUFFER_ALIGNMENT as usize;
    let size = contents.len().next_multiple_of(align).max(align);
    let buf = device.create_buffer(&wgpu::BufferDescriptor {
        label,
        size: size as u64,
        usage: usage | wgpu::BufferUsages::COPY_DST,
        mapped_at_creation: false,
    });
    if !contents.is_empty() {
        if contents.len() == size {
            queue.write_buffer(&buf, 0, contents);
        } else {
            let mut padded = contents.to_vec();
            padded.resize(size, 0);
            queue.write_buffer(&buf, 0, &padded);
        }
    }
    buf
}

pub(crate) fn make_mesh(device: &wgpu::Device, queue: &wgpu::Queue, data: &MeshData) -> GpuMesh {
    let vertices: Vec<Vertex> = data
        .positions
        .iter()
        .zip(&data.normals)
        .zip(&data.uvs)
        .map(|((p, n), uv)| Vertex {
            pos: p.to_array(),
            normal: n.to_array(),
            uv: uv.to_array(),
        })
        .collect();
    let vertex_buf = buffer_init(
        device,
        queue,
        None,
        bytemuck::cast_slice(&vertices),
        wgpu::BufferUsages::VERTEX | wgpu::BufferUsages::COPY_DST,
    );
    let index_buf = buffer_init(
        device,
        queue,
        None,
        bytemuck::cast_slice(&data.indices),
        wgpu::BufferUsages::INDEX,
    );
    let (mut lo, mut hi) = (Vec3::splat(f32::MAX), Vec3::splat(f32::MIN));
    for p in &data.positions {
        lo = lo.min(*p);
        hi = hi.max(*p);
    }
    if data.positions.is_empty() {
        lo = Vec3::ZERO;
        hi = Vec3::ZERO;
    }
    let center = (lo + hi) * 0.5;
    // (only content meshes can carry a screen; generated geometry - terrain, splines - is
    // rebuilt all the time and skips this walk)
    let slot_faces: Vec<(Vec3, Vec3)> = if !data.one_sided {
        Vec::new()
    } else {
        let mut acc: Vec<(Vec3, Vec3, u32)> = Vec::new();
        for &(first, count, slot) in &data.ranges {
            let slot = slot as usize;
            if acc.len() <= slot {
                acc.resize(slot + 1, (Vec3::ZERO, Vec3::ZERO, 0));
            }
            let end = ((first + count) as usize).min(data.indices.len());
            for &i in data.indices.get(first as usize..end).unwrap_or(&[]) {
                let i = i as usize;
                if let (Some(p), Some(n)) = (data.positions.get(i), data.normals.get(i)) {
                    acc[slot].0 += *n;
                    acc[slot].1 += *p;
                    acc[slot].2 += 1;
                }
            }
        }
        acc.into_iter()
            .map(|(n, p, c)| {
                (
                    n.normalize_or_zero(),
                    if c > 0 { p / c as f32 } else { Vec3::ZERO },
                )
            })
            .collect()
    };
    GpuMesh {
        vertex_buf,
        index_buf,
        ranges: data.ranges.clone(),
        bounds_center: center,
        bounds_radius: (hi - center).length(),
        one_sided: data.one_sided,
        source: None,
        slot_faces,
    }
}

pub struct PreparedMesh(pub(crate) GpuMesh);

pub fn prepare_mesh(device: &wgpu::Device, queue: &wgpu::Queue, data: &MeshData) -> PreparedMesh {
    let _turn = gl_worker_turn();
    PreparedMesh(make_mesh(device, queue, data))
}

impl Renderer {
    pub fn set_instance_mesh(&self, scene: &mut Scene, instance: usize, mesh: MeshId) {
        if scene.instances[instance].mesh != mesh {
            scene.instances[instance].mesh = mesh;
            scene.dirty = true;
        }
    }

    pub fn update_mesh(
        &self,
        scene: &mut Scene,
        id: MeshId,
        positions: &[Vec3],
        normals: &[Vec3],
        uvs: &[glam::Vec2],
    ) {
        let vertices: Vec<Vertex> = positions
            .iter()
            .zip(normals)
            .zip(uvs)
            .map(|((p, n), uv)| Vertex {
                pos: p.to_array(),
                normal: n.to_array(),
                uv: uv.to_array(),
            })
            .collect();
        let bytes: &[u8] = bytemuck::cast_slice(&vertices);
        let m = &mut scene.meshes[id];
        if (m.vertex_buf.size() as usize) < bytes.len() {
            return;
        }
        {
            let mut pending = self.pending_meshes.borrow_mut();
            match pending.iter_mut().find(|(mid, _)| *mid == id) {
                Some(e) => e.1 = bytes.to_vec(),
                None => pending.push((id, bytes.to_vec())),
            }
        }
        let (mut lo, mut hi) = (Vec3::splat(f32::MAX), Vec3::splat(f32::MIN));
        for p in positions {
            lo = lo.min(*p);
            hi = hi.max(*p);
        }
        if !positions.is_empty() {
            m.bounds_center = (lo + hi) * 0.5;
            m.bounds_radius = (hi - m.bounds_center).length();
            Self::mesh_bounds_changed(scene, id);
        }
    }

    pub(crate) fn flush_pending_meshes(&self, scene: &Scene, _encoder: &mut wgpu::CommandEncoder) {
        let pending = std::mem::take(&mut *self.pending_meshes.borrow_mut());
        for (id, b) in &pending {
            let Some(m) = scene.meshes.get(*id) else {
                continue;
            };
            let len = (b.len() as u64) / 4 * 4;
            if len == 0 || m.vertex_buf.size() < len {
                continue;
            }
            self.queue
                .write_buffer(&m.vertex_buf, 0, &b[..len as usize]);
        }
    }

    pub fn add_mesh(&self, scene: &mut Scene, data: &MeshData) -> MeshId {
        scene
            .meshes
            .push(make_mesh(&self.device, &self.queue, data));
        scene.meshes.len() - 1
    }

    pub fn add_prepared_mesh(&self, scene: &mut Scene, mesh: PreparedMesh) -> MeshId {
        scene.meshes.push(mesh.0);
        scene.meshes.len() - 1
    }

    pub fn mesh_bytes(&self, scene: &Scene) -> u64 {
        let freed = self
            .freed
            .get()
            .map(|f| (f.vertex_buf.clone(), f.index_buf.clone()));
        scene
            .meshes
            .iter()
            .filter(|m| freed.as_ref().is_none_or(|(v, _)| m.vertex_buf != *v))
            .map(|m| m.vertex_buf.size() + m.index_buf.size())
            .sum()
    }

    pub fn upload_speed_mb_s(&self) -> f64 {
        let n = 1024u32;
        let size = wgpu::Extent3d {
            width: n,
            height: n,
            depth_or_array_layers: 1,
        };
        let texture = self.device.create_texture(&wgpu::TextureDescriptor {
            label: Some("upload check"),
            size,
            mip_level_count: 1,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format: wgpu::TextureFormat::Rgba8Unorm,
            usage: wgpu::TextureUsages::COPY_DST,
            view_formats: &[],
        });
        let data = vec![0u8; (n * n * 4) as usize];
        let rounds = 4;
        let t = std::time::Instant::now();
        for _ in 0..rounds {
            self.queue.write_texture(
                wgpu::TexelCopyTextureInfo {
                    texture: &texture,
                    mip_level: 0,
                    origin: wgpu::Origin3d::ZERO,
                    aspect: wgpu::TextureAspect::All,
                },
                &data,
                wgpu::TexelCopyBufferLayout {
                    offset: 0,
                    bytes_per_row: Some(n * 4),
                    rows_per_image: Some(n),
                },
                size,
            );
        }
        let secs = t.elapsed().as_secs_f64().max(1e-6);
        self.queue.submit([]);
        texture.destroy();
        (data.len() * rounds) as f64 / 1e6 / secs
    }
}
