use crate::*;

#[test]
#[ignore = "requires a graphics adapter; checks cabin lamp codes across perspective triangles"]
fn first_cabin_lamp_off_matches_no_cabin_light() {
    let adapter = wgpu::Instance::new(wgpu::InstanceDescriptor::new_without_display_handle());
    let mut renderer = pollster::block_on(Renderer::new_with(
        &adapter,
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
    let texture = renderer.add_texture(
        &mut scene,
        &::texture::Image::solid([48, 48, 48, 255]),
        false,
    );
    let material =
        renderer.add_material(&mut scene, Some(texture), AlphaMode::Opaque, [1.0; 4], false);
    let mesh = renderer.add_mesh(
        &mut scene,
        &MeshData {
            // Unequal depths exercise perspective interpolation, including values that
            // round just below 1 when a constant lamp code is interpolated as a float.
            positions: vec![
                Vec3::new(-3.1, 1.2, -2.7),
                Vec3::new(3.3, 4.7, -2.3),
                Vec3::new(2.8, 4.1, 2.9),
                Vec3::new(-2.6, 1.8, 2.5),
            ],
            normals: vec![-Vec3::Y; 4],
            uvs: vec![glam::Vec2::splat(0.5); 4],
            indices: vec![0, 1, 2, 0, 2, 3],
            ranges: vec![(0, 6, 0)],
            one_sided: false,
        },
    );
    let object = renderer.add_instance(
        &mut scene,
        mesh,
        DVec3::ZERO,
        Mat4::IDENTITY,
        vec![material],
    );
    let first = renderer.alloc_interior_lights(&mut scene, 1);
    assert_eq!(first, 0);
    renderer.set_interior_light(
        &mut scene,
        first,
        PointLight {
            position: DVec3::new(0.0, -1.0, 2.0),
            radius: 100.0,
            core: 3.0,
            intensity: 0.0,
            ..Default::default()
        },
    );
    let camera = Camera {
        position: DVec3::ZERO,
        yaw: 0.0,
        pitch: -3.7,
        roll: 2.3,
        fov_deg: 85.0,
        near: 0.1,
        far: 100.0,
    };
    for enhanced in [false, true] {
        scene.interior_lights[first as usize].intensity = 0.0;
        let lighting = Lighting {
            enhanced,
            sun_intensity: 0.0,
            secondary: Vec3::ZERO,
            ambient: Vec3::splat(0.05),
            shadows: false,
            detail: false,
            ..Default::default()
        };
        renderer.set_interior_lamps(&mut scene, object, 0, 0);
        let unlit = renderer
            .render_to_image(&mut scene, 256, 192, &camera, &lighting)
            .unwrap();
        renderer.set_interior_lamps(&mut scene, object, first, 1);
        let lamp_off = renderer
            .render_to_image(&mut scene, 256, 192, &camera, &lighting)
            .unwrap();
        let changed = unlit
            .chunks_exact(4)
            .zip(lamp_off.chunks_exact(4))
            .filter(|(a, b)| (0..3).any(|c| a[c].abs_diff(b[c]) > 2))
            .count();
        assert_eq!(
            changed, 0,
            "lamp 0 off changed {changed} pixels (enhanced={enhanced})"
        );
        scene.interior_lights[first as usize].intensity = 1.0;
        let lamp_on = renderer
            .render_to_image(&mut scene, 256, 192, &camera, &lighting)
            .unwrap();
        let brighter = lamp_off
            .chunks_exact(4)
            .zip(lamp_on.chunks_exact(4))
            .filter(|(a, b)| b[0] > a[0].saturating_add(4))
            .count();
        assert!(
            brighter > 100,
            "lamp 0 on must light the surface (enhanced={enhanced})"
        );
    }
}

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
            &::texture::Image {
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
            &::texture::Image {
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
#[ignore = "requires a graphics adapter; renders a sphere-mapped dark body in both vanilla pictures"]
fn envmap_lerp_over_dark_paint_is_the_same_in_vanilla_plus() {
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
    let black = renderer.add_texture(&mut scene, &::texture::Image::solid([0, 0, 0, 255]), false);
    let sky = renderer.add_texture(
        &mut scene,
        &::texture::Image::solid([160, 160, 160, 255]),
        false,
    );
    // the BHD A20's window band: opaque, `[matl_envmap] … 0.18`, no mask
    let body = renderer.add_material_extra(
        &mut scene,
        Some(black),
        AlphaMode::Opaque,
        [1.0; 4],
        false,
        None,
        None,
        None,
        Some((sky, 0.18)),
        [0.0; 3],
        MaterialExtra::default(),
    );
    let mesh = renderer.add_mesh(
        &mut scene,
        &MeshData {
            positions: vec![
                Vec3::new(-5.0, -5.0, 0.0),
                Vec3::new(5.0, -5.0, 0.0),
                Vec3::new(5.0, 5.0, 0.0),
                Vec3::new(-5.0, 5.0, 0.0),
            ],
            normals: vec![Vec3::Z; 4],
            uvs: vec![glam::Vec2::ZERO; 4],
            indices: vec![0, 1, 2, 0, 2, 3],
            ranges: vec![(0, 6, 0)],
            one_sided: false,
        },
    );
    renderer.add_instance(&mut scene, mesh, DVec3::ZERO, Mat4::IDENTITY, vec![body]);
    let camera = Camera {
        position: DVec3::new(0.0, -0.105, 6.0),
        yaw: 0.0,
        pitch: -89.0,
        roll: 0.0,
        fov_deg: 90.0,
        near: 0.1,
        far: 100.0,
    };
    let mut centre = |classic: bool| -> [u8; 3] {
        let lighting = Lighting {
            classic,
            shadows: false,
            fog_density: 0.0,
            ..Default::default()
        };
        let rgba = renderer
            .render_to_image(&mut scene, 64, 64, &camera, &lighting)
            .unwrap();
        rgba[(32 * 64 + 32) * 4..(32 * 64 + 32) * 4 + 3]
            .try_into()
            .unwrap()
    };
    let (vanilla, plus) = (centre(true), centre(false));
    assert!(
        vanilla[0] > 10,
        "the sphere map must show over the dark paint: {vanilla:?}"
    );
    assert!(
        vanilla.iter().zip(plus).all(|(v, p)| v.abs_diff(p) <= 3),
        "Vanilla+ must reflect as strongly as Vanilla: vanilla {vanilla:?}, plus {plus:?}"
    );
}

#[test]
#[ignore = "requires a graphics adapter; renders a wet pane across the whole screen"]
fn rain_film_is_as_clear_at_the_screen_edge_as_in_the_middle() {
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
    // a dark street behind the glass, far darker than the sky a drop falls back to
    let backdrop = renderer.add_material(
        &mut scene,
        None,
        AlphaMode::Opaque,
        [0.02, 0.05, 0.02, 1.0],
        true,
    );
    let film = renderer.add_material_extra(
        &mut scene,
        None,
        AlphaMode::Blend,
        [1.0; 4],
        false,
        None,
        None,
        None,
        None,
        [0.0; 3],
        MaterialExtra {
            rain_film: true,
            no_z_write: true,
            ..Default::default()
        },
    );
    let mut quad = |y: f32, half: f32, material| {
        let mesh = renderer.add_mesh(
            &mut scene,
            &MeshData {
                positions: vec![
                    Vec3::new(-half, y, -half),
                    Vec3::new(half, y, -half),
                    Vec3::new(half, y, half),
                    Vec3::new(-half, y, half),
                ],
                normals: vec![-Vec3::Y; 4],
                uvs: vec![
                    glam::Vec2::new(0.0, 1.0),
                    glam::Vec2::new(1.0, 1.0),
                    glam::Vec2::new(1.0, 0.0),
                    glam::Vec2::new(0.0, 0.0),
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
    quad(10.0, 40.0, backdrop);
    // the pane fills the whole picture: the edge of the screen is not the edge of the glass
    quad(1.0, 3.0, film);
    let camera = Camera {
        position: DVec3::ZERO,
        yaw: 0.0,
        pitch: 0.0,
        roll: 0.0,
        fov_deg: 70.0,
        near: 0.1,
        far: 100.0,
    };
    const SIZE: usize = 128;
    let mean = |rgba: &[u8], xs: std::ops::Range<usize>, ys: std::ops::Range<usize>| -> f32 {
        let (mut sum, mut n) = (0.0, 0.0);
        for y in ys {
            for x in xs.clone() {
                let p = &rgba[(y * SIZE + x) * 4..(y * SIZE + x) * 4 + 3];
                sum += p.iter().map(|&c| c as f32).sum::<f32>() / 3.0;
                n += 1.0;
            }
        }
        sum / n
    };
    let (inner, outer) = (24..SIZE - 24, 0..4);
    let far = SIZE - 4..SIZE;
    for (enhanced, classic) in [(false, true), (false, false), (true, false)] {
        let lighting = Lighting {
            enhanced,
            classic,
            wetness: 1.0,
            rain: 0.8,
            shadows: false,
            fog_density: 0.0,
            ..Default::default()
        };
        let rgba = renderer
            .render_to_image(&mut scene, SIZE as u32, SIZE as u32, &camera, &lighting)
            .unwrap();
        let middle = mean(&rgba, inner.clone(), inner.clone());
        for (side, xs, ys) in [
            ("left", outer.clone(), inner.clone()),
            ("right", far.clone(), inner.clone()),
            ("top", inner.clone(), outer.clone()),
            ("bottom", inner.clone(), far.clone()),
        ] {
            let edge = mean(&rgba, xs, ys);
            assert!(
                (edge - middle).abs() < 5.0,
                "the wet glass at the {side} edge of the screen differs from the middle (enhanced {enhanced}, classic {classic}): edge {edge:.1}, middle {middle:.1}"
            );
        }
    }
}

#[test]
#[ignore = "requires a graphics adapter; renders a wet pane beside a dark frame"]
fn rain_film_drops_do_not_take_the_colour_of_a_frame_beside_the_pane() {
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
    let flat = |renderer: &mut Renderer, scene: &mut Scene, c: f32| {
        renderer.add_material(scene, None, AlphaMode::Opaque, [c, c, c, 1.0], true)
    };
    // a light street far behind the glass, and the bus's dark frame right behind the film
    let street = flat(&mut renderer, &mut scene, 0.8);
    let frame = flat(&mut renderer, &mut scene, 0.02);
    let film = renderer.add_material_extra(
        &mut scene,
        None,
        AlphaMode::Blend,
        [1.0; 4],
        false,
        None,
        None,
        None,
        None,
        [0.0; 3],
        MaterialExtra {
            rain_film: true,
            no_z_write: true,
            ..Default::default()
        },
    );
    let mut quad = |y: f32, (x0, x1): (f32, f32), half: f32, material| {
        let mesh = renderer.add_mesh(
            &mut scene,
            &MeshData {
                positions: vec![
                    Vec3::new(x0, y, -half),
                    Vec3::new(x1, y, -half),
                    Vec3::new(x1, y, half),
                    Vec3::new(x0, y, half),
                ],
                normals: vec![-Vec3::Y; 4],
                uvs: vec![
                    glam::Vec2::new(0.0, 1.0),
                    glam::Vec2::new(1.0, 1.0),
                    glam::Vec2::new(1.0, 0.0),
                    glam::Vec2::new(0.0, 0.0),
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
    quad(10.0, (-40.0, 40.0), 40.0, street);
    // the left half of the picture is the frame; the film covers the frame and the glass alike
    quad(0.16, (-3.0, 0.0), 3.0, frame);
    quad(0.15, (-3.0, 3.0), 3.0, film);
    let camera = Camera {
        position: DVec3::ZERO,
        yaw: 0.0,
        pitch: 0.0,
        roll: 0.0,
        fov_deg: 70.0,
        near: 0.1,
        far: 100.0,
    };
    const SIZE: usize = 256;
    let mean = |rgba: &[u8], xs: std::ops::Range<usize>| -> f32 {
        let (mut sum, mut n) = (0.0, 0.0);
        for y in 32..SIZE - 32 {
            for x in xs.clone() {
                let p = &rgba[(y * SIZE + x) * 4..(y * SIZE + x) * 4 + 3];
                sum += p.iter().map(|&c| c as f32).sum::<f32>() / 3.0;
                n += 1.0;
            }
        }
        sum / n
    };
    let half = SIZE / 2;
    for (enhanced, classic) in [(false, true), (false, false), (true, false)] {
        let lighting = Lighting {
            enhanced,
            classic,
            wetness: 1.0,
            rain: 0.8,
            shadows: false,
            fog_density: 0.0,
            ..Default::default()
        };
        let rgba = renderer
            .render_to_image(&mut scene, SIZE as u32, SIZE as u32, &camera, &lighting)
            .unwrap();
        // the wet glass next to the frame, against the wet glass well away from it
        let (beside, open) = (
            mean(&rgba, half + 4..half + 36),
            mean(&rgba, SIZE - 40..SIZE - 8),
        );
        assert!(
            open - beside < 4.0,
            "drops beside the frame take its colour (enhanced {enhanced}, classic {classic}): beside {beside:.1}, away {open:.1}"
        );
        // and the wet frame next to the glass, against the wet frame away from it
        let (beside, away) = (mean(&rgba, half - 36..half - 4), mean(&rgba, 8..40));
        assert!(
            beside - away < 4.0,
            "drops on the frame take the street's colour beside the glass (enhanced {enhanced}, classic {classic}): beside {beside:.1}, away {away:.1}"
        );
    }
}

#[test]
#[ignore = "requires a graphics adapter; renders wet glass that writes depth"]
fn rain_film_drops_bend_the_view_through_glass_that_writes_depth() {
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
    const SIZE: usize = 256;
    let mut render = |no_z_write: bool| -> Vec<u8> {
        let renderer = &mut renderer;
        let mut scene = renderer.new_scene();
        let flat = |renderer: &mut Renderer, scene: &mut Scene, c: f32| {
            renderer.add_material(scene, None, AlphaMode::Opaque, [c, c, c, 1.0], true)
        };
        let dark = flat(renderer, &mut scene, 0.05);
        let light = flat(renderer, &mut scene, 0.9);
        let glass = renderer.add_material_extra(
            &mut scene,
            None,
            AlphaMode::Blend,
            [0.5, 0.5, 0.5, 0.3],
            false,
            None,
            None,
            None,
            None,
            [0.0; 3],
            MaterialExtra {
                glass: true,
                no_z_write,
                ..Default::default()
            },
        );
        let film = renderer.add_material_extra(
            &mut scene,
            None,
            AlphaMode::Blend,
            [1.0; 4],
            false,
            None,
            None,
            None,
            None,
            [0.0; 3],
            MaterialExtra {
                rain_film: true,
                no_z_write: true,
                ..Default::default()
            },
        );
        let mut quad = |y: f32, (x0, x1): (f32, f32), half: f32, material| {
            let mesh = renderer.add_mesh(
                &mut scene,
                &MeshData {
                    positions: vec![
                        Vec3::new(x0, y, -half),
                        Vec3::new(x1, y, -half),
                        Vec3::new(x1, y, half),
                        Vec3::new(x0, y, half),
                    ],
                    normals: vec![-Vec3::Y; 4],
                    uvs: vec![
                        glam::Vec2::new(0.0, 1.0),
                        glam::Vec2::new(1.0, 1.0),
                        glam::Vec2::new(1.0, 0.0),
                        glam::Vec2::new(0.0, 0.0),
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
        // a striped street behind the glass, so that a bent view shows
        for i in -20..20 {
            let x = i as f32 * 2.0;
            let stripe = if i % 2 == 0 { dark } else { light };
            quad(10.0, (x, x + 2.0), 40.0, stripe);
        }
        quad(0.16, (-3.0, 3.0), 3.0, glass);
        quad(0.15, (-3.0, 3.0), 3.0, film);
        let camera = Camera {
            position: DVec3::ZERO,
            yaw: 0.0,
            pitch: 0.0,
            roll: 0.0,
            fov_deg: 70.0,
            near: 0.1,
            far: 100.0,
        };
        let lighting = Lighting {
            wetness: 1.0,
            rain: 0.8,
            shadows: false,
            fog_density: 0.0,
            ..Default::default()
        };
        renderer
            .render_to_image(&mut scene, SIZE as u32, SIZE as u32, &camera, &lighting)
            .unwrap()
    };
    let (plain, writing) = (render(true), render(false));
    let diff = plain
        .iter()
        .zip(&writing)
        .map(|(&a, &b)| (a as f32 - b as f32).abs())
        .sum::<f32>()
        / plain.len() as f32;
    assert!(
        diff < 0.5,
        "glass that writes depth changes the drops on it: mean difference {diff:.2}"
    );
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
            &::texture::Image {
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
    let inward = blended(
        &mut scene,
        None,
        MaterialExtra {
            normals_inward: true,
            ..Default::default()
        },
    );
    assert_eq!(scene.materials[inward].uniform.params2[3], 8.0);
    assert!(!scene.materials[inward].transmap_declared());
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

#[test]
#[ignore = "requires a graphics adapter; checks the fog along an articulated bus's saloon"]
fn no_fog_down_the_saloon_of_an_articulated_bus() {
    let adapter = wgpu::Instance::new(wgpu::InstanceDescriptor::new_without_display_handle());
    let mut renderer = pollster::block_on(Renderer::new_with(
        &adapter,
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
    let texture = renderer.add_texture(
        &mut scene,
        &::texture::Image::solid([40, 40, 40, 255]),
        false,
    );
    let material = renderer.add_material(
        &mut scene,
        Some(texture),
        AlphaMode::Opaque,
        [1.0; 4],
        false,
    );
    // the rear car's back wall, 16 m down the aisle
    let mesh = renderer.add_mesh(
        &mut scene,
        &MeshData {
            positions: vec![
                Vec3::new(-1.0, -16.0, 0.5),
                Vec3::new(1.0, -16.0, 0.5),
                Vec3::new(1.0, -16.0, 2.5),
                Vec3::new(-1.0, -16.0, 2.5),
            ],
            normals: vec![Vec3::Y; 4],
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
    );
    let camera = Camera {
        position: DVec3::new(0.0, 0.0, 1.5),
        yaw: 180.0,
        pitch: 0.0,
        roll: 0.0,
        fov_deg: 30.0,
        near: 0.1,
        far: 100.0,
    };
    let clear = Lighting {
        sun_dir: Vec3::Z,
        sun_intensity: 1.0,
        shadows: false,
        detail: false,
        fog_density: 0.0,
        ..Default::default()
    };
    // a 150 m fog; the front car's box spans y -6..6, the rear car's -19.5..-7.5 (the joint
    // between them is 1.5 m long)
    let front_car = Lighting {
        fog_color: Vec3::splat(0.8),
        fog_density: 2.3 / 150.0,
        inside: Some((DVec3::ZERO, 0.0, [2.5, 12.0, 3.0, 0.0, 0.0, 1.5])),
        ..clear.clone()
    };
    let whole_bus = Lighting {
        puddle_parts: vec![(
            DVec3::new(0.0, -13.5, 0.0),
            0.0,
            [2.5, 12.0, 3.0, 0.0, 0.0, 1.5],
        )],
        ..front_car.clone()
    };
    // a [boundingbox] that ends 0.15 m below the line of sight, as one under the ceiling
    let low = [2.5, 12.0, 2.0, 0.0, 0.0, 0.35];
    let low_boxes = Lighting {
        inside: Some((DVec3::ZERO, 0.0, low)),
        puddle_parts: vec![(DVec3::new(0.0, -13.5, 0.0), 0.0, low)],
        ..front_car.clone()
    };
    let mut centre = |lighting: &Lighting| -> i32 {
        let rgba = renderer
            .render_to_image(&mut scene, 64, 64, &camera, lighting)
            .unwrap();
        rgba[(32 * 64 + 32) * 4] as i32
    };
    let (clear, front_car, whole_bus, low_boxes) = (
        centre(&clear),
        centre(&front_car),
        centre(&whole_bus),
        centre(&low_boxes),
    );
    assert!(
        front_car - clear > 10,
        "fog over the rear car's 10 m: {clear} -> {front_car}"
    );
    assert!(
        (whole_bus - clear).abs() <= 2,
        "no fog inside the bus: {clear} -> {whole_bus}"
    );
    assert!(
        (low_boxes - clear).abs() <= 2,
        "no fog under the ceiling: {clear} -> {low_boxes}"
    );
}

#[test]
#[ignore = "requires a graphics adapter; run with --ignored on a GPU host"]
fn an_ordered_vehicle_leaves_the_ground_behind_its_hidden_parts() {
    ordered_vehicle_over_the_ground(4);
}

#[test]
#[ignore = "requires a graphics adapter; run with --ignored on a GPU host"]
fn an_ordered_vehicle_leaves_the_ground_behind_its_hidden_parts_without_msaa() {
    ordered_vehicle_over_the_ground(1);
}

/// One renderer per test: the pipeline cache belongs to the first device.
fn ordered_vehicle_over_the_ground(msaa: u32) {
    // A shadow blob (blended, writing depth) drawn before a wheel behind it: the ground
    // must still show through the blob (#292).
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
    let instance = wgpu::Instance::new(wgpu::InstanceDescriptor::new_without_display_handle());
    let mut renderer = pollster::block_on(Renderer::new_with(
        &instance,
        None,
        Some(wgpu::TextureFormat::Rgba8UnormSrgb),
        RenderOptions {
            msaa,
            ssao: true,
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
    let red = renderer.add_material(
        &mut scene,
        None,
        AlphaMode::Opaque,
        [1.0, 0.0, 0.0, 1.0],
        true,
    );
    let texture = renderer.add_texture(
        &mut scene,
        &::texture::Image {
            width: 1,
            height: 1,
            rgba: vec![255, 255, 255, 0],
            has_alpha: true,
        },
        false,
    );
    let blob_material =
        renderer.add_material(&mut scene, Some(texture), AlphaMode::Blend, [1.0; 4], true);
    let ground = quad(&renderer, &mut scene, 6.0, 10.0);
    renderer.add_instance(&mut scene, ground, DVec3::ZERO, Mat4::IDENTITY, vec![green]);
    let blob = quad(&renderer, &mut scene, 3.0, 1.5);
    let blob = renderer.add_instance(
        &mut scene,
        blob,
        DVec3::ZERO,
        Mat4::IDENTITY,
        vec![blob_material],
    );
    renderer.set_ordered(&mut scene, blob, 0);
    let wheel = quad(&renderer, &mut scene, 4.0, 0.5);
    let wheel = renderer.add_instance(&mut scene, wheel, DVec3::ZERO, Mat4::IDENTITY, vec![red]);
    renderer.set_ordered(&mut scene, wheel, 1);
    let lighting = Lighting {
        enhanced: true,
        shadows: false,
        fog_density: 0.0,
        ..Default::default()
    };
    let rgba = renderer
        .render_to_image(&mut scene, 64, 64, &camera, &lighting)
        .unwrap();
    let centre: [u8; 3] = rgba[(32 * 64 + 32) * 4..(32 * 64 + 32) * 4 + 3]
        .try_into()
        .unwrap();
    assert!(
        centre[1] > centre[0] + 40 && centre[1] > centre[2] + 40,
        "the ground shows behind the blob: {centre:?}; msaa {msaa}"
    );
}

#[test]
fn only_wall_triangles_go_into_the_early_depth_pass() {
    // one slot holding a road climbing 5 m, a kerb at each end of it and a 30 m facade
    let data = MeshData {
        positions: vec![
            Vec3::new(0.0, 0.0, 0.0),
            Vec3::new(10.0, 0.0, 0.0),
            Vec3::new(0.0, 50.0, 5.0),
            Vec3::new(0.0, 0.0, 0.0),
            Vec3::new(0.0, 0.0, 0.15),
            Vec3::new(10.0, 0.0, 0.0),
            Vec3::new(0.0, 50.0, 5.0),
            Vec3::new(0.0, 50.0, 5.15),
            Vec3::new(10.0, 50.0, 5.0),
            Vec3::new(0.0, 0.0, 0.0),
            Vec3::new(0.0, 0.0, 30.0),
            Vec3::new(10.0, 0.0, 0.0),
        ],
        normals: vec![Vec3::Z; 12],
        uvs: vec![glam::Vec2::ZERO; 12],
        ranges: vec![(0, 12, 0)],
        indices: (0..12).collect(),
        one_sided: true,
    };
    let (indices, walls) = upload_indices(&data);
    assert_eq!(&indices[..12], &data.indices[..]);
    assert_eq!(walls.len(), 1);
    let w = walls[0];
    assert_eq!(&indices[w.first as usize..(w.first + w.count) as usize], &[9, 10, 11]);
    assert_eq!(w.height, 30.0);
    for t in [Mat4::IDENTITY, Mat4::from_rotation_z(1.0)] {
        assert!(surface_wall(&walls, t, 0));
    }
    // a facade scaled down to 1.5 m, and a range the mesh does not have
    assert!(!surface_wall(&walls, Mat4::from_scale(Vec3::splat(0.05)), 0));
    assert!(!surface_wall(&walls, Mat4::IDENTITY, 1));

    // the slot's own draw keeps all of it; the wall draw (ranges.len() + 0) only the facade
    let renderer = super::instances::noop_renderer();
    let mut scene = renderer.new_scene();
    let id = renderer.add_mesh(&mut scene, &data);
    let mesh = &scene.meshes[id];
    assert_eq!(mesh.index_range(0), (0, 12));
    assert_eq!(mesh.index_range(1), (12, 3));

    // without walls: nothing extra
    let road = MeshData {
        ranges: vec![(0, 9, 0)],
        indices: (0..9).collect(),
        ..data
    };
    let (indices, walls) = upload_indices(&road);
    assert_eq!(indices.len(), 9);
    assert!(walls.is_empty());
    assert!(!surface_wall(&walls, Mat4::IDENTITY, 0));
}
