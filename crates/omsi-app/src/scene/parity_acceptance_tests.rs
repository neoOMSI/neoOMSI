//! Acceptance cases run against upstream before combining the local fixes.
use super::*;

fn fixture(name: &str) -> PathBuf {
    let root = std::env::temp_dir().join(format!(
        "neoomsi-acceptance-{name}-{}-{}",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    ));
    std::fs::create_dir_all(&root).unwrap();
    root
}

fn clean_fixture(root: PathBuf) {
    let actual = root.canonicalize().unwrap();
    let temp = std::env::temp_dir().canonicalize().unwrap();
    assert!(actual.starts_with(&temp) && actual != temp);
    std::fs::remove_dir_all(actual).unwrap();
}

#[test]
fn acceptance_crossing_collision_and_lod_share_authored_height() {
    let root = fixture("crossing");
    std::fs::write(root.join("global.cfg"), "[name]\nSynthetic crossing\n").unwrap();
    std::fs::write(root.join("crossing.sco"), "[fixed]\n[collision_mesh]\nplate.x\n[crossing_heightdeformation]\nfield.x\n[LOD]\n10\n[mesh]\nplate.x\n[LOD]\n0\n[mesh]\nplate.x\n").unwrap();
    std::fs::write(
        root.join("plate.x"),
        "xof 0303txt 0032\nMesh plate {3;0;0;0;,10;0;0;,0;0;10;;1;3;0,1,2;;}",
    )
    .unwrap();
    std::fs::write(root.join("field.x"), "xof 0303txt 0032\nMesh field {4;-20;-6;-20;,20;-6;-20;,-20;6;20;,20;6;20;;2;3;0,1,2;,3;1,3,2;;}").unwrap();
    let world = World::open(&root, &root.join("global.cfg"), 20261001).unwrap();
    let ot = world.object_type("crossing.sco").unwrap();
    let heights = [
        ot.meshes[0].0.positions[2].z,
        ot.lower_lods[0].1[0].0.positions[2].z,
        ot.collision.as_ref().unwrap().positions[2].z,
    ];
    drop(ot);
    drop(world);
    clean_fixture(root);
    assert_eq!(heights, [3.0; 3]);
}

#[test]
fn acceptance_after_vehicles_queue_preserves_fixed_prop_collision() {
    let root = fixture("queue");
    std::fs::write(
        root.join("global.cfg"),
        "[name]\nSynthetic prop\n[map]\n0\n0\ntile_0_0.map\n",
    )
    .unwrap();
    std::fs::write(
        root.join("prop.sco"),
        "[fixed]\n[boundingbox]\n2\n2\n3\n0\n0\n1.5\n[rendertype]\n4\n[mesh]\nprop.x\n",
    )
    .unwrap();
    std::fs::write(
        root.join("prop.x"),
        "xof 0303txt 0032\nMesh prop {3;0;0;0;,1;0;0;,0;3;0;;1;3;0,1,2;;}",
    )
    .unwrap();
    std::fs::write(
        root.join("tile_0_0.map"),
        "[version]\n14\n[object]\n0\nprop.sco\n42\n100\n100\n0\n0\n0\n0\n0\n",
    )
    .unwrap();
    let world = World::open(&root, &root.join("global.cfg"), 20261001).unwrap();
    let (prepared, _) = world.prepare_tiles(&[(0, 0, root.join("tile_0_0.map"))]);
    assert_eq!(prepared.len(), 1);
    let obstacles = world.tile_state.lock()[&(0, 0)].obstacles.len();
    drop(prepared);
    drop(world);
    clean_fixture(root);
    assert_eq!(obstacles, 1);
}
