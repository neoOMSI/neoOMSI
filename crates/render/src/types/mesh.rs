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

/// `GpuMesh::page` of a mesh with buffers of its own.
pub(crate) const NO_PAGE: u32 = u32::MAX;
/// The size of a page's vertex and index buffers. A mesh larger than a quarter of either
/// gets buffers of its own.
pub(crate) const MESH_PAGE_VERTEX_BYTES: u64 = 16 << 20;
pub(crate) const MESH_PAGE_INDEX_BYTES: u64 = 8 << 20;

/// A vertex and an index buffer holding many meshes, drawn with a base vertex and a first
/// index. A map's splines, terrain and objects are thousands of small meshes: with buffers
/// of their own every draw bound two other buffers first, the costliest part of recording
/// the picture whenever what is in sight changed. The space a freed mesh leaves goes to
/// the next one that fits.
pub(crate) struct MeshPage {
    pub(crate) vertex: wgpu::Buffer,
    pub(crate) index: wgpu::Buffer,
    vertex_top: u64,
    index_top: u64,
    vertex_free: Vec<(u64, u64)>,
    index_free: Vec<(u64, u64)>,
}

/// First fit of `len` bytes among the `free` (offset, length) spans, else at `top` below
/// `cap`.
pub(crate) fn page_take(free: &mut Vec<(u64, u64)>, top: &mut u64, cap: u64, len: u64) -> Option<u64> {
    if let Some(k) = free.iter().position(|&(_, l)| l >= len) {
        let (at, l) = free[k];
        if l == len {
            free.remove(k);
        } else {
            free[k] = (at + len, l - len);
        }
        return Some(at);
    }
    (*top + len <= cap).then(|| {
        *top += len;
        *top - len
    })
}

/// Give `len` bytes at `at` back, joined to the free spans beside them (and to the top).
pub(crate) fn page_give(free: &mut Vec<(u64, u64)>, top: &mut u64, at: u64, len: u64) {
    if len == 0 {
        return;
    }
    let k = free.partition_point(|&(o, _)| o < at);
    free.insert(k, (at, len));
    if k + 1 < free.len() && free[k].0 + free[k].1 == free[k + 1].0 {
        free[k].1 += free[k + 1].1;
        free.remove(k + 1);
    }
    if k > 0 && free[k - 1].0 + free[k - 1].1 == free[k].0 {
        free[k - 1].1 += free[k].1;
        free.remove(k);
    }
    if let Some(&(o, l)) = free.last()
        && o + l == *top
    {
        *top = o;
        free.pop();
    }
}

impl MeshPage {
    fn new(device: &wgpu::Device) -> Self {
        MeshPage {
            vertex: device.create_buffer(&wgpu::BufferDescriptor {
                label: Some("mesh page vertices"),
                size: MESH_PAGE_VERTEX_BYTES,
                usage: wgpu::BufferUsages::VERTEX | wgpu::BufferUsages::COPY_DST,
                mapped_at_creation: false,
            }),
            index: device.create_buffer(&wgpu::BufferDescriptor {
                label: Some("mesh page indices"),
                size: MESH_PAGE_INDEX_BYTES,
                usage: wgpu::BufferUsages::INDEX | wgpu::BufferUsages::COPY_DST,
                mapped_at_creation: false,
            }),
            vertex_top: 0,
            index_top: 0,
            vertex_free: Vec::new(),
            index_free: Vec::new(),
        }
    }

    /// Room for `vb` vertex and `ib` index bytes: their offsets, or nothing (and nothing
    /// taken) when one of them does not fit.
    fn take(&mut self, vb: u64, ib: u64) -> Option<(u64, u64)> {
        let v = page_take(&mut self.vertex_free, &mut self.vertex_top, MESH_PAGE_VERTEX_BYTES, vb)?;
        match page_take(&mut self.index_free, &mut self.index_top, MESH_PAGE_INDEX_BYTES, ib) {
            Some(i) => Some((v, i)),
            None => {
                page_give(&mut self.vertex_free, &mut self.vertex_top, v, vb);
                None
            }
        }
    }

