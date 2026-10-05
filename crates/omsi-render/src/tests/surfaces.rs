use crate::*;

#[test]
#[ignore = "requires a graphics adapter; renders terrain and foliage lighting"]
fn enhanced_masked_and_uncut_ground_share_lighting() {
    let instance = wgpu::Instance::new(wgpu::InstanceDescriptor::new_without_display_handle());
    let mut renderer = pollster::block_on(Renderer::new_with(
        &instance,
        None,
        Some(wgpu::TextureFormat::Rgba8UnormSrgb),
        RenderOptions {
            msaa: 1,
            ssao: false,
            shadow_size: 1024,
            fxaa: false,
            render_scale: 1.0,
            ..Default::default()
        },
    ))
    .expect("test renderer");
    let mut scene = renderer.new_scene();
    let mut texture = |rgba: [u8; 4]| {
        renderer.add_texture(
            &mut scene,
            &omsi_texture::Image {
                width: 1,
                height: 1,
                rgba: rgba.to_vec(),
                has_alpha: true,
            },
            false,
        )
    };
    let grey = texture([100, 100, 100, 255]);
    let opaque = texture([255; 4]);
    let transparent = texture([100, 100, 100, 0]);
    let masked =
        renderer.add_terrain_material(&mut scene, Some(grey), Some(opaque), None, 1.0, None, 0.0);
    let uncut = renderer.add_terrain_material(&mut scene, Some(grey), None, None, 1.0, None, 0.0);
    let cut = renderer.add_terrain_material(
        &mut scene,
        Some(grey),
        Some(transparent),
        None,
        1.0,
        None,
        0.0,
    );
    let foliage = renderer.add_material(&mut scene, Some(grey), AlphaMode::Test, [1.0; 4], false);
    let cut_foliage = renderer.add_material(
        &mut scene,
        Some(transparent),
        AlphaMode::Test,
        [1.0; 4],
        false,
    );
    let backdrop = renderer.add_material(
        &mut scene,
        None,
        AlphaMode::Opaque,
        [1.0, 0.0, 0.0, 1.0],
        true,
    );
    let mut quad = |left: f32, right: f32, z: f32, material| {
        let mesh = renderer.add_mesh(
            &mut scene,
            &MeshData {
                positions: vec![
                    Vec3::new(left, -5.0, z),
                    Vec3::new(right, -5.0, z),
                    Vec3::new(right, 5.0, z),
                    Vec3::new(left, 5.0, z),
                ],
                normals: vec![Vec3::Z; 4],
                uvs: vec![glam::Vec2::splat(0.5); 4],
                indices: vec![0, 1, 2, 0, 2, 3],
                ranges: vec![(0, 6, 0)],
                one_sided: false,
            },
        );
        renderer.add_instance(
            &mut scene,
            mesh,
            DVec3::ZERO,
            Mat4::IDENTITY,
            vec![material],
        )
    };
    quad(-6.0, 6.0, -1.0, backdrop);
    let ground = quad(-5.0, -0.5, 0.0, masked);
    let mapped = quad(0.5, 5.0, 0.0, uncut);
    scene.instances[ground].render_phase = RenderPhase::Terrain;
    scene.instances[mapped].render_phase = RenderPhase::Spline;
    scene.instances[mapped].surface = true;
    let camera = Camera {
        position: DVec3::new(0.0, -0.105, 6.0),
        yaw: 0.0,
        pitch: -89.0,
        roll: 0.0,
        fov_deg: 90.0,
        near: 0.1,
        far: 100.0,
    };
    let day = Lighting {
        enhanced: true,
        sun_dir: Vec3::Z,
        sun_intensity: 1.0,
        shadows: false,
        detail: false,
        fog_density: 0.0,
        ..Default::default()
    };
    let night = Lighting {
        sun_dir: -Vec3::Z,
        sun_intensity: 0.0,
        night: 1.0,
        ..day.clone()
    };
    let pixel = |rgba: &[u8], x: usize| -> [u8; 3] {
        rgba[(32 * 64 + x) * 4..(32 * 64 + x) * 4 + 3]
            .try_into()
            .unwrap()
    };
    for (name, lighting) in [("sun", &day), ("lamp", &night)] {
        scene.lights = if name == "lamp" {
            vec![PointLight {
                position: DVec3::new(0.0, 0.0, 4.0),
                radius: 20.0,
                core: 10.0,
                intensity: 1.0,
                ..Default::default()
            }]
        } else {
            Vec::new()
        };
        let rgba = renderer
            .render_to_image(&mut scene, 64, 64, &camera, lighting)
            .unwrap();
        let (a, b) = (pixel(&rgba, 16), pixel(&rgba, 47));
        assert!(
            a.iter().all(|v| *v > 10 && *v < 245),
            "lit, unclipped {name}: {a:?}"
        );
        assert!(
            a.iter().zip(b).all(|(a, b)| a.abs_diff(b) <= 2),
            "masked terrain and uncut mapped ground differ under {name}: {a:?} / {b:?}"
        );
    }
    renderer.set_material(&mut scene, ground, 0, cut);
    let rgba = renderer
        .render_to_image(&mut scene, 64, 64, &camera, &night)
        .unwrap();
    let a = pixel(&rgba, 16);
    assert!(
        a[0] > a[1] + 30 && a[0] > a[2] + 30,
        "road cut must reveal red: {a:?}"
    );

    renderer.set_material(&mut scene, ground, 0, masked);
    renderer.set_material(&mut scene, mapped, 0, foliage);
    scene.lights[0].position.z = -4.0;
    let rgba = renderer
        .render_to_image(&mut scene, 64, 64, &camera, &night)
        .unwrap();
    let (a, b) = (pixel(&rgba, 16), pixel(&rgba, 47));
    assert!(
        b[1] > a[1] + 8,
        "foliage retains backlighting: ground {a:?}, foliage {b:?}"
    );
    renderer.set_material(&mut scene, mapped, 0, cut_foliage);
    let rgba = renderer
        .render_to_image(&mut scene, 64, 64, &camera, &night)
        .unwrap();
    let b = pixel(&rgba, 47);
    assert!(
        b[0] > b[1] + 30 && b[0] > b[2] + 30,
        "foliage cutout must reveal red: {b:?}"
    );
}

