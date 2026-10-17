//! Exercise the production tile upload with a painted, cut terrain and mapped
//! surfaces. No installed map content or live game state is needed.
use super::*;

struct Fixture(PathBuf);

impl Fixture {
    fn new() -> Self {
        let path = std::env::temp_dir().join(format!(
            "neoomsi-terrain-mapping-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos(),
        ));
        std::fs::create_dir_all(&path).unwrap();
        Self(path)
    }

    fn write(&self, name: &str, text: &str) {
        std::fs::write(self.0.join(name), text).unwrap();
    }
}

impl Drop for Fixture {
    fn drop(&mut self) {
        if let (Ok(path), Ok(root)) = (self.0.canonicalize(), std::env::temp_dir().canonicalize()) {
            if path.starts_with(&root) && path != root {
                let _ = std::fs::remove_dir_all(path);
            }
        }
    }
}

#[test]
fn texture_preparation_deduplicates_files_without_merging_search_priorities() {
    let fixture = Fixture::new();
    let first = fixture.0.join("first");
    let second = fixture.0.join("second");
    std::fs::create_dir_all(&first).unwrap();
    std::fs::create_dir_all(&second).unwrap();
    for (dir, color) in [(&first, [40, 80, 120, 255]), (&second, [120, 80, 40, 255])] {
        image::RgbaImage::from_pixel(8, 8, image::Rgba(color))
            .save(dir.join("shared.png"))
            .unwrap();
    }
    let mut wanted = vec![
        ("shared.png".into(), vec![first.clone(), second.clone()]),
        ("shared.png".into(), vec![first.clone()]),
        ("SHARED.PNG".into(), vec![first.clone(), fixture.0.clone()]),
        ("missing.png".into(), vec![first.clone()]),
    ];
    let images = prepare_texture_images(&wanted);
    let data = &images[&first.join("shared.png")];
    assert_eq!((data.width, data.height), (8, 8));
    // All requested aliases remain available, sharing the same decoded allocation.
    for (name, dirs) in &wanted[..3] {
        let dirs: Vec<&Path> = dirs.iter().map(|d| d.as_path()).collect();
        let path = ::texture::find_texture(name, &dirs).unwrap();
        assert!(Arc::ptr_eq(&images[&path], data));
    }
    // Identically named textures in a higher-priority pack must keep their own identity.
    wanted.push(("shared.png".into(), vec![second.clone(), first.clone()]));
    let images = prepare_texture_images(&wanted);
    assert!(!Arc::ptr_eq(
        &images[&first.join("shared.png")],
        &images[&second.join("shared.png")]
    ));
}

fn triangle(z: f32) -> MeshData {
    MeshData {
        positions: vec![
            glam::Vec3::new(0.0, 0.0, z),
            glam::Vec3::new(10.0, 0.0, z),
            glam::Vec3::new(0.0, 10.0, z),
        ],
        normals: vec![glam::Vec3::Z; 3],
        uvs: vec![glam::Vec2::ZERO; 3],
        indices: vec![0, 1, 2],
        ranges: vec![(0, 3, 0)],
        ..Default::default()
    }
}

