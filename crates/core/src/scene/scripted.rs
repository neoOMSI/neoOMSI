use super::*;

/// An object whose night textures are lit by a `[NightMapMode]` timetable.
#[derive(Debug, Clone, Copy)]
pub struct NightMode {
    pub inst: usize,
    /// This object's in-use window and darkness threshold (see [`InUse`]).
    pub use_: InUse,
    pub slots: usize,
}

/// When a building with a `[NightMapMode]` is in use and when its windows are lit, as
/// OMSI decides once per object and every frame: in use
/// between `on` and `off` (seconds of the day; mode 2 homes 5.5-9.5 h until 22-24 h, mode 3
/// offices 6-8 h until 17-19 h on working days that are no holiday, mode 4 schools 6-8 h until
/// 14-16 h on school days, any other mode all day); lit while in use and the daylight under
/// `threshold` (0.6 for mode 0, else 0.3-0.75).
#[derive(Clone, Copy, Debug)]
pub struct InUse {
    pub mode: i32,
    pub on: f64,
    pub off: f64,
    pub threshold: f32,
}

/// The day as the in-use rules ask about it.
#[derive(Clone, Copy, Debug, Default)]
pub struct DayKind {
    pub workday: bool,
    pub holiday: bool,
    pub school_holiday: bool,
}

impl InUse {
    /// The window of object `seed` (its map id: the same building keeps its hours).
    pub fn new(mode: i32, seed: u64) -> InUse {
        let r = |k: u64| {
            let h =
                (seed ^ k.wrapping_mul(0x9E37_79B9_7F4A_7C15)).wrapping_mul(0xBF58_476D_1CE4_E5B9);
            ((h >> 11) % 1_000_000) as f64 / 1_000_000.0
        };
        let (on, off) = match mode {
            2 => (5.5 + 4.0 * r(1), 22.0 + 2.0 * r(2)),
            3 => (6.0 + 2.0 * r(1), 17.0 + 2.0 * r(2)),
            4 => (6.0 + 2.0 * r(1), 14.0 + 2.0 * r(2)),
            _ => (0.0, 24.0),
        };
        let threshold = if mode == 0 {
            0.6
        } else {
            (0.3 + 0.45 * r(3)) as f32
        };
        InUse {
            mode,
            on: on * 3600.0,
            off: off * 3600.0,
            threshold,
        }
    }

    pub fn in_use(&self, time: f64, day: DayKind) -> bool {
        let t = time.rem_euclid(86_400.0);
        let hours = t >= self.on && t <= self.off;
        match self.mode {
            3 => hours && day.workday && !day.holiday,
            4 => hours && day.workday && !day.holiday && !day.school_holiday,
            _ => hours,
        }
    }

    pub fn lit(&self, time: f64, day: DayKind, brightness: f32) -> bool {
        self.in_use(time, day) && brightness < self.threshold
    }
}

/// Distance (m) up to which the `[htmltexture]` pages of scenery objects are kept running.
pub const HTML_OBJECT_NEAR: f64 = 60.0;

/// Distance (m) beyond which what a vehicle's scripts redraw is uploaded only every half
/// second (the picture itself stays: see `Traffic::sync`).
pub const DISPLAYS_FAR: f64 = 50.0;

impl World {
    pub(crate) fn push_scripted(&self, obj: ScriptedObject) {
        let map_id = obj.map_id;
        let mut scripted = self.scripted.lock();
        let idx = scripted.len();
        scripted.push(obj);
        self.scripted_of_object.lock().insert(map_id, idx);
    }

    /// Light or darken the night textures of the objects with a `[NightMapMode]` timetable
    /// for this hour of the day (0..24).
    pub fn update_night_modes(
        &self,
        renderer: &Renderer,
        scene: &mut Scene,
        clock: &::simulation::SimClock,
        brightness: f32,
    ) {
        let day = self.day_kind(clock);
        for m in self.night_modes.lock().iter() {
            let v = if m.use_.lit(clock.time, day, brightness) {
                1.0
            } else {
                0.0
            };
            renderer.set_slot_night(scene, m.inst, &vec![v; m.slots]);
        }
    }

