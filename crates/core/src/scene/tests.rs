use super::*;

#[test]
fn nightlight_follows_the_objects_darkness_threshold() {
    let day = DayKind {
        workday: true,
        ..Default::default()
    };
    let plain = InUse::new(0, 7);
    assert!(plain.lit(12.0 * 3600.0, day, 0.5));
    assert!(!plain.lit(12.0 * 3600.0, day, 0.65));
    let home = InUse::new(2, 7);
    assert!((0.3..=0.75).contains(&home.threshold));
    assert!(!home.lit(3.0 * 3600.0, day, 0.0));
}

#[test]
fn vehicle_freetex_retries_paths_below_texture_component() {
    let root = std::env::temp_dir().join("neoomsi-freetex-path-test");
    let vehicle_texture = root.join("Vehicles/TestBus/Texture");
    let wanted = vehicle_texture.join("mb_pmon/alerta_FalhaCambio.bmp");
    std::fs::create_dir_all(wanted.parent().unwrap()).unwrap();
    std::fs::write(&wanted, b"x").unwrap();
    let dirs = [vehicle_texture.as_path()];
    let found = find_vehicle_freetex(r"..\Texture\mb_pmon\alerta_FalhaCambio.bmp", &dirs);
    assert_eq!(found.as_deref(), Some(wanted.as_path()));
    let _ = std::fs::remove_dir_all(root);
}

#[test]
fn traffic_light_program_requires_a_placed_signal() {
    let sco = SceneryObject::parse(&::legacy_config::CfgFile::from_str(
        "junction.sco",
        "[traffic_lights_group]\n72\n[traffic_light]\nMain\n[phase]\n0\n10\n[phase]\n6\n62\n",
    ));
    assert!(!traffic_light_program_enabled(&sco, false));
    assert!(traffic_light_program_enabled(&sco, true));
    let gate = SceneryObject {
        is_traffic_light: true,
        ..sco
    };
    assert!(traffic_light_program_enabled(&gate, false));
    assert!(!traffic_light_program_enabled(
        &SceneryObject::default(),
        true
    ));
}

fn freetex_test_look() -> Look {
    Look {
        alpha: AlphaMode::Opaque,
        color: [1.0; 4],
        emissive: [0.0; 3],
        unlit: false,
        diffuse: None,
        transmap: None,
        night: None,
        lightmap: None,
        envmap: None,
        extra: MaterialExtra::default(),
        dyn_tex: DynTex::default(),
    }
}

/// An LED panel's light map is one white pixel; a flipdot's is a picture with dark
/// parts (the Krueger's `vmatrix_leer_LM.bmp`), and does not make an LED panel (#413).
#[test]
fn a_lamps_lenses_follow_its_alphascale_and_light_map_variables() {
    // three lenses on one mesh, each faded by its colour's variable and lit by its
    // light map (Cheongsan's signals, #826); a fourth slot switched by nothing
    let slots = LampSlots {
        count: 4,
        alpha: vec![(0, "Red".into()), (1, "Yellow".into()), (2, "Green".into())],
        light: vec![
            (0, "Red".into()),
            (1, "Yellow".into()),
            (2, "Green".into()),
            (3, "NoSuchVar".into()),
        ],
    };
    let state = |v: &str| standard_traffic_lamp(v, true, false, false, false);
    let (alpha, light) = slots.values(&state);
    assert_eq!(alpha, vec![1.0, 0.0, 0.0, 1.0]);
    assert_eq!(light, vec![1.0, 0.0, 0.0, 1.0]);
    let state = |v: &str| standard_traffic_lamp(v, false, false, true, false);
    assert_eq!(
        slots.values(&state),
        (vec![0.0, 0.0, 1.0, 1.0], vec![0.0, 0.0, 1.0, 1.0])
    );
    // a light map without a variable is always on
    let plain = LampSlots {
        count: 1,
        alpha: vec![],
        light: vec![(0, String::new())],
    };
    assert_eq!(plain.values(&|_| Some(0.0)).1, vec![1.0]);
}

