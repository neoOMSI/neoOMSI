//! The game menu (Esc), its lists, drop-downs and edit fields, and the pages behind it.

use super::*;

pub(crate) fn route_char(code: KeyCode) -> Option<char> {
    let name = format!("{code:?}");
    let c = name
        .strip_prefix("Digit")
        .or_else(|| name.strip_prefix("Numpad"))
        .or_else(|| name.strip_prefix("Key"))?;
    let mut chars = c.chars();
    let ch = chars.next()?;
    (chars.next().is_none() && ch.is_ascii_alphanumeric()).then(|| ch.to_ascii_uppercase())
}

pub(crate) const SERVER_GAME_MENU: [(&str, &str); 6] = [
    ("resume", "Resume"),
    ("options", "Options..."),
    ("vehicle", "Vehicle options..."),
    ("world", "World options..."),
    ("map", "City map"),
    ("quit", "Leave the server"),
];

pub(crate) fn on_server(args: &crate::Args) -> bool {
    args.lan_join
        .as_deref()
        .map(|t| ::network::ws::ws_url(t).is_some())
        .unwrap_or(false)
}

pub(crate) fn game_menu_for(args: &crate::Args) -> &'static [(&'static str, &'static str)] {
    if on_server(args) {
        &SERVER_GAME_MENU
    } else {
        &GAME_MENU
    }
}

pub(crate) const GAME_MENU: [(&str, &str); 11] = [
    ("resume", "Resume"),
    ("options", "Options..."),
    ("vehicle", "Vehicle options..."),
    ("world", "World options..."),
    ("map", "City map"),
    ("duty", "Line and tour..."),
    ("endduty", "End the tour"),
    ("save", "Save the situation"),
    ("saveslot", "Save to a new slot"),
    ("load", "Load the quicksave"),
    ("quit", "End the session"),
];

impl App {
    pub(crate) fn open_game_menu(&mut self) {
        self.menu_prev_pause = self.paused;
        if self.lan.is_none() {
            self.paused = true;
        }
        self.game_menu = Some(0);
        self.menu_top = None;
        self.menu_kbd = true;
        self.menu_drag = None;
    }

    pub(crate) fn close_game_menu(&mut self) {
        self.report_view = None;
        if self.menu_edit_icao {
            if let Some(w) = self.window.as_ref() {
                w.set_ime_allowed(false);
            }
            self.menu_edit_icao = false;
            self.menu_edit = None;
        }
        self.game_menu = None;
        self.menu_top = None;
        self.key_capture = None;
        self.key_search_stop();
        self.paused = self.menu_prev_pause;
    }

    pub(crate) fn open_list(&mut self, kind: crate::game_lists::ListKind) {
        crate::game_lists::forget_page_titles();
        self.dropdown = None;
        self.admin_list = Some(crate::game_lists::items(self, &kind));
        self.list_kind = Some(kind);
        self.chooser = Some(if self.is_heading(0) {
            self.chooser_next(0, 1)
        } else {
            0
        });
    }

    pub(crate) fn is_heading(&self, k: usize) -> bool {
        self.admin_list
            .as_ref()
            .and_then(|l| l.get(k))
            .is_some_and(|l| l.1 == crate::game_lists::HEADING || l.1 == "keysearch")
    }

    pub(crate) fn chooser_next(&self, sel: usize, step: usize) -> usize {
        let n = self
            .admin_list
            .as_ref()
            .unwrap_or(&self.vehicle_list)
            .len()
            .max(1);
        let mut k = sel;
        for _ in 0..n {
            k = (k + step) % n;
            if !self.is_heading(k) {
                break;
            }
        }
        k
    }

    pub(crate) fn chooser_adjust(&mut self, k: usize, dir: &str) {
        let Some(action) = self
            .admin_list
            .as_ref()
            .and_then(|l| l.get(k))
            .and_then(|l| l.1.strip_suffix(crate::game_lists::ADJUST))
            .map(|a| format!("{a} {dir}"))
        else {
            return;
        };
        if let Some(l) = self.admin_list.as_mut().and_then(|l| l.get_mut(k)) {
            l.1 = action;
        }
        self.chooser_pick(k);
    }

    pub(crate) fn icao_edit_key(&mut self, code: KeyCode) {
        match code {
            KeyCode::Escape => {
                self.menu_edit = None;
                self.menu_edit_icao = false;
                if let Some(w) = self.window.as_ref() {
                    w.set_ime_allowed(false);
                }
            }
            KeyCode::Backspace | KeyCode::Delete => {
                if let Some(d) = self.menu_edit.as_mut() {
                    d.pop();
                }
            }
            KeyCode::Enter | KeyCode::NumpadEnter => {
                self.apply_icao_edit();
                return;
            }
            _ => {}
        }
        self.refresh_list();
    }

    pub(crate) fn icao_edit_text(&mut self, text: &str) {
        if !self.menu_edit_icao {
            return;
        }
        if let Some(d) = self.menu_edit.as_mut() {
            for c in text.chars().filter(|c| c.is_ascii_alphabetic()) {
                if d.len() >= 4 {
                    break;
                }
                d.push(c.to_ascii_uppercase());
            }
        }
        self.refresh_list();
    }

    pub(crate) fn start_icao_edit(&mut self) {
        self.menu_edit = Some(String::new());
        self.menu_edit_icao = true;
        if let Some(w) = self.window.as_ref() {
            w.set_ime_allowed(true);
        }
    }

    pub(crate) fn apply_icao_edit(&mut self) {
        let code = self
            .menu_edit
            .take()
            .unwrap_or_default()
            .trim()
            .to_ascii_uppercase();
        self.menu_edit_icao = false;
        if let Some(w) = self.window.as_ref() {
            w.set_ime_allowed(false);
        }
        if code.len() == 4 && code.chars().all(|c| c.is_ascii_alphabetic()) {
            ::config::set_setting("gameplay", "metar_station", code.clone());
            let _ = ::config::save();
            self.metar_rx = None;
            self.metar_once = false;
            self.metar_next = 0.0;
            self.service_msg = Some((format!("METAR source: {code}"), 3.0));
        } else if !code.is_empty() {
            self.service_msg = Some(("ICAO must be exactly 4 letters".into(), 3.0));
        }
        self.refresh_list();
    }