    /// A number plate from `registrations.txt` for a vehicle with `[registration_free]`
    /// (OMSI gives such a vehicle's `ident` a random line of it at the spawn).
    pub fn free_registration(&self, seed: u64) -> Option<String> {
        let list = self.registrations.get_or_init(|| {
            self.chrono_dirs
                .read()
                .iter()
                .rev()
                .chain(std::iter::once(&self.map_dir))
                .map(|d| ::legacy_config::resolve_path(d, "registrations.txt"))
                .find(|p| ::legacy_config::vfs::is_file(p))
                .map(|p| ::map::ailists::load_list(&p))
                .unwrap_or_default()
        });
        (!list.is_empty()).then(|| list[(seed % list.len() as u64) as usize].clone())
    }

    /// Working day, holiday and school holidays at `clock`'s date, from the map's
    /// `Holidays.txt`.
    pub fn day_kind(&self, clock: &::simulation::SimClock) -> DayKind {
        let cal = self.calendar.get_or_init(|| {
            ::map::Calendar::load(&self.map_dir.join("Holidays.txt")).unwrap_or_default()
        });
        let date = clock.date_code();
        DayKind {
            workday: clock.weekday() < 5,
            holiday: cal.is_holiday(date),
            school_holiday: cal.in_holiday_range(date),
        }
    }

    /// The echo at `p`: (reverberation time, how much of it is heard) - full inside an
    /// underpass's box, fading over its edge distance at the sides.
    pub fn reverb_at(&self, p: DVec3) -> (f32, f32) {
        let me = ::simulation::collision::Obb::point(p, 0.01);
        let mut best = (0.0f32, 0.0f32);
        for (b, time, fade) in self.reverb_zones.lock().iter() {
            if p.z < b.z0 - 1.0 || p.z > b.z1 + 1.0 {
                continue;
            }
            let inside = (-me.separation(b)) as f32;
            let mix = (inside / fade.max(0.1)).clamp(0.0, 1.0);
            if mix > best.1 {
                best = (*time, mix);
            }
        }
        best
    }

    /// The `[htmltexture]` page of a scenery object a ray lands on (within `reach` metres).
    /// The nearest triangle of those objects decides, as for the bus's pages: a part of
    /// the object in front of its page takes the click away from it.
    pub fn html_object_hit(&self, origin: DVec3, dir: glam::Vec3, reach: f32) -> Option<PageHit> {
        let scripted = self.scripted.lock();
        let mut best: Option<(f32, Option<PageHit>)> = None;
        for o in scripted.iter().filter(|o| !o.htmls.is_empty()) {
            if (o.pos - origin).length() > reach as f64 + 60.0 {
                continue;
            }
            let local = (origin - o.pos).as_vec3();
            for mi in 0..o.instances.len() {
                let Some((data, o3d_mats, overrides)) = o.ty.meshes.get(mi) else {
                    continue;
                };
                if !o.inst.mesh_visible.get(mi).copied().unwrap_or(true) {
                    continue;
                }
                let xf = o.xf
                    * o.inst
                    .mesh_transforms
                    .get(mi)
                    .copied()
                    .unwrap_or(Mat4::IDENTITY);
                let Some(hit) = ::geometry::ray_mesh_hit(local, dir, data, &xf) else {
                    continue;
                };
                if hit.t > reach || best.as_ref().is_some_and(|b| b.0 <= hit.t) {
                    continue;
                }
                // the page the hit material slot shows (a slot that shows none is in the way)
                let slot = data.slot_of(hit.index) as usize;
                let page = overrides
                    .iter()
                    .filter(|m| {
                        !m.item && ::simulation::vehicle::override_slot(o3d_mats, m) == Some(slot)
                    })
                    .find_map(|m| m.use_script_texture)
                    .map(|n| n.max(0) as usize)
                    .filter(|n| o.htmls.iter().any(|(i, _)| i == n));
                let page = page.map(|page| PageHit {
                    t: hit.t,
                    map_id: o.map_id,
                    page,
                    u: hit.uv.x.clamp(0.0, 1.0),
                    v: hit.uv.y.clamp(0.0, 1.0),
                });
                best = Some((hit.t, page));
            }
        }
        best.and_then(|b| b.1)
    }

