//! The game's own interface, drawn over the picture in Roboto with a dark outline (the
//! HUD's `.oft` fonts are OMSI's and stay for the time and speed): the chat of a LAN
//! session, the name of what the cursor points at next to the cursor, and the other
//! players' name tags above their buses.
//!
//! Every text is rendered once into a small texture and kept while it is shown; the
//! overlays are rectangles in physical pixels (`Scene::overlays`).

use ab_glyph::{Font, FontVec, PxScale, ScaleFont, VariableFont};
use omsi_render::{Renderer, Scene, TextureId};

mod run_report;

/// Roboto (Apache 2.0), the interface font.
const ROBOTO: &[u8] = include_bytes!("../../../assets/fonts/Roboto-VariableFont_wdth,wght.ttf");
pub(crate) const PAUSE_NOTICE: &str = "Paused  ·  P to go on";

/// A rendered text: its texture and size in pixels.
#[derive(Clone, Copy)]
struct Label {
    tex: TextureId,
    w: u32,
    h: u32,
    used: u64,
}

/// Texts rendered into textures, kept while they are used.
pub struct TextCache {
    font: FontVec,
    labels: hashbrown::HashMap<(String, u32, [u8; 4]), Label>,
    frame: u64,
    /// How strongly the background plates are drawn this frame (`backdrop`).
    backdrop: f32,
    /// No outline round the texts that ask for none (the game menu's: flat text on its card).
    flat: bool,
}

impl TextCache {
    pub fn new() -> Option<TextCache> {
        let mut font = FontVec::try_from_vec(ROBOTO.to_vec()).ok()?;
        // a little heavier than the regular 400: light text on a dark panel is thin and
        // greyish at menu sizes otherwise
        let _ = font.set_variation(b"wght", 500.0);
        Some(TextCache {
            font,
            labels: hashbrown::HashMap::new(),
            frame: 0,
            backdrop: 1.0,
            flat: false,
        })
    }

    /// The texture of `text` at `px` pixels in `color` (alpha = opacity of the outline), and
    /// its size.
    fn label(
        &mut self,
        r: &Renderer,
        scene: &mut Scene,
        text: &str,
        px: u32,
        color: [u8; 4],
    ) -> Label {
        let color = [
            color[0],
            color[1],
            color[2],
            outline_for(color, if self.flat { 1.0 } else { self.backdrop }),
        ];
        // (in the interface's language: the menu, the notes, the windows)
        let text = &*omsi_ui::tr(text);
        let key = (text.to_string(), px, color);
        if let Some(l) = self.labels.get_mut(&key) {
            l.used = self.frame;
            return *l;
        }
        let img = render_text(&self.font, text, px as f32, color);
        let tex = r.add_texture(scene, &img, false);
        let l = Label {
            tex,
            w: img.width,
            h: img.height,
            used: self.frame,
        };
        self.labels.insert(key, l);
        l
    }

    /// Text width in pixels, without rendering it.
    pub fn width(&self, text: &str, px: f32) -> f32 {
        let text = &*omsi_ui::tr(text);
        self.width_raw(text, px)
    }

    fn width_raw(&self, text: &str, px: f32) -> f32 {
        let mut w = 0.0;
        let mut prev: Option<(ab_glyph::GlyphId, *const FontVec)> = None;
        for c in text.chars() {
            let font = font_for(&self.font, c);
            let f = font.as_scaled(PxScale::from(px));
            let id = f.glyph_id(c);
            if let Some((p, pf)) = prev {
                if std::ptr::eq(pf, font) {
                    w += f.kern(p, id);
                }
            }
            w += f.h_advance(id);
            prev = Some((id, font as *const FontVec));
        }
        w + outline_px(px) * 2.0 + 2.0
    }

    /// End of a frame: labels not used for a few seconds are released.
    pub fn end_frame(&mut self, r: &Renderer, scene: &mut Scene) {
        self.frame += 1;
        if self.frame % 120 == 0 {
            let old: Vec<_> = self
                .labels
                .iter()
                .filter(|(_, l)| self.frame.saturating_sub(l.used) > 240)
                .map(|(k, _)| k.clone())
                .collect();
            for k in old {
                if let Some(l) = self.labels.remove(&k) {
                    r.free_texture(scene, l.tex);
                }
            }
        }
    }
}

/// How opaque a label's dark outline is (the alpha of its `color`): as asked, or - for a
/// panel's text, asked without one - more as the opacity setting thins the panels
/// (`backdrop` below 1): on a see-through panel over a bright sky the dim lines could not be
/// read. Light texts only: round a dark one (the highlighted menu line's, on the solid
/// accent) the outline is as dark as the glyphs and smeared them, like a shadow.
fn outline_for(color: [u8; 4], backdrop: f32) -> u8 {
    let light = 0.299 * color[0] as f32 + 0.587 * color[1] as f32 + 0.114 * color[2] as f32 > 100.0;
    match color[3] {
        0 if backdrop < 1.0 && light => (((1.0 - backdrop) * 1.6).min(0.9) * 255.0) as u8,
        a => a,
    }
}

/// The outline (UIStroke) around the glyphs, in pixels.
fn outline_px(px: f32) -> f32 {
    (px / 9.0).clamp(1.0, 3.0)
}

/// The font that draws `c`: Roboto, else the system's font for the script (Chinese,
/// Japanese, Korean, Thai, Hindi - the menu was a column of boxes in those languages).
fn font_for(roboto: &FontVec, c: char) -> &FontVec {
    if omsi_ui::text::needs_fallback(roboto, c) {
        if let Some(f) = omsi_ui::text::fallback_font(c) {
            return f;
        }
    }
    roboto
}

/// `text` as straight-alpha RGBA: the glyphs in `color` over a dark outline.
fn render_text(font: &FontVec, text: &str, px: f32, color: [u8; 4]) -> omsi_texture::Image {
    let f = font.as_scaled(PxScale::from(px));
    let stroke = outline_px(px);
    let pad = stroke.ceil() as i32 + 1;
    let asc = f.ascent();
    let h = (asc - f.descent()).ceil() as i32 + pad * 2;
    // the baseline and every glyph on whole pixels: glyphs drawn between pixels came out
    // different from letter to letter and soft (the pen itself still runs on fractions, so
    // the text keeps its width)
    let base = (pad as f32 + asc).round();
    // lay the glyphs out
    let mut glyphs: Vec<(&FontVec, ab_glyph::Glyph)> = Vec::new();
    let mut x = pad as f32;
    let mut prev: Option<(ab_glyph::GlyphId, *const FontVec)> = None;
    for c in text.chars() {
        let gf = font_for(font, c);
        let sf = gf.as_scaled(PxScale::from(px));
        let id = sf.glyph_id(c);
        if let Some((p, pf)) = prev {
            if std::ptr::eq(pf, gf) {
                x += sf.kern(p, id);
            }
        }
        glyphs.push((
            gf,
            id.with_scale_and_position(PxScale::from(px), ab_glyph::point(x.round(), base)),
        ));
        x += sf.h_advance(id);
        prev = Some((id, gf as *const FontVec));
    }
    let w = (x.ceil() as i32 + pad).max(1);
    let (wu, hu) = (w as usize, h.max(1) as usize);
    let mut cov = vec![0f32; wu * hu];
    for (gf, g) in glyphs {
        if let Some(o) = gf.outline_glyph(g) {
            let b = o.px_bounds();
            o.draw(|gx, gy, c| {
                let xx = b.min.x as i32 + gx as i32;
                let yy = b.min.y as i32 + gy as i32;
                if xx >= 0 && yy >= 0 && (xx as usize) < wu && (yy as usize) < hu {
                    let i = yy as usize * wu + xx as usize;
                    cov[i] = (cov[i] + c).min(1.0);
                }
            });
        }
    }
    // the texture is sRGB and blended in linear light: light text on a dark panel comes out
    // too fat and blurry, dark text on a light one too thin - bend the coverage to match
    let light = 0.299 * color[0] as f32 + 0.587 * color[1] as f32 + 0.114 * color[2] as f32 > 100.0;
    let gamma = if light { 1.45 } else { 0.8 };
    for c in cov.iter_mut() {
        if *c > 0.0 && *c < 1.0 {
            *c = c.powf(gamma);
        }
    }
    // the outline: the coverage grown by the stroke radius
    let r = stroke;
    let ri = r.ceil() as i32;
    let mut edge = vec![0f32; wu * hu];
    for y in 0..hu as i32 {
        for x in 0..wu as i32 {
            let mut m = 0f32;
            for dy in -ri..=ri {
                for dx in -ri..=ri {
                    let d = ((dx * dx + dy * dy) as f32).sqrt();
                    if d > r + 0.5 {
                        continue;
                    }
                    let (sx, sy) = (x + dx, y + dy);
                    if sx < 0 || sy < 0 || sx >= wu as i32 || sy >= hu as i32 {
                        continue;
                    }
                    let k = (r + 0.5 - d).clamp(0.0, 1.0);
                    m = m.max(cov[sy as usize * wu + sx as usize] * k);
                }
            }
            edge[y as usize * wu + x as usize] = m;
        }
    }
    let oa = color[3] as f32 / 255.0;
    let mut rgba = vec![0u8; wu * hu * 4];
    for i in 0..wu * hu {
        let a_text = cov[i];
        let a_edge = edge[i] * oa;
        let a = a_text + a_edge * (1.0 - a_text);
        if a <= 0.0 {
            continue;
        }
        for c in 0..3 {
            let v = (color[c] as f32 * a_text + 12.0 * a_edge * (1.0 - a_text)) / a;
            rgba[i * 4 + c] = v.round().clamp(0.0, 255.0) as u8;
        }
        rgba[i * 4 + 3] = (a * 255.0).round() as u8;
    }
    omsi_texture::Image {
        width: wu as u32,
        height: hu as u32,
        rgba,
        has_alpha: true,
    }
}

// ---------------------------------------------------------------------------------------
// the chat

/// How many lines the chat shows while it is closed, and how many it keeps to scroll back.
const CHAT_SHOWN: usize = 8;
pub const CHAT_KEEP: usize = 200;

/// What the chat widget needs from the session each frame.
pub struct ChatView<'a> {
    /// Every line, oldest first ("Name: text" or "* notice").
    pub lines: &'a [String],
    /// The line being typed (the input box is open).
    pub typing: Option<&'a str>,
    /// Why the last line was not sent.
    pub error: Option<&'a str>,
}

/// The chat's own state: shown or hidden (V), the scroll position and whether the cursor is
/// over it.
#[derive(Default)]
pub struct ChatWidget {
    pub hidden: bool,
    /// Lines scrolled back from the newest.
    pub scroll: usize,
    /// The chat's box on the screen (physical pixels) as drawn last: hovering over it shows
    /// the input box, a click there opens it.
    pub rect: [f32; 4],
    pub hovered: bool,
    pub caret_t: f32,
}

impl ChatWidget {
    pub fn contains(&self, x: f32, y: f32) -> bool {
        x >= self.rect[0] && x <= self.rect[2] && y >= self.rect[1] && y <= self.rect[3]
    }

    /// The mouse wheel over the chat (or while typing) scrolls the history.
    pub fn wheel(&mut self, lines: usize, amount: f32) {
        let max = lines.saturating_sub(CHAT_SHOWN);
        let s = self.scroll as i32 + amount.round() as i32;
        self.scroll = s.clamp(0, max as i32) as usize;
    }
}

/// How much larger than designed the interface is drawn, on top of the screen's scale
/// `dpi`: on a window taller than 1080 logical pixels as much as it is taller (up to twice),
/// times the player's `size` (`Settings::ui_scale`). Laid out for 1080p, the texts were half
/// their size on a 4K screen at 100 % display scaling; a window of 1080 lines or fewer is
/// drawn as it always was. With `window` off (`Settings::ui_scale_window`) it does not grow
/// with the window at all.
pub fn size_factor(height_px: f32, dpi: f32, size: f32, window: bool) -> f32 {
    let grown = if window {
        (height_px / dpi.max(0.5) / 1080.0).clamp(1.0, 2.0)
    } else {
        1.0
    };
    grown * size
}

/// How strongly the interface's backgrounds are drawn for the opacity setting
/// (`Settings::ui_opacity`): 1 at its default of 85 %, as designed; lower, the picture shows
/// through them (never less than 0.3), higher a little darker. The texts stay solid.
pub fn backdrop(opacity: f32) -> f32 {
    (opacity / 0.85).clamp(0.3, 1.3)
}

/// Which menu is open: its layout follows from it.
#[derive(Clone, Copy, PartialEq, Eq, Default, Debug)]
pub enum MenuKind {
    /// The game menu itself.
    #[default]
    Game,
    /// A settings window (options, vehicle, world): a sidebar of pages and rows with switches,
    /// sliders and buttons. A row's label is `name\u{1f}kind\u{1f}value\u{1f}description\u{1f}fraction`
    /// (kind: `s` switch, `v` slider, `c` stepper, `o` opens a list, `a` button, `i` information).
    Options,
    /// The lines of the map's timetable.
    Lines,
    /// One line's tours.
    Tours,
    /// Any other list (drivers, fleet numbers, destinations, liveries ...).
    List,
}

/// The timetable beside a list of lines or tours: a title, a line of facts and rows of
/// (what, time).
pub struct Preview {
    pub title: String,
    pub meta: String,
    pub rows: Vec<(String, String)>,
    /// The row chosen, when the rows can be chosen (the stops to start a tour from).
    pub chosen: Option<usize>,
    /// The button under the rows, when there is one.
    pub button: Option<String>,
    /// The time of the timetable the rows are of, with buttons to put it ahead (the trip of
    /// a tour to take on later), when it can be set.
    pub time: Option<String>,
}

/// A drop-down open over a row of a settings window: the entries, the one the keyboard is
/// on, the first one shown and the one in force now.
pub struct DropdownView<'a> {
    pub row: usize,
    pub items: Vec<&'a str>,
    pub sel: usize,
    pub top: usize,
    pub current: Option<usize>,
}

/// Everything the interface draws in a frame.
pub struct Frame<'a> {
    /// Physical pixels per logical one.
    pub scale: f32,
    /// How much larger than designed (`size_factor`): everything below is drawn this much
    /// larger on top of `scale`.
    pub ui_scale: f32,
    /// How strongly the backgrounds are drawn (`backdrop`): the menu's and the timetable's
    /// panels, the chat's box, the plates under the notes, the tooltip and the frame rate.
    pub opacity: f32,
    pub width: f32,
    pub height: f32,
    pub cursor: (f32, f32),
    /// An OpenXR headset is drawing this frame.
    pub vr: bool,
    /// The name of what the cursor points at (a switch, a part), shown next to it.
    pub tooltip: Option<String>,
    /// The chat, when a LAN session runs and the chat is not switched off.
    pub chat: Option<ChatView<'a>>,
    /// What the driver has to act on (why the bus does not move, a passenger's wish, the
    /// change due, a service done), top left.
    pub notes: &'a [String],
    /// The frame rate, top right (the `show_fps` setting).
    pub fps: Option<f32>,
    /// The game stands paused.
    pub paused: bool,
    /// The game menu is open, with this line chosen (labels from `GAME_MENU`).
    pub menu: Option<(usize, &'a [(&'a str, &'a str)])>,
    /// The first line shown when a finger scrolled the menu (`App::menu_top`).
    pub menu_top: Option<f32>,
    /// Ids of the game menu's lines that are greyed out and cannot be chosen (the timetable
    /// without an active route).
    pub menu_disabled: &'a [&'a str],
    /// The timetable window: its title and per stop (name, time, 0 served / 1 next / 2 ahead).
    pub timetable: Option<(String, Vec<(String, String, u8)>)>,
    /// The information bar along the top.
    pub info: Option<String>,
    /// A tutorial page: title, text, picture, page number and count.
    pub tutorial: Option<(&'a str, &'a str, Option<&'a std::path::Path>, usize, usize)>,
    /// Name tags: a screen position (the point above a bus), the name and a second line.
    pub tags: Vec<((f32, f32), String, String, f32)>,
    /// What kind of menu the lines belong to.
    pub menu_kind: MenuKind,
    pub report: Option<&'a crate::run_statistics::Report>,
    pub report_status: &'a str,
    /// The open list's title and the small line above it (the line a tour list is of).
    pub menu_head: Option<(String, String)>,
    /// The timetable of the chosen line or tour, beside the list.
    pub menu_preview: Option<Preview>,
    /// The first stop shown of the timetable beside the tours, when the wheel has scrolled it
    /// (`None`: the stop chosen is kept in view).
    pub pane_first: Option<usize>,
    /// The pages of an open settings window (their titles) and the one shown.
    pub menu_tabs: Option<(Vec<String>, usize)>,
    /// The keyboard chose the menu's line last: that line is shown lit (else only the one
    /// under the mouse is).
    pub menu_kbd: bool,
    /// The drop-down open over a row of the settings window.
    pub dropdown: Option<DropdownView<'a>>,
}

