use super::*;

/// A texture name that stands for "no texture": exporters write `null.bmp` into slots
/// that have none (the SD202's IBIS key click spots). The slot shows its material colour;
/// nothing is looked up, and nothing is reported missing.
pub(crate) fn is_null_texture(name: &str) -> bool {
    let n = name.trim();
    n.is_empty()
        || Path::new(&n.replace('\\', "/"))
        .file_stem()
        .is_some_and(|s| s.eq_ignore_ascii_case("null"))
}

pub(super) fn scenery_texture_key(name: &str) -> String {
    name.trim().replace('\\', "/").to_ascii_lowercase()
}

pub(super) fn scenery_texture_selection_matches(
    ot: &ObjectType,
    inst: &::simulation::scenery::SceneryInstance,
    prev: Option<&[usize]>,
) -> bool {
    let Some(prev) = prev else {
        return false;
    };
    if prev.len() != ot.dynamic_textures.len() {
        return false;
    }
    for (group, &expected) in ot.dynamic_textures.iter().zip(prev) {
        let actual = match inst.var(&group.variable) {
            Some(v) if v.is_finite() && v >= 0.0 => {
                let idx = v.trunc() as usize;
                if idx < group.choices.len() {
                    idx
                } else {
                    usize::MAX
                }
            }
            _ => usize::MAX,
        };
        if actual != expected {
            return false;
        }
    }
    true
}

pub(super) fn scenery_texture_selection(
    ot: &ObjectType,
    inst: &::simulation::scenery::SceneryInstance,
) -> Vec<usize> {
    ot.dynamic_textures
        .iter()
        .map(|group| {
            let Some(value) = inst.var(&group.variable) else {
                return usize::MAX;
            };
            if !value.is_finite() || value < 0.0 {
                return usize::MAX;
            }
            let index = value.trunc() as usize;
            if index < group.choices.len() {
                index
            } else {
                usize::MAX
            }
        })
        .collect()
}

/// The Direct3D material of a slot as OMSI sets it: a `[matl_allcolor]` (diffuse rgba,
/// ambient rgb, specular rgb, emissive rgb, power) replaces the o3d file's material. Returns (diffuse colour, emissive colour, specular colour and power).
/// OMSI uses a texture's own colour for an ordinary o3d material; its diffuse value is used
/// only without a texture. A `[matl_allcolor]` explicitly replaces that material and still
/// tints the texture. The diffuse alpha only counts where there is no texture.
/// The emissive colour lights the texture by itself (the NL202's interior display, the
/// lamps of a traffic light); the specular term is the sun's highlight.
pub(super) fn d3d_material(
    m: &::legacy_o3d::Material,
    allcolor: Option<[f32; 14]>,
    textured: bool,
) -> ([f32; 4], [f32; 3], [f32; 4], [f32; 3]) {
    // (the ambient colour: Omsi.exe gives every o3d slot a white one, 0x7c62f8, and a
    // textured .x slot too, 0x7c6d2d; a [matl_allcolor] sets its own)
    let (diffuse, emissive, specular, power, ambient) = match allcolor {
        Some(v) => (
            [v[0], v[1], v[2], v[3]],
            [v[10], v[11], v[12]],
            [v[7], v[8], v[9]],
            v[13],
            [v[4], v[5], v[6]],
        ),
        None => (
            m.diffuse,
            m.emissive,
            m.specular,
            m.specular_power,
            [1.0; 3],
        ),
    };
    let clamp01 = |x: f32| {
        if x.is_finite() {
            x.clamp(0.0, 1.0)
        } else {
            0.0
        }
    };
    let color = if textured && allcolor.is_none() {
        [1.0; 4]
    } else {
        [
            clamp01(diffuse[0]),
            clamp01(diffuse[1]),
            clamp01(diffuse[2]),
            if textured { 1.0 } else { clamp01(diffuse[3]) },
        ]
    };
    let emissive = emissive.map(clamp01);
    let specular = specular.map(clamp01);
    // D3D ignores the specular colour without a power to raise the highlight to
    let power = if power.is_finite() && power >= 1.0 && specular.iter().any(|c| *c > 0.004) {
        power.min(256.0)
    } else {
        0.0
    };
    (
        color,
        emissive,
        [specular[0], specular[1], specular[2], power],
        ambient.map(clamp01),
    )
}