    /// A press, release or move on a page of a scenery object (see [`Self::html_object_hit`]).
    /// What the page does (`omsi.setVar`, `omsi.trigger`) reaches the object's script.
    pub fn html_object_pointer(
        &self,
        map_id: i64,
        page: usize,
        u: f32,
        v: f32,
        kind: ::simulation::htmltex::PointerKind,
    ) -> bool {
        let mut scripted = self.scripted.lock();
        let idx = self.scripted_of_object.lock().get(&map_id).copied();
        let o = if let Some(i) = idx.filter(|&i| scripted.get(i).is_some_and(|o| o.map_id == map_id)) {
            scripted.get_mut(i)
        } else {
            scripted.iter_mut().find(|o| o.map_id == map_id)
        };
        match o {
            Some(o) => o.inst.html_pointer(page, u, v, kind),
            None => false,
        }
    }

    /// Move the particles of the placed objects within 1.5 km of `center`, their variables
    /// read from the object's script (the fireworks' frequency).
    pub fn update_particles(&self, dt: f32, center: DVec3) {
        let mut objs = self.particle_objects.lock();
        if objs.is_empty() {
            return;
        }
        let scripted = self.scripted.lock();
        let by_id = self.scripted_of_object.lock();
        static DEBUG: std::sync::OnceLock<bool> = std::sync::OnceLock::new();
        if *DEBUG.get_or_init(|| ::legacy_config::env::var_os("OMSI_DEBUG_PARTICLES").is_some()) {
            let mut near: Vec<(f64, &ParticleObject)> = objs
                .values()
                .flatten()
                .map(|po| ((po.pos - center).length(), po))
                .collect();
            near.sort_by(|a, b| a.0.total_cmp(&b.0));
            for (d, po) in near.iter().take(3) {
                log::info!(
                    "particle object {} at ({:.1}, {:.1}, {:.1}), {d:.0} m: {} particles",
                    po.map_id,
                    po.pos.x,
                    po.pos.y,
                    po.pos.z,
                    po.set.particles().count()
                );
            }
        }
        for list in objs.values_mut() {
            for po in list.iter_mut() {
                if (po.pos - center).length() > 1500.0 {
                    continue;
                }
                let inst = by_id
                    .get(&po.map_id)
                    .and_then(|&i| scripted.get(i))
                    .filter(|s| s.map_id == po.map_id)
                    .or_else(|| scripted.iter().find(|s| s.map_id == po.map_id))
                    .map(|s| &s.inst);
                let value = |n: &str| inst.and_then(|i| i.var(n)).unwrap_or(0.0);
                po.set.update(dt, po.pos, po.rot, &value);
            }
        }
    }

    /// The departures for the HTML pages of the player's vehicle: the stop names its pages asked
    /// for go to the boards, and the departures made for them come back into its host.
    pub fn sync_html_departures(&self, host: &mut ::simulation::host::VehicleHost) {
        if host.html_departure_wants.is_empty() {
            return;
        }
        let mut boards = self.timetable_boards.lock();
        for k in &host.html_departure_wants {
            if !boards.wanted_names.contains(k) {
                boards.wanted_names.push(k.clone());
            }
        }
        if host.html_departures_gen != boards.departures_gen {
            host.html_departures = host
                .html_departure_wants
                .iter()
                .filter_map(|k| boards.departures.get(k).map(|l| (k.clone(), l.clone())))
                .collect();
            host.html_departures_gen = boards.departures_gen;
        }
    }

