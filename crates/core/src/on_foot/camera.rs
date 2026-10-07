use super::{AVATAR_KEY, FootCam, OnFoot, wrap};
use crate::App;
use crate::humans::AvatarCmd;
use glam::{DVec3, Quat, Vec3};

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
        f.eye_pitch += (f.pitch - f.eye_pitch) * k as f32;
        // the vehicle's tilt (kneeling, slopes, bank) carries the view of a passenger along
        let target = match (f.seat.map(|s| s.0).or(f.inside.map(|i| i.0)), f.settle > 0.0) {
            (Some(bus), false) => self
                .humans
                .as_ref()
                .and_then(|h| h.bus_tilt(bus, at))
                .map(|m| Quat::from_mat4(&m).normalize())
                .unwrap_or(Quat::IDENTITY),
            _ => Quat::IDENTITY,
        };
        f.tilt = f.tilt.slerp(target, (1.0 - (-dt64 * 12.0).exp()) as f32);
        let (sy, cy) = f.eye_yaw.to_radians().sin_cos();
        let (sp, cp) = f.eye_pitch.to_radians().sin_cos();
        let fwd = f.tilt * Vec3::new(sy * cp, cy * cp, sp);
        let up = f.tilt * Vec3::Z;
        let r0 = Vec3::new(fwd.y, -fwd.x, 0.0).normalize_or_zero();
        let (yaw, pitch, roll) = if r0 == Vec3::ZERO {
            (f.eye_yaw, f.eye_pitch, 0.0)
        } else {
            let u0 = r0.cross(fwd);
            (
                fwd.x.atan2(fwd.y).to_degrees().rem_euclid(360.0),
                fwd.z.clamp(-1.0, 1.0).asin().to_degrees(),
                up.dot(r0).atan2(up.dot(u0)).to_degrees(),
            )
        };
        cam.position = at;
        cam.yaw = yaw;
        cam.pitch = pitch;
        cam.roll = roll;
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
        if ::legacy_config::env::var_os("OMSI_DEBUG_FOOT").is_none() || self.total_frames % 30 != 0 {
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
