use super::*;

impl App {
    pub(crate) fn start_text(&mut self, text: &str) {
        let ctrl = self.start_ctrl();
        if !ctrl && self.drv_text(text) {
            return;
        }
        if let Some(m) = self.menu.as_mut().filter(|m| m.page == StartPage::Multiplayer && m.mp_tab == 0 && m.addr_focus && !ctrl) {
            let clean: String = text.chars().filter(|c| !c.is_control()).collect();
            if !clean.is_empty() && m.addr_sel {
                m.address.clear();
                m.addr_sel = false;
            }
            m.address.push_str(&clean);
            m.address = m.address.chars().take(200).collect();
        }
        if let Some(m) = self.menu.as_mut().filter(|m| m.page == StartPage::Start && m.edit != 0 && !ctrl) {
            let clean: String = text.chars().filter(|c| !c.is_control()).collect();
            let (buf, max) = m.edit_buf();
            buf.push_str(&clean);
            *buf = buf.chars().take(max).collect();
            m.edit_changed();
        }
    }

    pub(crate) fn start_wheel(&mut self, amount: f32) {
        let (vm, vv) = self.ui.as_ref().map_or((8, 8), |u| (u.start.map_vis, u.start.veh_vis));
        let (vl, vt, lbox, tbox) = self.ui.as_ref().map_or((6, 6, [0.0; 4], [0.0; 4]), |u| {
            (u.start.line_vis, u.start.tour_vis, u.start.line_box, u.start.tour_box)
        });
        let cursor = self.ui_cursor();
        let pic = self.ui.as_ref().map_or([0.0; 4], |u| u.start.route_rect);
        let dbox = self.ui.as_ref().map_or([0.0; 4], |u| u.start.drop_box);
        let (vw, tfield) = self.ui.as_ref().map_or((8, [0.0; 4]), |u| (u.start.weather_vis, u.start.time_field));
        let vi = self.ui.as_ref().map_or(6, |u| u.start.icao_vis);
        let (ve, ebox) = self.ui.as_ref().map_or((6, [0.0; 4]), |u| (u.start.entry_vis, u.start.entry_box));
        if let Some(m) = self.menu.as_mut().filter(|m| m.page == StartPage::Start) {
            match m.sect {
                1 | 2 | 3 if m.drop != 0 && hit(&[dbox], cursor).is_some() => {
                    let max = m.drop_items(m.drop).len().saturating_sub(6) as f32;
                    m.drop_top = (m.drop_top as f32 - amount * 2.0).clamp(0.0, max).round() as usize;
                }
                1 if hit(&[pic], cursor).is_some() => {
                    let old = m.zoom;
                    m.zoom = (m.zoom * 1.25f32.powf(amount)).clamp(1.0, 80.0);
                    let f = m.zoom / old;
                    m.pan = (m.pan.0 * f, m.pan.1 * f);
                }
                1 if !m.route && hit(&[ebox], cursor).is_some() => {
                    let max = m.entry_rows().len().saturating_sub(ve) as f32;
                    m.entry_top = (m.entry_top as f32 - amount * 2.0).clamp(0.0, max).round() as usize;
                }
                1 if m.route && hit(&[lbox], cursor).is_some() => {
                    let max = m.line_rows().len().saturating_sub(vl) as f32;
                    m.line_top = (m.line_top as f32 - amount * 2.0).clamp(0.0, max).round() as usize;
                }
                1 if m.route && hit(&[tbox], cursor).is_some() => {
                    let max = m.tour_rows().len().saturating_sub(vt) as f32;
                    m.tour_top = (m.tour_top as f32 - amount * 2.0).clamp(0.0, max).round() as usize;
                }
                1 => {
                    let max = m.maps.len().saturating_sub(vm) as f32;
                    m.map_top = (m.map_top as f32 - amount * 2.0).clamp(0.0, max).round() as usize;
                }
                2 => {
                    let max = m.veh_rows().len().saturating_sub(vv) as f32;
                    m.veh_top = (m.veh_top as f32 - amount * 2.0).clamp(0.0, max).round() as usize;
                }
                3 if hit(&[tfield], cursor).is_some() => {
                    let dir = if amount > 0.0 { 1 } else { -1 };
                    if cursor.0 < (tfield[0] + tfield[2]) * 0.5 {
                        m.step(1, dir);
                    } else {
                        m.step_minute(dir);
                    }
                }
                3 if m.weather == 2 => {
                    let max = m.icao_matches(5).len().saturating_sub(vi) as f32;
                    m.icao_top = (m.icao_top as f32 - amount * 2.0).clamp(0.0, max).round() as usize;
                }
                3 => {
                    let max = m.weathers.len().saturating_sub(vw) as f32;
                    m.weather_top = (m.weather_top as f32 - amount * 2.0).clamp(0.0, max).round() as usize;
                }
                _ => {}
            }
        }
    }