#[test]
#[ignore = "requires a graphics adapter; renders a transmapped layer beyond its texture"]
fn transmap_reads_the_border_outside_the_texture() {
    let instance = wgpu::Instance::new(wgpu::InstanceDescriptor::new_without_display_handle());
    let mut renderer = pollster::block_on(Renderer::new_with(
        &instance,
        None,
        Some(wgpu::TextureFormat::Rgba8UnormSrgb),
        RenderOptions {
            msaa: 1,
            ssao: false,
            shadow_size: 1024,
            fxaa: false,
            render_scale: 1.0,
            ..Default::default()
        },
    ))
    .expect("test renderer");
    let mut scene = renderer.new_scene();
    let mut texture = |rgba: [u8; 4]| {
        renderer.add_texture(
            &mut scene,
            &omsi_texture::Image {
                width: 1,
                height: 1,
                rgba: rgba.to_vec(),
                has_alpha: true,
            },
            false,
        )
    };
    let blue = texture([0, 0, 255, 255]);
    let opaque = texture([255; 4]);
    let backdrop = renderer.add_material(
        &mut scene,
        None,
        AlphaMode::Opaque,
        [1.0, 0.0, 0.0, 1.0],
        true,
    );
    // the ICU400's screen: a transmap whose edge texels are opaque, border alpha 0
    renderer.address_next.set(TexAddressing::Clamp);
    let screen = renderer.add_material_extra(
        &mut scene,
        Some(blue),
        AlphaMode::Blend,
        [1.0; 4],
        true,
        Some((opaque, true)),
        None,
        None,
        None,
        [0.0; 3],
        MaterialExtra {
            border: Some([16.0 / 255.0, 81.0 / 255.0, 115.0 / 255.0, 0.0]),
            transmap_declared: true,
            ..Default::default()
        },
    );
    let mut quad = |z: f32, u: [f32; 2], material| {
        let mesh = renderer.add_mesh(
            &mut scene,
            &MeshData {
                positions: vec![
                    Vec3::new(-5.0, -5.0, z),
                    Vec3::new(5.0, -5.0, z),
                    Vec3::new(5.0, 5.0, z),
                    Vec3::new(-5.0, 5.0, z),
                ],
                normals: vec![Vec3::Z; 4],
                uvs: vec![
                    glam::Vec2::new(u[0], 1.0),
                    glam::Vec2::new(u[1], 1.0),
                    glam::Vec2::new(u[1], 0.0),
                    glam::Vec2::new(u[0], 0.0),
                ],
                indices: vec![0, 1, 2, 0, 2, 3],
                ranges: vec![(0, 6, 0)],
                one_sided: false,
            },
        );
        renderer.add_instance(
            &mut scene,
            mesh,
            DVec3::ZERO,
            Mat4::IDENTITY,
            vec![material],
        )
    };
    quad(-1.0, [0.0, 1.0], backdrop);
    // u runs from -1 to 2: only the middle third lies on the texture
    quad(0.0, [-1.0, 2.0], screen);
    let camera = Camera {
        position: DVec3::new(0.0, -0.105, 6.0),
        yaw: 0.0,
        pitch: -89.0,
        roll: 0.0,
        fov_deg: 90.0,
        near: 0.1,
        far: 100.0,
    };
    let pixel = |rgba: &[u8], x: usize| -> [u8; 3] {
        rgba[(32 * 64 + x) * 4..(32 * 64 + x) * 4 + 3]
            .try_into()
            .unwrap()
    };
    for enhanced in [false, true] {
        let lighting = Lighting {
            enhanced,
            shadows: false,
            fog_density: 0.0,
            ..Default::default()
        };
        let rgba = renderer
            .render_to_image(&mut scene, 64, 64, &camera, &lighting)
            .unwrap();
        let (outside, inside) = (pixel(&rgba, 10), pixel(&rgba, 32));
        assert!(
            outside[0] > outside[1] + 60 && outside[0] > outside[2] + 60,
            "beyond the texture the border's alpha 0 must show the backdrop (enhanced {enhanced}): {outside:?}"
        );
        assert!(
            inside[2] > inside[0] + 60,
            "on the texture the opaque transmap must show the layer (enhanced {enhanced}): {inside:?}"
        );
    }
}

