//! Synthetic crossing geometry; no original game content is used.
use super::*;
use glam::Vec3;

fn plane() -> MeshData {
    MeshData {
        positions: vec![
            Vec3::new(-20.0, -20.0, -6.0),
            Vec3::new(20.0, -20.0, -6.0),
            Vec3::new(-20.0, 20.0, 6.0),
            Vec3::new(20.0, 20.0, 6.0),
        ],
        indices: vec![0, 1, 2, 1, 3, 2],
        ..Default::default()
    }
}

fn path(only_editor: bool, direction: i32) -> SceneryObject {
    SceneryObject {
        only_editor,
        paths: vec![::scenery::PathDef {
            direction,
            width: 3.0,
            params: vec![0.0, 0.0, 0.0, 0.0, 0.0, 10.0, 0.0, 0.0, 0.0, 0.0, 0.0],
            ..Default::default()
        }],
        ..Default::default()
    }
}

#[test]
fn crossing_paths_follow_local_deformation_and_full_pose() {
    let pos = DVec3::new(1000000.0, -1000000.0, 50.0);
    let xf = object_rotation([37.0, 30.0, 17.0]);
    let field = plane();
    for hidden in [false, true] {
        let lanes = object_lanes(
            &path(hidden, 2),
            pos,
            xf,
            Some(&field),
            None,
            (0, 0),
            42,
            &[],
        );
        assert_eq!(lanes.len(), 2);
        for (lane, reverse) in lanes.iter().zip([false, true]) {
            let expected = [Vec3::ZERO, Vec3::new(0.0, 10.0, 3.0)]
                .map(|p| pos + xf.transform_point3(p).as_dvec3());
            let (a, b) = if reverse {
                (expected[1], expected[0])
            } else {
                (expected[0], expected[1])
            };
            assert!((lane.points[0] - a).length() < 1e-5);
            assert!((lane.points.last().unwrap() - b).length() < 1e-5);
            assert!((lane.length() - 109.0_f32.sqrt()).abs() < 1e-5);
            let (end, _) = lane.at(lane.length());
            assert!((end - b).length() < 1e-5);
            assert_eq!(lane.key.unwrap().id, 42);
        }
    }
}

#[test]
fn hidden_paths_are_tilted_without_a_deformation_field() {
    let xf = Mat4::from_rotation_x(30.0_f32.to_radians());
    let lanes = object_lanes(&path(true, 0), DVec3::ZERO, xf, None, None, (0, 0), 1, &[]);
    let end = *lanes[0].points.last().unwrap();
    assert!((end.y - 8.660254).abs() < 1e-5);
    assert!((end.z - 5.0).abs() < 1e-5);
    assert!((lanes[0].length() - 10.0).abs() < 1e-5);
}

#[test]
fn vertices_outside_the_crossing_field_are_not_extrapolated() {
    let mut mesh = MeshData {
        positions: vec![Vec3::new(0.0, 10.0, 0.2), Vec3::new(100.0, 100.0, 0.2)],
        ..Default::default()
    };
    deform_mesh(&mut mesh, &plane());
    assert!((mesh.positions[0].z - 3.2).abs() < 1e-6);
    assert_eq!(mesh.positions[1].z, 0.2);
}

