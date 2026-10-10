use super::*;

/// An object whose top stays this low (m over its foot) is no wall for a vehicle body; with
/// a `[collision_mesh]` it is a step the wheels climb (a traffic island).
pub const LOW_OBJECT: f32 = 0.3;

/// Faces this close over another road face are paint on it, not a step (m). Omsi.exe's
/// ground query (0x7a0814) takes the highest face whatever lies under it; this keeps only
/// the thinnest layers flat (a marking a centimetre or two over the asphalt). At 4.5 cm it
/// also took the speed cushions, manhole and plate objects, lowered kerbs and slab edges
/// away, and the bottom 4.5 cm of every speed bump's ramp - "no road bumps", and wheels
/// drawn sunk into what they drove on.
pub(super) const PAINT_LAYER: f32 = 0.02;

/// What a wheel stands on at world (x, y): the faces of the roads, crossings and surface
/// objects there, and the terrain wherever it is not cut away under them - the highest at
/// or below `top`, and the lowest above it (a kerb the tyre is up against).
pub fn drive_probe(
    terrains: &RwLock<HashMap<(i32, i32), Arc<Terrain>>>,
    surfaces: &RwLock<HashMap<(i32, i32), Arc<TileSurface>>>,
    x: f64,
    y: f64,
    top: f64,
) -> ::simulation::rigid::GroundProbe {
    let key = tile_key(x, y);
    let surface = surfaces.read().get(&key).cloned();
    let terrain = terrains.read().get(&key).cloned();
    probe_tile(surface.as_deref(), terrain.as_deref(), key, x, y, top)
}

pub(super) fn tile_key(x: f64, y: f64) -> (i32, i32) {
    (
        (x / tile_size()).floor() as i32,
        (y / tile_size()).floor() as i32,
    )
}

/// [`drive_probe`] on the tile `key` that holds (x, y).
pub(super) fn probe_tile(
    surface: Option<&TileSurface>,
    terrain: Option<&Terrain>,
    key: (i32, i32),
    x: f64,
    y: f64,
    top: f64,
) -> ::simulation::rigid::GroundProbe {
    let lx = (x - key.0 as f64 * tile_size()) as f32;
    let ly = (y - key.1 as f64 * tile_size()) as f32;
    let mut probe = ::geometry::Probe::default();
    let mut normal = None;
    if let Some(s) = surface {
        probe = s.drive.probe(lx, ly, top as f32);
        // a painted layer is no step: road markings made as `[surface]` objects or as
        // splines with a height profile lie a centimetre or three over the asphalt, and the
        // wheels climbed every line - the bus hopped at stops and over dotted lines (Horizon).
        // Where another road face lies that little below, the wheel stands on that one.
        // (only road faces: the terrain under a road is often that close too)
        // (layer under layer: a marking over a marking over the asphalt, as the map editor
        // stacks them where lines cross or a box junction lies over a lane's arrows, was
        // still a step when only the first was looked through)
        let mut layers = 0;
        while let Some(z1) = probe.below.filter(|_| layers < 4) {
            layers += 1;
            match s.drive.probe(lx, ly, z1 - 0.0005).below {
                Some(z2) if z1 - z2 < PAINT_LAYER => probe.below = Some(z2),
                _ => break,
            }
        }
        normal = probe
            .below
            .and_then(|z| s.drive.contact_below(lx, ly, z + 0.0001).map(|(_, n)| n));
    }
    // On a road the wheel stands on the road, as in OMSI: the terrain under it or over it
    // (an embankment the road runs under, ground poking through the asphalt) is no
    // ground and no wall there. Taken with the road, a terrain face over the carriageway was
    // an invisible wall under bridges, and one through it a bump that threw the bus.
    let ground = terrain.map(|t| {
        let h = ::geometry::terrain_height(t, lx, ly);
        let cut = surface
            .map(|s| s.cut_at(lx, ly, h, surface_flush()))
            .unwrap_or(false);
        (h, cut)
    });
    // ... unless that face lies buried well under ground that is drawn here and is under the
    // wheel, not over it: the lower slope of an embankment spline (Marcel's `Damm1` falls
    // 20 m over 30 m on each side) reaching under a junction the terrain carries. Omsi.exe
    // takes the highest face there, the ground; taken as the road, it dropped the bus 8 m
    // through the asphalt into the slope (Cotterell, the junction by the park at 250, 427).
    let buried = matches!((probe.below, ground), (Some(z), Some((h, false))) if h <= top as f32 && h - z > BURIED_FACE);
    let on_road = probe.below.is_some() && !buried;
    if let (Some((h, cut)), false) = (ground, on_road) {
        // the ground counts where it is drawn; where it is cut away and nothing else is
        // there (a surface without a collision), it still carries rather than let the
        // vehicle drop out of the world
        if !cut || (probe.below.is_none() && h <= top as f32) {
            if h <= top as f32 && probe.below.is_none_or(|z| h > z) {
                normal = Some(::geometry::terrain_normal(terrain.unwrap(), lx, ly));
            }
            probe = probe.merge(::geometry::Probe::of(h, top as f32));
        }
    }
    // A wall's top (a narrow height profile high on a wall spline) is never stood on: where
    // it stands a step over the ground here it is a wall the tyre meets, whatever the height
    // it is probed from; nearer the ground than that the wheel rolls on the road beside it
    // (where the wall's top met the road the wheels went up onto it and rode along it)
    if let Some(s) = surface {
        let walls = s.drive.probe_walls(lx, ly, f32::MAX);
        if let (Some(zw), Some(g)) = (walls.below, probe.below) {
            if zw > g + WALL_TOP_STEP {
                probe.above = Some(probe.above.map_or(zw, |a| a.min(zw)));
            }
        }
    }
    ::simulation::rigid::GroundProbe {
        below: probe.below.map(|z| z as f64),
        above: probe.above.map(|z| z as f64),
        normal,
    }
}

