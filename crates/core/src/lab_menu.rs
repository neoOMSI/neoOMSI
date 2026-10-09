//! The pause menu

use super::*;
use crate::ui::{ADMIN_PAGE, Dialog, OPTIONS_PAGE, OPTION_GROUPS, PAGE_COUNT, PauseState, VEHICLE_PAGE, WORLD_PAGE, WorldDrop, WorldGroup, WorldRow};

#[derive(Clone, Default)]
pub(crate) struct PlaceSel {
    pub id: &'static str,
    pub maker: Option<(String, String)>,
    pub ty: Option<(String, String)>,
    pub livery: Option<(String, String)>,
    pub hof: Option<(String, String)>,
    pub pick: usize,
}

type LabGroup = (String, Vec<(String, String)>, crate::lab_options::Subs);

fn pick_row(groups: &[LabGroup], g: usize, sub: usize, k: usize) -> Option<(String, String)> {
    let gr = groups.get(g)?;
    let rows = if sub == 0 { &gr.1 } else { &gr.2.get(sub - 1)?.1 };
    rows.get(k).cloned()
}

fn view_rows(rows: &[(String, String)]) -> Vec<WorldRow> {
    let tr = |t: &str| t.to_string();
    rows.iter()
        .map(|(row, _)| {
            let mut p = row.split('\u{1f}');
            let name = p.next().unwrap_or("");
            let kind = p.next().and_then(|k| k.chars().next()).unwrap_or('i');
            let value = p.next().unwrap_or("");
            let desc = p.next().unwrap_or("");
            let frac = p.next().and_then(|f| f.parse().ok()).unwrap_or(0.0);
            let tag = p.next().unwrap_or("").to_string();
            let meter = p.next().and_then(|m| m.parse::<f32>().ok());
            let meter_one_sided = p.next() == Some("u");
            let lit = p.next() == Some("1");
            WorldRow { name: tr(name), kind, value: value.to_string(), desc: tr(desc), frac, tag, meter, meter_one_sided, lit }
        })
        .collect()
}

thread_local! {
    static SYNC: std::cell::Cell<Option<(std::time::Instant, Option<usize>)>> = const { std::cell::Cell::new(None) };
    static SYNC_NOW: std::cell::Cell<bool> = const { std::cell::Cell::new(false) };
    static ADMIN_CACHE: std::cell::RefCell<Option<(std::time::Instant, Vec<(String, String)>)>> = const { std::cell::RefCell::new(None) };
}

fn tl(key: &str) -> String {
    ::i18n::translate(key, &[])
}

const PLACE_FIELDS: [&str; 4] = [
    "pause.dialog.place.model",
    "pause.dialog.place.type",
    "pause.dialog.place.livery",
    "pause.dialog.place.hof",
];

impl App {
    pub(crate) fn lab_entries(&self) -> Vec<(&'static str, String)> {
        self.game_menu_items()
            .into_iter()
            .filter(|(id, _)| !id.starts_with("report"))
            .map(|(id, label)| (id, ::i18n::translate(label, &[])))
            .collect()
    }

    fn lab_admin(&self) -> bool {
        let host = self.lan.as_ref().is_some_and(|l| l.role == ::network::Role::Host);
        host || self.is_admin
    }

    fn lab_pages(&self) -> usize {
        if self.lab_admin() { PAGE_COUNT } else { PAGE_COUNT - 1 }
    }