    pub(crate) fn time_edit_key(&mut self, code: KeyCode) {
        let digit = match code {
            KeyCode::Digit0 | KeyCode::Numpad0 => Some('0'),
            KeyCode::Digit1 | KeyCode::Numpad1 => Some('1'),
            KeyCode::Digit2 | KeyCode::Numpad2 => Some('2'),
            KeyCode::Digit3 | KeyCode::Numpad3 => Some('3'),
            KeyCode::Digit4 | KeyCode::Numpad4 => Some('4'),
            KeyCode::Digit5 | KeyCode::Numpad5 => Some('5'),
            KeyCode::Digit6 | KeyCode::Numpad6 => Some('6'),
            KeyCode::Digit7 | KeyCode::Numpad7 => Some('7'),
            KeyCode::Digit8 | KeyCode::Numpad8 => Some('8'),
            KeyCode::Digit9 | KeyCode::Numpad9 => Some('9'),
            _ => None,
        };
        match code {
            KeyCode::Escape => self.menu_edit = None,
            KeyCode::Backspace | KeyCode::Delete => {
                if let Some(d) = self.menu_edit.as_mut() {
                    d.pop();
                }
            }
            KeyCode::Enter | KeyCode::NumpadEnter => {
                self.apply_time_edit();
                return;
            }
            _ => {
                if let (Some(c), Some(d)) = (digit, self.menu_edit.as_mut()) {
                    if d.len() < 6 {
                        d.push(c);
                    }
                }
            }
        }
        self.refresh_list();
    }

    /// A key while a route number is typed in the destination list (#836): letters and
    /// digits ("5E", "N41"), Backspace, Enter sets it, Escape drops it.
    pub(crate) fn route_edit_key(&mut self, code: KeyCode) {
        match code {
            KeyCode::Escape => self.menu_edit = None,
            KeyCode::Backspace | KeyCode::Delete => {
                if let Some(t) = self.menu_edit.as_mut() {
                    t.pop();
                }
            }
            KeyCode::Enter | KeyCode::NumpadEnter => {
                if let Some(t) = self.menu_edit.take() {
                    crate::game_lists::set_route_by_hand(self, &t);
                    self.close_game_menu();
                }
                return;
            }
            _ => {
                if let (Some(c), Some(t)) = (route_char(code), self.menu_edit.as_mut()) {
                    if t.chars().count() < 8 {
                        t.push(c);
                    }
                }
            }
        }
        self.refresh_list();
    }

    pub(crate) fn apply_time_edit(&mut self) {
        let Some(d) = self.menu_edit.take() else {
            return;
        };
        if !d.is_empty() {
            let mut c = d.clone();
            while c.len() < 6 {
                c.push('0');
            }
            let n = |a: usize| c[a..a + 2].parse::<i64>().unwrap_or(0);
            let (h, m, sec) = (n(0), n(2), n(4));
            if h > 23 || m > 59 || sec > 59 {
                self.service_msg = Some((
                    format!("{:02}:{:02}:{:02} is no time of day", h, m, sec),
                    3.0,
                ));
            } else if self
                .lan
                .as_ref()
                .is_some_and(|l| l.role == ::network::Role::Client)
            {
                self.service_msg = Some(("In a LAN session the host sets the clock".into(), 3.0));
            } else if self.real_time_locked() {
                self.service_msg = Some((
                    "The time cannot be changed while the real-time sync is on".into(),
                    3.0,
                ));
            } else {
                let t = self.clock.time;
                let day_start = t - t.rem_euclid(86400.0);
                self.shift_clock(day_start + (h * 3600 + m * 60 + sec) as f64 - t);
                self.service_msg = Some((format!("Clock: {:02}:{:02}:{:02}", h, m, sec), 3.0));
            }
        }
        self.refresh_list();
    }

    pub(crate) fn refresh_list(&mut self) {
        let Some(kind) = self.list_kind.clone() else {
            return;
        };
        let keep = self.chooser;
        self.open_list(kind);
        if let (Some(k), Some(l)) = (keep, self.admin_list.as_ref()) {
            self.chooser = Some(k.min(l.len().saturating_sub(1)));
        }
    }

    pub(crate) fn settings_list(&self) -> bool {
        use crate::game_lists::ListKind;
        self.chooser.is_some()
            && matches!(
                self.list_kind,
                Some(ListKind::Options(_) | ListKind::Vehicle(_) | ListKind::World(_))
            )
    }

    pub(crate) fn close_list(&mut self) {
        self.key_capture = None;
        self.key_search_stop();
        self.dropdown = None;
        if self.menu_edit_icao {
            if let Some(w) = self.window.as_ref() {
                w.set_ime_allowed(false);
            }
        }
        self.menu_edit_icao = false;
        self.menu_edit = None;
        self.chooser = None;
        self.admin_list = None;
        self.list_kind = None;
        self.menu_top = None;
    }

    /// Remembers the Options tab the map page is opened from, and sends "back" from the map page there.
    fn map_page_target(
        &mut self,
        from: &crate::game_lists::ListKind,
        next: crate::game_lists::ListKind,
    ) -> crate::game_lists::ListKind {
        use crate::game_lists::{ListKind, is_sub_tab};
        if matches!(from, ListKind::Options(t) if *t == crate::game_lists::KEYS_TAB)
            && *from != next
        {
            self.key_search_stop();
        }
        match (from, &next) {
            (ListKind::Options(t), ListKind::Options(n)) if is_sub_tab(*n) && !is_sub_tab(*t) => {
                self.map_return_tab = *t;
                self.key_filter.clear();
            }
            (ListKind::Options(t), ListKind::Options(0)) if is_sub_tab(*t) => {
                return ListKind::Options(self.map_return_tab);
            }
            _ => {}
        }
        next
    }

