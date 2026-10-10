use super::build::*;
use super::*;
use glam::DVec3;

#[test]
fn materials_follow_the_paths_over_them() {
    let cfg = ::texture::TextureCfg::default();
    // whatever it is called: the street paths run over it
    assert_eq!(classify("asphalt1", &cfg, [500.0, 300.0, 0.0, 0.0]), Some(Surface::Road));
    assert_eq!(classify("xyz_01", &cfg, [500.0, 300.0, 0.0, 0.0]), Some(Surface::Road));
    // a pavement the pedestrians walk on, a track bed under the rails
    assert_eq!(classify("str_side1", &cfg, [400.0, 0.0, 200.0, 0.0]), Some(Surface::Footway));
    assert_eq!(classify("schotter", &cfg, [400.0, 0.0, 0.0, 300.0]), Some(Surface::Track));
    assert_eq!(classify("schotter", &cfg, [400.0, 0.0, 0.0, 10.0]), None);
    // asphalt no car drives on is still paved ground
    assert_eq!(classify("str_asphdrk", &cfg, [400.0, 0.0, 0.0, 0.0]), Some(Surface::Paved));
    // too little seen to judge: the name decides
    assert_eq!(classify("gehweg_platten", &cfg, [10.0, 10.0, 0.0, 0.0]), Some(Surface::Footway));
    assert_eq!(classify("wall_brick", &cfg, [400.0, 0.0, 0.0, 0.0]), None);
    let grass = ::texture::TextureCfg {
        terrain_mapping: true,
        ..Default::default()
    };
    assert_eq!(classify("verge", &grass, [400.0, 0.0, 0.0, 0.0]), Some(Surface::Green));
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
    let cfg = ::legacy_config::resolve_path(&root, &map);
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

fn draw(map: &SurfaceMap, lanes: &[::simulation::traffic::Lane], c: DVec2, radius: f64, path: &str) {
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
        Layer::Green => [38, 58, 40],
        Layer::Footway => [62, 64, 70],
        Layer::Track => [70, 58, 50],
        Layer::Drivable => [120, 122, 132],
        Layer::Paved => [92, 94, 102],
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
        for l in lanes.iter().filter(|l| l.kind == ::simulation::traffic::LaneKind::Street) {
            let col = if l.invisible { [200, 80, 200] } else { [60, 140, 255] };
            for ab in l.points.windows(2) {
                line(&mut img, to_px(ab[0].truncate()), to_px(ab[1].truncate()), col);
            }
        }
    }
    let _ = DVec3::ZERO;
    img.save(path).unwrap();
}
