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
    pub(super) fn redraw(&mut self, event_loop: &ActiveEventLoop) {
        thread_local! {
            static LAST_END: std::cell::Cell<Option<Instant>> = const { std::cell::Cell::new(None) };
        }
        if let Some(end) = LAST_END.with(|c| c.get()) {
            *self.profile.entry("frame.gap").or_default() += end.elapsed().as_secs_f64();
        }
        let t = Instant::now();
        self.redraw_inner(event_loop);
        *self.profile.entry("frame.total").or_default() += t.elapsed().as_secs_f64();
        LAST_END.with(|c| c.set(Some(Instant::now())));
    }

    // the order matters: the player's vehicle moves before the people are placed in it, and the
    // day's light made by the environment is the picture's
    fn redraw_inner(&mut self, event_loop: &ActiveEventLoop) {
        let t = Instant::now();
        #[cfg(not(target_os = "android"))]
        self.update_discord(self.starting.is_some());
        let ok = self.apply_pending_resize();
        *self.profile.entry("frame.resize").or_default() += t.elapsed().as_secs_f64();
        if !ok {
            return;
        }
        let t = Instant::now();
        let ok = self.redraw_begin(event_loop);
        *self.profile.entry("frame.begin").or_default() += t.elapsed().as_secs_f64();
        if !ok {
            return;
        }
        let t = Instant::now();
        let f = self.redraw_timing(event_loop);
        *self.profile.entry("frame.timing").or_default() += t.elapsed().as_secs_f64();
        let Some(f) = f else {
            return;
        };
        self.redraw_traffic(&f);
        self.redraw_player(&f);
        let t = Instant::now();
        self.redraw_world(event_loop, &f);
        *self.profile.entry("frame.world").or_default() += t.elapsed().as_secs_f64();
        let t = Instant::now();
        self.redraw_camera(&f);
        *self.profile.entry("frame.camera").or_default() += t.elapsed().as_secs_f64();
        let t = Instant::now();
        let daylight = self.redraw_environment(&f);
        *self.profile.entry("frame.environment").or_default() += t.elapsed().as_secs_f64();
        self.redraw_render(event_loop, &f, daylight);
    }
}
