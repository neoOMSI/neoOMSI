use crate::*;

fn noop_renderer() -> Renderer {
    let mut descriptor = wgpu::InstanceDescriptor::new_without_display_handle();
    descriptor.backends = wgpu::Backends::NOOP;
    descriptor.backend_options.noop = wgpu::NoopBackendOptions::enabled();
    let instance = wgpu::Instance::new(descriptor);
    pollster::block_on(Renderer::new_with(
        &instance,
        None,
        Some(wgpu::TextureFormat::Rgba8UnormSrgb),
        RenderOptions {
            msaa: 1,
            ssao: false,
            fxaa: false,
            no_enhanced: true,
            shadow_size: 1024,
            ..Default::default()
        },
    ))
    .expect("noop test renderer")
}

#[test]
fn scattered_updates_upload_only_the_changed_stream_and_slots() {
    let mut renderer = noop_renderer();
    renderer.profiling = true;
    let mut scene = renderer.new_scene();
    let material = renderer.add_material(&mut scene, None, AlphaMode::Opaque, [1.0; 4], true);
    let data = MeshData {
        positions: vec![Vec3::ZERO, Vec3::X, Vec3::Y],
        normals: vec![Vec3::Z; 3],
        uvs: vec![glam::Vec2::ZERO; 3],
        indices: vec![0, 1, 2],
        ranges: (0..4).map(|slot| (0, 3, slot)).collect(),
        ..Default::default()
    };
    let mesh = renderer.add_mesh(&mut scene, &data);
    for id in 0..128 {
        renderer.add_instance(
            &mut scene,
            mesh,
            DVec3::new(id as f64, 0.0, 0.0),
            Mat4::IDENTITY,
            vec![material; 4],
        );
    }
    renderer.prepare(&mut scene);
    let models = scene.cpu_models.clone();
    renderer.counts.borrow_mut().clear();
    for id in [0, 32, 64, 96] {
        renderer.set_slot_light(&mut scene, id, &[0.5; 4]);
    }
    renderer.prepare(&mut scene);
    assert_eq!(scene.cpu_models, models);
    assert_eq!(
        renderer.counts.borrow()["instance update bytes"],
        4.0 * 4.0 * 32.0
    );
    assert_eq!(renderer.counts.borrow()["instance update writes"], 4.0);
    let params = scene.cpu_params.clone();
    renderer.counts.borrow_mut().clear();
    renderer.set_transform(&mut scene, 64, DVec3::new(64.0, 0.0, 1.0), Mat4::IDENTITY);
    renderer.prepare(&mut scene);
    assert_eq!(scene.cpu_params, params);
    assert_eq!(
        renderer.counts.borrow()["instance update bytes"],
        4.0 * 64.0
    );
    assert_eq!(renderer.counts.borrow()["instance update writes"], 1.0);
    renderer.counts.borrow_mut().clear();
    scene.instances[64].roof = Some(2.0);
    renderer.set_transform(&mut scene, 64, DVec3::new(64.0, 0.0, 2.0), Mat4::IDENTITY);
    renderer.prepare(&mut scene);
    assert_eq!(
        renderer.counts.borrow()["instance update bytes"],
        4.0 * 96.0
    );
    assert_eq!(renderer.counts.borrow()["instance update writes"], 2.0);
    // The incremental CPU state must match a complete rebuild, including roof parameters.
    let mut expected_models = Vec::new();
    let mut expected_params = Vec::new();
    for inst in &scene.instances {
        Renderer::instance_entries(
            inst,
            scene.render_origin,
            &mut expected_models,
            &mut expected_params,
        );
    }
    assert_eq!(scene.cpu_models, expected_models);
    assert_eq!(scene.cpu_params, expected_params);
    assert!(scene.changed.is_empty());
    assert!(scene.changed_mark.iter().all(|&marked| !marked));
}

