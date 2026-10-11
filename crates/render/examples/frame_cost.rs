//! Synthetic CPU benchmarks, using wgpu's noop backend (no game assets or GPU timing).
//! Run with: cargo run --profile dev-release -p render --example frame_cost

use glam::{DVec3, Mat4, Vec2, Vec3};
use render::{AlphaMode, Camera, Lighting, RenderOptions, Renderer};
use std::time::Instant;

fn main() -> anyhow::Result<()> {
    let mut descriptor = wgpu::InstanceDescriptor::new_without_display_handle();
    descriptor.backends = wgpu::Backends::NOOP;
    descriptor.backend_options.noop = wgpu::NoopBackendOptions::enabled();
    let instance = wgpu::Instance::new(descriptor);
    let mut renderer = pollster::block_on(Renderer::new_with(
        &instance,
        None,
        Some(wgpu::TextureFormat::Rgba8UnormSrgb),
        RenderOptions {
            msaa: 1,
            ssao: false,
            fxaa: false,
            shadow_size: 1024,
            no_enhanced: true,
            ..Default::default()
        },
    ))?;
    let mut scene = renderer.new_scene();
    let material = renderer.add_material(&mut scene, None, AlphaMode::Opaque, [1.0; 4], true);
    let mut data = geometry::MeshData {
        positions: vec![Vec3::ZERO, Vec3::X, Vec3::Z],
        normals: vec![Vec3::Y; 3],
        uvs: vec![Vec2::ZERO; 3],
        indices: vec![0, 1, 2],
        ranges: vec![(0, 3, 0)],
        ..Default::default()
    };
    let meshes: Vec<_> = (0..4096)
        .map(|_| renderer.add_mesh(&mut scene, &data))
        .collect();
    let output = renderer.add_render_texture(&mut scene, 64, 64);
    let lighting = Lighting {
        shadows: false,
        light_shadows: false,
        ..Default::default()
    };
    let camera = Camera {
        position: DVec3::new(99.5, 0.0, 2.0),
        yaw: 0.0,
        pitch: 0.0,
        roll: 0.0,
        fov_deg: 60.0,
        near: 0.1,
        far: 6000.0,
    };
    for count in [128, 1024, 4096] {
        let started = Instant::now();
        for _ in 0..30 {
            for &mesh in &meshes[..count] {
                renderer.update_mesh(&mut scene, mesh, &data.positions, &data.normals, &data.uvs);
            }
            renderer.render_to_texture(&mut scene, output, &camera, &lighting, 1.0);
        }
        println!(
            "animated uploads {count:4}: {:.3} ms/frame",
            started.elapsed().as_secs_f64() * 1000.0 / 30.0
        );
    }

    data.ranges = (0..4).map(|slot| (0, 3, slot)).collect();
    let mesh = renderer.add_mesh(&mut scene, &data);
    for k in 0..30_000 {
        renderer.add_instance(
            &mut scene,
            mesh,
            DVec3::new(
                (k % 200) as f64 * 10.0,
                -1000.0 - (k / 200) as f64 * 10.0,
                0.0,
            ),
            Mat4::IDENTITY,
            vec![material; 4],
        );
    }
    let target = renderer.device.create_texture(&wgpu::TextureDescriptor {
        label: Some("benchmark output"),
        size: wgpu::Extent3d {
            width: 64,
            height: 64,
            depth_or_array_layers: 1,
        },
        mip_level_count: 1,
        sample_count: 1,
        dimension: wgpu::TextureDimension::D2,
        format: wgpu::TextureFormat::Rgba8UnormSrgb,
        usage: wgpu::TextureUsages::RENDER_ATTACHMENT,
        view_formats: &[],
    });
    let view = target.create_view(&Default::default());
    let mirror = Camera {
        position: camera.position + DVec3::X,
        ..camera
    };
    renderer.render(&mut scene, &view, 64, 64, &camera, &lighting);
    let started = Instant::now();
    for _ in 0..60 {
        renderer.render_to_texture(&mut scene, output, &mirror, &lighting, 1.0);
        renderer.render(&mut scene, &view, 64, 64, &camera, &lighting);
    }
    println!(
        "30,000 instances + boundary mirror: {:.3} ms/frame",
        started.elapsed().as_secs_f64() * 1000.0 / 60.0
    );

    let lighting = Lighting {
        shadows: true,
        ..lighting
    };
    let started = Instant::now();
    for _ in 0..60 {
        renderer.render(&mut scene, &view, 64, 64, &camera, &lighting);
    }
    println!(
        "30,000 distant shadow casters: {:.3} ms/frame",
        started.elapsed().as_secs_f64() * 1000.0 / 60.0
    );
    for transforms in [false, true] {
        let started = Instant::now();
        for frame in 0..60 {
            let value = if frame % 2 == 0 { 0.5 } else { 1.0 };
            for id in (0..scene.instances.len()).step_by(100) {
                if transforms {
                    let origin = scene.instances[id].origin;
                    renderer.set_transform(&mut scene, id, origin, Mat4::from_rotation_z(value));
                } else {
                    renderer.set_slot_light(&mut scene, id, &[value; 4]);
                }
            }
            renderer.prepare(&mut scene);
            renderer.queue.submit([]);
        }
        println!(
            "300 scattered {} updates: {:.3} ms/frame",
            if transforms { "transform" } else { "parameter" },
            started.elapsed().as_secs_f64() * 1000.0 / 60.0
        );
    }
    Ok(())
}
