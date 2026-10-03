//! `[texttexture]`: textures generated from script string variables with `.oft` fonts.
//!
//! Every text texture is identified by its index in the model's `[texttexture]` list;
//! materials refer to it with `[useTextTexture] n`.

use hashbrown::HashMap;
use omsi_content::font::{Font, FontAtlas};
use omsi_model::TextTexture;
use std::path::Path;
use std::sync::Arc;

pub struct FontLibrary {
    atlases: HashMap<String, Option<Arc<FontAtlas>>>,
    root: std::path::PathBuf,
    /// Every `[newfont]` of every content root's `Fonts/*.oft`, in lookup order; read once.
    index: Option<Vec<Font>>,
}

/// Style words at the end of a font name ("churafont++ 32x8 Bold").
const STYLE_WORDS: &[&str] = &[
    "bold",
    "heavy",
    "black",
    "light",
    "thin",
    "medium",
    "regular",
    "italic",
    "narrow",
    "condensed",
    "wide",
];

/// A font name without its trailing style words, lower case: the family and size.
fn family_of(name: &str) -> String {
    let mut words: Vec<String> = name
        .split_whitespace()
        .map(|w| w.to_ascii_lowercase())
        .collect();
    while words.len() > 1
        && words
            .last()
            .map(|w| STYLE_WORDS.contains(&w.as_str()))
            .unwrap_or(false)
    {
        words.pop();
    }
    words.join(" ")
}

/// The cell size a font's name carries ("Krueger 16x9", "churafont++ Numeric 26x11 Bold"),
/// used to keep a substitute the same size as the font a script asked for.
fn size_of(name: &str) -> Option<(u32, u32)> {
    name.split_whitespace().find_map(|w| {
        let w = w.to_ascii_lowercase();
        let (a, b) = w.split_once('x')?;
        Some((a.parse().ok()?, b.parse().ok()?))
    })
}

impl FontLibrary {
    pub fn new(root: &Path) -> FontLibrary {
        FontLibrary {
            atlases: HashMap::new(),
            root: root.to_path_buf(),
            index: None,
        }
    }

    fn index(&mut self) -> &[Font] {
        if self.index.is_none() {
            // the Fonts folder of every content root (installed mods and archives first, then
            // the installation): a mod's display fonts live in the content folder, and looking
            // only in the installation's `Fonts` left the O530's number plate, matrix and
            // dashboard displays empty. A file of the same name higher up replaces the stock one.
            let mut files = omsi_cfg::read_dir_merged("Fonts");
            if files.is_empty() {
                files = omsi_cfg::vfs::read_dir_paths(&self.root.join("Fonts"));
            }
            // each folder in root order, its files in name order (a listing comes in the file
            // system's order)
            let mut folders: Vec<std::path::PathBuf> = Vec::new();
            for p in &files {
                let d = p.parent().map(|d| d.to_path_buf()).unwrap_or_default();
                if !folders.contains(&d) {
                    folders.push(d);
                }
            }
            files.sort_by_cached_key(|p| {
                (
                    folders
                        .iter()
                        .position(|d| Some(d.as_path()) == p.parent())
                        .unwrap_or(usize::MAX),
                    p.file_name()
                        .map(|n| n.to_string_lossy().to_ascii_lowercase())
                        .unwrap_or_default(),
                )
            });
            // Two files of one folder naming the same font: the one read later takes its place,
            // as in the original (the LiAZ's `ANX_S.oft` spells its "ц" as "=", and a
            // "ANX_S - копия.oft" lying beside it, read first, left the letter out). A
            // folder of higher priority keeps its fonts over the folders after it.
            let mut all: Vec<Font> = Vec::new();
            let mut folder_of: Vec<usize> = Vec::new();
            for p in files {
                if !p
                    .extension()
                    .map(|x| x.eq_ignore_ascii_case("oft"))
                    .unwrap_or(false)
                {
                    continue;
                }
                let folder = folders
                    .iter()
                    .position(|d| Some(d.as_path()) == p.parent())
                    .unwrap_or(usize::MAX);
                // `Font::path` is the .oft (read through the VFS): its bitmaps lie next to it
                if let Ok(list) = Font::load_all(&p) {
                    for f in list {
                        let key = f.name.trim().to_ascii_lowercase();
                        match all
                            .iter()
                            .position(|o| o.name.trim().to_ascii_lowercase() == key)
                        {
                            Some(k) if folder_of[k] == folder => all[k] = f,
                            Some(_) => {}
                            None => {
                                all.push(f);
                                folder_of.push(folder);
                            }
                        }
                    }
                }
            }
            self.index = Some(all);
        }
        self.index.as_deref().unwrap_or(&[])
    }

    /// Whether a font of exactly this name is installed.
    pub fn has_exact(&mut self, name: &str) -> bool {
        let wanted = name.trim();
        self.index()
            .iter()
            .any(|f| f.name.trim().eq_ignore_ascii_case(wanted))
    }