/// A season's snow textures are told by their folder, whatever its case (#879).
#[test]
fn snow_pictures_are_the_winter_snow_folders() {
    assert!(is_snow_picture(Path::new(
        "/omsi/Texture/WinterSnow/gras.bmp"
    )));
    assert!(is_snow_picture(Path::new(
        "/omsi/Sceneryobjects/Buildings_RW1HH/texture/Wintersnow/wall.jpg"
    )));
    assert!(!is_snow_picture(Path::new("/omsi/Texture/Winter/gras.bmp")));
    assert!(!is_snow_picture(Path::new(
        "/omsi/Texture/WinterSnow_gras.bmp"
    )));
}

#[test]
fn only_a_white_light_map_makes_an_led_panel() {
    assert!(is_white_lightmap(&[255, 255, 255, 255]));
    assert!(is_white_lightmap(&[250, 248, 255, 0, 255, 255, 255, 255]));
    assert!(!is_white_lightmap(&[
        255, 255, 255, 255, 127, 127, 127, 255
    ]));
    assert!(!is_white_lightmap(&[0, 0, 0, 255]));
    assert!(!is_white_lightmap(&[]));
}

#[test]
#[ignore = "requires the installed SOR NB content in OMSI_TEST_CONTENT"]
fn installed_sor_ois_retains_powered_freetex() {
    let root = PathBuf::from(
        std::env::var_os("OMSI_TEST_CONTENT")
            .expect("set OMSI_TEST_CONTENT to the OMSI content root"),
    );
    let model =
        ::model::Model::load(&root.join("Vehicles/SOR NB/model/1_2011.cfg")).unwrap();
    let definitions: Vec<_> = model
        .meshes
        .iter()
        .flat_map(|mesh| {
            let refs: Vec<_> = mesh.materials.iter().collect();
            free_texture_defs(&refs)
        })
        .filter(|(_, _, var)| var == "mypoldisplej")
        .collect();
    assert!(!definitions.is_empty());
    assert!(
        definitions
            .iter()
            .any(|(item, key, _)| *item && key.eq_ignore_ascii_case("cerna.bmp")),
        "{definitions:?}"
    );
    println!("SOR NB OIS: {definitions:?}");
}

#[test]
fn powered_terminal_freetex_is_kept_and_replaces_its_black_nightmap() {
    // The vehicle's OIS declares the free texture in the powered item, not in
    // the base [matl]. The black key is also used as its self-lit night map.
    let model = ::model::Model::parse(&::legacy_config::CfgFile::from_str(
        "model.cfg",
        concat!(
        "[mesh]\nterminal.o3d\n[matl]\nblack.bmp\n0\n",
        "[matl_change]\nblack.bmp\n0\npower\n[matl_item]\n",
        "[matl_nightmap]\nblack.bmp\n[matl_freetex]\nblack.bmp\nscreen\n",
        ),
    ));
    let defs: Vec<&MaterialDef> = model.meshes[0].materials.iter().collect();
    assert_eq!(
        free_texture_defs(&defs),
        vec![(true, "black.bmp".into(), "screen".into())]
    );
    let base = freetex_test_look();
    let mut powered = base.clone();
    powered.night = Some(10);
    let spec = SlotSpec {
        base,
        item: Some(powered),
        more: Vec::new(),
    };
    let changed = spec.with_freetex(Some(10), 20, true, true);
    assert_eq!(changed.base.diffuse, None); // unpowered remains black
    assert_eq!(changed.item.as_ref().unwrap().diffuse, Some(20));
    assert_eq!(changed.item.as_ref().unwrap().night, Some(20));
    assert_eq!(spec.item.as_ref().unwrap().night, Some(10)); // reusable template
}