    fn lab_admin_groups(&self) -> Vec<LabGroup> {
        let items = ADMIN_CACHE.with(|c| {
            let mut c = c.borrow_mut();
            match c.as_ref() {
                Some((at, v)) if at.elapsed().as_secs_f32() < 0.5 => v.clone(),
                _ => {
                    let v = crate::admin::items(self);
                    *c = Some((std::time::Instant::now(), v.clone()));
                    v
                }
            }
        });
        let mut groups: [(&'static str, Vec<(String, String)>); 4] =
            [("Players", Vec::new()), ("Clock", Vec::new()), ("Weather", Vec::new()), ("Traffic", Vec::new())];
        for (label, action) in items {
            let g = match action.split(' ').next().unwrap_or("") {
                "back" => continue,
                "goto" | "bring" | "kick" | "ban" | "unstick" | "bringall" | "service" => 0,
                "clock" | "time" | "speed" => 1,
                "weather" => 2,
                _ => 3,
            };
            groups[g].1.push((format!("{label}\u{1f}a\u{1f}pause.page.admin.run\u{1f}\u{1f}0\u{1f}"), action));
        }
        groups.into_iter().filter(|g| !g.1.is_empty()).map(|(t, r)| (t.to_string(), r, Vec::new())).collect()
    }

    fn lab_group_page(&self) -> Option<usize> {
        self.lab_menu
            .and_then(|s| s.page)
            .filter(|p| *p == WORLD_PAGE || *p == OPTIONS_PAGE || *p == ADMIN_PAGE)
    }

    fn lab_world_raw(&self) -> Vec<LabGroup> {
        match self.lab_group_page() {
            Some(OPTIONS_PAGE) => crate::lab_options::options_groups(&crate::lab_options::Host::game(self)),
            Some(ADMIN_PAGE) => self.lab_admin_groups(),
            _ => crate::game_lists::world_groups(self),
        }
    }

    fn lab_world_kind(&self, g: usize) -> crate::game_lists::ListKind {
        match self.lab_group_page() {
            Some(OPTIONS_PAGE) => crate::game_lists::ListKind::Options(g),
            Some(ADMIN_PAGE) => crate::game_lists::ListKind::Admin,
            _ => crate::game_lists::ListKind::World(g),
        }
    }

    fn lab_sync_due(&self) -> bool {
        let page = self.lab_group_page();
        let live = self.ui.as_ref().is_some_and(|u| {
            u.world_drag.is_some() || u.world_bar_grab.is_some() || (page == Some(OPTIONS_PAGE) && u.world_sub >= 2)
        });
        let forced = SYNC_NOW.with(|c| c.replace(false));
        SYNC.with(|c| {
            let due = live
                || forced
                || c.get().map_or(true, |(t, p)| p != page || t.elapsed() >= std::time::Duration::from_millis(100));
            if due {
                c.set(Some((std::time::Instant::now(), page)));
            }
            due
        })
    }

    fn lab_world_sync(&mut self) {
        let shown = self.lab_group_page();
        let on = shown.is_some();
        if shown == Some(OPTIONS_PAGE) {
            let hwnd = self.window.as_deref().and_then(crate::controllers::window_handle);
            let root = self.args.root.clone();
            self.controllers
                .get_or_insert_with(|| crate::controllers::Controllers::new(&root, hwnd));
        }
        let view: Vec<WorldGroup> = if on {
            let tr = |t: &str| t.to_string();
            self.lab_world_raw()
                .into_iter()
                .map(|(title, rows, subs)| WorldGroup {
                    title: tr(&title),
                    tab: if shown == Some(OPTIONS_PAGE) {
                        OPTION_GROUPS.iter().find(|g| g.title == title.as_str()).map_or(String::new(), |g| tr(g.tab))
                    } else {
                        String::new()
                    },
                    rows: view_rows(&rows),
                    subs: subs
                        .iter()
                        .map(|(t, r)| WorldGroup { title: tr(t), tab: String::new(), rows: view_rows(r), subs: Vec::new() })
                        .collect(),
                })
                .collect()
        } else {
            Vec::new()
        };
        if let Some(u) = self.ui.as_mut() {
            if !on {
                u.world_drag = None;
                u.world_drop = None;
            }
            u.world_view = std::sync::Arc::new(view);
            u.world_view_page = shown.unwrap_or(usize::MAX);
            crate::lab_pads::set_tab(u.world_sub);
        }
    }

    fn lab_key_clear(&mut self, id: &str) {
        let mut it = id.splitn(4, ' ');
        if it.next() != Some("keybind") {
            return;
        }
        if let (Some(sec), Some(idx), Some(name)) = (
            it.next().and_then(|x| x.parse::<usize>().ok()),
            it.next().and_then(|x| x.parse::<usize>().ok()),
            it.next(),
        ) {
            let name = name.to_string();
            self.keybind_edit(sec, idx, &name, crate::game_menu::KeyEdit::Clear);
        }
    }

    /// Delete / Backspace on the key binding under the mouse.
    fn lab_key_clear_hovered(&mut self) {
        let (x, y) = self.cursor;
        let Some(u) = self.ui.as_ref() else {
            return;
        };
        let Some(i) = u.world_rows_rc.iter().position(|r| x >= r[0] && x < r[2] && y >= r[1] && y < r[3]) else {
            return;
        };
        let (k, g, sub) = (u.world_first + i, u.world_group, u.world_sub);
        let groups = self.lab_world_raw();
        if let Some((_, id)) = pick_row(&groups, g, sub, k) {
            self.lab_key_clear(&id);
        }
    }

    fn lab_world_click(&mut self) {
        SYNC_NOW.with(|c| c.set(true));
        let (x, y) = self.cursor;
        let hit = |list: &[[f32; 4]]| list.iter().position(|r| x >= r[0] && x < r[2] && y >= r[1] && y < r[3]);

        if self.key_capture.take().is_some() {
            return;
        }
        if self.key_search {
            self.key_search_stop();
        }
        let clear = self.ui.as_ref().and_then(|u| {
            let r = u.world_clear.iter().find(|(_, r)| hit(&[*r]).is_some())?;
            Some((r.0, u.world_group, u.world_sub))
        });
        if let Some((k, g, sub)) = clear {
            let groups = self.lab_world_raw();
            if let Some((_, id)) = pick_row(&groups, g, sub, k) {
                self.lab_key_clear(&id);
            }
            return;
        }
        let Some(u) = self.ui.as_mut() else {
            return;
        };
        if let Some((track, thumb)) = u.world_bar {
            if hit(&[track]).is_some() {
                u.world_bar_grab = Some(if y >= thumb[1] && y < thumb[3] { y - thumb[1] } else { (thumb[3] - thumb[1]) * 0.5 });
                self.lab_world_bar_set(y);
                return;
            }
        }
        if let Some(g) = hit(&u.world_groups_rc) {
            u.world_group = g;
            u.world_sub = 0;
            u.world_scroll = 0;
            return;
        }
        if let Some(i) = hit(&u.world_sub_rc) {
            let tabs = u.world_view.get(u.world_group).map_or(1, |g| g.subs.len() + 1);
            u.world_sub = if i < tabs {
                i
            } else if i == tabs {
                u.world_sub.saturating_sub(1)
            } else {
                (u.world_sub + 1).min(tabs - 1)
            };
            u.world_scroll = 0;
            return;
        }
        let Some(i) = hit(&u.world_rows_rc) else {
            return;
        };
        let k = u.world_first + i;
        let g = u.world_group;
        let sub = u.world_sub;
        let track = u.world_tracks.get(i).copied().flatten();
        let groups = self.lab_world_raw();
        let Some((row, id)) = pick_row(&groups, g, sub, k) else {
            return;
        };
        let mut p = row.split('\u{1f}');
        let _ = p.next();
        let kind = p.next().and_then(|k| k.chars().next()).unwrap_or('i');
        let list = self.lab_world_kind(g);
        if self.lab_group_page() == Some(ADMIN_PAGE) {
            ADMIN_CACHE.with(|c| *c.borrow_mut() = None);
        }
        if id == "keysearch" {
            self.key_search_start();
            return;
        }
        match kind {
            'i' | 'h' => {}
            'v' => {
                if let Some(t) = track {
                    if let Some(u) = self.ui.as_mut() {
                        u.world_drag = Some((k, t));
                    }
                    self.lab_world_set(x);
                }
            }
            'o' => match crate::game_lists::dropdown_for(self, k, &id) {
                Some(d) => {
                    let sel = d.current.unwrap_or(0).min(d.items.len().saturating_sub(1));
                    let vis = self.ui.as_ref().map_or(8, |u| u.world_drop_vis).max(1);
                    if let Some(u) = self.ui.as_mut() {
                        let labels: Vec<String> = d.items.iter().map(|i| i.0.clone()).collect();
                        let actions: Vec<String> = d.items.iter().map(|i| i.1.clone()).collect();
                        let searchable = !d.search.is_empty();
                        u.world_drop = Some(WorldDrop {
                            k,
                            all_labels: if searchable { labels.clone() } else { Vec::new() },
                            all_actions: if searchable { actions.clone() } else { Vec::new() },
                            hay: d.search.clone(),
                            filter: String::new(),
                            labels,
                            actions,
                            sel,
                            top: sel.saturating_sub(vis / 2),
                            current: d.current,
                        });
                    }
                }
                None => {
                    crate::game_lists::run(self, &list, &id);
                }
            },
            _ => {
                crate::game_lists::run(self, &list, &id);
            }
        }
    }

    fn lab_world_drop_click(&mut self) {
        let (x, y) = self.cursor;
        let Some(u) = self.ui.as_ref() else {
            return;
        };
        let hit = u.world_drop_rc.iter().position(|r| x >= r[0] && x < r[2] && y >= r[1] && y < r[3]);
        let at = hit.map(|i| u.world_drop_first + i);
        match at {
            Some(i) => self.lab_world_drop_pick(i),
            None => {
                if let Some(u) = self.ui.as_mut() {
                    u.world_drop = None;
                }
            }
        }
    }

    fn lab_world_drop_pick(&mut self, i: usize) {
        let Some(d) = self.ui.as_mut().and_then(|u| u.world_drop.take()) else {
            return;
        };
        if let Some(action) = d.actions.get(i) {
            crate::game_lists::dropdown_apply(self, action);
        }
    }

    pub(crate) fn lab_world_drop_text(&mut self, text: &str) {
        let Some(d) = self.ui.as_mut().and_then(|u| u.world_drop.as_mut()) else {
            return;
        };
        if d.hay.is_empty() {
            return;
        }
        d.filter.extend(text.chars().filter(|c| !c.is_control()));
        self.lab_world_drop_refilter();
    }

    fn lab_world_drop_refilter(&mut self) {
        let Some(d) = self.ui.as_mut().and_then(|u| u.world_drop.as_mut()) else {
            return;
        };
        let q = d.filter.trim().to_lowercase();
        let cur = d.current.and_then(|c| d.actions.get(c)).cloned();
        let keep: Vec<usize> = (0..d.all_labels.len()).filter(|i| *i == 0 || q.is_empty() || d.hay.get(*i).is_some_and(|h| h.contains(&q))).collect();
        d.labels = keep.iter().map(|i| d.all_labels[*i].clone()).collect();
        d.actions = keep.iter().map(|i| d.all_actions[*i].clone()).collect();
        d.current = cur.and_then(|c| d.actions.iter().position(|x| *x == c));
        d.sel = if q.is_empty() { d.current.unwrap_or(0) } else { 1.min(d.labels.len().saturating_sub(1)) };
        d.top = 0;
        let vis = self.ui.as_ref().map_or(8, |u| u.world_drop_vis).max(1);
        if let Some(d) = self.ui.as_mut().and_then(|u| u.world_drop.as_mut()) {
            if d.sel >= vis {
                d.top = d.sel + 1 - vis;
            }
        }
    }

    fn lab_world_drop_key(&mut self, code: KeyCode) {
        let Some(u) = self.ui.as_mut() else {
            return;
        };
        let vis = u.world_drop_vis;
        let Some(d) = u.world_drop.as_mut() else {
            return;
        };
        let n = d.labels.len().max(1);
        let ss = !d.hay.is_empty();
        match code {
            KeyCode::Escape => u.world_drop = None,
            KeyCode::Backspace if ss => {
                d.filter.pop();
                self.lab_world_drop_refilter();
                return;
            }
            KeyCode::ArrowUp => d.sel = (d.sel + n - 1) % n,
            KeyCode::KeyW if !ss => d.sel = (d.sel + n - 1) % n,
            KeyCode::ArrowDown => d.sel = (d.sel + 1) % n,
            KeyCode::KeyS if !ss => d.sel = (d.sel + 1) % n,
            KeyCode::PageUp => d.sel = d.sel.saturating_sub(5),
            KeyCode::PageDown => d.sel = (d.sel + 5).min(n - 1),
            KeyCode::Home => d.sel = 0,
            KeyCode::End => d.sel = n - 1,
            KeyCode::Enter | KeyCode::NumpadEnter => {
                let i = d.sel;
                self.lab_world_drop_pick(i);
                return;
            }
            KeyCode::Space if !ss => {
                let i = d.sel;
                self.lab_world_drop_pick(i);
                return;
            }
            _ => return,
        }
        if let Some(d) = u.world_drop.as_mut() {
            if d.sel < d.top {
                d.top = d.sel;
            } else if d.sel >= d.top + vis {
                d.top = d.sel + 1 - vis;
            }
        }
    }

    pub(crate) fn lab_world_bar_set(&mut self, y: f32) {
        let Some(u) = self.ui.as_mut() else {
            return;
        };
        let (Some((track, thumb)), Some(grab)) = (u.world_bar, u.world_bar_grab) else {
            return;
        };
        let travel = ((track[3] - track[1]) - (thumb[3] - thumb[1])).max(1.0);
        let f = ((y - grab - track[1]) / travel).clamp(0.0, 1.0);
        u.world_scroll = (f * u.world_max as f32).round() as usize;
    }

    pub(crate) fn lab_world_set(&mut self, x: f32) {
        let Some((k, t, g, sub)) = self
            .ui
            .as_ref()
            .and_then(|u| u.world_drag.map(|(k, t)| (k, t, u.world_group, u.world_sub)))
        else {
            return;
        };
        let fx = ((x - t[0]) / (t[2] - t[0]).max(1.0)).clamp(0.0, 1.0);
        let groups = self.lab_world_raw();
        let Some((_, id)) = pick_row(&groups, g, sub, k) else {
            return;
        };
        let list = self.lab_world_kind(g);
        crate::game_lists::run_move(self, &list, &id, crate::game_lists::Move::To(fx));
    }

    pub(crate) fn lab_world_wheel(&mut self, amount: f32) -> bool {
        if self.lab_group_page().is_none() || self.lab_list.is_some() {
            return false;
        }
        if let Some(u) = self.ui.as_mut() {
            let vis = u.world_drop_vis;
            if let Some(d) = u.world_drop.as_mut() {
                let max = d.labels.len().saturating_sub(vis) as f32;
                d.top = (d.top as f32 - amount * 2.0).clamp(0.0, max).round() as usize;
                return true;
            }
            u.world_scroll = (u.world_scroll as f32 - amount * 2.0).clamp(0.0, u.world_max as f32).round() as usize;
        }
        true
    }

    pub(crate) fn lab_entries_sync(&mut self) {
        let admin = self.lab_admin();
        if !admin && self.lab_menu.is_some_and(|s| s.page == Some(ADMIN_PAGE)) {
            self.lab_menu = self.lab_menu.map(|s| PauseState { page: None, ..s });
        }
        if let Some(u) = self.ui.as_mut() {
            u.admin_visible = admin;
        }
        if self.lab_sync_due() {
            self.lab_world_sync();
        }
        if self.lab_menu.is_none() {
            return;
        }
        let list: Vec<String> = self.lab_entries().into_iter().map(|e| e.1).collect();
        if let Some(u) = self.ui.as_mut() {
            u.pause_entries = list;
        }
    }

    fn lab_page_sel(&self, page: usize) -> usize {
        let id = ["map", "options", "world", "vehicle", "admin"].get(page).copied().unwrap_or("resume");
        self.lab_entries().iter().position(|e| e.0 == id).unwrap_or(0)
    }

    pub(crate) fn open_map_page(&mut self) {
        if self.lab_menu.is_none() {
            self.open_game_menu();
            self.lab_map_direct = true;
        }
        let sel = self.lab_page_sel(0);
        self.lab_menu = Some(PauseState { page: Some(0), sel });
    }

    pub(crate) fn toggle_map_page(&mut self) {
        if self.lab_menu.is_some_and(|s| s.page == Some(0)) {
            self.close_game_menu();
        } else if let Some(n) = self.navigator.as_mut().filter(|n| n.map_open() && n.city.embed.is_none()) {
            n.toggle_map();
        } else {
            self.open_map_page();
        }
    }

    pub(crate) fn lab_map_sync(&mut self) {
        let on = self.lab_menu.is_some_and(|s| s.page == Some(0));
        let rect = self
            .ui
            .as_ref()
            .map(|u| u.map_rect)
            .filter(|r| r[2] - r[0] >= 32.0 && r[3] - r[1] >= 32.0);
        if !on {
            self.teleport_pick = false;
        }
        let mut picture = None;
        let mut duty_btn = [0.0; 4];
        if let Some(n) = self.navigator.as_mut() {
            n.embed_map(if on { rect } else { None });
            picture = n.city.picture;
            duty_btn = n.duty_button();
        }
        let label = match self.duty.as_ref() {
            Some(d) => format!("{} / {}", d.line, d.tour),
            None => tl("pause.page.map.tour"),
        };
        let can = self.player.is_some() && self.lab_place.is_none() && self.lab_list.is_none();
        let preview = match (self.lab_list.as_ref(), self.ui.as_ref().and_then(|u| u.dialog.as_ref())) {
            (Some((list, kind, idx)), Some(Dialog::Select { sel, drop: None, .. }))
            if matches!(kind, crate::game_lists::ListKind::Lines | crate::game_lists::ListKind::Tours(..)) =>
                {
                    idx.get(*sel).and_then(|&k| {
                        crate::game_lists::menu_extras(Some(kind), Some(list), Some(k), self.schedule.as_ref(), self.clock.time).2
                    })
                }
            _ => None,
        };
        let tall = self.lab_list.as_ref().is_some_and(|l| matches!(l.1, crate::game_lists::ListKind::Lines | crate::game_lists::ListKind::Tours(..)))
            && self.lab_place.is_none();
        if let Some(u) = self.ui.as_mut() {
            u.dialog_tall = tall;
            u.dialog_back = tall && matches!(self.lab_list.as_ref().map(|l| &l.1), Some(crate::game_lists::ListKind::Tours(..)));
            u.dialog_preview = preview;
            u.map_picture = if on { picture } else { None };
            u.map_btn_label = if on { label } else { String::new() };
            u.map_btn_on = can;
            u.map_btn = if on { duty_btn } else { [0.0; 4] };
        }
    }

    pub(crate) fn lab_map_mouse(&mut self, pressed: Option<bool>) -> bool {
        if !self.lab_menu.is_some_and(|s| s.page == Some(0)) || self.lab_place.is_some() || self.lab_list.is_some() {
            return false;
        }
        let (x, y) = self.cursor;
        let Some(n) = self.navigator.as_mut().filter(|n| n.city.embed.is_some()) else {
            return false;
        };
        let r = n.city.rect;
        let b = self.ui.as_ref().map_or([0.0; 4], |u| u.map_btn);
        let on_btn = x >= b[0] && x < b[2] && y >= b[1] && y < b[3];
        let inside = x >= r[0] && x < r[2] && y >= r[1] && y < r[3] && !on_btn;
        match pressed {
            Some(true) if inside => {
                if self.teleport_pick {
                    if let Some(at) = n.map_point(x, y) {
                        self.teleport_pick = false;
                        self.close_game_menu();
                        self.place_bus_at(at);
                    }
                } else {
                    n.map_press(x, y);
                }
                true
            }
            Some(false) => {
                n.map_release();
                false
            }
            _ => false,
        }
    }

    pub(crate) fn lab_dialog_wheel(&mut self, amount: f32) -> bool {
        if self.lab_list.is_none() {
            return false;
        }
        let (cx, cy) = self.cursor;
        if let Some(u) = self.ui.as_mut() {
            let r = u.dialog_pane_rc;
            if u.dialog_pane_max > 0 && cx >= r[0] && cx <= r[2] && cy >= r[1] && cy <= r[3] {
                let top = (u.dialog_pane_top.unwrap_or(0) as f32 - amount * 2.0).clamp(0.0, u.dialog_pane_max as f32).round() as usize;
                u.dialog_pane_top = Some(top);
                return true;
            }
        }
        let vis = self.ui.as_ref().map_or(1, |u| u.dialog_vis);
        if let Some(Dialog::Select { scroll, options, .. }) = self.ui.as_mut().and_then(|u| u.dialog.as_mut()) {
            let max = options.len().saturating_sub(vis) as f32;
            *scroll = (*scroll as f32 - amount * 2.0).clamp(0.0, max).round() as usize;
        }
        true
    }

    pub(crate) fn lab_place_preview(&mut self, dt: f32) {
        let want = self.lab_place.as_ref().and_then(|s| {
            s.ty.as_ref().map(|t| (t.1.clone(), s.livery.as_ref().map(|l| l.1.clone()).unwrap_or_default()))
        });
        let (Some((bus, paint)), Some((w, h))) = (
            want,
            self.ui.as_ref().map(|u| u.place_preview_size).filter(|s| s.0 > 0 && s.1 > 0),
        ) else {
            if let Some(u) = self.ui.as_mut() {
                u.place_picture = None;
                u.place_status.clear();
            }
            if self.lab_place.is_none() {
                self.lab_room = None;
                if let (Some((id, _, _)), Some(r), Some(scene)) = (self.lab_pic.take(), self.renderer.as_ref(), self.scene.as_mut()) {
                    r.free_texture(scene, id);
                }
            }
            return;
        };
        let look = crate::launcher::showroom::Look {
            root: self.args.root.clone(),
            map: self.args.map.clone(),
            bus,
            paint,
            weather: String::new(),
            time: 12 * 60,
            date: String::new(),
        };
        let room = self.lab_room.get_or_insert_with(|| {
            let mut room = crate::launcher::showroom::Showroom::new();

            room.zoom_by(1.3);
            room
        });
        room.want(look);
        let (Some(r), Some(scene), Some(ui)) = (self.renderer.as_mut(), self.scene.as_mut(), self.ui.as_mut()) else {
            return;
        };
        room.update(r, dt);
        if !room.has_scene() {
            ui.place_picture = None;
            ui.place_status = match room.error.as_ref() {
                Some(e) => format!("No preview: {e}"),
                None => "Loading\u{2026}".to_string(),
            };
            return;
        }
        ui.place_status.clear();
        if let Some((pw, ph, mut rgba)) = room.snapshot(r, w, h) {
            for px in rgba.chunks_exact_mut(4) {
                px[3] = 255;
            }
            let id = match self.lab_pic {
                Some((id, tw, th)) if (tw, th) == (pw, ph) => id,
                old => {
                    if let Some((id, _, _)) = old {
                        r.free_texture(scene, id);
                    }
                    let id = r.add_blank_texture(scene, pw, ph);
                    self.lab_pic = Some((id, pw, ph));
                    id
                }
            };
            r.update_texture(scene, id, &::texture::Image { width: pw, height: ph, rgba, has_alpha: false });
        }
        ui.place_picture = self.lab_pic.map(|p| p.0);
    }

    fn lab_place_start(&mut self, id: &'static str) {
        self.lab_place = Some(PlaceSel { id, ..PlaceSel::default() });
        self.lab_list = None;
        self.lab_place_show();
    }

    fn lab_place_show(&mut self) {
        let Some(s) = self.lab_place.as_ref() else {
            return;
        };
        let dash = "\u{2014}".to_string();
        let val = |o: &Option<(String, String)>| o.as_ref().map_or(dash.clone(), |x| x.0.clone());
        let on = [true, s.maker.is_some(), s.ty.is_some(), s.ty.is_some()];
        let vals = [val(&s.maker), val(&s.ty), val(&s.livery), val(&s.hof)];
        let fields = (0..4).map(|i| (tl(PLACE_FIELDS[i]), vals[i].clone(), on[i])).collect();
        let mut preview = vec![match (&s.ty, &s.maker) {
            (Some(t), _) => t.0.clone(),
            (None, Some(m)) => m.0.clone(),
            _ => tl("pause.dialog.place.none"),
        }];
        if s.ty.is_some() {
            if let Some(m) = &s.maker {
                preview.push(m.0.clone());
            }
            preview.push(format!("{}: {}", tl(PLACE_FIELDS[2]), vals[2]));
            preview.push(format!("{}: {}", tl(PLACE_FIELDS[3]), vals[3]));
        }
        let title = tl(&format!("pause.page.vehicle.action.{}.name", s.id));
        let can_create = s.ty.is_some();
        if let Some(u) = self.ui.as_mut() {
            u.dialog_under = None;
            u.dialog = Some(Dialog::Place { title, preview, fields, create: tl("pause.dialog.place.create"), can_create });
        }
    }

    fn lab_place_open(&mut self, field: usize) {
        let Some(s) = self.lab_place.as_ref() else {
            return;
        };
        let (maker, ty) = (s.maker.clone(), s.ty.clone());
        let (list, kind) = match field {
            0 => (
                crate::game_lists::place_makers(self)
                    .into_iter()
                    .map(|(k, n, c)| (format!("{n}  ({c} {})", tl("pause.dialog.place.models")), format!("maker {k}")))
                    .collect::<Vec<_>>(),
                crate::game_lists::ListKind::PlaceMaker,
            ),
            1 => {
                let Some((_, key)) = maker else {
                    return;
                };
                let kind = crate::game_lists::ListKind::PlaceType(key);
                (crate::game_lists::items(self, &kind), kind)
            }
            2 | 3 => {
                let Some((_, path)) = ty else {
                    return;
                };
                let kind = if field == 2 {
                    crate::game_lists::ListKind::PlaceLivery(path)
                } else {
                    crate::game_lists::ListKind::PlaceHof(path, String::new())
                };
                (crate::game_lists::items(self, &kind), kind)
            }
            _ => return,
        };

        let list: Vec<(String, String)> = list.into_iter().filter(|l| l.1 != "back").collect();
        if let Some(s) = self.lab_place.as_mut() {
            s.pick = field;
        }
        let idx: Vec<usize> = (0..list.len()).collect();
        let options = list.iter().map(|l| l.0.clone()).collect();
        self.lab_list = Some((list, kind, idx));
        if let Some(u) = self.ui.as_mut() {
            if let Some(under) = u.dialog.take().filter(|d| matches!(d, Dialog::Place { .. })) {
                u.dialog_under = Some(under);
            }
            u.dialog = Some(Dialog::Select { title: tl(PLACE_FIELDS[field]), options, sel: 0, scroll: 0, search: None, drop: Some(field) });
        }
    }

    fn lab_place_pick(&mut self, k: usize) {
        let Some((list, _, idx)) = self.lab_list.take() else {
            return;
        };
        let Some((label, action)) = idx.get(k).and_then(|&i| list.get(i)).cloned() else {
            return;
        };
        let rest = |p: &str| action.strip_prefix(p).unwrap_or("").to_string();
        let mut single: Option<String> = None;
        if let Some(s) = self.lab_place.as_mut() {
            match s.pick {
                0 => {
                    let name = label.split("  (").next().unwrap_or("").to_string();
                    s.maker = Some((name, rest("maker ")));
                    s.ty = None;
                    s.livery = None;
                    s.hof = None;
                    single = Some(rest("maker "));
                }
                1 => {
                    s.ty = Some((label, rest("bus ")));
                    s.livery = Some((tl("pause.dialog.place.random_livery"), String::new()));
                    s.hof = Some((tl("pause.dialog.place.depot_file"), String::new()));
                }
                2 => s.livery = Some((label, rest("livery "))),
                _ => s.hof = Some((label, rest("placehof "))),
            }
        }

        if let Some(key) = single {
            let types: Vec<(String, String)> = crate::game_lists::items(self, &crate::game_lists::ListKind::PlaceType(key))
                .into_iter()
                .filter(|l| l.1.starts_with("bus "))
                .collect();
            if let (Some(s), [(label, action)]) = (self.lab_place.as_mut(), types.as_slice()) {
                s.ty = Some((label.clone(), action["bus ".len()..].to_string()));
                s.livery = Some((tl("pause.dialog.place.random_livery"), String::new()));
                s.hof = Some((tl("pause.dialog.place.depot_file"), String::new()));
            }
        }
        self.lab_place_show();
    }

    fn lab_place_create(&mut self) {
        let Some(s) = self.lab_place.clone() else {
            return;
        };
        let Some((_, path)) = s.ty else {
            return;
        };
        self.lab_place = None;
        self.lab_list = None;
        if let Some(u) = self.ui.as_mut() {
            u.dialog = None;
            u.dialog_under = None;
        }
        self.swap_pending = s.id == "swap" && self.player.is_some();
        self.close_game_menu();
        let paint = s.livery.map(|l| l.1).filter(|p| !p.is_empty());
        let hof = s.hof.map(|h| h.1).filter(|h| !h.is_empty());
        self.place_vehicle(&path, paint, hof);
    }

    fn lab_list_dialog(&mut self, id: &str) {
        let (Some(list), Some(kind)) = (self.admin_list.take(), self.list_kind.take()) else {
            return;
        };
        self.chooser = None;
        let no_back = matches!(kind, crate::game_lists::ListKind::Lines | crate::game_lists::ListKind::Tours(..));
        let last = list.len().saturating_sub(1);
        let idx: Vec<usize> = (0..list.len())
            .filter(|&k| list[k].1 != crate::game_lists::HEADING && !(no_back && k == last && list[k].1 == "back" && list.len() > 1))
            .collect();
        let options = idx.iter().map(|&k| list[k].0.clone()).collect();
        let key = format!("pause.page.vehicle.action.{id}.name");
        let mut title = ::i18n::translate(&key, &[]);
        if title == key {
            title = self
                .lab_entries()
                .into_iter()
                .find(|e| e.0 == id)
                .map_or_else(|| id.to_string(), |e| e.1);
        }
        self.lab_list = Some((list, kind, idx));
        if let Some(u) = self.ui.as_mut() {
            u.dialog = Some(Dialog::Select { title, options, sel: 0, scroll: 0, search: None, drop: None });
        }
    }

    fn lab_load_vehicles(&mut self, id: &'static str) {
        if self.vehicle_scan.is_none() {
            self.vehicle_scan = Some(scan_vehicles(self.args.root.clone(), self.args.map.clone()));
        }
        self.lab_load = Some(id);
        let title = ::i18n::translate(&format!("pause.page.vehicle.action.{id}.name"), &[]);
        if let Some(u) = self.ui.as_mut() {
            u.dialog = Some(Dialog::Loading { title });
        }
    }

    pub(crate) fn lab_poll(&mut self) {
        if let Some(rx) = self.vehicle_scan.as_ref() {
            match rx.try_recv() {
                Ok((mut list, meta)) => {
                    list.sort_by_key(|v| v.0.to_lowercase());
                    self.vehicle_list = list;
                    self.vehicle_meta = meta;
                    self.vehicle_scan = None;
                }
                Err(std::sync::mpsc::TryRecvError::Empty) => return,
                Err(_) => self.vehicle_scan = None,
            }
        }
        let Some(id) = self.lab_load.take() else {
            return;
        };
        if let Some(u) = self.ui.as_mut() {
            u.dialog = None;
        }
        if self.vehicle_list.is_empty() {
            self.service_msg = Some((tl("pause.dialog.no_vehicles"), 3.0));
            return;
        }
        if matches!(id, "place" | "swap") {
            self.lab_place_start(id);
            return;
        }
        self.page_action(id);
        if self.admin_list.is_some() {
            self.lab_list_dialog(id);
        }
    }

    fn lab_search(&self) -> Option<String> {
        match self.ui.as_ref().and_then(|u| u.dialog.as_ref()) {
            Some(Dialog::Select { search, .. }) => search.clone(),
            _ => None,
        }
    }

    fn lab_refilter(&mut self, q: Option<String>) {
        let Some((list, _, idx)) = self.lab_list.as_mut() else {
            return;
        };
        let low = q.as_deref().unwrap_or("").to_lowercase();
        *idx = (0..list.len())
            .filter(|&k| list[k].1 != crate::game_lists::HEADING && list[k].0.to_lowercase().contains(&low))
            .collect();
        let opts = idx.iter().map(|&k| list[k].0.clone()).collect();
        if let Some(Dialog::Select { options, sel, scroll, search, .. }) =
            self.ui.as_mut().and_then(|u| u.dialog.as_mut())
        {
            *options = opts;
            *sel = 0;
            *scroll = 0;
            *search = q;
        }
    }

    pub(crate) fn lab_dialog_text(&mut self, text: &str) {
        if let Some(mut q) = self.lab_search() {
            q.extend(text.chars().filter(|c| !c.is_control()));
            self.lab_refilter(Some(q));
        }
    }

    fn lab_dialog_close(&mut self) {
        self.lab_load = None;
        if self.lab_place.is_some() && self.lab_list.take().is_some() {
            self.lab_place_show();
            return;
        }
        self.lab_place = None;
        self.lab_list = None;
        if let Some(u) = self.ui.as_mut() {
            u.dialog = None;
            u.dialog_under = None;
        }
    }

    fn lab_tour_state(&self) -> Option<(String, String, usize, usize, usize, usize)> {
        let (list, kind, idx) = self.lab_list.as_ref()?;
        let crate::game_lists::ListKind::Tours(_, pick) = kind else {
            return None;
        };
        let Some(Dialog::Select { sel, .. }) = self.ui.as_ref()?.dialog.as_ref() else {
            return None;
        };
        let action = &list.get(*idx.get(*sel)?)?.1;
        let (ln, num) = action.strip_prefix("tour ")?.split_once('\u{1}')?;
        let sch = self.schedule.as_ref()?;
        let trips = sch.tour_trip_count(ln, num);
        let (stop, trip) = match pick {
            Some(p) if p.0 == num => (p.1, p.2),
            _ => (0, sch.tour_trip_now(ln, num, self.clock.time)),
        };
        let trip = trip.min(trips.saturating_sub(1));
        let n = sch.tour_trip_stops(ln, num, trip).len();
        (n > 0).then(|| (ln.to_string(), num.to_string(), trip, trips, stop.min(n - 1), n))
    }

    fn lab_pane_click(&mut self, i: usize) {
        let Some((ln, num, trip, trips, stop, n)) = self.lab_tour_state() else {
            return;
        };
        if i == usize::MAX {
            self.lab_list = None;
            if let Some(u) = self.ui.as_mut() {
                u.dialog = None;
            }
            crate::game_lists::start_duty_at(self, &ln, &num, trip, stop);
            self.close_game_menu();
            return;
        }
        let pick = if i == usize::MAX - 1 || i == usize::MAX - 2 {
            let to = if i == usize::MAX - 2 { (trip + 1).min(trips.saturating_sub(1)) } else { trip.saturating_sub(1) };
            (num, 0, to)
        } else if i < n {
            (num, i, trip)
        } else {
            return;
        };
        if let Some((_, crate::game_lists::ListKind::Tours(_, p), _)) = self.lab_list.as_mut() {
            *p = Some(pick);
        }
    }

    fn lab_dialog_pick(&mut self, k: usize) {
        if self.lab_place.is_some() {
            self.lab_place_pick(k);
            return;
        }

        if matches!(self.lab_list.as_ref().map(|l| &l.1), Some(crate::game_lists::ListKind::Tours(..))) {
            let is_tour = self
                .lab_list
                .as_ref()
                .and_then(|l| l.2.get(k).and_then(|&j| l.0.get(j)))
                .is_some_and(|e| e.1.starts_with("tour "));
            if is_tour {
                let cur = match self.ui.as_ref().and_then(|u| u.dialog.as_ref()) {
                    Some(Dialog::Select { sel, .. }) => *sel,
                    _ => k,
                };
                if cur != k {
                    if let Some(Dialog::Select { sel, .. }) = self.ui.as_mut().and_then(|u| u.dialog.as_mut()) {
                        *sel = k;
                    }
                } else {
                    self.lab_pane_click(usize::MAX);
                }
                return;
            }
        }
        let Some(&k) = self.lab_list.as_ref().and_then(|l| l.2.get(k)) else {
            return;
        };
        let Some((list, kind, _)) = self.lab_list.take() else {
            return;
        };
        if let Some(u) = self.ui.as_mut() {
            u.dialog = None;
        }
        self.admin_list = Some(list);
        self.list_kind = Some(kind);
        self.chooser_pick(k);

        if self.lab_menu.is_some() && self.admin_list.is_some() {
            let id = match self.list_kind {
                Some(crate::game_lists::ListKind::Lines | crate::game_lists::ListKind::Tours(..)) => "duty",
                _ => "place",
            };
            self.lab_list_dialog(id);
        }
    }

    fn lab_duty_open(&mut self, event_loop: &ActiveEventLoop) {
        let Some(k) = self.game_menu_items().iter().position(|m| m.0 == "duty") else {
            return;
        };
        self.menu_choose(event_loop, k);
        if self.lab_menu.is_some() && self.admin_list.is_some() {
            self.lab_list_dialog("duty");
        }
    }

    fn lab_activate(&mut self, event_loop: &ActiveEventLoop, entry: usize) {
        let Some(id) = self.lab_entries().get(entry).map(|e| e.0) else {
            return;
        };
        let page = |app: &mut Self, p: usize| {
            let sel = app.lab_page_sel(p);
            app.lab_menu = Some(PauseState { page: Some(p), sel });
        };
        match id {
            "resume" => self.close_game_menu(),
            "save" => {
                self.quick_save();
                self.close_game_menu();
            }
            "map" => page(self, 0),
            "options" => page(self, 1),
            "world" => page(self, 2),
            "vehicle" => page(self, VEHICLE_PAGE),
            "admin" => page(self, ADMIN_PAGE),
            "quit" => {
                self.game_menu = None;
                self.lab_menu = None;
                self.finish_session();
                crate::platform::exit(event_loop);
            }
            _ => {
                let Some(k) = self.game_menu_items().iter().position(|m| m.0 == id) else {
                    return;
                };
                self.menu_choose(event_loop, k);
                if self.lab_menu.is_some() && self.admin_list.is_some() {
                    self.lab_list_dialog(id);
                }
            }
        }
    }

    pub(crate) fn lab_key(&mut self, event_loop: &ActiveEventLoop, code: KeyCode) {
        SYNC_NOW.with(|c| c.set(true));
        let n = self.lab_pages();
        let st = self.lab_menu.unwrap_or_default();
        if self.lab_load.is_some() {
            if code == KeyCode::Escape {
                self.lab_dialog_close();
            }
            return;
        }
        if self.key_capture.is_some() {
            self.capture_key(code);
            return;
        }
        if self.key_search {
            match code {
                KeyCode::Escape | KeyCode::Enter | KeyCode::NumpadEnter => self.key_search_stop(),
                KeyCode::Backspace => {
                    self.key_filter.pop();
                }
                _ => {}
            }
            return;
        }
        if self.ui.as_ref().is_some_and(|u| u.world_drop.is_some()) {
            self.lab_world_drop_key(code);
            return;
        }
        if self.menu_edit.is_some() && st.page == Some(WORLD_PAGE) && self.lab_list.is_none() {
            if self.menu_edit_icao {
                self.icao_edit_key(code);
            } else {
                self.time_edit_key(code);
            }
            return;
        }
        if self.lab_place.is_some() && self.lab_list.is_none() {
            let field = match code {
                KeyCode::Digit1 | KeyCode::Numpad1 => Some(0),
                KeyCode::Digit2 | KeyCode::Numpad2 => Some(1),
                KeyCode::Digit3 | KeyCode::Numpad3 => Some(2),
                KeyCode::Digit4 | KeyCode::Numpad4 => Some(3),
                _ => None,
            };
            match code {
                KeyCode::Escape => self.lab_dialog_close(),
                KeyCode::Enter | KeyCode::NumpadEnter => self.lab_place_create(),
                _ => {
                    if let Some(i) = field {
                        self.lab_place_open(i);
                    }
                }
            }
            return;
        }
        if self.lab_list.is_some() {
            let (sel, len) = match self.ui.as_ref().and_then(|u| u.dialog.as_ref()) {
                Some(Dialog::Select { sel, options, .. }) => (*sel, options.len().max(1)),
                _ => (0, 1),
            };
            let set = |app: &mut Self, s: usize| {
                let vis = app.ui.as_ref().map_or(1, |u| u.dialog_vis);
                if let Some(Dialog::Select { sel, scroll, .. }) = app.ui.as_mut().and_then(|u| u.dialog.as_mut()) {
                    *sel = s;
                    if s < *scroll {
                        *scroll = s;
                    } else if s >= *scroll + vis {
                        *scroll = s + 1 - vis;
                    }
                }
            };
            let ctrl = self.keys.contains(&KeyCode::ControlLeft) || self.keys.contains(&KeyCode::ControlRight);
            let searching = self.lab_search();
            match code {
                KeyCode::Slash | KeyCode::NumpadDivide if searching.is_none() => self.lab_refilter(Some(String::new())),
                KeyCode::KeyF if ctrl && searching.is_none() => self.lab_refilter(Some(String::new())),
                KeyCode::Escape if searching.is_some() => self.lab_refilter(None),
                KeyCode::Escape => self.lab_dialog_close(),
                KeyCode::Backspace => {
                    if let Some(mut q) = searching {
                        q.pop();
                        self.lab_refilter(Some(q));
                    }
                }
                KeyCode::ArrowUp => set(self, (sel + len - 1) % len),
                KeyCode::ArrowDown => set(self, (sel + 1) % len),
                KeyCode::KeyW if searching.is_none() => set(self, (sel + len - 1) % len),
                KeyCode::KeyS if searching.is_none() => set(self, (sel + 1) % len),
                KeyCode::Enter | KeyCode::NumpadEnter => self.lab_dialog_pick(sel),
                KeyCode::Space if searching.is_none() => self.lab_dialog_pick(sel),
                _ => {}
            }
            return;
        }
        let Some(tab) = st.page else {
            let sels: Vec<usize> = (0..PAGE_COUNT).map(|p| self.lab_page_sel(p)).collect();
            let go = |p: usize| Some(PauseState { page: Some(p), sel: sels[p] });
            match code {
                KeyCode::Escape => self.close_game_menu(),
                KeyCode::ArrowUp | KeyCode::KeyW => {
                    let cnt = self.lab_entries().len();
                    self.lab_menu = Some(st.moved(cnt, -1));
                }
                KeyCode::ArrowDown | KeyCode::KeyS => {
                    let cnt = self.lab_entries().len();
                    self.lab_menu = Some(st.moved(cnt, 1));
                }
                KeyCode::Enter | KeyCode::NumpadEnter | KeyCode::Space => self.lab_activate(event_loop, st.sel),
                KeyCode::ArrowLeft | KeyCode::KeyA | KeyCode::KeyQ => self.lab_menu = go(n - 1),
                KeyCode::ArrowRight | KeyCode::KeyD | KeyCode::KeyE => {
                    self.lab_menu = go(0)
                }
                KeyCode::Digit1 | KeyCode::Numpad1 => self.lab_menu = go(0),
                KeyCode::Digit2 | KeyCode::Numpad2 => self.lab_menu = go(1),
                KeyCode::Digit3 | KeyCode::Numpad3 => self.lab_menu = go(2),
                KeyCode::Digit4 | KeyCode::Numpad4 => self.lab_menu = go(3),
                KeyCode::Digit5 | KeyCode::Numpad5 if n > ADMIN_PAGE => self.lab_menu = go(ADMIN_PAGE),
                _ => {}
            }
            return;
        };
        // a page
        let to = |p: usize| Some(PauseState { page: Some(p), ..st });
        match code {
            KeyCode::Escape if self.lab_map_direct => self.close_game_menu(),
            KeyCode::Escape => self.lab_menu = Some(PauseState { page: None, ..st }),
            KeyCode::Delete | KeyCode::Backspace => self.lab_key_clear_hovered(),
            KeyCode::ArrowLeft | KeyCode::KeyA | KeyCode::KeyQ => {
                self.lab_menu = to((tab + n - 1) % n)
            }
            KeyCode::ArrowRight | KeyCode::KeyD | KeyCode::KeyE => {
                self.lab_menu = to((tab + 1) % n)
            }
            KeyCode::Digit1 | KeyCode::Numpad1 => self.lab_menu = to(0),
            KeyCode::Digit2 | KeyCode::Numpad2 => self.lab_menu = to(1),
            KeyCode::Digit3 | KeyCode::Numpad3 => self.lab_menu = to(2),
            KeyCode::Digit4 | KeyCode::Numpad4 => self.lab_menu = to(3),
            KeyCode::Digit5 | KeyCode::Numpad5 if n > ADMIN_PAGE => self.lab_menu = to(ADMIN_PAGE),
            _ => {}
        }
    }

    pub(crate) fn lab_click(&mut self, event_loop: &ActiveEventLoop) {
        SYNC_NOW.with(|c| c.set(true));
        let (x, y) = self.cursor;
        let st = self.lab_menu.unwrap_or_default();
        let hit = |list: &[[f32; 4]]| {
            list.iter()
                .position(|r| x >= r[0] && x < r[2] && y >= r[1] && y < r[3])
        };
        if self.lab_load.is_some() {
            return;
        }
        if self.lab_place.is_some() && self.lab_list.is_none() {
            match self.ui.as_ref().and_then(|u| hit(&u.place_rects)) {
                Some(4) => self.lab_place_create(),
                Some(i) => self.lab_place_open(i),
                None => {
                    let (x, y) = self.cursor;
                    let b = self.ui.as_ref().map_or([0.0; 4], |u| u.place_box);
                    if !(x >= b[0] && x < b[2] && y >= b[1] && y < b[3]) {
                        self.lab_dialog_close();
                    }
                }
            }
            return;
        }
        if self.lab_list.is_some() {
            let pane = self.ui.as_ref().and_then(|u| {
                if u.menu_pane_go.as_ref().is_some_and(|g| hit(&[*g]).is_some()) {
                    return Some(usize::MAX);
                }
                if let Some(j) = hit(&u.menu_time) {
                    return Some(usize::MAX - 1 - j);
                }
                hit(&u.menu_pane).map(|i| i + u.menu_pane_start)
            });
            if let Some(i) = pane {
                self.lab_pane_click(i);
                return;
            }
            if self.ui.as_ref().is_some_and(|u| u.dialog_back && hit(&[u.dialog_back_rc]).is_some()) {
                self.lab_list = None;
                self.open_list(crate::game_lists::ListKind::Lines);
                self.lab_list_dialog("duty");
                return;
            }
            match self.ui.as_ref().and_then(|u| hit(&u.dialog_rects)) {
                Some(k) => self.lab_dialog_pick(k),
                None => {
                    let b = self.ui.as_ref().map_or([0.0; 4], |u| u.dialog_box);
                    if self.lab_place.is_none() && x >= b[0] && x < b[2] && y >= b[1] && y < b[3] {
                        return;
                    }

                    let other = match self.lab_place.is_some() {
                        true => self.ui.as_ref().and_then(|u| hit(&u.place_rects)).filter(|&i| i < 4),
                        false => None,
                    };
                    self.lab_dialog_close();
                    if let Some(i) = other {
                        self.lab_place_open(i);
                    }
                }
            }
            return;
        }
        if self.ui.as_ref().is_some_and(|u| u.world_drop.is_some()) {
            self.lab_world_drop_click();
            return;
        }
        if st.page.is_some() {
            if let Some(k) = self
                .ui
                .as_ref()
                .and_then(|u| hit(&u.lab_tabs))
            {
                self.lab_menu = Some(PauseState { page: Some(k), ..st });
            } else if st.page == Some(0) {
                if self.ui.as_ref().is_some_and(|u| u.map_btn_on && hit(&[u.map_btn]).is_some()) {
                    self.lab_duty_open(event_loop);
                }
            } else if st.page == Some(WORLD_PAGE) || st.page == Some(OPTIONS_PAGE) || st.page == Some(ADMIN_PAGE) {
                self.lab_world_click();
            } else if st.page == Some(VEHICLE_PAGE) {
                if let Some(k) = self.ui.as_ref().and_then(|u| hit(&u.lab_groups)) {
                    if let Some(u) = self.ui.as_mut() {
                        u.lab_group = k;
                    }
                } else if let Some(k) = self.ui.as_ref().and_then(|u| hit(&u.lab_actions)) {
                    let g = self.ui.as_ref().map_or(0, |u| u.lab_group);
                    let id = crate::game_lists::vehicle_menu(self)
                        .into_iter()
                        .nth(g)
                        .and_then(|(_, acts)| acts.into_iter().nth(k))
                        .map(|a| a.0);
                    if let Some(id) = id {
                        if matches!(id, "place" | "swap") {
                            if self.vehicle_list.is_empty() {
                                self.lab_load_vehicles(id);
                            } else {
                                self.lab_place_start(id);
                            }
                            return;
                        }
                        self.page_action(id);
                        if self.admin_list.is_some() {
                            self.lab_list_dialog(id);
                        }
                    }
                }
            }
        } else if let Some(k) = self.ui.as_ref().and_then(|u| hit(&u.pause_items)) {
            self.lab_menu = Some(PauseState { sel: k, ..st });
            self.lab_activate(event_loop, k);
        }
    }
}

pub(crate) fn scan_vehicles(
    root: std::path::PathBuf,
    map: String,
) -> std::sync::mpsc::Receiver<crate::app::VehicleScan> {
    let (tx, rx) = std::sync::mpsc::channel();
    std::thread::spawn(move || {
        let menu = crate::menu::Menu::new(&root, &map);
        let meta = menu.vehicles.iter().zip(menu.vehicle_meta).map(|(v, m)| (v.1.clone(), m)).collect();
        let _ = tx.send((menu.vehicles, meta));
    });
    rx
}
