use super::*;

/// An object as the object editor has left it: moved, turned about its place, or gone.
pub(super) fn show_edit(renderer: &Renderer, scene: &mut Scene, eo: &EditObject, e: ObjectEdit) {
    // (a deleted one goes deep under the ground rather than being hidden: its instances'
    // visibility is the level-of-detail switch's, and an undone delete brings it back)
    let rot = Mat4::from_rotation_z(-(e.turned.to_radians() as f32)) * eo.xf;
    let at = eo.pos + e.moved
        - if e.deleted {
        DVec3::Z * 10_000.0
    } else {
        DVec3::ZERO
    };
    for inst in &eo.instances {
        renderer.set_transform(scene, *inst, at, rot);
    }
}

/// Stop drawing an instance, keeping its other parameters.
pub(super) fn hide_instance(renderer: &Renderer, scene: &mut Scene, inst: usize) {
    let Some(i) = scene.instances.get(inst) else {
        return;
    };
    let (alpha, uv) = (i.slot_alpha.clone(), i.slot_uv.clone());
    renderer.set_params(scene, inst, &alpha, false, &uv);
}

pub(super) fn fallen_pole(xf: Mat4, push: DVec3) -> Mat4 {
    let dir = glam::Vec3::new(push.x as f32, push.y as f32, 0.0).normalize_or(glam::Vec3::Y);
    let axis = glam::Vec3::Z.cross(dir).normalize_or(glam::Vec3::X);
    Mat4::from_axis_angle(axis, 86f32.to_radians()) * xf
}

/// How many tiles OMSI keeps loaded around the camera's own: its `[performance_tiledistmax]`,
/// 1 in the shipped options.cfg and in the presets maps ask for (Chicago Downtown's manual:
/// "Set neighbor tiles count to 1 or max. 2").
pub(super) const OMSI_TILE_DIST: i32 = 1;

/// Where the camera has to stand for a stand-in for far tiles to be drawn: the ground of
/// the tiles OMSI loads with the one it is on. None for every other object.
///
/// OMSI has a tile's objects only while the camera is at most `OMSI_TILE_DIST` tiles away
/// from it, and maps build on that: Chicago Downtown puts a model of the whole city at Navy
/// Pier (`LOD_247.sco`, 5.5 km across, its parks and the lake as flat faces 2 m above the
/// streets) to fill the view beyond the tiles loaded there, with a hole where they are.
/// neoOMSI keeps the tiles of its whole view distance, so the model was there from
/// Columbus Drive on as well, its grass over the streets, the lower level and the vehicles
/// on it (#650). A stand-in is told apart by its size: more than twice as wide as all the
/// tiles OMSI has loaded with it (Chicago's are 3.6 to 8 km, its largest real objects -
/// Navy Pier, the Merchandise Mart, the road grids of whole tiles - at most 1.25 km), so
/// the far view keeps every ordinary object.
pub(super) fn stand_in_area(ot: &ObjectType, xf: &Mat4, pos: DVec3, tile: (i32, i32)) -> Option<[f64; 4]> {
    let ts = tile_size();
    let loaded = (2 * OMSI_TILE_DIST + 1) as f64 * ts;
    let wide = ot.meshes.iter().any(|(m, _, _)| {
        let b = mesh_bounds(m, xf, pos);
        (b[2] - b[0]).max(b[3] - b[1]) > 2.0 * loaded
    });
    if !wide {
        return None;
    }
    log::debug!(
        "{} on tile {tile:?} stands in for far tiles: drawn only from the tiles around it",
        ot.sco.path.display()
    );
    Some([
        (tile.0 - OMSI_TILE_DIST) as f64 * ts,
        (tile.1 - OMSI_TILE_DIST) as f64 * ts,
        (tile.0 + OMSI_TILE_DIST + 1) as f64 * ts,
        (tile.1 + OMSI_TILE_DIST + 1) as f64 * ts,
    ])
}

