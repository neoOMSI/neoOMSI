use super::*;

fn with_world(test: impl FnOnce(&World)) {
    let root = std::env::temp_dir().join(format!(
        "neoomsi-traffic-path-{}-{}",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    ));
    std::fs::create_dir_all(&root).unwrap();
    std::fs::write(root.join("global.cfg"), "[name]\nSynthetic traffic path\n").unwrap();
    let world = World::open(&root, &root.join("global.cfg"), 20261013).unwrap();
    test(&world);
    drop(world);
    let actual = root.canonicalize().unwrap();
    let temp = std::env::temp_dir().canonicalize().unwrap();
    assert!(actual.starts_with(&temp) && actual != temp);
    std::fs::remove_dir_all(actual).unwrap();
}

#[test]
fn loaded_paths_cross_tile_boundaries_without_terrain() {
    with_world(|world| {
        let size = tile_size();
        world
            .terrains
            .write()
            .insert((0, 0), Arc::new(Terrain::flat()));
        // A road or track belongs to tile (0, 0), but continues into the absent tile (1, 0).
        for kind in [LaneKind::Street, LaneKind::Rail] {
            for source in [1, 2] {
                let mut lane = LaneBuilder::polyline(
                    vec![
                        DVec3::new(size - 5.0, 10.0, 0.0),
                        DVec3::new(size + 5.0, 10.0, 0.0),
                    ],
                    kind,
                    3.0,
                );
                lane.key = Some(LaneKey {
                    tile: (0, 0),
                    id: 1,
                    path: 0,
                });
                lane.source = source;
                assert!(!world.has_ground(lane.end().x, lane.end().y));
                for s in [0.0, 5.0, 10.0] {
                    assert!(world.traffic_path_loaded(&lane, lane.at(s).0));
                }
                // Ground under the vehicle does not keep a path alive after its source goes.
                world
                    .terrains
                    .write()
                    .insert((1, 0), Arc::new(Terrain::flat()));
                world.terrains.write().remove(&(0, 0));
                assert!(world.has_ground(lane.end().x, lane.end().y));
                assert!(!world.traffic_path_loaded(&lane, lane.end()));
                world
                    .terrains
                    .write()
                    .insert((0, 0), Arc::new(Terrain::flat()));
                assert!(world.traffic_path_loaded(&lane, lane.end()));
                world.terrains.write().remove(&(1, 0));
            }
        }
    });
}

#[test]
fn connectors_need_local_ground_but_aircraft_do_not() {
    with_world(|world| {
        let pos = DVec3::new(10.0, 10.0, 0.0);
        let mut lane =
            LaneBuilder::polyline(vec![pos, pos + DVec3::Y * 10.0], LaneKind::Street, 3.0);
        assert!(!world.traffic_path_loaded(&lane, pos));
        world
            .terrains
            .write()
            .insert((0, 0), Arc::new(Terrain::flat()));
        assert!(world.traffic_path_loaded(&lane, pos));
        world.terrains.write().clear();
        lane.kind = LaneKind::Air;
        assert!(world.traffic_path_loaded(&lane, pos));
        lane.key = Some(LaneKey {
            tile: (0, 0),
            id: 1,
            path: 0,
        });
        assert!(world.traffic_path_loaded(&lane, pos));
    });
}
