mod frame;
mod key;
pub(crate) mod minimap;
mod mouse;
mod route;
mod update;

use super::*;
use crate::menu::Probe;
use crate::ui::{LineRow, PauseState, StartPage, StartView, TourRow, OPTIONS_PAGE};

fn clipboard_text() -> Option<String> {
    arboard::Clipboard::new().ok()?.get_text().ok()
}

fn clipboard_set(text: &str) {
    if let Ok(mut c) = arboard::Clipboard::new() {
        let _ = c.set_text(text.to_string());
    }
}

fn hit(list: &[[f32; 4]], (x, y): (f32, f32)) -> Option<usize> {
    list.iter().position(|r| x >= r[0] && x < r[2] && y >= r[1] && y < r[3])
}

const DOUBLE_CLICK: f32 = 0.45;

impl App {
    fn start_page(&self) -> Option<StartPage> {
        self.menu.as_ref().map(|m| m.page)
    }

    fn start_goto(&mut self, page: StartPage, sel: usize) {
        if let Some(m) = self.menu.as_mut() {
            m.page = page;
            m.sel = sel;
        }
    }

    fn start_show_map(&mut self) {
        let vis = self.ui.as_ref().map_or(8, |u| u.start.map_vis).max(1);
        if let Some(m) = self.menu.as_mut() {
            if m.map < m.map_top {
                m.map_top = m.map;
            } else if m.map >= m.map_top + vis {
                m.map_top = m.map + 1 - vis;
            }
        }
    }

    fn start_show_veh(&mut self) {
        let vis = self.ui.as_ref().map_or(8, |u| u.start.veh_vis).max(1);
        if let Some(m) = self.menu.as_mut() {
            let at = m.veh_row_of_selected();
            if at < m.veh_top {
                m.veh_top = at;
            } else if at >= m.veh_top + vis {
                m.veh_top = at + 1 - vis;
            }
        }
    }

    fn start_show_weather(&mut self) {
        let vis = self.ui.as_ref().map_or(8, |u| u.start.weather_vis).max(1);
        if let Some(m) = self.menu.as_mut() {
            m.weather_top = m.weather_top.max(4);
            if m.weather >= 4 && m.weather < m.weather_top {
                m.weather_top = m.weather;
            } else if m.weather >= m.weather_top + vis {
                m.weather_top = m.weather + 1 - vis;
            }
        }
    }

    fn start_show_lists(&mut self) {
        let (vl, vt) = self.ui.as_ref().map_or((6, 6), |u| (u.start.line_vis.max(1), u.start.tour_vis.max(1)));
        if let Some(m) = self.menu.as_mut() {
            let at = m.line_rows().iter().position(|r| r.0 == m.line).unwrap_or(0);
            if at < m.line_top {
                m.line_top = at;
            } else if at >= m.line_top + vl {
                m.line_top = at + 1 - vl;
            }
            let at = m.tour_rows().iter().position(|r| r.0 == m.tour).unwrap_or(0);
            if at < m.tour_top {
                m.tour_top = at;
            } else if at >= m.tour_top + vt {
                m.tour_top = at + 1 - vt;
            }
        }
    }

    fn start_open(&mut self, sect: usize) {
        if let Some(m) = self.menu.as_mut() {
            m.open_sect(sect);
        }
        self.start_show_map();
        self.start_show_veh();
    }

    fn start_back(&mut self) {
        if self.menu.as_ref().is_some_and(|m| m.page == StartPage::Start && m.sect != 0) {
            if let Some(m) = self.menu.as_mut() {
                m.open_sect(0);
            }
            return;
        }
        match self.start_page() {
            Some(StartPage::Start) => {}
            Some(StartPage::Multiplayer) => self.start_set_page(StartPage::Start),
            Some(StartPage::Options) => self.start_options_close(),
            _ => {}
        }
    }

    fn start_options_leave(&mut self) {
        self.restart_pending = false;
        self.key_capture = None;
        self.key_search_stop();
        self.lab_menu = None;
        self.lab_list = None;
        if let Some(u) = self.ui.as_mut() {
            u.world_drop = None;
            u.world_drag = None;
            u.world_bar_grab = None;
            u.dialog = None;
        }
    }

    pub(crate) fn start_options_close(&mut self) {
        self.start_options_leave();
        self.start_goto(StartPage::Start, 0);
    }

    fn start_set_page(&mut self, page: StartPage) {
        if self.start_page() == Some(StartPage::Options) && page != StartPage::Options {
            self.start_options_leave();
        }
        if page == StartPage::Options {
            self.lab_map_direct = false;
            self.menu_kbd = true;
            self.lab_menu = Some(PauseState { page: Some(OPTIONS_PAGE), sel: 0 });
        }
        let sel = match page {
            StartPage::Multiplayer => 2,
            StartPage::Options => 3,
            _ => 0,
        };
        self.start_goto(page, sel);
        if page == StartPage::Start {
            if let Some(m) = self.menu.as_mut() {
                m.open_sect(0);
            }
            self.start_show_map();
        }
    }

    pub(crate) fn start_tab(&mut self, dir: i32) {
        const TABS: [StartPage; 3] = [StartPage::Start, StartPage::Multiplayer, StartPage::Options];
        let Some(i) = self.start_page().and_then(|p| TABS.iter().position(|t| *t == p)) else {
            return;
        };
        self.start_set_page(TABS[(i as i32 + dir).rem_euclid(3) as usize]);
    }