#[test]
#[ignore = "requires a graphics adapter; run with --ignored on a GPU host"]
fn presurface_reveals_excavation_before_terrain_is_drawn() {
    fn quad(renderer: &Renderer, scene: &mut Scene, y: f32, half: f32) -> MeshId {
        renderer.add_mesh(
            scene,
            &MeshData {
                positions: vec![
                    Vec3::new(-half, y, -half),
                    Vec3::new(half, y, -half),
                    Vec3::new(half, y, half),
                    Vec3::new(-half, y, half),
                ],
                normals: vec![-Vec3::Y; 4],
                uvs: vec![glam::Vec2::ZERO; 4],
                ranges: vec![(0, 6, 0)],
                indices: vec![0, 1, 2, 0, 2, 3],
                one_sided: false,
            },
        )
    }
    let camera = Camera {
        position: DVec3::ZERO,
        yaw: 0.0,
        pitch: 0.0,
        roll: 0.0,
        fov_deg: 90.0,
        near: 0.1,
        far: 100.0,
    };
    for (msaa, ssao, enhanced) in [
        (1, false, false),
        (1, true, false),
        (1, true, true),
        (4, true, true),
    ] {
        let instance = wgpu::Instance::new(wgpu::InstanceDescriptor::new_without_display_handle());
        let mut renderer = pollster::block_on(Renderer::new_with(
            &instance,
            None,
            Some(wgpu::TextureFormat::Rgba8UnormSrgb),
            RenderOptions {
                msaa,
                ssao,
                shadow_size: 1024,
                fxaa: false,
                render_scale: 1.0,
                ..Default::default()
            },
        ))
        .expect("test renderer");
        let mut scene = renderer.new_scene();
        let green = renderer.add_material(
            &mut scene,
            None,
            AlphaMode::Opaque,
            [0.0, 1.0, 0.0, 1.0],
            true,
        );
        let blue = renderer.add_material(
            &mut scene,
            None,
            AlphaMode::Opaque,
            [0.0, 0.0, 1.0, 1.0],
            true,
        );
        let red = renderer.add_material(
            &mut scene,
            None,
            AlphaMode::Opaque,
            [1.0, 0.0, 0.0, 1.0],
            true,
        );
        let texture = renderer.add_texture(
            &mut scene,
            &omsi_texture::Image {
                width: 1,
                height: 1,
                rgba: vec![255, 255, 255, 0],
                has_alpha: true,
            },
            false,
        );
        let transparent =
            renderer.add_material(&mut scene, Some(texture), AlphaMode::Blend, [1.0; 4], true);
        let cutout =
            renderer.add_material(&mut scene, Some(texture), AlphaMode::Test, [1.0; 4], true);
        let terrain = quad(&renderer, &mut scene, 6.0, 10.0);
        renderer.add_instance(
            &mut scene,
            terrain,
            DVec3::ZERO,
            Mat4::IDENTITY,
            vec![green],
        );
        let floor = quad(&renderer, &mut scene, 8.0, 10.0);
        let floor = renderer.add_surface_instance(
            &mut scene,
            floor,
            DVec3::ZERO,
            Mat4::IDENTITY,
            vec![blue],
        );
        scene.instances[floor].presurface = true;
        let cover_mesh = quad(&renderer, &mut scene, 4.0, 1.5);
        let cover = renderer.add_surface_instance(
            &mut scene,
            cover_mesh,
            DVec3::ZERO,
            Mat4::IDENTITY,
            vec![transparent],
        );
        scene.instances[cover].presurface = true;
        let foreground_mesh = quad(&renderer, &mut scene, 2.0, 0.25);
        let foreground = renderer.add_instance(
            &mut scene,
            foreground_mesh,
            DVec3::ZERO,
            Mat4::IDENTITY,
            vec![red],
        );
        scene.instances[foreground].visible = false;
        let lighting = Lighting {
            enhanced,
            shadows: false,
            fog_density: 0.0,
            ..Default::default()
        };
        let pixel = |rgba: &[u8], x: usize| -> [u8; 3] {
            rgba[(32 * 64 + x) * 4..(32 * 64 + x) * 4 + 3]
                .try_into()
                .unwrap()
        };
        let rgba = renderer
            .render_to_image(&mut scene, 64, 64, &camera, &lighting)
            .unwrap();
        let centre = pixel(&rgba, 32);
        assert!(
            centre[2] > centre[1] + 40,
            "floor must show through cover: {centre:?}; {msaa}/{ssao}/{enhanced}"
        );
        let outside = pixel(&rgba, 4);
        assert!(
            outside[1] > outside[2] + 40,
            "terrain outside cover: {outside:?}"
        );
        scene.instances[foreground].visible = true;
        Renderer::mark_changed(&mut scene, foreground);
        let rgba = renderer
            .render_to_image(&mut scene, 64, 64, &camera, &lighting)
            .unwrap();
        let centre = pixel(&rgba, 32);
        assert!(
            centre[0] > centre[2] + 40,
            "foreground stays visible: {centre:?}"
        );
        scene.instances[foreground].visible = false;
        Renderer::mark_changed(&mut scene, foreground);
        for (presurface, alpha, no_z_write) in [
            (false, AlphaMode::Blend, false),
            (true, AlphaMode::Blend, true),
            (true, AlphaMode::Test, false),
        ] {
            scene.instances[cover].presurface = presurface;
            scene.materials[transparent].no_z_write = no_z_write;
            renderer.set_material(
                &mut scene,
                cover,
                0,
                if alpha == AlphaMode::Test {
                    cutout
                } else {
                    transparent
                },
            );
            let rgba = renderer
                .render_to_image(&mut scene, 64, 64, &camera, &lighting)
                .unwrap();
            let centre = pixel(&rgba, 32);
            assert!(
                centre[1] > centre[2] + 40,
                "terrain should show: {centre:?}; {presurface}/{alpha:?}/{no_z_write}"
            );
        }
    }
}

