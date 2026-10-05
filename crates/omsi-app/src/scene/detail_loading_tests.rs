//! Detail affects simulation loading as well as drawing; editor source remains intact.
use super::*;

#[test]
fn map_detail_omits_visual_and_collision_records_before_staging() {
    let root = std::env::temp_dir().join(format!(
        "neoomsi-detail-{}-{}",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    ));
    std::fs::create_dir_all(&root).unwrap();
    std::fs::write(
        root.join("global.cfg"),
        "[name]\nSynthetic detail\n[map]\n0\n0\ntile_0_0.map\n",
    )
    .unwrap();
    std::fs::write(
        root.join("prop.sco"),
        "[fixed]\n[boundingbox]\n2\n2\n3\n0\n0\n1.5\n[mesh]\nprop.x\n",
    )
    .unwrap();
    std::fs::write(
        root.join("prop.x"),
        "xof 0303txt 0032\nMesh prop {3;0;0;0;,1;0;0;,0;3;0;;1;3;0,1,2;;}",
    )
    .unwrap();
    let records = "[version]\n14\n[object]\n1\nprop.sco\n1\n100\n100\n0\n0\n0\n0\n0\n[object]\n2\nprop.sco\n2\n110\n100\n0\n0\n0\n0\n0\n";
    std::fs::write(root.join("tile_0_0.map"), records).unwrap();
    let mut world = World::open(&root, &root.join("global.cfg"), 20261001).unwrap();
    world.map_detail = 1;
    let (prepared, _) = world.prepare_tiles(&[(0, 0, root.join("tile_0_0.map"))]);
    assert_eq!(prepared.len(), 1);
    assert_eq!(prepared[0].objects.len(), 1);
    assert_eq!(world.tile_state.lock()[&(0, 0)].obstacles.len(), 1);
    assert!(world.object_positions.lock().contains_key(&1));
    assert!(!world.object_positions.lock().contains_key(&2));
    // Saving/editor reads use the original file, which must still include excluded data.
    let raw = omsi_map::Tile::load(&root.join("tile_0_0.map")).unwrap();
    assert_eq!(raw.objects.len(), 2);
    assert_eq!(
        std::fs::read_to_string(root.join("tile_0_0.map")).unwrap(),
        records
    );
    drop(prepared);
    drop(world);
    let actual = root.canonicalize().unwrap();
    let temp = std::env::temp_dir().canonicalize().unwrap();
    assert!(actual.starts_with(&temp) && actual != temp);
    std::fs::remove_dir_all(actual).unwrap();
}
