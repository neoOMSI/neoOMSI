use super::*;

#[test]
fn null_texture_names() {
    assert!(is_null_texture("null.bmp"));
    assert!(is_null_texture(" NULL.BMP "));
    assert!(is_null_texture("texture\\null.tga"));
    assert!(is_null_texture(""));
    assert!(!is_null_texture("D_Matrix.bmp"));
    assert!(!is_null_texture("nullschild.bmp"));
}

#[test]
fn d3d_material_colours() {
    // a Blender export: grey diffuse with alpha 0, a bogus specular, no emissive
    let m = ::legacy_o3d::Material {
        diffuse: [0.64, 0.64, 0.64, 0.0],
        specular: [255.0, 255.0, 355.0],
        emissive: [0.0; 3],
        specular_power: 96.0,
        texture: "int_glass.tga".into(),
    };
    let (color, emissive, specular, ambient) = d3d_material(&m, None, true);
    // Textured o3d materials retain their authored texture colours. In particular, the
    // Bowdenham white street-sign mesh carries a green diffuse value which OMSI ignores.
    assert_eq!(color, [1.0; 4]);
    assert_eq!(emissive, [0.0; 3]);
    assert_eq!(specular, [1.0, 1.0, 1.0, 96.0]);
    // Omsi.exe's o3d slot: a white ambient, whatever the diffuse colour (0x7c62f8)
    assert_eq!(ambient, [1.0; 3]);
    // untextured: the material's alpha
    assert_eq!(d3d_material(&m, None, false).0[3], 0.0);
    // no specular colour, no highlight whatever the power
    let plain = ::legacy_o3d::Material {
        specular_power: 25.0,
        ..Default::default()
    };
    assert_eq!(d3d_material(&plain, None, true).2[3], 0.0);
    // a lit display: emissive white
    let lcd = ::legacy_o3d::Material {
        emissive: [1.0; 3],
        ..Default::default()
    };
    assert_eq!(d3d_material(&lcd, None, true).1, [1.0; 3]);
    // [matl_allcolor] replaces the o3d material (the stock lower-deck lighting item)
    let all = [
        1.0, 1.0, 1.0, 1.0, 1.0, 1.0, 1.0, 0.0, 0.0, 0.0, 0.24, 0.23, 0.2, 0.0,
    ];
    let (c, e, s, _) = d3d_material(&m, Some(all), true);
    assert_eq!(c, [1.0; 4]);
    // [matl_allcolor]'s own ambient
    let mut dim = all;
    dim[4..7].copy_from_slice(&[0.2, 0.3, 0.4]);
    assert_eq!(d3d_material(&m, Some(dim), true).3, [0.2, 0.3, 0.4]);
    assert_eq!(e, [0.24, 0.23, 0.2]);
    assert_eq!(s[3], 0.0);
}

/// A second `[matl]` of the same slot with `[matl_alpha] 1` makes the slot
/// alpha-tested; a later `[matl_alpha] 0` makes it opaque again,
/// and a later `[matl]` without one keeps the mode.
#[test]
fn later_matl_of_the_same_slot_sets_its_alpha() {
    let mats = [::legacy_o3d::Material {
        texture: "Chain.dds".into(),
        ..Default::default()
    }];
    let def = |alpha: Option<i32>| MaterialDef {
        texture: "chain.dds".into(),
        alpha: alpha.unwrap_or(0),
        alpha_set: alpha.is_some(),
        ..Default::default()
    };
    assert_eq!(
        material_alpha(&mats, 0, &[def(None), def(Some(1))]),
        AlphaMode::Test
    );
    assert_eq!(
        material_alpha(&mats, 0, &[def(Some(1)), def(None)]),
        AlphaMode::Test
    );
    assert_eq!(
        material_alpha(&mats, 0, &[def(Some(2)), def(Some(0))]),
        AlphaMode::Opaque
    );
    assert_eq!(
        material_alpha(&mats, 0, &[def(None), def(None)]),
        AlphaMode::Opaque
    );
}