#[test]
fn loading_deforms_all_lods_and_collision_once_before_placement() {
    let root = std::env::temp_dir().join(format!(
        "neoomsi-crossing-{}-{}",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    ));
    std::fs::create_dir_all(&root).unwrap();
    std::fs::write(root.join("global.cfg"), "[name]\nSynthetic crossing\n").unwrap();
    // The collision declaration deliberately precedes the deformation declaration.
    std::fs::write(root.join("crossing.sco"), "[fixed]\n[collision_mesh]\nplate.x\n[crossing_heightdeformation]\nfield.x\n[LOD]\n10\n[mesh]\nplate.x\n[LOD]\n0\n[mesh]\nplate.x\n").unwrap();
    std::fs::write(
        root.join("plate.x"),
        "xof 0303txt 0032\nMesh plate {3;0;0;0;,10;0;0;,0;0;10;;1;3;0,1,2;;}",
    )
    .unwrap();
    std::fs::write(root.join("field.x"), "xof 0303txt 0032\nMesh field {4;-20;-6;-20;,20;-6;-20;,-20;6;20;,20;6;20;;2;3;0,1,2;,3;1,3,2;;}").unwrap();
    let world = World::open(&root, &root.join("global.cfg"), 20261001).unwrap();
    let ot = world.object_type("crossing.sco").unwrap();
    assert_eq!(ot.meshes.len(), 1);
    assert_eq!(ot.lower_lods.len(), 1);
    for mesh in [
        &ot.meshes[0].0,
        &ot.lower_lods[0].1[0].0,
        ot.collision.as_ref().unwrap(),
    ] {
        assert!((mesh.positions[2].z - 3.0).abs() < 1e-6);
        // Elevated and buried poses cannot change the type's local geometry.
        for z in [-31.0, 11.99, 12.01, 50.0] {
            let world_point = DVec3::new(0.0, 0.0, z) + mesh.positions[2].as_dvec3();
            assert!((world_point.z - (z + 3.0)).abs() < 1e-6);
        }
    }
    let cached = world.object_type("crossing.sco").unwrap();
    assert!(Arc::ptr_eq(&ot, &cached));
    assert!((cached.meshes[0].0.positions[2].z - 3.0).abs() < 1e-6);
    drop(cached);
    drop(ot);
    drop(world);
    // Only the unique synthetic fixture directory inside the system temp is removed.
    let actual = root.canonicalize().unwrap();
    let temp = std::env::temp_dir().canonicalize().unwrap();
    assert!(actual.starts_with(&temp) && actual != temp);
    std::fs::remove_dir_all(actual).unwrap();
}

/// An `[onlyeditor]` lamp still reads its crossing light (#303).
#[test]
fn an_editor_only_lamp_follows_its_crossing_light() {
    let root = std::env::temp_dir().join(format!(
        "neoomsi-editor-lamp-{}-{}",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    ));
    std::fs::create_dir_all(&root).unwrap();
    std::fs::write(
        root.join("global.cfg"),
        "[name]\nSynthetic crossing\n[map]\n0\n0\ntile_0_0.map\n",
    )
    .unwrap();
    std::fs::write(
        root.join("crossing.sco"),
        "[traffic_lights_group]\n10\n\
[traffic_light]\nroad\n[phase]\n6\n5\n[phase]\n2\n5\n[approachdist]\n0\n\
[traffic_light]\nbell\n[phase]\n2\n5\n[phase]\n6\n5\n[approachdist]\n0\n",
    )
    .unwrap();
    std::fs::write(
        root.join("gong.sco"),
        "[trafficlight]\n[sound]\ngong.cfg\n[script]\n1\ngong.osc\n[onlyeditor]\n[mesh]\ngong.x\n",
    )
    .unwrap();
    std::fs::write(root.join("gong.osc"), "{init}\n{end}\n{frame}\n{end}\n").unwrap();
    std::fs::write(
        root.join("gong.x"),
        "xof 0303txt 0032\nMesh gong {3;0;0;0;,1;0;0;,0;1;0;;1;3;0,1,2;;}",
    )
    .unwrap();
    std::fs::write(
        root.join("tile_0_0.map"),
        "[version]\n14\n\
[object]\n0\ncrossing.sco\n1\n100\n100\n0\n0\n0\n0\n0\n\n\
[object]\n0\ngong.sco\n2\n105\n100\n0\n0\n0\n0\n1\n1\n\n[varparent]\n1\n",
    )
    .unwrap();
    let world = World::open(&root, &root.join("global.cfg"), 20261001).unwrap();
    let (mut prepared, _) = world.prepare_tiles(&[(0, 0, root.join("tile_0_0.map"))]);
    assert_eq!(prepared.len(), 1);
    // (the upload adds the visible lamps to the editor-only ones)
    world.commit_upload(
        world.begin_upload(prepared.pop().unwrap()),
        &mut LoadStats::default(),
    );
    {
        let states = world.tile_state.lock();
        let lamps = &states[&(0, 0)].light_objects;
        assert_eq!(lamps.len(), 1);
        let lamp = &lamps[0];
        assert_eq!((lamp.parent, lamp.index, lamp.any_light), (1, 1, false));
        assert!(lamp.script.is_some() && lamp.sound.is_some());
    }
    // (not also run as a sound object without the crossing light)
    assert!(!world.scripted.lock().iter().any(|o| o.map_id == 2));
    drop(world);
    std::fs::remove_dir_all(&root).unwrap();
}

