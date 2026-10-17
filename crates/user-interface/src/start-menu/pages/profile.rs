use super::*;

fn tl(key: &str, params: &[(&str, &dyn std::fmt::Display)]) -> String {
    ::i18n::translate(key, params)
}

fn fmt_time(secs: i64) -> String {
    let m = secs.max(0) / 60;
    if m >= 60 {
        tl("start.profile.hm", &[("h", &(m / 60)), ("m", &(m % 60))])
    } else {
        tl("start.profile.min", &[("m", &m)])
    }
}

fn fmt_km(meters: i64) -> String {
    let km = meters.max(0) as f32 / 1000.0;
    if km < 100.0 {
        format!("{km:.1} km")
    } else {
        format!("{km:.0} km")
    }
}

const RATINGS: [(&str, &str); 3] = [
    ("start.profile.driving", "start.profile.driving_note"),
    ("start.profile.comfort", "start.profile.comfort_note"),
    ("start.profile.tickets", "start.profile.tickets_note"),
];

impl Ui {
    pub(in super::super) fn draw_start_profile(&mut self, r: &Renderer, scene: &mut Scene, f: &Frame, m: Metrics, v: &StartView) {
        let Metrics { w, h, u, .. } = m;
        let p = v.profile;
        self.text.rounded(r, scene, [0.0, 0.0, w, h], 0.0, BG);
        self.start.bare = true;
        let bar_h = self.draw_start_bar(r, scene, f, m, 0);
        let ep = out(self.start.page_t);
        let rise = 14.0 * u * (1.0 - ep);
        let card = [0.0, bar_h + rise, w, h + rise];
        self.text.alpha = ep;
        self.text.rounded(r, scene, card, 0.0, OPT_CARD);
        self.text.rounded(r, scene, [0.0, bar_h, w, bar_h + 1.0], 0.0, DIVIDER);

        let colw = (960.0 * u).min(w - 96.0 * u);
        let x0 = ((w - colw) * 0.5).round();
        let x1 = x0 + colw;
        let mut y = card[1] + 32.0 * u;

        let d = (96.0 * u).round();
        self.text.rounded(r, scene, [x0, y, x0 + d, y + d], d * 0.5, OPT_LINE);
        let ring = (6.0 * u).round();
        self.text.rounded(r, scene, [x0 + ring, y + ring, x0 + d - ring, y + d - ring], (d - 2.0 * ring) * 0.5, ACCENT);
        let ini = p.name.chars().next().map(|c| c.to_uppercase().to_string()).unwrap_or_else(|| "D".into());
        let il = self.text.label(r, scene, &ini, (44.0 * u) as u32, ON_ACCENT);
        il.place(scene, x0 + (d - il.w as f32) * 0.5, y + (d - il.h as f32) * 0.5);

        let nl = self.text.label(r, scene, &clip_to(&self.text, &p.name, 40.0 * u, colw - d - 24.0 * u), (40.0 * u) as u32, WHITE);
        let sub = tl("start.profile.level", &[("level", &p.level), ("xp", &p.xp_to_next), ("next", &(p.level + 1))]);
        let sl = self.text.label(r, scene, &sub, (15.0 * u) as u32, SOFT);
        let bh = (6.0 * u).round();
        let block = nl.h as f32 + sl.h as f32 + 10.0 * u + bh;
        let tx = x0 + d + 24.0 * u;
        let ty = y + (d - block) * 0.5;
        nl.place(scene, tx, ty);
        sl.place(scene, tx, ty + nl.h as f32);
        let by = ty + nl.h as f32 + sl.h as f32 + 10.0 * u;
        let bw = 280.0 * u;
        self.text.rounded(r, scene, [tx, by, tx + bw, by + bh], bh * 0.5, OPT_TRACK);
        let fw = (bw * p.xp_frac.clamp(0.0, 1.0)).max(bh);
        self.text.rounded(r, scene, [tx, by, tx + fw, by + bh], bh * 0.5, ACCENT);
        y += d + 36.0 * u;

        let cw = colw / 4.0;
        let on_time = p.on_time.map_or_else(|| "–".to_string(), |t| format!("{t} %"));
        let stats = [
            ("start.profile.hours", fmt_time(p.secs)),
            ("start.profile.distance", fmt_km(p.meters)),
            ("start.profile.stops", p.stops.to_string()),
            ("start.profile.ontime", on_time),
        ];
        let mut sh = 0.0;
        for (i, (key, val)) in stats.iter().enumerate() {
            let sx = x0 + cw * i as f32;
            let cap = self.text.label(r, scene, key, (13.0 * u) as u32, MUTED);
            let vl = self.text.label(r, scene, val, (30.0 * u) as u32, WHITE);
            let ix = if i == 0 { sx } else { sx + 20.0 * u };
            cap.place(scene, ix, y);
            vl.place(scene, ix, y + cap.h as f32 + 2.0 * u);
            sh = cap.h as f32 + vl.h as f32 + 2.0 * u;
            if i > 0 {
                self.text.rounded(r, scene, [sx, y, sx + 1.0, y + sh], 0.0, [255, 255, 255, 20]);
            }
        }
        y += sh + 36.0 * u;

        let hl = self.text.label(r, scene, "start.profile.ratings", (13.0 * u) as u32, MUTED);
        hl.place(scene, x0, y);
        y += hl.h as f32 + 10.0 * u;
        let rh = (68.0 * u).round();
        for (i, (key, note)) in RATINGS.iter().enumerate() {
            let t = self.text.label(r, scene, key, (17.0 * u) as u32, WHITE);
            let n = self.text.label(r, scene, note, (13.0 * u) as u32, MUTED);
            let ty = y + (rh - t.h as f32 - n.h as f32) * 0.5;
            t.place(scene, x0, ty);
            n.place(scene, x0, ty + t.h as f32);
            let val = p.ratings[i];
            let num = val.map_or_else(|| "–".to_string(), |x| x.to_string());
            let nl = self.text.label(r, scene, &num, (16.0 * u) as u32, WHITE);
            nl.place(scene, x1 - nl.w as f32 - 8.0 * u, y + (rh - nl.h as f32) * 0.5);
            let (tw, th) = (200.0 * u, (5.0 * u).round());
            let tx = x1 - 64.0 * u - tw;
            let tyy = y + (rh - th) * 0.5;
            self.text.rounded(r, scene, [tx, tyy, tx + tw, tyy + th], th * 0.5, OPT_LINE);
            if let Some(x) = val {
                let fw = (tw * x.min(100) as f32 / 100.0).max(th);
                self.text.rounded(r, scene, [tx, tyy, tx + fw, tyy + th], th * 0.5, ACCENT);
            }
            y += rh;
            self.text.rounded(r, scene, [x0, y, x1, y + 1.0], 0.0, [255, 255, 255, 14]);
        }
        y += 28.0 * u;

        let hl = self.text.label(r, scene, "start.profile.recent", (13.0 * u) as u32, MUTED);
        hl.place(scene, x0, y);
        y += hl.h as f32 + 10.0 * u;
        if p.drives.is_empty() {
            let l = self.text.label(r, scene, "start.profile.no_drives", (15.0 * u) as u32, MUTED);
            l.place(scene, x0, y + 6.0 * u);
        }
        let dh = (60.0 * u).round();
        let isz = (22.0 * u).round();
        for dr in &p.drives {
            if y + dh > card[3] - 8.0 * u {
                break;
            }
            if let Some(ic) = self.text.icon(r, scene, "explore", isz as u32, MUTED) {
                ic.place(scene, x0 + 14.0 * u, y + (dh - isz) * 0.5);
            }
            let tx = x0 + 14.0 * u + isz + 18.0 * u;
            let room = colw * 0.5 - 60.0 * u;
            let t = self.text.label(r, scene, &clip_to(&self.text, &dr.title, 17.0 * u, room), (17.0 * u) as u32, WHITE);
            if dr.sub.is_empty() {
                t.place(scene, tx, y + (dh - t.h as f32) * 0.5);
            } else {
                let n = self.text.label(r, scene, &clip_to(&self.text, &dr.sub, 13.0 * u, room), (13.0 * u) as u32, MUTED);
                let ty = y + (dh - t.h as f32 - n.h as f32) * 0.5;
                t.place(scene, tx, ty);
                n.place(scene, tx, ty + t.h as f32);
            }
            let r_ago = x1 - 8.0 * u;
            let cells = [(fmt_time(dr.secs), r_ago - 220.0 * u), (fmt_km(dr.meters), r_ago - 110.0 * u), (dr.ago.clone(), r_ago)];
            for (txt, right) in &cells {
                let l = self.text.label(r, scene, txt, (15.0 * u) as u32, SOFT);
                l.place(scene, right - l.w as f32, y + (dh - l.h as f32) * 0.5);
            }
            y += dh;
            self.text.rounded(r, scene, [x0, y, x1, y + 1.0], 0.0, [255, 255, 255, 14]);
        }
        self.text.alpha = 1.0;
    }
}
