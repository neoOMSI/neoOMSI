use crate::*;

pub const SHADOW_RANGE: f32 = 140.0;
pub const SHADOW_RANGE_FAR: f32 = 700.0;
pub const SHADOW_RANGE_CLOSE: f32 = 32.0;
pub(crate) const SHADOW_CLOSE_MAX: u32 = 2048;

pub(crate) const SPOT_SLOTS: usize = 32;
pub(crate) const SPOT_ROWS: usize = SPOT_SLOTS / 4;
pub(crate) const SPOT_DRAWS_PER_FRAME: usize = 6;
pub(crate) const SPOT_REDRAW_AGE: u32 = 45;
pub(crate) const SPOT_CAM_RANGE: f64 = 70.0;
pub(crate) const SPOT_RANGE_MAX: f32 = 45.0;
pub(crate) const SPOT_NEAR: f32 = 0.8;
pub(crate) const SHADOW_SETS: usize = 3 + SPOT_SLOTS;

#[derive(Clone, Copy)]
pub(crate) struct SpotPose {
    pub(crate) pos: DVec3,
    pub(crate) dir: Vec3,
    pub(crate) fov: f32,
    pub(crate) far: f32,
}

#[derive(Clone, Copy, Default)]
pub(crate) struct SpotSlot {
    pub(crate) seen: Option<SpotPose>,
    pub(crate) drawn: Option<SpotPose>,
    pub(crate) age: u32,
}

#[derive(Default)]
pub(crate) struct SpotShadowState {
    pub(crate) slots: [SpotSlot; SPOT_SLOTS],
    pub(crate) draws: Vec<usize>,
}

pub(crate) fn spot_view_proj(pos: Vec3, dir: Vec3, fov: f32, near: f32, far: f32) -> Mat4 {
    let f = dir.normalize_or_zero();
    let hint = if f.z.abs() > 0.95 { Vec3::Y } else { Vec3::Z };
    let r = f.cross(hint).normalize_or_zero();
    let u = r.cross(f);
    let view = Mat4::from_cols(
        Vec4::new(r.x, u.x, -f.x, 0.0),
        Vec4::new(r.y, u.y, -f.y, 0.0),
        Vec4::new(r.z, u.z, -f.z, 0.0),
        Vec4::new(-r.dot(pos), -u.dot(pos), f.dot(pos), 1.0),
    );
    let t = 1.0 / (fov * 0.5).tan();
    let proj = Mat4::from_cols(
        Vec4::new(t, 0.0, 0.0, 0.0),
        Vec4::new(0.0, t, 0.0, 0.0),
        Vec4::new(0.0, 0.0, far / (near - far), -1.0),
        Vec4::new(0.0, 0.0, near * far / (near - far), 0.0),
    );
    proj * view
}

