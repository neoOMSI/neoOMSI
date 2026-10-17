use crate::*;

#[test]
fn glass_shader_requires_shading_profile() {
    let source = scene_shader_source();
    let without_marker = source.replace("const SHADING_PROFILE_ENABLED: bool = true;", "");
    assert_ne!(source, without_marker);
    let error = wgpu::naga::front::wgsl::parse_str(&without_marker).unwrap_err();
    assert!(error.emit_to_string(&without_marker).contains("SHADING_PROFILE_ENABLED"));
}

#[test]
fn shaders_validate_and_match_the_uniforms() {
    use wgpu::naga;
    let modules = [
        ("scene", scene_shader_source()),
        ("sky", sky_shader_source()),
        ("corona", corona_shader_source()),
        (
            "post",
            include_str!("../../shaders/post/post.wgsl").to_string(),
        ),
        (
            "ssao",
            include_str!("../../shaders/post/ssao.wgsl").to_string(),
        ),
        ("puddles", puddles::shader_source()),
        (
            "upscale",
            include_str!("../../shaders/post/upscale.wgsl").to_string(),
        ),
        (
            "mip",
            include_str!("../../shaders/post/mip.wgsl").to_string(),
        ),
        (
            "xr_ui",
            include_str!("../../shaders/ui/xr_ui.wgsl").to_string(),
        ),
    ];
    let sizes: &[(&str, usize)] = &[
        ("Enhanced", std::mem::size_of::<EnhancedUniform>()),
        ("PostParams", std::mem::size_of::<PostUniform>()),
        ("PuddleParams", std::mem::size_of::<puddles::Uniform>()),
        (
            "VehicleReflection",
            std::mem::size_of::<puddles::VehicleUniform>(),
        ),
        ("PointLight", std::mem::size_of::<GpuPointLight>()),
        ("Camera", std::mem::size_of::<CameraUniform>()),
        ("MaterialParams", std::mem::size_of::<MaterialUniform>()),
    ];
    let mut checked = std::collections::HashSet::new();
    for (name, src) in &modules {
        let module = naga::front::wgsl::parse_str(src)
            .unwrap_or_else(|e| panic!("{name}: {}", e.emit_to_string(src)));
        let info = naga::valid::Validator::new(
            naga::valid::ValidationFlags::all(),
            naga::valid::Capabilities::all(),
        )
        .validate(&module)
        .unwrap_or_else(|e| panic!("{name}: {e:?}"));
        let (module, info) = naga::back::pipeline_constants::process_overrides(
            &module,
            &info,
            None,
            &Default::default(),
        )
        .unwrap_or_else(|e| panic!("{name}: overrides: {e:?}"));
        let (module, info) = (module.into_owned(), info.into_owned());
        #[cfg(target_os = "macos")]
        let options = naga::back::msl::Options {
            lang_version: (2, 4),
            ..Default::default()
        };
        #[cfg(target_os = "macos")]
        naga::back::msl::write_string(
            &module,
            &info,
            &options,
            &naga::back::msl::PipelineOptions::default(),
        )
        .unwrap_or_else(|e| panic!("{name}: Metal: {e:?}"));
        for entry in &module.entry_points {
            let pipeline = naga::back::spv::PipelineOptions {
                shader_stage: entry.stage,
                entry_point: entry.name.clone(),
            };
            naga::back::spv::write_vec(&module, &info, &Default::default(), Some(&pipeline))
                .unwrap_or_else(|e| panic!("{name}/{}: Vulkan: {e:?}", entry.name));
        }
        let mut layouter = naga::proc::Layouter::default();
        layouter.update(module.to_ctx()).expect("layout");
        for (ty_name, rust) in sizes {
            if *ty_name == "Camera" && (*name == "corona" || *name == "sky") {
                if let Some((h, _)) = module
                    .types
                    .iter()
                    .find(|(_, t)| t.name.as_deref() == Some(*ty_name))
                {
                    let prefix = if *name == "corona" {
                        std::mem::offset_of!(CameraUniform, clouds)
                    } else {
                        std::mem::offset_of!(CameraUniform, spot_vp)
                    };
                    assert_eq!(layouter[h].size as usize, prefix, "{name}: Camera prefix");
                }
                continue;
            }
            if let Some((h, _)) = module
                .types
                .iter()
                .find(|(_, t)| t.name.as_deref() == Some(*ty_name))
            {
                assert_eq!(layouter[h].size as usize, *rust, "{name}: {ty_name}");
                checked.insert(*ty_name);
            }
        }
    }
    assert_eq!(checked.len(), sizes.len(), "structs checked: {checked:?}");
}

