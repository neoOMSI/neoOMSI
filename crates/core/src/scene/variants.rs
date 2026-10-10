use super::*;

/// Whether a `[matl_change]` variable at `x` shows the slot's `[matl_item]`: Omsi.exe
/// (0x5fd6xx) rounds the variable (to the nearest, ties to even) and shows item `n` for
/// 1 <= n <= the items there are, the plain material otherwise - a lamp's variable at 2
/// with one item is dark. A variable no script declares is registered by the model loader
/// at 0 (the stock MANs' spare buttons, `*Noch nicht belegt*`, and a mod's door lamps
/// were lit for good when it was taken as on, #231).
pub(crate) fn change_picks_item(x: f32) -> bool {
    x.is_finite() && x.round_ties_even() == 1.0
}

/// The item a `[matl_change]` variable at `x` shows: its rounded value n >= 1, 0 = the plain
/// material.
pub(crate) fn variant_number(x: f32) -> usize {
    if x.is_finite() {
        let n = x.round_ties_even();
        if (1.0..=65535.0).contains(&n) {
            return n as usize;
        }
    }
    0
}

/// `looks` = [plain material, item 1, item 2, ...]; an item there is not falls back to the plain one.
pub(super) fn look_of(looks: &[MaterialId], n: usize) -> MaterialId {
    looks.get(n).copied().unwrap_or(looks[0])
}

/// A scenery `[matl_change]` variant: (instance or mesh index, slot, base material, item 1,
/// variable, items 2, 3, ... of the same `[matl_change]`).
pub type SceneryVariant = (usize, usize, MaterialId, MaterialId, String, Vec<MaterialId>);

/// The material a `[matl_change]` variable at `x` shows: item `n` (rounded, 1 <= n <= the
/// items there are), the plain material otherwise.
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

/// Apply [matl_change] variant switches to scenery instances, honoring any active dynamic
/// material overrides from [CTC] / [texchanges].
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

/// A material variant switched by a variable.
#[derive(Clone)]
pub struct VariantSlot {
    pub mesh: usize,
    pub slot: usize,
    pub base: MaterialId,
    pub item: MaterialId,
    /// Items 2, 3, ... of the first `[matl_change]`.
    pub more: Vec<MaterialId>,
    /// `[matl_change]` variable: at 1 (rounded) the item variant shows.
    pub var: String,
    /// The variables of the slot's further `[matl_change]`s: the item shows while any of
    /// them is on as well (Omsi.exe sub_7c2d80: each record picks its item by its own
    /// variable, and one at 0 leaves the material to the others).
    pub more_vars: Vec<String>,
    /// `[texchanges]`: the (base, item) pair of every entry of the master, in order.
    pub entries: Vec<(MaterialId, MaterialId)>,
    /// `[texchanges]` variable: its integer value picks the entry.
    pub tex_var: String,
    /// Free textures for the plain material and, independently, its switched item.
    pub free: Vec<FreeTex>,
    /// How to build a material of this slot for a texture loaded later.
    pub spec: SlotSpec,
    /// The textures `base`/`item` and each entry were made with (made again per vehicle
    /// when the slot shows the vehicle's own pictures, see `SlotSpec::per_vehicle`).
    pub base_tex: Option<TextureId>,
    pub entry_tex: Vec<Option<TextureId>>,
    /// Several `[matl_lightmap]`s on the slot (see `MultiLight`).
    pub lights: Option<MultiLight>,
}

/// A material slot with several `[matl_lightmap]`s, each a texture and a variable (the
/// LiAZ 5292's saloon: the cab lamp, saloon circuit 1 and circuit 2). OMSI keeps them
/// all; drawn with only the last one, the
/// saloon stayed unlit whenever circuit 2 was off. The slot's light map is the sum of the
/// maps switched on, made the first time that combination shows and shared by every
/// vehicle of the kind; the slot is as bright as its brightest variable.
#[derive(Clone)]
pub struct MultiLight {
    /// The maps' files and variables, in the order the model lists them.
    pub maps: Vec<(PathBuf, String)>,
    /// The set's own materials (the last map), shown while none is on.
    pub plain: (MaterialId, MaterialId),
    /// Materials made per switched-on combination (bit k: map k).
    pub cache: HashMap<u32, (MaterialId, MaterialId)>,
    pub current: u32,
    /// Composite maps are shared like the vehicles' pictures (see `FreeTex::shared`).
    pub shared: Arc<Mutex<HashMap<PathBuf, (TextureId, usize)>>>,
    pub held: Vec<PathBuf>,
}