#[test]
fn authored_surface_heights_meet_the_terrain_without_an_added_step() {
    let fixture = Fixture::new();
    fixture.write(
        "global.cfg",
        "[name]\nSurface height regression\n[map]\n0\n0\ntile_0_0.map\n",
    );
    fixture.write(
        "road.sli",
        "[profile]\n0\n[profilepnt]\n-2\n0\n0\n1\n[profilepnt]\n2\n0\n1\n1\n",
    );
    fixture.write("surface.sco", "[surface]\n[mesh]\nsurface.x\n");
    fixture.write(
        "surface.x",
        "xof 0303txt 0032\nMesh surface {3;0;0;0;,10;0;0;,0;0.5;10;;1;3;0,2,1;;}",
    );
    fixture.write(
        "tile_0_0.map",
        "[version]\n14\n[terrain]\n\n[spline]\n0\nroad.sli\n1\n0\n0\n20\n2\n20\n0\n20\n0\n5\n5\n0\n0\n0\n0\n0\n0\n\n[object]\n0\nsurface.sco\n2\n90\n20\n0\n0\n0\n0\n0\n",
    );
    let mut terrain = Terrain::flat();
    let cell = tile_size() as f32 / terrain.cells as f32;
    let n = terrain.samples();
    for iy in 0..n {
        for ix in 0..n {
            terrain.heights[iy * n + ix] = 1.0 + iy as f32 * cell * 0.05;
        }
    }
    std::fs::write(fixture.0.join("tile_0_0.map.terrain"), terrain.to_bytes()).unwrap();
    let world = World::open(&fixture.0, &fixture.0.join("global.cfg"), 20261001).unwrap();
    let (prepared, _) = world.prepare_tiles(&[(0, 0, fixture.0.join("tile_0_0.map"))]);
    assert_eq!(prepared.len(), 1);
    assert_eq!(prepared[0].splines.len(), 1);
    assert_eq!(prepared[0].objects.len(), 1);
    // Cross both ends of an inclined spline whose authored surface matches the terrain.
    // The old 8 cm lift affected the spline alone and made a step at each end. Paul likes :)
    for y in [19.9, 20.1, 25.0, 39.9, 40.1] {
        let expected = 1.0 + y * 0.05;
        let got = drive_probe(&world.terrains, &world.surfaces, 20.0, y, expected + 1.0);
        assert!(
            (got.below.unwrap() - expected).abs() < 1e-5,
            "y={y}: {got:?}"
        );
    }
    // A terrain-relative [surface] object must use the same height as well
    let got = drive_probe(&world.terrains, &world.surfaces, 91.0, 21.0, 3.0);
    assert!((got.below.unwrap() - 2.05).abs() < 1e-5, "surface: {got:?}");
}

