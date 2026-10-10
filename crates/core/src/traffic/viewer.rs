//! Viewer visibility/occlusion and the realized-body `Footprint` helpers.

use super::*;

impl Footprint {
    pub(crate) fn from_obb(car: usize, b: &::simulation::collision::Obb, speed: f32) -> Footprint {
        let (sh, ch) = (b.heading.sin(), b.heading.cos());
        Footprint {
            car,
            center: b.center,
            fwd: DVec2::new(sh, ch),
            right: DVec2::new(ch, -sh),
            half_len: b.half.y,
            half_w: b.half.x,
            speed,
            z: b.z0,
        }
    }


    /// Does this footprint overlap `o`, both grown by `margin` (separating axes)?
    pub(crate) fn overlaps(&self, o: &Footprint, margin: f64) -> bool {
        let d = o.center - self.center;
        for axis in [self.fwd, self.right, o.fwd, o.right] {
            let extent = |f: &Footprint| {
                (f.fwd.dot(axis)).abs() * (f.half_len + margin)
                    + (f.right.dot(axis)).abs() * (f.half_w + margin)
            };
            if d.dot(axis).abs() > extent(self) + extent(o) {
                return false;
            }
        }
        true
    }

}

impl Viewer {
    pub fn new(cam: &::render::Camera, aspect: f64, fog_range: f64) -> Viewer {
        let tan_y = (cam.fov_deg as f64 * 0.5).to_radians().tan();
        Viewer {
            pos: cam.position,
            forward: cam.forward().as_dvec3().normalize_or_zero(),
            tan_x: tan_y * aspect.max(0.2),
            tan_y,
            range: fog_range.min(VISIBLE_RANGE).min(cam.far as f64),
            min_size: 0.0,
            max_dist: 0.0,
            fov: (cam.fov_deg as f64).to_radians(),
        }
    }


    /// The renderer's culling as well (`RenderOptions::min_obj_size`, `max_obj_dist`).
    pub fn with_culling(mut self, min_size: f32, max_dist: f32) -> Viewer {
        self.min_size = min_size.max(0.0) as f64;
        self.max_dist = max_dist.max(0.0) as f64;
        self
    }


    /// Would the renderer draw an object of radius `r` this far away at all? Beyond that a
    /// car can come and go in plain view without anybody seeing it happen.
    pub fn draws(&self, dist: f64, r: f64) -> bool {
        // (the renderer measures a vehicle by a sphere about its origin, which may stand
        // well off its middle: half as much again, and a metre, to be sure)
        let r = r * 1.5 + 1.0;
        if self.max_dist > 0.0 && dist > self.max_dist + r {
            return false;
        }
        self.min_size <= 0.0 || 2.0 * r / (dist.max(0.01) * self.fov.max(1e-3)) >= self.min_size
    }


    /// Does a sphere of radius `r` at `p` lie within the view frustum and range?
    pub fn frames(&self, p: DVec3, r: f64) -> bool {
        let rel = p - self.pos;
        let dist = rel.length();
        if dist <= r {
            return true;
        }
        if dist - r > self.range {
            return false;
        }
        let f = self.forward;
        let mut right = f.cross(DVec3::Z);
        if right.length() < 1e-3 {
            right = DVec3::X;
        }
        let right = right.normalize();
        let up = right.cross(f);
        let z = rel.dot(f);
        if z < -r {
            return false;
        }
        let (x, y) = (rel.dot(right), rel.dot(up));
        x.abs() <= z * self.tan_x + r * (1.0 + self.tan_x * self.tan_x).sqrt()
            && y.abs() <= z * self.tan_y + r * (1.0 + self.tan_y * self.tan_y).sqrt()
    }

}

impl Traffic {

