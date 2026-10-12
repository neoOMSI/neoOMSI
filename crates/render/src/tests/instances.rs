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
    // The window's view keeps the origin across the boundary too...
    renderer.render(&mut scene, &view, 64, 64, &mirror, &lighting);
    assert_eq!(scene.render_origin, DVec3::ZERO);
    assert_eq!(scene.cpu_models, models);
    // ...and takes it along once the bus is far from it.
    let far = Camera {
        position: DVec3::new(612.5, -20.0, 2.0),
        ..camera
    };
    renderer.render(&mut scene, &view, 64, 64, &far, &lighting);
    assert_eq!(scene.render_origin, DVec3::new(600.0, -100.0, 0.0));
    assert_ne!(scene.cpu_models, models);
    assert_entries_match_a_rebuild(&scene);
}

/// The uploaded entries are the ones a complete rebuild would make.
fn assert_entries_match_a_rebuild(scene: &Scene) {
    let mut models = Vec::new();
    let mut params = Vec::new();
    for inst in &scene.instances {
        assert_eq!(inst.base as usize, models.len());
        Renderer::instance_entries(inst, scene.render_origin, &mut models, &mut params);
    }
    assert_eq!(scene.cpu_models, models);
    assert_eq!(scene.cpu_params, params);
}

#[test]
fn a_sideways_origin_move_rewrites_only_the_translations() {
    let renderer = noop_renderer();
    let mut scene = renderer.new_scene();
    let material = renderer.add_material(&mut scene, None, AlphaMode::Opaque, [1.0; 4], true);
    let mesh_of = |slots: u32| MeshData {
        positions: vec![Vec3::ZERO, Vec3::X, Vec3::Y],
        normals: vec![Vec3::Z; 3],
        uvs: vec![glam::Vec2::ZERO; 3],
        indices: vec![0, 1, 2],
        ranges: (0..slots).map(|slot| (0, 3, slot)).collect(),
        ..Default::default()
    };
    let meshes = [1, 3, 2].map(|slots| renderer.add_mesh(&mut scene, &mesh_of(slots)));
    for k in 0..300usize {
        let transform = Mat4::from_scale_rotation_translation(
            Vec3::splat(1.0 + k as f32 * 0.01),
            glam::Quat::from_rotation_z(k as f32 * 0.1),
            Vec3::new(0.5, -1.5, 0.25),
        );
        let i = renderer.add_instance(
            &mut scene,
            meshes[k % 3],
            DVec3::new(4_400_000.0 + k as f64 * 7.3, -2_400_000.0 - k as f64 * 3.1, 12.0),
            transform,
            vec![material; 3],
        );
        if k % 5 == 0 {
            scene.instances[i].roof = Some(3.0);
        }
    }
    renderer.set_render_origin(&mut scene, DVec3::new(4_400_000.0, -2_400_100.0, 0.0));
    renderer.prepare(&mut scene);
    assert_entries_match_a_rebuild(&scene);
    let params = scene.cpu_params.clone();
    let (models_buf, params_buf) = (scene.model_buf.clone(), scene.params_buf.clone());
    // sideways: the parameters (roof heights) stay, no buffer is made anew
    renderer.set_render_origin(&mut scene, DVec3::new(4_400_500.0, -2_400_400.0, 0.0));
    assert!(scene.origin_moved && !scene.dirty);
    // (an instance moved and one added in the same frame)
    renderer.set_transform(&mut scene, 7, DVec3::new(4_400_510.0, -2_400_390.0, 14.0), Mat4::IDENTITY);
    renderer.add_instance(
        &mut scene,
        meshes[1],
        DVec3::new(4_400_520.0, -2_400_380.0, 12.0),
        Mat4::IDENTITY,
        vec![material; 3],
    );
    renderer.prepare(&mut scene);
    assert!(!scene.origin_moved && !scene.dirty);
    assert_eq!(scene.model_buf, models_buf);
    assert_eq!(scene.params_buf, params_buf);
    assert_eq!(scene.cpu_params[..params.len()], params[..]);
    assert_entries_match_a_rebuild(&scene);
    // a move in height makes everything anew (the roof heights depend on it)
    renderer.set_render_origin(&mut scene, DVec3::new(4_400_500.0, -2_400_400.0, 100.0));
    assert!(scene.dirty);
    renderer.prepare(&mut scene);
    assert_entries_match_a_rebuild(&scene);
}