    /// The (section, entry, action) of the Keys page row `k`, if it is a key binding.
    fn keybind_row(&self, k: usize) -> Option<(usize, usize, String)> {
        let a = self
            .admin_list
            .as_ref()?
            .get(k)?
            .1
            .strip_prefix("keybind ")?
            .to_string();
        let mut it = a.splitn(3, ' ');
        Some((
            it.next()?.parse().ok()?,
            it.next()?.parse().ok()?,
            it.next()?.to_string(),
        ))
    }

    /// A key pressed while a binding waits for it (Escape leaves the binding as it is).
    pub(crate) fn capture_key(&mut self, code: KeyCode) {
        let Some((sec, idx)) = self.key_capture else {
            return;
        };
        if matches!(
            code,
            KeyCode::ShiftLeft
                | KeyCode::ShiftRight
                | KeyCode::ControlLeft
                | KeyCode::ControlRight
                | KeyCode::AltLeft
                | KeyCode::AltRight
                | KeyCode::SuperLeft
                | KeyCode::SuperRight
        ) {
            return;
        }
        let name = self.keybind_name(sec, idx);
        self.key_capture = None;
        if code == KeyCode::Escape {
            self.reopen_keys(sec, idx);
            return;
        }
        let Some(scan) = crate::keys::dik_code(code) else {
            self.service_msg = Some((
                format!("{code:?} has no key code the game understands"),
                3.0,
            ));
            self.reopen_keys(sec, idx);
            return;
        };
        let held = |a: KeyCode, b: KeyCode| self.keys.contains(&a) || self.keys.contains(&b);
        let m = ::content::input::chord(
            held(KeyCode::ShiftLeft, KeyCode::ShiftRight),
            held(KeyCode::ControlLeft, KeyCode::ControlRight),
            held(KeyCode::AltLeft, KeyCode::AltRight),
        ) as i64;
        if let Some(name) = name {
            self.keybind_edit(sec, idx, &name, KeyEdit::Set(scan as i64, m));
        }
    }

    /// The action name of the list row that is entry `idx` of section `sec`.
    fn keybind_name(&self, sec: usize, idx: usize) -> Option<String> {
        let list = self.admin_list.as_ref()?;
        (0..list.len())
            .filter_map(|k| self.keybind_row(k))
            .find(|r| r.0 == sec && r.1 == idx)
            .map(|r| r.2)
    }

    /// Opens the Keys page again with the row of entry `idx` of section `sec` selected.
    fn reopen_keys(&mut self, sec: usize, idx: usize) {
        let Some(kind) = self.list_kind.clone() else {
            return;
        };
        self.open_list(kind);
        let at = (0..self.admin_list.as_ref().map(|l| l.len()).unwrap_or(0)).find(|k| {
            self.keybind_row(*k)
                .is_some_and(|r| r.0 == sec && r.1 == idx)
        });
        if let Some(k) = at {
            self.chooser = Some(k);
        }
    }

    pub(crate) fn keybind_edit(&mut self, sec: usize, idx: usize, name: &str, edit: KeyEdit) {
        let section = ["vehicles", "game"][sec.min(1)];
        let mut v = match omsi_launcher_lib::get_keybindings() {
            Ok(v) => v,
            Err(e) => {
                self.service_msg = Some((format!("{e:#}"), 4.0));
                return;
            }
        };
        let target = idx;
        {
            let Some(arr) = v.get_mut(section).and_then(|a| a.as_array_mut()) else {
                return;
            };
            if arr
                .get(idx)
                .and_then(|b| b.get("action"))
                .and_then(|a| a.as_str())
                != Some(name)
            {
                self.reopen_keys(sec, idx);
                return;
            }
            let hold = arr[idx]
                .get("modifier")
                .and_then(|x| x.as_i64())
                .unwrap_or(0)
                & ::content::input::KEY_HOLD as i64;
            match edit {
                KeyEdit::Set(scan, m) => {
                    arr[idx]["scan_code"] = serde_json::json!(scan);
                    arr[idx]["modifier"] = serde_json::json!(m | hold);
                }
                KeyEdit::Clear => {
                    arr[idx]["scan_code"] = serde_json::json!(0);
                    arr[idx]["modifier"] = serde_json::json!(0);
                }
            }
        }
        if let Err(e) = omsi_launcher_lib::save_keybindings(&v) {
            self.service_msg = Some((format!("{e:#}"), 4.0));
            self.reopen_keys(sec, idx);
            return;
        }
        self.reload_keys();
        // (a key changed is a key the player wants: the ready-made layouts would ignore it)
        if sec == 0 && self.args.drive_keys != "omsi" {
            self.args.drive_keys = "omsi".into();
            ::config::set_setting("gameplay", "drive-keys", "omsi");
            let _ = ::config::save();
        }
        self.reopen_keys(sec, target);
    }

    /// Reads `keyboard.cfg` anew into what the running game uses.
    fn reload_keys(&mut self) {
        let path = crate::startup::keyboard_cfg(&self.args.root);
        self.game_keys = ::content::KeyboardCfg::load(&path)
            .unwrap_or_default()
            .with_game_defaults()
            .with_vr_defaults()
            .game;
        if let Ok(k) = ::content::KeyboardCfg::load(&path) {
            if let Some(p) = self.player.as_mut() {
                p.bindings = k.with_game_defaults().vehicles;
            }
        }
        self.own_keys = crate::startup::own_keys(&self.args.root);
        self.own_shift =
            crate::startup::own_bindings(&self.args.root, ::content::input::KEY_SHIFT);
    }

    /// The Keys page's rows anew (the search line shows whether it is being typed in), the
    /// row chosen and the scroll kept.
    fn rebuild_keys_rows(&mut self) {
        if !matches!(self.list_kind, Some(crate::game_lists::ListKind::Options(t)) if t == crate::game_lists::KEYS_TAB)
            || self.admin_list.is_none()
        {
            return;
        }
        if let Some(kind) = self.list_kind.clone() {
            let sel = self.chooser;
            self.admin_list = Some(crate::game_lists::items(self, &kind));
            self.chooser = sel;
        }
    }

    pub(crate) fn key_search_start(&mut self) {
        self.key_search = true;
        self.rebuild_keys_rows();
        if let Some(w) = self.window.as_ref() {
            w.set_ime_allowed(true);
        }
    }

