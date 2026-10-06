use super::{DEPTH_FORMAT, MASK_FORMAT, Renderer, puddles};

pub(crate) struct HdrTargets {
    pub(crate) msaa_view: Option<wgpu::TextureView>,
    pub(crate) view: wgpu::TextureView,
    /// The screen mask (`MASK_FORMAT`), multisampled and resolved like the picture.
    pub(crate) mask_msaa: Option<wgpu::TextureView>,
    pub(crate) mask: wgpu::TextureView,
    pub(crate) down: Vec<wgpu::TextureView>,
    pub(crate) up: Vec<wgpu::TextureView>,
    pub(crate) ldr: wgpu::TextureView,
    /// `down[i]` reads the scene (`i == 0`) or `down[i - 1]`; `up[i]` combines `up[i + 1]`
    /// (or the last `down` level) with `down[i]`.
    pub(crate) down_bg: Vec<wgpu::BindGroup>,
    pub(crate) up_bg: Vec<wgpu::BindGroup>,
    pub(crate) meter_bg: wgpu::BindGroup,
    pub(crate) tonemap_bg: [wgpu::BindGroup; 2],
    pub(crate) fxaa_bg: wgpu::BindGroup,
    /// Present classic shading directly without Enhanced grading.
    pub(crate) classic_bg: wgpu::BindGroup,
    /// Allocated only when wet roads need scene reflections in the main view.
    pub(crate) puddles: Option<puddles::Targets>,
}

pub(crate) struct AoTargets {
    size: (u32, u32),
    pub(crate) depth_view: wgpu::TextureView,
    pub(crate) ao_view: wgpu::TextureView,
    pub(crate) blur_view: wgpu::TextureView,
    pub(crate) ssao_bg: wgpu::BindGroup,
    pub(crate) blur_bg: wgpu::BindGroup,
}

const GLOW_LEVELS: usize = 6;

impl Renderer {
    /// Discard targets sized for the old surface.
    pub fn discard_resize_targets(&mut self) {
        self.depth = None;
        self.ao = None;
        self.target_use.clear();
        self.scale_targets.clear();
        self.msaa_targets.clear();
        self.hdr_targets.clear();
        self.glass_picture = None;
        self.glass_snapshot_source = None;
    }

    pub(crate) fn ensure_ao(&mut self, w: u32, h: u32) -> bool {
        if self.ao.as_ref().map(|a| a.size == (w, h)).unwrap_or(false) {
            return false;
        }
        let size = wgpu::Extent3d {
            width: w,
            height: h,
            depth_or_array_layers: 1,
        };
        let depth = self.device.create_texture(&wgpu::TextureDescriptor {
            label: Some("prepass depth"),
            size,
            mip_level_count: 1,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format: DEPTH_FORMAT,
            usage: wgpu::TextureUsages::RENDER_ATTACHMENT
                | wgpu::TextureUsages::TEXTURE_BINDING
                | if self.puddles.is_some() {
                    wgpu::TextureUsages::COPY_SRC
                } else {
                    wgpu::TextureUsages::empty()
                },
            view_formats: &[],
        });
        // Half-resolution AO uses one quarter of the pixels; the blur masks the lower resolution.
        let half = wgpu::Extent3d {
            width: w.div_ceil(2),
            height: h.div_ceil(2),
            depth_or_array_layers: 1,
        };
        let mk = |label: &str| {
            self.device.create_texture(&wgpu::TextureDescriptor {
                label: Some(label),
                size: half,
                mip_level_count: 1,
                sample_count: 1,
                dimension: wgpu::TextureDimension::D2,
                // R stores occlusion; G stores view depth (m) for depth-aware upsampling
                // (see `ao_at` in shaders/scene/scene_base.wgsl).
                format: wgpu::TextureFormat::Rg16Float,
                usage: wgpu::TextureUsages::RENDER_ATTACHMENT
                    | wgpu::TextureUsages::TEXTURE_BINDING,
                view_formats: &[],
            })
        };
        let ao = mk("ssao");
        let blur = mk("ssao blur");
        let depth_view = depth.create_view(&Default::default());
        let ao_view = ao.create_view(&Default::default());
        let blur_view = blur.create_view(&Default::default());
        let bg = |label: &str, tex: &wgpu::TextureView| {
            self.device.create_bind_group(&wgpu::BindGroupDescriptor {
                label: Some(label),
                layout: &self.ao_layout,
                entries: &[
                    wgpu::BindGroupEntry {
                        binding: 0,
                        resource: self.ao_buf.as_entire_binding(),
                    },
                    wgpu::BindGroupEntry {
                        binding: 1,
                        resource: wgpu::BindingResource::TextureView(&depth_view),
                    },
                    wgpu::BindGroupEntry {
                        binding: 2,
                        resource: wgpu::BindingResource::TextureView(tex),
                    },
                ],
            })
        };
        // SSAO does not read an AO texture, but the layout requires one; bind the blur target.
        let ssao_bg = bg("ssao", &blur_view);
        let blur_bg = bg("ssao blur", &ao_view);
        self.ao = Some(AoTargets {
            size: (w, h),
            depth_view,
            ao_view,
            blur_view,
            ssao_bg,
            blur_bg,
        });
        true
    }

