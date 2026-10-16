use super::build::*;
use super::*;
use glam::DVec3;

#[test]
fn materials_follow_the_paths_over_them() {
    let cfg = ::texture::TextureCfg::default();
    // whatever it is called: the street paths run over it
    assert_eq!(classify("asphalt1", &cfg, [500.0, 300.0, 0.0, 0.0, 0.0]), Some(Surface::Road));
    assert_eq!(classify("xyz_01", &cfg, [500.0, 300.0, 0.0, 0.0, 0.0]), Some(Surface::Road));
    // a pavement the pedestrians walk on, a track bed under the rails
    assert_eq!(classify("str_side1", &cfg, [400.0, 0.0, 200.0, 0.0, 0.0]), Some(Surface::Footway));
    assert_eq!(classify("schotter", &cfg, [400.0, 0.0, 0.0, 300.0, 0.0]), Some(Surface::Track));
    assert_eq!(classify("schotter", &cfg, [400.0, 0.0, 0.0, 10.0, 0.0]), None);
    // asphalt no car drives on is still paved ground
    assert_eq!(classify("str_asphdrk", &cfg, [400.0, 0.0, 0.0, 0.0, 0.0]), Some(Surface::Paved));
    // too little seen to judge: the name decides
    assert_eq!(classify("gehweg_platten", &cfg, [10.0, 10.0, 0.0, 0.0, 0.0]), Some(Surface::Footway));
    assert_eq!(classify("wall_brick", &cfg, [400.0, 0.0, 0.0, 0.0, 0.0]), None);
    // a crossing is the carriageway with its stripes, named so or crossed by cars and people
    assert_eq!(classify("crosswalk_asphalt", &cfg, [5.0, 0.0, 5.0, 0.0, 0.0]), Some(Surface::Road));
    assert_eq!(classify("fussgaengerueberweg", &cfg, [5.0, 0.0, 5.0, 0.0, 0.0]), Some(Surface::Road));
    // a pedestrian zone is no crossing, whatever its name begins with
    assert_ne!(classify("fussgaengerzone_pflaster", &cfg, [400.0, 0.0, 300.0, 0.0, 0.0]), Some(Surface::Road));
    assert_eq!(classify("xyz_02", &cfg, [500.0, 300.0, 500.0, 0.0, 0.0]), Some(Surface::Road));
    // a cycle path stays a path; a cycle lane painted on the carriageway is the road's
    assert_eq!(classify("bikeline_u", &cfg, [400.0, 0.0, 300.0, 0.0, 0.0]), Some(Surface::Footway));
    assert_eq!(classify("bike_pik", &cfg, [1.0, 1.0, 0.0, 0.0, 0.0]), Some(Surface::Road));
    // no car drives on a cycle lane, but it lies at the street's height beside it
    assert_eq!(classify("radfahrstreifen_rot", &cfg, [400.0, 0.0, 0.0, 0.0, 400.0]), Some(Surface::Road));
    assert_eq!(classify("bikelane", &cfg, [400.0, 0.0, 0.0, 0.0, 400.0]), Some(Surface::Road));
    // up on the kerb it is a cycle track, a path
    assert_eq!(classify("bikelane", &cfg, [400.0, 0.0, 0.0, 0.0, 0.0]), Some(Surface::Footway));
    let grass = ::texture::TextureCfg {
        terrain_mapping: true,
        ..Default::default()
    };
    assert_eq!(classify("verge", &grass, [400.0, 0.0, 0.0, 0.0, 0.0]), Some(Surface::Green));
}

