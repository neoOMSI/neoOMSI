use crate::*;

impl Renderer {
    #[allow(clippy::too_many_arguments)]
    pub(crate) fn render_inner(
        &mut self,
        scene: &mut Scene,
        target: &wgpu::TextureView,
        width: u32,
        height: u32,
        camera: &Camera,
        lighting: &Lighting,
        with_overlays: bool,
        exclude_texture: Option<TextureId>,
        projection: Option<Mat4>,
        second_eye: bool,
    ) {
        if ::legacy_config::env::var("OMSI_FAKE_GPU_ERROR").as_deref() == Ok("lost")
            && with_overlays
            && self.started.elapsed().as_secs_f32() > 3.0
            && self.device_lost().is_none()
        {
            log::error!("the graphics device was lost (test): OMSI_FAKE_GPU_ERROR=lost");
            self.device.destroy();
            *self.device_lost.lock().unwrap_or_else(|e| e.into_inner()) = Some("test".into());
        }
        if self
            .device_lost
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .is_some()
        {
            return;
        }
        if ::legacy_config::env::var("OMSI_FAKE_GPU_ERROR").as_deref() == Ok("frame")
            && self.options.msaa > 1
            && self.started.elapsed().as_secs_f32() > 3.0
        {
            let _ = self.device.create_texture(&wgpu::TextureDescriptor {
                label: Some("invalid"),
                size: wgpu::Extent3d {
                    width: 4,
                    height: 4,
                    depth_or_array_layers: 1,
                },
                mip_level_count: 1,
                sample_count: 3,
                dimension: wgpu::TextureDimension::D2,
                format: self.format,
                usage: wgpu::TextureUsages::RENDER_ATTACHMENT,
                view_formats: &[],
            });
        }
        if self.gpu_error.load(std::sync::atomic::Ordering::Relaxed) {
            self.fall_back_to_single_sample(scene);
        }
        self.collect_gpu_timers();
        let tset: Option<wgpu::QuerySet> = self.gpu_timers[with_overlays as usize]
            .as_ref()
            .filter(|t| !t.waiting && !t.unresolved)
            .map(|t| t.set.clone());
        let mut timed: Vec<&'static str> = Vec::new();
        let mut stage_t = std::time::Instant::now();
        let mut stage = |r: &Renderer, window: &'static str, mirror: &'static str| {
            if r.profiling {
                let now = std::time::Instant::now();
                *r.stats
                    .borrow_mut()
                    .entry(if with_overlays { window } else { mirror })
                    .or_default() += (now - stage_t).as_secs_f64();
                stage_t = now;
            }
        };
        let ro = (camera.position / 100.0).floor() * 100.0;
        self.set_render_origin(scene, ro);
        let (full_w, full_h) = (width, height);
        let (width, height) = if with_overlays {
            self.scene_size(full_w, full_h)
        } else {
            (full_w, full_h)
        };
        let vanilla_fxaa = with_overlays
            && (width, height) == (full_w, full_h)
            && self.options.fxaa
            && self.options.msaa <= 1
            && !(lighting.enhanced
                && self.hdr_pass.is_some()
                && ::legacy_config::env::var_os("OMSI_NO_ENHANCED").is_none())
            && ::legacy_config::env::var_os("OMSI_NO_FXAA").is_none();
        let enhanced_view = lighting.enhanced
            && self.hdr_pass.is_some()
            && ::legacy_config::env::var_os("OMSI_NO_ENHANCED").is_none();
        let glass_on = with_overlays
            && scene.glass_slot.is_some()
            && (lighting.rain > 0.001 || lighting.wetness > 0.02)
            && ::legacy_config::env::var_os("OMSI_NO_GLASS_PICTURE").is_none();
        let scaled =
            (width, height) != (full_w, full_h) || vanilla_fxaa || (glass_on && !enhanced_view);
        let scene_target: Option<(wgpu::TextureView, wgpu::BindGroup)> = if scaled {
            Some(self.scale_target(width, height))
        } else {
            None
        };
        let scene_view: &wgpu::TextureView = scene_target.as_ref().map(|t| &t.0).unwrap_or(target);
        let aspect = self
            .texture_aspect
            .unwrap_or(width as f32 / height.max(1) as f32);
        let cam_rel = (camera.position - ro).as_vec3();
        let xr_view = projection.is_some();
        let lead_view = with_overlays || (xr_view && !second_eye);
        let enhanced_frame = lighting.enhanced
            && self.hdr_pass.is_some()
            && ::legacy_config::env::var_os("OMSI_NO_ENHANCED").is_none()
            && (with_overlays
                || xr_view
                || ::legacy_config::env::var_os("OMSI_MIRROR_ENHANCED").is_some());
        let enhanced = enhanced_frame;
        let puddles_wanted = with_overlays
            && self.puddles.is_some()
            && self.options.reflections
            && lighting.wetness * (1.0 - lighting.snow.clamp(0.0, 1.0)) > 0.05
            && scene.materials.iter().any(|m| m.uniform.params2[2] > 0.0)
            && debug_view() == 0.0
            && ::legacy_config::env::var_os("OMSI_NO_PUDDLE_REFLECTIONS").is_none();
        let reflection_frame = !enhanced && puddles_wanted && self.reflection_pass.is_some();
        let masked_frame = enhanced || reflection_frame;
        let spot_plan = lighting.light_shadows
            && (with_overlays || (xr_view && !second_eye))
            && ::legacy_config::env::var_os("OMSI_NO_LIGHT_SHADOWS").is_none();
        if !lighting.light_shadows {
            *self.spot_state.borrow_mut() = Default::default();
        }
        let grid = self.prepare_lights(scene, cam_rel, enhanced_frame, spot_plan);
        self.prepare_coronas(scene, lighting.night);
        self.prepare_smoke(scene, camera.position);
        let ao_on = with_overlays
            && self.options.ssao
            && self.ssao_pipeline.is_some()
            && ::legacy_config::env::var_os("OMSI_NO_AO").is_none();
        let prepass_on = ao_on || glass_on || (masked_frame && (with_overlays || xr_view));
        if prepass_on && self.ensure_ao(width, height) {
            scene.dirty = true;
            scene.model_buf = None;
            self.hdr_targets.clear();
        }
        if masked_frame {
            self.hdr_targets(width, height);
        }
        if glass_on {
            self.prepare_glass_behind(scene, width, height);
        }
        let dt = {
            let now = std::time::Instant::now();
            let dt = self
                .last_frame
                .map(|t| (now - t).as_secs_f32())
                .unwrap_or(0.0);
            if lead_view {
                self.last_frame = Some(now);
            }
            dt
        };
        stage(self, "setup", "mirror.setup");
        self.prepare(scene);
        stage(self, "prepare", "mirror.prepare");
        let overlays: Vec<(TextureId, [f32; 4])> = if with_overlays {
            scene.overlays.clone()
        } else {
            Vec::new()
        };
        if with_overlays {
            scene.overlay_res.truncate(overlays.len());
            for (k, (tex, r)) in overlays.iter().copied().enumerate() {
                let r = snap_rect(r);
                let ndc = [
                    r[0] / full_w as f32 * 2.0 - 1.0,
                    1.0 - r[1] / full_h as f32 * 2.0,
                    r[2] / full_w as f32 * 2.0 - 1.0,
                    1.0 - r[3] / full_h as f32 * 2.0,
                    scene.premultiplied.contains(&tex) as u8 as f32,
                    0.0,
                    0.0,
                    0.0,
                ];
                if let Some((_, buf, _, last)) = scene.overlay_res.get_mut(k).filter(|o| o.0 == tex)
                {
                    if *last != ndc {
                        self.queue.write_buffer(buf, 0, bytemuck::cast_slice(&ndc));
                        *last = ndc;
                    }
                    continue;
                }
                let buf = buffer_init(
                    &self.device,
                    &self.queue,
                    Some("overlay rect"),
                    bytemuck::cast_slice(&ndc),
                    wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
                );
                let bg = self.device.create_bind_group(&wgpu::BindGroupDescriptor {
                    label: Some("overlay"),
                    layout: &self.overlay_layout,
                    entries: &[
                        wgpu::BindGroupEntry {
                            binding: 0,
                            resource: buf.as_entire_binding(),
                        },
                        wgpu::BindGroupEntry {
                            binding: 1,
                            resource: wgpu::BindingResource::TextureView(&scene.textures[tex].view),
                        },
                        wgpu::BindGroupEntry {
                            binding: 2,
                            resource: wgpu::BindingResource::Sampler(&self.sky_sampler),
                        },
                    ],
                });
                if k < scene.overlay_res.len() {
                    scene.overlay_res[k] = (tex, buf, bg, ndc);
                } else {
                    scene.overlay_res.push((tex, buf, bg, ndc));
                }
            }
        }
        let sun = lighting.sun_dir.normalize_or_zero();
        let shadows = (with_overlays || projection.is_some()) && lighting.casts_sun_shadows();
        let shared_xr_shadows = if second_eye && shadows {
            self.xr_shadow_cache
                .get()
                .filter(|(origin, previous_sun, _, _, _)| {
                    *origin == scene.render_origin && *previous_sun == sun
                })
        } else {
            None
        };
        let draw_shadows = shadows && shared_xr_shadows.is_none();
        let light_matrix = |range: f32| {
            let texel = range * 2.0 / self.options.shadow_size as f32;
            let up = if sun.z.abs() > 0.95 { Vec3::Y } else { Vec3::Z };
            let raw = cam_rel;
            let view0 = glam::camera::rh::view::look_at_mat4(sun * 900.0, Vec3::ZERO, up);
            let ls = view0.transform_point3(raw);
            let snapped = Vec3::new(
                (ls.x / texel).round() * texel,
                (ls.y / texel).round() * texel,
                ls.z,
            );
            let center = view0.inverse().transform_point3(snapped);
            let view = glam::camera::rh::view::look_at_mat4(center + sun * 900.0, center, up);
            let proj = glam::camera::rh::proj::directx::orthographic(
                -range, range, -range, range, 1.0, 2200.0,
            );
            proj * view
        };
        let near_wanted = light_matrix(SHADOW_RANGE);
        let (near_m, near_age, near_origin, near_sun) = self.shadow_near_cache.get();
        let near_jumped =
            (near_m.project_point3(cam_rel) - near_wanted.project_point3(cam_rel)).length() > 0.03;
        let redraw_near = draw_shadows
            && (near_age >= 1
                || near_jumped
                || near_m == Mat4::IDENTITY
                || near_origin != scene.render_origin
                || near_sun.dot(sun) < 0.99999
                || ::legacy_config::env::var_os("OMSI_SHADOW_NEAR_EVERY_FRAME").is_some());
        let light_view_proj = if let Some((_, _, near, _, _)) = shared_xr_shadows {
            near
        } else if !shadows {
            near_wanted
        } else if redraw_near {
            self.shadow_near_cache
                .set((near_wanted, 0, scene.render_origin, sun));
            near_wanted
        } else {
            self.shadow_near_cache
                .set((near_m, near_age + 1, near_origin, near_sun));
            near_m
        };
        let light_view_proj_close = shared_xr_shadows
            .map(|(_, _, _, _, close)| close)
            .unwrap_or_else(|| light_matrix(SHADOW_RANGE_CLOSE));
        let far_wanted = light_matrix(SHADOW_RANGE_FAR);
        let (far_m, far_age, far_origin, far_sun) = self.shadow_far_cache.get();
        let far_moved =
            (far_m.project_point3(cam_rel) - far_wanted.project_point3(cam_rel)).length() > 0.12;
        let redraw_far = draw_shadows
            && (far_age >= 3
                || far_moved
                || far_m == Mat4::IDENTITY
                || far_origin != scene.render_origin
                || far_sun.dot(sun) < 0.99999
                || ::legacy_config::env::var_os("OMSI_SHADOW_FAR_EVERY_FRAME").is_some());
        if redraw_far && ::legacy_config::env::var_os("OMSI_DEBUG_SHADOW_FAR").is_some() {
            log::info!(
                "far shadow redrawn: age {far_age} moved {far_moved} origin {} sun {:.6}",
                far_origin != scene.render_origin,
                far_sun.dot(sun)
            );
        }
        let light_view_proj_far = if let Some((_, _, _, far, _)) = shared_xr_shadows {
            far
        } else if !shadows {
            far_wanted
        } else if redraw_far {
            self.shadow_far_cache
                .set((far_wanted, 0, scene.render_origin, sun));
            far_wanted
        } else {
            self.shadow_far_cache
                .set((far_m, far_age + 1, far_origin, far_sun));
            far_m
        };
        if projection.is_some() && !second_eye && shadows {
            self.xr_shadow_cache.set(Some((
                scene.render_origin,
                sun,
                light_view_proj,
                light_view_proj_far,
                light_view_proj_close,
            )));
        }
        let vp_mat = projection
            .map(|p| {
                p * glam::camera::rh::view::look_to_mat4(
                    (camera.position - ro).as_vec3(),
                    camera.forward(),
                    camera.up(),
                )
            })
            .unwrap_or_else(|| camera.view_proj(aspect, ro));
        let sun_clip = vp_mat * (cam_rel + sun * 5000.0).extend(1.0);
        let sun_ndc = if sun_clip.w > 0.0 {
            Vec3::new(sun_clip.x / sun_clip.w, sun_clip.y / sun_clip.w, 1.0)
        } else {
            Vec3::new(9.0, 9.0, 0.0)
        };
        {
            let (lx, ly, side) = self.lm_place.get();
            let v: [f32; 4] = [
                (lx - ro.x) as f32,
                (ly - ro.y) as f32,
                side as f32,
                if side > 0.0 { 1.0 } else { 0.0 },
            ];
            self.queue
                .write_buffer(&self.lm_uniform, 0, bytemuck::cast_slice(&v));
        }
        let (spot_vp, spot_info) = {
            let st = self.spot_state.borrow();
            let mut m = [[[0.0f32; 4]; 4]; SPOT_SLOTS];
            for (k, sl) in st.slots.iter().enumerate() {
                if let Some(p) = sl.drawn {
                    m[k] = spot_view_proj(
                        (p.pos - scene.render_origin).as_vec3(),
                        p.dir,
                        p.fov,
                        SPOT_NEAR,
                        p.far,
                    )
                    .to_cols_array_2d();
                }
            }
            let sz = self.options.shadow_size as f32;
            let tile = self.spot_tile as f32;
            let h = sz + SPOT_ROWS as f32 * tile;
            (m, [tile / sz, tile / h, sz / h, tile])
        };
        let cu = CameraUniform {
            post: [
                if enhanced { 1.0 } else { 0.0 },
                lighting
                    .animation_time
                    .unwrap_or_else(|| self.started.elapsed().as_secs_f32()),
                sun_ndc.x,
                self.options.shadow_size.min(SHADOW_CLOSE_MAX) as f32
                    / self.options.shadow_size.max(1) as f32,
            ],
            view_proj: vp_mat.to_cols_array_2d(),
            cam_pos: cam_rel.extend(1.0).to_array(),
            world_origin: [
                ro.x.rem_euclid(1000.0) as f32,
                ro.y.rem_euclid(1000.0) as f32,
                ro.x.rem_euclid(CLOUD_ORIGIN_PERIOD) as f32,
                ro.y.rem_euclid(CLOUD_ORIGIN_PERIOD) as f32,
            ],
            sun_dir: lighting
                .sun_dir
                .normalize()
                .extend(lighting.sun_intensity)
                .to_array(),
            ambient: (lighting.ambient
                * if enhanced {
                    1.0
                } else {
                    night_scale(lighting.night, lighting.atmosphere_brightness)
                })
            .extend(lighting.snow.clamp(0.0, 1.0))
            .to_array(),
            fog: lighting.fog_color.extend(lighting.fog_density).to_array(),
            sun_color: lighting
                .sun_color
                .extend(lighting.night_maps.unwrap_or(lighting.night))
                .to_array(),
            sky_color: (lighting.secondary
                * if enhanced {
                    1.0
                } else {
                    night_scale(lighting.night, lighting.atmosphere_brightness)
                })
            .extend(if lighting.classic && !enhanced {
                1.0
            } else {
                0.0
            })
            .to_array(),
            light_grid: grid,
            sky: [
                lighting.sun_azimuth,
                lighting.sky_weights[0],
                lighting.sky_weights[1],
                lighting.sky_weights[2],
            ],
            clouds: [
                lighting.cloud_density,
                lighting.cloud_offset[0],
                lighting.cloud_offset[1],
                if ao_on { 1.0 } else { 0.0 },
            ],
            cam_right: camera
                .right()
                .extend(
                    self.env_heading
                        .get()
                        .map(|h| h.to_radians())
                        .unwrap_or(0.0),
                )
                .to_array(),
            cam_up: camera
                .right()
                .cross(camera.forward())
                .normalize_or_zero()
                .extend(if self.env_heading.get().is_some() {
                    1.0
                } else {
                    0.0
                })
                .to_array(),
            light_view_proj: light_view_proj.to_cols_array_2d(),
            light_view_proj_far: light_view_proj_far.to_cols_array_2d(),
            shadow: [
                if shadows { 1.0 } else { 0.0 },
                1.0 / self.options.shadow_size as f32,
                SHADOW_RANGE,
                lighting.wetness.clamp(0.0, 1.0),
            ],
            inside_a: match lighting.inside {
                Some((o, h, _)) => {
                    let r = (o - ro).as_vec3();
                    [r.x, r.y, r.z, (h as f32).to_radians().sin()]
                }
                None => [0.0; 4],
            },
            inside_b: match lighting.inside {
                Some((_, h, bb)) => [
                    (h as f32).to_radians().cos(),
                    bb[0] * 0.5,
                    bb[1] * 0.5,
                    bb[2] * 0.5,
                ],
                None => [1.0, 0.0, 0.0, 0.0],
            },
            inside_c: match lighting.inside {
                Some((_, _, bb)) => [bb[3], bb[4], bb[5], 1.0],
                None => [0.0; 4],
            },
            flags: [
                if lighting.detail { 1.0 } else { 0.0 },
                if enhanced { 1.0 } else { 0.0 },
                if glass_on { -1.0 } else { 0.0 },
                if shadows { SHADOW_RANGE_CLOSE } else { 0.0 },
            ],
            light_view_proj_close: light_view_proj_close.to_cols_array_2d(),
            wind: [
                lighting.glass_wind.x,
                lighting.glass_wind.y,
                lighting.glass_wind.z,
                1.0,
            ],
            spot_vp,
            spot_info,
        };
        self.queue
            .write_buffer(&self.camera_buf, 0, bytemuck::bytes_of(&cu));
        let probe_redraw = enhanced
            && (lead_view || self.sky_state.is_none())
            && self.prepare_enhanced(lighting, cam_rel, ro, dt);
        stage(self, "setup", "mirror.setup");
        let debug_draws = ::legacy_config::env::var_os("OMSI_DEBUG_DRAWS").is_some();
        let debug_cull = ::legacy_config::env::var_os("OMSI_DEBUG_CULL").is_some();
        let mut list: Vec<u32> = Vec::new();
        let mut items: Vec<DrawItem> = Vec::new();
        let mut shadow_batches: [Vec<Batch>; SHADOW_SETS] = std::array::from_fn(|_| Vec::new());
        let kind_of = |alpha: AlphaMode| -> u8 {
            match alpha {
                AlphaMode::Opaque => PIPE_OPAQUE,
                AlphaMode::Test => PIPE_ALPHA_TEST,
                AlphaMode::Blend => PIPE_BLEND,
            }
        };
        let lod_fov = camera.fov_deg.to_radians().max(1e-3);
        let lod_size = |inst: &Instance| -> f32 {
            let scale = Self::instance_scale(scene, inst);
            let radius = if inst.object_radius > 0.0 {
                inst.object_radius
            } else {
                scene.meshes[inst.mesh].bounds_radius
            } * scale;
            let d = ((inst.origin - scene.render_origin).as_vec3() - cam_rel).length();
            if d <= radius {
                f32::MAX
            } else {
                2.0 * radius / (d.max(0.01) * lod_fov)
            }
        };
        let spot_draws: Vec<(usize, SpotPose)> = if spot_plan {
            let st = self.spot_state.borrow();
            st.draws
                .iter()
                .filter_map(|&k| st.slots[k].drawn.map(|p| (k, p)))
                .collect()
        } else {
            Vec::new()
        };
        let spot_cull: Vec<(usize, Vec3, Vec3, f32, f32)> = spot_draws
            .iter()
            .map(|&(k, p)| {
                (
                    k,
                    (p.pos - scene.render_origin).as_vec3(),
                    p.dir,
                    p.fov * 0.5,
                    p.far,
                )
            })
            .collect();
        let mut active = [false; SHADOW_SETS];
        active[0] = draw_shadows && redraw_near;
        active[1] = draw_shadows && redraw_far;
        active[2] = draw_shadows;
        for &(k, _, _, _, _) in &spot_cull {
            active[3 + k] = true;
        }
        let boxes = [
            (SHADOW_RANGE, light_view_proj, 0.4f32),
            (SHADOW_RANGE_FAR, light_view_proj_far, 6.0),
            (SHADOW_RANGE_CLOSE, light_view_proj_close, 0.1),
        ];
        let dbg_shadow = ::legacy_config::env::var_os("OMSI_DEBUG_SHADOW").is_some();
        let dbg_r: f32 = ::legacy_config::env::var("OMSI_DEBUG_SHADOW")
            .ok()
            .and_then(|v| v.trim().parse().ok())
            .unwrap_or(3.0);
        let casters = |span: std::ops::Range<usize>| -> [Vec<DrawItem>; SHADOW_SETS] {
            let mut out: [Vec<DrawItem>; SHADOW_SETS] = std::array::from_fn(|_| Vec::new());
            let mut ranges: Vec<(u8, u32, u32, usize)> = Vec::new();
            for inst in &scene.instances[span] {
                if !inst.visible
                    || !inst.casts_shadow
                    || (self.options.omsi_shadow_casters && !inst.omsi_caster)
                {
                    continue;
                }
                let m = &scene.meshes[inst.mesh];
                if m.ranges.is_empty() {
                    continue;
                }
                let (c, r) = Self::bounding_sphere(scene, inst);
                if inst.lod.0 > 0.0 || inst.lod.1 < f32::MAX {
                    let size = lod_size(inst);
                    if size < inst.lod.0 || (inst.lod.1 < f32::MAX && size >= inst.lod.1) {
                        if dbg_shadow && r >= dbg_r {
                            log::info!(
                                "shadow: mesh r={r:.1} at {:?} is another LOD than the one shown",
                                inst.origin
                            );
                        }
                        continue;
                    }
                }
                ranges.clear();
                for (ri, (_, _, slot)) in m.ranges.iter().enumerate() {
                    let mat_id = inst.materials.get(*slot as usize).copied().unwrap_or(0);
                    let mat = &scene.materials[mat_id];
                    let mut kind = kind_of(mat.alpha);
                    let cut_body = kind == PIPE_BLEND && mat.transmap.is_some() && !mat.no_z_write;
                    if (kind == PIPE_BLEND && !cut_body) || mat.no_z_check {
                        if dbg_shadow && r >= dbg_r {
                            log::info!(
                                "shadow: mesh r={r:.1} at {:?} slot {slot} is blended, never a caster",
                                inst.origin
                            );
                        }
                        continue;
                    }
                    if cut_body {
                        kind = PIPE_ALPHA_TEST;
                    }
                    ranges.push((kind, ri as u32, *slot, mat_id));
                }
                for (cascade, &(range, lvp, min_radius)) in boxes.iter().enumerate() {
                    if !active[cascade] {
                        continue;
                    }
                    let dbg = dbg_shadow && cascade == 0 && r >= dbg_r;
                    if m.bounds_radius > 0.0 && m.bounds_radius < min_radius {
                        if dbg_shadow && cascade == 0 && m.bounds_radius >= dbg_r {
                            log::info!(
                                "shadow: mesh r={:.1} skipped (ranges {})",
                                m.bounds_radius,
                                m.ranges.len()
                            );
                        }
                        continue;
                    }
                    let lc = lvp.project_point3(c);
                    let rr = r / range;
                    if lc.x.abs() > 1.0 + rr || lc.y.abs() > 1.0 + rr {
                        if dbg {
                            log::info!(
                                "shadow: mesh r={r:.1} outside the light box at ({:.2}, {:.2})",
                                lc.x,
                                lc.y
                            );
                        }
                        continue;
                    }
                    if dbg {
                        log::info!(
                            "shadow: caster r={r:.1} at {:?} slots {:?}",
                            inst.origin,
                            ranges.iter().map(|x| x.0).collect::<Vec<_>>()
                        );
                    }
                    for &(kind, ri, slot, mat_id) in &ranges {
                        out[cascade].push(DrawItem {
                            pipe: kind,
                            mesh: inst.mesh as u32,
                            range: ri,
                            material: depth_only_material(kind, mat_id),
                            entry: inst.base + slot,
                        });
                    }
                }
                for &(k, lpos, ldir, half, far) in &spot_cull {
                    let v = c - lpos;
                    let d = v.length();
                    if r < 0.1 || d - r > far {
                        continue;
                    }
                    if d > r * 1.01 {
                        let ang = (v.dot(ldir) / d).clamp(-1.0, 1.0).acos();
                        if ang > half * 1.45 + (r / d).clamp(0.0, 1.0).asin() {
                            continue;
                        }
                    }
                    for &(kind, ri, slot, mat_id) in &ranges {
                        out[3 + k].push(DrawItem {
                            pipe: kind,
                            mesh: inst.mesh as u32,
                            range: ri,
                            material: depth_only_material(kind, mat_id),
                            entry: inst.base + slot,
                        });
                    }
                }
            }
            out
        };
        if active.iter().any(|a| *a) {
            let n = scene.instances.len();
            let (parts, chunk) = split_parts(self.encoding_pool.as_ref(), n);
            let mut found: [Vec<DrawItem>; SHADOW_SETS] = std::array::from_fn(|_| Vec::new());
            for part in run_parts(self.encoding_pool.as_ref(), parts, |p| {
                casters(p * chunk..((p + 1) * chunk).min(n))
            }) {
                for (a, b) in found.iter_mut().zip(part) {
                    a.extend(b);
                }
            }
            for cascade in 0..SHADOW_SETS {
                if !active[cascade] {
                    continue;
                }
                if debug_draws {
                    log::info!("shadow cascade {cascade}: {} draws", found[cascade].len());
                }
                batch_items(
                    scene,
                    &mut found[cascade],
                    true,
                    &mut list,
                    &mut shadow_batches[cascade],
                );
            }
        }
        if shadows && debug_draws {
            log::info!(
                "shadow passes: {} batches",
                shadow_batches.iter().map(|b| b.len()).sum::<usize>()
            );
        }
        stage(self, "shadow items", "mirror.shadow items");
        let view = glam::camera::rh::view::look_to_mat4(cam_rel, camera.forward(), camera.up());
        let (tan_x, tan_y) = if let Some(p) = projection {
            (
                ((p.z_axis.x - 1.0) / p.x_axis.x)
                    .abs()
                    .max(((p.z_axis.x + 1.0) / p.x_axis.x).abs()),
                ((p.z_axis.y - 1.0) / p.y_axis.y)
                    .abs()
                    .max(((p.z_axis.y + 1.0) / p.y_axis.y).abs()),
            )
        } else {
            let tan_y = (camera.fov_deg.to_radians() * 0.5).tan();
            (tan_y * aspect, tan_y)
        };
        let cos_y = 1.0 / (1.0 + tan_y * tan_y).sqrt();
        let cos_x = 1.0 / (1.0 + tan_x * tan_x).sqrt();
        let fog_far = if enhanced_frame {
            if lighting.fog_density > FOG_MIN_DENSITY {
                let base = lighting
                    .fog_base
                    .or(lighting.inside.map(|v| v.0.z))
                    .unwrap_or(camera.position.z - 2.0);
                let kh = ((camera.position.z - base).max(0.0) / 300.0) as f32;
                let thin = if kh < 1e-3 {
                    1.0
                } else {
                    (1.0 - (-kh).exp()) / kh
                };
                (4.6 / (lighting.fog_density * thin)).min(camera.far)
            } else {
                camera.far
            }
        } else if lighting.fog_density > FOG_MIN_DENSITY {
            (4.6 / lighting.fog_density).min(camera.far)
        } else {
            camera.far
        };
        if let Some(p) = ::legacy_config::env::var("OMSI_DEBUG_CULL").ok().and_then(|v| {
            let f: Vec<f64> = v.split(',').filter_map(|x| x.trim().parse().ok()).collect();
            (f.len() == 3).then(|| (DVec3::new(f[0], f[1], 0.0), f[2]))
        }) {
            for (i, inst) in scene.instances.iter().enumerate() {
                if (inst.origin.truncate() - p.0.truncate()).length() > p.1 || !with_overlays {
                    continue;
                }
                let m = &scene.meshes[inst.mesh];
                let (c, _) = Self::bounding_sphere(scene, inst);
                let v = view.transform_point3(c);
                log::info!(
                    "cull {i}: mesh {} r {:.2} centre {:?} origin {:?} view {:?} visible {} lod {:?} tan ({tan_x:.2}, {tan_y:.2}) fog {fog_far:.0}",
                    inst.mesh,
                    m.bounds_radius,
                    m.bounds_center,
                    inst.origin,
                    v,
                    inst.visible,
                    inst.lod
                );
            }
        }
        let fov_y = camera.fov_deg.to_radians().max(1e-3);
        let max_obj_dist = self.options.max_obj_dist;
        let min_obj_size = lighting.min_obj_size.max(self.options.min_obj_size);
        let main_view = with_overlays || xr_view;
        let mut drawn_before = if main_view {
            std::mem::take(&mut *self.cull_drawn.borrow_mut())
        } else {
            Vec::new()
        };
        let was_drawn = |i: usize| {
            drawn_before
                .get(i / 64)
                .is_some_and(|w| w & (1u64 << (i % 64)) != 0)
        };
        let (mut sizes_before, mut sizes_now) = if main_view {
            let sizes_before = std::mem::take(&mut *self.object_sizes.borrow_mut());
            let mut sizes_now = std::mem::take(&mut *self.object_sizes_scratch.borrow_mut());
            sizes_now.clear();
            (sizes_before, sizes_now)
        } else {
            Default::default()
        };
        let cull_one = |i: usize, sizes: &mut Vec<([u64; 4], f32)>| -> Option<(usize, f32, bool)> {
            let inst = &scene.instances[i];
            let m = &scene.meshes[inst.mesh];
            if m.ranges.is_empty() || !inst.visible || (inst.mirror_only && main_view) {
                return None;
            }
            if let Some([x0, y0, x1, y1]) = inst.near_only {
                let c = camera.position;
                if c.x < x0 || c.x > x1 || c.y < y0 || c.y > y1 {
                    return None;
                }
            }
            if inst.blob && !self.shadow_blobs {
                return None;
            }
            let (c, r) = Self::bounding_sphere(scene, inst);
            let v = view.transform_point3(c);
            let z = -v.z;
            let inside = v.length() <= r;
            if !inside && z + r < camera.near {
                return None;
            }
            if !inside && z - r > fog_far && !(enhanced_frame && (inst.surface || r > 100.0)) {
                return None;
            }
            if !inside && (v.x.abs() > z * tan_x + r / cos_x || v.y.abs() > z * tan_y + r / cos_y) {
                return None;
            }
            let size = if inst.object_radius > 0.0 {
                let scale = Self::instance_scale(scene, inst);
                let radius = inst.object_radius * scale;
                let ov = view.transform_point3((inst.origin - scene.render_origin).as_vec3());
                let (od, oz) = (ov.length(), -ov.z);
                if od <= radius {
                    f32::MAX
                } else {
                    let reach = if was_drawn(i) {
                        max_obj_dist * 1.05
                    } else {
                        max_obj_dist
                    };
                    if !inst.any_distance
                        && max_obj_dist > 0.0
                        && (od > radius + reach || oz - radius > reach)
                    {
                        return None;
                    }
                    let key = [
                        inst.origin.x.to_bits(),
                        inst.origin.y.to_bits(),
                        inst.origin.z.to_bits(),
                        radius.to_bits() as u64,
                    ];
                    let fresh = 2.0 * radius / (od.max(0.01) * fov_y);
                    let size = match sizes_before.get(&key) {
                        Some(&last) if fresh > last * 0.94 && fresh < last * 1.06 => last,
                        _ => fresh,
                    };
                    if main_view {
                        sizes.push((key, size));
                    }
                    size
                }
            } else if m.bounds_radius > 0.0 {
                2.0 * r / (v.length().max(0.01) * fov_y)
            } else {
                f32::MAX
            };
            let near_dbg =
                debug_cull && with_overlays && (inst.origin - camera.position).length() < 150.0;
            let keep = if was_drawn(i) && inst.object_radius <= 0.0 {
                0.85
            } else {
                1.0
            };
            if !inst.surface && size < min_obj_size * inst.detail * keep {
                if near_dbg {
                    log::info!(
                        "cull: instance {i} mesh {} at {:.0} m: size {size:.4} < {:.4} (radius {:.1}, detail {})",
                        inst.mesh,
                        (inst.origin - camera.position).length(),
                        min_obj_size * inst.detail,
                        inst.object_radius,
                        inst.detail
                    );
                }
                return None;
            }
            if (inst.lod.0 > 0.0 || inst.lod.1 < f32::MAX)
                && (size < inst.lod.0 || (inst.lod.1 < f32::MAX && size >= inst.lod.1))
            {
                if near_dbg && size < inst.lod.0 {
                    log::info!(
                        "cull: instance {i} mesh {} at {:.0} m: lod {:.4}..{:.4}, size {size:.4}",
                        inst.mesh,
                        (inst.origin - camera.position).length(),
                        inst.lod.0,
                        inst.lod.1
                    );
                }
                return None;
            }
            Some((i, z, inside))
        };
        let n = scene.instances.len();
        let (parts, chunk) = split_parts(self.encoding_pool.as_ref(), n);
        let (mut visible, mut found): (Vec<(usize, f32, bool)>, Vec<([u64; 4], f32)>) =
            (Vec::new(), Vec::new());
        for (v, sizes) in run_parts(self.encoding_pool.as_ref(), parts, |p| {
            let mut sizes = Vec::new();
            let v: Vec<_> = (p * chunk..((p + 1) * chunk).min(n))
                .filter_map(|i| cull_one(i, &mut sizes))
                .collect();
            (v, sizes)
        }) {
            visible.extend(v);
            found.extend(sizes);
        }
        if main_view {
            sizes_now.extend(found);
            *self.object_sizes.borrow_mut() = sizes_now;
            sizes_before.clear();
            *self.object_sizes_scratch.borrow_mut() = sizes_before;
        }
        if main_view {
            drawn_before.resize(scene.instances.len().div_ceil(64), 0);
            drawn_before.fill(0);
            for &(i, _, _) in &visible {
                drawn_before[i / 64] |= 1u64 << (i % 64);
            }
            *self.cull_drawn.borrow_mut() = drawn_before;
        }
        if with_overlays && ::legacy_config::env::var_os("OMSI_DEBUG_FLICKER").is_some() {
            let drawn: std::collections::HashSet<usize> = visible.iter().map(|v| v.0).collect();
            let mut prev = self.flicker.borrow_mut();
            let mut now: HashMap<usize, bool> = HashMap::new();
            for (i, inst) in scene.instances.iter().enumerate() {
                let m = &scene.meshes[inst.mesh];
                if m.ranges.is_empty() || !inst.visible || inst.surface {
                    continue;
                }
                let (c, r) = Self::bounding_sphere(scene, inst);
                let v = view.transform_point3(c);
                let z = -v.z;
                if v.length() > 150.0
                    || z + r < camera.near
                    || v.x.abs() > z * tan_x + r / cos_x
                    || v.y.abs() > z * tan_y + r / cos_y
                {
                    continue;
                }
                let d = drawn.contains(&i);
                if let Some(&was) = prev.get(&i) {
                    if was != d {
                        let od = (inst.origin - camera.position).length();
                        log::info!(
                            "flicker: instance {i} mesh {} {} at {od:.1} m (view z {z:.1}, r {r:.2}, object r {:.2}, detail {}, lod {:.3}..{:.3})",
                            inst.mesh,
                            if d { "appears" } else { "vanishes" },
                            inst.object_radius,
                            inst.detail,
                            inst.lod.0,
                            inst.lod.1
                        );
                    }
                }
                now.insert(i, d);
            }
            *prev = now;
        }
        stage(self, "cull", "mirror.cull");
        let only_surfaces = ::legacy_config::env::var_os("OMSI_ONLY_SURFACES").is_some();
        let visible: Vec<(usize, f32, bool)> = if only_surfaces {
            visible
                .into_iter()
                .filter(|(i, _, _)| scene.instances[*i].surface)
                .collect()
        } else {
            visible
        };
        if debug_draws {
            let surf = scene.instances.iter().filter(|i| i.surface).count();
            let vis_surf = visible
                .iter()
                .filter(|(i, _, _)| scene.instances[*i].surface)
                .count();
            log::info!(
                "draw: {} instances ({} surface), {} visible ({} surface)",
                scene.instances.len(),
                surf,
                visible.len(),
                vis_surf
            );
        }
        let mut prepass_batches: Vec<Batch> = Vec::new();
        let prepass_job = || -> (Vec<u32>, Vec<Batch>) {
            let mut items: Vec<DrawItem> = Vec::new();
            let mut list: Vec<u32> = Vec::new();
            let mut batches: Vec<Batch> = Vec::new();
            for &(i, _, _) in &visible {
                let inst = &scene.instances[i];
                let cull = culls_back_faces(scene, inst);
                for (ri, (_, _, slot)) in scene.meshes[inst.mesh].ranges.iter().enumerate() {
                    let mat_id = inst.materials.get(*slot as usize).copied().unwrap_or(0);
                    let mat = &scene.materials[mat_id];
                    let kind = kind_of(mat.alpha);
                    if kind == PIPE_BLEND && world_surface_phase(effective_render_phase(inst)) {
                        continue;
                    }
                    if let Some(pre_kind) = depth_prepass_kind(kind, mat, inst.presurface) {
                        items.push(DrawItem {
                            pipe: pre_kind * 2 + cull as u8,
                            mesh: inst.mesh as u32,
                            range: ri as u32,
                            material: depth_only_material(pre_kind, mat_id),
                            entry: inst.base + *slot,
                        });
                    }
                }
            }
            batch_items(scene, &mut items, true, &mut list, &mut batches);
            (list, batches)
        };
        let mut main_batches: Vec<Batch> = Vec::new();
        let mut main_draws = [0usize; 2];
        let has_presurface = visible
            .iter()
            .any(|&(i, _, _)| scene.instances[i].presurface);
        let mut prepass_found: Option<(Vec<u32>, Vec<Batch>)> = None;
        let pool = self.encoding_pool.as_ref();
        in_scope(pool, |scope| {
            if prepass_on {
                let job = &prepass_job;
                let slot = &mut prepass_found;
                scope.spawn(move |_| *slot = Some(job()));
            }
            let mut by_phase: [Vec<(usize, f32, bool)>; RenderPhase::COUNT] =
                std::array::from_fn(|_| Vec::new());
            for &entry in &visible {
                let phase = effective_render_phase(&scene.instances[entry.0]);
                by_phase[phase as usize].push(entry);
            }
            for phase in RenderPhase::DRAW_ORDER {
                if phase == RenderPhase::BeforeNormal {
                    items.clear();
                    for &(i, _, _) in by_phase[..RenderPhase::BeforeNormal as usize]
                        .iter()
                        .flatten()
                    {
                        let inst = &scene.instances[i];
                        for (ri, (_, _, slot)) in scene.meshes[inst.mesh].ranges.iter().enumerate()
                        {
                            let mat_id = inst.materials.get(*slot as usize).copied().unwrap_or(0);
                            let mat = &scene.materials[mat_id];
                            if !surface_depth_coverage(
                                effective_render_phase(inst),
                                mat.alpha,
                                mat.transmap.is_some(),
                                mat.no_z_check,
                            ) || exclude_texture.is_some_and(|t| mat.uses_texture(t))
                            {
                                continue;
                            }
                            items.push(DrawItem {
                                pipe: pipe_code(
                                    PIPE_SURFACE_DEPTH,
                                    culls_back_faces(scene, inst),
                                    instance_depth_bias(inst, mat),
                                ),
                                mesh: inst.mesh as u32,
                                range: ri as u32,
                                material: mat_id as u32,
                                entry: inst.base + *slot,
                            });
                        }
                    }
                    batch_items(scene, &mut items, true, &mut list, &mut main_batches);
                }
                let visible = &by_phase[phase as usize];
                items.clear();
                let mut blended: Vec<usize> = Vec::new();
                for &(i, _, _) in visible {
                    let inst = &scene.instances[i];
                    if inst.ordered {
                        blended.push(i);
                        continue;
                    }
                    let mut has_blend = false;
                    let cull = culls_back_faces(scene, inst);
                    for (ri, (_, _, slot)) in scene.meshes[inst.mesh].ranges.iter().enumerate() {
                        let mat_id = inst.materials.get(*slot as usize).copied().unwrap_or(0);
                        let mat = &scene.materials[mat_id];
                        let kind = kind_of(mat.alpha);
                        if kind == PIPE_BLEND || mat.no_z_check {
                            has_blend = true;
                            continue;
                        }
                        if exclude_texture.is_some_and(|t| mat.uses_texture(t)) {
                            continue;
                        }
                        items.push(DrawItem {
                            pipe: pipe_code(kind, cull, instance_depth_bias(inst, mat)),
                            mesh: inst.mesh as u32,
                            range: ri as u32,
                            material: mat_id as u32,
                            entry: inst.base + *slot,
                        });
                    }
                    if has_blend {
                        blended.push(i);
                    }
                }
                main_draws[0] += items.len();
                batch_items(scene, &mut items, true, &mut list, &mut main_batches);
                let mut holders: Vec<DVec3> = Vec::new();
                for &(i, _, inside) in visible {
                    let inst = &scene.instances[i];
                    if inside && !inst.surface && !holders.contains(&inst.origin) {
                        holders.push(inst.origin);
                    }
                }
                let player = lighting
                    .inside
                    .filter(|v| point_in_vehicle_box(camera.position, v))
                    .map(|v| v.0);
                let near_by_origin = if self.blend_by_origin {
                    HashMap::new()
                } else {
                    nearest_by_origin(blended.iter().filter_map(|&i| {
                        let inst = &scene.instances[i];
                        if inst.surface {
                            return None;
                        }
                        let (c, r) = Self::bounding_sphere(scene, inst);
                        Some((inst.origin, (c - cam_rel).length() - r))
                    }))
                };
                let mut keyed: Vec<(u8, f32, usize)> = blended
                    .iter()
                    .map(|&i| {
                        let inst = &scene.instances[i];
                        let rank = if self.blend_by_origin || inst.surface {
                            0
                        } else if player == Some(inst.origin) {
                            2
                        } else if holders.contains(&inst.origin) {
                            1
                        } else {
                            0
                        };
                        let dist = if let Some(sort_origin) = inst.blend_sort_origin {
                            horizontal_sort_distance(sort_origin, ro, cam_rel)
                        } else if self.blend_by_origin || inst.surface {
                            ((inst.origin - ro).as_vec3() - cam_rel).length()
                        } else {
                            near_by_origin
                                .get(&origin_key(inst.origin))
                                .copied()
                                .unwrap_or(0.0)
                        };
                        (rank, dist, i)
                    })
                    .collect();
                keyed.sort_unstable_by(|a, b| {
                    a.0.cmp(&b.0).then(b.1.total_cmp(&a.1)).then(a.2.cmp(&b.2))
                });
                items.clear();
                for (_, _, i) in keyed {
                    let inst = &scene.instances[i];
                    let cull = culls_back_faces(scene, inst);
                    for (ri, (_, _, slot)) in scene.meshes[inst.mesh].ranges.iter().enumerate() {
                        let mat_id = inst.materials.get(*slot as usize).copied().unwrap_or(0);
                        let mat = &scene.materials[mat_id];
                        if (mat.alpha != AlphaMode::Blend && !mat.no_z_check && !inst.ordered)
                            || exclude_texture.is_some_and(|t| mat.uses_texture(t))
                        {
                            continue;
                        }
                        if mat.alpha == AlphaMode::Blend
                            && inst
                                .slot_alpha
                                .get(*slot as usize)
                                .is_some_and(|a| *a < 1.0 / 512.0)
                        {
                            continue;
                        }
                        let kind = if mat.alpha != AlphaMode::Blend && !mat.no_z_check {
                            kind_of(mat.alpha)
                        } else if mat.no_z_write
                            || mat.no_z_check
                            || (world_surface_phase(inst.render_phase) && !inst.presurface)
                        {
                            PIPE_BLEND_NO_WRITE
                        } else {
                            PIPE_BLEND
                        };
                        items.push(DrawItem {
                            pipe: pipe_code(kind, cull, instance_depth_bias(inst, mat)),
                            mesh: inst.mesh as u32,
                            range: ri as u32,
                            material: mat_id as u32,
                            entry: inst.base + *slot,
                        });
                    }
                }
                main_draws[1] += items.len();
                batch_items(scene, &mut items, false, &mut list, &mut main_batches);
            }
        });
        if let Some((pre_list, mut pre_batches)) = prepass_found {
            let offset = list.len() as u32;
            for b in &mut pre_batches {
                b.instances = b.instances.start + offset..b.instances.end + offset;
            }
            list.extend(pre_list);
            prepass_batches = pre_batches;
        }
        if let Ok(skip) = ::legacy_config::env::var("OMSI_SKIP_PIPE") {
            let skip: Vec<u8> = skip
                .split(',')
                .filter_map(|x| x.trim().parse().ok())
                .collect();
            main_batches.retain(|b| !skip.contains(&(b.pipe / 4)));
        }
        if debug_draws {
            log::info!(
                "  main pass: {} opaque/alpha-tested and {} blended draws in {} batches; prepass {} batches; draw list {} entries",
                main_draws[0],
                main_draws[1],
                main_batches.len(),
                prepass_batches.len(),
                list.len()
            );
        }
        if self.profiling && !with_overlays {
            let mut c = self.counts.borrow_mut();
            *c.entry("mirror pictures").or_default() += 1.0;
            *c.entry("mirror visible instances").or_default() += visible.len() as f64;
        }
        if self.profiling && with_overlays {
            let mut c = self.counts.borrow_mut();
            *c.entry("scene instances").or_default() += scene.instances.len() as f64;
            *c.entry("visible instances").or_default() += visible.len() as f64;
            *c.entry("main draws").or_default() += (main_draws[0] + main_draws[1]) as f64;
            *c.entry("opaque draws").or_default() += main_draws[0] as f64;
            *c.entry("blended draws").or_default() += main_draws[1] as f64;
            *c.entry("main batches").or_default() += main_batches.len() as f64;
            *c.entry("prepass batches").or_default() += prepass_batches.len() as f64;
            *c.entry("shadow batches").or_default() +=
                (shadow_batches[0].len() + shadow_batches[1].len()) as f64;
            let tris = |bs: &[Batch]| {
                bs.iter()
                    .map(|b| b.count as f64 / 3.0 * b.instances.len() as f64)
                    .sum::<f64>()
                    / 1000.0
            };
            *c.entry("ktris main").or_default() += tris(&main_batches);
            *c.entry("ktris prepass").or_default() += tris(&prepass_batches);
            *c.entry("ktris shadow near").or_default() += tris(&shadow_batches[0]);
            *c.entry("ktris shadow far").or_default() += tris(&shadow_batches[1]);
            *c.entry("ktris shadow close").or_default() += tris(&shadow_batches[2]);
        }
        if self.profiling && with_overlays && self.draw_audit_at.elapsed().as_secs() >= 10 {
            self.draw_audit_at = std::time::Instant::now();
            let mut assets: HashMap<&str, (usize, usize, u64)> = HashMap::new();
            for b in &main_batches {
                let source = scene.meshes[b.mesh as usize]
                    .source
                    .as_deref()
                    .unwrap_or("procedural / vehicle");
                let cost = assets.entry(source).or_default();
                cost.0 += 1;
                cost.1 += b.instances.len();
                cost.2 += b.count as u64 / 3 * b.instances.len() as u64;
            }
            let mut assets: Vec<_> = assets.into_iter().collect();
            assets.sort_unstable_by(|a, b| b.1.0.cmp(&a.1.0).then(a.0.cmp(b.0)));
            for (source, (batches, draws, tris)) in assets.iter().take(12) {
                log::info!(
                    "draw audit: {batches} batches, {draws} draws, {tris} triangles: {source}"
                );
            }
            assets.sort_unstable_by(|a, b| b.1.2.cmp(&a.1.2).then(a.0.cmp(b.0)));
            for (source, (batches, draws, tris)) in assets.iter().take(12) {
                log::info!(
                    "triangle audit: {tris} triangles in {draws} draws ({batches} batches): {source}"
                );
            }
        }
        stage(self, "items", "mirror.items");
        let mut rain_batches = Vec::new();
        if glass_on {
            let (rain, main): (Vec<_>, Vec<_>) = main_batches.into_iter().partition(|b| {
                scene.materials[b.material as usize].uniform.emissive[3] > 1.5
            });
            rain_batches = rain;
            main_batches = main;
        }
        self.upload_draw_list(scene, &list);
        stage(self, "upload", "mirror.upload");
        let main_bundles = if ::legacy_config::env::var_os("OMSI_NO_BUNDLES").is_none() {
            let pp = self.main_pass(enhanced, reflection_frame);
            let format = if masked_frame {
                wgpu::TextureFormat::Rgba16Float
            } else {
                self.format
            };
            record_bundles(
                &self.device,
                self.encoding_pool.as_ref(),
                scene,
                &main_batches,
                pp,
                scene.camera_bind_group.as_ref().expect("camera bind group"),
                format,
                self.options.msaa,
            )
        } else {
            Vec::new()
        };
        stage(self, "bundles", "mirror.bundles");
        let mut shadow_encoder =
            self.device
                .create_command_encoder(&wgpu::CommandEncoderDescriptor {
                    label: Some("shadow maps"),
                });
        self.flush_pending_meshes(scene, &mut shadow_encoder);
        let mut prepass_encoder =
            self.device
                .create_command_encoder(&wgpu::CommandEncoderDescriptor {
                    label: Some("depth prepass"),
                });
        let mut encoder = self
            .device
            .create_command_encoder(&wgpu::CommandEncoderDescriptor {
                label: Some("picture"),
            });
        for cascade in [0usize, 1] {
            if !draw_shadows || (cascade == 1 && !redraw_far) {
                continue;
            }
            let view = if cascade == 0 {
                &self.shadow_view
            } else {
                &self.shadow_view_far
            };
            let keep_near = cascade == 0 && !redraw_near;
            let mut pass = shadow_encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                label: Some("shadow"),
                color_attachments: &[],
                depth_stencil_attachment: Some(wgpu::RenderPassDepthStencilAttachment {
                    view,
                    depth_ops: Some(wgpu::Operations {
                        load: if keep_near || cascade == 1 {
                            wgpu::LoadOp::Load
                        } else {
                            wgpu::LoadOp::Clear(1.0)
                        },
                        store: wgpu::StoreOp::Store,
                    }),
                    stencil_ops: None,
                }),
                timestamp_writes: pass_timer(
                    tset.as_ref(),
                    &mut timed,
                    if cascade == 0 {
                        "shadow near"
                    } else {
                        "shadow far"
                    },
                ),
                occlusion_query_set: None,
                multiview_mask: None,
            });
            pass.set_bind_group(0, scene.shadow_bind_group.as_ref().unwrap(), &[]);
            if cascade == 0 {
                let sz = self.options.shadow_size as f32;
                pass.set_viewport(0.0, 0.0, sz, sz, 0.0, 1.0);
                encode_batches(&mut pass, scene, &shadow_batches[0], |pipe| {
                    &self.shadow_pipelines[pipe as usize]
                });
                let csz = self.options.shadow_size.min(SHADOW_CLOSE_MAX) as f32;
                pass.set_viewport(sz, 0.0, csz, csz, 0.0, 1.0);
                if keep_near {
                    pass.set_pipeline(&self.shadow_clear_pipeline);
                    pass.draw(0..3, 0..1);
                    pass.set_bind_group(0, scene.shadow_bind_group.as_ref().unwrap(), &[]);
                }
                encode_batches(&mut pass, scene, &shadow_batches[2], |pipe| {
                    &self.shadow_pipelines[4 + pipe as usize]
                });
            } else {
                let sz = self.options.shadow_size;
                pass.set_viewport(0.0, 0.0, sz as f32, sz as f32, 0.0, 1.0);
                pass.set_scissor_rect(0, 0, sz, sz);
                pass.set_pipeline(&self.shadow_clear_pipeline);
                pass.draw(0..3, 0..1);
                pass.set_bind_group(0, scene.shadow_bind_group.as_ref().unwrap(), &[]);
                encode_batches(&mut pass, scene, &shadow_batches[cascade], |pipe| {
                    &self.shadow_pipelines[cascade * 2 + pipe as usize]
                });
            }
        }
        for &(k, pose) in &spot_draws {
            let Some(bg) = scene.spot_bind_groups.get(k) else {
                continue;
            };
            let mut cu_spot: CameraUniform = bytemuck::Zeroable::zeroed();
            cu_spot.light_view_proj = spot_view_proj(
                (pose.pos - scene.render_origin).as_vec3(),
                pose.dir,
                pose.fov,
                SPOT_NEAR,
                pose.far,
            )
            .to_cols_array_2d();
            self.queue
                .write_buffer(&self.spot_cam_bufs[k], 0, bytemuck::bytes_of(&cu_spot));
            let tile = self.spot_tile;
            let x = (k as u32 % 4) * tile;
            let y = self.options.shadow_size + (k as u32 / 4) * tile;
            let mut pass = shadow_encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                label: Some("spot shadow"),
                color_attachments: &[],
                depth_stencil_attachment: Some(wgpu::RenderPassDepthStencilAttachment {
                    view: &self.shadow_view_far,
                    depth_ops: Some(wgpu::Operations {
                        load: wgpu::LoadOp::Load,
                        store: wgpu::StoreOp::Store,
                    }),
                    stencil_ops: None,
                }),
                timestamp_writes: None,
                occlusion_query_set: None,
                multiview_mask: None,
            });
            pass.set_viewport(x as f32, y as f32, tile as f32, tile as f32, 0.0, 1.0);
            pass.set_scissor_rect(x, y, tile, tile);
            pass.set_pipeline(&self.shadow_clear_pipeline);
            pass.draw(0..3, 0..1);
            pass.set_bind_group(0, bg, &[]);
            encode_batches(&mut pass, scene, &shadow_batches[3 + k], |pipe| {
                &self.shadow_pipelines[pipe as usize]
            });
        }
        if prepass_on {
            let proj = glam::camera::rh::proj::directx::perspective(
                camera.fov_deg.to_radians(),
                aspect,
                camera.far,
                camera.near,
            );
            let u = SsaoUniform {
                inv_proj: proj.inverse().to_cols_array_2d(),
                params: [
                    1.0,
                    1.4,
                    width.div_ceil(2) as f32,
                    height.div_ceil(2) as f32,
                ],
            };
            self.queue
                .write_buffer(&self.ao_buf, 0, bytemuck::bytes_of(&u));
            let ao = self.ao.as_ref().unwrap();
            {
                let mut pass = prepass_encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                    label: Some("depth prepass"),
                    color_attachments: &[],
                    depth_stencil_attachment: Some(wgpu::RenderPassDepthStencilAttachment {
                        view: &ao.depth_view,
                        depth_ops: Some(wgpu::Operations {
                            load: wgpu::LoadOp::Clear(0.0),
                            store: wgpu::StoreOp::Store,
                        }),
                        stencil_ops: None,
                    }),
                    timestamp_writes: pass_timer(tset.as_ref(), &mut timed, "depth prepass"),
                    occlusion_query_set: None,
                    multiview_mask: None,
                });
                pass.set_bind_group(0, scene.camera_bind_group.as_ref().unwrap(), &[]);
                encode_batches(&mut pass, scene, &prepass_batches, |pipe| {
                    &self.prepass_pipelines[pipe as usize]
                });
            }
            for (pipe, bg, target, pass_label) in [
                (&self.ssao_pipeline, &ao.ssao_bg, &ao.ao_view, "ssao"),
                (&self.blur_pipeline, &ao.blur_bg, &ao.blur_view, "ssao blur"),
            ] {
                let Some(pipe) = pipe.as_ref().filter(|_| ao_on) else {
                    break;
                };
                let mut pass = prepass_encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                    label: Some("ssao"),
                    color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                        view: target,
                        depth_slice: None,
                        resolve_target: None,
                        ops: wgpu::Operations {
                            load: wgpu::LoadOp::Clear(wgpu::Color::WHITE),
                            store: wgpu::StoreOp::Store,
                        },
                    })],
                    depth_stencil_attachment: None,
                    timestamp_writes: pass_timer(tset.as_ref(), &mut timed, pass_label),
                    occlusion_query_set: None,
                    multiview_mask: None,
                });
                pass.set_pipeline(pipe);
                pass.set_bind_group(0, bg, &[]);
                pass.draw(0..3, 0..1);
            }
        }
        if enhanced && (lead_view || probe_redraw) {
            if let (Some(probe), Some(sky_bg)) =
                (self.probe.as_mut(), scene.sky_bind_group.as_ref())
            {
                let full = !probe.cube_filled || self.instant_exposure;
                probe.cube_wait += 1;
                let recapture = std::mem::take(&mut probe.cube_recapture);
                let draws: Vec<(u32, u32, f64)> = if full {
                    (0..6)
                        .flat_map(|f| {
                            (0..SKY_CUBE_ROUNDS).map(move |r| (f, r, r as f64 / (r as f64 + 1.0)))
                        })
                        .collect()
                } else if recapture {
                    let round = (probe.cube_round / 6) % SKY_CUBE_ROUNDS;
                    (0..6).map(|f| (f, round, 0.0)).collect()
                } else if (probe.cube_wait >= SKY_CUBE_EVERY && !redraw_near)
                    || probe.cube_wait >= SKY_CUBE_EVERY * 2
                    || !lead_view
                {
                    vec![(
                        probe.cube_next,
                        (probe.cube_round / 6) % SKY_CUBE_ROUNDS,
                        SKY_CUBE_HISTORY,
                    )]
                } else {
                    Vec::new()
                };
                let single = draws.len() == 1;
                if !draws.is_empty() {
                    probe.cube_wait = 0;
                    probe.cube_next = (probe.cube_next + 1) % 6;
                    probe.cube_round = probe.cube_round.wrapping_add(1);
                }
                probe.cube_filled = true;
                for (f, round, history) in draws {
                    let mut pass = prepass_encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                        label: Some("sky cube"),
                        color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                            view: &probe.cube_faces[f as usize],
                            depth_slice: None,
                            resolve_target: None,
                            ops: wgpu::Operations {
                                load: if history > 0.0 {
                                    wgpu::LoadOp::Load
                                } else {
                                    wgpu::LoadOp::Clear(wgpu::Color::BLACK)
                                },
                                store: wgpu::StoreOp::Store,
                            },
                        })],
                        depth_stencil_attachment: None,
                        timestamp_writes: if single {
                            pass_timer(tset.as_ref(), &mut timed, "sky cube")
                        } else {
                            None
                        },
                        occlusion_query_set: None,
                        multiview_mask: None,
                    });
                    pass.set_pipeline(&probe.cube_pipeline);
                    pass.set_blend_constant(wgpu::Color {
                        r: history,
                        g: history,
                        b: history,
                        a: history,
                    });
                    pass.set_bind_group(
                        0,
                        &probe.cube_bind_groups[(f * SKY_CUBE_ROUNDS + round) as usize],
                        &[],
                    );
                    pass.set_bind_group(1, sky_bg, &[]);
                    pass.draw(0..3, 0..1);
                }
            }
        }
        if probe_redraw {
            if let (Some(probe), Some(sky_bg)) =
                (self.probe.as_ref(), scene.sky_bind_group.as_ref())
            {
                for (m, faces) in probe.faces.iter().enumerate() {
                    for (half, bg) in probe.bind_groups[m].iter().enumerate() {
                        let attachments: Vec<Option<wgpu::RenderPassColorAttachment>> = faces
                            [half * 3..half * 3 + 3]
                            .iter()
                            .map(|v| {
                                Some(wgpu::RenderPassColorAttachment {
                                    view: v,
                                    depth_slice: None,
                                    resolve_target: None,
                                    ops: wgpu::Operations {
                                        load: wgpu::LoadOp::Clear(wgpu::Color::BLACK),
                                        store: wgpu::StoreOp::Store,
                                    },
                                })
                            })
                            .collect();
                        let mut pass =
                            prepass_encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                                label: Some("reflection probe"),
                                color_attachments: &attachments,
                                depth_stencil_attachment: None,
                                timestamp_writes: if m == 0 && half == 0 {
                                    pass_timer(tset.as_ref(), &mut timed, "probe")
                                } else {
                                    None
                                },
                                occlusion_query_set: None,
                                multiview_mask: None,
                            });
                        pass.set_pipeline(if m == 0 {
                            &probe.sky_pipeline
                        } else {
                            &probe.filter_pipeline
                        });
                        pass.set_bind_group(0, bg, &[]);
                        pass.set_bind_group(1, sky_bg, &[]);
                        pass.draw(0..3, 0..1);
                    }
                }
            }
        }
        let single = self.options.msaa <= 1;
        let share_depth = prepass_on
            && single
            && self.ao.is_some()
            && !has_presurface
            && !puddles_wanted;
        let targets = if share_depth {
            None
        } else {
            Some(self.msaa_targets(width, height))
        };
        let msaa_prepass = enhanced
            && !has_presurface
            && (with_overlays || xr_view)
            && !single
            && prepass_on
            && ::legacy_config::env::var_os("OMSI_NO_MSAA_PREPASS").is_none();
        let parts = if !cfg!(any(target_os = "macos", target_os = "ios"))
            && main_bundles.len() >= 2
            && ::legacy_config::env::var_os("OMSI_NO_MAIN_SPLIT").is_none()
        {
            main_bundles.len().min(2)
        } else {
            1
        };
        let mut lead = (parts > 1).then(|| {
            self.device
                .create_command_encoder(&wgpu::CommandEncoderDescriptor {
                    label: Some("main part"),
                })
        });
        if msaa_prepass {
            if let (Some(pipes), Some(t)) = (self.prepass_msaa_pipelines.as_ref(), targets.as_ref())
            {
                let mut pass = lead.as_mut().unwrap_or(&mut encoder).begin_render_pass(
                    &wgpu::RenderPassDescriptor {
                        label: Some("msaa depth prepass"),
                        color_attachments: &[],
                        depth_stencil_attachment: Some(wgpu::RenderPassDepthStencilAttachment {
                            view: &t.1,
                            depth_ops: Some(wgpu::Operations {
                                load: wgpu::LoadOp::Clear(0.0),
                                store: wgpu::StoreOp::Store,
                            }),
                            stencil_ops: None,
                        }),
                        timestamp_writes: pass_timer(tset.as_ref(), &mut timed, "msaa prepass"),
                        occlusion_query_set: None,
                        multiview_mask: None,
                    },
                );
                pass.set_bind_group(0, scene.camera_bind_group.as_ref().unwrap(), &[]);
                encode_batches_filtered(
                    &mut pass,
                    scene,
                    &prepass_batches,
                    |batch| batch.pipe / 2 != PIPE_ALPHA_TEST,
                    |pipe| &pipes[pipe as usize],
                );
            }
        }
        let msaa_prepass =
            msaa_prepass && self.prepass_msaa_pipelines.is_some() && targets.is_some();
        let mut main_parts: Vec<wgpu::CommandEncoder> = Vec::new();
        {
            let sky = if reflection_frame && lighting.classic {
                let decode = |v: f32| {
                    if v <= 0.04045 {
                        v / 12.92
                    } else {
                        ((v + 0.055) / 1.055).powf(2.4)
                    }
                };
                Vec3::new(
                    decode(lighting.sky_color.x),
                    decode(lighting.sky_color.y),
                    decode(lighting.sky_color.z),
                )
            } else {
                lighting.sky_color
            };
            let msaa_color = targets.as_ref().map(|t| &t.0);
            let depth_view: &wgpu::TextureView = match &targets {
                Some(t) => &t.1,
                None => &self.ao.as_ref().unwrap().depth_view,
            };
            let hdr = if masked_frame {
                self.hdr_targets.get(&(width, height))
            } else {
                None
            };
            let (draw_view, resolve_view): (&wgpu::TextureView, Option<&wgpu::TextureView>) =
                match hdr {
                    Some(h) => match &h.msaa_view {
                        Some(m) => (m, Some(&h.view)),
                        None => (&h.view, None),
                    },
                    None => {
                        if single {
                            (scene_view, None)
                        } else {
                            (msaa_color.expect("multisampled target"), Some(scene_view))
                        }
                    }
                };
            let pp = self.main_pass(enhanced, reflection_frame);
            let mask_attachment = hdr.map(|h| wgpu::RenderPassColorAttachment {
                view: h.mask_msaa.as_ref().unwrap_or(&h.mask),
                depth_slice: None,
                resolve_target: h.mask_msaa.as_ref().map(|_| &h.mask),
                ops: wgpu::Operations {
                    load: if parts > 1 {
                        wgpu::LoadOp::Load
                    } else {
                        wgpu::LoadOp::Clear(wgpu::Color::TRANSPARENT)
                    },
                    store: if h.mask_msaa.is_some() {
                        wgpu::StoreOp::Discard
                    } else {
                        wgpu::StoreOp::Store
                    },
                },
            });
            let per_part = main_bundles.len().div_ceil(parts.max(1));
            let sky_clear = wgpu::LoadOp::Clear(wgpu::Color {
                r: sky.x as f64,
                g: sky.y as f64,
                b: sky.z as f64,
                a: 1.0,
            });
            let depth_first = if share_depth || msaa_prepass {
                wgpu::LoadOp::Load
            } else {
                wgpu::LoadOp::Clear(0.0)
            };
            for g in 0..parts.saturating_sub(1) {
                let first = g == 0;
                let mut part = lead.take().unwrap_or_else(|| {
                    self.device
                        .create_command_encoder(&wgpu::CommandEncoderDescriptor {
                            label: Some("main part"),
                        })
                });
                {
                    let part_colors = [
                        Some(wgpu::RenderPassColorAttachment {
                            view: draw_view,
                            depth_slice: None,
                            resolve_target: None,
                            ops: wgpu::Operations {
                                load: if first { sky_clear } else { wgpu::LoadOp::Load },
                                store: wgpu::StoreOp::Store,
                            },
                        }),
                        hdr.map(|h| wgpu::RenderPassColorAttachment {
                            view: h.mask_msaa.as_ref().unwrap_or(&h.mask),
                            depth_slice: None,
                            resolve_target: None,
                            ops: wgpu::Operations {
                                load: if first {
                                    wgpu::LoadOp::Clear(wgpu::Color::TRANSPARENT)
                                } else {
                                    wgpu::LoadOp::Load
                                },
                                store: wgpu::StoreOp::Store,
                            },
                        }),
                    ];
                    let mut pass = part.begin_render_pass(&wgpu::RenderPassDescriptor {
                        label: Some("main part"),
                        color_attachments: if part_colors[1].is_some() {
                            &part_colors[..]
                        } else {
                            &part_colors[..1]
                        },
                        depth_stencil_attachment: Some(wgpu::RenderPassDepthStencilAttachment {
                            view: depth_view,
                            depth_ops: Some(wgpu::Operations {
                                load: if first {
                                    depth_first
                                } else {
                                    wgpu::LoadOp::Load
                                },
                                store: wgpu::StoreOp::Store,
                            }),
                            stencil_ops: None,
                        }),
                        timestamp_writes: None,
                        occlusion_query_set: None,
                        multiview_mask: None,
                    });
                    pass.set_bind_group(0, scene.camera_bind_group.as_ref().unwrap(), &[]);
                    if first {
                        if let Some(sky) = &scene.sky_bind_group {
                            pass.set_pipeline(&pp.sky_pipeline);
                            pass.set_bind_group(1, sky, &[]);
                            pass.set_vertex_buffer(0, Some(self.sky_mesh.0.slice(..)));
                            pass.set_index_buffer(
                                self.sky_mesh.1.slice(..),
                                wgpu::IndexFormat::Uint32,
                            );
                            pass.draw_indexed(0..self.sky_mesh.2, 0, 0..1);
                        }
                    }
                    pass.execute_bundles(
                        main_bundles[g * per_part..((g + 1) * per_part).min(main_bundles.len())]
                            .iter(),
                    );
                }
                main_parts.push(part);
            }
            let tail = (parts - 1) * per_part;
            let main_attachment = Some(wgpu::RenderPassColorAttachment {
                view: draw_view,
                depth_slice: None,
                resolve_target: resolve_view,
                ops: wgpu::Operations {
                    load: if parts > 1 {
                        wgpu::LoadOp::Load
                    } else {
                        sky_clear
                    },
                    store: if resolve_view.is_none() {
                        wgpu::StoreOp::Store
                    } else {
                        wgpu::StoreOp::Discard
                    },
                },
            });
            let colors = [main_attachment, mask_attachment];
            let colors = if colors[1].is_some() {
                &colors[..]
            } else {
                &colors[..1]
            };
            let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                label: Some("main"),
                color_attachments: colors,
                depth_stencil_attachment: Some(wgpu::RenderPassDepthStencilAttachment {
                    view: depth_view,
                    depth_ops: Some(wgpu::Operations {
                        load: if parts > 1 {
                            wgpu::LoadOp::Load
                        } else {
                            depth_first
                        },
                        store: if share_depth || msaa_prepass || ao_on {
                            wgpu::StoreOp::Store
                        } else {
                            wgpu::StoreOp::Discard
                        },
                    }),
                    stencil_ops: None,
                }),
                timestamp_writes: pass_timer(
                    tset.as_ref(),
                    &mut timed,
                    if with_overlays { "main" } else { "mirror" },
                ),
                occlusion_query_set: None,
                multiview_mask: None,
            });
            pass.set_bind_group(0, scene.camera_bind_group.as_ref().unwrap(), &[]);
            if let Some(sky) = scene.sky_bind_group.as_ref().filter(|_| parts == 1) {
                pass.set_pipeline(&pp.sky_pipeline);
                pass.set_bind_group(1, sky, &[]);
                pass.set_vertex_buffer(0, Some(self.sky_mesh.0.slice(..)));
                pass.set_index_buffer(self.sky_mesh.1.slice(..), wgpu::IndexFormat::Uint32);
                pass.draw_indexed(0..self.sky_mesh.2, 0, 0..1);
            }
            if main_bundles.is_empty() {
                pass.set_bind_group(0, scene.camera_bind_group.as_ref().unwrap(), &[]);
                encode_batches(&mut pass, scene, &main_batches, |pipe| {
                    main_pipeline(pp, pipe)
                });
            } else {
                pass.execute_bundles(main_bundles[tail..].iter());
                pass.set_bind_group(0, scene.camera_bind_group.as_ref().unwrap(), &[]);
            }
            self.encode_particles(&mut pass, scene, &pp.smoke_pipeline, &pp.corona_pipeline);
            if !overlays.is_empty() && !masked_frame && !scaled {
                pass.set_pipeline(&self.overlay_pipeline);
                for (k, _) in overlays.iter().enumerate() {
                    if let Some((_, _, bg, _)) = scene.overlay_res.get(k) {
                        pass.set_bind_group(0, bg, &[]);
                        pass.draw(0..6, 0..1);
                    }
                }
            }
        }
        let puddles_on = puddles_wanted
            && main_batches
                .iter()
                .any(|b| scene.materials[b.material as usize].uniform.params2[2] > 0.0)
            && self.prepare_puddle_reflections(
                width, height, camera, aspect, projection, &cu, lighting,
            );
        if puddles_on {
            self.encode_puddle_reflections(
                &mut encoder,
                width,
                height,
                scene,
                &main_batches,
                &list,
                lighting,
                camera,
                tset.as_ref(),
                &mut timed,
            );
        }
        if glass_on {
            let hdr = masked_frame.then(|| &self.hdr_targets[&(width, height)]);
            let view = hdr.map_or(scene_view, |h| {
                h.puddles.as_ref().filter(|_| puddles_on).map_or(&h.view, |p| &p.view)
            });
            if self.glass_snapshot_source.as_ref().is_none_or(|(source, _)| source != view) {
                self.glass_snapshot_source = Some((view.clone(), self.picture_group(view)));
            }
            let (_, bg) = self.glass_snapshot_source.as_ref().unwrap();
            let behind = self.glass_picture.as_ref().unwrap();
            {
                let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                    label: Some("half resolution scene behind glass"),
                    color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                        view: behind, depth_slice: None, resolve_target: None,
                        ops: wgpu::Operations {
                            load: wgpu::LoadOp::Clear(wgpu::Color::BLACK),
                            store: wgpu::StoreOp::Store,
                        },
                    })],
                    depth_stencil_attachment: None,
                    timestamp_writes: pass_timer(tset.as_ref(), &mut timed, "glass snapshot"),
                    occlusion_query_set: None,
                    multiview_mask: None,
                });
                pass.set_pipeline(&self.glass_snapshot_pipeline);
                pass.set_bind_group(0, bg, &[]);
                pass.draw(0..3, 0..1);
            }
            let colours = [
                Some(wgpu::RenderPassColorAttachment {
                    view, depth_slice: None, resolve_target: None,
                    ops: wgpu::Operations { load: wgpu::LoadOp::Load, store: wgpu::StoreOp::Store },
                }),
                hdr.map(|h| wgpu::RenderPassColorAttachment {
                    view: &h.mask, depth_slice: None, resolve_target: None,
                    ops: wgpu::Operations { load: wgpu::LoadOp::Load, store: wgpu::StoreOp::Store },
                }),
            ];
            let pipes = &self.main_pass(enhanced, reflection_frame).rain_pipelines;
            let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                label: Some("rain on current scene"),
                color_attachments: &colours[..if hdr.is_some() { 2 } else { 1 }],
                depth_stencil_attachment: Some(wgpu::RenderPassDepthStencilAttachment {
                    view: &self.ao.as_ref().unwrap().depth_view,
                    depth_ops: Some(wgpu::Operations { load: wgpu::LoadOp::Load, store: wgpu::StoreOp::Store }),
                    stencil_ops: None,
                }),
                timestamp_writes: None,
                occlusion_query_set: None,
                multiview_mask: None,
            });
            pass.set_bind_group(0, scene.camera_bind_group.as_ref().unwrap(), &[]);
            encode_batches(&mut pass, scene, &rain_batches, |pipe| &pipes[pipe as usize]);
        }
        if reflection_frame {
            let h = &self.hdr_targets[&(width, height)];
            let bg = h
                .puddles
                .as_ref()
                .filter(|_| puddles_on)
                .map_or(&h.classic_bg, |p| &p.classic_bg);
            let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                label: Some("classic reflections present"),
                color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                    view: scene_view,
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
            pass.set_pipeline(&self.copy_pipeline);
            pass.set_bind_group(0, bg, &[]);
            pass.draw(0..3, 0..1);
            if !overlays.is_empty() && !scaled {
                pass.set_pipeline(&self.overlay_pipeline_1x);
                draw_overlays(&mut pass, scene, overlays.len());
            }
        }
        if enhanced {
            let secs = |tau: f32| {
                if dt > 0.0 {
                    1.0 - (-dt / tau).exp()
                } else {
                    1.0
                }
            };
            let m = meter_tuning();
            let pu = PostUniform {
                a: [
                    0.035,
                    m[2],
                    m[3],
                    if self.instant_exposure || dt <= 0.0 {
                        1.0
                    } else {
                        0.0
                    },
                ],
                b: [secs(2.5), secs(0.6), m[1], m[4]],
                c: [
                    m[0],
                    m[5],
                    self.exposure.map(f32::exp).unwrap_or(1.0),
                    lighting.led_glow * 10.0,
                ],
                d: [lighting.nightmap_glow * 4.0, 0.0, 0.0, 0.0],
            };
            self.queue
                .write_buffer(&self.post_buf, 0, bytemuck::bytes_of(&pu));
            let fxaa = with_overlays
                && self.options.fxaa
                && ::legacy_config::env::var_os("OMSI_NO_FXAA").is_none();
            if let Some(h) = self.hdr_targets.get(&(width, height)) {
                let puddles = h.puddles.as_ref().filter(|_| puddles_on);
                let levels = h.down.len();
                for i in 0..levels {
                    post_pass(
                        &mut encoder,
                        &h.down[i],
                        None,
                        if i == 0 {
                            &self.post.down_first
                        } else {
                            &self.post.down
                        },
                        if i == 0 {
                            puddles.map(|p| &p.down_bg).unwrap_or(&h.down_bg[i])
                        } else {
                            &h.down_bg[i]
                        },
                    );
                }
                if lead_view {
                    post_pass(
                        &mut encoder,
                        &self.meter_view,
                        None,
                        &self.post.meter,
                        &h.meter_bg,
                    );
                    let front = self.adapt_front;
                    post_pass(
                        &mut encoder,
                        &self.adapt_views[1 - front],
                        None,
                        &self.post.adapt,
                        &self.adapt_bg[front],
                    );
                    self.adapt_front = 1 - front;
                }
                let lost = self.device_lost().is_some();
                if let Some(log) = self
                    .exposure_log
                    .as_mut()
                    .filter(|_| with_overlays && !lost)
                {
                    let pre = self.exposure.unwrap_or(0.0) / std::f32::consts::LN_2;
                    log.sample(&mut encoder, &self.adapt_views[self.adapt_front], pre, m);
                }
                for i in (0..levels).rev() {
                    let timer = if i == 0 {
                        pass_timer(tset.as_ref(), &mut timed, "glow+meter")
                    } else {
                        None
                    };
                    post_pass(&mut encoder, &h.up[i], timer, &self.post.up, &h.up_bg[i]);
                }
                let final_view = if fxaa { &h.ldr } else { scene_view };
                let tonemap_bg =
                    &puddles.map(|p| &p.tonemap_bg).unwrap_or(&h.tonemap_bg)[self.adapt_front];
                let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                    label: Some("tone map"),
                    color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                        view: final_view,
                        depth_slice: None,
                        resolve_target: None,
                        ops: wgpu::Operations {
                            load: wgpu::LoadOp::Clear(wgpu::Color::BLACK),
                            store: wgpu::StoreOp::Store,
                        },
                    })],
                    depth_stencil_attachment: None,
                    timestamp_writes: pass_timer(tset.as_ref(), &mut timed, "tone map"),
                    occlusion_query_set: None,
                    multiview_mask: None,
                });
                pass.set_pipeline(if fxaa {
                    &self.post.tonemap_encoded
                } else {
                    &self.post.tonemap
                });
                pass.set_bind_group(0, tonemap_bg, &[]);
                pass.draw(0..3, 0..1);
                if fxaa {
                    drop(pass);
                    pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                        label: Some("fxaa"),
                        color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                            view: scene_view,
                            depth_slice: None,
                            resolve_target: None,
                            ops: wgpu::Operations {
                                load: wgpu::LoadOp::Clear(wgpu::Color::BLACK),
                                store: wgpu::StoreOp::Store,
                            },
                        })],
                        depth_stencil_attachment: None,
                        timestamp_writes: pass_timer(tset.as_ref(), &mut timed, "fxaa"),
                        occlusion_query_set: None,
                        multiview_mask: None,
                    });
                    pass.set_pipeline(&self.post.fxaa);
                    pass.set_bind_group(0, &h.fxaa_bg, &[]);
                    pass.draw(0..3, 0..1);
                }
                if !overlays.is_empty() && !scaled {
                    pass.set_pipeline(&self.overlay_pipeline_1x);
                    draw_overlays(&mut pass, scene, overlays.len());
                }
            }
        }
        if let Some((_, bg)) = &scene_target {
            let sharpen = (1.0 - width as f32 / full_w as f32) * 2.0;
            self.queue.write_buffer(
                &self.upscale_buf,
                0,
                bytemuck::cast_slice(&[
                    width as f32,
                    height as f32,
                    sharpen.clamp(0.0, 0.8),
                    if vanilla_fxaa { 1.0 } else { 0.0 },
                ]),
            );
            let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                label: Some("upscale"),
                color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                    view: target,
                    depth_slice: None,
                    resolve_target: None,
                    ops: wgpu::Operations {
                        load: wgpu::LoadOp::Clear(wgpu::Color::BLACK),
                        store: wgpu::StoreOp::Store,
                    },
                })],
                depth_stencil_attachment: None,
                timestamp_writes: pass_timer(tset.as_ref(), &mut timed, "upscale"),
                occlusion_query_set: None,
                multiview_mask: None,
            });
            pass.set_pipeline(&self.upscale_pipeline);
            pass.set_bind_group(0, bg, &[]);
            pass.draw(0..3, 0..1);
            if !overlays.is_empty() {
                pass.set_pipeline(&self.overlay_pipeline_1x);
                draw_overlays(&mut pass, scene, overlays.len());
            }
        }
        stage(self, "encode", "mirror.encode");
        let big = shadow_batches.iter().map(|b| b.len()).sum::<usize>() + prepass_batches.len()
            > 64
            || !main_parts.is_empty();
        let profiling = self.profiling;
        let finish = |encoder: wgpu::CommandEncoder| {
            let start = profiling.then(std::time::Instant::now);
            let commands = encoder.finish();
            (
                commands,
                start.map(|t| t.elapsed().as_secs_f64()).unwrap_or(0.0),
            )
        };
        let (shadow_commands, prepass_commands, part_commands, commands, finish_times) =
            if big && self.encoding_pool.is_some() {
                self.encoding_pool
                    .as_ref()
                    .unwrap()
                    .in_place_scope_fifo(|scope| {
                        let (shadow_tx, shadow_rx) = std::sync::mpsc::sync_channel(1);
                        let (prepass_tx, prepass_rx) = std::sync::mpsc::sync_channel(1);
                        let parts = main_parts.len();
                        let (part_tx, part_rx) = std::sync::mpsc::sync_channel(parts.max(1));
                        let finish = &finish;
                        scope.spawn_fifo(move |_| {
                            let _ = shadow_tx.send(finish(shadow_encoder));
                        });
                        scope.spawn_fifo(move |_| {
                            let _ = prepass_tx.send(finish(prepass_encoder));
                        });
                        for (k, e) in main_parts.into_iter().enumerate() {
                            let tx = part_tx.clone();
                            scope.spawn_fifo(move |_| {
                                let _ = tx.send((k, finish(e).0));
                            });
                        }
                        let (commands, main_secs) = finish(encoder);
                        let wait = std::time::Instant::now();
                        let (shadow_commands, shadow_secs) =
                            shadow_rx.recv().expect("command encoding worker");
                        let shadow_wait = wait.elapsed().as_secs_f64();
                        let wait = std::time::Instant::now();
                        let (prepass_commands, prepass_secs) =
                            prepass_rx.recv().expect("command encoding worker");
                        let mut part_commands: Vec<Option<wgpu::CommandBuffer>> =
                            (0..parts).map(|_| None).collect();
                        for _ in 0..parts {
                            let (k, c) = part_rx.recv().expect("command encoding worker");
                            part_commands[k] = Some(c);
                        }
                        (
                            shadow_commands,
                            prepass_commands,
                            part_commands
                                .into_iter()
                                .map(|c| c.expect("main pass part"))
                                .collect::<Vec<_>>(),
                            commands,
                            [
                                shadow_secs,
                                prepass_secs,
                                main_secs,
                                shadow_wait,
                                wait.elapsed().as_secs_f64(),
                            ],
                        )
                    })
            } else if big {
                std::thread::scope(|scope| {
                    let finish = &finish;
                    let shadow = scope.spawn(move || finish(shadow_encoder));
                    let prepass = scope.spawn(move || finish(prepass_encoder));
                    let parts: Vec<_> = main_parts
                        .into_iter()
                        .map(|e| scope.spawn(move || finish(e).0))
                        .collect();
                    let (commands, main_secs) = finish(encoder);
                    let wait = std::time::Instant::now();
                    let (shadow_commands, shadow_secs) =
                        shadow.join().expect("command encoding thread");
                    let shadow_wait = wait.elapsed().as_secs_f64();
                    let wait = std::time::Instant::now();
                    let (prepass_commands, prepass_secs) =
                        prepass.join().expect("command encoding thread");
                    (
                        shadow_commands,
                        prepass_commands,
                        parts
                            .into_iter()
                            .map(|h| h.join().expect("command encoding thread"))
                            .collect::<Vec<_>>(),
                        commands,
                        [
                            shadow_secs,
                            prepass_secs,
                            main_secs,
                            shadow_wait,
                            wait.elapsed().as_secs_f64(),
                        ],
                    )
                })
            } else {
                let (shadow, shadow_secs) = finish(shadow_encoder);
                let (prepass, prepass_secs) = finish(prepass_encoder);
                let parts: Vec<_> = main_parts.into_iter().map(|e| finish(e).0).collect();
                let (main, main_secs) = finish(encoder);
                (
                    shadow,
                    prepass,
                    parts,
                    main,
                    [shadow_secs, prepass_secs, main_secs, 0.0, 0.0],
                )
            };
        if self.profiling {
            let keys = if with_overlays {
                [
                    "finish.shadow",
                    "finish.prepass",
                    "finish.main",
                    "finish.wait shadow",
                    "finish.wait prepass",
                ]
            } else {
                [
                    "mirror.finish.shadow",
                    "mirror.finish.prepass",
                    "mirror.finish.main",
                    "mirror.finish.wait shadow",
                    "mirror.finish.wait prepass",
                ]
            };
            for (key, secs) in keys.into_iter().zip(finish_times) {
                *self.stats.borrow_mut().entry(key).or_default() += secs;
            }
        }
        stage(self, "finish", "mirror.finish");
        self.queue.submit(
            [shadow_commands, prepass_commands]
                .into_iter()
                .chain(part_commands)
                .chain([commands]),
        );
        stage(self, "submit", "mirror.submit");
        if let (Some(t), false) = (
            self.gpu_timers[with_overlays as usize].as_mut(),
            timed.is_empty(),
        ) {
            t.pending = timed;
            t.unresolved = true;
        }
    }
}