    pub(crate) fn key_search_stop(&mut self) {
        let was = self.key_search;
        self.key_search = false;
        if was {
            self.rebuild_keys_rows();
        }
        if let Some(w) = self.window.as_ref() {
            w.set_ime_allowed(false);
        }
    }

    pub(crate) fn key_search_text(&mut self, text: &str) {
        // (a key being set is not search text)
        if self.key_capture.is_some() {
            return;
        }
        self.key_filter
            .extend(text.chars().filter(|c| !c.is_control()));
        self.refresh_keys_list();
    }

    /// A key pressed on the key bindings page (the letters come in as search text).
    fn key_search_key(&mut self, code: KeyCode) {
        let n = self
            .admin_list
            .as_ref()
            .map(|l| l.len())
            .unwrap_or(0)
            .max(1);
        let sel = self.chooser.unwrap_or(0);
        match code {
            KeyCode::Escape => self.key_search_stop(),
            KeyCode::Backspace => {
                self.key_filter.pop();
                self.refresh_keys_list();
            }
            KeyCode::ArrowUp => self.chooser = Some(self.chooser_next(sel, n - 1)),
            KeyCode::ArrowDown => self.chooser = Some(self.chooser_next(sel, 1)),
            KeyCode::PageUp => {
                self.chooser = Some(sel.saturating_sub(10)).map(|k| {
                    if self.is_heading(k) {
                        self.chooser_next(k, 1)
                    } else {
                        k
                    }
                })
            }
            KeyCode::PageDown => {
                self.chooser = Some((sel + 10).min(n - 1)).map(|k| {
                    if self.is_heading(k) {
                        self.chooser_next(k, 1)
                    } else {
                        k
                    }
                })
            }
            KeyCode::Enter | KeyCode::NumpadEnter if !self.is_heading(sel) => {
                self.chooser_pick(sel)
            }
            KeyCode::Delete if !self.is_heading(sel) => {
                if let Some((s, i, name)) = self.keybind_row(sel) {
                    self.keybind_edit(s, i, &name, KeyEdit::Clear);
                }
            }
            _ => {}
        }
    }

    /// The list anew after the search text changed: the first binding chosen, shown from the top.
    pub(crate) fn refresh_keys_list(&mut self) {
        let Some(kind) = self.list_kind.clone() else {
            return;
        };
        self.menu_top = None;
        self.open_list(kind);
    }

    pub(crate) fn settings_tab(&mut self, i: usize) {
        self.key_capture = None;
        self.key_search_stop();
        use crate::game_lists::ListKind;
        let next = match self.list_kind {
            Some(ListKind::Options(_)) => ListKind::Options(i),
            Some(ListKind::Vehicle(_)) => ListKind::Vehicle(i),
            Some(ListKind::World(_)) => ListKind::World(i),
            _ => return,
        };
        self.menu_top = None;
        self.menu_edit = None;
        self.open_list(next);
    }

    pub(crate) fn settings_tab_step(&mut self, forward: bool) {
        let Some(kind) = self.list_kind.clone() else {
            return;
        };
        let Some((titles, at)) = crate::game_lists::page_titles(self, &kind) else {
            return;
        };
        let n = titles.len().max(1);
        self.settings_tab(if forward {
            (at + 1) % n
        } else {
            (at + n - 1) % n
        });
    }

    pub(crate) fn settings_side_click(&mut self, i: usize) {
        let Some(kind) = self.list_kind.clone() else {
            return;
        };
        let n = crate::game_lists::page_titles(self, &kind)
            .map(|t| t.0.len())
            .unwrap_or(0);
        if i < n {
            self.settings_tab(i);
        } else if matches!(kind, crate::game_lists::ListKind::Options(t) if crate::game_lists::is_sub_tab(t))
        {
            self.key_search_stop();
            self.menu_top = None;
            self.open_list(crate::game_lists::ListKind::Options(self.map_return_tab));
            self.chooser = Some(0);
        } else {
            self.close_list();
        }
    }

    pub(crate) fn list_adjust(&mut self, k: usize, mv: crate::game_lists::Move) {
        use crate::game_lists::ListKind;
        let Some(kind) = self.list_kind.clone() else {
            return;
        };
        if !matches!(kind, ListKind::Options(_) | ListKind::World(_)) {
            return;
        }
        let Some(action) = self
            .admin_list
            .as_ref()
            .and_then(|l| l.get(k))
            .map(|x| x.1.clone())
        else {
            return;
        };
        let slider = crate::game_lists::is_slider(action.split(' ').next().unwrap_or(""));
        crate::game_lists::LIST_DIRTY.store(false, std::sync::atomic::Ordering::Relaxed);
        if let Some(next) = crate::game_lists::run_move(self, &kind, &action, mv) {
            let next = self.map_page_target(&kind, next);
            if slider
                && !crate::game_lists::LIST_DIRTY.swap(false, std::sync::atomic::Ordering::Relaxed)
            {
                return;
            }
            self.open_list(next);
            let last = self
                .admin_list
                .as_ref()
                .map(|l| l.len().saturating_sub(1))
                .unwrap_or(0);
            self.chooser = Some(k.min(last));
        }
    }

    pub(crate) fn list_click(&mut self, k: usize, fx: f32) -> bool {
        use crate::game_lists::Move;
        let Some(action) = self
            .admin_list
            .as_ref()
            .and_then(|l| l.get(k))
            .map(|x| x.1.clone())
        else {
            return false;
        };
        let verb = action.split(' ').next().unwrap_or("");
        let slider = crate::game_lists::is_slider(verb);
        let mv = if slider || verb == "mapopts" || verb == "lookopts" {
            Move::To(fx)
        } else if fx < 0.5 {
            Move::Dec
        } else {
            Move::Inc
        };
        self.list_adjust(k, mv);
        slider
    }