/// How far a road face may lie under drawn ground before it counts as buried (m): far more
/// than the ground poking through the asphalt that the road is there to keep out.
pub(super) const BURIED_FACE: f32 = 1.0;

/// How far over the ground a wall's top must stand to be a wall to the wheels (a kerb is
/// less, and the tyre climbs it).
pub(super) const WALL_TOP_STEP: f32 = 0.3;

/// The ground the player's wheels stand on: [`drive_probe`] over the loaded tiles.
pub struct DriveGround {
    pub terrains: Arc<RwLock<HashMap<(i32, i32), Arc<Terrain>>>>,
    pub surfaces: Arc<RwLock<HashMap<(i32, i32), Arc<TileSurface>>>>,
}

pub(super) type TileRefs = ((i32, i32), Option<Arc<TileSurface>>, Option<Arc<Terrain>>);

impl ::simulation::rigid::Ground for DriveGround {
    fn probe(&self, x: f64, y: f64, top: f64) -> ::simulation::rigid::GroundProbe {
        drive_probe(&self.terrains, &self.surfaces, x, y, top)
    }

    fn road_height(&self, x: f64, y: f64, reference: f64, range: f64) -> Option<f64> {
        let key = tile_key(x, y);
        let surface = self.surfaces.read().get(&key).cloned();
        if let Some(surface) = surface {
            let lx = (x - key.0 as f64 * tile_size()) as f32;
            let ly = (y - key.1 as f64 * tile_size()) as f32;
            let mut top = (reference + range) as f32;
            let mut nearest: Option<f64> = None;
            // Enumerate nearby drive faces, not the terrain fallback below the road.
            // The bound also prevents pathological layered scenery from unbounded work.
            for _ in 0..16 {
                let Some(z) = surface.drive.probe(lx, ly, top).below else { break };
                if z < (reference - range) as f32 { break; }
                let z64 = z as f64;
                if nearest.is_none_or(|old| (z64 - reference).abs() < (old - reference).abs()) {
                    nearest = Some(z64);
                }
                top = z - 0.0005;
                // Once below the reference, every remaining face is farther away.
                if z64 <= reference { break; }
            }
            if nearest.is_some() { return nearest; }
        }
        let probe = self.probe(x, y, reference);
        [probe.below, probe.above].into_iter().flatten()
            .filter(|z| z.is_finite() && (*z - reference).abs() <= range)
            .min_by(|a, b| (a - reference).abs().total_cmp(&(b - reference).abs()))
    }