pub struct Ui {
    pub text: TextCache,
    pub chat: ChatWidget,
    /// Where the game menu's lines were drawn this frame (physical pixels), for the mouse.
    pub menu_rects: Vec<[f32; 4]>,
    /// Per line of `menu_rects`, where the arrows round its value are (a list's setting,
    /// `game_lists::ADJUST`): `[from, to, plus]` - a click from `from` to `to` steps it
    /// down, one right of `plus` up.
    pub menu_arrows: Vec<Option<[f32; 3]>>,
    pub menu_scroll_thumb: Option<[f32; 4]>,
    pub menu_scroll_track: Option<[f32; 4]>,
    /// Where the controls of the settings rows were drawn (a slider's track, a stepper), one
    /// entry per line in `menu_rects`: a click there sets the value.
    pub menu_ctl: Vec<Option<[f32; 4]>>,
    /// The sidebar of a settings window: one box per page, the way back last.
    pub menu_side: Vec<[f32; 4]>,
    /// The rows of the timetable beside a line's tours (the stops to start from), the stop the
    /// first of them is, and the button that starts the trip.
    pub menu_pane: Vec<[f32; 4]>,
    pub menu_pane_start: usize,
    pub menu_pane_go: Option<[f32; 4]>,
    /// The whole timetable pane beside the tours: the wheel over it scrolls its stops.
    pub menu_pane_box: Option<[f32; 4]>,
    /// The two arrows beside the time of a tour: the trip before, the next one.
    pub menu_time: Vec<[f32; 4]>,
    /// The colours and positions of the menu's parts that ease to their new state (a line's
    /// light, a switch's knob ...), by what they belong to.
    anim: std::collections::HashMap<u64, f32>,
    /// Seconds since the last frame, for `ease`.
    anim_dt: f32,
    /// Overlay entries belonging to the game menu.
    pub menu_overlay_range: std::ops::Range<usize>,
    /// The pointer texture, positioned separately for each headset eye.
    pub vr_cursor_overlay: Option<usize>,
    pub vr_tooltip_overlay: Option<usize>,
    /// The first line of the menu shown (a long menu scrolls: `menu_rects[k]` is line
    /// `menu_start + k`).
    pub menu_start: usize,
    /// How many lines the menu shows at once, and how high one is (physical pixels): a
    /// finger's drag is turned into lines with it.
    pub menu_rows: usize,
    pub menu_row_h: f32,
    pub menu_search: Option<[f32; 4]>,
    pub caret_up: bool,
    /// The entries of the drop-down shown (their rects), the first of them, and how many fit.
    pub dd_rects: Vec<[f32; 4]>,
    pub dd_top: usize,
    pub dd_rows: usize,
    /// Pictures shown in the interface (a tutorial page's), by file.
    images: hashbrown::HashMap<std::path::PathBuf, Option<(TextureId, u32, u32)>>,
    /// The loading screen's background (the map's picture), looked for once per load.
    pub loading_bg: Option<Option<(TextureId, u32, u32)>>,
    /// The loading screen's full-screen picture (`assets/backgrounds/loading-screen.jpg`).
    loading_art: Option<Option<(TextureId, u32, u32)>>,
    /// The wordmark bottom left (`assets/logos/neoOMSI-wordmark.png`, cut to its content).
    loading_logo: Option<Option<(TextureId, u32, u32)>>,
    /// The wordmark cut to its content, full size, and its renderings at exact pixel heights.
    logo_src: Option<image::RgbaImage>,
    logo_cache: Vec<(u32, TextureId, u32)>,
    /// The loading screen's spinner: one texture per turn of 1/24.
    spinner: Vec<TextureId>,
}

impl Ui {
    /// The scene the interface draws into was replaced: every texture it made (labels,
    /// plates, images) belonged to the old one and is made again on demand.
    pub fn scene_replaced(&mut self) {
        self.text.labels.clear();
        self.images.clear();
        self.loading_bg = None;
        self.loading_art = None;
        self.loading_logo = None;
        self.logo_src = None;
        self.logo_cache.clear();
        self.spinner.clear();
    }

    pub fn new() -> Option<Ui> {
        Some(Ui {
            text: TextCache::new()?,
            chat: ChatWidget::default(),
            menu_rects: Vec::new(),
            menu_arrows: Vec::new(),
            menu_scroll_thumb: None,
            menu_scroll_track: None,
            menu_ctl: Vec::new(),
            dd_rects: Vec::new(),
            dd_top: 0,
            dd_rows: 8,
            menu_side: Vec::new(),
            menu_pane: Vec::new(),
            menu_pane_start: 0,
            menu_pane_go: None,
            menu_pane_box: None,
            menu_time: Vec::new(),
            anim: Default::default(),
            anim_dt: 0.0,
            menu_overlay_range: 0..0,
            vr_cursor_overlay: None,
            vr_tooltip_overlay: None,
            menu_start: 0,
            menu_rows: 0,
            menu_row_h: 1.0,
            menu_search: None,
            caret_up: false,
            images: Default::default(),
            loading_bg: None,
            loading_art: None,
            loading_logo: None,
            logo_src: None,
            logo_cache: Vec::new(),
            spinner: Vec::new(),
        })
    }

    /// Draw the frame's interface: its overlays go after the HUD's in `scene.overlays`.
    pub fn draw(&mut self, r: &Renderer, scene: &mut Scene, f: &Frame, dt: f32) {
        let s = f.scale.max(0.5) * f.ui_scale;
        self.text.backdrop = f.opacity;
        // --- name tags above the other players' buses
        for ((x, y), name, sub, alpha) in &f.tags {
            let a = (alpha.clamp(0.0, 1.0) * 255.0) as u8;
            let l = self
                .text
                .label(r, scene, name, (19.0 * s) as u32, [255, 255, 255, 220]);
            let x0 = x - l.w as f32 * 0.5;
            let y0 = y - l.h as f32;
            if a > 0 {
                scene
                    .overlays
                    .push((l.tex, [x0, y0, x0 + l.w as f32, y0 + l.h as f32]));
                if !sub.is_empty() {
                    let m = self
                        .text
                        .label(r, scene, sub, (12.0 * s) as u32, [210, 225, 255, 200]);
                    let mx = x - m.w as f32 * 0.5;
                    scene.overlays.push((
                        m.tex,
                        [
                            mx,
                            y0 + l.h as f32 - 3.0 * s,
                            mx + m.w as f32,
                            y0 + l.h as f32 - 3.0 * s + m.h as f32,
                        ],
                    ));
                }
            }
        }
        // watermark with git version / build id
        {
            let text = format!(
                "neoOMSI {} #{}",
                env!("CARGO_PKG_VERSION"),
                crate::startup::BUILD
                    .split_whitespace()
                    .next()
                    .unwrap_or("unknown")
            );

            let was_flat = self.text.flat;
            self.text.flat = true;
            let l = self
                .text
                .label(r, scene, &text, (14.0 * s) as u32, [255, 255, 255, 255]);
            self.text.flat = was_flat;
            let x1 = f.width - 10.0 * s;
            let y1 = f.height - 8.0 * s;
            scene
                .overlays
                .push((l.tex, [x1 - l.w as f32, y1 - l.h as f32, x1, y1]));
        }
        // the top of the windows in the corners: the notes on the left start on the line of
        // the timetable and a tutorial page on the right (14 px down, they stood higher)
        let corner_top = 60.0 * s;
        // --- notes, top left: white on a dark outline, one line each, on a dim plate (the
        // outline alone left a note hard to read over a bright sky)
        let notes_bottom = {
            let px = (16.0 * s) as u32;
            let x0 = 16.0 * s;
            // (below the on-screen buttons of a phone, which keep their size when the
            // interface is made smaller)
            let mut y = if crate::platform::touch_controls() {
                (80.0 * f.scale.max(0.5) * f.ui_scale.max(1.0)).max(corner_top)
            } else {
                corner_top
            };
            for n in f.notes.iter().filter(|n| !n.trim().is_empty()).take(8) {
                let text = omsi_ui::tr(n);
                let text = clip_to(&self.text, &text, px as f32, f.width * 0.6);
                let l = self.text.label(r, scene, &text, px, [255, 255, 255, 235]);
                let plate = self.text.plate(r, scene, 7);
                scene.overlays.push((
                    plate,
                    [x0 - 5.0 * s, y, x0 + l.w as f32 + 5.0 * s, y + l.h as f32],
                ));
                scene
                    .overlays
                    .push((l.tex, [x0, y, x0 + l.w as f32, y + l.h as f32]));
                y += l.h as f32 + 2.0 * s;
            }
            y
        };
        // --- the chat, top left under the notes, as Roblox has it (from the fourth note
        // on they ran into it)
        if let Some(c) = f.chat.as_ref().filter(|_| !self.chat.hidden) {
            let px = (17.0 * s) as u32;
            let lh = px as f32 * 1.35;
            let x0 = 14.0 * s;
            let y0 = (96.0 * s).max(notes_bottom + 10.0 * s);
            let width = (460.0 * s).min(f.width * 0.5);
            let open = c.typing.is_some();
            let n = c.lines.len();
            let shown = CHAT_SHOWN.min(n);
            let end = n.saturating_sub(if open || self.chat.hovered {
                self.chat.scroll
            } else {
                0
            });
            let start = end.saturating_sub(shown);
            let box_h = lh * CHAT_SHOWN as f32 + lh * 1.6;
            self.chat.rect = [x0 - 6.0 * s, y0 - 6.0 * s, x0 + width, y0 + box_h];
            self.chat.hovered = self.chat.contains(f.cursor.0, f.cursor.1);
            let show_box = open || self.chat.hovered;
            // the lines, newest at the bottom of the history area
            let mut y = y0 + lh * (CHAT_SHOWN - (end - start)) as f32;
            for line in &c.lines[start..end] {
                let color = if line.starts_with("* ") {
                    [255, 226, 140, 230]
                } else {
                    [255, 255, 255, 230]
                };
                let text = clip_to(&self.text, line, px as f32, width);
                let l = self.text.label(r, scene, &text, px, color);
                scene
                    .overlays
                    .push((l.tex, [x0, y, x0 + l.w as f32, y + l.h as f32]));
                y += lh;
            }
            if show_box {
                // the input box: shown when the cursor is over the chat or it is typed into
                let by = y0 + lh * CHAT_SHOWN as f32 + lh * 0.2;
                let bh = lh * 1.25;
                let plate = self.text.plate(r, scene, 0);
                scene
                    .overlays
                    .push((plate, [x0 - 4.0 * s, by, x0 + width, by + bh]));
                self.chat.caret_t += dt;
                let caret = if open && (self.chat.caret_t % 1.0) < 0.55 {
                    "|"
                } else {
                    ""
                };
                let (text, color) = match c.typing {
                    Some(t) => (format!("{t}{caret}"), [255, 255, 255, 240]),
                    None => (
                        "Click here or press / to chat".to_string(),
                        [190, 190, 190, 200],
                    ),
                };
                let text = clip_left(&self.text, &text, px as f32, width - 10.0 * s);
                let l = self.text.label(r, scene, &text, px, color);
                let ty = by + (bh - l.h as f32) * 0.5;
                scene.overlays.push((
                    l.tex,
                    [x0 + 2.0 * s, ty, x0 + 2.0 * s + l.w as f32, ty + l.h as f32],
                ));
                if self.chat.scroll > 0 && (open || self.chat.hovered) {
                    // (the words translated apart from the number: whole, the text was no key)
                    let m = self.text.label(
                        r,
                        scene,
                        &format!("{} {}", self.chat.scroll, omsi_ui::tr("newer below")),
                        (11.0 * s) as u32,
                        [200, 200, 200, 200],
                    );
                    scene.overlays.push((
                        m.tex,
                        [x0 + width - m.w as f32, by - m.h as f32, x0 + width, by],
                    ));
                }
            }
            if let Some(e) = c.error {
                let l = self.text.label(
                    r,
                    scene,
                    &format!("{}: {}", omsi_ui::tr("Not sent"), omsi_ui::tr(e)),
                    (12.0 * s) as u32,
                    [255, 150, 150, 220],
                );
                let ey = y0 + box_h;
                scene
                    .overlays
                    .push((l.tex, [x0, ey, x0 + l.w as f32, ey + l.h as f32]));
            }
        } else {
            self.chat.hovered = false;
            self.chat.rect = [0.0; 4];
        }
        if let Some(fps) = f.fps {
            let l = self.text.label(
                r,
                scene,
                &format!("{fps:.0} fps"),
                (13.0 * s) as u32,
                [255, 255, 255, 200],
            );
            let x = f.width - l.w as f32 - 12.0 * s;
            // (on a plate, as the notes are: over a bright sky the outline alone was not enough)
            let plate = self.text.plate(r, scene, 7);
            scene.overlays.push((
                plate,
                [
                    x - 5.0 * s,
                    10.0 * s,
                    x + l.w as f32 + 5.0 * s,
                    10.0 * s + l.h as f32,
                ],
            ));
            scene
                .overlays
                .push((l.tex, [x, 10.0 * s, x + l.w as f32, 10.0 * s + l.h as f32]));
        }
        // --- the information bar, along the top in the middle
        if let Some(info) = f.info.as_ref() {
            self.text.flat = true;
            let l = self.text.label(r, scene, info, (15.0 * s) as u32, WHITE);
            let pad = 12.0 * s;
            let (w, h) = (l.w as f32 + pad * 2.0, l.h as f32 + pad * 0.8);
            let x = ((f.width - w) * 0.5).round();
            let y = (8.0 * s).round();
            let radius = ROW_R * s;
            let card = [14, 14, 14, 245];
            self.text.rounded(
                r,
                scene,
                [x - 1.0, y - 1.0, x + w + 1.0, y + h + 1.0],
                radius + 1.0,
                BORDER,
            );
            self.text
                .rounded(r, scene, [x, y, x + w, y + h], radius, card);
            scene.overlays.push((
                l.tex,
                [
                    x + pad,
                    y + pad * 0.4,
                    x + pad + l.w as f32,
                    y + pad * 0.4 + l.h as f32,
                ],
            ));
            self.text.flat = false;
        }
        let tutorial_w = (420.0 * s).min(f.width * 0.42);
        // --- the timetable window, on the right
        if let Some((title, rows)) = f.timetable.as_ref() {
            let px = (14.0 * s) as u32;
            let lh = px as f32 * 1.55;
            let w = (340.0 * s).min(f.width * 0.4);
            let shown = rows.len().min(((f.height * 0.7) / lh) as usize).max(1);
            // (from the stop before the next one on)
            let next = rows.iter().position(|r| r.2 == 1).unwrap_or(0);
            let first = next.saturating_sub(1).min(rows.len().saturating_sub(shown));
            let h = lh * (shown as f32 + 1.6);
            // (left of a tutorial page, which has the same corner)
            let beside = if f.tutorial.is_some() {
                tutorial_w + 12.0 * s
            } else {
                0.0
            };
            let x = (f.width - w - 16.0 * s - beside).max(16.0 * s);
            let y = corner_top;
            self.text.flat = true;
            let radius = CARD_R * s;
            let card = [14, 14, 14, 245];
            self.text.rounded(
                r,
                scene,
                [x - 1.0, y - 1.0, x + w + 1.0, y + h + 1.0],
                radius + 1.0,
                BORDER,
            );
            self.text
                .rounded(r, scene, [x, y, x + w, y + h], radius, card);
            let t = self.text.label(
                r,
                scene,
                &clip_to(&self.text, title, px as f32 * 1.1, w - 20.0 * s),
                (px as f32 * 1.1) as u32,
                WHITE,
            );
            scene.overlays.push((
                t.tex,
                [
                    x + 10.0 * s,
                    y + 6.0 * s,
                    x + 10.0 * s + t.w as f32,
                    y + 6.0 * s + t.h as f32,
                ],
            ));
            let hair = (y + lh * 1.25).round();
            self.text.rounded(
                r,
                scene,
                [x, hair, x + w, hair + 1.0_f32.max(s).round()],
                0.0,
                BORDER,
            );
            // (the names start after the widest time: "12:03-05" of a stop with a wait)
            let time_w = rows
                .iter()
                .map(|r| self.text.width(&r.1, px as f32))
                .fold(0.0f32, f32::max)
                .max(40.0 * s);
            let name_x = x + 10.0 * s + time_w + 12.0 * s;
            for (k, (name, time, state)) in rows.iter().skip(first).take(shown).enumerate() {
                let ry = y + lh * (k as f32 + 1.3);
                if *state == 1 {
                    self.text.rounded(
                        r,
                        scene,
                        [
                            x + 4.0 * s,
                            ry - 2.0 * s,
                            x + w - 4.0 * s,
                            ry + lh - 4.0 * s,
                        ],
                        ROW_R * s,
                        ACCENT_SOFT,
                    );
                }
                let color = match state {
                    0 => MUTED,
                    1 => [232, 160, 48, 0],
                    _ => WHITE,
                };
                let tl = self.text.label(r, scene, time, px, color);
                scene.overlays.push((
                    tl.tex,
                    [
                        x + 10.0 * s,
                        ry,
                        x + 10.0 * s + tl.w as f32,
                        ry + tl.h as f32,
                    ],
                ));
                let nl = self.text.label(
                    r,
                    scene,
                    &clip_to(&self.text, name, px as f32, x + w - name_x - 10.0 * s),
                    px,
                    color,
                );
                scene
                    .overlays
                    .push((nl.tex, [name_x, ry, name_x + nl.w as f32, ry + nl.h as f32]));
            }
            self.text.flat = false;
        }
        // --- a tutorial page, on the right
        if let Some((title, text, image, at, count)) = f.tutorial {
            let w = tutorial_w;
            let x = f.width - w - 16.0 * s;
            let mut y = corner_top;
            let top = y;
            let pad = 14.0 * s;
            let mut items: Vec<(TextureId, [f32; 4])> = Vec::new();
            if let Some(p) = image {
                let entry = self.images.entry(p.to_path_buf()).or_insert_with(|| {
                    omsi_texture::decode_file(p).ok().map(|img| {
                        let (iw, ih) = (img.width, img.height);
                        (r.add_texture(scene, &img, false), iw, ih)
                    })
                });
                if let Some((tex, iw, ih)) = *entry {
                    let dw = w - pad * 2.0;
                    let dh = dw * ih as f32 / iw.max(1) as f32;
                    items.push((tex, [x + pad, y + pad, x + pad + dw, y + pad + dh]));
                    y += dh + pad;
                }
            }
            y += pad * 0.6;
            let tp = (18.0 * s) as u32;
            if !title.is_empty() {
                for line in wrap(&self.text, title, tp as f32, w - pad * 2.0) {
                    let l = self.text.label(r, scene, &line, tp, [255, 200, 110, 0]);
                    items.push((l.tex, [x + pad, y, x + pad + l.w as f32, y + l.h as f32]));
                    y += l.h as f32;
                }
                y += 6.0 * s;
            }
            let bp = (14.0 * s) as u32;
            let max_y = f.height - 60.0 * s;
            'text: for para in text.lines() {
                for line in wrap(&self.text, para, bp as f32, w - pad * 2.0) {
                    if y > max_y {
                        break 'text;
                    }
                    let l = self.text.label(r, scene, &line, bp, [235, 235, 235, 0]);
                    items.push((l.tex, [x + pad, y, x + pad + l.w as f32, y + l.h as f32]));
                    y += l.h as f32 * 0.95;
                }
                y += 5.0 * s;
            }
            let tr = |t: &str| omsi_ui::tr(t).into_owned();
            let foot = format!(
                "{} {}/{}   ·   {}   ·   {}   ·   {}",
                tr("Page"),
                at + 1,
                count,
                tr("Enter next"),
                tr("Page Up back"),
                tr("Ctrl+T hide")
            );
            let l = self
                .text
                .label(r, scene, &foot, (12.0 * s) as u32, [150, 150, 150, 0]);
            y += 4.0 * s;
            items.push((l.tex, [x + pad, y, x + pad + l.w as f32, y + l.h as f32]));
            y += l.h as f32 + pad;
            let plate = self.text.plate(r, scene, 3);
            scene.overlays.push((plate, [x, top, x + w, y]));
            scene.overlays.extend(items);
        }
        // --- paused
        if f.paused && f.menu.is_none() {
            let l = self.text.label(
                r,
                scene,
                "Paused  ·  P to go on",
                (18.0 * s) as u32,
                [255, 255, 255, 0],
            );
            let pad = 14.0 * s;
            let (w, h) = (l.w as f32 + pad * 2.0, l.h as f32 + pad);
            let x = (f.width - w) * 0.5;
            let y = f.height * 0.2;
            let plate = self.text.plate(r, scene, 3);
            scene.overlays.push((plate, [x, y, x + w, y + h]));
            scene.overlays.push((
                l.tex,
                [
                    x + pad,
                    y + pad * 0.5,
                    x + pad + l.w as f32,
                    y + pad * 0.5 + l.h as f32,
                ],
            ));
        }
        // --- the game menu and its lists, in the middle over a dimmed picture
        self.anim_dt = dt.clamp(0.0, 0.1);
        self.text.flat = true;
        self.draw_menu(r, scene, f);
        self.text.flat = false;
        // --- the mouse-over name, right of the cursor
        self.vr_tooltip_overlay = None;
        if let Some(t) = f.tooltip.as_ref().filter(|t| !t.is_empty()) {
            let l = self
                .text
                .label(r, scene, t, (14.0 * s) as u32, [255, 255, 255, 235]);
            let mut x = f.cursor.0 + 16.0 * s;
            let mut y = f.cursor.1 + 2.0 * s;
            if x + l.w as f32 > f.width {
                x = f.cursor.0 - 8.0 * s - l.w as f32;
            }
            if y + l.h as f32 > f.height {
                y = f.height - l.h as f32;
            }
            if f.vr {
                self.vr_tooltip_overlay = Some(scene.overlays.len());
            } else {
                // (in a headset the text alone is placed in front of the eye)
                let plate = self.text.plate(r, scene, 7);
                scene.overlays.push((
                    plate,
                    [x - 5.0 * s, y, x + l.w as f32 + 5.0 * s, y + l.h as f32],
                ));
            }
            scene
                .overlays
                .push((l.tex, [x, y, x + l.w as f32, y + l.h as f32]));
        }
        if f.vr {
            let pointer = self.text.vr_pointer(r, scene);
            self.vr_cursor_overlay = Some(scene.overlays.len());
            scene.overlays.push((pointer, [0.0, 0.0, 7.0 * s, 7.0 * s]));
        } else {
            self.vr_cursor_overlay = None;
        }
        self.text.end_frame(r, scene);
    }
}