impl MultiLight {
    /// The light map made of the maps in `mask`, from the shared store or read now.
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
                    // (maps of another size are sampled at the nearest texel)
                    let (w, h) = (acc.width as usize, acc.height as usize);
                    let (iw, ih) = (img.width as usize, img.height as usize);
                    for y in 0..h {
                        let sy = y * ih / h.max(1);
                        for x in 0..w {
                            let sx = x * iw / w.max(1);
                            let (d, s) = ((y * w + x) * 4, (sy * iw + sx) * 4);
                            for c in 0..3 {
                                // (ADDSMOOTH, as Omsi.exe chains a slot's maps in its
                                // texture stages, 0x7fe5ff: a + b - a b)
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

/// `[matl_freetex]`: the slot shows the texture file named by a string variable - the
/// SD200's destination roller reads the terminus pictures of the map's `.hof` this way.
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
    /// The original named texture can be used by several stages (a display commonly
    /// names the same black texture as its diffuse and its switched night map).
    pub key: Option<TextureId>,
    pub diffuse: bool,
    /// A declaration inside `[matl_item]` must not change the unpowered material.
    pub item_only: bool,
    /// Where the file name is looked up (the vehicle's texture folders).
    pub dirs: Vec<PathBuf>,
    pub textures: Arc<::texture::TextureCache>,
    /// File name (lower case) → the materials already built for it.
    pub cache: HashMap<String, (MaterialId, MaterialId)>,
    /// The name currently applied.
    pub current: Option<String>,
    /// The world's shared vehicle textures (a picture is one texture for every vehicle
    /// showing it; every bus had its own copy, half a gigabyte of destination pictures on
    /// Ahlheim), the pictures this vehicle holds, and where pictures uploaded as RGBA are
    /// sent to be compressed.
    pub shared: Arc<Mutex<HashMap<PathBuf, (TextureId, usize)>>>,
    pub held: Vec<PathBuf>,
    pub wants_upgrade: Arc<Mutex<Vec<PathBuf>>>,
}

impl VariantSlot {
    /// The material the slot shows now: `[texchanges]` picks the texture, `[matl_change]`
    /// then picks between the plain material and the `[matl_item]` variant.
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

/// How one material slot is built, so that the same description can be applied to every
/// texture a `[texchanges]` master or a `[matl_freetex]` string switches between.
#[derive(Clone)]
pub struct SlotSpec {
    pub(super) base: Look,
    /// The `[matl_item]` half.
    pub(super) item: Option<Look>,
    /// The first `[matl_change]`'s items after its first (shown at 2, 3, ...).
    pub(super) more: Vec<Look>,
}

/// How one half of a material slot (the plain material or its `[matl_item]`) is drawn.
#[derive(Clone)]
pub struct Look {
    pub(super) alpha: AlphaMode,
    pub(super) color: [f32; 4],
    pub(super) emissive: [f32; 3],
    pub(super) unlit: bool,
    /// A picture of the vehicle's own in place of the diffuse texture (see `DynTex`).
    pub(super) diffuse: Option<TextureId>,
    pub(super) transmap: Option<(TextureId, bool)>,
    pub(super) night: Option<TextureId>,
    pub(super) lightmap: Option<TextureId>,
    pub(super) envmap: Option<(TextureId, f32)>,
    pub(super) extra: MaterialExtra,
    pub(super) dyn_tex: DynTex,
}

/// The pictures of its own vehicle a half of a slot shows: `[useTextTexture]`,
/// `[useScriptTexture]` and a script texture as the transparency map
/// (`[matl_transmap] \S:n`), and whether it is addressed without repeating.
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

    /// This half with one vehicle's text and script textures, set up as
    /// `instantiate_vehicle` sets up a `DynSlot`.
    pub(super) fn for_vehicle(&self, text: &[Option<TextureId>], script: &[Option<TextureId>]) -> Look {
        let d = self.dyn_tex;
        let mut l = self.clone();
        l.dyn_tex = DynTex {
            address: d.address,
            ..DynTex::default()
        };
        if let Some(t) = d.text.and_then(|i| text.get(i).copied().flatten()) {
            // lit as the slot's own material is, as `instantiate_vehicle` makes a text slot
            // that is not switched: drawn unlit, a switched slot's fleet number or plate
            // shone at full brightness at night (#698)
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
        // A transmap is not a reason by itself to force a material into blend mode; it only
        // carries the alpha for the chosen material mode. Keep any explicit alpha setting,
        // otherwise body slots stay solid.
        if d.script.is_some() {
            l.color = [1.0; 4];
            l.emissive = [0.0; 3];
            l.unlit = true;
            // (shown as the script draws it, like an HTML page: the slot's night, light and
            // sphere maps would light the display's black parts)
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
            // A per-vehicle text/script texture has already replaced the original
            // diffuse and is not the file named by this free-texture declaration.
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
        spec
    }

    /// (plain material, `[matl_item]` material) for one diffuse texture; without a
    /// `[matl_item]` both are the same material.
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

    /// The slot shows pictures of its own vehicle: every vehicle makes its materials with
    /// `for_vehicle`.
    /// The same slot with another light map.
    pub fn set_lightmap(&mut self, tex: Option<TextureId>) {
        self.base.lightmap = tex;
        if let Some(it) = &mut self.item {
            it.lightmap = tex;
        }
        for it in &mut self.more {
            it.lightmap = tex;
        }
    }

    /// The further items' materials (each made last, so that `recycle` can move it).
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

/// Resolve the texture name for a scenery object's `[matl_freetex]` slot.
/// Tries the object's script variable first, then freetex probe, and falls back to
/// tile placement strings (by explicit numeric index or by freetex declaration order).
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