/// The material manager's depth and reflection settings of a slot's `[matl]` commands
/// (`bump`: the loaded `[matl_bumpmap]` height map and its factor).
pub(super) fn material_extra(
    ov: &[&MaterialDef],
    env_mask: Option<TextureId>,
    bump: Option<(TextureId, f32)>,
    specular: [f32; 4],
) -> MaterialExtra {
    MaterialExtra {
        env_mask,
        no_z_write: ov.iter().any(|o| o.no_z_write),
        // `[matl_noZcheck]` leaves Omsi.exe's depth test on: its draw of the slot (0x7fd6c4)
        // never reads the flag, which only adds a colourless stencil pass marking the panes
        // for the raindrops (0x7c32c4 -> 0x7fc58c, ZENABLE 1, blend ZERO/ONE). Taken as "no
        // depth test", the Sprinter's inner window glass (flagged so) was drawn over the
        // body skin round every opening. OMSI_NOZCHECK_BIAS=1: the old reading.
        no_z_check: ov.iter().any(|o| o.no_z_check)
            && ::legacy_config::env::var_os("OMSI_NOZCHECK_BIAS").is_some(),
        z_bias: ov.iter().map(|o| o.z_bias).find(|b| *b != 0).unwrap_or(0),
        ambient: None,
        specular,
        bump: bump.filter(|b| b.1.is_finite() && b.1 != 0.0),
        glass: false,
        night_switched: false,
        rain_film: false,
        water: false,
        display: false,
        screen: false,
        led: false,
        html: false,
        no_map_lights: false,
        tree: false,
        moisture: 0.0,
        transmap_declared: ov.iter().any(|o| o.transmap.is_some()),
        // (the last addressing command of the slot decides; the colour is given in bytes)
        border: ov
            .iter()
            .rev()
            .find(|o| o.tex_address != ::model::TexAddress::Wrap)
            .filter(|o| o.tex_address == ::model::TexAddress::Border)
            .map(|o| o.border_color.map(|c| (c / 255.0).clamp(0.0, 1.0))),
        metal_ok: false,
    }
}

/// The addressing of a slot's textures: its last `[matl_texadress_*]` command decides (the
/// border mode is clamped, its colour comes with `material_extra`).
pub(super) fn tex_addressing<'a>(
    ov: impl DoubleEndedIterator<Item = &'a MaterialDef>,
) -> ::render::TexAddressing {
    use ::model::TexAddress as A;
    use ::render::TexAddressing as R;
    match ov.rev().map(|o| o.tex_address).find(|a| *a != A::Wrap) {
        None | Some(A::Wrap) => R::Wrap,
        Some(A::Mirror) => R::Mirror,
        Some(A::Clamp | A::Border) => R::Clamp,
        Some(A::MirrorOnce) => R::MirrorOnce,
    }
}

/// The key a `[matl_bumpmap]` height map of `path` is kept under (the same file may be a
/// colour texture as well).
pub(super) fn bump_key(path: &Path) -> PathBuf {
    PathBuf::from(format!("{}#bump", path.display()))
}

/// Whether a texture file is a season's snow picture: it lies in a `WinterSnow` folder
/// (`Texture\WinterSnow\gras.bmp`, any case), where the snow weather finds the map's
/// snowy textures.
pub(super) fn is_snow_picture(path: &Path) -> bool {
    path.components().any(|c| {
        c.as_os_str()
            .to_str()
            .is_some_and(|s| s.eq_ignore_ascii_case("WinterSnow"))
    })
}

/// A PBR set beside the diffuse texture `path` (`foo_n.png` and the rest, see
/// `::texture::pbr`), put up and tied to texture `id` for the materials made with it.
pub(crate) fn attach_pbr(renderer: &Renderer, scene: &mut Scene, path: &Path, id: TextureId) {
    // (and a season's snow picture is known as one: it gets no snow laid over it, #879)
    if is_snow_picture(path) {
        scene.snow_textures.insert(id);
    }
    if ::legacy_config::env::var_os("OMSI_NO_PBR").is_some() {
        return;
    }
    let files = ::texture::pbr::find(path);
    if files.is_empty() {
        return;
    }
    if let Some(set) = ::texture::pbr::load_set(&files) {
        log::info!(
            "PBR maps for {}: normal {:?}, occlusion/roughness/metal {:?}",
            path.display(),
            set.normal.as_ref().map(|i| (i.width, i.height)),
            set.flags
        );
        renderer.add_pbr_maps(scene, id, &set);
    }
}

/// The texture data for a key of the texture maps: a file, or a file's bump height map.
pub(super) fn load_texture_key(key: &Path, compress: bool) -> Option<TextureData> {
    let k = key.to_string_lossy();
    match k.strip_suffix("#bump") {
        Some(file) => ::texture::decode_file(Path::new(file))
            .map_err(|e| log::warn!("{e}"))
            .ok()
            .map(|img| ::texture::gpu::prepare_bump(&img, compress)),
        None => {
            if compress {
                ::texture::gpu::load_gpu(key).ok().map(|t| t.0)
            } else {
                ::texture::gpu::load_gpu_fast(key).ok().map(|t| t.0)
            }
        }
    }
}

