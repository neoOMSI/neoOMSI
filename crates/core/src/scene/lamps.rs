use super::*;

/// The material slots of a lamp's mesh switched by its variables: `[alphascale] var`
/// fades a slot (a lens shown by its alpha rather than by a `[visible]` mesh, as the
/// Korean maps' signals do, #826) and `[matl_lightmap] tex var` lights it.
#[derive(Clone, Default, Debug, PartialEq)]
pub struct LampSlots {
    pub count: usize,
    pub alpha: Vec<(usize, String)>,
    pub light: Vec<(usize, String)>,
}

impl LampSlots {
    /// The `[alphascale]` and `[matl_lightmap]` variables of a mesh's material slots.
    pub fn of_mesh(
        o3d_mats: &[::legacy_o3d::Material],
        overrides: &[MaterialDef],
        count: usize,
    ) -> LampSlots {
        let mut l = LampSlots {
            count,
            ..Default::default()
        };
        for o in overrides.iter().filter(|o| !o.item) {
            let Some(slot) = ::simulation::vehicle::override_slot(o3d_mats, o) else {
                continue;
            };
            if let Some(v) = o.alphascale.as_ref().filter(|v| !v.trim().is_empty()) {
                l.alpha.push((slot, v.trim().to_string()));
            }
            if let Some((_, v)) = &o.lightmap {
                l.light.push((slot, v.trim().to_string()));
            }
        }
        l
    }

    pub fn is_empty(&self) -> bool {
        self.alpha.is_empty() && self.light.is_empty()
    }

    /// The slots' alpha and light-map switch from the lamp's variables (`value`: `None`
    /// for a variable the lamp does not have). Alpha: the variable's value, a slot without
    /// one opaque as authored. Light map: on from 0.5; a variable the lamp does not have
    /// (or none at all) leaves it on, as Omsi.exe's stage does with an unregistered one.
    pub fn values(&self, value: &dyn Fn(&str) -> Option<f32>) -> (Vec<f32>, Vec<f32>) {
        let mut alpha = vec![1.0; self.count.max(1)];
        let mut light = vec![1.0; self.count.max(1)];
        for (slot, v) in &self.alpha {
            if let (Some(a), Some(x)) = (alpha.get_mut(*slot), value(v)) {
                *a = x.clamp(0.0, 1.0);
            }
        }
        for (slot, v) in &self.light {
            if let (Some(l), Some(x)) = (
                light.get_mut(*slot),
                (!v.is_empty()).then(|| value(v)).flatten(),
            ) {
                *l = if x >= 0.5 { 1.0 } else { 0.0 };
            }
        }
        (alpha, light)
    }
}

/// A placed object's particle systems (`[smoke]`, `[particle_emitter]`).
pub struct ParticleObject {
    pub map_id: i64,
    pub pos: DVec3,
    pub rot: Mat4,
    pub set: ::simulation::particles::ParticleSet,
}

/// A `[light_enh]`/`[light_enh_2]` of a placed scenery object.
#[derive(Debug, Clone)]
pub struct StaticCorona {
    pub corona: ::render::Corona,
    /// What switches it: a constant, the night flag, or an object variable (treated as on).
    pub switch: LightSwitch,
}

#[derive(Debug, Clone, PartialEq)]
pub enum LightSwitch {
    Constant(f32),
    Night,
    Variable(String),
}

impl LightSwitch {
    pub fn parse(var: &str) -> LightSwitch {
        let v = var.trim();
        if let Ok(x) = v.parse::<f32>() {
            LightSwitch::Constant(x)
        } else if v.eq_ignore_ascii_case("NightlightA") || v.is_empty() {
            LightSwitch::Night
        } else {
            LightSwitch::Variable(v.to_string())
        }
    }
}

/// Resolve the three standard traffic-lamp channels without going through the scenery VM.
///
/// Stock `.sco` files use `red`, `yellow` and `green` in both `[visible]` and
/// `[light_enh_2]`.  Keeping this mapping at the renderer boundary is important: a missing
/// or platform-specific script load must not turn an unknown variable into an always-visible
/// mesh (which makes all three bulbs appear lit).  Non-standard channels, such as `Left`, are
/// still resolved by the object's script.
pub fn standard_traffic_lamp(
    var: &str,
    red: bool,
    yellow: bool,
    green: bool,
    approach: bool,
) -> Option<f32> {
    match var.trim().to_ascii_lowercase().as_str() {
        "red" | "rot" => Some(red as i32 as f32),
        "yellow" | "gelb" | "amber" => Some(yellow as i32 as f32),
        "green" | "gruen" | "grün" => Some(green as i32 as f32),
        "trafficlightapproach" => Some(approach as i32 as f32),
        _ => None,
    }
}

