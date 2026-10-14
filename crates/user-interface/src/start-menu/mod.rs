//! The start menu

use super::pause_menu::{Metrics, inside, out};
use super::*;

mod bar;
mod drivers;
mod pages;

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum StartPage {
    #[default]
    Start,
    Multiplayer,
    Options,
    Profile,
}

#[derive(Clone, Debug, Default)]
pub struct VehRow {
    pub label: String,
    pub count: usize,
    pub open: bool,
    pub vehicle: Option<usize>,
    pub on: bool,
}

#[derive(Clone, Debug, Default)]
pub struct LineRow {
    pub badge: String,
    pub label: String,
}

#[derive(Clone, Debug, Default)]
pub struct TourRow {
    pub label: String,
    pub span: String,
}

#[derive(Clone, Debug, Default)]
pub struct DriveRow {
    pub title: String,
    pub sub: String,
    pub secs: i64,
    pub meters: i64,
    pub ago: String,
}

#[derive(Clone, Debug, Default)]
pub struct DriverMenu {
    pub open: bool,
    pub names: Vec<String>,
    pub current: Option<usize>,
    pub new: Option<String>,
    pub confirm: Option<usize>,
    pub note: String,
}

#[derive(Clone, Debug, Default)]
pub struct ProfileView {
    pub name: String,
    pub level: u32,
    pub xp_to_next: u32,
    pub xp_frac: f32,
    pub secs: i64,
    pub meters: i64,
    pub stops: i64,
    pub on_time: Option<u32>,
    pub ratings: [Option<u32>; 3],
    pub drives: Vec<DriveRow>,
}

#[derive(Clone, Debug, Default)]
pub struct StartUpdate {
    pub kind: u8,
    pub label: String,
    pub progress: f32,
}

pub struct StartView<'a> {
    pub update: &'a StartUpdate,
    pub profile: &'a ProfileView,
    pub drivers: &'a DriverMenu,
    pub page: StartPage,
    pub sel: usize,
    pub can_continue: bool,
    pub maps: &'a [String],
    pub map: usize,
    pub map_top: usize,
    pub rows: &'a [(String, String)],
    pub row: usize,
    pub zone: usize,
    pub address: &'a str,
    pub addr_sel: bool,
    pub caret: bool,
    pub mp_tab: usize,
    pub host: bool,
    pub servers: &'a [(String, String)],
    pub server_top: usize,
    pub server_state: &'a [(u8, String)],
    pub addr_state: u8,
    pub addr_note: &'a str,
    pub maker: &'a str,
    pub vehicle: usize,
    pub vehicles: usize,
    pub vehicle_names: &'a [String],
    pub veh_rows: &'a [VehRow],
    pub veh_top: usize,
    pub query: &'a str,
    pub drop: usize,
    pub drop_items: &'a [String],
    pub drop_sel: usize,
    pub drop_top: usize,
    pub edit: u8,
    pub details: bool,
    pub livery: &'a str,
    pub depot: &'a str,
    pub fleet: &'a str,
    pub plate: &'a str,
    pub sect: usize,
    pub focus: usize,
    pub season: usize,
    pub time_text: &'a str,
    pub cal_open: bool,
    pub cal_title: &'a str,
    pub cal_cells: &'a [(i32, bool, bool)],
    pub weathers: &'a [(String, String)],
    pub weather: usize,
    pub weather_top: usize,
    pub custom: &'a [(String, String)],
    pub icao: &'a str,
    pub custom_fracs: &'a [f32],
    pub icao_rows: &'a [(String, String)],
    pub free: bool,
    pub on_foot: bool,
    pub lines: &'a [LineRow],
    pub line_sel: Option<usize>,
    pub line_top: usize,
    pub line_query: &'a str,
    pub tours: &'a [TourRow],
    pub tour_sel: Option<usize>,
    pub tour_top: usize,
    pub tour_query: &'a str,
    pub trip_text: &'a str,
    pub route_sub: &'a str,
    pub entries: &'a [String],
    pub entry_text: &'a str,
    pub entry_sel: Option<usize>,
    pub entry_top: usize,
}