#[test]
fn transmap_mask_does_not_make_opaque_body_blend() {
    let mats = [::legacy_o3d::Material {
        texture: "body.tga".into(),
        ..Default::default()
    }];
    let defs = [
        MaterialDef {
            texture: "body.tga".into(),
            index: 0,
            alpha: 0,
            transmap: Some("body_mask.tga".into()),
            ..Default::default()
        },
        MaterialDef {
            texture: "body.tga".into(),
            index: 0,
            alpha: 0,
            alphascale: Some("Rain_Window_Front_Wetness".into()),
            ..Default::default()
        },
    ];
    let alpha = material_alpha(&mats, 0, &defs);
    assert_eq!(alpha, AlphaMode::Opaque);
    assert_eq!(Renderer::clamp_slot_alpha(0.35, alpha, false), 1.0);
}

#[test]
fn blended_body_alpha_repair_does_not_touch_glass() {
    assert!(is_vehicle_body_material(
        "12m/wagenkasten_embl_eev.o3d",
        "01white_FL.tga",
        true,
        false,
        false,
        true
    ));
    assert!(is_vehicle_body_material(
        "A21_EEV/body.o3d",
        "a21_body.png",
        true,
        false,
        false,
        true
    ));
    assert!(is_vehicle_body_material(
        "Exterior/unnamed_shell.o3d",
        "paint.tga",
        true,
        false,
        false,
        true
    ));
    assert!(!is_vehicle_body_material(
        "A21/windows.o3d",
        "a21_body_windows.png",
        true,
        false,
        false,
        false
    ));
    assert!(!is_vehicle_body_material(
        "Exterior/unnamed_window.o3d",
        "glass.tga",
        true,
        false,
        false,
        true
    ));
    assert!(!is_vehicle_body_material(
        "12m/wagenkasten.o3d",
        "01white_FL.tga",
        true,
        true,
        false,
        true
    ));
    assert!(!is_vehicle_body_material(
        "12m/wagenkasten.o3d",
        "01white_FL.tga",
        true,
        false,
        true,
        true
    ));
}

/// The ICU400 controller's screen layer: a script texture as its transmap declares one.
#[test]
fn script_transmap_is_declared() {
    let text = "[mesh]\nscreen.o3d\n\n[matl]\nScreen.dds\n0\n[matl_transmap]\n\\S:1\n[alphascale]\nsignController_alphaScale\n[matl_alpha]\n2\n\n[matl]\nPlain.dds\n0\n";
    let m = ::model::Model::parse(&::legacy_config::CfgFile::from_str("model.cfg", text));
    let mats = &m.meshes[0].materials;
    let screen = mats.iter().find(|d| d.texture == "Screen.dds").unwrap();
    let plain = mats.iter().find(|d| d.texture == "Plain.dds").unwrap();
    assert_eq!(screen.transmap.as_deref(), Some("\\S:1"));
    assert!(material_extra(&[screen], None, None, [0.0; 4]).transmap_declared);
    assert!(!material_extra(&[plain], None, None, [0.0; 4]).transmap_declared);
}

