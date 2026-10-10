//! The game's own interface, drawn over the picture in Roboto with a dark outline (the
//! HUD's `.oft` fonts are OMSI's and stay for the time and speed): the chat of a LAN
//! session, the name of what the cursor points at next to the cursor, and the other
//! players' name tags above their buses.
//!
//! Every text is rendered once into a small texture and kept while it is shown; the
//! overlays are rectangles in physical pixels (`Scene::overlays`).

use ab_glyph::{Font, FontVec, PxScale, ScaleFont, VariableFont};
use ::render::{Renderer, Scene, TextureId};

mod frame;
#[path = "../pause-menu/mod.rs"]
mod pause_menu;
mod loading;
mod run_report;
mod shapes;
mod style;
#[cfg(test)]
mod tests;
mod text;
mod view;
mod widgets;

pub use self::pause_menu::{Dialog, Fmt, OptGroup, OptKind, OptRow, OptShow, PauseState, WorldDrop, WorldGroup, WorldRow, ADMIN_PAGE, OPTIONS_PAGE, OPTION_GROUPS, PAGE_COUNT, PAUSE_ENTRIES, VEHICLE_PAGE, WORLD_PAGE};
pub use self::view::*;
#[allow(unused_imports)]
use self::{style::*, text::*};

/// Roboto (Apache 2.0), the interface font.
const ROBOTO: &[u8] = include_bytes!("../../../../assets/fonts/Roboto-VariableFont_wdth,wght.ttf");

/// A rendered text: its texture and size in pixels.
#[derive(Clone, Copy)]
struct Label {
    tex: TextureId,
    w: u32,
    h: u32,
    used: u64,
}

impl Label {
    /// Pushes the texture as an overlay with its top left corner at (`x`, `y`).
    fn place(&self, scene: &mut Scene, x: f32, y: f32) {
        scene
            .overlays
            .push((self.tex, [x, y, x + self.w as f32, y + self.h as f32]));
    }
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
    alpha: f32,
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
    pub lab_tabs: Vec<[f32; 4]>,
    pub admin_visible: bool,
    pub pause_items: Vec<[f32; 4]>,
    pub lab_groups: Vec<[f32; 4]>,
    pub lab_actions: Vec<[f32; 4]>,
    pub lab_group: usize,
    pub dialog: Option<Dialog>,
    pub dialog_rects: Vec<[f32; 4]>,
    pub dialog_box: [f32; 4],
    pub dialog_back: bool,
    pub dialog_back_rc: [f32; 4],
    pub dialog_vis: usize,
    pub dialog_pane_rc: [f32; 4],
    pub dialog_pane_top: Option<usize>,
    pub dialog_pane_max: usize,
    pub dialog_bar: Option<[f32; 4]>,
    pub dialog_pane_bar: Option<[f32; 4]>,
    pub dialog_pane_key: String,
    pub dialog_pane_pos: f32,
    pub dialog_list_pos: f32,
    pub dialog_under: Option<Dialog>,
    pub place_rects: Vec<[f32; 4]>,
    pub place_preview_size: (u32, u32),
    pub place_picture: Option<TextureId>,
    pub map_rect: [f32; 4],
    pub map_picture: Option<TextureId>,
    pub map_btn: [f32; 4],
    pub map_btn_label: String,
    pub map_btn_on: bool,
    pub dialog_preview: Option<Preview>,
    pub dialog_tall: bool,
    pub pause_entries: Vec<String>,
    pub pause_entry_ids: Vec<&'static str>,
    pub world_view: std::sync::Arc<Vec<WorldGroup>>,
    pub world_view_page: usize,
    pub world_last_page: usize,
    pub world_groups_rc: Vec<[f32; 4]>,
    pub world_rows_rc: Vec<[f32; 4]>,
    pub world_tracks: Vec<Option<[f32; 4]>>,
    /// The small "x" behind a key binding: (row, its rectangle).
    pub world_clear: Vec<(usize, [f32; 4])>,
    pub world_group: usize,
    pub world_sub: usize,
    pub world_sub_rc: Vec<[f32; 4]>,
    pub world_scroll: usize,
    pub world_pos: f32,
    pub world_bar: Option<([f32; 4], [f32; 4])>,
    pub world_bar_grab: Option<f32>,
    pub world_first: usize,
    pub world_max: usize,
    pub world_drag: Option<(usize, [f32; 4])>,
    pub world_drop: Option<WorldDrop>,
    pub world_drop_rc: Vec<[f32; 4]>,
    pub world_drop_first: usize,
    pub world_drop_vis: usize,
    pub place_status: String,
    pub hand: bool,
    pub place_box: [f32; 4],
    dialog_t: f32,
    dialog_ghost: Option<Dialog>,
    pause_prev: Option<PauseState>,
    pause_closing: bool,
    pause_open: f32,
    page_t: f32,
    pause_last: usize,
    page_fade: bool,
    cat_key: usize,
    cat_t: f32,
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
    /// The loading screen's full-screen picture
    /// TODO: Add support for many pictures and randomize them on start-up
    loading_art: Option<Option<(TextureId, u32, u32)>>,
    /// The wordmark bottom left (`assets/logos/wordmark-gradient-dark.png`, cut to its content).
    loading_logo: Option<Option<(TextureId, u32, u32)>>,
    /// The wordmark cut to its content, full size, and its renderings at exact pixel heights.
    logo_src: Option<image::RgbaImage>,
    logo_cache: Vec<(u32, TextureId, u32)>,
    /// The loading screen's spinner: one texture per turn of 1/24.
    spinner: Vec<TextureId>,
}