#[test]
#[ignore = "requires a graphics adapter; uploads an isolated synthetic tile"]
fn mapped_splines_and_objects_use_uncut_base_while_ground_keeps_paint() {
    let fixture = Fixture::new();
    fixture.write("global.cfg", "[name]\nTerrain mapping regression\n");
    fixture.write("mapped.png.cfg", "[terrainmapping]\n");
    fixture.write(
        "mapped.sco",
        "[groups]\n1\nRegression\n[friendlyname]\nMapped island\n[mesh]\nisland.x\n",
    );
    fixture.write("island.x", r#"xof 0303txt 0032
        Mesh island {
            3; 0;0;2;, 10;0;2;, 0;10;2;;
            1; 3;0,1,2;;
            MeshTextureCoords {3;0;0;,1;0;,0;1;;}
            MeshMaterialList {1;1;0;; Material {1;1;1;1;;0;0;0;0;;0;0;0;; TextureFilename {"mapped.png";} }}
        }
    "#);
    for (name, color) in [
        ("base.png", [0, 255, 0, 255]),
        ("paint.png", [255, 0, 0, 255]),
        ("mapped.png", [0, 0, 255, 255]),
    ] {
        image::save_buffer(fixture.0.join(name), &color, 1, 1, image::ColorType::Rgba8).unwrap();
    }
    let mut world = World::open(&fixture.0, &fixture.0.join("global.cfg"), 20261001).unwrap();
    world.global.ground_textures = ["base.png", "paint.png"]
        .into_iter()
        .map(|texture| ::map::global::GroundTex {
            texture: texture.into(),
            params: [8.0, 4.0, 8.0],
            ..Default::default()
        })
        .collect();
    let object = world
        .object_type("mapped.sco")
        .expect("synthetic scenery must load");
    assert_eq!(object.meshes.len(), 1);
    let spline = Arc::new(SplineType {
        dir: fixture.0.clone(),
        surface_maps: None,
        def: Spline {
            path: fixture.0.join("mapped.sli"),
            textures: vec![::scenery::sli::SplineTexture {
                file: "mapped.png".into(),
                ..Default::default()
            }],
            ..Default::default()
        },
    });
    let instance = crate::graphics_instance();
    let renderer = pollster::block_on(Renderer::new_with(
        &instance,
        None,
        Some(wgpu::TextureFormat::Rgba8UnormSrgb),
        ::render::RenderOptions {
            msaa: 1,
            shadow_size: 1024,
            ..Default::default()
        },
    ))
    .expect("test renderer");
    let mut scene = renderer.new_scene();
    let prepared = Prepared {
        tx: 0,
        ty: 0,
        terrain: Some(triangle(0.0)),
        paint_masks: Vec::new(),
        paint: vec![(1, TextureData::from_image(Image::solid([255; 4])), 1.0)],
        water: None,
        // Both upload paths must agree: a spatially batched mapped spline and one
        // left in the per-spline path (OMSI_NO_GROUND_SPLINE_BATCHING).
        ground_splines: batch_ground_splines(vec![Arc::new(triangle(0.5))]),
        splines: vec![(Arc::new(triangle(1.0)), spline, false, DVec3::ZERO)],
        objects: vec![PlacedObject {
            ot: object,
            pos: DVec3::ZERO,
            xf: Mat4::IDENTITY,
            lamp: None,
            map_id: 1,
            key: 1,
            controller: None,
            strings: Vec::new(),
            var_parent: None,
            parked: false,
            editable: false,
            script: None,
        }],
        trees: Vec::new(),
        origin: DVec3::ZERO,
        light_map: Some(TextureData::from_image(Image::solid([80, 70, 60, 255]))),
        cut: Some(TextureData::from_image(Image::solid([255, 255, 255, 0]))),
        images: Arc::new(HashMap::new()),
    };
    let mut upload = world.begin_upload(prepared);
    while !world.upload_step(&renderer, &mut scene, &mut upload, None) {}
    while !world.place_step(&renderer, &mut scene, &mut upload, None) {}
    let base_texture = world.gpu.lock().ground.as_ref().unwrap().ground_id.unwrap();
    let terrain: Vec<_> = scene
        .instances
        .iter()
        .filter(|i| i.render_phase == RenderPhase::Terrain)
        .collect();
    assert_eq!(
        terrain.len(),
        2,
        "the ordinary terrain retains its base and painted layer"
    );
    let ground_base = &scene.materials[terrain[0].materials[0]];
    assert_eq!(ground_base.texture, Some(base_texture));
    assert!(
        ground_base.transmap.is_some(),
        "the actual ground retains its road cut"
    );
    let paint = &scene.materials[terrain[1].materials[0]];
    assert_ne!(paint.texture, Some(base_texture));
    assert!(terrain[1].ground_layer);

    let mapped: Vec<_> = scene
        .instances
        .iter()
        .filter(|i| {
            i.render_phase != RenderPhase::Terrain && !scene.meshes[i.mesh].ranges.is_empty()
        })
        .collect();
    assert_eq!(
        mapped.len(),
        3,
        "exactly one base-ground draw for each mapped spline/object, with no painted overlays"
    );
    assert_eq!(
        mapped
            .iter()
            .filter(|i| i.render_phase == RenderPhase::Spline)
            .count(),
        2
    );
    for instance in mapped {
        assert_eq!(instance.transform, Mat4::IDENTITY);
        assert_eq!(instance.origin, DVec3::ZERO);
        assert_eq!(instance.materials.len(), 1);
        let material = &scene.materials[instance.materials[0]];
        assert_eq!(material.texture, Some(base_texture));
        assert_eq!(
            material.transmap, None,
            "road holes must not cut mapped geometry"
        );
        assert_eq!(
            material.nightmap, ground_base.nightmap,
            "retain the tile's illumination"
        );
        assert!(material.nightmap.is_some());
        assert!(!instance.ground_layer);
    }
}

#[test]
fn surf_height_map_is_loaded_from_a_mounted_zip_sidecar() {
    use std::io::Write;

    let fixture = Fixture::new();
    let archive_path = fixture.0.join("surf-pack.zip");
    let mut bmp = Vec::new();
    image::codecs::bmp::BmpEncoder::new(&mut bmp)
        .encode(&[255, 0, 0, 255], 1, 1, image::ExtendedColorType::Rgba8)
        .unwrap();
    let mut archive = zip::ZipWriter::new(std::fs::File::create(&archive_path).unwrap());
    for file in ["Texture/road.bmp", "Texture/road.bmp.surf"] {
        archive
            .start_file(file, zip::write::SimpleFileOptions::default())
            .unwrap();
        archive.write_all(&bmp).unwrap();
    }
    archive.finish().unwrap();
    let mount = ::legacy_config::vfs::mount_zip(&archive_path).unwrap();
    let map = surface_height_map("road.bmp", &[mount.join("Texture")])
        .expect("the sidecar in the mounted content archive must load");
    assert!((map.sample(glam::Vec2::ZERO) - 0.02).abs() < 1e-6);
}

#[test]
fn flagged_terrain_cutters_share_the_drawn_ground_with_wheels_and_walkers() {
    let fixture = Fixture::new();
    fixture.write(
        "global.cfg",
        "[name]\nCut terrain regression\n[map]\n0\n0\ntile_0_0.map\n",
    );
    fixture.write(
        "portal.sco",
        "[absheight]\n[surface]\n[mesh]\nfloor.x\n[terrainhole]\nhole.x\n",
    );
    let floor = "xof 0303txt 0032\nMesh floor {4;101;7;101;,109;7;101;,109;7;109;,101;7;109;;2;3;0,2,1;,3;0,3,2;;}";
    fixture.write("floor.x", floor);
    fixture.write("hole.x", floor);
    fixture.write("tile_0_0.map", "[version]\n14\n[terrain]\n\n[object]\n0\nportal.sco\n1\n0\n0\n0\n0\n0\n0\n0\n[spline_terrain_align]\n\n[object]\n0\nportal.sco\n2\n30\n0\n0\n0\n0\n0\n0\n");
    let mut terrain = Terrain::flat();
    terrain.heights.fill(10.0);
    std::fs::write(fixture.0.join("tile_0_0.map.terrain"), terrain.to_bytes()).unwrap();
    let world = World::open(&fixture.0, &fixture.0.join("global.cfg"), 20261001).unwrap();
    let (prepared, _) = world.prepare_tiles(&[(0, 0, fixture.0.join("tile_0_0.map"))]);
    let mesh = prepared[0].terrain.as_ref().unwrap();
    let cast = |x: f32, y: f32| {
        ::geometry::ray_mesh(
            glam::Vec3::new(x, y, 20.0),
            -glam::Vec3::Z,
            mesh,
            &Mat4::IDENTITY,
        )
        .map(|d| 20.0 - d)
    };
    assert!(cast(103.0, 103.0).is_none(), "flagged portal cuts");
    assert_eq!(
        cast(133.0, 103.0),
        Some(10.0),
        "unflagged portal keeps the hill"
    );
    assert!((world.walk_height(103.0, 103.0).unwrap() - 7.0).abs() < 1e-4);
    assert!((world.walk_height(133.0, 103.0).unwrap() - 10.0).abs() < 1e-4);
    let h = cast(100.5, 103.0).unwrap() as f64;
    assert!(h > 7.0 && h < 10.0);
    let probe = drive_probe(&world.terrains, &world.surfaces, 100.5, 103.0, 20.0);
    assert!(
        (probe.below.unwrap() - h).abs() < 1e-4,
        "wheel: {probe:?}, rendered: {h}"
    );
    assert!((world.walk_height(100.5, 103.0).unwrap() - h).abs() < 1e-4);
    let empty = drive_probe(&world.terrains, &world.surfaces, 104.0, 104.0, 6.0);
    assert!(
        empty.below.is_none(),
        "the original hill must not reappear inside the cut: {empty:?}"
    );
}

#[test]
fn tree_cards_keep_their_uvs_at_zero_and_turn_with_positive_heading() {
    let mesh = tree_quad_mesh();
    for heading in [0.0_f64, 86.4, 90.0] {
        let xf = tree_transform(heading, 2.0, 3.0);
        let p = xf.transform_point3(mesh.positions[1]);
        let angle = heading.to_radians();
        assert!(p.distance(glam::Vec3::new(angle.cos() as f32, angle.sin() as f32, 0.0)) < 1e-5);
        assert_eq!(mesh.uvs[0], glam::Vec2::new(0.0, 1.0));
        assert_eq!(mesh.uvs[1], glam::Vec2::new(1.0, 1.0));
        assert!(xf.transform_point3(mesh.positions[2]).z == 3.0);
    }
}
