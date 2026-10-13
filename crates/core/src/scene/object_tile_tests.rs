use super::*;

#[test]
fn an_id_used_on_two_tiles_is_found_on_the_tile_named() {
    let root = std::env::temp_dir().join(format!("neoomsi-object-tile-{}", std::process::id()));
    std::fs::create_dir_all(&root).unwrap();
    std::fs::write(
        root.join("global.cfg"),
        "[name]\nTwo tiles\n[map]\n0\n0\ntile_0_0.map\n[map]\n1\n0\ntile_1_0.map\n",
    )
    .unwrap();
    let object = |x: u32| format!("[version]\n14\n[object]\n0\nstop.sco\n7\n{x}\n50\n0\n90\n0\n0\n0\n");
    std::fs::write(root.join("tile_0_0.map"), object(10)).unwrap();
    let utf16: Vec<u8> = [0xFF, 0xFE]
        .into_iter()
        .chain(object(20).encode_utf16().flat_map(|c| c.to_le_bytes()))
        .collect();
    std::fs::write(root.join("tile_1_0.map"), utf16).unwrap();
    let world = World::open(&root, &root.join("global.cfg"), 20261001).unwrap();
    world.index();
    let x = |group: i32| world.object_on_tile(group, 7).map(|(p, _)| p.x.round());
    let size = tile_size();
    assert_eq!(x(0), Some(10.0));
    assert_eq!(x(1), Some(size + 20.0));
    let _ = std::fs::remove_dir_all(&root);
}

/// A lane reaching over a tile the map does not have still has its ground (Gerolstein's
/// railway north of the station): the train there was taken away as if unloaded.
#[test]
fn a_lane_over_a_missing_tile_keeps_its_ground() {
    let root = std::env::temp_dir().join(format!(
        "neoomsi-missing-tile-{}-{}",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    ));
    std::fs::create_dir_all(&root).unwrap();
    std::fs::write(
        root.join("global.cfg"),
        "[name]\nOne tile\n[map]\n0\n0\ntile_0_0.map\n",
    )
    .unwrap();
    std::fs::write(root.join("tile_0_0.map"), "[version]\n14\n").unwrap();
    let world = World::open(&root, &root.join("global.cfg"), 20261001).unwrap();
    let (prepared, _) = world.prepare_tiles(&[(0, 0, root.join("tile_0_0.map"))]);
    assert_eq!(prepared.len(), 1);
    let size = tile_size();
    let (x, y) = (size * 0.5, size * 1.5);
    assert!(!world.has_ground(x, y));
    assert!(world.has_ground_on(x, y, Some((0, 0))));
    assert!(!world.has_ground_on(x, y, Some((0, 1))));
    assert!(!world.has_ground_on(x, y, None));
    assert!(world.has_ground_on(x, size * 0.5, None));
    drop(prepared);
    drop(world);
    std::fs::remove_dir_all(&root).unwrap();
}