    /// The font called `name`, or - when there is none - one of the same family and size in
    /// another weight: mods ask for weights their packs never shipped (the Citaro pack's
    /// Krüger matrix wants "churafont++ Numeric 26x11 Bold" and "churafont++ 32x8 Bold";
    /// the fonts are "churafont++ Numeric 26x11" and "churafont++ 32x8"), and without it
    /// the line number stayed off the destination display. A name of no known family (the
    /// depot strings the matrix tries as custom fonts) is still not found.
    fn find(&mut self, name: &str) -> Option<Font> {
        let wanted = name.trim();
        if wanted.is_empty() {
            return None;
        }
        let index = self.index();
        if let Some(f) = index
            .iter()
            .find(|f| f.name.trim().eq_ignore_ascii_case(wanted))
        {
            return Some(f.clone());
        }
        let family = family_of(wanted);
        // the plain weight first, then the others in file order - but only a font of the same
        // size. A dot-matrix display (the Krüger matrix asks for eight sizes by name, from
        // "Krueger 7x4" to "Krueger 16x9") draws its letters cell by cell: substituting
        // another size there does not make the text wider or narrower, it makes it a soup of
        // letter fragments. Without a font of that size the script must hear "no font" (-1)
        // and pick the next size itself, which is what the original does.
        let sibling = index
            .iter()
            .filter(|f| size_of(&f.name) == size_of(wanted))
            .find(|f| f.name.trim().eq_ignore_ascii_case(&family))
            .or_else(|| {
                index
                    .iter()
                    .filter(|f| size_of(&f.name) == size_of(wanted))
                    .find(|f| family_of(&f.name) == family)
            });
        let Some(sibling) = sibling else {
            // a display whose font is missing is worth a line in the log: it is the first
            // thing to look at when letters come out wrong on a matrix or a plate
            log::warn!("font \"{wanted}\" is in no Fonts folder of any content root");
            return None;
        };
        log::warn!(
            "font \"{wanted}\" not found; drawing with \"{}\" (same family and size)",
            sibling.name.trim()
        );
        Some(sibling.clone())
    }

    /// `get` with the built-in image decoder.
    pub fn load(&mut self, name: &str) -> Option<Arc<FontAtlas>> {
        self.get(name, &|p| {
            omsi_texture::decode_file(p)
                .ok()
                .map(|i| (i.width, i.height, i.rgba))
        })
    }

    /// Fonts are looked up by their `[newfont]` name across all `Fonts/*.oft` files of every
    /// content root (installed mods first, then the installation): a mod's display fonts
    /// live in the content folder, and looking only in the installation's `Fonts` left the
    /// O530's number plate, matrix and dashboard displays empty.
    pub fn get(
        &mut self,
        name: &str,
        decode: &dyn Fn(&Path) -> Option<(u32, u32, Vec<u8>)>,
    ) -> Option<Arc<FontAtlas>> {
        let key = name.to_ascii_lowercase();
        if let Some(a) = self.atlases.get(&key) {
            return a.clone();
        }
        let found = self.find(name);
        let atlas = found.and_then(|f| {
            // the bitmaps sit beside the .oft that names them
            let fonts_dir = f
                .path
                .parent()
                .map(Path::to_path_buf)
                .unwrap_or_else(|| self.root.join("Fonts"));
            let alpha_path = omsi_cfg::resolve_path(&fonts_dir, &f.alpha);
            let color_path = omsi_cfg::resolve_path(&fonts_dir, &f.bitmap);
            let (aw, ah, alpha) = decode(&alpha_path)?;
            let color = if color_path == alpha_path {
                alpha.clone()
            } else {
                decode(&color_path)
                    .map(|(_, _, c)| c)
                    .unwrap_or_else(|| alpha.clone())
            };
            let color = if color.len() == alpha.len() {
                color
            } else {
                alpha.clone()
            };
            Some(Arc::new(FontAtlas::new(f, aw, ah, color, alpha)))
        });
        if atlas.is_none() && !name.trim().is_empty() {
            log::warn!("font \"{name}\" not found");
        }
        self.atlases.insert(key, atlas.clone());
        atlas
    }
}

/// Runtime state of one `[texttexture]`.
pub struct TextTextureState {
    pub def: TextTexture,
    pub atlas: Option<Arc<FontAtlas>>,
    pub last_text: Option<String>,
    /// Latest rendered RGBA image, present when it changed since the last upload.
    pub pending: Option<Vec<u8>>,
}

impl TextTextureState {
    pub fn new(def: TextTexture, atlas: Option<Arc<FontAtlas>>) -> Self {
        Self {
            def,
            atlas,
            last_text: None,
            pending: None,
        }
    }

    /// Re-render when the string variable changed. Returns true when a new image is pending.
    pub fn update(&mut self, text: &str) -> bool {
        if self.last_text.as_deref() == Some(text) {
            return false;
        }
        self.last_text = Some(text.to_string());
        self.pending = Some(self.image(text));
        true
    }

    /// The picture of `text` in this texture's font, size, colour and placement.
    pub fn image(&self, text: &str) -> Vec<u8> {
        let (w, h) = (self.def.width.max(1) as u32, self.def.height.max(1) as u32);
        let rgb = [
            self.def.color[0] as u8,
            self.def.color[1] as u8,
            self.def.color[2] as u8,
        ];
        let align = omsi_content::font::TextAlign {
            orientation: self.def.orientation,
            grid: self.def.grid,
        };
        match &self.atlas {
            Some(a) => a.render_aligned(text, w, h, self.def.full_color, rgb, align),
            None => vec![0u8; (w * h * 4) as usize],
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn font_families() {
        assert_eq!(
            family_of("churafont++ Numeric 26x11 Bold"),
            "churafont++ numeric 26x11"
        );
        assert_eq!(family_of("churafont++ 32x10 Heavy"), "churafont++ 32x10");
        assert_eq!(family_of("churafont++ 14x10"), "churafont++ 14x10");
        assert_eq!(family_of("Bold"), "bold");
        assert_eq!(family_of("LEERFELD"), "leerfeld");
    }
}
