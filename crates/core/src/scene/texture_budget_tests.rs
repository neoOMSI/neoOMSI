use super::*;

struct Fixture {
    world: World,
    renderer: Renderer,
    scene: Scene,
    root: PathBuf,
}

impl Fixture {
    fn new(name: &str) -> Self {
        let root =
            std::env::temp_dir().join(format!("neoomsi-residency-{name}-{}", std::process::id()));
        std::fs::create_dir_all(&root).unwrap();
        std::fs::write(root.join("global.cfg"), "[name]\nResidency test\n").unwrap();
        let world = World::open(&root, &root.join("global.cfg"), 20261012).unwrap();
        let mut desc = wgpu::InstanceDescriptor::new_without_display_handle();
        desc.backends = wgpu::Backends::NOOP;
        desc.backend_options.noop = wgpu::NoopBackendOptions::enabled();
        let renderer = pollster::block_on(Renderer::new_with(
            &wgpu::Instance::new(desc),
            None,
            Some(wgpu::TextureFormat::Rgba8UnormSrgb),
            ::render::RenderOptions {
                msaa: 1,
                ssao: false,
                no_enhanced: true,
                shadow_size: 512,
                ..Default::default()
            },
        ))
        .unwrap();
        let scene = renderer.new_scene();
        Self {
            world,
            renderer,
            scene,
            root,
        }
    }

    fn scenery(&mut self, path: PathBuf, data: &TextureData, dropped: u32) -> TextureId {
        let id = self.renderer.add_texture_data(&mut self.scene, data);
        self.world.gpu.lock().textures.insert(
            path.clone(),
            super::super::gpu::TexEntry {
                id,
                alpha: data.has_alpha,
                users: 1,
                texels: data.width as u64 * data.height as u64,
                bytes: self.scene.texture_bytes_of(id),
                format: data.format,
                dropped,
            },
        );
        self.world
            .tile_state
            .lock()
            .entry((0, 0))
            .or_default()
            .gpu
            .shared_textures
            .push(path);
        id
    }

    fn check(&mut self, center: DVec3) -> usize {
        self.world
            .update_texture_budget(&self.renderer, &mut self.scene, &[center], true)
    }

    fn apply_one_restore(&mut self) {
        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(10);
        loop {
            assert_eq!(self.check(DVec3::ZERO), 0);
            let swapped = self.world.apply_texture_upgrades(
                &self.renderer,
                &mut self.scene,
                Some(std::time::Instant::now() + std::time::Duration::from_millis(2)),
            );
            assert!(self.world.gpu.lock().wants_restore.is_empty());
            assert!(self.world.upgrades_pending.lock().len() <= 1);
            if swapped > 0 {
                assert_eq!(swapped, 1);
                break;
            }
            assert!(
                std::time::Instant::now() < deadline,
                "restore worker timed out"
            );
            std::thread::sleep(std::time::Duration::from_millis(1));
        }
        assert!(self.world.upgrades_pending.lock().is_empty());
        assert!(self.world.upgrades_done.lock().is_empty());
    }
}

impl Drop for Fixture {
    fn drop(&mut self) {
        let actual = self.root.canonicalize().unwrap();
        let temp = std::env::temp_dir().canonicalize().unwrap();
        assert!(actual.starts_with(&temp) && actual != temp);
        std::fs::remove_dir_all(actual).unwrap();
    }
}

fn mip_data(format: ::texture::PixelFormat, width: u32, height: u32) -> TextureData {
    TextureData {
        width,
        height,
        format,
        levels: (0..::texture::gpu::mip_count(width, height))
            .map(|l| vec![128; format.level_bytes((width >> l).max(1), (height >> l).max(1))])
            .collect(),
        has_alpha: false,
        gpu_mips: false,
    }
}

fn write_restore_fixture(path: &Path) {
    const RESTORE_FIXTURE_ID: &[u8] = b"neoOMSI/neoOMSI:texture-residency:restore-worker:v1";
    // The TGA image ID records provenance without affecting decoded pixels.
    let mut tga = vec![0u8; 18];
    tga[0] = RESTORE_FIXTURE_ID.len() as u8;
    tga[2] = 2;
    tga[12..14].copy_from_slice(&256u16.to_le_bytes());
    tga[14..16].copy_from_slice(&128u16.to_le_bytes());
    tga[16] = 24;
    tga[17] = 0x20;
    tga.extend_from_slice(RESTORE_FIXTURE_ID);
    tga.extend([64u8, 128, 192].repeat(256 * 128));
    std::fs::write(path, tga).unwrap();
}

