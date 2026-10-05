mod cabin;
mod camera;
mod collision;
mod net;
mod vehicle;

use crate::App;
use crate::humans::BusId;
use collision::MoveEnv;
use glam::{DVec2, DVec3};
use winit::keyboard::KeyCode;

pub(crate) const AVATAR_KEY: u32 = 0;
pub(crate) const REMOTE_KEY: u32 = 1_000_000;

const WALK: f64 = 1.45;
const RUN: f64 = 4.3;
const ACCEL: f64 = 7.0;
const AIR_ACCEL: f64 = 1.0;
const JUMP: f64 = 4.0;
const GRAVITY: f64 = 9.81;
const RADIUS: f64 = 0.28;
const BODY_HEIGHT: f64 = 1.8;
const STEP_UP: f64 = 0.3;
const CLIMB_RATE: f64 = 3.0;
const SNAP_DOWN: f64 = 0.4;
const MAX_DT: f32 = 0.1;
const MAX_SPEED: f64 = RUN * 1.5;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum FootCam {
    First,
    Free,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Mode {
    Transit,
    Free,
    Seated,
    Aboard,
    Walking,
}

pub(crate) struct OnFoot {
    pub pos: DVec3,
    pub heading: f64,
    pub vel: DVec2,
    pub lift: f64,
    pub vz: f64,
    pub seat: Option<(BusId, usize)>,
    pub inside: Option<(BusId, glam::Vec3)>,
    pub cam: FootCam,
    pub yaw: f32,
    pub pitch: f32,
    pub eye: Option<DVec3>,
    pub eye_yaw: f32,
    pub lag: DVec3,
    pub settle: f32,
    pub view_before: String,
    pub kind: u64,
    pub face_seat: bool,
    pub transit: Option<Transit>,
    pub arrive: Option<Then>,
    pub safe: DVec3,
    pub on_lane: bool,
    pub door_grace: f32,
    pub attached: bool,
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub(crate) enum Then {
    Nothing,
    Wheel,
    Placed(u64),
}

#[derive(Debug, Clone, Copy)]
pub(crate) struct Transit {
    pub from: DVec3,
    pub to: DVec3,
    pub t: f32,
    pub dur: f32,
    pub after: Option<(BusId, glam::Vec3)>,
    pub then: Then,
}

impl Transit {
    fn new(
        from: DVec3,
        to: DVec3,
        (lo, hi): (f32, f32),
        after: Option<(BusId, glam::Vec3)>,
        then: Then,
    ) -> Transit {
        let d = (to - from).length() as f32;
        Transit {
            from,
            to,
            t: 0.0,
            dur: (d / WALK as f32).clamp(lo, hi),
            after,
            then,
        }
    }

    fn walk(from: DVec3, to: DVec3, after: Option<(BusId, glam::Vec3)>) -> Transit {
        Transit::new(from, to, (0.35, 2.0), after, Then::Nothing)
    }

    fn walk_in(from: DVec3, to: DVec3, then: Then) -> Transit {
        Transit::new(from, to, (0.6, 2.5), None, then)
    }
}

impl OnFoot {
    pub(crate) fn new(pos: DVec3, heading: f64) -> OnFoot {
        OnFoot {
            pos,
            heading,
            vel: DVec2::ZERO,
            lift: 0.0,
            vz: 0.0,
            seat: None,
            inside: None,
            cam: FootCam::First,
            yaw: heading as f32,
            pitch: -5.0,
            eye: None,
            eye_yaw: heading as f32,
            lag: DVec3::ZERO,
            settle: 0.0,
            view_before: "driver".into(),
            kind: 0,
            face_seat: false,
            transit: None,
            arrive: None,
            safe: pos,
            on_lane: false,
            door_grace: 0.0,
            attached: false,
        }
    }

    fn grounded(&self) -> bool {
        self.lift <= 1e-4 && self.vz <= 0.0
    }

    pub(crate) fn mode(&self) -> Mode {
        if self.transit.is_some() {
            Mode::Transit
        } else if self.cam == FootCam::Free {
            Mode::Free
        } else if self.seat.is_some() {
            Mode::Seated
        } else if self.inside.is_some() {
            Mode::Aboard
        } else {
            Mode::Walking
        }
    }

    fn can_jump(&self) -> bool {
        matches!(self.mode(), Mode::Walking) && self.grounded()
    }

    fn sanitize(&mut self) {
        if !self.pos.is_finite() {
            self.pos = if self.safe.is_finite() {
                self.safe
            } else {
                DVec3::ZERO
            };
            self.vel = DVec2::ZERO;
            self.lift = 0.0;
            self.vz = 0.0;
        }
        if !self.vel.is_finite() {
            self.vel = DVec2::ZERO;
        }
        if self.vel.length() > MAX_SPEED {
            self.vel = self.vel.normalize() * MAX_SPEED;
        }
        if !self.lift.is_finite() || self.lift < 0.0 {
            self.lift = 0.0;
        }
        if !self.vz.is_finite() {
            self.vz = 0.0;
        }
        if !self.heading.is_finite() {
            self.heading = 0.0;
        }
        self.heading = wrap(self.heading);
        if !self.yaw.is_finite() {
            self.yaw = self.heading as f32;
        }
        self.yaw = self.yaw.rem_euclid(360.0);
        self.pitch = if self.pitch.is_finite() {
            self.pitch.clamp(-80.0, 80.0)
        } else {
            0.0
        };
        if self.transit.is_some() {
            self.seat = None;
            self.inside = None;
        }
        if self.seat.is_some() {
            self.inside = None;
        }
    }
}

fn wrap(a: f64) -> f64 {
    (a + 180.0).rem_euclid(360.0) - 180.0
}

impl App {
    pub(crate) fn foot_key(
        &mut self,
        code: KeyCode,
        pressed: bool,
        repeat: bool,
        ctrl: bool,
        shift: bool,
    ) -> bool {
        if self.on_foot.is_none() {
            if pressed && !repeat && code == KeyCode::KeyG && ctrl && shift && self.player.is_some()
            {
                self.get_up();
                return true;
            }
            return false;
        }
        match code {
            KeyCode::Escape | KeyCode::F12 | KeyCode::KeyP | KeyCode::KeyV | KeyCode::Slash => {
                false
            }
            KeyCode::F1 => {
                if pressed && !repeat {
                    if let Some(f) = self.on_foot.as_mut() {
                        f.cam = FootCam::First;
                        f.eye = None;
                    }
                    self.view = "foot".into();
                }
                true
            }
            KeyCode::F4 => {
                if pressed && !repeat {
                    if let Some(f) = self.on_foot.as_mut() {
                        f.cam = FootCam::Free;
                        f.vel = DVec2::ZERO;
                    }
                    self.view = "free".into();
                    self.ego = false;
                }
                true
            }
            KeyCode::F2 | KeyCode::F3 => true,
            KeyCode::KeyM if shift && !ctrl => {
                if pressed && !repeat {
                    if let Some(n) = self.navigator.as_mut() {
                        n.toggle_map();
                    }
                }
                true
            }
            _ if self
                .on_foot
                .as_ref()
                .map(|f| f.cam == FootCam::Free)
                .unwrap_or(false) =>
            {
                false
            }
            KeyCode::KeyG => {
                if pressed && !repeat {
                    self.use_seat();
                }
                true
            }
            KeyCode::Space => {
                if let (true, false, Some(f)) = (pressed, repeat, self.on_foot.as_mut()) {
                    if f.can_jump() {
                        f.vz = JUMP;
                    }
                }
                true
            }
            _ => true,
        }
    }

    pub(crate) fn foot_look(&mut self, dx: f32, dy: f32) {
        if let Some(f) = self.on_foot.as_mut() {
            f.yaw = (f.yaw + dx).rem_euclid(360.0);
            f.pitch = (f.pitch - dy).clamp(-80.0, 80.0);
        }
    }

    pub(crate) fn tick_on_foot(&mut self, dt: f32) {
        let dt = if dt.is_finite() {
            dt.clamp(0.0, MAX_DT)
        } else {
            0.0
        };
        if self.foot_arrive() {
            return;
        }
        let Some(mut f) = self.on_foot.take() else {
            return;
        };
        f.sanitize();
        f.on_lane = false;
        self.foot_turn_keys(&mut f, dt);
        self.foot_validate(&mut f);
        if !self.paused {
            match f.mode() {
                Mode::Transit => Self::foot_transit(&mut f, dt),
                Mode::Walking | Mode::Aboard => self.foot_walk(&mut f, dt as f64),
                Mode::Free | Mode::Seated => {}
            }
        }
        f.sanitize();
        let free = f.cam == FootCam::Free;
        self.foot_avatar(&mut f, free);
        let body = self.humans.as_ref().and_then(|h| h.avatar_body(AVATAR_KEY));
        Self::foot_seat_follow(&mut f, body, dt);
        self.foot_camera(&mut f, body, free, dt);
        self.foot_debug(&f);
        self.on_foot = Some(f);
    }

    fn foot_turn_keys(&self, f: &mut OnFoot, dt: f32) {
        let key = |k: KeyCode| self.keys.contains(&k);
        if key(KeyCode::ArrowLeft) {
            f.yaw = (f.yaw - 90.0 * dt).rem_euclid(360.0);
        }
        if key(KeyCode::ArrowRight) {
            f.yaw = (f.yaw + 90.0 * dt).rem_euclid(360.0);
        }
        if key(KeyCode::ArrowUp) {
            f.pitch = (f.pitch + 60.0 * dt).min(80.0);
        }
        if key(KeyCode::ArrowDown) {
            f.pitch = (f.pitch - 60.0 * dt).max(-80.0);
        }
    }

    fn foot_wish(&self, yaw: f32) -> DVec2 {
        let key = |k: KeyCode| self.keys.contains(&k);
        let y = (yaw as f64).to_radians();
        let (fwd, right) = (DVec2::new(y.sin(), y.cos()), DVec2::new(y.cos(), -y.sin()));
        let mut dir = DVec2::ZERO;
        if key(KeyCode::KeyW) {
            dir += fwd;
        }
        if key(KeyCode::KeyS) {
            dir -= fwd;
        }
        if key(KeyCode::KeyD) {
            dir += right;
        }
        if key(KeyCode::KeyA) {
            dir -= right;
        }
        let run = key(KeyCode::ShiftLeft) || key(KeyCode::ShiftRight);
        dir.normalize_or_zero() * if run { RUN } else { WALK }
    }

    fn foot_walk(&self, f: &mut OnFoot, dt: f64) {
        let want = self.foot_wish(f.yaw);
        let rate = if f.grounded() { ACCEL } else { AIR_ACCEL };
        f.vel += (want - f.vel) * (1.0 - (-dt * rate).exp());
        if f.vel.length() < 0.02 && want == DVec2::ZERO {
            f.vel = DVec2::ZERO;
        }
        let turn = wrap(f.yaw as f64 - f.heading);
        let step = if f.vel.length() < 0.3 { 110.0 } else { 220.0 } * dt;
        f.heading = wrap(f.heading + turn.clamp(-step, step));

        let bus = self.foot_bus_step(f, dt);
        f.on_lane = bus.door.is_some();
        if bus.inside_moved {
            return;
        }
        let exempt = bus.exempt;
        let solids = |at: DVec2, feet: f64| self.foot_solids(at, feet, exempt);
        let ground = |p: DVec2, feet: f64, reach: f64| {
            if exempt.is_some() {
                None
            } else {
                self.foot_ground(p, feet, reach)
            }
        };
        collision::move_body(
            f,
            dt,
            &MoveEnv {
                solids: &solids,
                ground: &ground,
            },
        );
        if let Some(path) = bus.door {
            cabin::door_lane(f, path, dt);
        }
    }

    fn foot_arrive(&mut self) -> bool {
        match self.on_foot.as_mut().and_then(|f| f.arrive.take()) {
            Some(Then::Wheel) if self.player.is_some() => {
                self.sit_at_the_wheel();
                self.service_msg = Some(("Back at the wheel".into(), 2.0));
                true
            }
            Some(Then::Placed(uid)) => match self.placed.iter().position(|q| q.uid == uid) {
                Some(k) => {
                    self.take_placed(k);
                    true
                }
                None => false,
            },
            _ => false,
        }
    }

    fn foot_validate(&self, f: &mut OnFoot) {
        let Some(h) = self.humans.as_ref() else {
            return;
        };
        if let Some((bus, _)) = f.seat {
            if !h.bus_here(bus) {
                f.seat = None;
            }
        }
        if let Some((bus, _)) = f.inside {
            if !h.bus_here(bus) && !(bus == BusId::Player && self.player.is_some()) {
                f.inside = None;
            }
        }
    }

    fn foot_transit(f: &mut OnFoot, dt: f32) {
        let Some(mut tr) = f.transit else {
            return;
        };
        let dt64 = dt as f64;
        tr.t += dt;
        let k = (tr.t / tr.dur).clamp(0.0, 1.0) as f64;
        f.inside = None;
        f.seat = None;
        f.lift = 0.0;
        f.vz = 0.0;
        f.pos = tr.from.lerp(tr.to, k);
        f.vel = (tr.to - tr.from).truncate() / tr.dur as f64;
        if f.vel.length() > 0.1 {
            let course = f.vel.x.atan2(f.vel.y).to_degrees();
            let d = wrap(course - f.heading);
            f.heading = wrap(f.heading + d.clamp(-220.0 * dt64, 220.0 * dt64));
        }
        if k >= 1.0 {
            f.transit = None;
            f.inside = tr.after;
            f.vel = DVec2::ZERO;
            if tr.then != Then::Nothing {
                f.arrive = Some(tr.then);
            }
        } else {
            f.transit = Some(tr);
        }
    }

    pub(crate) fn foot_bus(&self) -> Option<BusId> {
        let f = self.on_foot.as_ref()?;
        f.seat.map(|s| s.0).or(f.inside.map(|i| i.0))
    }
}