#[test]
fn animated_uploads_keep_only_the_last_valid_pose_and_drain() {
    let renderer = noop_renderer();
    let mut scene = renderer.new_scene();
    let data = MeshData {
        positions: vec![Vec3::ZERO, Vec3::X, Vec3::Y],
        normals: vec![Vec3::Z; 3],
        uvs: vec![glam::Vec2::ZERO; 3],
        indices: vec![0, 1, 2],
        ranges: vec![(0, 3, 0)],
        ..Default::default()
    };
    let first = renderer.add_mesh(&mut scene, &data);
    let second = renderer.add_mesh(&mut scene, &data);
    let posed: Vec<_> = data.positions.iter().map(|p| *p + Vec3::Z * 2.0).collect();
    renderer.update_mesh(&mut scene, first, &data.positions, &data.normals, &data.uvs);
    renderer.update_mesh(
        &mut scene,
        second,
        &data.positions,
        &data.normals,
        &data.uvs,
    );
    renderer.update_mesh(&mut scene, first, &posed, &data.normals, &data.uvs);
    // An update that exceeds the allocated vertex buffer must not replace a valid one.
    renderer.update_mesh(
        &mut scene,
        first,
        &[Vec3::ZERO; 4],
        &[Vec3::Z; 4],
        &[glam::Vec2::ZERO; 4],
    );
    let pending = renderer.pending_meshes.borrow();
    assert_eq!(pending.len(), 2);
    assert_eq!(pending[&first][0].pos, posed[0].to_array());
    assert_eq!(pending[&second][0].pos, data.positions[0].to_array());
    let capacity = pending.capacity();
    drop(pending);
    let mut encoder = renderer.device.create_command_encoder(&Default::default());
    renderer.flush_pending_meshes(&scene, &mut encoder);
    assert!(renderer.pending_meshes.borrow().is_empty());
    assert_eq!(renderer.pending_meshes.borrow().capacity(), capacity);
    renderer.queue.submit([encoder.finish()]);
}

#[test]
fn mirror_and_second_eye_reuse_uploaded_models_at_a_cell_boundary() {
    let mut renderer = noop_renderer();
    let mut scene = renderer.new_scene();
    let material = renderer.add_material(&mut scene, None, AlphaMode::Opaque, [1.0; 4], true);
    let data = MeshData {
        positions: vec![Vec3::ZERO, Vec3::X, Vec3::Z],
        normals: vec![Vec3::Y; 3],
        uvs: vec![glam::Vec2::ZERO; 3],
        indices: vec![0, 1, 2],
        ranges: vec![(0, 3, 0)],
        ..Default::default()
    };
    let mesh = renderer.add_mesh(&mut scene, &data);
    renderer.add_instance(
        &mut scene,
        mesh,
        DVec3::new(100.0, 5.0, 0.0),
        Mat4::IDENTITY,
        vec![material],
    );
    let output = renderer.add_render_texture(&mut scene, 64, 64);
    let view = scene.textures[output].view.clone();
    let camera = Camera {
        position: DVec3::new(99.99, 0.0, 2.0),
        yaw: 0.0,
        pitch: 0.0,
        roll: 0.0,
        fov_deg: 60.0,
        near: 0.1,
        far: 6000.0,
    };
    let lighting = Lighting {
        shadows: false,
        light_shadows: false,
        ..Default::default()
    };
    renderer.render(&mut scene, &view, 64, 64, &camera, &lighting);
    let models = scene.cpu_models.clone();
    let mirror = Camera {
        position: camera.position + DVec3::X,
        ..camera
    };
    renderer.render_to_texture(&mut scene, output, &mirror, &lighting, 1.0);
    assert_eq!(scene.render_origin, DVec3::ZERO);
    assert_eq!(scene.cpu_models, models);
    let projection = glam::camera::rh::proj::directx::perspective(
        camera.fov_deg.to_radians(),
        1.0,
        camera.far,
        camera.near,
    );
    renderer.render_xr_eye(
        &mut scene, &view, 64, 64, &camera, &lighting, projection, false,
    );
    renderer.render_xr_eye(
        &mut scene, &view, 64, 64, &mirror, &lighting, projection, true,
    );
    assert_eq!(scene.render_origin, DVec3::ZERO);
    assert_eq!(scene.cpu_models, models);
    // The primary view still rebases normally when the bus crosses the boundary.
    renderer.render(&mut scene, &view, 64, 64, &mirror, &lighting);
    assert_eq!(scene.render_origin, DVec3::new(100.0, 0.0, 0.0));
    assert_ne!(scene.cpu_models, models);
}

#[test]
fn overlays_land_on_whole_pixels() {
    assert_eq!(
        snap_rect([25.25, 40.5, 145.25, 66.5]),
        [25.0, 41.0, 145.0, 67.0]
    );
    assert_eq!(
        snap_rect([10.0, 20.0, 30.0, 40.0]),
        [10.0, 20.0, 30.0, 40.0]
    );
    assert_eq!(snap_rect([-0.5, -2.5, 19.5, 7.5]), [0.0, -2.0, 20.0, 8.0]);
    let line = snap_rect([16.0, 100.3, 300.0, 100.9]);
    assert_eq!(line[3] - line[1], 1.0);
    assert_eq!(snap_rect([5.2, 5.2, 5.2, 5.2]), [5.0, 5.0, 5.0, 5.0]);
}