impl TextCache {
    /// A small white circle, centred on the point that receives the click.
    fn vr_pointer(&mut self, r: &Renderer, scene: &mut Scene) -> TextureId {
        let key = ("\u{0}vr_pointer_dot".to_string(), 0, [0, 0, 0, 0]);
        if let Some(label) = self.labels.get_mut(&key) {
            label.used = self.frame;
            return label.tex;
        }
        const W: usize = 32;
        const H: usize = 32;
        let mut rgba = vec![0u8; W * H * 4];
        for y in 0..H {
            for x in 0..W {
                let dx = x as f32 + 0.5 - W as f32 * 0.5;
                let dy = y as f32 + 0.5 - H as f32 * 0.5;
                let radius = (dx * dx + dy * dy).sqrt();
                let alpha = (16.0 - radius).clamp(0.0, 1.0);
                let white = radius < 13.0;
                let color = if white {
                    [255, 255, 255, (alpha * 255.0) as u8]
                } else {
                    [0, 0, 0, (alpha * 220.0) as u8]
                };
                rgba[(y * W + x) * 4..(y * W + x + 1) * 4].copy_from_slice(&color);
            }
        }
        let image = omsi_texture::Image {
            width: W as u32,
            height: H as u32,
            rgba,
            has_alpha: true,
        };
        let tex = r.add_texture(scene, &image, false);
        self.labels.insert(
            key,
            Label {
                tex,
                w: W as u32,
                h: H as u32,
                used: self.frame,
            },
        );
        tex
    }

    /// A plate of one colour: 0 the chat's dark translucent input box, 1 the loading
    /// screen's bar track, 2 its fill, 3 .. 7 as said below.
    fn plate(&mut self, r: &Renderer, scene: &mut Scene, kind: u8) -> TextureId {
        let mut rgba = match kind {
            1 => vec![255, 255, 255, 38],
            2 => vec![235, 238, 242, 255],
            // an opaque panel as designed (translucent dark panels show the sky through
            // them), unless the opacity setting asks for less
            3 => vec![22, 22, 22, 255],
            // the chosen line: the interface's accent
            4 => vec![232, 160, 48, 255],
            // a line the mouse is over, the next stop
            5 => vec![255, 255, 255, 28],
            // a hairline between groups (the card's border colour)
            9 => vec![255, 255, 255, 15],
            // the loading screen's black
            8 => vec![0, 0, 0, 255],
            // the picture dimmed behind the menu
            6 => vec![0, 0, 0, 120],
            // behind a note or the tooltip (the old HUD's 40 %)
            7 => vec![0, 0, 0, 102],
            _ => vec![10, 12, 16, 150],
        };
        // the backgrounds - the panels, the chat's box, the plates - as the opacity setting
        // has them (`backdrop`); the accent, the lines and the dimming stay as they are
        if matches!(kind, 0 | 3 | 7) {
            rgba[3] = (rgba[3] as f32 * self.backdrop).round().clamp(0.0, 255.0) as u8;
        }
        let key = (
            "\u{0}plate".to_string(),
            kind as u32,
            [rgba[0], rgba[1], rgba[2], rgba[3]],
        );
        if let Some(l) = self.labels.get_mut(&key) {
            l.used = self.frame;
            return l.tex;
        }
        let img = omsi_texture::Image {
            width: 1,
            height: 1,
            rgba,
            has_alpha: true,
        };
        let tex = r.add_texture(scene, &img, false);
        self.labels.insert(
            key,
            Label {
                tex,
                w: 1,
                h: 1,
                used: self.frame,
            },
        );
        tex
    }

    /// A plate of one RGBA colour, any size (stretched).
    fn solid(&mut self, r: &Renderer, scene: &mut Scene, rgba: [u8; 4]) -> TextureId {
        let key = ("\u{0}solid".to_string(), 0, rgba);
        if let Some(l) = self.labels.get_mut(&key) {
            l.used = self.frame;
            return l.tex;
        }
        let img = omsi_texture::Image {
            width: 1,
            height: 1,
            rgba: rgba.to_vec(),
            has_alpha: true,
        };
        let tex = r.add_texture(scene, &img, false);
        self.labels.insert(
            key,
            Label {
                tex,
                w: 1,
                h: 1,
                used: u64::MAX / 2,
            },
        );
        tex
    }

    /// One anti-aliased quarter disc of `rad` pixels: the rounded corner `idx` (0 top left,
    /// 1 top right, 2 bottom left, 3 bottom right) of a rounded plate.
    fn corner(
        &mut self,
        r: &Renderer,
        scene: &mut Scene,
        rad: u32,
        idx: u32,
        rgba: [u8; 4],
    ) -> TextureId {
        let key = ("\u{0}corner".to_string(), (rad << 8) | idx, rgba);
        if let Some(l) = self.labels.get_mut(&key) {
            l.used = self.frame;
            return l.tex;
        }
        let n = rad as usize;
        let (cx, cy) = (
            if idx & 1 == 0 { rad as f32 } else { 0.0 },
            if idx & 2 == 0 { rad as f32 } else { 0.0 },
        );
        let mut data = vec![0u8; n * n * 4];
        for py in 0..n {
            for px in 0..n {
                let (dx, dy) = (px as f32 + 0.5 - cx, py as f32 + 0.5 - cy);
                let cover = (rad as f32 - (dx * dx + dy * dy).sqrt() + 0.5).clamp(0.0, 1.0);
                let o = (py * n + px) * 4;
                data[o..o + 3].copy_from_slice(&rgba[..3]);
                data[o + 3] = (rgba[3] as f32 * cover).round() as u8;
            }
        }
        let img = omsi_texture::Image {
            width: rad,
            height: rad,
            rgba: data,
            has_alpha: true,
        };
        let tex = r.add_texture(scene, &img, false);
        self.labels.insert(
            key,
            Label {
                tex,
                w: rad,
                h: rad,
                used: u64::MAX / 2,
            },
        );
        tex
    }

    /// A texture made once for `key` (kept while it is used, like a text).
    fn cached(
        &mut self,
        r: &Renderer,
        scene: &mut Scene,
        key: (String, u32, [u8; 4]),
        size: (u32, u32),
        build: impl FnOnce() -> Vec<u8>,
    ) -> TextureId {
        if let Some(l) = self.labels.get_mut(&key) {
            l.used = self.frame;
            return l.tex;
        }
        let img = omsi_texture::Image {
            width: size.0,
            height: size.1,
            rgba: build(),
            has_alpha: true,
        };
        let tex = r.add_texture(scene, &img, false);
        self.labels.insert(
            key,
            Label {
                tex,
                w: size.0,
                h: size.1,
                used: self.frame,
            },
        );
        tex
    }

    /// A whole rounded plate of `w` x `h` pixels in one texture, anti-aliased.
    fn rrect(
        &mut self,
        r: &Renderer,
        scene: &mut Scene,
        w: u32,
        h: u32,
        rad: f32,
        rgba: [u8; 4],
    ) -> TextureId {
        let key = (format!("\u{0}rr{w}x{h}r{}", rad as u32), 0, rgba);
        self.cached(r, scene, key, (w, h), || {
            let mut data = vec![0u8; (w * h * 4) as usize];
            let ri = rad as u32;
            for py in 0..h {
                let band_y = py >= ri && py + ri < h;
                for px in 0..w {
                    let cover = if band_y || (px >= ri && px + ri < w) {
                        1.0
                    } else {
                        let d = rr_dist(
                            px as f32 + 0.5,
                            py as f32 + 0.5,
                            0.0,
                            0.0,
                            w as f32,
                            h as f32,
                            rad,
                        );
                        (0.5 - d).clamp(0.0, 1.0)
                    };
                    let o = ((py * w + px) * 4) as usize;
                    data[o..o + 3].copy_from_slice(&rgba[..3]);
                    data[o + 3] = (rgba[3] as f32 * cover).round() as u8;
                }
            }
            data
        })
    }

    /// A soft shadow under the rounded rectangle `rect`: it fades out over `spread` pixels
    /// and lies `dy` lower.
    fn shadow(
        &mut self,
        r: &Renderer,
        scene: &mut Scene,
        rect: [f32; 4],
        radius: f32,
        spread: f32,
        dy: f32,
        alpha: u8,
    ) {
        let [x0, y0, x1, y1] = [
            rect[0].round(),
            rect[1].round(),
            rect[2].round(),
            rect[3].round(),
        ];
        let (w, h) = ((x1 - x0).max(0.0) as u32, (y1 - y0).max(0.0) as u32);
        let m = spread.round().max(1.0) as u32;
        let (tw, th) = (w + 2 * m, h + 2 * m);
        if w < 2 || h < 2 || tw as u64 * th as u64 > 4_000_000 {
            return;
        }
        let rad = radius
            .round()
            .min((w as f32 * 0.5).floor())
            .min((h as f32 * 0.5).floor())
            .max(0.0);
        let key = (
            format!("\u{0}shadow{w}x{h}r{}m{m}", rad as u32),
            0,
            [0, 0, 0, alpha],
        );
        let tex = self.cached(r, scene, key, (tw, th), || {
            let mut data = vec![0u8; (tw * th * 4) as usize];
            for py in 0..th {
                for px in 0..tw {
                    let d = rr_dist(
                        px as f32 + 0.5,
                        py as f32 + 0.5,
                        m as f32,
                        m as f32,
                        w as f32,
                        h as f32,
                        rad,
                    );
                    let k = (1.0 - d / m as f32).clamp(0.0, 1.0);
                    data[((py * tw + px) * 4 + 3) as usize] = (alpha as f32 * k * k).round() as u8;
                }
            }
            data
        });
        scene.overlays.push((
            tex,
            [
                x0 - m as f32,
                y0 - m as f32 + dy,
                x1 + m as f32,
                y1 + m as f32 + dy,
            ],
        ));
    }