/// Decide the alpha mode of an o3d material from the model.cfg `[matl]` overrides.
pub(crate) fn material_alpha(
    materials: &[::legacy_o3d::Material],
    slot: usize,
    overrides: &[MaterialDef],
) -> AlphaMode {
    // the plain [matl] overrides of this slot decide; without one: opaque. A
    // `[matl_change]` record only opens the variants (`[matl_item]`) and says nothing of the
    // slot's own look: a `[matl]` of the same slot after it does. (The LED matrices of
    // churaPixel/Krüger++ open a change first and give the slot `[matl_alpha] 2` and the
    // script texture as its mask in a `[matl]` after it: taken as opaque from the change,
    // the mask cut nothing and the whole panel was lit.)
    // Several plain [matl] of one slot are one material in OMSI: each selects it again and
    // the commands after it modify it, so the last `[matl_alpha]` among them counts. (Taken
    // from the first block alone, an alpha-tested texture whose `[matl_alpha]` sits in a
    // second [matl] was drawn opaque, its transparent parts as solid areas.) model
    // already joins blocks spelt the same; this covers those that reach the slot otherwise
    // (an index of -1 selects the first one, as 0 does).
    let mine: Vec<&MaterialDef> = overrides
        .iter()
        .filter(|o| !o.item && ::simulation::vehicle::override_slot(materials, o) == Some(slot))
        .collect();
    let plain = || mine.iter().filter(|o| o.change.is_none());
    plain()
        .rev()
        .find(|o| o.alpha_set)
        .or_else(|| plain().next())
        .or(mine.first())
        .map(|o| alpha_mode(o.alpha))
        .unwrap_or(AlphaMode::Opaque)
}

pub(super) fn alpha_mode(a: i32) -> AlphaMode {
    match a {
        0 => AlphaMode::Opaque,
        1 => AlphaMode::Test,
        _ => AlphaMode::Blend,
    }
}

/// Whether a vehicle's `[useTextTexture]` slot is a display (`MaterialExtra::display`): one
/// that has a light of its own, a light map or a night map (a destination matrix, a
/// counter lit with the dashboard), not lettering on the body.
pub(super) fn text_is_display(lightmap: bool, night: bool) -> bool {
    lightmap || night
}

/// How a `[texttexture]` shows on its slot: alpha tested where the slot's `[matl_alpha]` is 1
/// (the stock route helpers, `routearrows_busstop.sco`: blended, the empty part of the text
/// wrote depth and cut away whatever was drawn behind it later - a bus beside the stop lost
/// half its roof), blended otherwise.
pub(super) fn text_alpha(
    materials: &[::legacy_o3d::Material],
    slot: usize,
    overrides: &[MaterialDef],
) -> AlphaMode {
    match material_alpha(materials, slot, overrides) {
        AlphaMode::Test => AlphaMode::Test,
        _ => AlphaMode::Blend,
    }
}

/// Placement is part of the picture: otherwise a centred sign can lend its cached
/// texture to a left-aligned one showing the same words.
pub(super) fn scenery_text_key(tt: &::model::TextTexture, text: &str, alpha: AlphaMode) -> String {
    format!(
        "{}|{}|{}x{}|{}|{:?}|{:?}|{}|{}",
        tt.font.to_ascii_lowercase(),
        text,
        tt.width.max(1),
        tt.height.max(1),
        tt.full_color,
        tt.color,
        alpha,
        tt.orientation,
        tt.grid,
    )
}

pub(super) fn scenery_text_image(
    tt: &::model::TextTexture,
    atlas: Option<Arc<::content::font::FontAtlas>>,
    text: &str,
) -> Image {
    // Static and scripted text textures use the placement from their definition.
    let state = ::simulation::texttex::TextTextureState::new(tt.clone(), atlas);
    Image {
        width: tt.width.max(1) as u32,
        height: tt.height.max(1) as u32,
        rgba: state.image(text),
        has_alpha: true,
    }
}

/// The name of a texture's night copy: the same file in a `night` folder beside it.
/// Whether an object's texture has its night copy (see [`night_texture_name`]). A texture
/// named by its full path - a parked car's paint, resolved in its scheme's folder - has it
/// there or not at all: the texture lookup takes such a path for one of its author's
/// machine and falls back to the bare file name, which found the day picture itself, and
/// every parked car of a paint scheme was lit by its own paint at night, glowing in the
/// dark street.
pub(super) fn night_texture_exists(rel: &str, dirs: &[&Path]) -> bool {
    // (`night_texture_name` writes backslashes: "\\Users\\...", "C:\\...")
    let norm = rel.trim().replace('\\', "/");
    if norm.starts_with('/') || norm.as_bytes().get(1) == Some(&b':') {
        return ::legacy_config::vfs::is_file(Path::new(&norm));
    }
    ::texture::find_texture(rel, dirs).is_some()
}

pub(super) fn night_texture_name(texture: &str) -> String {
    let name = texture.trim().replace('/', "\\");
    match name.rsplit_once('\\') {
        Some((dir, file)) => format!("{dir}\\night\\{file}"),
        None => format!("night\\{name}"),
    }
}