    /// The tiles under the vehicle are looked up once per step, not twice for every one of
    /// the few hundred points its tyres ask for (each a lock of both maps the loader works
    /// on, two lookups and two reference counts).
    fn session(&self) -> Box<dyn Fn(f64, f64, f64) -> ::simulation::rigid::GroundProbe + '_> {
        let tiles: std::cell::RefCell<Vec<TileRefs>> =
            std::cell::RefCell::new(Vec::with_capacity(4));
        Box::new(move |x, y, top| {
            let key = tile_key(x, y);
            let mut tiles = tiles.borrow_mut();
            let i = match tiles.iter().position(|t| t.0 == key) {
                Some(i) => i,
                None => {
                    tiles.push((
                        key,
                        self.surfaces.read().get(&key).cloned(),
                        self.terrains.read().get(&key).cloned(),
                    ));
                    tiles.len() - 1
                }
            };
            let (_, surface, terrain) = &tiles[i];
            probe_tile(surface.as_deref(), terrain.as_deref(), key, x, y, top)
        })
    }
}

/// Raster resolution of the per-tile surface mask (texels per tile edge).
pub const SURFACE_RASTER: usize = 512;

impl World {
    /// Ground height (road surface where present, else terrain) at world x, y.
    /// Terrain height alone at world x, y (no road surfaces).
    pub fn ground_terrain(&self, x: f64, y: f64) -> Option<f64> {
        let tx = (x / tile_size()).floor() as i32;
        let ty = (y / tile_size()).floor() as i32;
        let lx = (x - tx as f64 * tile_size()) as f32;
        let ly = (y - ty as f64 * tile_size()) as f32;
        let t = self.terrains.read();
        Some(t.get(&(tx, ty))?.sample(lx, ly) as f64)
    }

    /// The height a vehicle put down at (x, y) stands at: the face its wheels would stand on
    /// (a road, a deck, a floor, the ground; [`drive_probe`]) under `near` + 1.5 m and at most
    /// 3 m below it. The raster's [`World::ground_height`] takes the surface of its texel,
    /// and a bus put down beside a wall (an entry point on a pavement, London) stood on the
    /// wall's top and floated there.
    pub fn stand_height(&self, x: f64, y: f64, near: f64) -> Option<f64> {
        drive_probe(&self.terrains, &self.surfaces, x, y, near + 1.5)
            .below
            .filter(|g| near - g < 3.0)
    }

    /// Where object `id` of tile `group` stands: timetables name a stop by the index of its
    /// tile in the `[map]` list, as ids repeat on maps joined from several.
    pub fn object_on_tile(&self, group: i32, id: i64) -> Option<(DVec3, [f64; 3])> {
        let tile = usize::try_from(group)
            .ok()
            .and_then(|i| self.global.raw_tiles.get(i))
            .copied();
        if let Some(p) = tile.and_then(|t| self.object_dups.lock().get(&(t, id)).copied()) {
            return Some(p);
        }
        self.object_positions.lock().get(&id).copied()
    }

    /// Where entry point `ep` stands (position, heading): its object, found on the tile
    /// the entry point names (global.cfg's `[entrypoints]` record holds the index of its
    /// tile in the `[map]` list, and the place within that tile). An object of that id on
    /// another tile (a map joined from two, whose ids repeat) is not it: the record's own
    /// place is taken then.
    pub fn entry_point_place(
        &self,
        ep: &::map::global::EntryPoint,
    ) -> Option<(DVec3, [f64; 3])> {
        let s = tile_size();
        let tile = usize::try_from(ep.group)
            .ok()
            .and_then(|i| self.global.raw_tiles.get(i))
            .copied();
        if let Some(t) = tile {
            if let Some(p) = self.object_dups.lock().get(&(t, ep.object_id)) {
                return Some(*p);
            }
        }
        let found = self.object_positions.lock().get(&ep.object_id).copied();
        let recorded = tile
            .filter(|_| ep.pos.iter().chain(ep.quat.iter()).all(|v| v.is_finite()))
            .map(|(tx, ty)| {
                let heading = (2.0 * ep.quat[1].atan2(ep.quat[3]))
                    .to_degrees()
                    .rem_euclid(360.0);
                (
                    DVec3::new(
                        tx as f64 * s + ep.pos[0],
                        ty as f64 * s + ep.pos[1],
                        ep.pos[2],
                    ),
                    [heading, 0.0, 0.0],
                )
            });
        match (found, recorded) {
            // (an object may stand a little outside its tile's square: far off only is another)
            (Some(f), Some(r)) if (f.0.truncate() - r.0.truncate()).length() > 50.0 => {
                log::info!(
                    "entry point {} \"{}\": object {} stands at ({:.0}, {:.0}), on another tile than the entry point's ({:.0}, {:.0}): the entry point's own place",
                    ep.index,
                    ep.name,
                    ep.object_id,
                    f.0.x,
                    f.0.y,
                    r.0.x,
                    r.0.y
                );
                Some(r)
            }
            (Some(f), _) => Some(f),
            (None, r) => r,
        }
    }