    fn start_launch(&mut self, event_loop: &ActiveEventLoop, situation: Option<String>, join: Option<String>) {
        let Some(m) = self.menu.as_ref() else {
            return;
        };
        let host = m.host;
        self.args.map = m.maps.get(m.map).map(|x| x.1.clone()).unwrap_or(self.args.map.clone());
        self.args.bus = if m.on_foot { None } else { m.vehicles.get(m.vehicle).map(|x| x.1.clone()) };
        self.args.on_foot = m.on_foot;
        self.args.paint = match m.livery {
            0 => None,
            i => m.liveries.get(i - 1).cloned(),
        };
        self.args.hof = match m.depot {
            0 => None,
            i => m.depots.get(i - 1).map(|d| d.1.clone()),
        };
        self.args.number = match m.number {
            0 => None,
            i => m.numbers.get(i - 1).cloned(),
        };
        self.args.plate = Some(m.plate.trim().to_string()).filter(|p| !p.is_empty());
        self.args.time = format!("{:02}:{:02}", m.hour, m.minute);
        self.args.traffic = m.traffic;
        self.args.passengers = m.passengers;
        self.args.schedule = m.schedule;
        self.args.day_of_year = Some(m.day);
        let (y, mo, d) = m.ymd();
        self.args.date = Some(format!("{y}-{mo}-{d}"));
        self.args.season = m.season_arg();
        self.args.weather = m.weather_arg();
        if situation.is_none() && join.is_none() {
            m.remember();
        }
        self.args.trip = None;
        self.args.whole_tour = false;
        if m.route {
            if let Some((l, t)) = m.chosen_tour() {
                self.args.line = Some(l.name.clone());
                self.args.tour = Some(t.number.clone());
                self.args.trip = m.trip_arg();
                self.args.whole_tour = self.args.trip.is_some();
                self.args.schedule = true;
                self.args.auto_entry = true;
            }
        } else if !m.entry_rows().is_empty() {
            self.args.entry = m.entry_shown() as usize;
            self.args.auto_entry = false;
        }
        let chosen = match situation {
            Some(file) => Some(file),
            None => None,
        };
        if let Some(file) = chosen {
            self.args.situation = Some(file);
            if let Err(e) = apply_situation(&mut self.args) {
                log::error!("{e:#}");
            }
        }
        self.args.lan_host = if host && join.is_none() { Some(0) } else { None };
        if let Some(addr) = join {
            let addr = addr.trim();
            self.args.lan_join = Some(if addr.is_empty() { "auto".to_string() } else { addr.to_string() });
        }
        if let Some(t) = self.args.lan_join.clone().filter(|t| ::network::official::is_alias(t)) {
            match ::network::official::resolve_target(&t) {
                Ok(url) => self.args.lan_join = Some(url),
                Err(e) => log::warn!("LAN: {e}"),
            }
        }
        self.lan = None;
        self.lan = crate::lan::start(&self.args);
        if let Some(l) = self.lan.as_mut() {
            crate::lan::share_mods(&mut self.args, l);
            crate::lan::take_host_map(&mut self.args, l);
            if l.role == ::network::Role::Host {
                l.clock_speed = if ::config::get_bool("gameplay", "time_sync").unwrap_or(false) {
                    1.0
                } else {
                    ::config::get_float("gameplay", "time_speed").unwrap_or(1.0).clamp(1.0, 30.0)
                };
            }
        }
        if self.args.lan_join.is_some() {
            if let Some(t) = self.lan.as_ref().and_then(crate::lan::host_time_now) {
                self.args.time = t;
            }
        }
        self.menu = None;
        self.hud = None;
        self.lab_menu = None;
        self.lab_room = None;
        if let (Some((id, _, _)), Some(r), Some(scene)) = (self.lab_pic.take(), self.renderer.as_ref(), self.scene.as_mut()) {
            r.free_texture(scene, id);
        }
        if let Some(u) = self.ui.as_mut() {
            u.start_reset();
        }
        self.load_world_now(event_loop);
    }

    fn start_join(&mut self, event_loop: &ActiveEventLoop) {
        let addr = self.menu.as_ref().map(|m| m.address.trim().to_string()).unwrap_or_default();

        if addr.is_empty() || ::network::SessionCode::decode(&addr).is_ok() {
            self.start_launch(event_loop, None, Some(addr));
            return;
        }
        let Some(m) = self.menu.as_mut() else {
            return;
        };
        match m.probe_of(&addr) {
            Some(Probe::Online(_)) => self.start_launch(event_loop, None, Some(addr)),
            Some(Probe::Checking) => {}
            _ => {
                m.probe(&addr, 0.0);
                m.join_pending = true;
            }
        }
    }

    fn start_join_server(&mut self, event_loop: &ActiveEventLoop, i: usize) {
        let first = self.menu.as_ref().map_or(0, |m| m.server_top);
        let Some(m) = self.menu.as_ref() else {
            return;
        };
        let Some(addr) = m.servers.get(first + i).map(|s| s.1.clone()) else {
            return;
        };
        match m.probe_of(&addr) {
            Some(Probe::Online(_)) => self.start_launch(event_loop, None, Some(addr)),
            Some(Probe::Checking) => {}
            _ => m.probe(&addr, 0.0),
        }
    }
}