    fn give(&mut self, m: &GpuMesh) {
        page_give(&mut self.vertex_free, &mut self.vertex_top, m.vertex_offset, m.vertex_bytes);
        page_give(
            &mut self.index_free,
            &mut self.index_top,
            m.first_index as u64 * 4,
            m.index_bytes,
        );
    }
}

/// Give the page space `m` takes back (nothing for a mesh with buffers of its own).
pub(crate) fn release_mesh_space(pages: &mut [MeshPage], m: &GpuMesh) {
    if let Some(p) = pages.get_mut(m.page as usize) {
        p.give(m);
    }
}

impl Scene {
    /// Bytes the meshes' buffers take on the GPU: each page once, whatever share of it is
    /// in use, and the buffers of the meshes with their own (freed ones not counted).
    pub fn mesh_buffer_bytes(&self) -> u64 {
        let pages: u64 = self
            .mesh_pages
            .iter()
            .map(|p| p.vertex.size() + p.index.size())
            .sum();
        let own: u64 = self
            .meshes
            .iter()
            .filter(|m| m.page == NO_PAGE && !m.ranges.is_empty())
            .map(|m| m.vertex_buf.size() + m.index_buf.size())
            .sum();
        pages + own
    }
}

/// A mesh's vertices as the GPU takes them.
fn mesh_vertices(data: &MeshData) -> Vec<Vertex> {
    data.positions
        .iter()
        .zip(&data.normals)
        .zip(&data.uvs)
        .map(|((p, n), uv)| Vertex {
            pos: p.to_array(),
            normal: n.to_array(),
            uv: uv.to_array(),
        })
        .collect()
}

pub(crate) fn make_mesh(device: &wgpu::Device, queue: &wgpu::Queue, data: &MeshData) -> GpuMesh {
    let vertices = mesh_vertices(data);
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
    let (vertex_bytes, index_bytes) = (vertex_buf.size(), index_buf.size());
    mesh_with_buffers(data, vertex_buf, index_buf, NO_PAGE, 0, 0, vertex_bytes, index_bytes)
}

/// The mesh of `data` drawn from these buffers, at `vertex_offset` (bytes) and
/// `first_index`.
#[allow(clippy::too_many_arguments)]
fn mesh_with_buffers(
    data: &MeshData,
    vertex_buf: wgpu::Buffer,
    index_buf: wgpu::Buffer,
    page: u32,
    vertex_offset: u64,
    first_index: u32,
    vertex_bytes: u64,
    index_bytes: u64,
) -> GpuMesh {
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
    // (terrain is never drawn in a ground phase)
    let slot_walls = if data.one_sided {
        slot_wall_heights(data)
    } else {
        Vec::new()
    };
    GpuMesh {
        vertex_buf,
        index_buf,
        page,
        base_vertex: (vertex_offset / std::mem::size_of::<Vertex>() as u64) as i32,
        first_index,
        vertex_offset,
        vertex_bytes,
        index_bytes,
        ranges: data.ranges.clone(),
        bounds_center: center,
        bounds_radius: (hi - center).length(),
        one_sided: data.one_sided,
        source: None,
        slot_faces,
        slot_walls,
    }
}

/// Per material slot: the height of its tallest steep (over 45°) triangle - a facade's
/// height, but next to nothing for roads and kerbs, also on a slope.
pub(crate) fn slot_wall_heights(data: &MeshData) -> Vec<f32> {
    let mut out: Vec<f32> = Vec::new();
    for &(first, count, slot) in &data.ranges {
        let slot = slot as usize;
        if out.len() <= slot {
            out.resize(slot + 1, 0.0);
        }
        let end = ((first + count) as usize).min(data.indices.len());
        for tri in data.indices.get(first as usize..end).unwrap_or(&[]).chunks_exact(3) {
            let p = |i: u32| data.positions.get(i as usize).copied();
            let (Some(a), Some(b), Some(c)) = (p(tri[0]), p(tri[1]), p(tri[2])) else {
                continue;
            };
            let n = (b - a).cross(c - a).normalize_or_zero();
            if n != Vec3::ZERO && n.z.abs() < std::f32::consts::FRAC_1_SQRT_2 {
                let h = a.z.max(b.z).max(c.z) - a.z.min(b.z).min(c.z);
                out[slot] = out[slot].max(h);
            }
        }
    }
    out
}