    /// A rectangle with rounded corners of `radius` pixels in `rgba`, on whole pixels so
    /// that translucent colours show no seams: one texture for a plate of some size, else
    /// (a thin bar, a thumb that grows and shrinks) three bands and four corner discs.
    fn rounded(
        &mut self,
        r: &Renderer,
        scene: &mut Scene,
        rect: [f32; 4],
        radius: f32,
        rgba: [u8; 4],
    ) {
        let [x0, y0, x1, y1] = [
            rect[0].round(),
            rect[1].round(),
            rect[2].round(),
            rect[3].round(),
        ];
        let (w, h) = (x1 - x0, y1 - y0);
        if w < 1.0 || h < 1.0 {
            return;
        }
        let rad = radius
            .round()
            .min((w * 0.5).floor())
            .min((h * 0.5).floor())
            .max(0.0);
        if rad < 1.0 {
            let solid = self.solid(r, scene, rgba);
            scene.overlays.push((solid, [x0, y0, x1, y1]));
            return;
        }
        if w.min(h) >= 12.0 && w * h <= 4_000_000.0 {
            let tex = self.rrect(r, scene, w as u32, h as u32, rad, rgba);
            scene.overlays.push((tex, [x0, y0, x1, y1]));
            return;
        }
        let solid = self.solid(r, scene, rgba);
        let ri = rad as u32;
        // the middle band over the whole height, the side bands between the corners
        if x1 - rad > x0 + rad {
            scene.overlays.push((solid, [x0 + rad, y0, x1 - rad, y1]));
        }
        if h > rad * 2.0 {
            scene
                .overlays
                .push((solid, [x0, y0 + rad, x0 + rad, y1 - rad]));
            scene
                .overlays
                .push((solid, [x1 - rad, y0 + rad, x1, y1 - rad]));
        }
        let tl = self.corner(r, scene, ri, 0, rgba);
        let tr = self.corner(r, scene, ri, 1, rgba);
        let bl = self.corner(r, scene, ri, 2, rgba);
        let br = self.corner(r, scene, ri, 3, rgba);
        scene.overlays.push((tl, [x0, y0, x0 + rad, y0 + rad]));
        scene.overlays.push((tr, [x1 - rad, y0, x1, y0 + rad]));
        scene.overlays.push((bl, [x0, y1 - rad, x0 + rad, y1]));
        scene.overlays.push((br, [x1 - rad, y1 - rad, x1, y1]));
    }
}

impl Ui {
    pub fn loading(
        &mut self,
        r: &Renderer,
        scene: &mut Scene,
        width: f32,
        height: f32,
        scale: f32,
        title: &str,
        caption: &str,
        progress: Option<f32>,
        _map_dir: Option<&std::path::Path>,
        t: f32,
    ) {
        let s = scale.max(0.5);
        // --- black, so that nothing of the 3D picture shows
        let black = self.text.plate(r, scene, 8);
        scene.overlays.push((black, [0.0, 0.0, width, height]));
        // --- the full-screen picture behind everything, covering the screen and dimmed
        if self.loading_art.is_none() {
            static ART: &[u8] = include_bytes!("../../../assets/default-loading-screen/image1.png");
            self.loading_art = Some(image::load_from_memory(ART).ok().map(|i| {
                let i = i.into_rgba8();
                let (iw, ih) = i.dimensions();
                let img = omsi_texture::Image {
                    width: iw,
                    height: ih,
                    rgba: i.into_raw(),
                    has_alpha: false,
                };
                (r.add_texture(scene, &img, false), iw, ih)
            }));
        }
        if let Some(Some((tex, iw, ih))) = self.loading_art {
            let k = (width / iw.max(1) as f32).max(height / ih.max(1) as f32);
            let (dw, dh) = (iw as f32 * k, ih as f32 * k);
            let (x0, y0) = ((width - dw) * 0.5, (height - dh) * 0.5);
            scene.overlays.push((tex, [x0, y0, x0 + dw, y0 + dh]));
        }
        let m = 40.0 * s;
        // --- the game's name, bottom left
        self.ensure_logo();
        let logo_px = self.logo_at(r, scene, 52.0 * s);
        match logo_px {
            Some((tex, lw, lh)) => {
                let (lx, ly) = (m.round(), (height - m).round() - lh as f32);
                scene
                    .overlays
                    .push((tex, [lx, ly, lx + lw as f32, ly + lh as f32]));
            }
            _ => {
                let logo =
                    self.text
                        .label(r, scene, "neoOMSI", (64.0 * s) as u32, [255, 255, 255, 0]);
                scene.overlays.push((
                    logo.tex,
                    [m, height - m - logo.h as f32, m + logo.w as f32, height - m],
                ));
            }
        }
        // --- what is going on, bottom right: a dark box with a thin border, text and spinner
        if self.spinner.is_empty() {
            const N: usize = 24;
            const SZ: usize = 96;
            for k in 0..N {
                let head = k as f32 * std::f32::consts::TAU / N as f32;
                let mut data = vec![255u8; SZ * SZ * 4];
                for py in 0..SZ {
                    for px in 0..SZ {
                        let (dx, dy) = (
                            px as f32 + 0.5 - SZ as f32 * 0.5,
                            py as f32 + 0.5 - SZ as f32 * 0.5,
                        );
                        let rr = (dx * dx + dy * dy).sqrt();
                        let ring =
                            (45.0 - rr + 0.5).clamp(0.0, 1.0) * (rr - 31.0 + 0.5).clamp(0.0, 1.0);
                        // angle clockwise from the top; how far behind the head this pixel is
                        let ang = dx.atan2(-dy).rem_euclid(std::f32::consts::TAU);
                        let behind = (head - ang).rem_euclid(std::f32::consts::TAU);
                        let tail = std::f32::consts::PI * 1.1;
                        let arc = if behind < tail {
                            (1.0 - behind / tail).powf(0.8)
                        } else {
                            0.0
                        };
                        let a = ring * (0.28 + 0.72 * arc);
                        data[(py * SZ + px) * 4 + 3] = (a * 255.0).round() as u8;
                    }
                }
                let img = omsi_texture::Image {
                    width: SZ as u32,
                    height: SZ as u32,
                    rgba: data,
                    has_alpha: true,
                };
                let tex = r.add_texture(scene, &img, false);
                self.spinner.push(tex);
            }
        }
        let pct = progress
            .map(|p| format!("   {:.0} %", p.clamp(0.0, 1.0) * 100.0))
            .unwrap_or_default();
        let compose = |pct: &str| {
            if title.is_empty() {
                format!("{caption}{pct}")
            } else {
                format!("{caption} {title}{pct}")
            }
        };
        let line = compose(&pct);
        // (the box keeps its size while the percentage grows)
        let widest = compose(if progress.is_some() { "   100 %" } else { "" });
        let px_text = (18.0 * s) as u32;
        let text_w = self.text.width(&widest, px_text as f32);
        let ring = 22.0 * s;
        let (pad_x, pad_y, gap) = (16.0 * s, 10.0 * s, 16.0 * s);
        let c = self
            .text
            .label(r, scene, &line, px_text, [235, 235, 235, 0]);
        let box_h = (c.h as f32).max(ring) + pad_y * 2.0;
        let box_w = text_w + gap + ring + pad_x * 2.0;
        let (bx1, by1) = (width - m, height - m);
        let (bx0, by0) = (bx1 - box_w, by1 - box_h);
        self.text
            .rounded(r, scene, [bx0, by0, bx1, by1], 3.0 * s, [0, 0, 0, 240]);
        let cy = (by0 + by1) * 0.5;
        scene.overlays.push((
            c.tex,
            [
                bx0 + pad_x,
                cy - c.h as f32 * 0.5,
                bx0 + pad_x + c.w as f32,
                cy + c.h as f32 * 0.5,
            ],
        ));
        let frame = ((t * 26.0) as usize) % self.spinner.len().max(1);
        if let Some(tex) = self.spinner.get(frame).copied() {
            scene.overlays.push((
                tex,
                [
                    bx1 - pad_x - ring,
                    cy - ring * 0.5,
                    bx1 - pad_x,
                    cy + ring * 0.5,
                ],
            ));
        }
        // --- the hairline along the bottom edge
        let bh = (2.0 * s).max(2.0);
        let accent = self.text.plate(r, scene, 4);
        match progress {
            Some(p) => {
                let p = p.clamp(0.0, 1.0);
                if p > 0.0 {
                    scene
                        .overlays
                        .push((accent, [0.0, height - bh, width * p, height]));
                }
            }
            None => {
                let seg = 0.2;
                let head = -seg + (t * 0.5).fract() * (1.0 + seg);
                let (a, b) = (head.max(0.0), (head + seg).min(1.0));
                if b > a {
                    scene
                        .overlays
                        .push((accent, [width * a, height - bh, width * b, height]));
                }
            }
        }
        self.text.end_frame(r, scene);
    }
}

// ---------------------------------------------------------------------------------------
// the game menu and its lists: flat near-black greys and one amber accent, as the launcher

/// How long a fade of the menu takes (a line lit, a switch turned over): a short moment.
const FADE_SECS: f32 = 0.15;
const BAR_SECS: f32 = 0.19;

// (the launcher's values, see `launcher/theme.rs`: neutral greys, no tint)
const PANEL: [u8; 4] = [22, 22, 22, 255];
const PANEL_ALT: [u8; 4] = [31, 31, 31, 255];
const BORDER: [u8; 4] = [255, 255, 255, 15];
const ACCENT: [u8; 4] = [232, 160, 48, 255];
const ACCENT_SOFT: [u8; 4] = [232, 160, 48, 34];
const DANGER: [u8; 4] = [222, 78, 68, 255];
/// A line under the mouse, and the line chosen.
const LIT: [u8; 4] = [255, 255, 255, 16];
const SELECTED: [u8; 4] = [255, 255, 255, 24];
const LIT_DANGER: [u8; 4] = [222, 78, 68, 44];
const CHIP: [u8; 4] = [255, 255, 255, 20];
/// The accent's fill under the mouse, and the ink on it (the launcher's primary button).
const ACCENT_HOT: [u8; 4] = [246, 182, 84, 255];
const ON_ACCENT: [u8; 4] = [18, 14, 8, 0];
/// Text colours (alpha 0: no outline on the flat panel).
const WHITE: [u8; 4] = [236, 236, 236, 0];
const SOFT: [u8; 4] = [200, 200, 200, 0];
const MUTED: [u8; 4] = [142, 142, 142, 0];
const AMBER: [u8; 4] = [255, 200, 110, 0];

/// The ink of a greyed-out line and its small hint.
const OFF_INK: [u8; 4] = [96, 96, 96, 0];
const OFF_HINT: [u8; 4] = [96, 96, 96, 0];

/// The card's radius, a line's, and the inset of lines from the card's edge and of their
/// text from the line's edge (all times the scale).
const CARD_R: f32 = 8.0;
const ROW_R: f32 = 6.0;
const PAD: f32 = 12.0;
const TEXT_IN: f32 = 16.0;

/// `v` (0 to 1) in eighths.
fn quant(v: f32) -> f32 {
    (v.clamp(0.0, 1.0) * 8.0).round() / 8.0
}

/// The colour `t` (0 to 1) of the way from `a` to `b`.
fn mix(a: [u8; 4], b: [u8; 4], t: f32) -> [u8; 4] {
    let t = t.clamp(0.0, 1.0);
    let m = |x: u8, y: u8| (x as f32 + (y as f32 - x as f32) * t).round() as u8;
    [m(a[0], b[0]), m(a[1], b[1]), m(a[2], b[2]), m(a[3], b[3])]
}

/// `c` with its opacity times `k` (0 to 1).
fn fade(c: [u8; 4], k: f32) -> [u8; 4] {
    [
        c[0],
        c[1],
        c[2],
        (c[3] as f32 * k.clamp(0.0, 1.0)).round() as u8,
    ]
}

/// `c` as a text colour (no outline).
fn txt(c: [u8; 4]) -> [u8; 4] {
    [c[0], c[1], c[2], 0]
}

/// The distance of the point (`px`, `py`) from the rounded rectangle at (`x0`, `y0`) of
/// `w` x `h` with corners of `rad`: negative inside.
fn rr_dist(px: f32, py: f32, x0: f32, y0: f32, w: f32, h: f32, rad: f32) -> f32 {
    let (cx, cy) = (x0 + w * 0.5, y0 + h * 0.5);
    let qx = (px - cx).abs() - (w * 0.5 - rad);
    let qy = (py - cy).abs() - (h * 0.5 - rad);
    (qx.max(0.0) * qx.max(0.0) + qy.max(0.0) * qy.max(0.0)).sqrt() + qx.max(qy).min(0.0) - rad
}

/// A line that opens another list ends in dots: the text without them, and whether it did.
fn strip_more(label: &str) -> (&str, bool) {
    match label
        .strip_suffix("...")
        .or_else(|| label.strip_suffix('…'))
    {
        Some(t) => (t.trim_end(), true),
        None => (label, false),
    }
}

impl Ui {
    /// `text` at `x`, its middle on `cy`; returns its width.
    fn put(
        &mut self,
        r: &Renderer,
        scene: &mut Scene,
        text: &str,
        px: u32,
        color: [u8; 4],
        x: f32,
        cy: f32,
    ) -> f32 {
        let l = self.text.label(r, scene, text, px, color);
        let y = cy - l.h as f32 * 0.5;
        scene
            .overlays
            .push((l.tex, [x, y, x + l.w as f32, y + l.h as f32]));
        l.w as f32
    }

    /// The value that eases to `target` (a part of the menu changing its colour or place):
    /// each frame it goes `speed` per second of the way (exponentially). One value per `key`;
    /// the first time it is asked for it is there already.
    fn ease(&mut self, key: (u8, &str, usize), target: f32, speed: f32) -> f32 {
        // (`speed`: the share of the whole way it goes in a second: it takes 1 / speed seconds)
        use std::hash::{Hash, Hasher};
        let mut h = std::collections::hash_map::DefaultHasher::new();
        key.hash(&mut h);
        let dt = self.anim_dt;
        let v = self.anim.entry(h.finish()).or_insert(target);
        let step = speed * dt;
        if (target - *v).abs() <= step {
            *v = target;
        } else if target > *v {
            *v += step;
        } else {
            *v -= step;
        }
        *v
    }

    /// `ease`, in eighths: a colour or a light that fades is a texture made once per step
    /// (a plate or a text of that colour), so a fade goes in a few steps, not in hundreds.
    fn easeq(&mut self, key: (u8, &str, usize), target: f32, speed: f32) -> f32 {
        quant(self.ease(key, target, speed))
    }

    /// `text` ending at `right`, its middle on `cy`; returns its width.
    fn put_right(
        &mut self,
        r: &Renderer,
        scene: &mut Scene,
        text: &str,
        px: u32,
        color: [u8; 4],
        right: f32,
        cy: f32,
    ) -> f32 {
        let l = self.text.label(r, scene, text, px, color);
        let y = cy - l.h as f32 * 0.5;
        scene
            .overlays
            .push((l.tex, [right - l.w as f32, y, right, y + l.h as f32]));
        l.w as f32
    }

    /// The accent bar at the left edge of a line, growing from its middle as `k` goes 0 to 1.
    fn accent_bar(
        &mut self,
        r: &Renderer,
        scene: &mut Scene,
        rect: [f32; 4],
        k: f32,
        danger: bool,
        s: f32,
    ) {
        let full = (rect[3] - rect[1] - 20.0 * s).max(10.0 * s);
        let bh = full * (0.4 + 0.6 * k);
        let by = (rect[1] + rect[3]) * 0.5 - bh * 0.5;
        self.text.rounded(
            r,
            scene,
            [rect[0], by, rect[0] + 2.0 * s, by + bh],
            1.0 * s,
            fade(if danger { DANGER } else { ACCENT }, k),
        );
    }

    fn ensure_logo(&mut self) {
        if self.loading_logo.is_none() {
            static LOGO: &[u8] = include_bytes!("../../../assets/logos/neoOMSI-wordmark.png");
            self.loading_logo = Some(image::load_from_memory(LOGO).ok().map(|i| {
                let mut i = i.into_rgba8();
                // the empty margin around the lettering is cut away
                let (w, h) = i.dimensions();
                let (mut x0, mut y0, mut x1, mut y1) = (w, h, 0, 0);
                for (x, y, p) in i.enumerate_pixels() {
                    if p[3] > 8 {
                        x0 = x0.min(x);
                        y0 = y0.min(y);
                        x1 = x1.max(x + 1);
                        y1 = y1.max(y + 1);
                    }
                }
                if x1 > x0 && y1 > y0 {
                    i = image::imageops::crop_imm(&i, x0, y0, x1 - x0, y1 - y0).to_image();
                }
                let (iw, ih) = i.dimensions();
                self.logo_src = Some(i);
                (0, iw, ih)
            }));
        }
    }

