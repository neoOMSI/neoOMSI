use crate::*;

#[derive(Clone, Copy)]
pub(crate) struct DrawItem {
    pub(crate) pipe: u8,
    pub(crate) mesh: u32,
    pub(crate) range: u32,
    pub(crate) material: u32,
    /// The material's look: draws of materials that look alike are batched together.
    pub(crate) look: u32,
    pub(crate) entry: u32,
}

#[derive(Clone, PartialEq)]
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
    // a batch keeps the material of the one before it when they look alike: no rebinding
    let mut last = (u32::MAX, 0);
    let mut push = |d: &DrawItem, start: u32, list: &Vec<u32>| {
        if last.0 != d.look {
            last = (d.look, d.material);
        }
        let (first, count) = scene.meshes[d.mesh as usize].index_range(d.range);
        out.push(Batch {
            pipe: d.pipe,
            mesh: d.mesh,
            first,
            count,
            material: last.1,
            instances: start..list.len() as u32,
        });
    };
    if sort {
        let bits = |n: u32| u32::BITS - n.leading_zeros();
        let (mut look, mut mesh, mut range) = (0, 0, 0);
        for d in items.iter() {
            (look, mesh, range) = (look | d.look, mesh | d.mesh, range | d.range);
        }
        let mesh_shift = bits(range);
        let look_shift = mesh_shift + bits(mesh);
        let pipe_shift = look_shift + bits(look);
        if pipe_shift + 8 <= 64 {
            // one number a draw sorts twice as fast as comparing the four fields
            let mut keys: Vec<(u64, u32, u32)> = items
                .iter()
                .map(|d| {
                    let key = (d.pipe as u64) << pipe_shift
                        | (d.look as u64) << look_shift
                        | (d.mesh as u64) << mesh_shift
                        | d.range as u64;
                    (key, d.entry, d.material)
                })
                .collect();
            keys.sort_unstable_by_key(|k| k.0);
            let field =
                |key: u64, from: u32, to: u32| ((key >> from) & ((1 << (to - from)) - 1)) as u32;
            let mut k = 0;
            while k < keys.len() {
                let (key, _, material) = keys[k];
                let start = list.len() as u32;
                while k < keys.len() && keys[k].0 == key {
                    list.push(keys[k].1);
                    k += 1;
                }
                let d = DrawItem {
                    pipe: (key >> pipe_shift) as u8,
                    mesh: field(key, mesh_shift, look_shift),
                    range: field(key, 0, mesh_shift),
                    material,
                    look: field(key, look_shift, pipe_shift),
                    entry: 0,
                };
                push(&d, start, list);
            }
            return;
        }
        items.sort_unstable_by_key(|d| (d.pipe, d.look, d.mesh, d.range));
    }
    let mut k = 0;
    while k < items.len() {
        let d = items[k];
        let start = list.len() as u32;
        while k < items.len()
            && (items[k].pipe, items[k].mesh, items[k].range, items[k].look)
                == (d.pipe, d.mesh, d.range, d.look)
        {
            list.push(items[k].entry);
            k += 1;
        }
        push(&d, start, list);
    }
}

