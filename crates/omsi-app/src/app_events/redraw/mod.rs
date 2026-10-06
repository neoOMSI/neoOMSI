//! One frame (`RedrawRequested`), in phases that run in this order.

mod ai_traffic;
mod camera;
mod environment;
mod player;
mod render;
mod setup;
mod world;

use super::*;

/// What the phases after the frame timing need to know about the frame.
#[derive(Clone, Copy)]
pub(super) struct Frame {
    pub now: Instant,
    pub raw_dt: f32,
    pub dt: f32,
}

impl App {
    // the order matters: the player's vehicle moves before the people are placed in it, and the
    // day's light made by the environment is the picture's
    pub(super) fn redraw(&mut self, event_loop: &ActiveEventLoop) {
        #[cfg(not(target_os = "android"))]
        self.update_discord(self.starting.is_some());
        if !self.redraw_begin(event_loop) {
            return;
        }
        let Some(f) = self.redraw_timing(event_loop) else {
            return;
        };
        self.redraw_traffic(&f);
        self.redraw_player(&f);
        self.redraw_world(event_loop, &f);
        self.redraw_camera(&f);
        let daylight = self.redraw_environment(&f);
        self.redraw_render(event_loop, &f, daylight);
    }
}
