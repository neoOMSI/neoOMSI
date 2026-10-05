//! Omsi.exe's animation of its people (sub_626ae8): thirty joint angles from the walk phase,
//! turned into thirteen bone matrices by plain rotations about the `[links]` joints, with no
//! leg IK. Done with D3DX's row-vector matrices in Direct3D's frame so that the signs and the
//! order of the rotations are the original's; [`OmsiAnim::bones`] converts to this engine's.

use glam::{Affine3A, Mat4, Vec3, Vec4};
use omsi_content::Human;

pub const BONES: usize = 13;

/// A D3DX matrix: rows, row vectors.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct DMat(pub [[f32; 4]; 4]);

impl DMat {
    pub const IDENTITY: DMat = DMat([
        [1.0, 0.0, 0.0, 0.0],
        [0.0, 1.0, 0.0, 0.0],
        [0.0, 0.0, 1.0, 0.0],
        [0.0, 0.0, 0.0, 1.0],
    ]);

    pub fn translation(v: Vec3) -> DMat {
        let mut m = DMat::IDENTITY;
        m.0[3][0] = v.x;
        m.0[3][1] = v.y;
        m.0[3][2] = v.z;
        m
    }

    pub fn rot_x(a: f32) -> DMat {
        let (s, c) = a.sin_cos();
        let mut m = DMat::IDENTITY;
        m.0[1][1] = c;
        m.0[2][2] = c;
        m.0[1][2] = s;
        m.0[2][1] = -s;
        m
    }

    pub fn rot_y(a: f32) -> DMat {
        let (s, c) = a.sin_cos();
        let mut m = DMat::IDENTITY;
        m.0[0][0] = c;
        m.0[2][2] = c;
        m.0[0][2] = -s;
        m.0[2][0] = s;
        m
    }

    pub fn rot_z(a: f32) -> DMat {
        let (s, c) = a.sin_cos();
        let mut m = DMat::IDENTITY;
        m.0[0][0] = c;
        m.0[1][1] = c;
        m.0[0][1] = s;
        m.0[1][0] = -s;
        m
    }

    pub fn rot_axis(axis: Vec3, a: f32) -> DMat {
        let v = axis.normalize_or_zero();
        let (s, c) = a.sin_cos();
        let d = 1.0 - c;
        let mut m = DMat::IDENTITY;
        m.0[0][0] = d * v.x * v.x + c;
        m.0[1][0] = d * v.x * v.y - s * v.z;
        m.0[2][0] = d * v.x * v.z + s * v.y;
        m.0[0][1] = d * v.y * v.x + s * v.z;
        m.0[1][1] = d * v.y * v.y + c;
        m.0[2][1] = d * v.y * v.z - s * v.x;
        m.0[0][2] = d * v.z * v.x - s * v.y;
        m.0[1][2] = d * v.z * v.y + s * v.x;
        m.0[2][2] = d * v.z * v.z + c;
        m
    }

    /// D3DXMatrixMultiply(out, self, b): `self` first, then `b`.
    pub fn mul(&self, b: &DMat) -> DMat {
        let mut o = [[0.0f32; 4]; 4];
        for (i, row) in o.iter_mut().enumerate() {
            for (j, v) in row.iter_mut().enumerate() {
                *v = (0..4).map(|k| self.0[i][k] * b.0[k][j]).sum();
            }
        }
        DMat(o)
    }

    pub fn inverse(&self) -> DMat {
        DMat(
            Mat4::from_cols_array_2d(&self.0)
                .inverse()
                .to_cols_array_2d(),
        )
    }

    pub fn point(&self, p: Vec3) -> Vec3 {
        let r =
            |j: usize| p.x * self.0[0][j] + p.y * self.0[1][j] + p.z * self.0[2][j] + self.0[3][j];
        let w = r(3);
        let w = if w.abs() > 1e-12 { w } else { 1.0 };
        Vec3::new(r(0) / w, r(1) / w, r(2) / w)
    }

    /// In this engine's model frame (x right, y forward, z up): `S * M^T * S`, S swapping y and z.
    pub fn to_engine(&self) -> Affine3A {
        let m = &self.0;
        // column-vector form: its columns are the D3D matrix's rows
        let row = |j: usize| Vec4::new(m[j][0], m[j][1], m[j][2], m[j][3]);
        let t = Mat4::from_cols(row(0), row(1), row(2), row(3));
        let s = Mat4::from_cols(Vec4::X, Vec4::Z, Vec4::Y, Vec4::W);
        Affine3A::from_mat4(s * t * s)
    }
}

/// A point of this engine's model frame in Direct3D's (y and z swapped).
pub fn d3d(v: Vec3) -> Vec3 {
    Vec3::new(v.x, v.z, v.y)
}

#[derive(Debug, Clone)]
pub struct OmsiRig {
    pub hip: Vec3,
    pub knee: Vec3,
    pub waist: Vec3,
    pub shoulder: Vec3,
    pub elbow: Vec3,
    pub neck: Vec3,
    pub hand: Vec3,
    pub finger: Vec3,
    pub feet_dist: f32,
    pub height: f32,
    pub seat_height: f32,
    /// `[walk_param]`: stride (1.4), upper arm beta (66), arm swing (1), hip turn (1), waist (0).
    pub stride: f32,
    pub beta: f32,
    pub arm_swing: f32,
    pub hip_turn: f32,
    pub waist_bend: f32,
    pub upper_arm: [Vec3; 2],
    pub forearm: [Vec3; 2],
    pub thigh: [Vec3; 2],
}