#[test]
fn declared_transmap_ignores_slot_alpha() {
    let mut descriptor = wgpu::InstanceDescriptor::new_without_display_handle();
    descriptor.backends = wgpu::Backends::NOOP;
    descriptor.backend_options.noop = wgpu::NoopBackendOptions::enabled();
    let instance = wgpu::Instance::new(descriptor);
    let renderer = pollster::block_on(Renderer::new_with(
        &instance,
        None,
        Some(wgpu::TextureFormat::Rgba8UnormSrgb),
        RenderOptions {
            msaa: 1,
            shadow_size: 1024,
            ..Default::default()
        },
    ))
    .expect("noop renderer");
    let mut scene = renderer.new_scene();
    let blended = |scene: &mut Scene, transmap: Option<(TextureId, bool)>, extra: MaterialExtra| {
        renderer.add_material_extra(
            scene,
            None,
            AlphaMode::Blend,
            [1.0; 4],
            true,
            transmap,
            None,
            None,
            None,
            [0.0; 3],
            extra,
        )
    };
    // declared, its file missing: no transmap texture bound
    let declared = blended(
        &mut scene,
        None,
        MaterialExtra {
            transmap_declared: true,
            ..Default::default()
        },
    );
    let map = renderer.add_blank_texture(&mut scene, 1, 1);
    let bound = blended(&mut scene, Some((map, true)), MaterialExtra::default());
    // another bit of the same flags, no transmap
    let metal = blended(
        &mut scene,
        None,
        MaterialExtra {
            metal_ok: true,
            ..Default::default()
        },
    );
    let plain = blended(&mut scene, None, MaterialExtra::default());
    assert_eq!(scene.materials[metal].uniform.params2[3], 4.0);
    let flags: Vec<bool> = [declared, bound, metal, plain]
        .iter()
        .map(|&m| scene.materials[m].transmap_declared())
        .collect();
    assert_eq!(flags, [true, true, false, false]);
    let corner = [Vec3::ZERO, Vec3::X, Vec3::Y];
    let data = MeshData {
        positions: corner.repeat(4),
        normals: vec![Vec3::Z; 12],
        uvs: vec![glam::Vec2::ZERO; 12],
        indices: (0..12).collect(),
        ranges: (0..4).map(|s| (s * 3, 3, s)).collect(),
        ..Default::default()
    };
    let mesh = renderer.add_mesh(&mut scene, &data);
    let i = renderer.add_instance(
        &mut scene,
        mesh,
        DVec3::ZERO,
        Mat4::IDENTITY,
        vec![declared, bound, metal, plain],
    );
    renderer.set_params(&mut scene, i, &[0.0, 0.0, 0.35, 0.35], true, &[]);
    assert_eq!(scene.instances[i].slot_alpha, vec![1.0, 1.0, 0.35, 0.35]);
}

