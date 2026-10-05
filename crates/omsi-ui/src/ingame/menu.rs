//! The game menu and its lists (options, lines, tours).

use super::*;

impl Ui {
    /// The game menu and its lists (options, lines, tours ...): a card in the middle of a
    /// dimmed picture. Options are settings lines with switches and values; lines and tours
    /// are bigger lines with the timetable of the chosen one beside them.
    pub(super) fn draw_menu(&mut self, r: &Renderer, scene: &mut Scene, f: &Frame) {
        self.menu_rects.clear();
        self.menu_ctl.clear();
        self.menu_side.clear();
        self.menu_pane.clear();
        self.menu_pane_start = 0;
        self.menu_pane_go = None;
        self.menu_pane_box = None;
        self.menu_time.clear();
        self.menu_scroll_thumb = None;
        self.menu_scroll_track = None;
        self.dd_rects.clear();
        let overlay_start = scene.overlays.len();
        let Some((sel, items)) = f.menu else {
            self.menu_overlay_range = overlay_start..overlay_start;
            self.anim.clear();
            return;
        };
        if let Some(report) = f.report {
            self.draw_run_report(r, scene, f, report, sel);
            self.menu_overlay_range = overlay_start..scene.overlays.len();
            return;
        }
        if f.menu_kind == MenuKind::Options && f.menu_tabs.is_some() {
            self.draw_settings(r, scene, f, sel, items);
            self.menu_overlay_range = overlay_start..scene.overlays.len();
            return;
        }
        let s = f.scale.max(0.5);
        let kind = f.menu_kind;
        let dim = self.text.plate(r, scene, 6);
        let sep = self.text.plate(r, scene, 9);
        scene.overlays.push((dim, [0.0, 0.0, f.width, f.height]));
        let keys = !f.vr && !f.touch;
        let preview = f
            .menu_preview
            .as_ref()
            .filter(|_| !f.vr && f.width >= 760.0 * s);
        let timetable_kind = matches!(kind, MenuKind::Lines | MenuKind::Tours);
        let pane_w = if preview.is_some() {
            (if timetable_kind { 320.0 } else { 340.0 }) * s
        } else {
            0.0
        };
        let want = if timetable_kind { 360.0 * s } else { 380.0 * s };
        let w = (want + pane_w).min(f.width - 24.0 * s).max(200.0 * s);
        let list_w = w - pane_w;
        let header_h = 72.0 * s;
        let pad = PAD * s;
        let tin = TEXT_IN * s;
        let back_txt = crate::tr("Back").into_owned();
        let back_footer = (timetable_kind || kind == MenuKind::List)
            && items
                .last()
                .is_some_and(|&(id, l)| id == "back" && l == back_txt.as_str());
        let nl = items.len() - back_footer as usize;
        let foot_h = if back_footer { 48.0 * s } else { 0.0 };
        let fixed_h =
            matches!(kind, MenuKind::Lines | MenuKind::Tours | MenuKind::List).then(|| {
                if f.vr {
                    f.height * 0.60
                } else {
                    (520.0 * s).min(f.height * 0.94)
                }
            });
        let room = match fixed_h {
            Some(fh) => fh - header_h - pad - foot_h,
            None => f.height * (if f.vr { 0.60 } else { 0.92 }) - header_h - pad - 8.0 * s,
        };
        let base = match (f.vr, kind) {
            (true, _) => 40.0,
            (_, MenuKind::Lines) => 44.0,
            (_, MenuKind::Tours) => 40.0,
            _ => 42.0,
        } * s;
        let row_h = base.min(room / nl.max(1) as f32).max(34.0 * s);
        let rows = ((room / row_h).floor() as usize).clamp(1, nl.max(1));
        let sel_l = sel.min(nl.saturating_sub(1));
        let start = match (nl > rows, f.menu_top) {
            (false, _) => 0,
            (true, Some(top)) => (top.max(0.0).round() as usize).min(nl - rows),
            (true, None) => sel_l.saturating_sub(rows / 2).min(nl - rows),
        };
        self.menu_start = start;
        self.menu_rows = rows;
        self.menu_row_h = row_h;
        let px = (((if timetable_kind { 14.0 } else { 16.0 }) * s).min(row_h * 0.45)) as u32;
        let mut h = header_h + row_h * rows as f32 + pad + foot_h;
        if let Some(fh) = fixed_h {
            h = fh;
        } else if preview.is_some() {
            h = h.max((420.0 * s).min(f.height * 0.92));
        }
        let x = ((f.width - w) * 0.5).round();
        let y = ((f.height - h) * 0.5).round();
        let list_r = x + list_w;
        let radius = CARD_R * s;
        self.text.shadow(
            r,
            scene,
            [x, y, x + w, y + h],
            radius,
            28.0 * s,
            10.0 * s,
            110,
        );
        self.text.rounded(
            r,
            scene,
            [x - 1.0, y - 1.0, x + w + 1.0, y + h + 1.0],
            radius + 1.0,
            BORDER,
        );
        self.text
            .rounded(r, scene, [x, y, x + w, y + h], radius, PANEL);
        let (title, sub): (String, String) = match f.menu_head.as_ref() {
            Some((t, u)) => (t.clone(), u.clone()),
            None => {
                let t = if f.paused { "Paused" } else { "Menu" };
                (t.to_string(), String::new())
            }
        };
        if !(f.paused
            && f.menu_head.is_none()
            && self.menu_logo_header(r, scene, x, y, w, header_h, s))
        {
            self.menu_header(r, scene, x, y, w, header_h, &title, &sub, s);
        }
        let scrolls = nl > rows;
        if scrolls {
            let top = y + header_h;
            let track = [
                list_r - 11.0 * s,
                top,
                list_r - 8.0 * s,
                top + row_h * rows as f32 - 4.0 * s,
            ];
            self.menu_scroll_track = Some(track);
            self.text
                .rounded(r, scene, track, 1.5 * s, [255, 255, 255, 22]);
            let th = track[3] - track[1];
            let t0 = track[1] + th * start as f32 / nl as f32;
            let t1 = track[1] + th * (start + rows) as f32 / nl as f32;
            let thumb = [track[0], t0, track[2], t1];
            self.text.rounded(r, scene, thumb, 1.5 * s, ACCENT);
            self.menu_scroll_thumb =
                Some([thumb[0] - 6.0 * s, thumb[1], thumb[2] + 6.0 * s, thumb[3]]);
        }
        let over = |rect: [f32; 4]| {
            f.cursor.0 >= rect[0]
                && f.cursor.0 <= rect[2]
                && f.cursor.1 >= rect[1]
                && f.cursor.1 <= rect[3]
        };
        let any_hovered = over([x, y, list_r, y + h]);
        let right = list_r - if scrolls { 20.0 * s } else { pad };
        let line_pre = format!("{} ", crate::tr("Line"));
        let tour_pre = format!("{} ", crate::tr("Tour"));
        let mut sign_w = 48.0 * s;
        if kind == MenuKind::Lines {
            for &(_, label) in items.iter() {
                if let Some(n) = label
                    .strip_prefix(line_pre.as_str())
                    .and_then(|rest| rest.rsplit_once("  ("))
                    .map(|(n, _)| n)
                {
                    sign_w = sign_w.max(self.text.width(n, 15.0 * s) + 22.0 * s);
                }
            }
        }
        for (k, &(id, label)) in items[..nl].iter().enumerate().skip(start).take(rows) {
            let ry = y + header_h + row_h * (k - start) as f32;
            let gap = 4.0 * s;
            let rect = [x + pad, ry, right, ry + row_h - gap];
            let off = kind == MenuKind::Game && f.menu_disabled.contains(&id);
            let lit = !off && (over(rect) || (k == sel && f.menu_kbd && !any_hovered));
            let danger = id == "quit";
            let is_back = id == "back" && label == back_txt.as_str();
            let apart = k > start
                && match kind {
                    MenuKind::Game => matches!(id, "save" | "admin" | "quit"),
                    MenuKind::Lines => id == "free",
                    _ => false,
                };
            if apart {
                let sy = (ry - 2.0 * s).round();
                scene
                    .overlays
                    .push((sep, [x + pad + tin, sy, right - tin, sy + 1.0]));
            }
            let glow = self.easeq((7, id, k), if lit { 1.0 } else { 0.0 }, 1.0 / FADE_SECS);
            let active = kind == MenuKind::Tours && k == sel && !is_back;
            let a_act = self.easeq((14, id, k), if active { 1.0 } else { 0.0 }, 1.0 / FADE_SECS);
            let bar = self.easeq(
                (8, id, k),
                if lit || active { 1.0 } else { 0.0 },
                1.0 / BAR_SECS,
            );
            if a_act > 0.0 {
                self.text
                    .rounded(r, scene, rect, ROW_R * s, fade(SELECTED, a_act));
            }
            if glow > 0.0 {
                self.text.rounded(
                    r,
                    scene,
                    rect,
                    ROW_R * s,
                    fade(if danger { LIT_DANGER } else { LIT }, glow),
                );
            }
            if bar > 0.0 {
                self.accent_bar(r, scene, rect, bar, danger, s);
            }
            let ink = if off {
                OFF_INK
            } else {
                if danger {
                    mix([232, 138, 128, 0], [255, 176, 166, 0], glow)
                } else {
                    mix(SOFT, WHITE, glow.max(a_act))
                }
            };
            let cy = (rect[1] + rect[3]) * 0.5;
            let lx = rect[0] + tin;
            let rx = rect[2] - tin;
            let mut done = false;
            match kind {
                MenuKind::Lines => {
                    let parsed = label
                        .strip_prefix(line_pre.as_str())
                        .and_then(|rest| rest.rsplit_once("  ("))
                        .map(|(n, t)| (n, t.trim_end_matches(')')));
                    if let Some((name, info)) = parsed {
                        let bl = self
                            .text
                            .label(r, scene, name, (15.0 * s) as u32, ON_ACCENT);
                        let (bw, bh) = (sign_w, 28.0 * s);
                        let bx = lx;
                        let sign = mix(mix(ACCENT, [150, 104, 30, 255], 0.30), ACCENT_HOT, glow);
                        self.text.rounded(
                            r,
                            scene,
                            [bx, cy - bh * 0.5, bx + bw, cy + bh * 0.5],
                            7.0 * s,
                            sign,
                        );
                        let (lx0, ly0) = (bx + (bw - bl.w as f32) * 0.5, cy - bl.h as f32 * 0.5);
                        bl.place(scene, lx0, ly0);
                        let tx = bx + bw + 14.0 * s;
                        let d = 22.0 * s;
                        self.text.rounded(
                            r,
                            scene,
                            [rx - d, cy - d * 0.5, rx, cy + d * 0.5],
                            d * 0.5,
                            fade(ACCENT, 0.10 + 0.30 * glow),
                        );
                        let aw = self.text.width("›", (px + 2) as f32);
                        self.put(
                            r,
                            scene,
                            "›",
                            px + 2,
                            mix(MUTED, ACCENT_HOT, glow),
                            rx - d * 0.5 - aw * 0.5,
                            cy - 1.0 * s,
                        );
                        let info = clip_to(&self.text, info, px as f32, rx - d - 12.0 * s - tx);
                        self.put(r, scene, &info, px, ink, tx, cy);
                        done = true;
                    }
                }
                MenuKind::Tours => {
                    if let Some(rest) = label.strip_prefix(tour_pre.as_str()) {
                        let num = rest.split_once("  ").map(|(n, _)| n).unwrap_or(rest).trim();
                        let name =
                            clip_to(&self.text, &format!("{tour_pre}{num}"), px as f32, rx - lx);
                        self.put(r, scene, &name, px, ink, lx, cy);
                        done = true;
                    }
                }
                _ => {}
            }
            if !done {
                let (text, more) = strip_more(label);
                let mut avail = rx - lx;
                if is_back {
                    let plain = matches!(kind, MenuKind::Lines | MenuKind::Tours);
                    if plain {
                        self.text
                            .rounded(r, scene, rect, ROW_R * s, fade(LIT, 0.55));
                    }
                    let idle = if plain { SOFT } else { MUTED };
                    self.put(
                        r,
                        scene,
                        &format!("‹  {text}"),
                        px,
                        if lit { WHITE } else { idle },
                        lx,
                        cy,
                    );
                } else {
                    if off {
                        let cw = self.put_right(
                            r,
                            scene,
                            "No active route",
                            (12.0 * s) as u32,
                            OFF_HINT,
                            rx,
                            cy,
                        );
                        avail -= cw + 10.0 * s;
                    }
                    if more {
                        let cw = self.put_right(
                            r,
                            scene,
                            "›",
                            px + 6,
                            if lit { WHITE } else { MUTED },
                            rx,
                            cy,
                        );
                        avail -= cw + 10.0 * s;
                    } else if id == "resume" && keys {
                        let left = self.chip(
                            r,
                            scene,
                            "Esc",
                            (12.0 * s) as u32,
                            WHITE,
                            [52, 52, 52, 255],
                            true,
                            rx,
                            cy,
                            s,
                        );
                        avail = left - 10.0 * s - lx;
                    }
                    let text = clip_to(&self.text, text, px as f32, avail);
                    self.put(r, scene, &text, px, ink, lx, cy);
                }
            }
            self.menu_rects.push(rect);
        }
        if back_footer {
            let bk = items.len() - 1;
            let fr = [x + pad, y + h - pad - 36.0 * s, right, y + h - pad];
            let lit = over(fr) || (sel == bk && f.menu_kbd && !any_hovered);
            let glow = self.easeq(
                (7, "back", bk),
                if lit { 1.0 } else { 0.0 },
                1.0 / FADE_SECS,
            );
            self.text
                .rounded(r, scene, fr, ROW_R * s, fade(LIT, 0.55 + 0.45 * glow));
            if glow > 0.0 {
                self.accent_bar(r, scene, fr, glow, false, s);
            }
            let (text, _) = strip_more(back_txt.as_str());
            self.put(
                r,
                scene,
                &format!("‹  {text}"),
                px,
                mix(SOFT, WHITE, glow),
                fr[0] + tin,
                (fr[1] + fr[3]) * 0.5,
            );
            while self.menu_rects.len() < bk - start {
                self.menu_rects.push([-1.0e9; 4]);
            }
            self.menu_rects.push(fr);
        }
        if let Some(p) = preview {
            let (px0, py0) = (list_r + 4.0 * s, y + header_h);
            let (px1, py1) = (x + w - pad, y + h - pad);
            self.text.rounded(
                r,
                scene,
                [px0 - 1.0, py0 - 1.0, px1 + 1.0, py1 + 1.0],
                CARD_R * s + 1.0,
                BORDER,
            );
            self.text
                .rounded(r, scene, [px0, py0, px1, py1], CARD_R * s, PANEL_ALT);
            let pad = tin;
            let inner = px1 - px0 - pad * 2.0;
            let mut cy = py0 + 28.0 * s;
            let head = clip_to(&self.text, &p.title, 16.0 * s, inner);
            self.put(r, scene, &head, (16.0 * s) as u32, WHITE, px0 + pad, cy);
            cy += 22.0 * s;
            let meta = clip_to(&self.text, &p.meta, 11.0 * s, inner);
            self.put(r, scene, &meta, (11.0 * s) as u32, MUTED, px0 + pad, cy);
            cy += 18.0 * s;
            scene
                .overlays
                .push((sep, [px0 + pad, cy.round(), px1 - pad, cy.round() + 1.0]));
            let rpx = (13.0 * s) as u32;
            let lh = 24.0 * s;
            let mut top = cy + 10.0 * s;
            let nav_rows: Vec<(&String, f32, usize)> =
                p.time.iter().map(|t| (t, 20.0f32, 0usize)).collect();
            for (time, fs, base) in nav_rows {
                let bh = 30.0 * s;
                let bw = 46.0 * s;
                let by = top;
                let right = px1 - pad;
                let left = px0 + pad;
                for j in 0..2usize {
                    let rect = if j == 0 {
                        [left, by, left + bw, by + bh]
                    } else {
                        [right - bw, by, right, by + bh]
                    };
                    let a = self.easeq(
                        (14, "time", j + base),
                        if over(rect) { 1.0 } else { 0.0 },
                        1.0 / FADE_SECS,
                    );
                    self.text.rounded(
                        r,
                        scene,
                        rect,
                        ROW_R * s,
                        mix([232, 160, 48, 60], ACCENT, a * 0.7),
                    );
                    let col = mix(ACCENT_HOT, [18, 14, 8, 255], a);
                    let (cx, cy) = ((rect[0] + rect[2]) * 0.5, (rect[1] + rect[3]) * 0.5);
                    let dir = if j == 0 { -1.0 } else { 1.0 };
                    let (half, head_w, head_h) = (9.0 * s, 7.0 * s, 7.0 * s);
                    let t = (2.0 * s).max(2.0);
                    self.text.rounded(
                        r,
                        scene,
                        [cx - half, cy - t * 0.5, cx + half, cy + t * 0.5],
                        0.0,
                        col,
                    );
                    let tip = cx + dir * half;
                    let strips = 7;
                    for k in 0..strips {
                        let bx = tip - dir * head_w * (1.0 - k as f32 / strips as f32);
                        let ex = tip - dir * head_w * (1.0 - (k as f32 + 1.0) / strips as f32);
                        let h = head_h * (1.0 - (k as f32 + 0.5) / strips as f32);
                        self.text.rounded(
                            r,
                            scene,
                            [bx.min(ex), cy - h, bx.max(ex), cy + h],
                            0.0,
                            col,
                        );
                    }
                    self.menu_time.push(rect);
                }
                let mid_l = left + bw + 10.0 * s;
                let mid_r = right - bw - 10.0 * s;
                let time = clip_to(&self.text, time, fs * s, mid_r - mid_l);
                let tw = self.text.width(&time, fs * s);
                self.put(
                    r,
                    scene,
                    &time,
                    (fs * s) as u32,
                    if base == 0 { AMBER } else { WHITE },
                    mid_l + (mid_r - mid_l - tw) * 0.5,
                    by + bh * 0.5,
                );
                top = by + bh + 8.0 * s;
            }
            let n = p.rows.len();
            if let (Some(chosen), Some(button)) = (p.chosen, p.button.as_ref()) {
                let go_h = 34.0 * s;
                let go = [px0 + pad, py1 - 12.0 * s - go_h, px1 - pad, py1 - 12.0 * s];
                let fit = (((go[1] - 10.0 * s) - top) / lh).floor().max(1.0) as usize;
                let first = match f.pane_first {
                    Some(p) if n > fit => p.min(n - fit),
                    _ if n > fit => chosen.saturating_sub(fit / 2).min(n - fit),
                    _ => 0,
                };
                self.menu_pane_start = first;
                self.menu_pane_box = Some([px0, py0, px1, py1]);
                if n > fit {
                    let (tt, tb) = (top, go[1] - 10.0 * s);
                    let th = tb - tt;
                    self.text.rounded(
                        r,
                        scene,
                        [px1 - 6.0 * s, tt, px1 - 3.0 * s, tb],
                        1.5 * s,
                        [255, 255, 255, 22],
                    );
                    let t0 = tt + th * first as f32 / n as f32;
                    let t1 = tt + th * (first + fit) as f32 / n as f32;
                    self.text.rounded(
                        r,
                        scene,
                        [px1 - 6.0 * s, t0, px1 - 3.0 * s, t1],
                        1.5 * s,
                        ACCENT,
                    );
                }
                let time_w = p
                    .rows
                    .iter()
                    .skip(first)
                    .take(fit)
                    .map(|row| self.text.width(&row.1, rpx as f32))
                    .fold(0.0f32, f32::max);
                for (i, (what, when)) in p.rows.iter().enumerate().skip(first).take(fit) {
                    let ry = top + lh * (i - first) as f32 + lh * 0.5;
                    let rect = [px0 + 8.0 * s, ry - lh * 0.5, px1 - 8.0 * s, ry + lh * 0.5];
                    let on = i == chosen;
                    let hov = over(rect) && !on;
                    let a_on =
                        self.easeq((11, "stop", i), if on { 1.0 } else { 0.0 }, 1.0 / FADE_SECS);
                    let a_hov = self.easeq(
                        (12, "stop", i),
                        if hov { 1.0 } else { 0.0 },
                        1.0 / FADE_SECS,
                    );
                    if a_hov > 0.0 {
                        self.text
                            .rounded(r, scene, rect, ROW_R * s, fade(LIT, a_hov));
                    }
                    if a_on > 0.0 {
                        self.text
                            .rounded(r, scene, rect, ROW_R * s, fade(SELECTED, a_on));
                        self.text.rounded(
                            r,
                            scene,
                            [
                                rect[0],
                                rect[1] + 7.0 * s,
                                rect[0] + 2.0 * s,
                                rect[3] - 7.0 * s,
                            ],
                            1.0 * s,
                            fade(ACCENT, a_on),
                        );
                    }
                    self.put_right(r, scene, when, rpx, AMBER, px1 - pad, ry);
                    let what = clip_to(&self.text, what, rpx as f32, inner - time_w - 14.0 * s);
                    self.put(r, scene, &what, rpx, mix(SOFT, WHITE, a_on), px0 + pad, ry);
                    self.menu_pane.push(rect);
                }
                let a_go = self.easeq(
                    (13, "go", 0),
                    if over(go) { 1.0 } else { 0.0 },
                    1.0 / FADE_SECS,
                );
                self.text
                    .rounded(r, scene, go, ROW_R * s, mix(ACCENT, ACCENT_HOT, a_go));
                let l = self
                    .text
                    .label(r, scene, button, (14.0 * s) as u32, ON_ACCENT);
                let (gx, gy) = (
                    go[0] + (go[2] - go[0] - l.w as f32) * 0.5,
                    (go[1] + go[3]) * 0.5 - l.h as f32 * 0.5,
                );
                l.place(scene, gx, gy);
                self.menu_pane_go = Some(go);
            } else {
                let fit = ((py1 - 12.0 * s - top) / lh).floor().max(1.0) as usize;
                let shown = if n > fit { fit.saturating_sub(1) } else { n };
                let time_w = p
                    .rows
                    .iter()
                    .take(shown)
                    .map(|row| self.text.width(&row.1, rpx as f32))
                    .fold(0.0f32, f32::max);
                let tp = format!("{} ", crate::tr("Tour"));
                let tile_w = p
                    .rows
                    .iter()
                    .take(shown)
                    .filter_map(|row| {
                        row.0
                            .strip_prefix(tp.as_str())
                            .and_then(|x| x.split_once("  ›  ").map(|(n, _)| n).or(Some(x)))
                    })
                    .map(|n| self.text.width(n.trim(), rpx as f32) + 16.0 * s)
                    .fold(30.0 * s, f32::max);
                for (i, (what, when)) in p.rows.iter().take(shown).enumerate() {
                    let ry = top + lh * i as f32 + lh * 0.5;
                    self.put_right(r, scene, when, rpx, AMBER, px1 - pad, ry);
                    if let Some(rest) = what.strip_prefix(tp.as_str()) {
                        let (num, dest) = rest.split_once("  ›  ").unwrap_or((rest, ""));
                        let th = lh - 6.0 * s;
                        self.text.rounded(
                            r,
                            scene,
                            [px0 + pad, ry - th * 0.5, px0 + pad + tile_w, ry + th * 0.5],
                            5.0 * s,
                            ACCENT_SOFT,
                        );
                        let nw = self.text.width(num.trim(), rpx as f32);
                        self.put(
                            r,
                            scene,
                            num.trim(),
                            rpx,
                            txt(ACCENT),
                            px0 + pad + (tile_w - nw) * 0.5,
                            ry,
                        );
                        let dx = px0 + pad + tile_w + 10.0 * s;
                        let dest = clip_to(
                            &self.text,
                            dest,
                            rpx as f32,
                            px1 - pad - time_w - 14.0 * s - dx,
                        );
                        self.put(r, scene, &dest, rpx, SOFT, dx, ry);
                    } else {
                        let what = clip_to(&self.text, what, rpx as f32, inner - time_w - 14.0 * s);
                        self.put(r, scene, &what, rpx, SOFT, px0 + pad, ry);
                    }
                }
                if n > shown {
                    let ry = top + lh * shown as f32 + lh * 0.5;
                    self.put(
                        r,
                        scene,
                        &crate::tr("+{} more").replacen("{}", &(n - shown).to_string(), 1),
                        (12.0 * s) as u32,
                        MUTED,
                        px0 + pad,
                        ry,
                    );
                }
            }
        }
        self.menu_overlay_range = overlay_start..scene.overlays.len();
    }
}
