use super::*;

impl Navigator {
    pub(super) fn city(&mut self, renderer: &Renderer, scene: &mut Scene, f: &NavFrame) {
        self.atlas.begin_frame();
        let (sw, sh) = f.screen;
        let (w, h) = ((sw * 0.8).round(), (sh * 0.82).round());
        let (x0, y0) = (((sw - w) * 0.5).round(), ((sh - h) * 0.5).round());
        let (x0, y0, w, h) = match self.city.embed {
            Some(r) => (r[0].round(), r[1].round(), (r[2] - r[0]).round().max(32.0), (r[3] - r[1]).round().max(32.0)),
            None => (x0, y0, w, h),
        };
        self.city.rect = [x0, y0, x0 + w, y0 + h];
        let (tw, th) = (w as u32, h as u32);
        if self.gpu.is_none() {
            self.gpu = Some(Gpu::new(
                &renderer.device,
                renderer.format(),
                map_samples(renderer.format()),
                self.atlas.size,
            ));
        }
        if self
            .city
            .target
            .map(|t| (t.1, t.2) != (tw, th))
            .unwrap_or(true)
        {
            if let Some((t, _, _)) = self.city.target.take() {
                renderer.free_texture(scene, t);
                scene.premultiplied.remove(&t);
            }
            let t = renderer.add_render_texture(scene, tw, th);
            scene.premultiplied.insert(t);
            self.city.target = Some((t, tw, th));
        }
        if self.city.follow {
            self.city.center = f.bus.truncate();
        }
        let s = (h / 760.0).clamp(0.95, 2.0) * f.ui_scale;
        let global = self.global.clone();
        let net = global.as_deref().or(f.traffic.map(|t| &t.net));
        let mut roads_verts = None;
        let surfaces = self.surfaces.clone();
        if let Some(sm) = surfaces.as_deref() {
            // Navigator 2.0: the ground around what the window shows, finely when close,
            // coarsely from far; built again when the window leaves it or the detail changes
            let view = (w as f64).hypot(h as f64) * 0.5 * self.city.mpp;
            let coarse = self.city.mpp > 0.6;
            let stale = match self.city.surf {
                None => true,
                Some((c, r, co)) => {
                    co != coarse
                        || (self.city.center - c).length() + view > r
                        || r > view * 4.0 + 800.0
                }
            };
            let version = self.global_version * 1_000_000 + 999_999;
            if stale || self.city.roads.map(|r| r.0 != version).unwrap_or(true) {
                let (mut lo, mut hi) = (DVec2::splat(f64::MAX), DVec2::splat(f64::MIN));
                for k in sm.chunks.keys() {
                    let o = crate::navmap::SurfaceMap::chunk_origin(*k);
                    lo = lo.min(o);
                    hi = hi.max(o + DVec2::splat(crate::navmap::CHUNK));
                }
                if lo.x == f64::MAX {
                    lo = f.bus.truncate();
                    hi = lo;
                }
                self.city.extent = (lo, hi);
                // the anchor stays put while the window moves: the route drawn on it holds
                let anchor = self.city.roads.map(|r| r.2).unwrap_or((lo + hi) * 0.5);
                let radius = view * 2.0 + 250.0;
                let mut p = Painter::new();
                build_surfaces(
                    &mut p,
                    sm,
                    anchor,
                    self.city.center,
                    radius,
                    coarse,
                    if coarse { 0.0 } else { 1.0 },
                );
                self.city.surf = Some((self.city.center, radius, coarse));
                self.city.roads = Some((version, p.len(), anchor));
                roads_verts = Some(p.verts);
            }
        } else if let Some(n) = net {
            let version = self.global_version * 1_000_000 + n.lanes.len() as u64;
            if self.city.roads.map(|r| r.0 != version).unwrap_or(true) {
                let (mut lo, mut hi) = (DVec2::splat(f64::MAX), DVec2::splat(f64::MIN));
                let from_lanes;
                let road_lanes: &[MapRoad] = match self.graph.as_deref().filter(|_| global.is_some()) {
                    Some(g) => &g.roads,
                    None => {
                        from_lanes = road_geometry(n);
                        &from_lanes
                    }
                };
                for l in road_lanes {
                    for p in &l.points {
                        lo = lo.min(p.truncate());
                        hi = hi.max(p.truncate());
                    }
                }
                if lo.x == f64::MAX {
                    lo = f.bus.truncate();
                    hi = lo;
                }
                let anchor = (lo + hi) * 0.5;
                self.city.extent = (lo, hi);
                let rel =
                    |q: DVec3| Vec3::new((q.x - anchor.x) as f32, (q.y - anchor.y) as f32, 0.0);
                let mut p = Painter::new();
                for pass in 0..2 {
                    for l in road_lanes {
                        let pts =
                            simplify(&l.points.iter().map(|q| rel(*q)).collect::<Vec<_>>(), 0.12);
                        if pass == 0 {
                            p.ribbon(&pts, l.width + 2.0, 2.4, ROAD_CASING, true);
                        } else {
                            p.ribbon(
                                &pts,
                                l.width,
                                1.4,
                                if l.main { ROAD_MAIN } else { ROAD },
                                true,
                            );
                        }
                    }
                }
                self.city.roads = Some((version, p.len(), anchor));
                roads_verts = Some(p.verts);
            }
        }
        let anchor = self.city.roads.map(|r| r.2).unwrap_or(f.bus.truncate());
        let rel = |q: DVec3| Vec3::new((q.x - anchor.x) as f32, (q.y - anchor.y) as f32, 0.0);
        let mut route_verts = None;
        let key = (
            self.route.version,
            self.jam_version,
            0,
            self.city.roads.map(|r| r.0).unwrap_or(0),
        );
        let done = self.route.progress.min(self.route.lanes.len());
        if self.city.route.0 != key {
            let mut p = Painter::new();
            if let Some(n) = net {
                let r = &self.route;
                for &l in &r.lanes[..done] {
                    let Some(lane) = n.lanes.get(l) else { continue };
                    let pts: Vec<Vec3> = lane.points.iter().map(|q| rel(*q)).collect();
                    p.ribbon(&pts, lane.width.max(3.0) + 2.0, 5.0, DRIVEN, true);
                }
                build_route_line(
                    &mut p,
                    n,
                    &r.lanes[done..],
                    0.0,
                    anchor,
                    &self.route_jam,
                    f64::MAX,
                    6.0 * s,
                );
            }
            self.city.route = (key, p.len());
            route_verts = Some(p.verts);
        }
        let c = self.city.center - anchor;
        let (hw, hh) = (
            w as f64 * 0.5 * self.city.mpp,
            h as f64 * 0.5 * self.city.mpp,
        );
        let proj = glam::camera::rh::proj::directx::orthographic(
            (c.x - hw) as f32,
            (c.x + hw) as f32,
            (c.y - hh) as f32,
            (c.y + hh) as f32,
            -1000.0,
            1000.0,
        );
        let vp = [0.0, 0.0, w, h];
        let world = Layer {
            view_proj: proj,
            viewport: vp,
            clip: [0.0, 0.0, w, h],
            radius: 8.0 * s,
            opacity: 1.0,
            px_scale: self.city.mpp as f32,
            route: [
                if self.route.on_route && done < self.route.lanes.len() {
                    self.route.s
                } else {
                    -1.0e9
                },
                1.0e9,
                0.0,
            ],
        };
        let to_screen = |q: DVec3| -> Vec2 {
            let d = q.truncate() - self.city.center;
            Vec2::new(
                (w as f64 * 0.5 + d.x / self.city.mpp) as f32,
                (h as f64 * 0.5 - d.y / self.city.mpp) as f32,
            )
        };
        let win = Rect::new(0.0, 0.0, w, h);
        let emb = self.city.embed.is_some();
        let mut bg = Painter::new();
        bg.rounded(
            win,
            if emb { 0.0 } else { 8.0 * s },
            Color::rgba(
                10,
                10,
                10,
                if emb {
                    0.55
                } else {
                    (crate::ui::backdrop(self.opacity) * 1.3).min(1.0)
                },
            ),
        );
        let n_bg = bg.len();
        let mut dots = Painter::new();
        if let Some(t) = f.traffic.filter(|_| self.show_ai) {
            for car in t.cars.iter().filter(|c| !c.gone) {
                dots.world_disc(
                    rel(car.vehicle.position),
                    2.2,
                    3.4,
                    Color::rgba(8, 8, 8, 0.9),
                );
                dots.world_disc(rel(car.vehicle.position), 1.5, 2.3, DOT);
            }
        }
        let n_dots = dots.len();
        let mut ui = Painter::new();
        let n_stops = f.stops.len();
        let markers = spaced_markers(
            f.stops
                .iter()
                .enumerate()
                .map(|(k, st)| (k, to_screen(st.position)))
                .filter(|(_, p)| win.contains(*p) && p.y > 50.0 * s),
            20.0 * s,
        );
        let mut taken: Vec<Rect> = markers
            .iter()
            .map(|(_, p)| Rect::new(p.x - 9.0 * s, p.y - 9.0 * s, 18.0 * s, 18.0 * s))
            .collect();
        taken.push(Rect::new(0.0, 0.0, w, 50.0 * s));
        let mut stop_labels = Vec::new();
        for &(k, p) in &markers {
            if self.city.mpp >= 4.0 && k != 0 && k + 1 != n_stops {
                continue;
            }
            let st = &f.stops[k];
            let weight = if k == 0 { Weight::Bold } else { Weight::Medium };
            let name = format!(
                "{}  {:02}:{:02}",
                st.name.trim(),
                (st.arrival / 3600.0) as i32 % 24,
                ((st.arrival % 3600.0) / 60.0) as i32
            );
            let name = self
                .fonts
                .fit(&name, 12.5 * s, weight, (300.0 * s).min(w * 0.3));
            let lw = self.fonts.width(&name, 12.5 * s, weight) + 14.0 * s;
            if let Some(r) = stop_label_rect(p, lw, s, win, &taken) {
                taken.push(r);
                stop_labels.push((k, name, r));
            }
        }
        if let (Some(st), true) = (
            self.streets.clone(),
            self.city.mpp < 3.2 && self.global.is_some(),
        ) {
            let px = 12.0 * s;
            for (q, a, id) in &st.labels {
                let p = to_screen(q.extend(0.0));
                if !win.pad(60.0 * s, 30.0 * s).contains(p) {
                    continue;
                }
                let name = &st.names[*id as usize];
                let tw = self.fonts.width(name, px, Weight::Medium);
                let mut ang = -*a;
                if ang > std::f32::consts::FRAC_PI_2 {
                    ang -= std::f32::consts::PI;
                } else if ang <= -std::f32::consts::FRAC_PI_2 {
                    ang += std::f32::consts::PI;
                }
                let side = Vec2::new(-ang.sin(), ang.cos()) * (px * 0.9 + 2.0 * s);
                let c = p + side;
                let (hx, hy) = (
                    (ang.cos() * tw * 0.5).abs() + (ang.sin() * px * 0.6).abs(),
                    (ang.sin() * tw * 0.5).abs() + (ang.cos() * px * 0.6).abs(),
                );
                let bb = Rect::new(c.x - hx, c.y - hy, hx * 2.0, hy * 2.0);
                if taken.iter().any(|t| rects_overlap(t, &bb)) {
                    continue;
                }
                taken.push(bb);
                let halo = Color::rgba(22, 22, 22, 0.9);
                for o in [
                    Vec2::new(1.0, 0.0),
                    Vec2::new(-1.0, 0.0),
                    Vec2::new(0.0, 1.0),
                    Vec2::new(0.0, -1.0),
                ] {
                    ui.text_rotated(
                        &mut self.atlas,
                        &self.fonts,
                        name,
                        px,
                        Weight::Medium,
                        c + o * s,
                        ang,
                        halo,
                    );
                }
                ui.text_rotated(
                    &mut self.atlas,
                    &self.fonts,
                    name,
                    px,
                    Weight::Medium,
                    c,
                    ang,
                    STREET,
                );
            }
        }
        for (k, p) in markers.into_iter().rev() {
            let next = k == 0;
            if self.city.mpp < 4.0 || next || k + 1 == n_stops {
                let badge = if next { 7.0 } else { 5.5 } * s;
                ui.circle(p, badge + 1.5 * s, CARD);
                let fill = if next {
                    ACCENT
                } else if k + 1 == n_stops {
                    ROUTE
                } else {
                    Color::rgba(76, 91, 112, 0.98)
                };
                ui.circle(p, badge, fill);
                ui.icon(
                    &mut self.atlas,
                    "directions_bus",
                    p,
                    if next { 11.5 } else { 9.5 } * s,
                    if next {
                        Color::rgba(18, 14, 8, 1.0)
                    } else {
                        TEXT
                    },
                );
            } else {
                ui.circle(p, 5.0 * s, CARD);
                ui.circle(p, 3.2 * s, TEXT_DIM);
            }
        }
        for (k, name, r) in stop_labels {
            ui.rounded(r, 6.0 * s, CARD);
            ui.rounded_border(r, 6.0 * s, 1.0_f32.max(s), HAIR);
            ui.text_in(
                &mut self.atlas,
                &self.fonts,
                &name,
                12.5 * s,
                if k == 0 { Weight::Bold } else { Weight::Medium },
                r.pad(6.0 * s, 0.0),
                Align::Left,
                if k == 0 { TEXT } else { TEXT_DIM },
            );
        }
        {
            let bp = to_screen(f.bus);
            let a = (f.heading as f32).to_radians();
            let rot =
                |v: Vec2| Vec2::new(v.x * a.cos() - v.y * a.sin(), v.x * a.sin() + v.y * a.cos());
            let k = 10.0 * s;
            let tip = bp + rot(Vec2::new(0.0, -1.0) * k);
            let l = bp + rot(Vec2::new(-0.7, 0.8) * k);
            let m = bp + rot(Vec2::new(0.0, 0.4) * k);
            let r = bp + rot(Vec2::new(0.7, 0.8) * k);
            let dark = Color::rgba(22, 22, 22, 0.85);
            let grow = |p: Vec2| bp + (p - bp) * 1.25;
            ui.tri(grow(tip), grow(l), grow(m), dark, dark, dark);
            ui.tri(grow(tip), grow(m), grow(r), dark, dark, dark);
            ui.tri(tip, l, m, TEXT, TEXT, TEXT);
            ui.tri(tip, m, r, TEXT, TEXT, TEXT);
        }
        let head = Rect::new(0.0, 0.0, w, if emb { 0.0 } else { 44.0 * s });
        let pad = 16.0 * s;
        if !emb {
            ui.rect(head, Color::rgba(12, 12, 12, 0.97));
            ui.rect(
                Rect::new(0.0, head.bottom() - 1.0_f32.max(s), w, 1.0_f32.max(s)),
                HAIR,
            );
            let wd = words();
            let title = match (&f.line, &f.terminus) {
                (Some(l), Some(t)) => format!("{}  ›  {}", l.trim(), t.trim()),
                _ => wd.map.to_string(),
            };
            let title_w = ui.text_in(
                &mut self.atlas,
                &self.fonts,
                &title,
                15.0 * s,
                Weight::Bold,
                Rect::new(pad, head.y, w * 0.4, head.h),
                Align::Left,
                TEXT,
            );
            if let Some(st) = f.stops.first() {
                let d = self
                    .next_dist
                    .map(|d| distance(d, uses_miles(f.units)))
                    .unwrap_or_default();
                let t = format!(
                    "{}  ·  {}  ·  {:02}:{:02}",
                    st.name.trim(),
                    d,
                    (st.arrival / 3600.0) as i32 % 24,
                    ((st.arrival % 3600.0) / 60.0) as i32
                );
                ui.text_in(
                    &mut self.atlas,
                    &self.fonts,
                    &t,
                    13.5 * s,
                    Weight::Medium,
                    Rect::new(pad + title_w + 24.0 * s, head.y, w * 0.45, head.h),
                    Align::Left,
                    TEXT_DIM,
                );
            }
        }
        self.city.buttons.clear();
        let bs = 30.0 * s;
        let mut bx = w - pad - bs;
        let by_btn = if emb { h - pad - bs } else { head.y + (head.h - bs) * 0.5 };
        for (icon, id) in [
            ("close", 3u8),
            ("zoom_out", 2),
            ("zoom_in", 1),
            ("my_location", 0),
            ("directions_bus", 4),
        ] {
            if (emb && id == 3) || (!emb && id == 4) {
                continue;
            }
            let r = Rect::new(bx, by_btn, bs, bs);
            let on = id == 0 && self.city.follow;
            ui.rounded(r, 6.0 * s, if on { ACCENT.alpha(0.16) } else { CARD });
            ui.rounded_border(
                r,
                6.0 * s,
                1.0_f32.max(s),
                if on { ACCENT.alpha(0.55) } else { HAIR },
            );
            ui.icon(
                &mut self.atlas,
                icon,
                r.center(),
                18.0 * s,
                if on { ACCENT } else { TEXT },
            );
            self.city.buttons.push((r, id));
            bx -= bs + 8.0 * s;
        }
        let nice = [
            10.0, 20.0, 50.0, 100.0, 200.0, 500.0, 1000.0, 2000.0, 5000.0,
        ];
        let metres = nice
            .iter()
            .copied()
            .find(|m| m / self.city.mpp > 70.0 * s as f64)
            .unwrap_or(5000.0);
        let len = (metres / self.city.mpp) as f32;
        let by = h - 22.0 * s;
        ui.rect(Rect::new(pad, by, len, 2.0 * s), TEXT_DIM);
        ui.text(
            &mut self.atlas,
            &self.fonts,
            &distance(metres, uses_miles(f.units)),
            12.0 * s,
            Weight::Medium,
            Vec2::new(pad + len + 8.0 * s, by + 4.0 * s),
            Align::Left,
            TEXT_DIM,
        );
        if !emb {
            ui.rounded_border(win, 8.0 * s, 1.0_f32.max(s), HAIR);
        }

        let (tex, _, _) = self.city.target.unwrap();
        let Some(view) = renderer.texture_view(scene, tex) else {
            return;
        };
        let (Some(gpu), device, queue) = (self.gpu.as_mut(), &renderer.device, &renderer.queue)
        else {
            return;
        };
        if let Some(v) = roads_verts {
            gpu.upload(device, queue, 3, &v);
        }
        if let Some(v) = route_verts {
            gpu.upload(device, queue, 4, &v);
        }
        let mut all = bg.verts;
        all.extend(dots.verts);
        let n_ui = all.len() as u32;
        all.extend(ui.verts);
        gpu.upload(device, queue, 5, &all);
        gpu.upload_atlas(queue, &mut self.atlas);
        let flat = Layer::flat([0.0, 0.0, w, h], 8.0 * s, 1.0);
        let layers = [flat, world];
        let roads_n = self.city.roads.map(|r| r.1).unwrap_or(0);
        let draws = [
            Draw {
                buffer: 5,
                range: 0..n_bg,
                layer: 0,
                texture: 0,
            },
            Draw {
                buffer: 3,
                range: 0..roads_n,
                layer: 1,
                texture: 0,
            },
            Draw {
                buffer: 4,
                range: 0..self.city.route.1,
                layer: 1,
                texture: 0,
            },
            Draw {
                buffer: 5,
                range: n_bg..n_bg + n_dots,
                layer: 1,
                texture: 0,
            },
            Draw {
                buffer: 5,
                range: n_ui..all.len() as u32,
                layer: 0,
                texture: 0,
            },
        ];
        let mut enc = device.create_command_encoder(&wgpu::CommandEncoderDescriptor {
            label: Some("city map"),
        });
        gpu.render(
            device,
            queue,
            &mut enc,
            &view,
            (tw, th),
            Some(wgpu::Color::TRANSPARENT),
            &layers,
            &draws,
        );
        queue.submit([enc.finish()]);
        let _ = n_dots;
        if self.city.embed.is_some() {
            self.city.picture = Some(tex);
        } else {
            scene.overlays.push((tex, [x0, y0, x0 + w, y0 + h]));
        }
    }
}