    pub fn ground_height(&self, x: f64, y: f64) -> Option<f64> {
        let tx = (x / tile_size()).floor() as i32;
        let ty = (y / tile_size()).floor() as i32;
        let lx = (x - tx as f64 * tile_size()) as f32;
        let ly = (y - ty as f64 * tile_size()) as f32;
        if let Some(s) = self.surfaces.read().get(&(tx, ty)) {
            // the road the wheels stand on, not a bridge deck or an embankment over it
            if let Some(h) = s.sample_road(lx, ly).or_else(|| s.sample(lx, ly)) {
                return Some(h as f64);
            }
        }
        let t = self.terrains.read();
        let terrain = t.get(&(tx, ty))?;
        Some(terrain.sample(lx, ly) as f64)
    }

    /// The wetness a puddle would use at world (x, y): `wetness` where a road surface is
    /// under the point (the same `[moisture]` ground `enhanced.wgsl`'s reflective puddle
    /// patches sit on), 0 on bare terrain or where no surface is loaded there yet. Approximate
    /// on purpose - `puddles::puddle_coverage` only needs to agree with the shader's own mask
    /// closely enough that a wheel's splash starts where the reflection does, not to the texel.
    pub fn wet_road_at(&self, x: f64, y: f64, wetness: f32) -> f32 {
        let tx = (x / tile_size()).floor() as i32;
        let ty = (y / tile_size()).floor() as i32;
        let lx = (x - tx as f64 * tile_size()) as f32;
        let ly = (y - ty as f64 * tile_size()) as f32;
        let on_road = self
            .surfaces
            .read()
            .get(&(tx, ty))
            .is_some_and(|s| s.sample_road(lx, ly).is_some());
        if on_road {
            wetness.clamp(0.0, 1.0)
        } else {
            0.0
        }
    }

    /// The ground under a point of the outside camera's arm: the highest face of the roads,
    /// crossings, surface objects and terrain at or below `top`. A face higher up - the
    /// roof over a petrol station's forecourt, a bridge deck - is not the ground there (the
    /// top surface of the raster is, and it put the camera on the canopy); a roof's mesh
    /// stops the camera instead.
    pub fn camera_ground(&self, x: f64, y: f64, top: f64) -> Option<f64> {
        drive_probe(&self.terrains, &self.surfaces, x, y, top).below
    }

    /// Local visible road plane under a vehicle. Exact faces include the same draw lift
    /// as the road; raster heights do not. Choose the nearby deck, never a roof above it.
    pub fn puddle_surface(&self, position: DVec3) -> Option<(f64, glam::Vec3)> {
        let height = self.camera_ground(position.x, position.y, position.z + 0.35)?;
        let key = tile_key(position.x, position.y);
        let x = (position.x - key.0 as f64 * tile_size()) as f32;
        let y = (position.y - key.1 as f64 * tile_size()) as f32;
        let normal = self
            .surfaces
            .read()
            .get(&key)
            .and_then(|s| s.drive.surface_below(x, y, height as f32 + 0.002))
            .filter(|(z, _)| (*z as f64 - height).abs() < 0.005)
            .map(|(_, n)| n)
            .unwrap_or(glam::Vec3::Z);
        Some((height, normal))
    }