/// Draw the surfaces around a place of a real map into a PNG, from above, for a look at
/// what the navigator will show. `OMSI_ROOT`, `NAVMAP_MAP` (the map's global.cfg relative to
/// the root), `NAVMAP_AT` (`x,y` or part of a street sign's name), `NAVMAP_RADIUS` (m) and
/// `NAVMAP_OUT` (the PNG).
#[test]
#[ignore]
fn surfaces_of_a_real_map() {
    let root = std::path::PathBuf::from(std::env::var("OMSI_ROOT").expect("OMSI_ROOT"));
    let _ = crate::startup::content_dir();
    let map = std::env::var("NAVMAP_MAP").expect("NAVMAP_MAP");
    let cfg = ::mods::resolve_path(&root, &map);
    let world = crate::scene::World::open(&root, &cfg, 20261001).unwrap();
    world.index();
    let nav = world.navigation_map();
    let t0 = std::time::Instant::now();
    let surfaces = build_surface_map(&world, &nav.lanes);
    eprintln!(
        "surfaces built in {:.1} s: {} chunks, {} areas, {} triangles",
        t0.elapsed().as_secs_f64(),
        surfaces.chunks.len(),
        surfaces.chunks.values().map(|c| c.areas.len()).sum::<usize>(),
        surfaces.chunks.values().flat_map(|c| &c.areas).map(|a| a.tris.len() / 3).sum::<usize>()
    );
    {
        // a fingerprint of everything built, to tell a faster build that changed nothing
        use std::hash::{Hash, Hasher};
        let mut h = std::collections::hash_map::DefaultHasher::new();
        let mut keys: Vec<_> = surfaces.chunks.keys().copied().collect();
        keys.sort();
        for k in keys {
            k.hash(&mut h);
            let c = &surfaces.chunks[&k];
            for a in c.areas.iter().chain(&c.coarse) {
                (a.layer as u8, a.level, &a.tris).hash(&mut h);
                a.verts.iter().flatten().for_each(|v| v.to_bits().hash(&mut h));
                a.edges.iter().flatten().flatten().for_each(|v| v.to_bits().hash(&mut h));
            }
        }
        eprintln!("surfaces fingerprint {:016x}", h.finish());
    }
    for layer in Layer::ALL {
        let fine: usize = surfaces.chunks.values().flat_map(|c| &c.areas).filter(|a| a.layer == layer).map(|a| a.tris.len() / 3).sum();
        let coarse: usize = surfaces.chunks.values().flat_map(|c| &c.coarse).filter(|a| a.layer == layer).map(|a| a.tris.len() / 3).sum();
        let edges: usize = surfaces.chunks.values().flat_map(|c| &c.areas).filter(|a| a.layer == layer).flat_map(|a| &a.edges).map(|e| e.len()).sum();
        eprintln!("{layer:?}: {fine} triangles, {coarse} coarse, {edges} outline points");
    }
    let mut mats: Vec<&MaterialInfo> = surfaces.materials.iter().collect();
    mats.sort_by(|a, b| b.area.total_cmp(&a.area));
    for m in mats.iter().take(60) {
        eprintln!(
            "{:<36} {:>9.0} m² streets {:>3.0}% walks {:>3.0}% rails {:>3.0}% -> {:?}",
            m.name,
            m.area,
            m.car * 100.0,
            m.walk * 100.0,
            m.rail * 100.0,
            m.surface
        );
    }
    let radius: f64 = std::env::var("NAVMAP_RADIUS")
        .ok()
        .and_then(|v| v.parse().ok())
        .unwrap_or(150.0);
    let out = std::env::var("NAVMAP_OUT").unwrap_or_else(|_| "navmap.png".into());
    for at in std::env::var("NAVMAP_AT").unwrap_or_default().split(';') {
        let nums: Vec<f64> = at.split(',').filter_map(|v| v.trim().parse().ok()).collect();
        let centre = if nums.len() == 2 {
            DVec2::new(nums[0], nums[1])
        } else {
            let needle = at.to_lowercase();
            let Some(sign) = nav.signs.iter().find(|s| s.2.to_lowercase().contains(&needle)) else {
                eprintln!("no street sign '{at}'");
                continue;
            };
            sign.0.truncate()
        };
        let path = if at.is_empty() {
            out.clone()
        } else {
            out.replace(".png", &format!("_{}.png", at.replace([',', ' ', '.'], "_")))
        };
        draw(&surfaces, &nav.lanes, centre, radius, &path);
        eprintln!("{at}: ({:.0}, {:.0}) -> {path}", centre.x, centre.y);
    }
}

