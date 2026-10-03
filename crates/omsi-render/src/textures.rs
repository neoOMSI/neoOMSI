use super::{MaterialMaps, PbrMaps, Renderer, Scene, TextureId, fit_texture, gl_worker_turn};

struct RgbaRef<'a> {
    width: u32,
    height: u32,
    rgba: &'a [u8],
}

pub struct GpuTexture {
    #[allow(dead_code)]
    pub(super) texture: wgpu::Texture,
    pub(super) view: wgpu::TextureView,
    pub(super) size: (u32, u32),
    /// GPU storage across all mip levels, or 0 for a freed-slot placeholder.
    pub(super) bytes: u64,
    /// Changes on replacement to invalidate material bind-group cache keys.
    pub(super) generation: u64,
}

static TEXTURE_GEN: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(1);

pub(super) fn next_gen() -> u64 {
    TEXTURE_GEN.fetch_add(1, std::sync::atomic::Ordering::Relaxed)
}

impl GpuTexture {
    /// A slot that shows `view` (for example, the picture behind rain on glass) while
    /// `texture` remains a stand-in for texture metadata.
    pub(super) fn showing(
        texture: wgpu::Texture,
        view: wgpu::TextureView,
        size: (u32, u32),
    ) -> GpuTexture {
        GpuTexture {
            texture,
            view,
            size,
            bytes: 0,
            generation: next_gen(),
        }
    }

    pub(super) fn new(texture: wgpu::Texture, size: (u32, u32), bytes: u64) -> GpuTexture {
        let view = texture.create_view(&Default::default());
        GpuTexture {
            texture,
            view,
            size,
            bytes,
            generation: next_gen(),
        }
    }
}

pub(super) fn texture_bytes(format: wgpu::TextureFormat, w: u32, h: u32, levels: u32) -> u64 {
    let (bw, bh) = format.block_dimensions();
    let block = format.block_copy_size(None).unwrap_or(4) as u64;
    (0..levels)
        .map(|l| ((w >> l).max(1).div_ceil(bw) * (h >> l).max(1).div_ceil(bh)) as u64 * block)
        .sum()
}

impl Scene {
    /// Bytes of one texture on the GPU (0 for a freed slot or an unknown id).
    pub fn texture_bytes_of(&self, id: TextureId) -> u64 {
        self.textures.get(id).map(|t| t.bytes).unwrap_or(0)
    }

    /// Base-level dimensions of one texture.
    pub fn texture_size_of(&self, id: TextureId) -> Option<(u32, u32)> {
        self.textures.get(id).map(|t| t.size)
    }

    /// The GPU format of one texture, for statistics.
    pub fn texture_format_of(&self, id: TextureId) -> String {
        self.textures
            .get(id)
            .map(|t| format!("{:?}", t.texture.format()))
            .unwrap_or_default()
    }
}

impl Renderer {
    /// Put a texture made on another thread ([`prepare_texture`]) into the scene.
    pub fn add_prepared_texture(&self, scene: &mut Scene, texture: PreparedTexture) -> TextureId {
        scene.textures.push(texture.0);
        scene.textures.len() - 1
    }

    pub fn add_texture(
        &self,
        scene: &mut Scene,
        img: &omsi_texture::Image,
        mipmaps: bool,
    ) -> TextureId {
        let t = if mipmaps && img.width > 1 && img.height > 1 {
            self.upload_texture_gpu_mips(img)
        } else {
            upload_texture(&self.device, &self.queue, img, false)
        };
        scene.textures.push(t);
        scene.textures.len() - 1
    }

    pub fn add_blank_texture(&self, scene: &mut Scene, width: u32, height: u32) -> TextureId {
        let (width, height) = (width.max(1), height.max(1));
        // Script displays may be sampled before their first `STUnlock`, so initialize them
        // to transparent.
        let image = omsi_texture::Image {
            width,
            height,
            rgba: vec![0; (width * height * 4) as usize],
            has_alpha: true,
        };
        let texture = upload_texture(&self.device, &self.queue, &image, false);
        scene.textures.push(texture);
        scene.textures.len() - 1
    }

