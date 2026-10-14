use super::*;

impl App {
    pub(crate) fn start_key(&mut self, event_loop: &ActiveEventLoop, code: KeyCode, repeat: bool) {
        use KeyCode as K;
        let Some(page) = self.start_page() else {
            return;
        };
        if repeat && matches!(code, K::Escape | K::Enter | K::NumpadEnter | K::Space | K::Tab) {
            return;
        }
        let ctrl = self.start_ctrl();
        let enter = matches!(code, K::Enter | K::NumpadEnter);
        if self.drv_key(code) {
            return;
        }
        match page {
            StartPage::Start => {
                if self.menu.as_ref().is_some_and(|m| m.edit != 0) {
                    let paste = if ctrl && code == K::KeyV { clipboard_text() } else { None };
                    if let Some(m) = self.menu.as_mut() {
                        if let Some(t) = paste {
                            let clean: String = t.lines().next().unwrap_or("").chars().filter(|c| !c.is_control()).collect();
                            let (buf, max) = m.edit_buf();
                            buf.push_str(&clean);
                            *buf = buf.chars().take(max).collect();
                            m.edit_changed();
                        }
                        match code {
                            K::Backspace => {
                                m.edit_buf().0.pop();
                                m.edit_changed();
                            }
                            K::Escape | K::Enter | K::NumpadEnter | K::Tab => m.edit = 0,
                            _ => {}
                        }
                    }
                    return;
                }
                if code == K::Escape {
                    if let Some(m) = self.menu.as_mut().filter(|m| m.cal_open || m.drop != 0) {
                        m.cal_open = false;
                        m.drop = 0;
                        return;
                    }
                    self.start_back();
                    return;
                }
                if code == K::KeyQ || code == K::KeyE {
                    self.start_tab(if code == K::KeyQ { -1 } else { 1 });
                    return;
                }
                let ve = self.ui.as_ref().map_or(6, |u| u.start.entry_vis.max(1) as i32);
                let (vm, vv, vl, vt, vw) = self.ui.as_ref().map_or((8, 8, 6, 6, 8), |u| {
                    (
                        u.start.map_vis.max(1) as i32,
                        u.start.veh_vis.max(1) as i32,
                        u.start.line_vis.max(1) as i32,
                        u.start.tour_vis.max(1) as i32,
                        u.start.weather_vis.max(1) as i32,
                    )
                });
                let sect = self.menu.as_ref().map_or(0, |m| m.sect);
                if enter {
                    if sect == 0 || sect >= 3 {
                        self.start_launch(event_loop, None, None);
                    } else {
                        self.start_open(sect + 1);
                    }
                    return;
                }
                let up = matches!(code, K::ArrowUp | K::KeyW);
                let down = matches!(code, K::ArrowDown | K::KeyS);
                let left = matches!(code, K::ArrowLeft | K::KeyA);
                let right = matches!(code, K::ArrowRight | K::KeyD);
                let Some(m) = self.menu.as_mut() else {
                    return;
                };
                if sect == 0 {
                    match code {
                        _ if up => m.focus = (m.focus + 2) % 3,
                        _ if down => m.focus = (m.focus + 1) % 3,
                        _ if left => m.step(0, -1),
                        _ if right => m.step(0, 1),
                        K::Space => {
                            let s = m.focus + 1;
                            self.start_open(s);
                        }
                        _ => {}
                    }
                    return;
                }
                let mut show = false;
                if code == K::Tab && sect == 2 {
                    m.edit = 1;
                } else if sect == 2 && (left || right) {
                    m.step(8, if left { -1 } else { 1 });
                } else if code == K::Tab && sect == 3 {
                    if m.weather == 3 {
                        if m.zone == 0 {
                            m.zone = 1;
                            m.row = 12;
                        } else if m.row >= 15 {
                            m.zone = 0;
                        } else {
                            m.row += 1;
                        }
                    }
                } else if code == K::Tab && sect == 1 {
                    m.zone = match (m.zone, m.route) {
                        (0, true) => 2,
                        (0, false) => 4,
                        (2, _) => 3,
                        _ => 0,
                    };
                } else if sect == 1 && !m.route && m.zone == 4 {
                    let n = m.entry_rows().len() as i32;
                    let d = match code {
                        _ if up => -1,
                        _ if down => 1,
                        K::PageUp => -ve,
                        K::PageDown => ve,
                        K::Home => -n,
                        K::End => n,
                        _ => 0,
                    };
                    if d != 0 {
                        m.entry_step(d);
                    } else if left {
                        m.zone = 0;
                    }
                } else if sect == 1 && m.route && matches!(m.zone, 2 | 3) {
                    let (n, vis) = if m.zone == 2 {
                        (m.line_rows().len() as i32, vl)
                    } else {
                        (m.tour_rows().len() as i32, vt)
                    };
                    let d = match code {
                        _ if up => -1,
                        _ if down => 1,
                        K::PageUp => -vis,
                        K::PageDown => vis,
                        K::Home => -n,
                        K::End => n,
                        _ => 0,
                    };
                    if d != 0 {
                        if m.zone == 2 {
                            m.line_step(d);
                        } else {
                            m.tour_step(d);
                        }
                        show = true;
                    } else if left {
                        m.zone = if m.zone == 3 { 2 } else { 0 };
                    } else if right && m.zone == 2 {
                        m.zone = 3;
                    }
                } else if m.zone == 0 && matches!(sect, 1 | 2 | 3) {
                    let (n, vis) = match sect {
                        1 => (m.maps.len() as i32, vm),
                        2 => (m.vehicles.len() as i32, vv),
                        _ => (m.weathers.len() as i32, vw),
                    };
                    let d = match code {
                        _ if up => -1,
                        _ if down => 1,
                        K::PageUp => -vis,
                        K::PageDown => vis,
                        K::Home => -n,
                        K::End => n,
                        _ => 0,
                    };
                    if d != 0 {
                        match sect {
                            1 => m.map_step(d),
                            2 => m.vehicle_step(d),
                            _ => m.weather_step(d),
                        }
                        show = true;
                    } else if right && sect == 1 && m.route {
                        m.zone = 2;
                    } else if right && sect == 1 {
                        m.zone = 4;
                    } else if sect == 3 && (left || right) {
                        m.step(1, if left { -1 } else { 1 });
                    }
                } else {
                    let rows = crate::menu::Menu::sect_rows(sect);
                    if rows.is_empty() {
                        return;
                    }
                    let idx = rows.iter().position(|r| *r == m.row).unwrap_or(0);
                    match code {
                        _ if up => m.row = rows[(idx + rows.len() - 1) % rows.len()],
                        _ if down || code == K::Tab => m.row = rows[(idx + 1) % rows.len()],
                        _ if left => m.step(m.row, -1),
                        _ if right => m.step(m.row, 1),
                        _ => {}
                    }
                }
                if show {
                    self.start_show_map();
                    self.start_show_veh();
                    self.start_show_lists();
                    self.start_show_weather();
                }
            }
            StartPage::Multiplayer => match code {
                K::Escape => {
                    if let Some(m) = self.menu.as_mut().filter(|m| m.addr_focus) {
                        m.addr_focus = false;
                        m.addr_sel = false;
                    } else {
                        self.start_back();
                    }
                }
                K::KeyQ | K::KeyE if !ctrl && !self.menu.as_ref().is_some_and(|m| m.addr_focus) => {
                    self.start_tab(if code == K::KeyQ { -1 } else { 1 });
                }
                K::Enter | K::NumpadEnter => {
                    if self.menu.as_ref().is_some_and(|m| m.mp_tab == 0) {
                        self.start_join(event_loop);
                    }
                }
                K::Backspace => {
                    if let Some(m) = self.menu.as_mut().filter(|m| m.mp_tab == 0 && m.addr_focus) {
                        if m.addr_sel {
                            m.address.clear();
                            m.addr_sel = false;
                        } else {
                            m.address.pop();
                        }
                    }
                }
                K::KeyA if ctrl => {
                    if let Some(m) = self.menu.as_mut().filter(|m| m.mp_tab == 0 && m.addr_focus && !m.address.is_empty()) {
                        m.addr_sel = true;
                    }
                }
                K::KeyC | K::KeyX if ctrl => {
                    if let Some(m) = self.menu.as_mut().filter(|m| m.mp_tab == 0) {
                        if !m.address.is_empty() {
                            clipboard_set(&m.address);
                            if code == K::KeyX {
                                m.address.clear();
                            }
                        }
                        m.addr_sel = false;
                    }
                }
                K::KeyV if ctrl => {
                    if let Some(t) = clipboard_text() {
                        if let Some(m) = self.menu.as_mut().filter(|m| m.mp_tab == 0 && m.addr_focus) {
                            let clean: String = t.lines().next().unwrap_or("").trim().chars().filter(|c| !c.is_control()).collect();
                            if m.addr_sel {
                                m.address.clear();
                                m.addr_sel = false;
                            }
                            m.address.push_str(&clean);
                            m.address = m.address.chars().take(200).collect();
                        }
                    }
                }
                _ => {}
            },
            StartPage::Options => {
                if !repeat {
                    self.lab_key(event_loop, code);
                }
            }
            StartPage::Profile => {
                if code == K::Escape {
                    self.start_back();
                }
            }
        }
    }
}