/// The material and look a depth-only draw (shadow map, depth prepass) is batched with: an
/// opaque surface writes its depth whatever its texture, so all of them share the first
/// material and batch across materials; an alpha-tested one needs its own texture for the
/// cut-out.
pub(crate) fn depth_only_material(kind: u8, material: MaterialId, look: u32) -> (u32, u32) {
    if kind == 0 {
        (0, 0)
    } else {
        (material as u32, look)
    }
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
    // (meshes in one page share its buffers and materials that look alike share a group:
    // they are bound once for all the draws in a row that use them)
    let mut pipe = u8::MAX;
    let mut buffers: Option<(&wgpu::Buffer, &wgpu::Buffer)> = None;
    let mut group: Option<&wgpu::BindGroup> = None;
    for b in batches {
        if !include(b) {
            continue;
        }
        if b.pipe != pipe {
            pass.set_pipeline(pipeline(b.pipe));
            pipe = b.pipe;
        }
        let m = &scene.meshes[b.mesh as usize];
        if buffers != Some((&m.vertex_buf, &m.index_buf)) {
            pass.set_vertex_buffer(0, Some(m.vertex_buf.slice(..)));
            pass.set_index_buffer(m.index_buf.slice(..), wgpu::IndexFormat::Uint32);
            buffers = Some((&m.vertex_buf, &m.index_buf));
        }
        let g = &scene.materials[b.material as usize].bind_group;
        if group != Some(g) {
            pass.set_bind_group(1, Some(g), &[]);
            group = Some(g);
        }
        let first = m.first_index + b.first;
        pass.draw_indexed(first..first + b.count, m.base_vertex, b.instances.clone());
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

/// Drawn after scenery so the player's windows tint everything behind them (#231).
pub(crate) const OWN_VEHICLE_BUCKET: usize = RenderPhase::COUNT;

/// Origins of the player's vehicle and its trailers.
pub(crate) fn own_vehicle_origins(
    inside: Option<(DVec3, f64, [f32; 6])>,
    trailers: &[(DVec3, f64, [f32; 6])],
) -> Vec<DVec3> {
    let Some(vehicle) = inside else {
        return Vec::new();
    };
    std::iter::once(&vehicle).chain(trailers).map(|v| v.0).collect()
}

/// The main pass's buckets in drawing order, with the phase each is drawn as.
pub(crate) fn main_pass_buckets() -> impl Iterator<Item = (RenderPhase, usize)> {
    RenderPhase::DRAW_ORDER
        .map(|phase| (phase, phase as usize))
        .into_iter()
        .chain([(RenderPhase::Normal, OWN_VEHICLE_BUCKET)])
}

pub(crate) fn draw_bucket(phase: RenderPhase, origin: DVec3, own_vehicle: &[DVec3]) -> usize {
    if own_vehicle.contains(&origin) {
        OWN_VEHICLE_BUCKET
    } else {
        phase as usize
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

/// Height (m) of a steep face that makes a blended ground-phase slot a wall.
pub(crate) const SURFACE_WALL_HEIGHT: f32 = 2.0;

/// Whether a range has walls, such as a building carried by a `[rendertype] surface` object
/// (#444): their depth is written right after its phase, so later ground phases (rails)
/// stay behind them. `walls`: the mesh's `walls`.
pub(crate) fn surface_wall(walls: &[WallRange], transform: Mat4, range: usize) -> bool {
    walls
        .get(range)
        .is_some_and(|w| w.count > 0 && w.height * transform.z_axis.z.abs() > SURFACE_WALL_HEIGHT)
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

/// The depth prepass pipeline (kind 0, by cull) that writes a main pass batch's depth when
/// the main pass blends it with depth writes; `None` for every other batch.
pub(crate) fn blended_depth_pipe(pipe: u8) -> Option<u8> {
    (pipe / 4 == PIPE_BLEND).then_some((pipe >> 1) & 1)
}

/// Whether a vehicle's next blended instance in drawing order follows one of its rain
/// layers and has none itself; `seen` carries its earlier instances. OMSI draws in authored
/// order, so such a part without depth writes (the NLC's door dividers) covers the drops.
pub(crate) fn follows_rain(seen: &mut bool, rain: bool) -> bool {
    let follows = *seen && !rain;
    *seen |= rain;
    follows
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
        && !*NO_CULL.get_or_init(|| ::legacy_config::env::var_os("OMSI_NO_CULL").is_some())
        && glam::Mat3::from_mat4(inst.transform).determinant() > 0.0
}

/// The window's main-pass bundles as last recorded, part by part, with what each part was
/// recorded from (see `record_bundles`).
#[derive(Default)]
pub(crate) struct KeptBundles {
    key: Option<(wgpu::TextureFormat, u32, wgpu::BindGroup)>,
    parts: Vec<(Vec<Batch>, Vec<BatchBinds>, wgpu::RenderBundle)>,
}

/// What a batch's draw binds: its pipeline, its mesh's buffers and where in them it lies,
/// and its material's group (a mesh or material slot can be given to another since, in
/// another place of the same page, or a pipeline made anew).
struct BatchBinds(
    wgpu::RenderPipeline,
    wgpu::Buffer,
    wgpu::Buffer,
    wgpu::BindGroup,
    (i32, u32),
);

#[allow(clippy::type_complexity)]
fn batch_binds<'s>(
    scene: &'s Scene,
    pp: &'s PassPipelines,
    b: &Batch,
) -> (
    &'s wgpu::RenderPipeline,
    &'s wgpu::Buffer,
    &'s wgpu::Buffer,
    &'s wgpu::BindGroup,
    (i32, u32),
) {
    let m = &scene.meshes[b.mesh as usize];
    (
        main_pipeline(pp, b.pipe),
        &m.vertex_buf,
        &m.index_buf,
        &scene.materials[b.material as usize].bind_group,
        (m.base_vertex, m.first_index),
    )
}

/// The main pass's batches as render bundles, recorded on several threads. Checking every
/// draw is the costliest CPU step of a frame; a bundle is checked when it is made, and the
/// pass that runs it only replays it. The bundles keep the batches' order.
///
/// With `kept` (the window's picture) a part whose batches are the same as last frame's and
/// bind the same pipelines, buffers and groups, with the same camera group and targets, is
/// drawn with last frame's bundle again: a bundle holds only which draws to make, and what
/// they read (the instances' places, the draw list) is in buffers written every frame - the
/// picture is the same. A view standing still (the bus at a stop) recorded the same
/// thousands of draws anew every frame.
pub(crate) fn record_bundles(
    device: &wgpu::Device,
    pool: Option<&rayon::ThreadPool>,
    scene: &Scene,
    batches: &[Batch],
    pp: &PassPipelines,
    camera: &wgpu::BindGroup,
    format: wgpu::TextureFormat,
    samples: u32,
    kept: Option<&std::cell::RefCell<KeptBundles>>,
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
    let chunks: Vec<&[Batch]> = if parts == 1 {
        vec![batches]
    } else {
        batches.chunks(batches.len().div_ceil(parts)).collect()
    };
    let key = (format, samples, camera.clone());
    let mut reuse: Vec<Option<wgpu::RenderBundle>> = vec![None; chunks.len()];
    if let Some(k) = kept.map(|k| k.borrow()) {
        if k.key.as_ref() == Some(&key) {
            for (n, chunk) in chunks.iter().enumerate() {
                let Some((was, binds, bundle)) = k.parts.get(n) else {
                    continue;
                };
                let same = was.as_slice() == *chunk
                    && chunk.iter().zip(binds).all(|(b, k)| {
                        let (p, v, i, g, at) = batch_binds(scene, pp, b);
                        *p == k.0 && *v == k.1 && *i == k.2 && *g == k.3 && at == k.4
                    });
                if same {
                    reuse[n] = Some(bundle.clone());
                }
            }
        }
    }
    let part = |n: usize| reuse[n].clone().or_else(|| record(chunks[n]));
    let bundles: Vec<Option<wgpu::RenderBundle>> = if chunks.len() == 1 {
        vec![part(0)]
    } else {
        run_parts(pool, chunks.len(), part)
    };
    if let Some(kept) = kept {
        let mut k = kept.borrow_mut();
        // (not with a part left out: it would stay out frame after frame)
        if bundles.iter().all(Option::is_some) {
            let parts = chunks
                .iter()
                .zip(&bundles)
                .enumerate()
                .map(
                    |(n, (chunk, bundle))| match (reuse[n].is_some(), k.parts.get_mut(n)) {
                        // (unchanged: what it was made of is kept as it is)
                        (true, Some(old)) => (
                            std::mem::take(&mut old.0),
                            std::mem::take(&mut old.1),
                            bundle.clone().expect("recorded"),
                        ),
                        _ => (
                            chunk.to_vec(),
                            chunk
                                .iter()
                                .map(|b| {
                                    let (p, v, i, g, at) = batch_binds(scene, pp, b);
                                    BatchBinds(p.clone(), v.clone(), i.clone(), g.clone(), at)
                                })
                                .collect(),
                            bundle.clone().expect("recorded"),
                        ),
                    },
                )
                .collect();
            *k = KeptBundles {
                key: Some(key),
                parts,
            };
        } else {
            *k = KeptBundles::default();
        }
    }
    bundles.into_iter().flatten().collect()
}