#[derive(Default)]
pub struct StartUi {
    pub update: StartUpdate,
    pub update_btn: [f32; 4],
    pub active: bool,
    pub hand: bool,
    pub items: Vec<[f32; 4]>,
    pub tabs: Vec<[f32; 4]>,
    pub maps: Vec<[f32; 4]>,
    pub map_vis: usize,
    pub rows: Vec<[f32; 4]>,
    pub arrows: Vec<[f32; 4]>,
    pub mode: Vec<[f32; 4]>,
    pub summary: Vec<[f32; 4]>,
    pub sect_tabs: Vec<[f32; 4]>,
    pub close: [f32; 4],
    pub quit: [f32; 4],
    pub profile_btn: [f32; 4],
    pub drv_box: [f32; 4],
    pub drv_btn: [f32; 4],
    pub drv_open_now: bool,
    pub drv_items: Vec<[f32; 4]>,
    pub drv_dels: Vec<[f32; 4]>,
    pub drv_first: usize,
    pub drv_new_row: [f32; 4],
    pub drv_create: [f32; 4],
    pub driver: String,
    pub bare: bool,
    pub next: [f32; 4],
    pub cont: [f32; 4],
    pub veh: Vec<[f32; 4]>,
    pub veh_vis: usize,
    pub search: [f32; 4],
    pub plate: [f32; 4],
    pub drops: Vec<[f32; 4]>,
    pub drop_items: Vec<[f32; 4]>,
    pub drop_box: [f32; 4],
    /// Last frame's open dropdown box, kept across `clear`.
    pub drop_last: [f32; 4],
    pub drop_pending: Option<[f32; 4]>,
    pub drop_first: usize,
    pub drop_scroll: f32,
    pub view_h: f32,
    pub drop_scroll_for: usize,
    pub details: [f32; 4],
    pub pic_size: (u32, u32),
    pub pic_rect: [f32; 4],
    pub picture: Option<TextureId>,
    pub status: String,
    pub seasons: Vec<[f32; 4]>,
    pub time_field: [f32; 4],
    pub date_field: [f32; 4],
    pub now: [f32; 4],
    pub cal_days: Vec<[f32; 4]>,
    pub cal_btn: Vec<[f32; 4]>,
    pub weathers: Vec<[f32; 4]>,
    pub weather_vis: usize,
    pub weather_modes: Vec<[f32; 4]>,
    pub foot: [f32; 4],
    pub sliders: Vec<[f32; 4]>,
    pub icao_field: [f32; 4],
    pub icao_picks: Vec<[f32; 4]>,
    pub go: [f32; 4],
    pub field: [f32; 4],
    pub mp_tabs: Vec<[f32; 4]>,
    pub host_row: [f32; 4],
    pub servers: Vec<[f32; 4]>,
    pub lines: Vec<[f32; 4]>,
    pub tours: Vec<[f32; 4]>,
    pub line_vis: usize,
    pub tour_vis: usize,
    pub line_box: [f32; 4],
    pub tour_box: [f32; 4],
    pub entries: Vec<[f32; 4]>,
    pub entry_vis: usize,
    pub entry_box: [f32; 4],
    pub line_filter: [f32; 4],
    pub tour_filter: [f32; 4],
    pub route_pic: Option<TextureId>,
    pub route_size: (u32, u32),
    pub route_rect: [f32; 4],
    pub route_status: String,
    pub stops: Vec<(String, f32, f32)>,
    t: f32,
    page_t: f32,
    sect_t: f32,
    enter: f32,
    page_e: f32,
    sect_e: f32,
    prev_sect: usize,
    last_sect: Option<usize>,
    last: Option<StartPage>,
}

impl StartUi {
    fn clear(&mut self) {
        self.items.clear();
        self.tabs.clear();
        self.quit = [0.0; 4];
        self.profile_btn = [0.0; 4];
        self.drv_box = [0.0; 4];
        self.drv_btn = [0.0; 4];
        self.drv_items.clear();
        self.drv_dels.clear();
        self.drv_new_row = [0.0; 4];
        self.drv_create = [0.0; 4];
        self.bare = false;
        self.maps.clear();
        self.rows.clear();
        self.arrows.clear();
        self.mode.clear();
        self.summary.clear();
        self.sect_tabs.clear();
        self.veh.clear();
        self.search = [0.0; 4];
        self.plate = [0.0; 4];
        self.drops.clear();
        self.drop_items.clear();
        self.drop_box = [0.0; 4];
        self.drop_pending = None;
        self.details = [0.0; 4];
        self.seasons.clear();
        self.close = [0.0; 4];
        self.next = [0.0; 4];
        self.cont = [0.0; 4];
        self.time_field = [0.0; 4];
        self.date_field = [0.0; 4];
        self.now = [0.0; 4];
        self.cal_days.clear();
        self.cal_btn.clear();
        self.weathers.clear();
        self.weather_modes.clear();
        self.foot = [0.0; 4];
        self.sliders.clear();
        self.icao_field = [0.0; 4];
        self.icao_picks.clear();
        self.go = [0.0; 4];
        self.field = [0.0; 4];
        self.mp_tabs.clear();
        self.host_row = [0.0; 4];
        self.servers.clear();
        self.lines.clear();
        self.tours.clear();
        self.line_box = [0.0; 4];
        self.tour_box = [0.0; 4];
        self.entries.clear();
        self.entry_box = [0.0; 4];
        self.line_filter = [0.0; 4];
        self.tour_filter = [0.0; 4];
        self.route_rect = [0.0; 4];
        self.pic_rect = [0.0; 4];
        self.update_btn = [0.0; 4];
    }
}

