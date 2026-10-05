use crate::*;

#[derive(Clone, Copy)]
pub(crate) struct DrawItem {
    pub(crate) pipe: u8,
    pub(crate) mesh: u32,
    pub(crate) range: u32,
    pub(crate) material: u32,
    pub(crate) entry: u32,
}

pub(crate) struct Batch {
    pub(crate) pipe: u8,
    pub(crate) mesh: u32,
    pub(crate) first: u32,
    pub(crate) count: u32,
    pub(crate) material: u32,
    pub(crate) instances: std::ops::Range<u32>,
}

pub(crate) fn batch_items(
    scene: &Scene,
    items: &mut [DrawItem],
    sort: bool,
    list: &mut Vec<u32>,
    out: &mut Vec<Batch>,
) {
    if sort {
        items.sort_unstable_by_key(|d| (d.pipe, d.material, d.mesh, d.range));
    }
    let mut k = 0;
    while k < items.len() {
        let d = items[k];
        let start = list.len() as u32;
        while k < items.len()
            && (
                items[k].pipe,
                items[k].mesh,
                items[k].range,
                items[k].material,
            ) == (d.pipe, d.mesh, d.range, d.material)
        {
            list.push(items[k].entry);
            k += 1;
        }
        let (first, count, _) = scene.meshes[d.mesh as usize].ranges[d.range as usize];
        out.push(Batch {
            pipe: d.pipe,
            mesh: d.mesh,
            first,
            count,
            material: d.material,
            instances: start..list.len() as u32,
        });
    }
}

pub(crate) fn depth_only_material(kind: u8, material: MaterialId) -> u32 {
    if kind == 0 { 0 } else { material as u32 }
}

pub(crate) fn encode_batches<'a, E: wgpu::util::RenderEncoder<'a>>(
    pass: &mut E,
    scene: &'a Scene,
    batches: &[Batch],
    pipeline: impl Fn(u8) -> &'a wgpu::RenderPipeline,
) {
    encode_batches_filtered(pass, scene, batches, |_| true, pipeline);
}

pub(crate) fn encode_batches_filtered<'a, E: wgpu::util::RenderEncoder<'a>>(
    pass: &mut E,
    scene: &'a Scene,
    batches: &[Batch],
    include: impl Fn(&Batch) -> bool,
    pipeline: impl Fn(u8) -> &'a wgpu::RenderPipeline,
) {
    let (mut pipe, mut mesh, mut material) = (u8::MAX, u32::MAX, u32::MAX);
    for b in batches {
        if !include(b) {
            continue;
        }
        if b.pipe != pipe {
            pass.set_pipeline(pipeline(b.pipe));
            pipe = b.pipe;
        }
        if b.mesh != mesh {
            let m = &scene.meshes[b.mesh as usize];
            pass.set_vertex_buffer(0, Some(m.vertex_buf.slice(..)));
            pass.set_index_buffer(m.index_buf.slice(..), wgpu::IndexFormat::Uint32);
            mesh = b.mesh;
        }
        if b.material != material {
            pass.set_bind_group(
                1,
                Some(&scene.materials[b.material as usize].bind_group),
                &[],
            );
            material = b.material;
        }
        pass.draw_indexed(b.first..b.first + b.count, 0, b.instances.clone());
    }
}

pub(crate) const PIPE_OPAQUE: u8 = 0;
pub(crate) const PIPE_ALPHA_TEST: u8 = 1;
pub(crate) const PIPE_BLEND: u8 = 2;
pub(crate) const PIPE_BLEND_NO_WRITE: u8 = 3;
pub(crate) const PIPE_SURFACE_DEPTH: u8 = 4;
pub(crate) const PIPE_KINDS: u8 = 5;

pub(crate) fn effective_render_phase(instance: &Instance) -> RenderPhase {
    if instance.presurface {
        RenderPhase::PreSurface
    } else {
        instance.render_phase
    }
}

pub(crate) fn world_surface_phase(phase: RenderPhase) -> bool {
    matches!(
        phase,
        RenderPhase::PreSurface
            | RenderPhase::Surface
            | RenderPhase::Spline
            | RenderPhase::OnSurface
    )
}

pub(crate) fn surface_depth_coverage(
    phase: RenderPhase,
    alpha: AlphaMode,
    transmap: bool,
    no_z_check: bool,
) -> bool {
    world_surface_phase(phase) && alpha == AlphaMode::Blend && !transmap && !no_z_check
}
pub(crate) fn depth_prepass_kind(kind: u8, material: &Material, resurface: bool) -> Option<u8> {
    if material.no_z_check {
        return None;
    }
    if kind < PIPE_BLEND {
        Some(kind)
    } else if resurface && !material.no_z_write {
        Some(PIPE_OPAQUE)
    } else if kind == PIPE_BLEND && material.transmap.is_some() && !material.no_z_write {
        Some(2)
    } else {
        None
    }
}

