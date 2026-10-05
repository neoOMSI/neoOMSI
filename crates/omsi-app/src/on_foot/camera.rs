use super::{AVATAR_KEY, FootCam, OnFoot, wrap};
use crate::App;
use crate::humans::AvatarCmd;
use glam::DVec3;

impl App {
    pub(super) fn foot_avatar(&mut self, f: &mut OnFoot, show: bool) {
        let (Some(h), Some(w), Some(r), Some(scene)) = (
            self.humans.as_mut(),
            self.world.as_ref(),
            self.renderer.as_ref(),
            self.scene.as_mut(),
        ) else {
            return;
        };
        let cmd = AvatarCmd {
            pos: f.pos,
            heading: f.heading,
            vel: f.vel,
            lift: f.lift,
            seat: f.seat,
            floor: f
                .inside
                .map(|_| f.pos.z)
                .or(f.transit.map(|_| f.pos.z))
                .or(f.on_lane.then_some(f.pos.z)),
            aboard: f.inside,
        };
        h.avatar(AVATAR_KEY, w, r, scene, cmd, f.kind);
        h.avatar_show(AVATAR_KEY, show);
    }

    pub(super) fn foot_seat_follow(f: &mut OnFoot, body: Option<(DVec3, f64, DVec3)>, dt: f32) {
        let (Some((feet, heading, _)), Some(_)) = (body, f.seat) else {
            return;
        };
        let dt64 = dt as f64;
        f.pos = feet;
        f.heading = heading;
        if f.face_seat {
            let d = wrap(heading - f.yaw as f64);
            f.yaw = (f.yaw as f64 + d * (1.0 - (-dt64 * 4.0).exp())) as f32;
            f.pitch += (-5.0 - f.pitch) * (1.0 - (-dt * 4.0).exp());
            if d.abs() < 2.0 {
                f.face_seat = false;
            }
        }
    }

    pub(super) fn foot_camera(
        &mut self,
        f: &mut OnFoot,
        body: Option<(DVec3, f64, DVec3)>,
        free: bool,
        dt: f32,
    ) {
        let Some(cam) = self.camera.as_mut() else {
            return;
        };
        if free {
            return;
        }
        let dt64 = dt as f64;
        let eye = body
            .map(|b| b.2)
            .unwrap_or(f.pos + DVec3::new(0.0, 0.0, 1.62 + f.lift));
        let y = (f.yaw as f64).to_radians();
        let want = eye + DVec3::new(y.sin(), y.cos(), 0.0) * 0.08;
        let settling = f.settle > 0.0;
        f.settle = (f.settle - dt).max(0.0);
        let u = (1.0 - f.settle as f64).clamp(0.0, 1.0);
        let rate = 4.0 + 14.0 * u * u * (3.0 - 2.0 * u);
        let k = 1.0 - (-dt64 * rate).exp();
        let at = match f.eye {
            Some(e) if (e - want).length() < 60.0 => e + (want - e) * k,
            _ => want,
        };
        f.eye = Some(at);
        if settling {
            f.lag = at - want;
        } else {
            f.lag *= (-dt64 * 9.0).exp();
            if f.lag.length() < 0.001 {
                f.lag = DVec3::ZERO;
            }
        }
        let dy = ((f.yaw - f.eye_yaw + 540.0).rem_euclid(360.0)) - 180.0;
        f.eye_yaw = (f.eye_yaw + dy * k as f32).rem_euclid(360.0);
        cam.position = at;
        cam.yaw = f.eye_yaw;
        cam.pitch += (f.pitch - cam.pitch) * k as f32;
        cam.roll += (0.0 - cam.roll) * k as f32;
    }

    pub(crate) fn foot_after_humans(&mut self) {
        let Some(f) = self.on_foot.as_mut() else {
            return;
        };
        let rigid = f.seat.is_some() || f.inside.is_some();
        if !rigid || f.cam != FootCam::First || f.settle > 0.0 {
            f.attached = false;
            return;
        }
        let Some(h) = self.humans.as_ref() else {
            return;
        };
        if let Some((bus, l)) = f.inside {
            if let Some((w, _)) = h.cabin_world(bus, l) {
                f.pos = w;
            }
        }
        let eye = h
            .avatar_body(AVATAR_KEY)
            .map(|b| b.2)
            .unwrap_or(f.pos + DVec3::new(0.0, 0.0, 1.62));
        let y = (f.eye_yaw as f64).to_radians();
        let exact = eye + DVec3::new(y.sin(), y.cos(), 0.0) * 0.08;
        if !f.attached {
            f.lag = match f.eye {
                Some(e) if (e - exact).length() < 3.0 => e - exact,
                _ => DVec3::ZERO,
            };
            f.attached = true;
        }
        let at = exact + f.lag;
        f.eye = Some(at);
        if let Some(cam) = self.camera.as_mut() {
            cam.position = at;
        }
    }

    pub(super) fn foot_debug(&self, f: &OnFoot) {
        if omsi_cfg::env::var_os("OMSI_DEBUG_FOOT").is_none() || self.total_frames % 30 != 0 {
            return;
        }
        let body = self.humans.as_ref().and_then(|h| h.avatar_body(AVATAR_KEY));
        log::info!(
            "foot: inside {:?} pos ({:.2}, {:.2}, {:.2}) heading {:.0} yaw {:.0} vel ({:.2}, {:.2}) lift {:.2} seat {:?} cam {:?} body {:?}",
            f.inside,
            f.pos.x,
            f.pos.y,
            f.pos.z,
            f.heading,
            f.yaw,
            f.vel.x,
            f.vel.y,
            f.lift,
            f.seat,
            f.cam,
            body
        );
    }
}