#[test]
fn surface_depth_coverage_excludes_glass_and_terrain_masks() {
    assert!(surface_depth_coverage(
        RenderPhase::Spline,
        AlphaMode::Blend,
        false,
        false
    ));
    for phase in RenderPhase::DRAW_ORDER {
        if !world_surface_phase(phase) {
            assert!(!surface_depth_coverage(
                phase,
                AlphaMode::Blend,
                false,
                false
            ));
        } else {
            assert!(surface_depth_coverage(
                phase,
                AlphaMode::Blend,
                false,
                false
            ));
        }
    }
    assert!(!surface_depth_coverage(
        RenderPhase::Spline,
        AlphaMode::Blend,
        true,
        false
    ));
    assert!(!surface_depth_coverage(
        RenderPhase::Spline,
        AlphaMode::Blend,
        false,
        true
    ));
    assert!(!surface_depth_coverage(
        RenderPhase::Spline,
        AlphaMode::Opaque,
        false,
        false
    ));
    assert!(!surface_depth_coverage(
        RenderPhase::Spline,
        AlphaMode::Test,
        false,
        false
    ));
    let order = RenderPhase::DRAW_ORDER;
    assert!(
        order
            .iter()
            .position(|p| *p == RenderPhase::OnSurface)
            .unwrap()
            < order
                .iter()
                .position(|p| *p == RenderPhase::BeforeNormal)
                .unwrap()
    );
}

#[test]
fn opaque_and_transmapped_materials_ignore_dynamic_alpha() {
    assert_eq!(
        Renderer::clamp_slot_alpha(0.0, AlphaMode::Opaque, false),
        1.0
    );
    assert_eq!(
        Renderer::clamp_slot_alpha(0.35, AlphaMode::Opaque, false),
        1.0
    );
    assert_eq!(Renderer::clamp_slot_alpha(0.0, AlphaMode::Test, false), 1.0);
    assert_eq!(
        Renderer::clamp_slot_alpha(0.35, AlphaMode::Test, false),
        1.0
    );
    assert_eq!(
        Renderer::clamp_slot_alpha(0.85, AlphaMode::Blend, false),
        0.85
    );
    assert_eq!(Renderer::clamp_slot_alpha(0.0, AlphaMode::Blend, true), 1.0);
    assert_eq!(
        Renderer::clamp_slot_alpha(0.0, AlphaMode::Opaque, true),
        1.0
    );
    assert_eq!(Renderer::clamp_slot_alpha(0.0, AlphaMode::Test, true), 1.0);
}