    pub(crate) fn start_mouse(&mut self, event_loop: &ActiveEventLoop, pressed: bool) {
        let Some(page) = self.start_page() else {
            return;
        };
        let cursor = self.ui_cursor();
        if page == StartPage::Options {
            if !pressed {
                self.lab_bar_drag = 0;
                if let Some(u) = self.ui.as_mut() {
                    u.world_drag = None;
                    u.world_bar_grab = None;
                }
                return;
            }
            if self.drv_click(cursor) {
                return;
            }
            if self.ui.as_ref().is_some_and(|u| hit(&[u.start.quit], cursor).is_some()) {
                crate::platform::exit(event_loop);
                return;
            }
            let tab = self.ui.as_ref().and_then(|u| hit(&u.start.tabs, cursor));
            if let Some(k) = tab {
                self.start_set_page([StartPage::Start, StartPage::Multiplayer, StartPage::Options][k]);
            } else if !self.lab_bar_press() {
                self.lab_click(event_loop);
            }
            return;
        }
        if !pressed {
            let click = self
                .menu
                .as_ref()
                .filter(|m| m.sect == 1)
                .and_then(|m| m.drag)
                .is_some_and(|(from, _)| (cursor.0 - from.0).hypot(cursor.1 - from.1) < 5.0);
            if click {
                self.start_entry_click(cursor);
            }
            if let Some(m) = self.menu.as_mut() {
                m.drag = None;
                m.slider = None;
            }
            return;
        }
        if self.drv_click(cursor) {
            return;
        }
        let Some(u) = self.ui.as_ref() else {
            return;
        };
        let st = &u.start;
        if hit(&[st.quit], cursor).is_some() {
            crate::platform::exit(event_loop);
            return;
        }
        if hit(&[st.update_btn], cursor).is_some() {
            self.start_update_click();
            return;
        }
        if let Some(k) = hit(&st.tabs, cursor) {
            self.start_set_page([StartPage::Start, StartPage::Multiplayer, StartPage::Options][k]);
            return;
        }
        match page {
            StartPage::Start => {
                if hit(&[st.route_rect], cursor).is_some() {
                    if let Some(m) = self.menu.as_mut().filter(|m| m.sect == 1) {
                        m.drag = Some((cursor, m.pan));
                        return;
                    }
                }
                enum Act {
                    DropBar,
                    Go,
                    Cont,
                    Close,
                    Tab(usize),
                    Next,
                    Sum(usize),
                    Mode(usize),
                    Foot,
                    Season(usize),
                    TimeField,
                    DateField,
                    Now,
                    Cal(usize),
                    CalDay(usize),
                    Weather(usize),
                    WeatherMode(usize),
                    Slider(usize),
                    IcaoField,
                    MetarSync,
                    IcaoPick(usize),
                    Arrow(usize),
                    Row(usize),
                    Veh(usize),
                    Drop(usize),
                    DropItem(usize),
                    Search,
                    Plate,
                    Details,
                    Map(usize),
                    Entry(usize),
                    Line(usize),
                    Tour(usize),
                    LineFilter,
                    TourFilter,
                    Nothing,
                }
                let (n_maps, n_veh, mtop, vtop) = self
                    .menu
                    .as_ref()
                    .map_or((0, 0, 0, 0), |m| (m.maps.len(), m.veh_rows().len(), m.map_top, m.veh_top));
                let (n_lines, n_tours, ltop, ttop) = self
                    .menu
                    .as_ref()
                    .filter(|m| m.route)
                    .map_or((0, 0, 0, 0), |m| (m.line_rows().len(), m.tour_rows().len(), m.line_top, m.tour_top));
                let first_map = mtop.min(n_maps.saturating_sub(st.map_vis));
                let first_veh = vtop.min(n_veh.saturating_sub(st.veh_vis));
                let first_line = ltop.min(n_lines.saturating_sub(st.line_vis));
                let first_tour = ttop.min(n_tours.saturating_sub(st.tour_vis));
                let first_entry = self.menu.as_ref().filter(|m| !m.route).map_or(0, |m| {
                    m.entry_top.min(m.entry_rows().len().saturating_sub(st.entry_vis))
                });
                let first_weather = self
                    .menu
                    .as_ref()
                    .map_or(0, |m| m.weather_top.min(m.weathers.len().saturating_sub(st.weather_vis)).max(4.min(m.weathers.len())));
                let act = if st.drop_box[2] > st.drop_box[0] && hit(&[st.drop_box], cursor).is_some() {
                    match hit(&st.drop_items, cursor) {
                        Some(k) => Act::DropItem(st.drop_first + k),
                        None => Act::DropBar,
                    }
                } else if hit(&[st.go], cursor).is_some() {
                    Act::Go
                } else if hit(&[st.cont], cursor).is_some() {
                    Act::Cont
                } else if hit(&[st.close], cursor).is_some() {
                    Act::Close
                } else if let Some(k) = hit(&st.sect_tabs, cursor) {
                    Act::Tab(k)
                } else if hit(&[st.next], cursor).is_some() {
                    Act::Next
                } else if let Some(k) = hit(&st.cal_btn, cursor) {
                    Act::Cal(k)
                } else if let Some(k) = hit(&st.cal_days, cursor) {
                    Act::CalDay(k)
                } else if hit(&[st.time_field], cursor).is_some() {
                    Act::TimeField
                } else if hit(&[st.date_field], cursor).is_some() {
                    Act::DateField
                } else if hit(&[st.now], cursor).is_some() {
                    Act::Now
                } else if let Some(k) = hit(&st.drop_items, cursor) {
                    Act::DropItem(st.drop_first + k)
                } else if let Some(k) = hit(&st.drops, cursor) {
                    Act::Drop(k)
                } else if hit(&[st.foot], cursor).is_some() {
                    Act::Foot
                } else if let Some(k) = hit(&st.sliders, cursor) {
                    Act::Slider(k)
                } else if let Some(k) = hit(&st.weather_modes, cursor) {
                    Act::WeatherMode(k)
                } else if hit(&[st.icao_field], cursor).is_some() {
                    Act::IcaoField
                } else if hit(&[st.metar_sync], cursor).is_some() {
                    Act::MetarSync
                } else if let Some(k) = hit(&st.icao_picks, cursor) {
                    Act::IcaoPick(k)
                } else if let Some(k) = hit(&st.weathers, cursor) {
                    Act::Weather(first_weather + k)
                } else if let Some(k) = hit(&st.summary, cursor) {
                    Act::Sum(k)
                } else if let Some(k) = hit(&st.mode, cursor) {
                    Act::Mode(k)
                } else if let Some(k) = hit(&st.seasons, cursor) {
                    Act::Season(k)
                } else if let Some(j) = hit(&st.arrows, cursor) {
                    Act::Arrow(j)
                } else if let Some(i) = hit(&st.rows, cursor) {
                    Act::Row(i)
                } else if hit(&[st.line_filter], cursor).is_some() {
                    Act::LineFilter
                } else if hit(&[st.tour_filter], cursor).is_some() {
                    Act::TourFilter
                } else if let Some(k) = hit(&st.lines, cursor) {
                    Act::Line(first_line + k)
                } else if let Some(k) = hit(&st.tours, cursor) {
                    Act::Tour(first_tour + k)
                } else if hit(&[st.search], cursor).is_some() {
                    Act::Search
                } else if hit(&[st.plate], cursor).is_some() {
                    Act::Plate
                } else if hit(&[st.details], cursor).is_some() {
                    Act::Details
                } else if let Some(k) = hit(&st.veh, cursor) {
                    Act::Veh(first_veh + k)
                } else if let Some(k) = hit(&st.entries, cursor) {
                    Act::Entry(first_entry + k)
                } else if let Some(k) = hit(&st.maps, cursor) {
                    Act::Map(first_map + k)
                } else {
                    Act::Nothing
                };
                let sect = self.menu.as_ref().map_or(0, |m| m.sect);
                if let Some(m) = self.menu.as_mut() {
                    if m.cal_open && !matches!(act, Act::Cal(_) | Act::CalDay(_) | Act::DateField) {
                        m.cal_open = false;
                    }
                    if m.drop != 0 && !matches!(act, Act::Drop(_) | Act::DropItem(_) | Act::DropBar) {
                        m.drop = 0;
                    }
                    if matches!(act, Act::TimeField) {
                        m.time_buf.clear();
                    }
                    m.edit = match act {
                        Act::TimeField => 5,
                        Act::IcaoField => 6,
                        Act::Search => 1,
                        Act::Plate => 2,
                        Act::LineFilter => 3,
                        Act::TourFilter => 4,
                        _ => 0,
                    };
                }
                match act {
                    Act::Go => self.start_launch(event_loop, None, None),
                    Act::Cont => {
                        let file = self.menu.as_ref().and_then(|m| m.continue_sit.clone());
                        if let Some(file) = file {
                            self.start_launch(event_loop, Some(file), None);
                        }
                    }
                    Act::Close => self.start_open(0),
                    Act::Tab(k) | Act::Sum(k) => self.start_open(k + 1),
                    Act::Next => self.start_open((sect + 1).min(3)),
                    Act::Foot => {
                        if let Some(m) = self.menu.as_mut() {
                            m.on_foot = !m.on_foot;
                            ::config::set_setting("gameplay", "start_on_foot", m.on_foot);
                            let _ = ::config::save();
                        }
                    }
                    Act::Mode(_) => {
                        if let Some(m) = self.menu.as_mut() {
                            m.route = !m.route;
                            m.zone = 0;
                            m.situation = 0;
                            ::config::set_setting("start_menu", "free_drive", !m.route);
                            let _ = ::config::save();
                        }
                    }
                    Act::Season(k) => {
                        if let Some(m) = self.menu.as_mut() {
                            m.season_mode = k.min(4);
                        }
                    }
                    Act::TimeField => {}
                    Act::DateField => {
                        if let Some(m) = self.menu.as_mut() {
                            m.cal_toggle();
                        }
                    }
                    Act::Now => {
                        if let Some(m) = self.menu.as_mut() {
                            m.set_now();
                        }
                    }
                    Act::Cal(k) => {
                        if let Some(m) = self.menu.as_mut() {
                            match k {
                                0 => m.cal_shift(-1),
                                1 => m.cal_shift(1),
                                2 => m.cal_open = false,
                                _ => {
                                    m.set_now();
                                    m.cal_open = false;
                                }
                            }
                        }
                    }
                    Act::CalDay(k) => {
                        if let Some(m) = self.menu.as_mut() {
                            m.cal_pick(k);
                        }
                    }
                    Act::WeatherMode(k) => {
                        if let Some(m) = self.menu.as_mut() {
                            m.weather = k.min(3);
                            m.zone = 0;
                        }
                    }
                    Act::IcaoField => {}
                    Act::MetarSync => {
                        if let Some(m) = self.menu.as_mut() {
                            m.metar_live = !m.metar_live;
                        }
                    }
                    Act::Slider(k) => {
                        let rc = self.ui.as_ref().and_then(|u| u.start.sliders.get(k).copied());
                        if let (Some(rc), Some(m)) = (rc, self.menu.as_mut()) {
                            m.slider = Some(k);
                            m.zone = 1;
                            m.row = 12 + k;
                            m.custom_set(k, (cursor.0 - rc[0]) / (rc[2] - rc[0]).max(1.0));
                        }
                    }
                    Act::IcaoPick(k) => {
                        if let Some(m) = self.menu.as_mut() {
                            if let Some((code, _)) = m.icao_matches(5).get(m.icao_top + k) {
                                m.icao = code.clone();
                            }
                        }
                    }
                    Act::Weather(i) => {
                        if let Some(m) = self.menu.as_mut() {
                            if i < m.weathers.len() {
                                m.weather = i;
                            }
                            m.zone = 0;
                        }
                    }
                    Act::Arrow(j) => {
                        if let Some(m) = self.menu.as_mut() {
                            m.zone = 1;
                            m.row = j / 2;
                            m.step(j / 2, if j % 2 == 0 { -1 } else { 1 });
                        }
                        self.start_show_veh();
                    }
                    Act::Row(i) => {
                        if let Some(m) = self.menu.as_mut() {
                            m.zone = 1;
                            m.row = i;
                            m.step(i, 1);
                        }
                    }
                    Act::Veh(i) => {
                        if let Some(m) = self.menu.as_mut() {
                            m.veh_pick(i);
                            m.zone = 0;
                        }
                    }
                    Act::Drop(k) => {
                        if let Some(m) = self.menu.as_mut() {
                            let route = m.route;
                            m.drop_toggle(match sect {
                                1 if k > 0 && route => 16,
                                1 => 11 + k,
                                3 => 13 + k,
                                _ => 8 + k,
                            });
                        }
                    }
                    Act::DropBar => {
                        let bx = self.ui.as_ref().map_or([0.0; 4], |u| u.start.drop_box);
                        if cursor.0 >= bx[2] - ((bx[2] - bx[0]) * 0.06).max(16.0) {
                            if let Some(m) = self.menu.as_mut() {
                                m.slider = Some(usize::MAX);
                            }
                            self.start_slider_drag();
                        }
                    }
                    Act::DropItem(i) => {
                        if let Some(m) = self.menu.as_mut() {
                            let kind = m.drop;
                            if kind != 0 && i < m.drop_items(kind).len() {
                                m.drop_pick(kind, i);
                            }
                        }
                    }
                    Act::Search | Act::Plate | Act::LineFilter | Act::TourFilter => {}
                    Act::Line(pos) => {
                        if let Some(m) = self.menu.as_mut() {
                            if let Some(i) = m.line_rows().get(pos).map(|r| r.0) {
                                m.pick_line(i);
                                m.fix_filters();
                            }
                            m.zone = 2;
                        }
                    }
                    Act::Tour(pos) => {
                        if let Some(m) = self.menu.as_mut() {
                            if let Some(i) = m.tour_rows().get(pos).map(|r| r.0) {
                                m.pick_tour(i);
                            }
                            m.zone = 3;
                        }
                    }
                    Act::Entry(pos) => {
                        if let Some(m) = self.menu.as_mut() {
                            if let Some(i) = m.entry_rows().get(pos).map(|r| r.0) {
                                m.entry_pick(i);
                            }
                            m.zone = 4;
                        }
                    }
                    Act::Details => {
                        if let Some(m) = self.menu.as_mut() {
                            m.details = !m.details;
                        }
                    }
                    Act::Map(i) => {
                        let now = std::time::Instant::now();
                        let Some(m) = self.menu.as_mut() else {
                            return;
                        };
                        let twice = m
                            .clicked
                            .is_some_and(|(t, last)| last == i && t.elapsed().as_secs_f32() < DOUBLE_CLICK);
                        m.map = i;
                        m.zone = 0;
                        m.clicked = Some((now, i));
                        if twice {
                            self.start_launch(event_loop, None, None);
                        }
                    }
                    Act::Nothing => {}
                }
            }
            StartPage::Multiplayer => {
                let tab = hit(&st.mp_tabs, cursor);
                let server = hit(&st.servers, cursor);
                let go = hit(&[st.go], cursor).is_some();
                let host = hit(&[st.host_row], cursor).is_some();
                let tab0 = self.menu.as_ref().is_some_and(|m| m.mp_tab == 0);
                let on_field = tab0 && hit(&[st.field], cursor).is_some();
                if let Some(m) = self.menu.as_mut() {
                    m.addr_focus = on_field;
                    if !on_field {
                        m.addr_sel = false;
                    }
                }
                if let Some(k) = tab {
                    if let Some(m) = self.menu.as_mut() {
                        m.mp_tab = k;
                    }
                } else if tab0 && go {
                    self.start_join(event_loop);
                } else if tab0 && host {
                    if let Some(m) = self.menu.as_mut() {
                        m.host = !m.host;
                        ::config::set_setting("gameplay", "host_next_drive", m.host);
                        if let Err(e) = ::config::save() {
                            log::warn!("settings: cannot save: {e}");
                        }
                    }
                } else if let (Some(k), false) = (server, tab0) {
                    self.start_join_server(event_loop, k);
                }
            }
            StartPage::Options | StartPage::Profile => {}
        }
    }
}