#[test]
fn freetex_preserves_other_stages_and_per_vehicle_script_textures() {
    let mut base = freetex_test_look();
    base.night = Some(10);
    base.lightmap = Some(11);
    base.transmap = Some((10, true));
    base.envmap = Some((12, 0.5));
    let mut item = base.clone();
    item.diffuse = Some(99); // a script texture is not the file being replaced
    let spec = SlotSpec {
        base,
        item: Some(item),
        more: Vec::new(),
    };
    let changed = spec.with_freetex(Some(10), 20, true, false);
    assert_eq!(changed.base.diffuse, Some(20));
    assert_eq!(changed.base.night, Some(20));
    assert_eq!(changed.base.lightmap, Some(11));
    assert_eq!(changed.base.transmap, Some((20, true)));
    assert_eq!(changed.base.envmap, Some((12, 0.5)));
    assert_eq!(changed.item.as_ref().unwrap().diffuse, Some(99));
    let missing_key = spec.with_freetex(None, 21, true, true);
    assert_eq!(missing_key.item.as_ref().unwrap().night, Some(10));
    assert_eq!(missing_key.item.as_ref().unwrap().diffuse, Some(99));
}

#[test]
fn spline_batches_keep_materials_cells_shadows_and_long_segments_separate() {
    use ::scenery::sli::SplineTexture;
    let def = |file: &str| Spline {
        textures: vec![SplineTexture {
            file: file.into(),
            ..Default::default()
        }],
        ..Default::default()
    };
    let ty = Arc::new(SplineType {
        def: def("curb.dds"),
        dir: PathBuf::new(),
        surface_maps: None,
    });
    let other = Arc::new(SplineType {
        def: def("other.dds"),
        dir: PathBuf::new(),
        surface_maps: None,
    });
    let other_dir = Arc::new(SplineType {
        def: def("curb.dds"),
        dir: PathBuf::from("another_pack"),
        surface_maps: None,
    });
    let mut tested = def("curb.dds");
    tested.textures[0].alpha = 1;
    let tested = Arc::new(SplineType {
        def: tested,
        dir: PathBuf::new(),
        surface_maps: None,
    });
    let mut blended = def("curb.dds");
    blended.textures[0].alpha = 2;
    let blended = Arc::new(SplineType {
        def: blended,
        dir: PathBuf::new(),
        surface_maps: None,
    });
    let mut compatible = def("curb.dds");
    compatible.path = PathBuf::from("another_profile.sli");
    compatible.textures.push(SplineTexture {
        file: "unused-grass.dds".into(),
        ..Default::default()
    });
    let compatible = Arc::new(SplineType {
        def: compatible,
        dir: PathBuf::new(),
        surface_maps: None,
    });
    let mesh = |x: f32, length: f32| {
        Arc::new(MeshData {
            positions: vec![
                glam::Vec3::new(x, 0.0, 0.0),
                glam::Vec3::new(x + length, 0.0, 0.0),
                glam::Vec3::new(x, 1.0, 0.0),
            ],
            normals: vec![glam::Vec3::Z; 3],
            uvs: vec![glam::Vec2::ZERO; 3],
            indices: vec![0, 1, 2],
            ranges: vec![(0, 3, 0)],
            one_sided: true,
        })
    };
    let batched = batch_static_splines(vec![
        (mesh(1.0, 2.0), ty.clone(), false, DVec3::ZERO),
        (mesh(5.0, 2.0), ty.clone(), false, DVec3::ZERO),
        (mesh(9.0, 2.0), compatible, false, DVec3::ZERO),
        (mesh(49.0, 2.0), ty.clone(), false, DVec3::ZERO),
        (mesh(1.0, 2.0), other, false, DVec3::ZERO),
        (mesh(1.0, 2.0), other_dir, false, DVec3::ZERO),
        (mesh(1.0, 2.0), tested, false, DVec3::ZERO),
        (mesh(1.0, 2.0), ty.clone(), true, DVec3::ZERO),
        (mesh(1.0, 100.0), ty.clone(), false, DVec3::ZERO),
        (mesh(1.0, 100.0), ty, false, DVec3::ZERO),
        (
            mesh(1.0, 2.0),
            blended.clone(),
            false,
            DVec3::new(1.0, 2.0, 3.0),
        ),
        (mesh(5.0, 2.0), blended, false, DVec3::new(4.0, 5.0, 6.0)),
    ]);
    assert_eq!(batched.len(), 10);
    assert_eq!(batched[8].0.indices.len(), 3);
    assert_eq!(batched[9].0.indices.len(), 3);
    assert_eq!(batched[8].3, DVec3::new(1.0, 2.0, 3.0));
    assert_eq!(batched[9].3, DVec3::new(4.0, 5.0, 6.0));
    assert_eq!(batched[0].0.indices.len(), 9);
    assert_eq!(batched[0].0.ranges, vec![(0, 9, 0)]);
    assert_eq!(batched.iter().filter(|b| b.2).count(), 1);
    assert_eq!(batched.iter().map(|b| b.0.indices.len()).sum::<usize>(), 36);
}