    /// Is a vehicle of radius `r` at `p` hidden from the viewer by buildings (or a hill)?
    /// Every line of sight to it must be: to its middle, to both ends whichever way it
    /// points and over its roof. A single line to the middle let a car come and go half
    /// out from behind a corner, in plain view.
    pub(crate) fn occluded(&self, world: &World, v: &Viewer, p: DVec3, r: f64) -> bool {
        let rel = (p - v.pos).truncate();
        let across = if rel.length() > 1e-3 {
            DVec3::new(-rel.y, rel.x, 0.0).normalize()
        } else {
            DVec3::X
        };
        let along = DVec3::new(rel.x, rel.y, 0.0).normalize_or_zero();
        let reach = (r * 0.8).max(1.5);
        let mut targets = vec![
            p + DVec3::new(0.0, 0.0, 1.2),
            p + across * reach + DVec3::new(0.0, 0.0, 1.0),
            p - across * reach + DVec3::new(0.0, 0.0, 1.0),
            p - along * reach + DVec3::new(0.0, 0.0, 1.0),
        ];
        // the roof of a bus or a lorry shows over a wall a car hides behind
        if r > 4.0 {
            targets.push(p + DVec3::new(0.0, 0.0, 3.2));
        }
        targets.into_iter().all(|t| self.sight_blocked(world, v, t))
    }


    /// Is the line of sight from the viewer to the point `target` blocked by a building
    /// (or a hill)?
    pub(crate) fn sight_blocked(&self, world: &World, v: &Viewer, target: DVec3) -> bool {
        let p = target;
        let blocker = match &self.occluders {
            Some(c) => c.ray_blocker(v.pos, target, 2.5, 3.0),
            None => world.collision.lock().ray_blocker(v.pos, target, 2.5, 3.0),
        };
        if let Some(b) = blocker {
            if self.debug_population {
                log::info!(
                    "  line of sight to ({:.0}, {:.0}) blocked by a box at ({:.1}, {:.1}) {:.1} x {:.1} m, z {:.1}..{:.1}",
                    p.x,
                    p.y,
                    b.center.x,
                    b.center.y,
                    b.half.x * 2.0,
                    b.half.y * 2.0,
                    b.z0,
                    b.z1
                );
            }
            return true;
        }
        // the ground between: a crest or an embankment
        for k in 1..8 {
            let t = k as f64 / 8.0;
            let q = v.pos.lerp(target, t);
            if let Some(g) = terrain_height(world, q.x, q.y) {
                if g > q.z + 0.5 {
                    if self.debug_population {
                        log::info!(
                            "  line of sight to ({:.0}, {:.0}) blocked by the ground at ({:.0}, {:.0}): {:.1} over {:.1}",
                            p.x,
                            p.y,
                            q.x,
                            q.y,
                            g,
                            q.z
                        );
                    }
                    return true;
                }
            }
        }
        false
    }


    /// May a vehicle be put on the road at `p` without the player seeing it appear? A
    /// timetable bus asks this before it spawns mid-route.
    pub fn may_appear(&self, world: &World, p: DVec3) -> bool {
        self.initial || self.hidden(world, p, 8.0)
    }


    /// Could the player not see a vehicle (radius `r`) at `p` appear or vanish? Never close
    /// by: mirrors, a turn of the head and the gaps between houses see what is near,
    /// whatever the collision boxes say (a bus let appear 40 m away behind a box that stood
    /// for a building with a gateway in it was seen popping up in the middle of the street).
    /// Within `NEAR_HIDE` only behind a building or the ground, wherever the camera looks -
    /// the mirrors look back, and the head turns: buses appeared 160 m behind the player
    /// in plain sight of the mirrors, and a bus waiting at the edge of the loaded route
    /// vanished beside the player's bus because the camera was looking ahead. Further off:
    /// beyond what is drawn, out of the picture, or behind something.
    pub fn hidden(&self, world: &World, p: DVec3, r: f64) -> bool {
        let Some(v) = self.viewer else { return true };
        let d = (p - v.pos).length();
        if d < NEVER_VANISH_WITHIN {
            return false;
        }
        if !v.draws(d, r) {
            return true;
        }
        if d < NEAR_HIDE {
            return self.occluded(world, &v, p, r);
        }
        !v.frames(p, r) || self.occluded(world, &v, p, r)
    }

}
