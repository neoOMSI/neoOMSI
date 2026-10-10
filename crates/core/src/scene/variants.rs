use super::*;

pub(crate) fn change_picks_item(x: f32) -> bool {
    x.is_finite() && x.round_ties_even() == 1.0
}

pub(crate) fn variant_number(x: f32) -> usize {
    if x.is_finite() {
        let n = x.round_ties_even();
        if (1.0..=65535.0).contains(&n) {
            return n as usize;
        }
    }
    0
}

pub(super) fn look_of(looks: &[MaterialId], n: usize) -> MaterialId {
    looks.get(n).copied().unwrap_or(looks[0])
}

pub type SceneryVariant = (usize, usize, MaterialId, MaterialId, String, Vec<MaterialId>);

pub(crate) fn pick_variant(
    x: f32,
    base: MaterialId,
    item: MaterialId,
    more: &[MaterialId],
) -> MaterialId {
    if x.is_finite() {
        let n = x.round_ties_even();
        if n >= 2.0 && ((n - 2.0) as usize) < more.len() {
            return more[(n - 2.0) as usize];
        }
    }
    if change_picks_item(x) {
        item
    } else {
        base
    }
}

pub(crate) fn apply_scenery_variants(
    variants: &[SceneryVariant],
    dynamic_materials: &HashMap<(usize, usize), Vec<MaterialId>>,
    nightlight: f32,
    var: &impl Fn(&str) -> Option<f32>,
    renderer: &Renderer,
    scene: &mut Scene,
) {
    for (inst, slot, base, item, var_name, more) in variants {
        let x = if var_name.trim().eq_ignore_ascii_case("NightlightA") {
            nightlight
        } else {
            var_name
                .trim()
                .parse()
                .ok()
                .or_else(|| var(var_name))
                .unwrap_or(0.0)
        };
        let n = variant_number(x);
        let target_mat = if let Some(dyn_looks) = dynamic_materials.get(&(*inst, *slot)) {
            look_of(dyn_looks, n)
        } else {
            pick_variant(x, *base, *item, more)
        };
        if scene
            .instances
            .get(*inst)
            .and_then(|i| i.materials.get(*slot))
            != Some(&target_mat)
        {
            renderer.set_material(
                scene,
                *inst,
                *slot,
                target_mat,
            );
        }
    }
}

#[derive(Clone)]
pub struct VariantSlot {
    pub mesh: usize,
    pub slot: usize,
    pub base: MaterialId,
    pub item: MaterialId,
    pub more: Vec<MaterialId>,
    pub var: String,
    pub more_vars: Vec<String>,
    pub entries: Vec<(MaterialId, MaterialId)>,
    pub tex_var: String,
    pub free: Vec<FreeTex>,
    pub spec: SlotSpec,
    pub base_tex: Option<TextureId>,
    pub entry_tex: Vec<Option<TextureId>>,
    pub lights: Option<MultiLight>,
}

#[derive(Clone)]
pub struct MultiLight {
    pub maps: Vec<(PathBuf, String)>,
    pub plain: (MaterialId, MaterialId),
    pub cache: HashMap<u32, (MaterialId, MaterialId)>,
    pub current: u32,
    pub shared: Arc<Mutex<HashMap<PathBuf, (TextureId, usize)>>>,
    pub held: Vec<PathBuf>,
}

impl MultiLight {
    pub(super) fn composite(
        &mut self,
        renderer: &Renderer,
        scene: &mut Scene,
        mask: u32,
    ) -> Option<TextureId> {
        let mut key = String::from("lightmap-sum:");
        for (k, (path, _)) in self.maps.iter().enumerate() {
            if mask & (1 << k) != 0 {
                key.push_str(&path.to_string_lossy());
                key.push('|');
            }
        }
        let key = PathBuf::from(key);
        let mut shared = self.shared.lock();
        if let Some(e) = shared.get_mut(&key) {
            e.1 += 1;
            self.held.push(key);
            return Some(e.0);
        }
        let mut sum: Option<::texture::Image> = None;
        for (k, (path, _)) in self.maps.iter().enumerate() {
            if mask & (1 << k) == 0 {
                continue;
            }
            let Ok(img) = ::texture::decode_file(path) else {
                continue;
            };
            match &mut sum {
                None => sum = Some(img),
                Some(acc) => {
                    let (w, h) = (acc.width as usize, acc.height as usize);
                    let (iw, ih) = (img.width as usize, img.height as usize);
                    for y in 0..h {
                        let sy = y * ih / h.max(1);
                        for x in 0..w {
                            let sx = x * iw / w.max(1);
                            let (d, s) = ((y * w + x) * 4, (sy * iw + sx) * 4);
                            for c in 0..3 {
                                let (a, b) = (acc.rgba[d + c] as u32, img.rgba[s + c] as u32);
                                acc.rgba[d + c] = (a + b - a * b / 255).min(255) as u8;
                            }
                        }
                    }
                }
            }
        }
        let img = sum?;
        let id = renderer.add_texture(scene, &img, true);
        shared.insert(key.clone(), (id, 1));
        self.held.push(key);
        Some(id)
    }
}

