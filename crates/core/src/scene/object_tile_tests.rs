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

#[test]
fn a_tree_past_its_tiles_edge_is_left_out() {
    let root = std::env::temp_dir().join(format!("neoomsi-tree-edge-{}", std::process::id()));
    std::fs::create_dir_all(&root).unwrap();
    std::fs::write(
        root.join("global.cfg"),
        "[name]\nTree edge\n[map]\n0\n0\ntile_0_0.map\n",
    )
    .unwrap();
    std::fs::write(
        root.join("tree.sco"),
        "[tree]\ntree.tga\n12\n18\n0.9\n1.1\n",
    )
    .unwrap();
    let tree = |id: u32, x: f64, y: f64| {
        format!("[object]\n0\ntree.sco\n{id}\n{x}\n{y}\n0\n0\n0\n0\n4\ntree.tga\n13\n1.2\n\n\n")
    };
    let size = tile_size();
    let records: String = [
        (1, 218.5, -0.85),
        (2, 218.5, size + 0.85),
        (3, -0.85, 100.0),
        (4, size + 0.85, 100.0),
        (5, 218.5, 0.85),
    ]
    .into_iter()
    .map(|(id, x, y)| tree(id, x, y))
    .collect();
    std::fs::write(
        root.join("tile_0_0.map"),
        format!("[version]\n14\n{records}"),
    )
    .unwrap();
    let world = World::open(&root, &root.join("global.cfg"), 20261015).unwrap();
    let (prepared, _) = world.prepare_tiles(&[(0, 0, root.join("tile_0_0.map"))]);
    let trees: Vec<(f64, f64)> = prepared[0].trees.iter().map(|t| (t.2.x, t.2.y)).collect();
    assert_eq!(trees.len(), 1, "only the tree on its own tile: {trees:?}");
    assert!((trees[0].0 - 218.5).abs() < 1e-6 && (trees[0].1 - 0.85).abs() < 1e-6);
    let _ = std::fs::remove_dir_all(&root);
}