#[test]
fn material_extra_from_commands() {
    let glass = MaterialDef {
        texture: "wischwasser.tga".into(),
        alpha: 2,
        no_z_write: true,
        no_z_check: true,
        ..Default::default()
    };
    let decal = MaterialDef {
        texture: "bw_jul_mod5.bmp".into(),
        z_bias: 16,
        ..Default::default()
    };
    let e = material_extra(
        &[&glass, &decal],
        Some(7),
        Some((3, 0.1)),
        [0.2, 0.2, 0.2, 10.0],
    );
    assert!(e.no_z_write && !e.no_z_check);
    assert_eq!(e.z_bias, 16);
    assert_eq!(e.env_mask, Some(7));
    assert_eq!(e.specular, [0.2, 0.2, 0.2, 10.0]);
    assert_eq!(e.bump, Some((3, 0.1)));
    assert_eq!(
        material_extra(&[], None, None, [0.0; 4]),
        MaterialExtra::default()
    );
    // a factor of 0 moves nothing: no bump map to sample
    assert_eq!(
        material_extra(&[], None, Some((3, 0.0)), [0.0; 4]).bump,
        None
    );
    // [matl_texadress_border]: the colour in bytes, as 0..1
    let roller = MaterialDef {
        texture: "rlb_512.tga".into(),
        tex_address: ::model::TexAddress::Border,
        border_color: [255.0, 255.0, 255.0, 0.0],
        ..Default::default()
    };
    assert_eq!(
        material_extra(&[&roller], None, None, [0.0; 4]).border,
        Some([1.0, 1.0, 1.0, 0.0])
    );
    let clamped = MaterialDef {
        tex_address: ::model::TexAddress::Clamp,
        ..roller.clone()
    };
    assert_eq!(
        material_extra(&[&roller, &clamped], None, None, [0.0; 4]).border,
        None
    );
    // the slot's last addressing command decides how its textures repeat
    use ::render::TexAddressing as R;
    let mirror = MaterialDef {
        tex_address: ::model::TexAddress::Mirror,
        ..roller.clone()
    };
    let once = MaterialDef {
        tex_address: ::model::TexAddress::MirrorOnce,
        ..roller.clone()
    };
    let plain = MaterialDef::default();
    assert_eq!(tex_addressing([&plain].into_iter()), R::Wrap);
    assert_eq!(
        tex_addressing([&clamped, &mirror, &plain].into_iter()),
        R::Mirror
    );
    assert_eq!(tex_addressing([&mirror, &once].into_iter()), R::MirrorOnce);
    assert_eq!(tex_addressing([&once, &roller].into_iter()), R::Clamp);
}

#[test]
fn scenery_freetex_name_resolution() {
    let ov1 = MaterialDef {
        freetex: Some(("placeholder.bmp".into(), "Textur".into())),
        ..Default::default()
    };
    let ov2 = MaterialDef {
        freetex: Some(("placeholder2.bmp".into(), "Textur2".into())),
        ..Default::default()
    };
    let overrides = vec![ov1.clone(), ov2.clone()];

    // 1. Script variable takes precedence when available
    let mut prog = ::legacy_script::Program::default();
    prog.declare_str_var("Textur");
    let script = ::simulation::scenery::SceneryInstance::new(
        Arc::new(prog),
        &[],
        ::simulation::SimClock::default(),
        &["from_script.bmp".into()],
    );
    let from_strings = vec!["from_strings.bmp".to_string()];
    let name = resolve_scenery_freetex_name(
        "Textur",
        &ov1,
        &overrides,
        Some(&script),
        None,
        &from_strings,
    );
    assert_eq!(name, Some("from_script.bmp"));

    // 2. Fallback to strings by explicit numeric index (e.g. var = "1")
    let strings = vec!["zero.bmp".to_string(), "\"quoted_one.bmp\"".to_string()];
    let name = resolve_scenery_freetex_name("1", &ov1, &overrides, None, None, &strings);
    assert_eq!(name, Some("quoted_one.bmp"));

    // 3. Fallback to strings by freetex declaration order
    let name_first =
        resolve_scenery_freetex_name("Textur", &overrides[0], &overrides, None, None, &strings);
    assert_eq!(name_first, Some("zero.bmp"));
    let name_second = resolve_scenery_freetex_name(
        "Textur2",
        &overrides[1],
        &overrides,
        None,
        None,
        &strings,
    );
    assert_eq!(name_second, Some("quoted_one.bmp"));

    // 4. Returns None when no matching string exists
    let name_empty = resolve_scenery_freetex_name("Missing", &ov1, &overrides, None, None, &[]);
    assert_eq!(name_empty, None);
}

fn noop_renderer() -> Renderer {
    let mut descriptor = wgpu::InstanceDescriptor::new_without_display_handle();
    descriptor.backends = wgpu::Backends::NOOP;
    descriptor.backend_options.noop = wgpu::NoopBackendOptions::enabled();
    let instance = wgpu::Instance::new(descriptor);
    pollster::block_on(Renderer::new_with(
        &instance,
        None,
        Some(wgpu::TextureFormat::Rgba8UnormSrgb),
        ::render::RenderOptions::default(),
    ))
    .unwrap()
}

