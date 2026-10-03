//! The window's input events (focus, keyboard, mouse) as methods of `App`.

use super::*;

impl App {
    pub(super) fn on_focus_lost(&mut self) {
        self.finish_vr_nav_edit();
        self.window_focused = false;
        if let Some(ctl) = self.controllers.as_mut() {
            ctl.set_focus(false);
        }
        #[cfg(windows)]
        {
            self.vr_cursor_physical = None;
            self.vr_cursor_warp_pending = None;
        }
        // No key-up reaches us for whatever was held when focus left (alt-tab, a
        // click outside the window, an OS dialog popping up): without this, a held
        // modifier got "stuck" and made the next plain key press look like it was
        // held with that modifier - Shift got stuck this way once, and a plain `W`
        // (throttle in the wasd preset) was then read as Shift+W, OMSI's own wiper
        // key, toggling the wipers on every press instead of driving.
        self.keys.clear();
        if let Some(p) = self.player.as_mut() {
            p.axes.release_all();
        }
    }

    pub(super) fn on_window_key(
        &mut self,
        event_loop: &ActiveEventLoop,
        event: winit::event::KeyEvent,
    ) {
        if event.state == ElementState::Pressed && self.key_search {
            if let Some(text) = event.text.as_deref() {
                self.key_search_text(text);
            }
        }
        if event.state == ElementState::Pressed && self.menu_edit_icao {
            if let Some(text) = event.text.as_deref() {
                self.icao_edit_text(text);
            }
        }
        // '/' opens the chat's input box wherever the keyboard has it (the key
        // itself is then swallowed by the chat) - but not Numpad ÷, OMSI's stock
        // front door key (keyboard.cfg `bus_doorfront0 181`)
        // (only while `chat_open` is on its own key: one the player moved it to is
        // the only one, #130)
        if event.state == ElementState::Pressed
            && event.text.as_deref() == Some("/")
            && event.physical_key != PhysicalKey::Code(KeyCode::NumpadDivide)
            && self.game_keys.iter().any(|b| {
                b.action.eq_ignore_ascii_case("chat_open") && b.scan_code == 53 && b.chord() == 0
            })
            && self.lan.is_some()
            && !lan::chat_open(&self.remotes)
        {
            self.remotes.chat.open();
            if let PhysicalKey::Code(code) = event.physical_key {
                lan::chat_swallow(&mut self.remotes, code);
            }
            return;
        }
        if let (Some(text), true, true) = (
            event.text.as_deref(),
            event.state == ElementState::Pressed,
            lan::chat_open(&self.remotes),
        ) {
            lan::chat_type(&mut self.remotes, text);
        }
        if let (PhysicalKey::Code(code), false) = (event.physical_key, event.repeat) {
            if self.plugin_keys.len() < 64 {
                self.plugin_keys
                    .push((format!("{code:?}"), event.state == ElementState::Pressed));
            }
        }
        let physical = match event.physical_key {
            PhysicalKey::Code(KeyCode::BrowserBack) => PhysicalKey::Code(KeyCode::Escape),
            k => k,
        };
        if let PhysicalKey::Code(code) = physical {
            self.on_key(
                event_loop,
                code,
                event.state == ElementState::Pressed,
                event.repeat,
            );
        }
    }

    /// In VR right-click zooms; with mouse steering it first releases the steering.
    /// On the desktop a right-drag zooms, as in OMSI (`on_right`).
    pub(super) fn on_mouse_right(&mut self, state: ElementState) {
        if let Some(edit) = self.vr_nav_edit.as_mut() {
            edit.rotating = state == ElementState::Pressed;
            return;
        }
        if self
            .navigator
            .as_ref()
            .map(|n| n.map_open())
            .unwrap_or(false)
        {
            return;
        }
        if self.vr_active() {
            #[cfg(windows)]
            if state == ElementState::Pressed && self.game_menu.is_none() && self.chooser.is_none()
            {
                if self.mouse_drive {
                    self.set_mouse_drive(false);
                    self.service_msg = Some(("Mouse steering off".into(), 3.0));
                } else {
                    self.vr_zoom_active = !self.vr_zoom_active;
                }
            }
        } else {
            self.on_right(state == ElementState::Pressed);
        }
    }

    /// (the middle button - the wheel pressed - turns the view as well: OMSI's pan)
    pub(super) fn on_mouse_middle(&mut self, state: ElementState) {
        if self.vr_nav_edit.is_some() {
            return;
        }
        if self
            .navigator
            .as_ref()
            .map(|n| n.map_open())
            .unwrap_or(false)
        {
            return;
        }
        self.mouse_look = state == ElementState::Pressed;
        self.update_hover();
    }

    pub(super) fn on_mouse_wheel(&mut self, delta: winit::event::MouseScrollDelta) {
        let amount = match delta {
            winit::event::MouseScrollDelta::LineDelta(_, y) => y,
            winit::event::MouseScrollDelta::PixelDelta(p) => p.y as f32 / 40.0,
        };
        self.wheel(amount);
    }

    pub(super) fn on_cursor_moved(&mut self, position: winit::dpi::PhysicalPosition<f64>) {
        if self.vr_nav_edit.is_some() {
            return;
        }
        if let Some((x, y)) = self.cursor_hidden {
            if (position.x as f32 - x).abs() + (position.y as f32 - y).abs() > 8.0 {
                self.cursor_hidden = None;
                if let Some(win) = self.window.as_ref() {
                    win.set_cursor_visible(true);
                }
            }
        }
        if self.touch.enabled {
            self.finger_move(0, glam::Vec2::new(position.x as f32, position.y as f32));
        }
        #[cfg(windows)]
        let vr_cockpit = self.vr.is_some()
            && self.game_menu.is_none()
            && matches!(self.view.as_str(), "driver" | "pax");
        #[cfg(not(windows))]
        let vr_cockpit = false;
        if vr_cockpit && !self.mouse_look && !self.mouse_drive {
            #[cfg(windows)]
            self.on_vr_cursor_moved(position.x as f32, position.y as f32);
        } else {
            self.on_mouse_moved(position.x as f32, position.y as f32);
        }
    }

    pub(super) fn on_mouse_left(&mut self, event_loop: &ActiveEventLoop, state: ElementState) {
        if self.touch.enabled {
            let p = glam::Vec2::new(self.cursor.0, self.cursor.1);
            if state == ElementState::Pressed {
                self.finger_down(event_loop, 0, p);
            } else {
                self.finger_up(event_loop, 0, p, false);
            }
        } else {
            let pressed = state == ElementState::Pressed;
            self.buttons_held.0 = pressed;
            if pressed && self.buttons_held.1 && self.start_both_drag() {
                return;
            }
            if !pressed && self.both_drag.is_some() && !(self.buttons_held.1 && self.right_zooms())
            {
                self.both_drag = None;
                self.mouse_look = self.buttons_held.1;
                self.update_hover();
            }
            self.left_button(event_loop, pressed);
        }
    }
}
