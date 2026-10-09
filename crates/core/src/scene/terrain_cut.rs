use super::*;

impl World {
    /// Surface rasters: cut the terrain under roads and crossings, keep their heights (for
    /// the wheels and the feet), and decode the textures the upload will want. The roads that
    /// reach a tile and the surfaces and cutters of the tiles next to it count as much as its
    /// own, each as it finally stands.
    pub(super) fn cut_terrain(
        &self,
        prepared: &mut [Prepared],
        staged: &HashMap<(i32, i32), Arc<StagedTile>>,
        layout: &TileLayout,
    ) {
        let debug_raster = ::legacy_config::env::var("OMSI_DEBUG_RASTER").ok().and_then(|q| {
            q.split_once(',')
                .and_then(|(a, b)| Some((a.parse::<f64>().ok()?, b.parse::<f64>().ok()?)))
        });
        let check_roads = ::legacy_config::env::var_os("OMSI_CHECK_ROADS").is_some();
        let debug = ::legacy_config::env::var_os("OMSI_DEBUG_SPLINES").is_some();
        type Check = (usize, usize, Vec<(f64, f64, f32)>);
        let debug_physics = ::legacy_config::env::var_os("OMSI_DEBUG_PHYSICS").is_some();
        let results: Vec<(Arc<TileSurface>, Option<Image>, Check, usize)> = prepared
            .par_iter_mut()
            .map(|p| {
                let key = (p.tx, p.ty);
                let (tx, ty) = key;
                let (x0, y0) = (tx as f64 * tile_size(), ty as f64 * tile_size());
                let (x1, y1) = (x0 + tile_size(), y0 + tile_size());
                let outside = |b: &[f64; 4]| b[2] < x0 || b[0] > x1 || b[3] < y0 || b[1] > y1;
                let src = Self::sources(layout, staged, key);
                let mut order: Vec<&Arc<StagedTile>> = src.values().collect();
                order.sort_by_key(|q| (q.tx, q.ty));
                let mut ts = TileSurface::new(SURFACE_RASTER);
                // meshes the wheels stand on, and of them low objects they climb
                let mut wheel_meshes = 0usize;
                let report = |mesh: &MeshData,
                              xf: &Mat4,
                              o: DVec3,
                              b: &[f64; 4],
                              label: &dyn Fn() -> String| {
                    let Some((qx, qy)) = debug_raster else { return };
                    if qx < b[0] || qx > b[2] || qy < b[1] || qy > b[3] {
                        return;
                    }
                    let inside = mesh.indices.chunks_exact(3).any(|t| {
                        let w = |i: u32| {
                            let v = xf.transform_point3(mesh.positions[i as usize]).as_dvec3() + o;
                            (v.x, v.y)
                        };
                        let (a, bb, c) = (w(t[0]), w(t[1]), w(t[2]));
                        let s1 = (bb.0 - a.0) * (qy - a.1) - (bb.1 - a.1) * (qx - a.0);
                        let s2 = (c.0 - bb.0) * (qy - bb.1) - (c.1 - bb.1) * (qx - bb.0);
                        let s3 = (a.0 - c.0) * (qy - c.1) - (a.1 - c.1) * (qx - c.0);
                        (s1 >= 0.0 && s2 >= 0.0 && s3 >= 0.0)
                            || (s1 <= 0.0 && s2 <= 0.0 && s3 <= 0.0)
                    });
                    if inside {
                        log::info!("point ({qx},{qy}) covered by {} (bounds {:?})", label(), b);
                    }
                };
                // every road that reaches the tile; a railway embankment or a bridge deck is
                // a surface (the ground is cut under it) but not something the wheels stand
                // on: only splines that carry a road or footway path count as drivable
                for q in &order {
                    for sp in &q.splines {
                        // (a blended layer - Westcountry's lane darkeners over the painted
                        // ground of its junctions - cuts no ground away: under it the ground
                        // is what shows through, and cut away it was the sky)
                        // (nor do wires overhead: see `SPLINE_OVERHEAD`)
                        if !sp.cuts_terrain || sp.overlay || outside(&sp.bounds) {
                            continue;
                        }
                        report(&sp.shape, &Mat4::IDENTITY, q.origin, &sp.bounds, &|| {
                            format!("spline {}", sp.ty.def.path.display())
                        });
                        ts.rasterize_kind(
                            &sp.shape,
                            &Mat4::IDENTITY,
                            q.origin,
                            tx,
                            ty,
                            sp.drivable,
                        );
                    }
                    // The drawn spline mesh is the wheel surface; height profiles remain the
                    // optional A/B fallback selected by OMSI_HEIGHTPROFILE_GROUND.
                    for drive in &q.drive {
                        let bounds = match drive {
                            StagedDrive::HeightProfiles(_, bounds)
                            | StagedDrive::DrawnMesh { bounds, .. } => bounds,
                        };
                        if outside(bounds) {
                            continue;
                        }
                        match drive {
                            StagedDrive::HeightProfiles(mesh, _) => ts.add_height_profiles(
                                mesh,
                                scenery_draw_position(q.origin, true),
                                tx,
                                ty,
                            ),
                            StagedDrive::DrawnMesh {
                                mesh, surface_maps, ..
                            } => ts.add_drive_mesh(
                                mesh,
                                &Mat4::IDENTITY,
                                scenery_draw_position(q.origin, true),
                                tx,
                                ty,
                                surface_maps.as_deref(),
                            ),
                        }
                        wheel_meshes += 1;
                    }
                }
                // the surfaces and [terrainhole] cutters of this tile and the ones around it
                for q in &order {
                    if (q.tx - tx).abs() > 1 || (q.ty - ty).abs() > 1 {
                        continue;
                    }
                    let Some(res) =
                        self.resolve((q.tx, q.ty), &Self::sources(layout, staged, (q.tx, q.ty)))
                    else {
                        continue;
                    };
                    if ::legacy_config::env::var_os("OMSI_NO_SPLINE_HOLES").is_none() {
                        for ring in &q.hole_outlines {
                            ts.add_outline(ring, tx, ty);
                        }
                    }
                    for (o, pose) in q.objects.iter().zip(res.poses.iter()) {
                        let Some(pose) = pose else { continue };
                        let ot = &o.ot;
                        // Editor-only helpers and trees do not cut terrain.
                        if ot.sco.tree.is_some() || ot.sco.only_editor || ot.sco.is_help_arrow {
                            continue;
                        }
                        for h in &ot.holes {
                            if !outside(&mesh_bounds(h, &pose.rot, pose.pos)) {
                                ts.rasterize_hole(h, &pose.rot, pose.pos, tx, ty);
                                // and cut exactly along its rim, as along a spline's outline:
                                // by texel alone the ground stood a metre into the road at
                                // the edges of a junction (Spandau, Bahnstr./Hansastr.)
                                for ring in
                                    ::geometry::hole_mesh_outlines(h, &pose.rot, pose.pos)
                                {
                                    if !::geometry::outline_crosses_itself(&ring) {
                                        ts.add_outline(&ring, tx, ty);
                                    }
                                }
                            }
                        }
                        // An explicit cutter is independent of the object's render meshes.
                        if ot.meshes.is_empty() {
                            continue;
                        }
                        // Laid on the ground (the terrain is cut under it): a `[surface]` object
                        // and one drawn as a ground layer (`[rendertype]`).
                        let surface = ot.sco.render_type.is_ground_layer() || ot.sco.surface;
                        if !surface {
                            continue;
                        }
                        let meshes = ot.meshes.iter().map(|(m, _, _)| m);
                        // What the wheels stand on is Omsi.exe's ground query (0x7a0814): the
                        // terrain, the splines, and of the objects only the `[surface]` ones
                        // (the tile's list of them, 0x79eb63) - and of those only the first
                        // `[mesh]` of the model, a ray cast down into it (0x5f9218 with only
                        // mesh 0). A collision mesh is never ground (it only shapes the crash
                        // body), nor is an object drawn as a ground layer without `[surface]`
                        // (the road markings), nor are the other meshes of a surface object
                        // (the Spandau depot's buildings stand on its yard, `Betr_S_Boden`,
                        // its first mesh). Every one of those lifted the wheels here: the bus
                        // hopped over markings, low collision meshes and whatever a surface
                        // object carried - bumps nobody could see.
                        let ground_mesh = ot
                            .sco
                            .surface
                            .then(|| ot.mesh_def_index.iter().position(|&d| d == 0))
                            .flatten();
                        for (k, mesh) in meshes.enumerate() {
                            let b = mesh_bounds(mesh, &pose.rot, pose.pos);
                            if outside(&b) {
                                continue;
                            }
                            report(mesh, &pose.rot, pose.pos, &b, &|| {
                                format!(
                                    "object {} rendertype={:?} surface={}",
                                    ot.sco.path.display(),
                                    ot.sco.render_type,
                                    ot.sco.surface
                                )
                            });
                            ts.rasterize_kind(mesh, &pose.rot, pose.pos, tx, ty, true);
                            if Some(k) == ground_mesh {
                                ts.add_drive_mesh(
                                    mesh,
                                    &pose.rot,
                                    scenery_draw_position(pose.pos, true),
                                    tx,
                                    ty,
                                    ot.surface_maps.as_deref(),
                                );
                                wheel_meshes += 1;
                            }
                        }
                    }
                }
                ts.finish();
                let tile_terrain = self.terrains.read().get(&key).cloned();
                // How much of the ground the old cut rule ("anything below the terrain takes
                // it away") would have removed with nothing to put in its place: a hole in
                // the world you can see the sky through.
                let mut check: Check = (0, 0, Vec::new());
                if let (true, Some(t)) = (check_roads, tile_terrain.as_ref()) {
                    let n = ts.size;
                    let cell = tile_size() as f32 / n as f32;
                    for j in 0..n {
                        for i in 0..n {
                            let k = j * n + i;
                            if !ts.covered(k) {
                                continue;
                            }
                            check.1 += 1;
                            let th = t.sample((i as f32 + 0.5) * cell, (j as f32 + 0.5) * cell);
                            // the ground over a road: shows through it (Omsi.exe cuts nothing)
                            if ts.road_covered(k)
                                && th > ts.road_height(k) + 0.03
                                && th < ts.road_height(k) + 1.5
                                && !ts.cut_at(
                                (i as f32 + 0.5) * cell,
                                (j as f32 + 0.5) * cell,
                                th,
                                surface_flush(),
                            )
                            {
                                OVER_ROAD.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
                                if let Ok(mut w) = OVER_ROAD_AT.lock() {
                                    w.push((
                                        x0 + ((i as f32 + 0.5) * cell) as f64,
                                        y0 + ((j as f32 + 0.5) * cell) as f64,
                                        th - ts.road_height(k),
                                        th,
                                    ));
                                }
                            }
                            let old_rule = th >= ts.low_height(k) - surface_flush();
                            let new_rule = ts.low_height(k) - surface_flush() <= th
                                && th <= ts.height(k) + surface_flush();
                            if old_rule && !new_rule {
                                check.0 += 1;
                                if check.2.len() < 100 {
                                    check.2.push((
                                        x0 + (i as f32 * cell) as f64,
                                        y0 + (j as f32 * cell) as f64,
                                        th - ts.height(k),
                                    ));
                                }
                            }
                        }
                    }
                }
                let terrain_at = move |x: f32, y: f32| {
                    tile_terrain.as_ref().map(|t| t.sample(x, y)).unwrap_or(0.0)
                };
                // the hole the roads cut into the ground, as an alpha image in tile space
                let cut = if ts.cuts_anything(&terrain_at, surface_flush()) {
                    if debug {
                        log::info!("tile ({tx}, {ty}): terrain cut under flush surfaces");
                    }
                    let rgba = ts.mask_image(&terrain_at, surface_flush());
                    if let Some(dir) = ::legacy_config::env::var("OMSI_DUMP_CUT").ok() {
                        let a: Vec<u8> = rgba.chunks_exact(4).map(|p| p[3]).collect();
                        if let Some(img) =
                            image::GrayImage::from_raw(ts.size as u32, ts.size as u32, a)
                        {
                            let _ = image::imageops::flip_vertical(&img)
                                .save(format!("{dir}/cut_{tx}_{ty}.png"));
                        }
                    }
                    Some(Image {
                        width: ts.size as u32,
                        height: ts.size as u32,
                        rgba,
                        has_alpha: true,
                    })
                } else {
                    None
                };
                // the painted ground layers: where the roads cut the ground away the paint
                // goes too, and a layer with nothing left on the tile is not drawn
                let masks = std::mem::take(&mut p.paint_masks);
                p.paint = masks
                    .into_iter()
                    .filter_map(|(layer, img)| {
                        let (mut rgba, w, h) =
                            smooth_paint_mask(&img.rgba, img.width as usize, img.height as usize);
                        let img = Image {
                            width: w as u32,
                            height: h as u32,
                            rgba: Vec::new(),
                            has_alpha: true,
                        };
                        let mut painted = 0usize;
                        for j in 0..h {
                            for i in 0..w {
                                let a = &mut rgba[(j * w + i) * 4 + 3];
                                if let Some(c) = &cut {
                                    // The cut as the terrain's alpha test sees it: sampled
                                    // bilinearly at this texel's centre, cut below one half.
                                    // Taken from the nearest cut texel (1.5-3 m each), the
                                    // paint kept teeth over the hole the road left in the
                                    // ground - drawn on top of the carriageway, a staircase
                                    // of asphalt or cobbles reaching into the road.
                                    if bilinear_alpha(
                                        c,
                                        (i as f32 + 0.5) / w as f32,
                                        (j as f32 + 0.5) / h as f32,
                                    ) < 0.5
                                    {
                                        *a = 0;
                                    }
                                }
                                if *a > 8 {
                                    painted += 1;
                                }
                            }
                        }
                        (painted > 0).then(|| {
                            (
                                layer,
                                tile_texture(
                                    Image {
                                        width: img.width,
                                        height: img.height,
                                        rgba,
                                        has_alpha: true,
                                    },
                                    true,
                                ),
                                painted as f32 / (w * h).max(1) as f32,
                            )
                        })
                    })
                    .collect();
                (Arc::new(ts), cut, check, wheel_meshes)
            })
            .collect();
        let (mut holes, mut cells) = (0usize, 0usize);
        let mut where_: Vec<(f64, f64, f32)> = Vec::new();
        let (mut tris, mut wheel_meshes) = (0usize, 0usize);
        for (p, (ts, cut, check, wheels)) in prepared.iter_mut().zip(results) {
            p.cut = cut.map(|c| {
                if ::legacy_config::env::var_os("OMSI_CUT_PLAIN").is_some() {
                    ::texture::gpu::TextureData {
                        gpu_mips: false,
                        ..::texture::gpu::TextureData::from_image(c)
                    }
                } else {
                    tile_texture(c, true)
                }
            });
            tris += ts.drive.tris.len();
            wheel_meshes += wheels;
            // the wheel surfaces come and go with the tile (World::unload_tile)
            self.surfaces.write().insert((p.tx, p.ty), ts);
            holes += check.0;
            cells += check.1;
            where_.extend(check.2);
        }
        if debug_physics {
            log::info!(
                "wheel surfaces: {tris} faces on {} tiles from {wheel_meshes} meshes",
                prepared.len()
            );
        }
        if check_roads {
            where_.sort_by(|a, b| b.2.total_cmp(&a.2));
            log::info!(
                "ground-cut check: {holes} of {cells} covered ground points would be cut away with nothing under them ({:.2} %)",
                holes as f32 / cells.max(1) as f32 * 100.0
            );
            let over = OVER_ROAD.load(std::sync::atomic::Ordering::Relaxed);
            log::info!(
                "ground-over-road check: {over} of {cells} road points lie under the ground (3 cm to 1.5 m)"
            );
            if let Ok(mut w) = OVER_ROAD_AT.lock() {
                w.sort_by(|a, b| b.2.total_cmp(&a.2));
                let by = |lo: f32, hi: f32| w.iter().filter(|p| p.2 >= lo && p.2 < hi).count();
                log::info!(
                    "   by depth: 3-10 cm {}, 10-30 cm {}, 30-60 cm {}, 60 cm-1.5 m {}",
                    by(0.0, 0.1),
                    by(0.1, 0.3),
                    by(0.3, 0.6),
                    by(0.6, 9.0)
                );
                for (x, y, d, z) in w
                    .iter()
                    .filter(|p| p.2 > 0.08 && p.2 < 0.3)
                    .step_by(97)
                    .take(6)
                {
                    log::info!(
                        "   (shallow) ground {d:.2} m over the road at ({x:.1}, {y:.1}, {z:.1})"
                    );
                }
                for (x, y, d, _) in w.iter().take(8) {
                    log::info!("   ground {d:.2} m over the road at ({x:.1}, {y:.1})");
                }
            }
            for (x, y, d) in where_.iter().take(5) {
                log::info!("   {d:.1} m of nothing under the ground at ({x:.0}, {y:.0})");
            }
        }
        // The textures of object types, splines and trees that are not on the GPU yet are
        // decoded here instead of on the thread that draws. A big batch (a whole map at
        // once) decodes them as it uploads instead: all at once they would not fit.
        if prepared.len() <= 16 {
            for p in prepared.iter_mut() {
                for o in p.objects.iter_mut() {
                    let freetex =
                        o.ot.meshes
                            .iter()
                            .any(|(_, _, ov)| ov.iter().any(|m| !m.item && m.freetex.is_some()));
                    if o.lamp.is_some() || (o.ot.dynamic_textures.is_empty() && !freetex) {
                        continue;
                    }
                    if let Some(program) = o.ot.program.clone() {
                        o.script = Some(::simulation::scenery::SceneryInstance::new(
                            program,
                            &o.ot.mesh_defs(),
                            self.script_clock(),
                            &o.strings,
                        ));
                    }
                }
            }
            let mut wanted: Vec<(String, Vec<PathBuf>)> = prepared
                .iter()
                .flat_map(|p| self.wanted_textures(p))
                .collect();
            wanted.sort();
            wanted.dedup();
            let decoded: Vec<(PathBuf, Arc<TextureData>)> = wanted
                .par_iter()
                .filter_map(|(name, dirs)| {
                    let dirs_ref: Vec<&Path> = dirs.iter().map(|d| d.as_path()).collect();
                    let path = ::texture::find_texture(name, &dirs_ref)?;
                    let (img, _) = ::texture::gpu::load_gpu(&path).ok()?;
                    Some((path, Arc::new(img)))
                })
                .collect();
            let decoded: Arc<HashMap<PathBuf, Arc<TextureData>>> =
                Arc::new(decoded.into_iter().collect());
            for p in prepared.iter_mut() {
                p.images = decoded.clone();
            }
        }
    }
}