#[test]
fn ground_spline_batches_preserve_faces_and_uvs_with_local_bounds() {
    let mesh = |x: f32, z: f32, length: f32, one_sided: bool| {
        Arc::new(MeshData {
            positions: vec![
                glam::Vec3::new(x, 0.0, z),
                glam::Vec3::new(x + length, 0.0, z),
                glam::Vec3::new(x, 1.0, z),
            ],
            normals: vec![glam::Vec3::Z; 3],
            uvs: vec![glam::Vec2::new(x / 300.0, z / 300.0); 3],
            indices: vec![0, 1, 2],
            ranges: vec![(0, 3, 0)],
            one_sided,
        })
    };
    let a = mesh(1.0, 0.0, 2.0, true);
    let b = mesh(5.0, 0.0, 2.0, true);
    let batched = batch_ground_splines(vec![
        a.clone(),
        b.clone(),
        mesh(49.0, 0.0, 2.0, true),
        mesh(1.0, 49.0, 2.0, true),
        mesh(1.0, 0.0, 2.0, false),
        mesh(1.0, 0.0, 100.0, true),
        mesh(1.0, 0.0, 100.0, true),
    ]);
    assert_eq!(batched.len(), 6);
    let combined = &batched[0];
    assert_eq!(
        combined.positions,
        [a.positions.clone(), b.positions.clone()].concat()
    );
    assert_eq!(
        combined.normals,
        [a.normals.clone(), b.normals.clone()].concat()
    );
    assert_eq!(combined.uvs, [a.uvs.clone(), b.uvs.clone()].concat());
    assert_eq!(combined.indices, vec![0, 1, 2, 3, 4, 5]);
    assert_eq!(combined.ranges, vec![(0, 6, 0)]);
    assert_eq!(batched.iter().map(|m| m.indices.len()).sum::<usize>(), 21);
}

/// A light map covers the 3x3 tiles around its own: a lamp's pool in the middle of the
/// picture is in the middle of the tile, one in a neighbour's third is left out, and the
/// tile's edges take the texels a third of the way in.
#[test]
fn a_light_map_is_laid_on_its_middle_third() {
    let n = 12usize;
    let mut rgba = vec![0u8; n * n * 4];
    for y in 0..n {
        for x in 0..n {
            let i = (y * n + x) * 4;
            rgba[i] = (x * 20) as u8;
            rgba[i + 1] = (y * 20) as u8;
            rgba[i + 3] = 255;
        }
    }
    // a pool in the western neighbour, left out
    rgba[(6 * n + 1) * 4 + 2] = 255;
    let img = ::texture::Image {
        width: n as u32,
        height: n as u32,
        rgba,
        has_alpha: false,
    };
    let own = own_tile_of_light_map(&img);
    assert_eq!((own.width, own.height), (n as u32, n as u32));
    let at = |x: usize, y: usize, c: usize| own.rgba[(y * n + x) * 4 + c] as f32;
    // output texel x samples x = 4 + (x + 0.5) / 3 - 0.5 of the source
    let expect = |x: usize| 20.0 * (4.0 + (x as f32 + 0.5) / 3.0 - 0.5);
    for x in [0, 5, 11] {
        assert!(
            (at(x, 0, 0) - expect(x)).abs() <= 1.0,
            "column {x}: {} against {}",
            at(x, 0, 0),
            expect(x)
        );
        assert!((at(0, x, 1) - expect(x)).abs() <= 1.0, "row {x}");
    }
    assert!((0..n * n).all(|i| own.rgba[i * 4 + 2] == 0));
}

#[test]
fn surface_contact_height_matches_the_visible_surface_lift() {
    let authored = DVec3::new(12.0, 18.0, 3.5);
    let contact = scenery_draw_position(authored, true);
    assert!((contact.z - authored.z - OMSI_SURFACE_LIFT as f64).abs() < 1e-8);
}