fn draw(map: &SurfaceMap, lanes: &[::traffic::Lane], c: DVec2, radius: f64, path: &str) {
    let size = 1200u32;
    let scale = size as f64 / (2.0 * radius);
    let mut img = image::RgbImage::from_pixel(size, size, image::Rgb([24, 26, 30]));
    let to_px = |p: DVec2| -> (f64, f64) {
        ((p.x - c.x) * scale + size as f64 / 2.0, size as f64 / 2.0 - (p.y - c.y) * scale)
    };
    let fill = |img: &mut image::RgbImage, t: [(f64, f64); 3], col: [u8; 3]| {
        let y0 = t.iter().map(|p| p.1).fold(f64::INFINITY, f64::min).max(0.0) as i64;
        let y1 = t.iter().map(|p| p.1).fold(f64::NEG_INFINITY, f64::max).min(size as f64 - 1.0) as i64;
        for y in y0..=y1 {
            let yc = y as f64 + 0.5;
            let (mut lo, mut hi) = (f64::INFINITY, f64::NEG_INFINITY);
            for k in 0..3 {
                let (a, b) = (t[k], t[(k + 1) % 3]);
                if (a.1 <= yc && b.1 >= yc) || (b.1 <= yc && a.1 >= yc) {
                    if (b.1 - a.1).abs() < 1e-9 {
                        continue;
                    }
                    let x = a.0 + (yc - a.1) * (b.0 - a.0) / (b.1 - a.1);
                    lo = lo.min(x);
                    hi = hi.max(x);
                }
            }
            if lo > hi {
                continue;
            }
            for x in (lo - 0.5).ceil().max(0.0) as i64..=((hi - 0.5).floor().min(size as f64 - 1.0)) as i64 {
                img.put_pixel(x as u32, y as u32, image::Rgb(col));
            }
        }
    };
    let line = |img: &mut image::RgbImage, a: (f64, f64), b: (f64, f64), col: [u8; 3]| {
        let n = ((b.0 - a.0).abs().max((b.1 - a.1).abs()).ceil() as usize).max(1);
        for k in 0..=n {
            let t = k as f64 / n as f64;
            let (x, y) = (a.0 + (b.0 - a.0) * t, a.1 + (b.1 - a.1) * t);
            if x >= 0.0 && y >= 0.0 && x < size as f64 && y < size as f64 {
                img.put_pixel(x as u32, y as u32, image::Rgb(col));
            }
        }
    };
    let colour = |l: Layer| match l {
        Layer::Water => [40, 62, 96],
        Layer::Green => [38, 58, 40],
        Layer::Footway => [62, 64, 70],
        Layer::Track => [70, 58, 50],
        Layer::Canopy => [44, 78, 52],
        Layer::Drivable => [120, 122, 132],
        Layer::Paved => [92, 94, 102],
        Layer::Building => [74, 74, 86],
        Layer::Marking => [225, 225, 230],
    };
    for level in [-1i8, 0, 1] {
        for layer in Layer::ALL {
            for (k, ch) in map.chunks_near(c, radius * 1.5) {
                let o = SurfaceMap::chunk_origin(k);
                for a in ch.areas.iter().filter(|a| a.layer == layer && a.level == level) {
                    let p: Vec<(f64, f64)> = a
                        .verts
                        .iter()
                        .map(|v| to_px(o + DVec2::new(v[0] as f64, v[1] as f64)))
                        .collect();
                    for t in a.tris.chunks_exact(3) {
                        fill(&mut img, [p[t[0] as usize], p[t[1] as usize], p[t[2] as usize]], colour(layer));
                    }
                }
            }
        }
        for (k, ch) in map.chunks_near(c, radius * 1.5) {
            let o = SurfaceMap::chunk_origin(k);
            for a in ch.areas.iter().filter(|a| a.layer == Layer::Drivable && a.level == level) {
                for e in &a.edges {
                    for ab in e.windows(2) {
                        let pa = to_px(o + DVec2::new(ab[0][0] as f64, ab[0][1] as f64));
                        let pb = to_px(o + DVec2::new(ab[1][0] as f64, ab[1][1] as f64));
                        line(&mut img, pa, pb, [200, 202, 210]);
                    }
                }
            }
        }
    }
    for r in &map.rails {
        for ab in r.windows(2) {
            line(&mut img, to_px(ab[0].truncate()), to_px(ab[1].truncate()), [170, 120, 90]);
        }
    }
    if std::env::var_os("NAVMAP_LANES").is_some() {
        for l in lanes.iter().filter(|l| l.kind == ::traffic::LaneKind::Street) {
            let col = if l.invisible { [200, 80, 200] } else { [60, 140, 255] };
            for ab in l.points.windows(2) {
                line(&mut img, to_px(ab[0].truncate()), to_px(ab[1].truncate()), col);
            }
        }
    }
    let _ = DVec3::ZERO;
    img.save(path).unwrap();
}

