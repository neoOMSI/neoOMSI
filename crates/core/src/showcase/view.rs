use crate::*;
use ::render::Camera;

const YAW: usize = 0;
const PITCH: usize = 1;
const ZOOM: usize = 2;
const PAN_X: usize = 3;
const PAN_Y: usize = 4;

const START: [f32; 5] = [215.0, 8.0, 1.0, 0.0, 0.0];

pub(super) struct View {
    now: [f32; 5],
    to: [f32; 5],
    idle: f32,
}

impl View {
    pub fn new() -> View {
        View { now: START, to: START, idle: 0.0 }
    }

    pub fn orbit(&mut self, dx: f32, dy: f32) {
        self.to[YAW] += dx * 0.35;
        self.to[PITCH] = (self.to[PITCH] + dy * 0.25).clamp(-70.0, 85.0);
        self.idle = 0.0;
    }

    pub fn pan(&mut self, dx: f32, dy: f32, radius: f32) {
        let k = radius * 0.004 * self.to[ZOOM];
        let lim = radius * 1.5;
        self.to[PAN_X] = (self.to[PAN_X] - dx * k).clamp(-lim, lim);
        self.to[PAN_Y] = (self.to[PAN_Y] + dy * k).clamp(-lim, lim);
        self.idle = 0.0;
    }

    pub fn zoom_by(&mut self, k: f32) {
        self.to[ZOOM] = (self.to[ZOOM] * k).clamp(0.25, 3.0);
        self.idle = 0.0;
    }

    pub fn zoom_now(&mut self, k: f32) {
        self.zoom_by(k);
        self.now[ZOOM] = self.to[ZOOM];
    }

    pub fn reset(&mut self) {
        self.to = START;
        self.to[YAW] = self.now[YAW].rem_euclid(360.0) + (START[YAW] - self.now[YAW].rem_euclid(360.0)).rem_euclid(360.0);
        self.idle = 0.0;
    }

    pub fn step(&mut self, dt: f32, auto_turn: bool) -> bool {
        self.idle += dt;
        if auto_turn && self.idle > 4.0 {
            self.to[YAW] += dt * 6.0;
        }
        let k = 1.0 - (-dt / 0.15).exp();
        let mut moving = false;
        for i in 0..5 {
            let d = self.to[i] - self.now[i];
            let eps = if i == ZOOM { 0.001 } else { 0.01 };
            if d.abs() > eps {
                moving = true;
                self.now[i] += d * k;
            } else {
                self.now[i] = self.to[i];
            }
        }
        moving
    }

    pub fn camera(&self, centre: Vec3, radius: f32, w: u32, h: u32, focus: f32) -> Camera {
        let [yaw, pitch, zoom, pan_x, pan_y] = self.now;
        let fov = 30.0f32;
        let half_v = (fov.to_radians() * 0.5).tan();
        let aspect = w as f32 / h.max(1) as f32;
        let free = ((1.0 - focus) * 2.0).clamp(0.3, 1.0);
        let half = half_v.min(half_v * aspect * free).max(0.05);
        let dist = (radius * 1.08 / half * zoom).max(1.5);
        let (sy, cy) = yaw.to_radians().sin_cos();
        let (sp, cp) = pitch.to_radians().sin_cos();
        let fwd = Vec3::new(sy * cp, cy * cp, -sp);
        let right = Vec3::new(cy, -sy, 0.0);
        let up = right.cross(fwd);
        let target = centre + right * pan_x + up * pan_y;
        let pos = target - fwd * dist;
        let side = (focus - 0.5) * 2.0 * half_v * aspect;
        Camera {
            position: DVec3::new(pos.x as f64, pos.y as f64, pos.z as f64),
            yaw: yaw - side.atan().to_degrees(),
            pitch: -pitch,
            roll: 0.0,
            fov_deg: fov,
            near: (dist * 0.02).clamp(0.05, 0.5),
            far: (dist + radius * 4.0).max(100.0),
        }
    }
}
