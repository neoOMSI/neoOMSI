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
        let entered = Instant::now();
        #[cfg(not(target_os = "android"))]
        self.update_discord(self.starting.is_some());
        if !self.apply_pending_resize() {
            return;
        }
        if !self.redraw_begin(event_loop) {
            return;
        }
        let Some(f) = self.redraw_timing(event_loop) else {
            return;
        };
        // every phase timed (OMSI_PROFILE): the stages measured inside them leave gaps
        let mut lap = f.now;
        if let Some(ended) = self.redraw_ended {
            *self.profile.entry("frame.between").or_default() +=
                entered.saturating_duration_since(ended).as_secs_f64();
        }
        *self.profile.entry("frame.setup").or_default() +=
            f.now.saturating_duration_since(entered).as_secs_f64();
        let mut phase = |app: &mut App, name: &'static str| {
            let now = Instant::now();
            *app.profile.entry(name).or_default() += (now - lap).as_secs_f64();
            lap = now;
        };
        phase(self, "frame.timing");
        self.redraw_traffic(&f);
        phase(self, "frame.traffic");
        self.redraw_player(&f);
        phase(self, "frame.player");
        self.redraw_world(event_loop, &f);
        phase(self, "frame.world");
        self.redraw_camera(&f);
        phase(self, "frame.camera");
        let daylight = self.redraw_environment(&f);
        phase(self, "frame.environment");
        self.redraw_render(event_loop, &f, daylight);
        phase(self, "frame.render");
        self.redraw_ended = Some(Instant::now());
    }
}