impl OmsiRig {
    pub fn new(def: &Human) -> OmsiRig {
        let l = |i: usize| {
            def.links
                .get(i)
                .copied()
                .filter(|v| v.is_finite())
                .unwrap_or(0.0)
        };
        // [links] lists x, y (forward), z (up); Omsi.exe stores (x, z, y)
        let hip = Vec3::new(l(0), l(2), l(1));
        let knee = Vec3::new(l(3), l(5), l(4));
        let waist = Vec3::new(0.0, l(7), l(6));
        let shoulder = Vec3::new(l(8), l(10), l(9));
        let elbow = Vec3::new(l(11), l(13), l(12));
        let neck = Vec3::new(0.0, l(15), l(14));
        let hand = Vec3::new(l(16), l(18), l(17));
        let finger = Vec3::new(l(19), l(21), l(20));
        let mirror = |v: Vec3| Vec3::new(-v.x, v.y, v.z);
        let ua = elbow - shoulder;
        let fa = finger - elbow;
        let th = knee - hip;
        let wp = def.walk_param;
        OmsiRig {
            hip,
            knee,
            waist,
            shoulder,
            elbow,
            neck,
            hand,
            finger,
            feet_dist: def.feet_dist,
            height: def.height,
            seat_height: def.seat_height,
            stride: wp[0],
            beta: wp[1],
            arm_swing: wp[2],
            hip_turn: wp[3],
            waist_bend: wp[4],
            upper_arm: [mirror(ua), ua],
            forearm: [mirror(fa), fa],
            thigh: [mirror(th), th],
        }
    }
}

/// sub_624820: the thigh's and the knee's angle over the phase, in units of the swing angle.
const THIGH_CURVE: [(f32, f32); 4] = [(0.0, 0.75), (0.2, 1.1), (0.8, -1.1), (1.0, 0.75)];
const KNEE_CURVE: [(f32, f32); 4] = [(0.0, 1.0), (0.2, 0.0), (0.8, 0.0), (1.0, 1.0)];

fn curve(c: &[(f32, f32)], x: f32) -> f32 {
    if x < c[0].0 {
        return c[0].1;
    }
    let last = c[c.len() - 1];
    if last.0 <= x {
        return last.1;
    }
    let mut i = 1;
    while i < c.len() && c[i].0 <= x {
        i += 1;
    }
    let (a, b) = (c[i - 1], c[i]);
    if b.0 == a.0 {
        (a.1 + b.1) / 2.0
    } else {
        (x - a.0) * ((b.1 - a.1) / (b.0 - a.0)) + a.1
    }
}

fn arm_ik(rig: &OmsiRig, v: Vec3, fore: Vec3, a: &mut [f32; 30]) {
    let l30 = rig.upper_arm[1].length();
    let l34 = fore.length();
    let l38 = v.length();
    let c = ((l30 * l30 + l34 * l34) - l38 * l38) / (2.0 * l30 * l34);
    let c = if c > 1.0 { 1.0 } else { c };
    a[22] = if c > -1.0 {
        180.0 - c.acos() / DEG
    } else {
        0.0
    };
    let c = ((l30 * l30 + l38 * l38) - l34 * l34) / (2.0 * l30 * l38);
    let c = if c > 1.0 { 1.0 } else { c };
    a[20] = if c > -1.0 { -(c.acos() / DEG) } else { 0.0 };
    let arm = fore + rig.upper_arm[1];
    let flat_arm = Vec3::new(arm.x, 0.0, arm.z);
    let flat_v = Vec3::new(v.x, 0.0, v.z);
    a[14] = (flat_v.z.atan2(flat_v.x) - flat_arm.z.atan2(flat_arm.x)) / DEG;
    let s1 = (-v.y / v.length()).clamp(-1.0, 1.0).asin();
    let s2 = (arm.y / arm.length()).clamp(-1.0, 1.0).asin();
    a[16] = (s1 - s2) / DEG;
}

fn natural_stride(speed: f32) -> f32 {
    (speed / 1.2).powf(0.55).min(1.9)
}

fn smoothstep(e0: f32, e1: f32, x: f32) -> f32 {
    let t = ((x - e0) / (e1 - e0)).clamp(0.0, 1.0);
    t * t * (3.0 - 2.0 * t)
}

fn hash(seed: u32, n: u32) -> f32 {
    let mut x = (seed as u64) << 32 | n as u64;
    x = (x ^ (x >> 30)).wrapping_mul(0xbf58_476d_1ce4_e5b9);
    x = (x ^ (x >> 27)).wrapping_mul(0x94d0_49bb_1331_11eb);
    x ^= x >> 31;
    (x >> 40) as f32 / (1u64 << 24) as f32
}

/// Delphi's Trunc of a float (toward zero).
fn frac(x: f32) -> f32 {
    x - x.trunc()
}

const DEG: f32 = std::f32::consts::PI / 180.0;
/// The original's degree-to-radian factor (a 10-byte constant, 0.01745...).
const RAD: f32 = 0.017_453_292;

#[derive(Debug, Clone, Copy, Default)]
pub struct AnimInput {
    /// `PAX_State` rounded: 0 stand, 1 walk, 2 sit.
    pub kind: u8,
    pub speed: f32,
    pub moved: f32,
    /// Room height of the path link (m; 50 outside a vehicle).
    pub room_height: f32,
    pub seat_height: f32,
    /// The right hand reaches here (person's frame, Direct3D axes).
    pub reach: Option<Vec3>,
    /// The head looks here (person's frame, Direct3D axes).
    pub look: Option<Vec3>,
    /// The angles ease instead of jumping (at the validator or the cash desk, sitting).
    pub smooth: bool,
    pub dt_ms: f32,
    /// Off, exactly Omsi.exe's animation.
    pub natural: bool,
    pub seed: u32,
}