    pub(crate) fn chooser_key(&mut self, code: KeyCode) {
        if self.dropdown.is_some() {
            self.dropdown_key(code);
            return;
        }
        if self.menu_edit.is_some() {
            if self.menu_edit_icao {
                self.icao_edit_key(code);
            } else if matches!(
                self.list_kind,
                Some(crate::game_lists::ListKind::RouteNumbers)
            ) {
                self.route_edit_key(code);
            } else {
                self.time_edit_key(code);
            }
            return;
        }
        let n = self
            .admin_list
            .as_ref()
            .unwrap_or(&self.vehicle_list)
            .len()
            .max(1);
        let sel = self.chooser.unwrap_or(0);
        self.menu_top = None;
        match code {
            KeyCode::Escape => {
                if matches!(self.list_kind, Some(crate::game_lists::ListKind::Options(t)) if crate::game_lists::is_sub_tab(t))
                {
                    self.menu_top = None;
                    self.open_list(crate::game_lists::ListKind::Options(self.map_return_tab));
                    self.chooser = Some(0);
                } else if self.tours_list() {
                    self.open_list(crate::game_lists::ListKind::Lines);
                } else {
                    self.chooser = None;
                    self.admin_list = None;
                    self.list_kind = None;
                }
            }
            KeyCode::Delete | KeyCode::Backspace if self.keybind_row(sel).is_some() => {
                if let Some((s, i, name)) = self.keybind_row(sel) {
                    self.keybind_edit(s, i, &name, KeyEdit::Clear);
                }
            }
            KeyCode::ArrowUp | KeyCode::KeyW => self.chooser = Some(self.chooser_next(sel, n - 1)),
            KeyCode::ArrowDown | KeyCode::KeyS => self.chooser = Some(self.chooser_next(sel, 1)),
            KeyCode::ArrowLeft | KeyCode::KeyA if self.settings_list() => {
                self.list_adjust(sel, crate::game_lists::Move::Dec)
            }
            KeyCode::ArrowRight | KeyCode::KeyD if self.settings_list() => {
                self.list_adjust(sel, crate::game_lists::Move::Inc)
            }
            KeyCode::Minus | KeyCode::Slash | KeyCode::NumpadSubtract if self.tours_list() => {
                self.tour_stop_step(false)
            }
            KeyCode::Equal | KeyCode::BracketRight | KeyCode::NumpadAdd if self.tours_list() => {
                self.tour_stop_step(true)
            }
            KeyCode::ArrowLeft | KeyCode::KeyA if self.tours_list() => self.trip_step(false),
            KeyCode::ArrowRight | KeyCode::KeyD if self.tours_list() => self.trip_step(true),
            KeyCode::ArrowLeft | KeyCode::KeyA => self.chooser_adjust(sel, "-"),
            KeyCode::ArrowRight | KeyCode::KeyD => self.chooser_adjust(sel, "+"),
            KeyCode::PageUp if self.settings_list() => self.settings_tab_step(false),
            KeyCode::PageDown if self.settings_list() => self.settings_tab_step(true),
            KeyCode::PageUp => {
                self.chooser = Some(sel.saturating_sub(15)).map(|k| {
                    if self.is_heading(k) {
                        self.chooser_next(k, 1)
                    } else {
                        k
                    }
                })
            }
            KeyCode::PageDown => {
                self.chooser = Some((sel + 15).min(n - 1)).map(|k| {
                    if self.is_heading(k) {
                        self.chooser_next(k, 1)
                    } else {
                        k
                    }
                })
            }
            KeyCode::Enter | KeyCode::NumpadEnter | KeyCode::Space => self.chooser_pick(sel),
            _ => {}
        }
    }

    pub(crate) fn dropdown_key(&mut self, code: KeyCode) {
        let Some(d) = self.dropdown.as_mut() else {
            return;
        };
        let n = d.items.len().max(1);
        match code {
            KeyCode::Escape => {
                self.dropdown = None;
                return;
            }
            KeyCode::ArrowUp | KeyCode::KeyW => d.sel = (d.sel + n - 1) % n,
            KeyCode::ArrowDown | KeyCode::KeyS => d.sel = (d.sel + 1) % n,
            KeyCode::PageUp => d.sel = d.sel.saturating_sub(5),
            KeyCode::PageDown => d.sel = (d.sel + 5).min(n - 1),
            KeyCode::Home => d.sel = 0,
            KeyCode::End => d.sel = n - 1,
            KeyCode::Enter | KeyCode::NumpadEnter | KeyCode::Space => {
                let i = d.sel;
                self.dropdown_pick(i);
                return;
            }
            _ => return,
        }
        self.dd_reveal();
    }

    pub(crate) fn dd_reveal(&mut self) {
        let rows = self.ui.as_ref().map(|u| u.dd_rows).unwrap_or(8).max(1);
        if let Some(d) = self.dropdown.as_mut() {
            if d.sel < d.top {
                d.top = d.sel;
            } else if d.sel >= d.top + rows {
                d.top = d.sel + 1 - rows;
            }
        }
    }

    pub(crate) fn dropdown_pick(&mut self, i: usize) {
        let Some(d) = self.dropdown.take() else {
            return;
        };
        let Some((_, action)) = d.items.get(i).cloned() else {
            return;
        };
        crate::game_lists::dropdown_apply(self, &action);
        if let Some(kind) = self.list_kind.clone() {
            self.open_list(kind);
            let last = self
                .admin_list
                .as_ref()
                .map(|l| l.len().saturating_sub(1))
                .unwrap_or(0);
            self.chooser = Some(d.row.min(last));
        }
    }

    pub(crate) fn tours_list(&self) -> bool {
        self.chooser.is_some()
            && matches!(self.list_kind, Some(crate::game_lists::ListKind::Tours(..)))
    }

    pub(crate) fn trip_step(&mut self, forward: bool) {
        let k = self.chooser.unwrap_or(0);
        let (Some((line, tour)), Some((_, _, trip, trips))) = (
            crate::game_lists::tour_at(self, k),
            crate::game_lists::tour_choice(self, k),
        ) else {
            return;
        };
        let to = if forward {
            (trip + 1).min(trips.saturating_sub(1))
        } else {
            trip.saturating_sub(1)
        };
        if to != trip {
            self.pane_scroll = None;
            self.list_kind = Some(crate::game_lists::ListKind::Tours(
                line,
                Some((tour, 0, to)),
            ));
        }
    }