pub struct PreparedMesh(pub(crate) GpuMesh);

pub fn prepare_mesh(device: &wgpu::Device, queue: &wgpu::Queue, data: &MeshData) -> PreparedMesh {
    PreparedMesh(make_mesh(device, queue, data))
}

impl Renderer {
    /// Draw `instance` with another mesh. Its entries keep their number (the instance's
    /// slots), so only this instance is written again: a person or a bus part given its
    /// skinned copy made every instance of the scene anew.
    pub fn set_instance_mesh(&self, scene: &mut Scene, instance: usize, mesh: MeshId) {
        if scene.instances[instance].mesh != mesh {
            scene.instances[instance].mesh = mesh;
            Self::note_instance_mesh(scene, instance);
            Self::mark_changed(scene, instance);
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
        let m = &mut scene.meshes[id];
        let n = positions.len().min(normals.len()).min(uvs.len());
        if (m.vertex_bytes as usize) < n * std::mem::size_of::<Vertex>() {
            return;
        }
        let (mut lo, mut hi) = (Vec3::splat(f32::MAX), Vec3::splat(f32::MIN));
        // (a list kept from a mesh sent before: a new one for every person's mesh, every
        // frame, was as slow as the copying)
        let mut pending = self.pending_meshes.borrow_mut();
        let verts = pending
            .entry(id)
            .or_insert_with(|| self.spare_vertices.borrow_mut().pop().unwrap_or_default());
        verts.clear();
        verts.reserve(n);
        for ((p, nrm), uv) in positions.iter().zip(normals).zip(uvs) {
            lo = lo.min(*p);
            hi = hi.max(*p);
            verts.push(Vertex {
                pos: p.to_array(),
                normal: nrm.to_array(),
                uv: uv.to_array(),
            });
        }
        // (only the last pose before submission is kept, without walking every other
        // person's mesh)
        drop(pending);
        if n > 0 {
            m.bounds_center = (lo + hi) * 0.5;
            m.bounds_radius = (hi - m.bounds_center).length();
            Self::mesh_reshaped(scene, id);
        }
    }

    /// Sends the animated meshes' new vertices to their buffers, before anything this frame
    /// draws: all of them through one staging buffer, copied at the start of `encoder` (the
    /// first one submitted). A `write_buffer` each made the driver create and free a staging
    /// buffer per mesh, every frame - a few hundredths of a millisecond each, for every car,
    /// person and driver in sight.
    pub(crate) fn flush_pending_meshes(&self, scene: &Scene, encoder: &mut wgpu::CommandEncoder) {
        // Drain retains the table's capacity for the next frame.
        let mut pending = self.pending_meshes.borrow_mut();
        let uploads: Vec<(&wgpu::Buffer, u64, &[u8])> = pending
            .iter()
            .filter_map(|(id, verts)| {
                let m = scene.meshes.get(*id)?;
                let b: &[u8] = bytemuck::cast_slice(verts);
                let len = b.len() / 4 * 4;
                (len > 0 && m.vertex_bytes >= len as u64)
                    .then(|| (&m.vertex_buf, m.vertex_offset, &b[..len]))
            })
            .collect();
        let total: usize = uploads.iter().map(|u| u.2.len()).sum();
        let staging = (uploads.len() > 1).then(|| {
            self.device.create_buffer(&wgpu::BufferDescriptor {
                label: Some("animated meshes"),
                size: total as u64,
                usage: wgpu::BufferUsages::MAP_WRITE | wgpu::BufferUsages::COPY_SRC,
                mapped_at_creation: true,
            })
        });
        let staged = staging.as_ref().is_some_and(|staging| {
            let Ok(mut view) = staging.slice(..).get_mapped_range_mut() else {
                return false;
            };
            let mut at = 0;
            for (_, _, b) in &uploads {
                view.slice(at..at + b.len()).copy_from_slice(b);
                at += b.len();
            }
            true
        });
        match staging {
            Some(staging) if staged => {
                staging.unmap();
                let mut at = 0u64;
                for (buf, offset, b) in &uploads {
                    encoder.copy_buffer_to_buffer(&staging, at, buf, *offset, b.len() as u64);
                    at += b.len() as u64;
                }
            }
            _ => {
                for (buf, offset, b) in &uploads {
                    self.queue.write_buffer(buf, *offset, b);
                }
            }
        }
        drop(uploads);
        // (the lists go back for the next frame; drain retains the table's capacity)
        let mut spare = self.spare_vertices.borrow_mut();
        spare.extend(pending.drain().map(|(_, v)| v));
        // (no more than a frame's worth is kept)
        spare.truncate(512);
    }

    pub fn add_mesh(&self, scene: &mut Scene, data: &MeshData) -> MeshId {
        let mesh = self
            .page_mesh(scene, data)
            .unwrap_or_else(|| make_mesh(&self.device, &self.queue, data));
        scene.meshes.push(mesh);
        scene.meshes.len() - 1
    }

    /// A mesh whose vertices are sent again with every pose (`update_mesh`: people, the
    /// driver, skinned vehicle parts), in buffers of its own: hundreds of small copies a
    /// frame into the shared pages cost more than the bindings they save.
    pub fn add_animated_mesh(&self, scene: &mut Scene, data: &MeshData) -> MeshId {
        scene
            .meshes
            .push(make_mesh(&self.device, &self.queue, data));
        scene.meshes.len() - 1
    }

    /// The mesh of `data` placed in one of the scene's pages (a new one when none has
    /// room); nothing when the adapter cannot draw from pages or the mesh is too big for them.
    fn page_mesh(&self, scene: &mut Scene, data: &MeshData) -> Option<GpuMesh> {
        if !self.mesh_pages {
            return None;
        }
        let vertices = mesh_vertices(data);
        let (vb, ib) = (
            (vertices.len() * std::mem::size_of::<Vertex>()) as u64,
            (data.indices.len() * 4) as u64,
        );
        if vb == 0 || ib == 0 || vb > MESH_PAGE_VERTEX_BYTES / 4 || ib > MESH_PAGE_INDEX_BYTES / 4 {
            return None;
        }
        let found = scene
            .mesh_pages
            .iter_mut()
            .enumerate()
            .find_map(|(k, p)| p.take(vb, ib).map(|at| (k, at)));
        let (k, (v_at, i_at)) = match found {
            Some(f) => f,
            None => {
                let mut page = MeshPage::new(&self.device);
                let at = page.take(vb, ib)?;
                scene.mesh_pages.push(page);
                (scene.mesh_pages.len() - 1, at)
            }
        };
        let page = &scene.mesh_pages[k];
        self.queue.write_buffer(&page.vertex, v_at, bytemuck::cast_slice(&vertices));
        self.queue.write_buffer(&page.index, i_at, bytemuck::cast_slice(&data.indices));
        Some(mesh_with_buffers(
            data,
            page.vertex.clone(),
            page.index.clone(),
            k as u32,
            v_at,
            (i_at / 4) as u32,
            vb,
            ib,
        ))
    }

    pub fn add_prepared_mesh(&self, scene: &mut Scene, mesh: PreparedMesh) -> MeshId {
        scene.meshes.push(mesh.0);
        scene.meshes.len() - 1
    }

    pub fn mesh_bytes(&self, scene: &Scene) -> u64 {
        scene.mesh_buffer_bytes()
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