    /// Run the scripts and animations of the placed objects near `center` and push their
    /// mesh transforms / visibility to the renderer. `phase_of(controller, light)` gives the
    /// light's current state (the `TrafficLightPhase` value) and whether a vehicle is
    /// asking for it (`TrafficLightApproach`).
    pub fn update_scripted(
        &self,
        renderer: &Renderer,
        scene: &mut Scene,
        dt: f32,
        center: DVec3,
        brightness: f32,
        phase_of: &dyn Fn(usize, usize) -> (f32, f32),
        audio: Option<&::audio::AudioEngine>,
        muffled: bool,
    ) -> usize {
        self.update_particles(dt, center);
        let mut updated = 0;
        let now = self.script_clock();
        let day = self.day_kind(&now);
        let mut scripted = self.scripted.lock();
        let mut boards = self.timetable_boards.lock();
        let mut wanted: Vec<i64> = Vec::new();
        let mut wanted_names: Vec<String> = Vec::new();
        let mut texture_updates: Vec<(
            i64,
            Arc<ObjectType>,
            Vec<usize>,
            Vec<usize>,
            HashMap<(usize, usize), usize>,
        )> = Vec::new();
        // First what every object's script is given (in order: the light programs and the
        // boards are read here), then the scripts themselves, side by side on the worker
        // threads (a city's hundreds of scripted objects took a core's worth of a frame),
        // then what they did, in order again.
        let mut inputs: Vec<Option<::simulation::scenery::SceneryVars>> =
            Vec::with_capacity(scripted.len());
        for o in scripted.iter_mut() {
            let dist = (o.pos - center).length();
            if dist > 400.0 {
                inputs.push(None);
                if let (Some(a), Some(mut ss)) = (audio, o.sounds.take()) {
                    ss.stop_all(a);
                }
                continue;
            }
            // the object's own hours ([NightMapMode]): in use, and lit while in use and the
            // daylight under its own threshold (0.6, or 0.3-0.75 with a [NightMapMode])
            let use_ = InUse::new(o.ty.sco.night_map_mode, o.map_id as u64);
            let in_use = use_.in_use(now.time, day);
            let vars = ::simulation::scenery::SceneryVars {
                nightlight: use_.lit(now.time, day, brightness) as i32 as f32,
                in_use: in_use as i32 as f32,
                traffic_light_phase: o
                    .controller
                    .map(|c| phase_of(c, o.light_index).0)
                    .unwrap_or(-1.0),
                traffic_light_approach: o
                    .controller
                    .map(|c| phase_of(c, o.light_index).1)
                    .unwrap_or(0.0),
                switch: None,
            };
            // the scripts read the simulation's time of day (clocks, the display's blinking)
            if let Some(c) = &boards.clock {
                let own = &mut o.inst.host.clock;
                *own = c.clone();
                // (the update moves it on by `dt` again)
                if !own.paused {
                    own.time -= dt as f64;
                    own.run_time -= dt as f64;
                }
            }
            // a departure display: the buses due at its stop
            if let (true, Some(stop)) = (o.arrivals, o.var_parent) {
                wanted.push(stop);
                let now = boards.clock.as_ref().map(|c| c.time).unwrap_or(0.0);
                o.inst.host.arrivals = boards
                    .by_stop
                    .get(&stop)
                    .map(|l| {
                        l.iter()
                            .map(|(line, terminus, t)| ::simulation::host::Arrival {
                                line: line.clone(),
                                terminus: terminus.clone(),
                                due: (t - now) as f32,
                            })
                            .collect()
                    })
                    .unwrap_or_default();
            }
            // an HTML page that asks for departures by stop name
            if !o.htmls.is_empty()
                && dist < HTML_OBJECT_NEAR
                && !o.inst.host.html_departure_wants.is_empty()
            {
                for k in &o.inst.host.html_departure_wants {
                    if !wanted_names.contains(k) {
                        wanted_names.push(k.clone());
                    }
                }
                if o.inst.host.html_departures_gen != boards.departures_gen {
                    o.inst.host.html_departures = o
                        .inst
                        .host
                        .html_departure_wants
                        .iter()
                        .filter_map(|k| boards.departures.get(k).map(|l| (k.clone(), l.clone())))
                        .collect();
                    o.inst.host.html_departures_gen = boards.departures_gen;
                }
            }
            inputs.push(Some(vars));
        }
        {
            use rayon::prelude::*;
            scripted
                .par_iter_mut()
                .zip(inputs.par_iter())
                .for_each(|(o, vars)| {
                    if let Some(vars) = vars {
                        o.inst.update(dt, vars);
                    }
                });
        }
        for (o, vars) in scripted.iter_mut().zip(inputs.iter()) {
            let Some(nightlight) = vars.as_ref().map(|v| v.nightlight) else {
                continue;
            };
            let dist = (o.pos - center).length();
            // text textures from the script's strings whenever they change (`update` leaves
            // an unchanged one alone): read only on `Refresh_Strings`, a board whose string
            // was still empty at its first frame stayed blank for good (#367)
            if !o.texts.is_empty() && dist < 250.0 {
                let _ = o.inst.take_refresh_strings();
                for (tex, st) in o.texts.iter_mut() {
                    let text = o.inst.str_var(st.def.variable.trim());
                    if st.update(text) {
                        let (w, h) = (st.def.width.max(1) as u32, st.def.height.max(1) as u32);
                        if let Some(rgba) = st.pending.take() {
                            // OMSI_DUMP_SCENERY_TEXT=<dir>: the pictures as drawn
                            if let Some(dir) = ::legacy_config::env::var_os("OMSI_DUMP_SCENERY_TEXT") {
                                let path = std::path::Path::new(&dir)
                                    .join(format!("text_{}.png", o.map_id));
                                log::info!(
                                    "scenery text of object {}: {text:?} -> {}",
                                    o.map_id,
                                    path.display()
                                );
                                if let Some(img) = image::RgbaImage::from_raw(w, h, rgba.clone()) {
                                    let _ = img.save(&path);
                                }
                            }
                            renderer.update_texture_mips(
                                scene,
                                *tex,
                                &Image {
                                    width: w,
                                    height: h,
                                    rgba,
                                    has_alpha: true,
                                },
                            );
                        }
                    }
                }
            }
            // [htmltexture] pages: only near the listener (a page is a whole browser frame)
            if !o.htmls.is_empty() && dist > HTML_OBJECT_NEAR * 2.5 {
                for (_, tex) in &o.htmls {
                    if renderer
                        .texture_levels(scene, *tex)
                        .is_some_and(|(w, _, _)| w > 4)
                    {
                        let blank = Image {
                            width: 4,
                            height: 4,
                            rgba: [0, 0, 0, 255].repeat(16),
                            has_alpha: true,
                        };
                        if renderer.update_texture_mips(scene, *tex, &blank) {
                            renderer.rebind_textures(scene, &[*tex]);
                        }
                    }
                }
            }
            if !o.htmls.is_empty() && dist < HTML_OBJECT_NEAR {
                if o.htmls.iter().any(|(_, tex)| {
                    renderer
                        .texture_levels(scene, *tex)
                        .is_some_and(|(w, _, _)| w <= 4)
                }) {
                    o.inst.invalidate_html();
                }
                for (index, w, h, rgba) in o.inst.update_html_textures() {
                    if let Some((_, tex)) = o.htmls.iter().find(|(i, _)| *i == index) {
                        if renderer.update_texture_mips(
                            scene,
                            *tex,
                            &Image {
                                width: w,
                                height: h,
                                rgba,
                                has_alpha: true,
                            },
                        ) {
                            renderer.rebind_textures(scene, &[*tex]);
                        }
                    }
                }
            }
            // [sound] of scenery objects: crossing bells, ambient loops
            let fired: Vec<::simulation::host::FiredSound> =
                std::mem::take(&mut o.inst.host.fired_sounds);
            let events = crate::sound_events::events_from(
                ::audio::EventSource::Scenery,
                &fired,
                &[],
            );
            // (out of earshot with nothing playing: nothing to do - finding the sound file
            // for each of a city's scripted objects every frame took 1.8 ms)
            let near = dist < 300.0 || o.sounds.is_some();
            if let (Some(a), true) = (audio, near) {
                let ty = o.ty.clone();
                let path = ty.sound_path.get_or_init(|| {
                    let rel = ty.sco.sound.as_ref()?;
                    let dir = ty
                        .sco
                        .path
                        .parent()
                        .map(|p| p.to_path_buf())
                        .unwrap_or_default();
                    Some(::legacy_config::resolve_path(&dir, rel))
                });
                if let Some(path) = path {
                    let inst = &o.inst;
                    self.object_sounds(
                        a,
                        &mut o.sounds,
                        path,
                        dist,
                        muffled,
                        o.pos,
                        o.xf,
                        &|n| inst.var(n),
                        &events,
                    );
                }
            }
            apply_scenery_variants(
                &o.variants,
                &o.dynamic_materials,
                nightlight,
                &|n| o.inst.var(n),
                renderer,
                scene,
            );
            if !o.ty.dynamic_textures.is_empty()
                && !scenery_texture_selection_matches(
                    &o.ty,
                    &o.inst,
                    o.last_tex_selection.as_deref(),
                )
            {
                let selection = scenery_texture_selection(&o.ty, &o.inst);
                let switches = o
                    .variants
                    .iter()
                    .map(|(inst, slot, _, _, var, _)| {
                        let value = if var.trim().eq_ignore_ascii_case("NightlightA") {
                            nightlight
                        } else {
                            var.trim()
                                .parse::<f32>()
                                .ok()
                                .or_else(|| o.inst.var(var))
                                .unwrap_or(0.0)
                        };
                        ((*inst, *slot), variant_number(value))
                    })
                    .collect();
                texture_updates.push((o.map_id, o.ty.clone(), selection, o.instances.clone(), switches));
            }
            for ((inst, xf), &visible) in o
                .instances
                .iter()
                .zip(&o.inst.mesh_transforms)
                .zip(&o.inst.mesh_visible)
            {
                // Scripted tram switches keep the same world-space lift as on upload.
                // `o.pos` is the authored pose used by scripts/physics, not the draw pose.
                renderer.set_transform(
                    scene,
                    *inst,
                    scenery_draw_position(o.pos, drawn_on_surfaces(&o.ty.sco)),
                    o.xf * *xf,
                );
                let p = &mut scene.instances[*inst];
                if p.visible != visible {
                    renderer.set_params(scene, *inst, &[], visible, &[]);
                }
            }
            updated += 1;
        }
        wanted.sort_unstable();
        wanted.dedup();
        boards.wanted = wanted;
        boards.wanted_names = wanted_names;
        drop(scripted);
        drop(boards);
        // Scenery placement takes the GPU-cache lock before the script list. Apply dynamic
        // texture changes after releasing the script-list lock to keep that lock order
        // consistent.
        for (map_id, ty, selection, instances, switches) in texture_updates {
            let variant = {
                let mut gpu = self.gpu.lock();
                gpu.dynamic_texture_variant(
                    renderer,
                    scene,
                    Arc::as_ptr(&ty) as usize,
                    &selection,
                    &self.root,
                    &HashMap::new(),
                )
            };
            if let Some(rows) = variant {
                let mut dyn_mats: Vec<((usize, usize), Vec<MaterialId>)> = Vec::new();
                for (mi, row) in rows.iter().enumerate() {
                    let Some(&mesh_inst) = instances.get(mi) else {
                        continue;
                    };
                    for (slot, pair) in row.iter().enumerate() {
                        let Some(looks) = pair else { continue };
                        dyn_mats.push(((mesh_inst, slot), looks.clone()));
                        let picked = switches.get(&(mesh_inst, slot)).copied().unwrap_or(0);
                        let target_mat = look_of(looks, picked);
                        if scene
                            .instances
                            .get(mesh_inst)
                            .and_then(|i| i.materials.get(slot))
                            != Some(&target_mat)
                        {
                            renderer.set_material(scene, mesh_inst, slot, target_mat);
                        }
                    }
                }
                let mut scripted = self.scripted.lock();
                if let Some(o) = scripted.iter_mut().find(|o| o.map_id == map_id) {
                    o.last_tex_selection = Some(selection);
                    for (key, looks) in dyn_mats {
                        o.dynamic_materials.insert(key, looks);
                    }
                }
            }
        }
        // the lamps' own sounds (a level crossing's bell): their scripts run with the light
        // programs (`Traffic::sync`), what they fired is heard here
        for lamp in self.light_objects.lock().iter() {
            // (only the lamps that have a sound, and near enough to hear: a city map has
            // nearly a thousand lamps, and going through all of them took 1.8 ms a frame)
            let Some(path) = lamp.sound.as_ref() else {
                continue;
            };
            let Some(script) = lamp.script.as_ref() else {
                continue;
            };
            let dist = (lamp.pos - center).length();
            if dist >= 300.0 {
                if let (Some(a), Some(mut ss)) = (audio, lamp.sounds.lock().take()) {
                    ss.stop_all(a);
                }
                // (what it fired out of earshot is not heard later)
                script.lock().host.fired_sounds.clear();
                continue;
            }
            let mut inst = script.lock();
            let fired: Vec<::simulation::host::FiredSound> =
                std::mem::take(&mut inst.host.fired_sounds);
            let events =
                crate::sound_events::events_from(::audio::EventSource::Scenery, &fired, &[]);
            if let Some(a) = audio {
                let mut sounds = lamp.sounds.lock();
                self.object_sounds(
                    a,
                    &mut sounds,
                    path,
                    dist,
                    muffled,
                    lamp.pos,
                    lamp.xf,
                    &|n| inst.var(n),
                    &events,
                );
            }
        }
        updated
    }