impl Renderer {
    pub(crate) fn plan_spot_shadows(
        &self,
        scene: &Scene,
        cam: DVec3,
        enhanced: bool,
        plan: bool,
    ) -> Vec<u32> {
        let mut out = vec![0u32; scene.lights.len()];
        let mut st = self.spot_state.borrow_mut();
        let mut cands: Vec<(f32, usize, SpotPose)> = Vec::new();
        for (i, l) in scene.lights.iter().enumerate() {
            if !drawn_by(l, enhanced) || l.is_screen() {
                continue;
            }
            let d = (l.position - cam).length();
            if d > SPOT_CAM_RANGE {
                continue;
            }
            let far = l.radius.clamp(6.0, SPOT_RANGE_MAX);
            let pose = if l.direction.length_squared() < 1e-6 {
                SpotPose {
                    pos: l.position,
                    dir: -Vec3::Z,
                    fov: 2.5,
                    far,
                }
            } else {
                if l.cone[1] <= -0.99 {
                    continue;
                }
                let half = l.cone[1].clamp(-1.0, 1.0).acos();
                SpotPose {
                    pos: l.position,
                    dir: l.direction.normalize(),
                    fov: (2.0 * half + 0.09).clamp(0.2, 2.6),
                    far,
                }
            };
            let score = if l.shadow_first {
                let held = st.slots.iter().any(|sl| {
                    sl.seen.is_some_and(|s| {
                        (s.pos - pose.pos).length() < 2.5 && s.dir.dot(pose.dir) > 0.7
                    })
                });
                1.0e6 + (SPOT_CAM_RANGE - d).max(0.0) as f32 + if held { 30.0 } else { 0.0 }
            } else {
                l.intensity.clamp(0.05, 3.0) * far * far / (1.0 + (d * d) as f32)
            };
            cands.push((score, i, pose));
        }
        cands.sort_by(|a, b| b.0.total_cmp(&a.0));
        cands.truncate(SPOT_SLOTS);
        let mut slots = st.slots;
        let mut claimed = [false; SPOT_SLOTS];
        let mut assign: Vec<Option<usize>> = vec![None; cands.len()];
        for (ci, (_, _, pose)) in cands.iter().enumerate() {
            let mut best: Option<usize> = None;
            let mut best_d = 2.5f64;
            for (k, sl) in slots.iter().enumerate() {
                if claimed[k] {
                    continue;
                }
                if let Some(seen) = sl.seen {
                    let dd = (seen.pos - pose.pos).length();
                    if dd < best_d && seen.dir.dot(pose.dir) > 0.7 {
                        best = Some(k);
                        best_d = dd;
                    }
                }
            }
            if let Some(k) = best {
                claimed[k] = true;
                assign[ci] = Some(k);
            }
        }
        for a in assign.iter_mut() {
            if a.is_none() {
                if let Some(k) = (0..SPOT_SLOTS).find(|&k| !claimed[k]) {
                    claimed[k] = true;
                    slots[k] = SpotSlot::default();
                    *a = Some(k);
                }
            }
        }
        for k in 0..SPOT_SLOTS {
            if !claimed[k] {
                slots[k] = SpotSlot::default();
            }
        }
        if plan {
            let mut wants: Vec<(f32, usize)> = Vec::new();
            for (ci, (score, _, pose)) in cands.iter().enumerate() {
                let Some(k) = assign[ci] else { continue };
                slots[k].age += 1;
                let prio = match slots[k].drawn {
                    None => 1000.0 + *score,
                    Some(d) => {
                        let moved =
                            (d.pos - pose.pos).length() > 0.04 || d.dir.dot(pose.dir) < 0.99999;
                        if moved {
                            10.0 + *score
                        } else if slots[k].age >= SPOT_REDRAW_AGE {
                            1.0 + slots[k].age as f32 * 0.01
                        } else {
                            slots[k].seen = Some(*pose);
                            continue;
                        }
                    }
                };
                slots[k].seen = Some(*pose);
                wants.push((prio, k));
            }
            for (ci, (_, _, pose)) in cands.iter().enumerate() {
                if let Some(k) = assign[ci] {
                    slots[k].seen = Some(*pose);
                }
            }
            wants.sort_by(|a, b| b.0.total_cmp(&a.0));
            st.draws.clear();
            for &(_, k) in wants.iter().take(SPOT_DRAWS_PER_FRAME) {
                slots[k].drawn = slots[k].seen;
                slots[k].age = 0;
                st.draws.push(k);
            }
            st.slots = slots;
            if ::legacy_config::env::var_os("OMSI_DEBUG_LIGHT_SHADOWS").is_some() {
                static LAST: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
                let now = self.started.elapsed().as_secs();
                if LAST.swap(now, std::sync::atomic::Ordering::Relaxed) != now {
                    log::info!(
                        "light shadows: {} lights, {} candidates in reach, {} drawn this frame",
                        scene.lights.len(),
                        cands.len(),
                        st.draws.len()
                    );
                }
            }
        }
        for (ci, (_, li, pose)) in cands.iter().enumerate() {
            if let Some(k) = assign[ci] {
                if let Some(d) = slots[k].drawn {
                    let (tol, cos) = if scene.lights[*li].shadow_first {
                        (2.0, 0.9)
                    } else {
                        (0.5, 0.98)
                    };
                    if (d.pos - pose.pos).length() < tol && d.dir.dot(pose.dir) > cos {
                        out[*li] = k as u32 + 1;
                    }
                }
            }
        }
        out
    }
}
