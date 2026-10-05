//! The production loader must not make ordinary props non-solid because of draw order.
use super::*;

#[test]
fn fixed_prop_keeps_its_collision_in_every_numeric_draw_queue() {
    let root = std::env::temp_dir().join(format!(
        "neoomsi-queue-{}-{}",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    ));
    std::fs::create_dir_all(&root).unwrap();
    std::fs::write(
        root.join("global.cfg"),
        "[name]\nSynthetic prop\n[map]\n0\n0\ntile_0_0.map\n",
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
    for (queue, count) in [("0", 1), ("1", 1), ("3", 1), ("4", 1), ("surface", 0)] {
        std::fs::write(root.join("prop.sco"), format!("[fixed]\n[boundingbox]\n2\n2\n3\n0\n0\n1.5\n[rendertype]\n{queue}\n[mesh]\nprop.x\n")).unwrap();
        let world = World::open(&root, &root.join("global.cfg"), 20261001).unwrap();
        let (prepared, _) = world.prepare_tiles(&[(0, 0, root.join("tile_0_0.map"))]);
        assert_eq!(prepared.len(), 1);
        assert_eq!(
            world.tile_state.lock()[&(0, 0)].obstacles.len(),
            count,
            "queue {queue}"
        );
    }
    let actual = root.canonicalize().unwrap();
    let temp = std::env::temp_dir().canonicalize().unwrap();
    assert!(actual.starts_with(&temp) && actual != temp);
    std::fs::remove_dir_all(actual).unwrap();
}
