use super::*;

#[derive(Clone, Default, Debug, PartialEq)]
pub struct LampSlots {
    pub count: usize,
    pub alpha: Vec<(usize, String)>,
    pub light: Vec<(usize, String)>,
}

impl LampSlots {
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

pub struct ParticleObject {
    pub map_id: i64,
    pub pos: DVec3,
    pub rot: Mat4,
    pub set: ::simulation::particles::ParticleSet,
}

#[derive(Debug, Clone)]
pub struct StaticCorona {
    pub corona: ::render::Corona,
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

    /// The variable of the object's script this switch follows, if the script has it.
    pub fn script_var(&self, program: Option<&::legacy_script::Program>) -> Option<&str> {
        match self {
            LightSwitch::Variable(v) if program.is_some_and(|p| p.var(v).is_some()) => Some(v),
            _ => None,
        }
    }
}

/// The coronas of a placed object switched by its script, with their variable.
pub fn script_coronas(ot: &ObjectType, xf: Mat4, pos: DVec3) -> Vec<(::render::Corona, String)> {
    let switches: Mutex<Vec<LightSwitch>> = Mutex::new(Vec::new());
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
    coronas
        .into_iter()
        .filter_map(|(c, k)| {
            let var = switches.get(k)?.script_var(ot.program.as_deref())?;
            Some((c, var.to_string()))
        })
        .collect()
}

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

pub(crate) fn traffic_lamp_value(var: &str, scripted: Option<f32>, standard: Option<f32>) -> f32 {
    scripted
        .or_else(|| var.trim().parse::<f32>().ok())
        .or(standard)
        .unwrap_or(0.0)
}

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

struct EnhLight<'a> {
    pos: [f32; 3],
    dir: [f32; 3],
    up: [f32; 3],
    omni: bool,
    rotating: i32,
    color: [f32; 3],
    size: f32,
    cone_inner: f32,
    cone_outer: f32,
    variable: &'a str,
    z_offset: f32,
    effect: u8,
    cone: bool,
    bitmap: Option<&'a str>,
}

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
        if md.light_enh.is_empty() && md.light_enh_2.is_empty() {
            continue;
        }
        let first_li = li;
        li += md.light_enh_2.len();
        let xf = mesh_transforms(i);
        for l in &md.light_enh {
            owners.resize(out.len(), seq.wrapping_sub(1));
            seq += 1;
            // after the variable: factor, z offset, effect bits, time constant
            let factor = l.values.first().copied().unwrap_or(0.0);
            let b = value_of(&l.variable) * if factor > 0.0 { factor } else { 1.0 };
            if !(b > 0.0) || !b.is_finite() {
                continue;
            }
            let light = EnhLight {
                pos: l.pos,
                dir: [0.0; 3],
                up: [0.0; 3],
                omni: true,
                rotating: 2,
                color: l.color,
                size: l.size,
                cone_inner: 0.0,
                cone_outer: 0.0,
                variable: &l.variable,
                z_offset: l.values.get(1).copied().unwrap_or(0.0),
                effect: l.values.get(2).map_or(0, |e| (*e as i32).clamp(0, 7) as u8),
                cone: false,
                bitmap: l.texture.as_deref(),
            };
            push_light_sprites(&mut out, &light, b, &xf, pos, &model_dir);
        }
        for (k, l) in md.light_enh_2.iter().enumerate() {
            owners.resize(out.len(), seq.wrapping_sub(1));
            seq += 1;
            let b = match fades.get(first_li + k) {
                Some(f) => *f,
                None => value_of(&l.variable) * if l.factor > 0.0 { l.factor } else { 1.0 },
            };
            if !(b > 0.0) || !b.is_finite() {
                continue;
            }
            let light = EnhLight {
                pos: l.pos,
                dir: l.dir,
                up: l.up,
                omni: l.omni,
                rotating: l.rotating,
                color: l.color,
                size: l.size,
                cone_inner: l.cone_inner,
                cone_outer: l.cone_outer,
                variable: &l.variable,
                z_offset: l.z_offset,
                effect: l
                    .values
                    .first()
                    .map(|v| ::legacy_config::parse_f32(v) as i32)
                    .unwrap_or(0)
                    .clamp(0, 7) as u8,
                cone: l.cone,
                bitmap: l.bitmap.as_deref(),
            };
            push_light_sprites(&mut out, &light, b, &xf, pos, &model_dir);
        }
    }
    owners.resize(out.len(), seq.wrapping_sub(1));
    out.into_iter().zip(owners).collect()
}

/// `strength`: the light's variable times its factor, as Omsi.exe keeps it - the star grows
/// with it unclamped, the sprites' brightness stops at 2.
fn push_light_sprites(
    out: &mut Vec<::render::Corona>,
    l: &EnhLight,
    strength: f32,
    xf: &Mat4,
    pos: DVec3,
    model_dir: &std::path::Path,
) {
    let b = strength.clamp(0.0, 2.0);
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
    let flags = l.effect;
    let color = [l.color[0] / 255.0, l.color[1] / 255.0, l.color[2] / 255.0];
    let effect_tex = l
        .bitmap
        .map(|b| crate::lights::corona_texture_id(model_dir, b))
        .filter(|t| *t != 0);
    let glow = ::render::Corona {
        position: p,
        size: (l.size * 0.5).max(0.0),
        color,
        brightness: b,
        direction: dir,
        cone_cos: half_cos(outer),
        inner_cos: if inner > 0.0 { half_cos(inner) } else { -2.0 },
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
            }
            | if effect_tex.is_some() { 32 } else { 0 },
        texture: effect_tex.unwrap_or_else(crate::lights::glow_texture_id),
        ..Default::default()
    };
    if flags & 4 == 0 {
        out.push(glow);
    }
    if flags & 1 != 0 {
        out.push(::render::Corona {
            // Omsi.exe sizes it 2.5 x size x 0.6 x strength x (0.8 + night^2) (FUN_005a0118):
            // with a factor of 4 a C2's 5 cm headlamp throws a half-metre star. The night's
            // share is taken by `prepare_coronas`; 1.5 is the size measured at a strength of
            // 1 by night (#324).
            size: l.size * 1.5 * strength,
            rotating: 2,
            flags: 8 | 32,
            texture: crate::lights::star_texture_id(),
            ..glow
        });
    }
    if flags & 2 == 0 {
        out.push(::render::Corona {
            position: p,
            size: l.size,
            color,
            brightness: b,
            direction: dir,
            cone_cos: (outer * 0.5).to_radians(),
            inner_cos: (inner.max(0.0) * 0.5).to_radians(),
            texture: crate::lights::glow_texture_id(),
            halo: true,
            ..Default::default()
        });
    }
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
            color,
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

impl World {
    pub fn set_lamps(&self, renderer: &Renderer, scene: &mut Scene, on: bool) {
        for (inst, slot, m_on, m_off) in self.night_slots.lock().iter() {
            renderer.set_material(scene, *inst, *slot, if on { *m_on } else { *m_off });
        }
    }

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