    /// Play a scenery object's `[sound]` (the config at `path`) while the listener is within
    /// 300 m: loaded when it comes near, stopped when it goes.
    #[allow(clippy::too_many_arguments)]
    pub(super) fn object_sounds(
        &self,
        a: &::audio::AudioEngine,
        sounds: &mut Option<::audio::SoundSet>,
        path: &Path,
        dist: f64,
        muffled: bool,
        pos: DVec3,
        xf: Mat4,
        var: &dyn Fn(&str) -> Option<f32>,
        events: &[::audio::SoundEvent],
    ) {
        if dist >= 300.0 {
            if let Some(mut ss) = sounds.take() {
                ss.stop_all(a);
            }
            return;
        }
        if sounds.is_none() {
            // read once per file, the clips in the background (see AudioEngine::clips_ready)
            let cfg = self
                .sound_cfgs
                .lock()
                .entry(path.to_path_buf())
                .or_insert_with(|| {
                    ::legacy_vehicle::SoundCfg::load(path)
                        .map_err(|e| log::warn!("{e}"))
                        .ok()
                        .map(Arc::new)
                })
                .clone();
            if let Some(cfg) = cfg {
                let sdir = path.parent().map(|p| p.to_path_buf()).unwrap_or_default();
                if a.clips_ready(&::audio::SoundSet::clip_paths(&cfg, &sdir)) {
                    log::info!(
                        "scenery sound {}: {} sounds ({:.0} m away)",
                        path.display(),
                        cfg.sounds.len(),
                        dist
                    );
                    let mut ss = ::audio::SoundSet::new_world(a, &cfg, &sdir);
                    ss.master = crate::sound_gain(&crate::SOUND_SCENERY);
                    *sounds = Some(ss);
                }
            }
        }
        if let Some(ss) = sounds.as_mut() {
            // scenery (a fountain, machinery, a level crossing bell): heard through the
            // player's own bodywork and glass just like any other sound from outside the cabin
            ss.set_muffled(muffled);
            let xf = Mat4::from_translation(pos.as_vec3()) * xf;
            // scenery triggers carry no fire-time snapshot (its host keeps none): the
            // fallback to the current variable is the previous behavior
            ss.update_events(a, var, &xf, events, &|_| None);
        }
    }
}