#[derive(Debug, Clone)]
pub struct OmsiAnim {
    /// 0 .. 2 over two steps.
    pub phase: f32,
    pub angles: [f32; 30],
    bob: f32,
    clock: f32,
    shown: Option<(u8, bool)>,
    from: [f32; 30],
    fade: f32,
    reach_t: f32,
}

impl Default for OmsiAnim {
    fn default() -> Self {
        OmsiAnim {
            phase: 0.0,
            angles: [0.0; 30],
            bob: 0.0,
            clock: 0.0,
            shown: None,
            from: [0.0; 30],
            fade: 1.0,
            reach_t: 0.0,
        }
    }
}

#[derive(Debug, Clone, Copy, Default)]
pub struct AnimEvents {
    /// A foot came down: the phase crossed 0.2, 0.7, 1.2 or 1.7.
    pub step: bool,
}

impl OmsiAnim {
    pub fn advance(&mut self, rig: &OmsiRig, inp: &AnimInput) -> AnimEvents {
        let mut ev = AnimEvents::default();
        let mut a = [0.0f32; 30];
        // the stride used at this speed (+0x2d8 times |v| / 1.2, at most 1)
        let v = inp.speed.abs();
        let stride = if inp.natural {
            rig.stride * natural_stride(v) * (0.93 + 0.14 * hash(inp.seed, 14))
        } else {
            (v / 1.2).min(1.0) * rig.stride
        };
        let run = if inp.natural && inp.kind == 1 {
            smoothstep(1.9, 2.7, v)
        } else {
            0.0
        };
        // stooping under a low ceiling
        let l20 = inp.room_height - rig.waist.y;
        let l24 = rig.height - rig.waist.y;
        let stoop = if l20 < l24 {
            let c = l20.max(0.0) / l24;
            let s = c.clamp(-1.0, 1.0).acos() / DEG;
            a[12] = s / 2.0;
            a[11] = s / 2.0;
            s
        } else {
            0.0
        };
        let kind = inp.kind;
        if kind == 1 {
            a[12] = a[12].max(3.0);
            a[11] = a[11].max(3.0);
            a[9] = a[9].min(-5.0);
        }
        // the leg's swing angle (degrees)
        let swing = (stride / (4.0 * rig.hip.y)) / DEG;
        let mut bob = 0.0;
        match kind {
            0 => {
                let spread = -(rig.feet_dist / (2.0 * rig.hip.y)) / DEG;
                a[2] = spread;
                a[3] = spread;
            }
            1 => {
                let before = self.phase;
                if stride > 0.0 {
                    self.phase += inp.moved / stride;
                }
                // (0x6275e3: a foot down at 0.2, 0.7, 1.2 and 1.7)
                if [0.2f32, 0.7, 1.2, 1.7]
                    .iter()
                    .any(|&t| t <= self.phase && before < t)
                {
                    ev.step = true;
                }
                if self.phase > 2.0 {
                    self.phase -= 2.0;
                }
                let p = self.phase;
                let f_half = frac(p + 0.5);
                let f = frac(p);
                a[0] = curve(&THIGH_CURVE, f_half) * swing;
                a[4] = curve(&KNEE_CURVE, f_half) * swing * 1.5 * 2.0;
                a[1] = curve(&THIGH_CURVE, f) * swing;
                a[5] = curve(&KNEE_CURVE, f) * swing * 1.5 * 2.0;
                let spread = -(rig.feet_dist / (3.0 * rig.hip.y)) / DEG;
                a[2] = spread;
                a[3] = spread;
                let l24 = ((4.0 * std::f32::consts::PI * p).cos() - 1.0) / 2.0;
                bob =
                    (1.0 - (std::f32::consts::PI / 180.0 * swing).cos()) * (0.8 * rig.hip.y) * l24;
                a[11] -= swing * l24 * rig.waist_bend;
                a[12] += swing * l24 * rig.waist_bend;
                a[29] = (2.0 * std::f32::consts::PI * p).sin() * 5.0;
                a[10] = (2.0 * std::f32::consts::PI * p).sin() * rig.hip_turn;
                if inp.natural {
                    a[12] += ((v - 1.0) * 2.5).clamp(0.0, 3.0) + 7.0 * run;
                    a[4] *= 1.0 + 0.35 * run;
                    a[5] *= 1.0 + 0.35 * run;
                    // both feet leave the ground before each footfall (0.2, 0.7, ...)
                    let q = (p - 0.2).rem_euclid(0.5) / 0.5;
                    let flight = if q > 0.4 {
                        (std::f32::consts::PI * (q - 0.4) / 0.6).sin()
                    } else {
                        0.0
                    };
                    bob += (0.07 * rig.hip.y * flight - bob) * run;
                }
            }
            2 => {
                let thigh = rig.thigh[0].length();
                let l20 = (-(thigh - rig.seat_height) - inp.seat_height) + 0.1;
                let extra = if l20 > 0.0 {
                    let x = (l20 / thigh).min(0.99);
                    let d = x.clamp(-1.0, 1.0).asin() / DEG;
                    if d > 40.0 { 40.0 } else { d }
                } else {
                    0.0
                };
                a[9] = -20.0;
                a[0] = 60.0 + extra;
                a[1] = 60.0 + extra;
                let spread = -(rig.feet_dist / (1.5 * rig.hip.y)) / DEG;
                a[2] = spread;
                a[3] = spread;
                a[4] = 90.0 + extra;
                a[5] = 90.0 + extra;
            }
            _ => {}
        }
        // how much of the arm swing is left when stooping
        let upright = ((90.0 - stoop) / 90.0).max(0.0);
        let sin2 = (2.0 * std::f32::consts::PI * self.phase).sin();
        // the left hand holds, the right reaches
        match kind {
            2 => {
                a[13] = -3.0 - 0.4 * stoop;
                a[15] = 67.0;
                a[17] = -58.0;
                a[19] = 35.0 + stoop;
                a[21] = 46.0;
                a[23] = 41.0;
                a[25] = -26.0;
                a[27] = 5.0;
            }
            1 => {
                a[13] = -3.0;
                a[15] = rig.beta - 3.0;
                a[17] = -30.0;
                a[19] = (sin2 - 0.2) * (upright * rig.arm_swing * swing) + stoop * 0.5;
                a[21] = (sin2 + 1.0) * (upright * rig.arm_swing * swing) * 1.5;
            }
            _ => {
                a[13] = -3.0;
                a[15] = rig.beta;
                a[17] = -30.0;
                a[19] = 0.6 * stoop - 3.0;
                a[21] = 29.0;
            }
        }
        if run > 0.0 {
            a[19] *= 1.0 + 0.6 * run;
            a[21] += (80.0 + 15.0 * sin2 - a[21]) * run;
        }
        if inp.natural && kind == 0 {
            self.idle(inp.seed, &mut a);
        }
        if inp.natural && kind == 1 {
            self.gait(inp.seed, run, inp.reach.is_none(), &mut a);
        }
        // the right arm
        if let Some(target) = inp.reach {
            if inp.natural {
                self.reach_t = if self.shown.is_some_and(|s| s.1) {
                    self.reach_t + inp.dt_ms / 1000.0
                } else {
                    0.0
                };
                self.natural_reach(rig, target, bob, &mut a);
            } else {
                arm_ik(rig, target - rig.shoulder, rig.forearm[1], &mut a);
            }
        } else {
            match kind {
                2 => {
                    a[14] = -3.0 - 0.4 * stoop;
                    a[16] = 67.0;
                    a[18] = -58.0;
                    a[20] = 35.0 + stoop;
                    a[22] = 46.0;
                    a[24] = 41.0;
                    a[26] = -26.0;
                    a[28] = 5.0;
                }
                1 => {
                    a[14] = -3.0;
                    a[16] = rig.beta - 3.0;
                    a[18] = -30.0;
                    a[20] = (-sin2 - 0.2) * (upright * rig.arm_swing * swing) + stoop * 0.5;
                    a[22] = (-sin2 + 1.0) * (upright * rig.arm_swing * swing) * 1.2;
                    if run > 0.0 {
                        a[20] *= 1.0 + 0.6 * run;
                        a[22] += (80.0 - 15.0 * sin2 - a[22]) * run;
                    }
                }
                _ => {
                    a[14] = -3.0;
                    a[16] = rig.beta;
                    a[18] = -30.0;
                    a[20] = 0.6 * stoop - 3.0;
                    a[22] = 29.0;
                }
            }
        }
        // the head
        if let Some(t) = inp.look {
            let v = t - (rig.neck + Vec3::new(0.0, 0.13, 0.0));
            if v.z > 0.0 {
                let l = v.length();
                a[6] = (v.x / l).clamp(-1.0, 1.0).asin() / DEG;
                a[7] = -((v.y / l).clamp(-1.0, 1.0).asin() / DEG);
                if inp.natural {
                    a[6] = a[6].clamp(-70.0, 70.0);
                    a[29] = 0.25 * a[6];
                }
            }
        }
        let shown = (kind, inp.reach.is_some());
        if inp.natural && !inp.smooth && self.shown.is_some_and(|s| s != shown) {
            self.from = self.angles;
            self.fade = 0.0;
        }
        self.shown = Some(shown);
        // to the angles: at once, or easing towards them (0x628c95)
        let k = (inp.dt_ms / 1000.0 * 10.0).min(1.0);
        let lim = 18000.0 * inp.dt_ms / 1000.0;
        let blend = if self.fade < 1.0 && !inp.smooth {
            self.fade = (self.fade + inp.dt_ms / 350.0).min(1.0);
            Some(smoothstep(0.0, 1.0, self.fade))
        } else {
            self.fade = 1.0;
            None
        };
        for ((cur, to), from) in self.angles.iter_mut().zip(a.iter()).zip(self.from) {
            if inp.smooth {
                let d = ((to - *cur) * k).clamp(-lim, lim);
                *cur += d;
            } else if let Some(w) = blend {
                *cur = from + (to - from) * w;
            } else {
                *cur = *to;
            }
        }
        self.bob = bob;
        self.clock += inp.dt_ms / 1000.0;
        ev
    }

