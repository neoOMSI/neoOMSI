//! Common things for helping

use super::*;

impl Ui {
    pub(super) fn draw_frame(
        &mut self,
        r: &Renderer,
        scene: &mut Scene,
        rc: [f32; 4],
        b: f32,
        c: [u8; 4],
    ) {
        self.text
            .rounded(r, scene, [rc[0], rc[1], rc[2], rc[1] + b], 0.0, c);
        self.text
            .rounded(r, scene, [rc[0], rc[3] - b, rc[2], rc[3]], 0.0, c);
        self.text
            .rounded(r, scene, [rc[0], rc[1], rc[0] + b, rc[3]], 0.0, c);
        self.text
            .rounded(r, scene, [rc[2] - b, rc[1], rc[2], rc[3]], 0.0, c);
    }

    pub(super) fn draw_group_page(
        &mut self,
        r: &Renderer,
        scene: &mut Scene,
        f: &Frame,
        m: Metrics,
        top: f32,
        pt: f32,
        page: usize,
        head: &str,
        note: &str,
    ) {
        let Metrics { w, h, u, mx, line } = m;
        let top = self.draw_page_head(r, scene, m, head, note, top, pt);
        if self.world_last_page != page {
            self.world_last_page = page;
            self.world_group = 0;
            self.world_sub = 0;
            self.world_scroll = 0;
            self.world_pos = 0.0;
            self.world_drop = None;
            self.world_drag = None;
            self.world_bar_grab = None;
        }
        self.world_bar = None;
        self.world_groups_rc.clear();
        self.world_sub_rc.clear();
        self.world_rows_rc.clear();
        self.world_tracks.clear();
        self.world_clear.clear();
        self.world_drop_rc.clear();

        let groups = if self.world_view_page == page {
            self.world_view.clone()
        } else {
            Default::default()
        };
        if groups.is_empty() {
            return;
        }
        let gi = self.world_group.min(groups.len() - 1);
        self.world_group = gi;
        let foot_h = 34.0 * u;
        let bottom = h - foot_h;
        let gap = 28.0 * u;
        let inner_w = w - mx * 2.0;
        let busy = self.dialog.is_some() || self.world_drop.is_some();

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
            let hot = inside(rc, f.cursor) && !busy;
            let hv = self.ease((213, "grp", i), if hot { 1.0 } else { 0.0 }, 8.0);
            let sv = self.easeq((213, "grpsel", i), if i == gi { 1.0 } else { 0.0 }, 7.0);
            let bg = mix(
                mix([30, 32, 37, 255], [42, 45, 52, 255], hv),
                [58, 62, 70, 255],
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
            let name = clip_to(
                &self.text,
                &page_text(page, &g.title),
                bpx as f32,
                side_w - 32.0 * u,
            );
            let l = self.text.label(
                r,
                scene,
                &name,
                bpx,
                mix(if hot { WHITE } else { SOFT }, WHITE, sv),
            );
            l.place(scene, rc[0] + 16.0 * u, rc[1] + (btn_h - l.h as f32) * 0.5);
            self.world_groups_rc.push([mx, y, mx + side_w, y + btn_h]);
        }

        // middle: the title of the group and its rows, as many as fit (the wheel scrolls)
        let mid_x = mx + side_w + gap;
        let mid_w = ((inner_w - side_w - gap * 2.0) * 0.56).floor();
        let g = &groups[gi];
        // (the group's own tab, and a tab for each of its sub categories beside it)
        let si = self.world_sub.min(g.subs.len());
        self.world_sub = si;
        let rows = if si == 0 {
            &g.rows
        } else {
            &g.subs[si - 1].rows
        };
        // (a new group or sub tab: the rows come in again, one after the other)
        let ck = page * 10000 + gi * 100 + si;
        if self.cat_key != ck {
            self.cat_key = ck;
            self.cat_t = 0.0;
        }
        self.cat_t = (self.cat_t + self.anim_dt / 0.5).min(1.0);
        let ct = pt.min(self.cat_t);
        let tab_px = (22.0 * u) as u32;
        let tabbed = !g.subs.is_empty();
        let mut head_h = 0.0_f32;
        let first = if g.tab.is_empty() {
            g.title.as_str()
        } else {
            g.tab.as_str()
        };
        let names: Vec<String> = std::iter::once(first)
            .chain(g.subs.iter().map(|x| x.title.as_str()))
            .map(|t| page_text(page, t))
            .collect();
        let widths: Vec<f32> = names
            .iter()
            .map(|n| self.text.width(n, tab_px as f32))
            .collect();
        let tgap = 28.0 * u;
        let aw = 26.0 * u;
        let th = tab_px as f32 * 1.3;
        let span = |a: usize, b: usize| widths[a..=b].iter().sum::<f32>() + tgap * (b - a) as f32;
        let overflow = tabbed && span(0, names.len() - 1) > mid_w;
        // (too many tabs: the ones around the shown tab, and arrows to skip through them)
        let (lo, hi) = if overflow {
            let avail = mid_w - aw * 2.0 - 12.0 * u;
            let (mut lo, mut hi) = (si, si);
            loop {
                if hi + 1 < names.len() && span(lo, hi + 1) <= avail {
                    hi += 1;
                } else if lo > 0 && span(lo - 1, hi) <= avail {
                    lo -= 1;
                } else {
                    break;
                }
            }
            (lo, hi)
        } else {
            (0, names.len() - 1)
        };
        let mut tx = mid_x + if overflow { aw } else { 0.0 };
        for (i, name) in names.iter().enumerate() {
            if i < lo || i > hi {
                // (no tab here: a rectangle nothing hits, so the numbers of the tabs stay)
                if tabbed {
                    self.world_sub_rc.push([0.0; 4]);
                }
                continue;
            }
            let tw = widths[i];
            let rc = [tx - 8.0 * u, top, tx + tw + 8.0 * u, top + th + 10.0 * u];
            let hot = tabbed && inside(rc, f.cursor) && !busy;
            let sv = if tabbed {
                self.easeq((218, "sub", i), if i == si { 1.0 } else { 0.0 }, 7.0)
            } else {
                1.0
            };
            let col = mix(if hot { WHITE } else { MUTED }, WHITE, sv);
            self.text.alpha = out(self.cat_t.max(0.35));
            let hl = self.text.label(r, scene, name, tab_px, col);
            self.text.alpha = 1.0;
            hl.place(scene, tx, top + 10.0 * u * (1.0 - out(self.cat_t)));
            head_h = head_h.max(hl.h as f32);
            if tabbed {
                let uy = top + hl.h as f32 + 4.0 * u;
                self.text
                    .rounded(r, scene, [tx, uy, tx + tw * sv, uy + 3.0 * u], 0.0, ACCENT);
                self.world_sub_rc.push(rc);
                head_h = head_h.max(hl.h as f32 + 7.0 * u);
            }
            tx += tw + tgap;
        }
        if tabbed {
            // (the two arrows come after the tabs: their numbers are the tab count and the one after it)
            let last = names.len() - 1;
            for (k, (sign, x0)) in [("<", mid_x), (">", mid_x + mid_w - aw)]
                .into_iter()
                .enumerate()
            {
                if !overflow {
                    self.world_sub_rc.push([0.0; 4]);
                    continue;
                }
                let can = if k == 0 { si > 0 } else { si < last };
                let rc = [x0, top, x0 + aw, top + th + 10.0 * u];
                let hot = can && inside(rc, f.cursor) && !busy;
                let col = if !can {
                    [70, 73, 80, 255]
                } else if hot {
                    WHITE
                } else {
                    MUTED
                };
                let hl = self.text.label(r, scene, sign, tab_px, col);
                hl.place(scene, x0 + (aw - hl.w as f32) * 0.5, top);
                self.world_sub_rc.push(rc);
            }
        }
        let y0 = top + head_h + 12.0 * u;
        let row_h = 52.0 * u;
        let row_gap = 4.0 * u;
        let fit = (((bottom - y0) / (row_h + row_gap)).floor().max(1.0)) as usize;
        let max = rows.len().saturating_sub(fit);
        let target = self.world_scroll.min(max);
        self.world_scroll = target;
        self.world_max = max;
        // smooth scrolling
        let mut pos = self.world_pos.clamp(0.0, max as f32);
        if (target as f32 - pos).abs() < 0.01 {
            pos = target as f32;
        } else {
            pos += (target as f32 - pos) * (1.0 - (-18.0 * self.anim_dt).exp());
        }
        self.world_pos = pos;
        let first = (pos.floor() as usize).min(max);
        let frac = pos - first as f32;
        self.world_first = first;
        let step = row_h + row_gap;
        let view_end = y0 + step * fit as f32 - row_gap;
        let mut hovered: Option<usize> = None;
        for (n, row) in rows.iter().enumerate().skip(first).take(fit + 1) {
            let i = n - first;
            self.text.alpha = 1.0;
            let ry = y0 + step * i as f32 - step * frac;
            // rows leaving at the top or the bottom fade out
            let edge = ((ry + row_h - y0) / row_h)
                .min((view_end - ry) / row_h)
                .clamp(0.0, 1.0);
            if edge <= 0.03 {
                self.world_rows_rc.push([0.0; 4]);
                self.world_tracks.push(None);
                continue;
            }
            let e = out((ct - 0.04 * i.min(8) as f32 - 0.1) / 0.5) * edge;
            let rc = [mid_x, ry, mid_x + mid_w, ry + row_h];
            if row.kind == 'h' {
                // a separator: its title and a thin line, no entry
                let npx = (13.0 * u) as u32;
                let title = clip_to(
                    &self.text,
                    &page_text(page, &row.name).to_uppercase(),
                    npx as f32,
                    mid_w * 0.7,
                );
                let hl = self.text.label(r, scene, &title, npx, fade(MUTED, e));
                let ly = ry + row_h - 8.0 * u;
                hl.place(
                    scene,
                    mid_x + 24.0 * u * (1.0 - e),
                    ly - hl.h as f32 - 6.0 * u,
                );
                if !row.value.is_empty() {
                    let vw = self.text.width(&row.value, npx as f32);
                    let vl = self.text.label(r, scene, &row.value, npx, fade(MUTED, e));
                    vl.place(scene, mid_x + mid_w - vw, ly - vl.h as f32 - 6.0 * u);
                }
                self.text.rounded(
                    r,
                    scene,
                    [mid_x, ly, mid_x + mid_w, ly + line.max(1.0)],
                    0.0,
                    fade([58, 61, 67, 255], e),
                );
                self.world_rows_rc.push(rc);
                self.world_tracks.push(None);
                continue;
            }
            if row.kind == 'f' || row.kind == 'F' {
                // an input field (the search): its text, or what it is for while empty
                let active = row.kind == 'F';
                let hot = inside(rc, f.cursor) && !busy;
                if hot {
                    hovered = Some(n);
                }
                let hv = self.ease(
                    (219, "field", n),
                    if hot || active { 1.0 } else { 0.0 },
                    8.0,
                );
                self.text.rounded(
                    r,
                    scene,
                    rc,
                    0.0,
                    fade(mix([20, 22, 27, 255], [28, 30, 36, 255], hv), e),
                );
                let b = (1.5 * u).max(line);
                let c = fade(if active { ACCENT } else { [58, 61, 67, 255] }, e);
                self.text
                    .rounded(r, scene, [rc[0], rc[3] - b, rc[2], rc[3]], 0.0, c);
                let px = (16.0 * u) as u32;
                let shown = if row.value.is_empty() {
                    page_text(page, &row.name)
                } else if active {
                    format!("{}|", row.value)
                } else {
                    row.value.clone()
                };
                let shown = if row.value.is_empty() && active {
                    "|".to_string()
                } else {
                    shown
                };
                let col = if row.value.is_empty() && !active {
                    MUTED
                } else {
                    WHITE
                };
                let txt = clip_to(&self.text, &shown, px as f32, mid_w - 36.0 * u);
                let l = self.text.label(r, scene, &txt, px, fade(col, e));
                l.place(scene, rc[0] + 18.0 * u, ry + (row_h - l.h as f32) * 0.5);
                self.world_rows_rc.push(rc);
                self.world_tracks.push(None);
                continue;
            }
            self.text.alpha = e;
            let live = row.kind != 'i';
            let hot = inside(rc, f.cursor) && !busy;
            if hot {
                hovered = Some(n);
            }
            let hv = self.ease((214, "row", n), if hot && live { 1.0 } else { 0.0 }, 8.0);
            let rr = [
                rc[0] + 24.0 * u * (1.0 - e),
                rc[1],
                rc[2] + 24.0 * u * (1.0 - e),
                rc[3],
            ];
            let lit = self.ease((216, "lit", n), if row.lit { 1.0 } else { 0.0 }, 20.0);
            self.text.rounded(
                r,
                scene,
                rr,
                0.0,
                fade(
                    mix(mix([34, 36, 40, 255], [58, 61, 67, 255], hv), ACCENT, lit * 0.55),
                    e,
                ),
            );
            if lit > 0.0 {
                self.draw_frame(r, scene, rr, (2.0 * u).max(line), fade(ACCENT_HOT, e * lit));
            }
            if hv > 0.0 && !matches!(row.kind, 'a' | 'e' | 'E' | 'c') {
                // (the row under the mouse gets a light frame, buttons don't)
                self.draw_frame(r, scene, rr, (1.5 * u).max(line), fade(WHITE, e * hv));
            }
            let pad = 18.0 * u;
            let rx = rr[2] - pad;
            let vpx = (15.0 * u) as u32;
            let value = page_value(page, row.kind, &row.value);
            let mut ctl_w = 0.0;
            let mut track = None;
            match row.kind {
                's' => {
                    // ON / OFF and a switch
                    let on = row.value == "on";
                    let k = self.ease((215, "sw", n), if on { 1.0 } else { 0.0 }, 10.0);
                    let (pw, ph) = (48.0 * u, 24.0 * u);
                    let (x0, y0) = (rx - pw, ry + (row_h - ph) * 0.5);
                    self.text.rounded(
                        r,
                        scene,
                        [x0, y0, x0 + pw, y0 + ph],
                        0.0,
                        fade([62, 65, 72, 255], e),
                    );
                    let kw = 20.0 * u;
                    let kx = x0 + 2.0 * u + (pw - kw - 4.0 * u) * k;
                    self.text.rounded(
                        r,
                        scene,
                        [kx, y0 + 2.0 * u, kx + kw, y0 + ph - 2.0 * u],
                        0.0,
                        fade(mix([150, 153, 160, 255], ACCENT, k), e),
                    );
                    let word = t(if on {
                        "pause.page.world.on"
                    } else {
                        "pause.page.world.off"
                    });
                    let wl = self.text.label(r, scene, &word, vpx, mix(SOFT, WHITE, hv));
                    wl.place(
                        scene,
                        x0 - 14.0 * u - wl.w as f32,
                        ry + (row_h - wl.h as f32) * 0.5,
                    );
                    ctl_w = pw + 14.0 * u + wl.w as f32;
                }
                'v' => {
                    let vw_max = (180.0 * u).min(mid_w * 0.34);
                    let val = clip_to(&self.text, &value.to_uppercase(), vpx as f32, vw_max);
                    let vl = self.text.label(r, scene, &val, vpx, mix(SOFT, WHITE, hv));
                    vl.place(scene, rx - vl.w as f32, ry + (row_h - vl.h as f32) * 0.5);
                    let t1 = rx - vw_max - 12.0 * u;
                    let t0 = t1 - 150.0 * u;
                    let cy = ry + row_h * 0.5;
                    self.text.rounded(
                        r,
                        scene,
                        [t0, cy - 2.0 * u, t1, cy + 2.0 * u],
                        0.0,
                        fade([24, 26, 31, 255], e),
                    );
                    let fx = t0 + (t1 - t0) * row.frac.clamp(0.0, 1.0);
                    self.text.rounded(
                        r,
                        scene,
                        [t0, cy - 2.0 * u, fx, cy + 2.0 * u],
                        0.0,
                        fade(mix(ACCENT, ACCENT_HOT, hv), e),
                    );
                    let kd = 14.0 * u;
                    self.text.rounded(
                        r,
                        scene,
                        [fx - kd * 0.4, cy - kd * 0.5, fx + kd * 0.4, cy + kd * 0.5],
                        0.0,
                        fade(WHITE, e),
                    );
                    track = Some([
                        t0 - 6.0 * u,
                        ry + 6.0 * u,
                        t1 + 6.0 * u,
                        ry + row_h - 6.0 * u,
                    ]);
                    ctl_w = vw_max + 12.0 * u + 150.0 * u;
                }
                'o' => {
                    // the value and a small arrow: a drop-down
                    let bw = 12.0 * u;
                    let bx = rx - bw;
                    let open = self.world_drop.as_ref().is_some_and(|d| d.k == n);
                    let bc = fade(
                        mix(
                            [120, 123, 130, 255],
                            [236, 236, 236, 255],
                            (hv).max(if open { 1.0 } else { 0.0 }),
                        ),
                        e,
                    );
                    // (a little triangle, pointing down)
                    let (cx, cy) = (bx + bw * 0.5, ry + row_h * 0.5);
                    for s in 0..3 {
                        let half = (4.5 - 1.5 * s as f32) * u;
                        let yy = cy - 3.0 * u + 2.0 * u * s as f32;
                        self.text.rounded(
                            r,
                            scene,
                            [cx - half, yy, cx + half, yy + 2.0 * u],
                            0.0,
                            bc,
                        );
                    }
                    let txt = clip_to(&self.text, &value, vpx as f32, mid_w * 0.4);
                    let vw = self.text.width(&txt, vpx as f32);
                    let vl = self.text.label(r, scene, &txt, vpx, mix(SOFT, WHITE, hv));
                    vl.place(scene, bx - 10.0 * u - vw, ry + (row_h - vl.h as f32) * 0.5);
                    ctl_w = bw + 10.0 * u + vw;
                    if let Some(m) = row.meter {
                        let (mw, mh) = (110.0 * u, 6.0 * u);
                        let x1 = bx - 10.0 * u - vw - 16.0 * u;
                        let x0 = x1 - mw;
                        let cy = ry + row_h * 0.5;
                        let one = row.meter_one_sided;
                        let mid = if one { x0 } else { (x0 + x1) * 0.5 };
                        let px = if one {
                            x0 + (x1 - x0) * m.clamp(0.0, 1.0)
                        } else {
                            x0 + (x1 - x0) * (m.clamp(-1.0, 1.0) + 1.0) * 0.5
                        };
                        self.text.rounded(
                            r,
                            scene,
                            [x0, cy - mh * 0.5, x1, cy + mh * 0.5],
                            0.0,
                            fade([24, 26, 31, 255], e),
                        );
                        self.text.rounded(
                            r,
                            scene,
                            [mid.min(px), cy - mh * 0.5, mid.max(px), cy + mh * 0.5],
                            0.0,
                            fade(mix(ACCENT, ACCENT_HOT, hv), e),
                        );
                        let kd = 12.0 * u;
                        self.text.rounded(
                            r,
                            scene,
                            [px - kd * 0.3, cy - kd * 0.5, px + kd * 0.3, cy + kd * 0.5],
                            0.0,
                            fade(WHITE, e),
                        );
                        ctl_w += 16.0 * u + mw;
                    }
                }
                kind => {
                    let col = match kind {
                        'a' | 'e' => mix(ACCENT, WHITE, hv),
                        'E' => WHITE,
                        'c' => [235, 150, 60, 255],
                        _ => MUTED,
                    };
                    // a key binding that is set: a small x behind it takes the key away
                    let mut vr = rx;
                    if kind == 'k' && live && row.value != t("pause.page.keys.not_set") {
                        let xw = 22.0 * u;
                        let xr = [rx - xw, ry, rx + pad * 0.5, ry + row_h];
                        let hx = inside(xr, f.cursor) && !busy;
                        let xv = self.ease((215, "keyx", n), if hx { 1.0 } else { 0.0 }, 10.0);
                        let xl = self.text.label(
                            r,
                            scene,
                            "\u{d7}",
                            (18.0 * u) as u32,
                            plain(mix([110, 113, 120, 255], [235, 90, 80, 255], xv)),
                        );
                        xl.place(
                            scene,
                            rx - xw * 0.5 - xl.w as f32 * 0.5,
                            ry + (row_h - xl.h as f32) * 0.5,
                        );
                        self.world_clear.push((n, xr));
                        vr = rx - xw - 6.0 * u;
                        ctl_w = xw + 6.0 * u;
                    }
                    let txt = clip_to(&self.text, &value.to_uppercase(), vpx as f32, mid_w * 0.4);
                    if !txt.is_empty() {
                        let vw = self.text.width(&txt, vpx as f32);
                        let vl = self.text.label(r, scene, &txt, vpx, plain(col));
                        vl.place(scene, vr - vw, ry + (row_h - vl.h as f32) * 0.5);
                        ctl_w += vw;
                    }
                }
            }
            let npx = (17.0 * u) as u32;
            let text_w = mid_w - pad * 2.0 - ctl_w - 16.0 * u;
            let name = clip_to(&self.text, &page_text(page, &row.name), npx as f32, text_w);
            let nl = self
                .text
                .label(r, scene, &name, npx, if hot && live { WHITE } else { SOFT });
            nl.place(scene, rr[0] + pad, ry + (row_h - nl.h as f32) * 0.5);
            if !row.tag.is_empty() {
                // (a small text behind the name: the action's own name)
                let tpx = (12.0 * u) as u32;
                let tx = rr[0] + pad + nl.w as f32 + 10.0 * u;
                let room = text_w - nl.w as f32 - 10.0 * u;
                if room > 40.0 * u {
                    let tag = clip_to(&self.text, &row.tag, tpx as f32, room);
                    let tl = self
                        .text
                        .label(r, scene, &tag, tpx, fade([110, 113, 120, 255], e));
                    tl.place(scene, tx, ry + (row_h - tl.h as f32) * 0.5);
                }
            }
            self.world_rows_rc.push(rc);
            self.world_tracks.push(track);
        }

        self.text.alpha = 1.0;

        // the scroll bar, when there are more rows than fit
        if max > 0 {
            let track_h = (row_h + row_gap) * fit as f32 - row_gap;
            let sx = mid_x + mid_w + 10.0 * u;
            let thumb_h = (track_h * fit as f32 / rows.len() as f32)
                .max(28.0 * u)
                .min(track_h);
            let ty = y0 + (track_h - thumb_h) * pos / max as f32;
            let hit_track = [sx - 8.0 * u, y0, sx + 14.0 * u, y0 + track_h];
            let hit_thumb = [hit_track[0], ty, hit_track[2], ty + thumb_h];
            self.world_bar = Some((hit_track, hit_thumb));
            let grabbed = self.world_bar_grab.is_some();
            let hot = grabbed || (inside(hit_thumb, f.cursor) && !busy);
            let bw = self.ease((218, "bar", 0), if hot { 7.0 } else { 4.0 }, 40.0) * u;
            self.text.rounded(
                r,
                scene,
                [sx, y0, sx + bw, y0 + track_h],
                0.0,
                [34, 36, 42, 255],
            );
            self.text.rounded(
                r,
                scene,
                [sx, ty, sx + bw, ty + thumb_h],
                0.0,
                if hot { ACCENT_HOT } else { ACCENT },
            );
        }

        // the open drop-down: under its row (over it when the screen ends below)
        if let Some(mut d) = self.world_drop.clone() {
            if let Some(i) = d.k.checked_sub(first).filter(|i| *i < fit) {
                let rc = self.world_rows_rc[i];
                let item_h = 36.0 * u;
                let vis = d.labels.len().clamp(1, 8);
                let search_h = if d.hay.is_empty() { 0.0 } else { 44.0 * u };
                self.world_drop_vis = vis;
                d.top = d.top.min(d.labels.len().saturating_sub(vis));
                let dw = (300.0 * u).max(mid_w * 0.5).min(mid_w);
                let dh = item_h * vis as f32 + 2.0 * line + search_h;
                let x1 = rc[2];
                let mut dy = rc[3] + 2.0 * u;
                if dy + dh > h - 8.0 * u {
                    dy = rc[1] - dh - 2.0 * u;
                }
                let box_ = [x1 - dw, dy, x1, dy + dh];
                self.text.rounded(
                    r,
                    scene,
                    [
                        box_[0] - line,
                        box_[1] - line,
                        box_[2] + line,
                        box_[3] + line,
                    ],
                    0.0,
                    [150, 153, 160, 255],
                );
                self.text.rounded(r, scene, box_, 0.0, [26, 28, 33, 255]);
                self.world_drop_first = d.top;
                if !d.hay.is_empty() {
                    let rc = [
                        box_[0] + 4.0 * u,
                        box_[1] + 4.0 * u,
                        box_[2] - 4.0 * u,
                        box_[1] + search_h - 4.0 * u,
                    ];
                    self.text.rounded(r, scene, rc, 0.0, [8, 10, 14, 255]);
                    let bpx = 15.0 * u;
                    let q = &d.filter;
                    let (txt, col) = if q.is_empty() {
                        (t("pause.dialog.search"), SOFT)
                    } else {
                        (
                            clip_left(&self.text, q, bpx, rc[2] - rc[0] - 28.0 * u),
                            WHITE,
                        )
                    };
                    let l = self.text.label(r, scene, &txt, bpx as u32, col);
                    l.place(
                        scene,
                        rc[0] + 10.0 * u,
                        rc[1] + (rc[3] - rc[1] - l.h as f32) * 0.5,
                    );
                    if (self.text.frame / 30) % 2 == 0 {
                        let cx = rc[0]
                            + 10.0 * u
                            + if q.is_empty() { 0.0 } else { l.w as f32 }
                            + 2.0 * u;
                        self.text.rounded(
                            r,
                            scene,
                            [cx, rc[1] + 6.0 * u, cx + 2.0 * u, rc[3] - 6.0 * u],
                            0.0,
                            WHITE,
                        );
                    }
                }
                for (j, label) in d.labels.iter().enumerate().skip(d.top).take(vis) {
                    let iy = box_[1] + line + search_h + item_h * (j - d.top) as f32;
                    let irc = [box_[0] + line, iy, box_[2] - line, iy + item_h];
                    let hot = inside(irc, f.cursor);
                    let chosen = j == d.sel;
                    let hv = self.ease((216, "dd", j), if hot || chosen { 1.0 } else { 0.0 }, 10.0);
                    if hv > 0.0 {
                        self.text
                            .rounded(r, scene, irc, 0.0, fade([58, 61, 67, 255], hv));
                    }
                    if d.current == Some(j) {
                        self.text.rounded(
                            r,
                            scene,
                            [irc[0], irc[1], irc[0] + 4.0 * u, irc[3]],
                            0.0,
                            ACCENT,
                        );
                    }
                    let px = (15.0 * u) as u32;
                    let shown = world_value('a', label);
                    let txt = clip_to(&self.text, &shown, px as f32, dw - 40.0 * u);
                    let l = self.text.label(r, scene, &txt, px, mix(SOFT, WHITE, hv));
                    l.place(scene, irc[0] + 16.0 * u, iy + (item_h - l.h as f32) * 0.5);
                    self.world_drop_rc.push(irc);
                }
                if let Some(wd) = self.world_drop.as_mut() {
                    wd.top = d.top;
                }
            }
        }

        // right: what the row under the mouse does (nothing there, nothing shown)
        // (the row being worked on keeps its description: the one with the open list, the one whose
        // slider is dragged)
        let focus = self
            .world_drop
            .as_ref()
            .map(|d| d.k)
            .or(self.world_drag.map(|(k, _)| k))
            .or(hovered);
        let a = focus
            .and_then(|i| rows.get(i))
            .filter(|rw| !rw.desc.is_empty());
        let iv = self.ease((217, "info", 0), if a.is_some() { 1.0 } else { 0.0 }, 9.0);
        let px0 = mid_x + mid_w + gap;
        let Some(a) = a.filter(|_| w - mx - px0 > 120.0 * u) else {
            return;
        };
        let e = out(iv);
        let inner = w - mx - px0 - 44.0 * u;
        let tpx = (20.0 * u) as u32;
        let bpx = 15.0 * u;
        let lines = wrap(&self.text, &page_text(page, &a.desc), bpx, inner);
        let lh = bpx * 1.3 + 3.0 * u;
        let ph = (22.0 * u + tpx as f32 * 1.3 + 14.0 * u + lines.len() as f32 * lh + 22.0 * u)
            .min((bottom - top).max(0.0));
        let x0 = px0 + 14.0 * u * (1.0 - e);
        let card = [x0, top, x0 + (w - mx - px0), top + ph];
        self.text
            .rounded(r, scene, card, 0.0, fade([20, 22, 27, 255], e));
        self.text.rounded(
            r,
            scene,
            [card[0], card[1], card[0] + 3.0 * u, card[3]],
            0.0,
            fade(ACCENT, e),
        );
        let tl = self.text.label(
            r,
            scene,
            &clip_to(&self.text, &page_text(page, &a.name), tpx as f32, inner),
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
    }
}

pub(super) fn plain(c: [u8; 4]) -> [u8; 4] {
    [c[0], c[1], c[2], 0]
}

fn world_key(text: &str) -> String {
    let mut s = String::new();
    for c in text.chars() {
        if c.is_ascii_alphanumeric() {
            s.push(c.to_ascii_lowercase());
        } else if !s.ends_with('_') {
            s.push('_');
        }
    }
    format!("pause.world.text.{}", s.trim_matches('_'))
}

fn world_text(text: &str) -> String {
    if text.is_empty() {
        return String::new();
    }
    let key = world_key(text);
    if ::i18n::lookup("en", &key).is_some() {
        t(&key)
    } else {
        text.to_string()
    }
}

fn page_text(page: usize, text: &str) -> String {
    if page != WORLD_PAGE {
        if text.is_empty() {
            String::new()
        } else {
            t(text)
        }
    } else {
        world_text(text)
    }
}

fn page_value(page: usize, kind: char, v: &str) -> String {
    if page != WORLD_PAGE && matches!(kind, 'a' | 'o') {
        t(v)
    } else {
        world_value(kind, v)
    }
}

fn world_value(kind: char, v: &str) -> String {
    let mut s = v.to_string();
    if matches!(kind, 'a' | 'o' | 'i') {
        let key = world_key(v);
        if ::i18n::lookup("en", &key).is_some() {
            s = t(&key);
        }
    }
    if let Some(n) = s.strip_suffix(" vehicles") {
        s = format!("{n} {}", t("pause.world.vehicles"));
    }
    s = s.replace(" · dew ", &format!(" · {} ", t("pause.world.dew")));
    s = s.replace("(automatic)", &format!("({})", t("pause.world.automatic")));
    s.replace("(typing)", &format!("({})", t("pause.world.typing")))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn plain_zeroes_alpha() {
        assert_eq!(plain([1, 2, 3, 255]), [1, 2, 3, 0]);
    }

    #[test]
    fn world_key_normalises() {
        assert_eq!(world_key("Hello World"), "pause.world.text.hello_world");
        assert_eq!(world_key("  A -- B!! "), "pause.world.text.a_b");
        assert_eq!(world_key("abc123"), "pause.world.text.abc123");
        assert_eq!(world_key(""), "pause.world.text.");
        assert_eq!(world_key("!!!"), "pause.world.text.");
    }

    #[test]
    fn world_text_empty_and_unknown() {
        assert_eq!(world_text(""), "");
        assert_eq!(world_text("zz no such text qq"), "zz no such text qq");
    }

    #[test]
    fn page_text_empty_stays_empty() {
        assert_eq!(page_text(0, ""), "");
        assert_eq!(page_text(WORLD_PAGE, ""), "");
    }

    #[test]
    fn page_text_non_world_translates_key() {
        assert_eq!(page_text(OPTIONS_PAGE, "no.such.key"), "no.such.key");
    }

    #[test]
    fn page_text_world_keeps_unknown_text() {
        assert_eq!(
            page_text(WORLD_PAGE, "zz no such text qq"),
            "zz no such text qq"
        );
    }

    #[test]
    fn world_value_passes_unknown_through() {
        assert_eq!(world_value('x', "plain value"), "plain value");
        assert_eq!(
            world_value('a', "zz no such value qq"),
            "zz no such value qq"
        );
    }

    #[test]
    fn world_value_replaces_suffixes() {
        assert!(world_value('x', "5 vehicles").starts_with("5 "));
        assert!(
            !world_value('x', "5 vehicles").ends_with(" vehicles")
                || t("pause.world.vehicles") == "vehicles"
        );
        assert!(world_value('x', "foo (typing)").starts_with("foo ("));
        assert!(world_value('x', "foo (automatic)").starts_with("foo ("));
    }

    #[test]
    fn page_value_non_world_translates_selects() {
        assert_eq!(page_value(OPTIONS_PAGE, 'a', "no.such.key"), "no.such.key");
        assert_eq!(page_value(OPTIONS_PAGE, 'x', "plain"), "plain");
        assert_eq!(page_value(WORLD_PAGE, 'x', "plain"), "plain");
    }
}