    /// The ground painting of one tile: `texture/map/<tile>.map.<layer>.dds`, one 8-bit
    /// alpha mask per `[groundtex]` above the first that the editor's brush has touched on
    /// this tile. That is how OMSI puts asphalt under a car park, cobbles on a side street
    /// or a field into the meadow without placing a single object.
    ///
    /// The mask is stored like a picture (first row = north), the terrain mesh's v runs
    /// north with y, so the rows are turned over here.
    pub(super) fn load_ground_paint(&self, tile_path: &Path) -> Vec<(usize, Image)> {
        let Some(name) = tile_path
            .file_name()
            .map(|n| n.to_string_lossy().to_string())
        else {
            return Vec::new();
        };
        let mut out = Vec::new();
        for layer in 1..self.global.ground_textures.len() {
            let path =
                ::legacy_config::resolve_path(&self.map_dir, &format!("texture/map/{name}.{layer}.dds"));
            if !::legacy_config::vfs::is_file(&path) {
                continue;
            }
            match ::texture::decode_file(&path) {
                Ok(img) => {
                    let (w, h) = (img.width as usize, img.height as usize);
                    let mut rgba = vec![0u8; w * h * 4];
                    for j in 0..h {
                        let src = &img.rgba[(h - 1 - j) * w * 4..][..w * 4];
                        rgba[j * w * 4..][..w * 4].copy_from_slice(src);
                    }
                    out.push((
                        layer,
                        Image {
                            width: img.width,
                            height: img.height,
                            rgba,
                            has_alpha: true,
                        },
                    ));
                }
                Err(e) => log::warn!("ground paint {}: {e}", path.display()),
            }
        }
        out
    }

    /// Height for somebody on foot: the top of whatever is here - a pavement, a platform,
    /// a painted yard - and the bare ground where there is nothing. [`ground_height`] is
    /// the wheels' answer instead: it picks the drivable surface, which is the road *under*
    /// the kerb, and standing people on that buried them to the ankles in the pavement.
    pub fn walk_height(&self, x: f64, y: f64) -> Option<f64> {
        let tx = (x / tile_size()).floor() as i32;
        let ty = (y / tile_size()).floor() as i32;
        let lx = (x - tx as f64 * tile_size()) as f32;
        let ly = (y - ty as f64 * tile_size()) as f32;
        let surface = self
            .surfaces
            .read()
            .get(&(tx, ty))
            .and_then(|s| s.sample(lx, ly))
            .map(|h| h as f64);
        let terrain = self
            .terrains
            .read()
            .get(&(tx, ty))
            .map(|t| t.sample(lx, ly) as f64);
        let rough = match (surface, terrain) {
            (Some(s), Some(t)) => Some(s.max(t)),
            (s, t) => s.or(t),
        }?;
        // The raster says roughly where the floor is (a texel is 0.7 m on a Berlin tile, and
        // it holds the highest surface in it): the faces themselves say exactly. Read from the
        // raster, people stood 15 cm up in the air beside a kerb or sank into it, and climbed
        // every slope in steps. The highest face a little over the raster's height is taken:
        // the kerb's top on the pavement, the carriageway beside it.
        let probe = drive_probe(&self.terrains, &self.surfaces, x, y, rough + 0.3);
        match probe.below {
            Some(b) if rough - (b as f64) < 1.0 => Some(b as f64),
            _ => Some(rough),
        }
    }

    /// The floor under somebody at height `near` at (x, y): the highest face no more than a
    /// step (0.5 m, Omsi.exe 0x630498) over them - a station's floor under its roof, a car park's level under the
    /// deck above - else [`World::walk_height`]'s highest one. (Asked for the highest, the
    /// people of an indoor station stood on its roof.)
    ///
    /// Nothing under them within 3 m: the highest face, but only up to 0.5 m over them - a
    /// pavement whose tile came after them. Omsi.exe keeps its people at the heights of
    /// their paths and waiting places; the highest face, a bus shelter's roof 2.5 m up, put
    /// the people waiting under it on top of it.
    pub fn walk_height_near(&self, x: f64, y: f64, near: f64) -> Option<f64> {
        self.walk_height_reach(x, y, near, 0.5)
    }

    /// [`World::walk_height_near`] that also sees faces up to `reach` over `near`: the walker
    /// on foot looks a metre up to stop at a face too high to step onto (a platform's edge)
    /// instead of walking under it.
    pub fn walk_height_reach(&self, x: f64, y: f64, near: f64, reach: f64) -> Option<f64> {
        let probe = drive_probe(&self.terrains, &self.surfaces, x, y, near + reach);
        match probe.below {
            Some(b) if near - b < 3.0 => Some(b),
            _ => self
                .walk_height(x, y)
                .filter(|z| *z < near + reach.max(0.5)),
        }
    }

    /// Whether the ground at world (x, y) is loaded.
    pub fn has_ground(&self, x: f64, y: f64) -> bool {
        let key = (
            (x / tile_size()).floor() as i32,
            (y / tile_size()).floor() as i32,
        );
        self.terrains.read().contains_key(&key)
    }
}