    /// The PBR set found beside diffuse texture `diffuse` (`omsi_texture::pbr`): its maps up
    /// (as data, not colours) and known to the materials made with that texture from now on.
    pub fn add_pbr_maps(
        &self,
        scene: &mut Scene,
        diffuse: TextureId,
        set: &omsi_texture::pbr::PbrImages,
    ) {
        // sRGB decoding turns a stored value of 128 into about 0.22, which distorts normal
        // maps. Convert byte values through the sRGB curve before storing them.
        let lut: Vec<u8> = (0..256)
            .map(|v| {
                let l = v as f32 / 255.0;
                let s = if l <= 0.003_130_8 {
                    l * 12.92
                } else {
                    1.055 * l.powf(1.0 / 2.4) - 0.055
                };
                (s * 255.0 + 0.5).clamp(0.0, 255.0) as u8
            })
            .collect();
        let mut up = |img: &omsi_texture::Image| {
            let data = omsi_texture::Image {
                width: img.width,
                height: img.height,
                rgba: img.rgba.iter().map(|b| lut[*b as usize]).collect(),
                has_alpha: false,
            };
            self.add_texture(scene, &data, true)
        };
        let normal = set.normal.as_ref().map(&mut up);
        let orm = set.orm.as_ref().map(&mut up);
        scene.pbr_maps.insert(
            diffuse,
            PbrMaps {
                normal,
                orm,
                flags: set.flags,
            },
        );
    }

    /// The device takes BC1-3 (DXT) textures.
    pub fn supports_bc(&self) -> bool {
        self.device
            .features()
            .contains(wgpu::Features::TEXTURE_COMPRESSION_BC)
    }

    /// Upload a texture prepared by `omsi_texture::gpu` (blocks with their levels, or RGBA
    /// whose chain is made here).
    pub fn add_texture_data(
        &self,
        scene: &mut Scene,
        data: &omsi_texture::TextureData,
    ) -> TextureId {
        let t = self.upload_texture_data(data);
        scene.textures.push(t);
        scene.textures.len() - 1
    }

    fn upload_texture_data(&self, data: &omsi_texture::TextureData) -> GpuTexture {
        if let Some(small) = fit_texture(data, self.device.limits().max_texture_dimension_2d) {
            return self.upload_texture_data(&small);
        }
        if let Some(t) = prepare_texture(&self.device, &self.queue, data) {
            return t.0;
        }
        use omsi_texture::PixelFormat;
        let (w, h) = (data.width.max(1), data.height.max(1));
        let blocks_ok =
            !data.format.is_compressed() || (self.supports_bc() && w % 4 == 0 && h % 4 == 0);
        if !blocks_ok || data.levels.is_empty() {
            // Decode when the prepared levels cannot be uploaded directly.
            let rgba = match (data.format, data.levels.first()) {
                (PixelFormat::Rgba8, Some(l)) => l.clone(),
                (f, Some(l)) => omsi_texture::bc::decode(
                    l,
                    w,
                    h,
                    match f {
                        PixelFormat::Bc1 => omsi_texture::bc::Bc::Bc1 { punch: true },
                        PixelFormat::Bc2 => omsi_texture::bc::Bc::Bc2,
                        _ => omsi_texture::bc::Bc::Bc3,
                    },
                ),
                _ => vec![255; (w * h * 4) as usize],
            };
            return self.upload_texture_gpu_mips(&omsi_texture::Image {
                width: w,
                height: h,
                rgba,
                has_alpha: data.has_alpha,
            });
        }
        // The GPU builds the mip chain directly from the borrowed RGBA level.
        self.upload_rgba_gpu_mips(w, h, &data.levels[0])
    }

    pub fn texture_bytes(&self, scene: &Scene) -> u64 {
        scene.textures.iter().map(|t| t.bytes).sum()
    }

    /// Bytes of one texture (0 for a freed slot).
    pub fn texture_size_bytes(&self, scene: &Scene, id: TextureId) -> u64 {
        scene.textures.get(id).map(|t| t.bytes).unwrap_or(0)
    }

    /// Upload a texture and build its mip chain on the GPU: level 0 is written, every
    /// further level is the one above drawn at half size.
    fn upload_texture_gpu_mips(&self, img: &omsi_texture::Image) -> GpuTexture {
        self.upload_rgba_gpu_mips(img.width, img.height, &img.rgba)
    }