/// Resolve a lamp channel after its script has run. A zero script output is meaningful
/// (not a reason to use the stock phase), notably while a pedestrian lamp is blinking.
pub(crate) fn traffic_lamp_value(var: &str, scripted: Option<f32>, standard: Option<f32>) -> f32 {
    scripted
        .or_else(|| var.trim().parse::<f32>().ok())
        .or(standard)
        .unwrap_or(0.0)
}

/// Coronas and point lights of a model in the frame of `xf` (rotation) at `pos`.
/// `value_of` resolves the light variables (vehicle state or scenery switches), with the
/// lights' brightness as their `timeconst` has let it follow the switch (`fades`: one value
/// per `[light_enh_2]` of the model in order; missing = at once).
///
/// The lights of every detail level count, not only LOD 0's: a `[light_enh]` belongs to the
/// mesh before it, and 40 stock models (the Spandau neon, sodium and gas street lamps, the
/// Sv signals, the ICE and RE160 coaches) declare theirs after the far `[LOD] 0` mesh - their
/// glow still shows up close in OMSI. No stock model repeats a light in two levels.
pub fn model_lights_faded(
    model: &Model,
    mesh_transforms: &dyn Fn(usize) -> Mat4,
    pos: DVec3,
    value_of: &dyn Fn(&str) -> f32,
    fades: &[f32],
) -> Vec<::render::Corona> {
    model_lights_owned(model, mesh_transforms, pos, value_of, fades)
        .into_iter()
        .map(|c| c.0)
        .collect()
}

/// Every light of a model in the order [`model_lights_owned`] numbers them: the mesh it
/// belongs to, its place and its direction (zero for a `[light_enh]` and an omni light).
/// Omsi.exe files each `[light_enh]`/`[light_enh_2]` with the `[mesh]` before it (the
/// model loader, 0x5f3140: the light goes into the current mesh's list, mesh +0x1b0) and
/// draws it where that mesh's animation takes it - the lamps along a level crossing's arm
/// rise with the arm.
pub fn model_light_sources(model: &Model) -> Vec<(usize, glam::Vec3, glam::Vec3)> {
    let mut out = Vec::new();
    for (i, md) in model.meshes.iter().enumerate() {
        for l in &md.light_enh {
            out.push((i, glam::Vec3::from(l.pos), glam::Vec3::ZERO));
        }
        for l in &md.light_enh_2 {
            out.push((
                i,
                glam::Vec3::from(l.pos),
                if l.omni {
                    glam::Vec3::ZERO
                } else {
                    glam::Vec3::from(l.dir)
                },
            ));
        }
    }
    out
}