    fn logo_at(
        &mut self,
        r: &Renderer,
        scene: &mut Scene,
        h: f32,
    ) -> Option<(TextureId, u32, u32)> {
        let h = (h.round() as u32).clamp(8, 1024);
        if let Some(&(_, tex, w)) = self.logo_cache.iter().find(|c| c.0 == h) {
            return Some((tex, w, h));
        }
        let src = self.logo_src.as_ref()?;
        let w =
            ((src.width() as f32 * h as f32 / src.height().max(1) as f32).round() as u32).max(1);
        let mut pm = src.clone();
        for p in pm.pixels_mut() {
            let a = p[3] as u32;
            for c in 0..3 {
                p[c] = ((p[c] as u32 * a + 127) / 255) as u8;
            }
        }
        let mut d = image::imageops::resize(&pm, w, h, image::imageops::FilterType::Lanczos3);
        for p in d.pixels_mut() {
            let a = p[3] as u32;
            if a > 0 {
                for c in 0..3 {
                    p[c] = ((p[c] as u32 * 255 + a / 2) / a).min(255) as u8;
                }
            }
        }
        let img = omsi_texture::Image {
            width: w,
            height: h,
            rgba: d.into_raw(),
            has_alpha: true,
        };
        let tex = r.add_texture(scene, &img, false);
        if self.logo_cache.len() >= 6 {
            self.logo_cache.remove(0);
        }
        self.logo_cache.push((h, tex, w));
        Some((tex, w, h))
    }

    /// The pause menu's header: the wordmark instead of the game's name and "Paused".
    /// False when the logo could not be loaded (the text header is drawn then).
    fn menu_logo_header(
        &mut self,
        r: &Renderer,
        scene: &mut Scene,
        x: f32,
        y: f32,
        w: f32,
        header_h: f32,
        s: f32,
    ) -> bool {
        self.ensure_logo();
        if let Some((tex, iw, ih)) = self.logo_at(r, scene, 30.0 * s) {
            let (lw, lh) = (iw as f32, ih as f32);
            let left = (x + (w - lw) * 0.5).round();
            let top = (y + (header_h - 6.0 * s - lh) * 0.5).round();
            scene.overlays.push((tex, [left, top, left + lw, top + lh]));
            true
        } else {
            false
        }
    }

    /// The header of the card at (`x`, `y`) of `w` wide: what the list is of, small and in
    /// capitals, the title large under it, a hairline under both. Its text starts where the
    /// text of the lines does. Returns the middle of the header.
    fn menu_header(
        &mut self,
        r: &Renderer,
        scene: &mut Scene,
        x: f32,
        y: f32,
        w: f32,
        header_h: f32,
        title: &str,
        sub: &str,
        s: f32,
    ) -> f32 {
        let left = x + (PAD + TEXT_IN) * s;
        // (the game's name in the accent; the list names above are in capitals)
        let (eyebrow, eyebrow_ink) = if sub.is_empty() {
            ("neoOMSI".to_string(), txt(ACCENT))
        } else {
            (sub.to_uppercase(), MUTED)
        };
        let e = self
            .text
            .label(r, scene, &eyebrow, (12.0 * s) as u32, eyebrow_ink);
        let title = clip_to(
            &self.text,
            title,
            24.0 * s,
            w - (PAD + TEXT_IN) * 2.0 * s - 80.0 * s,
        );
        let t = self.text.label(r, scene, &title, (24.0 * s) as u32, WHITE);
        let band = header_h - 6.0 * s;
        let top = y + (band - (e.h as f32 + t.h as f32 - 2.0 * s)) * 0.5;
        scene
            .overlays
            .push((e.tex, [left, top, left + e.w as f32, top + e.h as f32]));
        let ty = top + e.h as f32 - 2.0 * s;
        scene
            .overlays
            .push((t.tex, [left, ty, left + t.w as f32, ty + t.h as f32]));
        y + band * 0.5
    }

    /// The key bindings page's own parts: the search field above the list (`label` is the
    /// search line's row: its value is the text typed) and the bar of key hints under it.
    fn keys_chrome(
        &mut self,
        r: &Renderer,
        scene: &mut Scene,
        label: &str,
        span: [f32; 2],
        top: f32,
        cursor: (f32, f32),
        _tin: f32,
        s: f32,
    ) {
        let mut parts = label.split('\u{1f}');
        let _ = parts.next();
        let typing = parts.next() == Some("E");
        let text = parts.next().unwrap_or("");
        let [x0, x1] = span;
        let h = 38.0 * s;
        let field = [x0, top + 3.0 * s, x1, top + 3.0 * s + h];
        let cy = (field[1] + field[3]) * 0.5;
        self.menu_search = Some(field);
        // (the focus fades in and out: glow, border, fill, magnifier and hint follow it)
        let hovered = cursor.0 >= field[0]
            && cursor.0 <= field[2]
            && cursor.1 >= field[1]
            && cursor.1 <= field[3];
        let f = self.easeq((20, "keysearch", 0), if typing { 1.0 } else { 0.0 }, 6.0);
        let hv = self.easeq(
            (21, "keysearch", 0),
            if hovered && !typing { 1.0 } else { 0.0 },
            8.0,
        );
        let lit = f.max(hv * 0.6);
        let rad = h * 0.5;
        let glow = 0.22 * f + 0.07 * hv;
        if glow > 0.0 {
            let g = (4.0 * f + 2.0 * hv) * s;
            self.text.rounded(
                r,
                scene,
                [field[0] - g, field[1] - g, field[2] + g, field[3] + g],
                rad + g,
                fade(ACCENT, glow),
            );
        }
        let fill = mix(PANEL_ALT, [40, 40, 40, 255], lit);
        self.text.rounded(
            r,
            scene,
            [
                field[0] - 1.0,
                field[1] - 1.0,
                field[2] + 1.0,
                field[3] + 1.0,
            ],
            rad + 1.0,
            mix(BORDER, ACCENT, lit),
        );
        self.text.rounded(r, scene, field, rad, fill);
        // the magnifier: a ring and its handle, lit by the focus
        let ink = mix([142, 142, 142, 255], ACCENT, f.max(hv * 0.8));
        let (mx, my) = (field[0] + 22.0 * s, cy - 1.5 * s);
        self.text.rounded(
            r,
            scene,
            [mx - 6.5 * s, my - 6.5 * s, mx + 6.5 * s, my + 6.5 * s],
            6.5 * s,
            ink,
        );
        self.text.rounded(
            r,
            scene,
            [mx - 4.5 * s, my - 4.5 * s, mx + 4.5 * s, my + 4.5 * s],
            4.5 * s,
            fill,
        );
        for n in 0..4 {
            let o = (5.5 + n as f32 * 1.6) * s;
            self.text.rounded(
                r,
                scene,
                [
                    mx + o - 1.1 * s,
                    my + o - 1.1 * s,
                    mx + o + 1.1 * s,
                    my + o + 1.1 * s,
                ],
                1.1 * s,
                ink,
            );
        }
        let px = (15.0 * s) as u32;
        let tx = field[0] + 42.0 * s;
        let room = field[2] - tx - 18.0 * s;
        let mut caret_x = tx;
        if text.is_empty() {
            let hint = clip_to(&self.text, "Search key bindings", px as f32, room);
            self.put(
                r,
                scene,
                &hint,
                px,
                txt(mix([142, 142, 142, 0], [92, 92, 92, 0], f)),
                tx + 4.0 * s * f,
                cy,
            );
        } else {
            let t = clip_left(&self.text, text, px as f32, room);
            let w = self.put(r, scene, &t, px, WHITE, tx, cy);
            caret_x = tx + w + 2.0 * s;
        }
        // the text cursor: blinks (fades in and out) while the field is being typed in
        if typing {
            let cv = self.ease(
                (23, "keysearch", 0),
                if self.caret_up { 1.0 } else { 0.0 },
                2.5,
            );
            if cv >= 1.0 {
                self.caret_up = false;
            } else if cv <= 0.0 {
                self.caret_up = true;
            }
            let a = quant(cv).max(0.125);
            self.text.rounded(
                r,
                scene,
                [caret_x, cy - 9.0 * s, caret_x + 2.0 * s, cy + 9.0 * s],
                1.0 * s,
                fade([236, 236, 236, 255], a),
            );
        }
    }

    /// A pill (`cap`: a key cap) of `fill` with `text` in it, ending at `right`; returns
    /// its left edge.
    fn chip(
        &mut self,
        r: &Renderer,
        scene: &mut Scene,
        text: &str,
        px: u32,
        color: [u8; 4],
        fill: [u8; 4],
        cap: bool,
        right: f32,
        cy: f32,
        s: f32,
    ) -> f32 {
        let l = self.text.label(r, scene, text, px, color);
        let (cw, ch) = (l.w as f32 + 18.0 * s, l.h as f32 + 6.0 * s);
        let x0 = right - cw;
        if cap {
            // (a key cap: a fine light edge round it)
            self.text.rounded(
                r,
                scene,
                [
                    x0 - 1.0,
                    cy - ch * 0.5 - 1.0,
                    right + 1.0,
                    cy + ch * 0.5 + 1.0,
                ],
                5.0 * s,
                [96, 96, 96, 255],
            );
        }
        self.text.rounded(
            r,
            scene,
            [x0, cy - ch * 0.5, right, cy + ch * 0.5],
            if cap { 4.0 * s } else { ROW_R * s },
            fill,
        );
        let (tx, ty) = (x0 + 9.0 * s, cy - l.h as f32 * 0.5);
        scene
            .overlays
            .push((l.tex, [tx, ty, tx + l.w as f32, ty + l.h as f32]));
        x0
    }