    pub(crate) fn tour_stop_step(&mut self, forward: bool) {
        let k = self.chooser.unwrap_or(0);
        let (Some((line, tour)), Some((n, at, trip, _))) = (
            crate::game_lists::tour_at(self, k),
            crate::game_lists::tour_choice(self, k),
        ) else {
            return;
        };
        let to = if forward {
            (at + 1).min(n - 1)
        } else {
            at.saturating_sub(1)
        };
        self.pane_scroll = None;
        self.list_kind = Some(crate::game_lists::ListKind::Tours(
            line,
            Some((tour, to, trip)),
        ));
    }

    pub(crate) fn tour_pane_click(&mut self, i: usize) {
        let k = self.chooser.unwrap_or(0);
        if i == usize::MAX - 1 || i == usize::MAX - 2 {
            self.trip_step(i == usize::MAX - 2);
            return;
        }
        let (Some((line, tour)), Some((n, at, trip, _))) = (
            crate::game_lists::tour_at(self, k),
            crate::game_lists::tour_choice(self, k),
        ) else {
            return;
        };
        if i < n {
            self.pane_scroll = None;
            self.list_kind = Some(crate::game_lists::ListKind::Tours(
                line,
                Some((tour, i, trip)),
            ));
            return;
        }
        crate::game_lists::start_duty_at(self, &line, &tour, trip, at);
        self.chooser = None;
        self.admin_list = None;
        self.list_kind = None;
        self.close_game_menu();
    }

    pub(crate) fn chooser_pick(&mut self, k: usize) {
        if self.is_heading(k) {
            return;
        }
        if self.settings_list() {
            let id = self
                .admin_list
                .as_ref()
                .and_then(|l| l.get(k))
                .map(|l| l.1.clone())
                .unwrap_or_default();
            if let Some(d) = crate::game_lists::dropdown_for(self, k, &id) {
                self.dropdown = Some(d);
                self.dd_reveal();
                return;
            }
        }
        self.chooser = None;
        if let Some(list) = self.admin_list.take() {
            let kind = self
                .list_kind
                .take()
                .unwrap_or(crate::game_lists::ListKind::Admin);
            let Some((_, action)) = list.get(k).cloned() else {
                return;
            };
            match crate::game_lists::run(self, &kind, &action) {
                Some(next) => {
                    let next = self.map_page_target(&kind, next);
                    let keep = next == kind;
                    if !keep {
                        self.menu_top = None;
                    }
                    self.open_list(next);
                    if keep {
                        self.chooser = Some(
                            k.min(
                                self.admin_list
                                    .as_ref()
                                    .map(|l| l.len().saturating_sub(1))
                                    .unwrap_or(0),
                            ),
                        );
                    }
                }
                None if action != "back"
                    && matches!(
                        kind,
                        crate::game_lists::ListKind::Tours(..)
                            | crate::game_lists::ListKind::Numbers
                            | crate::game_lists::ListKind::Destinations
                            | crate::game_lists::ListKind::RouteNumbers
                            | crate::game_lists::ListKind::Hofs
                            | crate::game_lists::ListKind::Spots
                    ) =>
                    {
                        self.close_game_menu()
                    }
                None => self.menu_top = None,
            }
            return;
        }
        self.menu_top = None;
        let Some((_, bus)) = self.vehicle_list.get(k).cloned() else {
            return;
        };
        self.open_list(crate::game_lists::ListKind::PlaceLivery(bus));
    }

    pub(crate) fn menu_key(&mut self, event_loop: &ActiveEventLoop, code: KeyCode) {
        self.menu_kbd = true;
        if self.report_view.is_some() {
            self.run_report_key(event_loop, code);
            return;
        }
        if self.key_capture.is_some() {
            self.capture_key(code);
            return;
        }
        if self.key_search
            && matches!(
                code,
                KeyCode::ArrowUp | KeyCode::ArrowDown | KeyCode::Enter | KeyCode::NumpadEnter
            )
        {
            self.key_search_stop();
        }
        if self.key_search {
            self.key_search_key(code);
            return;
        }
        if self.chooser.is_some() {
            self.chooser_key(code);
            return;
        }
        let n = self.game_menu_items().len();
        let sel = self.game_menu.unwrap_or(0);
        let modified = self.keys.iter().any(|key| {
            matches!(
                *key,
                KeyCode::ControlLeft
                    | KeyCode::ControlRight
                    | KeyCode::AltLeft
                    | KeyCode::AltRight
                    | KeyCode::ShiftLeft
                    | KeyCode::ShiftRight
            )
        });
        self.menu_top = None;
        match code {
            KeyCode::KeyP if !modified => self.toggle_pause(),
            KeyCode::Escape => self.close_game_menu(),
            KeyCode::ArrowUp | KeyCode::KeyW => {
                self.game_menu = Some(self.menu_step(sel, n, false))
            }
            KeyCode::ArrowDown | KeyCode::KeyS => {
                self.game_menu = Some(self.menu_step(sel, n, true))
            }
            KeyCode::Enter | KeyCode::NumpadEnter | KeyCode::Space => {
                self.menu_choose(event_loop, sel)
            }
            _ => {}
        }
    }

    pub(crate) fn menu_wheel(&mut self, amount: f32) {
        if self.dropdown.is_some() {
            self.wheel_acc += amount;
            let steps = self.wheel_acc.trunc() as i64;
            if steps == 0 {
                return;
            }
            self.wheel_acc -= steps as f32;
            let rows = self.ui.as_ref().map(|u| u.dd_rows).unwrap_or(8);
            if let Some(d) = self.dropdown.as_mut() {
                let max = d.items.len().saturating_sub(rows) as i64;
                d.top = (d.top as i64 - steps).clamp(0, max) as usize;
            }
            return;
        }
        self.wheel_acc += amount;
        let steps = self.wheel_acc.trunc() as i64;
        if steps == 0 {
            return;
        }
        self.wheel_acc -= steps as f32;
        if let (Some(u), Some(k)) = (self.ui.as_ref(), self.chooser) {
            let (x, y) = self.cursor;
            if u.menu_pane_box
                .is_some_and(|r| x >= r[0] && x <= r[2] && y >= r[1] && y <= r[3])
            {
                let first = (u.menu_pane_start as i64 - steps).max(0) as usize;
                self.pane_scroll = Some((k, first));
                return;
            }
        }
        let n = self.menu_len() as f32;
        let (start, rows) = self
            .ui
            .as_ref()
            .map(|u| (u.menu_start as f32, u.menu_rows as f32))
            .unwrap_or((0.0, n));
        let top = (self.menu_top.unwrap_or(start) - steps as f32).clamp(0.0, (n - rows).max(0.0));
        self.menu_top = Some(top);
    }