pub(super) fn free_texture_defs(overrides: &[&MaterialDef]) -> Vec<(bool, String, String)> {
    [false, true]
        .into_iter()
        .filter_map(|item| {
            overrides.iter().filter(|o| o.item == item).find_map(|o| {
                o.freetex
                    .as_ref()
                    .map(|(key, var)| (item, key.clone(), var.clone()))
            })
        })
        .collect()
}

#[derive(Clone)]
pub struct FreeTex {
    pub var: String,
    pub key: Option<TextureId>,
    pub diffuse: bool,
    pub item_only: bool,
    pub dirs: Vec<PathBuf>,
    pub textures: Arc<::texture::TextureCache>,
    pub cache: HashMap<String, (MaterialId, MaterialId)>,
    pub more_cache: HashMap<String, Vec<MaterialId>>,
    pub current: Option<String>,
    pub shared: Arc<Mutex<HashMap<PathBuf, (TextureId, usize)>>>,
    pub held: Vec<PathBuf>,
    pub wants_upgrade: Arc<Mutex<Vec<PathBuf>>>,
}

impl VariantSlot {
    pub fn material(&self, var: impl Fn(&str) -> Option<f32>) -> MaterialId {
        let (base, item) = if self.entries.is_empty() {
            (self.base, self.item)
        } else {
            let v = var(&self.tex_var).unwrap_or(0.0);
            let i = if v.is_finite() { v.trunc() as i64 } else { 0 };
            self.entries[i.clamp(0, self.entries.len() as i64 - 1) as usize]
        };
        let x = self
            .var
            .trim()
            .parse()
            .ok()
            .or_else(|| var(&self.var))
            .unwrap_or(0.0);
        if self.entries.is_empty() && x.is_finite() {
            let n = x.round_ties_even();
            if n >= 2.0 && ((n - 2.0) as usize) < self.more.len() {
                return self.more[(n - 2.0) as usize];
            }
        }
        if change_picks_item(x)
            || self
            .more_vars
            .iter()
            .any(|v| var(v).is_some_and(change_picks_item))
        {
            item
        } else {
            base
        }
    }
}

#[derive(Clone)]
pub struct SlotSpec {
    pub(super) base: Look,
    pub(super) item: Option<Look>,
    pub(super) more: Vec<Look>,
}

#[derive(Clone)]
pub struct Look {
    pub(super) alpha: AlphaMode,
    pub(super) color: [f32; 4],
    pub(super) emissive: [f32; 3],
    pub(super) unlit: bool,
    pub(super) diffuse: Option<TextureId>,
    pub(super) transmap: Option<(TextureId, bool)>,
    pub(super) night: Option<TextureId>,
    pub(super) lightmap: Option<TextureId>,
    pub(super) envmap: Option<(TextureId, f32)>,
    pub(super) extra: MaterialExtra,
    pub(super) dyn_tex: DynTex,
}

#[derive(Debug, Clone, Copy, Default, PartialEq)]
pub struct DynTex {
    pub(super) text: Option<usize>,
    pub(super) script: Option<usize>,
    pub(super) script_trans: Option<usize>,
    pub(super) address: ::render::TexAddressing,
}

impl DynTex {
    pub(super) fn any(&self) -> bool {
        self.text.is_some() || self.script.is_some() || self.script_trans.is_some()
    }
}

impl Look {
    pub(super) fn add(&self, renderer: &Renderer, scene: &mut Scene, tex: Option<TextureId>) -> MaterialId {
        renderer.address_next.set(self.dyn_tex.address);
        renderer.add_material_extra(
            scene,
            self.diffuse.or(tex),
            self.alpha,
            self.color,
            self.unlit,
            self.transmap,
            self.night,
            self.lightmap,
            self.envmap,
            self.emissive,
            self.extra,
        )
    }

    pub(super) fn for_vehicle(&self, text: &[Option<TextureId>], script: &[Option<TextureId>]) -> Look {
        let d = self.dyn_tex;
        let mut l = self.clone();
        l.dyn_tex = DynTex {
            address: d.address,
            ..DynTex::default()
        };
        if let Some(t) = d.text.and_then(|i| text.get(i).copied().flatten()) {
            let mut extra = l.extra;
            extra.display = text_is_display(l.lightmap.is_some(), l.night.is_some());
            extra.screen = true;
            return Look {
                diffuse: Some(t),
                alpha: AlphaMode::Blend,
                color: [1.0; 4],
                emissive: [0.0; 3],
                unlit: false,
                transmap: None,
                envmap: None,
                extra,
                ..l
            };
        }
        if let Some(t) = d.script.and_then(|i| script.get(i).copied().flatten()) {
            l.diffuse = Some(t);
        }
        if let Some(t) = d
            .script_trans
            .and_then(|i| script.get(i).copied().flatten())
        {
            l.transmap = Some((t, true));
        }
        if d.script.is_some() {
            l.color = [1.0; 4];
            l.emissive = [0.0; 3];
            l.unlit = true;
            l.night = None;
            l.lightmap = None;
            l.envmap = None;
            l.extra.night_switched = false;
        }
        l
    }
}