#[test]
#[ignore = "requires a graphics adapter; run with --ignored on a GPU host"]
fn cached_bounds_follow_transforms_skinning_and_recycled_resources() {
    let adapter = wgpu::Instance::new(wgpu::InstanceDescriptor::new_without_display_handle());
    let renderer = pollster::block_on(Renderer::new_with(
        &adapter,
        None,
        Some(wgpu::TextureFormat::Rgba8UnormSrgb),
        RenderOptions {
            msaa: 1,
            shadow_size: 1024,
            ..Default::default()
        },
    ))
    .expect("test renderer");
    let mut scene = renderer.new_scene();
    scene.cache_bounds = true;
    let material = renderer.add_material(&mut scene, None, AlphaMode::Opaque, [1.0; 4], true);
    let data = MeshData {
        positions: vec![Vec3::ZERO, Vec3::X, Vec3::Y],
        normals: vec![Vec3::Z; 3],
        uvs: vec![glam::Vec2::ZERO; 3],
        indices: vec![0, 1, 2],
        ranges: vec![(0, 3, 0)],
        ..Default::default()
    };
    let mesh = renderer.add_mesh(&mut scene, &data);
    let origin = DVec3::new(1_000_000.000_001, 2_000_000.0, 12.0);
    let i = renderer.add_instance(&mut scene, mesh, origin, Mat4::IDENTITY, vec![material]);
    let check = |scene: &Scene, i: usize| {
        let inst = &scene.instances[i];
        let expected = InstanceBounds::new(&scene.meshes[inst.mesh], inst.transform);
        assert_eq!(
            Renderer::bounding_sphere(scene, inst),
            (
                expected.centre + (inst.origin - scene.render_origin).as_vec3(),
                expected.radius
            )
        );
        assert_eq!(Renderer::instance_scale(scene, inst), expected.scale);
    };
    renderer.prepare(&mut scene);
    check(&scene, i);
    let transform = Mat4::from_scale_rotation_translation(
        Vec3::new(-2.0, 3.0, 4.0),
        glam::Quat::from_rotation_z(0.7),
        Vec3::new(10.0, 20.0, 30.0),
    );
    renderer.set_transform(&mut scene, i, origin, transform);
    renderer.prepare(&mut scene);
    check(&scene, i);
    let posed: Vec<_> = data.positions.iter().map(|p| *p * 5.0 + Vec3::Z).collect();
    renderer.update_mesh(&mut scene, mesh, &posed, &data.normals, &data.uvs);
    renderer.prepare(&mut scene);
    check(&scene, i);
    renderer.set_render_origin(&mut scene, origin - DVec3::new(0.25, 0.5, 0.75));
    renderer.prepare(&mut scene);
    check(&scene, i);
    renderer.free_mesh(&mut scene, mesh);
    renderer.prepare(&mut scene);
    check(&scene, i);
    let new = renderer.add_mesh(&mut scene, &data);
    assert_eq!(renderer.recycle_mesh(&mut scene, new, mesh), mesh);
    renderer.prepare(&mut scene);
    check(&scene, i);
    let replacement =
        renderer.add_instance(&mut scene, mesh, origin, Mat4::IDENTITY, vec![material]);
    assert_eq!(renderer.recycle_instance(&mut scene, replacement, i), i);
    renderer.prepare(&mut scene);
    check(&scene, i);
    let other = renderer.add_mesh(&mut scene, &data);
    renderer.set_instance_mesh(&mut scene, i, other);
    renderer.prepare(&mut scene);
    check(&scene, i);
}

#[test]
fn noop_backend_initializes_renderer() {
    let mut descriptor = wgpu::InstanceDescriptor::new_without_display_handle();
    descriptor.backends = wgpu::Backends::NOOP;
    descriptor.backend_options.noop = wgpu::NoopBackendOptions::enabled();
    let instance = wgpu::Instance::new(descriptor);
    let res = pollster::block_on(Renderer::new_with(
        &instance,
        None,
        Some(wgpu::TextureFormat::Rgba8UnormSrgb),
        RenderOptions {
            msaa: 2,
            shadow_size: 1024,
            ..Default::default()
        },
    ));
    assert!(
        res.is_ok(),
        "renderer should initialize on noop backend: {:?}",
        res.err()
    );
}

#[test]
fn render_phases_are_monotonic_and_complete() {
    assert_eq!(
        RenderPhase::DRAW_ORDER.map(|phase| phase as usize),
        [0, 1, 2, 3, 4, 5, 6, 7, 8]
    );
    assert_eq!(RenderPhase::default(), RenderPhase::Normal);
}

#[test]
fn metric_lifted_surfaces_keep_shading_class_without_view_space_pull() {
    assert_eq!(surface_instance_code(false, false, false, true, false), 0.9);
    assert_eq!(surface_instance_code(false, false, false, true, true), 1.0);
    assert_eq!(surface_instance_code(false, false, true, false, false), 0.9);
    assert_eq!(surface_instance_code(false, true, false, true, false), 0.75);
}

