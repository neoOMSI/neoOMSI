use crate::*;
use winit::event::MouseButton;

impl App {
    fn showcase_rect(&self) -> Option<[f32; 4]> {
        if !self.lab_room.as_ref()?.has_scene() {
            return None;
        }
        let u = self.ui.as_ref()?;
        let r = if self.lab_place.is_some() {
            u.place_preview_rect
        } else if u.start.hand {
            return None;
        } else {
            u.start.pic_rect
        };
        (r[2] > r[0] && r[3] > r[1]).then_some(r)
    }

    fn showcase_hover(&self) -> bool {
        let (x, y) = self.cursor;
        self.showcase_rect()
            .is_some_and(|r| x >= r[0] && x <= r[2] && y >= r[1] && y <= r[3])
    }

    pub(crate) fn showcase_wheel(&mut self, amount: f32) -> bool {
        if amount == 0.0 || !self.showcase_hover() {
            return false;
        }
        if let Some(room) = self.lab_room.as_mut() {
            room.zoom_by((1.0 - amount * 0.08).clamp(0.8, 1.25));
        }
        true
    }

    pub(crate) fn showcase_button(&mut self, button: MouseButton, pressed: bool) -> bool {
        let (x, y) = self.cursor;
        if !pressed {
            return match self.lab_room.as_mut() {
                Some(room) if button != MouseButton::Middle => room.end_grab(),
                _ => false,
            };
        }
        if !self.showcase_hover() {
            return false;
        }
        let Some(room) = self.lab_room.as_mut() else {
            return false;
        };
        match button {
            MouseButton::Left => room.begin_grab(x, y, false),
            MouseButton::Right => room.begin_grab(x, y, true),
            MouseButton::Middle => room.reset_view(),
            _ => return false,
        }
        true
    }

    pub(crate) fn showcase_moved(&mut self, x: f32, y: f32) {
        if let Some(room) = self.lab_room.as_mut() {
            room.drag_to(x, y);
        }
    }
}