pub(crate) fn instance_depth_bias(instance: &Instance, material: &Material) -> bool {
    instance.surface_bias || material.z_bias > 0 || (material.no_z_check && !instance.surface)
}

pub(crate) fn surface_instance_code(
    blob: bool,
    ground_layer: bool,
    decal: bool,
    surface: bool,
    surface_bias: bool,
) -> f32 {
    if blob {
        2.0
    } else if ground_layer {
        0.75
    } else if decal {
        if surface_bias { 1.25 } else { 0.9 }
    } else if surface {
        if surface_bias { 1.0 } else { 0.9 }
    } else {
        0.0
    }
}

pub(crate) fn horizontal_sort_distance(
    origin: DVec3,
    render_origin: DVec3,
    camera_relative: Vec3,
) -> f32 {
    let p = (origin - render_origin).as_vec3() - camera_relative;
    glam::Vec2::new(p.x, p.y).length()
}

pub(crate) fn pipe_code(kind: u8, cull: bool, surface: bool) -> u8 {
    debug_assert!(kind < PIPE_KINDS);
    kind * 4 + (cull as u8) * 2 + surface as u8
}

pub(crate) fn main_pipeline(pp: &PassPipelines, pipe: u8) -> &wgpu::RenderPipeline {
    #[cfg(all(feature = "devtools", debug_assertions))]
    if devtools::wireframe() {
        if let Some(w) = pp.wire_pipelines.as_ref() {
            return &w[pipe as usize];
        }
    }
    &pp.pipelines[pipe as usize]
}

pub(crate) fn one_sided_primitive(cull: bool) -> wgpu::PrimitiveState {
    wgpu::PrimitiveState {
        topology: wgpu::PrimitiveTopology::TriangleList,
        cull_mode: cull.then_some(wgpu::Face::Back),
        front_face: wgpu::FrontFace::Cw,
        ..Default::default()
    }
}

pub(crate) fn culls_back_faces(scene: &Scene, inst: &Instance) -> bool {
    static NO_CULL: std::sync::OnceLock<bool> = std::sync::OnceLock::new();
    scene.meshes[inst.mesh].one_sided
        && !*NO_CULL.get_or_init(|| omsi_cfg::env::var_os("OMSI_NO_CULL").is_some())
        && glam::Mat3::from_mat4(inst.transform).determinant() > 0.0
}

pub(crate) fn record_bundles(
    device: &wgpu::Device,
    pool: Option<&rayon::ThreadPool>,
    scene: &Scene,
    batches: &[Batch],
    pp: &PassPipelines,
    camera: &wgpu::BindGroup,
    format: wgpu::TextureFormat,
    samples: u32,
) -> Vec<wgpu::RenderBundle> {
    let color_formats = [Some(format), Some(MASK_FORMAT)];
    let record = |chunk: &[Batch]| -> wgpu::RenderBundle {
        let mut bundle =
            device.create_render_bundle_encoder(&wgpu::RenderBundleEncoderDescriptor {
                label: Some("main pass part"),
                color_formats: &color_formats[..if format == HDR_FORMAT { 2 } else { 1 }],
                depth_stencil: Some(wgpu::RenderBundleDepthStencil {
                    format: DEPTH_FORMAT,
                    depth_read_only: false,
                    stencil_read_only: true,
                }),
                sample_count: samples,
                multiview: None,
            });
        bundle.set_bind_group(0, camera, &[]);
        encode_batches(&mut bundle, scene, chunk, |pipe| main_pipeline(pp, pipe));
        bundle.finish(&wgpu::RenderBundleDescriptor {
            label: Some("main pass part"),
        })
    };
    let record = |chunk: &[Batch]| -> Option<wgpu::RenderBundle> {
        CATCHING.set(true);
        let r = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| record(chunk)));
        CATCHING.set(false);
        match r {
            Ok(b) => Some(b),
            Err(_) => {
                static SAID: std::sync::atomic::AtomicBool =
                    std::sync::atomic::AtomicBool::new(false);
                if !SAID.swap(true, std::sync::atomic::Ordering::Relaxed) {
                    log::error!(
                        "a part of the picture could not be recorded (the graphics card is out of memory?); left out"
                    );
                }
                None
            }
        }
    };
    let max_parts = pool
        .map(|p| (p.current_num_threads() + 1).clamp(4, 8))
        .unwrap_or(4);
    let parts = (batches.len() / 250).clamp(1, max_parts);
    if parts == 1 {
        return record(batches).into_iter().collect();
    }
    let chunks: Vec<&[Batch]> = batches.chunks(batches.len().div_ceil(parts)).collect();
    run_parts(pool, chunks.len(), |k| record(chunks[k]))
        .into_iter()
        .flatten()
        .collect()
}