    fn upload_rgba_gpu_mips(&self, width: u32, height: u32, rgba: &[u8]) -> GpuTexture {
        let img = RgbaRef {
            width,
            height,
            rgba,
        };
        let mip_count = (32 - img.width.max(img.height).leading_zeros()).max(1);
        let size = wgpu::Extent3d {
            width: img.width,
            height: img.height,
            depth_or_array_layers: 1,
        };
        let texture = self.device.create_texture(&wgpu::TextureDescriptor {
            label: None,
            size,
            mip_level_count: mip_count,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format: wgpu::TextureFormat::Rgba8UnormSrgb,
            usage: wgpu::TextureUsages::TEXTURE_BINDING
                | wgpu::TextureUsages::COPY_DST
                | wgpu::TextureUsages::COPY_SRC
                | wgpu::TextureUsages::RENDER_ATTACHMENT,
            view_formats: &[],
        });
        self.queue.write_texture(
            wgpu::TexelCopyTextureInfo {
                texture: &texture,
                mip_level: 0,
                origin: wgpu::Origin3d::ZERO,
                aspect: wgpu::TextureAspect::All,
            },
            img.rgba,
            wgpu::TexelCopyBufferLayout {
                offset: 0,
                bytes_per_row: Some(img.width * 4),
                rows_per_image: Some(img.height),
            },
            size,
        );
        self.generate_mip_chain(&texture, mip_count);
        let view = texture.create_view(&Default::default());
        GpuTexture {
            texture,
            view,
            size: (img.width, img.height),
            bytes: texture_bytes(
                wgpu::TextureFormat::Rgba8UnormSrgb,
                img.width,
                img.height,
                mip_count,
            ),
            generation: next_gen(),
        }
    }