/// World bounds (min x, min y, max x, max y) of a mesh placed with `xf` at `origin`.
pub(super) fn mesh_bounds(m: &MeshData, xf: &Mat4, origin: DVec3) -> [f64; 4] {
    let mut b = [f64::MAX, f64::MAX, f64::MIN, f64::MIN];
    for p in &m.positions {
        let w = xf.transform_point3(*p).as_dvec3() + origin;
        b[0] = b[0].min(w.x);
        b[1] = b[1].min(w.y);
        b[2] = b[2].max(w.x);
        b[3] = b[3].max(w.y);
    }
    b
}

impl World {
    /// Stand the objects of a staged tile on its final ground, hang the attached ones on
    /// their parents, and register what the traffic, the passengers, the collisions and the
    /// lights need from them.
    pub(super) fn place_tile(
        &self,
        key: (i32, i32),
        staged: &HashMap<(i32, i32), Arc<StagedTile>>,
        layout: &TileLayout,
        stats: &Mutex<LoadStats>,
    ) -> Option<Prepared> {
        let src = Self::sources(layout, staged, key);
        let st = src.get(&key)?.clone();
        let res = self.resolve(key, &src)?;
        let (tx, ty) = key;
        let first_load = self.seeded.lock().insert(key);
        let terrain: &Terrain = &res.terrain;
        self.terrains.write().insert(key, res.terrain.clone());
        let ground_at = |x: f64, y: f64| -> f64 {
            let lx = (x - st.origin.x).clamp(0.0, tile_size()) as f32;
            let ly = (y - st.origin.y).clamp(0.0, tile_size()) as f32;
            terrain.sample(lx, ly) as f64
        };
        let mut state = TileState::default();
        let mut lanes: Vec<Lane> = if first_load {
            std::mem::take(&mut *st.lanes.lock())
        } else {
            Vec::new()
        };
        let mut parked_cars: Vec<(DVec3, f64)> = Vec::new();
        let mut objects: Vec<PlacedObject> = Vec::new();
        let mut trees = Vec::new();
        let debug_objects = ::legacy_config::env::var_os("OMSI_DEBUG_OBJECTS").is_some();
        let check_objects = ::legacy_config::env::var_os("OMSI_CHECK_OBJECTS").is_some();
        let debug_float = ::legacy_config::env::var_os("OMSI_DEBUG_FLOAT").is_some();
        let index = self.index();
        for (o, fp) in st.objects.iter().zip(res.poses.iter()) {
            let Some(Pose { pos, rot: xf }) = *fp else {
                continue;
            };
            if let (true, Placement::Ground { x, y, .. }) = (debug_float, &o.place) {
                // how far the object stood off the ground when it was placed before the
                // roads and crossings had pulled the ground about
                let (lx, ly) = (
                    (x - st.origin.x).clamp(0.0, tile_size()) as f32,
                    (y - st.origin.y).clamp(0.0, tile_size()) as f32,
                );
                let moved = terrain.sample(lx, ly) - st.base_terrain.sample(lx, ly);
                if moved.abs() > 0.3 {
                    log::info!(
                        "float: {} id {} at ({:.1}, {:.1}): the ground under it moved {:+.2} m (it stood {:.2} m {} before)",
                        o.ot.sco.path.display(),
                        o.id,
                        x,
                        y,
                        moved,
                        moved.abs(),
                        if moved < 0.0 {
                            "in the air"
                        } else {
                            "in the ground"
                        }
                    );
                }
            }
            let ot = o.ot.clone();
            let heading = Pose { pos, rot: xf }.heading();
            // (a spline attachment row's first object stands for the row: an entry point or a
            // stop put on a road is found by its id)
            if o.map_object || o.instance == 0 {
                self.object_positions
                    .lock()
                    .insert(o.id, (pos, [heading, 0.0, 0.0]));
                let mut dups = self.object_dups.lock();
                if let Some(d) = dups.get_mut(&(key, o.id)) {
                    *d = (pos, [heading, 0.0, 0.0]);
                }
            }
            if o.parked {
                state.parked_count += 1;
                self.parked_live
                    .fetch_add(1, std::sync::atomic::Ordering::Relaxed);
            }
            if o.parked && first_load {
                // on the ground: the lane match of the traffic is by distance in 3D
                parked_cars.push((pos, heading));
            }
            if ot.sco.is_bus_stop {
                state.bus_stops.push((
                    o.id,
                    pos,
                    heading,
                    o.extra.first().cloned().unwrap_or_default(),
                ));
            }
            if let Some(rel) = &ot.sco.passenger_cabin {
                // waiting places: an object's `[passpos]` in its own frame (the editor-only
                // markers have no mesh, so this comes before that test)
                let dir = ot
                    .sco
                    .path
                    .parent()
                    .map(|d| d.to_path_buf())
                    .unwrap_or_default();
                if let Some(cabin) = self.waiting_cabin(&::legacy_config::resolve_path(&dir, rel)) {
                    for pp in &cabin.pass_positions {
                        let p = pos + xf.transform_vector3(glam::Vec3::from(pp.pos)).as_dvec3();
                        state
                            .waiting_places
                            .push((o.id, p, heading + pp.rot as f64, pp.height));
                    }
                }
            }
            if let Some((tex, min_h, max_h, min_r, max_r)) = &ot.sco.tree {
                // Trees are billboards: the map stores texture, height and the ratio of the
                // width to the height chosen by the editor; a row of trees along a spline
                // takes the middle of the type's ranges. OMSI scales its tree by (height x
                // ratio, height, height x ratio) (Omsi.exe 0x77e6b0 fills the record,
                // 0x774444 builds the matrix): the ratio multiplies. Divided by it, as here
                // before, a slim tree (0.4 on Spandau) came out six times too wide and its
                // crown stood metres away from its trunk's place.
                let texture = o
                    .extra
                    .first()
                    .cloned()
                    .filter(|t| !t.trim().is_empty())
                    .unwrap_or_else(|| tex.clone());
                let mid_h = ((min_h + max_h) * 0.5) as f64;
                let mid_r = ((min_r + max_r) * 0.5) as f64;
                let height = o
                    .extra
                    .get(1)
                    .map(|s| ::legacy_config::parse_f64(s))
                    .filter(|h| *h > 0.0)
                    .unwrap_or(if mid_h > 0.0 { mid_h } else { 10.0 });
                let ratio = o
                    .extra
                    .get(2)
                    .map(|s| ::legacy_config::parse_f64(s))
                    .filter(|r| *r > 0.0)
                    .unwrap_or(if mid_r > 0.0 { mid_r } else { 1.0 });
                trees.push((ot.clone(), texture, pos, height, height * ratio, heading));
                continue;
            }
            // Stock junctions carry a light program even where the map places no signals.
            // Use the map-wide index so lamps on an unloaded neighbouring tile still count.
            let controller = if !traffic_light_program_enabled(
                &ot.sco,
                index.traffic_light_parents.contains(&o.id),
            ) {
                None
            } else {
                let known = self.controller_of_object.lock().get(&o.id).copied();
                Some(known.unwrap_or_else(|| {
                    let program = ot.sco.traffic_lights.iter().map(|l| (l.phases.iter().map(|p| (p.state, p.duration)).collect(), l.approach_dist)).collect();
                    let c = TrafficLightController::from_program(program, ot.sco.traffic_lights_group, &ot.sco.traffic_light_stop, &ot.sco.traffic_light_jump);
                    let mut list = self.traffic_lights.lock();
                    list.push(c);
                    let idx = list.len() - 1;
                    self.controller_of_object.lock().insert(o.id, idx);
                    if ::legacy_config::env::var_os("OMSI_DEBUG_TRAFFIC").is_some() {
                        log::info!("traffic light program {idx}: object {} {} at ({:.1}, {:.1}) cycle {:?} lights {}", o.id, ot.sco.path.display(), pos.x, pos.y, ot.sco.traffic_lights_group, ot.sco.traffic_lights.len());
                    }
                    idx
                }))
            };
            if ot.sco.only_editor || ot.sco.is_help_arrow || ot.meshes.is_empty() {
                // invisible sound sources (ambient sound objects) still run their script
                if let (Some(program), true) = (&ot.program, ot.sco.sound.is_some()) {
                    let inst = ::simulation::scenery::SceneryInstance::new(
                        program.clone(),
                        &ot.mesh_defs(),
                        self.script_clock(),
                        &o.extra,
                    );
                    self.scripted.lock().push(ScriptedObject {
                        ty: ot.clone(),
                        pos,
                        xf,
                        instances: Vec::new(),
                        inst,
                        controller: None,
                        light_index: 0,
                        map_id: o.id,
                        variants: Vec::new(),
                        sounds: None,
                        tile: key,
                        var_parent: o.lamp_parent,
                        texts: Vec::new(),
                        arrivals: false,
                        htmls: Vec::new(),
                    });
                }
                // An editor-only object still lays its paths out: OMSI's invisible
                // crossings (Novi Sad's junctions are 52-path `[onlyeditor]` objects, their
                // light program driving the lamps placed round them) are where its traffic
                // and its timetable buses turn - skipped, the junctions were holes in the
                // road network and every bus route through one jumped across it.
                if first_load && !ot.sco.paths.is_empty() {
                    lanes.extend(object_lanes(
                        &ot.sco,
                        pos,
                        xf,
                        ot.deform.as_ref(),
                        controller,
                        key,
                        o.id,
                        &o.rules,
                    ));
                }
                continue;
            }
            let is_surface = ot.sco.render_type.is_ground_layer() || ot.sco.surface;
            if check_objects && is_surface {
                let over = pos.z - ground_at(pos.x, pos.y);
                if !(-1.0..=3.0).contains(&over) {
                    log::info!(
                        "surface object {:+.1} m from the ground at ({:.0}, {:.0}): {} (stored as {})",
                        over,
                        pos.x,
                        pos.y,
                        ot.sco.path.display(),
                        match o.place {
                            Placement::Ground { .. } => "height above the terrain",
                            Placement::Pose(_) => "absolute height",
                            Placement::Attached { .. } => "attachment",
                        }
                    );
                }
            }
            if first_load {
                lanes.extend(object_lanes(
                    &ot.sco,
                    pos,
                    xf,
                    ot.deform.as_ref(),
                    controller,
                    key,
                    o.id,
                    &o.rules,
                ));
            }
            // What vehicles hit, as OMSI gives it to ODE: the `[collision_mesh]` as a
            // triangle mesh when there is one (it wins over a `[boundingbox]`), else the
            // `[boundingbox]` as a box. A visual mesh without either declaration is not a
            // collision shape, and neither are the extents of a collision mesh: one box
            // around a housing estate's mesh or the Heerstraße bridge stood as an invisible
            // wall across the roads through and under it.
            // Only a `[fixed]` object (or a `[crashmode_pole]`) is solid for the vehicles, as
            // Omsi.exe sets it up (0x7af0a4: the shape is made for those only; any other is a
            // loose body the bus is not stopped by). We made every object with a shape solid:
            // the line plates and name signs hanging off bus stop poles, and any bridge or
            // gantry of a mod map not marked `[fixed]` - an invisible wall under it.
            // (a parked car is a vehicle: it is hit as the traffic is)
            let solid = ot.sco.fixed || ot.sco.crash_mode_pole.is_some() || o.parked;
            // (Not a `[surface]` object, although Omsi.exe makes it `[fixed]` and puts its
            // collision mesh into the tile's static ODE space like any other (0x7af0a4, the
            // vehicle collides with that space in 0x6ff5b8): the Spandau depot's
            // `Betr_S_Bauten` has fence rails 1.9 m up across its yard's drive paths, which
            // the original's buses pass through - something drops those contacts that is not
            // found yet, and made solid here they walled in the whole yard.)
            let mesh_shape = ot
                .collision
                .as_ref()
                .filter(|_| solid && !ot.sco.no_collision && !is_surface && !ot.meshes.is_empty())
                .filter(|_| !(o.parked && ot.sco.bounding_box.is_some()));
            if let Some(c) = mesh_shape {
                let tris = |m: &dyn Fn(glam::Vec3) -> glam::DVec3| -> Vec<[glam::DVec3; 3]> {
                    c.indices
                        .chunks_exact(3)
                        .map(|t| {
                            [
                                m(c.positions[t[0] as usize]),
                                m(c.positions[t[1] as usize]),
                                m(c.positions[t[2] as usize]),
                            ]
                        })
                        .collect()
                };
                // the shape is the type's, in its own frame; an object tilted on a slope
                // (rare) gets one of its own, turned by all but its heading
                let yaw = ::geometry::object_rotation([heading, 0.0, 0.0]);
                let tilt = yaw.inverse() * xf;
                let upright = (tilt.x_axis.truncate() - glam::Vec3::X).length() < 1e-3
                    && (tilt.y_axis.truncate() - glam::Vec3::Y).length() < 1e-3;
                let shape = if upright {
                    ot.collision_shape
                        .get_or_init(|| {
                            Arc::new(::simulation::collision::MeshShape::from_triangles(
                                tris(&|p| p.as_dvec3()).into_iter(),
                                LOW_OBJECT as f64,
                            ))
                        })
                        .clone()
                } else {
                    Arc::new(::simulation::collision::MeshShape::from_triangles(
                        tris(&|p| tilt.transform_vector3(p).as_dvec3()).into_iter(),
                        LOW_OBJECT as f64,
                    ))
                };
                if !shape.parts.is_empty() {
                    if ::legacy_config::env::var_os("OMSI_DEBUG_COLLISION").is_some() {
                        log::info!(
                            "obstacle {} key {} at ({:.1}, {:.1}) z {:.1} rot {:.0}: collision mesh of {} triangles as {} parts{}",
                            ot.sco.path.display(),
                            o.key,
                            pos.x,
                            pos.y,
                            pos.z,
                            heading,
                            c.indices.len() / 3,
                            shape.parts.len(),
                            if upright { "" } else { " (tilted)" }
                        );
                    }
                    state
                        .mesh_obstacles
                        .push(::simulation::collision::MeshObstacle::new(
                            shape, pos, heading, o.key,
                        ));
                }
            } else if solid && !ot.sco.no_collision && !is_surface && !ot.meshes.is_empty() {
                if let Some(bb) = ot.sco.bounding_box {
                    // Ignore flat decals and oversized helpers - and anything whose top stays
                    // under a bus floor: a manhole cover's half-metre box centred on the road
                    // (ViewApp's Kanaldeckel) stands 25 cm proud of the asphalt, and a
                    // pitching bus ran into it as into a wall.
                    let top = bb[5] + bb[2] * 0.5;
                    // a road that runs through the box (under a bridge, a gantry, an arch,
                    // a station hall) says it is no wall there: a mod map's big objects give
                    // their whole extent as the `[boundingbox]`, and the bus met an invisible
                    // wall across the street (Grand Paris Moulon, Saint Servant)
                    let probe = ::simulation::collision::Obb::from_box(bb, pos, heading);
                    let road_through = !o.parked && (bb[0] > 3.0 || bb[1] > 3.0) && {
                        let [r, f] = probe.axes();
                        // a street lane through its footprint, at a height a vehicle on it
                        // would be inside the box (not a road on its roof or far below)
                        st.street_points.iter().any(|w| {
                            let d = w.truncate() - probe.center;
                            d.dot(r).abs() <= probe.half.x
                                && d.dot(f).abs() <= probe.half.y
                                && w.z >= probe.z0 - 1.0
                                && w.z <= probe.z1 - 0.5
                        })
                    };
                    if road_through && ::legacy_config::env::var_os("OMSI_DEBUG_COLLISION").is_some() {
                        log::info!(
                            "no wall: {} key {} - a road runs through its [boundingbox]",
                            ot.sco.path.display(),
                            o.key
                        );
                    }
                    if bb[2] > 0.4
                        && top > LOW_OBJECT
                        && bb[0] < 400.0
                        && bb[1] < 400.0
                        && bb[0] > 0.05
                        && bb[1] > 0.05
                        && !road_through
                    {
                        let mut obb = ::simulation::collision::Obb::from_box(bb, pos, heading);
                        obb.pole = ot.sco.crash_mode_pole;
                        obb.id = o.key;
                        // (a car that has driven off leaves its space free)
                        let gone = o.parked && self.departed.lock().contains(&o.key);
                        if !gone {
                            state.obstacles.push(obb);
                        }
                        if o.parked && !gone {
                            state.parked_boxes.push(obb);
                        }
                        if ::legacy_config::env::var_os("OMSI_DEBUG_COLLISION").is_some() {
                            log::info!(
                                "obstacle {} key {} at ({:.1}, {:.1}) z {:.1}..{:.1} size {:.1}x{:.1}x{:.1} centre offset ({:.1}, {:.1}) rot {:.0}{}{}",
                                ot.sco.path.display(),
                                o.key,
                                pos.x,
                                pos.y,
                                obb.z0,
                                obb.z1,
                                bb[0],
                                bb[1],
                                bb[2],
                                bb[3],
                                bb[4],
                                heading,
                                if obb.pole.is_some() { " pole" } else { "" },
                                " [boundingbox]"
                            );
                        }
                    }
                }
            }
            if !is_surface
                && !ot.meshes.is_empty()
                && !(o.parked && ::legacy_config::env::var_os("OMSI_NO_LIGHT_OCCLUDERS").is_some())
            {
                let yaw = ::geometry::object_rotation([heading, 0.0, 0.0]);
                let tilt = yaw.inverse() * xf;
                if (tilt.x_axis.truncate() - glam::Vec3::X).length() < 1e-3
                    && (tilt.y_axis.truncate() - glam::Vec3::Y).length() < 1e-3
                {
                    let shape = ot.light_occluder_shape();
                    if !shape.parts.is_empty() {
                        state
                            .light_obstacles
                            .push(::simulation::collision::MeshObstacle::new(
                                shape, pos, heading, o.key,
                            ));
                    }
                }
            }
            if !ot.model.smokes.is_empty() || !ot.model.particle_emitters.is_empty() {
                let set = ::simulation::particles::ParticleSet::new(
                    ot.model.particle_systems(),
                    (o.id as u64) ^ 0x51ed_2701,
                );
                self.particle_objects
                    .lock()
                    .entry(key)
                    .or_default()
                    .push(ParticleObject {
                        map_id: o.id,
                        pos,
                        rot: xf,
                        set,
                    });
            }
            for tb in &ot.sco.trigger_boxes {
                if let Some((time, fade)) = tb.reverb {
                    let bb = [
                        tb.size[0],
                        tb.size[1],
                        tb.size[2],
                        tb.center[0],
                        tb.center[1],
                        tb.center[2],
                    ];
                    state.reverb_zones.push((
                        ::simulation::collision::Obb::from_box(bb, pos, heading),
                        time,
                        fade,
                    ));
                }
            }
            if ot.sco.is_petrol_station {
                if let Some(bb) = ot.sco.bounding_box.or_else(|| ot.local_box()) {
                    state
                        .petrol_stations
                        .push(::simulation::collision::Obb::from_box(bb, pos, heading));
                }
            }
            // what the outside camera cannot pass through: houses, walls, shelters, canopies
            // (surface objects too - a petrol station is a drivable [surface] with a roof)
            if let Some(shape) = ot.camera_shape() {
                state.blockers.push(crate::camera_arm::Blocker {
                    ty: Arc::downgrade(&ot),
                    pos,
                    xf,
                    radius: shape.radius(),
                });
            }
            // (OMSI hands `TrafficLightPhase` to any child of a crossing whose first string
            // names one of its lights, `[trafficlight]` or not - see `names_traffic_light`;
            // a mod lamp without the keyword sat at its "off" picture, blinking yellow.
            // Objects with textures of their own to choose stay ordinary objects.)
            let child_lamp = o
                .lamp_parent
                .is_some_and(|p| index.traffic_light_parents.contains(&p))
                && crate::tiles::names_traffic_light(&o.extra)
                && ot.dynamic_textures.is_empty()
                && !ot
                .meshes
                .iter()
                .any(|(_, _, ov)| ov.iter().any(|m| !m.item && m.freetex.is_some()));
            let lamp = if ot.sco.is_traffic_light || child_lamp {
                let named = o.extra.first().map(|s| s.trim()).filter(|s| !s.is_empty());
                let index = named.map(|s| ::legacy_config::parse_f64(s) as usize).unwrap_or(0);
                if ::legacy_config::env::var_os("OMSI_DEBUG_LAMPS").is_some() {
                    match o.lamp_parent {
                        None => log::info!(
                            "traffic light {} (id {}) names no crossing ([varparent]); extra {:?}",
                            ot.sco.path.display(),
                            o.id,
                            o.extra
                        ),
                        Some(p) => log::info!(
                            "traffic light {} (id {}) at ({:.0}, {:.0}): crossing {p}, light {:?}",
                            ot.sco.path.display(),
                            o.id,
                            pos.x,
                            pos.y,
                            o.extra
                        ),
                    }
                }
                o.lamp_parent.map(|p| (p, index, named.is_none()))
            } else {
                None
            };
            // lights of the placed object
            {
                let switches: Mutex<Vec<LightSwitch>> = Mutex::new(Vec::new());
                // (a light gives several sprites: each takes its own light's switch)
                let coronas = model_lights_owned(
                    &ot.model,
                    &|_| xf,
                    pos,
                    &|var| {
                        switches.lock().push(LightSwitch::parse(var));
                        1.0
                    },
                    &[],
                );
                let switches = switches.into_inner();
                for (c, sw) in coronas
                    .into_iter()
                    .filter_map(|(c, k)| switches.get(k).cloned().map(|sw| (c, sw)))
                {
                    // a traffic lamp's red, yellow and green glow with its state
                    // (`LightObject::coronas`), not all at once by night
                    if lamp.is_some() && matches!(sw, LightSwitch::Variable(_)) {
                        continue;
                    }
                    state.coronas.push(StaticCorona {
                        corona: c,
                        switch: sw,
                    });
                }
                for ml in &ot.sco.map_lights {
                    let p = xf.transform_point3(glam::Vec3::from(ml.pos)).as_dvec3() + pos;
                    // `[maplight] … radius` is the core the light fills at full colour; it
                    // fades inverse-square beyond and is cut off at six times that. The
                    // colour is the brightness, so the intensity stays at one: an Esso sign
                    // declared as 0.1 red is a glow by its pumps, not a red wash over the
                    // whole street.
                    state.lights.push(::render::PointLight {
                        position: p,
                        radius: ml.radius.max(0.5) * 6.0,
                        color: ml.color,
                        intensity: 1.0,
                        core: ml.radius.max(0.5),
                        ..Default::default()
                    });
                }
            }
            if debug_objects {
                let kind = match (&o.place, o.map_object) {
                    (Placement::Attached { .. }, _) => "attachObj",
                    (_, true) => "object",
                    (Placement::Pose(_), false) => "spline row",
                    (Placement::Ground { .. }, false) => "object",
                };
                log::info!(
                    "object {} id {} at ({:.1}, {:.1}, {:.1}) rot {:.1} tile ({tx}, {ty}) {kind}",
                    ot.sco.path.display(),
                    o.id,
                    pos.x,
                    pos.y,
                    pos.z,
                    heading
                );
            }
            objects.push(PlacedObject {
                ot,
                pos,
                xf,
                lamp,
                map_id: o.id,
                key: o.key,
                controller,
                strings: o.extra.clone(),
                var_parent: o.lamp_parent,
                parked: o.parked,
                editable: o.map_object && matches!(o.place, Placement::Ground { .. }),
                script: None,
            });
        }
        if first_load {
            // together, under the lanes lock (see `World::lane_tiles`)
            let mut all = self.lanes.lock();
            all.extend(lanes);
            self.parked_cars.lock().extend(parked_cars);
            self.lane_tiles.lock().push(key);
        }
        {
            let mut s = stats.lock();
            s.failed_objects += st.counts.failed_objects;
            s.empty_spaces += st.counts.empty_spaces;
            s.rows += st.counts.rows;
            s.attached += st.counts.attached;
            s.unattached += res.unattached;
            s.objects_placed += objects.len();
            s.ground_aligned += res.aligned_points;
            s.ground_aligned_tiles += (res.aligned_points > 0) as usize;
            s.ground_deformed_tiles += res.deformed as usize;
            s.crossings_warped += st.objects.iter().filter(|o| o.ot.deform.is_some()).count();
            if let Some(b) = res.biggest {
                if s.ground_moved_most.map(|m| b.0 > m.0).unwrap_or(true) {
                    s.ground_moved_most = Some(b);
                }
            }
        }
        self.tile_state.lock().insert(key, state);
        // the tile's night light map (lamp light pools on the ground)
        let light_map = self
            .global
            .tiles
            .iter()
            .find(|t| t.x == tx && t.y == ty)
            .and_then(|t| {
                let p = ::legacy_config::resolve_path(&self.map_dir, &format!("{}.LM.bmp", t.file));
                if ::legacy_config::vfs::is_file(&p) {
                    ::texture::decode_file(&p)
                        .ok()
                        .map(|img| own_tile_of_light_map(&img))
                } else {
                    None
                }
            });
        // kept for the light map atlas of the splines and [LightMapMapping] objects
        match &light_map {
            Some(img) => {
                self.light_maps.lock().insert(key, Arc::new(img.clone()));
            }
            None => {
                self.light_maps.lock().remove(&key);
            }
        }
        self.light_maps_generation
            .fetch_add(1, std::sync::atomic::Ordering::Relaxed);
        // The lamps' light takes the colour the map's light map gives the ground there: OMSI
        // lights the ground from that map only, and a map's author paints the sodium lamps'
        // orange into it, while the lamp objects' own [maplight] is often a generic white.
        // Lit by that white, the roads and houses under an orange pool of light turned white.
        if let Some(img) = light_map.as_ref() {
            if let Some(state) = self.tile_state.lock().get_mut(&key) {
                tint_lights_from_light_map(&mut state.lights, img, st.origin);
            }
        }
        // the whole spline meshes go to the GPU from here (a later load reads the tile again)
        let meshes = st.meshes.lock().take().unwrap_or_default();
        let splines: Vec<_> = meshes
            .into_iter()
            .zip(st.splines.iter())
            .map(|(m, sp)| (m, sp.ty.clone(), sp.casts_shadow, sp.sort_origin))
            .collect();
        let (splines, ground_splines) = if ::legacy_config::env::var_os("OMSI_NO_GROUND_SPLINE_BATCHING")
            .is_some()
            || ::legacy_config::env::var_os("OMSI_NO_SPLINE_BATCHING").is_some()
        {
            (splines, Vec::new())
        } else {
            let mut slots_by_type = HashMap::new();
            let mut rest = Vec::new();
            let mut ground = Vec::new();
            for (mesh, ty, casts, sort_origin) in splines {
                let slots: &Vec<usize> = slots_by_type
                    .entry(Arc::as_ptr(&ty) as usize)
                    .or_insert_with(|| {
                        let dirs = texture_dirs(&self.root, &ty.dir);
                        let dirs: Vec<&Path> = dirs.iter().map(|p| p.as_path()).collect();
                        ty.def
                            .textures
                            .iter()
                            .enumerate()
                            .filter(|(_, t)| self.textures.cfg(&t.file, &dirs).terrain_mapping)
                            .map(|(i, _)| i)
                            .collect()
                    });
                if slots.is_empty() {
                    rest.push((mesh, ty, casts, sort_origin));
                    continue;
                }
                let faces = terrain_ground(&mesh, slots, st.origin, Mat4::IDENTITY, st.origin);
                if !faces.is_empty() {
                    ground.push(Arc::new(faces));
                }
                let mesh = terrain_rest(&mesh, slots);
                if !mesh.ranges.is_empty() {
                    rest.push((Arc::new(mesh), ty, casts, sort_origin));
                }
            }
            (rest, batch_ground_splines(ground))
        };
        let splines = batch_static_splines(splines);
        Some(Prepared {
            tx,
            ty,
            terrain: Some(build_terrain_mesh(terrain)),
            paint_masks: self.load_ground_paint(&st.path),
            paint: Vec::new(),
            water: st.water,
            splines,
            ground_splines,
            objects,
            trees,
            origin: st.origin,
            light_map: light_map.map(|i| tile_texture(i, false)),
            cut: None,
            images: Arc::new(HashMap::new()),
        })
    }
}