/// [`model_lights_faded`], each sprite with the light it belongs to (the n-th light of the
/// model, `[light_enh]` and `[light_enh_2]` in file order - the order `value_of` is asked
/// in): one light gives several sprites (its glow, star, fog halo and cone).
pub fn model_lights_owned(
    model: &Model,
    mesh_transforms: &dyn Fn(usize) -> Mat4,
    pos: DVec3,
    value_of: &dyn Fn(&str) -> f32,
    fades: &[f32],
) -> Vec<(::render::Corona, usize)> {
    let mut out = Vec::new();
    let mut owners: Vec<usize> = Vec::new();
    let mut seq = 0usize;
    let mut li = 0usize;
    let model_dir = model
        .path
        .parent()
        .map(|p| p.to_path_buf())
        .unwrap_or_default();
    for (i, md) in model.meshes.iter().enumerate() {
        // most meshes carry no light, and their transform is not free (every mesh of every
        // AI car, every frame)
        if md.light_enh.is_empty() && md.light_enh_2.is_empty() {
            continue;
        }
        let first_li = li;
        li += md.light_enh_2.len();
        let xf = mesh_transforms(i);
        for l in &md.light_enh {
            owners.resize(out.len(), seq.wrapping_sub(1));
            seq += 1;
            let b = value_of(&l.variable).clamp(0.0, 1.0);
            if b <= 0.0 {
                continue;
            }
            let p = xf.transform_point3(glam::Vec3::from(l.pos)).as_dvec3() + pos;
            // (the glow is as wide as the light's size: OMSI draws its sprite half that
            // either side of the lamp)
            out.push(::render::Corona {
                position: p,
                size: (l.size * 0.5).max(0.0),
                color: [l.color[0] / 255.0, l.color[1] / 255.0, l.color[2] / 255.0],
                brightness: b,
                direction: glam::Vec3::ZERO,
                cone_cos: -1.0,
                texture: crate::lights::glow_texture_id(),
                ..Default::default()
            });
        }
        for (k, l) in md.light_enh_2.iter().enumerate() {
            owners.resize(out.len(), seq.wrapping_sub(1));
            seq += 1;
            // the fading variable: 0 dark, 1 normal, 2 double (times the factor), as far as
            // the lamp has come on or gone out (`timeconst`)
            let b = match fades.get(first_li + k) {
                Some(f) => *f,
                None => (value_of(&l.variable) * if l.factor > 0.0 { l.factor } else { 1.0 })
                    .clamp(0.0, 2.0),
            };
            if !(b > 0.0) || !b.is_finite() {
                continue;
            }
            let p = xf.transform_point3(glam::Vec3::from(l.pos)).as_dvec3() + pos;
            let dir = if l.omni {
                glam::Vec3::ZERO
            } else {
                xf.transform_vector3(glam::Vec3::from(l.dir))
                    .normalize_or_zero()
            };
            let up = xf
                .transform_vector3(glam::Vec3::from(l.up))
                .normalize_or(glam::Vec3::Z);
            let half_cos = |deg: f32| (deg.max(1.0) * 0.5).min(180.0).to_radians().cos();
            let (outer, inner) = (
                l.cone_outer.max(l.cone_inner),
                l.cone_inner.min(l.cone_outer),
            );
            let flags = l
                .values
                .first()
                .map(|v| ::legacy_config::parse_f32(v) as i32)
                .unwrap_or(0)
                .clamp(0, 7) as u8;
            let color = [l.color[0] / 255.0, l.color[1] / 255.0, l.color[2] / 255.0];
            // the original: the glow, the light's own bitmap (else
            // licht.bmp) as wide as its size, left out with effect bit 4; with effect bit 1 a
            // star (light_effect1.bmp) turned to the viewer, 2.5 times the size and growing
            // with the glow's strength (corona.wgsl, flag bit 8)
            let glow = ::render::Corona {
                position: p,
                size: (l.size * 0.5).max(0.0),
                color,
                brightness: b,
                direction: dir,
                cone_cos: half_cos(outer),
                inner_cos: if inner > 0.0 { half_cos(inner) } else { -2.0 },
                // an omnidirectional light has no face to show: it turns to the viewer
                rotating: if l.omni {
                    2
                } else {
                    l.rotating.clamp(0, 2) as u8
                },
                up,
                z_offset: l.z_offset.max(0.0),
                flags: (flags & !1)
                    | if matches!(
                        l.variable.to_ascii_lowercase().as_str(),
                        "lights_lowbeam" | "lights_mainbeam" | "lights_highbeam"
                    ) {
                    16
                } else {
                    0
                },
                texture: l
                    .bitmap
                    .as_deref()
                    .map(|b| crate::lights::corona_texture_id(&model_dir, b))
                    .filter(|t| *t != 0)
                    .unwrap_or_else(crate::lights::glow_texture_id),
                ..Default::default()
            };
            if flags & 4 == 0 {
                out.push(glow);
            }
            if flags & 1 != 0 {
                out.push(::render::Corona {
                    size: l.size * 1.25,
                    rotating: 2,
                    flags: 8,
                    texture: crate::lights::star_texture_id(),
                    ..glow
                });
            }
            // in fog a halo round it, seen from in front (sizes and strengths in
            // `lights::collect` and corona.wgsl, like the cone's)
            if flags & 2 == 0 {
                out.push(::render::Corona {
                    position: p,
                    size: l.size,
                    color,
                    brightness: b,
                    direction: dir,
                    cone_cos: (l.cone_outer.max(l.cone_inner) * 0.5).to_radians(),
                    inner_cos: (l.cone_inner.min(l.cone_outer).max(0.0) * 0.5).to_radians(),
                    texture: crate::lights::glow_texture_id(),
                    halo: true,
                    ..Default::default()
                });
            }
            // the light's cone in fog (the original: built for a directional light
            // with the cone flag whose cone angles make sense; effect bit 2 leaves it out).
            // Its size and strength follow the weather and the viewer (`lights::collect`,
            // corona.wgsl), so the raw values go along: the light's size and brightness and
            // the half cone angles in radians.
            if l.cone
                && !l.omni
                && dir.length_squared() > 0.5
                && l.cone_inner >= 0.0
                && l.cone_outer >= l.cone_inner
                && flags & 2 == 0
            {
                out.push(::render::Corona {
                    position: p,
                    size: l.size,
                    color: [l.color[0] / 255.0, l.color[1] / 255.0, l.color[2] / 255.0],
                    brightness: b,
                    direction: dir,
                    cone_cos: (l.cone_outer * 0.5).to_radians(),
                    inner_cos: (l.cone_inner * 0.5).to_radians(),
                    texture: crate::lights::cone_texture_id(),
                    beam: true,
                    ..Default::default()
                });
            }
        }
    }
    owners.resize(out.len(), seq.wrapping_sub(1));
    out.into_iter().zip(owners).collect()
}