impl SlotSpec {
    pub(super) fn with_freetex(
        &self,
        key: Option<TextureId>,
        tex: TextureId,
        diffuse: bool,
        item_only: bool,
    ) -> Self {
        let mut spec = self.clone();
        let replace = |look: &mut Look| {
            if (diffuse && look.diffuse.is_none()) || (key.is_some() && look.diffuse == key) {
                look.diffuse = Some(tex);
            }
            if let Some(key) = key {
                for stage in [&mut look.night, &mut look.lightmap] {
                    if *stage == Some(key) {
                        *stage = Some(tex);
                    }
                }
                if let Some((id, _)) = &mut look.transmap {
                    if *id == key {
                        *id = tex;
                    }
                }
                if let Some((id, _)) = &mut look.envmap {
                    if *id == key {
                        *id = tex;
                    }
                }
            }
        };
        if !item_only {
            replace(&mut spec.base);
        }
        if let Some(item) = &mut spec.item {
            replace(item);
        }
        for it in &mut spec.more {
            replace(it);
        }
        spec
    }

    pub fn build(
        &self,
        renderer: &Renderer,
        scene: &mut Scene,
        tex: Option<TextureId>,
    ) -> (MaterialId, MaterialId) {
        let base = self.base.add(renderer, scene, tex);
        let item = match &self.item {
            Some(it) => it.add(renderer, scene, tex),
            None => base,
        };
        (base, item)
    }

    pub fn set_lightmap(&mut self, tex: Option<TextureId>) {
        self.base.lightmap = tex;
        if let Some(it) = &mut self.item {
            it.lightmap = tex;
        }
        for it in &mut self.more {
            it.lightmap = tex;
        }
    }

    pub fn build_more(
        &self,
        renderer: &Renderer,
        scene: &mut Scene,
        tex: Option<TextureId>,
        mut recycle: impl FnMut(&mut Scene, MaterialId) -> MaterialId,
    ) -> Vec<MaterialId> {
        self.more
            .iter()
            .map(|l| {
                let m = l.add(renderer, scene, tex);
                recycle(scene, m)
            })
            .collect()
    }

    pub fn per_vehicle(&self) -> bool {
        self.base.dyn_tex.any()
            || self.item.as_ref().is_some_and(|i| i.dyn_tex.any())
            || self.more.iter().any(|i| i.dyn_tex.any())
    }

    pub fn for_vehicle(
        &self,
        text: &[Option<TextureId>],
        script: &[Option<TextureId>],
    ) -> SlotSpec {
        SlotSpec {
            base: self.base.for_vehicle(text, script),
            item: self.item.as_ref().map(|i| i.for_vehicle(text, script)),
            more: self
                .more
                .iter()
                .map(|i| i.for_vehicle(text, script))
                .collect(),
        }
    }
}

pub(crate) fn resolve_scenery_freetex_name<'a>(
    var: &str,
    override_: &MaterialDef,
    overrides: &[MaterialDef],
    object_script: Option<&'a ::simulation::scenery::SceneryInstance>,
    freetex_probe: Option<&'a ::simulation::scenery::SceneryInstance>,
    strings: &'a [String],
) -> Option<&'a str> {
    let script_name = object_script.map(|s| s.str_var(var).trim()).unwrap_or("");
    let probe_name = freetex_probe.map(|p| p.str_var(var).trim()).unwrap_or("");
    let string_by_idx = var
        .parse::<usize>()
        .ok()
        .and_then(|idx| strings.get(idx))
        .map(|s| s.trim())
        .unwrap_or("");
    let freetex_idx = overrides
        .iter()
        .filter(|o| !o.item && o.freetex.is_some())
        .position(|o| std::ptr::eq(o, override_))
        .unwrap_or(0);
    let string_by_order = strings.get(freetex_idx).map(|s| s.trim()).unwrap_or("");
    let name = if !script_name.is_empty() {
        script_name
    } else if !probe_name.is_empty() {
        probe_name
    } else if !string_by_idx.is_empty() {
        string_by_idx
    } else if !string_by_order.is_empty() {
        string_by_order
    } else {
        return None;
    };
    let name = name.trim_matches('"');
    if name.is_empty() { None } else { Some(name) }
}