    /// The game menu and its lists (options, lines, tours ...): a card in the middle of a
    /// dimmed picture. Options are settings lines with switches and values; lines and tours
    /// are bigger lines with the timetable of the chosen one beside them.
    fn draw_menu(&mut self, r: &Renderer, scene: &mut Scene, f: &Frame) {
        self.menu_rects.clear();
        self.menu_ctl.clear();
        self.menu_side.clear();
        self.menu_pane.clear();
        self.menu_pane_start = 0;
        self.menu_pane_go = None;
        self.menu_pane_box = None;
        self.menu_time.clear();
        self.menu_scroll_thumb = None;
        self.menu_scroll_track = None;
        self.dd_rects.clear();
        let overlay_start = scene.overlays.len();
        let Some((sel, items)) = f.menu else {
            self.menu_overlay_range = overlay_start..overlay_start;
            self.anim.clear();
            return;
        };
        if let Some(report) = f.report {
            self.draw_run_report(r, scene, f, report, sel);
            self.menu_overlay_range = overlay_start..scene.overlays.len();
            return;
        }
        // a settings window has its own layout
        if f.menu_kind == MenuKind::Options && f.menu_tabs.is_some() {
            self.draw_settings(r, scene, f, sel, items);
            self.menu_overlay_range = overlay_start..scene.overlays.len();
            return;
        }
        let s = f.scale.max(0.5);
        let kind = f.menu_kind;
        // the picture dimmed behind the menu (the first overlay: the headset's menu takes it
        // for the backdrop)
        let dim = self.text.plate(r, scene, 6);
        let sep = self.text.plate(r, scene, 9);
        scene.overlays.push((dim, [0.0, 0.0, f.width, f.height]));
        // key hints only where there is a keyboard
        let keys = !f.vr && !crate::platform::touch_controls();
        // the timetable beside the list, where the window is wide enough
        let preview = f
            .menu_preview
            .as_ref()
            .filter(|_| !f.vr && f.width >= 760.0 * s);
        let timetable_kind = matches!(kind, MenuKind::Lines | MenuKind::Tours);
        let pane_w = if preview.is_some() {
            (if timetable_kind { 320.0 } else { 340.0 }) * s
        } else {
            0.0
        };
        let want = if timetable_kind { 360.0 * s } else { 380.0 * s };
        let w = (want + pane_w).min(f.width - 24.0 * s).max(200.0 * s);
        let list_w = w - pane_w;
        let header_h = 72.0 * s;
        let pad = PAD * s;
        let tin = TEXT_IN * s;
        // as many lines as fit at a readable height; a longer menu scrolls (the wheel, the
        // arrow keys), the chosen line kept in view
        // (lines and tours: a card of one fixed height, a share of the screen's)
        // "Back" of lines, tours and the other lists stands alone under the list (not a line of
        // it: no scrolling down to it), and their card keeps one size
        let back_txt = omsi_ui::tr("Back").into_owned();
        let back_footer = (timetable_kind || kind == MenuKind::List)
            && items
            .last()
            .is_some_and(|&(id, l)| id == "back" && l == back_txt.as_str());
        let nl = items.len() - back_footer as usize;
        let foot_h = if back_footer { 48.0 * s } else { 0.0 };
        let fixed_h =
            matches!(kind, MenuKind::Lines | MenuKind::Tours | MenuKind::List).then(|| {
                if f.vr {
                    f.height * 0.60
                } else {
                    (520.0 * s).min(f.height * 0.94)
                }
            });
        let room = match fixed_h {
            Some(fh) => fh - header_h - pad - foot_h,
            None => f.height * (if f.vr { 0.60 } else { 0.92 }) - header_h - pad - 8.0 * s,
        };
        let base = match (f.vr, kind) {
            (true, _) => 40.0,
            (_, MenuKind::Lines) => 44.0,
            (_, MenuKind::Tours) => 40.0,
            _ => 42.0,
        } * s;
        let row_h = base.min(room / nl.max(1) as f32).max(34.0 * s);
        let rows = ((room / row_h).floor() as usize).clamp(1, nl.max(1));
        let sel_l = sel.min(nl.saturating_sub(1));
        let start = match (nl > rows, f.menu_top) {
            (false, _) => 0,
            (true, Some(top)) => (top.max(0.0).round() as usize).min(nl - rows),
            (true, None) => sel_l.saturating_sub(rows / 2).min(nl - rows),
        };
        self.menu_start = start;
        self.menu_rows = rows;
        self.menu_row_h = row_h;
        let px = (((if timetable_kind { 14.0 } else { 16.0 }) * s).min(row_h * 0.45)) as u32;
        let mut h = header_h + row_h * rows as f32 + pad + foot_h;
        if let Some(fh) = fixed_h {
            // (lines and tours: one size, whatever the list holds, like the settings window)
            h = fh;
        } else if preview.is_some() {
            h = h.max((420.0 * s).min(f.height * 0.92));
        }
        let x = ((f.width - w) * 0.5).round();
        let y = ((f.height - h) * 0.5).round();
        let list_r = x + list_w;
        let radius = CARD_R * s;
        // the card: a soft shadow, a 1 px hairline round it, the flat panel
        self.text.shadow(
            r,
            scene,
            [x, y, x + w, y + h],
            radius,
            28.0 * s,
            10.0 * s,
            110,
        );
        self.text.rounded(
            r,
            scene,
            [x - 1.0, y - 1.0, x + w + 1.0, y + h + 1.0],
            radius + 1.0,
            BORDER,
        );
        self.text
            .rounded(r, scene, [x, y, x + w, y + h], radius, PANEL);
        // the header: what the list is of (or the game's name), and the title
        let (title, sub): (String, String) = match f.menu_head.as_ref() {
            Some((t, u)) => (t.clone(), u.clone()),
            None => {
                let t = if f.paused { "Paused" } else { "Menu" };
                (t.to_string(), String::new())
            }
        };
        if !(f.paused
            && f.menu_head.is_none()
            && self.menu_logo_header(r, scene, x, y, w, header_h, s))
        {
            self.menu_header(r, scene, x, y, w, header_h, &title, &sub, s);
        }
        // the scroll bar: where the lines shown lie in the whole menu
        let scrolls = nl > rows;
        if scrolls {
            let top = y + header_h;
            let track = [
                list_r - 11.0 * s,
                top,
                list_r - 8.0 * s,
                top + row_h * rows as f32 - 4.0 * s,
            ];
            self.menu_scroll_track = Some(track);
            self.text
                .rounded(r, scene, track, 1.5 * s, [255, 255, 255, 22]);
            let th = track[3] - track[1];
            let t0 = track[1] + th * start as f32 / nl as f32;
            let t1 = track[1] + th * (start + rows) as f32 / nl as f32;
            let thumb = [track[0], t0, track[2], t1];
            self.text.rounded(r, scene, thumb, 1.5 * s, ACCENT);
            // (a wider grip than the drawn thumb: three pixels are hard to hit)
            self.menu_scroll_thumb =
                Some([thumb[0] - 6.0 * s, thumb[1], thumb[2] + 6.0 * s, thumb[3]]);
        }
        // (the line under the mouse is the one lit; the keyboard's choice only while the
        // mouse is off the lines - both lit at once read as two choices)
        let over = |rect: [f32; 4]| {
            f.cursor.0 >= rect[0]
                && f.cursor.0 <= rect[2]
                && f.cursor.1 >= rect[1]
                && f.cursor.1 <= rect[3]
        };
        // (the whole list: in the gaps between the lines the keyboard's choice, the top line,
        // lit up for a moment as the mouse went down the list)
        let any_hovered = over([x, y, list_r, y + h]);
        let right = list_r - if scrolls { 20.0 * s } else { pad };
        let line_pre = format!("{} ", omsi_ui::tr("Line"));
        let tour_pre = format!("{} ", omsi_ui::tr("Tour"));
        // (all line signs as wide as the widest, so that the texts after them line up)
        let mut sign_w = 48.0 * s;
        if kind == MenuKind::Lines {
            for &(_, label) in items.iter() {
                if let Some(n) = label
                    .strip_prefix(line_pre.as_str())
                    .and_then(|rest| rest.rsplit_once("  ("))
                    .map(|(n, _)| n)
                {
                    sign_w = sign_w.max(self.text.width(n, 15.0 * s) + 22.0 * s);
                }
            }
        }
        for (k, &(id, label)) in items[..nl].iter().enumerate().skip(start).take(rows) {
            let ry = y + header_h + row_h * (k - start) as f32;
            let gap = 4.0 * s;
            let rect = [x + pad, ry, right, ry + row_h - gap];
            // (a greyed-out line is never lit: not by the mouse, not by the keyboard)
            let off = kind == MenuKind::Game && f.menu_disabled.contains(&id);
            let lit = !off && (over(rect) || (k == sel && f.menu_kbd && !any_hovered));
            let danger = id == "quit";
            let is_back = id == "back" && label == back_txt.as_str();
            // a thin line between the groups: the everyday lines apart from the rarer ones
            // (none in the other lists: "Back" there stands apart under them, without a line)
            let apart = k > start
                && match kind {
                MenuKind::Game => matches!(id, "save" | "admin" | "quit"),
                MenuKind::Lines => id == "free",
                _ => false,
            };
            if apart {
                let sy = (ry - 2.0 * s).round();
                scene
                    .overlays
                    .push((sep, [x + pad + tin, sy, right - tin, sy + 1.0]));
            }
            // (the light of the line eases in and out)
            let glow = self.easeq((7, id, k), if lit { 1.0 } else { 0.0 }, 1.0 / FADE_SECS);
            // (the tour chosen stays marked, whatever the mouse is over)
            let active = kind == MenuKind::Tours && k == sel && !is_back;
            let a_act = self.easeq((14, id, k), if active { 1.0 } else { 0.0 }, 1.0 / FADE_SECS);
            let bar = self.easeq(
                (8, id, k),
                if lit || active { 1.0 } else { 0.0 },
                1.0 / BAR_SECS,
            );
            if a_act > 0.0 {
                self.text
                    .rounded(r, scene, rect, ROW_R * s, fade(SELECTED, a_act));
            }
            if glow > 0.0 {
                self.text.rounded(
                    r,
                    scene,
                    rect,
                    ROW_R * s,
                    fade(if danger { LIT_DANGER } else { LIT }, glow),
                );
            }
            if bar > 0.0 {
                // the accent bar at the left edge of the lit line, growing from its middle
                self.accent_bar(r, scene, rect, bar, danger, s);
            }
            let ink = if off {
                OFF_INK
            } else {
                if danger {
                    mix([232, 138, 128, 0], [255, 176, 166, 0], glow)
                } else {
                    mix(SOFT, WHITE, glow.max(a_act))
                }
            };
            let cy = (rect[1] + rect[3]) * 0.5;
            let lx = rect[0] + tin;
            let rx = rect[2] - tin;
            let mut done = false;
            match kind {
                // a line of the timetable: its number on a solid sign, like a line sign on a bus
                MenuKind::Lines => {
                    let parsed = label
                        .strip_prefix(line_pre.as_str())
                        .and_then(|rest| rest.rsplit_once("  ("))
                        .map(|(n, t)| (n, t.trim_end_matches(')')));
                    if let Some((name, info)) = parsed {
                        let bl = self
                            .text
                            .label(r, scene, name, (15.0 * s) as u32, ON_ACCENT);
                        let (bw, bh) = (sign_w, 28.0 * s);
                        let bx = lx;
                        let sign = mix(mix(ACCENT, [150, 104, 30, 255], 0.30), ACCENT_HOT, glow);
                        self.text.rounded(
                            r,
                            scene,
                            [bx, cy - bh * 0.5, bx + bw, cy + bh * 0.5],
                            7.0 * s,
                            sign,
                        );
                        let (lx0, ly0) = (bx + (bw - bl.w as f32) * 0.5, cy - bl.h as f32 * 0.5);
                        scene
                            .overlays
                            .push((bl.tex, [lx0, ly0, lx0 + bl.w as f32, ly0 + bl.h as f32]));
                        let tx = bx + bw + 14.0 * s;
                        let d = 22.0 * s;
                        self.text.rounded(
                            r,
                            scene,
                            [rx - d, cy - d * 0.5, rx, cy + d * 0.5],
                            d * 0.5,
                            fade(ACCENT, 0.10 + 0.30 * glow),
                        );
                        let aw = self.text.width("›", (px + 2) as f32);
                        self.put(
                            r,
                            scene,
                            "›",
                            px + 2,
                            mix(MUTED, ACCENT_HOT, glow),
                            rx - d * 0.5 - aw * 0.5,
                            cy - 1.0 * s,
                        );
                        let info = clip_to(&self.text, info, px as f32, rx - d - 12.0 * s - tx);
                        self.put(r, scene, &info, px, ink, tx, cy);
                        done = true;
                    }
                }
                // a tour: its name (the time is chosen beside the list)
                MenuKind::Tours => {
                    if let Some(rest) = label.strip_prefix(tour_pre.as_str()) {
                        let num = rest.split_once("  ").map(|(n, _)| n).unwrap_or(rest).trim();
                        let name =
                            clip_to(&self.text, &format!("{tour_pre}{num}"), px as f32, rx - lx);
                        self.put(r, scene, &name, px, ink, lx, cy);
                        done = true;
                    }
                }
                _ => {}
            }
            if !done {
                let (text, more) = strip_more(label);
                let mut avail = rx - lx;
                if is_back {
                    let plain = matches!(kind, MenuKind::Lines | MenuKind::Tours);
                    if plain {
                        self.text
                            .rounded(r, scene, rect, ROW_R * s, fade(LIT, 0.55));
                    }
                    let idle = if plain { SOFT } else { MUTED };
                    self.put(
                        r,
                        scene,
                        &format!("‹  {text}"),
                        px,
                        if lit { WHITE } else { idle },
                        lx,
                        cy,
                    );
                } else {
                    if off {
                        // (why the line cannot be chosen)
                        let cw = self.put_right(
                            r,
                            scene,
                            "No active route",
                            (12.0 * s) as u32,
                            OFF_HINT,
                            rx,
                            cy,
                        );
                        avail -= cw + 10.0 * s;
                    }
                    if more {
                        // (the line opens another list)
                        let cw = self.put_right(
                            r,
                            scene,
                            "›",
                            px + 6,
                            if lit { WHITE } else { MUTED },
                            rx,
                            cy,
                        );
                        avail -= cw + 10.0 * s;
                    } else if id == "resume" && keys {
                        // ("Resume" shows the key that does the same)
                        let left = self.chip(
                            r,
                            scene,
                            "Esc",
                            (12.0 * s) as u32,
                            WHITE,
                            [52, 52, 52, 255],
                            true,
                            rx,
                            cy,
                            s,
                        );
                        avail = left - 10.0 * s - lx;
                    }
                    let text = clip_to(&self.text, text, px as f32, avail);
                    self.put(r, scene, &text, px, ink, lx, cy);
                }
            }
            self.menu_rects.push(rect);
        }
        if back_footer {
            // ("Back" pinned to the bottom of the list: the rects of the lines
            // out of view are empty ones, so that a rect stays at the index of its line)
            let bk = items.len() - 1;
            let fr = [x + pad, y + h - pad - 36.0 * s, right, y + h - pad];
            let lit = over(fr) || (sel == bk && f.menu_kbd && !any_hovered);
            let glow = self.easeq(
                (7, "back", bk),
                if lit { 1.0 } else { 0.0 },
                1.0 / FADE_SECS,
            );
            self.text
                .rounded(r, scene, fr, ROW_R * s, fade(LIT, 0.55 + 0.45 * glow));
            if glow > 0.0 {
                self.accent_bar(r, scene, fr, glow, false, s);
            }
            let (text, _) = strip_more(back_txt.as_str());
            self.put(
                r,
                scene,
                &format!("‹  {text}"),
                px,
                mix(SOFT, WHITE, glow),
                fr[0] + tin,
                (fr[1] + fr[3]) * 0.5,
            );
            while self.menu_rects.len() < bk - start {
                self.menu_rects.push([-1.0e9; 4]);
            }
            self.menu_rects.push(fr);
        }
        // the timetable of the chosen line or tour, beside the list
        if let Some(p) = preview {
            let (px0, py0) = (list_r + 4.0 * s, y + header_h);
            let (px1, py1) = (x + w - pad, y + h - pad);
            self.text.rounded(
                r,
                scene,
                [px0 - 1.0, py0 - 1.0, px1 + 1.0, py1 + 1.0],
                CARD_R * s + 1.0,
                BORDER,
            );
            self.text
                .rounded(r, scene, [px0, py0, px1, py1], CARD_R * s, PANEL_ALT);
            let pad = tin;
            let inner = px1 - px0 - pad * 2.0;
            let mut cy = py0 + 28.0 * s;
            let head = clip_to(&self.text, &p.title, 16.0 * s, inner);
            self.put(r, scene, &head, (16.0 * s) as u32, WHITE, px0 + pad, cy);
            cy += 22.0 * s;
            let meta = clip_to(&self.text, &p.meta, 11.0 * s, inner);
            self.put(r, scene, &meta, (11.0 * s) as u32, MUTED, px0 + pad, cy);
            cy += 18.0 * s;
            scene
                .overlays
                .push((sep, [px0 + pad, cy.round(), px1 - pad, cy.round() + 1.0]));
            let rpx = (13.0 * s) as u32;
            let lh = 24.0 * s;
            let mut top = cy + 10.0 * s;
            // the tour, and the time of the trip, each between arrows that step through them
            let nav_rows: Vec<(&String, f32, usize)> =
                p.time.iter().map(|t| (t, 20.0f32, 0usize)).collect();
            for (time, fs, base) in nav_rows {
                let bh = 30.0 * s;
                let bw = 46.0 * s;
                let by = top;
                let right = px1 - pad;
                let left = px0 + pad;
                for j in 0..2usize {
                    let rect = if j == 0 {
                        [left, by, left + bw, by + bh]
                    } else {
                        [right - bw, by, right, by + bh]
                    };
                    let a = self.easeq(
                        (14, "time", j + base),
                        if over(rect) { 1.0 } else { 0.0 },
                        1.0 / FADE_SECS,
                    );
                    self.text.rounded(
                        r,
                        scene,
                        rect,
                        ROW_R * s,
                        mix([232, 160, 48, 60], ACCENT, a * 0.7),
                    );
                    let col = mix(ACCENT_HOT, [18, 14, 8, 255], a);
                    let (cx, cy) = ((rect[0] + rect[2]) * 0.5, (rect[1] + rect[3]) * 0.5);
                    // (a real arrow: a shaft and a head of stacked strips)
                    let dir = if j == 0 { -1.0 } else { 1.0 };
                    let (half, head_w, head_h) = (9.0 * s, 7.0 * s, 7.0 * s);
                    let t = (2.0 * s).max(2.0);
                    self.text.rounded(
                        r,
                        scene,
                        [cx - half, cy - t * 0.5, cx + half, cy + t * 0.5],
                        0.0,
                        col,
                    );
                    let tip = cx + dir * half;
                    let strips = 7;
                    for k in 0..strips {
                        // (strip k, from the head's base towards the tip, narrowing)
                        let bx = tip - dir * head_w * (1.0 - k as f32 / strips as f32);
                        let ex = tip - dir * head_w * (1.0 - (k as f32 + 1.0) / strips as f32);
                        let h = head_h * (1.0 - (k as f32 + 0.5) / strips as f32);
                        self.text.rounded(
                            r,
                            scene,
                            [bx.min(ex), cy - h, bx.max(ex), cy + h],
                            0.0,
                            col,
                        );
                    }
                    self.menu_time.push(rect);
                }
                let mid_l = left + bw + 10.0 * s;
                let mid_r = right - bw - 10.0 * s;
                let time = clip_to(&self.text, time, fs * s, mid_r - mid_l);
                let tw = self.text.width(&time, fs * s);
                self.put(
                    r,
                    scene,
                    &time,
                    (fs * s) as u32,
                    if base == 0 { AMBER } else { WHITE },
                    mid_l + (mid_r - mid_l - tw) * 0.5,
                    by + bh * 0.5,
                );
                top = by + bh + 8.0 * s;
            }
            let n = p.rows.len();
            if let (Some(chosen), Some(button)) = (p.chosen, p.button.as_ref()) {
                // the stops to start from: the one chosen marked, a click chooses another
                let go_h = 34.0 * s;
                let go = [px0 + pad, py1 - 12.0 * s - go_h, px1 - pad, py1 - 12.0 * s];
                let fit = (((go[1] - 10.0 * s) - top) / lh).floor().max(1.0) as usize;
                let first = match f.pane_first {
                    Some(p) if n > fit => p.min(n - fit),
                    _ if n > fit => chosen.saturating_sub(fit / 2).min(n - fit),
                    _ => 0,
                };
                self.menu_pane_start = first;
                self.menu_pane_box = Some([px0, py0, px1, py1]);
                // (a long list of stops: a thin scroll bar at the pane's edge)
                if n > fit {
                    let (tt, tb) = (top, go[1] - 10.0 * s);
                    let th = tb - tt;
                    self.text.rounded(
                        r,
                        scene,
                        [px1 - 6.0 * s, tt, px1 - 3.0 * s, tb],
                        1.5 * s,
                        [255, 255, 255, 22],
                    );
                    let t0 = tt + th * first as f32 / n as f32;
                    let t1 = tt + th * (first + fit) as f32 / n as f32;
                    self.text.rounded(
                        r,
                        scene,
                        [px1 - 6.0 * s, t0, px1 - 3.0 * s, t1],
                        1.5 * s,
                        ACCENT,
                    );
                }
                let time_w = p
                    .rows
                    .iter()
                    .skip(first)
                    .take(fit)
                    .map(|row| self.text.width(&row.1, rpx as f32))
                    .fold(0.0f32, f32::max);
                for (i, (what, when)) in p.rows.iter().enumerate().skip(first).take(fit) {
                    let ry = top + lh * (i - first) as f32 + lh * 0.5;
                    let rect = [px0 + 8.0 * s, ry - lh * 0.5, px1 - 8.0 * s, ry + lh * 0.5];
                    let on = i == chosen;
                    let hov = over(rect) && !on;
                    let a_on =
                        self.easeq((11, "stop", i), if on { 1.0 } else { 0.0 }, 1.0 / FADE_SECS);
                    let a_hov = self.easeq(
                        (12, "stop", i),
                        if hov { 1.0 } else { 0.0 },
                        1.0 / FADE_SECS,
                    );
                    if a_hov > 0.0 {
                        self.text
                            .rounded(r, scene, rect, ROW_R * s, fade(LIT, a_hov));
                    }
                    if a_on > 0.0 {
                        self.text
                            .rounded(r, scene, rect, ROW_R * s, fade(SELECTED, a_on));
                        self.text.rounded(
                            r,
                            scene,
                            [
                                rect[0],
                                rect[1] + 7.0 * s,
                                rect[0] + 2.0 * s,
                                rect[3] - 7.0 * s,
                            ],
                            1.0 * s,
                            fade(ACCENT, a_on),
                        );
                    }
                    self.put_right(r, scene, when, rpx, AMBER, px1 - pad, ry);
                    let what = clip_to(&self.text, what, rpx as f32, inner - time_w - 14.0 * s);
                    self.put(r, scene, &what, rpx, mix(SOFT, WHITE, a_on), px0 + pad, ry);
                    self.menu_pane.push(rect);
                }
                // the button that starts the trip
                let a_go = self.easeq(
                    (13, "go", 0),
                    if over(go) { 1.0 } else { 0.0 },
                    1.0 / FADE_SECS,
                );
                self.text
                    .rounded(r, scene, go, ROW_R * s, mix(ACCENT, ACCENT_HOT, a_go));
                let l = self
                    .text
                    .label(r, scene, button, (14.0 * s) as u32, ON_ACCENT);
                let (gx, gy) = (
                    go[0] + (go[2] - go[0] - l.w as f32) * 0.5,
                    (go[1] + go[3]) * 0.5 - l.h as f32 * 0.5,
                );
                scene
                    .overlays
                    .push((l.tex, [gx, gy, gx + l.w as f32, gy + l.h as f32]));
                self.menu_pane_go = Some(go);
            } else {
                let fit = ((py1 - 12.0 * s - top) / lh).floor().max(1.0) as usize;
                // (a list too long for the pane ends in how many more there are)
                let shown = if n > fit { fit.saturating_sub(1) } else { n };
                let time_w = p
                    .rows
                    .iter()
                    .take(shown)
                    .map(|row| self.text.width(&row.1, rpx as f32))
                    .fold(0.0f32, f32::max);
                let tp = format!("{} ", omsi_ui::tr("Tour"));
                // (the tour numbers in tiles of one width, the destination after them)
                let tile_w = p
                    .rows
                    .iter()
                    .take(shown)
                    .filter_map(|row| {
                        row.0
                            .strip_prefix(tp.as_str())
                            .and_then(|x| x.split_once("  ›  ").map(|(n, _)| n).or(Some(x)))
                    })
                    .map(|n| self.text.width(n.trim(), rpx as f32) + 16.0 * s)
                    .fold(30.0 * s, f32::max);
                for (i, (what, when)) in p.rows.iter().take(shown).enumerate() {
                    let ry = top + lh * i as f32 + lh * 0.5;
                    self.put_right(r, scene, when, rpx, AMBER, px1 - pad, ry);
                    if let Some(rest) = what.strip_prefix(tp.as_str()) {
                        let (num, dest) = rest.split_once("  ›  ").unwrap_or((rest, ""));
                        let th = lh - 6.0 * s;
                        self.text.rounded(
                            r,
                            scene,
                            [px0 + pad, ry - th * 0.5, px0 + pad + tile_w, ry + th * 0.5],
                            5.0 * s,
                            ACCENT_SOFT,
                        );
                        let nw = self.text.width(num.trim(), rpx as f32);
                        self.put(
                            r,
                            scene,
                            num.trim(),
                            rpx,
                            txt(ACCENT),
                            px0 + pad + (tile_w - nw) * 0.5,
                            ry,
                        );
                        let dx = px0 + pad + tile_w + 10.0 * s;
                        let dest = clip_to(
                            &self.text,
                            dest,
                            rpx as f32,
                            px1 - pad - time_w - 14.0 * s - dx,
                        );
                        self.put(r, scene, &dest, rpx, SOFT, dx, ry);
                    } else {
                        let what = clip_to(&self.text, what, rpx as f32, inner - time_w - 14.0 * s);
                        self.put(r, scene, &what, rpx, SOFT, px0 + pad, ry);
                    }
                }
                if n > shown {
                    let ry = top + lh * shown as f32 + lh * 0.5;
                    self.put(
                        r,
                        scene,
                        &omsi_ui::tr("+{} more").replacen("{}", &(n - shown).to_string(), 1),
                        (12.0 * s) as u32,
                        MUTED,
                        px0 + pad,
                        ry,
                    );
                }
            }
        }
        self.menu_overlay_range = overlay_start..scene.overlays.len();
    }
}