impl Ui {
    pub fn start_reset(&mut self) {
        self.start = StartUi::default();
        self.pause_reset();
    }

    pub fn draw_start(&mut self, r: &Renderer, scene: &mut Scene, f: &Frame, dt: f32, v: &StartView) {
        self.text.backdrop = f.opacity;
        self.anim_dt = dt.clamp(0.0, 0.1);
        self.text.flat = true;
        self.start.active = true;
        let m = Metrics::new(f);
        if self.start.last != Some(v.page) {
            self.start.last = Some(v.page);
            self.start.page_t = 0.0;
            self.pause_reset();
        }
        self.start.page_t = (self.start.page_t + self.anim_dt / 0.3).min(1.0);
        self.start.t = (self.start.t + self.anim_dt / 0.35).min(1.0);
        self.start.clear();
        self.start.update = v.update.clone();
        self.start.driver = v.profile.name.clone();
        self.start.drv_open_now = v.drivers.open;
        let at = |rs: &[[f32; 4]]| rs.iter().any(|rc| inside(*rc, f.cursor));
        match v.page {
            StartPage::Start => {
                self.draw_start_maps(r, scene, f, m, v);
                self.hand = at(&self.start.items)
                    || at(&self.start.maps)
                    || at(&self.start.rows)
                    || at(&self.start.arrows)
                    || at(&self.start.mode)
                    || at(&self.start.summary)
                    || at(&self.start.sect_tabs)
                    || at(&self.start.veh)
                    || at(&[self.start.search, self.start.plate, self.start.details])
                    || at(&self.start.drops)
                    || at(&self.start.drop_items)
                    || at(&self.start.lines)
                    || at(&self.start.tours)
                    || at(&self.start.entries)
                    || at(&[self.start.line_filter, self.start.tour_filter])
                    || at(&self.start.seasons)
                    || at(&[self.start.close, self.start.next, self.start.cont])
                    || at(&[self.start.time_field, self.start.date_field, self.start.now])
                    || at(&self.start.cal_days)
                    || at(&self.start.cal_btn)
                    || at(&self.start.weathers)
                    || at(&self.start.weather_modes)
                    || at(&self.start.sliders)
                    || at(&self.start.icao_picks)
                    || at(&[self.start.icao_field, self.start.foot])
                    || at(&self.start.tabs) || at(&[self.start.go, self.start.quit, self.start.update_btn, self.start.profile_btn]);
            }
            StartPage::Multiplayer => {
                self.draw_start_join(r, scene, f, m, v);
                self.hand = at(&self.start.mp_tabs)
                    || at(&self.start.servers)
                    || at(&self.start.tabs)
                    || at(&[self.start.go, self.start.field, self.start.host_row, self.start.quit, self.start.update_btn, self.start.profile_btn]);
            }
            StartPage::Options => {
                self.text.rounded(r, scene, [0.0, 0.0, m.w, m.h], 0.0, [10, 12, 16, 255]);
                self.draw_pause(r, scene, f, PauseState { page: Some(OPTIONS_PAGE), sel: 0 });
                self.hand |= at(&self.start.tabs) || at(&[self.start.quit, self.start.profile_btn]);
            }
            StartPage::Profile => {
                self.draw_start_profile(r, scene, f, m, v);
                self.hand = at(&self.start.tabs) || at(&[self.start.quit, self.start.update_btn, self.start.profile_btn]);
            }
        }
        self.draw_driver_menu(r, scene, f, m, v);
        self.hand |= at(&self.start.drv_items)
            || at(&self.start.drv_dels)
            || at(&[self.start.drv_btn, self.start.drv_new_row, self.start.drv_create]);
        self.start.hand = self.hand;
        self.text.flat = false;
        self.text.end_frame(r, scene);
    }
}

pub(super) fn head_alpha(pt: f32) -> f32 {
    out(pt / 0.6)
}

pub(super) const BG: [u8; 4] = [10, 12, 16, 255];
pub(super) const ACCENT_TEXT: [u8; 4] = [232, 160, 48, 0];