    fn natural_reach(&self, rig: &OmsiRig, target: Vec3, bob: f32, a: &mut [f32; 30]) {
        let fore = rig.hand - rig.elbow;
        let hand = (rig.finger - rig.hand).length();
        let length = rig.upper_arm[1].length() + fore.length();
        let mut probe = OmsiAnim {
            bob,
            ..OmsiAnim::default()
        };
        let wrist_at = |local: Vec3| {
            let h = Vec3::new(local.x - rig.shoulder.x, 0.0, local.z - rig.shoulder.z)
                .normalize_or(Vec3::Z);
            let down = 30.0 * DEG;
            local - (h * down.cos() - Vec3::Y * down.sin()) * hand
        };
        let mut best = (f32::MAX, 0.0, 0.0);
        for pitch in [0.0, 5.0, 10.0, 15.0, 20.0, 25.0, 30.0] {
            for twist in [0.0, -6.0, 6.0, -12.0, 12.0, -18.0, 18.0] {
                probe.angles = *a;
                probe.angles[12] += pitch;
                probe.angles[29] += twist;
                let local = probe.bones_d3d(rig)[9].inverse().point(target);
                let short = ((wrist_at(local) - rig.shoulder).length() - 0.93 * length).max(0.0);
                let cost = pitch + 0.5 * twist.abs() + 1000.0 * short;
                if cost < best.0 {
                    best = (cost, pitch, twist);
                }
            }
        }
        a[12] += best.1;
        a[29] += best.2;
        probe.angles = *a;
        let local = probe.bones_d3d(rig)[9].inverse().point(target);
        arm_ik(rig, wrist_at(local) - rig.shoulder, fore, a);
        // the hand is along x in the T-pose the bones start from: it bends about y and z
        let press = (std::f32::consts::PI * ((self.reach_t - 0.35) / 0.5).clamp(0.0, 1.0)).sin();
        let goal = target - Vec3::Y * 0.025 * press;
        probe.angles = *a;
        for (k, step) in [(26, 10.0), (28, 10.0), (26, 3.0), (28, 3.0)] {
            let mut best = (f32::MAX, probe.angles[k]);
            let around = probe.angles[k];
            for n in -6..=6 {
                probe.angles[k] = (around + n as f32 * step).clamp(-80.0, 80.0);
                let d = (probe.bones_d3d(rig)[12].point(rig.finger) - goal).length();
                if d < best.0 {
                    best = (d, probe.angles[k]);
                }
            }
            probe.angles[k] = best.1;
        }
        a[26] = probe.angles[26];
        a[28] = probe.angles[28];
    }