#[test]
fn a_strip_at_the_height_of_the_street_paths_beside_them_is_carriageway() {
    use ::scenery::{SplineProfile, SplineProfilePoint};
    let strip = |texture, x: [f32; 2], z: f32| SplineProfile {
        texture,
        points: x.iter().map(|&x| SplineProfilePoint { x, z, ..Default::default() }).collect(),
    };
    let profiles = vec![
        strip(0, [-3.5, 3.5], 0.0),  // the lanes
        strip(1, [3.5, 5.0], 0.0),   // a cycle lane painted beside them
        strip(2, [5.0, 6.5], 0.15),  // a cycle track up on the kerb
        strip(3, [12.0, 14.0], 0.0), // a strip far off the carriageway
    ];
    let streets = [(-1.75, 0.0), (1.75, 0.0)];
    assert!(level_with_streets(&profiles, 0, &streets));
    assert!(level_with_streets(&profiles, 1, &streets));
    assert!(!level_with_streets(&profiles, 2, &streets));
    assert!(!level_with_streets(&profiles, 3, &streets));
    // a texture no profile uses, a spline without a street path
    assert!(!level_with_streets(&profiles, 9, &streets));
    assert!(!level_with_streets(&profiles, 1, &[]));
}

/// A flat square face pair of `size` m at `at`, `above` m over the ground there.
fn square(at: (f64, f64, f64), size: f64, above: f64) -> Vec<PieceFace> {
    let (x, y, z) = at;
    let p = |dx: f64, dy: f64| DVec3::new(x + dx, y + dy, z);
    [[p(0.0, 0.0), p(size, 0.0), p(size, size)], [p(0.0, 0.0), p(size, size), p(0.0, size)]]
        .into_iter()
        .map(|w| PieceFace {
            w,
            uv: [[0.0; 2]; 3],
            ids: (0, 0),
            beside: false,
            above,
        })
        .collect()
}

#[test]
fn a_deck_lies_on_one_level_however_its_faces_straddle_the_gap() {
    // a bridge deck some 4 m over a canal's bed: its faces just below and just above the gap
    let mut deck = square((0.0, 0.0, 0.0), 10.0, 3.9);
    deck.extend(square((10.0, 0.0, 0.0), 10.0, 4.3));
    deck.extend(square((20.0, 0.0, 0.0), 10.0, 4.2));
    assert_eq!(piece_levels(&deck), vec![1; 6]);
    assert_eq!(piece_levels(&square((0.0, 0.0, 0.0), 10.0, 0.1)), vec![0; 2]);
    assert_eq!(piece_levels(&square((0.0, 0.0, 0.0), 10.0, -6.0)), vec![-1; 2]);
    assert!(piece_levels(&[]).is_empty());
}

#[test]
fn a_deck_and_the_ground_of_one_object_keep_their_own_levels() {
    // equal areas at 5 m and at 0 m: on average 2.5 m, but a deck over the ground
    let mut piece = square((0.0, 0.0, 5.0), 10.0, 5.0);
    piece.extend(square((20.0, 0.0, 0.0), 10.0, 0.0));
    assert_eq!(piece_levels(&piece), vec![1, 1, 0, 0]);
    // and a pavement on the deck, 0.2 m above its road, goes with the deck
    piece.extend(square((0.0, 10.0, 5.2), 3.0, 5.2));
    assert_eq!(piece_levels(&piece), vec![1, 1, 0, 0, 1, 1]);
}