#[test]
fn total_budget_includes_generated_and_pbr_resources() {
    let mut f = Fixture::new("total");
    let path = PathBuf::from("scenery.dds");
    let data = mip_data(::texture::PixelFormat::Rgba8, 256, 256);
    let scenery = f.scenery(path.clone(), &data, 0);
    let generated = f.renderer.add_blank_texture(&mut f.scene, 512, 256);
    let normal_image = ::texture::Image {
        width: 128,
        height: 128,
        rgba: vec![128; 128 * 128 * 4],
        has_alpha: false,
    };
    f.renderer.add_pbr_maps(
        &mut f.scene,
        scenery,
        &::texture::pbr::PbrImages {
            normal: Some(normal_image),
            orm: None,
            flags: [1.0, 0.0, 0.0, 0.0],
        },
    );
    let normal = f.scene.pbr_maps[&scenery].normal.unwrap();
    let normal_bytes = f.scene.texture_bytes_of(normal);
    f.world
        .set_texture_budget(f.scene.texture_bytes_of(scenery));
    assert_eq!(f.check(DVec3::ZERO), 0);
    assert_eq!(f.check(DVec3::new(-500.0, -500.0, 0.0)), 1);
    assert_eq!(f.world.gpu.lock().textures[&path].dropped, 1);
    assert_eq!(f.scene.texture_bytes_of(generated), 512 * 256 * 4);
    assert_eq!(f.scene.texture_bytes_of(normal), normal_bytes);
}

#[test]
fn budget_boundary_does_not_reduce_or_restore_distant_scenery_repeatedly() {
    let mut f = Fixture::new("boundary");
    let path = PathBuf::from("distant.dds");
    let data = mip_data(::texture::PixelFormat::Rgba8, 256, 256).without_top_levels(1);
    f.scenery(path.clone(), &data, 1);
    f.world
        .set_texture_budget(f.renderer.texture_bytes(&f.scene));
    for _ in 0..3 {
        assert_eq!(f.check(DVec3::new(-500.0, -500.0, 0.0)), 0);
    }
    assert_eq!(f.world.gpu.lock().textures[&path].dropped, 1);
    assert!(f.world.gpu.lock().wants_restore.is_empty());
}

#[test]
fn repeated_budget_checks_queue_each_nearby_restore_once() {
    let mut f = Fixture::new("duplicates");
    let path = PathBuf::from("nearby.dds");
    let data = mip_data(::texture::PixelFormat::Rgba8, 256, 256).without_top_levels(1);
    f.scenery(path.clone(), &data, 1);
    f.world.set_texture_budget(2_000_000);
    assert_eq!(f.check(DVec3::ZERO), 1);
    for _ in 0..3 {
        assert_eq!(f.check(DVec3::ZERO), 0);
        assert_eq!(f.world.gpu.lock().wants_restore, vec![path.clone()]);
    }
}

#[test]
fn queued_and_loading_replacements_reserve_headroom() {
    let mut f = Fixture::new("reservation");
    let paths = [
        PathBuf::from("optional-a.dds"),
        PathBuf::from("optional-b.dds"),
    ];
    let data = mip_data(::texture::PixelFormat::Rgba8, 128, 128).without_top_levels(1);
    for path in &paths {
        f.scenery(path.clone(), &data, 1);
    }
    let growth = f.world.gpu.lock().textures[&paths[0]].estimated_restore_growth();
    let usage = f.renderer.texture_bytes(&f.scene);
    // Headroom fits one restore, but not two.
    f.world
        .set_texture_budget((usage + growth * 3 / 2) * 10 / 9);
    let center = DVec3::new(-200.0, 0.0, 0.0);
    f.world.gpu.lock().wants_restore.push(paths[0].clone());
    assert_eq!(f.check(center), 0);
    assert_eq!(f.world.gpu.lock().wants_restore, vec![paths[0].clone()]);
    f.world.gpu.lock().wants_restore.clear();
    // Pending compression upgrades need the same restore headroom.
    f.world.upgrades_pending.lock().insert(paths[0].clone());
    assert_eq!(f.check(center), 0);
    assert!(f.world.gpu.lock().wants_restore.is_empty());
    f.world.upgrades_pending.lock().clear();
    assert_eq!(f.check(center), 1);
}