#[test]
fn road_markings_are_lifted_with_the_road_they_lie_on() {
    let sco = |text: &str| SceneryObject::parse(&::legacy_config::CfgFile::from_str("x.sco", text));
    // Spandau's VZ_surfmark_arrow_L: a terrain-relative object drawn on the surfaces
    assert!(drawn_on_surfaces(&sco(
        "[rendertype]\non_surface\n[mesh]\narrow.o3d\n"
    )));
    assert!(drawn_on_surfaces(&sco(
        "[rendertype]\nsurface\n[mesh]\nplate.o3d\n"
    )));
    assert!(drawn_on_surfaces(&sco("[surface]\n[mesh]\nplate.o3d\n")));
    assert!(!drawn_on_surfaces(&sco("[mesh]\nhouse.o3d\n")));
    assert!(!drawn_on_surfaces(&sco(
        "[rendertype]\npresurface\n[mesh]\nground.o3d\n"
    )));
}

#[test]
fn scripted_surface_draw_pose_keeps_the_upload_lift() {
    let authored = DVec3::new(-2165.1, -2368.8, -0.068);
    let uploaded = scenery_draw_position(authored, true);
    for _frame in 0..8 {
        assert_eq!(scenery_draw_position(authored, true), uploaded);
    }
    assert_eq!(scenery_draw_position(authored, false), authored);
    assert_eq!(uploaded.truncate(), authored.truncate());
    assert!((uploaded.z - authored.z - 0.08).abs() < 1e-8);
}

#[test]
fn scenery_render_types_map_to_the_cpp_pass_order() {
    use ::scenery::sco::RenderType as ScoPhase;
    for (source, expected) in [
        (ScoPhase::PreSurface, RenderPhase::PreSurface),
        (ScoPhase::Surface, RenderPhase::Surface),
        (ScoPhase::OnSurface, RenderPhase::OnSurface),
        (ScoPhase::BeforeNormal, RenderPhase::BeforeNormal),
        (ScoPhase::Normal, RenderPhase::Normal),
        (ScoPhase::AfterNormal, RenderPhase::AfterNormal),
        (ScoPhase::AfterVehicles, RenderPhase::AfterVehicles),
    ] {
        assert_eq!(scenery_render_phase(source), expected);
    }
}

/// A film modelled as a copy of the floor's faces with a slot of its own is an overlay;
/// a panel beside the floor, sharing one edge with it, is not.
#[test]
fn a_copy_of_another_slots_faces_is_an_overlay() {
    let v = glam::Vec3::new;
    let mesh = MeshData {
        positions: vec![
            v(0.0, 0.0, 0.0),
            v(4.0, 0.0, 0.0),
            v(4.0, 2.0, 0.0),
            v(0.0, 2.0, 0.0),
            v(4.0, 0.0, 1.0),
            v(0.0, 0.0, 1.0),
        ],
        // slot 0 the floor, slot 1 the film over it (the same corners), slot 2 a wall
        // standing on the floor's front edge
        indices: vec![0, 1, 2, 0, 2, 3, 0, 1, 2, 0, 2, 3, 0, 1, 4, 0, 4, 5],
        ranges: vec![(0, 6, 0), (6, 6, 1), (12, 6, 2)],
        ..Default::default()
    };
    assert!(slot_overlays_another(&mesh, 1));
    assert!(!slot_overlays_another(&mesh, 2));
}

/// A path's trafficdensity rules per group: the last of each, and traffic on the lane
/// while any group drives there (a path without a rule for the first group has its
/// medium density).
#[test]
fn path_densities_per_group() {
    let rule = |path: i32, value: f64, extra: f64| ::map::MapRule {
        path_index: path,
        kind: "trafficdensity".into(),
        value,
        extra,
        ..Default::default()
    };
    let rules = [
        rule(0, 0.0, 0.0),
        rule(0, 1.0, 4.0),
        rule(0, 0.5, 4.0),
        rule(1, 2.0, 0.0),
    ];
    assert_eq!(path_densities(&rules, 0), (0.5, vec![(0, 0.0), (4, 0.5)]));
    assert_eq!(path_densities(&rules, 1), (2.0, vec![(0, 2.0)]));
    assert_eq!(
        path_densities(&[rule(2, 0.3, 4.0)], 2),
        (1.0, vec![(4, 0.3)])
    );
    assert_eq!(path_densities(&[], 0), (1.0, vec![]));
}

