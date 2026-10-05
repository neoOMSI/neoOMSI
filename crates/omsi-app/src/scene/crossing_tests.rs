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
        paths: vec![omsi_scenery::PathDef {
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
