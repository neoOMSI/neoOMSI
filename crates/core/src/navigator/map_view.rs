use super::*;

impl Navigator {
    pub fn arrow_spots(
        &self,
        traffic: Option<&Network>,
        reach: f64,
        stop_pose: &dyn Fn(i64) -> Option<(DVec3, f64)>,
    ) -> Vec<(u64, DVec3, f64, &'static str, String)> {
        let mut out = Vec::new();
        if !self.arrows {
            return out;
        }
        let global = self.global.clone();
        let Some(net) = global.as_deref().or(traffic) else {
            return out;
        };
        let r = &self.route;
        if !r.on_route {
            return out;
        }
        let mut acc = -(r.s as f64);
        let mut prev_end: Option<f32> = None;
        for (j, &l) in r.lanes.iter().enumerate().skip(r.progress) {
            let Some(lane) = net.lanes.get(l) else { break };
            let len = lane.length();
            if acc > reach {
                break;
            }
            let (h0, h1) = (lane.start_heading(), lane.end_heading());
            let mut d = ::traffic::wrap_deg(h1 - h0);
            if let Some(pe) = prev_end {
                d += ::traffic::wrap_deg(h0 - pe);
            }
            let junction =
                net.crossings.get(l).map(|c| !c.is_empty()).unwrap_or(false) || lane.turn != 0;
            let turn = d.abs() > 35.0 && (len < 60.0 || d.abs() > 70.0);
            if (turn || junction) && acc + len as f64 > 5.0 {
                let kind = if !turn {
                    "dn"
                } else if d > 0.0 {
                    "R"
                } else {
                    "L"
                };
                let (p, _) = lane.at(10.0f32.min(len * 0.7));
                let h = h0;
                let text = r
                    .lanes
                    .iter()
                    .skip(j)
                    .take(5)
                    .find_map(|&x| self.street_of(x))
                    .map(str::to_string)
                    .unwrap_or_default();
                out.push((l as u64, p, h as f64, kind, text));
            }
            prev_end = Some(h1);
            acc += len as f64;
        }
        for (k, (p, name, h, id)) in self.stop_spots.iter().enumerate() {
            let (p, h) = stop_pose(*id).unwrap_or((*p, *h));
            let d = (p - self.bus_at).truncate().length();
            if d < reach && k < 2 {
                out.push((
                    (1u64 << 40) + p.x.to_bits().rotate_left(7) ^ p.y.to_bits() ^ h.to_bits(),
                    p,
                    h,
                    "busstop",
                    name.clone(),
                ));
            }
        }
        out
    }

    pub fn toggle_map(&mut self) {
        self.city.open = !self.city.open;
        if self.city.open {
            self.city.follow = true;
            if self.city.mpp <= 0.0 {
                self.city.mpp = ::legacy_config::env::var("OMSI_NAV_MAP_MPP")
                    .ok()
                    .and_then(|v| v.parse::<f64>().ok())
                    .filter(|v| v.is_finite() && (0.25..=20.0).contains(v))
                    .unwrap_or(2.5);
            }
        }
        self.city.drag = None;
    }

    pub fn map_open(&self) -> bool {
        self.city.open
    }

    pub fn embed_map(&mut self, rect: Option<[f32; 4]>) {
        match rect {
            Some(r) => {
                if self.city.embed.is_none() {
                    self.city.open = false;
                    self.toggle_map();
                }
                self.city.embed = Some(r);
            }
            None => {
                if self.city.embed.take().is_some() {
                    self.city.open = false;
                    self.city.picture = None;
                    self.city.drag = None;
                }
            }
        }
    }

    pub fn over_panel(&self, x: f32, y: f32) -> bool {
        let r = self.panel_rect;
        self.enabled && x >= r[0] && y >= r[1] && x < r[2] && y < r[3]
    }

    pub(super) fn map_hit(&self, x: f32, y: f32) -> bool {
        let r = self.city.rect;
        x >= r[0] && y >= r[1] && x < r[2] && y < r[3]
    }

    pub fn map_press(&mut self, x: f32, y: f32) {
        let embedded = self.city.embed.is_some();
        if !self.map_hit(x, y) {
            if !embedded {
                self.city.open = false;
            }
            return;
        }
        let local = Vec2::new(x - self.city.rect[0], y - self.city.rect[1]);
        let hit = self
            .city
            .buttons
            .iter()
            .find(|(r, _)| r.contains(local))
            .map(|b| b.1);
        match hit {
            Some(0) => self.city.follow = true,
            Some(1) => self.city.mpp = (self.city.mpp / 1.6).max(0.25),
            Some(2) => self.city.mpp = (self.city.mpp * 1.6).min(self.max_mpp()),
            Some(3) if embedded => {}
            Some(4) => {}
            Some(3) => self.city.open = false,
            _ => self.city.drag = Some((x, y)),
        }
    }

    pub fn duty_button(&self) -> [f32; 4] {
        if self.city.embed.is_none() {
            return [0.0; 4];
        }
        match self.city.buttons.iter().find(|(_, id)| *id == 4) {
            Some((r, _)) => {
                let (x, y) = (self.city.rect[0] + r.x, self.city.rect[1] + r.y);
                [x, y, x + r.w, y + r.h]
            }
            None => [0.0; 4],
        }
    }

    pub fn map_point(&self, x: f32, y: f32) -> Option<DVec2> {
        if !self.city.open || !self.map_hit(x, y) {
            return None;
        }
        let r = self.city.rect;
        let (w, h) = ((r[2] - r[0]) as f64, (r[3] - r[1]) as f64);
        let (lx, ly) = ((x - r[0]) as f64 - w * 0.5, (y - r[1]) as f64 - h * 0.5);
        Some(self.city.center + DVec2::new(lx, -ly) * self.city.mpp)
    }

    pub fn map_release(&mut self) {
        self.city.drag = None;
    }

    pub fn map_move(&mut self, x: f32, y: f32) {
        if let Some((px, py)) = self.city.drag {
            let (dx, dy) = ((x - px) as f64, (y - py) as f64);
            if dx.abs() + dy.abs() > 0.5 {
                self.city.follow = false;
            }
            self.city.center.x -= dx * self.city.mpp;
            self.city.center.y += dy * self.city.mpp;
            self.city.drag = Some((x, y));
        }
    }

    pub fn map_wheel(&mut self, amount: f32, x: f32, y: f32) {
        let r = self.city.rect;
        let (w, h) = ((r[2] - r[0]) as f64, (r[3] - r[1]) as f64);
        let (lx, ly) = ((x - r[0]) as f64 - w * 0.5, (y - r[1]) as f64 - h * 0.5);
        let before = self.city.center + DVec2::new(lx, -ly) * self.city.mpp;
        let k = (1.0 - amount as f64 * 0.15).clamp(0.6, 1.6);
        self.city.mpp = (self.city.mpp * k).clamp(0.25, self.max_mpp());
        let after = self.city.center + DVec2::new(lx, -ly) * self.city.mpp;
        self.city.center += before - after;
        if amount.abs() > 0.0 && (lx.abs() > 40.0 || ly.abs() > 40.0) {
            self.city.follow = false;
        }
    }

    pub(super) fn max_mpp(&self) -> f64 {
        let (lo, hi) = self.city.extent;
        let span = (hi - lo).max_element().max(2000.0);
        let r = self.city.rect;
        span / ((r[2] - r[0]).max(200.0) as f64) * 1.2
    }
}