impl Ui {
    /// A settings window (options, vehicle, world): a sidebar of pages on the left, the rows
    /// of the page shown on the right. A row is `name\u{1f}kind\u{1f}value\u{1f}description\u{1f}fraction`:
    /// `s` a switch (value "on"/"off"), `v` a slider (value text, fraction of the way),
    /// `c` a stepper (value between arrows), `o` opens a list, `a` a button (its text is the
    /// value), `i` information.
    fn draw_settings(
        &mut self,
        r: &Renderer,
        scene: &mut Scene,
        f: &Frame,
        sel: usize,
        items: &[(&str, &str)],
    ) {
        let s = f.scale.max(0.5);
        let dim = self.text.plate(r, scene, 6);
        let sep = self.text.plate(r, scene, 9);
        scene.overlays.push((dim, [0.0, 0.0, f.width, f.height]));
        let none: Vec<String> = Vec::new();
        let (titles, active) = match f.menu_tabs.as_ref() {
            Some((t, a)) => (t, *a),
            None => (&none, 0),
        };
        // a window of a fixed size (as far as the screen allows): the pages differ in how many
        // rows they have, the window does not - a long page scrolls inside it
        let want_side = if titles.is_empty() { 0.0 } else { 200.0 * s };
        let w = (want_side + 680.0 * s)
            .min(f.width - 24.0 * s)
            .max(260.0 * s);
        let side_w = want_side.min(w * 0.38);
        let header_h = 72.0 * s;
        let pad = PAD * s;
        let tin = TEXT_IN * s;
        let fixed_h = (if f.vr { 440.0 } else { 600.0 }) * s;
        let h = fixed_h
            .min(f.height * (if f.vr { 0.70 } else { 0.94 }))
            .max(220.0 * s);
        // the key bindings page: its first line is the search (drawn as a field above the
        // list, not a row of it), a hint bar under the list, and slimmer rows
        let keys_page = items.first().is_some_and(|i| i.0 == "keysearch");
        self.menu_search = None;
        let lead = keys_page as usize;
        let top_off = if keys_page { 48.0 * s } else { 0.0 };
        let bot_off = 0.0;
        let room = h - header_h - pad - top_off - bot_off;
        let row_h = if keys_page {
            (50.0 * s).min(room).max(30.0 * s)
        } else {
            ((if f.vr { 54.0 } else { 62.0 }) * s)
                .min(room)
                .max(30.0 * s)
        };
        let n_items = items.len() - lead;
        let rows = ((room / row_h).floor() as usize).clamp(1, n_items.max(1));
        let start = lead
            + match (n_items > rows, f.menu_top) {
            (false, _) => 0,
            (true, Some(top)) => (top.max(0.0).round() as usize)
                .saturating_sub(lead)
                .min(n_items - rows),
            (true, None) => sel
                .saturating_sub(lead)
                .saturating_sub(rows / 2)
                .min(n_items - rows),
        };
        self.menu_start = start;
        self.menu_rows = rows;
        self.menu_row_h = row_h;
        let x = ((f.width - w) * 0.5).round();
        let y = ((f.height - h) * 0.5).round();
        let radius = CARD_R * s;
        self.text.shadow(
            r,
            scene,
            [x, y, x + w, y + h],
            radius,
            28.0 * s,
            10.0 * s,
            110,
        );
        self.text.rounded(
            r,
            scene,
            [x - 1.0, y - 1.0, x + w + 1.0, y + h + 1.0],
            radius + 1.0,
            BORDER,
        );
        self.text
            .rounded(r, scene, [x, y, x + w, y + h], radius, PANEL);
        // the header: the game's name small, the title large
        let (title, sub): (String, String) = match f.menu_head.as_ref() {
            Some((t, u)) => (t.clone(), u.clone()),
            None => ("Options".to_string(), String::new()),
        };
        self.menu_header(r, scene, x, y, w, header_h, &title, &sub, s);
        let over = |rect: [f32; 4]| {
            f.cursor.0 >= rect[0]
                && f.cursor.0 <= rect[2]
                && f.cursor.1 >= rect[1]
                && f.cursor.1 <= rect[3]
        };
        // the sidebar: the pages, and the way back at its foot
        if side_w > 0.0 {
            let (sx0, sx1) = (x + pad, x + side_w);
            self.text.rounded(
                r,
                scene,
                [sx0 - 1.0, y + header_h - 1.0, sx1 + 1.0, y + h - pad + 1.0],
                CARD_R * s + 1.0,
                BORDER,
            );
            self.text.rounded(
                r,
                scene,
                [sx0, y + header_h, sx1, y + h - pad],
                CARD_R * s,
                PANEL_ALT,
            );
            let inset = 6.0 * s;
            let bottom = y + h - pad - inset;
            let pages_top = y + header_h + inset;
            let step = if f.vr {
                vr_settings_sidebar_step(bottom - 38.0 * s - inset - pages_top, titles.len(), s)
            } else {
                42.0 * s
            };
            let page_h = (step - 4.0 * s).max(1.0);
            let spx = (15.0 * s).min(page_h * 0.6).max(1.0) as u32;
            for (i, title) in titles.iter().enumerate() {
                let top = pages_top + i as f32 * step;
                let rect = [sx0 + inset, top, sx1 - inset, top + page_h];
                let on = i == active;
                let hov = over(rect) && !on;
                // (the page chosen and the page under the mouse fade in and out)
                let a_on = self.easeq(
                    (1, title.as_str(), 0),
                    if on { 1.0 } else { 0.0 },
                    1.0 / FADE_SECS,
                );
                let a_hov = self.easeq(
                    (2, title.as_str(), 0),
                    if hov { 1.0 } else { 0.0 },
                    1.0 / FADE_SECS,
                );
                let a_bar = self.easeq(
                    (9, title.as_str(), 0),
                    if on { 1.0 } else { 0.0 },
                    1.0 / BAR_SECS,
                );
                if a_hov > 0.0 {
                    self.text
                        .rounded(r, scene, rect, ROW_R * s, fade(LIT, a_hov));
                }
                if a_on > 0.0 {
                    self.text
                        .rounded(r, scene, rect, ROW_R * s, fade(SELECTED, a_on));
                }
                if a_bar > 0.0 {
                    self.accent_bar(r, scene, rect, a_bar, false, s);
                }
                let ink = mix(mix(MUTED, SOFT, a_hov), WHITE, a_on);
                let text = clip_to(&self.text, title, spx as f32, rect[2] - rect[0] - tin * 2.0);
                self.put(
                    r,
                    scene,
                    &text,
                    spx,
                    ink,
                    rect[0] + tin,
                    (rect[1] + rect[3]) * 0.5,
                );
                self.menu_side.push(rect);
            }
            let rect = [sx0 + inset, bottom - 38.0 * s, sx1 - inset, bottom];
            let a_back = self.easeq(
                (3, "back", 0),
                if over(rect) { 1.0 } else { 0.0 },
                1.0 / FADE_SECS,
            );
            if a_back > 0.0 {
                self.text
                    .rounded(r, scene, rect, ROW_R * s, fade(LIT, a_back));
            }
            let back = format!("‹  {}", omsi_ui::tr("Back"));
            self.put(
                r,
                scene,
                &back,
                spx,
                mix(MUTED, WHITE, a_back),
                rect[0] + tin,
                (rect[1] + rect[3]) * 0.5,
            );
            self.menu_side.push(rect);
        }
        // the scroll bar of a long page
        let scrolls = n_items > rows;
        let cx0 = x + side_w + if side_w > 0.0 { 8.0 * s } else { pad };
        let cx1 = x + w - pad - if scrolls { 8.0 * s } else { 0.0 };
        if scrolls {
            let top = y + header_h + top_off;
            let track = [
                x + w - 12.0 * s,
                top,
                x + w - 9.0 * s,
                top + row_h * rows as f32 - 4.0 * s,
            ];
            self.menu_scroll_track = Some(track);
            self.text
                .rounded(r, scene, track, 1.5 * s, [255, 255, 255, 22]);
            let th = track[3] - track[1];
            let t0 = track[1] + th * (start - lead) as f32 / n_items as f32;
            let t1 = track[1] + th * (start - lead + rows) as f32 / n_items as f32;
            let thumb = [track[0], t0, track[2], t1];
            self.text.rounded(r, scene, thumb, 1.5 * s, ACCENT);
            self.menu_scroll_thumb =
                Some([thumb[0] - 6.0 * s, thumb[1], thumb[2] + 6.0 * s, thumb[3]]);
        }
        // the rows
        let any_hovered = over([x + side_w, y + header_h + top_off, x + w, y + h - bot_off]);
        let px = ((15.0 * s).min(row_h * 0.34)) as u32;
        if keys_page {
            self.keys_chrome(
                r,
                scene,
                items[0].1,
                [cx0, x + w - pad],
                y + header_h,
                f.cursor,
                tin,
                s,
            );
        }
        // (the light of the row above: the line between two rows is hidden when either is lit)
        let mut prev_a = 0.0f32;
        for (k, &(id, label)) in items.iter().enumerate().skip(start).take(rows) {
            let ry = y + header_h + top_off + row_h * (k - start) as f32;
            let rect = [cx0, ry, cx1, ry + row_h - 4.0 * s];
            // (a group's name on the key bindings page: a small label, nothing to click)
            if keys_page && id == "#" {
                let mut parts = label.split('\u{1f}');
                let name = parts.next().unwrap_or("");
                let _ = parts.next();
                let count = parts.next().unwrap_or("");
                let cy = rect[3] - 12.0 * s;
                self.put(r, scene, name, (12.0 * s) as u32, MUTED, cx0 + tin, cy);
                self.put_right(r, scene, count, (12.0 * s) as u32, MUTED, cx1 - tin, cy);
                self.menu_rects.push([-1.0e9; 4]);
                self.menu_ctl.push(None);
                prev_a = 0.0;
                continue;
            }
            // (lit by the mouse over it; by the keyboard's choice only when the keyboard chose)
            let lit =
                f.dropdown.is_none() && (over(rect) || (k == sel && f.menu_kbd && !any_hovered));
            // the light of the row eases in and out
            let a = self.easeq((4, id, k), if lit { 1.0 } else { 0.0 }, 1.0 / FADE_SECS);
            let a_bar = self.easeq((10, id, k), if lit { 1.0 } else { 0.0 }, 1.0 / BAR_SECS);
            if a > 0.0 {
                self.text.rounded(r, scene, rect, ROW_R * s, fade(LIT, a));
            }
            if a_bar > 0.0 {
                self.accent_bar(r, scene, rect, a_bar, false, s);
            }
            if k > start && a.max(prev_a) < 0.5 {
                let sy = (rect[1] - 2.0 * s).round();
                scene
                    .overlays
                    .push((sep, [rect[0] + tin, sy, rect[2] - tin, sy + 1.0]));
            }
            prev_a = a;
            let ink = mix(SOFT, WHITE, a);
            let cy = (rect[1] + rect[3]) * 0.5;
            let nx = rect[0] + tin;
            let rx = rect[2] - tin;
            let mut parts = label.split('\u{1f}');
            let name = parts.next().unwrap_or("");
            let kind = parts.next().unwrap_or("a");
            let value = parts.next().unwrap_or("");
            let desc = parts.next().unwrap_or("");
            let frac: Option<f32> = parts.next().and_then(|p| p.parse().ok());
            let mut ctl: Option<[f32; 4]> = None;
            let left: f32 = match kind {
                // a switch ("m": the same, on a row that also opens a page: the arrow beside it)
                "s" | "m" => {
                    let on = value == "on";
                    let (tw, th) = (44.0 * s, 24.0 * s);
                    let tx = rx - tw;
                    let track = [tx, cy - th * 0.5, tx + tw, cy + th * 0.5];
                    // (the knob slides over and the track changes its colour)
                    let t = self.ease((5, id, k), if on { 1.0 } else { 0.0 }, 1.0 / FADE_SECS);
                    let tq = quant(t);
                    if tq < 1.0 {
                        self.text.rounded(
                            r,
                            scene,
                            [
                                track[0] - 1.0,
                                track[1] - 1.0,
                                track[2] + 1.0,
                                track[3] + 1.0,
                            ],
                            th * 0.5 + 1.0,
                            fade([255, 255, 255, 44], 1.0 - tq),
                        );
                    }
                    self.text.rounded(
                        r,
                        scene,
                        track,
                        th * 0.5,
                        mix([62, 62, 62, 255], ACCENT, tq),
                    );
                    let kn = 18.0 * s;
                    let kx = tx + 3.0 * s + (tw - kn - 6.0 * s) * t;
                    self.text.rounded(
                        r,
                        scene,
                        [kx, cy - kn * 0.5, kx + kn, cy + kn * 0.5],
                        kn * 0.5,
                        mix([142, 142, 142, 255], [240, 240, 240, 255], tq),
                    );
                    if kind == "m" {
                        let cw = self.put_right(
                            r,
                            scene,
                            "›",
                            px + 6,
                            mix(MUTED, WHITE, a),
                            tx - 12.0 * s,
                            cy,
                        );
                        ctl = Some([tx, rect[1], rx, rect[3]]);
                        tx - 12.0 * s - cw
                    } else {
                        tx
                    }
                }
                // a slider: the track with its knob, the value right of it
                "v" => {
                    let vw = 64.0 * s;
                    self.put_right(
                        r,
                        scene,
                        value,
                        (14.0 * s) as u32,
                        mix(SOFT, WHITE, a),
                        rx,
                        cy,
                    );
                    let tw = (200.0 * s).min((rx - nx) * 0.45);
                    let x1 = rx - vw - 10.0 * s;
                    let x0 = x1 - tw;
                    let th = 4.0 * s;
                    self.text.rounded(
                        r,
                        scene,
                        [x0, cy - th * 0.5, x1, cy + th * 0.5],
                        th * 0.5,
                        [255, 255, 255, 34],
                    );
                    // (the knob glides to its place and grows a little under the mouse)
                    let fr = self.ease(
                        (6, id, k),
                        frac.unwrap_or(0.0).clamp(0.0, 1.0),
                        2.0 / FADE_SECS,
                    );
                    let fx = x0 + tw * fr;
                    if fx - x0 >= 1.0 {
                        self.text.rounded(
                            r,
                            scene,
                            [x0, cy - th * 0.5, fx, cy + th * 0.5],
                            th * 0.5,
                            ACCENT,
                        );
                    }
                    let kn = (13.0 + 4.0 * a) * s;
                    self.text.rounded(
                        r,
                        scene,
                        [fx - kn * 0.5, cy - kn * 0.5, fx + kn * 0.5, cy + kn * 0.5],
                        kn * 0.5,
                        mix([200, 200, 200, 255], [240, 240, 240, 255], a),
                    );
                    ctl = Some([x0, rect[1], x1, rect[3]]);
                    x0
                }
                // a stepper: the value between two arrows, the left half steps back
                "c" => {
                    let text = format!("‹  {value}  ›");
                    let l = self.chip(
                        r,
                        scene,
                        &text,
                        (13.0 * s) as u32,
                        mix(SOFT, WHITE, a),
                        mix(CHIP, SELECTED, a),
                        false,
                        rx,
                        cy,
                        s,
                    );
                    ctl = Some([l, rect[1], rx, rect[3]]);
                    l
                }
                // opens another list
                "o" => {
                    let cw = self.put_right(r, scene, "›", px + 6, mix(MUTED, WHITE, a), rx, cy);
                    if value.is_empty() {
                        rx - cw
                    } else {
                        // (the value now, as the stepper showed it)
                        let vw = self.put_right(
                            r,
                            scene,
                            value,
                            (14.0 * s) as u32,
                            SOFT,
                            rx - cw - 8.0 * s,
                            cy,
                        );
                        rx - cw - 8.0 * s - vw
                    }
                }
                "i" => {
                    let cw = self.put_right(r, scene, value, (14.0 * s) as u32, SOFT, rx, cy);
                    rx - cw
                }
                // a key (or a chord) as key caps, one cap for each part
                "k" => {
                    let unset = value == "Not set";
                    let mut parts: Vec<&str> = Vec::new();
                    let mut rest = value;
                    if !unset {
                        while let Some(p) = ["Shift+", "Ctrl+", "Alt+"]
                            .iter()
                            .find_map(|m| rest.strip_prefix(m).map(|r| (*m, r)))
                        {
                            parts.push(p.0.trim_end_matches('+'));
                            rest = p.1;
                        }
                    }
                    parts.push(rest);
                    let fg = if unset {
                        mix(MUTED, SOFT, a)
                    } else {
                        mix(SOFT, AMBER, a)
                    };
                    let bg = if unset {
                        mix([255, 255, 255, 12], CHIP, a)
                    } else {
                        mix(CHIP, ACCENT_SOFT, a)
                    };
                    let mut cx = rx;
                    for p in parts.iter().rev() {
                        cx = self.chip(r, scene, p, (13.0 * s) as u32, fg, bg, !unset, cx, cy, s)
                            - 5.0 * s;
                    }
                    cx + 5.0 * s
                }
                // a value being typed: lit in the accent
                "E" => self.chip(
                    r,
                    scene,
                    value,
                    (13.0 * s) as u32,
                    AMBER,
                    ACCENT_SOFT,
                    false,
                    rx,
                    cy,
                    s,
                ),
                // a button
                _ => {
                    if value.is_empty() {
                        rx
                    } else {
                        self.chip(
                            r,
                            scene,
                            value,
                            (13.0 * s) as u32,
                            mix(SOFT, AMBER, a),
                            mix(CHIP, ACCENT_SOFT, a),
                            false,
                            rx,
                            cy,
                            s,
                        )
                    }
                }
            };
            let avail = left - 16.0 * s - nx;
            if desc.is_empty() {
                let n = clip_to(&self.text, name, px as f32, avail);
                self.put(r, scene, &n, px, ink, nx, cy);
            } else {
                let n = clip_to(&self.text, name, px as f32, avail);
                self.put(r, scene, &n, px, ink, nx, cy - 9.0 * s);
                let dpx = (12.0 * s) as u32;
                let d = clip_to(&self.text, desc, dpx as f32, avail);
                self.put(r, scene, &d, dpx, MUTED, nx, cy + 10.0 * s);
            }
            self.menu_rects.push(rect);
            self.menu_ctl.push(ctl);
        }
        // a drop-down over a row (the weather preset, the clouds): the entries under the
        // row's value as a select's in a page - over it where there is no room under
        if let Some(dd) = f
            .dropdown
            .as_ref()
            .filter(|d| d.row >= start && d.row < start + rows && !d.items.is_empty())
        {
            let ry = y + header_h + row_h * (dd.row - start) as f32;
            let row_b = ry + row_h - 4.0 * s;
            let item_h = (36.0 * s).min(row_h);
            let inner = 4.0 * s;
            let below = y + h - pad - row_b - 4.0 * s;
            let above = ry - (y + header_h) - 4.0 * s;
            let want = dd.items.len().min(8);
            let fits =
                |room: f32| (((room - 2.0 * inner) / item_h).floor().max(0.0) as usize).min(want);
            let down = fits(below) >= want || fits(below) >= fits(above);
            let n_vis = (if down { fits(below) } else { fits(above) }).max(1);
            let ph = n_vis as f32 * item_h + 2.0 * inner;
            let pw = (300.0 * s).min(cx1 - cx0);
            let (px1, py0) = (
                cx1 - 6.0 * s,
                if down {
                    row_b + 4.0 * s
                } else {
                    ry - 4.0 * s - ph
                },
            );
            let px0 = px1 - pw;
            let panel = [px0, py0, px1, py0 + ph];
            let top = dd.top.min(dd.items.len() - n_vis.min(dd.items.len()));
            self.dd_top = top;
            self.dd_rows = n_vis;
            let rad = (CARD_R * s).min(10.0 * s);
            self.text
                .shadow(r, scene, panel, rad, 18.0 * s, 6.0 * s, 150);
            self.text.rounded(r, scene, panel, rad, [38, 38, 38, 255]);
            let more = dd.items.len() > n_vis;
            let dpx = (14.0 * s) as u32;
            let tin = TEXT_IN * s;
            for i in 0..n_vis {
                let idx = top + i;
                let rect = [
                    px0 + inner,
                    py0 + inner + i as f32 * item_h,
                    px1 - inner - if more { 8.0 * s } else { 0.0 },
                    py0 + inner + (i + 1) as f32 * item_h,
                ];
                let cur = dd.current == Some(idx);
                let hot = over(rect) || (idx == dd.sel && f.menu_kbd && !over(panel));
                let a = self.easeq(
                    (15, "dropdown", idx),
                    if hot { 1.0 } else { 0.0 },
                    1.0 / FADE_SECS,
                );
                if cur {
                    self.text.rounded(r, scene, rect, ROW_R * s, SELECTED);
                }
                if a > 0.0 {
                    self.text.rounded(r, scene, rect, ROW_R * s, fade(LIT, a));
                }
                if cur {
                    self.accent_bar(r, scene, rect, 1.0, false, s);
                }
                let text = clip_to(
                    &self.text,
                    dd.items[idx],
                    dpx as f32,
                    rect[2] - rect[0] - tin * 2.0,
                );
                self.put(
                    r,
                    scene,
                    &text,
                    dpx,
                    if cur { WHITE } else { mix(SOFT, WHITE, a) },
                    rect[0] + tin,
                    (rect[1] + rect[3]) * 0.5,
                );
                self.dd_rects.push(rect);
            }
            if more {
                let track = [px1 - 7.0 * s, py0 + inner, px1 - 4.0 * s, py0 + ph - inner];
                self.text
                    .rounded(r, scene, track, 1.5 * s, [255, 255, 255, 22]);
                let th = track[3] - track[1];
                let n = dd.items.len() as f32;
                self.text.rounded(
                    r,
                    scene,
                    [
                        track[0],
                        track[1] + th * top as f32 / n,
                        track[2],
                        track[1] + th * (top + n_vis) as f32 / n,
                    ],
                    1.5 * s,
                    ACCENT,
                );
            }
        }
    }
}