    /// Keep target caches bounded during resize bursts: discard sizes unused for 250 ms and
    /// retain at most ten.
    pub(crate) fn evict_targets(&mut self) {
        const STALE: std::time::Duration = std::time::Duration::from_millis(250);
        const KEEP: usize = 10;
        let now = std::time::Instant::now();
        let mut uses: Vec<((u32, u32), std::time::Instant)> =
            self.target_use.iter().map(|(k, t)| (*k, *t)).collect();
        uses.sort_by_key(|(_, t)| std::cmp::Reverse(*t));
        let keep: std::collections::HashSet<(u32, u32)> = uses
            .iter()
            .filter(|(_, t)| now.duration_since(*t) < STALE)
            .take(KEEP)
            .map(|(k, _)| *k)
            .collect();
        self.target_use.retain(|k, _| keep.contains(k));
        self.scale_targets.retain(|k, _| keep.contains(k));
        self.msaa_targets.retain(|k, _| keep.contains(k));
        self.hdr_targets.retain(|k, _| keep.contains(k));
    }

    pub(crate) fn msaa_targets(
        &mut self,
        w: u32,
        h: u32,
    ) -> (wgpu::TextureView, wgpu::TextureView) {
        self.target_use.insert((w, h), std::time::Instant::now());
        if let Some(t) = self.msaa_targets.get(&(w, h)) {
            return t.clone();
        }
        self.evict_targets();
        let size = wgpu::Extent3d {
            width: w,
            height: h,
            depth_or_array_layers: 1,
        };
        let color = self.device.create_texture(&wgpu::TextureDescriptor {
            label: Some("msaa colour"),
            size,
            mip_level_count: 1,
            sample_count: self.options.msaa,
            dimension: wgpu::TextureDimension::D2,
            format: self.format,
            usage: wgpu::TextureUsages::RENDER_ATTACHMENT,
            view_formats: &[],
        });
        let depth = self.device.create_texture(&wgpu::TextureDescriptor {
            label: Some("msaa depth"),
            size,
            mip_level_count: 1,
            sample_count: self.options.msaa,
            dimension: wgpu::TextureDimension::D2,
            format: DEPTH_FORMAT,
            usage: wgpu::TextureUsages::RENDER_ATTACHMENT,
            view_formats: &[],
        });
        let t = (
            color.create_view(&Default::default()),
            depth.create_view(&Default::default()),
        );
        self.msaa_targets.insert((w, h), t.clone());
        t
    }