    fn gait(&self, seed: u32, run: f32, right_free: bool, a: &mut [f32; 30]) {
        let loose = (0.35 + 0.65 * hash(seed, 11).sqrt()) * (1.0 - run);
        let tau = std::f32::consts::TAU;
        let (s, c2) = ((tau * self.phase).sin(), (2.0 * tau * self.phase).cos());
        let k = 1.0 - 0.25 * loose;
        a[19] *= 0.8 + 0.4 * loose;
        a[21] = a[21] * k + 14.0 * loose;
        if right_free {
            a[20] *= 0.8 + 0.4 * loose;
            a[22] = a[22] * k + 14.0 * loose;
        }
        a[29] += s * 6.0 * loose;
        a[10] *= 1.0 + loose;
        a[4] += 5.0 * loose;
        a[5] += 5.0 * loose;
        a[12] += (2.5 * (hash(seed, 12) - 0.3)) * (1.0 - run);
        a[2] += 1.5 * (hash(seed, 13) - 0.5);
        a[3] += 1.5 * (hash(seed, 13) - 0.5);
        a[7] += 1.5 * loose * c2;
        let t = self.clock + 100.0 * hash(seed, 0);
        let look = (tau * t / (5.0 + 4.0 * hash(seed, 15))).sin();
        a[6] = 18.0 * loose * look.powi(9);
    }

    fn idle(&self, seed: u32, a: &mut [f32; 30]) {
        let t = self.clock + 100.0 * hash(seed, 0);
        let tau = std::f32::consts::TAU;
        let sway = (tau * t / (7.0 + 5.0 * hash(seed, 1)) + tau * hash(seed, 2)).sin();
        a[2] += 1.5 * sway;
        a[3] -= 1.5 * sway;
        a[10] += 2.0 * sway;
        a[12] += 0.6 * (tau * t / (3.8 + hash(seed, 3))).sin();
        a[21] += 14.0 * (hash(seed, 4) - 0.5);
        a[22] += 14.0 * (hash(seed, 5) - 0.5);
        let len = 2.5 + 3.5 * hash(seed, 6);
        let epoch = (t / len).floor();
        let glance = |e: f32| {
            let s = seed ^ (e as i64 as u32).wrapping_mul(0x9e37_79b9);
            let yaw = (2.0 * hash(s, 8) - 1.0) * if hash(s, 7) < 0.35 { 50.0 } else { 15.0 };
            let down = if hash(s, 10) < 0.15 { 14.0 } else { 0.0 };
            (yaw, (2.0 * hash(s, 9) - 1.0) * 5.0 + down)
        };
        let (was, now) = (glance(epoch - 1.0), glance(epoch));
        let k = smoothstep(0.0, 0.7, t - epoch * len);
        a[6] = was.0 + (now.0 - was.0) * k;
        a[7] = was.1 + (now.1 - was.1) * k;
        a[29] = 0.25 * a[6];
    }