#[test]
fn the_sky_is_recomputed_only_when_it_has_moved_on() {
    let a = atmosphere::SkyInput::default();
    assert!(!sky_input_differs(&a, &a));
    let turn = |deg: f32| {
        glam::Quat::from_axis_angle(a.sun_dir.any_orthonormal_vector(), deg.to_radians())
            * a.sun_dir
    };
    assert!(!sky_input_differs(
        &a,
        &atmosphere::SkyInput {
            sun_dir: turn(0.01),
            tint: [Vec3::splat(1.001); 3],
            ..a
        }
    ));
    assert!(sky_input_differs(
        &a,
        &atmosphere::SkyInput {
            sun_dir: turn(0.25),
            ..a
        }
    ));
    assert!(sky_input_differs(
        &a,
        &atmosphere::SkyInput { rain: 0.3, ..a }
    ));
    assert!(sky_input_differs(
        &a,
        &atmosphere::SkyInput {
            tint: [Vec3::new(1.2, 1.0, 0.9), Vec3::ONE, Vec3::ONE],
            ..a
        }
    ));
}

#[test]
fn each_path_gets_its_own_headlights() {
    let lamp = PointLight {
        radius: 30.0,
        color: [1.0, 0.78, 0.46],
        core: 5.0,
        ..Default::default()
    };
    let stand_in = PointLight {
        radius: 18.0,
        intensity: 0.8,
        mode: LightMode::Vanilla,
        ..Default::default()
    };
    let spot = PointLight {
        radius: 60.0,
        intensity: 20.0,
        direction: Vec3::new(0.0, 2.0, -0.6),
        cone: [0.97, 0.82],
        core: 1.0,
        beam: 24.0,
        mode: LightMode::Enhanced,
        ..Default::default()
    };
    assert!(drawn_by(&lamp, false) && drawn_by(&lamp, true));
    assert!(drawn_by(&stand_in, false) && !drawn_by(&stand_in, true));
    assert!(!drawn_by(&spot, false) && drawn_by(&spot, true));
    assert!(!drawn_by(
        &PointLight {
            intensity: 0.0,
            ..lamp
        },
        true
    ));
    let g = gpu_light(&lamp, Vec3::new(1.0, 2.0, 3.0));
    assert_eq!(g.pos, [1.0, 2.0, 3.0, 30.0]);
    assert_eq!(g.color, [1.0, 0.78, 0.46, 1.0]);
    assert_eq!(g.dir[3], -2.0);
    assert_eq!(g.extra[1..], [5.0, 0.0, 30.0]);
    let g = gpu_light(&spot, Vec3::ZERO);
    assert_eq!(g.pos[3], 0.0);
    assert_eq!(g.extra[3], 60.0);
    assert!(
        (Vec3::from_slice(&g.dir[..3]).length() - 1.0).abs() < 1e-5
            && g.dir[3] == 0.82
            && g.extra[0] == 0.97
    );
    assert_eq!(g.extra[2], 24.0);
    assert_eq!(gpu_light(&lamp, Vec3::ZERO).extra[2], 0.0);
}

#[test]
fn metering_defaults_are_gentle() {
    let m = meter_tuning();
    assert!(m[0] > 0.0 && m[0] < 0.6, "{m:?}");
    assert!(m[2] <= 0.75 && m[3] <= 1.0, "{m:?}");
    let ev = ((m[1] - (m[1] + 1.5)) * m[0]).clamp(-m[2], m[3]);
    assert!(ev < 0.0 && ev >= -0.75, "{ev}");
}

#[test]
fn half_floats_read_back() {
    for v in [0.0f32, 1.0, -2.5, 0.5, 1e-3, -3.46] {
        assert!(
            (half_to_f32(atmosphere::f16_bits(v)) - v).abs() <= v.abs() * 1e-3 + 1e-6,
            "{v}"
        );
    }
}

#[test]
fn trailer_fog_box_reaches_the_car_ahead() {
    // O530 G: the front car's box ends 0.75 m before the coupling, the rear car's 0.75 m after it
    let front = [2.55, 9.565, 2.0, 0.0, 1.191, 1.379];
    let rear = [2.55, 7.019, 2.0, 0.0, 0.111, 1.379];
    for heading in [0.0, 20.0, 200.0] {
        let (sh, ch) = f64::to_radians(heading).sin_cos();
        let o = DVec3::new(500.0, 700.0, 9.0);
        let t = o - DVec3::new(sh, ch, 0.0) * 8.706;
        let b = trailer_fog_boxes(Some((o, heading, front)), &[(t, heading, rear)], o);
        // front face at the front car's back face: 1.191 - 9.565 / 2 = -3.5915 from its origin
        let (half, centre) = (b[1][2], b[2][1]);
        assert!(
            ((centre + half) - (8.706 - 3.5915) as f32).abs() < 1e-3,
            "{heading}: {b:?}"
        );
        assert!(
            ((centre - half) - (0.111 - 3.5095)).abs() < 1e-3,
            "{heading}: {b:?}"
        );
        assert_eq!(b[5][3], 0.0);
    }
    // a drawbar trailer 4 m behind keeps its box: the air between is outside
    let t = DVec3::new(0.0, -8.706 - 4.0, 0.0);
    let b = trailer_fog_boxes(
        Some((DVec3::ZERO, 0.0, front)),
        &[(t, 0.0, rear)],
        DVec3::ZERO,
    );
    assert_eq!((b[1][2], b[2][1]), (rear[1] * 0.5, rear[4]));
}