#[test]
fn large_nearby_restores_share_headroom_across_budget_checks() {
    let mut f = Fixture::new("near-headroom");
    let data = mip_data(::texture::PixelFormat::Rgba8, 2048, 1024).without_top_levels(1);
    let paths: Vec<_> = (0..3)
        .map(|i| PathBuf::from(format!("near-{i}.dds")))
        .collect();
    for path in &paths {
        f.scenery(path.clone(), &data, 1);
    }
    let growth = f.world.gpu.lock().textures[&paths[0]].estimated_restore_growth();
    let usage = f.renderer.texture_bytes(&f.scene);
    f.world
        .set_texture_budget((usage + growth * 3 / 2) * 10 / 9);
    assert_eq!(f.check(DVec3::ZERO), 1);
    assert_eq!(f.world.gpu.lock().wants_restore.len(), 1);
    assert_eq!(f.check(DVec3::ZERO), 0);
    assert_eq!(f.world.gpu.lock().wants_restore.len(), 1);
    let queued = f.world.gpu.lock().wants_restore.pop().unwrap();
    f.world.upgrades_pending.lock().insert(queued);
    assert_eq!(f.check(DVec3::ZERO), 0);
    assert!(f.world.gpu.lock().wants_restore.is_empty());
}

#[test]
fn nearby_restores_reclaim_distant_mips_before_loading() {
    let mut f = Fixture::new("near-reclaim");
    let data = mip_data(::texture::PixelFormat::Rgba8, 256, 256).without_top_levels(1);
    let paths = [PathBuf::from("near-a.dds"), PathBuf::from("near-b.dds")];
    for path in &paths {
        f.scenery(path.clone(), &data, 1);
    }
    let far_path = PathBuf::from("far.dds");
    let far = f.scenery(
        far_path.clone(),
        &mip_data(::texture::PixelFormat::Rgba8, 512, 512),
        0,
    );
    {
        let mut states = f.world.tile_state.lock();
        states
            .get_mut(&(0, 0))
            .unwrap()
            .gpu
            .shared_textures
            .retain(|p| p != &far_path);
        states
            .entry((3, 0))
            .or_default()
            .gpu
            .shared_textures
            .push(far_path.clone());
    }
    let growth = f.world.gpu.lock().textures[&paths[0]].estimated_restore_growth();
    f.world
        .set_texture_budget(f.renderer.texture_bytes(&f.scene) + growth / 2);
    assert_eq!(f.check(DVec3::ZERO), 3);
    assert_eq!(f.world.gpu.lock().textures[&far_path].dropped, 1);
    assert_eq!(
        f.renderer.texture_levels(&f.scene, far),
        Some((256, 256, 9))
    );
    let gpu = f.world.gpu.lock();
    assert_eq!(gpu.wants_restore.len(), 2);
    let projected = f.renderer.texture_bytes(&f.scene)
        + gpu
            .wants_restore
            .iter()
            .map(|p| gpu.textures[p].estimated_restore_growth())
            .sum::<u64>();
    let limit = f.world.texture_budget_bytes();
    assert!(projected <= limit - limit / 10);
}

#[test]
fn oversubscribed_nearby_quality_recovers_one_worker_at_a_time() {
    let mut f = Fixture::new("near-oversubscribed");
    let mut textures = Vec::new();
    for i in 0..3 {
        let path = f.root.join(format!("near-{i}.tga"));
        write_restore_fixture(&path);
        let data = load_texture_key(&path, true).unwrap();
        let id = f.scenery(path.clone(), &data, 0);
        let full = f.renderer.texture_levels(&f.scene, id).unwrap();
        assert!(f.renderer.drop_top_levels(&mut f.scene, id, 1));
        let mut gpu = f.world.gpu.lock();
        let e = gpu.textures.get_mut(&path).unwrap();
        e.bytes = f.scene.texture_bytes_of(id);
        e.dropped = 1;
        textures.push((path, id, full));
    }
    f.world
        .set_texture_budget(f.renderer.texture_bytes(&f.scene));
    for _ in &textures {
        assert_eq!(f.check(DVec3::ZERO), 1);
        assert_eq!(f.world.gpu.lock().wants_restore.len(), 1);
        f.apply_one_restore();
    }
    for (path, id, full) in textures {
        assert_eq!(f.world.gpu.lock().textures[&path].dropped, 0);
        assert_eq!(f.renderer.texture_levels(&f.scene, id), Some(full));
    }
    assert!(f.renderer.texture_bytes(&f.scene) > f.world.texture_budget_bytes());
    assert_eq!(f.check(DVec3::ZERO), 0);
}