impl World {
    /// Switch the `NightlightA` material variants of static objects.
    pub fn set_lamps(&self, renderer: &Renderer, scene: &mut Scene, on: bool) {
        for (inst, slot, m_on, m_off) in self.night_slots.lock().iter() {
            renderer.set_material(scene, *inst, *slot, if on { *m_on } else { *m_off });
        }
    }

    /// Whether switch object `id` is set to its path `path` (None: no such switch, or the
    /// path has no `[switchdir]`).
    pub fn switch_set_to(&self, id: i64, path: u16) -> Option<bool> {
        let scripted = self.scripted.lock();
        let by_id = self.scripted_of_object.lock();
        let o = by_id
            .get(&id)
            .and_then(|&i| scripted.get(i))
            .filter(|o| o.map_id == id)
            .or_else(|| scripted.iter().find(|o| o.map_id == id))?;
        let d = (*o.ty.sco.path_switch_dir.get(path as usize)?)?;
        Some(
            o.inst
                .var("Switch")
                .map(|v| (v - d as f32).abs() < 0.5)
                .unwrap_or(false),
        )
    }

    /// Set the railway signals: `aspects` gives each signal object's `Signal` (0 stop, 1 go,
    /// 2 go at the route's speed limit) as the traffic worked it out, and every signal
    /// learns what the next one shows (`NextSignal`) - a distant signal what its main
    /// signal shows.
    pub fn set_signals(&self, aspects: &HashMap<i64, f32>) {
        if self.signal_routes.is_empty() {
            return;
        }
        let mut next: HashMap<i64, f32> = HashMap::new();
        for r in &self.signal_routes {
            let own = aspects.get(&r.signal.0).copied().unwrap_or(0.0);
            if let Some(d) = r.dist_signal {
                let e = next.entry(d).or_insert(0.0);
                *e = e.max(own);
            }
            if let Some(n) = r.next_signal {
                let e = next.entry(r.signal.0).or_insert(0.0);
                *e = e.max(aspects.get(&n).copied().unwrap_or(0.0));
            }
        }
        let ids: hashbrown::HashSet<i64> = self
            .signal_routes
            .iter()
            .flat_map(|r| std::iter::once(r.signal.0).chain(r.dist_signal))
            .collect();
        let mut scripted = self.scripted.lock();
        for o in scripted.iter_mut() {
            if !ids.contains(&o.map_id) {
                continue;
            }
            let a = aspects.get(&o.map_id).copied().unwrap_or(0.0);
            if o.inst.var("Signal") != Some(a) {
                o.inst.set_var("Signal", a);
                if ::legacy_config::env::var_os("OMSI_DEBUG_SIGNALS").is_some() {
                    log::info!(
                        "signal {} at ({:.0}, {:.0}) shows {a}",
                        o.map_id,
                        o.pos.x,
                        o.pos.y
                    );
                }
            }
            o.inst
                .set_var("NextSignal", next.get(&o.map_id).copied().unwrap_or(0.0));
        }
    }

    /// Throw the points the trains need (`Traffic::switch_requests`): a switch object whose
    /// `[path]` carries a `[switchdir]` gets that value in its script's `Switch` variable,
    /// and its blades turn with it.
    pub fn set_switches(&self, requests: &[(i64, u16)]) {
        if requests.is_empty() {
            return;
        }
        let mut scripted = self.scripted.lock();
        let by_id = self.scripted_of_object.lock();
        for &(id, path) in requests {
            let o = if let Some(&i) = by_id.get(&id).filter(|&&i| scripted.get(i).is_some_and(|o| o.map_id == id)) {
                scripted.get_mut(i)
            } else {
                scripted.iter_mut().find(|o| o.map_id == id)
            };
            let Some(o) = o else {
                continue;
            };
            if let Some(Some(d)) = o.ty.sco.path_switch_dir.get(path as usize) {
                let was = o.inst.var("Switch");
                if o.inst.set_var("Switch", *d as f32)
                    && was != Some(*d as f32)
                    && ::legacy_config::env::var_os("OMSI_DEBUG_SWITCHES").is_some()
                {
                    log::info!(
                        "switch {} ({}) at ({:.0}, {:.0}) thrown to {d} for a train",
                        id,
                        o.ty.sco.path.display(),
                        o.pos.x,
                        o.pos.y
                    );
                }
            }
        }
    }
}
