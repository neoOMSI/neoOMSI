//! The window's events: winit's `ApplicationHandler` for `App`.

mod governor;
mod info;
mod mouse_actions;
mod redraw;
mod window;

#[allow(unused_imports)]
pub(crate) use self::governor::*;
#[allow(unused_imports)]
pub(crate) use self::info::*;

use super::*;

/// How fast a stick turns the head, fully pushed (degrees a second, see `Analog::look`).
const LOOK_STICK_DEG_S: f32 = 120.0;

impl ApplicationHandler for App {
    fn resumed(&mut self, event_loop: &ActiveEventLoop) {
        self.resumed_impl(event_loop);
    }

    /// A phone put the app into the background: its window's surface goes (made again on
    /// `resumed`), the fingers and the held keys are let go.
    fn suspended(&mut self, _event_loop: &ActiveEventLoop) {
        self.surface = None;
        self.touch.drop_gpu();
        self.keys.clear();
        if let Some(p) = self.player.as_mut() {
            p.axes.release_all();
        }
        self.save_last_situation();
    }

    fn window_event(&mut self, event_loop: &ActiveEventLoop, _id: WindowId, event: WindowEvent) {
        match event {
            WindowEvent::CloseRequested => {
                self.finish_vr_nav_edit();
                self.finish_session();
                platform::exit(event_loop);
            }
            WindowEvent::Resized(size) => {
                if let (Some(s), Some(r)) = (self.surface.as_mut(), self.renderer.as_ref()) {
                    s.resize(r, size.width, size.height);
                }
            }
            WindowEvent::Focused(true) => {
                self.window_focused = true;
                if let Some(ctl) = self.controllers.as_mut() {
                    ctl.set_focus(true);
                }
            }
            WindowEvent::Focused(false) => self.on_focus_lost(),
            WindowEvent::KeyboardInput { event, .. } => self.on_window_key(event_loop, event),
            WindowEvent::MouseInput {
                state,
                button: winit::event::MouseButton::Right,
                ..
            } => self.on_mouse_right(state),
            WindowEvent::MouseInput {
                state,
                button: winit::event::MouseButton::Middle,
                ..
            } => self.on_mouse_middle(state),
            WindowEvent::MouseWheel { delta, .. } => self.on_mouse_wheel(delta),
            WindowEvent::CursorMoved { position, .. } => self.on_cursor_moved(position),
            WindowEvent::MouseInput {
                state,
                button: winit::event::MouseButton::Left,
                ..
            } => self.on_mouse_left(event_loop, state),
            WindowEvent::Touch(t) => self.on_touch(event_loop, t),
            WindowEvent::RedrawRequested => self.redraw(event_loop),
            _ => {}
        }
    }

    fn device_event(
        &mut self,
        _event_loop: &ActiveEventLoop,
        _device_id: winit::event::DeviceId,
        event: DeviceEvent,
    ) {
        if matches!(&event, DeviceEvent::Added | DeviceEvent::Removed) {
            if let Some(controllers) = self.controllers.as_ref() {
                controllers.refresh_devices();
            }
        }
        if let DeviceEvent::MouseMotion { delta } = event {
            if self.vr_nav_edit.is_some() {
                if self.window_focused {
                    self.vr_nav_drag(delta.0 as f32, delta.1 as f32);
                }
                return;
            }
            if self.game_menu.is_some() {
                return;
            }
            if self.mouse_look {
                if !self.cursor_looks() {
                    if self.view == "outside" {
                        // F3 chase orbits at its own gain, not the head's.
                        self.sync_view_look();
                        let (y, p) = chase_orbit_step(
                            self.look.0,
                            self.look.1,
                            delta.0 as f32,
                            delta.1 as f32,
                        );
                        self.look.0 = y;
                        self.look.1 = p;
                    } else {
                        let k = 0.15 * self.settings.look_sens;
                        self.look_by(delta.0 as f32 * k, delta.1 as f32 * k);
                    }
                }
            } else if self.mouse_drive && self.game_menu.is_none() {
                self.mouse_past_edge(delta.0 as f32);
            }
        }
    }

    fn about_to_wait(&mut self, _event_loop: &ActiveEventLoop) {
        game_lists::flush_settings(false);
        if self.mouse_edge != 0.0 && !self.mouse_drive {
            self.mouse_edge = 0.0;
        }
        if let Some(w) = &self.window {
            w.request_redraw();
        }
    }

    fn user_event(&mut self, event_loop: &ActiveEventLoop, _event: ()) {
        if let Some(sig) = quit::requested() {
            log::info!("{} received: ending the session", quit::signal_name(sig));
            self.finish_session();
            platform::exit(event_loop);
        }
    }

    /// Every way out ends here (Escape, the window's close button, Cmd+Q, --exit-after, a
    /// quit signal): the session is written and the LAN peers hear that we left, before
    /// anything else is torn down (Cmd+Q ends the process without returning from the loop).
    fn exiting(&mut self, _event_loop: &ActiveEventLoop) {
        game_lists::flush_settings(true);
        self.finish_session();
        if let Some(lan) = self.lan.take() {
            drop(lan);
            log::info!("LAN: left the session");
        }
        // the tunnel's cloudflared and the WebSocket gateway go with the game (kept in a
        // static, which Rust never drops: cloudflared outlived every session, holding the
        // port and a public tunnel open)
        lan::close_public_gateway();
        drop(lan::StatusFileGuard);
        log::info!("Shutting down");
    }
}