    pub(crate) fn menu_len(&self) -> usize {
        if let Some(report) = self.report_view.as_ref() {
            return report.trip.stops.len();
        }
        match self.chooser {
            Some(_) => self.admin_list.as_ref().unwrap_or(&self.vehicle_list).len(),
            None => self.game_menu_items().len(),
        }
    }

    pub(crate) fn menu_choose(&mut self, event_loop: &ActiveEventLoop, k: usize) {
        if self.report_view.is_some() {
            match k {
                0 => self.save_run_report(),
                1 => self.close_game_menu(),
                _ => {}
            }
            return;
        }
        self.menu_top = if self.chooser.is_some() {
            self.ui.as_ref().map(|u| u.menu_start as f32)
        } else {
            None
        };
        if self.chooser.is_some() {
            self.chooser_pick(k);
            return;
        }
        if self.menu_item_off(k) {
            return;
        }
        let Some(id) = self.game_menu_items().get(k).map(|m| m.0) else {
            return;
        };
        match id {
            "report_current" => self.open_run_report(true),
            "report_last" => self.open_run_report(false),
            "resume" => self.close_game_menu(),
            "screenshot" => self.enter_screenshot_mode(),
            "options" => self.open_list(crate::game_lists::ListKind::Options(0)),
            "vehicle" => self.open_list(crate::game_lists::ListKind::Vehicle(0)),
            "world" => self.open_list(crate::game_lists::ListKind::World(0)),
            "copycode" => {
                self.close_game_menu();
                self.copy_server_code();
            }
            "admin" => self.open_list(crate::game_lists::ListKind::Admin),
            "duty" => self.open_list(crate::game_lists::ListKind::Lines),
            "map" => {
                self.close_game_menu();
                if let Some(n) = self.navigator.as_mut() {
                    if !n.map_open() {
                        n.toggle_map();
                    }
                }
            }
            "save" => {
                self.quick_save();
                self.close_game_menu();
            }
            "saveslot" => {
                self.save_slot();
                self.close_game_menu();
            }
            "endduty" => {
                self.duty = None;
                self.service_msg = Some(("Free drive: no duty".into(), 4.0));
                self.close_game_menu();
            }
            "tobus" => {
                self.close_game_menu();
                self.back_to_bus();
            }
            "load" => {
                self.game_menu = None;
                if self.load_quicksave() {
                    self.finish_session();
                    crate::platform::exit(event_loop);
                }
            }
            "quit" => {
                self.game_menu = None;
                self.finish_session();
                crate::platform::exit(event_loop);
            }
            other => {
                self.page_action(other);
            }
        }
    }

    pub(crate) fn page_action(&mut self, id: &str) -> bool {
        match id {
            "swap" | "place" => {
                self.swap_pending = id == "swap" && self.player.is_some();
                if self.vehicle_list.is_empty() {
                    let menu = crate::menu::Menu::new(&self.args.root, &self.args.map);
                    self.vehicle_meta = menu
                        .vehicles
                        .iter()
                        .zip(menu.vehicle_meta)
                        .map(|(v, meta)| (v.1.clone(), meta))
                        .collect();
                    self.vehicle_list = menu.vehicles;
                    self.vehicle_list.sort_by_key(|v| v.0.to_lowercase());
                }
                if self.vehicle_list.is_empty() {
                    self.service_msg = Some(("No vehicles found".into(), 3.0));
                } else {
                    self.open_list(crate::game_lists::ListKind::PlaceMaker);
                }
            }
            "couple" => {
                self.close_game_menu();
                self.couple();
            }
            "uncouple" => {
                self.close_game_menu();
                self.uncouple();
            }
            "tobus" => {
                self.close_game_menu();
                self.back_to_bus();
            }
            "remove" => {
                self.close_game_menu();
                self.remove_driven_vehicle();
            }
            "reload" => {
                self.close_game_menu();
                self.reload_driven_vehicle();
            }
            "clearplaced" => {
                self.close_game_menu();
                self.remove_placed_vehicles();
            }
            "getout" => {
                self.close_game_menu();
                self.get_up();
            }
            "reset_vehicle" => {
                self.close_game_menu();
                if let Some(p) = self.player.as_ref() {
                    let (at, heading) = (p.vehicle.position, p.vehicle.heading);
                    crate::admin::teleport(self, at, heading);
                    self.service_msg = Some(("The vehicle stands on its wheels again".into(), 3.0));
                }
            }
            "teleport" => {
                self.close_game_menu();
                if let Some(n) = self.navigator.as_mut() {
                    if !n.map_open() {
                        n.toggle_map();
                    }
                    self.teleport_pick = true;
                    self.service_msg = Some((
                        "Click a street on the map: the bus is put there".into(),
                        6.0,
                    ));
                }
            }
            "driver" => self.open_list(crate::game_lists::ListKind::Drivers),
            "number" => self.open_list(crate::game_lists::ListKind::Numbers),
            "dest" => self.open_list(crate::game_lists::ListKind::Destinations),
            "hof" => self.open_list(crate::game_lists::ListKind::Hofs),
            "tplist" => self.open_list(crate::game_lists::ListKind::Spots),
            "editor" => {
                self.close_game_menu();
                self.toggle_editor();
            }
            "timetable" => {
                self.timetable = !self.timetable;
                self.close_game_menu();
            }
            "info" => {
                self.info_bar = !self.info_bar;
                self.close_game_menu();
            }
            "refuel" | "wash" | "repair" => {
                self.close_game_menu();
                self.run_service(id);
            }
            "weather" => {
                self.close_game_menu();
                self.next_weather();
            }
            "metar_once" => self.load_metar_once(),
            "metar_refresh" => self.refresh_metar_now(),
            "weather_custom" => self.current_weather_as_custom(),
            "switch" => {
                self.close_game_menu();
                self.switch_vehicle();
            }
            "later" | "earlier" | "later10" | "earlier10" => {
                self.close_game_menu();
                if self
                    .lan
                    .as_ref()
                    .map(|l| l.role == ::network::Role::Client)
                    .unwrap_or(false)
                {
                    self.service_msg =
                        Some(("In a LAN session the host sets the clock".into(), 3.0));
                } else {
                    self.shift_clock(match id {
                        "later" => 3600.0,
                        "earlier" => -3600.0,
                        "later10" => 600.0,
                        _ => -600.0,
                    });
                }
            }
            _ => return false,
        }
        true
    }