/// A lamp draws the texts its script writes (#305).
#[test]
fn a_board_on_a_crossing_light_draws_its_script_texts() {
    let root = std::env::temp_dir().join(format!(
        "neoomsi-lamp-text-{}-{}",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    ));
    std::fs::create_dir_all(&root).unwrap();
    std::fs::write(
        root.join("global.cfg"),
        "[name]\nSynthetic crossing\n[map]\n0\n0\ntile_0_0.map\n",
    )
    .unwrap();
    std::fs::write(
        root.join("crossing.sco"),
        "[traffic_lights_group]\n10\n[traffic_light]\nroad\n[phase]\n6\n5\n[phase]\n2\n5\n[approachdist]\n0\n",
    )
    .unwrap();
    // (no `[trafficlight]`: string 0 links it to light 0)
    std::fs::write(
        root.join("board.sco"),
        "[stringvarnamelist]\n1\nstrings.txt\n[script]\n1\nboard.osc\n\
[mesh]\nboard.x\n\
[texttexture]\nShown\nnofont\n64\n32\n1\n255\n255\n255\n\
[matl]\nboard.png\n0\n[useTextTexture]\n0\n",
    )
    .unwrap();
    std::fs::write(root.join("strings.txt"), "Shown\n").unwrap();
    std::fs::write(
        root.join("board.osc"),
        "{frame}\n(L.L.TrafficLightPhase) $IntToStr (S.$.Shown)\n{end}\n",
    )
    .unwrap();
    std::fs::write(
        root.join("board.x"),
        r#"xof 0303txt 0032
        Mesh board {
            3; 0;0;0;, 1;0;0;, 0;0;1;;
            1; 3;0,1,2;;
            MeshTextureCoords {3;0;0;,1;0;,0;1;;}
            MeshMaterialList {1;1;0;; Material {1;1;1;1;;0;0;0;0;;0;0;0;; TextureFilename {"board.png";} }}
        }
    "#,
    )
    .unwrap();
    std::fs::write(
        root.join("tile_0_0.map"),
        "[version]\n14\n\
[object]\n0\ncrossing.sco\n1\n100\n100\n0\n0\n0\n0\n0\n\n\
[object]\n0\nboard.sco\n2\n105\n100\n0\n0\n0\n0\n1\n0\n\n[varparent]\n1\n",
    )
    .unwrap();
    let world = World::open(&root, &root.join("global.cfg"), 20261013).unwrap();
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
    let mut scene = renderer.new_scene();
    let (mut prepared, _) = world.prepare_tiles(&[(0, 0, root.join("tile_0_0.map"))]);
    world.upload_tile(
        &renderer,
        &mut scene,
        prepared.pop().unwrap(),
        &mut LoadStats::default(),
    );
    let lamp = {
        let states = world.tile_state.lock();
        let lamps = &states[&(0, 0)].light_objects;
        assert_eq!(lamps.len(), 1);
        lamps[0].clone()
    };
    assert_eq!((lamp.parent, lamp.index), (1, 0));
    assert_eq!(
        lamp.texts.lock().len(),
        1,
        "the board's text slot is the script's"
    );
    // green (6), then red (2)
    let script = lamp.script.as_ref().unwrap();
    for (phase, shown) in [(6.0, "6"), (2.0, "2")] {
        let mut s = script.lock();
        s.update(
            0.1,
            &::simulation::scenery::SceneryVars {
                in_use: 1.0,
                traffic_light_phase: phase,
                ..Default::default()
            },
        );
        draw_script_texts(
            &renderer,
            &mut scene,
            &mut s,
            &mut lamp.texts.lock(),
            || "lamp".into(),
        );
        assert_eq!(lamp.texts.lock()[0].1.last_text.as_deref(), Some(shown));
    }
    drop(lamp);
    drop(world);
    std::fs::remove_dir_all(&root).unwrap();
}