#[test]
fn a_raised_floor_is_kept_where_paths_run_over_it_and_a_roof_is_not() {
    use ::simulation::traffic::{LaneBuilder, LaneKind};
    // a street across a bridge object 4 m over the ground, at z 4
    let road = LaneBuilder::polyline(
        vec![DVec3::new(-10.0, 5.0, 4.0), DVec3::new(30.0, 5.0, 4.0)],
        LaneKind::Street,
        3.0,
    );
    let ev = Evidence::new(&[road]);
    // the deck the street runs over, its pavement beside it (no path on it), its roof (a
    // canopy 6 m higher) and its foot on the ground
    let deck = || {
        let mut v = square((0.0, 0.0, 4.0), 10.0, 4.0);
        v.extend(square((0.0, 10.0, 4.2), 3.0, 4.2));
        v.extend(square((0.0, 0.0, 10.0), 10.0, 10.0));
        v.extend(square((40.0, 0.0, 0.0), 5.0, 0.0));
        v
    };
    let mut piece = deck();
    keep_floors(&mut piece, false, &ev);
    let zs: Vec<f64> = piece.iter().map(|f| f.w[0].z).collect();
    assert_eq!(zs, vec![4.0, 4.0, 4.2, 4.2, 0.0, 0.0], "the deck, its pavement and the foot");
    // no path over it: a storey or a roof, only the foot stays
    let mut piece = deck();
    keep_floors(&mut piece, false, &Evidence::new(&[]));
    assert!(piece.iter().all(|f| f.above <= 1.5));
    // an object with a street of its own keeps everything
    let mut piece = deck();
    keep_floors(&mut piece, true, &Evidence::new(&[]));
    assert_eq!(piece.len(), 8);
    // tracks over it: a railway bridge - no building, but its deck is not kept either, so
    // the road under it stays in sight
    let rails = LaneBuilder::polyline(
        vec![DVec3::new(-10.0, 5.0, 4.0), DVec3::new(30.0, 5.0, 4.0)],
        LaneKind::Rail,
        3.0,
    );
    let mut piece = deck();
    assert!(keep_floors(&mut piece, false, &Evidence::new(&[rails])), "a bridge");
    assert!(piece.iter().all(|f| f.above <= 1.5));
    // nothing over it: a house
    let mut piece = deck();
    assert!(!keep_floors(&mut piece, false, &Evidence::new(&[])));
}

fn area_of_layer(chunk: &Chunk, layer: Layer) -> f32 {
    chunk
        .areas
        .iter()
        .filter(|a| a.layer == layer)
        .map(|a| {
            a.tris
                .chunks_exact(3)
                .map(|t| {
                    let [p, q, r] = [t[0], t[1], t[2]].map(|k| glam::Vec2::from(a.verts[k as usize]));
                    (q - p).perp_dot(r - p).abs() * 0.5
                })
                .sum::<f32>()
        })
        .sum()
}

#[test]
fn the_ground_under_a_tiles_water_is_water() {
    let t = ::map::tile_size() as f32;
    let flat = ::map::Terrain::flat();
    let area = |tris: &[[glam::Vec2; 3]]| -> f32 {
        tris.iter().map(|p| (p[1] - p[0]).perp_dot(p[2] - p[0]).abs() * 0.5).sum()
    };
    // over flat ground: all of it, or (the water 5 m down, as a tile without a file has it)
    // none of it
    assert!((area(&under_water(&flat, [1.0; 4])) - t * t).abs() < 1.0);
    assert!(under_water(&flat, [-5.0; 4]).is_empty());
    // a surface tilted across the tile: the half below it
    let half = area(&under_water(&flat, [-1.0, 1.0, -1.0, 1.0]));
    assert!((half - t * t / 2.0).abs() < t, "{half}");
}