/// `text` broken into lines of at most `width` pixels, between words.
fn wrap(tc: &TextCache, text: &str, px: f32, width: f32) -> Vec<String> {
    let mut out = Vec::new();
    let mut line = String::new();
    for word in text.split_whitespace() {
        let try_line = if line.is_empty() {
            word.to_string()
        } else {
            format!("{line} {word}")
        };
        if tc.width(&try_line, px) > width && !line.is_empty() {
            out.push(std::mem::take(&mut line));
            line = word.to_string();
        } else {
            line = try_line;
        }
    }
    if !line.is_empty() {
        out.push(line);
    }
    out
}

/// `text` cut at the end to fit `width` pixels ("…").
fn clip_to(tc: &TextCache, text: &str, px: f32, width: f32) -> String {
    if tc.width(text, px) <= width {
        return text.to_string();
    }
    let chars: Vec<char> = text.chars().collect();
    let head = |n: usize| {
        let mut t: String = chars[..n].iter().collect();
        t.push('…');
        t
    };
    let (mut lo, mut hi) = (0usize, chars.len());
    while lo < hi {
        let mid = (lo + hi + 1) / 2;
        if tc.width_raw(&head(mid), px) <= width {
            lo = mid;
        } else {
            hi = mid - 1;
        }
    }
    head(lo)
}

/// `text` cut at the start to fit (the end of what is being typed stays visible).
fn clip_left(tc: &TextCache, text: &str, px: f32, width: f32) -> String {
    let mut t: Vec<char> = text.chars().collect();
    while t.len() > 1 && tc.width(&t.iter().collect::<String>(), px) > width {
        t.remove(0);
    }
    t.into_iter().collect()
}

/// A chat line with its bad words starred out (rustrict: profanity, slurs and the usual
/// ways of writing around a filter, without a word list to keep).
pub fn filter_chat(text: &str) -> String {
    use rustrict::CensorStr;
    text.censor()
}

/// Fit all VR page buttons above the separately reserved Back button.
fn vr_settings_sidebar_step(available: f32, pages: usize, scale: f32) -> f32 {
    (available.max(0.0) / pages.max(1) as f32).min(42.0 * scale)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn vr_settings_sidebar_keeps_back_clear() {
        for scale in [0.5, 1.0, 2.0] {
            for height in [220.0, 440.0] {
                let pages_top = (72.0 + 6.0) * scale;
                let back_top = (height - PAD - 6.0 - 38.0) * scale;
                for pages in [7, 8, 10] {
                    let step =
                        vr_settings_sidebar_step(back_top - 6.0 * scale - pages_top, pages, scale);
                    let last_bottom =
                        pages_top + (pages - 1) as f32 * step + (step - 4.0 * scale).max(1.0);
                    assert!(last_bottom < back_top);
                }
            }
        }
        assert_eq!(vr_settings_sidebar_step(1000.0, 8, 1.0), 42.0);
    }

    #[test]
    fn text_renders_with_an_outline() {
        let f = FontVec::try_from_vec(ROBOTO.to_vec()).unwrap();
        let img = render_text(&f, "Savva: hi", 20.0, [255, 255, 255, 220]);
        assert!(img.width > 40 && img.height > 14);
        // white text and dark outline pixels are both there
        let px: Vec<&[u8]> = img.rgba.chunks(4).collect();
        assert!(px.iter().any(|p| p[3] > 200 && p[0] > 200));
        assert!(px.iter().any(|p| p[3] > 100 && p[0] < 40));
    }

    /// The Esc menu in Chinese, Korean and Thai: the system's fonts, not boxes.
    #[test]
    fn scripts_roboto_lacks_come_from_the_system() {
        let f = FontVec::try_from_vec(ROBOTO.to_vec()).unwrap();
        for t in ["继续", "繼續", "계속", "ดำเนินการต่อ"] {
            if t.chars()
                .next()
                .and_then(omsi_ui::text::fallback_font)
                .is_none()
            {
                continue;
            }
            for c in t.chars() {
                let g = font_for(&f, c);
                assert!(!std::ptr::eq(g, &f) && g.glyph_id(c).0 != 0, "{t}: {c}");
            }
            let img = render_text(&f, t, 20.0, [255, 255, 255, 220]);
            let ink = img
                .rgba
                .chunks(4)
                .filter(|p| p[3] > 128 && p[0] > 128)
                .count();
            assert!(ink > 30, "{t}: {ink}");
        }
    }

    /// Laid out for 1080p: a lower window as it always was, a taller one in proportion.
    #[test]
    fn the_interface_grows_with_tall_windows() {
        assert_eq!(size_factor(900.0, 1.0, 1.0, true), 1.0);
        assert_eq!(size_factor(1080.0, 1.0, 1.0, true), 1.0);
        assert_eq!(size_factor(2160.0, 1.0, 1.0, true), 2.0);
        assert_eq!(size_factor(4320.0, 1.0, 1.0, true), 2.0);
        // (4K at 150 % display scaling is 1440 logical lines: with the screen's scale the
        // same size as at 100 %)
        assert!((1.5 * size_factor(2160.0, 1.5, 1.0, true) - 2.0).abs() < 1e-5);
        assert_eq!(size_factor(1080.0, 1.0, 1.5, true), 1.5);
        assert_eq!(size_factor(2160.0, 1.0, 0.5, true), 1.0);
        // (switched off: the size alone)
        assert_eq!(size_factor(2160.0, 1.0, 1.0, false), 1.0);
        assert_eq!(size_factor(2160.0, 1.0, 1.25, false), 1.25);
    }

    /// A thinned panel's light texts get an outline; dark ones - the highlighted menu line's,
    /// on the solid accent - do not (it came out as a smear round them, like a shadow).
    #[test]
    fn panel_texts_get_an_outline_only_where_it_helps() {
        // (full panels: as asked)
        assert_eq!(outline_for([235, 235, 235, 0], 1.0), 0);
        // (thinned: the light text gets one, the dark text none)
        assert!(outline_for([235, 235, 235, 0], 0.47) > 100);
        assert!(outline_for([140, 140, 140, 0], 0.47) > 100);
        assert_eq!(outline_for([20, 20, 20, 0], 0.47), 0);
        // (texts with an outline of their own keep it)
        assert_eq!(outline_for([255, 255, 255, 235], 0.47), 235);
    }

    /// The opacity's default is the design; below it the backgrounds fade, never quite away.
    #[test]
    fn backgrounds_follow_the_opacity_setting() {
        assert_eq!(backdrop(0.85), 1.0);
        assert!((backdrop(0.425) - 0.5).abs() < 1e-6);
        assert_eq!(backdrop(0.2), 0.3);
        assert!(backdrop(1.0) > 1.0);
    }

    #[test]
    fn chat_filter_stars_out_swearing() {
        assert_ne!(
            filter_chat("you are a fucking idiot"),
            "you are a fucking idiot"
        );
        assert_eq!(
            filter_chat("next stop Rathaus Spandau"),
            "next stop Rathaus Spandau"
        );
    }
}