#[test]
fn block_alignment_does_not_starve_nearby_restores() {
    let mut f = Fixture::new("near-block-alignment");
    let near_path = PathBuf::from("near.dds");
    f.scenery(
        near_path.clone(),
        &mip_data(::texture::PixelFormat::Rgba8, 128, 128).without_top_levels(1),
        1,
    );
    let far_path = PathBuf::from("far.dds");
    let far = f.scenery(
        far_path.clone(),
        &mip_data(::texture::PixelFormat::Bc1, 260, 260),
        0,
    );
    {
        let mut states = f.world.tile_state.lock();
        states
            .get_mut(&(0, 0))
            .unwrap()
            .gpu
            .shared_textures
            .retain(|p| p != &far_path);
        states
            .entry((3, 0))
            .or_default()
            .gpu
            .shared_textures
            .push(far_path.clone());
    }
    f.world
        .set_texture_budget(f.renderer.texture_bytes(&f.scene));
    assert_eq!(f.check(DVec3::ZERO), 1);
    assert_eq!(f.world.gpu.lock().wants_restore, vec![near_path]);
    assert_eq!(f.world.gpu.lock().textures[&far_path].dropped, 0);
    assert_eq!(
        f.renderer.texture_levels(&f.scene, far),
        Some((260, 260, 9))
    );
}

#[test]
fn block_alignment_can_allow_one_level_when_two_cannot_be_dropped() {
    let mut f = Fixture::new("one-level-alignment");
    let path = PathBuf::from("far.dds");
    let id = f.scenery(
        path.clone(),
        &mip_data(::texture::PixelFormat::Bc1, 1024, 1032),
        0,
    );
    f.world
        .set_texture_budget(f.renderer.texture_bytes(&f.scene) / 2);
    assert_eq!(f.check(DVec3::new(-1000.0, 0.0, 0.0)), 1);
    assert_eq!(f.world.gpu.lock().textures[&path].dropped, 1);
    assert_eq!(
        f.renderer.texture_levels(&f.scene, id),
        Some((512, 516, 10))
    );
}

#[test]
fn restore_estimate_error_fits_headroom_for_rectangular_and_bc_mip_chains() {
    use ::texture::PixelFormat::{Bc1, Bc2, Bc3, Rgba8};
    let mut underestimated = false;
    for (format, width, height, dropped) in [
        (Rgba8, 256, 128, 1),
        (Rgba8, 257, 131, 1),
        (Rgba8, 1023, 519, 3),
        (Bc1, 256, 128, 1),
        (Bc2, 512, 256, 2),
        (Bc3, 1024, 512, 3),
    ] {
        let whole = mip_data(format, width, height);
        let reduced = whole.clone().without_top_levels(dropped);
        let entry = super::super::gpu::TexEntry {
            id: 0,
            alpha: false,
            users: 1,
            texels: width as u64 * height as u64,
            bytes: reduced.gpu_bytes(),
            format,
            dropped: dropped as u32,
        };
        let actual = whole.gpu_bytes() - reduced.gpu_bytes();
        let estimate = entry.estimated_restore_growth();
        assert!(
            actual <= estimate + whole.gpu_bytes() / 10,
            "{format:?} {width}x{height}"
        );
        underestimated |= actual > estimate;
    }
    assert!(
        underestimated,
        "the estimate must not be documented as a strict upper bound"
    );
}

#[test]
fn nearby_texture_restores_through_the_background_worker() {
    let mut f = Fixture::new("worker");
    let path = f.root.join("restore.tga");
    write_restore_fixture(&path);
    let data = load_texture_key(&path, true).unwrap();
    let id = f.scenery(path.clone(), &data, 0);
    let full = f.renderer.texture_levels(&f.scene, id).unwrap();
    assert!(f.renderer.drop_top_levels(&mut f.scene, id, 1));
    {
        let mut gpu = f.world.gpu.lock();
        let e = gpu.textures.get_mut(&path).unwrap();
        e.bytes = f.scene.texture_bytes_of(id);
        e.dropped = 1;
    }
    f.world.set_texture_budget(2_000_000);
    assert_eq!(f.check(DVec3::ZERO), 1);
    f.apply_one_restore();
    assert_eq!(f.renderer.texture_levels(&f.scene, id), Some(full));
    assert_eq!(f.world.gpu.lock().textures[&path].dropped, 0);
    assert_eq!(f.check(DVec3::ZERO), 0);
}