#[test]
fn a_roof_is_a_building_and_a_lamp_or_a_slab_is_not() {
    use glam::Vec3;
    let quad = |x0: f32, y0: f32, x1: f32, y1: f32, z: f32| {
        [
            [Vec3::new(x0, y0, z), Vec3::new(x1, y0, z), Vec3::new(x1, y1, z)],
            [Vec3::new(x0, y0, z), Vec3::new(x1, y1, z), Vec3::new(x0, y1, z)],
        ]
    };
    assert_eq!(roof_of(&quad(0.0, 0.0, 10.0, 8.0, 9.0)).len(), 2, "a house's roof");
    assert!(roof_of(&quad(-0.4, -0.4, 0.4, 0.4, 6.0)).is_empty(), "a lamp's head");
    assert!(roof_of(&quad(0.0, 0.0, 20.0, 0.6, 6.0)).is_empty(), "a gantry's beam");
    assert!(roof_of(&quad(0.0, 0.0, 10.0, 8.0, 0.3)).is_empty(), "a slab on the ground");
}

#[test]
fn roofs_and_crowns_become_buildings_and_a_canopy() {
    let roof = |x0: f32, y0: f32, x1: f32, y1: f32| {
        [
            Shape::Roof([[x0, y0], [x1, y0], [x1, y1]]),
            Shape::Roof([[x0, y0], [x1, y1], [x0, y1]]),
        ]
    };
    let mut shapes: Vec<Shape> = Vec::new();
    shapes.extend(roof(10.0, 10.0, 20.0, 20.0));
    // a railing along it, 0.6 m wide: no building
    shapes.extend(roof(25.0, 10.0, 45.0, 10.6));
    // a row of trees 5 m apart: one canopy
    for k in 0..5 {
        shapes.push(Shape::Crown([10.0 + 5.0 * k as f32, 40.0], 3.0));
    }
    // a pond, 40 m by 12 (its corners rounded off by the smoothing of the banks)
    shapes.push(Shape::Water([[15.0, 48.0], [55.0, 48.0], [55.0, 60.0]]));
    shapes.push(Shape::Water([[15.0, 48.0], [55.0, 60.0], [15.0, 60.0]]));
    let mut chunk = Chunk::default();
    add_shapes(&mut chunk, &shapes);
    let buildings: Vec<_> = chunk.areas.iter().filter(|a| a.layer == Layer::Building).collect();
    assert_eq!(buildings.len(), 1, "the house, not the railing");
    assert!((area_of_layer(&chunk, Layer::Building) - 100.0).abs() < 6.0);
    assert_eq!(chunk.areas.iter().filter(|a| a.layer == Layer::Canopy).count(), 1);
    assert!(area_of_layer(&chunk, Layer::Canopy) > 100.0);
    let water = area_of_layer(&chunk, Layer::Water);
    assert!(water > 480.0 * 0.85 && water < 485.0, "{water}");
    assert!(chunk.areas.iter().all(|a| a.level == 0));
}

#[test]
fn a_tree_is_known_by_a_word_of_its_name_not_by_letters_anywhere_in_its_path() {
    let named = |p: &str| tree_named(std::path::Path::new(p));
    assert!(named("Sceneryobjects/Natur/Strassenbaum_01.sco"));
    assert!(named("Sceneryobjects/Natur/Linde-gross.sco"));
    assert!(named("Sceneryobjects/Bäume/Kiefer_02.sco"));
    assert!(named("Sceneryobjects/Generic/tree_oak.sco"));
    assert!(named("G:/OMSI 2/Sceneryobjects/Bäume/Laub/Ahorn.sco"));
    // (not by a folder outside the game's objects)
    assert!(!named("C:/Users/Baum/OMSI 2/Sceneryobjects/Haeuser/Haus.sco"));
    // a building under a folder with "tree" in it, a rail switch, a DIY store
    assert!(!named("Sceneryobjects/StreetObjects/Wohnhaus_3.sco"));
    assert!(!named("Sceneryobjects/Gleis/Weiche_links.sco"));
    assert!(!named("Sceneryobjects/Gewerbe/Baumarkt.sco"));
    assert!(!named("Sceneryobjects/Streets/Haus.sco"));
}