    /// Build lower levels from the preceding level. The level-zero pixels are already
    /// uploaded, and [`upload_rgba_gpu_mips`] creates the texture with `RENDER_ATTACHMENT` usage.
    fn generate_mip_chain(&self, texture: &wgpu::Texture, mip_count: u32) {
        let mut encoder = self
            .device
            .create_command_encoder(&wgpu::CommandEncoderDescriptor {
                label: Some("mips"),
            });
        for level in 1..mip_count {
            let src = texture.create_view(&wgpu::TextureViewDescriptor {
                base_mip_level: level - 1,
                mip_level_count: Some(1),
                ..Default::default()
            });
            let dst = texture.create_view(&wgpu::TextureViewDescriptor {
                base_mip_level: level,
                mip_level_count: Some(1),
                ..Default::default()
            });
            let bg = self.device.create_bind_group(&wgpu::BindGroupDescriptor {
                label: Some("mip"),
                layout: &self.mip_layout,
                entries: &[
                    wgpu::BindGroupEntry {
                        binding: 0,
                        resource: wgpu::BindingResource::TextureView(&src),
                    },
                    wgpu::BindGroupEntry {
                        binding: 1,
                        resource: wgpu::BindingResource::Sampler(&self.mip_sampler),
                    },
                ],
            });
            let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                label: Some("mip"),
                color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                    view: &dst,
                    depth_slice: None,
                    resolve_target: None,
                    ops: wgpu::Operations {
                        load: wgpu::LoadOp::Clear(wgpu::Color::BLACK),
                        store: wgpu::StoreOp::Store,
                    },
                })],
                depth_stencil_attachment: None,
                timestamp_writes: None,
                occlusion_query_set: None,
                multiview_mask: None,
            });
            pass.set_pipeline(&self.mip_pipeline);
            pass.set_bind_group(0, &bg, &[]);
            pass.draw(0..3, 0..1);
        }
        self.queue.submit([encoder.finish()]);
    }

    /// The view of a texture of the scene (to draw into it with another pipeline).
    pub fn texture_view(&self, scene: &Scene, id: TextureId) -> Option<wgpu::TextureView> {
        scene.textures.get(id).map(|t| t.view.clone())
    }

    /// Replace the pixels of a texture (same size as when created, no mipmaps regenerated).
    pub fn update_texture(&self, scene: &Scene, id: TextureId, img: &omsi_texture::Image) {
        let t = &scene.textures[id].texture;
        self.queue.write_texture(
            wgpu::TexelCopyTextureInfo {
                texture: t,
                mip_level: 0,
                origin: wgpu::Origin3d::ZERO,
                aspect: wgpu::TextureAspect::All,
            },
            &img.rgba,
            wgpu::TexelCopyBufferLayout {
                offset: 0,
                bytes_per_row: Some(img.width * 4),
                rows_per_image: Some(img.height),
            },
            wgpu::Extent3d {
                width: img.width,
                height: img.height,
                depth_or_array_layers: 1,
            },
        );
    }

    /// Replace the top pixels of a dynamic texture and rebuild its mip chain. Returns true
    /// when the texture had to be recreated (and its materials therefore need rebinding).
    /// This is used for OMSI script textures after `STFilter`.
    pub fn update_texture_mips(
        &self,
        scene: &mut Scene,
        id: TextureId,
        img: &omsi_texture::Image,
    ) -> bool {
        let Some(old) = scene.textures.get(id) else {
            return false;
        };
        let levels = (32 - img.width.max(img.height).leading_zeros()).max(1);
        if old.size != (img.width, img.height) || old.texture.mip_level_count() != levels {
            scene.textures[id] = self.upload_texture_gpu_mips(img);
            return true;
        }
        self.update_texture(scene, id, img);
        if levels > 1 {
            self.generate_mip_chain(&scene.textures[id].texture, levels);
        }
        false
    }

    /// Replace texture slot `id` with `data`; call [`Renderer::rebind_textures`] to refresh
    /// materials that use it.
    pub fn replace_texture(
        &self,
        scene: &mut Scene,
        id: TextureId,
        data: &omsi_texture::TextureData,
    ) {
        if id >= scene.textures.len() {
            return;
        }
        scene.textures[id] = self.upload_texture_data(data);
    }

    /// Mip levels and size of a texture: (width, height, levels).
    pub fn texture_levels(&self, scene: &Scene, id: TextureId) -> Option<(u32, u32, u32)> {
        let t = scene.textures.get(id)?;
        (t.bytes > 0).then(|| (t.size.0, t.size.1, t.texture.mip_level_count()))
    }

    /// Drop the `n` finest mip levels to reduce GPU memory use. The remaining levels are
    /// copied into a smaller texture. Returns false if the texture cannot shrink, for example
    /// when a block format would no longer be aligned. Rebind materials afterward.
    pub fn drop_top_levels(&self, scene: &mut Scene, id: TextureId, n: u32) -> bool {
        let Some(t) = scene.textures.get(id) else {
            return false;
        };
        let (w, h) = t.size;
        let levels = t.texture.mip_level_count();
        let format = t.texture.format();
        if n == 0 || n >= levels || !t.texture.usage().contains(wgpu::TextureUsages::COPY_SRC) {
            return false;
        }
        let (nw, nh) = ((w >> n).max(1), (h >> n).max(1));
        let (bw, bh) = format.block_dimensions();
        if nw % bw != 0 || nh % bh != 0 {
            return false;
        }
        let new_levels = levels - n;
        let texture = self.device.create_texture(&wgpu::TextureDescriptor {
            label: None,
            size: wgpu::Extent3d {
                width: nw,
                height: nh,
                depth_or_array_layers: 1,
            },
            mip_level_count: new_levels,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format,
            usage: wgpu::TextureUsages::TEXTURE_BINDING
                | wgpu::TextureUsages::COPY_DST
                | wgpu::TextureUsages::COPY_SRC,
            view_formats: &[],
        });
        let mut encoder = self
            .device
            .create_command_encoder(&wgpu::CommandEncoderDescriptor {
                label: Some("drop levels"),
            });
        for l in 0..new_levels {
            let (lw, lh) = ((nw >> l).max(1), (nh >> l).max(1));
            let size = wgpu::Extent3d {
                width: lw.div_ceil(bw) * bw,
                height: lh.div_ceil(bh) * bh,
                depth_or_array_layers: 1,
            };
            encoder.copy_texture_to_texture(
                wgpu::TexelCopyTextureInfo {
                    texture: &t.texture,
                    mip_level: l + n,
                    origin: wgpu::Origin3d::ZERO,
                    aspect: wgpu::TextureAspect::All,
                },
                wgpu::TexelCopyTextureInfo {
                    texture: &texture,
                    mip_level: l,
                    origin: wgpu::Origin3d::ZERO,
                    aspect: wgpu::TextureAspect::All,
                },
                size,
            );
        }
        self.queue.submit([encoder.finish()]);
        scene.textures[id] =
            GpuTexture::new(texture, (nw, nh), texture_bytes(format, nw, nh, new_levels));
        true
    }

    /// Rebuild the bind groups of the materials that sample any of `ids` (after
    /// [`Renderer::replace_texture`]). Returns how many were rebuilt.
    pub fn rebind_textures(&self, scene: &mut Scene, ids: &[TextureId]) -> usize {
        if ids.is_empty() {
            return 0;
        }
        let textures = &scene.textures;
        let pbr_maps = &scene.pbr_maps;
        let set: std::collections::HashSet<TextureId> = ids.iter().copied().collect();
        let mut n = 0;
        for m in scene.materials.iter_mut() {
            let uses = [
                m.texture,
                m.nightmap,
                m.lightmap,
                m.envmap.map(|e| e.0),
                m.transmap.map(|t| t.0),
                m.env_mask,
                m.bump.map(|b| b.0),
            ]
            .iter()
            .flatten()
            .any(|t| set.contains(t));
            if !uses {
                continue;
            }
            m.bind_group = self.material_bind_group(
                textures,
                MaterialMaps {
                    texture: m.texture,
                    transmap: m.transmap,
                    nightmap: m.nightmap,
                    lightmap: m.lightmap,
                    envmap: m.envmap,
                    env_mask: m.env_mask,
                    bump: m.bump,
                    pbr: m.texture.and_then(|t| pbr_maps.get(&t)).copied(),
                },
                m.address,
                &m.buf,
            );
            n += 1;
        }
        n
    }
}