#[test]
fn a_pose_measures_the_instances_drawing_reshaped_meshes() {
    let renderer = noop_renderer();
    let mut scene = renderer.new_scene();
    scene.cache_bounds = true;
    let mesh = line_of_instances(&renderer, &mut scene, 600);
    let data = MeshData {
        positions: vec![Vec3::ZERO, Vec3::X, Vec3::Y],
        normals: vec![Vec3::Z; 3],
        uvs: vec![glam::Vec2::ZERO; 3],
        indices: vec![0, 1, 2],
        ranges: vec![(0, 3, 0)],
        ..Default::default()
    };
    let person = renderer.add_mesh(&mut scene, &data);
    let first = renderer.add_instance(&mut scene, person, DVec3::new(5.0, 5.0, 0.0), Mat4::IDENTITY, vec![0]);
    renderer.prepare(&mut scene);
    let pose = |scale: f32| -> Vec<Vec3> { data.positions.iter().map(|p| *p * scale).collect() };
    renderer.update_mesh(&mut scene, person, &pose(4.0), &data.normals, &data.uvs);
    renderer.prepare(&mut scene);
    assert_eq!(scene.reshaped_users, vec![first]);
    // one made after the first pose, one given the reshaped mesh, one put in another's place
    let late = renderer.add_instance(&mut scene, person, DVec3::new(50.0, 5.0, 0.0), Mat4::IDENTITY, vec![0]);
    renderer.set_instance_mesh(&mut scene, 123, person);
    renderer.prepare(&mut scene);
    let replacement =
        renderer.add_instance(&mut scene, person, DVec3::new(90.0, 5.0, 0.0), Mat4::IDENTITY, vec![0]);
    assert_eq!(renderer.recycle_instance(&mut scene, replacement, 321), 321);
    renderer.prepare(&mut scene);
    renderer.update_mesh(&mut scene, person, &pose(9.0), &data.normals, &data.uvs);
    renderer.prepare(&mut scene);
    let expected = InstanceBounds::new(&scene.meshes[person], Mat4::IDENTITY);
    for i in [first, late, 123, 321] {
        assert_eq!(scene.instances[i].bounds.radius, expected.radius, "instance {i}");
    }
    // the other instances are not on the list, and their bounds stay
    let mut users = scene.reshaped_users.clone();
    users.sort_unstable();
    assert_eq!(users, vec![123, 321, first, late]);
    let plain = InstanceBounds::new(&scene.meshes[mesh], Mat4::IDENTITY);
    assert_eq!(scene.instances[0].bounds.radius, plain.radius);
    assert_blocks_hold_their_instances(&scene);
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
fn only_depth_writing_blends_get_a_depth_pipe_before_the_rain() {
    for cull in [false, true] {
        for surface in [false, true] {
            assert_eq!(
                blended_depth_pipe(pipe_code(PIPE_BLEND, cull, surface)),
                Some(cull as u8)
            );
            for kind in [PIPE_OPAQUE, PIPE_ALPHA_TEST, PIPE_BLEND_NO_WRITE, PIPE_SURFACE_DEPTH] {
                assert_eq!(blended_depth_pipe(pipe_code(kind, cull, surface)), None);
            }
        }
    }
}

#[test]
fn only_parts_after_a_rain_layer_follow_the_rain() {
    // pillar, rain pane, divider, another rain pane, sticker
    let mut seen = false;
    let follows: Vec<bool> = [false, true, false, true, false]
        .into_iter()
        .map(|rain| follows_rain(&mut seen, rain))
        .collect();
    assert_eq!(follows, [false, false, true, false, true]);
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

fn line_of_instances(renderer: &Renderer, scene: &mut Scene, n: usize) -> MeshId {
    let material = renderer.add_material(scene, None, AlphaMode::Opaque, [1.0; 4], true);
    let data = MeshData {
        positions: vec![Vec3::ZERO, Vec3::X, Vec3::Y],
        normals: vec![Vec3::Z; 3],
        uvs: vec![glam::Vec2::ZERO; 3],
        indices: vec![0, 1, 2],
        ranges: vec![(0, 3, 0)],
        ..Default::default()
    };
    let mesh = renderer.add_mesh(scene, &data);
    for id in 0..n {
        renderer.add_instance(
            scene,
            mesh,
            DVec3::new(id as f64 * 10.0, 0.0, 0.0),
            Mat4::IDENTITY,
            vec![material],
        );
    }
    mesh
}

fn assert_blocks_hold_their_instances(scene: &Scene) {
    for (k, i) in scene.instances.iter().enumerate() {
        let (lo, hi) = scene.block_bounds[k / crate::world::CULL_BLOCK];
        let c = i.world_centre();
        let r = i.bounds.radius as f64;
        assert!(
            (c - r).cmpge(lo - 1e-6).all() && (c + r).cmple(hi + 1e-6).all(),
            "instance {k} at {c:?} outside its block {lo:?}..{hi:?}"
        );
    }
}

#[test]
fn culling_blocks_hold_moved_and_new_instances() {
    let renderer = noop_renderer();
    let mut scene = renderer.new_scene();
    let mesh = line_of_instances(&renderer, &mut scene, 1000);
    renderer.prepare(&mut scene);
    assert_blocks_hold_their_instances(&scene);
    // moved far off, a few at a time over several frames, and more added
    for frame in 0..40 {
        let k = (frame * 37) % 1000;
        renderer.set_transform(
            &mut scene,
            k,
            DVec3::new(-5000.0, 3000.0 + frame as f64, 20.0),
            Mat4::IDENTITY,
        );
        if frame % 10 == 0 {
            renderer.add_instance(
                &mut scene,
                mesh,
                DVec3::new(frame as f64, -800.0, 0.0),
                Mat4::IDENTITY,
                vec![0],
            );
        }
        renderer.prepare(&mut scene);
        assert_blocks_hold_their_instances(&scene);
    }
}

#[test]
fn instances_near_a_point_are_all_that_can_be_in_reach() {
    let renderer = noop_renderer();
    let mut scene = renderer.new_scene();
    let mesh = line_of_instances(&renderer, &mut scene, 1000);
    renderer.prepare(&mut scene);
    let p = DVec3::new(4000.0, 5.0, 0.0);
    let reach = 120.0;
    let check = |scene: &Scene| {
        let mut near = Vec::new();
        scene.instances_near(p, reach, &mut near);
        assert!(near.windows(2).all(|w| w[0] < w[1]), "in order, each once");
        let wanted: Vec<usize> = (0..scene.instances.len())
            .filter(|&i| (scene.instances[i].world_centre() - p).length_squared() < reach * reach)
            .collect();
        for i in &wanted {
            assert!(
                near.binary_search(i).is_ok(),
                "instance {i} in reach left out"
            );
        }
        // (and far fewer than all of them)
        assert!(near.len() < scene.instances.len() / 2);
    };
    check(&scene);
    // moved into reach and made there since the boxes were measured
    renderer.set_transform(&mut scene, 3, p + DVec3::X, Mat4::IDENTITY);
    renderer.add_instance(&mut scene, mesh, p - DVec3::X, Mat4::IDENTITY, vec![0]);
    check(&scene);
    renderer.prepare(&mut scene);
    check(&scene);
}

#[test]
fn kept_main_bundles_are_drawn_again_only_where_nothing_changed() {
    let renderer = noop_renderer();
    let mut scene = renderer.new_scene();
    let mesh = line_of_instances(&renderer, &mut scene, 8);
    let other = renderer.add_material(&mut scene, None, AlphaMode::Opaque, [0.5; 4], true);
    let mut renderer = renderer;
    let view = Camera {
        position: DVec3::new(0.0, -20.0, 2.0),
        yaw: 0.0,
        pitch: 0.0,
        roll: 0.0,
        fov_deg: 60.0,
        near: 0.1,
        far: 500.0,
    };
    // (a picture made makes the camera's group)
    let _ = renderer.render_to_image(&mut scene, 64, 64, &view, &Lighting::default());
    let renderer = renderer;
    let camera = scene.camera_bind_group.clone().expect("camera bind group");
    let pp = renderer.main_pass(false, false);
    let mut batches: Vec<Batch> = (0..1200)
        .map(|k| Batch {
            pipe: PIPE_OPAQUE,
            mesh: mesh as u32,
            first: 0,
            count: 3,
            material: 0,
            instances: k..k + 1,
        })
        .collect();
    let kept = std::cell::RefCell::new(KeptBundles::default());
    let record = |batches: &[Batch]| {
        record_bundles(
            &renderer.device,
            renderer.encoding_pool.as_deref(),
            &scene,
            batches,
            pp,
            &camera,
            renderer.format,
            1,
            Some(&kept),
        )
    };
    let first = record(&batches);
    assert!(first.len() > 2, "several parts");
    // the same batches: every part drawn with the bundle recorded before
    let again = record(&batches);
    assert_eq!(again, first);
    // one batch of the last part bound to another material: that part alone is new
    batches[1199].material = other as u32;
    let changed = record(&batches);
    let last = first.len() - 1;
    assert_eq!(changed[..last], first[..last]);
    assert_ne!(changed[last], first[last]);
    // without keeping, nothing is taken over
    let fresh = record_bundles(
        &renderer.device,
        renderer.encoding_pool.as_deref(),
        &scene,
        &batches,
        pp,
        &camera,
        renderer.format,
        1,
        None,
    );
    assert!(fresh.iter().all(|b| !changed.contains(b)));
}

#[test]
fn page_spans_are_taken_first_fit_and_joined_when_given_back() {
    let (mut free, mut top) = (Vec::new(), 0u64);
    let a = page_take(&mut free, &mut top, 1000, 100).unwrap();
    let b = page_take(&mut free, &mut top, 1000, 200).unwrap();
    let c = page_take(&mut free, &mut top, 1000, 300).unwrap();
    assert_eq!((a, b, c, top), (0, 100, 300, 600));
    assert_eq!(page_take(&mut free, &mut top, 1000, 401), None);
    page_give(&mut free, &mut top, a, 100);
    // (too small a gap: taken from the top)
    assert_eq!(page_take(&mut free, &mut top, 1000, 150), Some(600));
    page_give(&mut free, &mut top, b, 200);
    // the two gaps side by side are one now, and the first fit
    assert_eq!(free, vec![(0, 300)]);
    assert_eq!(page_take(&mut free, &mut top, 1000, 250), Some(0));
    assert_eq!(free, vec![(250, 50)]);
    // the last span given back lowers the top, with the gap below it
    page_give(&mut free, &mut top, 600, 150);
    page_give(&mut free, &mut top, c, 300);
    page_give(&mut free, &mut top, 0, 250);
    assert!(free.is_empty());
    assert_eq!(top, 0);
}

#[test]
fn small_meshes_share_a_page_and_freed_space_is_used_again() {
    let mut renderer = noop_renderer();
    renderer.mesh_pages = true;
    let mut scene = renderer.new_scene();
    let tri = |n: usize| MeshData {
        positions: vec![Vec3::ZERO; n],
        normals: vec![Vec3::Z; n],
        uvs: vec![glam::Vec2::ZERO; n],
        indices: (0..n as u32).collect(),
        ranges: vec![(0, n as u32, 0)],
        ..Default::default()
    };
    let a = renderer.add_mesh(&mut scene, &tri(3));
    let b = renderer.add_mesh(&mut scene, &tri(6));
    let c = renderer.add_mesh(&mut scene, &tri(3));
    assert_eq!(scene.mesh_pages.len(), 1);
    let (ma, mb, mc) = (&scene.meshes[a], &scene.meshes[b], &scene.meshes[c]);
    assert!(ma.vertex_buf == mb.vertex_buf && mb.index_buf == mc.index_buf);
    assert_eq!((ma.base_vertex, mb.base_vertex, mc.base_vertex), (0, 3, 9));
    assert_eq!((ma.first_index, mb.first_index, mc.first_index), (0, 3, 9));
    let one_page = scene.mesh_buffer_bytes();
    assert_eq!(one_page, MESH_PAGE_VERTEX_BYTES + MESH_PAGE_INDEX_BYTES);
    // b's place goes to the next mesh that fits
    renderer.free_mesh(&mut scene, b);
    assert_eq!(scene.meshes[b].page, NO_PAGE);
    let d = renderer.add_mesh(&mut scene, &tri(6));
    assert_eq!((scene.meshes[d].base_vertex, scene.meshes[d].first_index), (3, 3));
    // a pose is written where the mesh lies in its page
    renderer.update_mesh(&mut scene, d, &[Vec3::X; 6], &[Vec3::Z; 6], &[glam::Vec2::ZERO; 6]);
    assert!(renderer.pending_meshes.borrow().contains_key(&d));
    let mut encoder = renderer.device.create_command_encoder(&Default::default());
    renderer.flush_pending_meshes(&scene, &mut encoder);
    renderer.queue.submit([encoder.finish()]);
    // (a pose larger than the mesh's place is left out)
    renderer.update_mesh(&mut scene, a, &[Vec3::X; 6], &[Vec3::Z; 6], &[glam::Vec2::ZERO; 6]);
    assert!(!renderer.pending_meshes.borrow().contains_key(&a));
    // a mesh too large for a page, and the meshes cut off, give their space back
    let big = renderer.add_mesh(&mut scene, &tri((MESH_PAGE_VERTEX_BYTES / 4 / 32) as usize + 1));
    assert_eq!(scene.meshes[big].page, NO_PAGE);
    assert!(scene.mesh_buffer_bytes() > one_page);
    let (textures, materials) = (scene.textures.len(), scene.materials.len());
    renderer.truncate(&mut scene, a + 1, textures, materials, 0);
    let e = renderer.add_mesh(&mut scene, &tri(9));
    assert_eq!((scene.meshes[e].base_vertex, scene.meshes[e].first_index), (3, 3));
    // a mesh posed every frame keeps buffers of its own
    let posed = renderer.add_animated_mesh(&mut scene, &tri(3));
    assert_eq!(scene.meshes[posed].page, NO_PAGE);
    // without pages every mesh has buffers of its own
    renderer.mesh_pages = false;
    let own = renderer.add_mesh(&mut scene, &tri(3));
    assert_eq!(scene.meshes[own].page, NO_PAGE);
    assert_eq!(scene.meshes[own].base_vertex, 0);
}
