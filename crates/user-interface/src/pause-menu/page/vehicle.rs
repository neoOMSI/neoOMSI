//! Vehicle related things

use super::*;

pub(super) const PAGE: Page = Page {
    nav: "pause.page.vehicle.nav",
    draw: Ui::draw_vehicle_page,
};

fn group_title(g: &VehicleGroup) -> String {
    t(&format!("pause.page.vehicle.group.{}.title", g.id))
}

fn action_key(a: &VehicleAction, field: &str) -> String {
    format!("pause.page.vehicle.action.{}.{field}", a.id)
}

impl Ui {
    pub(super) fn draw_vehicle_page(
        &mut self,
        r: &Renderer,
        scene: &mut Scene,
        f: &Frame,
        m: Metrics,
        top: f32,
        pt: f32,
    ) {
        let Metrics { w, h, u, mx, line } = m;
        let top = self.draw_page_head(
            r,
            scene,
            m,
            "pause.page.vehicle.head",
            "pause.page.vehicle.note",
            top,
            pt,
        );
        self.lab_groups.clear();
        self.lab_actions.clear();
        let groups = f.vehicle_menu;
        if groups.is_empty() {
            let l = self.text.label(
                r,
                scene,
                &t("pause.page.vehicle.empty"),
                (16.0 * u) as u32,
                MUTED,
            );
            l.place(scene, mx, top + 8.0 * u);
            return;
        }
        let gi = self.lab_group.min(groups.len() - 1);
        self.lab_group = gi;
        let foot_h = 34.0 * u;
        let bottom = h - foot_h;
        let gap = 28.0 * u;
        let inner_w = w - mx * 2.0;

        // left: the groups
        let side_w = (inner_w * 0.2).floor();
        let btn_h = 48.0 * u;
        let bpx = (16.0 * u) as u32;
        for (i, g) in groups.iter().enumerate() {
            let e = out((pt - 0.06 * i as f32 - 0.05) / 0.5);
            let y = top + (btn_h + 6.0 * u) * i as f32;
            if y + btn_h > bottom {
                break;
            }
            let rc = [
                mx - 20.0 * u * (1.0 - e),
                y,
                mx + side_w - 20.0 * u * (1.0 - e),
                y + btn_h,
            ];
            let hot = inside(rc, f.cursor) && self.dialog.is_none();
            let hv = self.ease((203, "grp", i), if hot { 1.0 } else { 0.0 }, 8.0);
            let sv = self.easeq((203, "grpsel", i), if i == gi { 1.0 } else { 0.0 }, 7.0);
            let bg = mix(
                mix(OPT_GROUP, OPT_GROUP_HOT, hv),
                OPT_GROUP_ON,
                sv,
            );
            self.text.rounded(r, scene, rc, 0.0, fade(bg, e));
            if sv > 0.0 {
                self.text.rounded(
                    r,
                    scene,
                    [rc[0], rc[1], rc[0] + 4.0 * u, rc[3]],
                    0.0,
                    fade(ACCENT, e * sv),
                );
            }
            let name = clip_to(&self.text, &group_title(g), bpx as f32, side_w - 32.0 * u);
            let l = self.text.label(
                r,
                scene,
                &name,
                bpx,
                mix(if hot { WHITE } else { SOFT }, WHITE, sv),
            );
            l.place(scene, rc[0] + 16.0 * u, rc[1] + (btn_h - l.h as f32) * 0.5);
            self.lab_groups.push([mx, y, mx + side_w, y + btn_h]);
        }

        // middle: the rows
        let mid_x = mx + side_w + gap;
        let mid_w = ((inner_w - side_w - gap * 2.0) * 0.56).floor();
        let g = &groups[gi];
        let ck = VEHICLE_PAGE * 10000 + gi * 100;
        if self.cat_key != ck {
            self.cat_key = ck;
            self.cat_t = 0.0;
        }
        self.cat_t = (self.cat_t + self.anim_dt / 0.5).min(1.0);
        let ct = pt.min(self.cat_t);
        let hpx = (22.0 * u) as u32;
        self.text.alpha = out(self.cat_t.max(0.35));
        let hl = self.text.label(r, scene, &group_title(g), hpx, WHITE);
        self.text.alpha = 1.0;
        hl.place(scene, mid_x, top + 10.0 * u * (1.0 - out(self.cat_t)));
        let y0 = top + hl.h as f32 + 12.0 * u;
        let row_h = 52.0 * u;
        let row_gap = 4.0 * u;
        let fit = (((bottom - y0) / (row_h + row_gap)).floor().max(0.0)) as usize;
        let mut hovered: Option<usize> = None;
        for (i, a) in g.actions.iter().enumerate().take(fit) {
            let e = out((ct - 0.04 * i.min(8) as f32 - 0.1) / 0.5);
            self.text.alpha = e;
            let ry = y0 + (row_h + row_gap) * i as f32;
            let ox = 24.0 * u * (1.0 - e);
            let rc = [mid_x, ry, mid_x + mid_w, ry + row_h];
            let hot = inside(rc, f.cursor) && self.dialog.is_none();
            if hot {
                hovered = Some(i);
            }
            let hv = self.ease((204, "row", i), if hot { 1.0 } else { 0.0 }, 8.0);
            let rr = [rc[0] + ox, rc[1], rc[2] + ox, rc[3]];
            self.text
                .rounded(r, scene, rr, 0.0, fade(mix(OPT_ROW, OPT_ROW_HOT, hv), e));
            let val = if a.opens {
                t("pause.page.vehicle.choose")
            } else {
                t(&action_key(a, "button"))
            };
            // the action: a small chip
            let cpx = (14.0 * u) as u32;
            let vl = self.text.label(r, scene, &val, cpx, plain(mix(SOFT, WHITE, hv)));
            let (chip_px, chip_py) = (10.0 * u, 4.0 * u);
            let vw = vl.w as f32 + chip_px * 2.0;
            let ch = vl.h as f32 + chip_py * 2.0;
            let cx = rr[2] - 18.0 * u - vw;
            let cy = rr[1] + (row_h - ch) * 0.5;
            self.text.rounded(
                r,
                scene,
                [cx, cy, cx + vw, cy + ch],
                4.0 * u,
                fade(mix(CHIP, ACCENT_SOFT, hv), e),
            );
            vl.place(scene, cx + chip_px, cy + chip_py);
            let npx = (18.0 * u) as u32;
            let name = clip_to(
                &self.text,
                &t(&action_key(a, "name")),
                npx as f32,
                mid_w - vw - 56.0 * u,
            );
            let nl = self
                .text
                .label(r, scene, &name, npx, if hot { WHITE } else { SOFT });
            nl.place(scene, rr[0] + 18.0 * u, rr[1] + (row_h - nl.h as f32) * 0.5);
            self.lab_actions.push(rc);
        }
        self.text.alpha = 1.0;

        // right: what the row under the mouse does (nothing there, nothing shown)
        let a = hovered.and_then(|i| g.actions.get(i));
        let iv = self.ease((206, "info", 0), if a.is_some() { 1.0 } else { 0.0 }, 9.0);
        let px0 = mid_x + mid_w + gap;
        let Some(a) = a.filter(|_| w - mx - px0 > 120.0 * u) else {
            return;
        };
        let e = out(iv);
        let inner = w - mx - px0 - 44.0 * u;
        let tpx = (20.0 * u) as u32;
        let bpx = 15.0 * u;
        let title = t(&action_key(a, "name"));
        let lines = wrap(&self.text, &t(&action_key(a, "desc")), bpx, inner);
        let lh = bpx * 1.3 + 3.0 * u;
        let ph = (22.0 * u + tpx as f32 * 1.3 + 14.0 * u + lines.len() as f32 * lh + 22.0 * u)
            .min((bottom - top).max(0.0));
        let x0 = px0 + 14.0 * u * (1.0 - e);
        let card = [x0, top, x0 + (w - mx - px0), top + ph];
        self.text.rounded(r, scene, card, 0.0, fade(OPT_CARD, e));
        self.text
            .rounded(r, scene, [card[0], card[1], card[0] + 3.0 * u, card[3]], 0.0, fade(ACCENT, e));
        let tl = self.text.label(
            r,
            scene,
            &clip_to(&self.text, &title, tpx as f32, inner),
            tpx,
            WHITE,
        );
        tl.place(scene, card[0] + 24.0 * u, card[1] + 22.0 * u);
        let mut ty = card[1] + 22.0 * u + tl.h as f32 + 14.0 * u;
        for ln in lines {
            if ty + bpx * 1.3 > card[3] - 12.0 * u {
                break;
            }
            let l = self.text.label(r, scene, &ln, bpx as u32, SOFT);
            l.place(scene, card[0] + 24.0 * u, ty);
            ty += l.h as f32 + 3.0 * u;
        }
        let _ = line;
    }
}