    pub fn bones_d3d(&self, rig: &OmsiRig) -> [DMat; BONES] {
        let a = &self.angles;
        let t = DMat::translation;
        let m = |x: f32, y: f32, z: f32| Vec3::new(x, y, z);
        let (h, k, w, s, e, n, hd) = (
            rig.hip,
            rig.knee,
            rig.waist,
            rig.shoulder,
            rig.elbow,
            rig.neck,
            rig.hand,
        );
        let mut b = [DMat::IDENTITY; BONES];
        // the pelvis: about the hip line, with the bob (+0x4f0)
        let pelvis = t(-h)
            .mul(&DMat::rot_x(RAD * a[11]))
            .mul(&t(m(0.0, self.bob, 0.0)))
            .mul(&t(h));
        // 8: the hip (lower torso), about the waist
        let waist = t(-w)
            .mul(&DMat::rot_y(RAD * a[10]))
            .mul(&DMat::rot_x(RAD * a[9]))
            .mul(&t(w));
        b[8] = waist.mul(&pelvis);
        // 0, 1: the thighs, from the hip bone with its turn taken out again
        let hl = m(-h.x, h.y, h.z);
        b[0] = t(-hl)
            .mul(&DMat::rot_z(-a[2] * RAD))
            .mul(&DMat::rot_x((-a[11] - a[0]) * RAD))
            .mul(&DMat::rot_y(-a[10] * RAD))
            .mul(&t(hl))
            .mul(&b[8]);
        b[1] = t(-h)
            .mul(&DMat::rot_z(RAD * a[3]))
            .mul(&DMat::rot_x((-a[11] - a[1]) * RAD))
            .mul(&DMat::rot_y(-a[10] * RAD))
            .mul(&t(h))
            .mul(&b[8]);
        // 2, 3: the shins, about the knees
        let kl = m(-k.x, k.y, k.z);
        b[2] = t(-kl).mul(&DMat::rot_x(RAD * a[4])).mul(&t(kl)).mul(&b[0]);
        b[3] = t(-k).mul(&DMat::rot_x(RAD * a[5])).mul(&t(k)).mul(&b[1]);
        // 9: the upper body, about the waist, on the pelvis
        b[9] = t(-w)
            .mul(&DMat::rot_x(RAD * a[12]))
            .mul(&DMat::rot_y(RAD * a[29]))
            .mul(&t(w))
            .mul(&pelvis);
        // 4, 5: the upper arms, about the shoulders
        let sl = m(-s.x, s.y, s.z);
        b[4] = t(-sl)
            .mul(&DMat::rot_axis(rig.upper_arm[0], RAD * a[17]))
            .mul(&DMat::rot_y(RAD * a[19]))
            .mul(&DMat::rot_z(RAD * a[15]))
            .mul(&DMat::rot_y(RAD * a[13]))
            .mul(&t(sl))
            .mul(&b[9]);
        b[5] = t(-s)
            .mul(&DMat::rot_axis(rig.upper_arm[1], -a[18] * RAD))
            .mul(&DMat::rot_y(-a[20] * RAD))
            .mul(&DMat::rot_z(-a[16] * RAD))
            .mul(&DMat::rot_y(-a[14] * RAD))
            .mul(&t(s))
            .mul(&b[9]);
        // 6, 7: the forearms, about the elbows
        let el = m(-e.x, e.y, e.z);
        b[6] = t(-el).mul(&DMat::rot_y(RAD * a[21])).mul(&t(el)).mul(&b[4]);
        b[7] = t(-e).mul(&DMat::rot_y(-a[22] * RAD)).mul(&t(e)).mul(&b[5]);
        // 11, 12: the hands, about the wrists
        let hl = m(-hd.x, hd.y, hd.z);
        b[11] = t(-hl)
            .mul(&DMat::rot_y(RAD * a[27]))
            .mul(&DMat::rot_z(RAD * a[25]))
            .mul(&DMat::rot_x(-a[23] * RAD))
            .mul(&t(hl))
            .mul(&b[6]);
        b[12] = t(-hd)
            .mul(&DMat::rot_y(-a[28] * RAD))
            .mul(&DMat::rot_z(-a[26] * RAD))
            .mul(&DMat::rot_x(-a[24] * RAD))
            .mul(&t(hd))
            .mul(&b[7]);
        // 10: the head, about the neck, on the upper body
        b[10] = t(-n)
            .mul(&DMat::rot_z(RAD * a[8]))
            .mul(&DMat::rot_x(RAD * a[7]))
            .mul(&DMat::rot_y((a[6] - a[29]) * RAD))
            .mul(&t(n))
            .mul(&b[9]);
        b
    }