/// A wire strung 5.5 m over its spline is no ground; a wall standing on it, or a
/// catenary spline that has a track bed at the bottom, is.
#[test]
fn only_splines_all_overhead_leave_the_ground() {
    use ::scenery::sli::{Spline, SplineProfile, SplineProfilePoint};
    let prof = |zs: &[f32]| SplineProfile {
        texture: 0,
        points: zs
            .iter()
            .map(|&z| SplineProfilePoint {
                x: z,
                z,
                ..Default::default()
            })
            .collect(),
    };
    let def = |ps: Vec<SplineProfile>| Spline {
        profiles: ps,
        ..Default::default()
    };
    assert!(overhead_only(&def(vec![
        prof(&[5.5, 5.6]),
        prof(&[2.0, 2.0])
    ])));
    assert!(!overhead_only(&def(vec![prof(&[0.0, 2.4])])));
    assert!(!overhead_only(&def(vec![
        prof(&[5.5, 5.6]),
        prof(&[-0.2, 0.0])
    ])));
    assert!(!overhead_only(&def(vec![])));
}

/// The car park's first string picks the list; anything that is no number is list 0.
#[test]
fn a_car_park_picks_its_parklist_by_its_first_string() {
    let s = |v: &[&str]| v.iter().map(|x| x.to_string()).collect::<Vec<_>>();
    assert_eq!(parklist_index(&s(&[])), 0);
    assert_eq!(parklist_index(&s(&["2", "x"])), 2);
    assert_eq!(parklist_index(&s(&[" 1 "])), 1);
    assert_eq!(parklist_index(&s(&["Taxi"])), 0);
}

/// A `[terrainmapping]` slot (TH_Wald's Fels01: rock in slot 0, grass top in slot 1)
/// leaves the object's own mesh and comes back in tile space, where the ground under the
/// placed object is: turned a quarter, 10 m into a tile whose corner is at 300/600.
#[test]
fn terrain_mapped_slots_split_off_in_tile_space() {
    let mut src = MeshData::default();
    for p in [
        [0.0, 0.0, 0.0],
        [1.0, 0.0, 0.0],
        [0.0, 1.0, 0.0],
        [0.0, 0.0, 2.0],
        [3.0, 0.0, 2.0],
        [0.0, 3.0, 2.0],
    ] {
        src.positions.push(glam::Vec3::from_array(p));
        src.normals.push(glam::Vec3::Z);
        src.uvs.push(glam::Vec2::ZERO);
    }
    src.indices = vec![0, 1, 2, 3, 4, 5];
    src.ranges = vec![(0, 3, 0), (3, 3, 1)];
    src.one_sided = true;
    let origin = DVec3::new(300.0, 600.0, 0.0);
    let pos = DVec3::new(310.0, 620.0, 5.0);
    let xf = Mat4::from_rotation_z(std::f32::consts::FRAC_PI_2);
    let (rest, ground) = split_terrain_mapped(&src, &[1], pos, xf, origin);
    assert_eq!(rest.ranges, vec![(0, 3, 0)]);
    assert_eq!(ground.ranges, vec![(0, 3, 0)]);
    assert!(ground.one_sided);
    assert_eq!(ground.positions, src.positions[3..6].to_vec());
    // (3, 0) turned a quarter is (0, 3): 10/23 m into the tile
    let uv = ground.uvs[1] * tile_size() as f32;
    assert!((uv - glam::Vec2::new(10.0, 23.0)).length() < 1e-3, "{uv:?}");
    let (_, none) = split_terrain_mapped(&src, &[2], pos, xf, origin);
    assert!(none.is_empty() && none.ranges.is_empty());
}

