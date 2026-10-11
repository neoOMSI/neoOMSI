use super::*;

impl Navigator {
    pub(super) fn city(&mut self, renderer: &Renderer, scene: &mut Scene, f: &NavFrame) {
        let t_frame = std::time::Instant::now();
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
        if let Some(sm) = surfaces.as_ref() {
            // Navigator 2.0: the ground around what the window shows, finely when close,
            // coarsely from far; built again on a thread of its own when the window nears the
            // edge of what is built or the detail changes, the old ground shown meanwhile
            let view = (w as f64).hypot(h as f64) * 0.5 * self.city.mpp;
            let coarse = self.city.mpp > 0.6;
            let version = self.global_version * 1_000_000 + 999_999;
            if let Some(rx) = self.city.ground.as_ref() {
                match rx.try_recv() {
                    Ok(g) => {
                        self.city.ground = None;
                        if let (Some(buffer), Some(gpu), true) =
                            (g.buffer, self.gpu.as_mut(), g.version == version)
                        {
                            gpu.put(3, buffer);
                            self.city.surf = Some(g.surf);
                            self.city.roads = Some((g.version, g.len, g.anchor));
                        }
                    }
                    Err(std::sync::mpsc::TryRecvError::Empty) => {}
                    Err(std::sync::mpsc::TryRecvError::Disconnected) => self.city.ground = None,
                }
            }
            let stale = match self.city.surf {
                None => true,
                Some((c, r, co)) => {
                    co != coarse
                        || (self.city.center - c).length() + view * 1.5 > r
                        || r > view * 4.0 + 800.0
                }
            };
            if self.city.ground.is_none()
                && (stale || self.city.roads.map(|r| r.0 != version).unwrap_or(true))
            {
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
                let anchor = self
                    .city
                    .roads
                    .filter(|r| r.0 == version)
                    .map(|r| r.2)
                    .unwrap_or((lo + hi) * 0.5);
                let radius = view * 2.0 + 250.0;
                let centre = self.city.center;
                let (sm, device) = (sm.clone(), renderer.device.clone());
                let (tx, rx) = std::sync::mpsc::channel();
                let spawned = std::thread::Builder::new()
                    .name("navigator map ground".into())
                    .spawn(move || {
                        let t0 = std::time::Instant::now();
                        let mut p = Painter::new();
                        build_surfaces(
                            &mut p,
                            &sm,
                            anchor,
                            centre,
                            radius,
                            coarse,
                            if coarse { 0.0 } else { 1.0 },
                        );
                        let buffer = Gpu::filled(&device, &p.verts);
                        if ::legacy_config::env::var_os("OMSI_DEBUG_NAV").is_some() {
                            log::info!(
                                "navigator map: ground within {radius:.0} m ({}) in {:.1} ms: {} vertices",
                                if coarse { "coarse" } else { "fine" },
                                t0.elapsed().as_secs_f64() * 1000.0,
                                p.len()
                            );
                        }
                        let _ = tx.send(Ground {
                            version,
                            anchor,
                            surf: (centre, radius, coarse),
                            len: p.len(),
                            buffer,
                        });
                    });
                match spawned {
                    Ok(_) => self.city.ground = Some(rx),
                    Err(e) => log::warn!("navigator: the map's ground cannot be built: {e}"),
                }
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
        // while the bus is still on its way to the trip's route, that way is the route
        // shown, and the trip after it is a dimmed, thinner line beneath
        let lead = self.route.lead.clamp(done, self.route.lanes.len());
        let lead_len: f64 = net
            .map(|n| {
                self.route.lanes[done..lead]
                    .iter()
                    .filter_map(|&l| n.lanes.get(l))
                    .map(|l| l.length() as f64)
                    .sum()
            })
            .unwrap_or(0.0);
        if self.city.route.0 != key {
            let t0 = std::time::Instant::now();
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
                    &r.lanes[lead..],
                    lead_len,
                    anchor,
                    &self.route_jam,
                    f64::MAX,
                    if lead > done { 3.5 } else { 6.0 } * s,
                );
                build_route_line(
                    &mut p,
                    n,
                    &r.lanes[done..lead],
                    0.0,
                    anchor,
                    &self.route_jam,
                    f64::MAX,
                    6.0 * s,
                );
            }
            if ::legacy_config::env::var_os("OMSI_DEBUG_NAV").is_some() {
                log::info!(
                    "navigator map: route line in {:.1} ms: {} vertices",
                    t0.elapsed().as_secs_f64() * 1000.0,
                    p.len()
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
                if lead > done { lead_len as f32 } else { 1.0e9 },
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
        let mut ui = Painter::new();
        if let Some(t) = f.traffic.filter(|_| self.show_ai) {
            // only what the window shows: a big map runs hundreds of cars
            let seen = win.pad(-30.0 * s, -30.0 * s);
            // from far the cars are only noise; the buses stay
            marks::vehicles(
                &mut dots,
                t,
                &rel,
                |q| seen.contains(to_screen(q)),
                self.city.mpp < 2.5,
            );
            if self.city.mpp > 1.1 {
                // from far a bus is a few pixels: a badge says what it is
                for c in t.cars.iter().filter(|c| !c.gone && c.is_bus()) {
                    let p = to_screen(c.vehicle.position);
                    if win.contains(p) {
                        ui.circle(p, 7.5 * s, marks::RIM);
                        ui.circle(p, 6.2 * s, marks::AI_BUS);
                        ui.icon(&mut self.atlas, "directions_bus", p, 8.5 * s, TEXT);
                    }
                }
            }
        }
        let n_dots = dots.len();
        let n_stops = f.stops.len();
        let markers = spaced_markers(
            f.stops
                .iter()
                .enumerate()
                .map(|(k, st)| (k, to_screen(st.position)))
                .filter(|(_, p)| win.contains(*p) && p.y > 50.0 * s),
            16.0 * s,
        );
        let bus_at = to_screen(f.bus);
        let mut taken: Vec<Rect> = markers
            .iter()
            .map(|(_, p)| Rect::new(p.x - 8.0 * s, p.y - 8.0 * s, 16.0 * s, 16.0 * s))
            .collect();
        taken.push(Rect::new(0.0, 0.0, w, 50.0 * s));
        taken.push(Rect::new(bus_at.x - 14.0 * s, bus_at.y - 14.0 * s, 28.0 * s, 28.0 * s));
        // the route ahead as small boxes, so no name is written across the line
        let mut on_route: Vec<Rect> = Vec::new();
        if let Some(n) = net {
            let step = 8.0 * s;
            let mut last: Option<Vec2> = None;
            for &l in &self.route.lanes[done..] {
                let Some(lane) = n.lanes.get(l) else { continue };
                for q in &lane.points {
                    let b = to_screen(*q);
                    let a = last.unwrap_or(b);
                    last = Some(b);
                    if !win.pad(-step, -step).contains(b) && !win.pad(-step, -step).contains(a) {
                        continue;
                    }
                    let k = ((b - a).length() / step).ceil().max(1.0) as usize;
                    for i in 1..=k {
                        let c = a + (b - a) * (i as f32 / k as f32);
                        on_route.push(Rect::new(c.x - 4.0 * s, c.y - 4.0 * s, 8.0 * s, 8.0 * s));
                    }
                }
            }
        }
        let far = self.city.mpp >= 4.0;
        let mut stop_labels = Vec::new();
        for &(k, p) in &markers {
            let pin = marks::Pin::of(k, n_stops);
            if far && pin == marks::Pin::On {
                continue;
            }
            let st = &f.stops[k];
            // the times of the stops between only when there is room for them
            let time = (pin != marks::Pin::On || self.city.mpp < 1.6).then(|| clock(st.arrival));
            let (lw, name, nw) = marks::stop_label_size(
                &self.fonts,
                st.name.trim(),
                time.as_deref(),
                pin,
                (280.0 * s).min(w * 0.3),
                s,
            );
            let clear = [taken.as_slice(), on_route.as_slice()].concat();
            // the next and the last stop always say their name, on the line if need be
            let r = stop_label_rect(p, lw, s, win, &clear)
                .or_else(|| (pin != marks::Pin::On).then(|| stop_label_rect(p, lw, s, win, &taken)).flatten());
            if let Some(r) = r {
                taken.push(r);
                stop_labels.push((pin, name, nw, time, r));
            }
        }
        if let (Some(st), true) = (
            self.streets.clone(),
            self.city.mpp < 3.2 && self.global.is_some(),
        ) {
            let px = 11.0 * s;
            // street names fade out as the map zooms away, rather than all leaving at once
            let fade = ((3.2 - self.city.mpp as f32) / 1.2).clamp(0.0, 1.0);
            let color = Color::rgba(168, 168, 176, fade);
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
                // a little air round each name, so they never crowd one another
                let bb = Rect::new(c.x - hx, c.y - hy, hx * 2.0, hy * 2.0).pad(-6.0 * s, -4.0 * s);
                if taken.iter().chain(&on_route).any(|t| rects_overlap(t, &bb)) {
                    continue;
                }
                taken.push(bb);
                marks::halo_text_rotated(
                    &mut ui,
                    &mut self.atlas,
                    &self.fonts,
                    name,
                    px,
                    Weight::Medium,
                    c,
                    ang,
                    color,
                    s,
                );
            }
        }
        for (k, p) in markers.into_iter().rev() {
            let pin = marks::Pin::of(k, n_stops);
            if far && pin == marks::Pin::On {
                continue;
            }
            marks::stop_pin(&mut ui, &mut self.atlas, p, pin, s);
        }
        for (pin, name, nw, time, r) in stop_labels {
            marks::stop_label(
                &mut ui,
                &mut self.atlas,
                &self.fonts,
                r,
                &name,
                nw,
                time.as_deref(),
                pin,
                s,
            );
        }
        marks::own_arrow(&mut ui, bus_at, (f.heading as f32).to_radians(), 9.5 * s, TEXT, Some(Color::rgba(0, 0, 0, 0.35)));
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
        let Some(view) = self.city.shot.clone().or_else(|| renderer.texture_view(scene, tex)) else {
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
            // the traffic under the route: parked cars must not cut the line up
            Draw {
                buffer: 5,
                range: n_bg..n_bg + n_dots,
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
        if ::legacy_config::env::var_os("OMSI_DEBUG_NAV").is_some() {
            log::info!("navigator map: frame in {:.1} ms", t_frame.elapsed().as_secs_f64() * 1000.0);
        }
        if self.city.embed.is_some() {
            self.city.picture = Some(tex);
        } else {
            scene.overlays.push((tex, [x0, y0, x0 + w, y0 + h]));
        }
    }
}