#[test]
fn dynamic_scenery_material_transitions_and_retention_lifecycle() {
    let renderer = noop_renderer();
    let mut scene = renderer.new_scene();

    let static_base: MaterialId = 1;
    let static_item: MaterialId = 2;
    let mesh = renderer.add_mesh(&mut scene, &::geometry::MeshData::default());
    let inst_idx = renderer.add_instance(
        &mut scene,
        mesh,
        DVec3::ZERO,
        Mat4::IDENTITY,
        vec![static_base],
    );
    let slot_idx = 0;

    let variants = vec![(inst_idx, slot_idx, static_base, static_item, "SwitchVar".to_string(), vec![])];
    let mut dynamic_materials: HashMap<(usize, usize), Vec<MaterialId>> = HashMap::new();
    let mut current_switch_var = 0.0f32;

    // 1. Initial state before any dynamic texture is loaded:
    apply_scenery_variants(
        &variants,
        &dynamic_materials,
        0.0,
        &|_| Some(current_switch_var),
        &renderer,
        &mut scene,
    );
    assert_eq!(scene.instances[inst_idx].materials[slot_idx], static_base);

    // 2. Dynamic texture variant 1 is applied (e.g. livery 1 loaded):
    let dyn_base_1: MaterialId = 101;
    let dyn_item_1: MaterialId = 102;
    dynamic_materials.insert((inst_idx, slot_idx), vec![dyn_base_1, dyn_item_1]);
    renderer.set_material(&mut scene, inst_idx, slot_idx, dyn_base_1);
    assert_eq!(scene.instances[inst_idx].materials[slot_idx], dyn_base_1);

    // 3. Ticks 2 and 3: Dynamic texture selection is unchanged, switch variable remains 0.0
    for _ in 0..2 {
        apply_scenery_variants(
            &variants,
            &dynamic_materials,
            0.0,
            &|_| Some(current_switch_var),
            &renderer,
            &mut scene,
        );
        assert_eq!(
            scene.instances[inst_idx].materials[slot_idx],
            dyn_base_1,
            "dynamic material must be retained across ticks without resetting to static base"
        );
    }

    // 4. Switch variable turns on (e.g. nightlight or switch activated):
    current_switch_var = 1.0;
    apply_scenery_variants(
        &variants,
        &dynamic_materials,
        0.0,
        &|_| Some(current_switch_var),
        &renderer,
        &mut scene,
    );
    assert_eq!(
        scene.instances[inst_idx].materials[slot_idx],
        dyn_item_1,
        "variant evaluation must switch to dynamic item material when variable triggers"
    );

    // 5. Another tick with switch variable still on:
    apply_scenery_variants(
        &variants,
        &dynamic_materials,
        0.0,
        &|_| Some(current_switch_var),
        &renderer,
        &mut scene,
    );
    assert_eq!(
        scene.instances[inst_idx].materials[slot_idx],
        dyn_item_1,
        "dynamic item material must be retained"
    );

    // 6. Dynamic texture assignment changes (e.g. livery 2 loaded):
    let dyn_base_2: MaterialId = 201;
    let dyn_item_2: MaterialId = 202;
    dynamic_materials.insert((inst_idx, slot_idx), vec![dyn_base_2, dyn_item_2]);
    let item_on = change_picks_item(current_switch_var);
    renderer.set_material(
        &mut scene,
        inst_idx,
        slot_idx,
        if item_on { dyn_item_2 } else { dyn_base_2 },
    );
    assert_eq!(scene.instances[inst_idx].materials[slot_idx], dyn_item_2);

    // 7. Tick with new dynamic texture selection unchanged:
    apply_scenery_variants(
        &variants,
        &dynamic_materials,
        0.0,
        &|_| Some(current_switch_var),
        &renderer,
        &mut scene,
    );
    assert_eq!(scene.instances[inst_idx].materials[slot_idx], dyn_item_2);

    // 8. Switch variable turns back off (0.0):
    current_switch_var = 0.0;
    apply_scenery_variants(
        &variants,
        &dynamic_materials,
        0.0,
        &|_| Some(current_switch_var),
        &renderer,
        &mut scene,
    );
    assert_eq!(
        scene.instances[inst_idx].materials[slot_idx],
        dyn_base_2,
        "variant evaluation must switch back to second dynamic base material"
    );
}
