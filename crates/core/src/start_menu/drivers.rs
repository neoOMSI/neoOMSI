use super::*;

const NAME_MAX: usize = 24;

impl App {
    fn drv_refresh(&mut self) {
        let names = omsi_launcher_lib::list_profiles().unwrap_or_default();
        if let Some(m) = self.menu.as_mut() {
            m.drv_names = names;
        }
    }

    fn drv_close(&mut self) {
        if let Some(m) = self.menu.as_mut() {
            m.drv_open = false;
            m.drv_new = None;
            m.drv_confirm = None;
            m.drv_note.clear();
        }
    }

    fn drv_set_current(&mut self, name: &str) {
        let mut c = omsi_launcher_lib::load_config();
        c.profile = name.to_string();
        if let Err(e) = omsi_launcher_lib::save_config(&c) {
            log::warn!("driver: cannot save the config: {e:#}");
            if let Some(m) = self.menu.as_mut() {
                m.drv_note = e.to_string();
            }
        }
        profile::invalidate();
    }

    fn drv_create(&mut self) {
        let name = self.menu.as_ref().and_then(|m| m.drv_new.clone()).unwrap_or_default();
        let name = name.trim().to_string();
        if name.is_empty() {
            return;
        }
        match omsi_launcher_lib::create_profile(&name, "M") {
            Ok(_) => {
                profile::invalidate();
                self.drv_close();
            }
            Err(e) => {
                log::warn!("driver: cannot create {name}: {e:#}");
                if let Some(m) = self.menu.as_mut() {
                    m.drv_note = e.to_string();
                }
            }
        }
    }

    fn drv_delete(&mut self, i: usize) {
        let Some(name) = self.menu.as_ref().and_then(|m| m.drv_names.get(i).cloned()) else {
            return;
        };
        if self.menu.as_ref().is_some_and(|m| m.drv_confirm != Some(i)) {
            if let Some(m) = self.menu.as_mut() {
                m.drv_confirm = Some(i);
                m.drv_new = None;
                m.drv_note.clear();
            }
            return;
        }
        if let Some(m) = self.menu.as_mut() {
            m.drv_confirm = None;
        }
        if let Err(e) = omsi_launcher_lib::delete_profile(&name) {
            log::warn!("driver: cannot delete {name}: {e:#}");
            if let Some(m) = self.menu.as_mut() {
                m.drv_note = e.to_string();
            }
            return;
        }
        self.drv_refresh();
        let current = omsi_launcher_lib::load_config().profile;
        if current.eq_ignore_ascii_case(&name) {
            let next = self
                .menu
                .as_ref()
                .and_then(|m| m.drv_names.iter().find(|n| !n.eq_ignore_ascii_case(&name)).cloned())
                .unwrap_or_default();
            self.drv_set_current(&next);
        }
        profile::invalidate();
    }

    /// A click while the profile button or its dropdown is there; true when it was used.
    pub(super) fn drv_click(&mut self, cursor: (f32, f32)) -> bool {
        let Some(s) = self.ui.as_ref().map(|u| {
            let s = &u.start;
            (s.profile_btn, s.drv_btn, s.drv_dels.clone(), s.drv_items.clone(), s.drv_create, s.drv_new_row, s.drv_box, s.drv_first)
        }) else {
            return false;
        };
        let (pbtn, btn, dels, items, create, new_row, bx, first) = s;
        let open = self.menu.as_ref().is_some_and(|m| m.drv_open);
        if hit(&[pbtn], cursor).is_some() && hit(&[btn], cursor).is_none() {
            self.drv_close();
            self.start_set_page(StartPage::Profile);
            return true;
        }
        if hit(&[btn], cursor).is_some() {
            if open {
                self.drv_close();
            } else {
                self.drv_refresh();
                if let Some(m) = self.menu.as_mut() {
                    m.drv_open = true;
                    m.drv_new = None;
                    m.drv_confirm = None;
                    m.drv_note.clear();
                }
            }
            return true;
        }
        if !open {
            return false;
        }
        if let Some(k) = hit(&dels, cursor) {
            self.drv_delete(first + k);
        } else if let Some(k) = hit(&items, cursor) {
            let name = self.menu.as_ref().and_then(|m| m.drv_names.get(first + k).cloned());
            if let Some(name) = name {
                self.drv_set_current(&name);
            }
            self.drv_close();
        } else if hit(&[create], cursor).is_some() {
            self.drv_create();
        } else if hit(&[new_row], cursor).is_some() {
            if let Some(m) = self.menu.as_mut() {
                m.drv_new = Some(String::new());
                m.drv_confirm = None;
                m.drv_note.clear();
            }
        } else if hit(&[bx], cursor).is_none() {
            self.drv_close();
        }
        true
    }

    /// A key while the dropdown is open; true when it was used.
    pub(super) fn drv_key(&mut self, code: KeyCode) -> bool {
        use KeyCode as K;
        let Some((open, typing)) = self.menu.as_ref().map(|m| (m.drv_open, m.drv_new.is_some())) else {
            return false;
        };
        if !open {
            return false;
        }
        match code {
            K::Escape => {
                if typing {
                    if let Some(m) = self.menu.as_mut() {
                        m.drv_new = None;
                    }
                } else {
                    self.drv_close();
                }
                true
            }
            K::Enter | K::NumpadEnter => {
                if typing {
                    self.drv_create();
                }
                true
            }
            K::Backspace if typing => {
                if let Some(m) = self.menu.as_mut() {
                    if let Some(t) = m.drv_new.as_mut() {
                        t.pop();
                    }
                }
                true
            }
            _ => typing,
        }
    }

    /// Typed text while a new driver's name is being written; true when it was used.
    pub(super) fn drv_text(&mut self, text: &str) -> bool {
        let Some(m) = self.menu.as_mut().filter(|m| m.drv_open && m.drv_new.is_some()) else {
            return false;
        };
        if let Some(t) = m.drv_new.as_mut() {
            t.extend(text.chars().filter(|c| !c.is_control() && !"/\\:*?\"<>|".contains(*c)));
            *t = t.chars().take(NAME_MAX).collect();
        }
        true
    }
}