#[test]
fn spline_blend_sort_ignores_height_and_camera_pitch() {
    let origin = DVec3::new(120.0, 45.0, 0.0);
    let render_origin = DVec3::new(100.0, 40.0, 0.0);
    let level_camera = Vec3::new(3.0, 1.0, 4.0);
    let high_camera = Vec3::new(3.0, 1.0, 80.0);
    assert_eq!(
        horizontal_sort_distance(origin, render_origin, level_camera),
        horizontal_sort_distance(origin + DVec3::Z * 60.0, render_origin, high_camera)
    );
}

#[test]
fn render_scale_auto_keeps_ordinary_windows_sharp() {
    assert_eq!(scene_scale_for(0.0, 1600, 900), 1.0);
    assert_eq!(scene_scale_for(0.0, 2560, 1080), 1.0);
    let s = scene_scale_for(0.0, 3200, 1800);
    if cfg!(target_os = "macos") || cfg!(target_os = "android") {
        assert!((s - 0.697).abs() < 0.01, "{s}");
        assert!((3200.0 * s * 1800.0 * s - AUTO_SCALE_PIXELS).abs() < 1.0);
    } else {
        assert_eq!(s, 1.0);
    }
    assert_eq!(scene_scale_for(0.0, 16384, 16384), 0.5);
    let s = scene_scale_for(0.75, 3200, 1800);
    if cfg!(target_os = "macos") || cfg!(target_os = "android") {
        assert!((s - 0.697).abs() < 0.01, "{s}");
        assert!((3200.0 * s * 1800.0 * s - AUTO_SCALE_PIXELS).abs() < 1.0);
    } else {
        assert_eq!(s, 0.75);
    }
    assert_eq!(scene_scale_for(0.3, 1600, 900), 0.5);
    assert_eq!(scene_scale_for(1.4, 1600, 900), 1.0);
}

#[test]
fn pipeline_codes_cover_the_table() {
    let mut seen = std::collections::HashSet::new();
    for kind in 0..PIPE_KINDS {
        for cull in [false, true] {
            for surface in [false, true] {
                let code = pipe_code(kind, cull, surface);
                assert!((code as usize) < PIPE_KINDS as usize * 4);
                assert!(seen.insert(code));
                assert_eq!(
                    code < pipe_code(PIPE_BLEND, false, false),
                    kind < PIPE_BLEND
                );
            }
        }
    }
    assert_eq!(seen.len(), PIPE_KINDS as usize * 4);
    assert_eq!(pipe_code(PIPE_OPAQUE, false, false), 0);
    assert!(pipe_code(PIPE_ALPHA_TEST, true, true) < pipe_code(PIPE_BLEND_NO_WRITE, false, false));
}

#[test]
fn nearest_by_origin_picks_the_closest_mesh_of_each_object() {
    let bus = DVec3::new(0.0, 0.0, 0.0);
    let car = DVec3::new(1.0, 0.0, 0.0);
    let by_origin = nearest_by_origin([(bus, 14.0), (bus, 3.0), (car, 8.0)]);
    let bus_dist = by_origin[&origin_key(bus)];
    let car_dist = by_origin[&origin_key(car)];
    assert_eq!(
        bus_dist, 3.0,
        "the object's distance is its nearest mesh, not the first or an average"
    );
    assert_eq!(car_dist, 8.0);
    assert!(car_dist > bus_dist);
    assert_eq!(
        by_origin.len(),
        2,
        "one entry per distinct origin, not per mesh"
    );
}

#[test]
fn origin_lookup_keeps_float_equality_and_large_coordinates() {
    let a = DVec3::new(-0.0, 1_000_000.000_001, 2.0);
    let b = DVec3::new(0.0, a.y, 2.0);
    let c = DVec3::new(0.0, a.y + 0.000_001, 2.0);
    let distances = nearest_by_origin([(a, 8.0), (b, 3.0), (c, 5.0), (DVec3::NAN, 1.0)]);
    assert_eq!(distances.len(), 2);
    assert_eq!(distances[&origin_key(a)], 3.0);
    assert_eq!(distances[&origin_key(c)], 5.0);
}

#[test]
fn vehicle_box_contains_the_driver() {
    let bus = (
        DVec3::new(100.0, 200.0, 30.0),
        90.0,
        [2.5, 12.0, 3.0, 0.0, 0.4, 1.5],
    );
    assert!(point_in_vehicle_box(DVec3::new(104.6, 200.7, 31.8), &bus));
    assert!(!point_in_vehicle_box(DVec3::new(104.6, 197.0, 31.8), &bus));
    assert!(!point_in_vehicle_box(DVec3::new(104.6, 200.0, 34.0), &bus));
    assert!(!point_in_vehicle_box(DVec3::new(93.0, 200.0, 31.0), &bus));
}