    pub(crate) fn game_menu_items(&self) -> Vec<(&'static str, &'static str)> {
        if self.report_view.is_some() {
            return vec![("report_save", "Save as text…"), ("resume", "Continue")];
        }
        let mut v: Vec<(&'static str, &'static str)> = game_menu_for(&self.args).to_vec();
        if self.duty.is_some() {
            v.insert(1, ("report_current", "Current trip evaluation…"));
        }
        if self.last_report.is_some() {
            v.insert(1, ("report_last", "Last trip evaluation…"));
        }
        if self.lan.is_none() {
            v.insert(1, ("screenshot", "Screenshot mode"));
        }
        let mut at = 1;
        if self.on_foot.is_some() && self.player.is_some() {
            v.insert(at, ("tobus", "Back to my bus"));
            at += 1;
        }
        if self.player.is_none() {
            v.retain(|x| x.0 != "duty");
        }
        if self.duty.is_none() {
            v.retain(|x| x.0 != "endduty");
        }
        if self.navigator.is_none() {
            v.retain(|x| x.0 != "map");
        }
        let host = self
            .lan
            .as_ref()
            .map(|l| l.role == ::network::Role::Host)
            .unwrap_or(false);
        if self.lan.is_some() || on_server(&self.args) {
            if let Some(w) = v.iter().position(|x| x.0 == "world") {
                v.insert(w + 1, ("copycode", "Copy server code"));
                at = at.max(w + 2);
            }
        }
        if host || self.is_admin {
            let before_quit = v
                .iter()
                .position(|x| x.0 == "quit")
                .unwrap_or(v.len())
                .max(at);
            v.insert(before_quit, ("admin", "Administration..."));
        }
        v
    }

    /// Enter a clean, paused free-camera view for screenshots.
    pub(crate) fn enter_screenshot_mode(&mut self) {
        self.close_game_menu();
        self.screenshot_mode = Some(crate::ScreenshotMode {
            view: self.view.clone(),
            ego: self.ego,
            paused: self.paused,
            help_left: 5.0,
        });
        if self.view != "free" {
            if let (Some(cam), Some(p)) = (self.camera.as_mut(), self.player.as_ref()) {
                let h = (p.vehicle.heading as f32).to_radians();
                cam.position = p.vehicle.position
                    + glam::DVec3::new(-(h.sin() as f64) * 25.0, -(h.cos() as f64) * 25.0, 30.0);
                cam.yaw = p.vehicle.heading as f32;
                cam.pitch = -45.0;
            }
        }
        self.view = "free".into();
        self.ego = false;
        self.paused = true;
        self.cursor_hidden = Some(self.cursor);
        if let Some(win) = self.window.as_ref() {
            win.set_cursor_visible(false);
        }
    }

    /// Leave screenshot mode and put the player back where they were.
    pub(crate) fn leave_screenshot_mode(&mut self) {
        let Some(mode) = self.screenshot_mode.take() else {
            return;
        };
        self.view = mode.view;
        self.ego = mode.ego;
        self.paused = mode.paused;
        self.cursor_hidden = None;
        if let Some(win) = self.window.as_ref() {
            win.set_cursor_visible(true);
        }
    }

    pub(crate) fn copy_server_code(&mut self) {
        let Some(code) = self
            .lan
            .as_ref()
            .and_then(|l| l.code())
            .map(|c| c.encode())
            .or_else(|| self.args.lan_join.clone())
            .filter(|c| !c.trim().is_empty())
        else {
            self.service_msg = Some((
                "No server code: not in a LAN session or on a server".into(),
                3.0,
            ));
            return;
        };
        #[cfg(not(target_os = "android"))]
        {
            thread_local! {
                static CLIPBOARD: std::cell::RefCell<Option<arboard::Clipboard>> = const { std::cell::RefCell::new(None) };
            }
            let ok = CLIPBOARD.with(|c| {
                let mut c = c.borrow_mut();
                if c.is_none() {
                    *c = arboard::Clipboard::new().ok();
                }
                c.as_mut()
                    .is_some_and(|cb| cb.set_text(code.clone()).is_ok())
            });
            self.service_msg = Some(if ok {
                ("Server code copied".into(), 3.0)
            } else {
                (format!("{}: {code}", ::user_interface::tr("Server code")), 8.0)
            });
        }
        #[cfg(target_os = "android")]
        {
            self.service_msg = Some((format!("{}: {code}", ::user_interface::tr("Server code")), 8.0));
        }
    }

    pub(crate) fn menu_disabled_ids(&self) -> &'static [&'static str] {
        &[]
    }

    pub(crate) fn menu_item_off(&self, k: usize) -> bool {
        self.chooser.is_none()
            && self
            .game_menu_items()
            .get(k)
            .is_some_and(|m| self.menu_disabled_ids().contains(&m.0))
    }

    pub(crate) fn menu_step(&self, from: usize, n: usize, down: bool) -> usize {
        let mut k = from;
        for _ in 0..n {
            k = if down { (k + 1) % n } else { (k + n - 1) % n };
            if !self.menu_item_off(k) {
                return k;
            }
        }
        from
    }
}

/// What the Keys page does to a binding.
#[derive(Clone, Copy)]
pub(crate) enum KeyEdit {
    Set(i64, i64),
    Clear,
}