#[test]
fn driver_budget_reserves_non_texture_allocations_and_headroom() {
    let memory = Some((2_000_000_000, 7_500_000_000));
    assert_eq!(
        cache_texture_limit(6_000_000_000, 1_000_000_000, memory),
        6_000_000_000
    );
    assert_eq!(
        cache_texture_limit(2_000_000_000, 1_000_000_000, memory),
        2_000_000_000
    );
    // 2 GB textures + 3 GB other allocations, against a 4 GB driver budget.
    assert_eq!(
        cache_texture_limit(
            3_000_000_000,
            2_000_000_000,
            Some((5_000_000_000, 4_000_000_000))
        ),
        800_000_000,
    );
    assert_eq!(cache_texture_limit(2_000_000_000, 1, None), 2_000_000_000);
    assert_eq!(
        cache_texture_limit(2_000_000_000, 1, Some((1, 0))),
        2_000_000_000
    );
    assert_eq!(
        cache_texture_limit(2_000_000_000, 0, Some((8_000_000_000, 4_000_000_000))),
        64_000_000
    );
    // User limits below the emergency floor still apply.
    assert_eq!(
        cache_texture_limit(32_000_000, 0, Some((8_000_000_000, 4_000_000_000))),
        32_000_000
    );
}

#[test]
fn budget_releases_unused_sets_but_preserves_active_and_shared_textures() {
    let mut f = Fixture::new("vehicles");
    let (world, renderer, scene) = (&f.world, &f.renderer, &mut f.scene);
    let image = ::texture::Image {
        width: 128,
        height: 128,
        rgba: vec![128; 128 * 128 * 4],
        has_alpha: false,
    };
    let active = renderer.add_texture(scene, &image, false);
    let shared = renderer.add_texture(scene, &image, false);
    let unused = renderer.add_texture(scene, &image, false);
    // PBR maps share their diffuse texture's lifetime.
    renderer.add_pbr_maps(
        scene,
        unused,
        &::texture::pbr::PbrImages {
            normal: Some(image.clone()),
            orm: None,
            flags: [1.0, 0.0, 0.0, 0.0],
        },
    );
    let normal = scene.pbr_maps[&unused].normal.unwrap();
    let a = PathBuf::from("active.dds");
    let b = PathBuf::from("shared.dds");
    let c = PathBuf::from("unused.dds");
    world.vehicle_textures.lock().extend([
        (a.clone(), (active, 1)),
        (b.clone(), (shared, 2)),
        (c.clone(), (unused, 1)),
    ]);
    let set = |users, textures| super::super::vehicles::VehicleSet {
        meshes: Vec::new(),
        dyn_slots: Vec::new(),
        variants: Vec::new(),
        textures,
        mesh_keys: Vec::new(),
        materials: Vec::new(),
        users,
        idle_since: (users == 0).then(std::time::Instant::now),
    };
    let active_key = (PathBuf::from("active.bus"), None);
    let unused_key = (PathBuf::from("unused.bus"), None);
    world.vehicle_gpu.lock().extend([
        (active_key.clone(), set(1, vec![a, b.clone()])),
        (unused_key.clone(), set(0, vec![b.clone(), c])),
    ]);
    let active_bytes = scene.texture_bytes_of(active) + scene.texture_bytes_of(shared);
    world.set_texture_budget(active_bytes);
    assert!(!world.can_precache_vehicle(renderer, scene));
    assert_eq!(world.trim_vehicle_cache(renderer, scene, active_bytes), 1);
    assert!(world.has_vehicle_set(&active_key));
    assert!(!world.has_vehicle_set(&unused_key));
    assert_eq!(renderer.texture_bytes(scene), active_bytes);
    assert_eq!(scene.texture_bytes_of(normal), 0);
    assert_eq!(world.vehicle_textures.lock()[&b].1, 1);
    assert_eq!(renderer.texture_levels(scene, active), Some((128, 128, 1)));
    assert_eq!(world.trim_vehicle_cache(renderer, scene, 1), 0);
    assert_eq!(renderer.texture_bytes(scene), active_bytes);
}