/// The Spandau neon lamp (Streetobjects_RUE/neonlight_M_whip_S.sco) declares its glow
/// after the far mesh of `[LOD] 0`: it still glows, near or far.
#[test]
fn lights_of_lower_lods_count() {
    let text = "[LOD]\n0.15\n[mesh]\nnear.o3d\n[LOD]\n0\n[mesh]\nfar.o3d\n[light_enh_2]\n-2.965\n0\n7.170\n-0.3817\n0\n-1.5344\n-1.5344\n0\n0.3817\n0\n1\n230\n230\n255\n2.0\n120\n200\nNightlightA\n0.8\n0.5\n1\n1\n0.2\nlichteffekt1.bmp\n";
    let model = Model::parse(&::legacy_config::CfgFile::from_str("lamp.sco", text));
    assert_eq!(model.lods.len(), 2);
    assert!(model.lod_meshes(0)[0].light_enh_2.is_empty());
    let asked = Mutex::new(Vec::new());
    let coronas = model_lights_faded(
        &model,
        &|_| Mat4::IDENTITY,
        DVec3::new(100.0, 200.0, 30.0),
        &|v| {
            asked.lock().push(v.to_string());
            1.0
        },
        &[],
    );
    // the glow, its star (effect bit 1), the halo round it in fog and (its cone flag set)
    // the light cone it throws there
    assert_eq!(coronas.len(), 4);
    assert!(!coronas[0].beam && !coronas[0].halo && coronas[0].flags & 8 == 0);
    assert!(coronas[1].flags & 8 != 0 && coronas[1].rotating == 2);
    assert!(coronas[2].halo && coronas[3].beam);
    assert_eq!(asked.into_inner(), vec!["NightlightA".to_string()]);
    assert!((coronas[0].position - DVec3::new(97.035, 200.0, 37.17)).length() < 1e-3);
    assert!(
        coronas[0].direction.z < -0.9,
        "points down: {:?}",
        coronas[0].direction
    );
}

#[test]
fn small_instrument_lights_keep_their_small_size() {
    let text = "[mesh]\ndash.o3d\n[light_enh]\n0\n0\n0\n255\n0\n0\n0.01\nspeedo_warn\n0\n";
    let model = Model::parse(&::legacy_config::CfgFile::from_str("bus.cfg", text));
    let coronas = model_lights_faded(&model, &|_| Mat4::IDENTITY, DVec3::ZERO, &|_| 1.0, &[]);
    assert_eq!(coronas.len(), 1);
    assert!((coronas[0].size - 0.005).abs() < 1e-6);
}

#[test]
fn scripted_lamp_channels_override_stock_phases_and_switch_led_materials() {
    // Numazu's pedestrian script shows green at phase 5, where stock car lamps
    // are red/yellow; it also switches the green mesh via its Yellow blink output.
    let stock_green = standard_traffic_lamp("green", true, true, false, false);
    let green = traffic_lamp_value("green", Some(1.0), stock_green);
    assert!(change_picks_item(green));
    let stock_yellow = standard_traffic_lamp("yellow", false, true, false, false);
    let blink_off = traffic_lamp_value("yellow", Some(0.0), stock_yellow);
    assert!(!change_picks_item(blink_off));
    assert!(change_picks_item(traffic_lamp_value(
        "yellow",
        Some(1.0),
        stock_yellow
    )));
    // A missing script keeps the safe stock fallback and unknown channels stay off.
    assert_eq!(traffic_lamp_value("green", None, stock_green), 0.0);
    assert_eq!(traffic_lamp_value("custom_channel", None, None), 0.0);
    assert_eq!(traffic_lamp_value("1", None, None), 1.0);
}

#[test]
fn standard_traffic_lamps_are_state_driven() {
    assert_eq!(
        standard_traffic_lamp("red", true, false, false, false),
        Some(1.0)
    );
    assert_eq!(
        standard_traffic_lamp("YELLOW", false, true, false, false),
        Some(1.0)
    );
    assert_eq!(
        standard_traffic_lamp("green", false, false, true, false),
        Some(1.0)
    );
    assert_eq!(
        standard_traffic_lamp("red", false, true, false, false),
        Some(0.0)
    );
    assert_eq!(
        standard_traffic_lamp("custom_channel", true, true, true, false),
        None
    );
}
