use super::REMOTE_KEY;
use crate::App;
use crate::humans::{AvatarCmd, BusId, Humans};
use glam::{DVec2, DVec3};

impl App {
    pub(crate) fn walker_pose(&self) -> Option<omsi_net::Walker> {
        let f = self.on_foot.as_ref()?;
        let my_id = self.lan.as_ref().map(|l| l.my_id).filter(|i| *i != 0);
        let owner = |b: BusId| match b {
            BusId::Player => my_id,
            BusId::Ai(x) => crate::humans::remote_bus_player(x),
        };
        let aboard = match (f.seat, f.inside) {
            (Some((b, k)), _) => owner(b).map(|o| omsi_net::Aboard {
                owner: o,
                local: self
                    .humans
                    .as_ref()
                    .and_then(|h| h.seat_stand(b, k))
                    .map(|l| l.to_array())
                    .unwrap_or_default(),
                seat: Some(k as u16),
            }),
            (None, Some((b, l))) => owner(b).map(|o| omsi_net::Aboard {
                owner: o,
                local: l.to_array(),
                seat: None,
            }),
            _ => None,
        };
        let course = if f.vel.length() > 0.05 {
            f.vel.x.atan2(f.vel.y).to_degrees() as f32
        } else {
            f.heading as f32
        };
        Some(omsi_net::Walker {
            x: f.pos.x,
            y: f.pos.y,
            z: f.pos.z + f.lift,
            heading: f.heading as f32,
            speed: f.vel.length() as f32,
            course,
            seated: f.seat.is_some(),
            aboard,
        })
    }

    pub(crate) fn sync_remote_walkers(&mut self) {
        let walkers: Vec<(u32, Option<omsi_net::Walker>, String)> = self
            .remotes
            .remotes
            .iter()
            .map(|(id, r)| (*id, r.last.walker, r.last.figure.clone()))
            .collect();
        if walkers.iter().all(|w| w.1.is_none()) && self.remote_walkers.is_empty() {
            return;
        }
        if walkers.iter().any(|w| w.1.is_some()) && self.humans.is_none() {
            let mut h = Humans::new(&self.args.root);
            h.avatar_only = true;
            self.humans = Some(h);
        }
        let (Some(h), Some(w), Some(r), Some(scene)) = (
            self.humans.as_mut(),
            self.world.as_ref(),
            self.renderer.as_ref(),
            self.scene.as_mut(),
        ) else {
            return;
        };
        let my_id = self.lan.as_ref().map(|l| l.my_id).unwrap_or(0);
        let mut now = Vec::new();
        for (id, wk, figure) in walkers {
            let Some(wk) = wk else { continue };
            let hh = (if wk.course.is_finite() {
                wk.course
            } else {
                wk.heading
            } as f64)
                .to_radians();
            let kind = omsi_net::human_path(&figure)
                .map(|rel| omsi_cfg::resolve_path(&self.args.root, &rel))
                .filter(|p| omsi_cfg::vfs::exists(p))
                .and_then(|p| crate::driver::cached_type(&p))
                .map(|t| h.type_index(t) as u64)
                .unwrap_or(id as u64 * 13 + 5);
            let aboard = wk.aboard.and_then(|a| {
                let bus = if a.owner == my_id {
                    BusId::Player
                } else {
                    BusId::Ai(crate::humans::remote_bus_id(a.owner))
                };
                let (at, bh) = h.cabin_world(bus, glam::Vec3::from(a.local))?;
                Some((bus, a.seat, at, bh))
            });
            let cmd = match aboard {
                Some((bus, Some(k), at, _)) => AvatarCmd {
                    pos: at,
                    heading: wk.heading as f64,
                    vel: DVec2::ZERO,
                    lift: 0.0,
                    seat: Some((bus, k as usize)),
                    floor: Some(at.z),
                    aboard: None,
                },
                Some((bus, None, at, _)) => AvatarCmd {
                    pos: at,
                    heading: wk.heading as f64,
                    vel: DVec2::new(hh.sin(), hh.cos()) * wk.speed as f64,
                    lift: 0.0,
                    seat: None,
                    floor: Some(at.z),
                    aboard: wk.aboard.map(|a| (bus, glam::Vec3::from(a.local))),
                },
                None if wk.seated => continue,
                None => AvatarCmd {
                    pos: DVec3::new(wk.x, wk.y, wk.z),
                    heading: wk.heading as f64,
                    vel: DVec2::new(hh.sin(), hh.cos()) * wk.speed as f64,
                    lift: 0.0,
                    seat: None,
                    floor: w
                        .walk_height(wk.x, wk.y)
                        .filter(|g| wk.z > g + 0.25)
                        .map(|_| wk.z),
                    aboard: None,
                },
            };
            h.avatar(REMOTE_KEY + id, w, r, scene, cmd, kind);
            if !self.remote_walkers.contains(&id) {
                log::info!(
                    "LAN: player {id} got up and walks at ({:.1}, {:.1})",
                    wk.x,
                    wk.y
                );
            }
            now.push(id);
        }
        for id in std::mem::take(&mut self.remote_walkers) {
            if !now.contains(&id) {
                h.avatar_remove(REMOTE_KEY + id);
            }
        }
        self.remote_walkers = now;
    }
}