    pub(crate) fn hdr_targets(&mut self, w: u32, h: u32) -> bool {
        self.target_use.insert((w, h), std::time::Instant::now());
        if self.hdr_targets.contains_key(&(w, h)) {
            return false;
        }
        self.evict_targets();
        let fmt = wgpu::TextureFormat::Rgba16Float;
        let target = |label: &str, tw: u32, th: u32, format: wgpu::TextureFormat, samples: u32| {
            let usage = if samples > 1 {
                wgpu::TextureUsages::RENDER_ATTACHMENT
            } else {
                wgpu::TextureUsages::RENDER_ATTACHMENT
                    | wgpu::TextureUsages::TEXTURE_BINDING
                    | wgpu::TextureUsages::COPY_SRC
            };
            self.device
                .create_texture(&wgpu::TextureDescriptor {
                    label: Some(label),
                    size: wgpu::Extent3d {
                        width: tw.max(1),
                        height: th.max(1),
                        depth_or_array_layers: 1,
                    },
                    mip_level_count: 1,
                    sample_count: samples,
                    dimension: wgpu::TextureDimension::D2,
                    format,
                    usage,
                    view_formats: &[],
                })
                .create_view(&Default::default())
        };
        let msaa_view =
            (self.options.msaa > 1).then(|| target("hdr msaa", w, h, fmt, self.options.msaa));
        let view = target("hdr", w, h, fmt, 1);
        let mask_msaa = (self.options.msaa > 1)
            .then(|| target("screen mask msaa", w, h, MASK_FORMAT, self.options.msaa));
        let mask = target("screen mask", w, h, MASK_FORMAT, 1);
        // Cap the glow pyramid before its levels become too small for useful blur.
        let levels = GLOW_LEVELS
            .min((w.min(h).max(16) as f32).log2() as usize - 3)
            .max(1);
        let down: Vec<wgpu::TextureView> = (1..=levels)
            .map(|k| target("glow down", w >> k, h >> k, fmt, 1))
            .collect();
        let up: Vec<wgpu::TextureView> = (1..=levels)
            .map(|k| target("glow up", w >> k, h >> k, fmt, 1))
            .collect();
        let ldr = target("tone mapped", w, h, wgpu::TextureFormat::Rgba8Unorm, 1);
        let bg = |src: &wgpu::TextureView, base: &wgpu::TextureView, adapt: &wgpu::TextureView| {
            self.device.create_bind_group(&wgpu::BindGroupDescriptor {
                label: Some("post"),
                layout: &self.post_layout,
                entries: &[
                    wgpu::BindGroupEntry {
                        binding: 0,
                        resource: self.post_buf.as_entire_binding(),
                    },
                    wgpu::BindGroupEntry {
                        binding: 1,
                        resource: wgpu::BindingResource::TextureView(src),
                    },
                    wgpu::BindGroupEntry {
                        binding: 2,
                        resource: wgpu::BindingResource::Sampler(&self.post_sampler),
                    },
                    wgpu::BindGroupEntry {
                        binding: 3,
                        resource: wgpu::BindingResource::TextureView(base),
                    },
                    wgpu::BindGroupEntry {
                        binding: 4,
                        resource: wgpu::BindingResource::TextureView(adapt),
                    },
                ],
            })
        };
        let none = &self.white_texture.view;
        // The first glow level uses the screen mask as `t_base`, excluding light from screens.
        let down_bg: Vec<wgpu::BindGroup> = (0..levels)
            .map(|i| {
                bg(
                    if i == 0 { &view } else { &down[i - 1] },
                    if i == 0 { &mask } else { none },
                    none,
                )
            })
            .collect();
        let up_bg: Vec<wgpu::BindGroup> = (0..levels)
            .map(|i| {
                bg(
                    if i + 1 == levels {
                        &down[i]
                    } else {
                        &up[i + 1]
                    },
                    &down[i],
                    none,
                )
            })
            .collect();
        let meter_bg = bg(&down[levels - 1], none, none);
        let tonemap_bg = [
            bg(&view, &up[0], &self.adapt_views[0]),
            bg(&view, &up[0], &self.adapt_views[1]),
        ];
        // FXAA samples the screen mask as `t_base` to leave screens unchanged.
        let fxaa_bg = bg(&ldr, &mask, none);
        let classic_bg = self.picture_group(&view);
        self.hdr_targets.insert(
            (w, h),
            HdrTargets {
                msaa_view,
                view,
                mask_msaa,
                mask,
                down,
                up,
                ldr,
                down_bg,
                up_bg,
                meter_bg,
                tonemap_bg,
                fxaa_bg,
                classic_bg,
                puddles: None,
            },
        );
        true
    }
}