/// A texture on the GPU, made on a worker thread; [`Renderer::add_prepared_texture`] puts it
/// into a scene.
pub struct PreparedTexture(GpuTexture);

impl PreparedTexture {
    pub fn bytes(&self) -> u64 {
        self.0.bytes
    }
}

/// Prepare data that already carries uploadable mip levels on a worker thread. Returns `None`
/// if its level data is empty, RGBA mips must be generated on the GPU, or compressed data is
/// unsupported.
pub fn prepare_texture(
    device: &wgpu::Device,
    queue: &wgpu::Queue,
    data: &omsi_texture::TextureData,
) -> Option<PreparedTexture> {
    if let Some(small) = fit_texture(data, device.limits().max_texture_dimension_2d) {
        return prepare_texture(device, queue, &small);
    }
    let _turn = gl_worker_turn();
    use omsi_texture::PixelFormat;
    let format = match data.format {
        PixelFormat::Rgba8 => wgpu::TextureFormat::Rgba8UnormSrgb,
        PixelFormat::Bc1 => wgpu::TextureFormat::Bc1RgbaUnormSrgb,
        PixelFormat::Bc2 => wgpu::TextureFormat::Bc2RgbaUnormSrgb,
        PixelFormat::Bc3 => wgpu::TextureFormat::Bc3RgbaUnormSrgb,
    };
    let (w, h) = (data.width.max(1), data.height.max(1));
    if data.levels.is_empty()
        || (data.format == PixelFormat::Rgba8
            && data.levels.len() == 1
            && data.gpu_mips
            && w > 1
            && h > 1)
    {
        return None;
    }
    if data.format.is_compressed()
        && (!device
            .features()
            .contains(wgpu::Features::TEXTURE_COMPRESSION_BC)
            || w % 4 != 0
            || h % 4 != 0)
    {
        return None;
    }
    let levels = data.levels.len() as u32;
    let size = wgpu::Extent3d {
        width: w,
        height: h,
        depth_or_array_layers: 1,
    };
    let texture = device.create_texture(&wgpu::TextureDescriptor {
        label: None,
        size,
        mip_level_count: levels,
        sample_count: 1,
        dimension: wgpu::TextureDimension::D2,
        format,
        usage: wgpu::TextureUsages::TEXTURE_BINDING
            | wgpu::TextureUsages::COPY_DST
            | wgpu::TextureUsages::COPY_SRC,
        view_formats: &[],
    });
    let (bw, bh) = format.block_dimensions();
    let block = format.block_copy_size(None).unwrap_or(4);
    for (l, bytes) in data.levels.iter().enumerate() {
        let (lw, lh) = ((w >> l).max(1), (h >> l).max(1));
        // compressed levels are written whole blocks at a time, also below 4x4
        let phys = wgpu::Extent3d {
            width: lw.div_ceil(bw) * bw,
            height: lh.div_ceil(bh) * bh,
            depth_or_array_layers: 1,
        };
        let need = ((phys.width / bw) * (phys.height / bh) * block) as usize;
        if bytes.len() < need {
            log::warn!(
                "texture level {l} of {w}x{h} {:?} has {} bytes, not {need}",
                data.format,
                bytes.len()
            );
            break;
        }
        queue.write_texture(
            wgpu::TexelCopyTextureInfo {
                texture: &texture,
                mip_level: l as u32,
                origin: wgpu::Origin3d::ZERO,
                aspect: wgpu::TextureAspect::All,
            },
            &bytes[..need],
            wgpu::TexelCopyBufferLayout {
                offset: 0,
                bytes_per_row: Some((phys.width / bw) * block),
                rows_per_image: Some(phys.height / bh),
            },
            phys,
        );
    }
    Some(PreparedTexture(GpuTexture::new(
        texture,
        (w, h),
        texture_bytes(format, w, h, levels),
    )))
}