    pub fn bones(&self, rig: &OmsiRig) -> [Affine3A; BONES] {
        self.bones_d3d(rig).map(|m| m.to_engine())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn rig() -> OmsiRig {
        let def = Human {
            height: 1.77,
            feet_dist: 0.04,
            seat_height: 0.83,
            links: vec![
                0.09, 0.0, 0.92, 0.09, -0.03, 0.53, 0.02, 1.17, 0.18, -0.05, 1.43, 0.44, -0.04,
                1.41, -0.02, 1.55, 0.69, -0.03, 1.43, 0.9, -0.03, 1.43,
            ],
            walk_param: [1.4, 80.0, 1.0, 1.0, 0.0],
            ..Default::default()
        };
        OmsiRig::new(&def)
    }

    #[test]
    fn curves_as_set_up_in_sub_624820() {
        assert_eq!(curve(&THIGH_CURVE, 0.0), 0.75);
        assert!((curve(&THIGH_CURVE, 0.1) - 0.925).abs() < 1e-6);
        assert!((curve(&THIGH_CURVE, 0.5) - 0.0).abs() < 1e-6);
        assert_eq!(curve(&KNEE_CURVE, 0.5), 0.0);
        assert_eq!(curve(&KNEE_CURVE, 1.0), 1.0);
    }

    #[test]
    fn d3dx_rotations_and_order() {
        // D3DX: RotationY(90°) takes +x to -z (left-handed, row vectors)
        let p = DMat::rot_y(std::f32::consts::FRAC_PI_2).point(Vec3::X);
        assert!((p - Vec3::new(0.0, 0.0, -1.0)).length() < 1e-6, "{p:?}");
        // a translation then a rotation: the rotation acts last
        let m = DMat::translation(Vec3::X).mul(&DMat::rot_z(std::f32::consts::FRAC_PI_2));
        let p = m.point(Vec3::ZERO);
        assert!((p - Vec3::new(0.0, 1.0, 0.0)).length() < 1e-6, "{p:?}");
    }

    #[test]
    fn the_hand_reaches_down_to_the_cash_desk() {
        // below the shoulder, as the money tray and the ticket slot are: the elbow stays under it
        let r = rig();
        for target in [
            Vec3::new(0.25, 1.0, 0.4),
            Vec3::new(0.1, 1.2, 0.45),
            Vec3::new(0.35, 1.1, 0.2),
        ] {
            let mut an = OmsiAnim::default();
            an.advance(
                &r,
                &AnimInput {
                    kind: 0,
                    room_height: 50.0,
                    dt_ms: 16.0,
                    reach: Some(target),
                    ..Default::default()
                },
            );
            let b = an.bones_d3d(&r);
            let finger = b[7].point(r.finger);
            let elbow = b[5].point(r.elbow);
            assert!(
                (finger - target).length() < 0.05,
                "{target:?}: the fingers at {finger:?}"
            );
            assert!(
                elbow.y < r.shoulder.y,
                "{target:?}: the elbow at {elbow:?} above the shoulder"
            );
        }
    }

    #[test]
    fn a_natural_reach_bends_the_wrist_and_leans_in_when_far() {
        let r = rig();
        let reach = |target: Vec3, frames: usize| {
            let mut an = OmsiAnim::default();
            for _ in 0..frames {
                an.advance(
                    &r,
                    &AnimInput {
                        kind: 0,
                        room_height: 50.0,
                        dt_ms: 16.0,
                        reach: Some(target),
                        natural: true,
                        ..Default::default()
                    },
                );
            }
            an
        };
        for target in [
            Vec3::new(0.25, 1.0, 0.4),
            Vec3::new(0.1, 1.2, 0.45),
            Vec3::new(0.25, 1.05, 0.62),
        ] {
            // past the fade in and the press
            let an = reach(target, 60);
            let b = an.bones_d3d(&r);
            let tip = b[12].point(r.finger);
            assert!(
                (tip - target).length() < 0.05,
                "{target:?}: the fingers at {tip:?}"
            );
            assert!(
                an.angles[26].abs() + an.angles[28].abs() > 5.0,
                "{target:?}: a stiff wrist"
            );
            let elbow = b[5].point(r.elbow);
            assert!(
                elbow.y < r.shoulder.y,
                "{target:?}: the elbow at {elbow:?} above the shoulder"
            );
        }
        let far = reach(Vec3::new(0.25, 1.05, 0.62), 60);
        assert!(far.angles[12] > 3.0, "{:?}", far.angles[12]);
        let near = reach(Vec3::new(0.25, 1.0, 0.4), 60);
        assert!(near.angles[12] < far.angles[12]);
    }

    #[test]
    fn standing_pose_keeps_the_feet_on_the_floor() {
        let r = rig();
        let mut an = OmsiAnim::default();
        an.advance(
            &r,
            &AnimInput {
                kind: 0,
                room_height: 50.0,
                dt_ms: 16.0,
                ..Default::default()
            },
        );
        let b = an.bones(&r);
        // the ankle stays at its height: the leg only turns in about the hip
        let foot = Vec3::new(0.09, -0.03, 0.05);
        let p = b[3].transform_point3(foot);
        assert!(
            (p.z - foot.z).abs() < 0.01 && (p.x - foot.x).abs() < 0.03,
            "{p:?}"
        );
        // the head is where the model has it (no turn)
        let head = Vec3::new(0.0, -0.02, 1.65);
        assert!((b[10].transform_point3(head) - head).length() < 1e-4);
    }

    #[test]
    fn walking_swings_the_legs_and_plays_steps() {
        let r = rig();
        let mut an = OmsiAnim::default();
        let mut steps = 0;
        let mut max_thigh: f32 = 0.0;
        for _ in 0..200 {
            let ev = an.advance(
                &r,
                &AnimInput {
                    kind: 1,
                    speed: 1.2,
                    moved: 1.2 * 0.016,
                    room_height: 50.0,
                    dt_ms: 16.0,
                    ..Default::default()
                },
            );
            steps += ev.step as u32;
            max_thigh = max_thigh.max(an.angles[0].abs());
        }
        // 200 frames at 1.2 m/s = 3.84 m = 2.74 strides of 1.4 m: a step every half stride
        assert!((4..=7).contains(&steps), "{steps}");
        // the thigh swings by 1.1 x (1.4 / (4 x 0.92)) rad = 24 degrees
        assert!((max_thigh - 24.0).abs() < 1.5, "{max_thigh}");
    }

    #[test]
    fn sitting_bends_hips_and_knees() {
        let r = rig();
        let mut an = OmsiAnim::default();
        an.advance(
            &r,
            &AnimInput {
                kind: 2,
                room_height: 50.0,
                seat_height: 0.45,
                dt_ms: 16.0,
                ..Default::default()
            },
        );
        assert!(an.angles[0] >= 60.0 && an.angles[4] >= 90.0);
        assert_eq!(an.angles[9], -20.0);
    }

    fn fingerprint(input: impl Fn(usize) -> AnimInput) -> f64 {
        let r = rig();
        let mut an = OmsiAnim::default();
        let mut sum = 0.0f64;
        for k in 0..240 {
            an.advance(&r, &input(k));
            sum += an
                .angles
                .iter()
                .enumerate()
                .map(|(j, a)| (j + 1) as f64 * *a as f64)
                .sum::<f64>();
            sum += an
                .bones_d3d(&r)
                .iter()
                .map(|b| b.0[3][1] as f64)
                .sum::<f64>()
                * 100.0;
        }
        sum
    }

    #[test]
    fn the_omsi_style_is_unchanged() {
        let walk = |speed: f32| {
            move |k: usize| AnimInput {
                kind: if k < 200 { 1 } else { 0 },
                speed,
                moved: speed * 0.016,
                room_height: if k > 120 && k < 150 { 1.6 } else { 50.0 },
                dt_ms: 16.0,
                reach: (k > 210).then_some(Vec3::new(0.2, 1.0, 0.4)),
                look: (k % 50 > 30).then_some(Vec3::new(1.0, 1.6, 2.0)),
                smooth: k > 220,
                ..Default::default()
            }
        };
        let sit = |k: usize| AnimInput {
            kind: 2,
            seat_height: 0.45,
            room_height: 50.0,
            dt_ms: 16.0,
            smooth: k % 2 == 0,
            ..Default::default()
        };
        // recorded from Omsi.exe's animation before the natural style was added
        for (got, want) in [
            (fingerprint(walk(0.8)), 829341.640916),
            (fingerprint(walk(1.6)), 908074.935841),
            (fingerprint(walk(3.0)), 903649.967610),
            (fingerprint(sit), 1478272.706201),
        ] {
            assert!((got - want).abs() < 1e-3, "{got} != {want}");
        }
    }

    fn natural_walk(an: &mut OmsiAnim, r: &OmsiRig, speed: f32, frames: usize) -> (u32, Vec<f32>) {
        let (mut steps, mut lowest) = (0, Vec::new());
        for _ in 0..frames {
            let ev = an.advance(
                r,
                &AnimInput {
                    kind: 1,
                    speed,
                    moved: speed * 0.016,
                    room_height: 50.0,
                    dt_ms: 16.0,
                    natural: true,
                    ..Default::default()
                },
            );
            steps += ev.step as u32;
            let b = an.bones(r);
            let ankle = Vec3::new(0.09, -0.03, 0.05);
            let left = b[2]
                .transform_point3(Vec3::new(-ankle.x, ankle.y, ankle.z))
                .z;
            lowest.push(left.min(b[3].transform_point3(ankle).z) - ankle.z);
        }
        (steps, lowest)
    }

    #[test]
    fn natural_steps_get_longer_and_quicker_with_speed() {
        let r = rig();
        let cadence = |speed: f32| {
            let mut an = OmsiAnim::default();
            natural_walk(&mut an, &r, speed, 625).0 as f32 / 10.0
        };
        let (slow, usual, brisk) = (cadence(0.8), cadence(1.3), cadence(1.8));
        assert!(slow < usual && usual < brisk, "{slow} {usual} {brisk}");
        // about 1.8 steps a second at 1.3 m/s, each about 0.7 m
        assert!((1.6..2.0).contains(&usual), "{usual}");
        assert!(1.3 / usual > 0.65 && 1.8 / brisk > 1.3 / usual);
    }

    #[test]
    fn running_has_a_flight_phase_and_walking_none() {
        let r = rig();
        let mut an = OmsiAnim::default();
        let (_, run) = natural_walk(&mut an, &r, 3.2, 300);
        let airborne = run.iter().filter(|z| **z > 0.03).count();
        assert!(airborne > 30, "{airborne} of 300 frames off the ground");
        let mut an = OmsiAnim::default();
        let (_, walk) = natural_walk(&mut an, &r, 1.3, 300);
        assert!(
            walk.iter().all(|z| *z < 0.02),
            "{:?}",
            walk.iter().fold(0f32, |a, b| a.max(*b))
        );
        let mut an = OmsiAnim::default();
        natural_walk(&mut an, &r, 3.2, 100);
        assert!(
            an.angles[21] > 60.0 && an.angles[12] > 6.0,
            "{:?}",
            an.angles
        );
    }

    #[test]
    fn walking_styles_differ_from_person_to_person() {
        let r = rig();
        let elbow = |seed: u32| {
            let mut an = OmsiAnim::default();
            let mut sum = 0.0;
            for _ in 0..200 {
                an.advance(
                    &r,
                    &AnimInput {
                        kind: 1,
                        speed: 1.3,
                        moved: 1.3 * 0.016,
                        room_height: 50.0,
                        dt_ms: 16.0,
                        natural: true,
                        seed,
                        ..Default::default()
                    },
                );
                sum += an.angles[21];
            }
            sum / 200.0
        };
        let all: Vec<f32> = (0..40).map(elbow).collect();
        let (lo, hi) = all
            .iter()
            .fold((f32::MAX, 0f32), |(l, h), x| (l.min(*x), h.max(*x)));
        assert!(hi - lo > 5.0, "{lo}..{hi}");
    }

    #[test]
    fn natural_standing_varies_and_changes_ease_in() {
        let r = rig();
        let stand = |seed: u32| AnimInput {
            kind: 0,
            room_height: 50.0,
            dt_ms: 16.0,
            natural: true,
            seed,
            ..Default::default()
        };
        let heads = |seed: u32| {
            let mut an = OmsiAnim::default();
            (0..1000)
                .map(|_| {
                    an.advance(&r, &stand(seed));
                    an.angles[6]
                })
                .collect::<Vec<_>>()
        };
        let (a, b) = (heads(1), heads(2));
        assert!(a != b);
        let spread = a.iter().fold(0f32, |m, x| m.max(x.abs()));
        assert!(spread > 3.0 && spread <= 50.0, "{spread}");
        assert!(
            a.windows(2).all(|w| (w[1] - w[0]).abs() < 2.0),
            "no jumps in the glances"
        );
        let mut an = OmsiAnim::default();
        natural_walk(&mut an, &r, 1.3, 40);
        let thigh = an.angles[0];
        an.advance(&r, &stand(1));
        assert!(
            (an.angles[0] - thigh).abs() < thigh.abs() * 0.2 + 0.5,
            "{thigh} -> {}",
            an.angles[0]
        );
        for _ in 0..30 {
            an.advance(&r, &stand(1));
        }
        assert!(an.angles[0].abs() < 1.0);
    }
}
