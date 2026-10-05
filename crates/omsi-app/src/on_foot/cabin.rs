use super::collision::push_out;
use super::vehicle::cabin_local;
use super::{OnFoot, RADIUS};
use crate::App;
use glam::{DVec2, DVec3};

const DOOR_GRACE: f32 = 3.0;
const LANE_LEAD: f64 = 0.5;
const DOOR_HALF: f64 = 0.55;

pub(super) type DoorPath = (DVec2, DVec2, f64, f64, f64);

pub(super) struct BusStep {
    pub inside_moved: bool,
    pub exempt: Option<DVec2>,
    pub door: Option<DoorPath>,
}

impl BusStep {
    fn none() -> BusStep {
        BusStep {
            inside_moved: false,
            exempt: None,
            door: None,
        }
    }
}

impl App {
    pub(super) fn foot_bus_step(&self, f: &mut OnFoot, dt: f64) -> BusStep {
        let Some(h) = self.humans.as_ref() else {
            return BusStep::none();
        };
        f.door_grace = (f.door_grace - dt as f32).max(0.0);
        if let Some((bus, local)) = f.inside {
            let hd = h
                .cabin_world(bus, local)
                .map(|x| x.1)
                .unwrap_or(0.0)
                .to_radians();
            let (bf, br) = (
                DVec2::new(hd.sin(), hd.cos()),
                DVec2::new(hd.cos(), -hd.sin()),
            );
            let step = glam::Vec2::new((f.vel.dot(br) * dt) as f32, (f.vel.dot(bf) * dt) as f32);
            let grace = f.door_grace > 0.0;
            if let Some((l, w)) = h.cabin_walk(bus, local, step, grace) {
                f.pos = w;
                f.inside = Some((bus, l));
                let free = |o: DVec3| {
                    let at = o.truncate();
                    !self
                        .foot_solids(at, o.z, Some(w.truncate()))
                        .iter()
                        .any(|b| (push_out(at, b, RADIUS) - at).length() > 0.05)
                };
                let mut leaves = false;
                let mut near_open = false;
                for (inside, outside, _, open) in h.cabin_doors(bus) {
                    if (inside.z - l.z).abs() > 0.8
                        || (inside.truncate() - l.truncate()).length() >= 0.6
                    {
                        continue;
                    }
                    near_open |= open;
                    if !open && !grace {
                        continue;
                    }
                    let Some(ol) = cabin_local(h, bus, outside.truncate(), inside.z) else {
                        continue;
                    };
                    let out = (ol.truncate() - inside.truncate()).normalize_or_zero();
                    if step.dot(out) > 0.0005 && (open || free(outside)) {
                        leaves = true;
                    }
                }
                if near_open {
                    f.door_grace = DOOR_GRACE;
                }
                if leaves {
                    f.inside = None;
                }
            }
            f.lift = 0.0;
            f.vz = 0.0;
            return BusStep {
                inside_moved: true,
                exempt: None,
                door: None,
            };
        }
        let mut out = BusStep::none();
        let mut best: Option<(
            f64,
            crate::humans::BusId,
            glam::Vec3,
            glam::DVec3,
            DVec2,
            DVec2,
            f64,
            f64,
            f64,
        )> = None;
        let mut best_open = false;
        for bus in h.bus_ids_near(f.pos, 25.0) {
            for (inside, outside, _, open) in h.cabin_doors(bus) {
                if !open && f.door_grace <= 0.0 {
                    continue;
                }
                let Some((wi, _)) = h.cabin_world(bus, inside) else {
                    continue;
                };
                let (a, b) = (outside.truncate(), wi.truncate());
                let ab = b - a;
                let len = ab.length();
                if len < 1e-3 {
                    continue;
                }
                let dir = ab / len;
                let rel = f.pos.truncate() - a;
                let (along, lateral) = (rel.dot(dir), rel.perp_dot(dir).abs());
                if along < -1.2 || along > len + 1.0 || lateral > DOOR_HALF + 0.15 {
                    continue;
                }
                if !open && along <= 0.0 {
                    continue;
                }
                if best.as_ref().is_none_or(|x| lateral < x.0) {
                    best = Some((lateral, bus, inside, wi, a, dir, len, along, outside.z));
                    best_open = open;
                }
            }
        }
        if let Some((_, bus, inside, wi, a, dir, len, mut along, oz)) = best {
            if best_open {
                f.door_grace = DOOR_GRACE;
            } else if along > len - 0.25 {
                f.pos.x -= dir.x * (along - (len - 0.25));
                f.pos.y -= dir.y * (along - (len - 0.25));
                along = len - 0.25;
            }
            out.exempt = Some(wi.truncate());
            let z0 = self
                .world
                .as_ref()
                .and_then(|w| w.walk_height_near(a.x, a.y, oz))
                .unwrap_or(oz);
            out.door = Some((a, dir, len, z0, wi.z));
            if best_open && along >= len - 0.25 && f.vel.dot(dir) > 0.0 {
                if let Some(l) = cabin_local(h, bus, f.pos.truncate(), inside.z) {
                    f.inside = Some((bus, l));
                    f.lift = 0.0;
                    f.vz = 0.0;
                    out.inside_moved = true;
                    return out;
                }
            }
        }
        out
    }
}

pub(super) fn door_lane(f: &mut OnFoot, path: DoorPath, _dt: f64) {
    let (a, dir, len, z0, z1) = path;
    let mut next = f.pos.truncate();
    let rel = next - a;
    let along = rel.dot(dir);
    let perp = DVec2::new(-dir.y, dir.x);
    if along > 0.0 {
        let lat = rel.dot(perp);
        next -= perp * (lat - lat.clamp(-DOOR_HALF, DOOR_HALF));
    }
    f.pos.x = next.x;
    f.pos.y = next.y;
    let k = ((along + LANE_LEAD) / (len + LANE_LEAD)).clamp(0.0, 1.0);
    let k = k * k * (3.0 - 2.0 * k);
    f.pos.z = z0 + (z1 - z0) * k;
    f.on_lane = true;
    f.lift = 0.0;
    f.vz = 0.0;
}