pub(super) fn upload_texture(
    device: &wgpu::Device,
    queue: &wgpu::Queue,
    img: &omsi_texture::Image,
    mipmaps: bool,
) -> GpuTexture {
    let mip_count = if mipmaps {
        (32 - img.width.max(img.height).leading_zeros()).max(1)
    } else {
        1
    };
    let size = wgpu::Extent3d {
        width: img.width,
        height: img.height,
        depth_or_array_layers: 1,
    };
    let texture = device.create_texture(&wgpu::TextureDescriptor {
        label: None,
        size,
        mip_level_count: mip_count,
        sample_count: 1,
        dimension: wgpu::TextureDimension::D2,
        format: wgpu::TextureFormat::Rgba8UnormSrgb,
        usage: wgpu::TextureUsages::TEXTURE_BINDING | wgpu::TextureUsages::COPY_DST,
        view_formats: &[],
    });
    // CPU box-filter mip chain
    let mut level: Vec<u8> = img.rgba.clone();
    let (mut w, mut h) = (img.width, img.height);
    for mip in 0..mip_count {
        queue.write_texture(
            wgpu::TexelCopyTextureInfo {
                texture: &texture,
                mip_level: mip,
                origin: wgpu::Origin3d::ZERO,
                aspect: wgpu::TextureAspect::All,
            },
            &level,
            wgpu::TexelCopyBufferLayout {
                offset: 0,
                bytes_per_row: Some(w * 4),
                rows_per_image: Some(h),
            },
            wgpu::Extent3d {
                width: w,
                height: h,
                depth_or_array_layers: 1,
            },
        );
        if mip + 1 == mip_count {
            break;
        }
        let (nw, nh) = ((w / 2).max(1), (h / 2).max(1));
        let mut next = vec![0u8; (nw * nh * 4) as usize];
        for y in 0..nh {
            for x in 0..nw {
                for c in 0..4 {
                    let mut sum = 0u32;
                    let mut n = 0;
                    for dy in 0..2 {
                        for dx in 0..2 {
                            let sx = (x * 2 + dx).min(w - 1);
                            let sy = (y * 2 + dy).min(h - 1);
                            sum += level[((sy * w + sx) * 4 + c) as usize] as u32;
                            n += 1;
                        }
                    }
                    next[((y * nw + x) * 4 + c) as usize] = (sum / n) as u8;
                }
            }
        }
        level = next;
        w = nw;
        h = nh;
    }
    let view = texture.create_view(&Default::default());
    GpuTexture {
        texture,
        view,
        size: (img.width, img.height),
        bytes: texture_bytes(
            wgpu::TextureFormat::Rgba8UnormSrgb,
            img.width,
            img.height,
            mip_count,
        ),
        generation: next_gen(),
    }
}
