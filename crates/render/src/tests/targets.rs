use crate::*;

#[test]
fn hdr_depth_does_not_allocate_unused_colour_and_classic_can_add_it() {
    let mut desc = wgpu::InstanceDescriptor::new_without_display_handle();
    desc.backends = wgpu::Backends::NOOP;
    desc.backend_options.noop = wgpu::NoopBackendOptions::enabled();
    let mut renderer = pollster::block_on(Renderer::new_with(
        &wgpu::Instance::new(desc),
        None,
        Some(wgpu::TextureFormat::Rgba8UnormSrgb),
        RenderOptions {
            msaa: 4,
            ssao: false,
            no_enhanced: true,
            shadow_size: 512,
            ..Default::default()
        },
    ))
    .unwrap();
    let (colour, depth) = renderer.msaa_targets(2000, 1125, false);
    assert!(colour.is_none());
    let (colour, same_depth) = renderer.msaa_targets(2000, 1125, true);
    assert!(colour.is_some());
    assert_eq!(depth, same_depth);
    let (again, _) = renderer.msaa_targets(2000, 1125, true);
    assert_eq!(colour, again);
}

#[test]
fn material_uniform_pages_keep_aligned_distinct_ranges_across_rollover_and_rebind() {
    let mut desc = wgpu::InstanceDescriptor::new_without_display_handle();
    desc.backends = wgpu::Backends::NOOP;
    desc.backend_options.noop = wgpu::NoopBackendOptions::enabled();
    let renderer = pollster::block_on(Renderer::new_with(
        &wgpu::Instance::new(desc), None, Some(wgpu::TextureFormat::Rgba8UnormSrgb),
        RenderOptions { msaa: 1, ssao: false, no_enhanced: true, shadow_size: 512, ..Default::default() },
    )).unwrap();
    let mut scene = renderer.new_scene();
    let texture = renderer.add_texture(&mut scene, &::texture::Image {
        width: 4, height: 4, rgba: vec![255; 64], has_alpha: false,
    }, false);
    for i in 0..600 {
        renderer.add_material(&mut scene, Some(texture), AlphaMode::Opaque, [i as f32 / 600.0, 1.0, 1.0, 1.0], true);
    }
    let alignment = renderer.device.limits().min_uniform_buffer_offset_alignment as u64;
    let first = &scene.materials[0];
    let second = &scene.materials[1];
    assert_eq!(first.buf, second.buf);
    assert_ne!(first.buf_offset, second.buf_offset);
    assert_ne!(first.buf, scene.materials[599].buf);
    let bytes = scene.gpu_bytes().2;
    assert!(bytes < 1_000_000, "600 uniforms should occupy a few pages, not 600 allocations: {bytes}");
    let ranges: Vec<_> = scene.materials.iter().map(|m| (m.buf.clone(), m.buf_offset)).collect();
    for m in &scene.materials {
        assert_eq!(m.buf_offset % alignment, 0);
        assert!(m.buf_offset + size_of::<MaterialUniform>() as u64 <= m.buf.size());
    }
    assert_eq!(renderer.rebind_textures(&mut scene, &[texture]), 600);
    for (m, (buf, offset)) in scene.materials.iter().zip(&ranges) {
        assert_eq!(&m.buf, buf);
        assert_eq!(m.buf_offset, *offset);
    }
    // Freed materials get a valid binding; creating another never overwrites an old
    // range still held by a queued frame or render bundle.
    renderer.free_material(&mut scene, 1);
    let new = renderer.add_material(&mut scene, Some(texture), AlphaMode::Opaque, [0.1; 4], true);
    let m = &scene.materials[new];
    assert!(!ranges.iter().any(|(buf, offset)| buf == &m.buf && *offset == m.buf_offset));
}
